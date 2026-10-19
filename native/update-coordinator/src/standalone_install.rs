//! Immutable approval and durable observations for Pet-only standalone installs.
//! Serialized evidence is never a substitute for an authenticated runtime Stop ACK.
use crate::installation::{InstallationKind, InstallationRecord};
use crate::native_runtime::ProfilePaths;
use crate::{
    ensure_absolute, owned_private_file, protocol, ExecutableIdentity, InstallOrigin, UpdateContext,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_PLAN: u64 = 65536;
const MAX_RECORD: u64 = 32768;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandaloneOriginalInstance {
    pub pid: u32,
    pub start_sec: u64,
    pub start_usec: u64,
    pub instance_id: String,
    pub image: ExecutableIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InstallSource {
    Running {
        instance: StandaloneOriginalInstance,
        context: UpdateContext,
    },
    Stopped {
        profile: ProfilePaths,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandaloneInstallPlan {
    pub version: u32,
    pub operation_id: String,
    pub original: InstallationRecord,
    pub candidate_bundle: PathBuf,
    pub candidate_image: ExecutableIdentity,
    pub candidate_version: String,
    pub helper_image: ExecutableIdentity,
    pub profile: ProfilePaths,
    pub source: InstallSource,
    pub original_stop_generation: u64,
    pub control_token: String,
}

fn identity(image: &ExecutableIdentity) -> Result<(), String> {
    ensure_absolute(&image.path)?;
    if image.sha256.len() != 64
        || !image
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("invalid standalone executable identity".into());
    }
    Ok(())
}

impl StandaloneInstallPlan {
    pub fn origin(&self) -> &InstallOrigin {
        &self.original.origin
    }
    pub fn running(&self) -> Option<(&StandaloneOriginalInstance, &UpdateContext)> {
        match &self.source {
            InstallSource::Running { instance, context } => Some((instance, context)),
            InstallSource::Stopped { .. } => None,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        protocol::validate_operation_id(&self.operation_id)?;
        if self.version != 1
            || self.original.version != 1
            || self.original.kind != InstallationKind::Standalone
            || !matches!(self.origin(), InstallOrigin::Standalone { .. })
            || self.original.executable != self.original.image.path
            || self.original.executable
                != crate::physical_install_root(self.origin())?.join("Contents/MacOS/desktop-pet")
            || self.control_token.len() != 64
            || !self
                .control_token
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("invalid standalone approval identity or owner".into());
        }
        ensure_absolute(&self.profile.config_dir)?;
        ensure_absolute(&self.profile.state_dir)?;
        if let InstallSource::Stopped { profile } = &self.source {
            if profile != &self.profile {
                return Err("stopped source profile differs from approval".into());
            }
        }
        ensure_absolute(crate::physical_install_root(self.origin())?)?;
        ensure_absolute(&self.candidate_bundle)?;
        identity(&self.original.image)?;
        identity(&self.candidate_image)?;
        identity(&self.helper_image)?;
        if self.candidate_image.path != self.candidate_bundle.join("Contents/MacOS/desktop-pet")
            || semver::Version::parse(&self.candidate_version)
                .ok()
                .zip(semver::Version::parse(&self.original.app_version).ok())
                .is_none_or(|(candidate, original)| candidate <= original)
        {
            return Err("invalid standalone candidate".into());
        }
        if let Some((instance, context)) = self.running() {
            if instance.pid == 0
                || instance.start_usec >= 1_000_000
                || instance.instance_id.is_empty()
                || instance.instance_id.len() > 128
                || instance.image != self.original.image
                || context.running != instance.image
                || context.instance_id != instance.instance_id
                || context.executable != self.original.executable
                || context.version != self.original.app_version
                || context.config_dir != self.profile.config_dir
                || context.state_dir != self.profile.state_dir
                || context.running_origin.as_deref() != Some(self.origin())
            {
                return Err("standalone original runtime context mismatch".into());
            }
            context.observation_mode()?;
            if let Some(assets) = &context.assets_override {
                ensure_absolute(assets)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandaloneInstallPhase {
    Approved,
    StopIntent,
    StopAcknowledged,
    OriginalStopped,
    Replacing,
    Installed,
    Starting,
    Completed,
    UserStopped,
    Failed,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStatus {
    Accepted,
    Running,
    Reused,
    InstalledStopped,
    Applied,
    Cancelled,
    Failed,
    InstalledNotApplied,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallRestart {
    NotRequested,
    Pending,
    Ready,
    UserStopped,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandaloneInstallResult {
    pub version: u32,
    pub operation_id: String,
    pub state_dir: PathBuf,
    pub status: InstallStatus,
    pub installation: Option<InstallationRecord>,
    pub installed: bool,
    pub applied: bool,
    pub restart: InstallRestart,
    pub retained_bundle: Option<PathBuf>,
    pub error: Option<String>,
}
impl StandaloneInstallResult {
    pub fn terminal(&self) -> bool {
        !matches!(
            self.status,
            InstallStatus::Accepted | InstallStatus::Running
        )
    }
    pub fn exit_code(&self) -> i32 {
        match self.status {
            InstallStatus::Reused | InstallStatus::InstalledStopped | InstallStatus::Applied => 0,
            InstallStatus::Cancelled => 2,
            _ => 1,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        protocol::validate_operation_id(&self.operation_id)?;
        ensure_absolute(&self.state_dir)?;
        if self.version != 1
            || self.applied && !self.installed
            || self.error.as_ref().is_some_and(|s| s.len() > 8192)
            || self.applied != (self.status == InstallStatus::Applied)
            || self.status == InstallStatus::InstalledStopped
                && (!self.installed || self.restart != InstallRestart::NotRequested)
            || self.status == InstallStatus::InstalledNotApplied
                && (!self.installed || self.applied)
            || self.restart == InstallRestart::Ready && !self.applied
            || self.applied && self.restart != InstallRestart::Ready
            || self.installed && self.installation.is_none()
        {
            return Err("invalid standalone installation outcome".into());
        }
        if let Some(path) = &self.retained_bundle {
            ensure_absolute(path)?;
        }
        if let Some(installation) = &self.installation {
            identity(&installation.image)?;
            ensure_absolute(&installation.executable)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandaloneInstallRecord {
    pub version: u32,
    pub operation_id: String,
    pub phase: StandaloneInstallPhase,
    pub result: StandaloneInstallResult,
}
impl StandaloneInstallRecord {
    pub fn validate(&self, plan: &StandaloneInstallPlan) -> Result<(), String> {
        plan.validate()?;
        self.result.validate()?;
        if self.version != 1
            || self.operation_id != plan.operation_id
            || self.result.operation_id != self.operation_id
            || self.result.state_dir != plan.profile.state_dir
            || plan.running().is_none()
                && (self.result.restart != InstallRestart::NotRequested || self.result.applied)
            || self.phase == StandaloneInstallPhase::Completed
                && !matches!(
                    self.result.status,
                    InstallStatus::Applied | InstallStatus::InstalledStopped
                )
        {
            return Err("standalone record does not match approval".into());
        }
        if self.result.installed {
            let installation = self
                .result
                .installation
                .as_ref()
                .ok_or("installed outcome has no installation")?;
            if installation.version != 1
                || installation.kind != InstallationKind::Standalone
                || installation.origin != *plan.origin()
                || installation.executable != plan.original.executable
                || installation.image.path != installation.executable
                || installation.image.sha256 != plan.candidate_image.sha256
                || installation.app_version != plan.candidate_version
            {
                return Err("installed outcome differs from approved candidate".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapStopAck {
    pub version: u32,
    pub operation_id: String,
    pub origin: InstallOrigin,
    pub profile: ProfilePaths,
    pub original: StandaloneOriginalInstance,
    pub original_stop_generation: u64,
    pub control_token: String,
}
impl BootstrapStopAck {
    pub fn validate(&self, plan: &StandaloneInstallPlan) -> Result<(), String> {
        plan.validate()?;
        let (original, _) = plan
            .running()
            .ok_or("stopped approval has no Stop authority")?;
        if self.version != 1
            || self.operation_id != plan.operation_id
            || self.origin != *plan.origin()
            || self.profile != plan.profile
            || &self.original != original
            || self.original_stop_generation != plan.original_stop_generation
            || self.control_token != plan.control_token
        {
            return Err("bootstrap Stop ACK does not match immutable approval".into());
        }
        Ok(())
    }
}

pub fn plan_path(state: &Path, id: &str) -> Result<PathBuf, String> {
    path(state, id, "plan")
}
pub fn record_path(state: &Path, id: &str) -> Result<PathBuf, String> {
    path(state, id, "result")
}
pub fn stop_ack_path(state: &Path, id: &str) -> Result<PathBuf, String> {
    path(state, id, "stop-ack")
}
pub fn replacement_path(state: &Path, id: &str) -> Result<PathBuf, String> {
    path(state, id, "replacement")
}
fn path(state: &Path, id: &str, suffix: &str) -> Result<PathBuf, String> {
    protocol::validate_operation_id(id)?;
    ensure_absolute(state)?;
    Ok(state
        .join(crate::identity::UPDATES_DIRECTORY)
        .join(format!("install-{id}.{suffix}.json")))
}
fn missing(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
}
pub fn write_plan(plan: &StandaloneInstallPlan) -> Result<PathBuf, String> {
    plan.validate()?;
    protocol::private_updates(&plan.profile.state_dir)?;
    let path = plan_path(&plan.profile.state_dir, &plan.operation_id)?;
    let bytes = serde_json::to_vec(plan).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_PLAN {
        return Err("standalone approval exceeds limit".into());
    }
    protocol::write_private_new(&path, &bytes, 0o600)?;
    Ok(path)
}
pub fn read_plan(state: &Path, id: &str) -> Result<StandaloneInstallPlan, String> {
    check_read_directory(state)?;
    let plan: StandaloneInstallPlan =
        serde_json::from_slice(&owned_private_file(&plan_path(state, id)?, MAX_PLAN)?)
            .map_err(|e| e.to_string())?;
    plan.validate()?;
    if plan.operation_id != id || plan.profile.state_dir != state {
        return Err("standalone approval scope mismatch".into());
    }
    Ok(plan)
}
pub fn read_record(state: &Path, id: &str) -> Result<Option<StandaloneInstallRecord>, String> {
    protocol::check_dir(state, false)?;
    let path = record_path(state, id)?;
    if !missing(&state.join(crate::identity::UPDATES_DIRECTORY)) {
        check_read_directory(state)?;
    }
    if missing(&path) {
        return Ok(None);
    }
    let record: StandaloneInstallRecord =
        serde_json::from_slice(&owned_private_file(&path, MAX_RECORD)?)
            .map_err(|e| e.to_string())?;
    record.validate(&read_plan(state, id)?)?;
    Ok(Some(record))
}
pub fn write_record(state: &Path, record: &StandaloneInstallRecord) -> Result<PathBuf, String> {
    record.validate(&read_plan(state, &record.operation_id)?)?;
    protocol::validate_managed_operation_marker(state)?;
    let path = record_path(state, &record.operation_id)?;
    if !missing(&path) {
        read_record(state, &record.operation_id)?;
    }
    let bytes = serde_json::to_vec(record).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_RECORD {
        return Err("standalone result exceeds limit".into());
    }
    protocol::write_private_bytes(&path, &bytes, 0o600)?;
    protocol::write_managed_operation_marker(
        state,
        protocol::ManagedOperationKind::StandaloneInstall,
        &record.operation_id,
    )?;
    Ok(path)
}
pub fn read_stop_ack(plan: &StandaloneInstallPlan) -> Result<Option<BootstrapStopAck>, String> {
    plan.validate()?;
    check_read_directory(&plan.profile.state_dir)?;
    let path = stop_ack_path(&plan.profile.state_dir, &plan.operation_id)?;
    if missing(&path) {
        return Ok(None);
    }
    let ack: BootstrapStopAck =
        serde_json::from_slice(&owned_private_file(&path, MAX_PLAN)?).map_err(|e| e.to_string())?;
    ack.validate(plan)?;
    Ok(Some(ack))
}

fn check_read_directory(state: &Path) -> Result<(), String> {
    protocol::check_dir(state, false)?;
    protocol::check_dir(&state.join(crate::identity::UPDATES_DIRECTORY), false)
}
