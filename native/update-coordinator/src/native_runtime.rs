//! Native lifecycle primitives. A socket reply is evidence only when bound to
//! its kernel peer, process incarnation, mapped main image and exact profile.
pub use crate::processes::{MappedFileIdentity, ProcessIncarnation};
use crate::{
    identity, installation, processes, protocol, ExecutableIdentity, InstallOrigin, UpdateContext,
    VerifiedInstallation,
};
use desktop_pet_contracts::{ObservationSelection, OBSERVATION_SELECTION_ENV};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfilePaths {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
}
impl ProfilePaths {
    pub fn validate(&self) -> Result<(), String> {
        for path in [&self.config_dir, &self.state_dir] {
            crate::ensure_absolute(path)?;
            let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
            if !m.is_dir()
                || m.uid() != unsafe { libc::geteuid() }
                || m.permissions().mode() & 0o077 != 0
                || fs::canonicalize(path).map_err(|e| e.to_string())? != *path
            {
                return Err(format!(
                    "unsafe native profile directory {}",
                    path.display()
                ));
            }
        }
        Ok(())
    }
}

pub fn captured_environment(
    observation_mode: ObservationSelection,
    mut lookup: impl FnMut(&str) -> Option<String>,
) -> Result<BTreeMap<String, String>, String> {
    #[cfg(desktop_pet_legacy)]
    {
        crate::legacy_entry::capture_environment(&observation_mode, lookup)
    }
    #[cfg(not(desktop_pet_legacy))]
    {
        let mut environment = BTreeMap::new();
        for key in [
            "HOME",
            "XDG_CONFIG_HOME",
            "XDG_STATE_HOME",
            "TERN_CONFIG_DIR",
            "TERN_DAEMON_SOCKET",
            "DESKTOP_PET_CONFIG_DIR",
            "DESKTOP_PET_STATE_DIR",
        ] {
            if let Some(value) = lookup(key) {
                environment.insert(key.into(), value);
            }
        }
        for key in ["TERN_CONFIG_DIR", "TERN_DAEMON_SOCKET"] {
            if let Some(value) = environment.get(key) {
                if value.len() > 4096 || value.contains('\0') || !Path::new(value).is_absolute() {
                    return Err(format!("invalid captured Tern routing variable {key}"));
                }
            }
        }
        environment.insert(
            OBSERVATION_SELECTION_ENV.into(),
            observation_mode.as_str().into(),
        );
        Ok(environment)
    }
}

#[derive(Clone, Debug)]
pub struct NativeInstance {
    pub incarnation: ProcessIncarnation,
    pub mapped_file: MappedFileIdentity,
    pub image: ExecutableIdentity,
    pub profile: ProfilePaths,
    pub instance_id: String,
    pub context: UpdateContext,
    reserved_start_generation: Option<u64>,
}

/// Holds original and prepared-copy FDs for the entire transaction. This is
/// intentionally not serializable: recovery must reopen and revalidate images.
pub struct NativeInstallScope {
    scope: processes::CompatibleClientScope,
}
impl NativeInstallScope {
    pub fn capture(
        original: &installation::ValidatedApp,
        profile: &ProfilePaths,
    ) -> Result<Self, String> {
        profile.validate()?;
        installation::validate_fresh_standalone_scope(&original.bundle)?;
        Ok(Self {
            scope: processes::CompatibleClientScope::capture(
                original,
                &profile.config_dir,
                &profile.state_dir,
            )?,
        })
    }
    pub fn reserve_target_scope(&mut self, target: &crate::InstallOrigin) -> Result<(), String> {
        self.scope.reserve_target_scope(target)
    }
    pub fn pin_prepared_candidate(
        &mut self,
        candidate: &installation::ValidatedApp,
    ) -> Result<(), String> {
        self.scope.pin_prepared_candidate(candidate)
    }
    pub fn admit(&self, daemon: Option<&NativeInstance>) -> Result<(), String> {
        let authorized = daemon
            .map(|daemon| -> Result<processes::AuthorizedDaemon, String> {
                let actual = processes::capture_process(daemon.incarnation.pid)?;
                if actual.incarnation != daemon.incarnation
                    || actual.mapped_file != daemon.mapped_file
                {
                    return Err("authorized daemon incarnation/image changed".into());
                }
                // Reauthenticate actual IPC status, not just a PID handed in by a caller.
                let live = status(&daemon.profile, Some(&actual), Duration::from_secs(3))?;
                verify_reply(&live, "status", &daemon.image, &daemon.profile)?;
                if live.instance_id != daemon.instance_id
                    || live.shutdown
                    || !live.running
                    || live.assets_override != daemon.context.assets_override
                    || live.observation_mode != daemon.context.observation_mode()?
                {
                    return Err("authorized daemon instance/context changed".into());
                }
                if actual.mapped_file == self.scope.original_identity() {
                    Ok(processes::AuthorizedDaemon::Original {
                        incarnation: actual.incarnation,
                        mapped_file: actual.mapped_file,
                    })
                } else if self.scope.candidate_identity() == Some(actual.mapped_file) {
                    let generation = protocol::stop_generation(&daemon.profile.state_dir)?;
                    if daemon.reserved_start_generation != Some(generation) {
                        return Err("candidate lacks complete reserved-start context".into());
                    }
                    Ok(processes::AuthorizedDaemon::Candidate {
                        incarnation: actual.incarnation,
                        mapped_file: actual.mapped_file,
                    })
                } else {
                    Err("daemon is not a pinned original/prepared image".into())
                }
            })
            .transpose()?;
        processes::admit_coordinated_processes(&self.scope, authorized.as_ref())
    }
}

/// Recovery pins only the opaque, snapshot-validated images produced by the
/// replacement reader, never caller-constructed app records or canonical bytes.
pub fn recover_install_scope(
    recovered: &installation::RecoveredReplacement,
    profile: &ProfilePaths,
) -> Result<NativeInstallScope, String> {
    profile.validate()?;
    Ok(NativeInstallScope {
        scope: processes::CompatibleClientScope::capture_recovered(
            recovered,
            &profile.config_dir,
            &profile.state_dir,
        )?,
    })
}

pub fn verify_original_scope(
    scope: &NativeInstallScope,
    original: &installation::InstallationRecord,
) -> Result<(), String> {
    if installation::inspect(&original.executable)? != *original {
        return Err("original installation owner/image changed before restoration".into());
    }
    scope.scope.verify_original_path(&original.image)
}

/// Offline source discovery never creates/adopts a profile. Profileless IPC
/// queries count as clients but contribute no restart or profile authority.
pub fn compatible_client_profiles(
    original: &installation::ValidatedApp,
) -> Result<(usize, Vec<ProfilePaths>), String> {
    let clients = processes::compatible_client_profiles(original)?;
    Ok((
        clients.client_count,
        clients
            .profiles
            .into_iter()
            .map(|(config_dir, state_dir)| ProfilePaths {
                config_dir,
                state_dir,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
struct Status {
    version: u32,
    command: String,
    ok: bool,
    running: bool,
    shutdown: bool,
    ready: bool,
    ui_ready: bool,
    control_ready: bool,
    instance_id: String,
    running_sha256: String,
    pid: u32,
    executable_path: PathBuf,
    config_dir: PathBuf,
    state_dir: PathBuf,
    app_version: String,
    updater_protocol: u32,
    observation_capability: u32,
    integration_api: u32,
    observation_mode: ObservationSelection,
    assets_override: Option<PathBuf>,
}

// Validate uniqueness recursively before serde decoding; Value alone is
// last-key-wins, including keys not present in a versioned reply struct.
struct UniqueJson;
impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueJson;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("unique JSON object keys")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<UniqueJson, M::Error> {
                let mut keys = BTreeSet::new();
                while let Some(key) = map.next_key::<String>()? {
                    if !keys.insert(key) {
                        return Err(serde::de::Error::custom("duplicate control reply key"));
                    }
                    map.next_value::<UniqueJson>()?;
                }
                Ok(UniqueJson)
            }
            fn visit_seq<S: serde::de::SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> Result<UniqueJson, S::Error> {
                while seq.next_element::<UniqueJson>()?.is_some() {}
                Ok(UniqueJson)
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
        }
        d.deserialize_any(Visitor)
    }
}

fn remaining(deadline: Instant) -> Result<Duration, String> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|v| !v.is_zero())
        .ok_or_else(|| "control exchange deadline exceeded".into())
}
fn poll(fd: i32, events: i16, deadline: Instant) -> Result<(), String> {
    loop {
        let ms = remaining(deadline)?.as_millis().clamp(1, i32::MAX as u128) as i32;
        let mut entry = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let n = unsafe { libc::poll(&mut entry, 1, ms) };
        if n > 0 {
            remaining(deadline)?;
            return Ok(());
        }
        if n == 0 {
            return Err("control exchange deadline exceeded".into());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error.to_string());
        }
    }
}
fn connect(path: &Path, deadline: Instant) -> Result<UnixStream, String> {
    let bytes = path.as_os_str().as_bytes();
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.is_empty() || bytes.len() >= address.sun_path.len() || bytes.contains(&0) {
        return Err("invalid control socket path".into());
    }
    address.sun_family = libc::AF_UNIX as _;
    for (dest, byte) in address.sun_path.iter_mut().zip(bytes) {
        *dest = *byte as _;
    }
    #[cfg(target_os = "macos")]
    {
        address.sun_len = std::mem::size_of_val(&address) as _;
    }
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    stream.set_nonblocking(true).map_err(|e| e.to_string())?;
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let n = unsafe {
        libc::connect(
            fd,
            (&address as *const libc::sockaddr_un).cast(),
            std::mem::size_of_val(&address) as _,
        )
    };
    if n < 0 {
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(e.to_string());
        }
        poll(fd, libc::POLLOUT, deadline)?;
        if let Some(error) = stream.take_error().map_err(|e| e.to_string())? {
            return Err(error.to_string());
        }
    }
    remaining(deadline)?;
    Ok(stream)
}
#[cfg(target_os = "macos")]
fn peer(stream: &UnixStream) -> Result<processes::ProcessEvidence, String> {
    let mut uid = 0;
    let mut gid = 0;
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0
        || uid != unsafe { libc::geteuid() }
    {
        return Err("control peer UID differs".into());
    }
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of_val(&pid) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            0,
            2,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut len,
        )
    } != 0
        || len as usize != std::mem::size_of_val(&pid)
        || pid <= 0
    {
        return Err("cannot authenticate control peer PID".into());
    }
    let evidence = processes::capture_process(pid as u32)?;
    if evidence.uid != uid {
        return Err("control peer effective UID changed".into());
    }
    Ok(evidence)
}
#[cfg(not(target_os = "macos"))]
fn peer(_: &UnixStream) -> Result<processes::ProcessEvidence, String> {
    Err("native peer proof requires macOS".into())
}

/// Shared absolute-deadline transport for the existing runner. Authenticates
/// UID and kernel process before sending even a query or updater control frame.
pub fn exchange<T: Serialize, R: for<'de> Deserialize<'de>>(
    path: &Path,
    request: &T,
    timeout: Duration,
) -> Result<R, String> {
    exchange_with_peer(path, request, timeout, None).map(|(value, _)| value)
}

/// Kernel evidence returned with a reply; fields are observation, not startup
/// authorization. Socket identity is authenticated before writing; that exact
/// process incarnation and mapped main image are rechecked after the frame.
pub struct NativePeer {
    incarnation: ProcessIncarnation,
    path: PathBuf,
}
impl NativePeer {
    pub fn incarnation(&self) -> ProcessIncarnation {
        self.incarnation
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}
pub fn exchange_observed<T: Serialize, R: for<'de> Deserialize<'de>>(
    path: &Path,
    request: &T,
    timeout: Duration,
) -> Result<(R, NativePeer), String> {
    exchange_with_peer(path, request, timeout, None).map(|(reply, peer)| {
        (
            reply,
            NativePeer {
                incarnation: peer.incarnation,
                path: peer.path,
            },
        )
    })
}
pub fn exchange_instance<T: Serialize, R: for<'de> Deserialize<'de>>(
    instance: &NativeInstance,
    request: &T,
    timeout: Duration,
) -> Result<R, String> {
    let expected = processes::capture_process(instance.incarnation.pid)?;
    if expected.incarnation != instance.incarnation || expected.mapped_file != instance.mapped_file
    {
        return Err("control source incarnation/main image changed".into());
    }
    exchange_with_peer(
        &instance.profile.state_dir.join("control.sock"),
        request,
        timeout,
        Some(&expected),
    )
    .map(|(reply, _)| reply)
}
fn exchange_with_peer<T: Serialize, R: for<'de> Deserialize<'de>>(
    path: &Path,
    request: &T,
    timeout: Duration,
    expected: Option<&processes::ProcessEvidence>,
) -> Result<(R, processes::ProcessEvidence), String> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or("invalid exchange deadline")?;
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err("unsafe control socket".into());
    }
    let mut bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
    const LIMIT: usize = 16384;
    if bytes.len() >= LIMIT {
        return Err("control request too large".into());
    }
    bytes.push(b'\n');
    let mut stream = connect(path, deadline)?;
    let before = peer(&stream)?;
    if expected.is_some_and(|e| e != &before) {
        return Err("control peer incarnation/main image differs".into());
    }
    let mut sent = 0;
    while sent < bytes.len() {
        poll(stream.as_raw_fd(), libc::POLLOUT, deadline)?;
        match stream.write(&bytes[sent..]) {
            Ok(0) => return Err("incomplete control request".into()),
            Ok(n) => sent += n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut response = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        poll(stream.as_raw_fd(), libc::POLLIN, deadline)?;
        match stream.read(&mut chunk) {
            Ok(0) => return Err("incomplete control reply".into()),
            Ok(n) => {
                if let Some(end) = chunk[..n].iter().position(|b| *b == b'\n') {
                    if end + 1 != n || response.len() + end >= LIMIT {
                        return Err("oversized or trailing control frame".into());
                    }
                    response.extend_from_slice(&chunk[..end]);
                    break;
                }
                if response.len() + n >= LIMIT {
                    return Err("control reply exceeds frame limit".into());
                }
                response.extend_from_slice(&chunk[..n]);
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    remaining(deadline)?;
    // The one-shot server may close immediately after its complete reply.
    // Darwin LOCAL_PEERPID then returns ENOTCONN even while the daemon is
    // alive. Recheck the process authenticated on this socket before sending,
    // never a PID supplied by the reply or discovered from the socket path.
    let after = processes::capture_process(before.incarnation.pid)?;
    if before != after {
        return Err("control peer changed during exchange".into());
    }
    remaining(deadline)?;
    serde_json::from_slice::<UniqueJson>(&response).map_err(|e| e.to_string())?;
    let value =
        serde_json::from_slice(&response).map_err(|e| format!("invalid control reply: {e}"))?;
    Ok((value, after))
}
fn status(
    profile: &ProfilePaths,
    expected: Option<&processes::ProcessEvidence>,
    timeout: Duration,
) -> Result<Status, String> {
    let (reply, evidence): (Status, _) = exchange_with_peer(
        &profile.state_dir.join("control.sock"),
        &serde_json::json!({"version":1,"command":"status"}),
        timeout,
        expected,
    )?;
    if reply.pid != evidence.incarnation.pid {
        return Err("status PID differs from authenticated peer".into());
    }
    Ok(reply)
}
fn verify_reply(
    reply: &Status,
    command: &str,
    image: &ExecutableIdentity,
    profile: &ProfilePaths,
) -> Result<(), String> {
    if reply.version != 1
        || reply.command != command
        || !reply.ok
        || reply.instance_id.is_empty()
        || reply.running_sha256 != image.sha256
        || reply.executable_path != image.path
        || reply.config_dir != profile.config_dir
        || reply.state_dir != profile.state_dir
        || reply.updater_protocol != identity::PROTOCOL
        || reply.observation_capability != identity::OBSERVATION_CAPABILITY
        || reply.integration_api != identity::INTEGRATION_API
    {
        return Err("native reply identity/protocol/profile mismatch".into());
    }
    Ok(())
}

pub fn capture_instance(
    origin: &InstallOrigin,
    profile: &ProfilePaths,
) -> Result<NativeInstance, String> {
    profile.validate()?;
    let image = match origin {
        InstallOrigin::Standalone { bundle } => installation::validate_staged_app(bundle)?.image,
        _ => return Err("coordinated native capture requires Standalone owner".into()),
    };
    let live = status(profile, None, Duration::from_secs(3))?;
    verify_reply(&live, "status", &image, profile)?;
    if !live.running || live.shutdown || live.app_version != crate::executable_version(&image)? {
        return Err("source daemon is not running the validated version".into());
    }
    let process = processes::capture_process(live.pid)?;
    if process.path != image.path {
        return Err("source main image path differs".into());
    }
    let pinned = std::fs::File::open(&image.path)
        .map_err(|e| e.to_string())?
        .metadata()
        .map_err(|e| e.to_string())?;
    if process.mapped_file
        != (MappedFileIdentity {
            device: pinned.dev() as u32,
            inode: pinned.ino(),
        })
    {
        return Err("source main image vnode differs".into());
    }
    let args = processes::capture_process_arguments(&process)?;
    let mut environment = BTreeMap::new();
    let mut startup = BTreeMap::new();
    for entry in args.environment {
        let bytes = entry.as_os_str().as_bytes();
        let Some(equal) = bytes.iter().position(|b| *b == b'=') else {
            return Err("invalid process environment".into());
        };
        // Only decode allowed keys; unrelated binary environment values carry no authority.
        let Ok(key) = std::str::from_utf8(&bytes[..equal]) else {
            continue;
        };
        if [
            identity::UPDATER_TOKEN_ENV,
            identity::UPDATER_GENERATION_ENV,
        ]
        .contains(&key)
        {
            let value = std::str::from_utf8(&bytes[equal + 1..])
                .map_err(|_| "invalid reserved-start environment")?;
            if startup.insert(key.to_owned(), value.to_owned()).is_some() {
                return Err("duplicate reserved-start environment".into());
            }
        }
        if [
            "HOME",
            "XDG_CONFIG_HOME",
            "XDG_STATE_HOME",
            "TERN_CONFIG_DIR",
            "TERN_DAEMON_SOCKET",
            "DESKTOP_PET_CONFIG_DIR",
            "DESKTOP_PET_STATE_DIR",
        ]
        .contains(&key)
        {
            let value = std::str::from_utf8(&bytes[equal + 1..])
                .map_err(|_| "non UTF-8 captured environment")?;
            if environment
                .insert(key.to_owned(), value.to_owned())
                .is_some()
            {
                return Err("duplicate captured environment key".into());
            }
        }
    }
    let reserved_start_generation = match (
        startup.get(identity::UPDATER_TOKEN_ENV),
        startup.get(identity::UPDATER_GENERATION_ENV),
    ) {
        (Some(token), Some(generation)) => {
            let generation = generation
                .parse()
                .map_err(|_| "invalid reserved-start generation")?;
            if protocol::start_allowed(origin, &profile.state_dir, Some(generation), Some(token))
                .is_ok()
            {
                Some(generation)
            } else {
                // A released operation no longer grants reserved execution.
                // Its existing runtime can still be observed using its exact
                // retained startup receipt and the unchanged Stop fence.
                protocol::start_allowed(origin, &profile.state_dir, None, None)?;
                if protocol::verified_runtime_start_fence(
                    origin,
                    &profile.state_dir,
                    live.pid,
                    &live.instance_id,
                    &image,
                )? != generation
                {
                    return Err("released runtime startup fence changed".into());
                }
                None
            }
        }
        (None, None) => None,
        _ => return Err("incomplete reserved-start environment".into()),
    };
    let environment = captured_environment(live.observation_mode.clone(), |key| {
        environment.get(key).cloned()
    })?;
    let context = UpdateContext {
        executable: image.path.clone(),
        version: live.app_version.clone(),
        instance_id: live.instance_id.clone(),
        config_dir: profile.config_dir.clone(),
        state_dir: profile.state_dir.clone(),
        running: image.clone(),
        running_origin: Some(Box::new(origin.clone())),
        assets_override: live.assets_override.clone(),
        environment,
        locale: "en".into(),
        #[cfg(desktop_pet_legacy)]
        host_plugin_config_dir: None,
        #[cfg(desktop_pet_legacy)]
        herdr_socket: profile.state_dir.join("control.sock"),
    };
    let rechecked = status(profile, Some(&process), Duration::from_secs(3))?;
    verify_reply(&rechecked, "status", &image, profile)?;
    if rechecked.instance_id != live.instance_id
        || rechecked.app_version != live.app_version
        || rechecked.assets_override != live.assets_override
        || rechecked.observation_mode != live.observation_mode
        || rechecked.shutdown
    {
        return Err("source context changed during capture".into());
    }
    Ok(NativeInstance {
        incarnation: process.incarnation,
        mapped_file: process.mapped_file,
        image,
        profile: profile.clone(),
        instance_id: live.instance_id,
        context,
        reserved_start_generation,
    })
}

fn lifecycle_quiescent(profile: &ProfilePaths) -> Result<bool, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(profile.state_dir.join("desktop-pet.lock"))
        .map_err(|e| e.to_string())?;
    let m = file.metadata().map_err(|e| e.to_string())?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || m.permissions().mode() & 0o077 != 0
    {
        return Err("unsafe native lifecycle lock".into());
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return if std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK) {
            Ok(false)
        } else {
            Err("cannot inspect native lifecycle lock".into())
        };
    }
    startup_quiescent(profile)
}

fn startup_quiescent(profile: &ProfilePaths) -> Result<bool, String> {
    let lifecycle = profile.config_dir.join("lifecycle.json");
    match fs::symlink_metadata(&lifecycle) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(e.to_string()),
        Ok(_) => {
            let bytes = crate::owned_private_file(&lifecycle, 65536)?;
            serde_json::from_slice::<UniqueJson>(&bytes).map_err(|e| e.to_string())?;
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            Ok(!value
                .as_object()
                .ok_or("invalid lifecycle state")?
                .contains_key("startup"))
        }
    }
}

pub fn profile_quiescent(profile: &ProfilePaths) -> Result<bool, String> {
    profile.validate()?;
    let socket_gone = fs::symlink_metadata(profile.state_dir.join("control.sock"))
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
    Ok(socket_gone && lifecycle_quiescent(profile)?)
}

pub fn wait_install_source_stopped(
    plan: &crate::standalone_install::StandaloneInstallPlan,
    timeout: Duration,
    mut tick: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    plan.validate()?;
    let deadline = Instant::now() + timeout;
    loop {
        tick()?;
        let absent = plan.running().is_none_or(|(original, _)| {
            (unsafe { libc::kill(original.pid as i32, 0) }) == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        });
        if absent && profile_quiescent(&plan.profile)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("approved original process/socket/lifecycle finalization unproved".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Authenticate the current owner without inheriting an earlier instance's mutable UI/settings
/// context. This is observation only: it does not issue a startup token or start a daemon.
pub fn verify_current_ready(instance: &NativeInstance, generation: u64) -> Result<(), String> {
    instance.profile.validate()?;
    let installed = installation::inspect(&instance.image.path)?;
    if installed.image != instance.image
        || instance.context.running_origin.as_deref() != Some(&installed.origin)
    {
        return Err("current runtime does not match its signed installation owner".into());
    }
    let evidence = processes::capture_process(instance.incarnation.pid)?;
    if evidence.incarnation != instance.incarnation
        || evidence.mapped_file != instance.mapped_file
        || evidence.path != instance.image.path
    {
        return Err("current runtime process incarnation/image changed".into());
    }
    let live = status(&instance.profile, Some(&evidence), Duration::from_secs(3))?;
    verify_reply(&live, "status", &instance.image, &instance.profile)?;
    if live.instance_id != instance.instance_id
        || !live.running
        || live.shutdown
        || live.app_version != installed.app_version
        || !live.ready
        || !live.ui_ready
        || !live.control_ready
        || !startup_quiescent(&instance.profile)?
        || protocol::stop_generation(&instance.profile.state_dir)? != generation
        || protocol::verified_runtime_start_fence(
            &installed.origin,
            &instance.profile.state_dir,
            instance.incarnation.pid,
            &instance.instance_id,
            &instance.image,
        )? != generation
    {
        return Err("current runtime UI/control/startup/Stop readiness is unproved".into());
    }
    Ok(())
}

pub fn verify_installed_ready(
    context: &UpdateContext,
    installed: &VerifiedInstallation,
    generation: u64,
) -> Result<NativeInstance, String> {
    let profile = ProfilePaths {
        config_dir: context.config_dir.clone(),
        state_dir: context.state_dir.clone(),
    };
    let instance = capture_instance(&installed.origin, &profile)?;
    let evidence = processes::capture_process(instance.incarnation.pid)?;
    let live = status(&profile, Some(&evidence), Duration::from_secs(3))?;
    verify_reply(&live, "status", &installed.image, &profile)?;
    if instance.image != installed.image
        || instance.instance_id == context.instance_id
        || instance.context.assets_override != context.assets_override
        || instance.context.observation_mode()? != context.observation_mode()?
        || !live.ready
        || !live.ui_ready
        || !live.control_ready
        || !startup_quiescent(&profile)?
        || protocol::stop_generation(&profile.state_dir)? != generation
        || protocol::verified_runtime_start_fence(
            &installed.origin,
            &profile.state_dir,
            instance.incarnation.pid,
            &instance.instance_id,
            &installed.image,
        )? != generation
    {
        return Err("installed image/profile/UI/control/Stop readiness is unproved".into());
    }
    Ok(instance)
}
pub fn wait_stopped(
    instance: &NativeInstance,
    timeout: Duration,
    mut tick: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        tick()?;
        // PID reuse is not proof of original exit: retain evidence until no process.
        let absent = unsafe { libc::kill(instance.incarnation.pid as i32, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        let socket_gone = fs::symlink_metadata(instance.profile.state_dir.join("control.sock"))
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
        if absent && socket_gone && lifecycle_quiescent(&instance.profile)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("original process/socket/lifecycle finalization unproved".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn start_verified(
    context: &UpdateContext,
    installed: &VerifiedInstallation,
    token: &str,
    generation: u64,
    mut tick: impl FnMut() -> Result<(), String>,
) -> Result<NativeInstance, String> {
    let profile = ProfilePaths {
        config_dir: context.config_dir.clone(),
        state_dir: context.state_dir.clone(),
    };
    profile.validate()?;
    protocol::start_allowed(
        &installed.origin,
        &profile.state_dir,
        Some(generation),
        Some(token),
    )?;
    if crate::executable_identity(&installed.image.path)? != installed.image {
        return Err("installed image changed before launch".into());
    }
    let selection = context.observation_mode()?;
    let mut command = Command::new(&installed.image.path);
    command
        .arg("start")
        .arg("--config-dir")
        .arg(&profile.config_dir)
        .arg("--state-dir")
        .arg(&profile.state_dir)
        .arg("--host")
        .arg(selection.as_str());
    if let Some(assets) = &context.assets_override {
        command.arg("--assets").arg(assets);
    }
    command
        .env_clear()
        .envs(&context.environment)
        .env(identity::OBSERVATION_ENV, selection.as_str())
        .env(identity::UPDATER_TOKEN_ENV, token)
        .env(identity::UPDATER_GENERATION_ENV, generation.to_string())
        .env("DESKTOP_PET_CONFIG_DIR", &profile.config_dir)
        .env("DESKTOP_PET_STATE_DIR", &profile.state_dir)
        .current_dir(&profile.state_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    tick()?;
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        tick()?;
        if protocol::stop_generation(&profile.state_dir)? != generation {
            return Err("user Stop supersedes reserved start".into());
        }
        let exited = match child.try_wait().map_err(|e| e.to_string())? {
            Some(exit) if !exit.success() => return Err(format!("native start failed: {exit}")),
            Some(_) => true,
            None => false,
        };
        if exited {
            if let Ok(instance) = capture_instance(&installed.origin, &profile) {
                let actual = processes::capture_process(instance.incarnation.pid)?;
                let live = status(
                    &profile,
                    Some(&actual),
                    remaining(deadline)?.min(Duration::from_secs(3)),
                )?;
                if instance.image == installed.image
                    && instance.instance_id != context.instance_id
                    && instance.context.assets_override == context.assets_override
                    && instance.context.observation_mode()? == selection
                    && live.ready
                    && live.ui_ready
                    && live.control_ready
                    && live.app_version == crate::executable_version(&installed.image)?
                    && startup_quiescent(&profile)?
                {
                    tick()?;
                    if protocol::stop_generation(&profile.state_dir)? != generation {
                        return Err("user Stop supersedes readiness".into());
                    }
                    return Ok(instance);
                }
            }
        }
        remaining(deadline)?;
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Only authenticated exchange can construct this authority. Durable JSON is
/// observation, never a constructor for a fresh in-memory acknowledgement.
pub struct VerifiedBootstrapStopAck {
    record: crate::standalone_install::BootstrapStopAck,
}
impl VerifiedBootstrapStopAck {
    pub fn record(&self) -> &crate::standalone_install::BootstrapStopAck {
        &self.record
    }
}
pub fn stop_for_install(
    plan: &crate::standalone_install::StandaloneInstallPlan,
    instance: &NativeInstance,
) -> Result<VerifiedBootstrapStopAck, String> {
    use crate::standalone_install::{BootstrapStopAck, InstallSource, StandaloneOriginalInstance};
    plan.validate_for_execution()?;
    let InstallSource::Running {
        instance: approved,
        context,
    } = &plan.source
    else {
        return Err("stopped-source install has no Stop authority".into());
    };
    let original = StandaloneOriginalInstance {
        pid: instance.incarnation.pid,
        start_sec: instance.incarnation.start_sec,
        start_usec: instance.incarnation.start_usec,
        instance_id: instance.instance_id.clone(),
        image: instance.image.clone(),
    };
    if approved != &original
        || context != &instance.context
        || plan.profile != instance.profile
        || plan.original_stop_generation != protocol::stop_generation(&instance.profile.state_dir)?
    {
        return Err("Stop source differs from immutable approval/baseline".into());
    }
    let evidence = processes::capture_process(instance.incarnation.pid)?;
    if evidence.incarnation != instance.incarnation || evidence.mapped_file != instance.mapped_file
    {
        return Err("Stop source process changed".into());
    }
    let live = status(&instance.profile, Some(&evidence), Duration::from_secs(3))?;
    verify_reply(&live, "status", &instance.image, &instance.profile)?;
    if live.instance_id != instance.instance_id
        || live.shutdown
        || !live.running
        || live.assets_override != context.assets_override
        || live.observation_mode != context.observation_mode()?
    {
        return Err("Stop source status changed".into());
    }
    // Exactly one attempt. Any exchange failure is Unknown, never replayed.
    let (reply, _): (Status, _) = exchange_with_peer(
        &instance.profile.state_dir.join("control.sock"),
        &serde_json::json!({"version":1,"command":"stop"}),
        Duration::from_secs(3),
        Some(&evidence),
    )?;
    verify_reply(&reply, "stop", &instance.image, &instance.profile)?;
    if !reply.shutdown || reply.pid != original.pid || reply.instance_id != original.instance_id {
        return Err("Stop acknowledgement does not identify shutting-down original".into());
    }
    Ok(VerifiedBootstrapStopAck {
        record: BootstrapStopAck {
            version: 1,
            operation_id: plan.operation_id.clone(),
            origin: plan.original.origin.clone(),
            profile: plan.profile.clone(),
            original,
            original_stop_generation: plan.original_stop_generation,
            control_token: plan.control_token.clone(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_authority_and_nested_keys_are_rejected() {
        for bytes in [
            br#"{"ok":true,"ok":false}"#.as_slice(),
            br#"{"extra":{"pid":1,"pid":2}}"#,
        ] {
            assert!(serde_json::from_slice::<UniqueJson>(bytes).is_err());
        }
    }
    #[test]
    #[cfg(target_os = "macos")]
    fn slow_partial_reply_cannot_extend_absolute_deadline() {
        use std::os::unix::net::UnixListener;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("control.sock");
        let listener = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 128];
            stream.read(&mut request).unwrap();
            for _ in 0..20 {
                if stream.write_all(b" ").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        });
        let begin = Instant::now();
        let result: Result<serde_json::Value, _> = exchange(
            &path,
            &serde_json::json!({"version":1,"command":"status"}),
            Duration::from_millis(150),
        );
        assert!(result.unwrap_err().contains("deadline"));
        assert!(begin.elapsed() < Duration::from_millis(500));
        server.join().unwrap();
    }
    #[test]
    #[cfg(target_os = "macos")]
    fn one_shot_reply_survives_peer_socket_close() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("control.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let expected = processes::capture_process(std::process::id()).unwrap();
        // Exercise the public observed exchange, including its pre-send
        // authentication and post-reply incarnation/main-image verification.
        // Each accepted socket has the same one-frame lifetime as control.rs.
        const EXCHANGES: usize = 16;
        let server = std::thread::spawn(move || {
            for _ in 0..EXCHANGES {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                std::io::BufRead::read_until(
                    &mut std::io::BufReader::new(&mut stream),
                    b'\n',
                    &mut request,
                )
                .unwrap();
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&request).unwrap(),
                    serde_json::json!({"version":1,"command":"status"}),
                );
                stream.write_all(b"{\"ok\":true}\n").unwrap();
                // Do not retain the socket to accommodate the client's
                // verifier: native control handlers close after the reply.
            }
        });
        for _ in 0..EXCHANGES {
            let (reply, observed): (serde_json::Value, _) = exchange_observed(
                &path,
                &serde_json::json!({"version":1,"command":"status"}),
                Duration::from_secs(3),
            )
            .unwrap();
            assert_eq!(reply, serde_json::json!({"ok":true}));
            assert_eq!(observed.incarnation(), expected.incarnation);
            assert_eq!(observed.path(), expected.path);
        }
        server.join().unwrap();
    }
    #[test]
    #[cfg(target_os = "macos")]
    fn mismatched_incarnation_receives_no_stop_frame() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("control.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let mut expected = processes::capture_process(std::process::id()).unwrap();
        expected.incarnation.start_usec = (expected.incarnation.start_usec + 1) % 1_000_000;
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = [0u8; 128];
            stream.read(&mut bytes).unwrap()
        });
        let result: Result<(serde_json::Value, _), String> = exchange_with_peer(
            &path,
            &serde_json::json!({"version":1,"command":"stop"}),
            Duration::from_secs(1),
            Some(&expected),
        );
        assert!(matches!(&result, Err(error) if error.contains("peer incarnation")));
        assert_eq!(server.join().unwrap(), 0);
    }
}
