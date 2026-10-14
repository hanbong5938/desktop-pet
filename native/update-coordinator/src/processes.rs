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

#[cfg(target_os = "macos")]
fn short_bsd_info_is_valid(pid: libc::pid_t, read: libc::c_int, info: &ShortBsdInfo) -> bool {
    read == std::mem::size_of::<ShortBsdInfo>() as libc::c_int && info.pid == pid as u32
}

#[cfg(target_os = "macos")]
fn process_short_bsd_info(pid: libc::pid_t) -> Option<ShortBsdInfo> {
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
    if short_bsd_info_is_valid(pid, read, &info) {
        Some(info)
    } else {
        None
    }
}

#[cfg(all(test, target_os = "macos"))]
fn process_is_provably_zombie(pid: libc::pid_t) -> bool {
    process_short_bsd_info(pid).is_some_and(|info| info.status == libc::SZOMB)
}

#[cfg(target_os = "macos")]
fn process_comm_may_be_pet(comm: &[libc::c_char]) -> bool {
    let Some(end) = comm.iter().position(|byte| *byte == 0) else {
        return true;
    };
    if end == 0 {
        return true;
    }
    // Executable-derived comm can exclude a process from scope, never authorize
    // it. A truncated pet name also keeps every colliding prefix in scope.
    let capacity = comm.len() - 1;
    [b"desktop-pet".as_slice(), b"herdr-desktop-pet".as_slice()]
        .into_iter()
        .any(|name| {
            let expected = &name[..name.len().min(capacity)];
            end == expected.len()
                && comm[..end]
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| *actual as u8 == *expected)
        })
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
        let path_error = std::io::Error::last_os_error();
        if let Some(info) = process_short_bsd_info(pid) {
            if info.status == libc::SZOMB || !process_comm_may_be_pet(&info.comm) {
                return Ok(());
            }
        }
        let kill_result = unsafe { libc::kill(pid, 0) };
        let kill_error = std::io::Error::last_os_error();
        if kill_result == -1 && kill_error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err(format!(
            "cannot authenticate live process image {pid}: {path_error}"
        ));
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

    fn kernel_comm<const N: usize>(name: &[u8]) -> [libc::c_char; N] {
        let mut comm = [0; N];
        for (dest, source) in comm.iter_mut().take(N.saturating_sub(1)).zip(name) {
            *dest = *source as libc::c_char;
        }
        comm
    }

    #[test]
    fn short_bsd_info_requires_exact_byte_count_and_pid() {
        let pid = 123;
        let mut info: ShortBsdInfo = unsafe { std::mem::zeroed() };
        info.pid = pid as u32;
        let size = std::mem::size_of::<ShortBsdInfo>() as libc::c_int;
        assert!(short_bsd_info_is_valid(pid, size, &info));
        for count in [-1, 0, size - 1, size + 1] {
            assert!(!short_bsd_info_is_valid(pid, count, &info));
        }
        info.pid += 1;
        assert!(!short_bsd_info_is_valid(pid, size, &info));
    }

    #[test]
    fn unrelated_comm_excludes_pet_scope() {
        for name in [
            b"sleep".as_slice(),
            b"unrelated-sleeper".as_slice(),
            b"desktop-pets".as_slice(),
            b"herdr-desktop-q".as_slice(),
        ] {
            assert!(!process_comm_may_be_pet(
                &kernel_comm::<{ libc::MAXCOMLEN }>(name)
            ));
        }
    }

    #[test]
    fn both_pet_names_remain_candidates_with_or_without_truncation() {
        for name in [b"desktop-pet".as_slice(), b"herdr-desktop-pet".as_slice()] {
            assert!(process_comm_may_be_pet(
                &kernel_comm::<{ libc::MAXCOMLEN }>(name)
            ));
            assert!(process_comm_may_be_pet(&kernel_comm::<64>(name)));
        }
    }

    #[test]
    fn truncated_prefix_collisions_remain_pet_candidates() {
        assert!(process_comm_may_be_pet(&kernel_comm::<8>(
            b"desktop-pet-unrelated-suffix"
        )));
        assert!(process_comm_may_be_pet(&kernel_comm::<8>(
            b"herdr-desktop-pet-unrelated-suffix"
        )));
        assert!(!process_comm_may_be_pet(&kernel_comm::<8>(b"desktoX")));
        assert!(!process_comm_may_be_pet(&kernel_comm::<8>(b"herdr-X")));
    }

    #[test]
    fn empty_or_unterminated_comm_cannot_exclude_pet_scope() {
        assert!(process_comm_may_be_pet(&[]));
        assert!(process_comm_may_be_pet(&[0; libc::MAXCOMLEN]));
        assert!(process_comm_may_be_pet(&[0]));
        assert!(process_comm_may_be_pet(
            &[b's' as libc::c_char; libc::MAXCOMLEN]
        ));
    }

    fn process_path(pid: libc::pid_t) -> std::io::Result<PathBuf> {
        use std::os::unix::ffi::OsStrExt;
        let mut buffer = [0u8; 4096];
        let read =
            unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        if read <= 0 {
            return Err(std::io::Error::last_os_error());
        }
        let end = buffer.iter().position(|byte| *byte == 0).unwrap();
        Ok(Path::new(std::ffi::OsStr::from_bytes(&buffer[..end])).to_path_buf())
    }

    struct UnlinkedSleep {
        child: std::process::Child,
        executable: PathBuf,
        _directory: tempfile::TempDir,
    }

    impl UnlinkedSleep {
        fn start(basename: &str, alias: Option<&str>) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let root = fs::canonicalize(directory.path()).unwrap();
            let executable = root.join(basename);
            fs::copy("/bin/sleep", &executable).unwrap();
            let launcher = if let Some(alias) = alias {
                let launcher = root.join(alias);
                std::os::unix::fs::symlink(&executable, &launcher).unwrap();
                launcher
            } else {
                executable.clone()
            };
            let child = std::process::Command::new(launcher)
                .arg("300")
                .spawn()
                .unwrap();
            let mut process = Self {
                child,
                executable,
                _directory: directory,
            };
            process.wait_for_path(true);
            process
        }

        fn pid(&self) -> libc::pid_t {
            self.child.id() as libc::pid_t
        }

        fn wait_for_path(&mut self, available: bool) {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                let observed = process_path(self.pid());
                assert!(
                    self.child.try_wait().unwrap().is_none(),
                    "sleep child exited while waiting for path availability {available}"
                );
                let ready = match (&observed, available) {
                    (Ok(path), true) => path == &self.executable,
                    (Err(_), false) => true,
                    _ => false,
                };
                if ready {
                    assert_eq!(unsafe { libc::kill(self.pid(), 0) }, 0);
                    return;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "sleep path did not reach availability {available}: {observed:?}"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        fn unlink(&mut self) {
            fs::remove_file(&self.executable).unwrap();
            self.wait_for_path(false);
        }
    }

    impl Drop for UnlinkedSleep {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[test]
    fn unlinked_unrelated_executable_is_admitted_but_cannot_authorize_a_process() {
        let mut process = UnlinkedSleep::start("unrelated-sleeper", None);
        let roots = [process.executable.parent().unwrap().to_path_buf()];
        let image = executable_identity(&process.executable).unwrap();
        assert_eq!(
            installation_process_admitted(process.pid(), &roots, &[], &mut [0; 4096]),
            Ok(())
        );
        process.unlink();
        assert_eq!(
            installation_process_admitted(process.pid(), &roots, &[], &mut [0; 4096]),
            Ok(())
        );
        assert!(
            installation_roots_admitted(&[], &[(process.child.id(), image)]).is_err(),
            "negative comm classification must not authenticate an authorized PID"
        );
    }

    fn assert_unlinked_pet_blocks(basename: &str, alias: Option<&str>) {
        let mut process = UnlinkedSleep::start(basename, alias);
        let roots = [process.executable.parent().unwrap().to_path_buf()];
        let error =
            installation_process_admitted(process.pid(), &roots, &[], &mut [0; 4096]).unwrap_err();
        assert!(error.starts_with("live installation process "));
        process.unlink();
        let info = process_short_bsd_info(process.pid()).unwrap();
        assert_ne!(info.status, libc::SZOMB);
        assert!(
            process_comm_may_be_pet(&info.comm),
            "an executable-derived pet comm must survive unlinking and symlink aliases"
        );
        let error =
            installation_process_admitted(process.pid(), &roots, &[], &mut [0; 4096]).unwrap_err();
        assert!(error.starts_with(&format!(
            "cannot authenticate live process image {}:",
            process.pid()
        )));
    }

    #[test]
    fn unlinked_pet_executables_remain_blocking() {
        for basename in ["desktop-pet", "herdr-desktop-pet"] {
            assert_unlinked_pet_blocks(basename, None);
        }
    }

    #[test]
    fn unlinked_pet_executables_launched_through_unrelated_symlinks_remain_blocking() {
        for basename in ["desktop-pet", "herdr-desktop-pet"] {
            assert_unlinked_pet_blocks(basename, Some("innocent-alias"));
        }
    }

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
