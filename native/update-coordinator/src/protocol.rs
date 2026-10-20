//! Private journals, per-install reservation and per-profile stop-intent gate.
//! Every ensure/start path must consult `start_allowed`; user stop calls `record_user_stop`
//! *before* touching a daemon, including when the daemon is already offline.
use crate::{ensure_absolute, owned_private_file, InstallOrigin};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationPhase {
    Preparing,
    Stopping,
    Installing,
    Validating,
    Starting,
    Completed,
    Failed,
    Unknown,
    UserStopped,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionFence {
    NoSpawn,
    ManagerIntent,
    ManagerExited,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationRecord {
    pub version: u32,
    pub operation_id: String,
    pub phase: OperationPhase,
    pub execution_fence: ExecutionFence,
    pub detail: String,
    pub installed: bool,
    pub applied: bool,
    pub candidate: Option<crate::ExecutableIdentity>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateControlRequest {
    pub version: u32,
    pub kind: String,
    pub command: UpdateCommand,
    pub instance_id: String,
    pub operation_id: String,
    pub token: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateCommand {
    Prepare,
    Stop,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateControlReply {
    pub version: u32,
    pub instance_id: String,
    pub operation_id: String,
    pub accepted: bool,
    pub detail: String,
}
pub const UPDATER_PROTOCOL: u32 = crate::identity::PROTOCOL;
const UPDATES_DIR: &str = crate::identity::UPDATES_DIRECTORY;

pub fn validate_operation_id(id: &str) -> Result<(), String> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err("operation id must be 128-bit lowercase hex".into());
    }
    Ok(())
}
pub(crate) fn check_dir(path: &Path, create: bool) -> Result<(), String> {
    ensure_absolute(path)?;
    if create {
        use std::os::unix::fs::DirBuilderExt;
        match fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.permissions().mode() & 0o077 != 0
    {
        return Err(format!(
            "update directory must be private and owned: {}",
            path.display()
        ));
    }
    Ok(())
}

/// Single-process lease held through journal creation, reservation, execution and recovery.
pub struct OperationLease {
    _file: FileLock,
}

pub fn lock_operation(state_dir: &Path, op: &str) -> Result<OperationLease, String> {
    validate_operation_id(op)?;
    let global = global_updates()?;
    let _gates = lock_gates(&global)?;
    reject_old_evidence(&global, None, None, Some(state_dir), false)?;
    let file = lock_with_mode(
        &private_updates(state_dir)?.join(format!("{op}.operation.lock")),
        libc::LOCK_EX | libc::LOCK_NB,
    )?;
    Ok(OperationLease { _file: file })
}
/// Nonblocking lease discovery: unsafe files and I/O failures are not "busy".
pub fn try_lock_operation(state_dir: &Path, op: &str) -> Result<Option<OperationLease>, String> {
    validate_operation_id(op)?;
    let global = global_updates()?;
    let _gates = lock_gates(&global)?;
    reject_old_evidence(&global, None, None, Some(state_dir), false)?;
    Ok(try_lock_with_mode(
        &private_updates(state_dir)?.join(format!("{op}.operation.lock")),
        libc::LOCK_EX | libc::LOCK_NB,
    )?
    .map(|file| OperationLease { _file: file }))
}

#[cfg(test)]
pub(crate) mod test_scope {
    use super::*;
    pub(crate) fn reserve_install_at(
        global: &Path,
        source: &InstallOrigin,
        target: Option<&InstallOrigin>,
        state: &Path,
        id: &str,
    ) -> Result<InstallationReservation, String> {
        super::reserve_install_at(global, source, target, state, id)
    }
    pub(crate) fn start_allowed_at(
        global: &Path,
        origin: &InstallOrigin,
        state: &Path,
        generation: Option<u64>,
        token: Option<&str>,
    ) -> Result<(), String> {
        super::start_allowed_at(global, origin, state, generation, token)
    }
    pub(crate) fn reservation_snapshot_at(
        global: &Path,
        source: &InstallOrigin,
        target: Option<&InstallOrigin>,
        state: &Path,
        id: &str,
    ) -> Result<Option<ReservationSnapshot>, String> {
        super::reservation_snapshot_at(global, source, target, state, id)
    }
    pub(crate) fn reservation_exists_at(
        global: &Path,
        source: &InstallOrigin,
        target: Option<&InstallOrigin>,
        state: &Path,
        id: &str,
    ) -> Result<bool, String> {
        super::reservation_exists_at(global, source, target, state, id)
    }
    pub(crate) fn stop_generation_at(global: &Path, state: &Path) -> Result<u64, String> {
        let _gates = lock_gates(global)?;
        generation(state)
    }
    pub(crate) fn lock_operation_at(
        global: &Path,
        state: &Path,
        id: &str,
    ) -> Result<Option<OperationLease>, String> {
        validate_operation_id(id)?;
        let _gates = lock_gates(global)?;
        reject_old_evidence(global, None, None, Some(state), false)?;
        Ok(try_lock_with_mode(
            &private_updates(state)?.join(format!("{id}.operation.lock")),
            libc::LOCK_EX | libc::LOCK_NB,
        )?
        .map(|file| OperationLease { _file: file }))
    }
}
pub fn private_updates(state_dir: &Path) -> Result<PathBuf, String> {
    check_dir(state_dir, false)?;
    let updates = state_dir.join(UPDATES_DIR);
    check_dir(&updates, true)?;
    Ok(updates)
}
fn sync_parent(path: &Path) -> Result<(), String> {
    File::open(path.parent().ok_or("missing parent")?)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}
/// Atomic bounded replacement; symlinks/hardlinks at the destination are rejected.
pub fn write_private_bytes(path: &Path, data: &[u8], mode: u32) -> Result<(), String> {
    ensure_absolute(path)?;
    let limit = if mode == 0o700 {
        64 * 1024 * 1024
    } else {
        1024 * 1024
    };
    if data.len() > limit || !matches!(mode, 0o600 | 0o700) {
        return Err("invalid private file size/mode".into());
    }
    check_dir(path.parent().ok_or("missing parent")?, false)?;
    match fs::symlink_metadata(path) {
        Ok(m)
            if !m.is_file()
                || m.file_type().is_symlink()
                || m.uid() != unsafe { libc::geteuid() }
                || m.nlink() != 1
                || m.permissions().mode() & 0o077 != 0 =>
        {
            return Err("unsafe existing update file".into())
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    let mut random = [0u8; 12];
    use std::io::Read;
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut random))
        .map_err(|e| e.to_string())?;
    let tmp = path.with_extension(format!(
        "tmp-{}",
        random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        file.write_all(data)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        sync_parent(path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Immutable helper/plan creation: an operation id can never silently replace a prior plan.
pub fn write_private_new(path: &Path, data: &[u8], mode: u32) -> Result<(), String> {
    ensure_absolute(path)?;
    let limit = if mode == 0o700 {
        64 * 1024 * 1024
    } else {
        1024 * 1024
    };
    if data.len() > limit || !matches!(mode, 0o600 | 0o700) {
        return Err("invalid private file size/mode".into());
    }
    check_dir(path.parent().ok_or("missing parent")?, false)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| e.to_string())?;
    let result = file
        .write_all(data)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
        .and_then(|_| sync_parent(path));
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

/// Publish complete immutable evidence without ever exposing a partial final file.
/// An error after the exclusive rename leaves the final evidence in place: callers
/// must classify and sync it before acting, not infer absence from this return value.
pub fn write_private_atomic_new(path: &Path, data: &[u8], mode: u32) -> Result<(), String> {
    ensure_absolute(path)?;
    if data.len() > 1024 * 1024 || mode != 0o600 {
        return Err("invalid atomic evidence size/mode".into());
    }
    let parent = path.parent().ok_or("missing parent")?;
    check_dir(parent, false)?;
    let temporary = parent.join(format!(".publish-{}", new_token()?));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        file.write_all(data)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        publish_exclusive(&temporary, path)?;
        sync_parent(path)
    })();
    // Only the unpublished temporary name may be cleaned up. In particular,
    // parent fsync failure must never erase an already-published commit receipt.
    let _ = fs::remove_file(&temporary);
    result
}

#[cfg(target_os = "macos")]
fn publish_exclusive(source: &Path, target: &Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn renamex_np(
            from: *const libc::c_char,
            to: *const libc::c_char,
            flags: libc::c_uint,
        ) -> libc::c_int;
    }
    let source = std::ffi::CString::new(source.as_os_str().as_bytes())
        .map_err(|_| "invalid publication path")?;
    let target = std::ffi::CString::new(target.as_os_str().as_bytes())
        .map_err(|_| "invalid publication path")?;
    if unsafe { renamex_np(source.as_ptr(), target.as_ptr(), 4) } != 0 {
        return Err(format!(
            "exclusive evidence publication: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn publish_exclusive(_: &Path, _: &Path) -> Result<(), String> {
    Err("atomic native evidence publication requires macOS".into())
}
pub fn write_operation(state_dir: &Path, record: &OperationRecord) -> Result<PathBuf, String> {
    validate_operation_id(&record.operation_id)?;
    let updates = private_updates(state_dir)?;
    validate_managed_operation_marker(state_dir)?;
    let target = updates.join(format!("{}.result.json", record.operation_id));
    if !missing_file(&target) {
        let previous = owned_private_file(&target, 16384)?;
        let previous: OperationRecord = serde_json::from_slice(&previous)
            .map_err(|_| "existing operation journal is unsupported; keep original bytes")?;
        if previous.version != UPDATER_PROTOCOL {
            return Err("existing operation journal is unsupported; keep original bytes".into());
        }
    }
    let latest = updates.join("latest-operation.json");
    if !missing_file(&latest) {
        let old_id: String = serde_json::from_slice(&owned_private_file(&latest, 128)?)
            .map_err(|_| "existing latest-operation marker is unsupported")?;
        read_operation_observation(state_dir, &old_id)?
            .ok_or("existing latest-operation marker has no durable journal")?;
    }
    if record.version != UPDATER_PROTOCOL {
        return Err("unsupported operation journal protocol".into());
    }
    if record.detail.len() > 8192 || record.applied && !record.installed {
        return Err("invalid operation outcome".into());
    }
    let path = target;
    let bytes = serde_json::to_vec(record).map_err(|e| e.to_string())?;
    write_private_bytes(&path, &bytes, 0o600)?;
    write_private_bytes(
        &path.with_file_name("latest-operation.json"),
        &serde_json::to_vec(&record.operation_id).map_err(|e| e.to_string())?,
        0o600,
    )?;
    write_managed_operation_marker(
        state_dir,
        ManagedOperationKind::Update,
        &record.operation_id,
    )?;
    Ok(path)
}
/// Executable authority: historical journals are never accepted by a current helper.
pub fn read_operation(
    state_dir: &Path,
    operation_id: &str,
) -> Result<Option<OperationRecord>, String> {
    let record = read_profile_operation(state_dir, operation_id)?;
    if record
        .as_ref()
        .is_some_and(|record| record.version != UPDATER_PROTOCOL)
    {
        return Err("unsupported operation journal protocol".into());
    }
    Ok(record)
}

fn read_profile_operation(
    state_dir: &Path,
    operation_id: &str,
) -> Result<Option<OperationRecord>, String> {
    validate_operation_id(operation_id)?;
    check_dir(state_dir, false)?;
    let updates = state_dir.join(UPDATES_DIR);
    if missing_file(&updates) {
        return Ok(None);
    }
    check_dir(&updates, false)?;
    read_operation_in(&updates, operation_id)
}

fn read_operation_in(
    updates: &Path,
    operation_id: &str,
) -> Result<Option<OperationRecord>, String> {
    validate_operation_id(operation_id)?;
    let path = updates.join(format!("{operation_id}.result.json"));
    let bytes = match owned_private_file(&path, 16384) {
        Ok(v) => v,
        Err(_) if missing_file(&path) => return Ok(None),
        Err(e) => return Err(e),
    };
    let record: OperationRecord = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if record.operation_id != operation_id {
        return Err("journal operation id mismatch".into());
    }
    if record.detail.len() > 8192 || record.applied && !record.installed {
        return Err("invalid operation outcome".into());
    }
    Ok(Some(record))
}

fn historical_identity(identity: &crate::ExecutableIdentity) -> Result<(), String> {
    ensure_absolute(&identity.path)?;
    if identity.sha256.len() != 64
        || !identity
            .sha256
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err("invalid historical executable identity".into());
    }
    Ok(())
}

fn historical_terminal(record: &OperationRecord) -> Result<(), String> {
    if !(1..UPDATER_PROTOCOL).contains(&record.version)
        || record.version > 3
        || record.detail.len() > 8192
        || record.applied && !record.installed
        || record.execution_fence == ExecutionFence::ManagerIntent
        || record.installed && record.execution_fence != ExecutionFence::ManagerExited
        || !match record.phase {
            OperationPhase::Completed => record.installed && record.applied,
            OperationPhase::Failed | OperationPhase::UserStopped => !record.applied,
            _ => false,
        }
    {
        return Err(
            "historical operation is unresolved; retain its matching helper and evidence".into(),
        );
    }
    if let Some(candidate) = record.candidate.as_ref() {
        historical_identity(candidate)?;
    }
    Ok(())
}

/// Read-only historical scope, never a plan accepted by `read_plan` or a runner.
/// No image is opened: an old keg/checkout may legitimately no longer exist.
pub fn historical_operation_plan(
    state_dir: &Path,
    record: &OperationRecord,
) -> Result<crate::UpdatePlan, String> {
    historical_terminal(record)?;
    validate_operation_id(&record.operation_id)?;
    check_dir(state_dir, false)?;
    let updates = state_dir.join(UPDATES_DIR);
    check_dir(&updates, false)?;
    let marker = updates.join("profile.reservation.json");
    if !missing_file(&marker) {
        owned_private_file(&marker, 16384)?;
        return Err("historical operation still has a reservation; retain matching helper".into());
    }
    let data = owned_private_file(
        &updates.join(format!("{}.plan.json", record.operation_id)),
        65536,
    )?;
    let plan: crate::UpdatePlan = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    if plan.version != record.version
        || plan.operation_id != record.operation_id
        || plan.context.state_dir != state_dir
        || plan.running != plan.context.running
        || plan.context.instance_id.is_empty()
        || plan.context.instance_id.len() > 128
        || plan
            .context
            .running_origin
            .as_deref()
            .is_some_and(|origin| origin != &plan.origin)
    {
        return Err("historical plan scope or identity mismatch".into());
    }
    ensure_absolute(&plan.context.config_dir)?;
    ensure_absolute(&plan.context.executable)?;
    ensure_absolute(crate::physical_install_root(&plan.origin)?)?;
    ensure_absolute(crate::physical_install_root(plan.target_origin())?)?;
    historical_identity(&plan.running)?;
    if let Some(candidate) = plan.candidate.as_ref() {
        historical_identity(candidate)?;
    }
    plan.context.observation_mode()?;
    Ok(plan)
}

/// Presentation/admission evidence only. The original version and bytes stay intact;
/// callers needing execution or recovery authority must use `read_operation`.
pub fn read_operation_observation(
    state_dir: &Path,
    operation_id: &str,
) -> Result<Option<OperationRecord>, String> {
    let record = read_profile_operation(state_dir, operation_id)?;
    if let Some(record) = record
        .as_ref()
        .filter(|record| record.version != UPDATER_PROTOCOL)
    {
        historical_operation_plan(state_dir, record)?;
    }
    Ok(record)
}

/// A reserved operation takes precedence over the most recently persisted outcome.
/// Read only: an untouched profile does not gain an update directory.
/// Historical terminal records are observations only, never recovery authority.
pub fn latest_operation(state_dir: &Path) -> Result<Option<OperationRecord>, String> {
    check_dir(state_dir, false)?;
    #[cfg(not(desktop_pet_legacy))]
    reject_profile_reservations(state_dir, true)?;
    let updates = state_dir.join(UPDATES_DIR);
    if missing_file(&updates) {
        return Ok(None);
    }
    check_dir(&updates, false)?;
    let marker = updates.join("profile.reservation.json");
    if !missing_file(&marker) {
        let scope = reservation(&owned_private_file(&marker, 16384)?)?;
        if scope.state_dir != state_dir {
            return Err("reserved update belongs to a different profile".into());
        }
        return read_operation(state_dir, &scope.operation_id)?
            .map(Some)
            .ok_or_else(|| {
                "reserved update has no durable outcome; inspect helper status/recover".into()
            });
    }
    let latest = updates.join("latest-operation.json");
    if missing_file(&latest) {
        return Ok(None);
    }
    let operation_id: String =
        serde_json::from_slice(&owned_private_file(&latest, 128)?).map_err(|e| e.to_string())?;
    read_operation_observation(state_dir, &operation_id)?
        .map(Some)
        .ok_or_else(|| "latest update marker has no durable outcome".into())
}

/// Typed presentation evidence. Neither variant grants execution authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationObservation {
    Update(OperationRecord),
    StandaloneInstall(crate::standalone_install::StandaloneInstallRecord),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedOperationKind {
    Update,
    StandaloneInstall,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagedOperationMarker {
    version: u32,
    kind: ManagedOperationKind,
    operation_id: String,
}

/// Called only after the corresponding result has been durably written.
pub fn write_managed_operation_marker(
    state: &Path,
    kind: ManagedOperationKind,
    id: &str,
) -> Result<(), String> {
    validate_operation_id(id)?;
    validate_managed_operation_marker(state)?;
    let durable = match kind {
        ManagedOperationKind::Update => read_operation_observation(state, id)?.is_some(),
        ManagedOperationKind::StandaloneInstall => {
            crate::standalone_install::read_record(state, id)?.is_some()
        }
    };
    if !durable {
        return Err("managed operation has no durable result".into());
    }
    write_private_bytes(
        &private_updates(state)?.join("latest-managed-operation.json"),
        &serde_json::to_vec(&ManagedOperationMarker {
            version: 1,
            kind,
            operation_id: id.into(),
        })
        .map_err(|e| e.to_string())?,
        0o600,
    )
}

pub(crate) fn validate_managed_operation_marker(state: &Path) -> Result<(), String> {
    let path = state
        .join(UPDATES_DIR)
        .join("latest-managed-operation.json");
    if missing_file(&path) {
        return Ok(());
    }
    let marker: ManagedOperationMarker =
        serde_json::from_slice(&owned_private_file(&path, 1024)?).map_err(|e| e.to_string())?;
    if marker.version != 1
        || observation_kind(state, &marker.operation_id, false)? != marker.kind
        || observe_operation(state, &marker.operation_id)?.is_none()
    {
        return Err("invalid latest managed operation marker".into());
    }
    Ok(())
}

fn observation_kind(
    state: &Path,
    id: &str,
    require_plan: bool,
) -> Result<ManagedOperationKind, String> {
    validate_operation_id(id)?;
    let normal = state.join(UPDATES_DIR).join(format!("{id}.plan.json"));
    let standalone = crate::standalone_install::plan_path(state, id)?;
    match (!missing_file(&normal), !missing_file(&standalone)) {
        (true, true) => Err("ambiguous managed operation approval kind".into()),
        (false, true) => {
            crate::standalone_install::read_plan(state, id)?;
            Ok(ManagedOperationKind::StandaloneInstall)
        }
        (true, false) => {
            let bytes = owned_private_file(&normal, 65536)?;
            let plan: crate::UpdatePlan =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if plan.operation_id != id || plan.context.state_dir != state {
                return Err("managed update approval scope mismatch".into());
            }
            if plan.version == UPDATER_PROTOCOL {
                crate::read_plan(state, id)?;
            } else {
                let record = read_operation_observation(state, id)?
                    .ok_or("historical approval has no result")?;
                historical_operation_plan(state, &record)?;
            }
            Ok(ManagedOperationKind::Update)
        }
        (false, false) if !missing_file(&crate::standalone_install::record_path(state, id)?) => {
            Err("standalone result has no immutable approval".into())
        }
        (false, false) if !require_plan => Ok(ManagedOperationKind::Update),
        (false, false) => Err("reserved managed operation has no approval".into()),
    }
}

pub fn observe_operation(state: &Path, id: &str) -> Result<Option<OperationObservation>, String> {
    check_dir(state, false)?;
    match observation_kind(state, id, false)? {
        ManagedOperationKind::Update => {
            read_operation_observation(state, id).map(|r| r.map(OperationObservation::Update))
        }
        ManagedOperationKind::StandaloneInstall => {
            crate::standalone_install::read_record(state, id)
                .map(|r| r.map(OperationObservation::StandaloneInstall))
        }
    }
}

pub fn observe_latest_operation(state: &Path) -> Result<Option<OperationObservation>, String> {
    observe_latest_operation_at(&global_updates()?, state)
}

fn observe_latest_operation_at(
    global: &Path,
    state: &Path,
) -> Result<Option<OperationObservation>, String> {
    check_dir(state, false)?;
    #[cfg(not(desktop_pet_legacy))]
    reject_profile_reservations(state, true)?;
    let _gates = lock_gates(global)?;
    let entries = scopes(global)?;
    let relevant: Vec<_> = entries
        .iter()
        .filter(|entry| entry.state_dir == state)
        .collect();
    if relevant.len() > 1 {
        return Err("ambiguous active managed operation".into());
    }
    let marker = state.join(UPDATES_DIR).join("profile.reservation.json");
    let marker = marker_at(&marker)?;
    let active = match (relevant.first(), marker.as_ref()) {
        (Some(entry), Some(marker))
            if entry.operation_id != marker.operation_id || marker.state_dir != state =>
        {
            return Err("active managed operation reservation mismatch".into());
        }
        (Some(entry), _) => Some(entry.operation_id.as_str()),
        (None, Some(_)) => return Err("managed operation marker lacks authoritative scope".into()),
        (None, None) => None,
    };
    if let Some(id) = active {
        observation_kind(state, id, true)?;
        return observe_operation(state, id)?.map(Some).ok_or_else(|| {
            "reserved managed operation has no durable result; inspect status/recover".into()
        });
    }
    let path = state
        .join(UPDATES_DIR)
        .join("latest-managed-operation.json");
    if missing_file(&path) {
        // Compatibility only: invalid typed markers never fall back.
        drop(_gates);
        return latest_operation(state).map(|record| record.map(OperationObservation::Update));
    }
    let marker: ManagedOperationMarker =
        serde_json::from_slice(&owned_private_file(&path, 1024)?).map_err(|e| e.to_string())?;
    if marker.version != 1 || observation_kind(state, &marker.operation_id, false)? != marker.kind {
        return Err("invalid latest managed operation marker".into());
    }
    observe_operation(state, &marker.operation_id)?
        .map(Some)
        .ok_or_else(|| "latest managed operation has no durable result".into())
}

pub fn installation_key(origin: &InstallOrigin) -> Result<String, String> {
    let root = crate::physical_install_root(origin)?;
    let material = match origin {
        #[cfg(desktop_pet_legacy)]
        InstallOrigin::Herdr { .. } => format!("checkout\0{}", root.display()),
        InstallOrigin::Source { .. } => format!("checkout\0{}", root.display()),
        InstallOrigin::Homebrew { formula, .. } => format!("brew\0{}\0{formula}", root.display()),
        InstallOrigin::Standalone { .. } | InstallOrigin::Manual { .. } => {
            format!("manual\0{}", root.display())
        }
        InstallOrigin::Unknown { .. } => {
            return Err("installation has no safely reservable owner".into())
        }
    };
    let digest = Sha256::digest(material.as_bytes());
    Ok(format!("{:x}", digest))
}

/// Earlier manager/formula and manual-image keys are never a clean namespace.
fn legacy_key(origin: &InstallOrigin) -> Option<String> {
    let material = match origin {
        InstallOrigin::Homebrew {
            prefix, formula, ..
        } => format!("brew\0{}\0{formula}", prefix.display()),
        InstallOrigin::Manual { locator } => format!("manual\0{}", locator.display()),
        _ => return None,
    };
    Some(format!("{:x}", Sha256::digest(material.as_bytes())))
}

fn reject_legacy_marker(global: &Path, origin: &InstallOrigin) -> Result<(), String> {
    if let Some(key) = legacy_key(origin) {
        if key != installation_key(origin)?
            && !missing_file(&global.join(format!("{key}.reservation.json")))
        {
            return Err(
                "legacy installation reservation remains blocked; preserve old marker".into(),
            );
        }
    }
    Ok(())
}

fn lock_scope(
    global: &Path,
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    keys: &[String],
) -> Result<Vec<FileLock>, String> {
    let mut origins = std::iter::once(source)
        .chain(target)
        .map(|origin| crate::physical_install_root(origin).map(|root| (root, origin)))
        .collect::<Result<Vec<_>, _>>()?;
    origins.sort_unstable_by(|a, b| a.0.cmp(b.0));
    let mut lock_keys = Vec::new();
    for (_, origin) in origins {
        let key = installation_key(origin)?;
        if keys.contains(&key) && !lock_keys.contains(&key) {
            lock_keys.push(key);
        }
        if let Some(old) = legacy_key(origin) {
            if !lock_keys.contains(&old) {
                lock_keys.push(old);
            }
        }
    }
    let old = old_global(global)?;
    let mut files = Vec::new();
    for key in &lock_keys {
        if let Some(old) = &old {
            files.push(lock_with_mode(
                &old.join(format!("{key}.lock")),
                libc::LOCK_EX | libc::LOCK_NB,
            )?);
        }
        files.push(lock_with_mode(
            &global.join(format!("{key}.lock")),
            libc::LOCK_EX | libc::LOCK_NB,
        )?);
    }
    Ok(files)
}
/// UID-wide durable private root outside replaceable app/checkout, shared by profiles.
pub fn global_updates() -> Result<PathBuf, String> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;
    let uid = unsafe { libc::geteuid() };
    let mut user: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0u8; 16384];
    let rc = unsafe {
        libc::getpwuid_r(
            uid,
            &mut user,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() || user.pw_dir.is_null() {
        return Err("cannot determine user home for updater reservation".into());
    }
    let home = Path::new(std::ffi::OsStr::from_bytes(
        unsafe { CStr::from_ptr(user.pw_dir) }.to_bytes(),
    ));
    ensure_absolute(home)?;
    let path = home.join(crate::identity::GLOBAL_UPDATES_DIRECTORY);
    check_dir(&path, true)?;
    Ok(path)
}

fn old_global(global: &Path) -> Result<Option<PathBuf>, String> {
    #[cfg(desktop_pet_legacy)]
    {
        let _ = global;
        Ok(None)
    }
    #[cfg(not(desktop_pet_legacy))]
    {
        let path = global
            .parent()
            .ok_or("global update root has no parent")?
            .join(".herdr-updates");
        check_dir(&path, true)?;
        Ok(Some(path))
    }
}

fn compatibility_global(global: &Path) -> Result<Option<PathBuf>, String> {
    #[cfg(desktop_pet_legacy)]
    {
        migration_global(global).map(Some)
    }
    #[cfg(not(desktop_pet_legacy))]
    {
        old_global(global)
    }
}

/// All namespace users acquire these gates in this order, never from HOME.
fn lock_gates(global: &Path) -> Result<Vec<FileLock>, String> {
    check_dir(global, false)?;
    let mut locks = Vec::new();
    if let Some(old) = old_global(global)? {
        locks.push(lock(&old.join("gate.lock"))?);
    }
    locks.push(lock(&global.join("gate.lock"))?);
    #[cfg(desktop_pet_legacy)]
    {
        let new = migration_global(global)?;
        locks.push(lock(&new.join("gate.lock"))?);
    }
    Ok(locks)
}

fn registry_files(global: &Path, suffix: &str) -> Result<Vec<PathBuf>, String> {
    check_dir(global, false)?;
    let mut paths = Vec::new();
    for entry in fs::read_dir(global).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.ends_with(suffix))
        {
            paths.push(entry.path());
            if paths.len() > 1024 {
                return Err("updater registry exceeds bound".into());
            }
        }
    }
    paths.sort_unstable();
    Ok(paths)
}

fn evidence_root(value: &serde_json::Value) -> Result<Option<PathBuf>, String> {
    let Some(kind) = value.get("kind").and_then(serde_json::Value::as_str) else {
        return Err("unsupported installation evidence; preserve original bytes".into());
    };
    let field = match kind {
        "herdr" => "checkout_root",
        "local" | "source" => "root",
        "homebrew" => "cellar_formula_root",
        "standalone" => "bundle",
        "manual" => "locator",
        "unknown" => return Ok(None),
        _ => return Err("unsupported installation owner; preserve original bytes".into()),
    };
    let path = PathBuf::from(
        value
            .get(field)
            .and_then(serde_json::Value::as_str)
            .ok_or("installation evidence has no physical root")?,
    );
    ensure_absolute(&path)?;
    if kind == "manual" {
        Ok(Some(
            path.parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .ok_or("invalid manual image evidence")?
                .to_path_buf(),
        ))
    } else {
        Ok(Some(path))
    }
}

fn evidence_matches(
    value: &serde_json::Value,
    roots: &[PathBuf],
    state: Option<&Path>,
) -> Result<bool, String> {
    let object = value
        .as_object()
        .ok_or("unsupported reservation evidence")?;
    if !matches!(
        object.get("version").and_then(serde_json::Value::as_u64),
        Some(1..=4)
    ) {
        return Err("unsupported reservation version; preserve original bytes".into());
    }
    let stored_state = PathBuf::from(
        object
            .get("state_dir")
            .and_then(serde_json::Value::as_str)
            .ok_or("reservation evidence has no profile")?,
    );
    ensure_absolute(&stored_state)?;
    let source = object
        .get("source")
        .ok_or("reservation evidence has no source")?;
    let mut matching = state.is_some_and(|state| state == stored_state);
    for origin in std::iter::once(source).chain(object.get("target").filter(|v| !v.is_null())) {
        if let Some(root) = evidence_root(origin)? {
            matching |= roots.iter().any(|candidate| candidate == &root);
        }
    }
    Ok(matching)
}

fn reject_reservation_evidence(
    global: &Path,
    roots: &[PathBuf],
    state: Option<&Path>,
) -> Result<(), String> {
    reject_migration_scopes(global, roots, state, None)?;
    reject_normal_reservation_evidence(global, roots, state)
}
fn reject_normal_reservation_evidence(
    global: &Path,
    roots: &[PathBuf],
    state: Option<&Path>,
) -> Result<(), String> {
    let index = scopes_path(global);
    if !missing_file(&index) {
        let entries: Vec<serde_json::Value> =
            serde_json::from_slice(&owned_private_file(&index, 1024 * 1024)?)
                .map_err(|_| "unsupported scope registry; preserve original bytes")?;
        if entries.len() > 256 {
            return Err("installation scope registry exceeds bound".into());
        }
        for entry in entries {
            if evidence_matches(&entry, roots, state)? {
                return Err(
                    "an existing update owns this profile or installation; use its matching helper"
                        .into(),
                );
            }
        }
    }
    for path in registry_files(global, ".reservation.json")? {
        let entry: serde_json::Value =
            serde_json::from_slice(&owned_private_file(&path, 16384)?)
                .map_err(|_| "unsupported reservation; preserve original bytes")?;
        if evidence_matches(&entry, roots, state)? {
            return Err(
                "existing installation reservation must be reconciled by its matching helper"
                    .into(),
            );
        }
    }
    Ok(())
}

fn root_registry_keys(roots: &[PathBuf]) -> Vec<String> {
    let mut keys = Vec::new();
    for root in roots {
        for material in [
            format!("checkout\0{}", root.display()),
            format!("manual\0{}", root.display()),
        ] {
            keys.push(format!("{:x}", Sha256::digest(material.as_bytes())));
        }
        for formula in [
            "desktop-pet",
            "desktop-pet-beta",
            "herdr-desktop-pet",
            "herdr-desktop-pet-beta",
            "hanbong5938/tap/desktop-pet",
            "hanbong5938/tap/desktop-pet-beta",
            "hanbong5938/tap/herdr-desktop-pet",
            "hanbong5938/tap/herdr-desktop-pet-beta",
        ] {
            keys.push(format!(
                "{:x}",
                Sha256::digest(format!("brew\0{}\0{formula}", root.display()).as_bytes())
            ));
        }
    }
    keys.sort_unstable();
    keys.dedup();
    keys
}

fn reject_active_evidence(
    global: &Path,
    roots: &[PathBuf],
    state: Option<&Path>,
    all_root_profiles: bool,
) -> Result<(), String> {
    let keys = root_registry_keys(roots);
    for path in registry_files(global, ".active.json")? {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("invalid active registry name")?;
        let root_matches = keys
            .iter()
            .any(|key| name.strip_suffix(".active.json") == Some(key.as_str()));
        let entries: Vec<ActiveProfile> =
            serde_json::from_slice(&owned_private_file(&path, 16384)?)
                .map_err(|_| "unsupported active registry; preserve original bytes")?;
        if entries.len() > 64 {
            return Err("active profile registry exceeds bound".into());
        }
        for entry in entries {
            ensure_absolute(&entry.state_dir)?;
            if live(entry.pid)
                && (state.is_some_and(|state| state == entry.state_dir)
                    || root_matches
                        && (all_root_profiles
                            || state.is_none_or(|state| state != entry.state_dir)))
            {
                return Err("a live daemon still owns the profile or installation".into());
            }
        }
    }
    Ok(())
}

fn reject_profile_reservations(state: &Path, old_only: bool) -> Result<(), String> {
    let directories: &[&str] = if old_only {
        &["updates"]
    } else {
        &["updates", "updates-v4"]
    };
    for directory in directories {
        let updates = state.join(directory);
        if missing_file(&updates) {
            continue;
        }
        check_dir(&updates, false)?;
        let marker = updates.join("profile.reservation.json");
        if !missing_file(&marker) {
            owned_private_file(&marker, 16384)?;
            return Err(
                "profile still has an updater reservation; preserve matching protocol evidence"
                    .into(),
            );
        }
        // An older coordinator may have crashed before writing its reservation.
        if *directory == "updates" && (old_only || UPDATER_PROTOCOL == 4) {
            let latest = updates.join("latest-operation.json");
            if !missing_file(&latest) {
                let id: String = serde_json::from_slice(&owned_private_file(&latest, 128)?)
                    .map_err(|_| "unsupported legacy journal locator; preserve original bytes")?;
                validate_operation_id(&id)?;
                let record = read_operation_in(&updates, &id)?
                    .ok_or("legacy journal locator has no durable outcome")?;
                historical_terminal(&record)?;
            }
        }
    }
    Ok(())
}

fn reject_old_evidence(
    global: &Path,
    source: Option<&InstallOrigin>,
    target: Option<&InstallOrigin>,
    state: Option<&Path>,
    starting: bool,
) -> Result<(), String> {
    let Some(old) = compatibility_global(global)? else {
        return Ok(());
    };
    let mut roots = Vec::new();
    for origin in source.into_iter().chain(target) {
        if !matches!(origin, InstallOrigin::Unknown { .. }) {
            roots.push(crate::physical_install_root(origin)?.to_path_buf());
            reject_legacy_marker(&old, origin)?;
        }
    }
    reject_reservation_evidence(&old, &roots, state)?;
    if let Some(state) = state {
        #[cfg(not(desktop_pet_legacy))]
        reject_profile_reservations(state, true)?;
        #[cfg(desktop_pet_legacy)]
        {
            let updates = state.join("updates-v4");
            if !missing_file(&updates) {
                check_dir(&updates, false)?;
                let marker = updates.join("profile.reservation.json");
                if !missing_file(&marker) {
                    owned_private_file(&marker, 16384)?;
                    return Err("neutral updater still owns this legacy profile".into());
                }
            }
        }
    }
    reject_active_evidence(&old, &roots, state, starting)
}

/// Retain this guard through staging, replacement and locator writes.
pub struct InstallationAdmission {
    _roots: Vec<FileLock>,
    _gates: Vec<FileLock>,
    roots: Vec<PathBuf>,
    global: PathBuf,
}
pub struct ProfileAdmission {
    _lifecycle: Vec<desktop_pet_contracts::LifecycleLock>,
    _admission: InstallationAdmission,
}
pub struct InstallationLocks {
    _admission: InstallationAdmission,
}

fn lock_roots(global: &Path, roots: &[PathBuf]) -> Result<Vec<FileLock>, String> {
    let old = old_global(global)?;
    let mut locks = Vec::new();
    for root in roots {
        for key in root_registry_keys(std::slice::from_ref(root)) {
            if let Some(old) = &old {
                locks.push(lock_with_mode(
                    &old.join(format!("{key}.lock")),
                    libc::LOCK_EX | libc::LOCK_NB,
                )?);
            }
            locks.push(lock_with_mode(
                &global.join(format!("{key}.lock")),
                libc::LOCK_EX | libc::LOCK_NB,
            )?);
        }
    }
    Ok(locks)
}

fn installation_roots(mut roots: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    if roots.is_empty() {
        return Err("installation admission requires a physical root".into());
    }
    for root in &roots {
        ensure_absolute(root)?;
        let mut ancestor = root.as_path();
        while missing_file(ancestor) {
            ancestor = ancestor
                .parent()
                .ok_or("installation root has no existing ancestor")?;
        }
        let physical = fs::canonicalize(ancestor).map_err(|e| e.to_string())?;
        if physical != ancestor || !fs::metadata(ancestor).map_err(|e| e.to_string())?.is_dir() {
            return Err("installation root must have a physical directory ancestor".into());
        }
    }
    roots.sort_unstable();
    roots.dedup();
    Ok(roots)
}

fn installation_admission_at(
    global: PathBuf,
    select_roots: impl FnOnce() -> Result<Vec<PathBuf>, String>,
    require_inactive: bool,
) -> Result<InstallationAdmission, String> {
    let gates = lock_gates(&global)?;
    // A waiting installer must select the winner's roots, not a pre-gate snapshot.
    let roots = installation_roots(select_roots()?)?;
    let locks = lock_roots(&global, &roots)?;
    let admission = InstallationAdmission {
        _roots: locks,
        _gates: gates,
        roots,
        global,
    };
    if require_inactive {
        admission.installation_inactive()?;
    } else {
        admission.installation_reservations_clear()?;
    }
    Ok(admission)
}

pub fn admit_installation(roots: &[PathBuf]) -> Result<InstallationAdmission, String> {
    installation_admission_at(global_updates()?, || Ok(roots.to_vec()), true)
}
/// Select under both global gates, then fence roots before deciding whether to reuse.
/// Replacement additionally requires `installation_inactive` under the retained guard.
#[cfg(not(desktop_pet_legacy))]
pub(crate) fn admit_installation_reuse(
    select_roots: impl FnOnce() -> Result<Vec<PathBuf>, String>,
) -> Result<InstallationAdmission, String> {
    installation_admission_at(global_updates()?, select_roots, false)
}

pub fn installer_admission(
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
) -> Result<InstallationAdmission, String> {
    let roots = std::iter::once(source)
        .chain(target)
        .map(|origin| crate::physical_install_root(origin).map(Path::to_path_buf))
        .collect::<Result<Vec<_>, _>>()?;
    admit_installation(&roots)
}

pub fn installation_locks(
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
) -> Result<InstallationLocks, String> {
    installer_admission(source, target).map(|admission| InstallationLocks {
        _admission: admission,
    })
}

impl InstallationAdmission {
    fn installation_reservations_clear(&self) -> Result<(), String> {
        let compatibility = compatibility_global(&self.global)?;
        for global in std::iter::once(self.global.as_path()).chain(compatibility.as_deref()) {
            reject_reservation_evidence(global, &self.roots, None)?;
        }
        Ok(())
    }

    #[cfg(not(desktop_pet_legacy))]
    pub(crate) fn holds_root(&self, root: &Path) -> bool {
        self.roots.iter().any(|held| held == root)
    }

    /// Caller must release its install lock before rebuilding the sorted root lockset.
    /// Both global gates stay held; every newly fenced root gets the same reservation checks.
    #[cfg(not(desktop_pet_legacy))]
    pub(crate) fn fence_additional_root(&mut self, root: &Path) -> Result<(), String> {
        let mut roots = self.roots.clone();
        roots.push(root.to_path_buf());
        let roots = installation_roots(roots)?;
        self._roots.clear();
        self._roots = lock_roots(&self.global, &roots)?;
        self.roots = roots;
        self.installation_reservations_clear()
    }

    pub fn installation_inactive(&self) -> Result<(), String> {
        for global in
            std::iter::once(self.global.clone()).chain(compatibility_global(&self.global)?)
        {
            reject_reservation_evidence(&global, &self.roots, None)?;
            reject_active_evidence(&global, &self.roots, None, true)?;
        }
        crate::processes::installation_roots_inactive(&self.roots)
    }
    pub fn profile_adoption_allowed(&self, config: &Path, state: &Path) -> Result<(), String> {
        check_dir(config, false)?;
        check_dir(state, false)?;
        reject_profile_reservations(state, false)?;
        for global in
            std::iter::once(self.global.clone()).chain(compatibility_global(&self.global)?)
        {
            reject_reservation_evidence(&global, &self.roots, Some(state))?;
            reject_active_evidence(&global, &self.roots, Some(state), true)?;
        }
        Ok(())
    }
}

pub fn admit_profile_adoption(config: &Path, state: &Path) -> Result<ProfileAdmission, String> {
    admit_profile_adoption_profiles(&[(config.to_path_buf(), state.to_path_buf())])
}

/// One transaction covers both the requested profile and the previous locator.
pub fn admit_profile_adoption_profiles(
    profiles: &[(PathBuf, PathBuf)],
) -> Result<ProfileAdmission, String> {
    if profiles.is_empty() {
        return Err("profile adoption requires a profile".into());
    }
    for (config, state) in profiles {
        check_dir(config, false)?;
        check_dir(state, false)?;
        reject_profile_reservations(state, false)?;
    }
    let global = global_updates()?;
    let gates = lock_gates(&global)?;
    let mut key_locks = Vec::new();
    let mut registry_keys = Vec::new();
    for namespace in std::iter::once(global.clone()).chain(compatibility_global(&global)?) {
        for (_, state) in profiles {
            reject_reservation_evidence(&namespace, &[], Some(state))?;
            reject_active_evidence(&namespace, &[], Some(state), true)?;
        }
        for path in registry_files(&namespace, ".active.json")? {
            let entries: Vec<ActiveProfile> =
                serde_json::from_slice(&owned_private_file(&path, 16384)?)
                    .map_err(|_| "unsupported active profile registry")?;
            if entries
                .iter()
                .any(|entry| profiles.iter().any(|(_, state)| *state == entry.state_dir))
            {
                let key = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| n.strip_suffix(".active.json"))
                    .ok_or("invalid active profile registry")?
                    .to_owned();
                registry_keys.push(key);
            }
        }
    }
    registry_keys.sort_unstable();
    registry_keys.dedup();
    let old = old_global(&global)?;
    for key in registry_keys {
        if let Some(old) = &old {
            key_locks.push(lock_with_mode(
                &old.join(format!("{key}.lock")),
                libc::LOCK_EX | libc::LOCK_NB,
            )?);
        }
        key_locks.push(lock_with_mode(
            &global.join(format!("{key}.lock")),
            libc::LOCK_EX | libc::LOCK_NB,
        )?);
    }
    let admission = InstallationAdmission {
        _roots: key_locks,
        _gates: gates,
        roots: Vec::new(),
        global,
    };
    let mut states = profiles.iter().map(|(_, state)| state).collect::<Vec<_>>();
    states.sort_unstable();
    states.dedup();
    let mut lifecycle = Vec::new();
    for state in states {
        lifecycle.push(
            desktop_pet_contracts::LifecycleLock::try_acquire(state)?
                .ok_or("profile is owned by a live daemon")?,
        );
    }
    for (config, state) in profiles {
        admission.profile_adoption_allowed(config, state)?;
    }
    Ok(ProfileAdmission {
        _lifecycle: lifecycle,
        _admission: admission,
    })
}

/// Read-only admission snapshot. Mutation must retain `admit_profile_adoption`.
pub fn profile_adoption_allowed(config: &Path, state: &Path) -> Result<(), String> {
    admit_profile_adoption(config, state).map(|_| ())
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationProfile {
    pub pid: u32,
    pub instance_id: String,
    pub state_dir: PathBuf,
    pub config_dir: PathBuf,
    pub image: crate::ExecutableIdentity,
    pub stop_generation: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationStartup {
    root: PathBuf,
    image: crate::ExecutableIdentity,
    state_dir: PathBuf,
    stop_generation: u64,
    token: String,
    #[serde(default)]
    committed_instance: Option<ActiveProfile>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationScope {
    version: u32,
    operation_id: String,
    roots: Vec<PathBuf>,
    profiles: Vec<MigrationProfile>,
    readonly_target_profiles: Vec<MigrationProfile>,
    owner_token: String,
    startups: Vec<MigrationStartup>,
}
pub struct LegacyMigrationAdmission {
    _locks: Vec<FileLock>,
    _gates: Vec<FileLock>,
    global: PathBuf,
    scope: MigrationScope,
}
pub struct MigrationReservation {
    global: PathBuf,
    operation_id: String,
    owner_token: String,
}
fn migration_global(global: &Path) -> Result<PathBuf, String> {
    let path = global
        .parent()
        .ok_or("missing global parent")?
        .join(".desktop-pet-updates");
    check_dir(&path, true)?;
    Ok(path)
}
fn migration_path(global: &Path, operation: &str) -> PathBuf {
    global.join(format!("{operation}.migration.json"))
}
fn valid_token(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn migration_scopes(global: &Path) -> Result<Vec<MigrationScope>, String> {
    let namespace = migration_global(global)?;
    let mut entries = Vec::new();
    for path in registry_files(&namespace, ".migration.json")? {
        let scope: MigrationScope = serde_json::from_slice(&owned_private_file(&path, 128 * 1024)?)
            .map_err(|_| "unsupported migration scope; preserve original bytes")?;
        validate_operation_id(&scope.operation_id)?;
        if scope.version != 4
            || scope.roots.is_empty()
            || scope.roots.len() > 16
            || scope.profiles.is_empty()
            || scope.profiles.len() > 64
            || scope.readonly_target_profiles.len() > 64
            || scope.startups.len() > scope.profiles.len()
            || !valid_token(&scope.owner_token)
            || migration_path(&namespace, &scope.operation_id) != path
        {
            return Err("unsupported migration ownership evidence".into());
        }
        if scope.roots.windows(2).any(|roots| roots[0] >= roots[1]) {
            return Err("migration roots must be sorted and unique".into());
        }
        for root in &scope.roots {
            ensure_absolute(root)?;
        }
        for (index, profile) in scope.profiles.iter().enumerate() {
            if scope.profiles[..index]
                .iter()
                .any(|other| other.state_dir == profile.state_dir || other.pid == profile.pid)
            {
                return Err("migration source profiles must be unique".into());
            }
            ensure_absolute(&profile.state_dir)?;
            ensure_absolute(&profile.config_dir)?;
            ensure_absolute(&profile.image.path)?;
            if profile.pid == 0
                || profile.instance_id.is_empty()
                || profile.instance_id.len() > 128
                || !scope
                    .roots
                    .iter()
                    .any(|root| profile.image.path.starts_with(root))
            {
                return Err("invalid migration source identity".into());
            }
        }
        validate_readonly_target_profiles(&scope)?;
        for (index, startup) in scope.startups.iter().enumerate() {
            if scope.startups[..index]
                .iter()
                .any(|other| other.state_dir == startup.state_dir)
            {
                return Err("migration startup profiles must be unique".into());
            }
            ensure_absolute(&startup.image.path)?;
            if !scope.roots.contains(&startup.root)
                || !startup.image.path.starts_with(&startup.root)
                || !valid_token(&startup.token)
                || !scope.profiles.iter().any(|profile| {
                    profile.state_dir == startup.state_dir
                        && profile.stop_generation == startup.stop_generation
                })
            {
                return Err("invalid migration startup authority".into());
            }
        }
        entries.push(scope);
        if entries.len() > 256 {
            return Err("migration scope registry exceeds bound".into());
        }
    }
    Ok(entries)
}
fn reject_migration_scopes(
    global: &Path,
    roots: &[PathBuf],
    state: Option<&Path>,
    owned: Option<&str>,
) -> Result<(), String> {
    for scope in migration_scopes(global)? {
        if owned == Some(scope.operation_id.as_str()) {
            continue;
        }
        if scope.roots.iter().any(|root| roots.contains(root))
            || state.is_some_and(|state| {
                scope
                    .profiles
                    .iter()
                    .chain(&scope.readonly_target_profiles)
                    .any(|profile| profile.state_dir == state)
            })
        {
            return Err("profile or installation is reserved by a legacy migration".into());
        }
    }
    Ok(())
}
fn combined_generation(state: &Path) -> Result<u64, String> {
    check_dir(state, false)?;
    read_generation(&state.join("updates/stop-generation.json"))?
        .checked_add(read_generation(
            &state.join("updates-v4/stop-generation.json"),
        )?)
        .ok_or_else(|| "stop generation overflow".into())
}
fn migration_profile_checks(scope: &MigrationScope) -> Result<(), String> {
    for profile in &scope.profiles {
        check_dir(&profile.config_dir, false)?;
        check_dir(&profile.state_dir, false)?;
        reject_profile_reservations(&profile.state_dir, false)?;
        if combined_generation(&profile.state_dir)? != profile.stop_generation {
            return Err("user stop superseded migration".into());
        }
        if crate::executable_identity(&profile.image.path)? != profile.image {
            return Err("authenticated bridge image changed".into());
        }
    }
    Ok(())
}

fn migration_source_roots(scope: &MigrationScope) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    for profile in &scope.profiles {
        let root = scope
            .roots
            .iter()
            .filter(|root| profile.image.path.starts_with(root))
            .max_by_key(|root| root.components().count())
            .ok_or("source image is outside migration roots")?;
        roots.push(root.clone());
    }
    roots.sort_unstable();
    roots.dedup();
    Ok(roots)
}

fn validate_readonly_target_profiles(scope: &MigrationScope) -> Result<(), String> {
    if scope.readonly_target_profiles.is_empty() {
        return Ok(());
    }
    let source_roots = migration_source_roots(scope)?;
    for (index, profile) in scope.readonly_target_profiles.iter().enumerate() {
        ensure_absolute(&profile.config_dir)?;
        ensure_absolute(&profile.state_dir)?;
        ensure_absolute(&profile.image.path)?;
        if profile.pid == 0
            || profile.instance_id.is_empty()
            || profile.instance_id.len() > 128
            || !scope
                .roots
                .iter()
                .any(|root| profile.image.path.starts_with(root))
            || source_roots
                .iter()
                .any(|root| profile.image.path.starts_with(root))
            || scope
                .profiles
                .iter()
                .chain(&scope.readonly_target_profiles[..index])
                .any(|other| {
                    other.state_dir == profile.state_dir
                        || other.config_dir == profile.config_dir
                        || other.pid == profile.pid
                })
        {
            return Err(
                "read-only target profile must be unique and outside every source root".into(),
            );
        }
    }
    Ok(())
}

fn readonly_target_profile_checks(scope: &MigrationScope) -> Result<(), String> {
    validate_readonly_target_profiles(scope)?;
    for profile in &scope.readonly_target_profiles {
        if live(profile.pid) {
            verify_process_identity(profile.pid, &profile.image)?;
            check_dir(&profile.config_dir, false)?;
            check_dir(&profile.state_dir, false)?;
            reject_profile_reservations(&profile.state_dir, false)?;
        }
        if crate::executable_identity(&profile.image.path)? != profile.image {
            return Err("verified read-only target image changed".into());
        }
    }
    Ok(())
}
pub fn active_profiles_for_roots(roots: &[PathBuf]) -> Result<Vec<ActiveProfile>, String> {
    let global = global_updates()?;
    let _gates = lock_gates(&global)?;
    let keys = root_registry_keys(roots);
    let mut result = Vec::new();
    for namespace in std::iter::once(global.clone()).chain(compatibility_global(&global)?) {
        for key in &keys {
            result.extend(
                active(&namespace, key)?
                    .into_iter()
                    .filter(|profile| live(profile.pid)),
            );
        }
    }
    result.sort_unstable_by(|a, b| a.state_dir.cmp(&b.state_dir).then(a.pid.cmp(&b.pid)));
    result.dedup_by(|a, b| {
        a.state_dir == b.state_dir && a.pid == b.pid && a.instance_id == b.instance_id
    });
    Ok(result)
}
pub fn admit_legacy_migration(
    roots: &[PathBuf],
    profiles: &[MigrationProfile],
    readonly_target_profiles: &[MigrationProfile],
    operation_id: &str,
) -> Result<LegacyMigrationAdmission, String> {
    validate_operation_id(operation_id)?;
    if profiles.is_empty()
        || profiles.len() > 64
        || readonly_target_profiles.len() > 64
        || roots.is_empty()
        || roots.len() > 16
    {
        return Err("migration requires bounded authenticated profiles and roots".into());
    }
    let global = global_updates()?;
    let gates = lock_gates(&global)?;
    let mut roots = roots.to_vec();
    for root in &roots {
        ensure_absolute(root)?;
    }
    roots.sort_unstable();
    roots.dedup();
    reject_migration_scopes(&global, &roots, None, None)?;
    for (index, profile) in profiles.iter().enumerate() {
        if profiles[..index].iter().any(|other| {
            other.state_dir == profile.state_dir
                || other.config_dir == profile.config_dir
                || other.pid == profile.pid
        }) {
            return Err("migration source profiles must be unique".into());
        }
        if profile.pid == 0
            || profile.instance_id.is_empty()
            || profile.instance_id.len() > 128
            || !roots
                .iter()
                .any(|root| profile.image.path.starts_with(root))
        {
            return Err(
                "migration profile has no exact source identity in the reserved roots".into(),
            );
        }
        reject_migration_scopes(&global, &roots, Some(&profile.state_dir), None)?;
    }
    let locks = lock_roots(&global, &roots)?;
    let scope = MigrationScope {
        version: 4,
        operation_id: operation_id.into(),
        roots,
        profiles: profiles.to_vec(),
        readonly_target_profiles: readonly_target_profiles.to_vec(),
        owner_token: String::new(),
        startups: Vec::new(),
    };
    let admission = LegacyMigrationAdmission {
        _locks: locks,
        _gates: gates,
        global,
        scope,
    };
    admission.recheck_profiles()?;
    Ok(admission)
}
fn migration_recheck(global: &Path, scope: &MigrationScope) -> Result<(), String> {
    migration_profile_checks(scope)?;
    readonly_target_profile_checks(scope)?;
    let keys = root_registry_keys(&scope.roots);
    let mut registered = vec![false; scope.profiles.len() + scope.readonly_target_profiles.len()];
    for namespace in std::iter::once(global.to_path_buf()).chain(compatibility_global(global)?) {
        reject_migration_scopes(&namespace, &scope.roots, None, Some(&scope.operation_id))?;
        let index = scopes_path(&namespace);
        if !missing_file(&index) {
            let entries: Vec<serde_json::Value> =
                serde_json::from_slice(&owned_private_file(&index, 1024 * 1024)?)
                    .map_err(|_| "unsupported scope registry")?;
            if entries.len() > 256 {
                return Err("scope registry exceeds bound".into());
            }
            for entry in entries {
                for profile in scope.profiles.iter().chain(&scope.readonly_target_profiles) {
                    if evidence_matches(&entry, &scope.roots, Some(&profile.state_dir))? {
                        return Err("normal updater operation must finish before migration".into());
                    }
                }
            }
        }
        for path in registry_files(&namespace, ".reservation.json")? {
            let entry: serde_json::Value =
                serde_json::from_slice(&owned_private_file(&path, 16384)?)
                    .map_err(|_| "unsupported reservation")?;
            for profile in scope.profiles.iter().chain(&scope.readonly_target_profiles) {
                if evidence_matches(&entry, &scope.roots, Some(&profile.state_dir))? {
                    return Err("normal updater reservation must finish before migration".into());
                }
            }
        }
        for path in registry_files(&namespace, ".active.json")? {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("invalid active registry")?;
            let entries: Vec<ActiveProfile> =
                serde_json::from_slice(&owned_private_file(&path, 16384)?)
                    .map_err(|_| "unsupported active registry")?;
            if entries.len() > 64 {
                return Err("active registry exceeds bound".into());
            }
            for entry in entries.into_iter().filter(|entry| live(entry.pid)) {
                let relevant = keys
                    .iter()
                    .any(|key| name.strip_suffix(".active.json") == Some(key.as_str()))
                    || scope
                        .profiles
                        .iter()
                        .chain(&scope.readonly_target_profiles)
                        .any(|profile| profile.state_dir == entry.state_dir);
                if relevant
                    && !scope
                        .profiles
                        .iter()
                        .chain(&scope.readonly_target_profiles)
                        .any(|profile| {
                            profile.pid == entry.pid
                                && profile.instance_id == entry.instance_id
                                && profile.state_dir == entry.state_dir
                        })
                {
                    return Err("unapproved live profile shares migration installations".into());
                }
                for (index, profile) in scope
                    .profiles
                    .iter()
                    .chain(&scope.readonly_target_profiles)
                    .enumerate()
                {
                    if profile.pid == entry.pid
                        && profile.instance_id == entry.instance_id
                        && profile.state_dir == entry.state_dir
                    {
                        registered[index] = true;
                    }
                }
            }
        }
    }
    if scope
        .profiles
        .iter()
        .chain(&scope.readonly_target_profiles)
        .enumerate()
        .any(|(index, profile)| live(profile.pid) && !registered[index])
    {
        return Err("authenticated source is missing its exact active registration".into());
    }
    let authorized = scope
        .profiles
        .iter()
        .chain(&scope.readonly_target_profiles)
        .filter(|profile| live(profile.pid))
        .map(|profile| (profile.pid, profile.image.clone()))
        .collect::<Vec<_>>();
    crate::processes::installation_roots_admitted(&scope.roots, &authorized)
}
impl LegacyMigrationAdmission {
    pub fn recheck_profiles(&self) -> Result<(), String> {
        migration_recheck(&self.global, &self.scope)
    }
    /// Lease an initial operation only under authenticated root/profile admission.
    /// Ordinary updater leasing correctly rejects the still-live legacy owner.
    pub fn lock_operation(&self) -> Result<OperationLease, String> {
        let state = &self.scope.profiles[0].state_dir;
        let file = lock_with_mode(
            &private_updates(state)?.join(format!("{}.operation.lock", self.scope.operation_id)),
            libc::LOCK_EX | libc::LOCK_NB,
        )?;
        Ok(OperationLease { _file: file })
    }
    pub fn profiles_stopped(&self) -> Result<(), String> {
        self.recheck_profiles()?;
        if self.scope.profiles.iter().any(|profile| live(profile.pid)) {
            return Err("authenticated bridge profiles have not all stopped".into());
        }
        crate::processes::installation_roots_inactive(&migration_source_roots(&self.scope)?)
    }
    pub fn stop_requested(&self) -> Result<bool, String> {
        for profile in &self.scope.profiles {
            if combined_generation(&profile.state_dir)? != profile.stop_generation {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn reserve(mut self, token: &str) -> Result<MigrationReservation, String> {
        if !valid_token(token) {
            return Err("migration plan token must be 256-bit lowercase hex".into());
        }
        self.recheck_profiles()?;
        self.scope.owner_token = token.into();
        write_private_new(
            &migration_path(&migration_global(&self.global)?, &self.scope.operation_id),
            &serde_json::to_vec(&self.scope).map_err(|e| e.to_string())?,
            0o600,
        )?;
        Ok(MigrationReservation {
            global: self.global.clone(),
            operation_id: self.scope.operation_id.clone(),
            owner_token: token.into(),
        })
    }
}
/// Only the authenticated bridge instance may accept bounded migration Prepare/Stop.
pub fn verify_legacy_migration_control(
    state: &Path,
    pid: u32,
    instance: &str,
    operation: &str,
    token: &str,
) -> Result<(), String> {
    validate_operation_id(operation)?;
    let global = global_updates()?;
    let _gates = lock_gates(&global)?;
    let scope = migration_scopes(&global)?
        .into_iter()
        .find(|scope| scope.operation_id == operation && scope.owner_token == token)
        .ok_or("migration request has no owned durable scope")?;
    if !scope.profiles.iter().any(|profile| {
        profile.state_dir == state
            && profile.pid == pid
            && profile.instance_id == instance
            && live(pid)
    }) {
        return Err("migration request does not authorize this bridge instance".into());
    }
    migration_recheck(&global, &scope)
}

/// Commit authorization is bound to the actual registered target, not a locator.
pub fn verify_migration_target(
    state: &Path,
    pid: u32,
    instance: &str,
    operation: &str,
    token: &str,
) -> Result<(), String> {
    validate_operation_id(operation)?;
    let global = global_updates()?;
    let _gates = lock_gates(&global)?;
    verify_migration_target_locked(&global, state, pid, instance, operation, token).map(|_| ())
}

fn verify_migration_target_locked(
    global: &Path,
    state: &Path,
    pid: u32,
    instance: &str,
    operation: &str,
    token: &str,
) -> Result<MigrationScope, String> {
    let scope = migration_scopes(global)?
        .into_iter()
        .find(|scope| scope.operation_id == operation && scope.owner_token == token)
        .ok_or("migration request has no owned durable scope")?;
    let startup = scope
        .startups
        .iter()
        .find(|startup| startup.state_dir == state)
        .ok_or("migration has not authorized this target profile")?;
    if combined_generation(state)? != startup.stop_generation {
        return Err("user stop superseded migration commit".into());
    }
    let mut registered = false;
    for key in root_registry_keys(std::slice::from_ref(&startup.root)) {
        registered |= active(global, &key)?.iter().any(|entry| {
            entry.state_dir == state
                && entry.pid == pid
                && entry.instance_id == instance
                && live(pid)
        });
    }
    if !registered {
        return Err("target instance has no exact active registration".into());
    }
    migration_start_registry_checks(global, &scope, Some((pid, startup.image.clone())))?;
    Ok(scope)
}

/// Atomically fence preference persistence and target mutation-gate release
/// against user Stop and ordinary profile/installation admission.
/// The caller holds its lifecycle writer; the callback may acquire the shared
/// profile writer and then the domain lock, but must not reenter updater gates.
pub fn with_migration_target_commit<T>(
    state: &Path,
    pid: u32,
    instance: &str,
    operation_id: &str,
    token: &str,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    validate_operation_id(operation_id)?;
    let global = global_updates()?;
    let _gates = lock_gates(&global)?;
    let mut scope =
        verify_migration_target_locked(&global, state, pid, instance, operation_id, token)?;
    let _roots = lock_roots(&global, &scope.roots)?;
    let result = operation()?;
    let startup = scope
        .startups
        .iter_mut()
        .find(|startup| startup.state_dir == state)
        .ok_or("committed migration startup disappeared")?;
    startup.committed_instance = Some(ActiveProfile {
        state_dir: state.into(),
        pid,
        instance_id: instance.into(),
    });
    write_private_bytes(
        &migration_path(&migration_global(&global)?, operation_id),
        &serde_json::to_vec(&scope).map_err(|e| e.to_string())?,
        0o600,
    )?;
    Ok(result)
}

#[cfg(target_os = "macos")]
fn verify_process_identity(pid: u32, image: &crate::ExecutableIdentity) -> Result<(), String> {
    if !live(pid) {
        return Err("migration instance is no longer live".into());
    }
    unsafe extern "C" {
        fn proc_pidpath(pid: i32, buffer: *mut std::ffi::c_void, buffersize: u32) -> i32;
    }
    let mut buffer = [0u8; 4096];
    let length =
        unsafe { proc_pidpath(pid as i32, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if length <= 0 {
        return Err("cannot authenticate migration process image".into());
    }
    use std::os::unix::ffi::OsStrExt;
    let end = buffer
        .iter()
        .position(|byte| *byte == 0)
        .ok_or("unterminated migration process image")?;
    let path = Path::new(std::ffi::OsStr::from_bytes(&buffer[..end]));
    if path != image.path || crate::executable_identity(path)? != *image {
        return Err("migration process image does not match its authenticated identity".into());
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn verify_process_identity(_: u32, _: &crate::ExecutableIdentity) -> Result<(), String> {
    Err("migration process proof requires macOS".into())
}

fn cleanup_instance_registered(
    global: &Path,
    state: &Path,
    pid: u32,
    instance: &str,
) -> Result<bool, String> {
    for namespace in std::iter::once(global.to_path_buf()).chain(compatibility_global(global)?) {
        for path in registry_files(&namespace, ".active.json")? {
            let entries: Vec<ActiveProfile> =
                serde_json::from_slice(&owned_private_file(&path, 16384)?)
                    .map_err(|_| "unsupported active profile registry")?;
            if entries.len() > 64 {
                return Err("active profile registry exceeds bound".into());
            }
            if entries.iter().any(|entry| {
                entry.state_dir == state && entry.pid == pid && entry.instance_id == instance
            }) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Cleanup only: a user Stop cannot strand an authenticated prepared bridge.
/// This grants neither startup nor installation/migration commit permission.
pub fn verify_legacy_migration_abort(
    state: &Path,
    pid: u32,
    instance: &str,
    operation: &str,
    token: &str,
) -> Result<(), String> {
    validate_operation_id(operation)?;
    let global = global_updates()?;
    let _gates = lock_gates(&global)?;
    let scope = migration_scopes(&global)?
        .into_iter()
        .find(|scope| scope.operation_id == operation && scope.owner_token == token)
        .ok_or("migration abort has no owned durable scope")?;
    let profile = scope
        .profiles
        .iter()
        .find(|profile| {
            profile.state_dir == state && profile.pid == pid && profile.instance_id == instance
        })
        .ok_or("migration abort does not authorize this bridge instance")?;
    if !cleanup_instance_registered(&global, state, pid, instance)? {
        return Err("migration abort instance has no exact active registration".into());
    }
    verify_process_identity(pid, &profile.image)
}

/// Shutdown only: honor user Stop without reopening launch or commit authority.
pub fn verify_migration_target_stop(
    state: &Path,
    pid: u32,
    instance: &str,
    operation: &str,
    token: &str,
) -> Result<(), String> {
    validate_operation_id(operation)?;
    let global = global_updates()?;
    let _gates = lock_gates(&global)?;
    let scope = migration_scopes(&global)?
        .into_iter()
        .find(|scope| scope.operation_id == operation && scope.owner_token == token)
        .ok_or("migration stop has no owned durable scope")?;
    let startup = scope
        .startups
        .iter()
        .find(|startup| startup.state_dir == state)
        .ok_or("migration has not issued target startup authority for this profile")?;
    if !cleanup_instance_registered(&global, state, pid, instance)? {
        return Err("migration stop target has no exact active registration".into());
    }
    verify_process_identity(pid, &startup.image)
}

pub fn lock_migration_operation(
    state: &Path,
    operation: &str,
    token: &str,
) -> Result<OperationLease, String> {
    validate_operation_id(operation)?;
    let global = global_updates()?;
    let _gates = lock_gates(&global)?;
    let scope = migration_scopes(&global)?
        .into_iter()
        .find(|scope| {
            scope.operation_id == operation
                && scope.owner_token == token
                && scope
                    .profiles
                    .iter()
                    .any(|profile| profile.state_dir == state)
        })
        .ok_or("migration operation lease has no owned profile scope")?;
    reject_migration_scopes(&global, &scope.roots, Some(state), Some(operation))?;
    let file = lock_with_mode(
        &private_updates(state)?.join(format!("{operation}.operation.lock")),
        libc::LOCK_EX | libc::LOCK_NB,
    )?;
    Ok(OperationLease { _file: file })
}

pub fn load_migration_reservation(
    operation_id: &str,
    token: &str,
) -> Result<MigrationReservation, String> {
    validate_operation_id(operation_id)?;
    if !valid_token(token) {
        return Err("invalid migration ownership token".into());
    }
    let global = global_updates()?;
    let _gates = lock_gates(&global)?;
    let reservation = MigrationReservation {
        global: global.clone(),
        operation_id: operation_id.into(),
        owner_token: token.into(),
    };
    reservation.scope()?;
    Ok(reservation)
}
fn distinct_brew_keg(origin: &InstallOrigin, previous: &Path, target: &Path) -> bool {
    let InstallOrigin::Homebrew {
        cellar_formula_root,
        ..
    } = origin
    else {
        return false;
    };
    let (Ok(previous), Ok(target)) = (
        previous.strip_prefix(cellar_formula_root),
        target.strip_prefix(cellar_formula_root),
    ) else {
        return false;
    };
    matches!((previous.components().next(), target.components().next()),
        (Some(std::path::Component::Normal(previous)), Some(std::path::Component::Normal(target))) if previous != target)
}
impl MigrationReservation {
    /// Serialize pointer CAS/adoption/rollback with ordinary profile adoption.
    /// The callback must not reenter protocol gates or perform lifecycle/control I/O.
    pub fn with_pointer_mutation<T>(
        &self,
        operation: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _gates = lock_gates(&self.global)?;
        let scope = self.scope()?;
        let _roots = lock_roots(&self.global, &scope.roots)?;
        readonly_target_profile_checks(&scope)?;
        for namespace in
            std::iter::once(self.global.clone()).chain(compatibility_global(&self.global)?)
        {
            for profile in &scope.profiles {
                reject_migration_scopes(
                    &namespace,
                    &scope.roots,
                    Some(&profile.state_dir),
                    Some(&self.operation_id),
                )?;
                reject_normal_reservation_evidence(
                    &namespace,
                    &scope.roots,
                    Some(&profile.state_dir),
                )?;
            }
        }
        operation()
    }

    fn scope(&self) -> Result<MigrationScope, String> {
        migration_scopes(&self.global)?
            .into_iter()
            .find(|scope| {
                scope.operation_id == self.operation_id && scope.owner_token == self.owner_token
            })
            .ok_or_else(|| "migration ownership changed".into())
    }
    pub fn recheck_profiles(&self) -> Result<(), String> {
        let _gates = lock_gates(&self.global)?;
        migration_recheck(&self.global, &self.scope()?)
    }
    pub fn profiles_stopped(&self) -> Result<(), String> {
        let _gates = lock_gates(&self.global)?;
        let scope = self.scope()?;
        migration_recheck(&self.global, &scope)?;
        if scope.profiles.iter().any(|profile| live(profile.pid)) {
            return Err("bridge still running".into());
        }
        crate::processes::installation_roots_inactive(&migration_source_roots(&scope)?)
    }
    pub fn recheck_inactive_source(&self) -> Result<(), String> {
        let _gates = lock_gates(&self.global)?;
        let scope = self.scope()?;
        if scope.profiles.iter().any(|profile| live(profile.pid)) {
            return Err("legacy source daemon still running".into());
        }
        let roots = migration_source_roots(&scope)?;
        crate::processes::installation_roots_inactive(&roots)
    }
    /// Any captured read-only target user forbids target image replacement.
    pub fn target_reuse_present(&self) -> Result<bool, String> {
        let _gates = lock_gates(&self.global)?;
        Ok(!self.scope()?.readonly_target_profiles.is_empty())
    }

    pub fn stop_requested(&self) -> Result<bool, String> {
        let _gates = lock_gates(&self.global)?;
        for profile in self.scope()?.profiles {
            if combined_generation(&profile.state_dir)? != profile.stop_generation {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn authorize_start(
        &self,
        origin: &InstallOrigin,
        image: &crate::ExecutableIdentity,
        state: &Path,
    ) -> Result<(u64, String), String> {
        let _gates = lock_gates(&self.global)?;
        let mut scope = self.scope()?;
        migration_profile_checks(&scope)?;
        readonly_target_profile_checks(&scope)?;
        let root = crate::physical_install_root(origin)?.to_path_buf();
        if !scope.roots.contains(&root)
            || !image.path.starts_with(&root)
            || crate::executable_identity(&image.path)? != *image
            || scope.profiles.iter().any(|profile| live(profile.pid))
        {
            return Err("migration start requires exact verified image and stopped sources".into());
        }
        if let Some(target) = scope
            .readonly_target_profiles
            .iter()
            .find(|profile| profile.image.path.starts_with(&root))
        {
            if target.image != *image && !distinct_brew_keg(origin, &target.image.path, &image.path)
            {
                return Err("migration may only reuse the pinned live target image".into());
            }
        }
        migration_start_registry_checks(&self.global, &scope, None)?;
        let expected = scope
            .profiles
            .iter()
            .find(|profile| profile.state_dir == state)
            .ok_or("startup profile is outside migration")?
            .stop_generation;
        if let Some(existing) = scope.startups.iter().find(|startup| {
            startup.state_dir == state && startup.image == *image && startup.root == root
        }) {
            return Ok((expected, existing.token.clone()));
        }
        // Changing target to rollback is legal only after the previous image stops.
        let lifecycle = desktop_pet_contracts::LifecycleLock::try_acquire(state)?
            .ok_or("startup profile is still owned by a live daemon")?;
        let token = new_token()?;
        scope.startups.retain(|startup| startup.state_dir != state);
        scope.startups.push(MigrationStartup {
            root,
            image: image.clone(),
            state_dir: state.into(),
            stop_generation: expected,
            token: token.clone(),
            committed_instance: None,
        });
        write_private_bytes(
            &migration_path(&migration_global(&self.global)?, &self.operation_id),
            &serde_json::to_vec(&scope).map_err(|e| e.to_string())?,
            0o600,
        )?;
        drop(lifecycle);
        Ok((expected, token))
    }
    pub fn release(&self) -> Result<(), String> {
        let _gates = lock_gates(&self.global)?;
        self.scope()?;
        let path = migration_path(&migration_global(&self.global)?, &self.operation_id);
        fs::remove_file(&path).map_err(|e| e.to_string())?;
        sync_parent(&path)
    }
}
/// This permits fast reuse of an already authenticated target daemon only.
/// It cannot authorize a spawn or refresh the captured instance registration.
fn migration_readonly_reuse_for(
    global: &Path,
    origin: &InstallOrigin,
    state: &Path,
    expected: Option<u64>,
    token: Option<&str>,
) -> Result<Option<MigrationProfile>, String> {
    if expected.is_some() || token.is_some() {
        return Ok(None);
    }
    let root = match origin {
        InstallOrigin::Unknown { .. } => return Ok(None),
        _ => crate::physical_install_root(origin)?,
    };
    let mut admitted = None;
    for scope in migration_scopes(global)? {
        let Some(profile) = scope
            .readonly_target_profiles
            .iter()
            .find(|profile| profile.state_dir == state && profile.image.path.starts_with(root))
        else {
            continue;
        };
        if admitted.is_some() {
            return Err("overlapping read-only target migration scopes".into());
        }
        readonly_target_profile_checks(&scope)?;
        if !cleanup_instance_registered(global, state, profile.pid, &profile.instance_id)? {
            return Err("read-only target reuse has lost its captured active instance".into());
        }
        verify_process_identity(profile.pid, &profile.image)?;
        admitted = Some(profile.clone());
    }
    Ok(admitted)
}

/// A committed target may serve ordinary SDK exchanges while its owner remains
/// reserved. This is live-instance reuse only: it grants no spawn, registration
/// replacement, installation mutation or reservation release authority.
fn migration_committed_reuse_for(
    global: &Path,
    origin: &InstallOrigin,
    state: &Path,
    expected: Option<u64>,
    token: Option<&str>,
) -> Result<Option<ActiveProfile>, String> {
    if expected.is_some() || token.is_some() {
        return Ok(None);
    }
    let root = match origin {
        InstallOrigin::Unknown { .. } => return Ok(None),
        _ => crate::physical_install_root(origin)?,
    };
    let mut admitted = None;
    for scope in migration_scopes(global)? {
        let Some(startup) = scope
            .startups
            .iter()
            .find(|startup| startup.state_dir == state && startup.root == root)
        else {
            continue;
        };
        let Some(instance) = startup.committed_instance.as_ref() else {
            continue;
        };
        if admitted.is_some() {
            return Err("overlapping committed migration scopes".into());
        }
        if combined_generation(state)? != startup.stop_generation
            || instance.state_dir != state
            || !cleanup_instance_registered(global, state, instance.pid, &instance.instance_id)?
        {
            return Err(
                "committed migration target changed or user Stop superseded SDK reuse".into(),
            );
        }
        verify_process_identity(instance.pid, &startup.image)?;
        migration_start_registry_checks(global, &scope, None)?;
        admitted = Some(instance.clone());
    }
    Ok(admitted)
}

fn migration_start_for(
    global: &Path,
    origin: &InstallOrigin,
    state: &Path,
    expected: Option<u64>,
    token: Option<&str>,
) -> Result<Option<(MigrationScope, MigrationStartup)>, String> {
    let root = match origin {
        InstallOrigin::Unknown { .. } => None,
        _ => Some(crate::physical_install_root(origin)?),
    };
    let mut admitted = None;
    for scope in migration_scopes(global)? {
        if !scope
            .profiles
            .iter()
            .any(|profile| profile.state_dir == state)
            && root.is_none_or(|root| !scope.roots.iter().any(|stored| stored == root))
        {
            continue;
        }
        if admitted.is_some() {
            return Err("overlapping migration scopes".into());
        }
        let startup = scope
            .startups
            .iter()
            .find(|startup| {
                startup.state_dir == state
                    && root == Some(startup.root.as_path())
                    && expected == Some(startup.stop_generation)
                    && token == Some(startup.token.as_str())
            })
            .cloned()
            .ok_or("installation is reserved by migration; startup is not authorized")?;
        if combined_generation(state)? != startup.stop_generation
            || crate::executable_identity(&startup.image.path)? != startup.image
        {
            return Err("migration startup image or stop generation changed".into());
        }
        admitted = Some((scope, startup));
    }
    Ok(admitted)
}

fn reject_migration_for_origins(
    global: &Path,
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    state: &Path,
) -> Result<(), String> {
    let roots = std::iter::once(source)
        .chain(target)
        .map(|origin| crate::physical_install_root(origin).map(Path::to_path_buf))
        .collect::<Result<Vec<_>, _>>()?;
    reject_migration_scopes(global, &roots, Some(state), None)
}

fn migration_start_registry_checks(
    global: &Path,
    scope: &MigrationScope,
    registering: Option<(u32, crate::ExecutableIdentity)>,
) -> Result<(), String> {
    let mut authorized = Vec::new();
    if let Some(registering) = registering {
        authorized.push(registering);
    }
    readonly_target_profile_checks(scope)?;
    for profile in &scope.profiles {
        reject_profile_reservations(&profile.state_dir, false)?;
        if combined_generation(&profile.state_dir)? != profile.stop_generation {
            return Err("user stop superseded migration startup".into());
        }
    }
    let keys = root_registry_keys(&scope.roots);
    for namespace in std::iter::once(global.to_path_buf()).chain(compatibility_global(global)?) {
        for profile in scope.profiles.iter().chain(&scope.readonly_target_profiles) {
            reject_normal_reservation_evidence(&namespace, &scope.roots, Some(&profile.state_dir))?;
        }
        for path in registry_files(&namespace, ".active.json")? {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("invalid registry path")?;
            let entries: Vec<ActiveProfile> =
                serde_json::from_slice(&owned_private_file(&path, 16384)?)
                    .map_err(|_| "unsupported active registry")?;
            if entries.len() > 64 {
                return Err("active profile registry exceeds bound".into());
            }
            for entry in entries.into_iter().filter(|entry| live(entry.pid)) {
                if !keys
                    .iter()
                    .any(|key| name.strip_suffix(".active.json") == Some(key.as_str()))
                    && !scope
                        .profiles
                        .iter()
                        .chain(&scope.readonly_target_profiles)
                        .any(|profile| profile.state_dir == entry.state_dir)
                {
                    continue;
                }
                if let Some(profile) = scope.readonly_target_profiles.iter().find(|profile| {
                    profile.state_dir == entry.state_dir
                        && profile.pid == entry.pid
                        && profile.instance_id == entry.instance_id
                }) {
                    authorized.push((entry.pid, profile.image.clone()));
                } else {
                    let startup = scope
                        .startups
                        .iter()
                        .find(|startup| startup.state_dir == entry.state_dir)
                        .ok_or(
                            "unapproved live source or new target user during migration startup",
                        )?;
                    authorized.push((entry.pid, startup.image.clone()));
                }
            }
        }
    }
    crate::processes::installation_roots_admitted(&scope.roots, &authorized)
}

struct FileLock {
    file: File,
    owner_pid: libc::pid_t,
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Closing alone leaves flock held by any concurrent fork until exec.
        // Only the acquiring process may release this shared open-file lock.
        if unsafe { libc::getpid() } == self.owner_pid {
            unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}

fn lock(path: &Path) -> Result<FileLock, String> {
    lock_with_mode(path, libc::LOCK_EX)
}
fn lock_with_mode(path: &Path, mode: libc::c_int) -> Result<FileLock, String> {
    try_lock_with_mode(path, mode)?
        .ok_or_else(|| "installation busy: operation lease is held".into())
}
fn try_lock_with_mode(path: &Path, mode: libc::c_int) -> Result<Option<FileLock>, String> {
    check_dir(path.parent().ok_or("lock without parent")?, false)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| e.to_string())?;
    let m = file.metadata().map_err(|e| e.to_string())?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || m.permissions().mode() & 0o077 != 0
    {
        return Err("unsafe update lock".into());
    }
    if unsafe { libc::flock(file.as_raw_fd(), mode) } != 0 {
        let error = std::io::Error::last_os_error();
        if mode & libc::LOCK_NB != 0 && error.kind() == std::io::ErrorKind::WouldBlock {
            return Ok(None);
        }
        return Err(format!("installation lock failed: {error}"));
    }
    Ok(Some(FileLock {
        file,
        owner_pid: unsafe { libc::getpid() },
    }))
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reservation {
    version: u32,
    operation_id: String,
    source: InstallOrigin,
    target: Option<InstallOrigin>,
    keys: Vec<String>,
    state_dir: PathBuf,
    stop_generation: u64,
    control_token: String,
    startup_origin: Option<InstallOrigin>,
    startup_token: Option<String>,
}

fn selectable_origin(
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    origin: &InstallOrigin,
) -> bool {
    if origin == source || target == Some(origin) {
        return true;
    }
    if target.is_some() {
        return false;
    }
    match (source, origin) {
        #[cfg(desktop_pet_legacy)]
        (
            InstallOrigin::Herdr {
                host,
                host_config_dir,
                checkout_root,
                plugin_root,
                source,
                requested_ref,
                enabled,
                ..
            },
            InstallOrigin::Herdr {
                host: actual_host,
                host_config_dir: actual_config,
                checkout_root: actual_checkout,
                plugin_root: actual_plugin,
                source: actual_source,
                requested_ref: actual_ref,
                resolved_commit: Some(commit),
                enabled: actual_enabled,
            },
        ) => {
            host == actual_host
                && host_config_dir == actual_config
                && checkout_root == actual_checkout
                && plugin_root == actual_plugin
                && source == actual_source
                && requested_ref == actual_ref
                && enabled == actual_enabled
                && commit.len() == 40
                && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        }
        _ => false,
    }
}

fn valid_reservation(marker: &Reservation) -> Result<(), String> {
    if marker.version != UPDATER_PROTOCOL
        || validate_operation_id(&marker.operation_id).is_err()
        || marker.keys != installation_keys(&marker.source, marker.target.as_ref())?
        || marker.startup_origin.is_some() != marker.startup_token.is_some()
        || marker.startup_origin.as_ref().is_some_and(|origin| {
            !selectable_origin(&marker.source, marker.target.as_ref(), origin)
        })
    {
        return Err("unsupported reservation; preserve original marker".into());
    }
    Ok(())
}
fn reservation(bytes: &[u8]) -> Result<Reservation, String> {
    let marker: Reservation = serde_json::from_slice(bytes)
        .map_err(|_| "unsupported reservation; preserve original marker")?;
    valid_reservation(&marker)?;
    Ok(marker)
}

fn installation_keys(
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
) -> Result<Vec<String>, String> {
    let mut keys = vec![installation_key(source)?];
    if let Some(target) = target {
        keys.push(installation_key(target)?);
    }
    keys.sort_unstable();
    keys.dedup();
    Ok(keys)
}

// The atomic index is written before any individual marker and removed last.
// Thus a crash between writes never leaves an unguarded root, even if the
// profile or one of the installation markers has not yet been created.
fn scopes_path(global: &Path) -> PathBuf {
    global.join("installation-scopes.json")
}
fn scopes(global: &Path) -> Result<Vec<Reservation>, String> {
    let path = scopes_path(global);
    if missing_file(&path) {
        return Ok(Vec::new());
    }
    let bytes = owned_private_file(&path, 1024 * 1024)?;
    let entries: Vec<Reservation> = serde_json::from_slice(&bytes)
        .map_err(|_| "unsupported installation scopes; preserve original bytes")?;
    if entries.len() > 256 {
        return Err("installation scope registry exceeds bound".into());
    }
    for entry in &entries {
        valid_reservation(entry)?;
    }
    Ok(entries)
}
fn write_scopes(global: &Path, entries: &[Reservation]) -> Result<(), String> {
    write_private_bytes(
        &scopes_path(global),
        &serde_json::to_vec(entries).map_err(|e| e.to_string())?,
        0o600,
    )
}
fn marker_path(global: &Path, key: &str) -> PathBuf {
    global.join(format!("{key}.reservation.json"))
}
fn marker_at(path: &Path) -> Result<Option<Reservation>, String> {
    if missing_file(path) {
        Ok(None)
    } else {
        reservation(&owned_private_file(path, 16384)?).map(Some)
    }
}
fn markers_match(global: &Path, data: &Reservation) -> Result<bool, String> {
    if marker_at(&profile_marker(&data.state_dir)?)?.as_ref() != Some(data) {
        return Ok(false);
    }
    for key in &data.keys {
        if marker_at(&marker_path(global, key))?.as_ref() != Some(data) {
            return Ok(false);
        }
    }
    Ok(true)
}
fn write_markers(global: &Path, data: &Reservation) -> Result<(), String> {
    let bytes = serde_json::to_vec(data).map_err(|e| e.to_string())?;
    write_private_bytes(&profile_marker(&data.state_dir)?, &bytes, 0o600)?;
    for key in &data.keys {
        write_private_bytes(&marker_path(global, key), &bytes, 0o600)?;
    }
    Ok(())
}
fn scope_for<'a>(
    entries: &'a [Reservation],
    op: &str,
    state_dir: &Path,
) -> Result<&'a Reservation, String> {
    entries
        .iter()
        .find(|entry| entry.operation_id == op && entry.state_dir == state_dir)
        .ok_or_else(|| "operation has no durable installation scope".into())
}
fn missing_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
}
fn generation_file(state_dir: &Path) -> Result<PathBuf, String> {
    Ok(private_updates(state_dir)?.join("stop-generation.json"))
}
fn read_generation(path: &Path) -> Result<u64, String> {
    if let Some(parent) = path.parent() {
        if !missing_file(parent) {
            check_dir(parent, false)?;
        }
    }
    if missing_file(path) {
        return Ok(0);
    }
    serde_json::from_slice(&owned_private_file(path, 32)?).map_err(|e| e.to_string())
}
fn generation(state_dir: &Path) -> Result<u64, String> {
    check_dir(state_dir, false)?;
    let current = read_generation(&state_dir.join(UPDATES_DIR).join("stop-generation.json"))?;
    #[cfg(desktop_pet_legacy)]
    {
        Ok(current)
    }
    #[cfg(not(desktop_pet_legacy))]
    {
        let old = read_generation(&state_dir.join("updates/stop-generation.json"))?;
        current
            .checked_add(old)
            .ok_or_else(|| "stop generation overflow".into())
    }
}
fn profile_marker(state_dir: &Path) -> Result<PathBuf, String> {
    Ok(private_updates(state_dir)?.join("profile.reservation.json"))
}
/// Consult during both fast live reuse and spawn, while holding startup lease.
/// Only a helper-issued token can start the reserved install; user stop generation wins.
pub fn start_allowed(
    origin: &InstallOrigin,
    state_dir: &Path,
    expected_generation: Option<u64>,
    updater_token: Option<&str>,
) -> Result<(), String> {
    start_allowed_at(
        &global_updates()?,
        origin,
        state_dir,
        expected_generation,
        updater_token,
    )
}
fn start_allowed_at(
    global: &Path,
    origin: &InstallOrigin,
    state_dir: &Path,
    expected_generation: Option<u64>,
    updater_token: Option<&str>,
) -> Result<(), String> {
    let _gate = lock_gates(global)?;
    start_allowed_locked(
        global,
        origin,
        state_dir,
        expected_generation,
        updater_token,
    )
}
fn start_allowed_locked(
    global: &Path,
    origin: &InstallOrigin,
    state_dir: &Path,
    expected_generation: Option<u64>,
    updater_token: Option<&str>,
) -> Result<(), String> {
    if migration_committed_reuse_for(
        global,
        origin,
        state_dir,
        expected_generation,
        updater_token,
    )?
    .is_some()
    {
        return Ok(());
    }
    if migration_readonly_reuse_for(
        global,
        origin,
        state_dir,
        expected_generation,
        updater_token,
    )?
    .is_some()
    {
        return Ok(());
    }
    if let Some((scope, startup)) = migration_start_for(
        global,
        origin,
        state_dir,
        expected_generation,
        updater_token,
    )? {
        let current = std::env::current_exe().map_err(|e| e.to_string())?;
        let registering = if crate::executable_identity(&current)? == startup.image {
            Some((std::process::id(), startup.image))
        } else {
            None
        };
        migration_start_registry_checks(global, &scope, registering)?;
        return Ok(());
    }
    reject_old_evidence(global, Some(origin), None, Some(state_dir), true)?;
    let current = generation(state_dir)?;
    if expected_generation.is_some_and(|g| g != current) {
        return Err("user requested stop after updater prepared startup".into());
    }
    let profile_path = profile_marker(state_dir)?;
    let key = match origin {
        InstallOrigin::Unknown { .. } => None,
        _ => {
            reject_legacy_marker(global, origin)?;
            Some(installation_key(origin)?)
        }
    };
    let entries = scopes(global)?;
    let relevant = entries.iter().filter(|entry| {
        entry.state_dir == state_dir || key.as_ref().is_some_and(|key| entry.keys.contains(key))
    });
    let mut admitted = false;
    for entry in relevant {
        bootstrap_start_fence(entry)?;
        if entry.state_dir != state_dir
            || entry.startup_origin.as_ref() != Some(origin)
            || expected_generation != Some(entry.stop_generation)
            || updater_token.is_none()
            || updater_token != entry.startup_token.as_deref()
            || !markers_match(global, entry)?
        {
            return Err("installation or profile reserved by updater".into());
        }
        admitted = true;
    }
    if let Some(marker) = marker_at(&profile_path)? {
        if !admitted || marker.state_dir != state_dir {
            return Err("profile reservation lacks matching durable scope".into());
        }
    }
    if let Some(key) = key {
        if marker_at(&marker_path(global, &key))?.is_some() && !admitted {
            return Err("installation reservation lacks matching durable scope".into());
        }
    }
    if updater_token.is_some() && !admitted {
        return Err("updater start token has no active full-scope reservation".into());
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveProfile {
    pub state_dir: PathBuf,
    pub pid: u32,
    pub instance_id: String,
}
fn active_path(global: &Path, key: &str) -> PathBuf {
    global.join(format!("{key}.active.json"))
}
fn active(global: &Path, key: &str) -> Result<Vec<ActiveProfile>, String> {
    let path = active_path(global, key);
    if missing_file(&path) {
        return Ok(Vec::new());
    }
    let entries: Vec<ActiveProfile> =
        serde_json::from_slice(&owned_private_file(&path, 16384)?).map_err(|e| e.to_string())?;
    if entries.len() > 64 {
        return Err("active profile registry exceeds bound".into());
    }
    Ok(entries)
}
fn active_for_origin(global: &Path, origin: &InstallOrigin) -> Result<Vec<ActiveProfile>, String> {
    let key = installation_key(origin)?;
    let mut entries = active(global, &key)?;
    if let Some(old) = legacy_key(origin).filter(|old| old != &key) {
        entries.extend(active(global, &old)?);
    }
    if let Some(old_global) = compatibility_global(global)? {
        entries.extend(active(&old_global, &key)?);
        if let Some(old) = legacy_key(origin).filter(|old| old != &key) {
            entries.extend(active(&old_global, &old)?);
        }
    }
    Ok(entries)
}
fn live(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    (unsafe { libc::kill(pid as i32, 0) }) == 0
        || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}
/// Daemon calls this after startup admission, and removes its exact instance on finalization.
pub fn register_active(
    origin: &InstallOrigin,
    state_dir: &Path,
    pid: u32,
    instance_id: &str,
    expected_generation: Option<u64>,
    updater_token: Option<&str>,
) -> Result<(), String> {
    register_active_at(
        &global_updates()?,
        origin,
        state_dir,
        pid,
        instance_id,
        expected_generation,
        updater_token,
    )
}
fn register_active_at(
    global: &Path,
    origin: &InstallOrigin,
    state_dir: &Path,
    pid: u32,
    instance_id: &str,
    expected_generation: Option<u64>,
    updater_token: Option<&str>,
) -> Result<(), String> {
    if pid == 0 || instance_id.is_empty() || instance_id.len() > 128 {
        return Err("invalid active daemon identity".into());
    }
    let _gate = lock_gates(global)?;
    start_allowed_locked(
        global,
        origin,
        state_dir,
        expected_generation,
        updater_token,
    )?;
    if let Some(profile) = migration_committed_reuse_for(
        global,
        origin,
        state_dir,
        expected_generation,
        updater_token,
    )? {
        if profile.pid != pid || profile.instance_id != instance_id {
            return Err(
                "committed migration admission permits only its existing daemon instance".into(),
            );
        }
        return Ok(());
    }
    if let Some(profile) = migration_readonly_reuse_for(
        global,
        origin,
        state_dir,
        expected_generation,
        updater_token,
    )? {
        if profile.pid != pid || profile.instance_id != instance_id {
            return Err(
                "migration permits only the captured read-only target instance, not a new daemon"
                    .into(),
            );
        }
        return Ok(());
    }
    if let Some((scope, startup)) = migration_start_for(
        global,
        origin,
        state_dir,
        expected_generation,
        updater_token,
    )? {
        migration_start_registry_checks(global, &scope, Some((pid, startup.image)))?;
    }
    let key = installation_key(origin)?;
    let mut entries = active(global, &key)?;
    entries.retain(|e| live(e.pid));
    if entries
        .iter()
        .any(|e| e.state_dir == state_dir && (e.pid != pid || e.instance_id != instance_id))
    {
        return Err("profile already has another active instance".into());
    }
    if entries
        .iter()
        .any(|e| e.state_dir == state_dir && e.pid == pid && e.instance_id == instance_id)
    {
        return Ok(());
    }
    if entries.len() >= 64 {
        return Err("too many active profiles for installation".into());
    }
    entries.push(ActiveProfile {
        state_dir: state_dir.into(),
        pid,
        instance_id: instance_id.into(),
    });
    write_private_bytes(
        &active_path(global, &key),
        &serde_json::to_vec(&entries).map_err(|e| e.to_string())?,
        0o600,
    )
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeStart {
    version: u32,
    root_key: String,
    pid: u32,
    instance_id: String,
    image: crate::ExecutableIdentity,
    stop_generation: u64,
}
fn runtime_start_path(state: &Path, instance: &str) -> PathBuf {
    state.join("updates-v4").join(format!(
        "{:x}.runtime-start.json",
        Sha256::digest(instance.as_bytes())
    ))
}
/// Capture before startup admission, then retain after registration. A Stop
/// racing startup cannot become new restart/reconnection authority.
pub fn capture_runtime_start_fence(state: &Path) -> Result<u64, String> {
    let _gate = lock_gates(&global_updates()?)?;
    combined_generation(state)
}
pub fn retain_runtime_start(
    origin: &InstallOrigin,
    state: &Path,
    pid: u32,
    instance: &str,
    image: &crate::ExecutableIdentity,
    generation: u64,
) -> Result<(), String> {
    let global = global_updates()?;
    let _gate = lock_gates(&global)?;
    let key = installation_key(origin)?;
    if combined_generation(state)? != generation
        || !active(&global, &key)?.iter().any(|entry| {
            entry.state_dir == state && entry.pid == pid && entry.instance_id == instance
        })
    {
        return Err(
            "user Stop or changed active instance cancelled runtime startup authority".into(),
        );
    }
    historical_identity(image)?;
    check_dir(&state.join("updates-v4"), true)?;
    let receipt = RuntimeStart {
        version: 1,
        root_key: key,
        pid,
        instance_id: instance.into(),
        image: image.clone(),
        stop_generation: generation,
    };
    write_private_new(
        &runtime_start_path(state, instance),
        &serde_json::to_vec(&receipt).map_err(|e| e.to_string())?,
        0o600,
    )
}
pub fn verified_runtime_start_fence(
    origin: &InstallOrigin,
    state: &Path,
    pid: u32,
    instance: &str,
    image: &crate::ExecutableIdentity,
) -> Result<u64, String> {
    verified_runtime_start_fence_at(&global_updates()?, origin, state, pid, instance, image)
}
fn verified_runtime_start_fence_at(
    global: &Path,
    origin: &InstallOrigin,
    state: &Path,
    pid: u32,
    instance: &str,
    image: &crate::ExecutableIdentity,
) -> Result<u64, String> {
    let _gate = lock_gates(global)?;
    let receipt: RuntimeStart = serde_json::from_slice(&owned_private_file(
        &runtime_start_path(state, instance),
        16384,
    )?)
    .map_err(|e| e.to_string())?;
    if receipt.version != 1
        || receipt.root_key != installation_key(origin)?
        || receipt.pid != pid
        || receipt.instance_id != instance
        || &receipt.image != image
        || receipt.stop_generation != combined_generation(state)?
    {
        return Err("current runtime restart/reconnection authority was superseded by user Stop or changed identity".into());
    }
    Ok(receipt.stop_generation)
}
pub fn unregister_active(
    origin: &InstallOrigin,
    state_dir: &Path,
    pid: u32,
    instance_id: &str,
) -> Result<(), String> {
    let global = global_updates()?;
    let _gate = lock_gates(&global)?;
    let key = installation_key(origin)?;
    let mut entries = active(&global, &key)?;
    entries.retain(|e| !(e.state_dir == state_dir && e.pid == pid && e.instance_id == instance_id));
    write_private_bytes(
        &active_path(&global, &key),
        &serde_json::to_vec(&entries).map_err(|e| e.to_string())?,
        0o600,
    )
}
pub fn conflicting_active_profiles(
    origin: &InstallOrigin,
    state_dir: &Path,
) -> Result<Vec<ActiveProfile>, String> {
    let global = global_updates()?;
    let _gate = lock_gates(&global)?;
    Ok(active_for_origin(&global, origin)?
        .into_iter()
        .filter(|e| e.state_dir != state_dir && live(e.pid))
        .collect())
}

/// Control requests must match both the original and proposed installation scope.
pub fn verify_control_for(
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    state_dir: &Path,
    operation_id: &str,
    token: &str,
) -> Result<(), String> {
    verify_control_at(
        &global_updates()?,
        source,
        target,
        state_dir,
        operation_id,
        token,
    )
}
fn verify_control_at(
    global: &Path,
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    state_dir: &Path,
    operation_id: &str,
    token: &str,
) -> Result<(), String> {
    validate_operation_id(operation_id)?;
    let _gate = lock_gates(global)?;
    reject_migration_for_origins(global, source, target, state_dir)?;
    reject_old_evidence(global, Some(source), target, Some(state_dir), false)?;
    let entries = scopes(global)?;
    let data = scope_for(&entries, operation_id, state_dir)?;
    if &data.source != source
        || data.target.as_ref() != target
        || data.keys != installation_keys(source, target)?
        || data.control_token != token
        || generation(state_dir)? != data.stop_generation
        || !markers_match(global, data)?
    {
        return Err("updater request has no valid full-scope reservation".into());
    }
    Ok(())
}
pub fn stop_generation(state_dir: &Path) -> Result<u64, String> {
    let _gate = lock_gates(&global_updates()?)?;
    generation(state_dir)
}
pub fn record_user_stop(state_dir: &Path) -> Result<u64, String> {
    record_user_stop_at(&global_updates()?, state_dir)
}
fn record_user_stop_at(global: &Path, state_dir: &Path) -> Result<u64, String> {
    let _gate = lock_gates(global)?;
    let next = read_generation(&generation_file(state_dir)?)?
        .checked_add(1)
        .ok_or("stop generation overflow")?;
    write_private_bytes(
        &generation_file(state_dir)?,
        &serde_json::to_vec(&next).map_err(|e| e.to_string())?,
        0o600,
    )?;
    generation(state_dir)
}
/// Durable atomic scope index gates both roots even across partial marker writes.
/// Dropping the locks never clears the scope; release follows reconciliation.
pub struct InstallationReservation {
    _locks: Vec<FileLock>,
    _profile_lock: FileLock,
    source: InstallOrigin,
    target: Option<InstallOrigin>,
    operation_id: String,
    state_dir: PathBuf,
    global_dir: PathBuf,
    control_token: String,
    pub stop_generation: u64,
}
/// Read-only inspection of the authoritative scope index. This never repairs
/// partially written markers or treats selection as startup admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReservationSnapshot {
    pub startup_origin: Option<InstallOrigin>,
    pub markers_complete: bool,
}

pub fn reservation_snapshot(
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    state_dir: &Path,
    operation_id: &str,
) -> Result<Option<ReservationSnapshot>, String> {
    reservation_snapshot_at(&global_updates()?, source, target, state_dir, operation_id)
}

fn reservation_snapshot_at(
    global: &Path,
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    state_dir: &Path,
    operation_id: &str,
) -> Result<Option<ReservationSnapshot>, String> {
    validate_operation_id(operation_id)?;
    check_dir(state_dir, false)?;
    let updates = state_dir.join(UPDATES_DIR);
    if !missing_file(&updates) {
        check_dir(&updates, false)?;
    }
    let keys = installation_keys(source, target)?;
    let _gate = lock_gates(global)?;
    reject_old_evidence(global, Some(source), target, Some(state_dir), false)?;
    for origin in std::iter::once(source).chain(target) {
        reject_legacy_marker(global, origin)?;
    }
    let entries = scopes(global)?;
    let found = entries.iter().find(|entry| {
        entry.operation_id == operation_id
            || entry.state_dir == state_dir
            || entry.keys.iter().any(|key| keys.contains(key))
    });
    let paths = std::iter::once(updates.join("profile.reservation.json"))
        .chain(keys.iter().map(|key| marker_path(global, key)));
    match found {
        Some(entry)
            if entry.operation_id == operation_id
                && entry.state_dir == state_dir
                && &entry.source == source
                && entry.target.as_ref() == target
                && entry.keys == keys =>
        {
            let mut markers_complete = true;
            for path in paths {
                match marker_at(&path)? {
                    Some(marker) if compatible_marker(&marker, entry) => {
                        markers_complete &= marker == *entry;
                    }
                    Some(_) => {
                        return Err("reservation evidence conflicts with full scope".into());
                    }
                    None => markers_complete = false,
                }
            }
            Ok(Some(ReservationSnapshot {
                startup_origin: entry.startup_origin.clone(),
                markers_complete,
            }))
        }
        Some(_) => Err("another update has reserved this profile or installation".into()),
        None => {
            for path in paths {
                if !missing_file(&path) {
                    return Err(
                        "reservation marker has no durable scope; preserve original bytes".into(),
                    );
                }
            }
            Ok(None)
        }
    }
}

/// Read-only discovery includes index-only reservations left by an interrupted
/// marker write; incompatible evidence is never interpreted as absence.
pub fn reservation_exists(
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    state_dir: &Path,
    operation_id: &str,
) -> Result<bool, String> {
    reservation_exists_at(&global_updates()?, source, target, state_dir, operation_id)
}
fn reservation_exists_at(
    global: &Path,
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    state_dir: &Path,
    operation_id: &str,
) -> Result<bool, String> {
    Ok(reservation_snapshot_at(global, source, target, state_dir, operation_id)?.is_some())
}

pub fn reserve_install(
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    state_dir: &Path,
    operation_id: &str,
) -> Result<InstallationReservation, String> {
    reserve_install_at(&global_updates()?, source, target, state_dir, operation_id)
}
fn reserve_install_at(
    global: &Path,
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    state_dir: &Path,
    operation_id: &str,
) -> Result<InstallationReservation, String> {
    validate_operation_id(operation_id)?;
    check_dir(global, false)?;
    let keys = installation_keys(source, target)?;
    let _gate = lock_gates(global)?;
    reject_migration_for_origins(global, source, target, state_dir)?;
    reject_old_evidence(global, Some(source), target, Some(state_dir), false)?;
    let locks = lock_scope(global, source, target, &keys)?;
    let profile_lock = lock_with_mode(
        &private_updates(state_dir)?.join("profile.lock"),
        libc::LOCK_EX | libc::LOCK_NB,
    )?;
    // The gates were acquired before the sorted installation locks.
    let mut entries = scopes(global)?;
    for origin in std::iter::once(source).chain(target) {
        reject_legacy_marker(global, origin)?;
        if active_for_origin(global, origin)?
            .iter()
            .any(|entry| entry.state_dir != state_dir && live(entry.pid))
        {
            return Err("another active profile shares the installation".into());
        }
    }
    if entries.iter().any(|entry| {
        entry.state_dir == state_dir
            || entry.operation_id == operation_id
            || entry.keys.iter().any(|key| keys.contains(key))
    }) || !missing_file(&profile_marker(state_dir)?)
        || keys
            .iter()
            .any(|key| !missing_file(&marker_path(global, key)))
    {
        return Err(
            "installation or profile already reserved; inspect/recover previous operation".into(),
        );
    }
    if entries.len() >= 256 {
        return Err("installation scope registry is full".into());
    }
    let stop_generation = generation(state_dir)?;
    let control_token = new_token()?;
    let data = Reservation {
        version: UPDATER_PROTOCOL,
        operation_id: operation_id.into(),
        source: source.clone(),
        target: target.cloned(),
        keys,
        state_dir: state_dir.into(),
        stop_generation,
        control_token: control_token.clone(),
        startup_origin: None,
        startup_token: None,
    };
    entries.push(data.clone());
    write_scopes(global, &entries)?;
    // Keep the index after a failed marker write: it is the durable barrier
    // for both roots, and recovery completes the missing individual markers.
    write_markers(global, &data)?;
    Ok(InstallationReservation {
        _locks: locks,
        _profile_lock: profile_lock,
        source: source.clone(),
        target: target.cloned(),
        operation_id: operation_id.into(),
        state_dir: state_dir.into(),
        global_dir: global.into(),
        control_token,
        stop_generation,
    })
}
fn new_token() -> Result<String, String> {
    let mut random = [0u8; 32];
    use std::io::Read;
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut random))
        .map_err(|e| e.to_string())?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut token = String::with_capacity(64);
    for byte in random {
        token.push(HEX[(byte >> 4) as usize] as char);
        token.push(HEX[(byte & 15) as usize] as char);
    }
    Ok(token)
}
fn compatible_marker(actual: &Reservation, expected: &Reservation) -> bool {
    actual == expected
        || (actual.startup_origin.is_none()
            && actual.startup_token.is_none()
            && actual.version == expected.version
            && actual.operation_id == expected.operation_id
            && actual.source == expected.source
            && actual.target == expected.target
            && actual.keys == expected.keys
            && actual.state_dir == expected.state_dir
            && actual.stop_generation == expected.stop_generation
            && actual.control_token == expected.control_token)
}
/// Reattach to an atomic scope after coordinator death; callers still reconcile
/// disk state and journal before they start or release anything.
pub fn recover_reservation(
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    state_dir: &Path,
    operation_id: &str,
) -> Result<InstallationReservation, String> {
    recover_reservation_at(&global_updates()?, source, target, state_dir, operation_id)
}
fn recover_reservation_at(
    global: &Path,
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    state_dir: &Path,
    operation_id: &str,
) -> Result<InstallationReservation, String> {
    validate_operation_id(operation_id)?;
    if !missing_file(&crate::standalone_install::plan_path(
        state_dir,
        operation_id,
    )?) {
        return Err("standalone approval requires bootstrap-specific reservation recovery".into());
    }
    let keys = installation_keys(source, target)?;
    let _gate = lock_gates(global)?;
    reject_migration_for_origins(global, source, target, state_dir)?;
    reject_old_evidence(global, Some(source), target, Some(state_dir), false)?;
    let locks = lock_scope(global, source, target, &keys)?;
    let profile_lock = lock_with_mode(
        &private_updates(state_dir)?.join("profile.lock"),
        libc::LOCK_EX | libc::LOCK_NB,
    )?;
    // Never attach a protocol-4 helper to legacy evidence.
    for origin in std::iter::once(source).chain(target) {
        reject_legacy_marker(global, origin)?;
    }
    let entries = scopes(global)?;
    let data = scope_for(&entries, operation_id, state_dir)?;
    if &data.source != source || data.target.as_ref() != target || data.keys != keys {
        return Err("recovery full installation scope does not match".into());
    }
    for path in std::iter::once(profile_marker(state_dir)?)
        .chain(keys.iter().map(|key| marker_path(global, key)))
    {
        if let Some(marker) = marker_at(&path)? {
            if !compatible_marker(&marker, data) {
                return Err("recovery marker conflicts with durable installation scope".into());
            }
        }
    }
    write_markers(global, data)?;
    Ok(InstallationReservation {
        _locks: locks,
        _profile_lock: profile_lock,
        source: source.clone(),
        target: target.cloned(),
        operation_id: operation_id.into(),
        state_dir: state_dir.into(),
        global_dir: global.into(),
        control_token: data.control_token.clone(),
        stop_generation: data.stop_generation,
    })
}

fn bootstrap_start_fence(data: &Reservation) -> Result<(), String> {
    let path = crate::standalone_install::plan_path(&data.state_dir, &data.operation_id)?;
    if missing_file(&path) {
        return Ok(());
    }
    let plan = crate::standalone_install::read_plan(&data.state_dir, &data.operation_id)?;
    if plan.version != 2 || crate::identity::LEGACY {
        return Err(
            "historical standalone approval has no startup authority in this helper".into(),
        );
    }
    if plan.running().is_none() {
        return Err("stopped standalone approval does not authorize startup".into());
    }
    let ack = crate::standalone_install::read_stop_ack(&plan)?
        .ok_or("standalone startup lacks authenticated durable Stop ACK")?;
    ack.validate(&plan)?;
    let next = plan
        .original_stop_generation
        .checked_add(1)
        .ok_or("stop generation overflow")?;
    if data.source != *plan.origin()
        || data.target.as_ref() != plan.reservation_target()
        || data.control_token != plan.control_token
        || data.stop_generation != next
    {
        return Err("standalone Stop ACK has not advanced the authoritative scope".into());
    }
    Ok(())
}

/// This recovery never promotes an ACK whose authoritative index is still at g.
pub fn recover_bootstrap_reservation(
    plan: &crate::standalone_install::StandaloneInstallPlan,
) -> Result<InstallationReservation, String> {
    recover_bootstrap_reservation_at(&global_updates()?, plan)
}

fn recover_bootstrap_reservation_at(
    global: &Path,
    plan: &crate::standalone_install::StandaloneInstallPlan,
) -> Result<InstallationReservation, String> {
    recover_bootstrap_scope_at(global, plan, false)
}

/// Reconciliation-only ownership. No control token or startup API is exposed.
pub struct BootstrapReleaseReservation {
    reservation: InstallationReservation,
}
impl BootstrapReleaseReservation {
    pub fn release(self) -> Result<(), String> {
        self.reservation.release()
    }
}
pub fn recover_bootstrap_release(
    plan: &crate::standalone_install::StandaloneInstallPlan,
) -> Result<BootstrapReleaseReservation, String> {
    Ok(BootstrapReleaseReservation {
        reservation: recover_bootstrap_scope_at(&global_updates()?, plan, true)?,
    })
}

/// Reattach only to release an unadvanced admission. This handle deliberately
/// exposes no token/start methods and cannot turn partial publication into Stop authority.
pub fn recover_unstarted_installation(
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    state: &Path,
    id: &str,
    expected_generation: u64,
) -> Result<BootstrapReleaseReservation, String> {
    recover_unstarted_installation_at(
        &global_updates()?,
        source,
        target,
        state,
        id,
        expected_generation,
    )
}
fn recover_unstarted_installation_at(
    global: &Path,
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    state: &Path,
    id: &str,
    expected_generation: u64,
) -> Result<BootstrapReleaseReservation, String> {
    validate_operation_id(id)?;
    if !missing_file(&crate::standalone_install::plan_path(state, id)?) {
        let plan = crate::standalone_install::read_plan(state, id)?;
        if plan.origin() != source
            || plan.reservation_target() != target
            || plan.original_stop_generation != expected_generation
            || crate::standalone_install::read_stop_ack(&plan)?.is_some()
        {
            return Err("unstarted approval scope differs".into());
        }
        if let Some(record) = crate::standalone_install::read_record(state, id)? {
            if !matches!(
                record.phase,
                crate::standalone_install::StandaloneInstallPhase::Approved
                    | crate::standalone_install::StandaloneInstallPhase::Failed
            ) {
                return Err("Stop may already have been requested".into());
            }
        }
    }
    for path in [
        crate::standalone_install::stop_ack_path(state, id)?,
        crate::standalone_install::replacement_path(state, id)?,
        crate::standalone_install::relocation_commit_path(state, id)?,
    ] {
        if !missing_file(&path) {
            return Err("unstarted release contains mutation evidence".into());
        }
    }
    let keys = installation_keys(source, target)?;
    let _gates = lock_gates(global)?;
    reject_migration_for_origins(global, source, target, state)?;
    reject_old_evidence(global, Some(source), target, Some(state), false)?;
    let locks = lock_scope(global, source, target, &keys)?;
    let profile_lock = lock_with_mode(
        &private_updates(state)?.join("profile.lock"),
        libc::LOCK_EX | libc::LOCK_NB,
    )?;
    for origin in std::iter::once(source).chain(target) {
        reject_legacy_marker(global, origin)?;
    }
    let entries = scopes(global)?;
    let data = scope_for(&entries, id, state)?;
    if data.source != *source
        || data.target.as_ref() != target
        || data.keys != keys
        || data.stop_generation != expected_generation
        || generation(state)? != expected_generation
        || data.startup_origin.is_some()
        || data.startup_token.is_some()
    {
        return Err("unstarted release generation, scope or startup authority differs".into());
    }
    for path in std::iter::once(profile_marker(state)?)
        .chain(keys.iter().map(|key| marker_path(global, key)))
    {
        if let Some(marker) = marker_at(&path)? {
            if marker != *data {
                return Err("unstarted marker differs from exact unadvanced scope".into());
            }
        }
    }
    write_markers(global, data)?;
    Ok(BootstrapReleaseReservation {
        reservation: InstallationReservation {
            _locks: locks,
            _profile_lock: profile_lock,
            source: source.clone(),
            target: target.cloned(),
            operation_id: id.into(),
            state_dir: state.into(),
            global_dir: global.into(),
            control_token: data.control_token.clone(),
            stop_generation: expected_generation,
        },
    })
}

fn recover_bootstrap_scope_at(
    global: &Path,
    plan: &crate::standalone_install::StandaloneInstallPlan,
    release_only: bool,
) -> Result<InstallationReservation, String> {
    plan.validate()?;
    if plan.version != 2 || crate::identity::LEGACY {
        return Err("historical standalone approval requires its recorded matching helper".into());
    }
    let state = &plan.profile.state_dir;
    if crate::standalone_install::read_plan(state, &plan.operation_id)? != *plan {
        return Err("bootstrap recovery approval differs from durable approval".into());
    }
    let _gates = lock_gates(global)?;
    reject_migration_for_origins(global, plan.origin(), plan.reservation_target(), state)?;
    reject_old_evidence(
        global,
        Some(plan.origin()),
        plan.reservation_target(),
        Some(state),
        false,
    )?;
    reject_legacy_marker(global, plan.origin())?;
    if let Some(target) = plan.reservation_target() {
        reject_legacy_marker(global, target)?;
    }
    let keys = installation_keys(plan.origin(), plan.reservation_target())?;
    let locks = lock_scope(global, plan.origin(), plan.reservation_target(), &keys)?;
    let profile_lock = lock_with_mode(
        &private_updates(state)?.join("profile.lock"),
        libc::LOCK_EX | libc::LOCK_NB,
    )?;
    let entries = scopes(global)?;
    let data = scope_for(&entries, &plan.operation_id, state)?;
    if data.source != *plan.origin()
        || data.target.as_ref() != plan.reservation_target()
        || data.keys != keys
        || data.control_token != plan.control_token
    {
        return Err("bootstrap recovery scope differs from immutable approval".into());
    }
    let running = plan.running().is_some();
    let baseline = if running && release_only {
        let next = plan
            .original_stop_generation
            .checked_add(1)
            .ok_or("stop generation overflow")?;
        let ack = crate::standalone_install::read_stop_ack(plan)?;
        if let Some(ack) = &ack {
            ack.validate(plan)?;
        }
        if data.stop_generation == plan.original_stop_generation
            && data.startup_origin.is_none()
            && data.startup_token.is_none()
        {
            plan.original_stop_generation
        } else if data.stop_generation == next && ack.is_some() {
            bootstrap_start_fence(data)?;
            next
        } else {
            return Err(
                "release recovery cannot authenticate the approved index transition".into(),
            );
        }
    } else if running {
        bootstrap_start_fence(data)?;
        plan.original_stop_generation
            .checked_add(1)
            .ok_or("stop generation overflow")?
    } else {
        if crate::standalone_install::read_stop_ack(plan)?.is_some()
            || data.startup_token.is_some()
            || data.startup_origin.is_some()
        {
            return Err("stopped approval contains unexpected restart authority".into());
        }
        plan.original_stop_generation
    };
    let current = generation(state)?;
    if data.stop_generation != baseline
        || if release_only {
            current < baseline
        } else {
            current != baseline
        }
    {
        return Err("user Stop or unadvanced bootstrap index forbids restart recovery".into());
    }
    for path in std::iter::once(profile_marker(state)?)
        .chain(keys.iter().map(|key| marker_path(global, key)))
    {
        if let Some(marker) = marker_at(&path)? {
            let mut old = data.clone();
            old.stop_generation = plan.original_stop_generation;
            old.startup_origin = None;
            old.startup_token = None;
            if !compatible_marker(&marker, data) && !(running && marker == old) {
                return Err(
                    "bootstrap recovery marker is not the exact approved generation transition"
                        .into(),
                );
            }
        }
    }
    write_markers(global, data)?;
    Ok(InstallationReservation {
        _locks: locks,
        _profile_lock: profile_lock,
        source: data.source.clone(),
        target: data.target.clone(),
        operation_id: data.operation_id.clone(),
        state_dir: state.clone(),
        global_dir: global.into(),
        control_token: data.control_token.clone(),
        stop_generation: baseline,
    })
}

impl InstallationReservation {
    /// Only the native runtime's authenticated exchange can supply this value.
    pub fn acknowledge_bootstrap_stop(
        &mut self,
        ack: &crate::native_runtime::VerifiedBootstrapStopAck,
    ) -> Result<(), String> {
        self.acknowledge_bootstrap_stop_record(ack.record())
    }

    fn acknowledge_bootstrap_stop_record(
        &mut self,
        ack: &crate::standalone_install::BootstrapStopAck,
    ) -> Result<(), String> {
        let _gates = lock_gates(&self.global_dir)?;
        let plan = crate::standalone_install::read_plan(&self.state_dir, &self.operation_id)?;
        if plan.version != 2 || crate::identity::LEGACY {
            return Err(
                "historical standalone approval has no Stop authority in this helper".into(),
            );
        }
        ack.validate(&plan)?;
        let next = plan
            .original_stop_generation
            .checked_add(1)
            .ok_or("stop generation overflow")?;
        let mut entries = scopes(&self.global_dir)?;
        let data = self.checked_scope(&entries)?;
        if data.source != *plan.origin()
            || data.target.as_ref() != plan.reservation_target()
            || data.control_token != plan.control_token
            || data.stop_generation != plan.original_stop_generation
            || data.startup_origin.is_some()
            || data.startup_token.is_some()
            || !markers_match(&self.global_dir, data)?
            || generation(&self.state_dir)? != next
            || crate::standalone_install::read_stop_ack(&plan)?.is_some()
        {
            return Err("bootstrap Stop cannot advance this reservation".into());
        }
        let record = crate::standalone_install::read_record(&self.state_dir, &self.operation_id)?
            .ok_or("bootstrap Stop lacks durable intent")?;
        if record.phase != crate::standalone_install::StandaloneInstallPhase::StopIntent {
            return Err("bootstrap Stop lacks exact durable StopIntent".into());
        }
        // ACK first; an interrupted index write is Unknown and never auto-promoted.
        write_private_new(
            &crate::standalone_install::stop_ack_path(&self.state_dir, &self.operation_id)?,
            &serde_json::to_vec(ack).map_err(|e| e.to_string())?,
            0o600,
        )?;
        let index = entries
            .iter()
            .position(|entry| {
                entry.operation_id == self.operation_id && entry.state_dir == self.state_dir
            })
            .ok_or("bootstrap scope disappeared")?;
        entries[index].stop_generation = next;
        write_scopes(&self.global_dir, &entries)?;
        // The live object advances only after complete markers.
        write_markers(&self.global_dir, &entries[index])?;
        self.stop_generation = next;
        Ok(())
    }
    fn checked_scope<'a>(&self, entries: &'a [Reservation]) -> Result<&'a Reservation, String> {
        let data = scope_for(entries, &self.operation_id, &self.state_dir)?;
        if data.source != self.source
            || data.target != self.target
            || data.control_token != self.control_token
            || data.stop_generation != self.stop_generation
        {
            return Err("reservation full-scope ownership mismatch".into());
        }
        Ok(data)
    }
    fn selected(&self, origin: &InstallOrigin) -> Result<(), String> {
        if !selectable_origin(&self.source, self.target.as_ref(), origin) {
            return Err("startup origin is outside reserved installations".into());
        }
        Ok(())
    }
    pub fn control_token(&self) -> &str {
        &self.control_token
    }
    pub fn user_stop_requested(&self) -> Result<bool, String> {
        let _gate = lock_gates(&self.global_dir)?;
        Ok(generation(&self.state_dir)? != self.stop_generation)
    }
    pub fn issued_start_token_for(&self, origin: &InstallOrigin) -> Result<Option<String>, String> {
        self.selected(origin)?;
        let _gate = lock_gates(&self.global_dir)?;
        reject_old_evidence(
            &self.global_dir,
            Some(&self.source),
            self.target.as_ref(),
            Some(&self.state_dir),
            false,
        )?;
        if generation(&self.state_dir)? != self.stop_generation {
            return Err("user stop superseded updater start".into());
        }
        let entries = scopes(&self.global_dir)?;
        let data = self.checked_scope(&entries)?;
        bootstrap_start_fence(data)?;
        if !markers_match(&self.global_dir, data)? {
            return Err("startup reservation scopes do not match".into());
        }
        if data
            .startup_origin
            .as_ref()
            .is_some_and(|selected| selected != origin)
        {
            return Err("startup authorization belongs to another installation".into());
        }
        Ok(data.startup_token.clone())
    }
    /// The selected candidate or verified original is the only admissible start.
    pub fn authorize_start_for(&self, origin: &InstallOrigin) -> Result<String, String> {
        self.selected(origin)?;
        let _gate = lock_gates(&self.global_dir)?;
        reject_old_evidence(
            &self.global_dir,
            Some(&self.source),
            self.target.as_ref(),
            Some(&self.state_dir),
            false,
        )?;
        if generation(&self.state_dir)? != self.stop_generation {
            return Err("user stop superseded updater start".into());
        }
        let mut entries = scopes(&self.global_dir)?;
        let data = self.checked_scope(&entries)?;
        bootstrap_start_fence(data)?;
        if !markers_match(&self.global_dir, data)? {
            return Err("startup reservation scopes do not match".into());
        }
        if data.startup_token.is_some() {
            return Err("startup authorization already issued".into());
        }
        let token = new_token()?;
        let index = entries
            .iter()
            .position(|entry| {
                entry.operation_id == self.operation_id && entry.state_dir == self.state_dir
            })
            .ok_or("operation has no durable installation scope")?;
        entries[index].startup_origin = Some(origin.clone());
        entries[index].startup_token = Some(token.clone());
        // Atomic index first: a partial authorization cannot admit either root.
        write_scopes(&self.global_dir, &entries)?;
        write_markers(&self.global_dir, &entries[index])?;
        Ok(token)
    }
    /// Release only after durable final result and reconciled installation state.
    pub fn release(self) -> Result<(), String> {
        let _gate = lock_gates(&self.global_dir)?;
        let mut entries = scopes(&self.global_dir)?;
        let data = self.checked_scope(&entries)?;
        let paths = std::iter::once(profile_marker(&self.state_dir)?)
            .chain(
                data.keys
                    .iter()
                    .map(|key| marker_path(&self.global_dir, key)),
            )
            .collect::<Vec<_>>();
        for path in &paths {
            if let Some(marker) = marker_at(path)? {
                if !compatible_marker(&marker, &data) {
                    return Err("release marker conflicts with durable installation scope".into());
                }
            }
        }
        for path in &paths {
            if !missing_file(path) {
                fs::remove_file(path).map_err(|e| e.to_string())?;
                sync_parent(path)?;
            }
        }
        entries.retain(|entry| {
            entry.operation_id != self.operation_id || entry.state_dir != self.state_dir
        });
        write_scopes(&self.global_dir, &entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[cfg(target_os = "macos")]
    #[test]
    fn immutable_publication_supports_full_length_evidence_basename() {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join(format!("{}.json", "a".repeat(250)));
        write_private_atomic_new(&path, b"complete evidence", 0o600).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"complete evidence");
        assert!(write_private_atomic_new(&path, b"foreign evidence", 0o600).is_err());
        assert_eq!(fs::read(path).unwrap(), b"complete evidence");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn immutable_publication_preserves_winner_and_rejects_symlink() {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("commit.json");
        std::thread::scope(|scope| {
            let first = scope.spawn(|| write_private_atomic_new(&path, b"first", 0o600));
            let second = scope.spawn(|| write_private_atomic_new(&path, b"second", 0o600));
            let first = first.join().unwrap();
            let second = second.join().unwrap();
            assert_ne!(first.is_ok(), second.is_ok());
            assert_eq!(
                fs::read(&path).unwrap(),
                if first.is_ok() {
                    b"first".as_slice()
                } else {
                    b"second".as_slice()
                }
            );
        });
        let untouched = directory.path().join("foreign");
        fs::write(&untouched, b"foreign evidence").unwrap();
        let link = directory.path().join("linked-commit.json");
        symlink(&untouched, &link).unwrap();
        assert!(write_private_atomic_new(&link, b"replacement", 0o600).is_err());
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(untouched).unwrap(), b"foreign evidence");
    }

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, InstallOrigin) {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join(crate::identity::GLOBAL_UPDATES_DIRECTORY);
        let state = dir.path().join("profile");
        fs::create_dir(&global).unwrap();
        fs::create_dir(&state).unwrap();
        fs::set_permissions(&global, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
        let origin = InstallOrigin::Source {
            root: dir.path().join("checkout"),
        };
        (dir, global, state, origin)
    }

    #[test]
    fn operation_lease_observation_distinguishes_busy_from_unsafe_evidence() {
        let (_directory, global, state, _) = fixture();
        let id = "9".repeat(32);
        let lease = test_scope::lock_operation_at(&global, &state, &id)
            .unwrap()
            .unwrap();
        assert!(test_scope::lock_operation_at(&global, &state, &id)
            .unwrap()
            .is_none());
        drop(lease);
        assert!(test_scope::lock_operation_at(&global, &state, &id)
            .unwrap()
            .is_some());
        let path = private_updates(&state)
            .unwrap()
            .join(format!("{id}.operation.lock"));
        fs::remove_file(&path).unwrap();
        let foreign = state.join("foreign");
        write_private_new(&foreign, b"unchanged", 0o600).unwrap();
        std::os::unix::fs::symlink(&foreign, &path).unwrap();
        assert!(test_scope::lock_operation_at(&global, &state, &id).is_err());
        assert_eq!(fs::read(&foreign).unwrap(), b"unchanged");
    }

    #[cfg(not(desktop_pet_legacy))]
    fn bootstrap_fixture(
        running: bool,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        crate::standalone_install::StandaloneInstallPlan,
        InstallationReservation,
    ) {
        use crate::standalone_install::*;
        let (dir, global, state, _) = fixture();
        let origin = InstallOrigin::Standalone {
            bundle: crate::installation::standalone_bundle().unwrap(),
        };
        let image = crate::ExecutableIdentity {
            path: crate::physical_install_root(&origin)
                .unwrap()
                .join("Contents/MacOS/desktop-pet"),
            sha256: "a".repeat(64),
        };
        let id = "0123456789abcdef0123456789abcdef";
        let reservation = reserve_install_at(&global, &origin, None, &state, id).unwrap();
        let profile = crate::native_runtime::ProfilePaths {
            config_dir: state.clone(),
            state_dir: state.clone(),
        };
        let source = if running {
            InstallSource::Running {
                instance: StandaloneOriginalInstance {
                    pid: std::process::id(),
                    start_sec: 123,
                    start_usec: 456,
                    instance_id: "original-instance".into(),
                    image: image.clone(),
                },
                context: crate::UpdateContext {
                    executable: image.path.clone(),
                    version: "0.4.13-beta.1".into(),
                    instance_id: "original-instance".into(),
                    config_dir: state.clone(),
                    state_dir: state,
                    running: image.clone(),
                    running_origin: Some(Box::new(origin.clone())),
                    assets_override: None,
                    environment: Default::default(),
                    locale: "en".into(),
                },
            }
        } else {
            InstallSource::Stopped {
                profile: profile.clone(),
            }
        };
        let candidate_bundle = dir.path().join("operation/stage/DesktopPet.app");
        let plan = StandaloneInstallPlan {
            version: 2,
            purpose: StandaloneInstallPurpose::Upgrade,
            operation_id: id.into(),
            original: crate::installation::InstallationRecord {
                version: 1,
                executable: image.path.clone(),
                kind: crate::installation::InstallationKind::Standalone,
                app_version: "0.4.13-beta.1".into(),
                protocol: 4,
                observation_mode: 1,
                integration_api: 1,
                origin,
                image,
            },
            candidate_image: crate::ExecutableIdentity {
                path: candidate_bundle.join("Contents/MacOS/desktop-pet"),
                sha256: "b".repeat(64),
            },
            candidate_bundle,
            candidate_version: "0.4.13-beta.5".into(),
            helper_image: crate::ExecutableIdentity {
                path: dir.path().join("operation/helper"),
                sha256: "c".repeat(64),
            },
            profile,
            source,
            original_stop_generation: reservation.stop_generation,
            control_token: reservation.control_token().into(),
        };
        write_plan(&plan).unwrap();
        write_record(
            &plan.profile.state_dir,
            &bootstrap_record(&plan, StandaloneInstallPhase::Approved),
        )
        .unwrap();
        (dir, global, plan, reservation)
    }

    #[cfg(not(desktop_pet_legacy))]
    fn bootstrap_record(
        plan: &crate::standalone_install::StandaloneInstallPlan,
        phase: crate::standalone_install::StandaloneInstallPhase,
    ) -> crate::standalone_install::StandaloneInstallRecord {
        use crate::standalone_install::*;
        StandaloneInstallRecord {
            version: 1,
            operation_id: plan.operation_id.clone(),
            phase,
            result: StandaloneInstallResult {
                version: 1,
                operation_id: plan.operation_id.clone(),
                state_dir: plan.profile.state_dir.clone(),
                status: InstallStatus::Running,
                installation: None,
                installed: false,
                applied: false,
                restart: if plan.running().is_some() {
                    InstallRestart::Pending
                } else {
                    InstallRestart::NotRequested
                },
                retained_bundle: None,
                error: None,
            },
        }
    }

    #[cfg(not(desktop_pet_legacy))]
    fn bootstrap_ack(
        plan: &crate::standalone_install::StandaloneInstallPlan,
    ) -> crate::standalone_install::BootstrapStopAck {
        crate::standalone_install::BootstrapStopAck {
            version: 1,
            operation_id: plan.operation_id.clone(),
            origin: plan.origin().clone(),
            profile: plan.profile.clone(),
            original: plan.running().unwrap().0.clone(),
            original_stop_generation: plan.original_stop_generation,
            control_token: plan.control_token.clone(),
        }
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn unstarted_release_recovers_index_only_and_partial_markers_without_plan() {
        use crate::standalone_install::*;
        for partial in [false, true] {
            let (_dir, global, plan, reservation) = bootstrap_fixture(true);
            let state = &plan.profile.state_dir;
            fs::remove_file(plan_path(state, &plan.operation_id).unwrap()).unwrap();
            fs::remove_file(record_path(state, &plan.operation_id).unwrap()).unwrap();
            let keys = installation_keys(plan.origin(), plan.reservation_target()).unwrap();
            fs::remove_file(profile_marker(state).unwrap()).unwrap();
            if !partial {
                for key in &keys {
                    fs::remove_file(marker_path(&global, key)).unwrap();
                }
            }
            drop(reservation);
            recover_unstarted_installation_at(
                &global,
                plan.origin(),
                plan.reservation_target(),
                state,
                &plan.operation_id,
                plan.original_stop_generation,
            )
            .unwrap()
            .release()
            .unwrap();
            assert!(!reservation_exists_at(
                &global,
                plan.origin(),
                plan.reservation_target(),
                state,
                &plan.operation_id
            )
            .unwrap());
            assert_eq!(generation(state).unwrap(), plan.original_stop_generation);
            assert!(!plan_path(state, &plan.operation_id).unwrap().exists());
        }
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn cancelled_before_release_keeps_live_process_and_durable_winner_unchanged() {
        use crate::standalone_install::*;
        let (_directory, global, plan, reservation) = bootstrap_fixture(true);
        let state = &plan.profile.state_dir;
        let mut cancelled = bootstrap_record(&plan, StandaloneInstallPhase::Failed);
        cancelled.result.status = InstallStatus::Cancelled;
        cancelled.result.restart = InstallRestart::NotRequested;
        write_record(state, &cancelled).unwrap();
        let result_path = record_path(state, &plan.operation_id).unwrap();
        let original_bytes = fs::read(&result_path).unwrap();
        let mut original = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        assert!(original.try_wait().unwrap().is_none());
        drop(reservation);
        let release = recover_unstarted_installation_at(
            &global,
            plan.origin(),
            plan.reservation_target(),
            state,
            &plan.operation_id,
            plan.original_stop_generation,
        )
        .unwrap();
        // Durable cancellation is not release proof. The exact scope remains
        // until cleanup explicitly releases it; cleanup neither waits nor Stops.
        assert!(reservation_exists_at(
            &global,
            plan.origin(),
            plan.reservation_target(),
            state,
            &plan.operation_id
        )
        .unwrap());
        release.release().unwrap();
        assert!(original.try_wait().unwrap().is_none());
        assert_eq!(fs::read(&result_path).unwrap(), original_bytes);
        assert_eq!(
            read_record(state, &plan.operation_id).unwrap().unwrap(),
            cancelled
        );
        assert_eq!(generation(state).unwrap(), plan.original_stop_generation);
        assert!(read_stop_ack(&plan).unwrap().is_none());
        assert!(!reservation_exists_at(
            &global,
            plan.origin(),
            plan.reservation_target(),
            state,
            &plan.operation_id
        )
        .unwrap());
        original.kill().unwrap();
        original.wait().unwrap();
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn unstarted_release_rejects_stop_intent_later_generation_and_startup_authority() {
        use crate::standalone_install::*;
        for boundary in 0..3 {
            let (_dir, global, plan, reservation) = bootstrap_fixture(true);
            let state = &plan.profile.state_dir;
            if boundary == 0 {
                write_record(
                    state,
                    &bootstrap_record(&plan, StandaloneInstallPhase::StopIntent),
                )
                .unwrap();
            } else if boundary == 1 {
                record_user_stop_at(&global, state).unwrap();
            } else {
                let mut entries = scopes(&global).unwrap();
                entries[0].startup_origin = Some(plan.origin().clone());
                entries[0].startup_token = Some("f".repeat(64));
                write_scopes(&global, &entries).unwrap();
                write_markers(&global, &entries[0]).unwrap();
            }
            drop(reservation);
            assert!(recover_unstarted_installation_at(
                &global,
                plan.origin(),
                plan.reservation_target(),
                state,
                &plan.operation_id,
                plan.original_stop_generation
            )
            .is_err());
            assert!(reservation_exists_at(
                &global,
                plan.origin(),
                plan.reservation_target(),
                state,
                &plan.operation_id
            )
            .unwrap());
        }
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn historical_bootstrap_is_observable_but_cannot_stop_start_or_recover_with_new_helper() {
        use crate::standalone_install::*;
        let (_dir, global, mut plan, mut reservation) = bootstrap_fixture(true);
        plan.version = 1;
        let state = &plan.profile.state_dir;
        write_private_bytes(
            &plan_path(state, &plan.operation_id).unwrap(),
            &serde_json::to_vec(&plan).unwrap(),
            0o600,
        )
        .unwrap();
        let record = bootstrap_record(&plan, StandaloneInstallPhase::StopIntent);
        write_private_bytes(
            &record_path(state, &plan.operation_id).unwrap(),
            &serde_json::to_vec(&record).unwrap(),
            0o600,
        )
        .unwrap();
        assert_eq!(read_plan(state, &plan.operation_id).unwrap(), plan);
        assert!(matches!(
            observe_operation(state, &plan.operation_id).unwrap(),
            Some(OperationObservation::StandaloneInstall(_))
        ));
        record_user_stop_at(&global, state).unwrap();
        let ack = bootstrap_ack(&plan);
        assert!(reservation.acknowledge_bootstrap_stop_record(&ack).is_err());
        assert!(read_stop_ack(&plan).unwrap().is_none());
        // Even a historical helper's genuine completed ACK/index is not fresh
        // execution authority for the new helper.
        write_private_new(
            &stop_ack_path(state, &plan.operation_id).unwrap(),
            &serde_json::to_vec(&ack).unwrap(),
            0o600,
        )
        .unwrap();
        let mut entries = scopes(&global).unwrap();
        entries[0].stop_generation = 1;
        write_scopes(&global, &entries).unwrap();
        write_markers(&global, &entries[0]).unwrap();
        assert!(bootstrap_start_fence(&entries[0]).is_err());
        assert_eq!(read_stop_ack(&plan).unwrap(), Some(ack));
        drop(reservation);
        assert!(recover_bootstrap_reservation_at(&global, &plan).is_err());
        assert!(recover_bootstrap_scope_at(&global, &plan, true).is_err());
        assert_eq!(
            read_record(state, &plan.operation_id).unwrap(),
            Some(record)
        );
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn relocation_reserves_both_roots_and_recovers_target_without_rebinding_source_ack() {
        use crate::standalone_install::*;
        let (_dir, global, state, _) = fixture();
        let profile = crate::native_runtime::ProfilePaths {
            config_dir: state.clone(),
            state_dir: state.clone(),
        };
        let mut plan = crate::standalone_install::tests::relocation_plan(profile);
        let mut reservation = reserve_install_at(
            &global,
            plan.origin(),
            plan.reservation_target(),
            &state,
            &plan.operation_id,
        )
        .unwrap();
        plan.original_stop_generation = reservation.stop_generation;
        plan.control_token = reservation.control_token().into();
        if let StandaloneInstallPurpose::PrivateRelocation { parent, .. } = &mut plan.purpose {
            parent.generation = plan.original_stop_generation;
        }
        plan.validate().unwrap();
        // Fixture publication intentionally bypasses fresh signed-app execution
        // admission. This test exercises protocol scope and ACK boundaries only.
        write_private_new(
            &plan_path(&state, &plan.operation_id).unwrap(),
            &serde_json::to_vec(&plan).unwrap(),
            0o600,
        )
        .unwrap();
        write_record(
            &state,
            &bootstrap_record(&plan, StandaloneInstallPhase::StopIntent),
        )
        .unwrap();
        assert_eq!(scopes(&global).unwrap()[0].keys.len(), 2);
        assert!(reservation
            .authorize_start_for(plan.installed_origin())
            .is_err());
        record_user_stop_at(&global, &state).unwrap();
        let mut wrong = bootstrap_ack(&plan);
        wrong.origin = plan.installed_origin().clone();
        assert!(reservation
            .acknowledge_bootstrap_stop_record(&wrong)
            .is_err());
        reservation
            .acknowledge_bootstrap_stop_record(&bootstrap_ack(&plan))
            .unwrap();
        assert_eq!(
            read_stop_ack(&plan).unwrap().unwrap().origin,
            *plan.origin()
        );
        drop(reservation);
        let recovered = recover_bootstrap_reservation_at(&global, &plan).unwrap();
        assert_eq!(recovered.target.as_ref(), plan.reservation_target());
        let token = recovered
            .authorize_start_for(plan.installed_origin())
            .unwrap();
        start_allowed_at(
            &global,
            plan.installed_origin(),
            &state,
            Some(1),
            Some(&token),
        )
        .unwrap();
        assert!(start_allowed_at(&global, plan.origin(), &state, Some(1), Some(&token),).is_err());
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn bootstrap_ack_advances_only_exact_own_stop_and_duplicate_cannot_advance_again() {
        use crate::standalone_install::*;
        let (_dir, global, plan, mut reservation) = bootstrap_fixture(true);
        let state = &plan.profile.state_dir;
        let ack = bootstrap_ack(&plan);
        assert!(reservation.authorize_start_for(plan.origin()).is_err());
        assert!(reservation.acknowledge_bootstrap_stop_record(&ack).is_err());
        write_record(
            state,
            &bootstrap_record(&plan, StandaloneInstallPhase::StopIntent),
        )
        .unwrap();
        assert!(reservation.acknowledge_bootstrap_stop_record(&ack).is_err());
        record_user_stop_at(&global, state).unwrap();
        let mut wrong = ack.clone();
        wrong.original.start_usec += 1;
        assert!(reservation
            .acknowledge_bootstrap_stop_record(&wrong)
            .is_err());
        assert!(read_stop_ack(&plan).unwrap().is_none());
        reservation.acknowledge_bootstrap_stop_record(&ack).unwrap();
        assert_eq!(reservation.stop_generation, 1);
        assert_eq!(read_stop_ack(&plan).unwrap(), Some(ack.clone()));
        assert!(reservation.acknowledge_bootstrap_stop_record(&ack).is_err());
        assert_eq!(generation(state).unwrap(), 1);
        assert_eq!(plan.original_stop_generation, 0);
        let token = reservation.authorize_start_for(plan.origin()).unwrap();
        start_allowed_at(&global, plan.origin(), state, Some(1), Some(&token)).unwrap();
        record_user_stop_at(&global, state).unwrap();
        assert!(reservation.issued_start_token_for(plan.origin()).is_err());
        assert!(start_allowed_at(&global, plan.origin(), state, Some(1), Some(&token)).is_err());
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn release_only_recovery_clears_fences_without_promoting_lost_ack_or_later_stop() {
        use crate::standalone_install::*;
        for (running, acknowledged, ack_without_advance) in [
            (false, false, false),
            (true, false, false),
            (true, false, true),
            (true, true, false),
        ] {
            let (_dir, global, plan, mut reservation) = bootstrap_fixture(running);
            let state = &plan.profile.state_dir;
            if acknowledged {
                write_record(
                    state,
                    &bootstrap_record(&plan, StandaloneInstallPhase::StopIntent),
                )
                .unwrap();
                record_user_stop_at(&global, state).unwrap();
                reservation
                    .acknowledge_bootstrap_stop_record(&bootstrap_ack(&plan))
                    .unwrap();
                reservation.authorize_start_for(plan.origin()).unwrap();
            }
            record_user_stop_at(&global, state).unwrap();
            if ack_without_advance {
                write_private_new(
                    &stop_ack_path(state, &plan.operation_id).unwrap(),
                    &serde_json::to_vec(&bootstrap_ack(&plan)).unwrap(),
                    0o600,
                )
                .unwrap();
            }
            let current = generation(state).unwrap();
            let baseline = reservation.stop_generation;
            drop(reservation);
            assert!(recover_bootstrap_reservation_at(&global, &plan).is_err());
            let release = recover_bootstrap_scope_at(&global, &plan, true).unwrap();
            assert_eq!(release.stop_generation, baseline);
            assert_eq!(scopes(&global).unwrap()[0].stop_generation, baseline);
            assert!(release.authorize_start_for(plan.origin()).is_err());
            assert!(start_allowed_at(&global, plan.origin(), state, None, None).is_err());
            BootstrapReleaseReservation {
                reservation: release,
            }
            .release()
            .unwrap();
            assert_eq!(generation(state).unwrap(), current);
            assert_eq!(
                read_stop_ack(&plan).unwrap().is_some(),
                acknowledged || ack_without_advance
            );
            start_allowed_at(&global, plan.origin(), state, None, None).unwrap();
        }
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn bootstrap_extra_stop_and_existing_start_authority_reject_ack_without_writing_it() {
        use crate::standalone_install::*;
        for extra_stop in [false, true] {
            let (_dir, global, plan, mut reservation) = bootstrap_fixture(true);
            write_record(
                &plan.profile.state_dir,
                &bootstrap_record(&plan, StandaloneInstallPhase::StopIntent),
            )
            .unwrap();
            record_user_stop_at(&global, &plan.profile.state_dir).unwrap();
            if extra_stop {
                record_user_stop_at(&global, &plan.profile.state_dir).unwrap();
            } else {
                let mut entries = scopes(&global).unwrap();
                entries[0].startup_origin = Some(plan.origin().clone());
                entries[0].startup_token = Some("d".repeat(64));
                write_scopes(&global, &entries).unwrap();
                write_markers(&global, &entries[0]).unwrap();
            }
            assert!(reservation
                .acknowledge_bootstrap_stop_record(&bootstrap_ack(&plan))
                .is_err());
            assert!(read_stop_ack(&plan).unwrap().is_none());
        }
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn bootstrap_recovery_requires_ack_and_advanced_index_and_only_repairs_exact_old_markers() {
        use crate::standalone_install::*;
        for (ack_present, advanced) in [(false, false), (true, false), (false, true), (true, true)]
        {
            let (_dir, global, plan, reservation) = bootstrap_fixture(true);
            let state = &plan.profile.state_dir;
            record_user_stop_at(&global, state).unwrap();
            if ack_present {
                write_private_new(
                    &stop_ack_path(state, &plan.operation_id).unwrap(),
                    &serde_json::to_vec(&bootstrap_ack(&plan)).unwrap(),
                    0o600,
                )
                .unwrap();
            }
            if advanced {
                let mut entries = scopes(&global).unwrap();
                entries[0].stop_generation = 1;
                write_scopes(&global, &entries).unwrap();
            }
            // The index is authoritative even with a missing root marker.
            let root = marker_path(&global, &installation_key(plan.origin()).unwrap());
            fs::remove_file(&root).unwrap();
            drop(reservation);
            assert!(start_allowed_at(&global, plan.origin(), state, None, None).is_err());
            assert!(recover_reservation_at(
                &global,
                plan.origin(),
                None,
                state,
                &plan.operation_id
            )
            .is_err());
            let recovered = recover_bootstrap_reservation_at(&global, &plan);
            if ack_present && advanced {
                let recovered = recovered.unwrap();
                assert_eq!(recovered.stop_generation, 1);
                assert!(markers_match(&global, &scopes(&global).unwrap()[0]).unwrap());
                let token = recovered.authorize_start_for(plan.origin()).unwrap();
                start_allowed_at(&global, plan.origin(), state, Some(1), Some(&token)).unwrap();
            } else {
                assert!(recovered.is_err());
                assert!(!root.exists());
                assert_eq!(
                    scopes(&global).unwrap()[0].stop_generation,
                    if advanced { 1 } else { 0 }
                );
            }
        }
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn bootstrap_recovery_rejects_conflict_later_stop_and_overflow() {
        use crate::standalone_install::*;
        for conflict in [false, true] {
            let (_dir, global, plan, reservation) = bootstrap_fixture(true);
            let state = &plan.profile.state_dir;
            record_user_stop_at(&global, state).unwrap();
            write_private_new(
                &stop_ack_path(state, &plan.operation_id).unwrap(),
                &serde_json::to_vec(&bootstrap_ack(&plan)).unwrap(),
                0o600,
            )
            .unwrap();
            let mut entries = scopes(&global).unwrap();
            entries[0].stop_generation = 1;
            write_scopes(&global, &entries).unwrap();
            if conflict {
                let mut bad = entries[0].clone();
                bad.control_token = "e".repeat(64);
                write_markers(&global, &bad).unwrap();
            } else {
                record_user_stop_at(&global, state).unwrap();
            }
            drop(reservation);
            assert!(recover_bootstrap_reservation_at(&global, &plan).is_err());
        }
        let (_dir, global, mut plan, mut reservation) = bootstrap_fixture(true);
        plan.original_stop_generation = u64::MAX;
        let path = plan_path(&plan.profile.state_dir, &plan.operation_id).unwrap();
        write_private_bytes(&path, &serde_json::to_vec(&plan).unwrap(), 0o600).unwrap();
        assert!(reservation
            .acknowledge_bootstrap_stop_record(&bootstrap_ack(&plan))
            .is_err());
        assert!(read_stop_ack(&plan).unwrap().is_none());
        assert_eq!(scopes(&global).unwrap()[0].stop_generation, 0);
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn typed_observation_finds_index_only_standalone_and_rejects_ambiguous_approval() {
        use crate::standalone_install::*;
        let (_dir, global, plan, _reservation) = bootstrap_fixture(true);
        let state = &plan.profile.state_dir;
        fs::remove_file(profile_marker(state).unwrap()).unwrap();
        assert!(matches!(
            observe_latest_operation_at(&global, state).unwrap(),
            Some(OperationObservation::StandaloneInstall(_))
        ));
        let mut record = bootstrap_record(&plan, StandaloneInstallPhase::Completed);
        record.result.status = InstallStatus::Applied;
        record.result.installed = true;
        record.result.applied = true;
        record.result.restart = InstallRestart::Ready;
        let mut installed = plan.original.clone();
        installed.image.sha256 = plan.candidate_image.sha256.clone();
        installed.app_version = plan.candidate_version.clone();
        record.result.installation = Some(installed);
        write_record(state, &record).unwrap();
        // Readiness observation is not settlement or reservation release.
        assert!(
            matches!(observe_latest_operation_at(&global, state).unwrap(),
            Some(OperationObservation::StandaloneInstall(observed)) if observed == record)
        );
        assert!(start_allowed_at(&global, plan.origin(), state, None, None).is_err());
        let normal = private_updates(state)
            .unwrap()
            .join(format!("{}.plan.json", plan.operation_id));
        write_private_new(&normal, b"{}", 0o600).unwrap();
        assert!(observe_latest_operation_at(&global, state).is_err());
        fs::remove_file(&normal).unwrap();
        fs::remove_file(crate::standalone_install::plan_path(state, &plan.operation_id).unwrap())
            .unwrap();
        assert!(observe_latest_operation_at(&global, state).is_err());
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn typed_latest_marker_orders_standalone_then_normal_and_invalid_never_falls_back() {
        use crate::standalone_install::*;
        let (_dir, global, plan, reservation) = bootstrap_fixture(false);
        let state = &plan.profile.state_dir;
        let mut stopped = bootstrap_record(&plan, StandaloneInstallPhase::Completed);
        stopped.result.status = InstallStatus::InstalledStopped;
        stopped.result.installed = true;
        let mut installed = plan.original.clone();
        installed.image.sha256 = plan.candidate_image.sha256.clone();
        installed.app_version = plan.candidate_version.clone();
        stopped.result.installation = Some(installed);
        write_record(state, &stopped).unwrap();
        reservation.release().unwrap();
        assert!(
            matches!(observe_latest_operation_at(&global, state).unwrap(),
            Some(OperationObservation::StandaloneInstall(record)) if record == stopped)
        );
        let record = OperationRecord {
            version: UPDATER_PROTOCOL,
            operation_id: "1123456789abcdef0123456789abcdef".into(),
            phase: OperationPhase::Failed,
            execution_fence: ExecutionFence::NoSpawn,
            detail: "new normal operation".into(),
            installed: false,
            applied: false,
            candidate: None,
        };
        write_operation(state, &record).unwrap();
        assert_eq!(
            observe_latest_operation_at(&global, state).unwrap(),
            Some(OperationObservation::Update(record.clone()))
        );
        let typed = private_updates(state)
            .unwrap()
            .join("latest-managed-operation.json");
        write_private_bytes(&typed, b"{bad", 0o600).unwrap();
        assert!(observe_latest_operation_at(&global, state).is_err());
        write_private_bytes(
            &typed,
            &serde_json::to_vec(&serde_json::json!({
                "version": 2, "kind": "update", "operation_id": record.operation_id,
            }))
            .unwrap(),
            0o600,
        )
        .unwrap();
        assert!(observe_latest_operation_at(&global, state).is_err());
        write_private_bytes(
            &typed,
            &serde_json::to_vec(&serde_json::json!({
                "version": 1, "kind": "standalone_install", "operation_id": record.operation_id,
            }))
            .unwrap(),
            0o600,
        )
        .unwrap();
        assert!(observe_latest_operation_at(&global, state).is_err());
        fs::remove_file(&typed).unwrap();
        assert_eq!(
            observe_latest_operation_at(&global, state).unwrap(),
            Some(OperationObservation::Update(record))
        );
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn stopped_approval_has_no_restart_context_and_cannot_gain_start_authority() {
        use crate::standalone_install::*;
        let (_dir, global, plan, reservation) = bootstrap_fixture(false);
        assert!(plan.running().is_none());
        assert!(reservation.authorize_start_for(plan.origin()).is_err());
        let mut record = bootstrap_record(&plan, StandaloneInstallPhase::Starting);
        record.result.restart = InstallRestart::Pending;
        assert!(write_record(&plan.profile.state_dir, &record).is_err());
        drop(reservation);
        let recovered = recover_bootstrap_reservation_at(&global, &plan).unwrap();
        assert!(recovered.authorize_start_for(plan.origin()).is_err());
        recovered.release().unwrap();
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn standalone_approval_is_immutable_private_bounded_and_rejects_source_context_smuggling() {
        use crate::standalone_install::*;
        let (_dir, _global, plan, _reservation) = bootstrap_fixture(false);
        let state = &plan.profile.state_dir;
        let path = plan_path(state, &plan.operation_id).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(write_plan(&plan).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let mut value = serde_json::to_value(&plan).unwrap();
        value["source"]["context"] = serde_json::json!({"environment": {}});
        assert!(serde_json::from_value::<StandaloneInstallPlan>(value).is_err());
        let duplicate = format!(
            "{{\"version\":1,{}",
            std::str::from_utf8(&bytes[1..]).unwrap()
        );
        write_private_bytes(&path, duplicate.as_bytes(), 0o600).unwrap();
        assert!(read_plan(state, &plan.operation_id).is_err());
        write_private_bytes(&path, &vec![b' '; 65537], 0o600).unwrap();
        assert!(read_plan(state, &plan.operation_id).is_err());
        write_private_bytes(&path, &bytes, 0o600).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_plan(state, &plan.operation_id).is_err());
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn standalone_result_requires_real_candidate_and_preserves_invalid_typed_evidence() {
        use crate::standalone_install::*;
        let (_dir, _global, plan, _reservation) = bootstrap_fixture(false);
        let state = &plan.profile.state_dir;
        let path = record_path(state, &plan.operation_id).unwrap();
        let old = fs::read(&path).unwrap();
        let mut record = bootstrap_record(&plan, StandaloneInstallPhase::Completed);
        record.result.status = InstallStatus::InstalledStopped;
        record.result.installed = true;
        record.result.installation = Some(plan.original.clone());
        assert!(write_record(state, &record).is_err());
        assert_eq!(fs::read(&path).unwrap(), old);
        let marker = private_updates(state)
            .unwrap()
            .join("latest-managed-operation.json");
        write_private_bytes(&marker, b"{bad", 0o600).unwrap();
        assert!(write_record(
            state,
            &bootstrap_record(&plan, StandaloneInstallPhase::Approved)
        )
        .is_err());
        assert_eq!(fs::read(&marker).unwrap(), b"{bad");
        assert_eq!(fs::read(&path).unwrap(), old);
    }

    #[test]
    fn retained_runtime_authority_cannot_absorb_a_later_user_stop() {
        let (dir, global, state, origin) = fixture();
        let image = crate::ExecutableIdentity {
            path: dir.path().join("current-image"),
            sha256: "a".repeat(64),
        };
        check_dir(&state.join("updates-v4"), true).unwrap();
        let receipt = RuntimeStart {
            version: 1,
            root_key: installation_key(&origin).unwrap(),
            pid: std::process::id(),
            instance_id: "current-instance".into(),
            image: image.clone(),
            stop_generation: 0,
        };
        let path = runtime_start_path(&state, &receipt.instance_id);
        write_private_new(&path, &serde_json::to_vec(&receipt).unwrap(), 0o600).unwrap();
        let original = fs::read(&path).unwrap();
        assert_eq!(
            verified_runtime_start_fence_at(
                &global,
                &origin,
                &state,
                receipt.pid,
                &receipt.instance_id,
                &image
            )
            .unwrap(),
            0
        );
        record_user_stop_at(&global, &state).unwrap();
        assert!(verified_runtime_start_fence_at(
            &global,
            &origin,
            &state,
            receipt.pid,
            &receipt.instance_id,
            &image
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn settled_old_update_with_removed_keg_does_not_block_current_owner_gate() {
        let (dir, global, state, origin) = fixture();
        let old = state.join("updates");
        check_dir(&old, true).unwrap();
        let operation = "0123456789abcdef0123456789abcdef";
        let marker = old.join("latest-operation.json");
        write_private_new(&marker, &serde_json::to_vec(operation).unwrap(), 0o600).unwrap();
        for version in [2, 3] {
            let mut record = OperationRecord {
                version,
                operation_id: operation.into(),
                phase: OperationPhase::Completed,
                execution_fence: ExecutionFence::ManagerExited,
                detail: "original completed bridge update".into(),
                installed: true,
                applied: true,
                candidate: Some(crate::ExecutableIdentity {
                    path: dir.path().join(
                        "removed-legacy-keg/HerdrDesktopPet.app/Contents/MacOS/herdr-desktop-pet",
                    ),
                    sha256: "b".repeat(64),
                }),
            };
            let path = old.join(format!("{operation}.result.json"));
            write_private_bytes(&path, &serde_json::to_vec(&record).unwrap(), 0o600).unwrap();
            let bytes = fs::read(&path).unwrap();
            assert!(!record.candidate.as_ref().unwrap().path.exists());
            assert!(start_allowed_at(&global, &origin, &state, None, None).is_ok());
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert_eq!(
                fs::read(&marker).unwrap(),
                serde_json::to_vec(operation).unwrap()
            );
            record.phase = OperationPhase::Unknown;
            write_private_bytes(&path, &serde_json::to_vec(&record).unwrap(), 0o600).unwrap();
            assert!(start_allowed_at(&global, &origin, &state, None, None).is_err());
        }
    }

    #[test]
    fn dropped_reservation_can_recover_while_an_unrelated_fork_is_alive() {
        let (_dir, global, state, origin) = fixture();
        let operation = "0123456789abcdef0123456789abcdef";
        let reservation = reserve_install_at(&global, &origin, None, &state, operation).unwrap();
        let mut pipe = [0; 2];
        assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
        let child = unsafe { libc::fork() };
        assert!(child >= 0);
        if child == 0 {
            // Only async-signal-safe calls after fork in a multi-threaded test.
            unsafe {
                libc::close(pipe[1]);
                let mut byte = 0u8;
                libc::read(pipe[0], (&mut byte as *mut u8).cast(), 1);
                libc::_exit(0);
            }
        }
        unsafe { libc::close(pipe[0]) };
        drop(reservation);
        let recovered = recover_reservation_at(&global, &origin, None, &state, operation);
        // Always reap the child before asserting, including the failing case.
        unsafe {
            libc::close(pipe[1]);
            libc::waitpid(child, std::ptr::null_mut(), 0);
        }
        recovered.unwrap().release().unwrap();
    }

    #[test]
    fn registered_qualified_brew_profile_blocks_root_replacement() {
        let (_dir, global, state, _) = fixture();
        let global = fs::canonicalize(global).unwrap();
        let state = fs::canonicalize(state).unwrap();
        let prefix = global.parent().unwrap().join("brew");
        let root = prefix.join("Cellar").join(crate::identity::STABLE_FORMULA);
        let origin = InstallOrigin::Homebrew {
            brew: prefix.join("bin/brew"),
            prefix: prefix.clone(),
            cellar_formula_root: root.clone(),
            formula: format!("hanbong5938/tap/{}", crate::identity::STABLE_FORMULA),
            locator: prefix
                .join("opt")
                .join(crate::identity::STABLE_FORMULA)
                .join("app"),
        };
        register_active_at(
            &global,
            &origin,
            &state,
            std::process::id(),
            "registered-brew-owner",
            None,
            None,
        )
        .unwrap();
        let unrelated = prefix.join("Cellar/unrelated");
        reject_active_evidence(&global, &[unrelated], None, true).unwrap();
        assert!(reject_active_evidence(&global, &[root], None, true).is_err());
    }

    #[cfg(not(desktop_pet_legacy))]
    fn installation_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let (dir, global, state, _) = fixture();
        let global = fs::canonicalize(global).unwrap();
        let state = fs::canonicalize(state).unwrap();
        let desired = global.parent().unwrap().join("Applications/DesktopPet.app");
        (dir, global, state, desired)
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn waiting_installer_selects_and_fences_the_winner_under_both_gates() {
        use std::sync::mpsc;

        let (_dir, global, _state, desired) = installation_fixture();
        let winner = global.parent().unwrap().join("selected-checkout");
        let (publish, worker_selection) = mpsc::channel::<PathBuf>();
        let first =
            installation_admission_at(global.clone(), || Ok(vec![desired.clone()]), false).unwrap();
        let worker_global = global.clone();
        let worker_desired = desired.clone();
        let (ready, waiting) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            ready.send(()).unwrap();
            let mut selected_root = None;
            let admission = installation_admission_at(
                worker_global.clone(),
                || {
                    for namespace in std::iter::once(worker_global.clone())
                        .chain(old_global(&worker_global).unwrap())
                    {
                        assert!(lock_with_mode(
                            &namespace.join("gate.lock"),
                            libc::LOCK_EX | libc::LOCK_NB,
                        )
                        .is_err());
                    }
                    let root = worker_selection.recv().unwrap();
                    selected_root = Some(root.clone());
                    Ok(vec![worker_desired, root])
                },
                false,
            )
            .unwrap();
            (admission, selected_root.unwrap())
        });
        waiting.recv().unwrap();
        publish.send(winner.clone()).unwrap();
        drop(first);

        let (admission, selection) = worker.join().unwrap();
        assert_eq!(selection, winner);
        for root in [&desired, &winner] {
            assert!(admission.holds_root(root));
            for namespace in std::iter::once(global.clone()).chain(old_global(&global).unwrap()) {
                for key in root_registry_keys(std::slice::from_ref(root)) {
                    assert!(lock_with_mode(
                        &namespace.join(format!("{key}.lock")),
                        libc::LOCK_EX | libc::LOCK_NB,
                    )
                    .is_err());
                }
            }
        }
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn root_refresh_retains_both_gates_and_all_sorted_root_fences() {
        let (_dir, global, _state, desired) = installation_fixture();
        let winner = global.parent().unwrap().join("0-selected-checkout");
        let mut admission =
            installation_admission_at(global.clone(), || Ok(vec![desired.clone()]), false).unwrap();
        assert!(!admission.holds_root(&winner));
        admission.fence_additional_root(&winner).unwrap();
        assert_eq!(admission.roots, vec![winner.clone(), desired.clone()]);
        for namespace in std::iter::once(global.clone()).chain(old_global(&global).unwrap()) {
            assert!(
                lock_with_mode(&namespace.join("gate.lock"), libc::LOCK_EX | libc::LOCK_NB,)
                    .is_err()
            );
            for root in [&desired, &winner] {
                for key in root_registry_keys(std::slice::from_ref(root)) {
                    assert!(lock_with_mode(
                        &namespace.join(format!("{key}.lock")),
                        libc::LOCK_EX | libc::LOCK_NB,
                    )
                    .is_err());
                }
            }
        }
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn newly_selected_root_reservations_in_either_namespace_block_refresh() {
        for legacy in [false, true] {
            let (_dir, global, state, desired) = installation_fixture();
            let winner = global.parent().unwrap().join("selected-checkout");
            let namespace = if legacy {
                old_global(&global).unwrap().unwrap()
            } else {
                global.clone()
            };
            let marker = namespace.join("selected.reservation.json");
            let bytes = serde_json::to_vec(&serde_json::json!({
                "version": if legacy { 3 } else { UPDATER_PROTOCOL },
                "source": {"kind": "source", "root": winner},
                "target": null,
                "state_dir": state,
            }))
            .unwrap();
            write_private_new(&marker, &bytes, 0o600).unwrap();
            let mut admission =
                installation_admission_at(global, || Ok(vec![desired]), false).unwrap();
            assert!(admission.fence_additional_root(&winner).is_err());
            assert_eq!(fs::read(marker).unwrap(), bytes);
        }
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn live_selected_installation_can_be_reused_but_not_replaced() {
        let (_dir, global, state, desired) = installation_fixture();
        let origin = InstallOrigin::Standalone {
            bundle: desired.clone(),
        };
        let entry = ActiveProfile {
            state_dir: state,
            pid: std::process::id(),
            instance_id: "selected-winner".into(),
        };
        for namespace in std::iter::once(global.clone()).chain(old_global(&global).unwrap()) {
            write_private_new(
                &active_path(&namespace, &installation_key(&origin).unwrap()),
                &serde_json::to_vec(&vec![entry.clone()]).unwrap(),
                0o600,
            )
            .unwrap();
        }
        let admission = installation_admission_at(global, || Ok(vec![desired]), false).unwrap();
        assert!(admission.installation_reservations_clear().is_ok());
        assert!(admission
            .installation_inactive()
            .unwrap_err()
            .contains("a live daemon"));
    }

    #[cfg(desktop_pet_legacy)]
    fn herdr_source(state: &Path) -> InstallOrigin {
        let root = state.parent().unwrap();
        InstallOrigin::Herdr {
            host: root.join("host"),
            host_config_dir: root.join("host-config"),
            checkout_root: root.join("checkout"),
            plugin_root: root.join("checkout/plugin"),
            source: "owner/repository".into(),
            requested_ref: Some("beta".into()),
            resolved_commit: Some("a".repeat(40)),
            enabled: true,
        }
    }

    #[cfg(desktop_pet_legacy)]
    fn herdr_commit(source: &InstallOrigin, commit: &str) -> InstallOrigin {
        let mut next = source.clone();
        if let InstallOrigin::Herdr {
            resolved_commit, ..
        } = &mut next
        {
            *resolved_commit = Some(commit.into());
        }
        next
    }

    #[cfg(desktop_pet_legacy)]
    #[test]
    fn targetless_herdr_selects_exact_actual_commit_and_admits_only_its_token() {
        let (_dir, global, state, _) = fixture();
        let source = herdr_source(&state);
        let candidate = herdr_commit(&source, &"b".repeat(40));
        let other_commit = herdr_commit(&source, &"c".repeat(40));
        let op = "0123456789abcdef0123456789abcdef";
        let lease = reserve_install_at(&global, &source, None, &state, op).unwrap();
        let token = lease.authorize_start_for(&candidate).unwrap();
        assert_eq!(
            lease.issued_start_token_for(&candidate).unwrap(),
            Some(token.clone())
        );
        assert!(lease.issued_start_token_for(&source).is_err());
        assert!(lease.authorize_start_for(&other_commit).is_err());
        assert!(lease.authorize_start_for(&source).is_err());
        assert_eq!(
            reservation_snapshot_at(&global, &source, None, &state, op).unwrap(),
            Some(ReservationSnapshot {
                startup_origin: Some(candidate.clone()),
                markers_complete: true,
            })
        );
        start_allowed_at(&global, &candidate, &state, Some(0), Some(&token)).unwrap();
        for origin in [&source, &other_commit] {
            assert!(start_allowed_at(&global, origin, &state, Some(0), Some(&token)).is_err());
        }
        assert!(start_allowed_at(&global, &candidate, &state, None, Some(&token)).is_err());
        assert!(start_allowed_at(&global, &candidate, &state, Some(0), None).is_err());
        assert!(start_allowed_at(&global, &candidate, &state, Some(0), Some("wrong")).is_err());
        lease.release().unwrap();

        let lease = reserve_install_at(&global, &source, None, &state, op).unwrap();
        let old_token = lease.authorize_start_for(&source).unwrap();
        assert_eq!(
            lease.issued_start_token_for(&source).unwrap(),
            Some(old_token.clone())
        );
        assert!(lease.issued_start_token_for(&candidate).is_err());
        assert!(lease.authorize_start_for(&candidate).is_err());
        assert!(start_allowed_at(&global, &candidate, &state, Some(0), Some(&old_token)).is_err());
        lease.release().unwrap();
    }

    #[cfg(desktop_pet_legacy)]
    #[test]
    fn targetless_herdr_rejects_scope_drift_invalid_commit_and_explicit_target_delta() {
        let (_dir, global, state, _) = fixture();
        let source = herdr_source(&state);
        let candidate = herdr_commit(&source, &"b".repeat(40));
        let op = "0123456789abcdef0123456789abcdef";
        let lease = reserve_install_at(&global, &source, None, &state, op).unwrap();
        let mut mutations = Vec::new();
        for field in 0..7 {
            let mut drift = candidate.clone();
            if let InstallOrigin::Herdr {
                host,
                host_config_dir,
                checkout_root,
                plugin_root,
                source,
                requested_ref,
                enabled,
                ..
            } = &mut drift
            {
                match field {
                    0 => *host = host.with_file_name("other-host"),
                    1 => *host_config_dir = host_config_dir.with_file_name("other-config"),
                    2 => *checkout_root = checkout_root.with_file_name("other-checkout"),
                    3 => *plugin_root = plugin_root.with_file_name("other-plugin"),
                    4 => *source = "other/repository".into(),
                    5 => *requested_ref = Some("other-ref".into()),
                    _ => *enabled = false,
                }
            }
            mutations.push(drift);
        }
        mutations.push(herdr_commit(&source, "not-a-commit"));
        mutations.push(herdr_commit(&source, &"g".repeat(40)));
        let mut no_commit = candidate.clone();
        if let InstallOrigin::Herdr {
            resolved_commit, ..
        } = &mut no_commit
        {
            *resolved_commit = None;
        }
        mutations.push(no_commit);
        for drift in &mutations {
            assert!(lease.authorize_start_for(drift).is_err(), "{drift:?}");
        }
        let token = lease.authorize_start_for(&candidate).unwrap();
        for drift in &mutations {
            assert!(start_allowed_at(&global, drift, &state, Some(0), Some(&token)).is_err());
        }
        lease.release().unwrap();

        let lease = reserve_install_at(&global, &source, Some(&candidate), &state, op).unwrap();
        assert!(lease
            .authorize_start_for(&herdr_commit(&source, &"c".repeat(40)))
            .is_err());
        let explicit_token = lease.authorize_start_for(&candidate).unwrap();
        start_allowed_at(&global, &candidate, &state, Some(0), Some(&explicit_token)).unwrap();
        lease.release().unwrap();
    }

    #[cfg(desktop_pet_legacy)]
    #[test]
    fn snapshot_observes_index_before_markers_without_repair_then_reuses_token() {
        let (_dir, global, state, _) = fixture();
        let source = herdr_source(&state);
        let candidate = herdr_commit(&source, &"b".repeat(40));
        let op = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            reservation_snapshot_at(&global, &source, None, &state, op).unwrap(),
            None
        );
        assert!(!state.join(UPDATES_DIR).exists());
        let lease = reserve_install_at(&global, &source, None, &state, op).unwrap();
        assert_eq!(
            reservation_snapshot_at(&global, &source, None, &state, op).unwrap(),
            Some(ReservationSnapshot {
                startup_origin: None,
                markers_complete: true,
            })
        );
        let mut entries = scopes(&global).unwrap();
        let token = new_token().unwrap();
        entries[0].startup_origin = Some(candidate.clone());
        entries[0].startup_token = Some(token.clone());
        write_scopes(&global, &entries).unwrap();
        let profile = profile_marker(&state).unwrap();
        let root_marker = marker_path(&global, &installation_key(&source).unwrap());
        let old_profile = fs::read(&profile).unwrap();
        fs::remove_file(&root_marker).unwrap();
        assert_eq!(
            reservation_snapshot_at(&global, &source, None, &state, op).unwrap(),
            Some(ReservationSnapshot {
                startup_origin: Some(candidate.clone()),
                markers_complete: false,
            })
        );
        assert_eq!(fs::read(&profile).unwrap(), old_profile);
        assert!(!root_marker.exists());
        fs::remove_file(&profile).unwrap();
        assert_eq!(
            reservation_snapshot_at(&global, &source, None, &state, op).unwrap(),
            Some(ReservationSnapshot {
                startup_origin: Some(candidate.clone()),
                markers_complete: false,
            })
        );
        assert!(!profile.exists());
        assert!(start_allowed_at(&global, &candidate, &state, Some(0), Some(&token)).is_err());
        drop(lease);
        let recovered = recover_reservation_at(&global, &source, None, &state, op).unwrap();
        assert_eq!(
            recovered.issued_start_token_for(&candidate).unwrap(),
            Some(token.clone())
        );
        assert!(recovered.authorize_start_for(&candidate).is_err());
        assert_eq!(
            reservation_snapshot_at(&global, &source, None, &state, op).unwrap(),
            Some(ReservationSnapshot {
                startup_origin: Some(candidate.clone()),
                markers_complete: true,
            })
        );
        start_allowed_at(&global, &candidate, &state, Some(0), Some(&token)).unwrap();
        recovered.release().unwrap();
    }

    #[cfg(desktop_pet_legacy)]
    #[test]
    fn snapshot_rejects_conflicting_or_malformed_markers_and_stop_preserves_selection() {
        let (_dir, global, state, _) = fixture();
        let source = herdr_source(&state);
        let candidate = herdr_commit(&source, &"b".repeat(40));
        let other = herdr_commit(&source, &"c".repeat(40));
        let op = "0123456789abcdef0123456789abcdef";
        let lease = reserve_install_at(&global, &source, None, &state, op).unwrap();
        let token = lease.authorize_start_for(&candidate).unwrap();
        drop(lease);
        record_user_stop_at(&global, &state).unwrap();
        let profile = profile_marker(&state).unwrap();
        let original = fs::read(&profile).unwrap();
        let mut conflicting = scopes(&global).unwrap().remove(0);
        conflicting.startup_origin = Some(other);
        let bytes = serde_json::to_vec(&conflicting).unwrap();
        write_private_bytes(&profile, &bytes, 0o600).unwrap();
        assert!(reservation_snapshot_at(&global, &source, None, &state, op).is_err());
        assert!(recover_reservation_at(&global, &source, None, &state, op).is_err());
        assert_eq!(fs::read(&profile).unwrap(), bytes);
        write_private_bytes(&profile, b"{malformed", 0o600).unwrap();
        assert!(reservation_snapshot_at(&global, &source, None, &state, op).is_err());
        assert!(recover_reservation_at(&global, &source, None, &state, op).is_err());
        assert_eq!(fs::read(&profile).unwrap(), b"{malformed");
        write_private_bytes(&profile, &original, 0o600).unwrap();
        let recovered = recover_reservation_at(&global, &source, None, &state, op).unwrap();
        assert!(recovered.user_stop_requested().unwrap());
        assert_eq!(
            reservation_snapshot_at(&global, &source, None, &state, op).unwrap(),
            Some(ReservationSnapshot {
                startup_origin: Some(candidate.clone()),
                markers_complete: true,
            })
        );
        assert!(recovered.issued_start_token_for(&candidate).is_err());
        assert!(recovered.authorize_start_for(&candidate).is_err());
        assert!(start_allowed_at(&global, &candidate, &state, Some(0), Some(&token)).is_err());
        recovered.release().unwrap();
    }

    #[cfg(desktop_pet_legacy)]
    #[test]
    fn user_stop_allows_explicit_partial_marker_recovery_and_safe_release() {
        let (_dir, global, state, _) = fixture();
        let source = herdr_source(&state);
        let candidate = herdr_commit(&source, &"b".repeat(40));
        let op = "0123456789abcdef0123456789abcdef";
        let lease = reserve_install_at(&global, &source, None, &state, op).unwrap();
        let token = lease.authorize_start_for(&candidate).unwrap();
        let generation = lease.stop_generation;
        drop(lease);
        let profile = profile_marker(&state).unwrap();
        let original = fs::read(&profile).unwrap();
        let root_marker = marker_path(&global, &installation_key(&source).unwrap());
        fs::remove_file(&root_marker).unwrap();
        record_user_stop_at(&global, &state).unwrap();
        let selection = Some(ReservationSnapshot {
            startup_origin: Some(candidate.clone()),
            markers_complete: false,
        });
        assert_eq!(
            reservation_snapshot_at(&global, &source, None, &state, op).unwrap(),
            selection
        );
        assert_eq!(fs::read(&profile).unwrap(), original);
        assert!(!root_marker.exists());
        let recovered = recover_reservation_at(&global, &source, None, &state, op).unwrap();
        assert!(recovered.user_stop_requested().unwrap());
        assert_eq!(
            reservation_snapshot_at(&global, &source, None, &state, op).unwrap(),
            Some(ReservationSnapshot {
                startup_origin: Some(candidate.clone()),
                markers_complete: true,
            })
        );
        assert_eq!(
            scopes(&global).unwrap()[0].startup_origin,
            Some(candidate.clone())
        );
        assert!(recovered.issued_start_token_for(&candidate).is_err());
        assert!(recovered.authorize_start_for(&candidate).is_err());
        assert!(
            start_allowed_at(&global, &candidate, &state, Some(generation), Some(&token)).is_err()
        );
        recovered.release().unwrap();
        assert!(!reservation_exists_at(&global, &source, None, &state, op).unwrap());
    }

    #[test]
    fn snapshot_never_treats_orphan_legacy_or_invalid_index_as_absence() {
        let (_dir, global, state, origin) = fixture();
        let op = "0123456789abcdef0123456789abcdef";
        let marker = marker_path(&global, &installation_key(&origin).unwrap());
        write_private_bytes(&marker, b"orphan marker", 0o600).unwrap();
        assert!(reservation_snapshot_at(&global, &origin, None, &state, op).is_err());
        assert_eq!(fs::read(&marker).unwrap(), b"orphan marker");
        fs::remove_file(&marker).unwrap();

        let index = scopes_path(&global);
        write_private_bytes(&index, b"malformed index", 0o600).unwrap();
        assert!(reservation_snapshot_at(&global, &origin, None, &state, op).is_err());
        assert_eq!(fs::read(&index).unwrap(), b"malformed index");
        fs::remove_file(&index).unwrap();

        let prefix = state.parent().unwrap().join("brew");
        let brew = InstallOrigin::Homebrew {
            brew: prefix.join("bin/brew"),
            prefix: prefix.clone(),
            cellar_formula_root: prefix.join("Cellar/herdr-desktop-pet"),
            formula: "hanbong5938/tap/herdr-desktop-pet".into(),
            locator: prefix.join("opt/herdr-desktop-pet/libexec/app"),
        };
        let legacy = marker_path(&global, &legacy_key(&brew).unwrap());
        write_private_bytes(&legacy, b"legacy evidence", 0o600).unwrap();
        assert!(reservation_snapshot_at(&global, &brew, None, &state, op).is_err());
        assert_eq!(fs::read(legacy).unwrap(), b"legacy evidence");
    }

    #[test]
    fn reservation_blocks_other_profiles_and_user_stop_defeats_authorized_start() {
        let (_dir, global, state, origin) = fixture();
        let unknown = InstallOrigin::Unknown {
            reason: "no receipt".into(),
        };
        start_allowed_at(&global, &unknown, &state, None, None).unwrap();
        let id = "0123456789abcdef0123456789abcdef";
        let reservation = reserve_install_at(&global, &origin, None, &state, id).unwrap();
        let different_install = InstallOrigin::Source {
            root: state.parent().unwrap().join("other-checkout"),
        };
        assert!(start_allowed_at(&global, &different_install, &state, None, None).is_err());
        assert!(reserve_install_at(
            &global,
            &different_install,
            None,
            &state,
            "ffffffffffffffffffffffffffffffff"
        )
        .is_err());
        assert!(start_allowed_at(&global, &unknown, &state, None, None).is_err());
        assert!(start_allowed_at(&global, &origin, &state, None, None).is_err());
        assert!(verify_control_at(&global, &origin, None, &state, id, "invalid").is_err());
        verify_control_at(
            &global,
            &origin,
            None,
            &state,
            id,
            reservation.control_token(),
        )
        .unwrap();
        let startup = reservation.authorize_start_for(&origin).unwrap();
        assert!(
            start_allowed_at(&global, &different_install, &state, Some(0), Some(&startup)).is_err()
        );
        start_allowed_at(
            &global,
            &origin,
            &state,
            Some(reservation.stop_generation),
            Some(&startup),
        )
        .unwrap();
        let other = state.parent().unwrap().join("other");
        fs::create_dir(&other).unwrap();
        fs::set_permissions(&other, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(start_allowed_at(&global, &origin, &other, Some(0), Some(&startup)).is_err());
        record_user_stop_at(&global, &state).unwrap();
        assert!(reservation.user_stop_requested().unwrap());
        assert!(reservation.issued_start_token_for(&origin).is_err());
        assert!(start_allowed_at(&global, &origin, &state, Some(0), Some(&startup)).is_err());
        assert!(verify_control_at(
            &global,
            &origin,
            None,
            &state,
            id,
            reservation.control_token()
        )
        .is_err());
        reservation.release().unwrap();
        start_allowed_at(&global, &origin, &state, None, None).unwrap();
    }

    #[test]
    fn other_live_profile_prevents_physical_replacement() {
        let (_dir, global, state, origin) = fixture();
        let other = state.parent().unwrap().join("other");
        fs::create_dir(&other).unwrap();
        fs::set_permissions(&other, fs::Permissions::from_mode(0o700)).unwrap();
        register_active_at(
            &global,
            &origin,
            &other,
            std::process::id(),
            "other-instance",
            None,
            None,
        )
        .unwrap();
        let id = "0123456789abcdef0123456789abcdef";
        assert!(reserve_install_at(&global, &origin, None, &state, id).is_err());
        assert!(active(&global, &installation_key(&origin).unwrap())
            .unwrap()
            .iter()
            .any(|e| e.state_dir == other));
    }

    #[test]
    fn crashed_owner_keeps_reservation_until_explicit_recovery() {
        let (_dir, global, state, origin) = fixture();
        let id = "0123456789abcdef0123456789abcdef";
        let lease = reserve_install_at(&global, &origin, None, &state, id).unwrap();
        lease.authorize_start_for(&origin).unwrap();
        drop(lease);
        fs::remove_file(global.join(format!(
            "{}.reservation.json",
            installation_key(&origin).unwrap()
        )))
        .unwrap();
        let different_install = InstallOrigin::Source {
            root: state.parent().unwrap().join("other-checkout"),
        };
        assert!(recover_reservation_at(&global, &different_install, None, &state, id).is_err());
        assert!(start_allowed_at(&global, &origin, &state, None, None).is_err());
        assert!(recover_reservation_at(
            &global,
            &origin,
            None,
            &state,
            "ffffffffffffffffffffffffffffffff"
        )
        .is_err());
        let recovered = recover_reservation_at(&global, &origin, None, &state, id).unwrap();
        let startup = recovered.issued_start_token_for(&origin).unwrap().unwrap();
        start_allowed_at(&global, &origin, &state, Some(0), Some(&startup)).unwrap();
        assert!(recovered.authorize_start_for(&origin).is_err());
        recovered.release().unwrap();
        start_allowed_at(&global, &origin, &state, None, None).unwrap();
    }

    #[test]
    fn partial_start_authorization_can_resume_without_second_issue() {
        let (_dir, global, state, origin) = fixture();
        let id = "0123456789abcdef0123456789abcdef";
        let lease = reserve_install_at(&global, &origin, None, &state, id).unwrap();
        lease.authorize_start_for(&origin).unwrap();
        let profile = profile_marker(&state).unwrap();
        let mut interrupted: Reservation =
            serde_json::from_slice(&owned_private_file(&profile, 16384).unwrap()).unwrap();
        interrupted.startup_origin = None;
        interrupted.startup_token = None;
        write_private_bytes(&profile, &serde_json::to_vec(&interrupted).unwrap(), 0o600).unwrap();
        assert!(start_allowed_at(&global, &origin, &state, Some(0), None).is_err());
        drop(lease);
        let recovered = recover_reservation_at(&global, &origin, None, &state, id).unwrap();
        let token = recovered.issued_start_token_for(&origin).unwrap().unwrap();
        start_allowed_at(&global, &origin, &state, Some(0), Some(&token)).unwrap();
        assert!(recovered.authorize_start_for(&origin).is_err());
        recovered.release().unwrap();
    }

    #[test]
    fn reconnect_never_reports_old_success_over_reserved_work() {
        let (_dir, global, state, origin) = fixture();
        assert!(latest_operation(&state).unwrap().is_none());
        assert!(!state.join(UPDATES_DIR).exists());
        let completed = OperationRecord {
            version: UPDATER_PROTOCOL,
            operation_id: "0123456789abcdef0123456789abcdef".into(),
            phase: OperationPhase::Completed,
            execution_fence: ExecutionFence::ManagerExited,
            detail: "previous operation".into(),
            installed: true,
            applied: true,
            candidate: None,
        };
        write_operation(&state, &completed).unwrap();
        let id = "ffffffffffffffffffffffffffffffff";
        let lease = reserve_install_at(&global, &origin, None, &state, id).unwrap();
        assert!(latest_operation(&state).is_err());
        let mut active = OperationRecord {
            version: UPDATER_PROTOCOL,
            operation_id: id.into(),
            phase: OperationPhase::Preparing,
            execution_fence: ExecutionFence::NoSpawn,
            detail: "new operation".into(),
            installed: false,
            applied: false,
            candidate: None,
        };
        write_operation(&state, &active).unwrap();
        write_operation(&state, &completed).unwrap();
        assert_eq!(
            latest_operation(&state).unwrap().unwrap().phase,
            OperationPhase::Preparing
        );
        active.phase = OperationPhase::Failed;
        active.detail = "preparation rejected before mutation".into();
        write_operation(&state, &active).unwrap();
        lease.release().unwrap();
        assert_eq!(
            latest_operation(&state).unwrap().unwrap().phase,
            OperationPhase::Failed
        );
    }

    #[test]
    fn private_journal_rejects_symlink_in_parent_and_target() {
        let (_dir, _global, state, _origin) = fixture();
        let id = "0123456789abcdef0123456789abcdef";
        let record = OperationRecord {
            version: UPDATER_PROTOCOL,
            operation_id: id.into(),
            phase: OperationPhase::Unknown,
            execution_fence: ExecutionFence::NoSpawn,
            detail: "recover".into(),
            installed: false,
            applied: false,
            candidate: None,
        };
        let outside = state.parent().unwrap().join("outside");
        fs::write(&outside, b"keep").unwrap();
        symlink(&outside, state.join(UPDATES_DIR)).unwrap();
        assert!(write_operation(&state, &record).is_err());
        fs::remove_file(state.join(UPDATES_DIR)).unwrap();
        private_updates(&state).unwrap();
        symlink(
            &outside,
            state.join(UPDATES_DIR).join(format!("{id}.result.json")),
        )
        .unwrap();
        assert!(read_operation(&state, id).is_err());
        assert!(write_operation(&state, &record).is_err());
        assert_eq!(fs::read(outside).unwrap(), b"keep");
    }

    #[test]
    fn immutable_plan_file_cannot_be_replaced_by_same_operation() {
        let (_dir, _global, state, _origin) = fixture();
        let file = private_updates(&state).unwrap().join("plan.json");
        write_private_new(&file, b"original", 0o600).unwrap();
        assert!(write_private_new(&file, b"changed", 0o600).is_err());
        assert_eq!(fs::read(file).unwrap(), b"original");
    }

    #[test]
    fn operation_lease_is_nonblocking_and_retained_until_release() {
        let (_dir, _global, state, _origin) = fixture();
        let id = "0123456789abcdef0123456789abcdef";
        let held = lock_operation(&state, id).unwrap();
        assert!(lock_operation(&state, id).is_err());
        drop(held);
        lock_operation(&state, id).unwrap();
    }

    #[test]
    fn formula_reservations_cover_all_kegs_but_not_other_formula() {
        let (_dir, global, state, _) = fixture();
        let prefix = state.parent().unwrap().join("brew");
        let stable = InstallOrigin::Homebrew {
            brew: prefix.join("bin/brew"),
            prefix: prefix.clone(),
            cellar_formula_root: prefix.join("Cellar/herdr-desktop-pet"),
            formula: "hanbong5938/tap/herdr-desktop-pet".into(),
            locator: prefix.join("opt/herdr-desktop-pet/libexec/app"),
        };
        let mut next_keg = stable.clone();
        if let InstallOrigin::Homebrew { locator, .. } = &mut next_keg {
            *locator = locator.with_file_name("next-keg");
        }
        let beta = InstallOrigin::Homebrew {
            brew: prefix.join("bin/brew"),
            prefix: prefix.clone(),
            cellar_formula_root: prefix.join("Cellar/herdr-desktop-pet-beta"),
            formula: "hanbong5938/tap/herdr-desktop-pet-beta".into(),
            locator: prefix.join("opt/herdr-desktop-pet-beta/libexec/app"),
        };
        let other = state.parent().unwrap().join("other-profile");
        fs::create_dir(&other).unwrap();
        fs::set_permissions(&other, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            installation_key(&stable).unwrap(),
            installation_key(&next_keg).unwrap()
        );
        assert_ne!(
            installation_key(&stable).unwrap(),
            installation_key(&beta).unwrap()
        );
        let stable_lease = reserve_install_at(
            &global,
            &stable,
            None,
            &state,
            "0123456789abcdef0123456789abcdef",
        )
        .unwrap();
        assert!(reserve_install_at(
            &global,
            &next_keg,
            None,
            &other,
            "ffffffffffffffffffffffffffffffff"
        )
        .is_err());
        let beta_lease = reserve_install_at(
            &global,
            &beta,
            None,
            &other,
            "ffffffffffffffffffffffffffffffff",
        )
        .unwrap();
        beta_lease.release().unwrap();
        stable_lease.release().unwrap();
    }

    #[test]
    fn dual_scope_blocks_both_roots_and_only_selected_origin_can_start() {
        let (_dir, global, state, source) = fixture();
        let target = InstallOrigin::Source {
            root: state.parent().unwrap().join("beta-checkout"),
        };
        let other = state.parent().unwrap().join("other-profile");
        fs::create_dir(&other).unwrap();
        fs::set_permissions(&other, fs::Permissions::from_mode(0o700)).unwrap();
        let id = "0123456789abcdef0123456789abcdef";
        let lease = reserve_install_at(&global, &source, Some(&target), &state, id).unwrap();
        let entries = scopes(&global).unwrap();
        assert_eq!(
            entries[0].keys,
            installation_keys(&source, Some(&target)).unwrap()
        );
        assert_eq!(entries[0].source, source);
        assert_eq!(entries[0].target.as_ref(), Some(&target));
        assert!(start_allowed_at(&global, &source, &other, None, None).is_err());
        assert!(start_allowed_at(&global, &target, &other, None, None).is_err());
        assert!(reserve_install_at(&global, &target, Some(&source), &other, id).is_err());
        assert!(
            verify_control_at(&global, &source, None, &state, id, lease.control_token()).is_err()
        );
        verify_control_at(
            &global,
            &source,
            Some(&target),
            &state,
            id,
            lease.control_token(),
        )
        .unwrap();
        let token = lease.authorize_start_for(&target).unwrap();
        assert!(lease.issued_start_token_for(&source).is_err());
        assert!(start_allowed_at(&global, &source, &state, Some(0), Some(&token)).is_err());
        start_allowed_at(&global, &target, &state, Some(0), Some(&token)).unwrap();
        lease.release().unwrap();
        assert!(start_allowed_at(&global, &target, &state, Some(0), Some(&token)).is_err());
        start_allowed_at(&global, &source, &other, None, None).unwrap();
        start_allowed_at(&global, &target, &other, None, None).unwrap();
    }

    #[test]
    fn dual_scope_restore_selects_original_without_unblocking_target() {
        let (_dir, global, state, source) = fixture();
        let target = InstallOrigin::Source {
            root: state.parent().unwrap().join("beta-target"),
        };
        let id = "0123456789abcdef0123456789abcdef";
        let lease = reserve_install_at(&global, &source, Some(&target), &state, id).unwrap();
        let token = lease.authorize_start_for(&source).unwrap();
        start_allowed_at(&global, &source, &state, Some(0), Some(&token)).unwrap();
        assert!(start_allowed_at(&global, &target, &state, Some(0), Some(&token)).is_err());
        record_user_stop_at(&global, &state).unwrap();
        assert!(start_allowed_at(&global, &source, &state, Some(0), Some(&token)).is_err());
        lease.release().unwrap();
    }

    #[test]
    fn live_profile_on_proposed_target_prevents_replacement() {
        let (_dir, global, state, source) = fixture();
        let target = InstallOrigin::Source {
            root: state.parent().unwrap().join("beta-checkout"),
        };
        let other = state.parent().unwrap().join("other-profile");
        fs::create_dir(&other).unwrap();
        fs::set_permissions(&other, fs::Permissions::from_mode(0o700)).unwrap();
        register_active_at(
            &global,
            &target,
            &other,
            std::process::id(),
            "target-instance",
            None,
            None,
        )
        .unwrap();
        let id = "0123456789abcdef0123456789abcdef";
        assert!(reserve_install_at(&global, &source, Some(&target), &state, id).is_err());
        assert!(scopes(&global).unwrap().is_empty());
        assert!(!profile_marker(&state).unwrap().exists());
    }

    #[test]
    fn same_key_dedup_still_requires_exact_selected_origin() {
        let (_dir, global, state, _) = fixture();
        let root = state.parent().unwrap().join("brew");
        let source = InstallOrigin::Homebrew {
            brew: root.join("bin/brew"),
            prefix: root.clone(),
            cellar_formula_root: root.join("Cellar/herdr"),
            formula: "hanbong5938/tap/herdr-desktop-pet".into(),
            locator: root.join("opt/herdr/old"),
        };
        let mut target = source.clone();
        if let InstallOrigin::Homebrew { locator, .. } = &mut target {
            *locator = root.join("opt/herdr/new");
        }
        let id = "0123456789abcdef0123456789abcdef";
        let lease = reserve_install_at(&global, &source, Some(&target), &state, id).unwrap();
        assert_eq!(scopes(&global).unwrap()[0].keys.len(), 1);
        let token = lease.authorize_start_for(&target).unwrap();
        assert!(start_allowed_at(&global, &source, &state, Some(0), Some(&token)).is_err());
        start_allowed_at(&global, &target, &state, Some(0), Some(&token)).unwrap();
        lease.release().unwrap();
    }

    #[test]
    fn partial_dual_markers_keep_both_roots_gated_and_recover_from_index() {
        let (_dir, global, state, source) = fixture();
        let target = InstallOrigin::Source {
            root: state.parent().unwrap().join("target"),
        };
        let id = "0123456789abcdef0123456789abcdef";
        let lease = reserve_install_at(&global, &source, Some(&target), &state, id).unwrap();
        drop(lease);
        fs::remove_file(profile_marker(&state).unwrap()).unwrap();
        fs::remove_file(marker_path(&global, &installation_key(&target).unwrap())).unwrap();
        fs::remove_file(marker_path(&global, &installation_key(&source).unwrap())).unwrap();
        assert!(reservation_exists_at(&global, &source, Some(&target), &state, id).unwrap());
        let other = state.parent().unwrap().join("other-profile");
        fs::create_dir(&other).unwrap();
        fs::set_permissions(&other, fs::Permissions::from_mode(0o700)).unwrap();
        for origin in [&source, &target] {
            assert!(start_allowed_at(&global, origin, &state, None, None).is_err());
            assert!(start_allowed_at(&global, origin, &other, None, None).is_err());
            assert!(reserve_install_at(&global, origin, None, &other, id).is_err());
        }
        assert!(recover_reservation_at(&global, &source, None, &state, id).is_err());
        let recovered =
            recover_reservation_at(&global, &source, Some(&target), &state, id).unwrap();
        assert!(markers_match(&global, &scopes(&global).unwrap()[0]).unwrap());
        recovered.release().unwrap();
        assert!(!reservation_exists_at(&global, &source, Some(&target), &state, id).unwrap());
    }

    #[test]
    fn partial_dual_authorization_never_admits_until_recovered() {
        let (_dir, global, state, source) = fixture();
        let target = InstallOrigin::Source {
            root: state.parent().unwrap().join("target"),
        };
        let id = "0123456789abcdef0123456789abcdef";
        let lease = reserve_install_at(&global, &source, Some(&target), &state, id).unwrap();
        let token = lease.authorize_start_for(&target).unwrap();
        fs::remove_file(marker_path(&global, &installation_key(&source).unwrap())).unwrap();
        assert!(start_allowed_at(&global, &target, &state, Some(0), Some(&token)).is_err());
        drop(lease);
        let recovered =
            recover_reservation_at(&global, &source, Some(&target), &state, id).unwrap();
        assert_eq!(
            recovered.issued_start_token_for(&target).unwrap(),
            Some(token.clone())
        );
        assert!(recovered.issued_start_token_for(&source).is_err());
        start_allowed_at(&global, &target, &state, Some(0), Some(&token)).unwrap();
        recovered.release().unwrap();
    }

    #[test]
    fn legacy_brew_key_and_version_one_markers_block_without_rewriting_bytes() {
        let (_dir, global, state, _origin) = fixture();
        let prefix = state.parent().unwrap().join("brew");
        let origin = InstallOrigin::Homebrew {
            brew: prefix.join("bin/brew"),
            prefix: prefix.clone(),
            cellar_formula_root: prefix.join("Cellar/herdr-desktop-pet"),
            formula: "hanbong5938/tap/herdr-desktop-pet".into(),
            locator: prefix.join("opt/herdr-desktop-pet/app"),
        };
        assert_ne!(
            installation_key(&origin).unwrap(),
            legacy_key(&origin).unwrap()
        );
        let old_lock = global.join(format!("{}.lock", legacy_key(&origin).unwrap()));
        let old_owner = lock_with_mode(&old_lock, libc::LOCK_EX | libc::LOCK_NB).unwrap();
        let id = "0123456789abcdef0123456789abcdef";
        assert!(reserve_install_at(&global, &origin, None, &state, id).is_err());
        drop(old_owner);
        let lease = reserve_install_at(&global, &origin, None, &state, id).unwrap();
        assert!(lock_with_mode(&old_lock, libc::LOCK_EX | libc::LOCK_NB).is_err());
        lease.release().unwrap();
        let old = global.join(format!("{}.reservation.json", legacy_key(&origin).unwrap()));
        let old_bytes = br#"{"operation_id":"0123456789abcdef0123456789abcdef","version":1}"#;
        write_private_new(&old, old_bytes, 0o600).unwrap();
        assert!(start_allowed_at(&global, &origin, &state, None, None).is_err());
        assert!(reserve_install_at(&global, &origin, None, &state, id).is_err());
        assert!(recover_reservation_at(&global, &origin, None, &state, id).is_err());
        assert_eq!(fs::read(&old).unwrap(), old_bytes);
        let (_other_dir, other_global, other_state, other_origin) = fixture();
        let marker = profile_marker(&other_state).unwrap();
        write_private_new(&marker, old_bytes, 0o600).unwrap();
        assert!(start_allowed_at(&other_global, &other_origin, &other_state, None, None).is_err());
        assert!(reserve_install_at(&other_global, &other_origin, None, &other_state, id).is_err());
        assert!(latest_operation(&other_state).is_err());
        assert_eq!(fs::read(marker).unwrap(), old_bytes);
    }

    #[test]
    fn version_one_journal_and_corrupt_latest_cannot_be_overwritten() {
        let (_dir, _global, state, _origin) = fixture();
        let id = "0123456789abcdef0123456789abcdef";
        let journal = private_updates(&state)
            .unwrap()
            .join(format!("{id}.result.json"));
        let old = br#"{"version":1,"operation_id":"0123456789abcdef0123456789abcdef","phase":"preparing","detail":"old","installed":false,"applied":false,"candidate":null}"#;
        write_private_new(&journal, old, 0o600).unwrap();
        let record = OperationRecord {
            version: UPDATER_PROTOCOL,
            operation_id: id.into(),
            phase: OperationPhase::Preparing,
            execution_fence: ExecutionFence::NoSpawn,
            detail: "new".into(),
            installed: false,
            applied: false,
            candidate: None,
        };
        assert!(write_operation(&state, &record).is_err());
        assert_eq!(fs::read(&journal).unwrap(), old);
        assert!(read_operation(&state, id).is_err());
    }

    #[test]
    fn protocol_two_evidence_stays_blocked_and_byte_preserved() {
        let (_dir, global, state, source) = fixture();
        let target = InstallOrigin::Source {
            root: state.parent().unwrap().join("beta-target"),
        };
        let id = "0123456789abcdef0123456789abcdef";
        let old_marker = br#"{"version":2,"operation_id":"0123456789abcdef0123456789abcdef","installation_key":"legacy","state_dir":"/legacy","stop_generation":0,"control_token":"legacy","startup_token":null}"#;
        let target_marker = marker_path(&global, &installation_key(&target).unwrap());
        write_private_new(&target_marker, old_marker, 0o600).unwrap();
        assert!(start_allowed_at(&global, &target, &state, None, None).is_err());
        assert!(reservation_exists_at(&global, &source, Some(&target), &state, id).is_err());
        assert!(reserve_install_at(&global, &source, Some(&target), &state, id).is_err());
        assert!(recover_reservation_at(&global, &source, Some(&target), &state, id).is_err());
        assert_eq!(fs::read(&target_marker).unwrap(), old_marker);

        let old_journal = br#"{"version":2,"operation_id":"0123456789abcdef0123456789abcdef","phase":"preparing","execution_fence":"no_spawn","detail":"old","installed":false,"applied":false,"candidate":null}"#;
        let journal = private_updates(&state)
            .unwrap()
            .join(format!("{id}.result.json"));
        write_private_new(&journal, old_journal, 0o600).unwrap();
        let record = OperationRecord {
            version: UPDATER_PROTOCOL,
            operation_id: id.into(),
            phase: OperationPhase::Preparing,
            execution_fence: ExecutionFence::NoSpawn,
            detail: "new".into(),
            installed: false,
            applied: false,
            candidate: None,
        };
        assert!(read_operation(&state, id).is_err());
        assert!(write_operation(&state, &record).is_err());
        assert_eq!(fs::read(&journal).unwrap(), old_journal);
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn historical_profile_admission_requires_a_complete_settled_journal() {
        let (_dir, global, state, origin) = fixture();
        let updates = state.join("updates");
        check_dir(&updates, true).unwrap();
        let id = "0123456789abcdef0123456789abcdef";
        let journal = updates.join(format!("{id}.result.json"));
        let latest = updates.join("latest-operation.json");
        let completed = OperationRecord {
            version: 2,
            operation_id: id.into(),
            phase: OperationPhase::Completed,
            execution_fence: ExecutionFence::ManagerExited,
            detail: "retained historical installation".into(),
            installed: true,
            applied: true,
            candidate: None,
        };
        let original = serde_json::to_vec_pretty(&completed).unwrap();
        let pointer = serde_json::to_vec(id).unwrap();
        write_private_new(&journal, &original, 0o600).unwrap();
        write_private_new(&latest, &pointer, 0o600).unwrap();
        assert!(latest_operation(&state).unwrap().is_none());
        assert!(!state.join(UPDATES_DIR).exists());
        start_allowed_at(&global, &origin, &state, None, None).unwrap();
        assert_eq!(fs::read(&journal).unwrap(), original);
        assert_eq!(fs::read(&latest).unwrap(), pointer);

        for (key, value) in [
            ("version", serde_json::json!(UPDATER_PROTOCOL)),
            ("operation_id", serde_json::json!("b".repeat(32))),
            ("phase", serde_json::json!("unknown")),
            ("phase", serde_json::json!("installing")),
            ("execution_fence", serde_json::json!("manager_intent")),
            ("applied", serde_json::json!(false)),
            ("installed", serde_json::json!(false)),
            ("unrecognized", serde_json::json!(true)),
        ] {
            let mut invalid = serde_json::to_value(&completed).unwrap();
            invalid[key] = value;
            let bytes = serde_json::to_vec(&invalid).unwrap();
            write_private_bytes(&journal, &bytes, 0o600).unwrap();
            assert!(latest_operation(&state).is_err(), "{key}: {invalid}");
            assert!(start_allowed_at(&global, &origin, &state, None, None).is_err());
            assert_eq!(fs::read(&journal).unwrap(), bytes);
            assert_eq!(fs::read(&latest).unwrap(), pointer);
        }
        write_private_bytes(&journal, &original, 0o600).unwrap();
        let marker = updates.join("profile.reservation.json");
        let reserved = br#"{"version":2}"#;
        write_private_new(&marker, reserved, 0o600).unwrap();
        assert!(latest_operation(&state).is_err());
        assert!(start_allowed_at(&global, &origin, &state, None, None).is_err());
        assert_eq!(fs::read(&marker).unwrap(), reserved);
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn old_scope_blocks_neutral_and_remains_byte_identical() {
        let (_dir, global, state, origin) = fixture();
        let old = old_global(&global).unwrap().unwrap();
        let evidence = serde_json::json!([{
            "version": 3,
            "operation_id": "0123456789abcdef0123456789abcdef",
            "source": {"kind":"local", "root": crate::physical_install_root(&origin).unwrap()},
            "target": null,
            "state_dir": state,
            "keys": [installation_key(&origin).unwrap()]
        }]);
        let bytes = serde_json::to_vec(&evidence).unwrap();
        write_private_bytes(&scopes_path(&old), &bytes, 0o600).unwrap();
        let id = "abcdef0123456789abcdef0123456789";
        assert!(start_allowed_at(&global, &origin, &state, None, None).is_err());
        assert!(reserve_install_at(&global, &origin, None, &state, id).is_err());
        assert!(recover_reservation_at(&global, &origin, None, &state, id).is_err());
        assert!(verify_control_at(&global, &origin, None, &state, id, "token").is_err());
        assert_eq!(fs::read(scopes_path(&old)).unwrap(), bytes);
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn old_registration_and_each_namespace_stop_generation_gate_neutral() {
        let (_dir, global, state, origin) = fixture();
        let old = old_global(&global).unwrap().unwrap();
        let old_updates = state.join("updates");
        check_dir(&old_updates, true).unwrap();
        write_private_bytes(&old_updates.join("stop-generation.json"), b"7", 0o600).unwrap();
        assert_eq!(generation(&state).unwrap(), 7);
        assert_eq!(record_user_stop_at(&global, &state).unwrap(), 8);
        assert_eq!(
            fs::read(old_updates.join("stop-generation.json")).unwrap(),
            b"7"
        );
        write_private_bytes(&old_updates.join("stop-generation.json"), b"8", 0o600).unwrap();
        assert!(start_allowed_at(&global, &origin, &state, Some(8), None).is_err());
        let profiles = vec![ActiveProfile {
            state_dir: state.clone(),
            pid: std::process::id(),
            instance_id: "legacy".into(),
        }];
        write_private_bytes(
            &active_path(&old, &installation_key(&origin).unwrap()),
            &serde_json::to_vec(&profiles).unwrap(),
            0o600,
        )
        .unwrap();
        assert!(start_allowed_at(&global, &origin, &state, None, None).is_err());
        assert!(register_active_at(
            &global,
            &origin,
            &state,
            std::process::id(),
            "neutral",
            None,
            None
        )
        .is_err());
    }

    #[test]
    fn durable_migration_scope_blocks_normal_paths_and_honors_stop_without_reencoding() {
        let (_dir, global, state, origin) = fixture();
        let operation = "0123456789abcdef0123456789abcdef";
        let token = "a".repeat(64);
        let root = crate::physical_install_root(&origin).unwrap().to_path_buf();
        let scope = MigrationScope {
            version: 4,
            operation_id: operation.into(),
            roots: vec![root.clone()],
            profiles: vec![MigrationProfile {
                pid: std::process::id(),
                instance_id: "authenticated-bridge".into(),
                state_dir: state.clone(),
                config_dir: state.clone(),
                image: crate::ExecutableIdentity {
                    path: root.join("HerdrDesktopPet.app/Contents/MacOS/herdr-desktop-pet"),
                    sha256: "b".repeat(64),
                },
                stop_generation: 0,
            }],
            readonly_target_profiles: Vec::new(),
            owner_token: token.clone(),
            startups: Vec::new(),
        };
        let namespace = migration_global(&global).unwrap();
        let path = migration_path(&namespace, operation);
        let bytes = serde_json::to_vec(&scope).unwrap();
        write_private_new(&path, &bytes, 0o600).unwrap();
        assert!(start_allowed_at(&global, &origin, &state, None, None).is_err());
        assert!(reserve_install_at(&global, &origin, None, &state, operation).is_err());
        assert!(recover_reservation_at(&global, &origin, None, &state, operation).is_err());
        assert!(reject_reservation_evidence(&global, &[root], Some(&state)).is_err());
        let reservation = MigrationReservation {
            global: global.clone(),
            operation_id: operation.into(),
            owner_token: token,
        };
        assert!(!reservation.stop_requested().unwrap());
        record_user_stop_at(&global, &state).unwrap();
        assert!(reservation.stop_requested().unwrap());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        reservation.release().unwrap();
        assert!(!path.exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn readonly_target_reuse_keeps_its_live_instance_and_only_source_stop_fences_migration() {
        let (_dir, global, state, source) = fixture();
        let readonly_state = state.parent().unwrap().join("existing-neutral-profile");
        check_dir(&readonly_state, true).unwrap();
        let image = crate::executable_identity(&std::env::current_exe().unwrap()).unwrap();
        let target_root = image.path.parent().unwrap().to_path_buf();
        let source_root = crate::physical_install_root(&source).unwrap().to_path_buf();
        let target_origin = InstallOrigin::Source {
            root: target_root.clone(),
        };
        let operation = "0123456789abcdef0123456789abcdef";
        let token = "d".repeat(64);
        let mut roots = vec![source_root.clone(), target_root];
        roots.sort_unstable();
        let readonly = MigrationProfile {
            pid: std::process::id(),
            instance_id: "existing-neutral".into(),
            state_dir: readonly_state.clone(),
            config_dir: readonly_state.clone(),
            image,
            stop_generation: 0,
        };
        let scope = MigrationScope {
            version: 4,
            operation_id: operation.into(),
            roots,
            profiles: vec![MigrationProfile {
                pid: 1,
                instance_id: "bridge".into(),
                state_dir: state.clone(),
                config_dir: state.clone(),
                image: crate::ExecutableIdentity {
                    path: source_root.join("HerdrDesktopPet.app/Contents/MacOS/herdr-desktop-pet"),
                    sha256: "b".repeat(64),
                },
                stop_generation: 0,
            }],
            readonly_target_profiles: vec![readonly.clone()],
            owner_token: token.clone(),
            startups: Vec::new(),
        };
        assert_eq!(migration_source_roots(&scope).unwrap(), vec![source_root]);
        readonly_target_profile_checks(&scope).unwrap();
        let namespace = migration_global(&global).unwrap();
        write_private_new(
            &migration_path(&namespace, operation),
            &serde_json::to_vec(&scope).unwrap(),
            0o600,
        )
        .unwrap();
        write_private_bytes(
            &active_path(&global, &installation_key(&target_origin).unwrap()),
            &serde_json::to_vec(&vec![ActiveProfile {
                state_dir: readonly_state.clone(),
                pid: readonly.pid,
                instance_id: readonly.instance_id.clone(),
            }])
            .unwrap(),
            0o600,
        )
        .unwrap();
        start_allowed_at(&global, &target_origin, &readonly_state, None, None).unwrap();
        assert!(register_active_at(
            &global,
            &target_origin,
            &readonly_state,
            readonly.pid + 1,
            "replacement-instance",
            None,
            None
        )
        .is_err());
        record_user_stop_at(&global, &readonly_state).unwrap();
        let reservation = MigrationReservation {
            global: global.clone(),
            operation_id: operation.into(),
            owner_token: token,
        };
        assert!(!reservation.stop_requested().unwrap());
        readonly_target_profile_checks(&scope).unwrap();
        record_user_stop_at(&global, &state).unwrap();
        assert!(reservation.stop_requested().unwrap());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn committed_migration_sdk_reuses_only_exact_live_instance_without_releasing_owner() {
        let (_dir, global, state, source) = fixture();
        let operation = "0123456789abcdef0123456789abcdef";
        let image = crate::executable_identity(&std::env::current_exe().unwrap()).unwrap();
        let target_root = image.path.parent().unwrap().to_path_buf();
        let target = InstallOrigin::Source {
            root: target_root.clone(),
        };
        let source_root = crate::physical_install_root(&source).unwrap().to_path_buf();
        let instance = ActiveProfile {
            state_dir: state.clone(),
            pid: std::process::id(),
            instance_id: "committed-target".into(),
        };
        let mut scope = MigrationScope {
            version: 4,
            operation_id: operation.into(),
            roots: vec![source_root.clone(), target_root.clone()],
            profiles: vec![MigrationProfile {
                pid: 1,
                instance_id: "stopped-source".into(),
                state_dir: state.clone(),
                config_dir: state.clone(),
                image: crate::ExecutableIdentity {
                    path: source_root.join("HerdrDesktopPet.app/Contents/MacOS/herdr-desktop-pet"),
                    sha256: "b".repeat(64),
                },
                stop_generation: 0,
            }],
            readonly_target_profiles: Vec::new(),
            owner_token: "a".repeat(64),
            startups: vec![MigrationStartup {
                root: target_root,
                image,
                state_dir: state.clone(),
                stop_generation: 0,
                token: "c".repeat(64),
                committed_instance: None,
            }],
        };
        scope.roots.sort_unstable();
        let path = migration_path(&migration_global(&global).unwrap(), operation);
        write_private_new(&path, &serde_json::to_vec(&scope).unwrap(), 0o600).unwrap();
        write_private_bytes(
            &active_path(&global, &installation_key(&target).unwrap()),
            &serde_json::to_vec(&vec![instance.clone()]).unwrap(),
            0o600,
        )
        .unwrap();
        assert!(start_allowed_at(&global, &target, &state, None, None).is_err());
        scope.startups[0].committed_instance = Some(instance.clone());
        let committed = serde_json::to_vec(&scope).unwrap();
        write_private_bytes(&path, &committed, 0o600).unwrap();
        start_allowed_at(&global, &target, &state, None, None).unwrap();
        assert!(reserve_install_at(&global, &source, Some(&target), &state, operation).is_err());
        assert!(register_active_at(
            &global,
            &target,
            &state,
            instance.pid,
            "replacement-instance",
            None,
            None
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), committed);
        record_user_stop_at(&global, &state).unwrap();
        assert!(start_allowed_at(&global, &target, &state, None, None).is_err());
        assert_eq!(fs::read(&path).unwrap(), committed);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cleanup_requires_the_exact_live_process_image_without_global_census() {
        let image = crate::executable_identity(&std::env::current_exe().unwrap()).unwrap();
        verify_process_identity(std::process::id(), &image).unwrap();
        let mut drifted = image.clone();
        drifted.sha256 = if image.sha256 == "a".repeat(64) {
            "b".repeat(64)
        } else {
            "a".repeat(64)
        };
        assert!(verify_process_identity(std::process::id(), &drifted).is_err());
        assert!(verify_process_identity(0, &image).is_err());
    }

    #[test]
    fn writerless_private_fifo_rejects_without_waiting_under_gate() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::time::{Duration, Instant};
        let (_dir, global, state, origin) = fixture();
        let path = private_updates(&state)
            .unwrap()
            .join("stop-generation.json");
        let c_path = CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        let start = Instant::now();
        assert!(start_allowed_at(&global, &origin, &state, None, None).is_err());
        assert!(record_user_stop_at(&global, &state).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}

#[cfg(test)]
mod migration_owner_upgrade_tests {
    use super::*;
    #[test]
    fn readonly_brew_reuse_can_start_only_an_independent_verified_keg() {
        let root = PathBuf::from("/private/tmp/Cellar/desktop-pet");
        let origin = InstallOrigin::Homebrew {
            brew: "/private/tmp/bin/brew".into(),
            prefix: "/private/tmp".into(),
            cellar_formula_root: root.clone(),
            formula: "hanbong5938/tap/desktop-pet".into(),
            locator:
                "/private/tmp/opt/desktop-pet/libexec/DesktopPet.app/Contents/MacOS/desktop-pet"
                    .into(),
        };
        let old = root.join("0.4.8/libexec/DesktopPet.app/Contents/MacOS/desktop-pet");
        let new = root.join("0.4.9/libexec/DesktopPet.app/Contents/MacOS/desktop-pet");
        assert!(distinct_brew_keg(&origin, &old, &new));
        assert!(!distinct_brew_keg(&origin, &old, &old));
        assert!(!distinct_brew_keg(
            &origin,
            &old,
            &root.join("0.4.8/other-app")
        ));
        assert!(!distinct_brew_keg(
            &InstallOrigin::Standalone {
                bundle: "/private/tmp/DesktopPet.app".into()
            },
            &old,
            &new
        ));
    }
}
