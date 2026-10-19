//! Detached Pet-only installer. Shell lifetime is never installation authority.
use crate::helper_ui::Panel;
use desktop_pet_update_coordinator::{
    self as coordinator,
    installation::{self, InstallRequest, InstallationKind, InstallationRecord, ValidatedApp},
    native_runtime::{self, NativeInstallScope, NativeInstance, ProfilePaths},
    protocol,
    standalone_install::{
        self as journal, InstallRestart, InstallSource, InstallStatus,
        StandaloneInstallPhase as Phase, StandaloneInstallPlan as Plan,
        StandaloneInstallRecord as Record, StandaloneInstallResult as Outcome,
        StandaloneOriginalInstance,
    },
    ExecutableIdentity, InstallOrigin, VerifiedInstallation,
};
use serde::{Deserialize, Serialize};
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
    Ok(launch)
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
        NativeInstallScope::capture(app, &pair)?.admit(Some(&instance))?;
        return Ok(Some((pair, Some(instance))));
    }
    let (clients, pairs) = native_runtime::compatible_client_profiles(app)?;
    let pair = stopped_profile(clients, &pairs)?;
    if let Some(pair) = &pair {
        pair.validate()?;
        NativeInstallScope::capture(app, pair)?.admit(None)?;
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
    let desired = InstallOrigin::Standalone {
        bundle: installation::standalone_bundle()?,
    };
    if request.source_root.is_some()
        || original.kind != InstallationKind::Standalone
        || original.origin != desired
    {
        return Err("healthy installation belongs to another owner; explicit manager transition is required".into());
    }
    let app =
        installation::validate_staged_app(coordinator::physical_install_root(&original.origin)?)?;
    let Some((profile, _)) = select_source(&original, &app)? else {
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
        },
    )?;
    File::open(&root)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    File::open(&updates)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    detached(&profile.state_dir, &id, &helper, false)
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
            if exit.success() {
                if let Some(record) = journal::read_record(state, id)? {
                    if record.result.terminal() {
                        return Ok(record.result);
                    }
                }
            }
            let plan = journal::read_plan(state, id)?;
            let mut result = journal::read_record(state, id)?
                .map(|r| r.result)
                .unwrap_or_else(|| outcome(id, state, plan.running().is_some()));
            result.status = InstallStatus::Unknown;
            result.applied = false;
            if plan.running().is_some() {
                result.restart = InstallRestart::Unknown;
            }
            result.error = Some("install helper exited without a terminal outcome; retain evidence and use install-recover".into());
            if recover {
                return Ok(result);
            }
            // Child is gone; take its lease before publishing reconciliation.
            let _lease = protocol::lock_operation(state, id)?;
            write(&plan, Phase::Unknown, &result)?;
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
    let result = run_child_owned(state, id);
    if std::env::var_os("DESKTOP_PET_INSTALL_RECOVER").as_deref() == Some(std::ffi::OsStr::new("1"))
    {
        // Failed recovery admission cannot rewrite history over a newer operation.
        return result;
    }
    if let Err(error) = &result {
        if let Ok(plan) = journal::read_plan(state, id) {
            if let Ok(_lease) = protocol::lock_operation(state, id) {
                let mut observed = journal::read_record(state, id)?
                    .map(|r| r.result)
                    .unwrap_or_else(|| outcome(id, state, plan.running().is_some()));
                observed.applied = false;
                observed.status = if observed.installed {
                    InstallStatus::InstalledNotApplied
                } else {
                    InstallStatus::Unknown
                };
                if plan.running().is_some() {
                    observed.restart = InstallRestart::Unknown;
                }
                observed.error = Some(error.clone());
                write(&plan, Phase::Unknown, &observed)?;
            }
        }
    }
    result
}
fn run_child_owned(state: &Path, id: &str) -> Result<(), String> {
    let launch = read_launch(state, id)?;
    let executing = coordinator::verify_executing_helper(&launch.helper.path)?;
    if executing != launch.helper {
        return Err("executing install helper differs from durable ownership".into());
    }
    let _lease = protocol::lock_operation(state, id)?;
    if std::env::var_os("DESKTOP_PET_INSTALL_RECOVER").as_deref() == Some(std::ffi::OsStr::new("1"))
    {
        let plan = journal::read_plan(state, id)?;
        if plan.helper_image != executing {
            return Err("recovery helper differs from approval".into());
        }
        ownership_ack();
        return recover_owned(&plan);
    }
    let candidate = installation::validate_staged_app(&launch.request.staged_app)?;
    if installation::inspect(&launch.original.executable)? != launch.original {
        return Err("installation changed before detached admission".into());
    }
    let original = installation::validate_staged_app(coordinator::physical_install_root(
        &launch.original.origin,
    )?)?;
    let Some((profile, instance)) = select_source(&launch.original, &original)? else {
        return Err("selected profile disappeared before detached admission".into());
    };
    if profile != launch.profile {
        return Err("selected install profile changed".into());
    }
    if let Some(instance) = &instance {
        coordinator::validate_candidate_for_context(&candidate.image.path, &instance.context)?;
    }
    let mut scope = NativeInstallScope::capture(&original, &profile)?;
    let mut reservation = protocol::reserve_install(&launch.original.origin, None, state, id)?;
    if instance.is_some() && reservation.stop_generation.checked_add(1).is_none() {
        reservation.release()?;
        return Err("Stop generation overflow forbids coordinated running install".into());
    }
    let source = match &instance {
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
    };
    let plan = Plan {
        version: 1,
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
    ownership_ack();
    // No mutation or Stop has occurred. Any UI/consent failure releases cleanly.
    let consent = Panel::for_install(&plan).and_then(|panel| {
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
            return Ok(());
        }
    };
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
        }
        Err(error) => {
            if let Ok(recovered) = installation::recover_replacement(
                &journal::replacement_path(state, id)?,
                id,
                &plan.original,
            ) {
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
                return reservation.release();
            }
            if !result.installed
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
                return reservation.release();
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

fn execute(
    plan: &Plan,
    candidate: &ValidatedApp,
    instance: Option<&NativeInstance>,
    scope: &mut NativeInstallScope,
    reservation: &mut protocol::InstallationReservation,
    panel: &Panel,
    result: &mut Outcome,
) -> Result<(), String> {
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
    let replaced = installation::replace_standalone_coordinated(
        candidate,
        &plan.original,
        scope,
        &plan.operation_id,
        &journal::replacement_path(&plan.profile.state_dir, &plan.operation_id)?,
    )?;
    result.installed = true;
    result.installation = Some(replaced.installation.clone());
    result.retained_bundle = Some(replaced.retained_bundle);
    write(plan, Phase::Installed, result)?;
    if let Some((_, context)) = plan.running() {
        tick(panel, reservation)?;
        let token = reservation.authorize_start_for(plan.origin())?;
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
    journal::read_record(state, id)?
        .map(|r| r.result)
        .ok_or_else(|| "standalone install has no durable observation".into())
}
pub fn recover(state: &Path, id: &str) -> Result<Outcome, String> {
    let plan = journal::read_plan(state, id)?;
    detached(state, id, &plan.helper_image, true)
}
fn recover_owned(plan: &Plan) -> Result<(), String> {
    let mut result = inspect(&plan.profile.state_dir, &plan.operation_id)?;
    let reservation_present = protocol::reservation_snapshot(
        plan.origin(),
        None,
        &plan.profile.state_dir,
        &plan.operation_id,
    )?
    .is_some();
    let panel = Panel::for_install(plan)?;
    if !panel.confirm_install(plan, true)? {
        panel.close();
        return Ok(());
    }
    let attempt = reconcile(plan, &panel, &mut result);
    if let Err(error) = attempt {
        if !reservation_present {
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
    let snapshot = protocol::reservation_snapshot(plan.origin(), None, state, &plan.operation_id)?;
    if snapshot.is_none() {
        // Released operations are observation only, never renewed restart authority.
        if result.installed
            && installation::inspect(&plan.original.executable)?
                == *result
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
    let mut recovered =
        installation::recover_replacement(&intent, &plan.operation_id, &plan.original)?;
    approved_replacement(plan, &recovered)?;
    let mut scope = native_runtime::recover_install_scope(&recovered, &plan.profile)?;
    if recovered.installation.is_none() {
        // Metadata repair requires quiescence. A fully committed candidate may
        // already be alive and is instead authenticated below without mutation.
        recovered = installation::reconcile_replacement(
            &intent,
            &plan.operation_id,
            &plan.original,
            &plan.profile,
            &mut scope,
        )?;
    }
    result.retained_bundle = recovered.retained_bundle;
    let Some(installation) = recovered.installation else {
        restore_original_precommit(plan, &scope, &reservation, panel)?;
        result.status = InstallStatus::Failed;
        result.error = Some("replacement did not commit; unchanged original reconciled".into());
        if plan.running().is_some() {
            result.restart = InstallRestart::Failed;
        }
        write(plan, Phase::Failed, result)?;
        return reservation.release();
    };
    result.installed = true;
    result.installation = Some(installation.clone());
    let installed = VerifiedInstallation {
        image: installation.image,
        origin: installation.origin,
    };
    if let Some((_, context)) = plan.running() {
        tick(panel, &reservation)?;
        let started = match native_runtime::capture_instance(plan.origin(), &plan.profile) {
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
                if reservation.issued_start_token_for(plan.origin())?.is_some() {
                    return Err(
                        "issued start has no verified ready instance; launch will not be replayed"
                            .into(),
                    );
                }
                let token = reservation.authorize_start_for(plan.origin())?;
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
        let mut recovered =
            installation::recover_replacement(&intent, &plan.operation_id, &plan.original)?;
        approved_replacement(plan, &recovered)?;
        let mut recovered_scope = native_runtime::recover_install_scope(&recovered, &plan.profile)?;
        recovered_scope.admit(None)?;
        if recovered.installation.is_none() {
            recovered = installation::reconcile_replacement(
                &intent,
                &plan.operation_id,
                &plan.original,
                &plan.profile,
                &mut recovered_scope,
            )?;
            approved_replacement(plan, &recovered)?;
        }
        if recovered.installation.is_none() {
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
    if command == "install-coordinated" {
        if args.len() != 2 || args[0] != "--request" {
            return Err("install-coordinated requires only --request ABS".into());
        }
        let result = coordinated(Path::new(&args[1]))?;
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
            Ok(0)
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
