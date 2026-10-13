//! Shared process-image/root admission for neutral and legacy envelopes.
use crate::ExecutableIdentity;
#[cfg(target_os = "macos")]
use crate::{ensure_absolute, executable_identity};
use std::path::PathBuf;
#[cfg(all(test, target_os = "macos"))]
use std::time::Duration;
#[cfg(target_os = "macos")]
use std::{fs, path::Path};

#[cfg(target_os = "macos")]
fn process_is_provably_zombie(pid: libc::pid_t) -> bool {
    // Public sys/proc_info.h ABI; unlike full BSDINFO this flavor can inspect
    // other UIDs. A nonzero arg includes zombies, which have no proc_pidpath.
    #[repr(C)]
    struct ShortBsdInfo {
        pid: u32,
        ppid: u32,
        pgid: u32,
        status: u32,
        comm: [libc::c_char; libc::MAXCOMLEN],
        flags: u32,
        uid: libc::uid_t,
        gid: libc::gid_t,
        ruid: libc::uid_t,
        rgid: libc::gid_t,
        svuid: libc::uid_t,
        svgid: libc::gid_t,
        reserved: u32,
    }
    const PROC_PIDT_SHORTBSDINFO: libc::c_int = 13;
    let mut info: ShortBsdInfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as libc::c_int;
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            PROC_PIDT_SHORTBSDINFO,
            1,
            (&mut info as *mut ShortBsdInfo).cast(),
            size,
        )
    };
    read == size && info.pid == pid as u32 && info.status == libc::SZOMB
}

#[cfg(target_os = "macos")]
fn installation_process_admitted(
    pid: libc::pid_t,
    roots: &[PathBuf],
    authorized: &[(u32, ExecutableIdentity)],
    buffer: &mut [u8; 4096],
) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let n = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if n <= 0 {
        if process_is_provably_zombie(pid) {
            return Ok(());
        }
        if unsafe { libc::kill(pid, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
        {
            return Err(format!("cannot authenticate live process image {pid}"));
        }
        return Ok(());
    }
    let end = buffer
        .iter()
        .position(|b| *b == 0)
        .ok_or("unterminated process path")?;
    let path = Path::new(std::ffi::OsStr::from_bytes(&buffer[..end]));
    if path
        .file_name()
        .is_none_or(|name| name != "desktop-pet" && name != "herdr-desktop-pet")
    {
        return Ok(());
    }
    ensure_absolute(path)?;
    let physical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if authorized.iter().any(|(allowed_pid, image)| {
        *allowed_pid == pid as u32
            && image.path == path
            && executable_identity(path).as_ref() == Ok(image)
    }) {
        return Ok(());
    }
    if roots
        .iter()
        .any(|root| physical.starts_with(root) || path.starts_with(root))
    {
        return Err(format!(
            "live installation process {pid} uses {}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn installation_roots_admitted(
    roots: &[PathBuf],
    authorized: &[(u32, ExecutableIdentity)],
) -> Result<(), String> {
    unsafe extern "C" {
        fn proc_listallpids(buffer: *mut std::ffi::c_void, buffersize: i32) -> i32;
        fn proc_pidpath(pid: i32, buffer: *mut std::ffi::c_void, buffersize: u32) -> i32;
    }
    use std::os::unix::ffi::OsStrExt;
    let mut buffer = [0u8; 4096];
    for (pid, image) in authorized {
        if *pid == 0 || *pid > i32::MAX as u32 {
            return Err("invalid authorized process PID".into());
        }
        ensure_absolute(&image.path)?;
        let n =
            unsafe { proc_pidpath(*pid as i32, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        if n <= 0 {
            return Err(format!("authorized process {pid} is not provably live"));
        }
        let end = buffer
            .iter()
            .position(|b| *b == 0)
            .ok_or("unterminated authorized process path")?;
        let actual = Path::new(std::ffi::OsStr::from_bytes(&buffer[..end]));
        if actual != image.path || executable_identity(actual)? != *image {
            return Err(format!(
                "authorized process {pid} image differs from captured identity"
            ));
        }
        if !roots.is_empty() && !roots.iter().any(|root| actual.starts_with(root)) {
            return Err(format!(
                "authorized process {pid} is outside installation roots"
            ));
        }
    }
    // Cleanup authenticates only its exact caller; unrelated installations are not its scope.
    if roots.is_empty() {
        return Ok(());
    }
    let count = unsafe { proc_listallpids(std::ptr::null_mut(), 0) };
    if !(0..=100_000).contains(&count) {
        return Err("cannot enumerate installation users".into());
    }
    let mut pids = vec![0i32; count as usize + 512];
    let read = unsafe { proc_listallpids(pids.as_mut_ptr().cast(), (pids.len() * 4) as i32) };
    if read < 0 || read as usize >= pids.len() {
        return Err("process enumeration changed; retry when quiet".into());
    }
    for pid in pids.into_iter().take(read as usize).filter(|pid| *pid > 0) {
        installation_process_admitted(pid, roots, authorized, &mut buffer)?;
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn installation_roots_admitted(
    _: &[PathBuf],
    _: &[(u32, ExecutableIdentity)],
) -> Result<(), String> {
    Err("installation process proof requires macOS".into())
}

pub(crate) fn installation_roots_inactive(roots: &[PathBuf]) -> Result<(), String> {
    installation_roots_admitted(roots, &[])
}

#[cfg(all(test, target_os = "macos"))]
mod process_admission_tests {
    use super::*;

    #[test]
    fn every_authorized_pid_requires_exact_live_image_even_without_roots() {
        let image = executable_identity(&std::env::current_exe().unwrap()).unwrap();
        let pid = std::process::id();
        assert!(installation_roots_admitted(&[], &[(pid, image.clone())]).is_ok());
        assert!(installation_roots_admitted(&[], &[(0, image.clone())]).is_err());
        let mut changed = image.clone();
        changed.sha256 = if image.sha256 == "a".repeat(64) {
            "b".repeat(64)
        } else {
            "a".repeat(64)
        };
        assert!(installation_roots_admitted(&[], &[(pid, changed)]).is_err());
        let unrelated = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(unrelated.path()).unwrap();
        assert!(installation_roots_admitted(&[root], &[(pid, image)]).is_err());
    }

    #[test]
    fn exited_unreaped_process_cannot_authorize_or_block_an_installation() {
        struct Reap(libc::pid_t);
        impl Drop for Reap {
            fn drop(&mut self) {
                unsafe { libc::waitpid(self.0, std::ptr::null_mut(), 0) };
            }
        }
        let image = executable_identity(&std::env::current_exe().unwrap()).unwrap();
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0);
        if pid == 0 {
            unsafe { libc::_exit(0) };
        }
        let _reap = Reap(pid);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if process_is_provably_zombie(pid) {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "child did not exit");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(installation_roots_admitted(&[], &[(pid as u32, image)]).is_err());
        let unrelated = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(unrelated.path()).unwrap();
        assert_eq!(
            installation_process_admitted(pid, &[root], &[], &mut [0u8; 4096]),
            Ok(())
        );
    }
}
