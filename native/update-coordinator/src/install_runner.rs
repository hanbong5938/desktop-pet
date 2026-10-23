//! Detached Pet-only installer. Shell lifetime is never installation authority.
//!
//! `install-manual-recovery --request ABS` records explicit SSH operator approval
//! for an already-stopped historical owner and preserves the original profile.
//! Its immutable resources live at `updates-v4/install-{id}.resources`; the
//! operator request is fixed to `operator-request.json` there. Completion means
//! InstalledStopped/NotRequested, never an inferred restart or native consent.
//! A separate normal start is permitted only after exact reservation release.
use crate::helper_ui::Panel;
use desktop_pet_update_coordinator::{
    self as coordinator,
    installation::{self, InstallRequest, InstallationKind, InstallationRecord, ValidatedApp},
    native_runtime::{self, NativeInstallScope, NativeInstance, ProfilePaths},
    protocol,
    standalone_install::{
        self as journal, InstallRestart, InstallSource, InstallStatus,
        StandaloneInstallPhase as Phase, StandaloneInstallPlan as Plan, StandaloneInstallPurpose,
        StandaloneInstallRecord as Record, StandaloneInstallResult as Outcome,
        StandaloneOriginalInstance,
    },
    ExecutableIdentity, InstallOrigin, VerifiedInstallation,
};
use journal::{StandaloneAdmissionPhase as AdmissionPhase, StandaloneAdmissionRecord as Admission};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Launch {
    version: u32,
    operation_id: String,
    original: InstallationRecord,
    profile: ProfilePaths,
    helper: ExecutableIdentity,
    request: InstallRequest,
    original_evidence: OriginalEvidence,
    #[serde(default)]
    purpose: StandaloneInstallPurpose,
    #[serde(default)]
    locale: String,
    source: InstallSource,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct OriginalEvidence {
    sidecar: String,
    locator: Option<String>,
    image_device: u64,
    image_inode: u64,
    bundle_device: u64,
    bundle_inode: u64,
}
fn metadata_bytes(path: &Path, optional: bool) -> Result<Option<String>, String> {
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if optional && error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.nlink() != 1
        || meta.permissions().mode() & 0o022 != 0
        || meta.len() > 65536
    {
        return Err("unsafe original installation metadata".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 65536 {
        return Err("original metadata exceeded bound".into());
    }
    Ok(Some(
        String::from_utf8(bytes).map_err(|_| "non-UTF8 original metadata")?,
    ))
}
fn original_evidence(original: &InstallationRecord) -> Result<OriginalEvidence, String> {
    let mut sidecar = coordinator::physical_install_root(&original.origin)?
        .as_os_str()
        .to_os_string();
    sidecar.push(".rig-native.json");
    let image = fs::symlink_metadata(&original.executable).map_err(|e| e.to_string())?;
    let bundle = fs::symlink_metadata(coordinator::physical_install_root(&original.origin)?)
        .map_err(|e| e.to_string())?;
    if !image.is_file()
        || image.file_type().is_symlink()
        || !bundle.is_dir()
        || bundle.file_type().is_symlink()
    {
        return Err("original bundle/image changed while capturing evidence".into());
    }
    Ok(OriginalEvidence {
        sidecar: metadata_bytes(Path::new(&sidecar), false)?
            .ok_or("original sidecar is missing")?,
        locator: metadata_bytes(&installation::locator_path()?, true)?,
        image_device: image.dev(),
        image_inode: image.ino(),
        bundle_device: bundle.dev(),
        bundle_inode: bundle.ino(),
    })
}
fn unchanged_original(plan: &Plan) -> Result<(), String> {
    let launch = read_launch(&plan.profile.state_dir, &plan.operation_id)?;
    if launch.original != plan.original
        || original_evidence(&plan.original)? != launch.original_evidence
        || installation::inspect(&plan.original.executable)? != plan.original
    {
        return Err("original bundle, sidecar or locator differs from pre-Stop evidence".into());
    }
    Ok(())
}

fn random_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn operation_dir(state: &Path, id: &str) -> Result<PathBuf, String> {
    protocol::validate_operation_id(id)?;
    if !state.is_absolute() {
        return Err("install state directory must be absolute".into());
    }
    Ok(state
        .join(coordinator::identity::UPDATES_DIRECTORY)
        .join(format!("install-{id}.resources")))
}
fn persist_new<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    protocol::write_private_new(
        path,
        &serde_json::to_vec(value).map_err(|e| e.to_string())?,
        0o600,
    )
}
fn read_launch(state: &Path, id: &str) -> Result<Launch, String> {
    let root = operation_dir(state, id)?;
    let launch: Launch = serde_json::from_slice(&coordinator::owned_private_file(
        &root.join("launch.json"),
        196608,
    )?)
    .map_err(|e| e.to_string())?;
    launch.profile.validate()?;
    if launch.version != 1
        || launch.operation_id != id
        || launch.profile.state_dir != state
        || launch.helper.path != root.join("helper")
        || launch.request.staged_app != root.join("DesktopPet.app")
        || launch.request.version != 1
        || launch.request.kind != InstallationKind::Standalone
        || launch.request.source_root.is_some()
    {
        return Err("durable install launch scope mismatch".into());
    }
    match &launch.source {
        InstallSource::Running { instance, context } => {
            if instance.image != launch.original.image
                || context.running != instance.image
                || context.instance_id != instance.instance_id
                || context.executable != launch.original.executable
                || context.config_dir != launch.profile.config_dir
                || context.state_dir != launch.profile.state_dir
            {
                return Err("durable launch source instance/profile differs".into());
            }
        }
        InstallSource::Stopped { profile } if profile != &launch.profile => {
            return Err("durable stopped launch profile differs".into());
        }
        InstallSource::Stopped { .. } => {}
    }
    if let StandaloneInstallPurpose::ManualRecovery {
        target,
        release,
        previous_context,
        beta_confirmation,
        source_closure_sha256,
        ..
    } = &launch.purpose
    {
        let request = journal::read_manual_recovery_request(
            &journal::manual_operator_request_path(state, id)?,
        )?;
        if request.context != *previous_context
            || request.release != *release
            || request.candidate_bundle != launch.request.staged_app
            || beta_confirmation.as_deref()
                != (request.version == 2).then_some(request.confirmation.as_str())
            || previous_context.running != launch.original.image
            || previous_context.executable != launch.original.executable
            || previous_context.version != launch.original.app_version
            || previous_context.config_dir != launch.profile.config_dir
            || previous_context.state_dir != launch.profile.state_dir
            || previous_context.running_origin.as_deref() != Some(&launch.original.origin)
            || !matches!(launch.source, InstallSource::Stopped { .. })
            || *target
                != (InstallOrigin::Standalone {
                    bundle: installation::standalone_bundle()?,
                })
            || launch.original.kind != InstallationKind::Standalone
            || launch.original.protocol != 4
            || launch.original.observation_mode != 2
            || launch.original.integration_api != 1
            || digest(launch.original_evidence.sidecar.as_bytes()) != *source_closure_sha256
        {
            return Err("manual launch differs from immutable operator request/source".into());
        }
        installation::validate_release_evidence(release, &root, &request.candidate_bundle)?;
    }
    Ok(launch)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn missing(path: &Path) -> bool {
    fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
}
fn launch_digest(state: &Path, id: &str) -> Result<String, String> {
    Ok(digest(&coordinator::owned_private_file(
        &operation_dir(state, id)?.join("launch.json"),
        196608,
    )?))
}
fn prepare_admission(state: &Path, id: &str) -> Result<(), String> {
    journal::write_admission(
        state,
        &Admission {
            version: 1,
            operation_id: id.into(),
            launch_sha256: launch_digest(state, id)?,
            original_stop_generation: protocol::stop_generation(state)?,
            phase: AdmissionPhase::Prepared,
            result: None,
        },
    )
}
fn admission_for_launch(state: &Path, id: &str) -> Result<Admission, String> {
    let admission = journal::read_admission(state, id)?.ok_or("install admission is missing")?;
    if admission.launch_sha256 != launch_digest(state, id)? {
        return Err("install launch differs from durable admission".into());
    }
    Ok(admission)
}
fn validate_plan_launch(plan: &Plan, launch: &Launch, admission: &Admission) -> Result<(), String> {
    if plan.operation_id != launch.operation_id
        || plan.original != launch.original
        || plan.profile != launch.profile
        || plan.source != launch.source
        || plan.purpose != launch.purpose
        || plan.helper_image != launch.helper
        || plan.candidate_bundle != launch.request.staged_app
        || plan.original_stop_generation != admission.original_stop_generation
    {
        return Err("immutable plan differs from admitted launch/source/purpose".into());
    }
    Ok(())
}
fn settle_admission(plan: &Plan, result: &Outcome) -> Result<(), String> {
    settle_launch(
        &plan.profile.state_dir,
        &plan.operation_id,
        plan.origin(),
        plan.reservation_target(),
        result,
    )
}
fn settle_launch(
    state: &Path,
    id: &str,
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    result: &Outcome,
) -> Result<(), String> {
    if !result.terminal() || protocol::reservation_exists(source, target, state, id)? {
        return Err(
            "admission cannot settle before durable terminal outcome and reservation release"
                .into(),
        );
    }
    let durable = if !missing(&journal::plan_path(state, id)?) {
        journal::read_record(state, id)?
            .ok_or("terminal result has not been published")?
            .result
    } else {
        serde_json::from_slice::<Outcome>(&coordinator::owned_private_file(
            &operation_dir(state, id)?.join("interrupted-result.json"),
            32768,
        )?)
        .map_err(|e| format!("invalid interrupted admission result: {e}"))?
    };
    if durable != *result {
        return Err("settlement differs from durable terminal result".into());
    }
    let mut admission = admission_for_launch(state, id)?;
    admission.phase = AdmissionPhase::Settled;
    admission.result = Some(result.clone());
    journal::write_admission(state, &admission)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Consent {
    version: u32,
    operation_id: String,
    plan_sha256: String,
}
fn plan_digest(plan: &Plan) -> Result<String, String> {
    let bytes = coordinator::owned_private_file(
        &journal::plan_path(&plan.profile.state_dir, &plan.operation_id)?,
        65536,
    )?;
    let durable: Plan = serde_json::from_slice(&bytes)
        .map_err(|e| format!("invalid immutable consent plan: {e}"))?;
    if durable != *plan {
        return Err("visible approval differs from durable plan bytes".into());
    }
    Ok(digest(&bytes))
}
fn verify_consent(plan: &Plan) -> Result<(), String> {
    if plan.purpose.is_upgrade() {
        return Ok(());
    }
    if matches!(
        plan.purpose,
        StandaloneInstallPurpose::ManualRecovery { .. }
    ) {
        return Err("SSH operator recovery cannot use native-panel consent".into());
    }
    let consent: Consent = serde_json::from_slice(&coordinator::owned_private_file(
        &journal::consent_path(&plan.profile.state_dir, &plan.operation_id)?,
        16384,
    )?)
    .map_err(|e| format!("invalid relocation consent: {e}"))?;
    if consent.version != 1
        || consent.operation_id != plan.operation_id
        || consent.plan_sha256 != plan_digest(plan)?
    {
        return Err("relocation consent differs from immutable approval".into());
    }
    Ok(())
}
fn save_consent(plan: &Plan) -> Result<(), String> {
    if plan.purpose.is_upgrade() {
        return Ok(());
    }
    if matches!(
        plan.purpose,
        StandaloneInstallPurpose::ManualRecovery { .. }
    ) {
        return Err("SSH operator recovery cannot create native-panel consent".into());
    }
    protocol::write_private_atomic_new(
        &journal::consent_path(&plan.profile.state_dir, &plan.operation_id)?,
        &serde_json::to_vec(&Consent {
            version: 1,
            operation_id: plan.operation_id.clone(),
            plan_sha256: plan_digest(plan)?,
        })
        .map_err(|e| e.to_string())?,
        0o600,
    )
}

fn approved_source(profile: &ProfilePaths, instance: Option<&NativeInstance>) -> InstallSource {
    match instance {
        Some(instance) => InstallSource::Running {
            instance: StandaloneOriginalInstance {
                pid: instance.incarnation.pid,
                start_sec: instance.incarnation.start_sec,
                start_usec: instance.incarnation.start_usec,
                instance_id: instance.instance_id.clone(),
                image: instance.image.clone(),
            },
            context: instance.context.clone(),
        },
        None => InstallSource::Stopped {
            profile: profile.clone(),
        },
    }
}

fn select_source(
    original: &InstallationRecord,
    app: &ValidatedApp,
) -> Result<Option<(ProfilePaths, Option<NativeInstance>)>, String> {
    let profiles = protocol::active_profiles_for_roots(&[coordinator::physical_install_root(
        &original.origin,
    )?
    .to_path_buf()])?;
    if profiles.len() > 1 {
        return Err(format!(
            "multiple active native profiles share this installation; no app was stopped: {}",
            profiles
                .iter()
                .map(|p| p.state_dir.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(profile) = profiles.first() {
        // The registry is discovery only. The direct socket and kernel authenticate authority.
        let status: serde_json::Value = native_runtime::exchange(
            &profile.state_dir.join("control.sock"),
            &serde_json::json!({"version":1,"command":"status"}),
            Duration::from_secs(3),
        )?;
        let config = status
            .get("config_dir")
            .and_then(serde_json::Value::as_str)
            .ok_or("active status omitted exact config directory")?;
        let pair = ProfilePaths {
            config_dir: PathBuf::from(config),
            state_dir: profile.state_dir.clone(),
        };
        let instance = native_runtime::capture_instance(&original.origin, &pair)?;
        if instance.incarnation.pid != profile.pid
            || instance.instance_id != profile.instance_id
            || instance.image != original.image
        {
            return Err("active profile changed during authenticated capture".into());
        }
        return Ok(Some((pair, Some(instance))));
    }
    let (clients, pairs) = native_runtime::compatible_client_profiles(app)?;
    let pair = stopped_profile(clients, &pairs)?;
    if let Some(pair) = &pair {
        pair.validate()?;
    }
    Ok(pair.map(|pair| (pair, None)))
}
fn stopped_profile(clients: usize, pairs: &[ProfilePaths]) -> Result<Option<ProfilePaths>, String> {
    match pairs {
        [] if clients == 0 => Ok(None),
        [pair] => Ok(Some(pair.clone())),
        _ => Err("offline installation clients must identify exactly one explicit config/state pair; no profile was created and no app was stopped".into()),
    }
}

fn outcome(id: &str, state: &Path, running: bool) -> Outcome {
    Outcome {
        version: 1,
        operation_id: id.into(),
        state_dir: state.into(),
        status: InstallStatus::Running,
        installation: None,
        installed: false,
        applied: false,
        restart: if running {
            InstallRestart::Pending
        } else {
            InstallRestart::NotRequested
        },
        retained_bundle: None,
        error: None,
    }
}
fn write(plan: &Plan, phase: Phase, result: &Outcome) -> Result<(), String> {
    journal::write_record(
        &plan.profile.state_dir,
        &Record {
            version: 1,
            operation_id: plan.operation_id.clone(),
            phase,
            result: result.clone(),
        },
    )
    .map(|_| ())
}
fn emit(result: &Outcome) -> Result<(), String> {
    emit_to(result, &mut std::io::stdout().lock())
}
fn emit_to(result: &Outcome, output: &mut impl Write) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(result).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    match output.write_all(&bytes).and_then(|_| output.flush()) {
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => other.map_err(|e| e.to_string()),
    }
}
fn raw(request: &InstallRequest, reused: bool) -> Result<Outcome, String> {
    let record = installation::install(request)?;
    let mut result = outcome(&random_id()?, &protocol::global_updates()?, false);
    result.status = if reused {
        InstallStatus::Reused
    } else {
        InstallStatus::InstalledStopped
    };
    result.installed = true;
    result.installation = Some(record);
    Ok(result)
}

pub fn coordinated(request_path: &Path) -> Result<Outcome, String> {
    let request: InstallRequest =
        serde_json::from_slice(&coordinator::owned_private_file(request_path, 16384)?)
            .map_err(|e| format!("invalid private install request: {e}"))?;
    if request.version != 1 || !request.staged_app.is_absolute() {
        return Err("invalid install request version or stage path".into());
    }
    let candidate = installation::validate_staged_app(&request.staged_app)?;
    let current = installation::discover(None)?;
    let Some(original) = current else {
        return raw(&request, false);
    };
    let previous = semver::Version::parse(&original.app_version).map_err(|e| e.to_string())?;
    let next = semver::Version::parse(&candidate.version).map_err(|e| e.to_string())?;
    if previous >= next {
        return raw(&request, true);
    }
    if request.kind != InstallationKind::Standalone {
        return raw(&request, false);
    }
    if request.source_root.is_some()
        || original.kind != InstallationKind::Standalone
        || !matches!(original.origin, InstallOrigin::Standalone { .. })
    {
        return Err("healthy installation belongs to another owner; explicit manager transition is required".into());
    }
    let app =
        installation::validate_staged_app(coordinator::physical_install_root(&original.origin)?)?;
    let Some((profile, instance)) = select_source(&original, &app)? else {
        return raw(&request, false);
    };
    let id = random_id()?;
    let updates = protocol::private_updates(&profile.state_dir)?;
    let root = operation_dir(&profile.state_dir, &id)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .map_err(|e| e.to_string())?;
    let staged =
        installation::copy_standalone_install_stage(&candidate, &root.join("DesktopPet.app"))?;
    let helper = coordinator::executable_identity(
        &staged
            .bundle
            .join("Contents/MacOS/desktop-pet-update-coordinator"),
    )?;
    coordinator::stage_verified_helper(&helper, &root.join("helper"))?;
    let helper = coordinator::executable_identity(&root.join("helper"))?;
    let request = InstallRequest {
        version: 1,
        staged_app: staged.bundle,
        kind: InstallationKind::Standalone,
        source_root: None,
    };
    persist_new(&root.join("request.json"), &request)?;
    let original_evidence = original_evidence(&original)?;
    persist_new(
        &root.join("launch.json"),
        &Launch {
            version: 1,
            operation_id: id.clone(),
            original,
            profile: profile.clone(),
            helper: helper.clone(),
            request,
            original_evidence,
            purpose: StandaloneInstallPurpose::Upgrade,
            locale: String::new(),
            source: approved_source(&profile, instance.as_ref()),
        },
    )?;
    File::open(&root)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    File::open(&updates)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    prepare_admission(&profile.state_dir, &id)?;
    detached(&profile.state_dir, &id, &helper, false)
}

/// Explicit SSH approval is separate from native-panel consent. The original
/// profile must already be stopped; this operation never obtains restart rights.
pub fn manual_recovery(request_path: &Path) -> Result<Outcome, String> {
    let mut operator = journal::read_manual_recovery_request(request_path)?;
    let original = installation::inspect(&operator.context.executable)?;
    let profile = ProfilePaths {
        config_dir: operator.context.config_dir.clone(),
        state_dir: operator.context.state_dir.clone(),
    };
    profile.validate()?;
    if original.image != operator.context.running
        || original.app_version != operator.context.version
        || operator.context.running_origin.as_deref() != Some(&original.origin)
        || !native_runtime::profile_quiescent(&profile)?
    {
        return Err("manual recovery requires the exact original, already stopped profile".into());
    }
    let generation = protocol::stop_generation(&profile.state_dir)?;
    let source =
        installation::validate_staged_app(coordinator::physical_install_root(&original.origin)?)?;
    let candidate = installation::validate_staged_app(&operator.candidate_bundle)?;
    coordinator::validate_candidate_for_context(&candidate.image.path, &operator.context)?;
    let target = InstallOrigin::Standalone {
        bundle: installation::standalone_bundle()?,
    };
    let mut scope = NativeInstallScope::capture(&source, &profile)?;
    scope.reserve_target_scope(&target)?;
    scope.admit(None)?;
    installation::validate_release_evidence(
        &operator.release,
        operator
            .candidate_bundle
            .parent()
            .ok_or("candidate lacks parent")?,
        &operator.candidate_bundle,
    )?;
    let evidence = original_evidence(&original)?;
    let tern = coordinator::migration::capture_relocation_tern(&operator.context, &target)?;
    if protocol::stop_generation(&profile.state_dir)? != generation
        || !native_runtime::profile_quiescent(&profile)?
    {
        return Err("manual recovery source generation or quiescence changed".into());
    }
    scope.admit(None)?;
    let id = random_id()?;
    if protocol::reservation_snapshot(&original.origin, Some(&target), &profile.state_dir, &id)?
        .is_some()
    {
        return Err("manual recovery scope is already reserved".into());
    }
    installation::preflight_standalone_relocation_source(&original, &profile)?;
    let updates = protocol::private_updates(&profile.state_dir)?;
    let root = operation_dir(&profile.state_dir, &id)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .map_err(|e| e.to_string())?;
    let staged =
        installation::copy_standalone_install_stage(&candidate, &root.join("DesktopPet.app"))?;
    let previous_directory = operator
        .candidate_bundle
        .parent()
        .ok_or("candidate lacks parent")?;
    for (name, bound) in [
        (
            operator.release.asset_name.as_str(),
            operator.release.archive_size,
        ),
        ("archive.sha256", 4096),
        ("SHA256SUMS", 65536),
    ] {
        copy_release_resource(&previous_directory.join(name), &root.join(name), bound)?;
    }
    installation::validate_release_evidence(&operator.release, &root, &staged.bundle)?;
    let helper = coordinator::executable_identity(
        &staged
            .bundle
            .join("Contents/MacOS/desktop-pet-update-coordinator"),
    )?;
    coordinator::stage_verified_helper(&helper, &root.join("helper"))?;
    let helper = coordinator::executable_identity(&root.join("helper"))?;
    operator.candidate_bundle = staged.bundle.clone();
    persist_new(
        &journal::manual_operator_request_path(&profile.state_dir, &id)?,
        &operator,
    )?;
    let request = InstallRequest {
        version: 1,
        staged_app: staged.bundle,
        kind: InstallationKind::Standalone,
        source_root: None,
    };
    persist_new(&root.join("request.json"), &request)?;
    persist_new(
        &root.join("launch.json"),
        &Launch {
            version: 1,
            operation_id: id.clone(),
            original,
            profile: profile.clone(),
            helper: helper.clone(),
            request,
            purpose: StandaloneInstallPurpose::ManualRecovery {
                target,
                release: operator.release,
                previous_context: operator.context,
                beta_confirmation: (operator.version == 2).then_some(operator.confirmation),
                source_closure_sha256: digest(evidence.sidecar.as_bytes()),
                tern,
            },
            original_evidence: evidence,
            locale: String::new(),
            source: InstallSource::Stopped {
                profile: profile.clone(),
            },
        },
    )?;
    File::open(&root)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    File::open(&updates)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    journal::write_admission(
        &profile.state_dir,
        &Admission {
            version: 1,
            operation_id: id.clone(),
            launch_sha256: launch_digest(&profile.state_dir, &id)?,
            original_stop_generation: generation,
            phase: AdmissionPhase::Prepared,
            result: None,
        },
    )?;
    detached(&profile.state_dir, &id, &helper, false)
}

fn copy_release_resource(source: &Path, target: &Path, bound: u64) -> Result<(), String> {
    let mut input = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(source)
        .map_err(|e| e.to_string())?;
    let meta = input.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.permissions().mode() & 0o022 != 0
        || meta.len() > bound
    {
        return Err("unsafe or oversized official release resource".into());
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(target)
        .map_err(|e| e.to_string())?;
    let count = std::io::copy(&mut (&mut input).take(bound.saturating_add(1)), &mut output)
        .map_err(|e| e.to_string())?;
    if count != meta.len() || count > bound {
        return Err("official release resource changed while copying".into());
    }
    output.sync_all().map_err(|e| e.to_string())
}

pub fn relocate(request_path: &Path) -> Result<Outcome, String> {
    relocate_request(request_path, false)
}
pub fn relocate_manual(request_path: &Path) -> Result<Outcome, String> {
    relocate_request(request_path, true)
}
fn relocation_request_locale(
    captured: &coordinator::UpdateContext,
    requested: coordinator::UpdateContext,
) -> Result<String, String> {
    // Kernel-authenticated capture has no UI locale. Normalize presentation only,
    // then use full equality so no current or future authority field is omitted.
    let locale = requested.locale;
    let normalized = coordinator::UpdateContext {
        locale: captured.locale.clone(),
        ..requested
    };
    if *captured != normalized {
        return Err("relocation request differs from authenticated running context".into());
    }
    Ok(locale)
}

fn relocate_request(request_path: &Path, manual: bool) -> Result<Outcome, String> {
    let request: coordinator::standalone_relocation::RelocationRequest =
        serde_json::from_slice(&coordinator::owned_private_file(request_path, 16384)?)
            .map_err(|e| format!("invalid private relocation request: {e}"))?;
    if request.version != 1 || coordinator::identity::LEGACY {
        return Err("unsupported relocation request".into());
    }
    let state = &request.parent.profile.state_dir;
    request.parent.validate()?;
    if request.context.state_dir != *state
        || request.context.config_dir != request.parent.profile.config_dir
    {
        return Err("relocation request profile differs from parent".into());
    }
    // A separate, deterministic lease serializes attempt preparation only. It
    // is never used as installation or restart authority.
    let preparation_id =
        digest(format!("relocation-admission:{}", request.parent.operation_id).as_bytes());
    let _preparation = protocol::lock_operation(state, &preparation_id[..32])?;
    for id in journal::relocation_attempts(state, &request.parent.operation_id)? {
        let launch = read_launch(state, &id)?;
        if !matches!(&launch.purpose, StandaloneInstallPurpose::PrivateRelocation { parent, .. } if parent == &request.parent)
        {
            return Err("relocation attempt belongs to different parent evidence".into());
        }
        let admission = admission_for_launch(state, &id)?;
        if admission.phase != AdmissionPhase::Settled
            || protocol::reservation_exists(
                &launch.original.origin,
                launch.purpose_target(),
                state,
                &id,
            )?
        {
            return detached(state, &id, &launch.helper, manual);
        }
    }
    if let Some(offer) = journal::read_relocation_offer(state, &request.parent.operation_id)? {
        if offer.parent != request.parent {
            return Err("relocation offer parent differs".into());
        }
        let launch = read_launch(state, &offer.operation_id)?;
        let admission = admission_for_launch(state, &offer.operation_id)?;
        if !manual {
            return detached(state, &offer.operation_id, &launch.helper, false);
        }
        if admission.phase != AdmissionPhase::Settled
            || protocol::reservation_exists(
                &launch.original.origin,
                launch.purpose_target(),
                state,
                &offer.operation_id,
            )?
        {
            return detached(state, &offer.operation_id, &launch.helper, true);
        }
    }
    coordinator::standalone_relocation::validate_settled_parent(&request.context, &request.parent)?;
    let original = installation::inspect(&request.context.executable)?;
    if original.image != request.parent.candidate_image {
        return Err("relocation source differs from settled parent".into());
    }
    installation::preflight_standalone_relocation_source(&original, &request.parent.profile)?;
    let target = InstallOrigin::Standalone {
        bundle: installation::standalone_bundle()?,
    };
    let tern = coordinator::migration::capture_relocation_tern(&request.context, &target)?;
    let source =
        installation::validate_staged_app(coordinator::physical_install_root(&original.origin)?)?;
    let id = random_id()?;
    let updates = protocol::private_updates(state)?;
    let root = operation_dir(state, &id)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .map_err(|e| e.to_string())?;
    let staged =
        installation::copy_standalone_install_stage(&source, &root.join("DesktopPet.app"))?;
    let source_helper = coordinator::executable_identity(
        &staged
            .bundle
            .join("Contents/MacOS/desktop-pet-update-coordinator"),
    )?;
    coordinator::stage_verified_helper(&source_helper, &root.join("helper"))?;
    let helper = coordinator::executable_identity(&root.join("helper"))?;
    let install_request = InstallRequest {
        version: 1,
        staged_app: staged.bundle,
        kind: InstallationKind::Standalone,
        source_root: None,
    };
    persist_new(&root.join("request.json"), &install_request)?;
    persist_new(&root.join("relocation-request.json"), &request)?;
    let instance = native_runtime::capture_instance(&original.origin, &request.parent.profile)?;
    let locale = relocation_request_locale(&instance.context, request.context)?;
    let launch = Launch {
        version: 1,
        operation_id: id.clone(),
        original_evidence: original_evidence(&original)?,
        original,
        profile: request.parent.profile.clone(),
        helper: helper.clone(),
        request: install_request,
        purpose: StandaloneInstallPurpose::PrivateRelocation {
            target,
            parent: request.parent.clone(),
            source_closure_sha256: request.parent.source_closure_sha256.clone(),
            tern,
        },
        locale,
        source: approved_source(&request.parent.profile, Some(&instance)),
    };
    persist_new(&root.join("launch.json"), &launch)?;
    File::open(&root)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    prepare_admission(state, &id)?;
    File::open(&updates)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    if !manual || journal::read_relocation_offer(state, &request.parent.operation_id)?.is_none() {
        let winner = claim_relocation_offer(state, &request.parent, &id)?;
        if winner.operation_id != id {
            let winner_launch = read_launch(state, &winner.operation_id)?;
            return detached(state, &winner.operation_id, &winner_launch.helper, false);
        }
    }
    let attempt = journal::RelocationOffer {
        version: 1,
        parent: request.parent.clone(),
        operation_id: id.clone(),
    };
    protocol::write_private_atomic_new(
        &journal::relocation_attempt_path(state, &id)?,
        &serde_json::to_vec(&attempt).map_err(|e| e.to_string())?,
        0o600,
    )?;
    detached(state, &id, &helper, false)
}

fn claim_relocation_offer(
    state: &Path,
    parent: &coordinator::standalone_relocation::SettledRelocationParent,
    id: &str,
) -> Result<journal::RelocationOffer, String> {
    parent.validate()?;
    if parent.profile.state_dir != state {
        return Err("relocation claim profile differs".into());
    }
    let path = journal::relocation_offer_path(state, &parent.operation_id)?;
    if let Some(winner) = journal::read_relocation_offer(state, &parent.operation_id)? {
        if winner.parent != *parent {
            return Err("competing relocation offer parent differs".into());
        }
        File::open(protocol::private_updates(state)?)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        return Ok(winner);
    }
    let prepared = admission_for_launch(state, id)?;
    if prepared.phase != AdmissionPhase::Prepared {
        return Err("new relocation offer requires a completely prepared admission".into());
    }
    let offer = journal::RelocationOffer {
        version: 1,
        parent: parent.clone(),
        operation_id: id.into(),
    };
    let bytes = serde_json::to_vec(&offer).map_err(|e| e.to_string())?;
    let publication = protocol::write_private_atomic_new(&path, &bytes, 0o600);
    let winner = journal::read_relocation_offer(state, &parent.operation_id)?.ok_or_else(|| {
        publication
            .err()
            .unwrap_or_else(|| "relocation offer publication disappeared".into())
    })?;
    if winner.parent != *parent {
        return Err("competing relocation offer parent differs".into());
    }
    File::open(protocol::private_updates(state)?)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(winner)
}

#[derive(Debug, PartialEq, Eq)]
enum AdmissionDisposition {
    Settled,
    ExistingPlan,
    CleanupLaunch,
    FirstAdmission,
}
fn admission_disposition(
    admission: &Admission,
    has_plan: bool,
    has_reservation: bool,
    recovery: bool,
) -> AdmissionDisposition {
    if admission.phase == AdmissionPhase::Settled {
        AdmissionDisposition::Settled
    } else if has_plan {
        AdmissionDisposition::ExistingPlan
    } else if has_reservation || admission.phase != AdmissionPhase::Prepared || recovery {
        AdmissionDisposition::CleanupLaunch
    } else {
        AdmissionDisposition::FirstAdmission
    }
}
impl Launch {
    fn purpose_target(&self) -> Option<&InstallOrigin> {
        match &self.purpose {
            StandaloneInstallPurpose::Upgrade => None,
            StandaloneInstallPurpose::PrivateRelocation { target, .. }
            | StandaloneInstallPurpose::ManualRecovery { target, .. } => Some(target),
        }
    }
}

fn configure_detached_session(command: &mut Command, ack_fd: i32) {
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::dup2(ack_fd, 3) < 0 || libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}
fn detached(
    state: &Path,
    id: &str,
    helper: &ExecutableIdentity,
    recover: bool,
) -> Result<Outcome, String> {
    if coordinator::executable_identity(&helper.path)? != *helper {
        return Err("durable install helper changed".into());
    }
    let root = operation_dir(state, id)?;
    // Reestablish publication durability on same-ID reentry, including a prior
    // exclusive rename whose directory sync was interrupted.
    File::open(&root)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    File::open(protocol::private_updates(state)?)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    let log = OpenOptions::new()
        .write(true)
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root.join("operation.log"))
        .map_err(|e| e.to_string())?;
    let metadata = log.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err("unsafe durable install log".into());
    }
    // Separate from stdout/logs. A lost parent never turns acceptance into success.
    let (mut parent, child_ack) = UnixStream::pair().map_err(|e| e.to_string())?;
    let ack_fd = child_ack.as_raw_fd();
    let mut command = Command::new(&helper.path);
    command
        .arg("install-run")
        .arg("--state-dir")
        .arg(state)
        .arg("--operation-id")
        .arg(id)
        .env_clear()
        .env("HOME", installation::os_home()?)
        .env(
            "DESKTOP_PET_INSTALL_RECOVER",
            if recover { "1" } else { "0" },
        )
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(log));
    configure_detached_session(&mut command, ack_fd);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    drop(child_ack);
    parent
        .set_read_timeout(Some(Duration::from_secs(60)))
        .map_err(|e| e.to_string())?;
    let mut ack = [0u8; 1];
    let acknowledgement = parent
        .read_exact(&mut ack)
        .map_err(|e| e.to_string())
        .and_then(|_| {
            if ack == [1] {
                Ok(())
            } else {
                Err("invalid ownership ACK".into())
            }
        });
    if let Err(error) = acknowledgement {
        let plan = journal::read_plan(state, id).ok();
        let mut result = journal::read_record(state, id)
            .ok()
            .flatten()
            .map(|r| r.result)
            .unwrap_or_else(|| {
                outcome(
                    id,
                    state,
                    plan.as_ref().is_some_and(|p| p.running().is_some()),
                )
            });
        result.status = InstallStatus::Unknown;
        result.applied = false;
        if plan.as_ref().is_some_and(|p| p.running().is_some()) {
            result.restart = InstallRestart::Unknown;
        }
        result.error = Some(format!("durable ownership ACK unavailable ({error}); resources retained; inspect operation {id}; recovery helper {}", helper.path.display()));
        // The child may still own its lease. Never overwrite its journal here.
        return Ok(result);
    }
    loop {
        if let Some(exit) = child.try_wait().map_err(|e| e.to_string())? {
            if missing(&journal::admission_path(state, id)?) {
                let plan = journal::read_plan(state, id)?;
                if plan.version == 1 {
                    let result = inspect(state, id)?;
                    if result.terminal()
                        && (exit.success() || exit.code() == Some(result.exit_code()))
                    {
                        return Ok(result);
                    }
                }
            }
            if let Some(admission) = journal::read_admission(state, id)? {
                if admission.phase == AdmissionPhase::Settled {
                    let result = admission.result.ok_or("settled admission omitted result")?;
                    if exit.code() == Some(result.exit_code()) || exit.code() == Some(75) {
                        return Ok(result);
                    }
                    return Err("helper did not authenticate the settled operation; durable winner evidence was not changed".into());
                }
            }
            if let Ok(result) = inspect(state, id) {
                if matches!(
                    result.status,
                    InstallStatus::Unknown | InstallStatus::InstalledNotApplied
                ) {
                    return Ok(result);
                }
            }
            match protocol::try_lock_operation(state, id)? {
                None => {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Some(_lease) => {}
            }
            let launch = read_launch(state, id)?;
            let running = matches!(launch.source, InstallSource::Running { .. });
            let mut result = outcome(id, state, running);
            result.status = InstallStatus::Unknown;
            if running {
                result.restart = InstallRestart::Unknown;
            }
            result.error = Some("install helper exited without verified settlement; retain evidence and use install-recover".into());
            // This process owns neither the child lease nor its admission. It
            // may report uncertainty, but must never publish over the winner.
            return Ok(result);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn ownership_ack() {
    // A manual invocation may have no ACK socket. FD 3 could instead be this
    // process's operation lease; never blindly close it.
    let mut kind: libc::c_int = 0;
    let mut length = std::mem::size_of_val(&kind) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            3,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut libc::c_int).cast(),
            &mut length,
        )
    } != 0
        || kind != libc::SOCK_STREAM
    {
        return;
    }
    let mut stream = unsafe { UnixStream::from_raw_fd(3) };
    // A broken parent pipe changes neither approval nor the durable outcome.
    let _ = stream.write_all(&[1]);
}
fn source_matches(plan: &Plan, instance: &NativeInstance) -> bool {
    plan.running().is_some_and(|(approved, context)| {
        approved.pid == instance.incarnation.pid
            && approved.start_sec == instance.incarnation.start_sec
            && approved.start_usec == instance.incarnation.start_usec
            && approved.instance_id == instance.instance_id
            && approved.image == instance.image
            && *context == instance.context
    })
}
fn tick(panel: &Panel, reservation: &protocol::InstallationReservation) -> Result<(), String> {
    panel.pump();
    if reservation.user_stop_requested()? {
        return Err("a later user Stop supersedes installation restart".into());
    }
    Ok(())
}

pub fn run_child(state: &Path, id: &str) -> Result<(), String> {
    // Publication belongs exclusively to the lease-owning state machine. A late
    // child or failed lease admission must never rewrite the winner's result.
    run_child_owned(state, id)
}

fn validate_released_outcome(plan: &Plan, result: &Outcome) -> Result<(), String> {
    if result.installed {
        let installed = result
            .installation
            .as_ref()
            .ok_or("installed outcome omitted installation")?;
        if installed.origin != *plan.installed_origin()
            || installation::inspect(&installed.executable)? != *installed
        {
            return Err("released installed outcome differs from current installation".into());
        }
    } else if matches!(
        result.status,
        InstallStatus::Cancelled | InstallStatus::Failed
    ) {
        unchanged_original(plan)?;
    } else {
        return Err("released outcome has no verified settlement".into());
    }
    Ok(())
}

fn unstarted_evidence_allows_cleanup(
    phase: Option<Phase>,
    mutation_evidence: bool,
    current_generation: u64,
    original_generation: u64,
) -> bool {
    phase.is_none_or(|phase| matches!(phase, Phase::Approved | Phase::Failed))
        && !mutation_evidence
        && current_generation == original_generation
}
fn interrupted_outcome(result: &mut Outcome) {
    if !result.terminal() {
        result.status = InstallStatus::Failed;
        result.error =
            Some("Installation confirmation was interrupted; no app was stopped.".into());
    }
    result.restart = InstallRestart::NotRequested;
    result.installed = false;
    result.applied = false;
    result.installation = None;
}
fn publish_consent_failure(plan: &Plan, result: &mut Outcome, error: String) -> Result<(), String> {
    if result.terminal() {
        return Err(error);
    }
    result.status = InstallStatus::Unknown;
    result.applied = false;
    result.restart = if plan.running().is_some() {
        InstallRestart::Unknown
    } else {
        InstallRestart::NotRequested
    };
    result.error = Some(error);
    write(plan, Phase::Unknown, result)
}

fn cleanup_unstarted(plan: &Plan) -> Result<bool, String> {
    let state = &plan.profile.state_dir;
    let record = journal::read_record(state, &plan.operation_id)?;
    let mutation_evidence = [
        journal::stop_ack_path(state, &plan.operation_id)?,
        journal::replacement_path(state, &plan.operation_id)?,
        journal::relocation_commit_path(state, &plan.operation_id)?,
    ]
    .iter()
    .any(|path| !missing(path));
    if !unstarted_evidence_allows_cleanup(
        record.as_ref().map(|r| r.phase),
        mutation_evidence,
        protocol::stop_generation(state)?,
        plan.original_stop_generation,
    ) {
        return Ok(false);
    }
    unchanged_original(plan)?;
    let original =
        installation::validate_staged_app(coordinator::physical_install_root(plan.origin())?)?;
    let mut scope = NativeInstallScope::capture(&original, &plan.profile)?;
    if let Some(target) = plan.reservation_target() {
        scope.reserve_target_scope(target)?;
    }
    if plan.running().is_some() {
        let live = native_runtime::capture_instance(plan.origin(), &plan.profile)?;
        if !source_matches(plan, &live) {
            return Err("unstarted admission source identity changed".into());
        }
        scope.admit(Some(&live))?;
    } else {
        scope.admit(None)?;
    }
    let snapshot = protocol::reservation_snapshot(
        plan.origin(),
        plan.reservation_target(),
        state,
        &plan.operation_id,
    )?;
    if let Some(snapshot) = &snapshot {
        if snapshot.startup_origin.is_some() {
            return Ok(false);
        }
    }
    let release = if snapshot.is_some() {
        Some(protocol::recover_unstarted_installation(
            plan.origin(),
            plan.reservation_target(),
            state,
            &plan.operation_id,
            plan.original_stop_generation,
        )?)
    } else {
        None
    };
    let mut result = record
        .map(|r| r.result)
        .unwrap_or_else(|| outcome(&plan.operation_id, state, plan.running().is_some()));
    interrupted_outcome(&mut result);
    write(plan, Phase::Failed, &result)?;
    if let Some(release) = release {
        release.release()?;
    }
    settle_admission(plan, &result)?;
    Ok(true)
}

fn cleanup_unstarted_launch(launch: &Launch, admission: &Admission) -> Result<(), String> {
    let state = &launch.profile.state_dir;
    let id = &launch.operation_id;
    if launch.original != installation::inspect(&launch.original.executable)?
        || original_evidence(&launch.original)? != launch.original_evidence
        || protocol::stop_generation(state)? != admission.original_stop_generation
    {
        return Err("unstarted launch source or generation changed; evidence retained".into());
    }
    let app = installation::validate_staged_app(coordinator::physical_install_root(
        &launch.original.origin,
    )?)?;
    let mut scope = NativeInstallScope::capture(&app, &launch.profile)?;
    if let Some(target) = launch.purpose_target() {
        scope.reserve_target_scope(target)?;
    }
    match &launch.source {
        InstallSource::Running { .. } => {
            let instance =
                native_runtime::capture_instance(&launch.original.origin, &launch.profile)?;
            if approved_source(&launch.profile, Some(&instance)) != launch.source {
                return Err("unstarted launch live identity differs".into());
            }
            scope.admit(Some(&instance))?;
        }
        InstallSource::Stopped { .. } => {
            if !native_runtime::profile_quiescent(&launch.profile)? {
                return Err("unstarted stopped profile became active".into());
            }
            scope.admit(None)?;
        }
    }
    let mut result = outcome(id, state, false);
    result.status = InstallStatus::Failed;
    result.restart = InstallRestart::NotRequested;
    result.error = Some("Installation confirmation was interrupted; no app was stopped.".into());
    let result_path = operation_dir(state, id)?.join("interrupted-result.json");
    let bytes = serde_json::to_vec(&result).map_err(|e| e.to_string())?;
    if missing(&result_path) {
        protocol::write_private_atomic_new(&result_path, &bytes, 0o600)?;
    } else if coordinator::owned_private_file(&result_path, 32768)? != bytes {
        return Err("pre-plan interruption result differs; evidence retained".into());
    }
    if protocol::reservation_exists(&launch.original.origin, launch.purpose_target(), state, id)? {
        protocol::recover_unstarted_installation(
            &launch.original.origin,
            launch.purpose_target(),
            state,
            id,
            admission.original_stop_generation,
        )?
        .release()?;
    }
    settle_launch(
        state,
        id,
        &launch.original.origin,
        launch.purpose_target(),
        &result,
    )
}
fn run_child_owned(state: &Path, id: &str) -> Result<(), String> {
    let launch = read_launch(state, id)?;
    let executing = coordinator::verify_executing_helper(&launch.helper.path)?;
    if executing != launch.helper {
        return Err("executing install helper differs from durable ownership".into());
    }
    let _lease = match protocol::try_lock_operation(state, id)? {
        Some(lease) => lease,
        None => {
            ownership_ack();
            admission_for_launch(state, id)?;
            return Ok(());
        }
    };
    let mut admission = admission_for_launch(state, id)?;
    let snapshot = protocol::reservation_snapshot(
        &launch.original.origin,
        launch.purpose_target(),
        state,
        id,
    )?;
    let has_plan = !missing(&journal::plan_path(state, id)?);
    let existing = if has_plan {
        journal::read_record(state, id)?
    } else {
        None
    };
    let recovery = std::env::var_os("DESKTOP_PET_INSTALL_RECOVER").as_deref()
        == Some(std::ffi::OsStr::new("1"));
    let disposition = admission_disposition(&admission, has_plan, snapshot.is_some(), recovery);
    if disposition == AdmissionDisposition::Settled {
        if snapshot.is_some() {
            return Err("settled admission retains a reservation".into());
        }
        if has_plan {
            let plan = journal::read_plan(state, id)?;
            validate_plan_launch(&plan, &launch, &admission)?;
            validate_released_outcome(
                &plan,
                admission
                    .result
                    .as_ref()
                    .ok_or("settled admission omitted outcome")?,
            )?;
        }
        ownership_ack();
        return Ok(());
    }
    if disposition == AdmissionDisposition::ExistingPlan {
        let plan = journal::read_plan(state, id)?;
        if plan.version != 2 || plan.helper_image != executing {
            return Err("recovery requires the approval's matching v2 helper".into());
        }
        validate_plan_launch(&plan, &launch, &admission)?;
        ownership_ack();
        if matches!(
            plan.purpose,
            StandaloneInstallPurpose::ManualRecovery { .. }
        ) {
            if snapshot.is_none() {
                let result = existing
                    .ok_or("manual recovery lacks durable outcome")?
                    .result;
                validate_released_outcome(&plan, &result)?;
                return settle_admission(&plan, &result);
            }
            // Same-ID reentry is headless and idempotent. It observes the
            // original operator approval, never presents a native panel.
            return recover_manual_owned(&plan);
        }
        if cleanup_unstarted(&plan)? {
            return Ok(());
        }
        if snapshot.is_none() && existing.as_ref().is_some_and(|r| r.result.terminal()) {
            let result = existing.ok_or("missing terminal result")?.result;
            validate_released_outcome(&plan, &result)?;
            return settle_admission(&plan, &result);
        }
        if !recovery {
            return Ok(());
        }
        return recover_owned(&plan);
    }
    if disposition == AdmissionDisposition::CleanupLaunch {
        ownership_ack();
        return cleanup_unstarted_launch(&launch, &admission);
    }
    let candidate = installation::validate_staged_app(&launch.request.staged_app)?;
    if installation::inspect(&launch.original.executable)? != launch.original
        || original_evidence(&launch.original)? != launch.original_evidence
    {
        return Err("installation changed before detached admission".into());
    }
    let original = installation::validate_staged_app(coordinator::physical_install_root(
        &launch.original.origin,
    )?)?;
    let (profile, instance) = if matches!(
        launch.purpose,
        StandaloneInstallPurpose::ManualRecovery { .. }
    ) {
        if !native_runtime::profile_quiescent(&launch.profile)? {
            return Err("manual recovery requires an already stopped profile".into());
        }
        (launch.profile.clone(), None)
    } else {
        select_source(&launch.original, &original)?
            .ok_or("selected profile disappeared before detached admission")?
    };
    if profile != launch.profile {
        return Err("selected install profile changed".into());
    }
    if let Some(instance) = &instance {
        coordinator::validate_candidate_for_context(&candidate.image.path, &instance.context)?;
    }
    let mut scope = NativeInstallScope::capture(&original, &profile)?;
    if let StandaloneInstallPurpose::PrivateRelocation { target, parent, .. } = &launch.purpose {
        let current = instance
            .as_ref()
            .ok_or("relocation requires the original running profile")?;
        coordinator::standalone_relocation::validate_settled_parent(&current.context, parent)?;
        installation::preflight_standalone_relocation_source(&launch.original, &profile)?;
        scope.reserve_target_scope(target)?;
    }
    if let StandaloneInstallPurpose::ManualRecovery {
        target,
        previous_context,
        ..
    } = &launch.purpose
    {
        coordinator::validate_candidate_for_context(&candidate.image.path, previous_context)?;
        scope.reserve_target_scope(target)?;
        scope.admit(None)?;
        if protocol::stop_generation(state)? != admission.original_stop_generation {
            return Err("manual recovery Stop generation changed before admission".into());
        }
        installation::preflight_standalone_relocation_source(&launch.original, &profile)?;
    }
    scope.admit(instance.as_ref())?;
    let mut reservation =
        protocol::reserve_install(&launch.original.origin, launch.purpose_target(), state, id)?;
    if reservation.stop_generation != admission.original_stop_generation {
        reservation.release()?;
        return Err("Stop generation changed before installation admission".into());
    }
    if instance.is_some() && reservation.stop_generation.checked_add(1).is_none() {
        reservation.release()?;
        return Err("Stop generation overflow forbids coordinated running install".into());
    }
    let source = approved_source(&profile, instance.as_ref());
    if source != launch.source {
        reservation.release()?;
        return Err("source identity changed after durable launch capture".into());
    }
    let plan = Plan {
        version: 2,
        purpose: launch.purpose.clone(),
        operation_id: id.into(),
        original: launch.original,
        candidate_bundle: candidate.bundle.clone(),
        candidate_image: candidate.image.clone(),
        candidate_version: candidate.version.clone(),
        helper_image: executing,
        profile,
        source,
        original_stop_generation: reservation.stop_generation,
        control_token: reservation.control_token().into(),
    };
    journal::write_plan(&plan)?;
    let mut result = outcome(id, state, instance.is_some());
    write(&plan, Phase::Approved, &result)?;
    admission.phase = AdmissionPhase::Presenting;
    journal::write_admission(state, &admission)?;
    ownership_ack();
    if matches!(
        plan.purpose,
        StandaloneInstallPurpose::ManualRecovery { .. }
    ) {
        journal::write_operator_consent(&plan, &journal::manual_operator_request_path(state, id)?)?;
        return execute_manual_owned(&plan, &candidate, &mut scope, reservation, &mut result);
    }
    // No mutation or Stop has occurred. Any UI/consent failure releases cleanly.
    let consent = Panel::for_install(&launch.locale, &plan).and_then(|panel| {
        let approved = panel.confirm_install(&plan, false)?;
        Ok((panel, approved))
    });
    let panel = match consent {
        Ok((panel, true)) => panel,
        other => {
            let error = match other {
                Ok((panel, false)) => {
                    panel.close();
                    None
                }
                Err(error) => Some(error),
                _ => unreachable!(),
            };
            result.status = if error.is_some() {
                InstallStatus::Failed
            } else {
                InstallStatus::Cancelled
            };
            result.error = error;
            if instance.is_some() {
                result.restart = InstallRestart::NotRequested;
            }
            write(&plan, Phase::Failed, &result)?;
            reservation.release()?;
            settle_admission(&plan, &result)?;
            return Ok(());
        }
    };
    save_consent(&plan)?;
    let execution = execute(
        &plan,
        &candidate,
        instance.as_ref(),
        &mut scope,
        &mut reservation,
        &panel,
        &mut result,
    );
    match execution {
        Ok(()) => {
            write(&plan, Phase::Completed, &result)?;
            reservation.release()?;
            settle_admission(&plan, &result)?;
        }
        Err(error) => {
            let mut rollback_safe = missing(&journal::replacement_path(state, id)?)
                && missing(&journal::relocation_commit_path(state, id)?);
            if let Ok(recovered) = installation::recover_replacement(
                &journal::replacement_path(state, id)?,
                id,
                &plan.original,
                plan.installed_origin(),
            ) {
                rollback_safe =
                    recovered.disk_state == installation::ReplacementDiskState::Original;
                result.retained_bundle = recovered.retained_bundle;
                if let Some(installed) = recovered.installation {
                    result.installed = true;
                    result.installation = Some(installed);
                }
            }
            let phase = journal::read_record(state, id)?.map(|record| record.phase);
            if phase == Some(Phase::Approved) {
                result.status = InstallStatus::Failed;
                result.error = Some(error);
                if plan.running().is_some() {
                    result.restart = InstallRestart::NotRequested;
                }
                write(&plan, Phase::Failed, &result)?;
                panel.close();
                reservation.release()?;
                return settle_admission(&plan, &result);
            }
            if !result.installed
                && rollback_safe
                && matches!(phase, Some(Phase::OriginalStopped | Phase::Replacing))
                && restore_original_precommit(&plan, &scope, &reservation, &panel).is_ok()
            {
                result.status = InstallStatus::Failed;
                result.error = Some(error);
                if plan.running().is_some() {
                    result.restart = InstallRestart::Failed;
                }
                write(&plan, Phase::Failed, &result)?;
                panel.close();
                reservation.release()?;
                return settle_admission(&plan, &result);
            }
            let user_stopped = later_user_stop(&plan).unwrap_or(false);
            result.status = if result.installed {
                InstallStatus::InstalledNotApplied
            } else {
                InstallStatus::Unknown
            };
            result.applied = false;
            result.error = Some(error);
            if plan.running().is_some() {
                result.restart = if user_stopped {
                    InstallRestart::UserStopped
                } else {
                    InstallRestart::Unknown
                };
            }
            write(
                &plan,
                if user_stopped {
                    Phase::UserStopped
                } else {
                    Phase::Unknown
                },
                &result,
            )?;
            // Uncertain Stop/commit/start must retain both resources and reservation.
        }
    }
    panel.close();
    Ok(())
}

fn restore_original_precommit(
    plan: &Plan,
    scope: &NativeInstallScope,
    reservation: &protocol::InstallationReservation,
    panel: &Panel,
) -> Result<(), String> {
    tick(panel, reservation)?;
    if !plan.purpose.is_upgrade()
        && !missing(&journal::relocation_commit_path(
            &plan.profile.state_dir,
            &plan.operation_id,
        )?)
    {
        return Err(
            "relocation commit evidence forbids restarting the original installation".into(),
        );
    }
    unchanged_original(plan)?;
    native_runtime::wait_install_source_stopped(plan, Duration::from_secs(45), || {
        tick(panel, reservation)
    })?;
    scope.admit(None)?;
    native_runtime::verify_original_scope(scope, &plan.original)?;
    if let Some((_, context)) = plan.running() {
        if journal::read_stop_ack(plan)?.is_none()
            || plan.original_stop_generation.checked_add(1) != Some(reservation.stop_generation)
            || reservation.issued_start_token_for(plan.origin())?.is_some()
        {
            return Err(
                "precommit original restoration lacks complete, unused bootstrap restart authority"
                    .into(),
            );
        }
        tick(panel, reservation)?;
        let token = reservation.authorize_start_for(plan.origin())?;
        let restored = native_runtime::start_verified(
            context,
            &VerifiedInstallation {
                image: plan.original.image.clone(),
                origin: plan.origin().clone(),
            },
            &token,
            reservation.stop_generation,
            || tick(panel, reservation),
        )?;
        scope.admit(Some(&restored))?;
        tick(panel, reservation)?;
    }
    Ok(())
}

fn manual_authority(plan: &Plan) -> Result<(), String> {
    plan.validate()?;
    if !matches!(
        plan.purpose,
        StandaloneInstallPurpose::ManualRecovery { .. }
    ) || plan.running().is_some()
        || protocol::stop_generation(&plan.profile.state_dir)? != plan.original_stop_generation
        || !missing(&journal::stop_ack_path(
            &plan.profile.state_dir,
            &plan.operation_id,
        )?)
        || !missing(&journal::consent_path(
            &plan.profile.state_dir,
            &plan.operation_id,
        )?)
    {
        return Err("manual stopped authority or original Stop generation changed".into());
    }
    journal::read_operator_consent(
        plan,
        &journal::manual_operator_request_path(&plan.profile.state_dir, &plan.operation_id)?,
    )?
    .ok_or("manual recovery lacks exact SSH operator consent")?;
    let snapshot = protocol::reservation_snapshot(
        plan.origin(),
        plan.reservation_target(),
        &plan.profile.state_dir,
        &plan.operation_id,
    )?
    .ok_or("manual recovery reservation is absent")?;
    if snapshot.startup_origin.is_some() || !snapshot.markers_complete {
        return Err(
            "manual recovery reservation contains startup authority or incomplete markers".into(),
        );
    }
    if !native_runtime::profile_quiescent(&plan.profile)? {
        return Err("manual recovery profile is no longer stopped".into());
    }
    Ok(())
}

fn manual_drain(plan: &Plan, scope: &NativeInstallScope) -> Result<(), String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(45);
    loop {
        manual_authority(plan)?;
        match scope.admit(None) {
            Ok(()) => return Ok(()),
            Err(error) if std::time::Instant::now() >= deadline => return Err(error),
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn execute_manual_owned(
    plan: &Plan,
    candidate: &ValidatedApp,
    scope: &mut NativeInstallScope,
    reservation: protocol::InstallationReservation,
    result: &mut Outcome,
) -> Result<(), String> {
    let attempt = (|| {
        manual_authority(plan)?;
        plan.validate_for_execution()?;
        installation::preflight_standalone_relocation(plan)?;
        coordinator::migration::verify_relocation_tern_unchanged(plan)?;
        unchanged_original(plan)?;
        native_runtime::wait_install_source_stopped(plan, Duration::from_secs(45), || {
            manual_authority(plan)
        })?;
        write(plan, Phase::OriginalStopped, result)?;
        manual_drain(plan, scope)?;
        manual_authority(plan)?;
        write(plan, Phase::Replacing, result)?;
        let replaced = installation::replace_standalone_coordinated(candidate, plan, scope)?;
        result.installed = true;
        result.installation = Some(replaced.installation);
        result.retained_bundle = Some(replaced.retained_bundle);
        write(plan, Phase::Installed, result)?;
        manual_authority(plan)?;
        scope.admit(None)?;
        result.status = InstallStatus::InstalledStopped;
        result.applied = false;
        result.restart = InstallRestart::NotRequested;
        result.error = None;
        write(plan, Phase::Completed, result)
    })();
    if let Err(error) = attempt {
        // A stopped source never has restoration/start authority. Classify the
        // existing transaction under the same lease instead of issuing Stop.
        if let Err(recovery_error) = reconcile_manual_disk(plan, result, Some(&error)) {
            return publish_manual_uncertainty(plan, result, format!("{error}; {recovery_error}"));
        }
    }
    manual_authority(plan)?;
    reservation.release()?;
    settle_admission(plan, result)
}

fn publish_manual_uncertainty(
    plan: &Plan,
    result: &mut Outcome,
    error: String,
) -> Result<(), String> {
    // A late/error path must not replace an already durable terminal winner.
    if journal::read_record(&plan.profile.state_dir, &plan.operation_id)?
        .is_some_and(|record| record.result.terminal())
    {
        return Err(error);
    }
    result.status = InstallStatus::Unknown;
    result.applied = false;
    result.restart = InstallRestart::NotRequested;
    result.error = Some(error);
    write(plan, Phase::Unknown, result)
}

fn recover_manual_owned(plan: &Plan) -> Result<(), String> {
    if cleanup_unstarted(plan)? {
        return Ok(());
    }
    let mut result = inspect(&plan.profile.state_dir, &plan.operation_id)?;
    // This opaque handle can release only; no Stop/control/start API is used.
    let release = protocol::recover_bootstrap_release(plan)?;
    manual_authority(plan)?;
    let previous_error = result.error.clone();
    if let Err(error) = reconcile_manual_disk(plan, &mut result, previous_error.as_deref()) {
        return publish_manual_uncertainty(plan, &mut result, error);
    }
    manual_authority(plan)?;
    release.release()?;
    settle_admission(plan, &result)
}

fn reconcile_manual_disk(
    plan: &Plan,
    result: &mut Outcome,
    failure: Option<&str>,
) -> Result<(), String> {
    manual_authority(plan)?;
    native_runtime::wait_install_source_stopped(plan, Duration::from_secs(45), || {
        manual_authority(plan)
    })?;
    let state = &plan.profile.state_dir;
    let intent = journal::replacement_path(state, &plan.operation_id)?;
    if missing(&intent) {
        if !missing(&journal::relocation_commit_path(state, &plan.operation_id)?) {
            return Err("manual recovery commit lacks replacement intent".into());
        }
        unchanged_original(plan)?;
        let original =
            installation::validate_staged_app(coordinator::physical_install_root(plan.origin())?)?;
        let mut scope = NativeInstallScope::capture(&original, &plan.profile)?;
        scope.reserve_target_scope(plan.installed_origin())?;
        scope.admit(None)?;
        result.installed = false;
        result.installation = None;
        result.retained_bundle = None;
    } else {
        let mut recovered = installation::recover_replacement(
            &intent,
            &plan.operation_id,
            &plan.original,
            plan.installed_origin(),
        )?;
        approved_replacement(plan, &recovered)?;
        let mut scope = native_runtime::recover_install_scope(&recovered, &plan.profile)?;
        manual_drain(plan, &scope)?;
        if recovered.disk_state != installation::ReplacementDiskState::Installed {
            manual_authority(plan)?;
            recovered = installation::reconcile_replacement(
                &intent,
                &plan.operation_id,
                &plan.original,
                plan.installed_origin(),
                &plan.profile,
                &mut scope,
            )?;
            approved_replacement(plan, &recovered)?;
        }
        if recovered.disk_state == installation::ReplacementDiskState::Original {
            unchanged_original(plan)?;
        } else if recovered.disk_state != installation::ReplacementDiskState::Installed {
            return Err("manual replacement remains incompletely committed".into());
        }
        manual_authority(plan)?;
        scope.admit(None)?;
        result.installed = recovered.installation.is_some();
        result.installation = recovered.installation;
        result.retained_bundle = recovered.retained_bundle;
    }
    result.applied = false;
    result.restart = InstallRestart::NotRequested;
    result.status = if result.installed {
        InstallStatus::InstalledStopped
    } else {
        InstallStatus::Failed
    };
    result.error = if result.installed {
        None
    } else {
        Some(
            failure
                .unwrap_or("manual recovery did not commit; original remains stopped")
                .into(),
        )
    };
    manual_authority(plan)?;
    if let Some(record) = journal::read_record(state, &plan.operation_id)? {
        if matches!(
            record.result.status,
            InstallStatus::InstalledStopped | InstallStatus::Failed
        ) {
            if record.result != *result {
                return Err(
                    "manual recovery classification differs from durable terminal winner".into(),
                );
            }
            return Ok(());
        }
    }
    write(
        plan,
        if result.installed {
            Phase::Completed
        } else {
            Phase::Failed
        },
        result,
    )
}

fn execute(
    plan: &Plan,
    candidate: &ValidatedApp,
    instance: Option<&NativeInstance>,
    scope: &mut NativeInstallScope,
    reservation: &mut protocol::InstallationReservation,
    panel: &Panel,
    result: &mut Outcome,
) -> Result<(), String> {
    verify_consent(plan)?;
    panel.require_install_visible(plan)?;
    plan.validate_for_execution()?;
    if !plan.purpose.is_upgrade() {
        installation::preflight_standalone_relocation(plan)?;
        coordinator::migration::verify_relocation_tern_unchanged(plan)?;
    }
    unchanged_original(plan)?;
    tick(panel, reservation)?;
    if let Some(instance) = instance {
        if !source_matches(
            plan,
            &native_runtime::capture_instance(plan.origin(), &plan.profile)?,
        ) {
            return Err("original source changed before Stop".into());
        }
        scope.admit(Some(instance))?;
        panel.set_status("Quitting Desktop Pet", "Tern and your work remain running.");
        write(plan, Phase::StopIntent, result)?;
        let ack = native_runtime::stop_for_install(plan, instance)?;
        reservation.acknowledge_bootstrap_stop(&ack)?;
        write(plan, Phase::StopAcknowledged, result)?;
        native_runtime::wait_stopped(instance, Duration::from_secs(45), || {
            tick(panel, reservation)
        })?;
    } else {
        native_runtime::wait_install_source_stopped(plan, Duration::from_secs(45), || {
            tick(panel, reservation)
        })?;
    }
    write(plan, Phase::OriginalStopped, result)?;
    drain(scope, panel, reservation)?;
    tick(panel, reservation)?;
    panel.set_status(
        "Installing Desktop Pet",
        "The original signed bundle is retained for existing clients.",
    );
    write(plan, Phase::Replacing, result)?;
    let replaced = installation::replace_standalone_coordinated(candidate, plan, scope)?;
    result.installed = true;
    result.installation = Some(replaced.installation.clone());
    result.retained_bundle = Some(replaced.retained_bundle);
    write(plan, Phase::Installed, result)?;
    if let Some((_, context)) = plan.running() {
        tick(panel, reservation)?;
        let token = reservation.authorize_start_for(plan.installed_origin())?;
        write(plan, Phase::Starting, result)?;
        panel.set_status(
            "Starting Desktop Pet",
            "Restoring only the original approved native profile.",
        );
        let installed = VerifiedInstallation {
            image: replaced.installation.image,
            origin: replaced.installation.origin,
        };
        let started = native_runtime::start_verified(
            context,
            &installed,
            &token,
            reservation.stop_generation,
            || tick(panel, reservation),
        )?;
        scope.admit(Some(&started))?;
        tick(panel, reservation)?;
        result.status = InstallStatus::Applied;
        result.applied = true;
        result.restart = InstallRestart::Ready;
    } else {
        native_runtime::wait_install_source_stopped(plan, Duration::from_secs(45), || {
            tick(panel, reservation)
        })?;
        scope.admit(None)?;
        result.status = InstallStatus::InstalledStopped;
    }
    Ok(())
}
fn drain(
    scope: &NativeInstallScope,
    panel: &Panel,
    reservation: &protocol::InstallationReservation,
) -> Result<(), String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(45);
    loop {
        tick(panel, reservation)?;
        match scope.admit(None) {
            Ok(()) => return Ok(()),
            Err(error) if std::time::Instant::now() >= deadline => return Err(error),
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

pub fn inspect(state: &Path, id: &str) -> Result<Outcome, String> {
    if !missing(&journal::plan_path(state, id)?) {
        if let Some(record) = journal::read_record(state, id)? {
            return Ok(record.result);
        }
    }
    journal::read_admission(state, id)?
        .and_then(|a| a.result)
        .ok_or_else(|| "standalone install has no durable observation".into())
}
pub fn recover(state: &Path, id: &str) -> Result<Outcome, String> {
    if !missing(&journal::plan_path(state, id)?) {
        let plan = journal::read_plan(state, id)?;
        if plan.version == 1 {
            return detached(state, id, &plan.helper_image, true);
        }
    }
    let launch = read_launch(state, id)?;
    detached(state, id, &launch.helper, true)
}
fn recover_owned(plan: &Plan) -> Result<(), String> {
    if cleanup_unstarted(plan)? {
        return Ok(());
    }
    let mut result = inspect(&plan.profile.state_dir, &plan.operation_id)?;
    let previous_terminal = result.terminal();
    if let Err(error) = verify_consent(plan) {
        publish_consent_failure(plan, &mut result, error)?;
        return Ok(());
    }
    let reservation_present = protocol::reservation_snapshot(
        plan.origin(),
        plan.reservation_target(),
        &plan.profile.state_dir,
        &plan.operation_id,
    )?
    .is_some();
    let locale = read_launch(&plan.profile.state_dir, &plan.operation_id)?.locale;
    let panel = Panel::for_install(&locale, plan)?;
    if !panel.confirm_install(plan, true)? {
        panel.close();
        return Ok(());
    }
    let attempt = reconcile(plan, &panel, &mut result);
    if let Err(error) = attempt {
        if !reservation_present || previous_terminal {
            panel.close();
            return Err(error);
        }
        result.error = Some(error);
        result.applied = false;
        result.status = if result.installed {
            InstallStatus::InstalledNotApplied
        } else {
            InstallStatus::Unknown
        };
        let user_stopped = later_user_stop(plan).unwrap_or(false);
        if plan.running().is_some() {
            result.restart = if user_stopped {
                InstallRestart::UserStopped
            } else {
                InstallRestart::Unknown
            };
        }
        write(
            plan,
            if user_stopped {
                Phase::UserStopped
            } else {
                Phase::Unknown
            },
            &result,
        )?;
    }
    if !protocol::reservation_exists(
        plan.origin(),
        plan.reservation_target(),
        &plan.profile.state_dir,
        &plan.operation_id,
    )? {
        settle_admission(plan, &result)?;
    }
    panel.close();
    Ok(())
}
fn later_user_stop(plan: &Plan) -> Result<bool, String> {
    let expected = plan
        .original_stop_generation
        .checked_add(u64::from(plan.running().is_some()))
        .ok_or("Stop generation overflow")?;
    // g+1 without the authenticated ACK remains ambiguous, not evidence of
    // either successful bootstrap handoff or a distinct additional Stop.
    Ok(protocol::stop_generation(&plan.profile.state_dir)? > expected)
}
fn reconcile(plan: &Plan, panel: &Panel, result: &mut Outcome) -> Result<(), String> {
    let state = &plan.profile.state_dir;
    let snapshot = protocol::reservation_snapshot(
        plan.origin(),
        plan.reservation_target(),
        state,
        &plan.operation_id,
    )?;
    if snapshot.is_none() {
        // Released operations are observation only, never renewed restart authority.
        if result.installed
            && installation::inspect(
                &coordinator::physical_install_root(plan.installed_origin())?
                    .join("Contents/MacOS/desktop-pet"),
            )? == *result
                .installation
                .as_ref()
                .ok_or("missing installed outcome")?
        {
            return Ok(());
        }
        if !result.installed
            && matches!(
                result.status,
                InstallStatus::Failed | InstallStatus::Cancelled
            )
        {
            unchanged_original(plan)?;
            return Ok(());
        }
        return Err("released reservation has no matching installed outcome; recovery cannot recreate authority".into());
    }
    let reservation = match protocol::recover_bootstrap_reservation(plan) {
        Ok(reservation) => reservation,
        Err(error) => return reconcile_release_only(plan, panel, result, &error),
    };
    let intent = journal::replacement_path(state, &plan.operation_id)?;
    if missing(&intent) && !missing(&journal::relocation_commit_path(state, &plan.operation_id)?) {
        return Err("relocation commit has no replacement intent; evidence retained".into());
    }
    if !result.installed
        && result.status == InstallStatus::Failed
        && result.restart == InstallRestart::Failed
        && reservation.issued_start_token_for(plan.origin())?.is_some()
    {
        // A previous rollback already issued its one startup authorization.
        // Authenticate the ready original; never replay that token or Stop it.
        unchanged_original(plan)?;
        if !missing(&intent) {
            let proof = installation::recover_replacement(
                &intent,
                &plan.operation_id,
                &plan.original,
                plan.installed_origin(),
            )?;
            approved_replacement(plan, &proof)?;
            if proof.disk_state != installation::ReplacementDiskState::Original {
                return Err("original restart conflicts with committed replacement".into());
            }
        }
        let context = plan
            .running()
            .ok_or("original restart has no running approval")?
            .1;
        let original =
            installation::validate_staged_app(coordinator::physical_install_root(plan.origin())?)?;
        let mut scope = NativeInstallScope::capture(&original, &plan.profile)?;
        if let Some(target) = plan.reservation_target() {
            scope.reserve_target_scope(target)?;
        }
        let ready = native_runtime::verify_installed_ready(
            context,
            &VerifiedInstallation {
                image: plan.original.image.clone(),
                origin: plan.origin().clone(),
            },
            reservation.stop_generation,
        )?;
        scope.admit(Some(&ready))?;
        tick(panel, &reservation)?;
        write(plan, Phase::Failed, result)?;
        return reservation.release();
    }
    if fs::symlink_metadata(&intent).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        let original =
            installation::validate_staged_app(coordinator::physical_install_root(plan.origin())?)?;
        let scope = NativeInstallScope::capture(&original, &plan.profile)?;
        restore_original_precommit(plan, &scope, &reservation, panel)?;
        result.status = InstallStatus::Failed;
        result.error = Some("installation did not commit; original disk image and original running state reconciled".into());
        if plan.running().is_some() {
            result.restart = InstallRestart::Failed;
        }
        write(plan, Phase::Failed, result)?;
        return reservation.release();
    }
    let mut recovered = installation::recover_replacement(
        &intent,
        &plan.operation_id,
        &plan.original,
        plan.installed_origin(),
    )?;
    approved_replacement(plan, &recovered)?;
    let mut scope = native_runtime::recover_install_scope(&recovered, &plan.profile)?;
    if recovered.disk_state != installation::ReplacementDiskState::Installed {
        // Metadata repair requires quiescence. A fully committed candidate may
        // already be alive and is instead authenticated below without mutation.
        recovered = installation::reconcile_replacement(
            &intent,
            &plan.operation_id,
            &plan.original,
            plan.installed_origin(),
            &plan.profile,
            &mut scope,
        )?;
    }
    result.retained_bundle = recovered.retained_bundle;
    if recovered.disk_state == installation::ReplacementDiskState::Original {
        restore_original_precommit(plan, &scope, &reservation, panel)?;
        result.status = InstallStatus::Failed;
        result.error = Some("replacement did not commit; unchanged original reconciled".into());
        if plan.running().is_some() {
            result.restart = InstallRestart::Failed;
        }
        write(plan, Phase::Failed, result)?;
        return reservation.release();
    }
    let installation = recovered
        .installation
        .ok_or("committed replacement lacks installed evidence")?;
    result.installed = true;
    result.installation = Some(installation.clone());
    let installed = VerifiedInstallation {
        image: installation.image,
        origin: installation.origin,
    };
    if let Some((_, context)) = plan.running() {
        tick(panel, &reservation)?;
        let started = match native_runtime::capture_instance(plan.installed_origin(), &plan.profile)
        {
            Ok(_) => native_runtime::verify_installed_ready(
                context,
                &installed,
                reservation.stop_generation,
            )?,
            Err(_) => {
                native_runtime::wait_install_source_stopped(plan, Duration::from_secs(45), || {
                    tick(panel, &reservation)
                })?;
                scope.admit(None)?;
                // An already-issued token is never reissued/replayed: an uncertain
                // launcher may still be pending even when its daemon is absent.
                if reservation
                    .issued_start_token_for(plan.installed_origin())?
                    .is_some()
                {
                    return Err(
                        "issued start has no verified ready instance; launch will not be replayed"
                            .into(),
                    );
                }
                let token = reservation.authorize_start_for(plan.installed_origin())?;
                native_runtime::start_verified(
                    context,
                    &installed,
                    &token,
                    reservation.stop_generation,
                    || tick(panel, &reservation),
                )?
            }
        };
        scope.admit(Some(&started))?;
        tick(panel, &reservation)?;
        result.applied = true;
        result.status = InstallStatus::Applied;
        result.restart = InstallRestart::Ready;
    } else {
        native_runtime::wait_install_source_stopped(plan, Duration::from_secs(45), || {
            tick(panel, &reservation)
        })?;
        scope.admit(None)?;
        result.status = InstallStatus::InstalledStopped;
    }
    result.error = None;
    write(plan, Phase::Completed, result)?;
    reservation.release()
}

fn approved_replacement(
    plan: &Plan,
    recovered: &installation::RecoveredReplacement,
) -> Result<(), String> {
    if recovered.original.image.sha256 != plan.original.image.sha256
        || recovered.original.version != plan.original.app_version
        || recovered.candidate.image.sha256 != plan.candidate_image.sha256
        || recovered.candidate.version != plan.candidate_version
    {
        return Err("replacement proof differs from immutable standalone approval".into());
    }
    Ok(())
}

fn reconcile_release_only(
    plan: &Plan,
    panel: &Panel,
    result: &mut Outcome,
    restart_error: &str,
) -> Result<(), String> {
    // This opaque handle cannot issue or expose a token. A later user Stop,
    // lost ACK, or unadvanced index is never converted into restart permission.
    let release = protocol::recover_bootstrap_release(plan)?;
    native_runtime::wait_install_source_stopped(plan, Duration::from_secs(45), || {
        panel.pump();
        Ok(())
    })?;
    let intent = journal::replacement_path(&plan.profile.state_dir, &plan.operation_id)?;
    if missing(&intent)
        && !missing(&journal::relocation_commit_path(
            &plan.profile.state_dir,
            &plan.operation_id,
        )?)
    {
        return Err("relocation commit has no replacement intent; evidence retained".into());
    }
    let scope;
    let installation;
    if fs::symlink_metadata(&intent).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        unchanged_original(plan)?;
        let original =
            installation::validate_staged_app(coordinator::physical_install_root(plan.origin())?)?;
        scope = NativeInstallScope::capture(&original, &plan.profile)?;
        scope.admit(None)?;
        installation = None;
        result.retained_bundle = None;
    } else {
        let mut recovered = installation::recover_replacement(
            &intent,
            &plan.operation_id,
            &plan.original,
            plan.installed_origin(),
        )?;
        approved_replacement(plan, &recovered)?;
        let mut recovered_scope = native_runtime::recover_install_scope(&recovered, &plan.profile)?;
        recovered_scope.admit(None)?;
        if recovered.disk_state != installation::ReplacementDiskState::Installed {
            recovered = installation::reconcile_replacement(
                &intent,
                &plan.operation_id,
                &plan.original,
                plan.installed_origin(),
                &plan.profile,
                &mut recovered_scope,
            )?;
            approved_replacement(plan, &recovered)?;
        }
        if recovered.disk_state == installation::ReplacementDiskState::Original {
            unchanged_original(plan)?;
        }
        result.retained_bundle = recovered.retained_bundle;
        installation = recovered.installation;
        scope = recovered_scope;
    }
    scope.admit(None)?;
    if !native_runtime::profile_quiescent(&plan.profile)? {
        return Err("release-only recovery lost execution quiescence".into());
    }
    result.installed = installation.is_some();
    result.installation = installation;
    result.applied = false;
    let phase = if plan.running().is_some() {
        let user_stopped = later_user_stop(plan)?;
        result.restart = if user_stopped {
            InstallRestart::UserStopped
        } else {
            InstallRestart::Unknown
        };
        result.status = if result.installed {
            InstallStatus::InstalledNotApplied
        } else {
            InstallStatus::Failed
        };
        result.error = Some(format!(
            "disk and stopped execution reconciled without restart authority: {restart_error}"
        ));
        if user_stopped {
            Phase::UserStopped
        } else {
            Phase::Failed
        }
    } else {
        result.restart = InstallRestart::NotRequested;
        if result.installed {
            result.status = InstallStatus::InstalledStopped;
            result.error = None;
            Phase::Completed
        } else {
            result.status = InstallStatus::Failed;
            result.error = Some(
                "installation did not commit; original disk verified and profile remains stopped"
                    .into(),
            );
            Phase::Failed
        }
    };
    write(plan, phase, result)?;
    release.release()
}

pub fn dispatch(command: &str, args: Vec<OsString>) -> Result<i32, String> {
    if coordinator::identity::LEGACY {
        return Err("legacy distribution cannot run neutral standalone installation".into());
    }
    if matches!(
        command,
        "install-coordinated"
            | "install-relocate"
            | "install-relocate-manual"
            | "install-manual-recovery"
    ) {
        if args.len() != 2 || args[0] != "--request" || !Path::new(&args[1]).is_absolute() {
            return Err(format!("{command} requires only --request ABS"));
        }
        let result = match command {
            "install-relocate" => relocate(Path::new(&args[1]))?,
            "install-relocate-manual" => relocate_manual(Path::new(&args[1]))?,
            "install-manual-recovery" => manual_recovery(Path::new(&args[1]))?,
            _ => coordinated(Path::new(&args[1]))?,
        };
        emit(&result)?;
        return Ok(result.exit_code());
    }
    let (state, id) = parse_scope(&args)?;
    match command {
        "install-status" => {
            emit(&inspect(&state, &id)?)?;
            Ok(0)
        }
        "install-recover" => {
            let result = recover(&state, &id)?;
            emit(&result)?;
            Ok(result.exit_code())
        }
        "install-run" => {
            run_child(&state, &id)?;
            Ok(inspect(&state, &id)
                .map(|r| if r.terminal() { r.exit_code() } else { 75 })
                .unwrap_or(75))
        }
        _ => Err("unknown standalone install command".into()),
    }
}
fn parse_scope(args: &[OsString]) -> Result<(PathBuf, String), String> {
    let mut state = None;
    let mut id = None;
    let mut pairs = args.chunks_exact(2);
    for pair in &mut pairs {
        if pair[0] == "--state-dir" && state.is_none() {
            state = Some(PathBuf::from(&pair[1]));
        } else if pair[0] == "--operation-id" && id.is_none() {
            id = Some(
                pair[1]
                    .clone()
                    .into_string()
                    .map_err(|_| "invalid operation ID encoding")?,
            );
        } else {
            return Err("install command accepts only --state-dir ABS --operation-id ID".into());
        }
    }
    if !pairs.remainder().is_empty() {
        return Err("missing install option value".into());
    }
    let state = state.ok_or("missing --state-dir")?;
    if !state.is_absolute() {
        return Err("install state directory must be absolute".into());
    }
    let id = id.ok_or("missing --operation-id")?;
    protocol::validate_operation_id(&id)?;
    Ok((state, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relocation_locale_does_not_change_authenticated_context_authority() {
        let image = ExecutableIdentity {
            path: "/account/Applications/DesktopPet.app/Contents/MacOS/desktop-pet".into(),
            sha256: "a".repeat(64),
        };
        let captured = coordinator::UpdateContext {
            executable: image.path.clone(),
            version: "0.4.16".into(),
            instance_id: "running-instance".into(),
            config_dir: "/account/profile/config".into(),
            state_dir: "/account/profile/state".into(),
            running: image,
            running_origin: Some(Box::new(InstallOrigin::Standalone {
                bundle: "/account/Applications/DesktopPet.app".into(),
            })),
            assets_override: None,
            environment: [("HOME".into(), "/account".into())].into(),
            locale: "en".into(),
            #[cfg(desktop_pet_legacy)]
            host_plugin_config_dir: None,
            #[cfg(desktop_pet_legacy)]
            herdr_socket: "/account/profile/state/control.sock".into(),
        };
        for locale in ["en", "ko"] {
            let mut requested = captured.clone();
            requested.locale = locale.into();
            assert_eq!(
                relocation_request_locale(&captured, requested).unwrap(),
                locale
            );
        }
        let changes: &[fn(&mut coordinator::UpdateContext)] = &[
            |context| context.executable = "/other/desktop-pet".into(),
            |context| context.version = "0.4.17".into(),
            |context| context.instance_id = "different-instance".into(),
            |context| context.config_dir = "/other/config".into(),
            |context| context.state_dir = "/other/state".into(),
            |context| context.running.path = "/other/desktop-pet".into(),
            |context| context.running.sha256 = "b".repeat(64),
            |context| context.running_origin = None,
            |context| context.assets_override = Some("/other/assets".into()),
            |context| {
                context.environment.insert("HOME".into(), "/other".into());
            },
            |context| {
                context
                    .environment
                    .insert(coordinator::identity::OBSERVATION_ENV.into(), "auto".into());
            },
            #[cfg(desktop_pet_legacy)]
            |context| context.host_plugin_config_dir = Some("/other/host".into()),
            #[cfg(desktop_pet_legacy)]
            |context| context.herdr_socket = "/other/control.sock".into(),
        ];
        for change in changes {
            let mut requested = captured.clone();
            requested.locale = "ko".into();
            change(&mut requested);
            assert!(relocation_request_locale(&captured, requested).is_err());
        }
        assert_eq!(captured.locale, "en");
    }

    fn manual_plan_fixture(state: &Path) -> Plan {
        manual_plan_fixture_for_channel(state, false)
    }

    fn manual_plan_fixture_for_channel(state: &Path, beta: bool) -> Plan {
        let id = "9".repeat(32);
        let historical = installation::os_home()
            .unwrap()
            .join("Applications/DesktopPet.app");
        let origin = InstallOrigin::Standalone {
            bundle: historical.clone(),
        };
        let image = ExecutableIdentity {
            path: historical.join("Contents/MacOS/desktop-pet"),
            sha256: "a".repeat(64),
        };
        let profile = ProfilePaths {
            config_dir: state.into(),
            state_dir: state.into(),
        };
        let context = coordinator::UpdateContext {
            executable: image.path.clone(),
            version: "0.4.13-beta.7".into(),
            instance_id: "stopped-source".into(),
            config_dir: state.into(),
            state_dir: state.into(),
            #[cfg(desktop_pet_legacy)]
            host_plugin_config_dir: None,
            #[cfg(desktop_pet_legacy)]
            herdr_socket: state.join("host.sock"),
            running: image.clone(),
            running_origin: Some(Box::new(origin.clone())),
            assets_override: None,
            environment: Default::default(),
            locale: "ko".into(),
        };
        let version = if beta { "0.4.14-beta.1" } else { "0.4.14" };
        let tag = format!("v{version}");
        let asset_name = format!("DesktopPet-v{version}-macos-arm64.tar.gz");
        let release = coordinator::ReleaseCandidate {
            version: version.into(),
            channel: if beta {
                coordinator::ReleaseChannel::Beta
            } else {
                coordinator::ReleaseChannel::Stable
            },
            tag: tag.clone(),
            release_id: 123,
            archive_url: format!(
                "{}{tag}/{asset_name}",
                coordinator::identity::neutral::RELEASE_DOWNLOAD
            ),
            checksum_url: format!(
                "{}{tag}/{asset_name}.sha256",
                coordinator::identity::neutral::RELEASE_DOWNLOAD
            ),
            asset_name: asset_name.into(),
            archive_size: 123,
            sha256: "e".repeat(64),
        };
        let root = operation_dir(state, &id).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let plan = Plan {
            version: 2,
            operation_id: id,
            original: InstallationRecord {
                version: 1,
                executable: image.path.clone(),
                kind: InstallationKind::Standalone,
                app_version: context.version.clone(),
                protocol: 4,
                observation_mode: 2,
                integration_api: 1,
                origin,
                image,
            },
            purpose: StandaloneInstallPurpose::ManualRecovery {
                target: InstallOrigin::Standalone {
                    bundle: installation::standalone_bundle().unwrap(),
                },
                release: release.clone(),
                previous_context: context.clone(),
                beta_confirmation: beta.then(|| journal::MANUAL_BETA_RECOVERY_CONFIRMATION.into()),
                source_closure_sha256: digest(b"source signed closure"),
                tern: None,
            },
            candidate_bundle: root.join("DesktopPet.app"),
            candidate_image: ExecutableIdentity {
                path: root.join("DesktopPet.app/Contents/MacOS/desktop-pet"),
                sha256: "b".repeat(64),
            },
            candidate_version: release.version.clone(),
            helper_image: ExecutableIdentity {
                path: root.join("helper"),
                sha256: "c".repeat(64),
            },
            profile: profile.clone(),
            source: InstallSource::Stopped { profile },
            original_stop_generation: 0,
            control_token: "d".repeat(64),
        };
        plan.validate().unwrap();
        persist_new(
            &journal::plan_path(state, &plan.operation_id).unwrap(),
            &plan,
        )
        .unwrap();
        persist_new(
            &journal::manual_operator_request_path(state, &plan.operation_id).unwrap(),
            &journal::StandaloneManualRecoveryRequest {
                version: if beta { 2 } else { 1 },
                context,
                candidate_bundle: plan.candidate_bundle.clone(),
                release,
                confirmation: if beta {
                    journal::MANUAL_BETA_RECOVERY_CONFIRMATION.into()
                } else {
                    "replace_original_app_preserve_profile".into()
                },
            },
        )
        .unwrap();
        plan
    }

    #[test]
    fn manual_dispatch_is_strict_and_never_falls_through_to_native_install() {
        for args in [
            vec![],
            vec!["--request".into(), "relative.json".into()],
            vec![
                "--request".into(),
                "/private/request.json".into(),
                "--start".into(),
            ],
            vec!["--plan".into(), "/private/request.json".into()],
        ] {
            assert!(dispatch("install-manual-recovery", args).is_err());
        }
    }

    #[test]
    fn manual_consent_cannot_be_native_and_changed_generation_has_no_stop_or_start() {
        assert_manual_consent_and_generation(false);
    }

    #[test]
    fn beta_manual_consent_cannot_gain_native_restart_or_survive_lost_generation() {
        assert_manual_consent_and_generation(true);
    }

    fn assert_manual_consent_and_generation(beta: bool) {
        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        protocol::private_updates(temp.path()).unwrap();
        let mut plan = manual_plan_fixture_for_channel(temp.path(), beta);
        assert!(save_consent(&plan).is_err());
        assert!(verify_consent(&plan).is_err());
        let request_path =
            journal::manual_operator_request_path(temp.path(), &plan.operation_id).unwrap();
        journal::write_operator_consent(&plan, &request_path).unwrap();
        assert!(journal::read_operator_consent(&plan, &request_path)
            .unwrap()
            .is_some());
        assert!(manual_authority(&plan)
            .unwrap_err()
            .contains("reservation is absent"));
        // Even authentic SSH consent cannot survive an intervening user Stop.
        protocol::write_private_bytes(
            &protocol::private_updates(temp.path())
                .unwrap()
                .join("stop-generation.json"),
            b"1",
            0o600,
        )
        .unwrap();
        assert!(manual_authority(&plan)
            .unwrap_err()
            .contains("generation changed"));
        let result = outcome(&plan.operation_id, temp.path(), false);
        for phase in [Phase::StopIntent, Phase::StopAcknowledged, Phase::Starting] {
            assert!(write(&plan, phase, &result).is_err());
        }
        assert!(!journal::stop_ack_path(temp.path(), &plan.operation_id)
            .unwrap()
            .exists());
        assert!(!journal::consent_path(temp.path(), &plan.operation_id)
            .unwrap()
            .exists());
        assert!(!journal::replacement_path(temp.path(), &plan.operation_id)
            .unwrap()
            .exists());
        plan.original_stop_generation = 1;
        assert!(journal::read_operator_consent(&plan, &request_path).is_err());
    }

    #[test]
    fn manual_request_substitution_and_late_error_preserve_durable_winner() {
        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        protocol::private_updates(temp.path()).unwrap();
        let plan = manual_plan_fixture(temp.path());
        let request_path =
            journal::manual_operator_request_path(temp.path(), &plan.operation_id).unwrap();
        journal::write_operator_consent(&plan, &request_path).unwrap();
        let mut bytes = coordinator::owned_private_file(&request_path, 65536).unwrap();
        bytes.push(b'\n');
        protocol::write_private_bytes(&request_path, &bytes, 0o600).unwrap();
        assert!(journal::read_operator_consent(&plan, &request_path).is_err());
        let mut result = outcome(&plan.operation_id, temp.path(), false);
        result.status = InstallStatus::Failed;
        result.error = Some("original remains stopped".into());
        write(&plan, Phase::Failed, &result).unwrap();
        let path = journal::record_path(temp.path(), &plan.operation_id).unwrap();
        let winner = coordinator::owned_private_file(&path, 32768).unwrap();
        assert!(publish_manual_uncertainty(&plan, &mut result, "late duplicate".into()).is_err());
        assert_eq!(
            coordinator::owned_private_file(&path, 32768).unwrap(),
            winner
        );
        assert_eq!(result.restart, InstallRestart::NotRequested);
        assert!(!result.applied);
    }

    #[test]
    fn manual_release_resources_are_bounded_private_and_immutable() {
        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let source = temp.path().join("archive");
        let target = temp.path().join("copy");
        protocol::write_private_new(&source, b"official archive bytes", 0o600).unwrap();
        copy_release_resource(&source, &target, 100).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"official archive bytes");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(copy_release_resource(&source, &target, 100).is_err());
        assert!(copy_release_resource(&source, &temp.path().join("oversized"), 3).is_err());
        let link = temp.path().join("symlink");
        std::os::unix::fs::symlink(&source, &link).unwrap();
        assert!(copy_release_resource(&link, &temp.path().join("linked-copy"), 100).is_err());
    }

    #[test]
    fn manual_plan_reentry_is_bound_to_launch_purpose_source_and_generation() {
        assert_manual_plan_reentry(false);
    }

    #[test]
    fn beta_manual_reentry_rejects_changed_helper_opt_in_profile_and_generation() {
        assert_manual_plan_reentry(true);
    }

    fn assert_manual_plan_reentry(beta: bool) {
        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        protocol::private_updates(temp.path()).unwrap();
        let plan = manual_plan_fixture_for_channel(temp.path(), beta);
        let launch = Launch {
            version: 1,
            operation_id: plan.operation_id.clone(),
            original: plan.original.clone(),
            profile: plan.profile.clone(),
            helper: plan.helper_image.clone(),
            request: InstallRequest {
                version: 1,
                staged_app: plan.candidate_bundle.clone(),
                kind: InstallationKind::Standalone,
                source_root: None,
            },
            original_evidence: OriginalEvidence {
                sidecar: "source signed closure".into(),
                locator: None,
                image_device: 1,
                image_inode: 2,
                bundle_device: 1,
                bundle_inode: 3,
            },
            purpose: plan.purpose.clone(),
            locale: String::new(),
            source: plan.source.clone(),
        };
        let admission = Admission {
            version: 1,
            operation_id: plan.operation_id.clone(),
            launch_sha256: "a".repeat(64),
            original_stop_generation: 0,
            phase: AdmissionPhase::Presenting,
            result: None,
        };
        validate_plan_launch(&plan, &launch, &admission).unwrap();
        for field in 0..8 {
            let mut changed = plan.clone();
            match field {
                0 => changed.purpose = StandaloneInstallPurpose::Upgrade,
                1 => changed.original_stop_generation += 1,
                2 => changed.candidate_bundle = temp.path().join("substituted/DesktopPet.app"),
                3 => {
                    changed.source = InstallSource::Stopped {
                        profile: ProfilePaths {
                            config_dir: temp.path().join("other-config"),
                            state_dir: temp.path().into(),
                        },
                    }
                }
                4 => changed.original.image.sha256 = "f".repeat(64),
                5 => changed.helper_image.sha256 = "f".repeat(64),
                6 => changed.helper_image.path = temp.path().join("foreign-helper"),
                _ => {
                    let StandaloneInstallPurpose::ManualRecovery {
                        beta_confirmation, ..
                    } = &mut changed.purpose
                    else {
                        unreachable!()
                    };
                    *beta_confirmation = Some("forged".into());
                }
            }
            assert!(validate_plan_launch(&changed, &launch, &admission).is_err());
        }
        assert_eq!(
            admission_disposition(&admission, true, true, false),
            AdmissionDisposition::ExistingPlan,
        );
        let mut settled = admission;
        let mut result = outcome(&plan.operation_id, temp.path(), false);
        result.status = InstallStatus::Failed;
        settled.phase = AdmissionPhase::Settled;
        settled.result = Some(result);
        assert_eq!(
            admission_disposition(&settled, true, false, false),
            AdmissionDisposition::Settled,
        );
    }

    fn private_admission_fixture(state: &Path, id: &str) -> Admission {
        let resources = operation_dir(state, id).unwrap();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&resources)
            .unwrap();
        protocol::write_private_new(
            &resources.join("launch.json"),
            br#"{"captured":"immutable source scope"}"#,
            0o600,
        )
        .unwrap();
        let admission = Admission {
            version: 1,
            operation_id: id.into(),
            launch_sha256: launch_digest(state, id).unwrap(),
            original_stop_generation: 7,
            phase: AdmissionPhase::Prepared,
            result: None,
        };
        journal::write_admission(state, &admission).unwrap();
        admission
    }

    #[test]
    fn persisted_offer_claim_and_spawn_before_lease_reenter_same_id_after_restart() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path();
        fs::set_permissions(state, fs::Permissions::from_mode(0o700)).unwrap();
        protocol::private_updates(state).unwrap();
        let parent = coordinator::standalone_relocation::SettledRelocationParent {
            version: 1,
            operation_id: "a".repeat(32),
            plan_sha256: "b".repeat(64),
            result_sha256: "c".repeat(64),
            generation: 7,
            candidate_image: ExecutableIdentity {
                path: state.join("source/DesktopPet.app/Contents/MacOS/desktop-pet"),
                sha256: "d".repeat(64),
            },
            source_closure_sha256: "e".repeat(64),
            profile: ProfilePaths {
                config_dir: state.into(),
                state_dir: state.into(),
            },
        };
        let id = "1".repeat(32);
        let other_id = "2".repeat(32);
        private_admission_fixture(state, &id);
        private_admission_fixture(state, &other_id);
        let winner = claim_relocation_offer(state, &parent, &id).unwrap();
        let path = journal::relocation_offer_path(state, &parent.operation_id).unwrap();
        let original_bytes = coordinator::owned_private_file(&path, 65536).unwrap();
        // Claim-before-spawn and spawn-before-child-lease have the same durable
        // state. Neither guessing a process's existence nor a fresh ID is needed.
        for _interruption in ["claim_before_spawn", "spawn_before_child_lease"] {
            let reloaded = journal::read_relocation_offer(state, &parent.operation_id)
                .unwrap()
                .unwrap();
            assert_eq!(reloaded, winner);
            let prepared = admission_for_launch(state, &reloaded.operation_id).unwrap();
            assert_eq!(
                admission_disposition(&prepared, false, false, false),
                AdmissionDisposition::FirstAdmission
            );
            assert_eq!(
                claim_relocation_offer(state, &parent, &other_id).unwrap(),
                winner
            );
        }
        let mut presenting = admission_for_launch(state, &id).unwrap();
        presenting.phase = AdmissionPhase::Presenting;
        journal::write_admission(state, &presenting).unwrap();
        let reloaded = admission_for_launch(state, &id).unwrap();
        assert_eq!(
            admission_disposition(&reloaded, true, true, false),
            AdmissionDisposition::ExistingPlan
        );
        assert_eq!(
            admission_disposition(&reloaded, false, true, false),
            AdmissionDisposition::CleanupLaunch
        );
        let mut cancellation = outcome(&id, state, false);
        cancellation.status = InstallStatus::Cancelled;
        presenting.phase = AdmissionPhase::Settled;
        presenting.result = Some(cancellation.clone());
        journal::write_admission(state, &presenting).unwrap();
        // A reboot or late competing helper observes the immutable cancelled
        // winner; another automatic operation/dialog cannot be claimed.
        assert_eq!(
            claim_relocation_offer(state, &parent, &other_id).unwrap(),
            winner
        );
        let settled = admission_for_launch(state, &winner.operation_id).unwrap();
        assert_eq!(
            admission_disposition(&settled, true, false, false),
            AdmissionDisposition::Settled
        );
        assert_eq!(settled.result, Some(cancellation));
        assert_eq!(
            coordinator::owned_private_file(&path, 65536).unwrap(),
            original_bytes
        );
    }

    #[test]
    fn preplan_partial_scope_and_dead_presenting_are_cleanup_only_without_restart() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path();
        fs::set_permissions(state, fs::Permissions::from_mode(0o700)).unwrap();
        protocol::private_updates(state).unwrap();
        let id = "3".repeat(32);
        let mut admission = private_admission_fixture(state, &id);
        assert_eq!(
            admission_disposition(&admission, false, true, false),
            AdmissionDisposition::CleanupLaunch
        );
        admission.phase = AdmissionPhase::Presenting;
        journal::write_admission(state, &admission).unwrap();
        let reloaded = admission_for_launch(state, &id).unwrap();
        assert_eq!(
            admission_disposition(&reloaded, true, true, false),
            AdmissionDisposition::ExistingPlan
        );
        assert!(unstarted_evidence_allows_cleanup(
            Some(Phase::Approved),
            false,
            7,
            7
        ));
        for phase in [
            Phase::StopIntent,
            Phase::StopAcknowledged,
            Phase::OriginalStopped,
            Phase::Replacing,
            Phase::Installed,
            Phase::Starting,
            Phase::Completed,
            Phase::Unknown,
            Phase::UserStopped,
        ] {
            assert!(!unstarted_evidence_allows_cleanup(Some(phase), false, 7, 7));
        }
        assert!(!unstarted_evidence_allows_cleanup(
            Some(Phase::Approved),
            true,
            7,
            7
        ));
        assert!(!unstarted_evidence_allows_cleanup(
            Some(Phase::Approved),
            false,
            8,
            7
        ));
        let mut interrupted = outcome(&id, state, true);
        interrupted_outcome(&mut interrupted);
        assert_eq!(interrupted.status, InstallStatus::Failed);
        assert_eq!(interrupted.restart, InstallRestart::NotRequested);
        assert_eq!(
            interrupted.error.as_deref(),
            Some("Installation confirmation was interrupted; no app was stopped.")
        );
        let mut cancelled = outcome(&id, state, true);
        cancelled.status = InstallStatus::Cancelled;
        cancelled.restart = InstallRestart::NotRequested;
        let durable_cancelled = cancelled.clone();
        interrupted_outcome(&mut cancelled);
        assert_eq!(cancelled, durable_cancelled);
        assert!(!journal::stop_ack_path(state, &id).unwrap().exists());
        assert!(!journal::replacement_path(state, &id).unwrap().exists());
        assert!(!journal::relocation_commit_path(state, &id)
            .unwrap()
            .exists());
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn relocation_consent_is_immutable_and_binds_actual_plan_bytes_before_stop() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().to_path_buf();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
        protocol::private_updates(&state).unwrap();
        let profile = ProfilePaths {
            config_dir: state.clone(),
            state_dir: state.clone(),
        };
        let private = installation::standalone_bundle().unwrap();
        let historical = installation::os_home()
            .unwrap()
            .join("Applications/DesktopPet.app");
        let origin = InstallOrigin::Standalone {
            bundle: historical.clone(),
        };
        let image = ExecutableIdentity {
            path: historical.join("Contents/MacOS/desktop-pet"),
            sha256: "a".repeat(64),
        };
        let context = coordinator::UpdateContext {
            executable: image.path.clone(),
            version: "0.4.16".into(),
            instance_id: "approved-source".into(),
            config_dir: state.clone(),
            state_dir: state.clone(),
            running: image.clone(),
            running_origin: Some(Box::new(origin.clone())),
            assets_override: None,
            environment: Default::default(),
            locale: "ko".into(),
        };
        let id = "e".repeat(32);
        let candidate_bundle = state.join("stage/DesktopPet.app");
        let plan = Plan {
            version: 2,
            operation_id: id.clone(),
            original: InstallationRecord {
                version: 1,
                executable: image.path.clone(),
                kind: InstallationKind::Standalone,
                app_version: "0.4.16".into(),
                protocol: 4,
                observation_mode: 2,
                integration_api: 1,
                origin,
                image: image.clone(),
            },
            purpose: StandaloneInstallPurpose::PrivateRelocation {
                target: InstallOrigin::Standalone { bundle: private },
                parent: coordinator::standalone_relocation::SettledRelocationParent {
                    version: 1,
                    operation_id: "f".repeat(32),
                    plan_sha256: "b".repeat(64),
                    result_sha256: "c".repeat(64),
                    generation: 7,
                    candidate_image: image.clone(),
                    source_closure_sha256: "d".repeat(64),
                    profile: profile.clone(),
                },
                source_closure_sha256: "d".repeat(64),
                tern: None,
            },
            candidate_image: ExecutableIdentity {
                path: candidate_bundle.join("Contents/MacOS/desktop-pet"),
                sha256: image.sha256.clone(),
            },
            candidate_bundle,
            candidate_version: "0.4.16".into(),
            helper_image: ExecutableIdentity {
                path: state.join("helper"),
                sha256: "b".repeat(64),
            },
            profile,
            source: InstallSource::Running {
                instance: StandaloneOriginalInstance {
                    pid: 42,
                    start_sec: 123,
                    start_usec: 456,
                    instance_id: context.instance_id.clone(),
                    image,
                },
                context,
            },
            original_stop_generation: 7,
            control_token: "a".repeat(64),
        };
        let path = journal::plan_path(&state, &id).unwrap();
        protocol::write_private_new(&path, &serde_json::to_vec(&plan).unwrap(), 0o600).unwrap();
        assert!(verify_consent(&plan).is_err());
        let mut missing_consent = outcome(&id, &state, true);
        write(&plan, Phase::StopIntent, &missing_consent).unwrap();
        let error = verify_consent(&plan).unwrap_err();
        publish_consent_failure(&plan, &mut missing_consent, error).unwrap();
        let unknown = journal::read_record(&state, &id).unwrap().unwrap();
        assert_eq!(unknown.phase, Phase::Unknown);
        assert_eq!(unknown.result.status, InstallStatus::Unknown);
        let winner_bytes =
            coordinator::owned_private_file(&journal::record_path(&state, &id).unwrap(), 32768)
                .unwrap();
        assert!(
            publish_consent_failure(&plan, &mut missing_consent, "late duplicate".into()).is_err()
        );
        assert_eq!(
            coordinator::owned_private_file(&journal::record_path(&state, &id).unwrap(), 32768)
                .unwrap(),
            winner_bytes
        );
        save_consent(&plan).unwrap();
        verify_consent(&plan).unwrap();
        assert!(save_consent(&plan).is_err());
        let consent =
            coordinator::owned_private_file(&journal::consent_path(&state, &id).unwrap(), 16384)
                .unwrap();
        let mut changed = plan.clone();
        changed.original_stop_generation += 1;
        protocol::write_private_bytes(&path, &serde_json::to_vec(&changed).unwrap(), 0o600)
            .unwrap();
        assert!(verify_consent(&changed).is_err());
        assert_eq!(
            coordinator::owned_private_file(&journal::consent_path(&state, &id).unwrap(), 16384)
                .unwrap(),
            consent
        );
        let mut reformatted = serde_json::to_vec(&plan).unwrap();
        reformatted.push(b'\n');
        protocol::write_private_bytes(&path, &reformatted, 0o600).unwrap();
        let mut mismatch = outcome(&id, &state, true);
        write(&plan, Phase::StopIntent, &mismatch).unwrap();
        let error = verify_consent(&plan).unwrap_err();
        publish_consent_failure(&plan, &mut mismatch, error).unwrap();
        assert_eq!(
            journal::read_record(&state, &id)
                .unwrap()
                .unwrap()
                .result
                .status,
            InstallStatus::Unknown
        );
        assert!(!journal::stop_ack_path(&state, &id).unwrap().exists());
        assert!(!journal::replacement_path(&state, &id).unwrap().exists());
        assert!(!journal::relocation_commit_path(&state, &id)
            .unwrap()
            .exists());
    }

    #[test]
    fn admission_binds_actual_launch_bytes_and_settlement_is_immutable() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path();
        fs::set_permissions(state, fs::Permissions::from_mode(0o700)).unwrap();
        protocol::private_updates(state).unwrap();
        let id = "a".repeat(32);
        let resources = operation_dir(state, &id).unwrap();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&resources)
            .unwrap();
        protocol::write_private_new(
            &resources.join("launch.json"),
            b"{\"captured\":\"exact bytes\"}",
            0o600,
        )
        .unwrap();
        let mut admission = Admission {
            version: 1,
            operation_id: id.clone(),
            launch_sha256: launch_digest(state, &id).unwrap(),
            original_stop_generation: 7,
            phase: AdmissionPhase::Prepared,
            result: None,
        };
        journal::write_admission(state, &admission).unwrap();
        assert_eq!(admission_for_launch(state, &id).unwrap(), admission);
        admission.phase = AdmissionPhase::Presenting;
        journal::write_admission(state, &admission).unwrap();
        let mut rollback = admission.clone();
        rollback.phase = AdmissionPhase::Prepared;
        assert!(journal::write_admission(state, &rollback).is_err());
        protocol::write_private_bytes(
            &resources.join("launch.json"),
            b"{\"captured\":\"substituted\"}",
            0o600,
        )
        .unwrap();
        assert!(admission_for_launch(state, &id).is_err());
        let mut result = outcome(&id, state, false);
        result.status = InstallStatus::Cancelled;
        admission.phase = AdmissionPhase::Settled;
        admission.result = Some(result.clone());
        journal::write_admission(state, &admission).unwrap();
        assert_eq!(inspect(state, &id).unwrap(), result);
        let saved =
            coordinator::owned_private_file(&journal::admission_path(state, &id).unwrap(), 65536)
                .unwrap();
        result.status = InstallStatus::Unknown;
        admission.result = Some(result);
        assert!(journal::write_admission(state, &admission).is_err());
        assert_eq!(
            coordinator::owned_private_file(&journal::admission_path(state, &id).unwrap(), 65536)
                .unwrap(),
            saved
        );
    }

    #[test]
    fn admission_rejects_terminal_result_before_settlement_and_scope_substitution() {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path();
        fs::set_permissions(state, fs::Permissions::from_mode(0o700)).unwrap();
        protocol::private_updates(state).unwrap();
        let id = "b".repeat(32);
        let mut result = outcome(&id, state, false);
        result.status = InstallStatus::Failed;
        let mut admission = Admission {
            version: 1,
            operation_id: id.clone(),
            launch_sha256: "c".repeat(64),
            original_stop_generation: 1,
            phase: AdmissionPhase::Prepared,
            result: Some(result),
        };
        assert!(journal::write_admission(state, &admission).is_err());
        admission.phase = AdmissionPhase::Settled;
        admission.result.as_mut().unwrap().operation_id = "d".repeat(32);
        assert!(journal::write_admission(state, &admission).is_err());
        admission.result.as_mut().unwrap().operation_id = id.clone();
        admission.result.as_mut().unwrap().status = InstallStatus::Running;
        assert!(journal::write_admission(state, &admission).is_err());
        assert!(!journal::admission_path(state, &id).unwrap().exists());
    }
    #[test]
    fn install_status_rejects_ambiguous_or_relative_scope() {
        let id = "a".repeat(32);
        for args in [
            vec!["--state-dir", "relative", "--operation-id", &id],
            vec![
                "--state-dir",
                "/private/state",
                "--operation-id",
                &id,
                "--start",
            ],
            vec![
                "--state-dir",
                "/private/state",
                "--state-dir",
                "/private/other",
                "--operation-id",
                &id,
            ],
        ] {
            assert!(
                parse_scope(&args.into_iter().map(OsString::from).collect::<Vec<_>>()).is_err()
            );
        }
        assert_eq!(
            parse_scope(&[
                "--operation-id".into(),
                id.into(),
                "--state-dir".into(),
                "/private/state".into()
            ])
            .unwrap()
            .0,
            PathBuf::from("/private/state")
        );
    }
    #[test]
    fn output_contract_distinguishes_observation_from_success() {
        let mut result = outcome(&"a".repeat(32), Path::new("/private/state"), true);
        assert_eq!(result.exit_code(), 1);
        result.status = InstallStatus::Cancelled;
        assert_eq!(result.exit_code(), 2);
        result.status = InstallStatus::InstalledNotApplied;
        assert_eq!(result.exit_code(), 1);
        result.status = InstallStatus::Applied;
        assert_eq!(result.exit_code(), 0);
    }
    #[test]
    fn stopped_discovery_never_creates_a_pair_or_infers_restart() {
        let root = tempfile::tempdir().unwrap();
        let pair = ProfilePaths {
            config_dir: root.path().join("config"),
            state_dir: root.path().join("state"),
        };
        assert!(stopped_profile(2, &[]).is_err());
        assert_eq!(stopped_profile(0, &[]).unwrap(), None);
        assert_eq!(
            stopped_profile(3, std::slice::from_ref(&pair)).unwrap(),
            Some(pair.clone())
        );
        let other = ProfilePaths {
            config_dir: root.path().join("other-config"),
            state_dir: root.path().join("other-state"),
        };
        assert!(stopped_profile(3, &[pair.clone(), other]).is_err());
        assert!(pair.validate().is_err());
        assert!(!pair.config_dir.exists());
        assert!(!pair.state_dir.exists());
    }
    #[test]
    fn actual_status_dispatch_does_not_create_a_missing_profile() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("absent-profile");
        let args = vec![
            "--state-dir".into(),
            state.as_os_str().into(),
            "--operation-id".into(),
            "a".repeat(32).into(),
        ];
        assert!(dispatch("install-status", args).is_err());
        assert!(!state.exists());
    }
    #[test]
    fn result_output_is_one_json_frame_and_broken_pipe_is_not_failure() {
        let mut result = outcome(&"a".repeat(32), Path::new("/private/state"), false);
        result.status = InstallStatus::Unknown;
        result.error = Some("preserve resources".into());
        let mut bytes = Vec::new();
        emit_to(&result, &mut bytes).unwrap();
        assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 1);
        let decoded: Outcome = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, result);
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        emit_to(&result, &mut Closed).unwrap();
        assert_eq!(result.status, InstallStatus::Unknown);
        assert_eq!(result.exit_code(), 1);
    }
    #[test]
    fn detached_worker_survives_owning_parent_exit() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("worker-finished");
        let status = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("install_runner::tests::detached_parent_fixture")
            .env("DESKTOP_PET_INSTALL_PARENT_FIXTURE", &marker)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !marker.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(fs::read_to_string(marker).unwrap(), "finished");
    }
    #[test]
    fn detached_parent_fixture() {
        let Some(marker) = std::env::var_os("DESKTOP_PET_INSTALL_PARENT_FIXTURE") else {
            return;
        };
        let (mut reader, writer) = UnixStream::pair().unwrap();
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("printf '\\001' >&3; /bin/sleep 0.3; printf finished > \"$1.pending\" && /bin/mv \"$1.pending\" \"$1\"")
            .arg("pet-worker")
            .arg(marker)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_detached_session(&mut command, writer.as_raw_fd());
        let _worker = command.spawn().unwrap();
        drop(writer);
        reader
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ack = [0u8; 1];
        reader.read_exact(&mut ack).unwrap();
        assert_eq!(ack, [1]);
        // Returning exits this dedicated test process; its detached worker must
        // finish independently, without a Rust Child lifetime owning its fate.
    }
}
