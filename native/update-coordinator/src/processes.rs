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

/// A PID is never an identity: BSD start time is checked around every capture.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessIncarnation {
    pub pid: u32,
    pub start_sec: u64,
    pub start_usec: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappedFileIdentity {
    pub device: u32,
    pub inode: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessEvidence {
    pub incarnation: ProcessIncarnation,
    pub mapped_file: MappedFileIdentity,
    pub path: PathBuf,
    pub uid: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessArguments {
    pub argv: Vec<std::ffi::OsString>,
    pub environment: Vec<std::ffi::OsString>,
}

/// Only the authenticated runtime may create one, after binding a control status
/// to the same peer PID, incarnation, installed image and exact profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AuthorizedDaemon {
    Original {
        incarnation: ProcessIncarnation,
        mapped_file: MappedFileIdentity,
    },
    Candidate {
        incarnation: ProcessIncarnation,
        mapped_file: MappedFileIdentity,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InstallationProcessRole {
    AuthorizedDaemon,
    CompatibleClient,
    BlockingUser,
    Unrelated,
}

struct PinnedClientImage {
    // This open file keeps the inode from being recycled throughout the swap.
    _file: std::fs::File,
    metadata: std::fs::Metadata,
    mapped_file: MappedFileIdentity,
    bundle: PathBuf,
    compatible: bool,
}

/// Opt-in admission for a single physical installation and exact profile pair.
/// Raw install, cleanup and migration keep using the strict APIs above.
pub(crate) struct CompatibleClientScope {
    original: PinnedClientImage,
    candidate: Option<PinnedClientImage>,
    config: PathBuf,
    state: PathBuf,
}

const PUBLIC_BETA_ONE_SHA256: &str =
    "569699e2beceeeb447cc4e7f6707e067d7dd84ce3c4d93a8bf8f97a9d7402612";
// SHA256 of the published v0.4.13-beta.1 rig-native sidecar. Pinning this
// immutable closure prevents a rewritten ad-hoc bundle from borrowing the old
// application's hash to authorize different dynamically loaded code/resources.
const PUBLIC_BETA_ONE_SIDECAR_SHA256: &str =
    "92dd301bab357ed56f6023186c71ee8a6bafa2fe46cdfb86d4a9628182c50689";

fn release_allows_clients(contract: Option<u32>, app_hash: &str, sidecar_hash: &str) -> bool {
    contract == Some(1)
        || (app_hash == PUBLIC_BETA_ONE_SHA256 && sidecar_hash == PUBLIC_BETA_ONE_SIDECAR_SHA256)
}

impl PinnedClientImage {
    fn capture(app: &crate::installation::ValidatedApp) -> Result<Self, String> {
        use sha2::{Digest, Sha256};
        // Do not trust a caller-constructed ValidatedApp or a version string.
        // This includes the external sidecar, signature and full deployed closure.
        let validated = crate::installation::validate_staged_app(&app.bundle)?;
        if validated.image != app.image || validated.version != app.version {
            return Err("scoped application differs from validated bundle".into());
        }
        let sidecar_hash = format!(
            "{:x}",
            Sha256::digest(crate::owned_file(
                &crate::installation::sidecar_path(&validated.bundle),
                2 * 1024 * 1024,
            )?)
        );
        Self::pin_validated(&validated, &sidecar_hash)
    }

    fn capture_recovered(proof: &crate::installation::RecoveredScopeImage) -> Result<Self, String> {
        use std::os::unix::fs::MetadataExt;
        let pinned = Self::pin_validated(proof.app(), proof.sidecar_sha256())?;
        if (pinned.metadata.dev(), pinned.metadata.ino()) != proof.file_identity() {
            return Err("recovered executable differs from immutable replacement inode".into());
        }
        Ok(pinned)
    }

    fn pin_validated(
        validated: &crate::installation::ValidatedApp,
        sidecar_hash: &str,
    ) -> Result<Self, String> {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&validated.image.path)
            .map_err(|e| e.to_string())?;
        let before = file.metadata().map_err(|e| e.to_string())?;
        if !before.is_file() || before.nlink() == 0 {
            return Err("scoped executable is not a linked regular file".into());
        }
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        let after = file.metadata().map_err(|e| e.to_string())?;
        if before.dev() != after.dev()
            || before.ino() != after.ino()
            || before.len() != after.len()
            || before.mtime() != after.mtime()
            || before.mtime_nsec() != after.mtime_nsec()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
            || format!("{:x}", hash.finalize()) != validated.image.sha256
            || crate::executable_identity(&validated.image.path)? != validated.image
        {
            return Err("scoped executable changed while pinning".into());
        }
        let path_metadata =
            std::fs::symlink_metadata(&validated.image.path).map_err(|e| e.to_string())?;
        if path_metadata.dev() != after.dev() || path_metadata.ino() != after.ino() {
            return Err("scoped executable path no longer names pinned file".into());
        }
        let compatible = release_allows_clients(
            validated.installation_client_contract,
            &validated.image.sha256,
            sidecar_hash,
        );
        Ok(Self {
            _file: file,
            metadata: after,
            mapped_file: MappedFileIdentity {
                device: before.dev() as u32,
                inode: before.ino(),
            },
            bundle: validated.bundle.clone(),
            compatible,
        })
    }

    fn unchanged(&self) -> Result<(), String> {
        use std::os::unix::fs::MetadataExt;
        let now = self._file.metadata().map_err(|e| e.to_string())?;
        let old = &self.metadata;
        if !now.is_file()
            || now.nlink() == 0
            || now.dev() != old.dev()
            || now.ino() != old.ino()
            || now.len() != old.len()
            || now.mtime() != old.mtime()
            || now.mtime_nsec() != old.mtime_nsec()
            || now.ctime() != old.ctime()
            || now.ctime_nsec() != old.ctime_nsec()
        {
            return Err("pinned installation image changed or was unlinked".into());
        }
        Ok(())
    }
}

impl CompatibleClientScope {
    pub(crate) fn capture(
        original: &crate::installation::ValidatedApp,
        config: &std::path::Path,
        state: &std::path::Path,
    ) -> Result<Self, String> {
        crate::ensure_absolute(config)?;
        crate::ensure_absolute(state)?;
        Ok(Self {
            original: PinnedClientImage::capture(original)?,
            candidate: None,
            config: config.to_path_buf(),
            state: state.to_path_buf(),
        })
    }

    pub(crate) fn capture_recovered(
        recovered: &crate::installation::RecoveredReplacement,
        config: &std::path::Path,
        state: &std::path::Path,
    ) -> Result<Self, String> {
        crate::ensure_absolute(config)?;
        crate::ensure_absolute(state)?;
        let (original, candidate) = recovered.scope_images();
        let original = PinnedClientImage::capture_recovered(original)?;
        let candidate = PinnedClientImage::capture_recovered(candidate)?;
        if original.mapped_file == candidate.mapped_file {
            return Err("recovered candidate aliases original executable".into());
        }
        Ok(Self {
            original,
            candidate: Some(candidate),
            config: config.to_path_buf(),
            state: state.to_path_buf(),
        })
    }

    pub(crate) fn pin_prepared_candidate(
        &mut self,
        candidate: &crate::installation::ValidatedApp,
    ) -> Result<(), String> {
        let pinned = PinnedClientImage::capture(candidate)?;
        if let Some(existing) = &self.candidate {
            if existing.mapped_file == pinned.mapped_file {
                return Ok(());
            }
            return Err("prepared candidate was already pinned to another file".into());
        }
        if pinned.mapped_file == self.original.mapped_file {
            return Err("prepared candidate aliases original executable".into());
        }
        self.candidate = Some(pinned);
        Ok(())
    }

    pub(crate) fn original_identity(&self) -> MappedFileIdentity {
        self.original.mapped_file
    }

    pub(crate) fn verify_original_path(&self, image: &ExecutableIdentity) -> Result<(), String> {
        use std::os::unix::fs::MetadataExt;
        self.original.unchanged()?;
        let metadata = std::fs::symlink_metadata(&image.path).map_err(|e| e.to_string())?;
        if !metadata.is_file()
            || metadata.dev() != self.original.metadata.dev()
            || metadata.ino() != self.original.metadata.ino()
            || crate::executable_identity(&image.path)? != *image
        {
            return Err("original canonical executable differs from pinned preflight inode".into());
        }
        Ok(())
    }

    pub(crate) fn candidate_identity(&self) -> Option<MappedFileIdentity> {
        self.candidate.as_ref().map(|image| image.mapped_file)
    }

    fn image_for(&self, identity: MappedFileIdentity) -> Option<&PinnedClientImage> {
        std::iter::once(&self.original)
            .chain(self.candidate.iter())
            .find(|image| image.mapped_file == identity)
    }

    fn path_in_scope(&self, path: &std::path::Path) -> bool {
        std::iter::once(&self.original)
            .chain(self.candidate.iter())
            .any(|image| path.starts_with(&image.bundle))
    }

    fn classify(
        &self,
        evidence: &ProcessEvidence,
        arguments: Option<&ProcessArguments>,
        authorized: Option<&AuthorizedDaemon>,
    ) -> InstallationProcessRole {
        let Some(image) = self.image_for(evidence.mapped_file) else {
            return if self.path_in_scope(&evidence.path) && path_may_be_app(&evidence.path) {
                InstallationProcessRole::BlockingUser
            } else {
                InstallationProcessRole::Unrelated
            };
        };
        if evidence.uid != unsafe { libc::geteuid() } {
            return InstallationProcessRole::BlockingUser;
        }
        let allowed = match authorized {
            Some(AuthorizedDaemon::Original {
                incarnation,
                mapped_file,
            }) => {
                *incarnation == evidence.incarnation
                    && *mapped_file == evidence.mapped_file
                    && *mapped_file == self.original.mapped_file
            }
            Some(AuthorizedDaemon::Candidate {
                incarnation,
                mapped_file,
            }) => {
                *incarnation == evidence.incarnation
                    && *mapped_file == evidence.mapped_file
                    && self.candidate_identity() == Some(*mapped_file)
            }
            None => false,
        };
        if allowed {
            return InstallationProcessRole::AuthorizedDaemon;
        }
        if image.compatible
            && arguments.is_some_and(|args| {
                compatible_cli(&args.argv).is_some_and(|profile| match profile {
                    None => true,
                    Some((config, state)) => {
                        config.as_os_str() == self.config.as_os_str()
                            && state.as_os_str() == self.state.as_os_str()
                    }
                })
            })
        {
            InstallationProcessRole::CompatibleClient
        } else {
            InstallationProcessRole::BlockingUser
        }
    }
}

/// Returns None for a mutating/unknown grammar; Some(None) for a profileless
/// query, or the exact explicit pair for a profile-bearing IPC client.
fn compatible_cli(argv: &[std::ffi::OsString]) -> Option<Option<(PathBuf, PathBuf)>> {
    let (_, args) = argv.split_first()?;
    if args.len() == 1
        && ["update-capabilities", "distribution-identity", "--version"]
            .iter()
            .any(|expected| args[0] == *expected)
    {
        return Some(None);
    }
    let args =
        if args.len() == 7 && args[0] == "profile" && args[1] == "show" && args[2] == "--json" {
            &args[3..]
        } else if args.len() == 5 && args[0] == "tern-exchange" {
            &args[1..]
        } else {
            return None;
        };
    if args[0] != "--config-dir" || args[2] != "--state-dir" {
        return None;
    }
    let config = PathBuf::from(&args[1]);
    let state = PathBuf::from(&args[3]);
    if !normalized_explicit_path(&config) || !normalized_explicit_path(&state) {
        return None;
    }
    Some(Some((config, state)))
}

fn normalized_explicit_path(path: &std::path::Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    crate::ensure_absolute(path).is_ok()
        && (bytes == b"/"
            || bytes[1..]
                .split(|byte| *byte == b'/')
                .all(|part| !part.is_empty() && part != b"." && part != b".."))
}

fn path_may_be_app(path: &std::path::Path) -> bool {
    path.file_name()
        .is_none_or(|name| name == "desktop-pet" || name == "herdr-desktop-pet")
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct RegionInfo {
    protection: u32,
    max_protection: u32,
    inheritance: u32,
    flags: u32,
    offset: u64,
    behavior: u32,
    user_wired_count: u32,
    user_tag: u32,
    pages_resident: u32,
    pages_shared_now_private: u32,
    pages_swapped_out: u32,
    pages_dirtied: u32,
    ref_count: u32,
    shadow_depth: u32,
    share_mode: u32,
    private_pages_resident: u32,
    shared_pages_resident: u32,
    obj_id: u32,
    depth: u32,
    address: u64,
    size: u64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct RegionWithPath {
    region: RegionInfo,
    stat: libc::vinfo_stat,
    vnode_type: libc::c_int,
    vnode_pad: libc::c_int,
    fsid: [i32; 2],
    path: [libc::c_char; 1024],
}

#[cfg(target_os = "macos")]
fn process_path(pid: u32) -> Result<PathBuf, String> {
    use std::os::unix::ffi::OsStrExt;
    let mut bytes = [0u8; 4096];
    let n =
        unsafe { libc::proc_pidpath(pid as i32, bytes.as_mut_ptr().cast(), bytes.len() as u32) };
    if n <= 0 || n as usize >= bytes.len() {
        return Err(format!("cannot read process {pid} path"));
    }
    let end = bytes
        .iter()
        .position(|b| *b == 0)
        .ok_or("unterminated kernel process path")?;
    if end == 0 || end != n as usize {
        return Err("invalid kernel process path length".into());
    }
    let path = PathBuf::from(std::ffi::OsStr::from_bytes(&bytes[..end]));
    crate::ensure_absolute(&path)?;
    Ok(path)
}

#[cfg(target_os = "macos")]
fn process_bsd_info(pid: u32) -> Result<libc::proc_bsdinfo, String> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Err("invalid process PID".into());
    }
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    let n = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    if n != size
        || info.pbi_pid != pid
        || info.pbi_start_tvusec >= 1_000_000
        || info.pbi_start_tvsec == 0
        || info.pbi_status == libc::SZOMB
    {
        return Err(format!("cannot capture live process {pid} incarnation"));
    }
    Ok(info)
}

#[cfg(target_os = "macos")]
fn incarnation(info: &libc::proc_bsdinfo) -> ProcessIncarnation {
    ProcessIncarnation {
        pid: info.pbi_pid,
        start_sec: info.pbi_start_tvsec,
        start_usec: info.pbi_start_tvusec,
    }
}

#[cfg(target_os = "macos")]
fn main_region_identity(
    info: &RegionWithPath,
    executable: &std::path::Path,
) -> Result<Option<MappedFileIdentity>, String> {
    use std::os::unix::ffi::OsStrExt;
    let end = info
        .path
        .iter()
        .position(|b| *b == 0)
        .ok_or("unterminated VM region path")?;
    let bytes = unsafe { std::slice::from_raw_parts(info.path.as_ptr().cast::<u8>(), end) };
    if end != 0
        && std::ffi::OsStr::from_bytes(bytes) == executable.as_os_str()
        && info.region.protection & 4 != 0
        && info.region.offset == 0
        && info.vnode_type == 1
        && info.stat.vst_mode & libc::S_IFMT == libc::S_IFREG
        && info.stat.vst_ino != 0
        && info.stat.vst_nlink != 0
    {
        Ok(Some(MappedFileIdentity {
            device: info.stat.vst_dev,
            inode: info.stat.vst_ino,
        }))
    } else {
        Ok(None)
    }
}
#[cfg(target_os = "macos")]
fn main_image_vnode(pid: u32, executable: &std::path::Path) -> Result<MappedFileIdentity, String> {
    let mut address = 0u64;
    for _ in 0..65536 {
        let mut info: RegionWithPath = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&info) as i32;
        let n = unsafe {
            libc::proc_pidinfo(
                pid as i32,
                8,
                address,
                (&mut info as *mut RegionWithPath).cast(),
                size,
            )
        };
        if n != size {
            return Err(format!(
                "cannot inspect process {pid} main executable mapping"
            ));
        }
        let next = info
            .region
            .address
            .checked_add(info.region.size)
            .filter(|next| *next > address)
            .ok_or("invalid kernel VM region bounds")?;
        // A full-size zero-vnode response is not executable evidence.
        if let Some(identity) = main_region_identity(&info, executable)? {
            return Ok(identity);
        }
        address = next;
    }
    Err("too many process VM regions to authenticate main executable".into())
}

#[cfg(target_os = "macos")]
pub(crate) fn capture_process(pid: u32) -> Result<ProcessEvidence, String> {
    let before = process_bsd_info(pid)?;
    let path = process_path(pid)?;
    let mapped_file = main_image_vnode(pid, &path)?;
    let after_path = process_path(pid)?;
    let after = process_bsd_info(pid)?;
    let after_mapping = main_image_vnode(pid, &after_path)?;
    if incarnation(&before) != incarnation(&after)
        || path != after_path
        || before.pbi_uid != after.pbi_uid
        || before.pbi_ruid != after.pbi_ruid
        || before.pbi_uid != before.pbi_ruid
        || mapped_file != after_mapping
    {
        return Err("process exec, credentials or incarnation changed during capture".into());
    }
    Ok(ProcessEvidence {
        incarnation: incarnation(&after),
        mapped_file,
        path,
        uid: after.pbi_uid,
    })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn capture_process(_: u32) -> Result<ProcessEvidence, String> {
    Err("process incarnation and vnode proof requires macOS".into())
}

fn parse_procargs(bytes: &[u8]) -> Result<ProcessArguments, String> {
    use std::os::unix::ffi::OsStringExt;
    let raw_count: [u8; 4] = bytes
        .get(..4)
        .ok_or("truncated kernel argc")?
        .try_into()
        .map_err(|_| "truncated kernel argc")?;
    let argc = i32::from_ne_bytes(raw_count);
    if argc <= 0 || argc as usize > 65536 || argc as usize > bytes.len() / 2 {
        return Err("invalid kernel argc".into());
    }
    let mut position = 4;
    let executable_end = bytes[position..]
        .iter()
        .position(|b| *b == 0)
        .map(|n| n + position)
        .ok_or("unterminated kernel executable path")?;
    if executable_end == position || bytes[position] != b'/' {
        return Err("invalid kernel executable path".into());
    }
    position = executable_end + 1;
    while bytes.get(position) == Some(&0) {
        position += 1;
    }
    let mut argv = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        let tail = bytes.get(position..).ok_or("truncated kernel argv")?;
        let end = tail
            .iter()
            .position(|b| *b == 0)
            .ok_or("unterminated kernel argv")?;
        argv.push(std::ffi::OsString::from_vec(tail[..end].to_vec()));
        position += end + 1;
    }
    if argv[0].is_empty() {
        return Err("empty kernel argv[0]".into());
    }
    let mut environment = Vec::new();
    loop {
        let tail = bytes
            .get(position..)
            .filter(|tail| !tail.is_empty())
            .ok_or("truncated kernel environment")?;
        let end = tail
            .iter()
            .position(|b| *b == 0)
            .ok_or("unterminated kernel environment")?;
        if end == 0 {
            // Darwin appends an opaque apple-vector after the environment's
            // empty entry. It is not argv/environment and grants no authority.
            break;
        }
        let entry = &tail[..end];
        if !entry
            .iter()
            .position(|b| *b == b'=')
            .is_some_and(|equal| equal > 0)
        {
            return Err("malformed kernel environment entry".into());
        }
        environment.push(std::ffi::OsString::from_vec(entry.to_vec()));
        position += end + 1;
    }
    Ok(ProcessArguments { argv, environment })
}

#[cfg(target_os = "macos")]
pub(crate) fn capture_process_arguments(
    evidence: &ProcessEvidence,
) -> Result<ProcessArguments, String> {
    let before = capture_process(evidence.incarnation.pid)?;
    if before != *evidence {
        return Err("process changed before argument capture".into());
    }
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    let mut argmax = 0i32;
    let mut size = std::mem::size_of_val(&argmax);
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            (&mut argmax as *mut i32).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 || size != std::mem::size_of_val(&argmax) || argmax <= 0 {
        return Err("cannot determine bounded kernel ARGMAX".into());
    }
    let capacity = (argmax as usize).min(1024 * 1024);
    let mut bytes = vec![0u8; capacity];
    let mut length = bytes.len();
    let mut mib = [
        libc::CTL_KERN,
        libc::KERN_PROCARGS2,
        evidence.incarnation.pid as i32,
    ];
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            bytes.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 || length == 0 || length >= capacity {
        return Err("cannot capture complete bounded process argv/environment".into());
    }
    let arguments = parse_procargs(&bytes[..length])?;
    if capture_process(evidence.incarnation.pid)? != before {
        return Err("process changed while capturing argv/environment".into());
    }
    Ok(arguments)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn capture_process_arguments(_: &ProcessEvidence) -> Result<ProcessArguments, String> {
    Err("kernel argv/environment capture requires macOS".into())
}

#[cfg(target_os = "macos")]
fn all_process_ids() -> Result<Vec<i32>, String> {
    unsafe extern "C" {
        fn proc_listallpids(buffer: *mut std::ffi::c_void, size: i32) -> i32;
    }
    let count = unsafe { proc_listallpids(std::ptr::null_mut(), 0) };
    if !(0..=100_000).contains(&count) {
        return Err("cannot enumerate installation users".into());
    }
    let mut pids = vec![0i32; count as usize + 512];
    let n = unsafe { proc_listallpids(pids.as_mut_ptr().cast(), (pids.len() * 4) as i32) };
    if n < 0 || n as usize >= pids.len() {
        return Err("installation process enumeration overflow".into());
    }
    pids.truncate(n as usize);
    Ok(pids)
}

#[cfg(target_os = "macos")]
fn provably_gone(pid: i32) -> bool {
    if process_short_bsd_info(pid).is_some_and(|info| info.status == libc::SZOMB) {
        return true;
    }
    let result = unsafe { libc::kill(pid, 0) };
    result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

#[cfg(target_os = "macos")]
fn capture_scoped_process(pid: i32) -> Result<Option<ProcessEvidence>, String> {
    match capture_process(pid as u32) {
        Ok(evidence) => Ok(Some(evidence)),
        Err(error) => {
            if provably_gone(pid) {
                return Ok(None);
            }
            // Scope before UID/argv failures. Helper and unrelated processes do
            // not become blockers merely because another UID hides their VM.
            if let Ok(path) = process_path(pid as u32) {
                if !path_may_be_app(&path) {
                    return Ok(None);
                }
            } else if process_short_bsd_info(pid)
                .is_some_and(|info| !process_comm_may_be_pet(&info.comm))
            {
                return Ok(None);
            }
            Err(format!(
                "cannot authenticate relevant process {pid}: {error}"
            ))
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn admit_coordinated_processes(
    scope: &CompatibleClientScope,
    authorized: Option<&AuthorizedDaemon>,
) -> Result<(), String> {
    scope.original.unchanged()?;
    if let Some(candidate) = &scope.candidate {
        candidate.unchanged()?;
    }
    if let Some(daemon) = authorized {
        let (incarnation, mapped_file, expected) = match daemon {
            AuthorizedDaemon::Original {
                incarnation,
                mapped_file,
            } => (incarnation, mapped_file, Some(scope.original_identity())),
            AuthorizedDaemon::Candidate {
                incarnation,
                mapped_file,
            } => (incarnation, mapped_file, scope.candidate_identity()),
        };
        if expected != Some(*mapped_file) {
            return Err("authorized daemon is not the pinned original/prepared candidate".into());
        }
        let actual = capture_process(incarnation.pid)?;
        if actual.incarnation != *incarnation
            || actual.mapped_file != *mapped_file
            || actual.uid != unsafe { libc::geteuid() }
        {
            return Err("authorized daemon incarnation or mapped image changed".into());
        }
    }
    for pid in all_process_ids()?.into_iter().filter(|pid| *pid > 0) {
        let Some(evidence) = capture_scoped_process(pid)? else {
            continue;
        };
        if scope.image_for(evidence.mapped_file).is_none()
            && (!scope.path_in_scope(&evidence.path) || !path_may_be_app(&evidence.path))
        {
            continue;
        }
        let role = scope.classify(&evidence, None, authorized);
        if matches!(
            role,
            InstallationProcessRole::AuthorizedDaemon | InstallationProcessRole::Unrelated
        ) {
            continue;
        }
        let arguments = match capture_process_arguments(&evidence) {
            Ok(arguments) => arguments,
            Err(_) if provably_gone(pid) => continue,
            Err(error) => {
                return Err(format!(
                    "cannot authenticate installation client {pid}: {error}"
                ))
            }
        };
        if scope.classify(&evidence, Some(&arguments), authorized)
            == InstallationProcessRole::BlockingUser
        {
            return Err(format!(
                "blocking installation process {pid} uses {}",
                evidence.path.display()
            ));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn admit_coordinated_processes(
    _: &CompatibleClientScope,
    _: Option<&AuthorizedDaemon>,
) -> Result<(), String> {
    Err("coordinated installation process proof requires macOS".into())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompatibleClientProfiles {
    pub client_count: usize,
    pub profiles: Vec<(PathBuf, PathBuf)>,
}

/// Used only when no active daemon supplied the profile. Unknown/mutating
/// commands still block; profileless queries never invent a profile.
#[cfg(target_os = "macos")]
pub(crate) fn compatible_client_profiles(
    original: &crate::installation::ValidatedApp,
) -> Result<CompatibleClientProfiles, String> {
    let image = PinnedClientImage::capture(original)?;
    let mut result = CompatibleClientProfiles {
        client_count: 0,
        profiles: Vec::new(),
    };
    for pid in all_process_ids()?.into_iter().filter(|pid| *pid > 0) {
        let Some(evidence) = capture_scoped_process(pid)? else {
            continue;
        };
        if evidence.mapped_file != image.mapped_file {
            if evidence.path.starts_with(&image.bundle) && path_may_be_app(&evidence.path) {
                return Err(format!("unknown installation process {pid}"));
            }
            continue;
        }
        if evidence.uid != unsafe { libc::geteuid() } || !image.compatible {
            return Err(format!("incompatible installation process {pid}"));
        }
        let arguments = match capture_process_arguments(&evidence) {
            Ok(arguments) => arguments,
            Err(_) if provably_gone(pid) => continue,
            Err(error) => return Err(format!("cannot authenticate client profile {pid}: {error}")),
        };
        let profile = compatible_cli(&arguments.argv)
            .ok_or_else(|| format!("blocking installation process {pid}"))?;
        result.client_count += 1;
        if let Some(profile) = profile {
            if !result.profiles.contains(&profile) {
                result.profiles.push(profile);
            }
        }
    }
    image.unchanged()?;
    Ok(result)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn compatible_client_profiles(
    _: &crate::installation::ValidatedApp,
) -> Result<CompatibleClientProfiles, String> {
    Err("kernel client profile discovery requires macOS".into())
}

#[cfg(all(test, target_os = "macos"))]
mod process_admission_tests {
    use super::*;

    #[test]
    fn main_image_proof_rejects_zero_vnodes_dylibs_offsets_and_nonexecutables() {
        let path = Path::new("/Applications/DesktopPet.app/Contents/MacOS/desktop-pet");
        let mut info: RegionWithPath = unsafe { std::mem::zeroed() };
        assert_eq!(main_region_identity(&info, path).unwrap(), None);
        use std::os::unix::ffi::OsStrExt;
        for (dest, byte) in info.path.iter_mut().zip(path.as_os_str().as_bytes()) {
            *dest = *byte as libc::c_char;
        }
        info.region.protection = 5;
        info.stat.vst_mode = libc::S_IFREG | 0o755;
        info.stat.vst_nlink = 1;
        info.stat.vst_ino = 10;
        info.stat.vst_dev = 1;
        info.vnode_type = 1;
        assert_eq!(
            main_region_identity(&info, path).unwrap(),
            Some(MappedFileIdentity {
                device: 1,
                inode: 10
            })
        );
        assert_eq!(
            main_region_identity(&info, Path::new("/bundle/libpet.dylib")).unwrap(),
            None
        );
        info.region.offset = 4096;
        assert_eq!(main_region_identity(&info, path).unwrap(), None);
        info.region.offset = 0;
        info.region.protection = 1;
        assert_eq!(main_region_identity(&info, path).unwrap(), None);
        info.region.protection = 5;
        info.stat.vst_ino = 0;
        assert_eq!(main_region_identity(&info, path).unwrap(), None);
        info.stat.vst_ino = 10;
        info.stat.vst_nlink = 0;
        assert_eq!(main_region_identity(&info, path).unwrap(), None);
        info.stat.vst_nlink = 1;
        info.stat.vst_mode = libc::S_IFDIR;
        assert_eq!(main_region_identity(&info, path).unwrap(), None);
        info.path.fill(b'x' as libc::c_char);
        assert!(main_region_identity(&info, path).is_err());
    }

    #[test]
    fn kernel_snapshot_binds_main_vnode_fd_incarnation_and_complete_arguments() {
        use std::os::unix::fs::MetadataExt;
        let first = UnlinkedExecutable::start("desktop-pet", None);
        let second = UnlinkedExecutable::start("desktop-pet", None);
        let file = fs::File::open(&first.executable).unwrap();
        let metadata = file.metadata().unwrap();
        let captured = capture_process(first.child.id()).unwrap();
        assert_eq!(
            captured.mapped_file,
            MappedFileIdentity {
                device: metadata.dev() as u32,
                inode: metadata.ino(),
            }
        );
        assert_ne!(
            captured.incarnation,
            capture_process(second.child.id()).unwrap().incarnation
        );
        assert_ne!(
            captured.mapped_file,
            capture_process(second.child.id()).unwrap().mapped_file
        );
        let arguments = capture_process_arguments(&captured).unwrap();
        assert!(arguments.argv.iter().any(|arg| arg == UNLINKED_CHILD_TEST));
        assert!(arguments
            .environment
            .iter()
            .any(|entry| { entry.to_string_lossy() == format!("{UNLINKED_CHILD_ENV}=1") }));
        let mut reused_pid = captured.clone();
        reused_pid.incarnation.start_sec += 1;
        assert!(capture_process_arguments(&reused_pid).is_err());
        let mut exec_changed = captured.clone();
        exec_changed.mapped_file = capture_process(second.child.id()).unwrap().mapped_file;
        assert!(capture_process_arguments(&exec_changed).is_err());
        let retained = first.executable.with_file_name("retained-original");
        fs::rename(&first.executable, &retained).unwrap();
        let renamed = capture_process(first.child.id()).unwrap();
        assert_eq!(renamed.incarnation, captured.incarnation);
        assert_eq!(renamed.mapped_file, captured.mapped_file);
        assert!(capture_process_arguments(&renamed).is_ok());
    }

    #[test]
    fn unlinked_scoped_main_image_never_becomes_an_argv_exemption() {
        let mut process = UnlinkedExecutable::start("desktop-pet", None);
        assert!(capture_process(process.child.id()).is_ok());
        process.unlink();
        assert!(capture_process(process.child.id()).is_err());
        assert!(capture_scoped_process(process.pid()).is_err());
    }

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

    const UNLINKED_CHILD_ENV: &str = "DESKTOP_PET_TEST_UNLINKED_CHILD";
    const UNLINKED_CHILD_TEST: &str = "processes::process_admission_tests::unlinked_unrelated_executable_is_admitted_but_cannot_authorize_a_process";

    fn unlinked_child_mode() -> bool {
        if std::env::var_os(UNLINKED_CHILD_ENV).is_none() {
            return false;
        }
        use std::io::{Read, Write};
        let mut stdin = std::io::stdin().lock();
        let mut stdout = std::io::stdout().lock();
        writeln!(
            stdout,
            "DESKTOP_PET_UNLINKED_CHILD_READY:{}",
            std::process::id()
        )
        .unwrap();
        stdout.flush().unwrap();
        drop(stdout);
        // Keep the real executable alive until its owning parent closes stdin.
        loop {
            match stdin.read(&mut [0]) {
                Ok(0) => return true,
                Ok(_) => panic!("unlinked child received unexpected parent input"),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("unlinked child lifetime pipe failed: {error}"),
            }
        }
    }

    struct UnlinkedExecutable {
        child: std::process::Child,
        executable: PathBuf,
        _directory: tempfile::TempDir,
    }

    impl UnlinkedExecutable {
        fn start(basename: &str, alias: Option<&str>) -> Self {
            use std::process::{Command, Stdio};
            let directory = tempfile::tempdir().unwrap();
            let root = fs::canonicalize(directory.path()).unwrap();
            let executable = root.join(basename);
            // proc_pidpath can expose an exec image before userspace is ready.
            // Unlike /bin/sleep, this real copied image explicitly acknowledges
            // startup before the parent is allowed to unlink it.
            fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
            let launcher = if let Some(alias) = alias {
                let launcher = root.join(alias);
                std::os::unix::fs::symlink(&executable, &launcher).unwrap();
                launcher
            } else {
                executable.clone()
            };
            let child = Command::new(launcher)
                .args(["--exact", UNLINKED_CHILD_TEST, "--nocapture"])
                .env(UNLINKED_CHILD_ENV, "1")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let mut process = Self {
                child,
                executable,
                _directory: directory,
            };
            process.wait_for_ready();
            process.wait_for_path(true);
            process
        }

        fn pid(&self) -> libc::pid_t {
            self.child.id() as libc::pid_t
        }

        fn assert_live(&mut self, context: &str) {
            match self.child.try_wait() {
                Ok(None) => {}
                Ok(Some(status)) => panic!(
                    "unlinked executable child {} ({}) exited with {status} ({status:?}) while {context}",
                    self.child.id(),
                    self.executable.display()
                ),
                Err(error) => panic!(
                    "cannot inspect unlinked executable child {} while {context}: {error}",
                    self.child.id()
                ),
            }
        }

        fn wait_for_ready(&mut self) {
            use std::io::Read;
            use std::os::fd::AsRawFd;
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let marker = format!("DESKTOP_PET_UNLINKED_CHILD_READY:{}\n", self.child.id());
            let mut output = Vec::new();
            loop {
                self.assert_live("waiting for userspace readiness");
                let remaining = deadline
                    .checked_duration_since(std::time::Instant::now())
                    .unwrap_or_default();
                assert!(
                    !remaining.is_zero(),
                    "unlinked executable child {} did not report userspace readiness; stdout: {}",
                    self.child.id(),
                    String::from_utf8_lossy(&output)
                );
                let mut fd = libc::pollfd {
                    fd: self.child.stdout.as_ref().unwrap().as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let ready =
                    unsafe { libc::poll(&mut fd, 1, remaining.as_millis().max(1) as libc::c_int) };
                if ready < 0 {
                    let error = std::io::Error::last_os_error();
                    if error.kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    panic!("cannot poll unlinked executable child readiness: {error}");
                }
                if ready == 0 {
                    continue;
                }
                let mut bytes = [0; 1024];
                let count = match self.child.stdout.as_mut().unwrap().read(&mut bytes) {
                    Ok(count) => count,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        panic!("cannot read unlinked executable child readiness: {error}")
                    }
                };
                if count == 0 {
                    self.assert_live("stdout closed before userspace readiness");
                    panic!(
                        "unlinked executable child {} closed stdout before readiness; stdout: {}",
                        self.child.id(),
                        String::from_utf8_lossy(&output)
                    );
                }
                output.extend_from_slice(&bytes[..count]);
                if output
                    .windows(marker.len())
                    .any(|window| window == marker.as_bytes())
                {
                    self.assert_live("acknowledging userspace readiness");
                    return;
                }
            }
        }

        fn wait_for_path(&mut self, available: bool) {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let context = format!("waiting for path availability {available}");
            loop {
                self.assert_live(&context);
                let observed = process_path(self.pid());
                self.assert_live(&context);
                let ready = match (&observed, available) {
                    (Ok(path), true) => path == &self.executable,
                    (Err(_), false) => true,
                    _ => false,
                };
                if ready {
                    assert_eq!(unsafe { libc::kill(self.pid(), 0) }, 0);
                    self.assert_live(&context);
                    return;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "unlinked executable child {} path did not reach availability {available}: {observed:?}",
                    self.child.id()
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        fn unlink(&mut self) {
            self.assert_live("unlinking its executable");
            fs::remove_file(&self.executable).unwrap();
            self.wait_for_path(false);
        }
    }

    impl Drop for UnlinkedExecutable {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[test]
    fn unlinked_unrelated_executable_is_admitted_but_cannot_authorize_a_process() {
        if unlinked_child_mode() {
            return;
        }
        let mut process = UnlinkedExecutable::start("unrelated-sleeper", None);
        let roots = [process.executable.parent().unwrap().to_path_buf()];
        let image = executable_identity(&process.executable).unwrap();
        let admission = installation_process_admitted(process.pid(), &roots, &[], &mut [0; 4096]);
        process.assert_live("checking admission of its linked unrelated image");
        assert_eq!(admission, Ok(()));
        process.unlink();
        let admission = installation_process_admitted(process.pid(), &roots, &[], &mut [0; 4096]);
        process.assert_live("checking admission of its unlinked unrelated image");
        assert_eq!(admission, Ok(()));
        let authorization = installation_roots_admitted(&[], &[(process.child.id(), image)]);
        process.assert_live("checking that an unlinked image cannot authorize its PID");
        assert!(
            authorization.is_err(),
            "negative comm classification must not authenticate an authorized PID"
        );
    }

    fn assert_unlinked_pet_blocks(basename: &str, alias: Option<&str>) {
        let mut process = UnlinkedExecutable::start(basename, alias);
        let roots = [process.executable.parent().unwrap().to_path_buf()];
        let admission = installation_process_admitted(process.pid(), &roots, &[], &mut [0; 4096]);
        process.assert_live("checking refusal of its linked pet image");
        let error = admission.unwrap_err();
        assert!(error.starts_with("live installation process "));
        process.unlink();
        let info = process_short_bsd_info(process.pid());
        process.assert_live("observing its unlinked executable-derived comm");
        let info = info.unwrap();
        assert_ne!(info.status, libc::SZOMB);
        assert!(
            process_comm_may_be_pet(&info.comm),
            "an executable-derived pet comm must survive unlinking and symlink aliases"
        );
        let admission = installation_process_admitted(process.pid(), &roots, &[], &mut [0; 4096]);
        process.assert_live("checking refusal of its unlinked pet image");
        let error = admission.unwrap_err();
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

#[cfg(test)]
mod coordinated_process_tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn historical_client_authority_pins_the_entire_published_closure() {
        assert!(release_allows_clients(
            None,
            PUBLIC_BETA_ONE_SHA256,
            PUBLIC_BETA_ONE_SIDECAR_SHA256
        ));
        assert!(!release_allows_clients(
            None,
            PUBLIC_BETA_ONE_SHA256,
            "rewritten-sidecar"
        ));
        assert!(!release_allows_clients(
            None,
            "version-only-beta-one",
            PUBLIC_BETA_ONE_SIDECAR_SHA256
        ));
        assert!(!release_allows_clients(
            None,
            "unknown-image",
            "unknown-closure"
        ));
        assert!(release_allows_clients(
            Some(1),
            "new-paired-contract-image",
            ""
        ));
        assert!(!release_allows_clients(Some(2), "new-image", ""));
    }

    fn argv(args: &[&str]) -> Vec<OsString> {
        std::iter::once("desktop-pet")
            .chain(args.iter().copied())
            .map(OsString::from)
            .collect()
    }

    fn image(compatible: bool, inode: u64) -> PinnedClientImage {
        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        let metadata = file.metadata().unwrap();
        PinnedClientImage {
            _file: file,
            metadata,
            mapped_file: MappedFileIdentity { device: 1, inode },
            bundle: "/Applications/DesktopPet.app".into(),
            compatible,
        }
    }

    fn scope(compatible: bool) -> CompatibleClientScope {
        CompatibleClientScope {
            original: image(compatible, 10),
            candidate: None,
            config: "/private/profile/config".into(),
            state: "/private/profile/state".into(),
        }
    }

    fn evidence() -> ProcessEvidence {
        ProcessEvidence {
            incarnation: ProcessIncarnation {
                pid: 123,
                start_sec: 456,
                start_usec: 789,
            },
            mapped_file: MappedFileIdentity {
                device: 1,
                inode: 10,
            },
            path: "/private/retained-old/DesktopPet.app/Contents/MacOS/desktop-pet".into(),
            uid: unsafe { libc::geteuid() },
        }
    }

    fn arguments(args: &[&str]) -> ProcessArguments {
        ProcessArguments {
            argv: argv(args),
            environment: Vec::new(),
        }
    }

    #[test]
    fn exact_compatible_cli_grammar_does_not_grant_mutation_or_profile_aliases() {
        for args in [
            vec!["update-capabilities"],
            vec!["distribution-identity"],
            vec!["--version"],
            vec![
                "profile",
                "show",
                "--json",
                "--config-dir",
                "/private/profile/config",
                "--state-dir",
                "/private/profile/state",
            ],
            vec![
                "tern-exchange",
                "--config-dir",
                "/private/profile/config",
                "--state-dir",
                "/private/profile/state",
            ],
        ] {
            assert_eq!(
                scope(true).classify(&evidence(), Some(&arguments(&args)), None),
                InstallationProcessRole::CompatibleClient
            );
        }
        for args in [
            vec![],
            vec!["update-capabilities", "--json"],
            vec!["update-capabilities-extra"],
            vec!["profile", "show", "--json"],
            vec!["tern-exchange"],
            vec!["profile", "adopt"],
            vec!["legacy-migration-route"],
            vec!["migration-integration-handover"],
            vec!["installation", "discover"],
            vec!["installation", "inspect"],
            vec!["start"],
            vec!["ensure"],
            vec!["daemon"],
            vec!["stop"],
            vec!["settings"],
            vec!["pack", "validate"],
            vec!["unknown"],
            vec![
                "tern-exchange",
                "--state-dir",
                "/private/profile/state",
                "--config-dir",
                "/private/profile/config",
            ],
            vec![
                "tern-exchange",
                "--config-dir",
                "/private/profile/config",
                "--state-dir",
                "/private/other/state",
            ],
            vec![
                "tern-exchange",
                "--config-dir",
                "relative",
                "--state-dir",
                "/private/profile/state",
            ],
            vec![
                "tern-exchange",
                "--config-dir",
                "/private/profile/config",
                "--state-dir",
                "/private/profile/state",
                "--json",
            ],
            vec![
                "profile",
                "show",
                "--config-dir",
                "/private/profile/config",
                "--state-dir",
                "/private/profile/state",
                "--json",
            ],
            vec![
                "tern-exchange",
                "--config-dir",
                "/private/profile/config",
                "--config-dir",
                "/private/profile/config",
            ],
            vec![
                "tern-exchange",
                "--config-dir",
                "/private/profile/./config",
                "--state-dir",
                "/private/profile/state",
            ],
            vec![
                "tern-exchange",
                "--config-dir",
                "/private//profile/config",
                "--state-dir",
                "/private/profile/state",
            ],
            vec![
                "tern-exchange",
                "--config-dir",
                "/private/profile/config/",
                "--state-dir",
                "/private/profile/state",
            ],
        ] {
            assert_eq!(
                scope(true).classify(&evidence(), Some(&arguments(&args)), None),
                InstallationProcessRole::BlockingUser,
                "{args:?}"
            );
        }
    }

    #[test]
    fn profileless_queries_never_supply_a_stopped_profile() {
        assert_eq!(compatible_cli(&argv(&["--version"])), Some(None));
        assert_eq!(
            compatible_cli(&argv(&[
                "tern-exchange",
                "--config-dir",
                "/config",
                "--state-dir",
                "/state"
            ])),
            Some(Some(("/config".into(), "/state".into())))
        );
        assert_eq!(compatible_cli(&argv(&["tern-exchange"])), None);
    }

    #[test]
    fn admission_requires_pinned_vnode_contract_uid_and_complete_arguments() {
        let scope = scope(true);
        let args = arguments(&["--version"]);
        assert_eq!(
            scope.classify(&evidence(), None, None),
            InstallationProcessRole::BlockingUser
        );
        let mut other_uid = evidence();
        other_uid.uid = other_uid.uid.wrapping_add(1);
        assert_eq!(
            scope.classify(&other_uid, Some(&args), None),
            InstallationProcessRole::BlockingUser
        );
        let mut unknown_image = evidence();
        unknown_image.mapped_file.inode += 1;
        unknown_image.path = scope.original.bundle.join("Contents/MacOS/desktop-pet");
        assert_eq!(
            scope.classify(&unknown_image, Some(&args), None),
            InstallationProcessRole::BlockingUser
        );
        unknown_image.path = "/another/install/Contents/MacOS/desktop-pet".into();
        assert_eq!(
            scope.classify(&unknown_image, None, None),
            InstallationProcessRole::Unrelated
        );
        let unknown_release = super::coordinated_process_tests::scope(false);
        assert_eq!(
            unknown_release.classify(&evidence(), Some(&args), None),
            InstallationProcessRole::BlockingUser
        );
        // The same old vnode remains in scope even after canonical-root exchange.
        assert_eq!(
            scope.classify(&evidence(), Some(&args), None),
            InstallationProcessRole::CompatibleClient
        );
    }

    #[test]
    fn helper_and_nonapp_images_do_not_block_or_supply_client_profiles() {
        let scope = scope(true);
        for name in [
            "desktop-pet-update-coordinator",
            "rig-decode-worker",
            "unrelated-tool",
        ] {
            let mut helper = evidence();
            helper.path = scope.original.bundle.join("Contents/MacOS").join(name);
            helper.mapped_file.inode = 999;
            helper.uid = helper.uid.wrapping_add(1);
            // Before attempting UID/argv capture, both admission and profile
            // discovery exclude an unknown vnode with a proven non-app path.
            assert!(!path_may_be_app(&helper.path));
            assert_eq!(
                scope.classify(&helper, None, None),
                InstallationProcessRole::Unrelated
            );
            assert_eq!(
                scope.classify(&helper, Some(&arguments(&["daemon"])), None),
                InstallationProcessRole::Unrelated
            );
            // Names never exempt a process whose actual main-image vnode is pinned.
            helper.mapped_file = scope.original_identity();
            assert_eq!(
                scope.classify(&helper, None, None),
                InstallationProcessRole::BlockingUser
            );
        }
    }

    #[test]
    fn candidate_daemon_requires_pinned_candidate_and_exact_authenticated_incarnation() {
        let mut scope = scope(true);
        let original = evidence();
        let daemon_args = arguments(&["daemon"]);
        let original_auth = AuthorizedDaemon::Original {
            incarnation: original.incarnation,
            mapped_file: original.mapped_file,
        };
        assert_eq!(
            scope.classify(&original, Some(&daemon_args), Some(&original_auth)),
            InstallationProcessRole::AuthorizedDaemon
        );
        let mut recycled = original.clone();
        recycled.incarnation.start_usec += 1;
        assert_eq!(
            scope.classify(&recycled, Some(&daemon_args), Some(&original_auth)),
            InstallationProcessRole::BlockingUser
        );
        let mut candidate = original.clone();
        candidate.path = scope.original.bundle.join("Contents/MacOS/desktop-pet");
        candidate.mapped_file.inode = 20;
        let auth = AuthorizedDaemon::Candidate {
            incarnation: candidate.incarnation,
            mapped_file: candidate.mapped_file,
        };
        assert_eq!(
            scope.classify(&candidate, Some(&daemon_args), Some(&auth)),
            InstallationProcessRole::BlockingUser
        );
        scope.candidate = Some(image(true, 20));
        assert_eq!(
            scope.classify(&candidate, Some(&daemon_args), None),
            InstallationProcessRole::BlockingUser
        );
        assert_eq!(
            scope.classify(&candidate, Some(&daemon_args), Some(&original_auth)),
            InstallationProcessRole::BlockingUser
        );
        assert_eq!(
            scope.classify(&candidate, Some(&daemon_args), Some(&auth)),
            InstallationProcessRole::AuthorizedDaemon
        );
        let wrong_kind = AuthorizedDaemon::Original {
            incarnation: candidate.incarnation,
            mapped_file: candidate.mapped_file,
        };
        assert_eq!(
            scope.classify(&candidate, Some(&daemon_args), Some(&wrong_kind)),
            InstallationProcessRole::BlockingUser
        );
    }

    fn procargs(args: &[&[u8]], env: &[&[u8]]) -> Vec<u8> {
        let mut bytes = (args.len() as i32).to_ne_bytes().to_vec();
        bytes.extend_from_slice(b"/Applications/DesktopPet.app/Contents/MacOS/desktop-pet\0\0\0");
        for arg in args {
            bytes.extend_from_slice(arg);
            bytes.push(0);
        }
        for entry in env {
            bytes.extend_from_slice(entry);
            bytes.push(0);
        }
        bytes.push(0);
        bytes
    }

    #[test]
    fn bounded_kernel_parser_requires_complete_argv_and_environment() {
        let bytes = procargs(
            &[b"desktop-pet", b"tern-exchange", b""],
            &[b"HOME=/private/home", b"KEY=a=b"],
        );
        let parsed = parse_procargs(&bytes).unwrap();
        assert_eq!(
            parsed.argv,
            vec![
                OsString::from("desktop-pet"),
                "tern-exchange".into(),
                "".into()
            ]
        );
        assert_eq!(
            parsed.environment,
            vec![OsString::from("HOME=/private/home"), "KEY=a=b".into()]
        );
        for end in 0..bytes.len() {
            assert!(
                parse_procargs(&bytes[..end]).is_err(),
                "accepted truncation at {end}"
            );
        }
        for invalid in [
            procargs(&[], &[]),
            procargs(&[b"desktop-pet"], &[b"MISSING_EQUALS"]),
            procargs(&[b"desktop-pet"], &[b"=missing_key"]),
        ] {
            assert!(parse_procargs(&invalid).is_err());
        }
        let mut invalid_argc = bytes.clone();
        invalid_argc[..4].copy_from_slice(&i32::MAX.to_ne_bytes());
        assert!(parse_procargs(&invalid_argc).is_err());
        let mut apple_vector = bytes;
        apple_vector.extend_from_slice(b"DESKTOP_PET_UPDATER_TOKEN=not-environment\0");
        assert_eq!(
            parse_procargs(&apple_vector).unwrap().environment,
            vec![OsString::from("HOME=/private/home"), "KEY=a=b".into()]
        );
    }
}
