//! Immutable v2 approval and durable observations for Pet-only standalone installs.
//! Version 1 is historical read-only evidence, never this helper's mutation authority.
//! Serialized evidence is never a substitute for an authenticated runtime Stop ACK.
//! Manual recovery is a distinct SSH-operator consent for H → C using an
//! already normally stopped source. Its prior runtime context is historical:
//! installation completes stopped and never authorizes Stop, ACK, or restart.
//! The runner must establish fresh generation/quiescence and retain the official
//! archive, archive.sha256, and SHA256SUMS beside the candidate bundle.
use crate::installation::{InstallationKind, InstallationRecord};
use crate::native_runtime::ProfilePaths;
use crate::standalone_relocation::{
    validate_sha256, RelocationTernApproval, SettledRelocationParent,
};
use crate::{
    ensure_absolute, owned_private_file, protocol, ExecutableIdentity, InstallOrigin,
    ReleaseCandidate, ReleaseChannel, UpdateContext,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::os::unix::fs::MetadataExt;
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

/// The purpose is the only destination authority in a standalone approval.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StandaloneInstallPurpose {
    #[default]
    Upgrade,
    PrivateRelocation {
        target: InstallOrigin,
        parent: SettledRelocationParent,
        source_closure_sha256: String,
        tern: Option<RelocationTernApproval>,
    },
    /// An explicit operator-approved stopped-source replacement, not a native
    /// relocation approval and never authority to Stop or restart the source.
    ManualRecovery {
        target: InstallOrigin,
        release: ReleaseCandidate,
        previous_context: UpdateContext,
        /// Only a v2 operator request can opt into a newer official beta.
        /// Omission preserves the historical stable-only approval semantics.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        beta_confirmation: Option<String>,
        source_closure_sha256: String,
        tern: Option<RelocationTernApproval>,
    },
}

impl StandaloneInstallPurpose {
    pub fn is_upgrade(&self) -> bool {
        matches!(self, Self::Upgrade)
    }
}

pub const MANUAL_BETA_RECOVERY_CONFIRMATION: &str =
    "replace_original_app_preserve_profile_allow_beta";

fn manual_recovery_release_allowed(
    release: &ReleaseCandidate,
    beta_confirmation: Option<&str>,
) -> bool {
    match beta_confirmation {
        None => release.channel == ReleaseChannel::Stable,
        Some(MANUAL_BETA_RECOVERY_CONFIRMATION) => release.channel == ReleaseChannel::Beta,
        Some(_) => false,
    }
}

/// Private CLI operator input. This is intentionally not native-panel consent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandaloneManualRecoveryRequest {
    pub version: u32,
    pub context: UpdateContext,
    pub candidate_bundle: PathBuf,
    pub release: ReleaseCandidate,
    pub confirmation: String,
}

impl StandaloneManualRecoveryRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.release.validate_neutral()?;
        ensure_absolute(&self.candidate_bundle)?;
        let [private, historical] = crate::installation::standalone_bundles()?;
        let source = canonical_version(&self.context.version)?;
        let candidate = canonical_version(&self.release.version)?;
        let beta_confirmation = match (self.version, self.confirmation.as_str()) {
            (1, "replace_original_app_preserve_profile") => None,
            (2, MANUAL_BETA_RECOVERY_CONFIRMATION) => Some(self.confirmation.as_str()),
            _ => {
                return Err(
                    "invalid manual recovery operator request version or confirmation".into(),
                )
            }
        };
        if !manual_recovery_release_allowed(&self.release, beta_confirmation)
            || candidate.cmp_precedence(&source) != std::cmp::Ordering::Greater
            || !crate::UpdatePolicy::StableAndBeta.allows_version(&self.context.version)?
            || self.context.executable != historical.join("Contents/MacOS/desktop-pet")
            || self.context.running.path != self.context.executable
            || self.context.running_origin.as_deref()
                != Some(&InstallOrigin::Standalone {
                    bundle: historical.clone(),
                })
            || self.context.instance_id.is_empty()
            || self.context.instance_id.len() > 128
        {
            return Err("invalid manual recovery operator request".into());
        }
        identity(&self.context.running)?;
        self.context.observation_mode()?;
        validate_recovery_paths(
            &ProfilePaths {
                config_dir: self.context.config_dir.clone(),
                state_dir: self.context.state_dir.clone(),
            },
            &self.context,
            &historical,
            &private,
        )
    }

    fn validate_for_plan(&self, plan: &StandaloneInstallPlan) -> Result<(), String> {
        self.validate()?;
        plan.validate()?;
        let StandaloneInstallPurpose::ManualRecovery {
            release,
            previous_context,
            beta_confirmation,
            ..
        } = &plan.purpose
        else {
            return Err("operator request is not authority for this installation purpose".into());
        };
        if &self.context != previous_context
            || &self.release != release
            || self.candidate_bundle != plan.candidate_bundle
            || beta_confirmation.as_deref()
                != (self.version == 2).then_some(self.confirmation.as_str())
        {
            return Err("manual recovery request differs from the immutable approval".into());
        }
        Ok(())
    }
}

fn manual_request_bytes(path: &Path) -> Result<Vec<u8>, String> {
    ensure_absolute(path)?;
    protocol::check_dir(
        path.parent()
            .ok_or("manual recovery request lacks parent")?,
        false,
    )?;
    owned_private_file(path, MAX_PLAN)
}

pub fn read_manual_recovery_request(
    path: &Path,
) -> Result<StandaloneManualRecoveryRequest, String> {
    let request: StandaloneManualRecoveryRequest =
        serde_json::from_slice(&manual_request_bytes(path)?)
            .map_err(|e| format!("invalid manual recovery request: {e}"))?;
    request.validate()?;
    Ok(request)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandaloneOperatorConsent {
    pub version: u32,
    pub operation_id: String,
    pub plan_sha256: String,
    pub request_sha256: String,
    pub method: String,
}

impl StandaloneOperatorConsent {
    /// Digest arguments must be hashes of actual stored bytes, not reserialized
    /// structures. This consent grants neither Stop, ACK nor startup authority.
    pub fn validate(
        &self,
        plan: &StandaloneInstallPlan,
        plan_sha256: &str,
        request_sha256: &str,
    ) -> Result<(), String> {
        plan.validate()?;
        protocol::validate_operation_id(&self.operation_id)?;
        validate_sha256(&self.plan_sha256)?;
        validate_sha256(&self.request_sha256)?;
        validate_sha256(plan_sha256)?;
        validate_sha256(request_sha256)?;
        if self.version != 1
            || self.operation_id != plan.operation_id
            || self.plan_sha256 != plan_sha256
            || self.request_sha256 != request_sha256
            || self.method != "ssh_operator"
            || !matches!(
                plan.purpose,
                StandaloneInstallPurpose::ManualRecovery { .. }
            )
        {
            return Err("manual recovery operator consent does not match stored approval".into());
        }
        Ok(())
    }
}

pub fn operator_consent_path(state: &Path, id: &str) -> Result<PathBuf, String> {
    path(state, id, "operator-consent")
}

/// Fixed immutable request resource shared by the runner and historical readers.
pub fn manual_operator_request_path(state: &Path, id: &str) -> Result<PathBuf, String> {
    protocol::validate_operation_id(id)?;
    ensure_absolute(state)?;
    Ok(state
        .join(crate::identity::UPDATES_DIRECTORY)
        .join(format!("install-{id}.resources"))
        .join("operator-request.json"))
}

fn operator_consent_digests(
    plan: &StandaloneInstallPlan,
    request_path: &Path,
) -> Result<(String, String), String> {
    plan.validate()?;
    check_read_directory(&plan.profile.state_dir)?;
    let plan_bytes = owned_private_file(
        &plan_path(&plan.profile.state_dir, &plan.operation_id)?,
        MAX_PLAN,
    )?;
    let stored: StandaloneInstallPlan =
        serde_json::from_slice(&plan_bytes).map_err(|e| e.to_string())?;
    if stored != *plan {
        return Err("operator consent approval differs from stored bytes".into());
    }
    let request_bytes = manual_request_bytes(request_path)?;
    let request: StandaloneManualRecoveryRequest =
        serde_json::from_slice(&request_bytes).map_err(|e| e.to_string())?;
    request.validate_for_plan(plan)?;
    Ok((
        format!("{:x}", Sha256::digest(&plan_bytes)),
        format!("{:x}", Sha256::digest(&request_bytes)),
    ))
}

pub fn read_operator_consent(
    plan: &StandaloneInstallPlan,
    request_path: &Path,
) -> Result<Option<StandaloneOperatorConsent>, String> {
    let (plan_digest, request_digest) = operator_consent_digests(plan, request_path)?;
    let consent_path = operator_consent_path(&plan.profile.state_dir, &plan.operation_id)?;
    if missing(&consent_path) {
        return Ok(None);
    }
    let consent: StandaloneOperatorConsent =
        serde_json::from_slice(&owned_private_file(&consent_path, MAX_RECORD)?)
            .map_err(|e| format!("invalid manual recovery operator consent: {e}"))?;
    consent.validate(plan, &plan_digest, &request_digest)?;
    Ok(Some(consent))
}

pub fn write_operator_consent(
    plan: &StandaloneInstallPlan,
    request_path: &Path,
) -> Result<PathBuf, String> {
    let (plan_digest, request_digest) = operator_consent_digests(plan, request_path)?;
    let consent = StandaloneOperatorConsent {
        version: 1,
        operation_id: plan.operation_id.clone(),
        plan_sha256: plan_digest,
        request_sha256: request_digest,
        method: "ssh_operator".into(),
    };
    consent.validate(plan, &consent.plan_sha256, &consent.request_sha256)?;
    let consent_path = operator_consent_path(&plan.profile.state_dir, &plan.operation_id)?;
    protocol::write_private_atomic_new(
        &consent_path,
        &serde_json::to_vec(&consent).map_err(|e| e.to_string())?,
        0o600,
    )?;
    Ok(consent_path)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandaloneInstallPlan {
    pub version: u32,
    #[serde(default, skip_serializing_if = "StandaloneInstallPurpose::is_upgrade")]
    pub purpose: StandaloneInstallPurpose,
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
    /// Source owner: Stop and its ACK always refer to this origin.
    pub fn origin(&self) -> &InstallOrigin {
        &self.original.origin
    }
    pub fn installed_origin(&self) -> &InstallOrigin {
        self.reservation_target().unwrap_or_else(|| self.origin())
    }
    pub fn reservation_target(&self) -> Option<&InstallOrigin> {
        match &self.purpose {
            StandaloneInstallPurpose::Upgrade => None,
            StandaloneInstallPurpose::PrivateRelocation { target, .. }
            | StandaloneInstallPurpose::ManualRecovery { target, .. } => Some(target),
        }
    }
    pub fn running(&self) -> Option<(&StandaloneOriginalInstance, &UpdateContext)> {
        match &self.source {
            InstallSource::Running { instance, context } => Some((instance, context)),
            InstallSource::Stopped { .. } => None,
        }
    }
    /// Structural historical validation only: never opens a retired source or
    /// compares an old relocation's stable version with this helper's version.
    pub fn validate(&self) -> Result<(), String> {
        protocol::validate_operation_id(&self.operation_id)?;
        if !matches!(self.version, 1 | 2)
            || self.version == 1 && !self.purpose.is_upgrade()
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
        if self.candidate_image.path != self.candidate_bundle.join("Contents/MacOS/desktop-pet") {
            return Err("invalid standalone candidate".into());
        }
        let (candidate, original) = if self.version == 1 {
            (
                semver::Version::parse(&self.candidate_version).map_err(|e| e.to_string())?,
                semver::Version::parse(&self.original.app_version).map_err(|e| e.to_string())?,
            )
        } else {
            (
                canonical_version(&self.candidate_version)?,
                canonical_version(&self.original.app_version)?,
            )
        };
        match &self.purpose {
            StandaloneInstallPurpose::Upgrade => {
                if if self.version == 1 {
                    candidate <= original
                } else {
                    candidate.cmp_precedence(&original) != std::cmp::Ordering::Greater
                } {
                    return Err("standalone upgrade must be strictly newer".into());
                }
                if self.version == 2
                    && self.origin()
                        != &(InstallOrigin::Standalone {
                            bundle: crate::installation::standalone_bundle()?,
                        })
                {
                    return Err(
                        "new standalone upgrades require the private canonical owner".into(),
                    );
                }
            }
            StandaloneInstallPurpose::PrivateRelocation {
                target,
                parent,
                source_closure_sha256,
                tern,
            } => {
                parent.validate()?;
                validate_sha256(source_closure_sha256)?;
                let [private, historical] = crate::installation::standalone_bundles()?;
                if !matches!(self.origin(), InstallOrigin::Standalone { bundle } if bundle == &historical)
                    || !matches!(target, InstallOrigin::Standalone { bundle } if bundle == &private)
                    || self.running().is_none()
                    || candidate != original
                    || !candidate.pre.is_empty()
                    || self.original.protocol != 4
                    || self.original.observation_mode != 2
                    || self.original.integration_api != 1
                    || self.original.image.sha256 != self.candidate_image.sha256
                    || parent.operation_id == self.operation_id
                    || parent.candidate_image != self.original.image
                    || parent.profile != self.profile
                    || parent.generation != self.original_stop_generation
                    || parent.source_closure_sha256 != *source_closure_sha256
                {
                    return Err(
                        "invalid private relocation source, target, or settled parent".into(),
                    );
                }
                for path in [&self.profile.config_dir, &self.profile.state_dir]
                    .into_iter()
                    .chain(
                        self.running()
                            .and_then(|(_, context)| context.assets_override.as_ref()),
                    )
                {
                    if path.starts_with(&historical) || path.starts_with(&private) {
                        return Err(
                            "relocation profile or assets reside inside an installation root"
                                .into(),
                        );
                    }
                }
                if let Some(tern) = tern {
                    ensure_absolute(&tern.configuration.path)?;
                    validate_sha256(&tern.configuration.sha256)?;
                    if tern.configuration.connection != tern.original
                        || tern.original.version != 1
                        || tern.original.executable != self.original.executable
                        || tern.original.config_dir != self.profile.config_dir
                        || tern.original.state_dir != self.profile.state_dir
                        || tern.target.version != 1
                        || tern.target.executable != private.join("Contents/MacOS/desktop-pet")
                        || tern.target.config_dir != tern.original.config_dir
                        || tern.target.state_dir != tern.original.state_dir
                    {
                        return Err(
                            "relocation Tern approval changes more than the executable".into()
                        );
                    }
                }
            }
            StandaloneInstallPurpose::ManualRecovery {
                target,
                release,
                previous_context,
                beta_confirmation,
                source_closure_sha256,
                tern,
            } => {
                validate_sha256(source_closure_sha256)?;
                release.validate_neutral()?;
                let [private, historical] = crate::installation::standalone_bundles()?;
                if !matches!(self.origin(), InstallOrigin::Standalone { bundle } if bundle == &historical)
                    || !matches!(target, InstallOrigin::Standalone { bundle } if bundle == &private)
                    || !matches!(self.source, InstallSource::Stopped { .. })
                    || !manual_recovery_release_allowed(release, beta_confirmation.as_deref())
                    || candidate.cmp_precedence(&original) != std::cmp::Ordering::Greater
                    || !crate::UpdatePolicy::StableAndBeta
                        .allows_version(&self.original.app_version)?
                    || release.version != self.candidate_version
                    || self.original.protocol != 4
                    || self.original.observation_mode != 2
                    || self.original.integration_api != 1
                {
                    return Err("invalid manual recovery source, target, or release".into());
                }
                validate_previous_context(previous_context, &self.original, &self.profile)?;
                validate_recovery_paths(&self.profile, previous_context, &historical, &private)?;
                validate_tern_approval(tern.as_ref(), &self.original, &self.profile, &private)?;
            }
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

    /// Fresh execution authority, deliberately separate from historical decoding.
    /// Historical reads must remain possible after H has been retired and after
    /// this product's compiled version advances.
    pub fn validate_for_execution(&self) -> Result<(), String> {
        self.validate()?;
        if self.version != 2 || crate::identity::LEGACY {
            return Err("only neutral v2 standalone approvals authorize new execution".into());
        }
        let (source_closure_sha256, manual_release) = match &self.purpose {
            StandaloneInstallPurpose::Upgrade => return Ok(()),
            StandaloneInstallPurpose::PrivateRelocation {
                source_closure_sha256,
                ..
            } => (source_closure_sha256, None),
            StandaloneInstallPurpose::ManualRecovery {
                source_closure_sha256,
                release,
                ..
            } => (source_closure_sha256, Some(release)),
        };
        if self.candidate_version != crate::identity::VERSION
            || (manual_release.is_none()
                && !canonical_version(&self.candidate_version)?.pre.is_empty())
        {
            return Err(
                "relocation requires the compiled stable version; recovery requires the compiled release".into(),
            );
        }
        let source_bundle = crate::physical_install_root(self.origin())?;
        let source_sidecar = crate::installation::sidecar_path(source_bundle);
        let candidate_sidecar = crate::installation::sidecar_path(&self.candidate_bundle);
        let source_bytes = crate::owned_file(&source_sidecar, 2 * 1024 * 1024)?;
        let candidate_bytes = crate::owned_file(&candidate_sidecar, 2 * 1024 * 1024)?;
        if manual_release.is_some() {
            validate_source_sidecar(&source_bytes, source_closure_sha256)?;
        } else {
            validate_relocation_sidecars(&source_bytes, &candidate_bytes, source_closure_sha256)?;
        }
        // Both signed complete deployment maps are independently checked. Manual
        // recovery deliberately permits different source and candidate closures.
        let source = crate::installation::validate_staged_app(source_bundle)?;
        let candidate = crate::installation::validate_staged_app(&self.candidate_bundle)?;
        validate_relocation_images(self, &source, &candidate)?;
        if let Some(release) = manual_release {
            crate::installation::validate_release_evidence(
                release,
                self.candidate_bundle
                    .parent()
                    .ok_or("recovery stage lacks parent")?,
                &self.candidate_bundle,
            )?;
        }
        validate_distinct_executable_inodes(&self.original.executable, &self.candidate_image.path)?;
        if crate::owned_file(&source_sidecar, 2 * 1024 * 1024)? != source_bytes
            || crate::owned_file(&candidate_sidecar, 2 * 1024 * 1024)? != candidate_bytes
        {
            return Err("relocation closure changed or stage reuses the source inode".into());
        }
        Ok(())
    }
}

fn canonical_version(value: &str) -> Result<semver::Version, String> {
    let version = semver::Version::parse(value).map_err(|e| e.to_string())?;
    if version.to_string() != value || !version.build.is_empty() {
        return Err("standalone version must be canonical without build metadata".into());
    }
    Ok(version)
}

fn validate_previous_context(
    context: &UpdateContext,
    original: &InstallationRecord,
    profile: &ProfilePaths,
) -> Result<(), String> {
    if context.executable != original.executable
        || context.version != original.app_version
        || context.running != original.image
        || context.running_origin.as_deref() != Some(&original.origin)
        || context.config_dir != profile.config_dir
        || context.state_dir != profile.state_dir
        || context.instance_id.is_empty()
        || context.instance_id.len() > 128
    {
        return Err("manual recovery previous context differs from the original owner".into());
    }
    context.observation_mode()?;
    Ok(())
}

fn validate_recovery_paths(
    profile: &ProfilePaths,
    context: &UpdateContext,
    historical: &Path,
    private: &Path,
) -> Result<(), String> {
    for path in [&profile.config_dir, &profile.state_dir]
        .into_iter()
        .chain(context.assets_override.as_ref())
    {
        ensure_absolute(path)?;
        if path.starts_with(historical) || path.starts_with(private) {
            return Err("recovery profile or assets reside inside an installation root".into());
        }
    }
    Ok(())
}

fn validate_tern_approval(
    tern: Option<&RelocationTernApproval>,
    original: &InstallationRecord,
    profile: &ProfilePaths,
    private: &Path,
) -> Result<(), String> {
    if let Some(tern) = tern {
        ensure_absolute(&tern.configuration.path)?;
        validate_sha256(&tern.configuration.sha256)?;
        if tern.configuration.connection != tern.original
            || tern.original.version != 1
            || tern.original.executable != original.executable
            || tern.original.config_dir != profile.config_dir
            || tern.original.state_dir != profile.state_dir
            || tern.target.version != 1
            || tern.target.executable != private.join("Contents/MacOS/desktop-pet")
            || tern.target.config_dir != tern.original.config_dir
            || tern.target.state_dir != tern.original.state_dir
        {
            return Err("recovery Tern approval changes more than the executable".into());
        }
    }
    Ok(())
}

fn validate_source_sidecar(source: &[u8], expected_sha256: &str) -> Result<(), String> {
    validate_sha256(expected_sha256)?;
    if format!("{:x}", Sha256::digest(source)) != expected_sha256 {
        return Err("source deployment sidecar differs from the approved complete closure".into());
    }
    Ok(())
}

fn validate_distinct_executable_inodes(source: &Path, candidate: &Path) -> Result<(), String> {
    let source = std::fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    let candidate = std::fs::symlink_metadata(candidate).map_err(|e| e.to_string())?;
    if !source.is_file()
        || !candidate.is_file()
        || (source.dev(), source.ino()) == (candidate.dev(), candidate.ino())
    {
        return Err("recovery stage must use a distinct regular executable inode".into());
    }
    Ok(())
}

fn validate_relocation_images(
    plan: &StandaloneInstallPlan,
    source: &crate::installation::ValidatedApp,
    candidate: &crate::installation::ValidatedApp,
) -> Result<(), String> {
    if source.image != plan.original.image
        || source.bundle != crate::physical_install_root(plan.origin())?
        || source.version != plan.original.app_version
        || candidate.image != plan.candidate_image
        || candidate.bundle != plan.candidate_bundle
        || candidate.version != plan.candidate_version
    {
        return Err("relocation requires the exact approved signed deployment closure".into());
    }
    Ok(())
}

fn validate_relocation_sidecars(
    source: &[u8],
    candidate: &[u8],
    expected_sha256: &str,
) -> Result<(), String> {
    if source != candidate || format!("{:x}", Sha256::digest(source)) != expected_sha256 {
        return Err(
            "relocation deployment sidecars differ from the approved complete closure".into(),
        );
    }
    Ok(())
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
            || matches!(
                plan.purpose,
                StandaloneInstallPurpose::ManualRecovery { .. }
            ) && matches!(
                self.phase,
                StandaloneInstallPhase::StopIntent
                    | StandaloneInstallPhase::StopAcknowledged
                    | StandaloneInstallPhase::Starting
            )
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
                || installation.origin != *plan.installed_origin()
                || installation.executable
                    != crate::physical_install_root(plan.installed_origin())?
                        .join("Contents/MacOS/desktop-pet")
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
pub fn relocation_commit_path(state: &Path, id: &str) -> Result<PathBuf, String> {
    path(state, id, "relocation-commit")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandaloneAdmissionPhase {
    Prepared,
    Presenting,
    Settled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandaloneAdmissionRecord {
    pub version: u32,
    pub operation_id: String,
    pub launch_sha256: String,
    pub original_stop_generation: u64,
    pub phase: StandaloneAdmissionPhase,
    pub result: Option<StandaloneInstallResult>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelocationOffer {
    pub version: u32,
    pub parent: SettledRelocationParent,
    pub operation_id: String,
}

pub fn admission_path(state: &Path, id: &str) -> Result<PathBuf, String> {
    path(state, id, "admission")
}
pub fn consent_path(state: &Path, id: &str) -> Result<PathBuf, String> {
    path(state, id, "consent")
}
pub fn relocation_offer_path(state: &Path, parent_id: &str) -> Result<PathBuf, String> {
    protocol::validate_operation_id(parent_id)?;
    ensure_absolute(state)?;
    Ok(state
        .join(crate::identity::UPDATES_DIRECTORY)
        .join(format!("relocation-offer-{parent_id}.json")))
}
pub fn read_admission(state: &Path, id: &str) -> Result<Option<StandaloneAdmissionRecord>, String> {
    check_read_directory(state)?;
    let path = admission_path(state, id)?;
    if missing(&path) {
        return Ok(None);
    }
    let record: StandaloneAdmissionRecord =
        serde_json::from_slice(&owned_private_file(&path, MAX_PLAN)?)
            .map_err(|e| format!("invalid standalone admission: {e}"))?;
    protocol::validate_operation_id(id)?;
    validate_sha256(&record.launch_sha256)?;
    if record.version != 1
        || record.operation_id != id
        || (record.phase == StandaloneAdmissionPhase::Settled) != record.result.is_some()
    {
        return Err("standalone admission scope or phase mismatch".into());
    }
    if let Some(result) = &record.result {
        result.validate()?;
        if !result.terminal() || result.operation_id != id || result.state_dir != state {
            return Err("standalone admission terminal result mismatch".into());
        }
    }
    Ok(Some(record))
}
pub fn write_admission(state: &Path, record: &StandaloneAdmissionRecord) -> Result<(), String> {
    let path = admission_path(state, &record.operation_id)?;
    protocol::validate_operation_id(&record.operation_id)?;
    validate_sha256(&record.launch_sha256)?;
    if record.version != 1
        || (record.phase == StandaloneAdmissionPhase::Settled) != record.result.is_some()
    {
        return Err("invalid admission version or terminal phase".into());
    }
    if let Some(result) = &record.result {
        result.validate()?;
        if !result.terminal()
            || result.operation_id != record.operation_id
            || result.state_dir != state
        {
            return Err("admission result scope differs".into());
        }
    }
    if let Some(previous) = read_admission(state, &record.operation_id)? {
        if previous.launch_sha256 != record.launch_sha256
            || previous.original_stop_generation != record.original_stop_generation
            || previous.phase == StandaloneAdmissionPhase::Settled && previous != *record
            || previous.phase == StandaloneAdmissionPhase::Presenting
                && record.phase == StandaloneAdmissionPhase::Prepared
        {
            return Err("standalone admission cannot replace its immutable authority".into());
        }
        protocol::write_private_bytes(
            &path,
            &serde_json::to_vec(record).map_err(|e| e.to_string())?,
            0o600,
        )
    } else {
        protocol::write_private_atomic_new(
            &path,
            &serde_json::to_vec(record).map_err(|e| e.to_string())?,
            0o600,
        )
    }
}
pub fn read_relocation_offer(
    state: &Path,
    parent_id: &str,
) -> Result<Option<RelocationOffer>, String> {
    check_read_directory(state)?;
    let path = relocation_offer_path(state, parent_id)?;
    if missing(&path) {
        return Ok(None);
    }
    let offer: RelocationOffer = serde_json::from_slice(&owned_private_file(&path, MAX_PLAN)?)
        .map_err(|e| format!("invalid relocation offer: {e}"))?;
    offer.parent.validate()?;
    protocol::validate_operation_id(&offer.operation_id)?;
    if offer.version != 1
        || offer.parent.operation_id != parent_id
        || offer.parent.profile.state_dir != state
    {
        return Err("relocation offer scope mismatch".into());
    }
    Ok(Some(offer))
}

pub fn relocation_attempt_path(state: &Path, id: &str) -> Result<PathBuf, String> {
    path(state, id, "relocation-attempt")
}
/// Immutable attempt provenance, not Stop or startup authority.
pub fn relocation_attempts(state: &Path, parent_id: &str) -> Result<Vec<String>, String> {
    protocol::validate_operation_id(parent_id)?;
    check_read_directory(state)?;
    let directory = state.join(crate::identity::UPDATES_DIRECTORY);
    if missing(&directory) {
        return Ok(Vec::new());
    }
    let mut attempts = Vec::new();
    let mut count = 0usize;
    for entry in std::fs::read_dir(&directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(id) = name
            .strip_prefix("install-")
            .and_then(|n| n.strip_suffix(".relocation-attempt.json"))
        else {
            continue;
        };
        count += 1;
        if count > 1024 {
            return Err("relocation attempt history exceeds bound".into());
        }
        protocol::validate_operation_id(id)?;
        let attempt: RelocationOffer =
            serde_json::from_slice(&owned_private_file(&entry.path(), MAX_PLAN)?)
                .map_err(|e| format!("invalid relocation attempt: {e}"))?;
        attempt.parent.validate()?;
        if attempt.version != 1
            || attempt.operation_id != id
            || attempt.parent.profile.state_dir != state
        {
            return Err("relocation attempt scope differs".into());
        }
        if attempt.parent.operation_id == parent_id {
            attempts.push(id.to_owned());
        }
    }
    attempts.sort();
    Ok(attempts)
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
    plan.validate_for_execution()?;
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
    let plan = read_plan(state, &record.operation_id)?;
    if plan.version != 2 {
        return Err("historical standalone approval is read-only".into());
    }
    record.validate(&plan)?;
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

#[cfg(all(test, not(desktop_pet_legacy)))]
pub(crate) mod tests {
    use super::*;
    use crate::installation::ValidatedApp;
    use std::os::unix::fs::PermissionsExt;

    fn upgrade(profile: ProfilePaths) -> StandaloneInstallPlan {
        let bundle = crate::installation::standalone_bundle().unwrap();
        let original_image = ExecutableIdentity {
            path: bundle.join("Contents/MacOS/desktop-pet"),
            sha256: "a".repeat(64),
        };
        let candidate_bundle = profile.state_dir.join("stage/DesktopPet.app");
        StandaloneInstallPlan {
            version: 2,
            purpose: StandaloneInstallPurpose::Upgrade,
            operation_id: "1".repeat(32),
            original: InstallationRecord {
                version: 1,
                executable: original_image.path.clone(),
                kind: InstallationKind::Standalone,
                app_version: "0.4.13-beta.7".into(),
                protocol: 4,
                observation_mode: 2,
                integration_api: 1,
                origin: InstallOrigin::Standalone { bundle },
                image: original_image,
            },
            candidate_image: ExecutableIdentity {
                path: candidate_bundle.join("Contents/MacOS/desktop-pet"),
                sha256: "b".repeat(64),
            },
            candidate_bundle,
            candidate_version: "0.4.13".into(),
            helper_image: ExecutableIdentity {
                path: profile.state_dir.join("helper"),
                sha256: "c".repeat(64),
            },
            source: InstallSource::Stopped {
                profile: profile.clone(),
            },
            profile,
            original_stop_generation: 7,
            control_token: "d".repeat(64),
        }
    }

    pub(crate) fn relocation_plan(profile: ProfilePaths) -> StandaloneInstallPlan {
        let mut plan = upgrade(profile);
        let historical = crate::installation::os_home()
            .unwrap()
            .join("Applications/DesktopPet.app");
        plan.original.origin = InstallOrigin::Standalone {
            bundle: historical.clone(),
        };
        plan.original.executable = historical.join("Contents/MacOS/desktop-pet");
        plan.original.image.path = plan.original.executable.clone();
        plan.original.app_version = plan.candidate_version.clone();
        plan.candidate_image.sha256 = plan.original.image.sha256.clone();
        let instance = StandaloneOriginalInstance {
            pid: std::process::id(),
            start_sec: 123,
            start_usec: 456,
            instance_id: "source-instance".into(),
            image: plan.original.image.clone(),
        };
        let context = UpdateContext {
            executable: plan.original.executable.clone(),
            version: plan.original.app_version.clone(),
            instance_id: instance.instance_id.clone(),
            config_dir: plan.profile.config_dir.clone(),
            state_dir: plan.profile.state_dir.clone(),
            running: plan.original.image.clone(),
            running_origin: Some(Box::new(plan.original.origin.clone())),
            assets_override: None,
            environment: Default::default(),
            locale: "ko".into(),
        };
        plan.source = InstallSource::Running { instance, context };
        let closure = format!("{:x}", Sha256::digest(b"complete signed sidecar"));
        plan.purpose = StandaloneInstallPurpose::PrivateRelocation {
            target: InstallOrigin::Standalone {
                bundle: crate::installation::standalone_bundle().unwrap(),
            },
            parent: SettledRelocationParent {
                version: 1,
                operation_id: "2".repeat(32),
                plan_sha256: "e".repeat(64),
                generation: plan.original_stop_generation,
                candidate_image: plan.original.image.clone(),
                source_closure_sha256: closure.clone(),
                profile: plan.profile.clone(),
                result_sha256: "f".repeat(64),
            },
            source_closure_sha256: closure,
            tern: None,
        };
        plan
    }

    fn profile() -> ProfilePaths {
        ProfilePaths {
            config_dir: PathBuf::from("/private/standalone-fixture/config"),
            state_dir: PathBuf::from("/private/standalone-fixture/state"),
        }
    }

    fn record(plan: &StandaloneInstallPlan, installed: bool) -> StandaloneInstallRecord {
        let installation = installed.then(|| {
            let mut installation = plan.original.clone();
            installation.origin = plan.installed_origin().clone();
            installation.executable = crate::physical_install_root(plan.installed_origin())
                .unwrap()
                .join("Contents/MacOS/desktop-pet");
            installation.image.path = installation.executable.clone();
            installation.image.sha256 = plan.candidate_image.sha256.clone();
            installation.app_version = plan.candidate_version.clone();
            installation
        });
        StandaloneInstallRecord {
            version: 1,
            operation_id: plan.operation_id.clone(),
            phase: if installed {
                StandaloneInstallPhase::Completed
            } else {
                StandaloneInstallPhase::Failed
            },
            result: StandaloneInstallResult {
                version: 1,
                operation_id: plan.operation_id.clone(),
                state_dir: plan.profile.state_dir.clone(),
                status: if installed {
                    InstallStatus::Applied
                } else {
                    InstallStatus::Cancelled
                },
                installation,
                installed,
                applied: installed,
                restart: if installed {
                    InstallRestart::Ready
                } else {
                    InstallRestart::NotRequested
                },
                retained_bundle: None,
                error: None,
            },
        }
    }

    fn ack(plan: &StandaloneInstallPlan) -> BootstrapStopAck {
        BootstrapStopAck {
            version: 1,
            operation_id: plan.operation_id.clone(),
            origin: plan.origin().clone(),
            profile: plan.profile.clone(),
            original: plan.running().unwrap().0.clone(),
            original_stop_generation: plan.original_stop_generation,
            control_token: plan.control_token.clone(),
        }
    }

    #[test]
    fn historical_v1_at_applications_reads_without_granting_mutation_authority() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        std::fs::create_dir(&state).unwrap();
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
        protocol::private_updates(&state).unwrap();
        let mut plan = relocation_plan(ProfilePaths {
            config_dir: state.clone(),
            state_dir: state.clone(),
        });
        plan.version = 1;
        plan.purpose = StandaloneInstallPurpose::Upgrade;
        plan.candidate_version = "0.4.14".into();
        plan.candidate_image.sha256 = "b".repeat(64);
        let bytes = serde_json::to_vec(&plan).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("purpose"));
        protocol::write_private_new(
            &plan_path(&state, &plan.operation_id).unwrap(),
            &bytes,
            0o600,
        )
        .unwrap();
        let saved_record = record(&plan, false);
        protocol::write_private_new(
            &record_path(&state, &plan.operation_id).unwrap(),
            &serde_json::to_vec(&saved_record).unwrap(),
            0o600,
        )
        .unwrap();
        let saved_ack = ack(&plan);
        protocol::write_private_new(
            &stop_ack_path(&state, &plan.operation_id).unwrap(),
            &serde_json::to_vec(&saved_ack).unwrap(),
            0o600,
        )
        .unwrap();
        assert_eq!(read_plan(&state, &plan.operation_id).unwrap(), plan);
        assert_eq!(
            read_record(&state, &plan.operation_id).unwrap(),
            Some(saved_record.clone())
        );
        assert_eq!(read_stop_ack(&plan).unwrap(), Some(saved_ack));
        assert!(plan.validate_for_execution().is_err());
        assert!(write_plan(&plan).is_err());
        assert!(write_record(&state, &saved_record).is_err());
        assert_eq!(
            crate::owned_private_file(&plan_path(&state, &plan.operation_id).unwrap(), MAX_PLAN)
                .unwrap(),
            bytes
        );
    }

    #[test]
    fn v1_cannot_smuggle_a_relocation_purpose_or_destination() {
        let mut plan = relocation_plan(profile());
        plan.version = 1;
        assert!(plan.validate().is_err());
        let mut wire = serde_json::to_value(upgrade(profile())).unwrap();
        wire["version"] = 1.into();
        wire["target"] = serde_json::to_value(plan.installed_origin()).unwrap();
        assert!(serde_json::from_value::<StandaloneInstallPlan>(wire).is_err());
    }

    #[test]
    fn v2_upgrade_remains_strictly_newer_and_canonical_same_owner() {
        let plan = upgrade(profile());
        plan.validate().unwrap();
        plan.validate_for_execution().unwrap();
        assert_eq!(plan.installed_origin(), plan.origin());
        assert!(plan.reservation_target().is_none());
        for version in ["0.4.13-beta.7", "0.4.12", "0.4.13+substitute"] {
            let mut wrong = plan.clone();
            wrong.candidate_version = version.into();
            assert!(wrong.validate().is_err(), "{version}");
        }
        let mut wrong = plan;
        wrong.original.origin = InstallOrigin::Standalone {
            bundle: crate::installation::os_home()
                .unwrap()
                .join("Applications/DesktopPet.app"),
        };
        wrong.original.executable = crate::physical_install_root(wrong.origin())
            .unwrap()
            .join("Contents/MacOS/desktop-pet");
        wrong.original.image.path = wrong.original.executable.clone();
        assert!(wrong.validate().is_err());
    }

    #[test]
    fn historical_relocation_ignores_compiled_version_and_does_not_open_retired_source() {
        let mut plan = relocation_plan(profile());
        // Deliberately different from any release helper built from this source.
        let historical = if crate::identity::VERSION == "0.4.12" {
            "0.4.11"
        } else {
            "0.4.12"
        };
        plan.original.app_version = historical.into();
        plan.candidate_version = historical.into();
        if let InstallSource::Running { context, .. } = &mut plan.source {
            context.version = historical.into();
        }
        plan.validate().unwrap();
        record(&plan, true).validate(&plan).unwrap();
        ack(&plan).validate(&plan).unwrap();
        assert!(plan.validate_for_execution().is_err());
        let bytes = serde_json::to_vec(&plan).unwrap();
        serde_json::from_slice::<StandaloneInstallPlan>(&bytes)
            .unwrap()
            .validate()
            .unwrap();
    }

    #[test]
    fn relocation_rejects_nonrunning_wrong_owner_version_image_or_parent() {
        let plan = relocation_plan(profile());
        plan.validate().unwrap();
        assert_eq!(plan.reservation_target(), Some(plan.installed_origin()));
        for mutation in 0..12 {
            let mut wrong = plan.clone();
            match mutation {
                0 => {
                    wrong.source = InstallSource::Stopped {
                        profile: wrong.profile.clone(),
                    }
                }
                1 => {
                    if let StandaloneInstallPurpose::PrivateRelocation { target, .. } =
                        &mut wrong.purpose
                    {
                        *target = wrong.original.origin.clone();
                    }
                }
                2 => wrong.candidate_version = "0.4.14".into(),
                3 => {
                    wrong.candidate_version = "0.4.13-beta.7".into();
                    wrong.original.app_version = wrong.candidate_version.clone();
                    if let InstallSource::Running { context, .. } = &mut wrong.source {
                        context.version = wrong.candidate_version.clone();
                    }
                }
                4 => wrong.candidate_image.sha256 = "b".repeat(64),
                5 => {
                    if let StandaloneInstallPurpose::PrivateRelocation { parent, .. } =
                        &mut wrong.purpose
                    {
                        parent.candidate_image.path = wrong.candidate_image.path.clone();
                    }
                }
                6 => {
                    if let StandaloneInstallPurpose::PrivateRelocation { parent, .. } =
                        &mut wrong.purpose
                    {
                        parent.generation += 1;
                    }
                }
                7 => {
                    if let StandaloneInstallPurpose::PrivateRelocation { parent, .. } =
                        &mut wrong.purpose
                    {
                        parent.profile.state_dir = PathBuf::from("/private/other-profile");
                    }
                }
                8 => {
                    if let StandaloneInstallPurpose::PrivateRelocation {
                        source_closure_sha256,
                        ..
                    } = &mut wrong.purpose
                    {
                        *source_closure_sha256 = "b".repeat(64);
                    }
                }
                9 => {
                    if let StandaloneInstallPurpose::PrivateRelocation { target, .. } =
                        &mut wrong.purpose
                    {
                        *target = InstallOrigin::Standalone {
                            bundle: PathBuf::from("/Applications/DesktopPet.app"),
                        };
                    }
                }
                10 => {
                    if let StandaloneInstallPurpose::PrivateRelocation { parent, .. } =
                        &mut wrong.purpose
                    {
                        parent.operation_id = wrong.operation_id.clone();
                    }
                }
                11 => wrong.original.origin = wrong.installed_origin().clone(),
                _ => unreachable!(),
            }
            assert!(wrong.validate().is_err(), "mutation {mutation}");
        }
    }

    #[test]
    fn relocation_rejects_profiles_and_assets_inside_either_root() {
        let plan = relocation_plan(profile());
        for root in [plan.origin(), plan.installed_origin()] {
            let nested = crate::physical_install_root(root)
                .unwrap()
                .join("user-data");
            for field in 0..3 {
                let mut wrong = plan.clone();
                if let InstallSource::Running { context, .. } = &mut wrong.source {
                    match field {
                        0 => {
                            wrong.profile.config_dir = nested.clone();
                            context.config_dir = nested.clone();
                        }
                        1 => {
                            wrong.profile.state_dir = nested.clone();
                            context.state_dir = nested.clone();
                        }
                        _ => context.assets_override = Some(nested.clone()),
                    }
                }
                if let StandaloneInstallPurpose::PrivateRelocation { parent, .. } =
                    &mut wrong.purpose
                {
                    parent.profile = wrong.profile.clone();
                }
                assert!(wrong.validate().is_err());
            }
        }
    }

    #[test]
    fn relocation_requires_entire_identical_sidecar_not_only_equal_executable_hashes() {
        let plan = relocation_plan(profile());
        let source = ValidatedApp {
            bundle: crate::physical_install_root(plan.origin())
                .unwrap()
                .to_path_buf(),
            image: plan.original.image.clone(),
            version: plan.original.app_version.clone(),
            installation_client_contract: Some(1),
        };
        let candidate = ValidatedApp {
            bundle: plan.candidate_bundle.clone(),
            image: plan.candidate_image.clone(),
            version: plan.candidate_version.clone(),
            installation_client_contract: Some(1),
        };
        let expected = format!("{:x}", Sha256::digest(b"complete signed sidecar"));
        validate_relocation_images(&plan, &source, &candidate).unwrap();
        validate_relocation_sidecars(
            b"complete signed sidecar",
            b"complete signed sidecar",
            &expected,
        )
        .unwrap();
        for (original, staged, digest) in [
            (
                b"complete signed sidecar".as_slice(),
                b"other resources, same executable".as_slice(),
                expected.as_str(),
            ),
            (
                b"substituted".as_slice(),
                b"substituted".as_slice(),
                expected.as_str(),
            ),
            (
                b"complete signed sidecar".as_slice(),
                b"complete signed sidecar".as_slice(),
                plan.original.image.sha256.as_str(),
            ),
        ] {
            assert!(validate_relocation_sidecars(original, staged, digest).is_err());
        }
        let mut wrong = candidate;
        wrong.image.sha256 = "b".repeat(64);
        assert!(validate_relocation_images(&plan, &source, &wrong).is_err());
    }

    #[test]
    fn structural_relocation_evidence_alone_cannot_publish_fresh_execution_approval() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        std::fs::create_dir(&state).unwrap();
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
        let plan = relocation_plan(ProfilePaths {
            config_dir: state.clone(),
            state_dir: state.clone(),
        });
        plan.validate().unwrap();
        // No signed source/stage closure has been qualified in this fixture.
        // Merely serializing equal executable digests cannot create approval.
        assert!(plan.validate_for_execution().is_err());
        assert!(write_plan(&plan).is_err());
        assert!(!plan_path(&state, &plan.operation_id).unwrap().exists());
    }

    #[test]
    fn tern_relocation_approval_preserves_saved_profile_and_only_changes_executable() {
        use crate::migration::{TernConfiguration, TernConnection};
        let mut plan = relocation_plan(profile());
        let original = TernConnection {
            version: 1,
            executable: plan.original.executable.clone(),
            config_dir: plan.profile.config_dir.clone(),
            state_dir: plan.profile.state_dir.clone(),
        };
        let target = TernConnection {
            executable: crate::physical_install_root(plan.installed_origin())
                .unwrap()
                .join("Contents/MacOS/desktop-pet"),
            ..original.clone()
        };
        let configuration: TernConfiguration = serde_json::from_value(serde_json::json!({
            "path": "/private/tern/kv.json",
            "sha256": "a".repeat(64),
            "connection": original,
            "device": 1,
            "inode": 2,
            "length": 3,
            "modified_seconds": 4,
            "modified_nanoseconds": 5,
            "bound_source": plan.original.image,
            "source_binding_sha256": null,
        }))
        .unwrap();
        if let StandaloneInstallPurpose::PrivateRelocation { tern, .. } = &mut plan.purpose {
            *tern = Some(RelocationTernApproval {
                configuration,
                original,
                target,
            });
        }
        plan.validate().unwrap();
        for field in 0..4 {
            let mut wrong = plan.clone();
            if let StandaloneInstallPurpose::PrivateRelocation {
                tern: Some(tern), ..
            } = &mut wrong.purpose
            {
                match field {
                    0 => tern.target.state_dir = PathBuf::from("/private/new-state"),
                    1 => tern.target.executable = tern.original.executable.clone(),
                    2 => tern.configuration.connection.executable = tern.target.executable.clone(),
                    3 => tern.original.config_dir = PathBuf::from("/private/new-config"),
                    _ => unreachable!(),
                }
            }
            assert!(wrong.validate().is_err());
        }
    }

    #[test]
    fn relocation_ack_is_source_bound_and_installed_record_is_target_bound() {
        let plan = relocation_plan(profile());
        let saved_ack = ack(&plan);
        saved_ack.validate(&plan).unwrap();
        let mut wrong_ack = saved_ack.clone();
        wrong_ack.origin = plan.installed_origin().clone();
        assert!(wrong_ack.validate(&plan).is_err());
        wrong_ack = saved_ack.clone();
        wrong_ack.original_stop_generation += 1;
        assert!(wrong_ack.validate(&plan).is_err());
        wrong_ack = saved_ack;
        wrong_ack.original.image = plan.candidate_image.clone();
        assert!(wrong_ack.validate(&plan).is_err());
        let saved_record = record(&plan, true);
        saved_record.validate(&plan).unwrap();
        let mut wrong = saved_record.clone();
        wrong.result.installation = Some(plan.original.clone());
        assert!(wrong.validate(&plan).is_err());
        let mut wrong = saved_record;
        let installation = wrong.result.installation.as_mut().unwrap();
        installation.executable = plan.original.executable.clone();
        installation.image.path = installation.executable.clone();
        assert!(wrong.validate(&plan).is_err());
    }

    #[test]
    fn relocation_commit_path_is_operation_scoped_and_rejects_arbitrary_paths() {
        let state = Path::new("/private/original-state");
        let id = "a".repeat(32);
        assert_eq!(
            relocation_commit_path(state, &id).unwrap(),
            state
                .join(crate::identity::UPDATES_DIRECTORY)
                .join(format!("install-{id}.relocation-commit.json")),
        );
        assert!(relocation_commit_path(state, "../foreign").is_err());
        assert!(relocation_commit_path(Path::new("relative"), &id).is_err());
    }

    fn manual_plan(profile: ProfilePaths) -> StandaloneInstallPlan {
        manual_plan_for_release(profile, "0.4.14")
    }

    fn manual_plan_for_release(profile: ProfilePaths, version: &str) -> StandaloneInstallPlan {
        let mut plan = relocation_plan(profile);
        let mut previous_context = plan.running().unwrap().1.clone();
        plan.original.app_version = "0.4.13-beta.7".into();
        previous_context.version = plan.original.app_version.clone();
        plan.candidate_version = version.into();
        plan.candidate_image.sha256 = "b".repeat(64);
        let asset_name = format!(
            "{}-v{}-macos-arm64.tar.gz",
            crate::identity::neutral::ARCHIVE_PREFIX,
            plan.candidate_version,
        );
        let tag = format!("v{}", plan.candidate_version);
        let beta = !canonical_version(version).unwrap().pre.is_empty();
        let release = ReleaseCandidate {
            version: plan.candidate_version.clone(),
            channel: if beta {
                ReleaseChannel::Beta
            } else {
                ReleaseChannel::Stable
            },
            tag: tag.clone(),
            release_id: 123,
            archive_url: format!(
                "{}{tag}/{asset_name}",
                crate::identity::neutral::RELEASE_DOWNLOAD,
            ),
            checksum_url: format!(
                "{}{tag}/{asset_name}.sha256",
                crate::identity::neutral::RELEASE_DOWNLOAD,
            ),
            asset_name,
            archive_size: 123,
            sha256: "e".repeat(64),
        };
        plan.purpose = StandaloneInstallPurpose::ManualRecovery {
            target: plan.installed_origin().clone(),
            release,
            previous_context,
            beta_confirmation: beta.then(|| MANUAL_BETA_RECOVERY_CONFIRMATION.into()),
            source_closure_sha256: format!("{:x}", Sha256::digest(b"original signed sidecar")),
            tern: None,
        };
        plan.source = InstallSource::Stopped {
            profile: plan.profile.clone(),
        };
        plan
    }

    fn manual_request(plan: &StandaloneInstallPlan) -> StandaloneManualRecoveryRequest {
        let StandaloneInstallPurpose::ManualRecovery {
            release,
            previous_context,
            beta_confirmation,
            ..
        } = &plan.purpose
        else {
            unreachable!()
        };
        StandaloneManualRecoveryRequest {
            version: if beta_confirmation.is_some() { 2 } else { 1 },
            context: previous_context.clone(),
            candidate_bundle: plan.candidate_bundle.clone(),
            release: release.clone(),
            confirmation: beta_confirmation
                .clone()
                .unwrap_or_else(|| "replace_original_app_preserve_profile".into()),
        }
    }

    #[test]
    fn beta_manual_recovery_requires_explicit_versioned_request_and_immutable_opt_in() {
        let plan = manual_plan_for_release(profile(), "0.4.14-beta.1");
        plan.validate().unwrap();
        let request = manual_request(&plan);
        request.validate_for_plan(&plan).unwrap();
        assert_eq!(request.version, 2);
        assert_eq!(request.confirmation, MANUAL_BETA_RECOVERY_CONFIRMATION);
        let saved = serde_json::to_vec(&plan).unwrap();
        let historical: StandaloneInstallPlan = serde_json::from_slice(&saved).unwrap();
        assert_eq!(historical, plan);
        historical.validate().unwrap();
        for change in 0..9 {
            let mut wrong = request.clone();
            match change {
                0 => wrong.version = 1,
                1 => wrong.version = 3,
                2 => wrong.confirmation = "replace_original_app_preserve_profile".into(),
                3 => wrong.confirmation.push('!'),
                4 => wrong.context.version = wrong.release.version.clone(),
                5 => wrong.context.version = "0.4.14-beta.2".into(),
                6 => wrong.context.state_dir = "/private/foreign-state".into(),
                7 => wrong.release.archive_url = "https://untrusted.invalid/release".into(),
                8 => wrong.release.channel = ReleaseChannel::Stable,
                _ => unreachable!(),
            }
            if matches!(change, 4 | 5) {
                assert!(
                    wrong.validate().is_err(),
                    "accepted non-newer beta request {change}"
                );
            }
            assert!(
                wrong.validate_for_plan(&plan).is_err(),
                "accepted request {change}"
            );
        }
        for marker in [None, Some("forged".into())] {
            let mut wrong = plan.clone();
            let StandaloneInstallPurpose::ManualRecovery {
                beta_confirmation, ..
            } = &mut wrong.purpose
            else {
                unreachable!()
            };
            *beta_confirmation = marker;
            assert!(wrong.validate().is_err());
        }
        let stable = manual_plan(profile());
        let mut stable_as_v2 = manual_request(&stable);
        stable_as_v2.version = 2;
        stable_as_v2.confirmation = MANUAL_BETA_RECOVERY_CONFIRMATION.into();
        assert!(stable_as_v2.validate().is_err());
        let mut stable_with_beta_marker = stable.clone();
        let StandaloneInstallPurpose::ManualRecovery {
            beta_confirmation, ..
        } = &mut stable_with_beta_marker.purpose
        else {
            unreachable!()
        };
        *beta_confirmation = Some(MANUAL_BETA_RECOVERY_CONFIRMATION.into());
        assert!(stable_with_beta_marker.validate().is_err());
        for version in ["0.4.13-beta.6", "0.4.13-beta.7"] {
            assert!(manual_plan_for_release(profile(), version)
                .validate()
                .is_err());
        }
        let encoded = serde_json::to_value(&stable).unwrap();
        assert!(encoded["purpose"].get("beta_confirmation").is_none());
        let decoded: StandaloneInstallPlan = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, stable);
        manual_request(&decoded)
            .validate_for_plan(&decoded)
            .unwrap();
        assert_ne!(plan.candidate_version, crate::identity::VERSION);
        assert!(plan
            .validate_for_execution()
            .unwrap_err()
            .contains("compiled"));
    }

    #[test]
    fn manual_recovery_is_stopped_h_to_c_without_parent_or_restart_authority() {
        let plan = manual_plan(profile());
        plan.validate().unwrap();
        assert!(plan.running().is_none());
        assert_ne!(plan.origin(), plan.installed_origin());
        assert_eq!(plan.reservation_target(), Some(plan.installed_origin()));
        assert_ne!(plan.original.image.sha256, plan.candidate_image.sha256);
        assert!(ack(&relocation_plan(profile())).validate(&plan).is_err());
        let mut stopped = record(&plan, true);
        assert!(stopped.validate(&plan).is_err());
        stopped.result.status = InstallStatus::InstalledStopped;
        stopped.result.applied = false;
        stopped.result.restart = InstallRestart::NotRequested;
        stopped.validate(&plan).unwrap();
        for phase in [
            StandaloneInstallPhase::StopIntent,
            StandaloneInstallPhase::StopAcknowledged,
            StandaloneInstallPhase::Starting,
        ] {
            stopped.phase = phase;
            assert!(stopped.validate(&plan).is_err());
        }
        let mut later_generation = plan.clone();
        later_generation.original_stop_generation += 1;
        // Fresh generation/quiescence belongs to the runner, not historical context.
        later_generation.validate().unwrap();
        let mut historical_v1 = plan.clone();
        historical_v1.version = 1;
        assert!(historical_v1.validate().is_err());
        let mut normal_upgrade = plan;
        normal_upgrade.purpose = StandaloneInstallPurpose::Upgrade;
        assert!(
            normal_upgrade.validate().is_err(),
            "H cannot be a perpetual update owner"
        );
    }

    #[test]
    fn manual_historical_validation_rejects_forged_owner_context_and_versions() {
        let plan = manual_plan(profile());
        for field in 0..17 {
            let mut wrong = plan.clone();
            match field {
                0 => wrong.source = relocation_plan(profile()).source,
                1 => wrong.original.origin = wrong.installed_origin().clone(),
                2 => wrong.original.app_version = wrong.candidate_version.clone(),
                3 => wrong.candidate_version = "0.4.12".into(),
                4 => wrong.original.app_version = "0.4.13-rc.1".into(),
                5 => wrong.original.app_version = "0.4.13-beta.7+local".into(),
                6 => wrong.candidate_version = "0.4.14-beta.1".into(),
                _ => {
                    let StandaloneInstallPurpose::ManualRecovery {
                        target,
                        previous_context,
                        release,
                        source_closure_sha256,
                        ..
                    } = &mut wrong.purpose
                    else {
                        unreachable!()
                    };
                    match field {
                        7 => *target = wrong.original.origin.clone(),
                        8 => previous_context.running.sha256 = "f".repeat(64),
                        9 => previous_context.version = "0.4.12".into(),
                        10 => previous_context.running_origin = None,
                        11 => previous_context.config_dir = "/private/other-profile".into(),
                        12 => previous_context.instance_id.clear(),
                        13 => previous_context.executable = wrong.candidate_image.path.clone(),
                        14 => release.archive_url = "https://untrusted.invalid/archive".into(),
                        15 => release.version = "0.4.15".into(),
                        16 => *source_closure_sha256 = "not-a-complete-closure-digest".into(),
                        _ => unreachable!(),
                    }
                }
            }
            assert!(wrong.validate().is_err(), "accepted forged field {field}");
        }
        // Neither the removed H nor this helper's compiled version is consulted.
        let mut past = plan;
        past.original.app_version = "0.4.12".into();
        if let StandaloneInstallPurpose::ManualRecovery {
            previous_context, ..
        } = &mut past.purpose
        {
            previous_context.version = past.original.app_version.clone();
        }
        past.validate().unwrap();
        assert!(
            past.validate_for_execution().is_err(),
            "unqualified closures cannot execute"
        );
    }

    #[test]
    fn manual_profile_and_assets_must_remain_outside_both_installation_roots() {
        let plan = manual_plan(profile());
        let roots = crate::installation::standalone_bundles().unwrap();
        for root in roots {
            for field in 0..3 {
                let mut wrong = plan.clone();
                let StandaloneInstallPurpose::ManualRecovery {
                    previous_context, ..
                } = &mut wrong.purpose
                else {
                    unreachable!()
                };
                match field {
                    0 => {
                        wrong.profile.config_dir = root.join("config");
                        previous_context.config_dir = wrong.profile.config_dir.clone();
                    }
                    1 => {
                        wrong.profile.state_dir = root.join("state");
                        previous_context.state_dir = wrong.profile.state_dir.clone();
                    }
                    2 => previous_context.assets_override = Some(root.join("assets")),
                    _ => unreachable!(),
                }
                wrong.source = InstallSource::Stopped {
                    profile: wrong.profile.clone(),
                };
                assert!(wrong.validate().is_err());
            }
        }
    }

    #[test]
    fn manual_source_and_candidate_closures_are_independently_bound() {
        let plan = manual_plan(profile());
        let source = ValidatedApp {
            bundle: crate::physical_install_root(plan.origin())
                .unwrap()
                .to_path_buf(),
            image: plan.original.image.clone(),
            version: plan.original.app_version.clone(),
            installation_client_contract: Some(1),
        };
        let candidate = ValidatedApp {
            bundle: plan.candidate_bundle.clone(),
            image: plan.candidate_image.clone(),
            version: plan.candidate_version.clone(),
            installation_client_contract: Some(1),
        };
        validate_relocation_images(&plan, &source, &candidate).unwrap();
        let StandaloneInstallPurpose::ManualRecovery {
            source_closure_sha256,
            ..
        } = &plan.purpose
        else {
            unreachable!()
        };
        validate_source_sidecar(b"original signed sidecar", source_closure_sha256).unwrap();
        assert!(
            validate_source_sidecar(b"candidate signed sidecar", source_closure_sha256).is_err()
        );
        assert!(
            validate_relocation_sidecars(
                b"original signed sidecar",
                b"candidate signed sidecar",
                source_closure_sha256,
            )
            .is_err(),
            "same-byte relocation must remain stricter than recovery"
        );
        let mut changed = candidate;
        changed.version = "0.4.15".into();
        assert!(validate_relocation_images(&plan, &source, &changed).is_err());
        assert!(plan.validate_for_execution().is_err());
    }

    #[test]
    fn manual_tern_approval_only_changes_the_saved_executable() {
        use crate::migration::{TernConfiguration, TernConnection};
        let mut plan = manual_plan(profile());
        let original = TernConnection {
            version: 1,
            executable: plan.original.executable.clone(),
            config_dir: plan.profile.config_dir.clone(),
            state_dir: plan.profile.state_dir.clone(),
        };
        let target = TernConnection {
            executable: crate::physical_install_root(plan.installed_origin())
                .unwrap()
                .join("Contents/MacOS/desktop-pet"),
            ..original.clone()
        };
        let configuration: TernConfiguration = serde_json::from_value(serde_json::json!({
            "path": "/private/tern/kv.json", "sha256": "a".repeat(64),
            "connection": original, "device": 1, "inode": 2, "length": 3,
            "modified_seconds": 4, "modified_nanoseconds": 5,
            "bound_source": plan.original.image, "source_binding_sha256": null,
        }))
        .unwrap();
        if let StandaloneInstallPurpose::ManualRecovery { tern, .. } = &mut plan.purpose {
            *tern = Some(RelocationTernApproval {
                configuration,
                original,
                target,
            });
        }
        plan.validate().unwrap();
        for field in 0..4 {
            let mut wrong = plan.clone();
            if let StandaloneInstallPurpose::ManualRecovery {
                tern: Some(tern), ..
            } = &mut wrong.purpose
            {
                match field {
                    0 => tern.target.state_dir = "/private/other-state".into(),
                    1 => tern.target.executable = tern.original.executable.clone(),
                    2 => tern.configuration.connection.executable = tern.target.executable.clone(),
                    3 => tern.original.config_dir = "/private/other-config".into(),
                    _ => unreachable!(),
                }
            }
            assert!(wrong.validate().is_err());
        }
    }

    #[test]
    fn operator_consent_is_immutable_and_hashes_actual_private_request_and_plan_bytes() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let plan = manual_plan(ProfilePaths {
            config_dir: temp.path().join("config"),
            state_dir: temp.path().to_path_buf(),
        });
        protocol::private_updates(temp.path()).unwrap();
        let plan_bytes = serde_json::to_vec_pretty(&plan).unwrap();
        // Historical fixture publication: not fresh execution authority.
        protocol::write_private_atomic_new(
            &plan_path(temp.path(), &plan.operation_id).unwrap(),
            &plan_bytes,
            0o600,
        )
        .unwrap();
        let request = manual_request(&plan);
        let request_path = temp.path().join("manual-request.json");
        let request_bytes = serde_json::to_vec_pretty(&request).unwrap();
        protocol::write_private_atomic_new(&request_path, &request_bytes, 0o600).unwrap();
        assert_eq!(
            read_manual_recovery_request(&request_path).unwrap(),
            request
        );
        assert!(read_operator_consent(&plan, &request_path)
            .unwrap()
            .is_none());
        let consent_path = write_operator_consent(&plan, &request_path).unwrap();
        assert_eq!(
            consent_path,
            operator_consent_path(temp.path(), &plan.operation_id).unwrap()
        );
        let consent = read_operator_consent(&plan, &request_path)
            .unwrap()
            .unwrap();
        assert_eq!(
            consent.plan_sha256,
            format!("{:x}", Sha256::digest(&plan_bytes))
        );
        assert_eq!(
            consent.request_sha256,
            format!("{:x}", Sha256::digest(&request_bytes))
        );
        assert_ne!(
            consent.plan_sha256,
            format!("{:x}", Sha256::digest(serde_json::to_vec(&plan).unwrap()))
        );
        assert!(write_operator_consent(&plan, &request_path).is_err());
        let saved = owned_private_file(&consent_path, MAX_RECORD).unwrap();
        for field in 0..5 {
            let mut wrong = consent.clone();
            match field {
                0 => wrong.method = "native_panel".into(),
                1 => wrong.operation_id = "f".repeat(32),
                2 => wrong.plan_sha256 = "f".repeat(64),
                3 => wrong.request_sha256 = "f".repeat(64),
                4 => wrong.version = 2,
                _ => unreachable!(),
            }
            assert!(wrong
                .validate(&plan, &consent.plan_sha256, &consent.request_sha256)
                .is_err());
        }
        protocol::write_private_bytes(&request_path, &serde_json::to_vec(&request).unwrap(), 0o600)
            .unwrap();
        assert!(
            read_operator_consent(&plan, &request_path).is_err(),
            "semantically equal but changed request bytes must invalidate consent"
        );
        assert_eq!(
            owned_private_file(&consent_path, MAX_RECORD).unwrap(),
            saved
        );
        assert!(operator_consent_path(temp.path(), "../foreign").is_err());
    }

    #[test]
    fn manual_request_reader_rejects_unknown_fields_public_symlink_and_oversized_input() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let request = manual_request(&manual_plan(profile()));
        let path = temp.path().join("request.json");
        let bytes = serde_json::to_vec(&request).unwrap();
        protocol::write_private_atomic_new(&path, &bytes, 0o600).unwrap();
        read_manual_recovery_request(&path).unwrap();
        let alias = temp.path().join("alias.json");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert!(read_manual_recovery_request(&alias).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_manual_recovery_request(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut forged = serde_json::to_value(&request).unwrap();
        forged["target"] = serde_json::json!("/arbitrary/DesktopPet.app");
        protocol::write_private_bytes(&path, &serde_json::to_vec(&forged).unwrap(), 0o600).unwrap();
        assert!(read_manual_recovery_request(&path).is_err());
        protocol::write_private_bytes(&path, &vec![b' '; MAX_PLAN as usize + 1], 0o600).unwrap();
        assert!(read_manual_recovery_request(&path).is_err());
        let mut wrong = request;
        wrong.confirmation = "native_approval".into();
        assert!(wrong.validate().is_err());
        wrong.confirmation = "replace_original_app_preserve_profile".into();
        wrong.release.version = "0.4.13-beta.7".into();
        assert!(wrong.validate().is_err());
    }

    #[test]
    fn recovery_stage_rejects_same_inode_even_with_matching_executable_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let candidate = temp.path().join("candidate");
        let hardlink = temp.path().join("hardlink");
        let symlink = temp.path().join("symlink");
        std::fs::write(&source, b"identical executable bytes").unwrap();
        std::fs::copy(&source, &candidate).unwrap();
        std::fs::hard_link(&source, &hardlink).unwrap();
        std::os::unix::fs::symlink(&source, &symlink).unwrap();
        validate_distinct_executable_inodes(&source, &candidate).unwrap();
        assert!(validate_distinct_executable_inodes(&source, &source).is_err());
        assert!(validate_distinct_executable_inodes(&source, &hardlink).is_err());
        assert!(validate_distinct_executable_inodes(&source, &symlink).is_err());
    }

    #[test]
    fn manual_operator_request_path_is_fixed_to_the_operation_resource_directory() {
        let state = Path::new("/private/original-state");
        let id = "a".repeat(32);
        assert_eq!(
            manual_operator_request_path(state, &id).unwrap(),
            state
                .join(crate::identity::UPDATES_DIRECTORY)
                .join(format!("install-{id}.resources/operator-request.json")),
        );
        assert!(manual_operator_request_path(state, "../foreign").is_err());
        assert!(manual_operator_request_path(Path::new("relative"), &id).is_err());
    }
}
