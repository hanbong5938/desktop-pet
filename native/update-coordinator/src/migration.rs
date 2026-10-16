//! Explicit bridge-to-neutral migration. Legacy updater evidence is read-only.
//! This journal is not a protocol-three update plan and is never given to its helper.
use crate::{
    executable_identity, owned_private_file, protocol, ExecutableIdentity, InstallOrigin,
    ReleaseCandidate, UpdateContext,
};
use desktop_pet_contracts::ObservationSelection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[path = "migration_approval.rs"]
mod approval;
pub use approval::{
    approve_profile_update, authorize_update_approval, delegated_approval,
    prepare_automatic_bridge_plan,
};
#[path = "migration_integrations.rs"]
mod integrations;
pub use integrations::{
    committed_route, integration_handover, observe_for_context, refresh_committed_integrations,
    MigrationIntegrationStatus, TernDelivery,
};
#[path = "migration_ancestry.rs"]
mod ancestry;
pub use ancestry::LegacySourceAncestor;
#[path = "migration_tern_config.rs"]
mod tern_config;
pub use tern_config::{TernConfiguration, TernConnection};

const MAX_PLAN: u64 = 256 * 1024;
const MAX_FRAME: usize = 16 * 1024;
pub const PLAN_ENV: &str = "DESKTOP_PET_MIGRATION_PLAN";
pub const OPERATION_ENV: &str = "DESKTOP_PET_MIGRATION_OPERATION";
pub const TOKEN_ENV: &str = "DESKTOP_PET_MIGRATION_TOKEN";

/// Implemented by the independent native helper window, not a daemon flag.
pub trait MigrationPresentation {
    fn show(&self, phase: MigrationPhase, detail: Option<&str>);
    fn pump(&self);
    fn require_visible(&self, plan: &MigrationPlan) -> Result<(), String>;
}
fn pump(presentation: Option<&dyn MigrationPresentation>) {
    if let Some(presentation) = presentation {
        presentation.pump();
    }
}
fn visible(
    presentation: Option<&dyn MigrationPresentation>,
    plan: &MigrationPlan,
) -> Result<(), String> {
    if delegated_approval(plan)? {
        return Ok(());
    }
    presentation
        .ok_or("migration mutation requires independently visible native helper consent")?
        .require_visible(plan)
}
fn off_ui<T: Send + 'static>(
    presentation: Option<&dyn MigrationPresentation>,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    if presentation.is_none() {
        return work();
    }
    let task = thread::spawn(work);
    while !task.is_finished() {
        pump(presentation);
        thread::sleep(Duration::from_millis(25));
    }
    task.join()
        .map_err(|_| "migration validation worker panicked")?
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LegacyOrigin {
    Homebrew {
        brew: ExecutableIdentity,
        prefix: PathBuf,
        cellar_formula_root: PathBuf,
        formula: String,
    },
    HerdrManaged {
        host: ExecutableIdentity,
        host_config_dir: PathBuf,
        checkout_root: PathBuf,
        plugin_root: PathBuf,
        registry_sha256: String,
        resolved_commit: String,
        integration_commit: String,
    },
    Generated {
        source_root: PathBuf,
        bundle: PathBuf,
    },
    Manual {
        bundle: PathBuf,
    },
}
impl LegacyOrigin {
    pub fn root(&self) -> &Path {
        match self {
            Self::Homebrew {
                cellar_formula_root,
                ..
            } => cellar_formula_root,
            Self::HerdrManaged { checkout_root, .. } => checkout_root,
            Self::Generated { source_root, .. } => source_root,
            Self::Manual { bundle } => bundle,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LegacyProfile {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub control_socket: PathBuf,
    pub pid: u32,
    pub instance_id: String,
    pub stop_generation: u64,
    pub observation: ObservationSelection,
    pub herdr_socket: Option<PathBuf>,
    pub assets_override: Option<PathBuf>,
    pub host_plugin_config_dir: Option<PathBuf>,
    pub tern_reconnect_required: bool,
    /// Captured bridge routing only; target does not inherit old product path variables.
    pub environment: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PreviousRecord {
    pub path: PathBuf,
    pub bytes: Option<Vec<u8>>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MigrationTarget {
    pub release: ReleaseCandidate,
    pub image: ExecutableIdentity,
    #[serde(with = "neutral_origin_codec")]
    pub origin: InstallOrigin,
    /// Signed release archive stage is retained even when an existing install is reused.
    pub staged_bundle: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_installation: Option<Box<crate::installation::InstallationRecord>>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MigrationPlan {
    pub version: u32,
    pub kind: String,
    pub operation_id: String,
    pub token: String,
    pub source_image: ExecutableIdentity,
    pub source_origin: LegacyOrigin,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_ancestry: Vec<LegacySourceAncestor>,
    /// Every live profile on the source root, including the selected first profile.
    pub profiles: Vec<LegacyProfile>,
    pub readonly_target_profiles: Vec<protocol::MigrationProfile>,
    pub target: MigrationTarget,
    pub target_manager_program: Option<ExecutableIdentity>,
    pub legacy_aliases: Vec<PathBuf>,
    pub previous_locator: PreviousRecord,
    pub previous_default_profile: PreviousRecord,
    pub adopt_as_default: bool,
    pub retirement_consent: bool,
    pub tern_reconnect_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tern_delivery: Option<TernDelivery>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tern_deliveries: Vec<TernDelivery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tern_delivery_problem: Option<String>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPhase {
    Validating,
    Staged,
    Preparing,
    Stopping,
    Adopting,
    Starting,
    NativeReady,
    Integrating,
    Retiring,
    RetirementFailed,
    ManagerUnknown,
    AwaitingReconnect,
    Complete,
    RollingBack,
    RolledBack,
    UserStopped,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ManagerEvidence {
    pub program: ExecutableIdentity,
    pub args: Vec<String>,
    pub intent_at: u64,
    pub pid: Option<u32>,
    pub pgid: Option<u32>,
    pub exit_code: Option<i32>,
    pub quiescent: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MigrationTargetProfile {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub pid: Option<u32>,
    pub instance_id: Option<String>,
    pub image: ExecutableIdentity,
    pub native_ready: bool,
    pub committed: bool,
    /// Durable intent fences rollback when a Commit reply is lost.
    #[serde(default)]
    pub commit_requested: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MigrationRecord {
    pub version: u32,
    pub operation_id: String,
    pub plan_sha256: String,
    pub phase: MigrationPhase,
    pub prepared_profiles: Vec<String>,
    pub preparing_profiles: Vec<String>,
    pub acknowledged_profiles: Vec<String>,
    pub stopped_profiles: Vec<String>,
    pub target_pid: Option<u32>,
    pub target_instance: Option<String>,
    pub locator_adopted: bool,
    pub default_adopted: bool,
    pub native_ready: bool,
    pub target_committed: bool,
    /// Empty only for historical journals; primary fields never prove secondary targets.
    #[serde(default)]
    pub target_profiles: Vec<MigrationTargetProfile>,
    #[serde(default)]
    pub stopped_target_profiles: Vec<MigrationProfilePaths>,
    #[serde(default)]
    pub restored_profiles: Vec<protocol::MigrationProfile>,
    pub manager: Option<ManagerEvidence>,
    pub target_manager: Option<ManagerEvidence>,
    /// Actual manager-derived image; the approved public release remains immutable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_image: Option<ExecutableIdentity>,
    pub reconnected_profiles: Vec<MigrationProfilePaths>,
    #[serde(default)]
    pub integration_status: Vec<MigrationIntegrationStatus>,
    #[serde(default)]
    pub integration_delivery_error: Option<String>,
    pub retirement_attempt: u32,
    pub error: Option<String>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MigrationCommand {
    Prepare,
    ConsentAck,
    DelegationAck,
    Stop,
    Abort,
    Commit,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MigrationControlRequest {
    pub version: u32,
    pub kind: String,
    pub command: MigrationCommand,
    pub operation_id: String,
    pub instance_id: String,
    pub token: String,
    pub plan_path: PathBuf,
    pub source_image: ExecutableIdentity,
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub stop_generation: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MigrationControlReply {
    pub version: u32,
    pub kind: String,
    pub command: MigrationCommand,
    pub operation_id: String,
    pub instance_id: String,
    pub token: String,
    pub plan_path: PathBuf,
    pub source_image: ExecutableIdentity,
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub stop_generation: u64,
    pub accepted: bool,
    pub error: Option<String>,
}
#[derive(Clone, Debug)]
pub struct MigrationStartup {
    pub operation_id: String,
    pub token: String,
    pub stop_generation: u64,
    pub source: bool,
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn image_valid(image: &ExecutableIdentity) -> Result<(), String> {
    crate::ensure_absolute(&image.path)?;
    if !hex(&image.sha256, 64) {
        return Err("invalid image digest".into());
    }
    Ok(())
}
fn owned_directory(path: &Path) -> Result<(), String> {
    crate::ensure_absolute(path)?;
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o022 != 0
        || fs::canonicalize(path).map_err(|e| e.to_string())? != path
    {
        return Err("migration directories must be owned physical non-writable roots".into());
    }
    Ok(())
}
fn instance_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
pub fn validate_control_request(
    request: &MigrationControlRequest,
    context: &UpdateContext,
    current_stop_generation: u64,
    bridge: bool,
) -> Result<(), String> {
    protocol::validate_operation_id(&request.operation_id)?;
    image_valid(&request.source_image)?;
    crate::ensure_absolute(&request.plan_path)?;
    if !hex(&request.token, 64)
        || !instance_valid(&request.instance_id)
        || request.version != if bridge { 1 } else { 4 }
        || request.kind != if bridge { "legacy-migration" } else { "update" }
        || request.instance_id != context.instance_id
        || request.source_image != context.running
        || request.config_dir != context.config_dir
        || request.state_dir != context.state_dir
        || request.stop_generation != current_stop_generation
            && request.command != MigrationCommand::Abort
            && (bridge || request.command != MigrationCommand::Stop)
        || bridge && request.command == MigrationCommand::Commit
        || !bridge
            && !matches!(
                request.command,
                MigrationCommand::Stop | MigrationCommand::Commit
            )
    {
        return Err(
            "migration authorization does not match this live instance/profile/image".into(),
        );
    }
    Ok(())
}
fn journal_path(plan: &MigrationPlan) -> PathBuf {
    plan.profiles[0]
        .state_dir
        .join("updates-v4")
        .join(format!("{}.migration.json", plan.operation_id))
}
fn evidence_path(plan: &MigrationPlan) -> PathBuf {
    plan.profiles[0]
        .state_dir
        .join("updates-v4")
        .join(format!("{}.migration-plan.json", plan.operation_id))
}
fn save(plan: &MigrationPlan, record: &MigrationRecord) -> Result<(), String> {
    protocol::write_private_bytes(
        &journal_path(plan),
        &serde_json::to_vec(record).map_err(|e| e.to_string())?,
        0o600,
    )
}
fn transition(
    plan: &MigrationPlan,
    record: &mut MigrationRecord,
    phase: MigrationPhase,
    presentation: Option<&dyn MigrationPresentation>,
) -> Result<(), String> {
    record.phase = phase;
    save(plan, record)?;
    if let Some(presentation) = presentation {
        presentation.show(phase, record.error.as_deref());
        presentation.pump();
    }
    Ok(())
}
pub fn read_record(plan: &MigrationPlan) -> Result<MigrationRecord, String> {
    let record: MigrationRecord =
        serde_json::from_slice(&owned_private_file(&journal_path(plan), MAX_PLAN)?)
            .map_err(|e| e.to_string())?;
    let evidence = owned_private_file(&evidence_path(plan), MAX_PLAN)?;
    let captured: MigrationPlan = serde_json::from_slice(&evidence).map_err(|e| e.to_string())?;
    let digest = format!("{:x}", Sha256::digest(&evidence));
    if captured != *plan
        || record.version != 1
        || record.operation_id != plan.operation_id
        || record.plan_sha256 != digest
    {
        return Err("migration journal does not match immutable evidence".into());
    }
    target_image(plan, &record)?;
    Ok(record)
}

/// A different installed SHA is allowed only for the certain, successful Brew
/// transaction whose original-release/signing derivation was verified on disk.
pub fn target_image<'a>(
    plan: &'a MigrationPlan,
    record: &'a MigrationRecord,
) -> Result<&'a ExecutableIdentity, String> {
    let Some(image) = &record.target_image else {
        return Ok(&plan.target.image);
    };
    image_valid(image)?;
    if image.path != plan.target.image.path {
        return Err("actual migration candidate changed its approved executable path".into());
    }
    if image != &plan.target.image {
        let InstallOrigin::Homebrew { formula, .. } = &plan.target.origin else {
            return Err("only the approved Brew owner may derive the migration image".into());
        };
        let manager = record
            .target_manager
            .as_ref()
            .ok_or("derived migration candidate has no actual manager result")?;
        let action = if plan.target.previous_installation.is_some() {
            "upgrade"
        } else {
            "install"
        };
        if !manager_can_resume(manager)
            || manager.exit_code != Some(0)
            || Some(&manager.program) != plan.target_manager_program.as_ref()
            || manager.args.len() != 2
            || manager.args[0] != action
            || manager.args[1] != formula.as_str()
        {
            return Err("derived migration candidate lacks its certain successful approved manager transaction".into());
        }
    }
    Ok(image)
}
pub fn read_plan(path: &Path) -> Result<MigrationPlan, String> {
    let plan: MigrationPlan =
        serde_json::from_slice(&owned_private_file(path, MAX_PLAN)?).map_err(|e| e.to_string())?;
    validate_plan(&plan)?;
    Ok(plan)
}
pub fn validate_plan(plan: &MigrationPlan) -> Result<(), String> {
    if plan.target.release.version != crate::identity::neutral::VERSION
        || plan.target.release.channel != crate::ReleaseChannel::Stable
    {
        return Err(format!(
            "migration requires approved neutral stable {} release",
            crate::identity::neutral::VERSION
        ));
    }
    validate_plan_structure(plan)
}
fn validate_plan_structure(plan: &MigrationPlan) -> Result<(), String> {
    protocol::validate_operation_id(&plan.operation_id)?;
    if plan.version != 1
        || plan.kind != "legacy_to_desktop_pet"
        || !hex(&plan.token, 64)
        || plan.profiles.is_empty()
        || plan.profiles.len() > 64
        || !plan.retirement_consent
    {
        return Err("invalid migration version/kind/consent/profile bound".into());
    }
    if plan.tern_reconnect_required!=plan.profiles.iter().any(|profile|profile.tern_reconnect_required)
        || plan.profiles.iter().any(|profile|matches!(&profile.observation,ObservationSelection::Provider(provider) if provider=="tern")
            && !profile.tern_reconnect_required) {
        return Err("known Tern profile reconnect requirements cannot be dropped from migration consent".into());
    }
    for delivery in plan.tern_delivery.iter().chain(&plan.tern_deliveries) {
        image_valid(&delivery.program)?;
        tern_config::validate_delivery(plan, delivery)?;
        if !plan.tern_reconnect_required
            || delivery.package
                != plan
                    .target
                    .staged_bundle
                    .join("Contents/Resources/integrations/tern-desktop-pet")
            || !plan.profiles.iter().any(|profile| {
                profile.tern_reconnect_required
                    && profile.config_dir == delivery.configuration.connection.config_dir
                    && profile.state_dir == delivery.configuration.connection.state_dir
            })
            || !hex(&delivery.package_sha256, 64)
            || !hex(&delivery.revision, 40)
        {
            return Err("Tern consumer delivery does not match this bounded signed target".into());
        }
    }
    image_valid(&plan.source_image)?;
    ancestry::validate_source_ancestry(plan)?;
    image_valid(&plan.target.image)?;
    crate::ensure_absolute(plan.source_origin.root())?;
    if !plan
        .source_image
        .path
        .starts_with(plan.source_origin.root())
        || plan.source_image.path == plan.target.image.path
        || crate::physical_install_root(&plan.target.origin)?.starts_with(plan.source_origin.root())
        || plan
            .source_origin
            .root()
            .starts_with(crate::physical_install_root(&plan.target.origin)?)
    {
        return Err("source and neutral installation roots must be disjoint".into());
    }
    if let Some(previous) = &plan.target.previous_installation {
        image_valid(&previous.image)?;
        if previous.version != 1
            || previous.protocol != 4
            || previous.observation_mode != 2
            || previous.integration_api != 1
            || previous.executable != previous.image.path
            || previous.origin != plan.target.origin
            || !matches!(previous.app_version.as_str(), "0.4.7" | "0.4.8")
        {
            return Err(
                "previous neutral owner upgrade proof is incompatible with this approved target"
                    .into(),
            );
        }
    }
    if plan.readonly_target_profiles.len() > 64 {
        return Err("read-only target census exceeds bound".into());
    }
    for (index, profile) in plan.readonly_target_profiles.iter().enumerate() {
        image_valid(&profile.image)?;
        crate::ensure_absolute(&profile.config_dir)?;
        crate::ensure_absolute(&profile.state_dir)?;
        if (profile.image != plan.target.image
            && !matches!(&plan.target.origin, InstallOrigin::Homebrew { cellar_formula_root, .. } if profile.image.path.starts_with(cellar_formula_root)))
            || !instance_valid(&profile.instance_id)
            || profile.pid == 0
            || profile.image.path.starts_with(plan.source_origin.root())
            || plan.profiles.iter().any(|source| {
                source.state_dir == profile.state_dir
                    || source.config_dir == profile.config_dir
                    || source.pid == profile.pid
            })
            || plan.readonly_target_profiles[..index].iter().any(|other| {
                other.state_dir == profile.state_dir
                    || other.config_dir == profile.config_dir
                    || other.pid == profile.pid
            })
        {
            return Err(
                "read-only target profile is not an independent pinned neutral instance".into(),
            );
        }
    }
    plan.target.release.validate_neutral()?;
    if plan.target.release.channel != crate::ReleaseChannel::Stable {
        return Err("migration evidence requires an approved neutral stable release".into());
    }
    crate::ensure_absolute(&plan.target.staged_bundle)?;
    if let Some(manager) = &plan.target_manager_program {
        image_valid(manager)?;
        if !matches!(&plan.target.origin, InstallOrigin::Homebrew { brew, .. } if brew == &manager.path)
        {
            return Err("target manager evidence does not belong to captured neutral owner".into());
        }
    }
    if plan.legacy_aliases.len() > 32 {
        return Err("legacy alias evidence exceeds bound".into());
    }
    for alias in &plan.legacy_aliases {
        crate::ensure_absolute(alias)?;
    }
    for (index, profile) in plan.profiles.iter().enumerate() {
        for path in [
            &profile.config_dir,
            &profile.state_dir,
            &profile.control_socket,
        ] {
            crate::ensure_absolute(path)?;
        }
        for root in [
            plan.source_origin.root(),
            crate::physical_install_root(&plan.target.origin)?,
        ] {
            if profile.config_dir.starts_with(root)
                || profile.state_dir.starts_with(root)
                || root.starts_with(&profile.config_dir)
                || root.starts_with(&profile.state_dir)
            {
                return Err(
                    "durable profiles must not overlap replaceable installation roots".into(),
                );
            }
        }
        if profile.pid == 0
            || profile.pid > i32::MAX as u32
            || !instance_valid(&profile.instance_id)
            || plan.profiles[..index].iter().any(|other| {
                other.state_dir == profile.state_dir
                    || other.config_dir == profile.config_dir
                    || other.pid == profile.pid
            })
            || profile.environment.len() > 16
        {
            return Err("invalid or duplicate source profile".into());
        }
        if let Some(socket) = &profile.herdr_socket {
            crate::ensure_absolute(socket)?;
        }
        if let Some(host_config) = &profile.host_plugin_config_dir {
            crate::ensure_absolute(host_config)?;
        }
        if let Some(assets) = &profile.assets_override {
            crate::ensure_absolute(assets)?;
            let retired_root = match &plan.source_origin {
                LegacyOrigin::Generated { bundle, .. } => bundle.as_path(),
                _ => plan.source_origin.root(),
            };
            if assets.starts_with(retired_root)
                || assets.starts_with(crate::physical_install_root(&plan.target.origin)?)
            {
                return Err("explicit renderer assets overlap an installation that migration retires/replaces".into());
            }
        }
        for (key, value) in &profile.environment {
            if !matches!(
                key.as_str(),
                "HERDR_BIN_PATH"
                    | "HERDR_SOCKET_PATH"
                    | "HERDR_ENV"
                    | "HERDR_PLUGIN_ID"
                    | "HERDR_PLUGIN_CONFIG_DIR"
                    | "HERDR_PLUGIN_STATE_DIR"
                    | "XDG_CONFIG_HOME"
                    | "XDG_STATE_HOME"
                    | "HOME"
                    | "HERDR_PET_OBSERVATION_MODE"
                    | "TERN_CONFIG_DIR"
                    | "TERN_DAEMON_SOCKET"
            ) || value.len() > 4096
                || value.contains('\0')
            {
                return Err("unapproved bridge routing environment".into());
            }
            if matches!(key.as_str(), "TERN_CONFIG_DIR" | "TERN_DAEMON_SOCKET") {
                crate::ensure_absolute(Path::new(value))?;
            }
        }
        if let Some(mode) = profile.environment.get("HERDR_PET_OBSERVATION_MODE") {
            if ObservationSelection::migrate_legacy_input(Some(mode))? != profile.observation {
                return Err(
                    "captured legacy observation environment differs from consented selection"
                        .into(),
                );
            }
        }
    }
    for record in [&plan.previous_locator, &plan.previous_default_profile] {
        crate::ensure_absolute(&record.path)?;
        if record
            .bytes
            .as_ref()
            .is_some_and(|bytes| bytes.len() > 16384)
        {
            return Err("previous record exceeds bound".into());
        }
        if let Some(bytes) = &record.bytes {
            let _: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        }
    }
    let home = crate::origin::os_home()?;
    if plan.previous_locator.path != home.join(".local/share/desktop-pet/installation.json")
        || plan.previous_default_profile.path
            != neutral_config_root(&plan.profiles[0].environment, &home)?
                .join("default-profile.json")
    {
        return Err("migration locator/profile pointer is not a canonical product record".into());
    }
    if plan.adopt_as_default
        && plan.target.release.version == crate::identity::neutral::VERSION
        && !default_profile_matches(&plan.previous_default_profile, &plan.profiles[0])?
    {
        return Err("migration cannot replace an unrelated existing default profile".into());
    }
    match &plan.source_origin {
        LegacyOrigin::Homebrew {
            brew,
            prefix,
            cellar_formula_root,
            formula,
        } => {
            image_valid(brew)?;
            crate::ensure_absolute(prefix)?;
            crate::ensure_absolute(cellar_formula_root)?;
            if formula != "hanbong5938/tap/herdr-desktop-pet"
                || cellar_formula_root != &prefix.join("Cellar/herdr-desktop-pet")
            {
                return Err("only consented old stable Homebrew formula may retire".into());
            }
        }
        LegacyOrigin::HerdrManaged {
            host,
            host_config_dir,
            checkout_root,
            plugin_root,
            registry_sha256,
            resolved_commit,
            integration_commit,
        } => {
            image_valid(host)?;
            for path in [host_config_dir, checkout_root, plugin_root] {
                crate::ensure_absolute(path)?;
            }
            if !hex(registry_sha256, 64)
                || !hex(resolved_commit, 40)
                || !hex(integration_commit, 40)
                || checkout_root == &host_config_dir.join("plugins/github")
                || !checkout_root.starts_with(host_config_dir.join("plugins/github"))
                || !plugin_root.starts_with(checkout_root)
                || plan.source_image.path
                    != plugin_root.join("dist/HerdrDesktopPet.app/Contents/MacOS/herdr-desktop-pet")
            {
                return Err("invalid same-registry managed migration authority".into());
            }
        }
        LegacyOrigin::Generated {
            source_root,
            bundle,
        } => {
            crate::ensure_absolute(bundle)?;
            if bundle != &source_root.join("dist/HerdrDesktopPet.app") {
                return Err("only generated legacy bundle may retire".into());
            }
        }
        LegacyOrigin::Manual { bundle } => {
            crate::ensure_absolute(bundle)?;
        }
    }
    Ok(())
}
fn unchanged(record: &PreviousRecord) -> Result<(), String> {
    let actual = match fs::symlink_metadata(&record.path) {
        Ok(_) => Some(owned_private_file(&record.path, 16384)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    if actual != record.bytes {
        return Err("installation/default profile record changed since consent".into());
    }
    Ok(())
}
fn stop_requested(plan: &MigrationPlan) -> Result<bool, String> {
    for profile in &plan.profiles {
        if migration_stop_generation(&profile.state_dir)? != profile.stop_generation {
            return Ok(true);
        }
    }
    Ok(false)
}
fn migration_stop_generation(state: &Path) -> Result<u64, String> {
    let mut generation = 0u64;
    for directory in ["updates", "updates-v4"] {
        let path = state.join(directory).join("stop-generation.json");
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
            Ok(_) => {
                let value: u64 = serde_json::from_slice(&owned_private_file(&path, 4096)?)
                    .map_err(|e| e.to_string())?;
                generation = generation
                    .checked_add(value)
                    .ok_or("Stop generation overflow")?;
            }
        }
    }
    Ok(generation)
}
#[derive(Deserialize)]
struct LiveStatus {
    ok: bool,
    running: bool,
    shutdown: bool,
    ready: bool,
    ui_ready: bool,
    control_ready: bool,
    app_version: String,
    updater_protocol: u32,
    observation_capability: u32,
    integration_api: u32,
    instance_id: String,
    running_sha256: String,
    pid: u32,
    executable_path: PathBuf,
    config_dir: PathBuf,
    state_dir: PathBuf,
    observation_mode: String,
    #[serde(default)]
    migration_operation: Option<String>,
    #[serde(default)]
    migration_gated: bool,
    #[serde(default)]
    migration_source_environment: Option<BTreeMap<String, String>>,
    #[serde(default)]
    migration_source_host_plugin_config_dir: Option<PathBuf>,
    #[serde(default)]
    update_ui_visible: bool,
    #[serde(default)]
    update_operation_id: Option<String>,
    #[serde(default)]
    herdr_socket: Option<PathBuf>,
    #[serde(default)]
    assets_override: Option<PathBuf>,
    #[serde(default)]
    host_connections: BTreeMap<String, usize>,
}
fn source_requires_tern(live: &LiveStatus) -> Result<bool, String> {
    Ok(
        matches!(&live_observation(live)?,ObservationSelection::Provider(provider) if provider=="tern")
            || live.host_connections.get("tern").copied().unwrap_or(0) > 0,
    )
}
fn exchange(path: &Path, request: &impl Serialize) -> Result<Vec<u8>, String> {
    exchange_timeout(path, request, Duration::from_secs(5))
}
fn exchange_timeout(
    path: &Path,
    request: &impl Serialize,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    exchange_bounded(path, request, timeout, MAX_FRAME)
}
fn exchange_bounded(
    path: &Path,
    request: &impl Serialize,
    timeout: Duration,
    response_limit: usize,
) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    use std::os::unix::fs::FileTypeExt;
    if !metadata.file_type().is_socket()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err("control socket is not private and owned".into());
    }
    let deadline = Instant::now() + timeout;
    let mut stream = connect_bounded(path, deadline)?;
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd;
        let mut uid = 0;
        let mut gid = 0;
        if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0
            || uid != unsafe { libc::geteuid() }
        {
            return Err("control peer UID differs from migration account".into());
        }
    }
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let mut bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
    if bytes.len() >= MAX_FRAME {
        return Err("migration control request exceeds bound".into());
    }
    bytes.push(b'\n');
    stream.write_all(&bytes).map_err(|e| e.to_string())?;
    let mut reply = Vec::with_capacity(2048);
    let mut buffer = [0u8; 4096];
    while reply.len() < response_limit {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("migration control deadline expired")?;
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|e| e.to_string())?;
        let available = (response_limit - reply.len()).min(buffer.len());
        let count = stream
            .read(&mut buffer[..available])
            .map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("control reply truncated".into());
        }
        if let Some(end) = buffer[..count].iter().position(|byte| *byte == b'\n') {
            if end + 1 != count {
                return Err("control reply contains trailing bytes".into());
            }
            reply.extend_from_slice(&buffer[..end]);
            return Ok(reply);
        }
        reply.extend_from_slice(&buffer[..count]);
    }
    Err("migration control reply exceeds bound".into())
}
fn status(profile: &LegacyProfile) -> Result<LiveStatus, String> {
    status_at(&profile.control_socket)
}
fn status_at(socket: &Path) -> Result<LiveStatus, String> {
    serde_json::from_slice(&exchange_bounded(
        socket,
        &serde_json::json!({"version":1,"command":"status"}),
        Duration::from_secs(5),
        MAX_PLAN as usize,
    )?)
    .map_err(|e| e.to_string())
}
fn live_observation(live: &LiveStatus) -> Result<ObservationSelection, String> {
    if matches!(
        live.app_version.as_str(),
        "0.3.12" | "0.3.13" | "0.3.14" | "0.3.15"
    ) {
        ObservationSelection::migrate_legacy_input(Some(&live.observation_mode))
    } else {
        live.observation_mode.parse()
    }
}
fn actual_process_image(pid: u32) -> Result<PathBuf, String> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Err("invalid daemon PID".into());
    }
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn proc_pidpath(pid: libc::c_int, buffer: *mut libc::c_void, size: u32) -> libc::c_int;
        }
        let mut bytes = [0u8; 4096];
        let n = unsafe { proc_pidpath(pid as i32, bytes.as_mut_ptr().cast(), bytes.len() as u32) };
        if n <= 0 {
            return Err("cannot attest daemon process executable".into());
        }
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(n as usize);
        let text = std::str::from_utf8(&bytes[..end]).map_err(|e| e.to_string())?;
        Ok(PathBuf::from(text))
    }
    #[cfg(not(target_os = "macos"))]
    {
        fs::read_link(format!("/proc/{pid}/exe")).map_err(|e| e.to_string())
    }
}
fn verify_status(
    plan: &MigrationPlan,
    profile: &LegacyProfile,
    live: &LiveStatus,
    target: bool,
) -> Result<(), String> {
    let target_record = target.then(|| read_record(plan)).transpose()?;
    let image = if let Some(record) = &target_record {
        target_image(plan, record)?
    } else {
        &plan.source_image
    };
    if !live.ok
        || !live.running
        || live.shutdown
        || !live.ready
        || !live.ui_ready
        || !live.control_ready
        || live.app_version
            != if target {
                crate::identity::neutral::VERSION
            } else {
                "0.3.15"
            }
        || live.updater_protocol != if target { 4 } else { 3 }
        || live.observation_capability != if target { 2 } else { 1 }
        || live.integration_api != 1
        || live.executable_path != image.path
        || live.running_sha256 != image.sha256
        || live.config_dir != profile.config_dir
        || live.state_dir != profile.state_dir
        || !target && live.migration_source_host_plugin_config_dir != profile.host_plugin_config_dir
        || live_observation(live)? != profile.observation
        || !instance_valid(&live.instance_id)
        || !target && source_requires_tern(live)? && !profile.tern_reconnect_required
        || live.assets_override != profile.assets_override
        || !target && live.herdr_socket != profile.herdr_socket
        || !target && (live.pid != profile.pid || live.instance_id != profile.instance_id)
        || target
            && (live.migration_operation.as_deref() != Some(&plan.operation_id)
                || !live.migration_gated)
        || actual_process_image(live.pid)? != image.path
        || executable_identity(&image.path)? != *image
    {
        return Err(
            "actual daemon PID/image/version/profile/capability/readiness differs from consent"
                .into(),
        );
    }
    Ok(())
}
fn control(
    plan: &MigrationPlan,
    profile: &LegacyProfile,
    instance: &str,
    command: MigrationCommand,
    target: bool,
) -> Result<(), String> {
    let request = MigrationControlRequest {
        version: if target { 4 } else { 1 },
        kind: if target { "update" } else { "legacy-migration" }.into(),
        command,
        operation_id: plan.operation_id.clone(),
        instance_id: instance.into(),
        token: plan.token.clone(),
        plan_path: evidence_path(plan),
        source_image: if target {
            target_image(plan, &read_record(plan)?)?.clone()
        } else {
            plan.source_image.clone()
        },
        config_dir: profile.config_dir.clone(),
        state_dir: profile.state_dir.clone(),
        stop_generation: profile.stop_generation,
    };
    let reply: MigrationControlReply = serde_json::from_slice(&exchange_timeout(
        &profile.control_socket,
        &request,
        Duration::from_secs(30),
    )?)
    .map_err(|e| e.to_string())?;
    if !reply.accepted
        || reply.version != request.version
        || reply.kind != request.kind
        || reply.command != request.command
        || reply.operation_id != request.operation_id
        || reply.instance_id != request.instance_id
        || reply.token != request.token
        || reply.plan_path != request.plan_path
        || reply.source_image != request.source_image
        || reply.config_dir != request.config_dir
        || reply.state_dir != request.state_dir
        || reply.stop_generation != request.stop_generation
        || reply.error.is_some()
    {
        return Err(reply
            .error
            .unwrap_or_else(|| "migration acknowledgement is not exact authorization".into()));
    }
    Ok(())
}
fn process_live(pid: u32) -> bool {
    unsafe {
        libc::kill(pid as i32, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}
fn wait_stopped(pid: u32, presentation: Option<&dyn MigrationPresentation>) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(15);
    while process_live(pid) {
        pump(presentation);
        if Instant::now() >= until {
            return Err("authorized daemon did not stop".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}
fn command_output(program: &Path, args: &[String]) -> Result<String, String> {
    let borrowed = args.iter().map(String::as_str).collect::<Vec<_>>();
    crate::origin::command_output(program, &borrowed, 1024 * 1024, 30)
}
fn verify_source(plan: &MigrationPlan) -> Result<(), String> {
    owned_directory(plan.source_origin.root())?;
    if executable_identity(&plan.source_image.path)? != plan.source_image {
        return Err("bridge image changed".into());
    }
    let version = command_output(&plan.source_image.path, &["--version".into()])?;
    if version.trim() != "herdr-desktop-pet 0.3.15" {
        return Err("Operation A must finish with the genuine bridge image".into());
    }
    let caps: serde_json::Value = serde_json::from_str(&command_output(
        &plan.source_image.path,
        &["update-capabilities".into()],
    )?)
    .map_err(|e| e.to_string())?;
    if caps.get("protocol").and_then(serde_json::Value::as_u64) != Some(3)
        || caps
            .get("observation_mode")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
    {
        return Err("bridge is not a matched protocol-three observation-one image".into());
    }
    let helper = source_bundle(plan)?.join("Contents/MacOS/herdr-update-coordinator");
    let helper_caps: serde_json::Value =
        serde_json::from_str(&command_output(&helper, &["update-capabilities".into()])?)
            .map_err(|e| e.to_string())?;
    if helper_caps
        .get("protocol")
        .and_then(serde_json::Value::as_u64)
        != Some(3)
        || helper_caps
            .get("observation_mode")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
    {
        return Err("retained source helper is not the matched bridge protocol-three pair".into());
    }
    #[cfg(target_os = "macos")]
    command_output(
        Path::new("/usr/bin/codesign"),
        &[
            "--verify".into(),
            "--deep".into(),
            "--strict".into(),
            source_bundle(plan)?.to_string_lossy().into_owned(),
        ],
    )?;
    match &plan.source_origin {
        LegacyOrigin::Homebrew {
            brew,
            prefix,
            formula,
            ..
        } => {
            if executable_identity(&brew.path)? != *brew {
                return Err("captured Brew changed".into());
            }
            let installed = command_output(&brew.path, &["--prefix".into(), formula.clone()])?;
            let root = fs::canonicalize(installed.trim()).map_err(|e| e.to_string())?;
            if !root.starts_with(prefix.join("Cellar/herdr-desktop-pet"))
                || !plan.source_image.path.starts_with(&root)
            {
                return Err("old image is not owned by captured old stable formula".into());
            }
            let receipt = root.join("INSTALL_RECEIPT.json");
            let doc: serde_json::Value =
                serde_json::from_slice(&crate::owned_file(&receipt, 1024 * 1024)?)
                    .map_err(|e| e.to_string())?;
            if doc
                .pointer("/source/tap")
                .and_then(serde_json::Value::as_str)
                != Some("hanbong5938/tap")
            {
                return Err("old Brew receipt has another authority".into());
            }
        }
        LegacyOrigin::HerdrManaged {
            host,
            host_config_dir,
            registry_sha256,
            resolved_commit,
            checkout_root,
            plugin_root,
            ..
        } => {
            if executable_identity(&host.path)? != *host {
                return Err("captured Herdr changed".into());
            }
            let bytes = crate::owned_file(&host_config_dir.join("plugins.json"), 1024 * 1024)?;
            if format!("{:x}", Sha256::digest(&bytes)) != *registry_sha256 {
                return Err("consented Herdr registry changed".into());
            }
            let records: Vec<serde_json::Value> =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if records.len() > 256 {
                return Err("Herdr registry exceeds bound".into());
            }
            let entries = records
                .iter()
                .filter(|v| {
                    v.get("plugin_id").and_then(serde_json::Value::as_str) == Some("desktop-pet")
                })
                .collect::<Vec<_>>();
            if entries.len() != 1 {
                return Err("ambiguous Herdr desktop-pet registration".into());
            }
            let entry = entries[0];
            if entry.get("plugin_root").and_then(serde_json::Value::as_str) != plugin_root.to_str()
                || entry
                    .pointer("/source/kind")
                    .and_then(serde_json::Value::as_str)
                    != Some("github")
                || entry
                    .pointer("/source/owner")
                    .and_then(serde_json::Value::as_str)
                    != Some("hanbong5938")
                || entry
                    .pointer("/source/repo")
                    .and_then(serde_json::Value::as_str)
                    != Some("herdr-desktop-pet")
                || entry
                    .pointer("/source/managed_path")
                    .and_then(serde_json::Value::as_str)
                    != checkout_root.to_str()
                || entry
                    .pointer("/source/resolved_commit")
                    .and_then(serde_json::Value::as_str)
                    != Some(resolved_commit)
            {
                return Err("same-registry source registration does not authorize refresh".into());
            }
            let head = command_output(
                Path::new("/usr/bin/git"),
                &[
                    "-C".into(),
                    checkout_root.to_string_lossy().into_owned(),
                    "rev-parse".into(),
                    "HEAD".into(),
                ],
            )?;
            let dirty = command_output(
                Path::new("/usr/bin/git"),
                &[
                    "-C".into(),
                    checkout_root.to_string_lossy().into_owned(),
                    "status".into(),
                    "--porcelain".into(),
                    "--untracked-files=all".into(),
                ],
            )?;
            if head.trim() != resolved_commit || !dirty.trim().is_empty() {
                return Err("managed source changed; migration never resets user changes".into());
            }
            verify_manager_registry(plan)?;
            verify_git_remote(checkout_root)?;
        }
        LegacyOrigin::Generated { bundle, .. } | LegacyOrigin::Manual { bundle } => {
            owned_directory(bundle)?;
            if plan.source_image.path != bundle.join("Contents/MacOS/herdr-desktop-pet") {
                return Err("exact approved legacy bundle differs".into());
            }
        }
    }
    Ok(())
}
fn source_bundle(plan: &MigrationPlan) -> Result<PathBuf, String> {
    let path = plan
        .source_image
        .path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or("invalid bridge bundle image")?;
    if path.file_name().and_then(|s| s.to_str()) != Some("HerdrDesktopPet.app") {
        return Err("bridge bundle envelope is not legacy stable".into());
    }
    Ok(path.to_path_buf())
}
/// Launch validation is deliberately journal-based, never a bare environment-token bypass.
pub fn startup_authorization(
    path: &Path,
    operation: &str,
    token: &str,
    context: &UpdateContext,
) -> Result<MigrationStartup, String> {
    let plan = read_plan(path)?;
    if path != evidence_path(&plan) || operation != plan.operation_id || token != plan.token {
        return Err("startup migration evidence/token differs".into());
    }
    let record = read_record(&plan)?;
    let profile = plan
        .profiles
        .iter()
        .find(|p| p.config_dir == context.config_dir && p.state_dir == context.state_dir)
        .ok_or("startup profile not consented")?;
    if context.assets_override != profile.assets_override
        || context.observation_mode()? != profile.observation
    {
        return Err(
            "migration startup selection/renderer does not match authenticated profile".into(),
        );
    }
    if stop_requested(&plan)? {
        return Err("user Stop cancelled migration startup".into());
    }
    let source = context.running == plan.source_image;
    if source {
        #[cfg(desktop_pet_legacy)]
        if context.host_plugin_config_dir != profile.host_plugin_config_dir
            || Some(&context.herdr_socket) != profile.herdr_socket.as_ref()
        {
            return Err(
                "rollback legacy host/profile routing provenance differs from authenticated plan"
                    .into(),
            );
        }
        if record.phase != MigrationPhase::RollingBack
            || record.manager.is_some()
            || context.version != "0.3.15"
        {
            return Err("bridge restart lacks valid rollback authorization".into());
        }
    } else if &context.running != target_image(&plan, &record)?
        || context.version != crate::identity::neutral::VERSION
        || record.phase != MigrationPhase::Starting
    {
        return Err("target startup lacks held migration authorization".into());
    }
    Ok(MigrationStartup {
        operation_id: operation.into(),
        token: token.into(),
        stop_generation: profile.stop_generation,
        source,
    })
}

/// Check durable state in addition to the bounded wire envelope. The control
/// server serializes this check and admission with its domain mutation gate.
pub fn verify_live_control(
    path: &Path,
    request: &MigrationControlRequest,
    context: &UpdateContext,
    bridge: bool,
) -> Result<(), String> {
    let plan = read_plan(path)?;
    let record = read_record(&plan)?;
    validate_control_request(
        request,
        context,
        migration_stop_generation(&context.state_dir)?,
        bridge,
    )?;
    if path != evidence_path(&plan)
        || request.plan_path != path
        || request.operation_id != plan.operation_id
        || request.token != plan.token
    {
        return Err("control authorization has no matching immutable migration evidence".into());
    }
    let profile = plan
        .profiles
        .iter()
        .find(|p| p.config_dir == context.config_dir && p.state_dir == context.state_dir)
        .ok_or("control profile was not consented")?;
    if profile.stop_generation != request.stop_generation
        || stop_requested(&plan)?
            && request.command != MigrationCommand::Abort
            && (bridge || request.command != MigrationCommand::Stop)
    {
        return Err("user Stop superseded migration".into());
    }
    let admitted = match (bridge, request.command) {
        (true, MigrationCommand::Prepare) => {
            record.phase == MigrationPhase::Preparing
                && profile.instance_id == context.instance_id
                && context.running == plan.source_image
        }
        (true, MigrationCommand::ConsentAck | MigrationCommand::DelegationAck) => {
            record.phase == MigrationPhase::Preparing
                && record.prepared_profiles.contains(&profile.instance_id)
                && context.running == plan.source_image
                && (request.command != MigrationCommand::DelegationAck
                    || delegated_approval(&plan)?)
        }
        (true, MigrationCommand::Stop) => {
            record.phase == MigrationPhase::Stopping
                && record.prepared_profiles.contains(&profile.instance_id)
                && profile.instance_id == context.instance_id
                && plan.profiles.iter().all(|profile| {
                    record.prepared_profiles.contains(&profile.instance_id)
                        && record.acknowledged_profiles.contains(&profile.instance_id)
                })
                && context.running == plan.source_image
        }
        (true, MigrationCommand::Abort) => {
            matches!(
                record.phase,
                MigrationPhase::RollingBack | MigrationPhase::UserStopped | MigrationPhase::Failed
            ) && record.preparing_profiles.contains(&profile.instance_id)
                && context.running == plan.source_image
        }
        (false, MigrationCommand::Commit) => {
            record.native_ready
                && record.target_profiles.iter().any(|target| {
                    target.config_dir == context.config_dir
                        && target.state_dir == context.state_dir
                        && target.pid == Some(std::process::id())
                        && target.instance_id.as_deref() == Some(context.instance_id.as_str())
                        && target.native_ready
                        && target.commit_requested
                        && target.image == context.running
                })
                && matches!(
                    record.phase,
                    MigrationPhase::NativeReady
                        | MigrationPhase::Integrating
                        | MigrationPhase::Retiring
                        | MigrationPhase::RetirementFailed
                        | MigrationPhase::ManagerUnknown
                        | MigrationPhase::AwaitingReconnect
                        | MigrationPhase::Complete
                )
                && &context.running == target_image(&plan, &record)?
        }
        (false, MigrationCommand::Stop) => {
            matches!(
                record.phase,
                MigrationPhase::RollingBack
                    | MigrationPhase::UserStopped
                    | MigrationPhase::RetirementFailed
            ) && &context.running == target_image(&plan, &record)?
        }
        _ => false,
    };
    if !admitted {
        return Err("migration journal phase does not authorize this command".into());
    }
    Ok(())
}

fn make_record(plan: &MigrationPlan, bytes: &[u8]) -> MigrationRecord {
    MigrationRecord {
        version: 1,
        operation_id: plan.operation_id.clone(),
        plan_sha256: format!("{:x}", Sha256::digest(bytes)),
        phase: MigrationPhase::Validating,
        prepared_profiles: Vec::new(),
        preparing_profiles: Vec::new(),
        acknowledged_profiles: Vec::new(),
        stopped_profiles: Vec::new(),
        target_pid: None,
        target_instance: None,
        locator_adopted: false,
        default_adopted: false,
        native_ready: false,
        target_committed: false,
        target_profiles: Vec::new(),
        stopped_target_profiles: Vec::new(),
        restored_profiles: Vec::new(),
        manager: None,
        target_manager: None,
        target_image: None,
        reconnected_profiles: Vec::new(),
        integration_status: Vec::new(),
        integration_delivery_error: None,
        retirement_attempt: 0,
        error: None,
    }
}
fn pointer_bytes(plan: &MigrationPlan) -> Result<(Vec<u8>, Option<Vec<u8>>), String> {
    let kind = match &plan.target.origin {
        InstallOrigin::Homebrew { .. } => "homebrew",
        InstallOrigin::Standalone { .. } => "standalone",
        InstallOrigin::Source { .. } => "source",
        _ => return Err("neutral target must have a supported verified owner".into()),
    };
    let locator = serde_json::to_vec(&serde_json::json!({
        "version":1,"executable":plan.target.image.path,"kind":kind
    }))
    .map_err(|e| e.to_string())?;
    let selected = &plan.profiles[0];
    let wanted = serde_json::json!({"version":1,"config_dir":selected.config_dir,"state_dir":selected.state_dir});
    let adopt = if plan.adopt_as_default {
        match &plan.previous_default_profile.bytes {
            None => true,
            Some(bytes) => {
                let old: serde_json::Value =
                    serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
                // An existing different default is never overwritten by migration consent.
                old == wanted
            }
        }
    } else {
        false
    };
    Ok((
        locator,
        if adopt {
            Some(serde_json::to_vec(&wanted).map_err(|e| e.to_string())?)
        } else {
            None
        },
    ))
}
fn adopt_pointers(plan: &MigrationPlan, record: &mut MigrationRecord) -> Result<(), String> {
    unchanged(&plan.previous_locator)?;
    unchanged(&plan.previous_default_profile)?;
    ensure_private_directory(
        plan.previous_locator
            .path
            .parent()
            .ok_or("locator parent missing")?,
    )?;
    let (locator, profile) = pointer_bytes(plan)?;
    if profile.is_some() {
        ensure_private_directory(
            plan.previous_default_profile
                .path
                .parent()
                .ok_or("profile pointer parent missing")?,
        )?;
    }
    // Intent precedes each pointer write. Rollback also covers a crash between
    // the atomic replacement and its acknowledgement in the journal.
    record.locator_adopted = true;
    save(plan, record)?;
    protocol::write_private_bytes(&plan.previous_locator.path, &locator, 0o600)?;
    if let Some(bytes) = profile {
        record.default_adopted = true;
        save(plan, record)?;
        protocol::write_private_bytes(&plan.previous_default_profile.path, &bytes, 0o600)?;
    }
    Ok(())
}
fn restore_pointer(previous: &PreviousRecord, adopted: &[u8]) -> Result<(), String> {
    let current = match fs::symlink_metadata(&previous.path) {
        Ok(_) => Some(owned_private_file(&previous.path, 16384)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    if current == previous.bytes {
        return Ok(());
    }
    if current.as_deref() != Some(adopted) {
        return Err("pointer changed externally; exact rollback cannot overwrite it".into());
    }
    match &previous.bytes {
        Some(bytes) => protocol::write_private_bytes(&previous.path, bytes, 0o600),
        None => {
            fs::remove_file(&previous.path).map_err(|e| e.to_string())?;
            File::open(previous.path.parent().ok_or("pointer parent missing")?)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())
        }
    }
}
fn restore_pointers(plan: &MigrationPlan, record: &MigrationRecord) -> Result<(), String> {
    let (locator, profile) = pointer_bytes(plan)?;
    if record.default_adopted {
        restore_pointer(
            &plan.previous_default_profile,
            &profile.ok_or("journal claims an unapproved default adoption")?,
        )?;
    }
    if record.locator_adopted {
        restore_pointer(&plan.previous_locator, &locator)?;
    }
    Ok(())
}
fn neutral_environment_key(key: &str) -> bool {
    matches!(
        key,
        "HERDR_BIN_PATH"
            | "HERDR_SOCKET_PATH"
            | "HERDR_ENV"
            | "HOME"
            | "XDG_CONFIG_HOME"
            | "XDG_STATE_HOME"
            | "TERN_CONFIG_DIR"
            | "TERN_DAEMON_SOCKET"
    )
}
fn neutral_environment(profile: &LegacyProfile) -> BTreeMap<String, String> {
    profile
        .environment
        .iter()
        .filter(|(key, _)| neutral_environment_key(key))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
fn launch(
    plan: &MigrationPlan,
    profile: &LegacyProfile,
    target: bool,
    authorization: (u64, String),
) -> Result<u32, String> {
    if stop_requested(plan)? {
        return Err("user Stop prevents migration launch".into());
    }
    let target_record = target.then(|| read_record(plan)).transpose()?;
    let image = if let Some(record) = &target_record {
        target_image(plan, record)?
    } else {
        &plan.source_image
    };
    if executable_identity(&image.path)? != *image {
        return Err("authorized launch image changed".into());
    }
    let mut environment = if target {
        neutral_environment(profile)
    } else {
        profile.environment.clone()
    };
    if let Some(socket) = &profile.herdr_socket {
        environment.insert(
            "HERDR_SOCKET_PATH".into(),
            socket.to_str().ok_or("host socket is not UTF8")?.into(),
        );
    }
    let mut command = Command::new(&image.path);
    command
        .args([
            "--config-dir",
            profile.config_dir.to_str().ok_or("profile is not UTF8")?,
            "--state-dir",
            profile.state_dir.to_str().ok_or("profile is not UTF8")?,
            "--host",
            profile.observation.as_str(),
        ])
        .env_clear()
        .envs(environment)
        .env("PATH", "/usr/bin:/bin")
        .env(PLAN_ENV, evidence_path(plan))
        .env(OPERATION_ENV, &plan.operation_id)
        .env(TOKEN_ENV, &plan.token)
        .env(
            if target {
                crate::identity::neutral::UPDATER_TOKEN_ENV
            } else {
                "HERDR_DESKTOP_PET_UPDATER_TOKEN"
            },
            authorization.1,
        )
        .env(
            if target {
                crate::identity::neutral::UPDATER_GENERATION_ENV
            } else {
                "HERDR_DESKTOP_PET_UPDATER_GENERATION"
            },
            authorization.0.to_string(),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    if !target {
        if let Some(host_config) = &profile.host_plugin_config_dir {
            command.env(
                "HERDR_DESKTOP_PET_CAPTURED_HOST_PLUGIN_CONFIG_DIR",
                host_config,
            );
        }
    }
    if let Some(assets) = &profile.assets_override {
        command.args([
            "--assets",
            assets.to_str().ok_or("renderer assets are not UTF8")?,
        ]);
    }
    command.arg("daemon");
    // Final Stop check is adjacent to spawn; daemon independently checks the
    // durable generation under the startup gate before any mutation.
    if stop_requested(plan)? {
        return Err("user Stop prevents migration launch".into());
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let pid = child.id();
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}
fn wait_ready(
    plan: &MigrationPlan,
    profile: &LegacyProfile,
    pid: u32,
    target: bool,
    presentation: Option<&dyn MigrationPresentation>,
) -> Result<LiveStatus, String> {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        pump(presentation);
        if stop_requested(plan)? {
            return Err("user Stop during readiness".into());
        }
        if !process_live(pid) {
            return Err("launched daemon exited before readiness".into());
        }
        if let Ok(live) = status(profile) {
            if live.pid != pid {
                return Err("different daemon owns migration profile".into());
            }
            let mut expected = profile.clone();
            if !target {
                expected.pid = live.pid;
                expected.instance_id = live.instance_id.clone();
            }
            if verify_status(plan, &expected, &live, target).is_ok() {
                return Ok(live);
            }
        }
        if Instant::now() >= until {
            return Err("migration target did not prove actual native readiness".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
}
fn preserve_bridge(plan: &MigrationPlan) -> Result<(), String> {
    let backup = plan.profiles[0]
        .state_dir
        .join("updates-v4")
        .join(format!("{}.bridge-evidence.app", plan.operation_id));
    if backup.exists() {
        return Err("bridge evidence already exists; operation must not replay".into());
    }
    command_output(
        Path::new("/bin/cp"),
        &[
            "-R".into(),
            source_bundle(plan)?.to_string_lossy().into_owned(),
            backup.to_string_lossy().into_owned(),
        ],
    )?;
    let copied = executable_identity(&backup.join("Contents/MacOS/herdr-desktop-pet"))?;
    if copied.sha256 != plan.source_image.sha256 {
        return Err("retained bridge evidence differs from source".into());
    }
    #[cfg(target_os = "macos")]
    command_output(
        Path::new("/usr/bin/codesign"),
        &[
            "--verify".into(),
            "--deep".into(),
            "--strict".into(),
            backup.to_string_lossy().into_owned(),
        ],
    )?;
    Ok(())
}
#[cfg(not(desktop_pet_legacy))]
fn migration_profiles(plan: &MigrationPlan) -> Vec<protocol::MigrationProfile> {
    plan.profiles
        .iter()
        .map(|p| protocol::MigrationProfile {
            pid: p.pid,
            instance_id: p.instance_id.clone(),
            state_dir: p.state_dir.clone(),
            config_dir: p.config_dir.clone(),
            image: plan.source_image.clone(),
            stop_generation: p.stop_generation,
        })
        .collect()
}
fn roots(plan: &MigrationPlan) -> Result<Vec<PathBuf>, String> {
    Ok(vec![
        plan.source_origin.root().to_path_buf(),
        crate::physical_install_root(&plan.target.origin)?.to_path_buf(),
    ])
}

/// Initial runs are single-use. Recovery never silently repeats installation,
/// launch or a manager process whose durable intent might have executed.
#[cfg(not(desktop_pet_legacy))]
pub fn run_with_presentation(
    path: &Path,
    presentation: &dyn MigrationPresentation,
) -> Result<MigrationRecord, String> {
    run_impl(path, Some(presentation))
}
#[cfg(not(desktop_pet_legacy))]
pub fn run_delegated(path: &Path, retirement_only: bool) -> Result<MigrationRecord, String> {
    let plan = read_plan(path)?;
    if !delegated_approval(&plan)? {
        return Err("automatic migration requires exact delegated update approval".into());
    }
    if retirement_only {
        resume_retirement_impl(path, None)
    } else {
        run_impl(path, None)
    }
}
#[cfg(not(desktop_pet_legacy))]
fn run_impl(
    path: &Path,
    presentation: Option<&dyn MigrationPresentation>,
) -> Result<MigrationRecord, String> {
    let plan = read_plan(path)?;
    visible(presentation, &plan)?;
    let admission = protocol::admit_legacy_migration(
        &roots(&plan)?,
        &migration_profiles(&plan),
        &plan.readonly_target_profiles,
        &plan.operation_id,
    )?;
    let _lease = admission.lock_operation()?;
    let bytes = owned_private_file(path, MAX_PLAN)?;
    let captured: MigrationPlan = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if captured != plan {
        return Err("consent plan changed while acquiring migration ownership".into());
    }
    if journal_path(&plan).exists() || evidence_path(&plan).exists() {
        return Err(
            "migration already has durable evidence; initial execution cannot replay".into(),
        );
    }
    for profile in &plan.profiles {
        owned_directory(&profile.config_dir)?;
        owned_directory(&profile.state_dir)?;
    }
    unchanged(&plan.previous_locator)?;
    unchanged(&plan.previous_default_profile)?;
    verify_source(&plan)?;
    ancestry::verify_source_ancestry(&plan)?;
    for profile in &plan.profiles {
        verify_status(&plan, profile, &status(profile)?, false)?;
    }
    admission.recheck_profiles()?;
    if admission.stop_requested()? || stop_requested(&plan)? {
        return Err("user Stop superseded migration consent".into());
    }
    // This immutable copy is the only plan accepted by bridge and target auth.
    protocol::write_private_new(&evidence_path(&plan), &bytes, 0o600)?;
    let mut record = make_record(&plan, &bytes);
    save(&plan, &record)?;
    let reservation = admission.reserve(&plan.token)?;
    let result = execute(&plan, &reservation, &mut record, presentation);
    match result {
        Ok(()) => Ok(record),
        Err(error) => {
            record.error = Some(error);
            if commit_may_have_applied(&record) || record.phase == MigrationPhase::ManagerUnknown {
                if !record.target_committed
                    && record
                        .target_profiles
                        .iter()
                        .any(|target| target.commit_requested || target.committed)
                {
                    record.error = record.error.take().map(|error| format!(
                        "Partial or uncertain per-profile Commit; committed targets retained, remaining profiles not confirmed; legacy retirement pending. {error}"
                    ));
                }
                if stop_requested(&plan)? {
                    transition(
                        &plan,
                        &mut record,
                        MigrationPhase::UserStopped,
                        presentation,
                    )?;
                    stop_launched_targets(&plan, &mut record, true, presentation)?;
                } else if record.phase != MigrationPhase::ManagerUnknown {
                    transition(
                        &plan,
                        &mut record,
                        MigrationPhase::RetirementFailed,
                        presentation,
                    )?;
                } else {
                    save(&plan, &record)?;
                }
                // A partial/uncertain Commit never rolls back an already committed target.
                // Keep ownership and the old installation until integration/retirement resolves.
                Ok(record)
            } else {
                rollback(&plan, &reservation, &mut record, presentation)?;
                Ok(record)
            }
        }
    }
}
#[cfg(not(desktop_pet_legacy))]
fn execute(
    plan: &MigrationPlan,
    reservation: &protocol::MigrationReservation,
    record: &mut MigrationRecord,
    presentation: Option<&dyn MigrationPresentation>,
) -> Result<(), String> {
    if let Some(presentation) = presentation {
        presentation.show(MigrationPhase::Validating, None);
    }
    let staged = if presentation.is_some() {
        let stage_plan = plan.clone();
        off_ui(presentation, move || {
            crate::installation::stage_migration_target(&stage_plan)
        })?
    } else {
        crate::installation::stage_migration_target(plan)?
    };
    if presentation.is_some() {
        let backup_plan = plan.clone();
        off_ui(presentation, move || preserve_bridge(&backup_plan))?;
    } else {
        preserve_bridge(plan)?;
    }
    transition(plan, record, MigrationPhase::Staged, presentation)?;
    install_target_manager(plan, record, presentation)?;
    if record.target_manager.is_some() {
        let installed_stage = crate::installation::stage_migration_target_with_image(
            plan,
            target_image(plan, record)?,
        )?;
        if installed_stage != staged {
            return Err("target owner upgrade changed the approved signed stage".into());
        }
    }
    if presentation.is_some() {
        let ref_plan = plan.clone();
        off_ui(presentation, move || verify_integration_ref(&ref_plan))?;
    } else {
        verify_integration_ref(plan)?;
    }
    if stop_requested(plan)? {
        return Err("user Stop before Prepare".into());
    }
    visible(presentation, plan)?;
    transition(plan, record, MigrationPhase::Preparing, presentation)?;
    for profile in &plan.profiles {
        if stop_requested(plan)? {
            return Err("user Stop during all-profile Prepare".into());
        }
        verify_status(plan, profile, &status(profile)?, false)?;
        record.preparing_profiles.push(profile.instance_id.clone());
        save(plan, record)?;
        presented_control(
            plan,
            profile,
            &profile.instance_id,
            MigrationCommand::Prepare,
            false,
            presentation,
        )?;
        record.prepared_profiles.push(profile.instance_id.clone());
        save(plan, record)?;
    }
    // Installation proof is repeated after Prepare; no preference/schema
    // mutations or host work can now race the source shutdown.
    verify_source(plan)?;
    visible(presentation, plan)?;
    let delegated = delegated_approval(plan)?;
    for profile in &plan.profiles {
        if !delegated {
            assert_final_source_visibility(plan, profile)?;
        }
        presented_control(
            plan,
            profile,
            &profile.instance_id,
            if delegated {
                MigrationCommand::DelegationAck
            } else {
                MigrationCommand::ConsentAck
            },
            false,
            presentation,
        )?;
        record
            .acknowledged_profiles
            .push(profile.instance_id.clone());
        save(plan, record)?;
    }
    visible(presentation, plan)?;
    write_authorization_ack(plan, record, false)?;
    transition(plan, record, MigrationPhase::Stopping, presentation)?;
    for profile in &plan.profiles {
        if stop_requested(plan)? {
            return Err("user Stop during source shutdown".into());
        }
        presented_control(
            plan,
            profile,
            &profile.instance_id,
            MigrationCommand::Stop,
            false,
            presentation,
        )?;
        wait_stopped(profile.pid, presentation)?;
        record.stopped_profiles.push(profile.instance_id.clone());
        save(plan, record)?;
    }
    reservation.profiles_stopped()?;
    if stop_requested(plan)? {
        return Err("user Stop before adoption".into());
    }
    let installed_image = target_image(plan, record)?.clone();
    if reservation.target_reuse_present()? {
        let existing = crate::installation::inspect(&installed_image.path)?;
        if existing.image != installed_image
            || existing.origin != plan.target.origin
            || existing.app_version != crate::identity::neutral::VERSION
        {
            return Err(
                "live read-only target installation changed; migration may not replace it".into(),
            );
        }
    } else {
        crate::installation::commit_migration_target(plan, &staged, &installed_image)?;
    }
    if executable_identity(&installed_image.path)? != installed_image {
        return Err(
            "installed target differs from its verified approved release derivation".into(),
        );
    }
    transition(plan, record, MigrationPhase::Adopting, presentation)?;
    reservation.with_pointer_mutation(|| adopt_pointers(plan, record))?;
    if stop_requested(plan)? {
        return Err("user Stop before target launch".into());
    }
    transition(plan, record, MigrationPhase::Starting, presentation)?;
    record.target_profiles = plan
        .profiles
        .iter()
        .map(|profile| MigrationTargetProfile {
            config_dir: profile.config_dir.clone(),
            state_dir: profile.state_dir.clone(),
            pid: None,
            instance_id: None,
            image: installed_image.clone(),
            native_ready: false,
            committed: false,
            commit_requested: false,
        })
        .collect();
    save(plan, record)?;
    integrations::retain_profile_plans(plan, record)?;
    for (index, profile) in plan.profiles.iter().enumerate() {
        let authorization = reservation.authorize_start(
            &plan.target.origin,
            &installed_image,
            &profile.state_dir,
        )?;
        let pid = launch(plan, profile, true, authorization)?;
        record.target_profiles[index].pid = Some(pid);
        if index == 0 {
            record.target_pid = Some(pid);
        }
        save(plan, record)?;
        let live = wait_ready(plan, profile, pid, true, presentation)?;
        record.target_profiles[index].instance_id = Some(live.instance_id.clone());
        record.target_profiles[index].native_ready = true;
        if index == 0 {
            record.target_instance = Some(live.instance_id);
        }
        save(plan, record)?;
    }
    sync_target_aggregates(plan, record);
    transition(plan, record, MigrationPhase::NativeReady, presentation)?;
    for (index, profile) in plan.profiles.iter().enumerate() {
        if stop_requested(plan)? {
            return Err("user Stop before per-profile migration commit".into());
        }
        let instance = record.target_profiles[index]
            .instance_id
            .clone()
            .ok_or("native-ready target instance missing")?;
        record.target_profiles[index].commit_requested = true;
        save(plan, record)?;
        presented_control(
            plan,
            profile,
            &instance,
            MigrationCommand::Commit,
            true,
            presentation,
        )?;
        record.target_profiles[index].committed = true;
        sync_target_aggregates(plan, record);
        save(plan, record)?;
    }
    integrate_before_retirement(plan, record, presentation)?;
    retire(plan, reservation, record, presentation)?;
    Ok(())
}

fn target_profiles_complete(plan: &MigrationPlan, record: &MigrationRecord) -> bool {
    let image = record.target_image.as_ref().unwrap_or(&plan.target.image);
    record.target_profiles.len() == plan.profiles.len()
        && !plan.profiles.is_empty()
        && plan.profiles.iter().all(|profile| {
            record
                .target_profiles
                .iter()
                .filter(|target| {
                    target.config_dir == profile.config_dir
                        && target.state_dir == profile.state_dir
                        && target
                            .pid
                            .is_some_and(|pid| pid != 0 && pid <= i32::MAX as u32)
                        && target.instance_id.as_deref().is_some_and(instance_valid)
                        && &target.image == image
                })
                .count()
                == 1
        })
}

fn sync_target_aggregates(plan: &MigrationPlan, record: &mut MigrationRecord) {
    let complete_set = target_profiles_complete(plan, record);
    record.native_ready = complete_set
        && record
            .target_profiles
            .iter()
            .all(|target| target.native_ready);
    record.target_committed =
        complete_set && record.target_profiles.iter().all(|target| target.committed);
}

fn retirement_gate(
    plan: &MigrationPlan,
    record: &MigrationRecord,
    integrations_confirmed: bool,
) -> Result<(), String> {
    if !target_profiles_complete(plan, record)
        || !record.native_ready
        || !record.target_committed
        || record
            .target_profiles
            .iter()
            .any(|target| !target.native_ready || !target.committed)
    {
        return Err(
            "legacy retirement requires every approved profile's own native-ready durable Commit"
                .into(),
        );
    }
    if !integrations_confirmed {
        return Err(
            "app committed; actual SDK handover/binding missing; legacy retirement pending".into(),
        );
    }
    Ok(())
}

fn commit_may_have_applied(record: &MigrationRecord) -> bool {
    record.target_committed
        || record
            .target_profiles
            .iter()
            .any(|target| target.committed || target.commit_requested)
}

#[cfg(not(desktop_pet_legacy))]
fn integrate_before_retirement(
    plan: &MigrationPlan,
    record: &mut MigrationRecord,
    presentation: Option<&dyn MigrationPresentation>,
) -> Result<(), String> {
    verify_all_committed_targets(plan, record)?;
    transition(plan, record, MigrationPhase::Integrating, presentation)?;
    if stop_requested(plan)? {
        return Err("user Stop before integration delivery".into());
    }
    let delivery = if presentation.is_some() {
        thread::scope(|scope| {
            let task = scope.spawn(|| integrations::begin_fresh_integrations(plan, record));
            while !task.is_finished() {
                pump(presentation);
                thread::sleep(Duration::from_millis(25));
            }
            task.join()
                .map_err(|_| "migration consumer delivery worker panicked".to_string())?
        })
    } else {
        integrations::begin_fresh_integrations(plan, record)
    };
    if let Err(error) = delivery {
        record.integration_delivery_error = Some(error.clone());
        save(plan, record)?;
        return Err(format!(
            "app committed; integration unavailable; legacy retirement pending: {error}"
        ));
    }
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        pump(presentation);
        if stop_requested(plan)? {
            return Err("user Stop while awaiting actual integration binding".into());
        }
        verify_all_committed_targets(plan, record)?;
        if integrations::refresh_fresh_integrations(plan, record)? {
            save(plan, record)?;
            return Ok(());
        }
        save(plan, record)?;
        if Instant::now() >= until {
            let error = "app committed; actual SDK load/binding was not confirmed for every required profile; legacy installation retained and retirement pending".to_string();
            record.integration_delivery_error = Some(error.clone());
            save(plan, record)?;
            return Err(error);
        }
        thread::sleep(Duration::from_millis(200));
    }
}
#[cfg(not(desktop_pet_legacy))]
fn rollback(
    plan: &MigrationPlan,
    reservation: &protocol::MigrationReservation,
    record: &mut MigrationRecord,
    presentation: Option<&dyn MigrationPresentation>,
) -> Result<(), String> {
    let stopped_by_user = stop_requested(plan)?;
    record.native_ready = false;
    transition(
        plan,
        record,
        if stopped_by_user {
            MigrationPhase::UserStopped
        } else {
            MigrationPhase::RollingBack
        },
        presentation,
    )?;
    stop_launched_targets(plan, record, stopped_by_user, presentation)?;
    reservation.with_pointer_mutation(|| restore_pointers(plan, record))?;
    if stopped_by_user || stop_requested(plan)? {
        for profile in &plan.profiles {
            if record.preparing_profiles.contains(&profile.instance_id) && process_live(profile.pid)
            {
                abort_live_preparation(plan, profile, presentation)?;
            }
        }
        transition(plan, record, MigrationPhase::UserStopped, presentation)?;
        reservation.release()?;
        return Ok(());
    }
    verify_source(plan)?;
    for profile in &plan.profiles {
        if stop_requested(plan)? {
            transition(plan, record, MigrationPhase::UserStopped, presentation)?;
            return Ok(());
        }
        if record.prepared_profiles.contains(&profile.instance_id) && !process_live(profile.pid) {
            let origin = rollback_origin(plan)?;
            let authorization =
                reservation.authorize_start(&origin, &plan.source_image, &profile.state_dir)?;
            let pid = launch(plan, profile, false, authorization)?;
            let live = wait_ready(plan, profile, pid, false, presentation)?;
            record.restored_profiles.push(protocol::MigrationProfile {
                pid: live.pid,
                instance_id: live.instance_id,
                config_dir: profile.config_dir.clone(),
                state_dir: profile.state_dir.clone(),
                image: plan.source_image.clone(),
                stop_generation: profile.stop_generation,
            });
            save(plan, record)?;
        } else if record.preparing_profiles.contains(&profile.instance_id)
            && process_live(profile.pid)
        {
            abort_live_preparation(plan, profile, presentation)?;
        }
    }
    transition(plan, record, MigrationPhase::RolledBack, presentation)?;
    reservation.release()
}

#[cfg(not(desktop_pet_legacy))]
fn stop_launched_targets(
    plan: &MigrationPlan,
    record: &mut MigrationRecord,
    include_committed: bool,
    presentation: Option<&dyn MigrationPresentation>,
) -> Result<(), String> {
    for index in 0..record.target_profiles.len() {
        let target = &record.target_profiles[index];
        if !include_committed && (target.committed || target.commit_requested) {
            continue;
        }
        let Some(pid) = target.pid else {
            continue;
        };
        if !process_live(pid) {
            continue;
        }
        let profile = plan
            .profiles
            .iter()
            .find(|profile| {
                profile.config_dir == target.config_dir && profile.state_dir == target.state_dir
            })
            .ok_or("rollback target profile was not approved")?;
        if actual_process_image(pid)? != target.image.path
            || executable_identity(&target.image.path)? != target.image
        {
            return Err("rollback target PID/image no longer matches the authorized child".into());
        }
        match status(profile) {
            Ok(live) => {
                if live.pid != pid
                    || target
                        .instance_id
                        .as_ref()
                        .is_some_and(|instance| instance != &live.instance_id)
                {
                    return Err("rollback target control socket/instance changed".into());
                }
                control(
                    plan,
                    profile,
                    &live.instance_id,
                    MigrationCommand::Stop,
                    true,
                )?;
            }
            Err(_) if target.instance_id.is_none() => {
                // Not-ready child has not exposed its control socket yet.
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
            Err(error) => {
                return Err(format!(
                    "cannot authenticate ready target shutdown: {error}"
                ))
            }
        }
        wait_stopped(pid, presentation)?;
        record.stopped_target_profiles.push(MigrationProfilePaths {
            config_dir: profile.config_dir.clone(),
            state_dir: profile.state_dir.clone(),
        });
        save(plan, record)?;
    }
    Ok(())
}
fn rollback_origin(plan: &MigrationPlan) -> Result<InstallOrigin, String> {
    Ok(match &plan.source_origin {
        LegacyOrigin::Homebrew {
            brew,
            prefix,
            cellar_formula_root,
            formula,
        } => InstallOrigin::Homebrew {
            brew: brew.path.clone(),
            prefix: prefix.clone(),
            cellar_formula_root: cellar_formula_root.clone(),
            formula: formula.clone(),
            locator: plan.source_image.path.clone(),
        },
        LegacyOrigin::HerdrManaged { checkout_root, .. }
        | LegacyOrigin::Generated {
            source_root: checkout_root,
            ..
        } => InstallOrigin::Source {
            root: checkout_root.clone(),
        },
        LegacyOrigin::Manual { .. } => InstallOrigin::Manual {
            locator: plan.source_image.path.clone(),
        },
    })
}

fn manager_can_resume(item: &ManagerEvidence) -> bool {
    item.pid.is_some() && item.pgid == item.pid && item.exit_code.is_some() && item.quiescent
}
fn manager_group_stopped(pid: u32) -> Result<bool, String> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Err("invalid journaled manager group".into());
    }
    let rc = unsafe { libc::kill(-(pid as i32), 0) };
    if rc == 0 {
        return Ok(false);
    }
    if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(true)
    } else {
        Err("manager process group state is unknown".into())
    }
}
fn manager_environment(plan: &MigrationPlan) -> Result<BTreeMap<String, String>, String> {
    let mut environment = plan.profiles[0].environment.clone();
    environment.remove("HERDR_BIN_PATH");
    environment.remove("HERDR_PET_OBSERVATION_MODE");
    if let Some(socket) = &plan.profiles[0].herdr_socket {
        environment.insert(
            "HERDR_SOCKET_PATH".into(),
            socket
                .to_str()
                .ok_or("captured host socket is not UTF8")?
                .into(),
        );
    }
    if let LegacyOrigin::HerdrManaged {
        host_config_dir, ..
    } = &plan.source_origin
    {
        let base = environment
            .get("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                environment
                    .get("HOME")
                    .map(|home| Path::new(home).join(".config"))
            })
            .ok_or("managed refresh lacks captured host home/config routing")?;
        let name = host_config_dir
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("invalid Herdr registry root")?;
        if !matches!(name, "herdr" | "herdr-dev")
            || fs::canonicalize(base.join(name)).map_err(|e| e.to_string())? != *host_config_dir
        {
            return Err("captured manager environment would select another registry".into());
        }
    }
    environment.insert(
        "PATH".into(),
        "/usr/bin:/bin:/opt/homebrew/bin:/usr/local/bin".into(),
    );
    environment.insert("HOMEBREW_NO_AUTO_UPDATE".into(), "1".into());
    environment.insert("HOMEBREW_NO_ANALYTICS".into(), "1".into());
    environment.insert("HOMEBREW_NO_INSTALL_CLEANUP".into(), "1".into());
    environment.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
    environment.insert("GIT_CONFIG_NOSYSTEM".into(), "1".into());
    environment.insert("GIT_CONFIG_GLOBAL".into(), "/dev/null".into());
    environment.insert("GIT_ASKPASS".into(), "/usr/bin/false".into());
    Ok(environment)
}
fn persist_manager(
    plan: &MigrationPlan,
    record: &mut MigrationRecord,
    item: ManagerEvidence,
    installation: bool,
) -> Result<(), String> {
    let suffix = if installation {
        "target-manager"
    } else {
        "retirement-manager"
    };
    let attempt = if installation {
        0
    } else {
        record.retirement_attempt
    };
    let path = plan.profiles[0]
        .state_dir
        .join("updates-v4")
        .join(format!("{}.{}-{}.json", plan.operation_id, suffix, attempt));
    protocol::write_private_bytes(
        &path,
        &serde_json::to_vec(&item).map_err(|e| e.to_string())?,
        0o600,
    )?;
    if installation {
        record.target_manager = Some(item);
    } else {
        record.manager = Some(item);
    }
    save(plan, record)
}
fn execute_manager(
    plan: &MigrationPlan,
    record: &mut MigrationRecord,
    program: &ExecutableIdentity,
    args: Vec<String>,
    installation: bool,
    presentation: Option<&dyn MigrationPresentation>,
) -> Result<(), String> {
    if stop_requested(plan)? {
        return Err("user Stop prevents manager launch".into());
    }
    if executable_identity(&program.path)? != *program {
        return Err("consented manager executable changed".into());
    }
    let environment = manager_environment(plan)?;
    let mut item = ManagerEvidence {
        program: program.clone(),
        args: args.clone(),
        intent_at: crate::now()?,
        pid: None,
        pgid: None,
        exit_code: None,
        quiescent: false,
    };
    // Intent is durable before spawn. A missing PID is unknown, not NoSpawn.
    persist_manager(plan, record, item.clone(), installation)?;
    transition(plan, record, MigrationPhase::ManagerUnknown, presentation)?;
    if stop_requested(plan)? {
        return Err("user Stop after durable manager intent".into());
    }
    let base = plan.profiles[0].state_dir.join("updates-v4");
    let output = |suffix: &str| -> Result<File, String> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(base.join(format!(
                "{}.manager-{}-{}.{}",
                plan.operation_id,
                if installation { "target" } else { "retire" },
                record.retirement_attempt,
                suffix
            )))
            .map_err(|e| e.to_string())
    };
    fn drain(mut pipe: impl Read, mut output: File) {
        let mut buffer = [0u8; 8192];
        let mut retained = 0usize;
        while let Ok(n) = pipe.read(&mut buffer) {
            if n == 0 {
                break;
            }
            let keep = n.min(65536usize.saturating_sub(retained));
            if keep > 0 {
                let _ = output.write_all(&buffer[..keep]);
                retained += keep;
            }
        }
        let _ = output.sync_all();
    }
    let stdout_file = output("stdout")?;
    let stderr_file = output("stderr")?;
    if stop_requested(plan)? {
        return Err("user Stop immediately before manager spawn; durable intent retained".into());
    }
    let mut child = Command::new(&program.path)
        .args(args)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| e.to_string())?;
    item.pid = Some(child.id());
    item.pgid = Some(child.id());
    persist_manager(plan, record, item.clone(), installation)?;
    let stdout = child.stdout.take().ok_or("manager stdout unavailable")?;
    let stderr = child.stderr.take().ok_or("manager stderr unavailable")?;
    let out = thread::spawn(move || drain(stdout, stdout_file));
    let err = thread::spawn(move || drain(stderr, stderr_file));
    let deadline = Instant::now() + Duration::from_secs(300);
    let result = loop {
        pump(presentation);
        if let Some(exit) = child.try_wait().map_err(|e| e.to_string())? {
            break exit;
        }
        if Instant::now() >= deadline {
            // Preserve unknown. A manager may already have committed while
            // descendants are still running; timeout is not proof of failure.
            thread::spawn(move || {
                let _ = child.wait();
            });
            return Err(
                "manager outcome unknown; explicit external reconciliation required".into(),
            );
        }
        thread::sleep(Duration::from_millis(100));
    };
    item.exit_code = result.code();
    persist_manager(plan, record, item.clone(), installation)?;
    while !manager_group_stopped(item.pid.ok_or("manager PID not journaled")?)?
        || !out.is_finished()
        || !err.is_finished()
    {
        pump(presentation);
        if Instant::now() >= deadline {
            return Err("manager descendants/output remain unknown; never replay".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
    out.join().map_err(|_| "manager stdout drain failed")?;
    err.join().map_err(|_| "manager stderr drain failed")?;
    item.quiescent = true;
    persist_manager(plan, record, item, installation)?;
    transition(
        plan,
        record,
        if installation {
            MigrationPhase::Staged
        } else {
            MigrationPhase::Retiring
        },
        presentation,
    )?;
    if !result.success() {
        return Err(format!("manager exited unsuccessfully: {result}"));
    }
    Ok(())
}
#[cfg(not(desktop_pet_legacy))]
fn install_target_manager(
    plan: &MigrationPlan,
    record: &mut MigrationRecord,
    presentation: Option<&dyn MigrationPresentation>,
) -> Result<(), String> {
    if let InstallOrigin::Homebrew { brew, formula, .. } = &plan.target.origin {
        if plan.target.image.path.exists() {
            if executable_identity(&plan.target.image.path)? != plan.target.image {
                return Err("existing neutral Brew image differs from consent".into());
            }
            return Ok(());
        }
        if formula != "hanbong5938/tap/desktop-pet" {
            return Err("neutral Brew target formula is not approved".into());
        }
        let manager = plan
            .target_manager_program
            .as_ref()
            .ok_or("neutral Brew install lacks consented manager image")?;
        if &manager.path != brew {
            return Err("neutral Brew manager path changed".into());
        }
        crate::origin::verify_migration_brew_release(
            brew,
            formula,
            &plan.target.release,
            &manager_environment(plan)?,
        )?;
        execute_manager(
            plan,
            record,
            manager,
            vec![
                if plan.target.previous_installation.is_some() {
                    "upgrade".into()
                } else {
                    "install".into()
                },
                formula.clone(),
            ],
            true,
            presentation,
        )?;
        record.target_image = Some(crate::installation::installed_migration_candidate(plan)?);
        target_image(plan, record)?;
        save(plan, record)?;
    }
    Ok(())
}
fn retired(plan: &MigrationPlan) -> Result<(), String> {
    if plan.source_image.path.exists() || source_bundle(plan)?.exists() {
        return Err("legacy image/bundle remains installed".into());
    }
    if let LegacyOrigin::Homebrew { prefix, .. } = &plan.source_origin {
        for name in ["herdr-desktop-pet", "herdr-update-coordinator"] {
            if fs::symlink_metadata(prefix.join("bin").join(name)).is_ok() {
                return Err("legacy CLI alias remains published".into());
            }
        }
    }
    for alias in &plan.legacy_aliases {
        if fs::symlink_metadata(alias).is_ok() {
            return Err("consented legacy CLI alias remains published".into());
        }
    }
    crate::processes::installation_roots_inactive(&[plan.source_origin.root().to_path_buf()])
}
#[cfg(not(desktop_pet_legacy))]
fn retire(
    plan: &MigrationPlan,
    reservation: &protocol::MigrationReservation,
    record: &mut MigrationRecord,
    presentation: Option<&dyn MigrationPresentation>,
) -> Result<(), String> {
    if !record.native_ready || !record.target_committed {
        return Err("legacy retirement requires committed native readiness".into());
    }
    if stop_requested(plan)? {
        return Err("user Stop prevents retirement".into());
    }
    reservation.recheck_inactive_source()?;
    verify_all_committed_targets(plan, record)?;
    let integrations_confirmed = integrations::refresh_fresh_integrations(plan, record)?;
    retirement_gate(plan, record, integrations_confirmed)?;
    if stop_requested(plan)? {
        return Err("user Stop before approved owner retirement".into());
    }
    if let Some(item) = &record.manager {
        if !manager_can_resume(item) {
            transition(plan, record, MigrationPhase::ManagerUnknown, presentation)?;
            return Err("uncertain manager intent cannot replay".into());
        }
    }
    let manager_applied = record
        .manager
        .as_ref()
        .is_some_and(|item| manager_can_resume(item) && item.exit_code == Some(0));
    record.retirement_attempt = record
        .retirement_attempt
        .checked_add(1)
        .ok_or("retirement attempts overflow")?;
    transition(plan, record, MigrationPhase::Retiring, presentation)?;
    if !manager_applied && plan.source_image.path.exists() {
        verify_source(plan)?;
        match &plan.source_origin {
            LegacyOrigin::Homebrew { brew, formula, .. } => {
                execute_manager(
                    plan,
                    record,
                    brew,
                    vec!["uninstall".into(), formula.clone()],
                    false,
                    presentation,
                )?;
            }
            LegacyOrigin::HerdrManaged {
                host,
                integration_commit,
                ..
            } => {
                verify_integration_ref(plan)?;
                execute_manager(
                    plan,
                    record,
                    host,
                    vec![
                        "plugin".into(),
                        "install".into(),
                        "--yes".into(),
                        "--ref".into(),
                        integration_commit.clone(),
                        "hanbong5938/herdr-desktop-pet".into(),
                    ],
                    false,
                    presentation,
                )?;
            }
            LegacyOrigin::Generated { bundle, .. } | LegacyOrigin::Manual { bundle } => {
                retire_exact_bundle(plan, bundle, "retired-bridge")?;
            }
        }
    }
    if let LegacyOrigin::HerdrManaged { .. } = &plan.source_origin {
        verify_managed_identity(plan)?;
        let bundle = source_bundle(plan)?;
        if bundle.exists() {
            retire_exact_bundle(plan, &bundle, "managed-retired-bridge")?;
        }
        remove_empty_generated_dist(&bundle)?;
    }
    verify_managed_retirement(plan)?;
    retire_aliases(plan)?;
    retired(plan)?;
    if stop_requested(plan)? {
        transition(plan, record, MigrationPhase::UserStopped, presentation)?;
        return reservation.release();
    }
    verify_all_committed_targets(plan, record)?;
    transition(plan, record, MigrationPhase::Complete, presentation)?;
    reservation.release()
}
/// Sole manager retry entry point: unknown intent/running state never qualifies.
#[cfg(not(desktop_pet_legacy))]
pub fn resume_retirement_with_presentation(
    path: &Path,
    presentation: &dyn MigrationPresentation,
) -> Result<MigrationRecord, String> {
    resume_retirement_impl(path, Some(presentation))
}
#[cfg(not(desktop_pet_legacy))]
fn resume_retirement_impl(
    path: &Path,
    presentation: Option<&dyn MigrationPresentation>,
) -> Result<MigrationRecord, String> {
    let plan = read_plan(path)?;
    let _lease = protocol::lock_migration_operation(
        &plan.profiles[0].state_dir,
        &plan.operation_id,
        &plan.token,
    )?;
    let mut record = read_record(&plan)?;
    if !record.native_ready
        || !record.target_committed
        || !matches!(
            record.phase,
            MigrationPhase::RetirementFailed
                | MigrationPhase::Retiring
                | MigrationPhase::NativeReady
        )
        || record
            .manager
            .as_ref()
            .is_some_and(|item| !manager_can_resume(item))
        || record
            .target_manager
            .as_ref()
            .is_some_and(|item| !manager_can_resume(item))
    {
        return Err(
            "journal does not authorize retirement resume; uncertain intent never replays".into(),
        );
    }
    let reservation = protocol::load_migration_reservation(&plan.operation_id, &plan.token)?;
    if stop_requested(&plan)? {
        return Err("user Stop prevents retirement resume".into());
    }
    verify_all_committed_targets(&plan, &record)?;
    visible(presentation, &plan)?;
    write_authorization_ack(&plan, &record, true)?;
    transition(
        &plan,
        &mut record,
        MigrationPhase::Integrating,
        presentation,
    )?;
    match retire(&plan, &reservation, &mut record, presentation) {
        Ok(()) => Ok(record),
        Err(error) => {
            record.error = Some(error);
            if record.phase != MigrationPhase::ManagerUnknown {
                transition(
                    &plan,
                    &mut record,
                    MigrationPhase::RetirementFailed,
                    presentation,
                )?;
            } else {
                save(&plan, &record)?;
            }
            Ok(record)
        }
    }
}

fn random_hex(bytes: usize) -> Result<String, String> {
    if bytes > 32 {
        return Err("random migration identity exceeds bound".into());
    }
    let mut random = [0u8; 32];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut random[..bytes]))
        .map_err(|e| e.to_string())?;
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes * 2);
    for byte in &random[..bytes] {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 15) as usize] as char);
    }
    Ok(output)
}
fn capture_record(path: PathBuf) -> Result<PreviousRecord, String> {
    let bytes = match fs::symlink_metadata(&path) {
        Ok(_) => Some(owned_private_file(&path, 16384)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    Ok(PreviousRecord { path, bytes })
}
pub fn source_origin_from_context(
    context: &UpdateContext,
    _integration_commit: Option<&str>,
) -> Result<LegacyOrigin, String> {
    let origin = context
        .running_origin
        .as_deref()
        .ok_or("bridge lacks captured installation authority")?;
    let bundle = context
        .running
        .path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or("invalid bridge image")?
        .to_path_buf();
    Ok(match origin {
        InstallOrigin::Homebrew {
            brew,
            prefix,
            cellar_formula_root,
            formula,
            ..
        } => LegacyOrigin::Homebrew {
            brew: executable_identity(brew)?,
            prefix: prefix.clone(),
            cellar_formula_root: cellar_formula_root.clone(),
            formula: formula.clone(),
        },
        #[cfg(desktop_pet_legacy)]
        InstallOrigin::Herdr {
            host,
            host_config_dir,
            checkout_root,
            plugin_root,
            resolved_commit,
            source,
            ..
        } => {
            if source != "hanbong5938/herdr-desktop-pet" {
                return Err(
                    "managed migration requires exact old official integration source".into(),
                );
            }
            LegacyOrigin::HerdrManaged {
                host: executable_identity(host)?,
                host_config_dir: host_config_dir.clone(),
                checkout_root: checkout_root.clone(),
                plugin_root: plugin_root.clone(),
                registry_sha256: format!(
                    "{:x}",
                    Sha256::digest(crate::owned_file(
                        &host_config_dir.join("plugins.json"),
                        1024 * 1024
                    )?)
                ),
                resolved_commit: resolved_commit
                    .clone()
                    .ok_or("managed source lacks immutable installed revision")?,
                integration_commit: _integration_commit
                    .filter(|value| hex(value, 40))
                    .ok_or("managed migration lacks pinned integration-only0.4.1 commit")?
                    .into(),
            }
        }
        InstallOrigin::Source { root } => LegacyOrigin::Generated {
            source_root: root.clone(),
            bundle,
        },
        InstallOrigin::Manual { .. } => LegacyOrigin::Manual { bundle },
        _ => return Err("source is not an authenticated supported legacy installation".into()),
    })
}
/// Availability never stages, writes a plan, acquires reservations or starts a
/// manager. The worker repeats durable admission before a consent can execute.
pub fn readonly_eligibility(context: &UpdateContext) -> Result<(), String> {
    if context.version != "0.3.15" || executable_identity(&context.running.path)? != context.running
    {
        return Err(
            "finish the approved update with genuine bridge 0.3.15 before automatic migration"
                .into(),
        );
    }
    source_origin_from_context(context, None)?;
    let updates = context.state_dir.join("updates");
    if fs::symlink_metadata(updates.join("profile.reservation.json")).is_ok() {
        return Err("Operation A still owns the profile; use its matching helper".into());
    }
    if let Ok(entries) = fs::read_dir(&updates) {
        let mut count = 0;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry
                .file_name()
                .to_string_lossy()
                .ends_with(".result.json")
            {
                continue;
            }
            count += 1;
            if count > 1024 {
                return Err("old operation evidence exceeds bound".into());
            }
            let value: serde_json::Value =
                serde_json::from_slice(&owned_private_file(&entry.path(), 16384)?)
                    .map_err(|e| e.to_string())?;
            let version = value
                .get("version")
                .and_then(serde_json::Value::as_u64)
                .ok_or("old operation has no version")?;
            let phase = value
                .get("phase")
                .and_then(serde_json::Value::as_str)
                .ok_or("old operation has no phase")?;
            if !(1..=3).contains(&version)
                || !matches!(phase, "completed" | "failed" | "user_stopped")
                || value
                    .get("execution_fence")
                    .and_then(serde_json::Value::as_str)
                    == Some("manager_intent")
            {
                return Err(
                    "old operation is unresolved; retain evidence and use its matching helper"
                        .into(),
                );
            }
        }
    }
    Ok(())
}
fn build_plan_with_id(
    context: &UpdateContext,
    target: MigrationTarget,
    integration_commit: Option<&str>,
    operation_id: String,
) -> Result<MigrationPlan, String> {
    readonly_eligibility(context)?;
    let source_origin = source_origin_from_context(context, integration_commit)?;
    let active = protocol::active_profiles_for_roots(&[source_origin.root().to_path_buf()])?;
    if active.is_empty() || active.len() > 64 {
        return Err("source root has no bounded authenticated active-profile census".into());
    }
    let mut profiles = Vec::with_capacity(active.len());
    for registered in active {
        let mut profile = LegacyProfile {
            config_dir: context.config_dir.clone(),
            control_socket: registered.state_dir.join("control.sock"),
            state_dir: registered.state_dir,
            pid: registered.pid,
            instance_id: registered.instance_id,
            stop_generation: 0,
            observation: ObservationSelection::Auto,
            environment: BTreeMap::new(),
            herdr_socket: None,
            assets_override: None,
            host_plugin_config_dir: None,
            tern_reconnect_required: false,
        };
        let live = status(&profile)?;
        profile.config_dir = live.config_dir.clone();
        profile.observation = live_observation(&live)?;
        profile.stop_generation = migration_stop_generation(&profile.state_dir)?;
        profile.herdr_socket = live.herdr_socket.clone();
        profile.assets_override = live.assets_override.clone();
        profile.host_plugin_config_dir = live.migration_source_host_plugin_config_dir.clone();
        profile.tern_reconnect_required = source_requires_tern(&live)?;
        profile.environment = live
            .migration_source_environment
            .ok_or("bridge must expose its captured safe routing environment")?;
        profiles.push(profile);
    }
    let selected = profiles
        .iter()
        .position(|p| p.state_dir == context.state_dir && p.instance_id == context.instance_id)
        .ok_or("current bridge not in authenticated root census")?;
    profiles.swap(0, selected);
    let home = crate::origin::os_home()?;
    let config_root = neutral_config_root(&profiles[0].environment, &home)?;
    let previous_default_profile = capture_record(config_root.join("default-profile.json"))?;
    let adopt_as_default = default_profile_matches(&previous_default_profile, &profiles[0])?;
    let target_manager_program = match &target.origin {
        InstallOrigin::Homebrew { brew, .. } => Some(executable_identity(brew)?),
        _ => None,
    };
    let legacy_aliases = discover_legacy_aliases(&source_origin, &context.running, &home)?;
    let readonly_target_profiles = capture_target_profiles(&target)?;
    let tern_reconnect_required = profiles
        .iter()
        .any(|profile| profile.tern_reconnect_required);
    let source_ancestry = ancestry::capture_source_ancestry(&source_origin, &context.running)?;
    let mut plan = MigrationPlan {
        version: 1,
        kind: "legacy_to_desktop_pet".into(),
        operation_id,
        token: random_hex(32)?,
        source_image: context.running.clone(),
        source_origin,
        source_ancestry,
        profiles,
        target,
        target_manager_program,
        legacy_aliases,
        readonly_target_profiles,
        previous_locator: capture_record(home.join(".local/share/desktop-pet/installation.json"))?,
        previous_default_profile,
        adopt_as_default,
        retirement_consent: true,
        tern_reconnect_required,
        tern_delivery: None,
        tern_deliveries: Vec::new(),
        tern_delivery_problem: None,
    };
    tern_config::configure_delivery(&mut plan);
    validate_plan(&plan)?;
    verify_source(&plan)?;
    for profile in &plan.profiles {
        verify_status(&plan, profile, &status(profile)?, false)?;
    }
    let admission = protocol::admit_legacy_migration(
        &roots(&plan)?,
        &plan
            .profiles
            .iter()
            .map(|p| protocol::MigrationProfile {
                pid: p.pid,
                instance_id: p.instance_id.clone(),
                state_dir: p.state_dir.clone(),
                config_dir: p.config_dir.clone(),
                image: plan.source_image.clone(),
                stop_generation: p.stop_generation,
            })
            .collect::<Vec<_>>(),
        &plan.readonly_target_profiles,
        &plan.operation_id,
    )?;
    admission.recheck_profiles()?;
    Ok(plan)
}
pub fn build_bridge_plan(
    context: &UpdateContext,
    target: MigrationTarget,
    integration_commit: Option<&str>,
) -> Result<MigrationPlan, String> {
    build_plan_with_id(context, target, integration_commit, random_hex(16)?)
}
pub fn prepare_bridge_plan(
    context: &UpdateContext,
    integration_commit: Option<&str>,
) -> Result<MigrationPlan, String> {
    readonly_eligibility(context)?;
    source_origin_from_context(context, integration_commit)?;
    let operation = random_hex(16)?;
    let target = crate::installation::stage_neutral_migration_release(
        &context.state_dir,
        &operation,
        &context.observation_mode()?,
    )?;
    build_plan_with_id(context, target, integration_commit, operation)
}
pub fn write_new_plan(plan: &MigrationPlan) -> Result<PathBuf, String> {
    validate_plan(plan)?;
    let directory = plan.profiles[0].state_dir.join("updates-v4");
    ensure_private_directory(&directory)?;
    let path = directory.join(format!("{}.migration-consent.json", plan.operation_id));
    protocol::write_private_new(
        &path,
        &serde_json::to_vec(plan).map_err(|e| e.to_string())?,
        0o600,
    )?;
    Ok(path)
}
fn ensure_private_directory(path: &Path) -> Result<(), String> {
    crate::ensure_absolute(path)?;
    let mut missing = Vec::new();
    let mut parent = path;
    while fs::symlink_metadata(parent).is_err() {
        missing.push(parent.to_path_buf());
        parent = parent.parent().ok_or("directory has no owned parent")?;
    }
    owned_directory(parent)?;
    use std::os::unix::fs::DirBuilderExt;
    for directory in missing.into_iter().rev() {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(|e| e.to_string())?;
        owned_directory(&directory)?;
        File::open(directory.parent().ok_or("directory parent missing")?)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
    }
    owned_directory(path)
}

pub(crate) mod neutral_origin_codec {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        origin: &InstallOrigin,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut value = serde_json::to_value(origin).map_err(serde::ser::Error::custom)?;
        if matches!(origin, InstallOrigin::Source { .. }) {
            value["kind"] = serde_json::Value::String("source".into());
        }
        value.serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<InstallOrigin, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        decode_neutral_origin(value).map_err(serde::de::Error::custom)
    }
}
pub fn decode_neutral_origin(value: serde_json::Value) -> Result<InstallOrigin, String> {
    #[cfg(desktop_pet_legacy)]
    let mut value = value;
    if !matches!(
        value.get("kind").and_then(serde_json::Value::as_str),
        Some("source" | "standalone" | "homebrew" | "manual" | "unknown")
    ) {
        return Err("migration neutral owner is unsupported".into());
    }
    #[cfg(desktop_pet_legacy)]
    if value.get("kind").and_then(serde_json::Value::as_str) == Some("source") {
        value["kind"] = serde_json::Value::String("local".into());
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

fn remove_empty_generated_dist(bundle: &Path) -> Result<(), String> {
    let dist = bundle.parent().ok_or("managed bundle parent missing")?;
    match fs::symlink_metadata(dist) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err("managed generated dist is not a real directory".into());
        }
        Ok(_) => {}
    }
    // Never remove unexpected contents. The final closure gate rejects them.
    let mut entries = fs::read_dir(dist).map_err(|e| e.to_string())?;
    if entries
        .next()
        .transpose()
        .map_err(|e| e.to_string())?
        .is_none()
    {
        fs::remove_dir(dist).map_err(|e| e.to_string())?;
        File::open(dist.parent().ok_or("managed dist parent missing")?)
            .and_then(|file| file.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn integration_github_json(path: &str) -> Result<serde_json::Value, String> {
    let text = command_output(
        Path::new("/usr/bin/curl"),
        &[
            "--disable".into(),
            "--fail".into(),
            "--proto".into(),
            "=https".into(),
            "--connect-timeout".into(),
            "10".into(),
            "--max-time".into(),
            "20".into(),
            "--silent".into(),
            format!("https://api.github.com/repos/hanbong5938/herdr-desktop-pet/{path}"),
        ],
    )?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn verify_integration_ref(plan: &MigrationPlan) -> Result<(), String> {
    if let LegacyOrigin::HerdrManaged {
        integration_commit, ..
    } = &plan.source_origin
    {
        let release = integration_github_json("releases/tags/v0.4.1")?;
        if release.get("tag_name").and_then(serde_json::Value::as_str) != Some("v0.4.1")
            || release.get("draft").and_then(serde_json::Value::as_bool) != Some(false)
            || release
                .get("prerelease")
                .and_then(serde_json::Value::as_bool)
                != Some(false)
            || release
                .get("published_at")
                .and_then(serde_json::Value::as_str)
                .is_none()
            || !release
                .get("assets")
                .and_then(serde_json::Value::as_array)
                .is_some_and(Vec::is_empty)
        {
            return Err("Herdr integration0.4.1 is not a published assetless release".into());
        }
        let reference = integration_github_json("git/ref/tags/v0.4.1")?;
        let mut object = reference
            .get("object")
            .cloned()
            .ok_or("published integration tag object absent")?;
        let mut resolved = false;
        for _ in 0..8 {
            let sha = object
                .get("sha")
                .and_then(serde_json::Value::as_str)
                .filter(|sha| hex(sha, 40))
                .ok_or("invalid published integration tag SHA")?;
            match object.get("type").and_then(serde_json::Value::as_str) {
                Some("commit") if sha == integration_commit => {
                    resolved = true;
                    break;
                }
                Some("tag") => {
                    object = integration_github_json(&format!("git/tags/{sha}"))?
                        .get("object")
                        .cloned()
                        .ok_or("annotated integration tag object absent")?;
                }
                _ => return Err("published Herdr integration tag differs from consent".into()),
            }
        }
        if !resolved {
            return Err("published Herdr integration tag could not be resolved".into());
        }
        let url=format!("https://raw.githubusercontent.com/hanbong5938/herdr-desktop-pet/{integration_commit}/herdr-plugin.toml");
        let manifest = command_output(
            Path::new("/usr/bin/curl"),
            &[
                "--disable".into(),
                "--fail".into(),
                "--proto".into(),
                "=https".into(),
                "--connect-timeout".into(),
                "10".into(),
                "--max-time".into(),
                "20".into(),
                "--silent".into(),
                url,
            ],
        )?;
        let document: toml::Value = manifest
            .parse()
            .map_err(|e| format!("immutable integration manifest: {e}"))?;
        validate_integration_manifest(&document)?;
        let url=format!("https://api.github.com/repos/hanbong5938/herdr-desktop-pet/git/commits/{integration_commit}");
        let text = command_output(
            Path::new("/usr/bin/curl"),
            &[
                "--disable".into(),
                "--fail".into(),
                "--proto".into(),
                "=https".into(),
                "--connect-timeout".into(),
                "10".into(),
                "--max-time".into(),
                "20".into(),
                "--silent".into(),
                url,
            ],
        )?;
        let commit: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let tree_sha = commit
            .pointer("/tree/sha")
            .and_then(serde_json::Value::as_str)
            .ok_or("immutable commit tree is absent")?;
        if commit.get("sha").and_then(serde_json::Value::as_str) != Some(integration_commit)
            || !hex(tree_sha, 40)
        {
            return Err("integration commit identity differs from consent".into());
        }
        let url=format!("https://api.github.com/repos/hanbong5938/herdr-desktop-pet/git/trees/{tree_sha}?recursive=1");
        let text = command_output(
            Path::new("/usr/bin/curl"),
            &[
                "--disable".into(),
                "--fail".into(),
                "--proto".into(),
                "=https".into(),
                "--connect-timeout".into(),
                "10".into(),
                "--max-time".into(),
                "20".into(),
                "--silent".into(),
                url,
            ],
        )?;
        let tree: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if tree.get("sha").and_then(serde_json::Value::as_str) != Some(tree_sha)
            || tree.get("truncated").and_then(serde_json::Value::as_bool) != Some(false)
        {
            return Err("immutable integration source tree could not be fully verified".into());
        }
        let entries = tree
            .get("tree")
            .and_then(serde_json::Value::as_array)
            .ok_or("integration tree missing")?;
        if entries.len() > 10000
            || entries.iter().any(|entry| {
                entry
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .is_none_or(integration_owns_product)
                    || !matches!(
                        (
                            entry.get("type").and_then(serde_json::Value::as_str),
                            entry.get("mode").and_then(serde_json::Value::as_str)
                        ),
                        (Some("tree"), Some("040000")) | (Some("blob"), Some("100644" | "100755"))
                    )
            })
        {
            return Err(
                "consented Herdr ref still includes a concrete legacy native installation".into(),
            );
        }
    }
    Ok(())
}
fn verify_managed_identity(plan: &MigrationPlan) -> Result<(), String> {
    if let LegacyOrigin::HerdrManaged {
        host_config_dir,
        checkout_root,
        plugin_root,
        integration_commit,
        ..
    } = &plan.source_origin
    {
        let records: Vec<serde_json::Value> = serde_json::from_slice(&crate::owned_file(
            &host_config_dir.join("plugins.json"),
            1024 * 1024,
        )?)
        .map_err(|e| e.to_string())?;
        if records.len() > 256 {
            return Err("Herdr registry exceeds bound".into());
        }
        let entries = records
            .iter()
            .filter(|r| {
                r.get("plugin_id").and_then(serde_json::Value::as_str) == Some("desktop-pet")
            })
            .collect::<Vec<_>>();
        if entries.len() != 1
            || entries[0]
                .get("plugin_root")
                .and_then(serde_json::Value::as_str)
                != plugin_root.to_str()
            || entries[0]
                .pointer("/source/resolved_commit")
                .and_then(serde_json::Value::as_str)
                != Some(integration_commit)
        {
            return Err("Herdr registry did not adopt exact same-registry integration ref".into());
        }
        owned_directory(checkout_root)?;
        owned_directory(plugin_root)?;
        verify_git_remote(checkout_root)?;
        let head = command_output(
            Path::new("/usr/bin/git"),
            &[
                "-C".into(),
                checkout_root.to_string_lossy().into_owned(),
                "rev-parse".into(),
                "HEAD".into(),
            ],
        )?;
        if head.trim() != integration_commit {
            return Err(
                "actual managed checkout did not apply immutable integration commit".into(),
            );
        }
        let bytes = crate::owned_file(&plugin_root.join("herdr-plugin.toml"), 1024 * 1024)?;
        let manifest: toml::Value = std::str::from_utf8(&bytes)
            .map_err(|e| e.to_string())?
            .parse()
            .map_err(|e| format!("integration manifest: {e}"))?;
        validate_integration_manifest(&manifest)?;
    }
    Ok(())
}
fn verify_managed_retirement(plan: &MigrationPlan) -> Result<(), String> {
    verify_managed_identity(plan)?;
    if let LegacyOrigin::HerdrManaged { plugin_root, .. } = &plan.source_origin {
        verify_integration_directory(plugin_root)?;
    }
    Ok(())
}

fn validate_integration_manifest(manifest: &toml::Value) -> Result<(), String> {
    if manifest.get("id").and_then(toml::Value::as_str) != Some("desktop-pet")
        || manifest.get("version").and_then(toml::Value::as_str) != Some("0.4.1")
        || manifest
            .get("min_herdr_version")
            .and_then(toml::Value::as_str)
            != Some("0.9.3")
    {
        return Err("consented immutable ref is not Herdr integration0.4.1".into());
    }
    let command_matches = |entry: &toml::Value, expected: &[&str]| {
        entry
            .get("command")
            .and_then(toml::Value::as_array)
            .is_some_and(|command| {
                command.len() == expected.len()
                    && command
                        .iter()
                        .zip(expected)
                        .all(|(value, expected)| value.as_str() == Some(*expected))
            })
    };
    for (kind, command) in [
        ("build", &["bash", "scripts/install.sh"][..]),
        ("startup", &["bash", "scripts/run-native.sh", "ensure"][..]),
        ("events", &["bash", "scripts/run-native.sh", "ensure"][..]),
    ] {
        let hooks = manifest
            .get(kind)
            .and_then(toml::Value::as_array)
            .ok_or("integration manifest lacks consumer hooks")?;
        if hooks.len() != 1
            || !command_matches(&hooks[0], command)
            || kind == "events"
                && hooks[0].get("on").and_then(toml::Value::as_str) != Some("client.attached")
        {
            return Err("integration manifest has non-consumer hooks".into());
        }
    }
    let expected_actions = [
        "settings",
        "start",
        "ensure",
        "stop",
        "status",
        "toggle",
        "show",
        "hide",
        "passthrough",
        "alpha_passthrough",
        "show_bubble",
        "hide_bubble",
        "bubble_above",
        "bubble_below",
        "bubble_left",
        "bubble_right",
        "bubble_auto",
        "reset",
        "bigger",
        "smaller",
        "restart",
    ];
    let actions = manifest
        .get("actions")
        .and_then(toml::Value::as_array)
        .ok_or("integration manifest lacks retained actions")?;
    if actions.len() != expected_actions.len()
        || actions.iter().zip(expected_actions).any(|(action, id)| {
            action.get("id").and_then(toml::Value::as_str) != Some(id)
                || !command_matches(action, &["bash", "scripts/run-native.sh", id])
        })
    {
        return Err("integration manifest actions are not exact consumer calls".into());
    }
    Ok(())
}

fn integration_owns_product(path: &str) -> bool {
    const OBSOLETE: &[&str] = &[
        "native",
        "dist",
        "web",
        "assets",
        "tools",
        ".agents/skills/character-creator",
        ".agents/skills/create-pet-character",
        "scripts/package-native.ts",
        "scripts/build-rig-native.mjs",
        "scripts/release-docs.ts",
        "scripts/release-docs.test.ts",
        "scripts/wiki-docs.ts",
        "scripts/wiki-docs.test.ts",
        "scripts/publish-wiki.sh",
        ".github/workflows/wiki.yml",
    ];
    if path.starts_with('/')
        || OBSOLETE.iter().any(|root| {
            path.get(..root.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(root))
                && (path.len() == root.len()
                    || path
                        .get(root.len()..)
                        .is_some_and(|rest| rest.starts_with('/')))
        })
    {
        return true;
    }
    path.split('/').any(|component| {
        if component.is_empty()
            || component == "."
            || component == ".."
            || [
                "dist",
                "target",
                "node_modules",
                "Contents",
                "_CodeSignature",
                "rig",
                "rig-native",
                "creator",
                "rig.json",
                "rig-native.json",
                "Info.plist",
                "RigLimits.rs",
                "RigLimits.swift",
                "desktop-pet",
                "desktop-pet-update-coordinator",
                "herdr-desktop-pet",
                "herdr-update-coordinator",
                "rig-decode-worker",
            ]
            .iter()
            .any(|name| component.eq_ignore_ascii_case(name))
        {
            return true;
        }
        if component
            .get(.."libherdr_rig".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("libherdr_rig"))
        {
            return true;
        }
        let filename = [".sha256", ".sha512"]
            .iter()
            .find_map(|suffix| {
                let split = component.len().checked_sub(suffix.len())?;
                component
                    .get(split..)
                    .filter(|ending| ending.eq_ignore_ascii_case(suffix))?;
                component.get(..split)
            })
            .unwrap_or(component);
        [
            ".app",
            ".framework",
            ".bundle",
            ".tar",
            ".gz",
            ".tgz",
            ".bz2",
            ".tbz",
            ".tbz2",
            ".xz",
            ".txz",
            ".zst",
            ".zip",
            ".7z",
            ".rar",
            ".dmg",
            ".pkg",
            ".o",
            ".a",
            ".so",
            ".dylib",
            ".dll",
            ".exe",
            ".wasm",
            ".bin",
            ".herdrchar",
            "rig-native.json",
        ]
        .iter()
        .any(|suffix| {
            filename
                .get(filename.len().saturating_sub(suffix.len())..)
                .is_some_and(|ending| ending.eq_ignore_ascii_case(suffix))
        })
    })
}

/// Inspect the installed closure without deleting unexpected files or following links.
fn verify_integration_directory(root: &Path) -> Result<(), String> {
    let mut directories = vec![root.to_path_buf()];
    let mut count = 0;
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let relative = path.strip_prefix(root).map_err(|e| e.to_string())?;
            if relative == Path::new(".git") {
                continue;
            }
            count += 1;
            if count > 10000 || relative.to_str().is_none_or(integration_owns_product) {
                return Err("managed Herdr integration still owns product artifacts".into());
            }
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                directories.push(path);
            } else if !kind.is_file() {
                return Err("managed Herdr integration contains a linked/nonregular source".into());
            }
        }
    }
    Ok(())
}
fn discover_legacy_aliases(
    origin: &LegacyOrigin,
    image: &ExecutableIdentity,
    home: &Path,
) -> Result<Vec<PathBuf>, String> {
    let mut candidates = vec![
        home.join(".local/bin/herdr-desktop-pet"),
        PathBuf::from("/usr/local/bin/herdr-desktop-pet"),
        PathBuf::from("/opt/homebrew/bin/herdr-desktop-pet"),
    ];
    if let LegacyOrigin::Homebrew { prefix, .. } = origin {
        candidates.push(prefix.join("bin/herdr-desktop-pet"));
    }
    candidates.sort_unstable();
    candidates.dedup();
    Ok(candidates
        .into_iter()
        .filter(|path| fs::canonicalize(path).ok().as_ref() == Some(&image.path))
        .collect())
}
fn retire_aliases(plan: &MigrationPlan) -> Result<(), String> {
    for alias in &plan.legacy_aliases {
        let metadata = match fs::symlink_metadata(alias) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.to_string()),
        };
        if metadata.uid() != unsafe { libc::geteuid() } || !metadata.file_type().is_symlink() {
            return Err(
                "legacy alias is not an owned exact image symlink; never delete another executable"
                    .into(),
            );
        }
        let target = fs::read_link(alias).map_err(|e| e.to_string())?;
        let absolute = if target.is_absolute() {
            target
        } else {
            alias.parent().ok_or("alias parent missing")?.join(target)
        };
        let mut normalized = PathBuf::new();
        for component in absolute.components() {
            match component {
                std::path::Component::ParentDir => {
                    normalized.pop();
                }
                std::path::Component::CurDir => {}
                component => normalized.push(component.as_os_str()),
            }
        }
        if normalized != plan.source_image.path {
            return Err("legacy alias changed after consent".into());
        }
        if stop_requested(plan)? {
            return Err("user Stop prevents legacy alias retirement".into());
        }
        fs::remove_file(alias).map_err(|e| e.to_string())?;
        File::open(alias.parent().ok_or("alias parent missing")?)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TernBindingEvidence {
    version: u32,
    kind: String,
    instance_id: String,
    window_id: String,
    binding_epoch: u64,
    sequence: u64,
    local_host: String,
    observation_generation: u64,
    native_generation: u64,
    fresh: bool,
}
fn verify_all_committed_targets(
    plan: &MigrationPlan,
    record: &MigrationRecord,
) -> Result<(), String> {
    if record.target_profiles.len() != plan.profiles.len()
        || !record.target_committed
        || !record.native_ready
    {
        return Err("every approved profile requires its own durable ready/Commit evidence".into());
    }
    for profile in &plan.profiles {
        verify_committed_profile(plan, record, profile, &status(profile)?)?;
    }
    Ok(())
}

fn verify_committed_profile(
    plan: &MigrationPlan,
    record: &MigrationRecord,
    selected: &LegacyProfile,
    live: &LiveStatus,
) -> Result<(), String> {
    let target = record.target_profiles.iter().find(|target| {
        target.config_dir == selected.config_dir && target.state_dir == selected.state_dir
    });
    let (pid, instance) = if let Some(target) = target {
        if !target.native_ready || !target.committed || &target.image != target_image(plan, record)?
        {
            return Err("target profile lacks its own native-ready Commit evidence".into());
        }
        (target.pid, target.instance_id.as_deref())
    } else if record.target_profiles.is_empty()
        && selected.config_dir == plan.profiles[0].config_dir
        && selected.state_dir == plan.profiles[0].state_dir
    {
        // Historical primary codec is admitted only for its original selected profile.
        (record.target_pid, record.target_instance.as_deref())
    } else {
        return Err("target profile has no recorded Commit identity".into());
    };
    let image = target_image(plan, record)?;
    if !live.ok
        || !live.running
        || live.shutdown
        || !live.ready
        || !live.ui_ready
        || !live.control_ready
        || live.migration_gated
        || live.app_version != plan.target.release.version
        || live.updater_protocol != 4
        || live.observation_capability != 2
        || live.integration_api != 1
        || live.pid != pid.ok_or("native-ready profile PID missing")?
        || Some(live.instance_id.as_str()) != instance
        || actual_process_image(live.pid)? != image.path
        || executable_identity(&image.path)? != *image
        || live.executable_path != image.path
        || live.running_sha256 != image.sha256
        || live.assets_override != selected.assets_override
        || live.config_dir != selected.config_dir
        || live.state_dir != selected.state_dir
    {
        return Err("actual native target no longer matches committed migration readiness".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::os::unix::net::UnixListener;

    pub(super) fn fixture() -> (tempfile::TempDir, MigrationPlan) {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let state = root.join("state");
        let config = root.join("config");
        for path in [&state, &config] {
            fs::create_dir(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        ensure_private_directory(&state.join("updates-v4")).unwrap();
        let legacy = root.join("legacy/dist/HerdrDesktopPet.app");
        let target = root.join("target/DesktopPet.app");
        let image = ExecutableIdentity {
            path: legacy.join("Contents/MacOS/herdr-desktop-pet"),
            sha256: "b".repeat(64),
        };
        let home = crate::origin::os_home().unwrap();
        let plan=MigrationPlan {
            version:1,kind:"legacy_to_desktop_pet".into(),operation_id:"1".repeat(32),token:"2".repeat(64),
            source_image:image,source_origin:LegacyOrigin::Generated{source_root:root.join("legacy"),bundle:legacy},readonly_target_profiles:Vec::new(),
            profiles:vec![LegacyProfile {config_dir:config,state_dir:state.clone(),control_socket:state.join("control.sock"),
                pid:1000,instance_id:"source-instance".into(),stop_generation:0,observation:ObservationSelection::Auto,environment:BTreeMap::new(),
                herdr_socket:None,assets_override:None,host_plugin_config_dir:None,tern_reconnect_required:false}],
            target:MigrationTarget {release:ReleaseCandidate {
                version:crate::identity::neutral::VERSION.into(),channel:crate::ReleaseChannel::Stable,
                tag:format!("v{}", crate::identity::neutral::VERSION),release_id:1,
                asset_name:format!("DesktopPet-v{}-macos-arm64.tar.gz", crate::identity::neutral::VERSION),
                archive_url:format!("https://github.com/hanbong5938/desktop-pet/releases/download/v{0}/DesktopPet-v{0}-macos-arm64.tar.gz", crate::identity::neutral::VERSION),
                archive_size:10,sha256:"a".repeat(64),
                checksum_url:format!("https://github.com/hanbong5938/desktop-pet/releases/download/v{0}/DesktopPet-v{0}-macos-arm64.tar.gz.sha256", crate::identity::neutral::VERSION)
            },image:ExecutableIdentity {path:target.join("Contents/MacOS/desktop-pet"),sha256:"c".repeat(64)},
                origin:InstallOrigin::Standalone{bundle:target},staged_bundle:root.join("stage/DesktopPet.app"),previous_installation:None},
            target_manager_program:None,legacy_aliases:Vec::new(),
            previous_locator:PreviousRecord {path:home.join(".local/share/desktop-pet/installation.json"),bytes:None},
            previous_default_profile:PreviousRecord {path:home.join(".config/desktop-pet/default-profile.json"),bytes:None},
            adopt_as_default:false,retirement_consent:true,tern_reconnect_required:false,
            tern_delivery:None,tern_deliveries:Vec::new(),tern_delivery_problem:None,
            source_ancestry:Vec::new(),
        };
        (temp, plan)
    }
    fn retained_fixture(version: &str) -> (tempfile::TempDir, MigrationPlan) {
        let (temp, mut plan) = fixture();
        plan.target.release.version = version.into();
        plan.target.release.tag = format!("v{version}");
        plan.target.release.asset_name = format!("DesktopPet-v{version}-macos-arm64.tar.gz");
        plan.target.release.archive_url = format!("https://github.com/hanbong5938/desktop-pet/releases/download/v{version}/DesktopPet-v{version}-macos-arm64.tar.gz");
        plan.target.release.checksum_url = format!("{}.sha256", plan.target.release.archive_url);
        (temp, plan)
    }
    pub(super) fn persist_fixture(plan: &MigrationPlan) -> MigrationRecord {
        let bytes = serde_json::to_vec(plan).unwrap();
        protocol::write_private_new(&evidence_path(plan), &bytes, 0o600).unwrap();
        let record = make_record(plan, &bytes);
        save(plan, &record).unwrap();
        record
    }
    pub(super) fn context(plan: &MigrationPlan) -> UpdateContext {
        let profile = &plan.profiles[0];
        UpdateContext {
            executable: plan.source_image.path.clone(),
            version: "0.3.15".into(),
            instance_id: profile.instance_id.clone(),
            config_dir: profile.config_dir.clone(),
            state_dir: profile.state_dir.clone(),
            #[cfg(desktop_pet_legacy)]
            host_plugin_config_dir: None,
            #[cfg(desktop_pet_legacy)]
            herdr_socket: profile.state_dir.join("herdr.sock"),
            running: plan.source_image.clone(),
            running_origin: Some(Box::new(rollback_origin(plan).unwrap())),
            assets_override: None,
            environment: BTreeMap::new(),
            locale: "en".into(),
        }
    }
    fn request(plan: &MigrationPlan) -> MigrationControlRequest {
        MigrationControlRequest {
            version: 1,
            kind: "legacy-migration".into(),
            command: MigrationCommand::Prepare,
            operation_id: plan.operation_id.clone(),
            instance_id: plan.profiles[0].instance_id.clone(),
            token: plan.token.clone(),
            plan_path: evidence_path(plan),
            source_image: plan.source_image.clone(),
            config_dir: plan.profiles[0].config_dir.clone(),
            state_dir: plan.profiles[0].state_dir.clone(),
            stop_generation: 0,
        }
    }
    #[test]
    fn two_affected_profiles_require_distinct_ready_and_commit_evidence() {
        let (_temp, mut plan) = fixture();
        let original = plan.profiles[0].state_dir.join("original-history");
        fs::write(&original, b"preserved original history").unwrap();
        let mut secondary = plan.profiles[0].clone();
        secondary.state_dir = secondary.state_dir.join("secondary");
        secondary.config_dir = secondary.config_dir.join("secondary");
        secondary.control_socket = secondary.state_dir.join("control.sock");
        secondary.pid += 1;
        secondary.instance_id = "secondary-instance".into();
        plan.profiles.push(secondary);
        validate_plan(&plan).unwrap();
        let mut record = make_record(&plan, &serde_json::to_vec(&plan).unwrap());
        record.target_profiles.push(ready_target(&plan, 0));
        sync_target_aggregates(&plan, &mut record);
        assert!(!record.native_ready && !record.target_committed);
        assert!(retirement_gate(&plan, &record, true).is_err());
        record.target_profiles.push(ready_target(&plan, 1));
        sync_target_aggregates(&plan, &mut record);
        assert!(record.native_ready && record.target_committed);
        assert!(retirement_gate(&plan, &record, false).is_err());
        retirement_gate(&plan, &record, true).unwrap();
        record.target_profiles[1].state_dir = plan.profiles[0].state_dir.clone();
        sync_target_aggregates(&plan, &mut record);
        assert!(!record.native_ready && !record.target_committed);
        assert_eq!(fs::read(original).unwrap(), b"preserved original history");
        assert!(!journal_path(&plan).exists());
    }

    fn ready_target(plan: &MigrationPlan, index: usize) -> MigrationTargetProfile {
        let profile = &plan.profiles[index];
        MigrationTargetProfile {
            config_dir: profile.config_dir.clone(),
            state_dir: profile.state_dir.clone(),
            pid: Some(2000 + index as u32),
            instance_id: Some(format!("target-{index}")),
            image: plan.target.image.clone(),
            native_ready: true,
            committed: true,
            commit_requested: true,
        }
    }

    #[test]
    fn committed_primary_or_historical_flags_cannot_authorize_retirement_without_profile_binding() {
        let (_temp, plan) = fixture();
        let mut record = make_record(&plan, &serde_json::to_vec(&plan).unwrap());
        record.native_ready = true;
        record.target_committed = true;
        record.target_pid = Some(2000);
        record.target_instance = Some("target-0".into());
        assert!(retirement_gate(&plan, &record, true).is_err());
        record.target_profiles.push(ready_target(&plan, 0));
        assert!(retirement_gate(&plan, &record, false).is_err());
        assert_eq!(record.retirement_attempt, 0);
        assert!(record.manager.is_none());
        retirement_gate(&plan, &record, true).unwrap();
    }

    #[test]
    fn captured_tern_endpoints_must_remain_absolute_before_any_mutation() {
        let (temp, mut plan) = fixture();
        for key in ["TERN_CONFIG_DIR", "TERN_DAEMON_SOCKET"] {
            plan.profiles[0]
                .environment
                .insert(key.into(), "relative-route".into());
            assert!(validate_plan(&plan).is_err());
            plan.profiles[0].environment.insert(
                key.into(),
                temp.path().join(key).to_string_lossy().into_owned(),
            );
            validate_plan(&plan).unwrap();
        }
        assert!(!journal_path(&plan).exists());
    }

    #[test]
    fn partial_or_uncertain_commit_is_never_eligible_for_whole_operation_rollback() {
        let (_temp, mut plan) = fixture();
        let mut second = plan.profiles[0].clone();
        second.config_dir = second.config_dir.join("second");
        second.state_dir = second.state_dir.join("second");
        plan.profiles.push(second);
        let mut record = make_record(&plan, &serde_json::to_vec(&plan).unwrap());
        record.target_profiles = vec![ready_target(&plan, 0), ready_target(&plan, 1)];
        record.target_profiles[1].committed = false;
        sync_target_aggregates(&plan, &mut record);
        assert!(record.native_ready && !record.target_committed);
        assert!(commit_may_have_applied(&record));
        assert!(record.target_profiles[0].committed);
        assert!(retirement_gate(&plan, &record, true).is_err());
        record.target_profiles[0].committed = false;
        assert!(commit_may_have_applied(&record)); // Lost Commit reply is Unknown, not rollback permission.
        for target in &mut record.target_profiles {
            target.commit_requested = false;
        }
        assert!(!commit_may_have_applied(&record));
    }

    #[test]
    fn secondary_stop_after_partial_commit_prevents_every_target_and_rollback_launch() {
        let (_temp, mut plan) = fixture();
        let mut second = plan.profiles[0].clone();
        second.config_dir = second.config_dir.join("second");
        second.state_dir = second.state_dir.join("second");
        second.control_socket = second.state_dir.join("control.sock");
        second.pid += 1;
        second.instance_id = "second-source".into();
        fs::create_dir_all(second.state_dir.join("updates-v4")).unwrap();
        fs::set_permissions(
            second.state_dir.join("updates-v4"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        plan.profiles.push(second);
        let mut record = make_record(&plan, &serde_json::to_vec(&plan).unwrap());
        record.target_profiles = vec![ready_target(&plan, 0), ready_target(&plan, 1)];
        record.target_profiles[1].committed = false;
        sync_target_aggregates(&plan, &mut record);
        let evidence = serde_json::to_vec(&record).unwrap();
        assert!(!stop_requested(&plan).unwrap());
        protocol::write_private_bytes(
            &plan.profiles[1]
                .state_dir
                .join("updates-v4/stop-generation.json"),
            b"1",
            0o600,
        )
        .unwrap();
        assert!(stop_requested(&plan).unwrap());
        for profile in &plan.profiles {
            for target in [false, true] {
                let error = launch(&plan, profile, target, (0, "c".repeat(64))).unwrap_err();
                assert!(error.contains("user Stop"));
            }
        }
        assert_eq!(serde_json::to_vec(&record).unwrap(), evidence);
        assert!(record.target_profiles[0].committed);
        assert!(!journal_path(&plan).exists());
    }

    #[test]
    fn derived_brew_candidate_requires_exact_certain_successful_manager() {
        let (temp, mut plan) = fixture();
        let formula = "hanbong5938/tap/desktop-pet".to_owned();
        let brew = ExecutableIdentity {
            path: temp.path().join("bin/brew"),
            sha256: "e".repeat(64),
        };
        let cellar = temp.path().join("Cellar/desktop-pet");
        plan.target.image.path =
            cellar.join("0.4.11/libexec/DesktopPet.app/Contents/MacOS/desktop-pet");
        plan.target.origin = InstallOrigin::Homebrew {
            brew: brew.path.clone(),
            prefix: temp.path().into(),
            cellar_formula_root: cellar,
            formula: formula.clone(),
            locator: plan.target.image.path.clone(),
        };
        plan.target_manager_program = Some(brew.clone());
        let mut record = make_record(&plan, &serde_json::to_vec(&plan).unwrap());
        let mut derived = plan.target.image.clone();
        derived.sha256 = "f".repeat(64);
        record.target_image = Some(derived.clone());
        assert!(target_image(&plan, &record).is_err());
        record.target_manager = Some(ManagerEvidence {
            program: brew,
            args: vec!["install".into(), formula],
            intent_at: 1,
            pid: Some(1000),
            pgid: Some(1000),
            exit_code: Some(0),
            quiescent: false,
        });
        assert!(target_image(&plan, &record).is_err());
        record.target_manager.as_mut().unwrap().quiescent = true;
        assert_eq!(target_image(&plan, &record).unwrap(), &derived);
        record.target_manager.as_mut().unwrap().exit_code = None;
        assert!(target_image(&plan, &record).is_err());
        record.target_manager.as_mut().unwrap().exit_code = Some(0);
        record.target_manager.as_mut().unwrap().args[0] = "reinstall".into();
        assert!(target_image(&plan, &record).is_err());
    }

    #[test]
    fn integration_closure_rejects_product_roots_bundles_and_archives() {
        for path in [
            "dist",
            "dist/HerdrDesktopPet.app/Contents/MacOS/herdr-desktop-pet",
            "native/core/Cargo.toml",
            "assets/rubelia-default/rig.json",
            "web/rig/package.json",
            "copies/DesktopPet.app",
            "copies/Renderer.framework/renderer",
            "copies/character.bundle",
            "DesktopPet.tar.gz",
            "DesktopPet.zip.sha256",
            "backup/renderer.dylib",
            "native-adapter/target/release/libadapter.a",
            "somewhere/node_modules/tool/index.js",
            "somewhere/rig-native.json",
        ] {
            assert!(integration_owns_product(path), "{path}");
        }
        for path in [
            "native-adapter/src/lib.rs",
            "native-adapter/Cargo.toml",
            "herdr-plugin.toml",
            "scripts/consumer.sh",
            "site/index.html",
            "integrations/herdr/client-attached.patch",
            "integrations/omp/herdr-pet-outcomes.ts",
            "release-evidence/desktop-pet-install-v0.4.2.sh.sha256",
            "release-evidence/release.json",
        ] {
            assert!(!integration_owns_product(path), "{path}");
        }
    }

    #[test]
    fn installed_closure_rejects_untracked_products_without_touching_sources_or_profiles() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("integration");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("herdr-plugin.toml"), b"reviewed source").unwrap();
        fs::write(temp.path().join("usercfg"), b"user config").unwrap();
        fs::write(temp.path().join("state"), b"user state").unwrap();
        verify_integration_directory(&root).unwrap();
        fs::write(root.join("unexpected.zip"), b"unexpected archive").unwrap();
        assert!(verify_integration_directory(&root).is_err());
        assert_eq!(
            fs::read(root.join("unexpected.zip")).unwrap(),
            b"unexpected archive"
        );
        assert_eq!(
            fs::read(root.join("herdr-plugin.toml")).unwrap(),
            b"reviewed source"
        );
        assert_eq!(
            fs::read(temp.path().join("usercfg")).unwrap(),
            b"user config"
        );
        assert_eq!(fs::read(temp.path().join("state")).unwrap(), b"user state");
    }

    #[test]
    fn generated_dist_cleanup_removes_only_empty_real_directory() {
        let temp = tempfile::tempdir().unwrap();
        let dist = temp.path().join("dist");
        let bundle = dist.join("HerdrDesktopPet.app");
        remove_empty_generated_dist(&bundle).unwrap();
        fs::create_dir(&dist).unwrap();
        fs::write(dist.join("unexpected.txt"), b"keep").unwrap();
        remove_empty_generated_dist(&bundle).unwrap();
        assert_eq!(fs::read(dist.join("unexpected.txt")).unwrap(), b"keep");
        fs::remove_file(dist.join("unexpected.txt")).unwrap();
        remove_empty_generated_dist(&bundle).unwrap();
        assert!(!dist.exists());
        let linked = temp.path().join("source");
        fs::create_dir(&linked).unwrap();
        fs::write(linked.join("source.txt"), b"linked source").unwrap();
        symlink(&linked, &dist).unwrap();
        assert!(remove_empty_generated_dist(&bundle).is_err());
        assert_eq!(
            fs::read(linked.join("source.txt")).unwrap(),
            b"linked source"
        );
    }

    #[test]
    fn managed_integration_manifest_requires_corrected_version_and_consumer_commands() {
        let mut table = toml::map::Map::new();
        table.insert("id".into(), toml::Value::String("desktop-pet".into()));
        table.insert("version".into(), toml::Value::String("0.4.1".into()));
        table.insert(
            "min_herdr_version".into(),
            toml::Value::String("0.9.3".into()),
        );
        let command = |args: &[&str]| {
            toml::Value::Array(
                args.iter()
                    .map(|arg| toml::Value::String((*arg).into()))
                    .collect(),
            )
        };
        for (kind, args) in [
            ("build", &["bash", "scripts/install.sh"][..]),
            ("startup", &["bash", "scripts/run-native.sh", "ensure"][..]),
            ("events", &["bash", "scripts/run-native.sh", "ensure"][..]),
        ] {
            let mut hook = toml::map::Map::new();
            hook.insert("command".into(), command(args));
            if kind == "events" {
                hook.insert("on".into(), toml::Value::String("client.attached".into()));
            }
            table.insert(
                kind.into(),
                toml::Value::Array(vec![toml::Value::Table(hook)]),
            );
        }
        table.insert(
            "actions".into(),
            toml::Value::Array(
                [
                    "settings",
                    "start",
                    "ensure",
                    "stop",
                    "status",
                    "toggle",
                    "show",
                    "hide",
                    "passthrough",
                    "alpha_passthrough",
                    "show_bubble",
                    "hide_bubble",
                    "bubble_above",
                    "bubble_below",
                    "bubble_left",
                    "bubble_right",
                    "bubble_auto",
                    "reset",
                    "bigger",
                    "smaller",
                    "restart",
                ]
                .iter()
                .map(|id| {
                    let mut action = toml::map::Map::new();
                    action.insert("id".into(), toml::Value::String((*id).into()));
                    action.insert(
                        "command".into(),
                        command(&["bash", "scripts/run-native.sh", id]),
                    );
                    toml::Value::Table(action)
                })
                .collect(),
            ),
        );
        let mut manifest = toml::Value::Table(table);
        validate_integration_manifest(&manifest).unwrap();
        manifest["version"] = toml::Value::String("0.4.0".into());
        assert!(validate_integration_manifest(&manifest).is_err());
        manifest["version"] = toml::Value::String("0.4.1".into());
        manifest["build"][0]["command"] = command(&["bun", "run", "build:native"]);
        assert!(validate_integration_manifest(&manifest).is_err());
    }

    #[test]
    fn migration_plan_uses_fixed_neutral_authority_even_in_bridge_build() {
        let (_temp, mut plan) = fixture();
        validate_plan(&plan).unwrap();
        for version in [
            "0.4.1", "0.4.2", "0.4.3", "0.4.4", "0.4.5", "0.4.6", "0.4.7", "0.4.8", "0.4.9",
            "0.4.10",
        ] {
            let mut obsolete = plan.clone();
            obsolete.target.release.version = version.into();
            assert!(validate_plan(&obsolete).is_err());
        }
        plan.target.release.archive_url = plan
            .target
            .release
            .archive_url
            .replace("desktop-pet/releases", "herdr-desktop-pet/releases");
        assert!(validate_plan(&plan).is_err());
    }
    #[test]
    fn migration_eligibility_accepts_only_the_current_bridge_image() {
        let (_temp, plan) = fixture();
        let mut ctx = context(&plan);
        fs::create_dir_all(ctx.executable.parent().unwrap()).unwrap();
        fs::write(&ctx.executable, b"bridge executable image").unwrap();
        fs::set_permissions(&ctx.executable, fs::Permissions::from_mode(0o755)).unwrap();
        ctx.running = executable_identity(&ctx.executable).unwrap();
        readonly_eligibility(&ctx).unwrap();
        for version in [
            "0.3.6",
            "0.3.7",
            "0.3.8",
            "0.3.9",
            "0.3.10",
            "0.3.11",
            "0.3.12",
            "0.3.13",
            "0.3.14",
            crate::identity::neutral::VERSION,
        ] {
            ctx.version = version.into();
            assert!(readonly_eligibility(&ctx).is_err());
        }
        ctx.version = "0.3.15".into();
        ctx.running.sha256 = "f".repeat(64);
        assert!(readonly_eligibility(&ctx).is_err());
    }
    #[test]
    fn successor_eligibility_preserves_old_evidence_and_rejects_unresolved_ownership() {
        let (_temp, plan) = fixture();
        let mut ctx = context(&plan);
        fs::create_dir_all(ctx.executable.parent().unwrap()).unwrap();
        fs::write(&ctx.executable, b"bridge executable image").unwrap();
        fs::set_permissions(&ctx.executable, fs::Permissions::from_mode(0o755)).unwrap();
        ctx.running = executable_identity(&ctx.executable).unwrap();
        let updates = ctx.state_dir.join("updates");
        ensure_private_directory(&updates).unwrap();
        let journal = updates.join(format!("{}.result.json", "a".repeat(32)));
        for version in 1..=3 {
            for phase in ["completed", "failed", "user_stopped"] {
                let bytes = format!(
                    " {{\"version\":{version},\"phase\":\"{phase}\",\"app_version\":\"0.3.10\"}}\n"
                )
                .into_bytes();
                protocol::write_private_bytes(&journal, &bytes, 0o600).unwrap();
                readonly_eligibility(&ctx).unwrap();
                assert_eq!(fs::read(&journal).unwrap(), bytes);
            }
        }
        for bytes in [
            &b"{\"version\":3,\"phase\":\"unknown\"}"[..],
            &b"{\"version\":3,\"phase\":\"installing\"}"[..],
            &b"{\"version\":3,\"phase\":\"completed\",\"execution_fence\":\"manager_intent\"}"[..],
            &b"{\"version\":4,\"phase\":\"completed\"}"[..],
            &b"{\"version\":3}"[..],
            &b"{"[..],
        ] {
            protocol::write_private_bytes(&journal, bytes, 0o600).unwrap();
            assert!(readonly_eligibility(&ctx).is_err());
            assert_eq!(fs::read(&journal).unwrap(), bytes);
        }
        let settled = b"{\"version\":3,\"phase\":\"completed\",\"app_version\":\"0.3.10\"}";
        protocol::write_private_bytes(&journal, settled, 0o600).unwrap();
        let reservation = updates.join("profile.reservation.json");
        let reservation_bytes = b"{\"version\":3}";
        protocol::write_private_bytes(&reservation, reservation_bytes, 0o600).unwrap();
        assert!(readonly_eligibility(&ctx).is_err());
        assert_eq!(fs::read(&journal).unwrap(), settled.as_slice());
        assert_eq!(
            fs::read(&reservation).unwrap(),
            reservation_bytes.as_slice()
        );
    }
    #[test]
    fn source_auth_rejects_stale_instance_image_profile_and_stop_generation() {
        let (_temp, plan) = fixture();
        let ctx = context(&plan);
        let base = request(&plan);
        validate_control_request(&base, &ctx, 0, true).unwrap();
        let mut stale = base.clone();
        stale.instance_id = "other-instance".into();
        assert!(validate_control_request(&stale, &ctx, 0, true).is_err());
        stale = base.clone();
        stale.source_image.sha256 = "d".repeat(64);
        assert!(validate_control_request(&stale, &ctx, 0, true).is_err());
        stale = base.clone();
        stale.state_dir = plan.profiles[0].state_dir.join("other");
        assert!(validate_control_request(&stale, &ctx, 0, true).is_err());
        assert!(validate_control_request(&base, &ctx, 1, true).is_err());
        stale = base;
        stale.command = MigrationCommand::Abort;
        validate_control_request(&stale, &ctx, 1, true).unwrap();
    }
    #[test]
    fn journal_does_not_authorize_stop_before_all_profile_prepare() {
        let (_temp, plan) = fixture();
        let mut record = persist_fixture(&plan);
        record.phase = MigrationPhase::Stopping;
        save(&plan, &record).unwrap();
        let mut req = request(&plan);
        req.command = MigrationCommand::Stop;
        assert!(verify_live_control(&req.plan_path, &req, &context(&plan), true).is_err());
        record
            .prepared_profiles
            .push(plan.profiles[0].instance_id.clone());
        save(&plan, &record).unwrap();
        assert!(verify_live_control(&req.plan_path, &req, &context(&plan), true).is_err());
        record
            .acknowledged_profiles
            .push(plan.profiles[0].instance_id.clone());
        save(&plan, &record).unwrap();
        verify_live_control(&req.plan_path, &req, &context(&plan), true).unwrap();
    }
    #[test]
    fn durable_plan_substitution_and_journal_digest_change_are_rejected() {
        let (_temp, plan) = fixture();
        let mut record = persist_fixture(&plan);
        read_record(&plan).unwrap();
        let mut altered = plan.clone();
        altered.source_image.sha256 = "d".repeat(64);
        assert!(read_record(&altered).is_err());
        record.plan_sha256 = "0".repeat(64);
        save(&plan, &record).unwrap();
        assert!(read_record(&plan).is_err());
    }
    #[test]
    fn legacy_and_neutral_stop_evidence_is_combined_without_rewriting() {
        let (_temp, mut plan) = fixture();
        let state = &plan.profiles[0].state_dir;
        ensure_private_directory(&state.join("updates")).unwrap();
        let old = state.join("updates/stop-generation.json");
        let new = state.join("updates-v4/stop-generation.json");
        protocol::write_private_new(&old, b"7", 0o600).unwrap();
        plan.profiles[0].stop_generation = 7;
        assert!(!stop_requested(&plan).unwrap());
        protocol::write_private_new(&new, b"1", 0o600).unwrap();
        assert!(stop_requested(&plan).unwrap());
        assert_eq!(fs::read(&old).unwrap(), b"7");
        protocol::write_private_bytes(&new, u64::MAX.to_string().as_bytes(), 0o600).unwrap();
        assert!(stop_requested(&plan).is_err());
    }
    #[test]
    fn selected_original_custom_pair_becomes_default_only_when_absent_or_identical() {
        let (_temp, mut plan) = fixture();
        plan.profiles[0].config_dir = plan.profiles[0].config_dir.join("custom-original");
        plan.profiles[0].state_dir = plan.profiles[0].state_dir.join("custom-original");
        assert!(
            default_profile_matches(&plan.previous_default_profile, &plan.profiles[0]).unwrap()
        );
        plan.previous_default_profile.bytes = Some(serde_json::to_vec(&serde_json::json!({
            "version":1,"config_dir":plan.profiles[0].config_dir,"state_dir":plan.profiles[0].state_dir,
        })).unwrap());
        assert!(
            default_profile_matches(&plan.previous_default_profile, &plan.profiles[0]).unwrap()
        );
        plan.previous_default_profile.bytes = Some(
            br#"{"version":1,"config_dir":"/unrelated/config","state_dir":"/unrelated/state"}"#
                .to_vec(),
        );
        assert!(
            !default_profile_matches(&plan.previous_default_profile, &plan.profiles[0]).unwrap()
        );
    }

    #[test]
    fn previous_other_default_is_preserved_byte_for_byte() {
        let (_temp, mut plan) = fixture();
        plan.adopt_as_default = true;
        let original=b"{ \"version\":1,\"config_dir\":\"/different/config\",\"state_dir\":\"/different/state\" }\n".to_vec();
        plan.previous_default_profile.bytes = Some(original.clone());
        assert!(pointer_bytes(&plan).unwrap().1.is_none());
        assert_eq!(plan.previous_default_profile.bytes.unwrap(), original);
    }
    #[test]
    fn rollback_pointer_handles_fault_between_replace_and_journal_ack() {
        let (temp, _plan) = fixture();
        let path = temp.path().join("installation.json");
        let previous = PreviousRecord {
            path: path.clone(),
            bytes: Some(b"{\"version\":1,\"old\":true}".to_vec()),
        };
        protocol::write_private_new(&path, b"{\"version\":1,\"new\":true}", 0o600).unwrap();
        restore_pointer(&previous, b"{\"version\":1,\"new\":true}").unwrap();
        assert_eq!(fs::read(path).unwrap(), previous.bytes.unwrap());
    }
    #[test]
    fn rollback_does_not_overwrite_external_pointer_or_follow_symlink() {
        let (temp, _plan) = fixture();
        let path = temp.path().join("installation.json");
        let previous = PreviousRecord {
            path: path.clone(),
            bytes: None,
        };
        protocol::write_private_new(&path, b"{\"external\":true}", 0o600).unwrap();
        assert!(restore_pointer(&previous, b"{\"adopted\":true}").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"{\"external\":true}");
        let link = temp.path().join("link.json");
        symlink(&path, &link).unwrap();
        assert!(restore_pointer(
            &PreviousRecord {
                path: link,
                bytes: None
            },
            b"{\"external\":true}"
        )
        .is_err());
        assert_eq!(fs::read(path).unwrap(), b"{\"external\":true}");
    }
    #[test]
    fn manager_intent_unknown_and_undrained_exit_never_qualify_for_resume() {
        let (_temp, plan) = fixture();
        let mut item = ManagerEvidence {
            program: plan.source_image.clone(),
            args: vec!["uninstall".into()],
            intent_at: 1,
            pid: None,
            pgid: None,
            exit_code: None,
            quiescent: false,
        };
        assert!(!manager_can_resume(&item));
        item.pid = Some(123);
        item.pgid = Some(123);
        assert!(!manager_can_resume(&item));
        item.exit_code = Some(1);
        assert!(!manager_can_resume(&item));
        item.quiescent = true;
        assert!(manager_can_resume(&item));
        item.pgid = Some(124);
        assert!(!manager_can_resume(&item));
    }
    #[test]
    fn bounded_socket_rejects_truncated_and_oversize_acknowledgements() {
        for oversized in [false, true] {
            let (temp, _plan) = fixture();
            let socket = temp.path().join("control.sock");
            let listener = UnixListener::bind(&socket).unwrap();
            fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut byte = [0u8; 1];
                while stream.read_exact(&mut byte).is_ok() && byte[0] != b'\n' {}
                if oversized {
                    let _ = stream.write_all(&vec![b'x'; MAX_FRAME + 1]);
                }
            });
            assert!(exchange(&socket, &serde_json::json!({"version":1})).is_err());
            server.join().unwrap();
        }
    }
    #[test]
    fn duplicate_control_authority_fields_are_not_last_key_wins() {
        let (_temp, plan) = fixture();
        let bytes = serde_json::to_string(&request(&plan)).unwrap();
        let duplicate = bytes.replacen("\"version\":1", "\"version\":1,\"version\":4", 1);
        assert!(serde_json::from_str::<MigrationControlRequest>(&duplicate).is_err());
    }
    #[test]
    fn neutral_source_origin_serializes_canonically_without_legacy_alias() {
        let (_temp, mut plan) = fixture();
        plan.target.origin = InstallOrigin::Source {
            root: PathBuf::from("/neutral/source"),
        };
        let value = serde_json::to_value(&plan.target).unwrap();
        assert_eq!(value["origin"]["kind"], "source");
        let decoded: MigrationTarget = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.origin, plan.target.origin);
    }
    #[test]
    fn missing_real_helper_visibility_never_authorizes_migration_mutation() {
        let (_temp, plan) = fixture();
        assert!(visible(None, &plan).is_err());
        assert!(!journal_path(&plan).exists());
    }
    #[test]
    fn auto_without_observed_tern_is_not_a_configured_consumer() {
        let (_temp, plan) = fixture();
        let mut value = serde_json::json!({
            "ok":true,"running":true,"shutdown":false,"ready":true,"ui_ready":true,"control_ready":true,
            "app_version":"0.3.15","updater_protocol":3,"observation_capability":1,"integration_api":1,
            "instance_id":"source-instance","running_sha256":plan.source_image.sha256,"pid":1000,
            "executable_path":plan.source_image.path,"config_dir":plan.profiles[0].config_dir,"state_dir":plan.profiles[0].state_dir,
            "observation_mode":"both","host_connections":{"herdr":1,"tern":0}
        });
        let live: LiveStatus = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(live_observation(&live).unwrap(), ObservationSelection::Auto);
        assert!(!source_requires_tern(&live).unwrap());
        value["migration_source_environment"] =
            serde_json::json!({"TERN_WINDOW_KEY":"ambient-window","TERM_PROGRAM":"Tern"});
        let ambient: LiveStatus = serde_json::from_value(value.clone()).unwrap();
        assert!(
            !source_requires_tern(&ambient).unwrap(),
            "ambient Tern routing is not a configured integration"
        );
        value["host_connections"]["tern"] = serde_json::json!(1);
        let live: LiveStatus = serde_json::from_value(value.clone()).unwrap();
        assert!(source_requires_tern(&live).unwrap());
        value["host_connections"]["tern"] = serde_json::json!(0);
        value["observation_mode"] = serde_json::json!("tern");
        let live: LiveStatus = serde_json::from_value(value.clone()).unwrap();
        assert!(source_requires_tern(&live).unwrap());
        value["observation_mode"] = serde_json::json!("both");
        for version in [
            "0.3.6",
            "0.3.7",
            "0.3.8",
            "0.3.9",
            "0.3.10",
            crate::identity::neutral::VERSION,
        ] {
            value["app_version"] = serde_json::json!(version);
            let live: LiveStatus = serde_json::from_value(value.clone()).unwrap();
            assert!(live_observation(&live).is_err());
        }
    }
    #[test]
    fn retained_partial_history_is_bound_to_current_image_and_profile() {
        let (_temp, plan) = fixture();
        let mut record = persist_fixture(&plan);
        record.phase = MigrationPhase::ManagerUnknown;
        save(&plan, &record).unwrap();
        let ctx = context(&plan);
        assert_eq!(
            latest_for_context(&ctx).unwrap().unwrap().1.phase,
            MigrationPhase::ManagerUnknown
        );
        let mut wrong = ctx.clone();
        wrong.running.sha256 = "f".repeat(64);
        assert!(latest_for_context(&wrong).unwrap().is_none());
        wrong = ctx;
        wrong.config_dir = wrong.config_dir.join("other");
        assert!(latest_for_context(&wrong).unwrap().is_none());
    }
    #[test]
    fn unverified_newer_native_image_cannot_settle_old_completed_history() {
        let (_temp, plan) = retained_fixture("0.4.2");
        let mut record = persist_fixture(&plan);
        record.phase = MigrationPhase::Complete;
        save(&plan, &record).unwrap();
        let evidence_before = fs::read(evidence_path(&plan)).unwrap();
        let journal_before = fs::read(journal_path(&plan)).unwrap();
        let mut ctx = context(&plan);
        ctx.executable = plan.target.image.path.clone();
        ctx.version = crate::identity::neutral::VERSION.into();
        ctx.running = plan.target.image.clone();
        ctx.running.sha256 = "f".repeat(64);
        ctx.running_origin = Some(Box::new(plan.target.origin.clone()));
        assert!(latest_for_context(&ctx).is_err());
        assert_eq!(fs::read(evidence_path(&plan)).unwrap(), evidence_before);
        assert_eq!(fs::read(journal_path(&plan)).unwrap(), journal_before);
    }
    #[test]
    fn retained_older_unknown_history_is_read_only_and_keeps_its_evidence_binding() {
        let (_temp, plan) = retained_fixture("0.4.2");
        let mut record = persist_fixture(&plan);
        record.phase = MigrationPhase::ManagerUnknown;
        save(&plan, &record).unwrap();
        let ctx = context(&plan);
        let (retained, observed) = latest_for_context(&ctx).unwrap().unwrap();
        assert_eq!(retained, plan);
        assert_eq!(observed.phase, MigrationPhase::ManagerUnknown);
        assert!(validate_plan(&retained).is_err());
        assert!(read_plan(&evidence_path(&retained)).is_err());
        assert!(write_new_plan(&retained).is_err());
        record.plan_sha256 = "0".repeat(64);
        save(&plan, &record).unwrap();
        assert!(latest_for_context(&ctx).is_err());
    }
    #[test]
    fn retained_045_history_keeps_original_scope_without_active_authority() {
        let (_temp, plan) = retained_fixture("0.4.5");
        let mut record = persist_fixture(&plan);
        record.phase = MigrationPhase::ManagerUnknown;
        save(&plan, &record).unwrap();
        let evidence_before = fs::read(evidence_path(&plan)).unwrap();
        let journal_before = fs::read(journal_path(&plan)).unwrap();
        let mut ctx = context(&plan);
        ctx.version = "0.3.10".into();
        let (retained, observed) = latest_for_context(&ctx).unwrap().unwrap();
        assert_eq!(retained, plan);
        assert_eq!(observed, record);
        assert!(read_plan(&evidence_path(&retained)).is_err());
        assert!(write_new_plan(&retained).is_err());
        ctx.state_dir = ctx.state_dir.join("unrelated");
        assert!(latest_for_context(&ctx).unwrap().is_none());
        assert_eq!(fs::read(evidence_path(&plan)).unwrap(), evidence_before);
        assert_eq!(fs::read(journal_path(&plan)).unwrap(), journal_before);
    }
    #[test]
    fn retained_046_unknown_history_cannot_authorize_the_047_engine() {
        let (_temp, plan) = retained_fixture("0.4.6");
        let mut record = persist_fixture(&plan);
        record.phase = MigrationPhase::ManagerUnknown;
        save(&plan, &record).unwrap();
        let evidence_before = fs::read(evidence_path(&plan)).unwrap();
        let journal_before = fs::read(journal_path(&plan)).unwrap();
        assert_eq!(
            latest_for_context(&context(&plan)).unwrap(),
            Some((plan.clone(), record))
        );
        assert!(read_plan(&evidence_path(&plan)).is_err());
        assert!(write_new_plan(&plan).is_err());
        assert_eq!(fs::read(evidence_path(&plan)).unwrap(), evidence_before);
        assert_eq!(fs::read(journal_path(&plan)).unwrap(), journal_before);
    }

    fn consent_frame(
        plan: &MigrationPlan,
        path: &Path,
        retirement_only: bool,
        outcome: MigrationFinalConsentOutcome,
    ) -> MigrationFinalConsentFrame {
        final_consent_frame(
            plan,
            format!(
                "{:x}",
                Sha256::digest(owned_private_file(path, MAX_PLAN).unwrap())
            ),
            retirement_only,
            outcome,
        )
    }

    fn frame_bytes(frame: &MigrationFinalConsentFrame) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(frame).unwrap();
        bytes.push(b'\n');
        bytes
    }

    #[test]
    fn final_consent_cancel_is_distinct_from_visible_approval_and_preserves_profiles() {
        let (_temp, plan) = fixture();
        let config = plan.profiles[0].config_dir.join("config.json");
        let state = plan.profiles[0].state_dir.join("state.json");
        fs::write(&config, b"original configuration").unwrap();
        fs::write(&state, b"original state").unwrap();
        let path = write_new_plan(&plan).unwrap();
        let plan_before = fs::read(&path).unwrap();
        for retirement_only in [false, true] {
            for outcome in [
                MigrationFinalConsentOutcome::Cancelled,
                MigrationFinalConsentOutcome::VisibleApproved,
            ] {
                let frame = consent_frame(&plan, &path, retirement_only, outcome);
                assert_eq!(
                    frame.helper_ui_visible,
                    outcome == MigrationFinalConsentOutcome::VisibleApproved
                );
                assert_eq!(
                    frame.source_ui_visible,
                    outcome == MigrationFinalConsentOutcome::VisibleApproved && !retirement_only
                );
                let mut bytes = frame_bytes(&frame);
                if outcome == MigrationFinalConsentOutcome::VisibleApproved {
                    bytes.extend_from_slice(b"retained terminal result\n");
                }
                let mut reader = bytes.as_slice();
                assert_eq!(
                    read_final_consent_outcome(&mut reader, &plan, &path, retirement_only).unwrap(),
                    outcome
                );
                assert!(reader.is_empty());
            }
        }
        assert_eq!(fs::read(config).unwrap(), b"original configuration");
        assert_eq!(fs::read(state).unwrap(), b"original state");
        assert_eq!(fs::read(path).unwrap(), plan_before);
        assert!(!journal_path(&plan).exists());
        assert!(!evidence_path(&plan).exists());
    }

    #[test]
    fn final_consent_rejects_foreign_authority_and_false_visibility() {
        let (_temp, plan) = fixture();
        let path = write_new_plan(&plan).unwrap();
        for outcome in [
            MigrationFinalConsentOutcome::Cancelled,
            MigrationFinalConsentOutcome::VisibleApproved,
        ] {
            let original = consent_frame(&plan, &path, false, outcome);
            let mut altered = Vec::new();
            let mut frame = original.clone();
            frame.operation_id = "f".repeat(32);
            altered.push(frame);
            let mut frame = original.clone();
            frame.plan_sha256 = "0".repeat(64);
            altered.push(frame);
            let mut frame = original.clone();
            frame.source_image.sha256 = "e".repeat(64);
            altered.push(frame);
            let mut frame = original.clone();
            frame.target_image.sha256 = "e".repeat(64);
            altered.push(frame);
            let mut frame = original.clone();
            frame.profile_paths[0].state_dir.push("foreign");
            altered.push(frame);
            let mut frame = original.clone();
            frame.helper_ui_visible = !frame.helper_ui_visible;
            altered.push(frame);
            let mut frame = original.clone();
            frame.source_ui_visible = !frame.source_ui_visible;
            altered.push(frame);
            let mut frame = original.clone();
            frame.retirement_only = true;
            altered.push(frame);
            let mut frame = original.clone();
            frame.version = 3;
            altered.push(frame);
            let mut frame = original;
            frame.kind = "legacy_migration_consent_ack".into();
            altered.push(frame);
            for frame in altered {
                let bytes = frame_bytes(&frame);
                let mut reader = bytes.as_slice();
                assert!(read_final_consent_outcome(&mut reader, &plan, &path, false).is_err());
                assert!(reader.is_empty());
            }
        }
        let mut changed_plan = plan.clone();
        changed_plan.adopt_as_default = true;
        let frame = consent_frame(
            &changed_plan,
            &path,
            false,
            MigrationFinalConsentOutcome::Cancelled,
        );
        assert!(read_final_consent_outcome(
            &mut frame_bytes(&frame).as_slice(),
            &changed_plan,
            &path,
            false
        )
        .is_err());
    }

    #[test]
    fn final_consent_eof_truncation_invalid_frames_and_cancel_extra_output_stay_ambiguous() {
        let (_temp, plan) = fixture();
        let path = write_new_plan(&plan).unwrap();
        let valid = frame_bytes(&consent_frame(
            &plan,
            &path,
            false,
            MigrationFinalConsentOutcome::Cancelled,
        ));
        let mut cases = vec![
            Vec::new(),
            valid[..valid.len() - 1].to_vec(),
            b"{broken}\n".to_vec(),
            vec![b'x'; MAX_PLAN as usize + 1],
        ];
        for (field, value) in [
            ("outcome", serde_json::json!("unsupported")),
            ("unexpected", serde_json::json!(true)),
        ] {
            let mut frame: serde_json::Value = serde_json::from_slice(&valid).unwrap();
            frame[field] = value;
            let mut bytes = serde_json::to_vec(&frame).unwrap();
            bytes.push(b'\n');
            cases.push(bytes);
        }
        let text = String::from_utf8(valid.clone()).unwrap();
        cases.push(
            text.replacen("\"version\":4", "\"version\":4,\"version\":4", 1)
                .into_bytes(),
        );
        for extra in [b"\n".as_slice(), b"{\"phase\":\"complete\"}\n".as_slice()] {
            let mut bytes = valid.clone();
            bytes.extend_from_slice(extra);
            cases.push(bytes);
        }
        for bytes in cases {
            let mut reader = bytes.as_slice();
            assert!(read_final_consent_outcome(&mut reader, &plan, &path, false).is_err());
            assert!(reader.is_empty());
        }
    }

    #[test]
    fn final_consent_cancel_requires_a_successful_reaped_child_not_a_frame_alone() {
        let (_temp, plan) = fixture();
        let path = write_new_plan(&plan).unwrap();
        let bytes = frame_bytes(&consent_frame(
            &plan,
            &path,
            false,
            MigrationFinalConsentOutcome::Cancelled,
        ));
        for (output, termination, expected_cancel) in [
            (bytes.as_slice(), "exit 0", true),
            (bytes.as_slice(), "exit 1", false),
            (bytes.as_slice(), "kill -TERM $$", false),
            (b"".as_slice(), "exit 0", false),
            (b"".as_slice(), "kill -TERM $$", false),
            (b"{broken}\n".as_slice(), "exit 0", false),
        ] {
            let mut child = Command::new("/bin/sh")
                .args(["-c", &format!("cat; {termination}")])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(output).unwrap();
            let result = wait_for_final_consent(&mut child, &plan, &path, false);
            if expected_cancel {
                assert_eq!(result.unwrap(), MigrationFinalConsentOutcome::Cancelled);
            } else {
                assert!(result.is_err());
            }
            assert!(child.try_wait().unwrap().is_some());
        }
    }
}

fn presented_control(
    plan: &MigrationPlan,
    profile: &LegacyProfile,
    instance: &str,
    command: MigrationCommand,
    target: bool,
    presentation: Option<&dyn MigrationPresentation>,
) -> Result<(), String> {
    if presentation.is_none() {
        return control(plan, profile, instance, command, target);
    }
    let plan = plan.clone();
    let profile = profile.clone();
    let instance = instance.to_owned();
    off_ui(presentation, move || {
        control(&plan, &profile, &instance, command, target)
    })
}
fn abort_live_preparation(
    plan: &MigrationPlan,
    profile: &LegacyProfile,
    presentation: Option<&dyn MigrationPresentation>,
) -> Result<(), String> {
    let live = status(profile)?;
    if live.pid != profile.pid
        || live.instance_id != profile.instance_id
        || live.executable_path != plan.source_image.path
        || actual_process_image(live.pid)? != plan.source_image.path
    {
        return Err("rollback preparation instance changed".into());
    }
    if live.migration_operation.as_deref() == Some(&plan.operation_id) && live.migration_gated {
        presented_control(
            plan,
            profile,
            &profile.instance_id,
            MigrationCommand::Abort,
            false,
            presentation,
        )?;
    }
    Ok(())
}
fn assert_final_source_visibility(
    plan: &MigrationPlan,
    profile: &LegacyProfile,
) -> Result<(), String> {
    let live = status(profile)?;
    verify_status(plan, profile, &live, false)?;
    if !live.update_ui_visible
        || live.update_operation_id.as_deref() != Some(&plan.operation_id)
        || !live.migration_gated
        || live.migration_operation.as_deref() != Some(&plan.operation_id)
    {
        return Err(
            "bridge final consent UI is not visibly bound to the prepared operation".into(),
        );
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MigrationProfilePaths {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MigrationFinalConsentOutcome {
    VisibleApproved,
    DelegatedApproved,
    Cancelled,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct MigrationFinalConsentFrame {
    version: u32,
    kind: String,
    operation_id: String,
    plan_sha256: String,
    source_image: ExecutableIdentity,
    target_image: ExecutableIdentity,
    profile_paths: Vec<MigrationProfilePaths>,
    retirement_only: bool,
    outcome: MigrationFinalConsentOutcome,
    helper_ui_visible: bool,
    source_ui_visible: bool,
}
fn final_consent_frame(
    plan: &MigrationPlan,
    plan_sha256: String,
    retirement_only: bool,
    outcome: MigrationFinalConsentOutcome,
) -> MigrationFinalConsentFrame {
    let approved = outcome == MigrationFinalConsentOutcome::VisibleApproved;
    MigrationFinalConsentFrame {
        version: 4,
        kind: "legacy_migration_final_consent".into(),
        operation_id: plan.operation_id.clone(),
        plan_sha256,
        source_image: plan.source_image.clone(),
        target_image: plan.target.image.clone(),
        profile_paths: plan
            .profiles
            .iter()
            .map(|profile| MigrationProfilePaths {
                config_dir: profile.config_dir.clone(),
                state_dir: profile.state_dir.clone(),
            })
            .collect(),
        retirement_only,
        outcome,
        helper_ui_visible: approved,
        source_ui_visible: approved && !retirement_only,
    }
}
fn write_final_consent_frame(frame: &MigrationFinalConsentFrame) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(frame).map_err(|e| e.to_string())?;
    if bytes.len() + 1 > MAX_PLAN as usize {
        return Err("final consent outcome exceeds bound".into());
    }
    bytes.push(b'\n');
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&bytes)
        .and_then(|_| stdout.flush())
        .map_err(|e| format!("cannot flush migration final consent outcome: {e}"))
}
/// Called only after the native final dialog actually returns Cancel, before
/// entering the migration engine. Cancellation is not a visibility acknowledgement.
pub fn write_final_consent_cancelled(
    plan: &MigrationPlan,
    plan_path: &Path,
    retirement_only: bool,
) -> Result<(), String> {
    let evidence = owned_private_file(plan_path, MAX_PLAN)?;
    let stored: MigrationPlan = serde_json::from_slice(&evidence).map_err(|e| e.to_string())?;
    if stored != *plan {
        return Err("cancelled migration consent plan changed".into());
    }
    write_final_consent_frame(&final_consent_frame(
        plan,
        format!("{:x}", Sha256::digest(&evidence)),
        retirement_only,
        MigrationFinalConsentOutcome::Cancelled,
    ))
}
fn write_authorization_ack(
    plan: &MigrationPlan,
    record: &MigrationRecord,
    retirement_only: bool,
) -> Result<(), String> {
    if !plan
        .profiles
        .iter()
        .all(|profile| record.acknowledged_profiles.contains(&profile.instance_id))
    {
        return Err("not every source profile flushed its final consent acknowledgement".into());
    }
    write_final_consent_frame(&final_consent_frame(
        plan,
        record.plan_sha256.clone(),
        retirement_only,
        if delegated_approval(plan)? {
            MigrationFinalConsentOutcome::DelegatedApproved
        } else {
            MigrationFinalConsentOutcome::VisibleApproved
        },
    ))
}
fn read_final_consent_frame<R: Read>(
    reader: &mut R,
    plan: &MigrationPlan,
    plan_path: &Path,
    retirement_only: bool,
) -> Result<MigrationFinalConsentOutcome, String> {
    let mut bytes = Vec::with_capacity(2048);
    let mut byte = [0u8; 1];
    loop {
        if bytes.len() >= MAX_PLAN as usize {
            return Err("helper final consent outcome exceeds bound".into());
        }
        if reader.read(&mut byte).map_err(|e| e.to_string())? == 0 {
            return Err("helper exited without a complete final consent outcome".into());
        }
        if byte[0] == b'\n' {
            break;
        }
        bytes.push(byte[0]);
    }
    let frame: MigrationFinalConsentFrame =
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let evidence = owned_private_file(plan_path, MAX_PLAN)?;
    let stored: MigrationPlan = serde_json::from_slice(&evidence).map_err(|e| e.to_string())?;
    let approved = frame.outcome == MigrationFinalConsentOutcome::VisibleApproved;
    if frame.outcome == MigrationFinalConsentOutcome::DelegatedApproved
        && !delegated_approval(plan)?
    {
        return Err("helper delegation has no matching original update authority".into());
    }
    if stored != *plan
        || frame.version != 4
        || frame.kind != "legacy_migration_final_consent"
        || frame.operation_id != plan.operation_id
        || frame.plan_sha256 != format!("{:x}", Sha256::digest(&evidence))
        || frame.source_image != plan.source_image
        || frame.target_image != plan.target.image
        || frame.profile_paths.len() != plan.profiles.len()
        || !frame
            .profile_paths
            .iter()
            .zip(&plan.profiles)
            .all(|(paths, profile)| {
                paths.config_dir == profile.config_dir && paths.state_dir == profile.state_dir
            })
        || frame.retirement_only != retirement_only
        || frame.helper_ui_visible != approved
        || frame.source_ui_visible != (approved && !retirement_only)
    {
        return Err("helper final consent outcome is not bound to this exact consent plan".into());
    }
    Ok(frame.outcome)
}
/// Consume the owned helper's bounded first frame, then drain its stdout before
/// the caller reaps it. A cancellation must be the entire successful output;
/// an approved frame never substitutes for the independently retained journal.
pub fn read_final_consent_outcome<R: Read>(
    reader: &mut R,
    plan: &MigrationPlan,
    plan_path: &Path,
    retirement_only: bool,
) -> Result<MigrationFinalConsentOutcome, String> {
    let outcome = read_final_consent_frame(reader, plan, plan_path, retirement_only);
    // Drain even malformed frames so the child cannot block on its output pipe
    // while the owner waits to reap it.
    let remaining = std::io::copy(reader, &mut std::io::sink())
        .map_err(|error| format!("Migration helper result channel failed: {error}"));
    let outcome = outcome?;
    if remaining? != 0 && outcome == MigrationFinalConsentOutcome::Cancelled {
        return Err("cancelled migration helper emitted unexpected additional output".into());
    }
    Ok(outcome)
}

/// Reap the exact launched helper even if its ownership frame is invalid.
/// A valid cancellation frame alone is insufficient without a coherent exit.
pub fn wait_for_final_consent(
    child: &mut std::process::Child,
    plan: &MigrationPlan,
    plan_path: &Path,
    retirement_only: bool,
) -> Result<MigrationFinalConsentOutcome, String> {
    let outcome = match child.stdout.take() {
        Some(mut stdout) => {
            read_final_consent_outcome(&mut stdout, plan, plan_path, retirement_only)
        }
        None => Err("Migration helper ownership channel is missing".into()),
    };
    let status = child
        .wait()
        .map_err(|error| format!("Migration helper wait failed; delivery is unknown: {error}"))?;
    let outcome = outcome?;
    if !status.success() {
        return Err(format!(
            "Migration helper exited with {status}; inspect the retained journal before another operation"
        ));
    }
    Ok(outcome)
}

fn profile_home(environment: &BTreeMap<String, String>, os_home: &Path) -> Result<PathBuf, String> {
    let home = environment
        .get("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| os_home.to_path_buf());
    crate::ensure_absolute(&home)?;
    Ok(home)
}
fn neutral_config_root(
    environment: &BTreeMap<String, String>,
    os_home: &Path,
) -> Result<PathBuf, String> {
    let root = environment
        .get("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or(profile_home(environment, os_home)?.join(".config"));
    crate::ensure_absolute(&root)?;
    Ok(root.join("desktop-pet"))
}
fn default_profile_matches(
    previous: &PreviousRecord,
    profile: &LegacyProfile,
) -> Result<bool, String> {
    let Some(bytes) = &previous.bytes else {
        return Ok(true);
    };
    let old: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    Ok(
        old == serde_json::json!({"version":1,"config_dir":profile.config_dir,"state_dir":profile.state_dir})
    )
}

fn capture_target_profiles(
    target: &MigrationTarget,
) -> Result<Vec<protocol::MigrationProfile>, String> {
    let active =
        protocol::active_profiles_for_roots(&[
            crate::physical_install_root(&target.origin)?.to_path_buf()
        ])?;
    let mut profiles = Vec::with_capacity(active.len());
    for registered in active {
        let live = status_at(&registered.state_dir.join("control.sock"))?;
        let image = ExecutableIdentity {
            path: live.executable_path.clone(),
            sha256: live.running_sha256.clone(),
        };
        let current =
            image == target.image && live.app_version == crate::identity::neutral::VERSION;
        let retained_brew = matches!(&target.origin, InstallOrigin::Homebrew { cellar_formula_root, .. }
            if image.path.starts_with(cellar_formula_root) && image.path != target.image.path)
            && matches!(live.app_version.as_str(), "0.4.7" | "0.4.8");
        if !live.ok
            || !live.running
            || live.shutdown
            || !live.ready
            || !live.ui_ready
            || !live.control_ready
            || !(current || retained_brew)
            || live.updater_protocol != 4
            || live.observation_capability != 2
            || live.integration_api != 1
            || live.pid != registered.pid
            || live.instance_id != registered.instance_id
            || live.state_dir != registered.state_dir
            || live.migration_gated
            || actual_process_image(live.pid)? != image.path
            || executable_identity(&image.path)? != image
        {
            return Err(
                "existing target profile does not prove immutable compatible neutral reuse".into(),
            );
        }
        if retained_brew {
            let bundle = image
                .path
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .ok_or("retained Brew target image has no bundle")?;
            command_output(
                Path::new("/usr/bin/codesign"),
                &[
                    "--verify".into(),
                    "--deep".into(),
                    "--strict".into(),
                    bundle.to_string_lossy().into_owned(),
                ],
            )?;
        }
        owned_directory(&live.config_dir)?;
        owned_directory(&live.state_dir)?;
        profiles.push(protocol::MigrationProfile {
            pid: live.pid,
            instance_id: live.instance_id,
            config_dir: live.config_dir,
            state_dir: live.state_dir.clone(),
            image,
            stop_generation: migration_stop_generation(&live.state_dir)?,
        });
    }
    Ok(profiles)
}
/// Bounded, read-only history lookup. A terminal native journal is not converted
/// into new authority; explicit resume still verifies its owned scope and Stop.
pub fn latest_for_context(
    context: &UpdateContext,
) -> Result<Option<(MigrationPlan, MigrationRecord)>, String> {
    crate::ensure_absolute(&context.state_dir)?;
    image_valid(&context.running)?;
    let directory = context.state_dir.join("updates-v4");
    match fs::symlink_metadata(&directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
        Ok(_) => owned_directory(&directory)?,
    }
    let mut latest = None;
    let mut latest_time = None;
    let mut count = 0;
    for entry in fs::read_dir(&directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.ends_with(".migration-plan.json") && !name.ends_with(".migration-profile.json") {
            continue;
        }
        count += 1;
        if count > 1024 {
            return Err("migration history exceeds bound".into());
        }
        let plan_path = if name.ends_with(".migration-profile.json") {
            integrations::profile_plan_path(&entry.path(), &context.config_dir, &context.state_dir)?
        } else {
            entry.path()
        };
        // History keeps its originally approved release; current-version binding
        // still applies to every new plan, execution, startup and resume.
        let plan: MigrationPlan =
            serde_json::from_slice(&owned_private_file(&plan_path, MAX_PLAN)?)
                .map_err(|e| e.to_string())?;
        validate_plan_structure(&plan)?;
        if plan_path != evidence_path(&plan) {
            return Err("migration evidence name does not match operation/profile".into());
        }
        if !plan
            .profiles
            .iter()
            .any(|p| p.config_dir == context.config_dir && p.state_dir == context.state_dir)
        {
            continue;
        }
        let retained = read_record(&plan)?;
        let actual_target = target_image(&plan, &retained)?;
        if &context.running != actual_target
            && context.running != plan.source_image
            && !integrations::retained_owner_matches(&plan, context)?
        {
            continue;
        }
        let record = if &context.running != actual_target && context.running != plan.source_image {
            integrations::observe_for_context(&plan, context)?
        } else {
            retained
        };
        let modified = fs::symlink_metadata(journal_path(&plan))
            .map_err(|e| e.to_string())?
            .modified()
            .map_err(|e| e.to_string())?;
        if latest_time.is_none_or(|time| modified > time) {
            latest_time = Some(modified);
            latest = Some((plan, record));
        }
    }
    Ok(latest)
}

fn verify_neutral_profile(
    image: &ExecutableIdentity,
    profile: &LegacyProfile,
    live: &LiveStatus,
) -> Result<(), String> {
    if !live.ok
        || !live.running
        || live.shutdown
        || !live.ready
        || !live.ui_ready
        || !live.control_ready
        || live.migration_gated
        || live.app_version != crate::identity::neutral::VERSION
        || live.updater_protocol != 4
        || live.observation_capability != 2
        || live.integration_api != 1
        || live.executable_path != image.path
        || live.running_sha256 != image.sha256
        || !instance_valid(&live.instance_id)
        || live.config_dir != profile.config_dir
        || live.state_dir != profile.state_dir
        || actual_process_image(live.pid)? != image.path
        || executable_identity(&image.path)? != *image
    {
        return Err(
            "current native daemon does not prove the migrated image/profile readiness".into(),
        );
    }
    Ok(())
}

fn connect_bounded(path: &Path, deadline: Instant) -> Result<UnixStream, String> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.contains(&0) || bytes.len() >= address.sun_path.len() {
        return Err("control socket path exceeds Unix address bound".into());
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (target, source) in address.sun_path.iter_mut().zip(bytes) {
        *target = *source as libc::c_char;
    }
    let length = std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1;
    #[cfg(target_os = "macos")]
    {
        address.sun_len = length as u8;
    }
    let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if raw < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let socket = unsafe { OwnedFd::from_raw_fd(raw) };
    if unsafe { libc::fcntl(socket.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0
        || unsafe { libc::fcntl(socket.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } < 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    loop {
        let result = unsafe {
            libc::connect(
                socket.as_raw_fd(),
                (&address as *const libc::sockaddr_un).cast(),
                length as libc::socklen_t,
            )
        };
        if result == 0 {
            break;
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EINPROGRESS) | Some(libc::EALREADY) => {
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .ok_or("control connect deadline expired")?;
                let millis = remaining.as_millis().max(1).min(i32::MAX as u128) as i32;
                let mut item = libc::pollfd {
                    fd: socket.as_raw_fd(),
                    events: libc::POLLOUT,
                    revents: 0,
                };
                let ready = unsafe { libc::poll(&mut item, 1, millis) };
                if ready == 0 {
                    return Err("control connect deadline expired".into());
                }
                if ready < 0 {
                    if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                        continue;
                    }
                    return Err(std::io::Error::last_os_error().to_string());
                }
                let mut failure: libc::c_int = 0;
                let mut size = std::mem::size_of_val(&failure) as libc::socklen_t;
                if unsafe {
                    libc::getsockopt(
                        socket.as_raw_fd(),
                        libc::SOL_SOCKET,
                        libc::SO_ERROR,
                        (&mut failure as *mut libc::c_int).cast(),
                        &mut size,
                    )
                } != 0
                {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                if failure != 0 {
                    return Err(std::io::Error::from_raw_os_error(failure).to_string());
                }
                break;
            }
            Some(libc::EAGAIN) | Some(libc::EINTR) => {
                if Instant::now() >= deadline {
                    return Err("control connect deadline expired".into());
                }
                thread::sleep(Duration::from_millis(10));
            }
            _ => return Err(error.to_string()),
        }
    }
    let stream = UnixStream::from(socket);
    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
    Ok(stream)
}

fn verify_manager_registry(plan: &MigrationPlan) -> Result<(), String> {
    if let LegacyOrigin::HerdrManaged {
        host,
        host_config_dir,
        ..
    } = &plan.source_origin
    {
        if executable_identity(&host.path)? != *host {
            return Err("captured manager executable changed".into());
        }
        let directory = crate::origin::command_output_with_environment(
            &host.path,
            &["plugin", "config-dir", "desktop-pet"],
            8192,
            15,
            &manager_environment(plan)?,
        )?;
        if directory.lines().count() != 1 {
            return Err("manager config-dir did not return one bounded registry path".into());
        }
        let path = PathBuf::from(directory.trim());
        crate::ensure_absolute(&path)?;
        let actual = fs::canonicalize(&path).map_err(|e| e.to_string())?;
        if actual != host_config_dir.join("plugins/config/desktop-pet") {
            return Err("captured manager would refresh a different Herdr registry".into());
        }
    }
    Ok(())
}
fn verify_git_remote(root: &Path) -> Result<(), String> {
    let remote = command_output(
        Path::new("/usr/bin/git"),
        &[
            "-C".into(),
            root.to_string_lossy().into_owned(),
            "config".into(),
            "--get".into(),
            "remote.origin.url".into(),
        ],
    )?;
    if !matches!(
        remote.trim(),
        "https://github.com/hanbong5938/herdr-desktop-pet"
            | "https://github.com/hanbong5938/herdr-desktop-pet.git"
            | "git@github.com:hanbong5938/herdr-desktop-pet.git"
            | "ssh://git@github.com/hanbong5938/herdr-desktop-pet.git"
    ) {
        return Err(
            "managed source remote changed; migration never rewrites/reset user source".into(),
        );
    }
    Ok(())
}
fn retire_exact_bundle(plan: &MigrationPlan, bundle: &Path, label: &str) -> Result<(), String> {
    owned_directory(bundle)?;
    if executable_identity(&bundle.join("Contents/MacOS/herdr-desktop-pet"))? != plan.source_image {
        return Err("retirement bundle changed since consent; preserve user bytes".into());
    }
    #[cfg(target_os = "macos")]
    command_output(
        Path::new("/usr/bin/codesign"),
        &[
            "--verify".into(),
            "--deep".into(),
            "--strict".into(),
            bundle.to_string_lossy().into_owned(),
        ],
    )?;
    let retained = plan.profiles[0]
        .state_dir
        .join("updates-v4")
        .join(format!("{}.{label}.app", plan.operation_id));
    if retained.exists() {
        return Err("retired bundle evidence already exists; preserve both copies".into());
    }
    if stop_requested(plan)? {
        return Err("user Stop prevents approved bundle retirement".into());
    }
    match fs::rename(bundle, &retained) {
        Ok(()) => {}
        Err(error) if error.raw_os_error() == Some(libc::EXDEV) => {
            // The full signed pre-Stop snapshot already resides on the evidence
            // volume. Cross-volume retirement removes only the consented app.
            let backup = plan.profiles[0]
                .state_dir
                .join("updates-v4")
                .join(format!("{}.bridge-evidence.app", plan.operation_id));
            if executable_identity(&backup.join("Contents/MacOS/herdr-desktop-pet"))?.sha256
                != plan.source_image.sha256
            {
                return Err(
                    "cross-volume retirement lacks exact retained signed bridge evidence".into(),
                );
            }
            #[cfg(target_os = "macos")]
            command_output(
                Path::new("/usr/bin/codesign"),
                &[
                    "--verify".into(),
                    "--deep".into(),
                    "--strict".into(),
                    backup.to_string_lossy().into_owned(),
                ],
            )?;
            if stop_requested(plan)? {
                return Err("user Stop prevents cross-volume legacy retirement".into());
            }
            fs::remove_dir_all(bundle).map_err(|e| e.to_string())?;
        }
        Err(error) => return Err(error.to_string()),
    }
    File::open(bundle.parent().ok_or("legacy bundle parent missing")?)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    File::open(retained.parent().ok_or("evidence parent missing")?)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}
