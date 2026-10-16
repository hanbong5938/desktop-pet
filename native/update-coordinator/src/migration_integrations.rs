//! App cutover completion and external provider availability are separate facts.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TernDelivery {
    pub program: ExecutableIdentity,
    pub package: PathBuf,
    pub package_sha256: String,
    pub revision: String,
    pub environment: BTreeMap<String, String>,
    pub configuration: TernConfiguration,
    #[serde(default)]
    pub installed_package: Option<PathBuf>,
    #[serde(default)]
    pub daemon_socket: Option<PathBuf>,
}
fn package_identity(package: &Path) -> Result<(String, String), String> {
    owned_directory(package)?;
    let mut files = fs::read_dir(package)
        .map_err(|e| e.to_string())?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name())
                .map_err(|e| e.to_string())
        })
        .take(5)
        .collect::<Result<Vec<_>, _>>()?;
    files.sort();
    if !files.iter().map(|name| name.to_str()).eq([
        "LICENSE.txt",
        "plugin.toml",
        "revision",
        "window.luau",
    ]
    .map(Some))
    {
        return Err(
            "signed Tern delivery package must contain exactly the approved four regular files"
                .into(),
        );
    }
    let mut hash = Sha256::new();
    let mut revision = String::new();
    let mut manifest = String::new();
    for name in files {
        let bound = match name.to_str() {
            Some("revision") => 41,
            Some("plugin.toml") => 65536,
            _ => 512 * 1024,
        };
        let bytes = crate::owned_file(&package.join(&name), bound)?;
        hash.update(name.as_encoded_bytes());
        hash.update([0]);
        hash.update(&bytes);
        match name.to_str() {
            Some("revision") => revision = String::from_utf8(bytes).map_err(|e| e.to_string())?,
            Some("plugin.toml") => {
                manifest = String::from_utf8(bytes).map_err(|e| e.to_string())?
            }
            _ => {}
        }
    }
    if revision.len() != 41 || !revision.ends_with('\n') || !hex(&revision[..40], 40) {
        return Err("signed Tern package has no exact immutable consumer revision".into());
    }
    if !manifest
        .lines()
        .any(|line| line.trim() == "id = \"tern-desktop-pet\"")
        || !manifest
            .lines()
            .any(|line| line.trim() == "window = \"window.luau\"")
        || !manifest
            .lines()
            .any(|line| line.trim() == "version = \"0.2.1\"")
    {
        return Err("signed Tern package does not name the configured integration".into());
    }
    Ok((format!("{:x}", hash.finalize()), revision[..40].into()))
}
pub(super) fn capture_tern_delivery(
    target: &MigrationTarget,
    profile: &LegacyProfile,
    configuration: TernConfiguration,
) -> Result<TernDelivery, String> {
    let package = target
        .staged_bundle
        .join("Contents/Resources/integrations/tern-desktop-pet");
    let (package_sha256, revision) = package_identity(&package)?;
    let home = crate::origin::os_home()?;
    let candidates = [
        PathBuf::from("/Applications/Tern.app/Contents/MacOS/tern"),
        home.join("Applications/Tern.app/Contents/MacOS/tern"),
    ];
    let program = candidates
        .iter()
        .find(|path| path.is_file())
        .ok_or("configured Tern application CLI is unavailable; it was not reconnected")?;
    let program = executable_identity(program)?;
    let environment = neutral_environment(profile);
    let installed_package = tern_config::verify_cli_routing(profile, &program.path, &environment)?;
    let daemon_socket = environment.get("TERN_DAEMON_SOCKET").map(PathBuf::from);
    let catalog: serde_json::Value =
        serde_json::from_str(&crate::origin::command_output_with_environment(
            &program.path,
            &["plugin", "list", "--json"],
            256 * 1024,
            10,
            &environment,
        )?)
        .map_err(|e| format!("cannot inspect configured Tern integration: {e}"))?;
    if !catalog["plugins"].as_array().is_some_and(|plugins| {
        plugins
            .iter()
            .any(|plugin| plugin["id"].as_str() == Some("tern-desktop-pet"))
    }) {
        return Err("configured Tern provider has no installed tern-desktop-pet consumer; no unrelated plugin was installed".into());
    }
    Ok(TernDelivery {
        program,
        package,
        package_sha256,
        revision,
        environment,
        configuration,
        installed_package: Some(installed_package),
        daemon_socket,
    })
}
fn delivery_path_for(plan: &MigrationPlan, state: &Path) -> PathBuf {
    state
        .join("updates-v4")
        .join(format!("{}.tern-delivery.json", plan.operation_id))
}
#[cfg(test)]
fn delivery_path(plan: &MigrationPlan) -> PathBuf {
    delivery_path_for(plan, &plan.profiles[0].state_dir)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeliveryResult {
    version: u32,
    operation_id: String,
    plan_sha256: String,
    #[serde(default)]
    plan_path: Option<PathBuf>,
    original_record_sha256: Option<String>,
    current_image: ExecutableIdentity,
    owner_upgrade: Option<OwnerUpgradeProof>,
    state_dir: PathBuf,
    stop_generation: u64,
    delivery: TernDelivery,
    intent: bool,
    settled: bool,
    error: Option<String>,
}
fn previous_delivery(
    path: &Path,
    plan: &MigrationPlan,
    record: &MigrationRecord,
    image: &ExecutableIdentity,
    owner: Option<&OwnerUpgradeProof>,
    original_record: Option<&str>,
    state: &Path,
    generation: u64,
) -> Result<Option<DeliveryResult>, String> {
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    let result: DeliveryResult = serde_json::from_slice(&owned_private_file(path, MAX_PLAN)?)
        .map_err(|e| {
            format!("prior Tern delivery intent cannot be authenticated; it was not replayed: {e}")
        })?;
    if result.version != 1
        || !result.intent
        || result.operation_id != plan.operation_id
        || result.plan_sha256 != record.plan_sha256
        || &result.current_image != image
        || result.owner_upgrade.as_ref() != owner
        || result.original_record_sha256.as_deref() != original_record
        || result.state_dir != state
        || (!result.settled && result.stop_generation != generation)
    {
        return Err("prior Tern delivery authority differs; its durable intent was preserved without replay".into());
    }
    Ok(Some(result))
}
fn delivery_error(result: &DeliveryResult) -> Option<String> {
    if result.settled {
        result.error.clone()
    } else {
        Some("Tern delivery has no settled outcome; its durable intent will not be replayed".into())
    }
}
fn verify_delivery_package(delivery: &TernDelivery) -> Result<(), String> {
    let (package_sha256, revision) = package_identity(&delivery.package)?;
    if executable_identity(&delivery.program.path)? != delivery.program
        || package_sha256 != delivery.package_sha256
        || revision != delivery.revision
    {
        return Err(
            "approved Tern CLI or signed consumer package changed; no installation was attempted"
                .into(),
        );
    }
    tern_config::verify_delivery_routing(delivery)?;
    Ok(())
}
fn verify_delivery(delivery: &TernDelivery, state: &Path, generation: u64) -> Result<(), String> {
    if migration_stop_generation(state)? != generation {
        return Err("user Stop prevents Tern integration delivery".into());
    }
    tern_config::verify_configuration(&delivery.configuration)?;
    verify_delivery_package(delivery)?;
    Ok(())
}
fn verify_installed_delivery(delivery: &TernDelivery) -> Result<(), String> {
    verify_delivery_package(delivery)?;
    let installed = delivery
        .installed_package
        .as_deref()
        .ok_or("retained delivery has no captured actual Tern installation root")?;
    let (digest, revision) = package_identity(installed)?;
    if digest != delivery.package_sha256 || revision != delivery.revision {
        return Err(
            "actual installed Tern package differs from the captured signed consumer".into(),
        );
    }
    Ok(())
}
fn delivery_stop_plan(intent: &DeliveryResult) -> Result<Option<MigrationPlan>, String> {
    let path = intent.plan_path.as_deref().ok_or(
        "Tern delivery has no retained central original approval; its intent was not replayed",
    )?;
    let bytes = owned_private_file(path, MAX_PLAN)?;
    let plan: MigrationPlan = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    validate_plan_structure(&plan)?;
    if path != evidence_path(&plan)
        || plan.operation_id != intent.operation_id
        || format!("{:x}", Sha256::digest(&bytes)) != intent.plan_sha256
        || !plan.profiles.iter().any(|profile| {
            profile.state_dir == intent.state_dir
                && profile.config_dir == intent.delivery.configuration.connection.config_dir
        })
    {
        return Err("Tern delivery central original approval/profile changed".into());
    }
    if intent.owner_upgrade.is_some() {
        let record = read_record(&plan)?;
        if intent.original_record_sha256.as_deref()
            != Some(
                format!(
                    "{:x}",
                    Sha256::digest(owned_private_file(&journal_path(&plan), MAX_PLAN)?)
                )
                .as_str(),
            )
            || !record.native_ready
            || !record.target_committed
        {
            return Err("historical Tern delivery original committed journal changed".into());
        }
        let profile = plan
            .profiles
            .iter()
            .find(|profile| {
                profile.state_dir == intent.state_dir
                    && profile.config_dir == intent.delivery.configuration.connection.config_dir
            })
            .ok_or("historical delivery original profile differs")?;
        verify_retained_commit(&plan, &record, profile)?;
        Ok(None)
    } else {
        Ok(Some(plan))
    }
}
fn check_delivery_stop(
    intent: &DeliveryResult,
    plan: Option<&MigrationPlan>,
) -> Result<(), String> {
    if migration_stop_generation(&intent.state_dir)? != intent.stop_generation
        || plan.map(stop_requested).transpose()?.unwrap_or(false)
    {
        return Err("user Stop prevents Tern integration delivery and settlement".into());
    }
    Ok(())
}
fn execute_delivery(mut intent: DeliveryResult, path: PathBuf) -> Result<(), String> {
    let mut unknown = false;
    let result = (|| -> Result<(), String> {
        let approved_plan = delivery_stop_plan(&intent)?;
        check_delivery_stop(&intent, approved_plan.as_ref())?;
        verify_delivery(&intent.delivery, &intent.state_dir, intent.stop_generation)?;
        let base = intent.state_dir.join("updates-v4");
        let name = path
            .file_stem()
            .ok_or("Tern delivery intent has no file name")?
            .to_string_lossy();
        let stdout_path = base.join(format!("{name}.stdout"));
        let stderr_path = base.join(format!("{name}.stderr"));
        let output = |path: &Path| {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path)
                .map_err(|e| e.to_string())
        };
        let mut command = Command::new(&intent.delivery.program.path);
        command
            .args(["plugin", "install"])
            .arg(&intent.delivery.package)
            .args(["--force", "--json"])
            .env_clear()
            .envs(&intent.delivery.environment)
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(output(&stdout_path)?)
            .stderr(output(&stderr_path)?)
            .process_group(0);
        check_delivery_stop(&intent, approved_plan.as_ref())?;
        let mut child = command
            .spawn()
            .map_err(|e| format!("Tern consumer installation could not start: {e}"))?;
        let until = Instant::now() + Duration::from_secs(75);
        let exit = loop {
            if let Err(error) = check_delivery_stop(&intent, approved_plan.as_ref()) {
                unknown = true;
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGTERM);
                }
                thread::spawn(move || {
                    let _ = child.wait();
                });
                return Err(error);
            }
            if let Some(exit) = child.try_wait().map_err(|e| {
                unknown = true;
                e.to_string()
            })? {
                break exit;
            }
            if Instant::now() >= until {
                unknown = true;
                thread::spawn(move || {
                    let _ = child.wait();
                });
                return Err("Tern integration delivery outcome is unknown; its durable intent will not be replayed".into());
            }
            thread::sleep(Duration::from_millis(100));
        };
        let stderr = String::from_utf8_lossy(&owned_private_file(&stderr_path, 65536)?)
            .trim()
            .to_owned();
        if !exit.success() {
            return Err(format!(
                "Tern integration installation failed ({exit}): {stderr}"
            ));
        }
        let installed: serde_json::Value =
            serde_json::from_slice(&owned_private_file(&stdout_path, 65536)?).map_err(|e| {
                format!("Tern installer returned no valid installation result: {e}; {stderr}")
            })?;
        if installed["id"].as_str() != Some("tern-desktop-pet")
            || installed["dir"].as_str().map(Path::new)
                != intent.delivery.installed_package.as_deref()
        {
            return Err("Tern installer did not report the approved integration and captured installation root".into());
        }
        verify_installed_delivery(&intent.delivery)?;
        if !stderr.is_empty() {
            return Err(format!(
                "Tern package installed but provider reload is unavailable: {stderr}"
            ));
        }
        check_delivery_stop(&intent, approved_plan.as_ref())?;
        Ok(())
    })();
    if !unknown {
        intent.settled = true;
        intent.error = result.as_ref().err().cloned();
        protocol::write_private_bytes(
            &path,
            &serde_json::to_vec(&intent).map_err(|e| e.to_string())?,
            0o600,
        )?;
    }
    result
}
/// Delivery intent is single-use even when the host CLI times out after commit.
/// Its exit/reload report is never treated as actual reconnection evidence.
pub(super) fn deliver_tern(plan: &MigrationPlan, record: &mut MigrationRecord) {
    let mut errors = Vec::new();
    if let Some(problem) = &plan.tern_delivery_problem {
        errors.push(problem.clone());
    }
    for profile in plan
        .profiles
        .iter()
        .filter(|profile| profile.tern_reconnect_required)
    {
        let result = (|| -> Result<(), String> {
            let delivery = plan
                .tern_deliveries
                .iter()
                .chain(plan.tern_delivery.iter())
                .find(|delivery| {
                    delivery.configuration.connection.config_dir == profile.config_dir
                        && delivery.configuration.connection.state_dir == profile.state_dir
                })
                .ok_or(
                    "approved original profile has no authenticated Tern consumer delivery package",
                )?;
            let image = committed_profile_image(plan, record, profile)?;
            let path = delivery_path_for(plan, &profile.state_dir);
            if let Some(previous) = previous_delivery(
                &path,
                plan,
                record,
                image,
                None,
                None,
                &profile.state_dir,
                profile.stop_generation,
            )? {
                return delivery_error(&previous).map_or(Ok(()), Err);
            }
            verify_delivery(delivery, &profile.state_dir, profile.stop_generation)?;
            if stop_requested(plan)? {
                return Err("user Stop prevents Tern integration delivery".into());
            }
            let intent = DeliveryResult {
                version: 1,
                operation_id: plan.operation_id.clone(),
                plan_sha256: record.plan_sha256.clone(),
                plan_path: Some(evidence_path(plan)),
                original_record_sha256: None,
                current_image: image.clone(),
                owner_upgrade: None,
                state_dir: profile.state_dir.clone(),
                stop_generation: profile.stop_generation,
                delivery: delivery.clone(),
                intent: true,
                settled: false,
                error: None,
            };
            protocol::write_private_new(
                &path,
                &serde_json::to_vec(&intent).map_err(|e| e.to_string())?,
                0o600,
            )?;
            execute_delivery(intent, path)
        })();
        if let Err(error) = result {
            errors.push(format!("{}: {error}", profile.state_dir.display()));
        }
    }
    record.integration_delivery_error = (!errors.is_empty()).then(|| errors.join("; "));
}
pub(super) fn begin_fresh_integrations(
    plan: &MigrationPlan,
    record: &mut MigrationRecord,
) -> Result<(), String> {
    if record.phase != MigrationPhase::Integrating
        || !record.target_committed
        || stop_requested(plan)?
    {
        return Err("fresh integration delivery requires all durable target Commits and unchanged Stop authority".into());
    }
    retain_profile_plans(plan, record)?;
    deliver_tern(plan, record);
    // Provider failure is a committed-app/retirement-pending outcome, not rollback.
    Ok(())
}
pub(super) fn refresh_fresh_integrations(
    plan: &MigrationPlan,
    record: &mut MigrationRecord,
) -> Result<bool, String> {
    if record.phase != MigrationPhase::Integrating || stop_requested(plan)? {
        return Err(
            "user Stop or operation phase prevents committed integration settlement".into(),
        );
    }
    verify_all_committed_targets(plan, record)?;
    observe_integrations(plan, record)?;
    Ok(record.integration_delivery_error.is_none()
        && record
            .integration_status
            .iter()
            .all(|status| status.connected))
}
fn forward_path(plan: &MigrationPlan, state: &Path, image: &ExecutableIdentity) -> PathBuf {
    state.join("updates-v4").join(format!(
        "{}.tern-forward-{}.json",
        plan.operation_id, image.sha256
    ))
}
fn deliver_forward(
    original: &MigrationPlan,
    current: &mut MigrationPlan,
    record: &mut MigrationRecord,
    owner: &OwnerUpgradeProof,
    original_record: &str,
    generation: u64,
) {
    let result = (|| -> Result<(), String> {
        let profile = &current.profiles[0];
        let path = forward_path(original, &profile.state_dir, &current.target.image);
        if let Some(previous) = previous_delivery(
            &path,
            original,
            record,
            &current.target.image,
            Some(owner),
            Some(original_record),
            &profile.state_dir,
            generation,
        )? {
            current.tern_reconnect_required = true;
            current.profiles[0].tern_reconnect_required = true;
            return delivery_error(&previous).map_or(Ok(()), Err);
        }
        let previous_image = target_image(original, record)?;
        tern_config::configure_delivery_with_previous(current, Some(previous_image));
        let Some(delivery) = current.tern_deliveries.first() else {
            return current.tern_delivery_problem.clone().map_or(Ok(()), Err);
        };
        verify_delivery(delivery, &current.profiles[0].state_dir, generation)?;
        let intent = DeliveryResult {
            version: 1,
            operation_id: original.operation_id.clone(),
            plan_sha256: record.plan_sha256.clone(),
            plan_path: Some(evidence_path(original)),
            original_record_sha256: Some(original_record.into()),
            current_image: current.target.image.clone(),
            owner_upgrade: Some(owner.clone()),
            state_dir: current.profiles[0].state_dir.clone(),
            stop_generation: generation,
            delivery: delivery.clone(),
            intent: true,
            settled: false,
            error: None,
        };
        protocol::write_private_new(
            &path,
            &serde_json::to_vec(&intent).map_err(|e| e.to_string())?,
            0o600,
        )?;
        thread::spawn(move || {
            let _ = execute_delivery(intent, path);
        });
        Err("Configured Tern consumer refresh is in progress; reconnection requires a fresh live same-window handover".into())
    })();
    record.integration_delivery_error = result.err();
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MigrationIntegrationStatus {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub provider: String,
    pub connected: bool,
    pub error: Option<String>,
}
fn committed_profile_image<'a>(
    plan: &'a MigrationPlan,
    record: &'a MigrationRecord,
    profile: &LegacyProfile,
) -> Result<&'a ExecutableIdentity, String> {
    if record.target_profiles.is_empty() {
        if profile.config_dir != plan.profiles[0].config_dir
            || profile.state_dir != plan.profiles[0].state_dir
            || !record.native_ready
            || !record.target_committed
            || record.target_pid.is_none()
            || record
                .target_instance
                .as_deref()
                .is_none_or(|instance| !instance_valid(instance))
        {
            return Err(
                "historical migration has no genuine Commit for this original profile".into(),
            );
        }
        return target_image(plan, record);
    }
    let target = record
        .target_profiles
        .iter()
        .find(|target| {
            target.config_dir == profile.config_dir && target.state_dir == profile.state_dir
        })
        .ok_or("original profile has no captured target Commit")?;
    if !target.native_ready
        || !target.committed
        || target.pid.is_none()
        || target.instance_id.is_none()
    {
        return Err("original profile has not durably committed its native target".into());
    }
    Ok(&target.image)
}
fn verify_target_manager_result(
    plan: &MigrationPlan,
    record: &MigrationRecord,
) -> Result<(), String> {
    let Some(manager) = &record.target_manager else {
        return Ok(());
    };
    let InstallOrigin::Homebrew { formula, .. } = &plan.target.origin else {
        return Err("retained target manager has no approved owner transaction".into());
    };
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
        || manager.args[1] != *formula
    {
        return Err(
            "retained target manager is not the certain successful original approved transaction"
                .into(),
        );
    }
    Ok(())
}
fn verify_retained_commit(
    plan: &MigrationPlan,
    record: &MigrationRecord,
    profile: &LegacyProfile,
) -> Result<(), String> {
    committed_profile_image(plan, record, profile)?;
    if !matches!(
        record.phase,
        MigrationPhase::Complete | MigrationPhase::AwaitingReconnect
    ) || !record.native_ready
        || !record.target_committed
        || record.retirement_attempt == 0
    {
        return Err("historical integration requires actual original Commit and completed retirement evidence".into());
    }
    verify_target_manager_result(plan, record)?;
    let manager = record.manager.as_ref();
    let owner_matches = match (&plan.source_origin, manager) {
        (LegacyOrigin::Homebrew { brew, formula, .. }, Some(manager)) => {
            &manager.program == brew
                && manager.args.len() == 2
                && manager.args[0] == "uninstall"
                && manager.args[1] == *formula
        }
        (
            LegacyOrigin::HerdrManaged {
                host,
                integration_commit,
                ..
            },
            Some(manager),
        ) => {
            &manager.program == host
                && manager.args.len() == 6
                && manager.args[0] == "plugin"
                && manager.args[1] == "install"
                && manager.args[2] == "--yes"
                && manager.args[3] == "--ref"
                && manager.args[4] == *integration_commit
                && manager.args[5] == "hanbong5938/herdr-desktop-pet"
        }
        (LegacyOrigin::Generated { .. } | LegacyOrigin::Manual { .. }, None) => true,
        _ => false,
    };
    if !owner_matches
        || manager
            .is_some_and(|manager| !manager_can_resume(manager) || manager.exit_code != Some(0))
    {
        return Err("historical integration has no certain successful exact approved-owner retirement transaction".into());
    }
    retired(plan)
}
pub(super) fn observe_integrations(
    plan: &MigrationPlan,
    record: &mut MigrationRecord,
) -> Result<(), String> {
    let (observations, reconnected) = integration_observations(plan, record, None);
    record.integration_status = observations;
    record.reconnected_profiles = reconnected;
    Ok(())
}
fn integration_observations(
    plan: &MigrationPlan,
    record: &MigrationRecord,
    historical_image: Option<&ExecutableIdentity>,
) -> (Vec<MigrationIntegrationStatus>, Vec<MigrationProfilePaths>) {
    let mut observations = Vec::new();
    let mut reconnected = Vec::new();
    for profile in &plan.profiles {
        let live = committed_profile_image(plan, record, profile).and_then(|image| {
            let live = status(profile)?;
            verify_neutral_profile(historical_image.unwrap_or(image), profile, &live)?;
            Ok(live)
        });
        if profile.herdr_socket.is_some() {
            let result = live.as_ref().map_err(Clone::clone).and_then(|live| {
                if live.host_connections.get("herdr").copied().unwrap_or(0) > 0 {
                    Ok(())
                } else {
                    Err(format!(
                        "Herdr adapter has not connected to configured socket {}",
                        profile.herdr_socket.as_ref().unwrap().display()
                    ))
                }
            });
            observations.push(observation(profile, "herdr", result));
        }
        if profile.tern_reconnect_required {
            let result = live
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|live| verify_confirmed_handover(plan, profile, live));
            if result.is_ok() {
                reconnected.push(MigrationProfilePaths {
                    config_dir: profile.config_dir.clone(),
                    state_dir: profile.state_dir.clone(),
                });
            }
            observations.push(observation(profile, "tern", result));
        }
    }
    (observations, reconnected)
}
fn observation(
    profile: &LegacyProfile,
    provider: &str,
    result: Result<(), String>,
) -> MigrationIntegrationStatus {
    MigrationIntegrationStatus {
        config_dir: profile.config_dir.clone(),
        state_dir: profile.state_dir.clone(),
        provider: provider.into(),
        connected: result.is_ok(),
        error: result.err(),
    }
}
/// Forward-only settlement of an already committed/retired operation. No manager,
/// launch, source Stop or mutable preference is executed here.
pub fn refresh_committed_integrations(plan: &MigrationPlan) -> Result<MigrationRecord, String> {
    validate_plan_structure(plan)?;
    let record = read_record(plan)?;
    if !matches!(
        record.phase,
        MigrationPhase::Complete | MigrationPhase::AwaitingReconnect
    ) {
        return Ok(record);
    }
    let profile = &plan.profiles[0];
    let live = status(profile)?;
    let image = ExecutableIdentity {
        path: live.executable_path.clone(),
        sha256: live.running_sha256.clone(),
    };
    let context = runtime_context(profile, &live, image, profile.environment.clone())?;
    observe_for_context(plan, &context)
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum OwnerUpgradeProof {
    Updater {
        operation_id: String,
        plan_sha256: String,
        result_sha256: String,
    },
    InstalledOwner {
        installation: crate::installation::InstallationRecord,
        previous_image: ExecutableIdentity,
    },
}
fn owner_upgrade(
    plan: &MigrationPlan,
    context: &UpdateContext,
) -> Result<Option<OwnerUpgradeProof>, String> {
    let Some(result) = protocol::latest_operation(&context.state_dir)? else {
        return installed_owner_upgrade(plan, context);
    };
    if result.version != protocol::UPDATER_PROTOCOL {
        return installed_owner_upgrade(plan, context);
    }
    let prior = crate::read_plan(&context.state_dir, &result.operation_id)?;
    if &prior.running != target_image(plan, &read_record(plan)?)? {
        return installed_owner_upgrade(plan, context);
    }
    let _lease = protocol::lock_operation(&context.state_dir, &result.operation_id)?;
    if result.phase != protocol::OperationPhase::Completed
        || !result.installed
        || !result.applied
        || result.execution_fence != protocol::ExecutionFence::ManagerExited
        || result.candidate.as_ref() != Some(&context.running)
        || prior.context.config_dir != context.config_dir
        || prior.context.state_dir != context.state_dir
        || crate::physical_install_root(prior.target_origin())?
            != crate::physical_install_root(&plan.target.origin)?
        || context.running_origin.as_deref() != Some(prior.target_origin())
        || context.version != crate::identity::neutral::VERSION
        || executable_identity(&context.running.path)? != context.running
    {
        return Err("retained migration owner upgrade is not a settled exact-image/profile/installation cutover".into());
    }
    let directory = context.state_dir.join("updates-v4");
    Ok(Some(OwnerUpgradeProof::Updater {
        operation_id: result.operation_id.clone(),
        plan_sha256: format!(
            "{:x}",
            Sha256::digest(owned_private_file(
                &directory.join(format!("{}.plan.json", result.operation_id)),
                MAX_PLAN
            )?)
        ),
        result_sha256: format!(
            "{:x}",
            Sha256::digest(owned_private_file(
                &directory.join(format!("{}.result.json", result.operation_id)),
                MAX_FRAME as u64
            )?)
        ),
    }))
}
fn installed_owner_upgrade(
    plan: &MigrationPlan,
    context: &UpdateContext,
) -> Result<Option<OwnerUpgradeProof>, String> {
    if context.version != crate::identity::neutral::VERSION {
        return Ok(None);
    }
    let installed = crate::installation::inspect(&context.running.path)?;
    if installed.image != context.running
        || installed.app_version != context.version
        || context.running_origin.as_deref() != Some(&installed.origin)
        || crate::physical_install_root(&installed.origin)?
            != crate::physical_install_root(&plan.target.origin)?
    {
        return Ok(None);
    }
    protocol::start_allowed(&installed.origin, &context.state_dir, None, None)?;
    // Real current owner receipts plus the retained signed old image attest the
    // external upgrade. Neither an arbitrary current version nor an ACK suffices.
    let staged_image = plan.target.staged_bundle.join("Contents/MacOS/desktop-pet");
    let mut previous = None;
    for candidate in [&staged_image, &plan.target.image.path] {
        if !candidate.is_file() {
            continue;
        }
        let identity = executable_identity(candidate)?;
        if identity.sha256 != plan.target.image.sha256 {
            continue;
        }
        let bundle = candidate
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .ok_or("retained target bundle has no envelope")?;
        let app = crate::installation::validate_staged_app(bundle)?;
        if app.version == plan.target.release.version
            && app.image.sha256 == plan.target.image.sha256
        {
            previous = Some(app.image);
            break;
        }
    }
    let previous_image = previous.ok_or("owner upgrade is installed, but the original committed migration image has no retained signed stage/keg; original evidence was preserved")?;
    Ok(Some(OwnerUpgradeProof::InstalledOwner {
        installation: installed,
        previous_image,
    }))
}
pub(super) fn retained_owner_matches(
    plan: &MigrationPlan,
    context: &UpdateContext,
) -> Result<bool, String> {
    // Cheap history selection only; observe_for_context authenticates the actual
    // installed owner/upgrade once before it creates any settlement receipt.
    if context.version != crate::identity::neutral::VERSION {
        return Ok(false);
    }
    let Some(origin) = context.running_origin.as_deref() else {
        return Ok(false);
    };
    Ok(crate::physical_install_root(origin)? == crate::physical_install_root(&plan.target.origin)?)
}
#[derive(Serialize)]
struct RetainedSettlement<'a> {
    version: u32,
    operation_id: &'a str,
    plan_sha256: &'a str,
    original_record_sha256: &'a str,
    current_image: &'a ExecutableIdentity,
    config_dir: &'a Path,
    state_dir: &'a Path,
    owner_upgrade: OwnerUpgradeProof,
    current_instance_id: &'a str,
    current_pid: u32,
    stop_generation: u64,
    integration_status: &'a [MigrationIntegrationStatus],
    integration_delivery_error: Option<&'a str>,
}
/// Historical plan/journal bytes stay immutable. A distinct current-version
/// receipt settles app completion only after authenticating the owner upgrade.
pub fn observe_for_context(
    plan: &MigrationPlan,
    context: &UpdateContext,
) -> Result<MigrationRecord, String> {
    validate_plan_structure(plan)?;
    let mut record = read_record(plan)?;
    if !matches!(
        record.phase,
        MigrationPhase::Complete | MigrationPhase::AwaitingReconnect
    ) {
        return Ok(record);
    }
    let _lease = match protocol::lock_operation(&plan.profiles[0].state_dir, &plan.operation_id) {
        Ok(lease) => lease,
        Err(error) if error.starts_with("installation busy: ") => return Ok(record),
        Err(error) => return Err(error),
    };
    let selected = plan
        .profiles
        .iter()
        .find(|profile| {
            profile.config_dir == context.config_dir && profile.state_dir == context.state_dir
        })
        .ok_or("retained migration profile differs")?;
    verify_retained_commit(plan, &record, selected)?;
    let live = status(selected)?;
    verify_neutral_profile(&context.running, selected, &live)?;
    if live.instance_id != context.instance_id {
        return Err("current runtime instance changed during retained settlement".into());
    }
    let origin = context
        .running_origin
        .as_deref()
        .ok_or("current owner origin is unavailable")?;
    let generation = protocol::verified_runtime_start_fence(
        origin,
        &context.state_dir,
        live.pid,
        &context.instance_id,
        &context.running,
    )?;
    let owner = owner_upgrade(plan, context)?
        .ok_or("retained migration has no authenticated successor owner-upgrade proof")?;
    let original_record_sha256 = format!(
        "{:x}",
        Sha256::digest(owned_private_file(&journal_path(plan), MAX_PLAN)?)
    );
    // Integration-only view: never persisted as a plan or given to a migration
    // executor, manager, native Prepare/Stop/Commit, or launcher.
    let mut current = plan.clone();
    current.profiles = vec![selected.clone()];
    current.profiles[0].environment = context.environment.clone();
    current.profiles[0].stop_generation = generation;
    current.profiles[0].tern_reconnect_required |=
        live.host_connections.get("tern").copied().unwrap_or(0) > 0;
    current.tern_reconnect_required = current.profiles[0].tern_reconnect_required;
    current.tern_delivery_problem = None;
    current.target.image = context.running.clone();
    current.target.release.version = context.version.clone();
    current.target.staged_bundle = context
        .running
        .path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or("current verified target has no application envelope")?
        .to_path_buf();
    deliver_forward(
        plan,
        &mut current,
        &mut record,
        &owner,
        &original_record_sha256,
        generation,
    );
    let mut observed = plan.clone();
    for profile in &mut observed.profiles {
        if profile.config_dir == selected.config_dir && profile.state_dir == selected.state_dir {
            *profile = current.profiles[0].clone();
        }
    }
    let (observations, reconnected) =
        integration_observations(&observed, &record, Some(&context.running));
    record.integration_status = observations;
    record.reconnected_profiles = reconnected;
    if migration_stop_generation(&context.state_dir)? != generation {
        return Err("user Stop superseded retained integration settlement".into());
    }
    record.phase = MigrationPhase::Complete;
    let receipt = RetainedSettlement {
        version: 1,
        operation_id: &plan.operation_id,
        plan_sha256: &record.plan_sha256,
        original_record_sha256: &original_record_sha256,
        current_image: &context.running,
        config_dir: &context.config_dir,
        state_dir: &context.state_dir,
        current_instance_id: &context.instance_id,
        current_pid: live.pid,
        stop_generation: generation,
        owner_upgrade: owner,
        integration_status: &record.integration_status,
        integration_delivery_error: record.integration_delivery_error.as_deref(),
    };
    protocol::write_private_bytes(
        &context
            .state_dir
            .join("updates-v4")
            .join(format!("{}.migration-settlement.json", plan.operation_id)),
        &serde_json::to_vec(&receipt).map_err(|e| e.to_string())?,
        0o600,
    )?;
    Ok(record)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfilePlan {
    version: u32,
    operation_id: String,
    plan_path: PathBuf,
    plan_sha256: String,
    config_dir: PathBuf,
    state_dir: PathBuf,
}
pub(super) fn profile_plan_path(
    path: &Path,
    config: &Path,
    state: &Path,
) -> Result<PathBuf, String> {
    let pointer: ProfilePlan =
        serde_json::from_slice(&owned_private_file(path, 16384)?).map_err(|e| e.to_string())?;
    protocol::validate_operation_id(&pointer.operation_id)?;
    crate::ensure_absolute(&pointer.plan_path)?;
    if pointer.version != 1
        || pointer.config_dir != config
        || pointer.state_dir != state
        || !hex(&pointer.plan_sha256, 64)
        || path
            != state
                .join("updates-v4")
                .join(format!("{}.migration-profile.json", pointer.operation_id))
    {
        return Err(
            "migration profile pointer does not select this exact approved original pair".into(),
        );
    }
    let bytes = owned_private_file(&pointer.plan_path, MAX_PLAN)?;
    if format!("{:x}", Sha256::digest(&bytes)) != pointer.plan_sha256 {
        return Err("migration profile pointer plan digest changed".into());
    }
    let plan: MigrationPlan = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    validate_plan_structure(&plan)?;
    if pointer.operation_id != plan.operation_id
        || pointer.plan_path != evidence_path(&plan)
        || !plan
            .profiles
            .iter()
            .any(|profile| profile.config_dir == config && profile.state_dir == state)
    {
        return Err("migration profile pointer lacks original approval scope".into());
    }
    Ok(pointer.plan_path)
}
pub(super) fn retain_profile_plans(
    plan: &MigrationPlan,
    record: &MigrationRecord,
) -> Result<(), String> {
    for profile in &plan.profiles {
        ensure_private_directory(&profile.state_dir.join("updates-v4"))?;
        let pointer = ProfilePlan {
            version: 1,
            operation_id: plan.operation_id.clone(),
            plan_path: evidence_path(plan),
            plan_sha256: record.plan_sha256.clone(),
            config_dir: profile.config_dir.clone(),
            state_dir: profile.state_dir.clone(),
        };
        let path = profile
            .state_dir
            .join("updates-v4")
            .join(format!("{}.migration-profile.json", plan.operation_id));
        let bytes = serde_json::to_vec(&pointer).map_err(|e| e.to_string())?;
        if path.try_exists().map_err(|e| e.to_string())? {
            if owned_private_file(&path, 16384)? != bytes {
                return Err("original migration profile pointer changed".into());
            }
        } else {
            protocol::write_private_new(&path, &bytes, 0o600)?;
        }
    }
    Ok(())
}
fn runtime_context(
    profile: &LegacyProfile,
    live: &LiveStatus,
    image: ExecutableIdentity,
    environment: BTreeMap<String, String>,
) -> Result<UpdateContext, String> {
    let installed = crate::installation::inspect(&image.path)?;
    if installed.image != image {
        return Err("current migration owner image changed".into());
    }
    Ok(UpdateContext {
        executable: image.path.clone(),
        version: live.app_version.clone(),
        instance_id: live.instance_id.clone(),
        config_dir: profile.config_dir.clone(),
        state_dir: profile.state_dir.clone(),
        #[cfg(desktop_pet_legacy)]
        host_plugin_config_dir: profile.host_plugin_config_dir.clone(),
        #[cfg(desktop_pet_legacy)]
        herdr_socket: profile
            .herdr_socket
            .clone()
            .ok_or("legacy source host provenance is unavailable")?,
        running: image,
        running_origin: Some(Box::new(installed.origin)),
        assets_override: profile.assets_override.clone(),
        environment,
        locale: "en".into(),
    })
}
struct RouteAuthority {
    generation: u64,
    owner: Option<OwnerUpgradeProof>,
    original_record_sha256: Option<String>,
}
fn route_authority(
    plan: &MigrationPlan,
    record: &MigrationRecord,
    profile: &LegacyProfile,
    live: &LiveStatus,
) -> Result<RouteAuthority, String> {
    let committed = committed_profile_image(plan, record, profile)?;
    let image = ExecutableIdentity {
        path: live.executable_path.clone(),
        sha256: live.running_sha256.clone(),
    };
    if record.target_profiles.is_empty()
        && (record.target_pid.is_none()
            || record
                .target_instance
                .as_deref()
                .is_none_or(|instance| !instance_valid(instance)))
    {
        return Err(
            "historical migration has no retained original target PID/instance Commit evidence"
                .into(),
        );
    }
    if !record.native_ready || !record.target_committed {
        return Err("migration target has no certain successful durable Commit authority".into());
    }
    verify_target_manager_result(plan, record)?;
    if matches!(
        record.phase,
        MigrationPhase::Integrating | MigrationPhase::Retiring | MigrationPhase::RetirementFailed
    ) {
        if committed != &image || stop_requested(plan)? {
            return Err("fresh committed target/profile/instance or Stop authority changed".into());
        }
        verify_committed_profile(plan, record, profile, live)?;
        let generation = protocol::verified_runtime_start_fence(
            &plan.target.origin,
            &profile.state_dir,
            live.pid,
            &live.instance_id,
            &image,
        )?;
        if generation != profile.stop_generation {
            return Err("user Stop superseded the original migration approval".into());
        }
        // This committed/pre-retirement proof deliberately does not call retired().
        return Ok(RouteAuthority {
            generation,
            owner: None,
            original_record_sha256: None,
        });
    }
    verify_retained_commit(plan, record, profile)?;
    verify_neutral_profile(&image, profile, live)?;
    let context = runtime_context(profile, live, image, profile.environment.clone())?;
    let owner = owner_upgrade(plan, &context)?
        .ok_or("retained route has no authenticated current installed owner")?;
    let generation = protocol::verified_runtime_start_fence(
        context
            .running_origin
            .as_deref()
            .ok_or("current owner is unavailable")?,
        &profile.state_dir,
        live.pid,
        &live.instance_id,
        &context.running,
    )?;
    Ok(RouteAuthority {
        generation,
        owner: Some(owner),
        original_record_sha256: Some(format!(
            "{:x}",
            Sha256::digest(owned_private_file(&journal_path(plan), MAX_PLAN)?)
        )),
    })
}
fn check_handover_stop(
    plan: &MigrationPlan,
    profile: &LegacyProfile,
    authority: &RouteAuthority,
) -> Result<(), String> {
    if migration_stop_generation(&profile.state_dir)? != authority.generation
        || (authority.owner.is_none() && stop_requested(plan)?)
    {
        return Err("user Stop superseded Tern handover authority".into());
    }
    Ok(())
}
fn authorized_route(
    plan: &MigrationPlan,
    executable: &Path,
    config: &Path,
    state: &Path,
) -> Result<bool, String> {
    Ok(executable == plan.target.image.path
        || tern_config::approved_saved_route(plan, executable, config, state)?
        || ancestry::retained_source_route(plan, executable, config, state)?.is_some())
}
fn authenticated_delivery(
    plan: &MigrationPlan,
    record: &MigrationRecord,
    profile: &LegacyProfile,
    image: &ExecutableIdentity,
    authority: &RouteAuthority,
) -> Result<(PathBuf, DeliveryResult), String> {
    let path = if authority.owner.is_some() {
        forward_path(plan, &profile.state_dir, image)
    } else {
        delivery_path_for(plan, &profile.state_dir)
    };
    let delivery = previous_delivery(
        &path,
        plan,
        record,
        image,
        authority.owner.as_ref(),
        authority.original_record_sha256.as_deref(),
        &profile.state_dir,
        authority.generation,
    )?
    .ok_or("approved Tern consumer delivery has not started")?;
    if let Some(error) = delivery_error(&delivery) {
        return Err(error);
    }
    let connection = &delivery.delivery.configuration.connection;
    if connection.config_dir != profile.config_dir || connection.state_dir != profile.state_dir {
        return Err("Tern consumer delivery belongs to another original profile".into());
    }
    if authority.owner.is_none()
        && !plan
            .tern_deliveries
            .iter()
            .chain(plan.tern_delivery.iter())
            .any(|approved| approved == &delivery.delivery)
    {
        return Err("Tern delivery differs from original approved package/CLI/routing".into());
    }
    if authority.owner.is_some() {
        let bundle = image
            .path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .ok_or("current owner has no signed application envelope")?;
        if delivery.delivery.package
            != bundle.join("Contents/Resources/integrations/tern-desktop-pet")
        {
            return Err(
                "forward Tern package is not inside the authenticated current owner".into(),
            );
        }
    }
    verify_installed_delivery(&delivery.delivery)?;
    Ok((path, delivery))
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct Handover {
    operation_id: String,
    plan_path: PathBuf,
    plan_sha256: String,
    token: String,
    consumer_revision: String,
    target_instance_id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HandoverChallenge {
    version: u32,
    handover: Handover,
    original: TernConnection,
    target: TernConnection,
    current_image: ExecutableIdentity,
    stop_generation: u64,
    delivery_path: PathBuf,
    delivery_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HandoverReceipt {
    version: u32,
    operation_id: String,
    plan_sha256: String,
    challenge_sha256: String,
    current_image: ExecutableIdentity,
    config_dir: PathBuf,
    state_dir: PathBuf,
    target_instance_id: String,
    window_id: String,
    stop_generation: u64,
    binding_epoch: u64,
    sequence: u64,
    observation_generation: u64,
    native_generation: u64,
    consumer_revision: String,
    package_sha256: String,
}
fn handover_path(
    plan: &MigrationPlan,
    state: &Path,
    image: &ExecutableIdentity,
    confirmed: bool,
) -> PathBuf {
    state.join("updates-v4").join(format!(
        "{}.tern-{}-{}.json",
        plan.operation_id,
        if confirmed { "confirmed" } else { "handover" },
        image.sha256
    ))
}
fn issue_handover(
    plan: &MigrationPlan,
    record: &MigrationRecord,
    profile: &LegacyProfile,
    live: &LiveStatus,
    original: &TernConnection,
    target: &TernConnection,
    authority: &RouteAuthority,
) -> Result<Handover, String> {
    let image = ExecutableIdentity {
        path: live.executable_path.clone(),
        sha256: live.running_sha256.clone(),
    };
    let (delivery_path, delivery) =
        authenticated_delivery(plan, record, profile, &image, authority)?;
    if original != &delivery.delivery.configuration.connection && original != target {
        return Err(
            "Tern saved route changed from the captured exact original SDK identity".into(),
        );
    }
    check_handover_stop(plan, profile, authority)?;
    let delivery_sha256 = format!(
        "{:x}",
        Sha256::digest(owned_private_file(&delivery_path, MAX_PLAN)?)
    );
    let path = handover_path(plan, &profile.state_dir, &image, false);
    if path.try_exists().map_err(|e| e.to_string())? {
        let previous: HandoverChallenge =
            serde_json::from_slice(&owned_private_file(&path, MAX_PLAN)?)
                .map_err(|e| e.to_string())?;
        if previous.version == 1
            && previous.original == *original
            && previous.target == *target
            && previous.handover.operation_id == plan.operation_id
            && previous.handover.plan_path == evidence_path(plan)
            && previous.handover.plan_sha256 == record.plan_sha256
            && previous.handover.consumer_revision == delivery.delivery.revision
            && hex(&previous.handover.token, 64)
            && previous.current_image == image
            && previous.handover.target_instance_id == live.instance_id
            && previous.stop_generation == authority.generation
            && previous.delivery_path == delivery_path
            && previous.delivery_sha256 == delivery_sha256
        {
            return Ok(previous.handover);
        }
    }
    let handover = Handover {
        operation_id: plan.operation_id.clone(),
        plan_path: evidence_path(plan),
        plan_sha256: record.plan_sha256.clone(),
        token: random_hex(32)?,
        consumer_revision: delivery.delivery.revision,
        target_instance_id: live.instance_id.clone(),
    };
    let challenge = HandoverChallenge {
        version: 1,
        handover: handover.clone(),
        original: original.clone(),
        target: target.clone(),
        current_image: image,
        stop_generation: authority.generation,
        delivery_path,
        delivery_sha256,
    };
    check_handover_stop(plan, profile, authority)?;
    protocol::write_private_bytes(
        &path,
        &serde_json::to_vec(&challenge).map_err(|e| e.to_string())?,
        0o600,
    )?;
    Ok(handover)
}
fn fresh_binding(
    profile: &LegacyProfile,
    instance: &str,
    window: &str,
) -> Result<TernBindingEvidence, String> {
    let binding: TernBindingEvidence = serde_json::from_slice(&exchange_timeout(
        &profile.control_socket,
        &serde_json::json!({"version":1,"kind":"tern-migration-evidence","window_id":window}),
        Duration::from_millis(750),
    )?)
    .map_err(|e| e.to_string())?;
    if binding.version != 1
        || binding.kind != "tern-migration-evidence"
        || !binding.fresh
        || binding.instance_id != instance
        || binding.window_id != window
        || binding.window_id.is_empty()
        || binding.binding_epoch == 0
        || binding.sequence == 0
        || binding.local_host.is_empty()
        || binding.observation_generation == 0
        || binding.native_generation == 0
    {
        return Err(
            "Tern has no actual fresh same-window binding to the exact committed target instance"
                .into(),
        );
    }
    Ok(binding)
}
fn verify_confirmed_handover(
    plan: &MigrationPlan,
    profile: &LegacyProfile,
    live: &LiveStatus,
) -> Result<(), String> {
    let image = ExecutableIdentity {
        path: live.executable_path.clone(),
        sha256: live.running_sha256.clone(),
    };
    let receipt: HandoverReceipt = serde_json::from_slice(&owned_private_file(
        &handover_path(plan, &profile.state_dir, &image, true),
        MAX_PLAN,
    )?)
    .map_err(|e| e.to_string())?;
    let bytes = owned_private_file(
        &handover_path(plan, &profile.state_dir, &image, false),
        MAX_PLAN,
    )?;
    let challenge: HandoverChallenge = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let generation = migration_stop_generation(&profile.state_dir)?;
    let record = read_record(plan)?;
    if receipt.version != 1
        || receipt.operation_id != plan.operation_id
        || receipt.plan_sha256 != record.plan_sha256
        || challenge.version != 1
        || challenge.handover.operation_id != plan.operation_id
        || challenge.handover.plan_path != evidence_path(plan)
        || challenge.handover.plan_sha256 != record.plan_sha256
        || receipt.challenge_sha256 != format!("{:x}", Sha256::digest(&bytes))
        || receipt.current_image != image
        || receipt.config_dir != profile.config_dir
        || receipt.state_dir != profile.state_dir
        || receipt.target_instance_id != live.instance_id
        || receipt.stop_generation != generation
        || challenge.stop_generation != generation
        || challenge.current_image != image
        || challenge.handover.target_instance_id != live.instance_id
        || challenge.target.config_dir != profile.config_dir
        || challenge.target.state_dir != profile.state_dir
        || challenge.target.executable != image.path
        || challenge.original.config_dir != profile.config_dir
        || challenge.original.state_dir != profile.state_dir
    {
        return Err(
            "Tern handover receipt is stale or belongs to another profile/instance/Stop generation"
                .into(),
        );
    }
    let delivery: DeliveryResult =
        serde_json::from_slice(&owned_private_file(&challenge.delivery_path, MAX_PLAN)?)
            .map_err(|e| e.to_string())?;
    if delivery_error(&delivery).is_some()
        || delivery.operation_id != plan.operation_id
        || delivery.plan_sha256 != record.plan_sha256
        || delivery.current_image != image
        || delivery.state_dir != profile.state_dir
        || receipt.consumer_revision != delivery.delivery.revision
        || receipt.package_sha256 != delivery.delivery.package_sha256
        || challenge.handover.consumer_revision != delivery.delivery.revision
        || (challenge.delivery_path != delivery_path_for(plan, &profile.state_dir)
            && challenge.delivery_path != forward_path(plan, &profile.state_dir, &image))
        || format!(
            "{:x}",
            Sha256::digest(owned_private_file(&challenge.delivery_path, MAX_PLAN)?)
        ) != challenge.delivery_sha256
    {
        return Err("Tern handover has no unchanged settled signed-package delivery intent".into());
    }
    verify_installed_delivery(&delivery.delivery)?;
    tern_config::verify_canonical_target(&delivery.delivery.configuration, &challenge.target)?;
    let binding = fresh_binding(profile, &live.instance_id, &receipt.window_id)?;
    if binding.binding_epoch < receipt.binding_epoch || binding.sequence < receipt.sequence {
        return Err("Tern binding predates its actual handover receipt".into());
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteRequest {
    version: u32,
    kind: String,
    executable: PathBuf,
    config_dir: PathBuf,
    state_dir: PathBuf,
}
/// Route proof never invokes a manager, launcher, Stop or migration replay. The
/// only new bytes are bounded per-profile handover evidence, never old journals.
pub fn committed_route(
    config: &Path,
    state: &Path,
    bytes: &[u8],
) -> Result<serde_json::Value, String> {
    let request: RouteRequest = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if request.version != 1
        || request.kind != "legacy-migration-route"
        || request.config_dir != config
        || request.state_dir != state
    {
        return Err("migration route request does not select the original exact profile".into());
    }
    owned_directory(config)?;
    owned_directory(state)?;
    let directory = state.join("updates-v4");
    owned_directory(&directory)?;
    let mut count = 0;
    let mut seen = std::collections::BTreeSet::new();
    for entry in fs::read_dir(&directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = if name.ends_with(".migration-plan.json") {
            entry.path()
        } else if name.ends_with(".migration-profile.json") {
            profile_plan_path(&entry.path(), config, state)?
        } else {
            continue;
        };
        count += 1;
        if count > 1024 {
            return Err("migration route history exceeds bound".into());
        }
        if !seen.insert(path.clone()) {
            continue;
        }
        let plan: MigrationPlan = serde_json::from_slice(&owned_private_file(&path, MAX_PLAN)?)
            .map_err(|e| e.to_string())?;
        validate_plan_structure(&plan)?;
        if path != evidence_path(&plan) {
            return Err("migration route evidence name differs from original scope".into());
        }
        let Some(profile) = plan
            .profiles
            .iter()
            .find(|profile| profile.config_dir == config && profile.state_dir == state)
        else {
            continue;
        };
        let saved_ancestry = authorized_route(&plan, &request.executable, config, state)?;
        let record = read_record(&plan)?;
        if !record.native_ready || !record.target_committed {
            continue;
        }
        let live = status(profile)?;
        // An already-current canonical path is not success by itself: it still
        // passes the complete original Commit/current owner/runtime proof below.
        if !saved_ancestry && request.executable != live.executable_path {
            continue;
        }
        let authority = route_authority(&plan, &record, profile, &live)?;
        let original = TernConnection {
            version: 1,
            executable: request.executable.clone(),
            config_dir: config.to_path_buf(),
            state_dir: state.to_path_buf(),
        };
        let target = TernConnection {
            version: 1,
            executable: live.executable_path.clone(),
            config_dir: config.to_path_buf(),
            state_dir: state.to_path_buf(),
        };
        let mut response = serde_json::json!({"version":1,"kind":"legacy-migration-route","committed":true,"original":original,"target":target});
        let handover = issue_handover(
            &plan, &record, profile, &live, &original, &target, &authority,
        )?;
        response
            .as_object_mut()
            .expect("route response object")
            .insert(
                "handover".into(),
                serde_json::to_value(handover).map_err(|e| e.to_string())?,
            );
        return Ok(response);
    }
    Err("no authenticated committed migration preserves this saved executable and original profile pair".into())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HandoverRequest {
    version: u32,
    kind: String,
    loaded_protocol: u32,
    handover: Handover,
    original: TernConnection,
    target: TernConnection,
    window_id: String,
}
fn validate_handover_request(
    request: &HandoverRequest,
    config: &Path,
    state: &Path,
) -> Result<(), String> {
    let handover = &request.handover;
    if request.version != 1
        || request.kind != "migration-integration-handover"
        || request.loaded_protocol != 1
        || !hex(&handover.operation_id, 32)
        || !hex(&handover.plan_sha256, 64)
        || !hex(&handover.token, 64)
        || !hex(&handover.consumer_revision, 40)
        || handover.target_instance_id.is_empty()
        || handover.target_instance_id.len() > 128
        || handover
            .plan_path
            .to_str()
            .is_none_or(|path| path.len() > 4096)
        || !handover.plan_path.is_absolute()
        || request.window_id.is_empty()
        || request.window_id.len() > 256
        || request.original.version != 1
        || request.target.version != 1
        || request.original.config_dir != config
        || request.target.config_dir != config
        || request.original.state_dir != state
        || request.target.state_dir != state
    {
        return Err(
            "Tern handover does not match the strict loaded-protocol/original-profile contract"
                .into(),
        );
    }
    Ok(())
}
/// Public bounded stdin entrypoint. Revision is authorized capability, not VM
/// bytecode attestation; installed bytes, SDK KV and native binding prove receipt.
pub fn integration_handover(
    config: &Path,
    state: &Path,
    bytes: &[u8],
) -> Result<serde_json::Value, String> {
    let request: HandoverRequest = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    validate_handover_request(&request, config, state)?;
    if crate::identity::LEGACY {
        return Err("legacy envelope has no migration handover authority".into());
    }
    let plan_bytes = owned_private_file(&request.handover.plan_path, MAX_PLAN)?;
    let plan: MigrationPlan = serde_json::from_slice(&plan_bytes).map_err(|e| e.to_string())?;
    validate_plan_structure(&plan)?;
    if request.handover.plan_path != evidence_path(&plan)
        || request.handover.operation_id != plan.operation_id
        || request.handover.plan_sha256 != format!("{:x}", Sha256::digest(&plan_bytes))
    {
        return Err("Tern handover differs from immutable original migration approval".into());
    }
    let record = read_record(&plan)?;
    let profile = plan
        .profiles
        .iter()
        .find(|profile| profile.config_dir == config && profile.state_dir == state)
        .ok_or("Tern handover profile was not in original approval")?;
    let live = status(profile)?;
    let image = ExecutableIdentity {
        path: live.executable_path.clone(),
        sha256: live.running_sha256.clone(),
    };
    let executing = executable_identity(&std::env::current_exe().map_err(|e| e.to_string())?)?;
    if executing != image
        || request.target.executable != image.path
        || request.handover.target_instance_id != live.instance_id
    {
        return Err("handover executing app/target instance differs from the current original-profile daemon".into());
    }
    let authority = route_authority(&plan, &record, profile, &live)?;
    let challenge_bytes =
        owned_private_file(&handover_path(&plan, state, &image, false), MAX_PLAN)?;
    let challenge: HandoverChallenge =
        serde_json::from_slice(&challenge_bytes).map_err(|e| e.to_string())?;
    if challenge.version != 1
        || challenge.handover != request.handover
        || challenge.original != request.original
        || challenge.target != request.target
        || challenge.current_image != image
        || challenge.stop_generation != authority.generation
    {
        return Err("Tern handover challenge/profile/instance changed".into());
    }
    let (delivery_path, delivery) =
        authenticated_delivery(&plan, &record, profile, &image, &authority)?;
    if delivery_path != challenge.delivery_path
        || request.handover.consumer_revision != delivery.delivery.revision
        || format!(
            "{:x}",
            Sha256::digest(owned_private_file(&delivery_path, MAX_PLAN)?)
        ) != challenge.delivery_sha256
    {
        return Err("Tern handover durable delivery intent changed".into());
    }
    tern_config::verify_canonical_target(&delivery.delivery.configuration, &request.target)?;
    let binding = fresh_binding(profile, &live.instance_id, &request.window_id)?;
    check_handover_stop(&plan, profile, &authority)?;
    let receipt = HandoverReceipt {
        version: 1,
        operation_id: plan.operation_id.clone(),
        plan_sha256: record.plan_sha256.clone(),
        challenge_sha256: format!("{:x}", Sha256::digest(&challenge_bytes)),
        current_image: image.clone(),
        config_dir: config.to_path_buf(),
        state_dir: state.to_path_buf(),
        target_instance_id: live.instance_id.clone(),
        window_id: request.window_id.clone(),
        stop_generation: authority.generation,
        binding_epoch: binding.binding_epoch,
        sequence: binding.sequence,
        observation_generation: binding.observation_generation,
        native_generation: binding.native_generation,
        consumer_revision: delivery.delivery.revision,
        package_sha256: delivery.delivery.package_sha256,
    };
    protocol::write_private_bytes(
        &handover_path(&plan, state, &image, true),
        &serde_json::to_vec(&receipt).map_err(|e| e.to_string())?,
        0o600,
    )?;
    Ok(
        serde_json::json!({"version":1,"kind":"migration-integration-handover","confirmed":true,
        "window_id":request.window_id,"target_instance_id":live.instance_id}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn delivery_fixture() -> (tempfile::TempDir, MigrationPlan) {
        let (temp, mut plan) = crate::migration::tests::fixture();
        plan.tern_reconnect_required = true;
        plan.profiles[0].tern_reconnect_required = true;
        let tern_root = plan.profiles[0]
            .state_dir
            .join("fixture-tern-config")
            .to_string_lossy()
            .into_owned();
        plan.profiles[0]
            .environment
            .insert("TERN_CONFIG_DIR".into(), tern_root);
        let package = plan
            .target
            .staged_bundle
            .join("Contents/Resources/integrations/tern-desktop-pet");
        ensure_private_directory(&package).unwrap();
        for (name, bytes) in [
            ("plugin.toml", b"schema = 1\nid = \"tern-desktop-pet\"\nversion = \"0.2.1\"\nwindow = \"window.luau\"\n".to_vec()),
            ("window.luau", b"-- fixture consumer\n".to_vec()),
            ("LICENSE.txt", b"fixture license\n".to_vec()),
            ("revision", format!("{}\n", "a".repeat(40)).into_bytes()),
        ] { protocol::write_private_new(&package.join(name), &bytes, 0o600).unwrap(); }
        let (package_sha256, revision) = package_identity(&package).unwrap();
        plan.tern_delivery = Some(TernDelivery {
            program: executable_identity(Path::new("/usr/bin/true")).unwrap(),
            package,
            package_sha256,
            revision,
            environment: neutral_environment(&plan.profiles[0]),
            configuration: tern_config::fixture_configuration(&plan),
            installed_package: None,
            daemon_socket: None,
        });
        (temp, plan)
    }

    #[test]
    fn changed_consumer_bytes_are_not_installed_or_misreported_as_reconnected() {
        let (_temp, plan) = delivery_fixture();
        let delivery = plan.tern_delivery.as_ref().unwrap();
        protocol::write_private_bytes(
            &delivery.package.join("window.luau"),
            b"changed source",
            0o600,
        )
        .unwrap();
        let mut record = make_record(&plan, &serde_json::to_vec(&plan).unwrap());
        record.phase = MigrationPhase::Complete;
        record.native_ready = true;
        record.target_committed = true;
        record.target_pid = Some(2000);
        record.target_instance = Some("target-fixture".into());
        deliver_tern(&plan, &mut record);
        assert_eq!(record.phase, MigrationPhase::Complete);
        assert!(record
            .integration_delivery_error
            .as_deref()
            .unwrap()
            .contains("changed"));
        assert!(record.reconnected_profiles.is_empty());
        assert!(!delivery_path(&plan).exists());
    }

    #[test]
    fn changed_saved_connection_prevents_delivery_without_overwriting_sdk_kv() {
        let (_temp, plan) = delivery_fixture();
        let configuration = &plan.tern_delivery.as_ref().unwrap().configuration;
        let changed = br#"{"connection":{"version":1,"executable":"/user/changed","config_dir":"/user/config","state_dir":"/user/state"}}"#;
        protocol::write_private_bytes(&configuration.path, changed, 0o600).unwrap();
        let mut record = make_record(&plan, &serde_json::to_vec(&plan).unwrap());
        record.phase = MigrationPhase::Complete;
        record.native_ready = true;
        record.target_committed = true;
        record.target_pid = Some(2000);
        record.target_instance = Some("target-fixture".into());
        deliver_tern(&plan, &mut record);
        assert!(record
            .integration_delivery_error
            .as_deref()
            .unwrap()
            .contains("saved connection changed"));
        assert_eq!(fs::read(&configuration.path).unwrap(), changed);
        assert!(!delivery_path(&plan).exists());
        assert!(record.reconnected_profiles.is_empty());
    }

    #[test]
    fn unknown_delivery_intent_is_preserved_and_never_replayed() {
        let (_temp, plan) = delivery_fixture();
        let mut record = make_record(&plan, &serde_json::to_vec(&plan).unwrap());
        record.native_ready = true;
        record.target_committed = true;
        record.target_pid = Some(2000);
        record.target_instance = Some("target-fixture".into());
        let intent = serde_json::to_vec(&DeliveryResult {
            version: 1,
            operation_id: plan.operation_id.clone(),
            plan_sha256: record.plan_sha256.clone(),
            plan_path: Some(evidence_path(&plan)),
            original_record_sha256: None,
            current_image: plan.target.image.clone(),
            owner_upgrade: None,
            state_dir: plan.profiles[0].state_dir.clone(),
            stop_generation: plan.profiles[0].stop_generation,
            delivery: plan.tern_delivery.as_ref().unwrap().clone(),
            intent: true,
            settled: false,
            error: None,
        })
        .unwrap();
        protocol::write_private_new(&delivery_path(&plan), &intent, 0o600).unwrap();
        record.phase = MigrationPhase::Complete;
        deliver_tern(&plan, &mut record);
        assert_eq!(record.phase, MigrationPhase::Complete);
        assert!(record.integration_delivery_error.is_some());
        assert_eq!(
            fs::read(delivery_path(&plan)).unwrap().as_slice(),
            intent.as_slice()
        );
        assert!(!plan.profiles[0]
            .state_dir
            .join("updates-v4")
            .join(format!("{}.tern-delivery.stdout", plan.operation_id))
            .exists());
        assert!(record.reconnected_profiles.is_empty());
    }

    #[test]
    fn retained_unknown_migration_is_observed_without_mutation_or_owner_replay() {
        let (_temp, plan) = crate::migration::tests::fixture();
        let mut record = crate::migration::tests::persist_fixture(&plan);
        record.phase = MigrationPhase::ManagerUnknown;
        record.error = Some("original actual uncertain manager result".into());
        save(&plan, &record).unwrap();
        let bytes = fs::read(journal_path(&plan)).unwrap();
        let context = crate::migration::tests::context(&plan);
        assert_eq!(observe_for_context(&plan, &context).unwrap(), record);
        assert_eq!(fs::read(journal_path(&plan)).unwrap(), bytes);
        assert!(!plan.profiles[0]
            .state_dir
            .join("updates-v4")
            .join(format!("{}.migration-settlement.json", plan.operation_id))
            .exists());
    }

    #[test]
    fn historical_pending_requires_real_successor_owner_proof_and_preserves_old_bytes() {
        let (_temp, mut plan) = crate::migration::tests::fixture();
        plan.target.release.version = "0.4.8".into();
        plan.target.release.tag = "v0.4.8".into();
        plan.target.release.asset_name = "DesktopPet-v0.4.8-macos-arm64.tar.gz".into();
        plan.target.release.archive_url = "https://github.com/hanbong5938/desktop-pet/releases/download/v0.4.8/DesktopPet-v0.4.8-macos-arm64.tar.gz".into();
        plan.target.release.checksum_url = format!("{}.sha256", plan.target.release.archive_url);
        let mut record = crate::migration::tests::persist_fixture(&plan);
        record.phase = MigrationPhase::AwaitingReconnect;
        record.native_ready = true;
        record.target_committed = true;
        save(&plan, &record).unwrap();
        let old_plan = fs::read(evidence_path(&plan)).unwrap();
        let old_record = fs::read(journal_path(&plan)).unwrap();
        let mut context = crate::migration::tests::context(&plan);
        context.version = crate::identity::neutral::VERSION.into();
        context.running = plan.target.image.clone();
        context.running.sha256 = "f".repeat(64);
        context.running_origin = Some(Box::new(plan.target.origin.clone()));
        assert!(observe_for_context(&plan, &context).is_err());
        assert_eq!(fs::read(evidence_path(&plan)).unwrap(), old_plan);
        assert_eq!(fs::read(journal_path(&plan)).unwrap(), old_record);
    }

    fn handover_request_fixture(plan: &MigrationPlan) -> serde_json::Value {
        let profile = &plan.profiles[0];
        serde_json::json!({"version":1,"kind":"migration-integration-handover","loaded_protocol":1,
            "handover":{"operation_id":plan.operation_id,"plan_path":evidence_path(plan),"plan_sha256":"a".repeat(64),
                "token":"b".repeat(64),"consumer_revision":"c".repeat(40),"target_instance_id":"target-instance"},
            "original":{"version":1,"executable":plan.source_image.path,"config_dir":profile.config_dir,"state_dir":profile.state_dir},
            "target":{"version":1,"executable":plan.target.image.path,"config_dir":profile.config_dir,"state_dir":profile.state_dir},
            "window_id":"actual-window"})
    }
    #[test]
    fn handover_wire_rejects_wrong_profile_window_protocol_and_unknown_fields() {
        let (_temp, plan) = crate::migration::tests::fixture();
        let original = handover_request_fixture(&plan);
        let parse = |value| serde_json::from_value::<HandoverRequest>(value);
        validate_handover_request(
            &parse(original.clone()).unwrap(),
            &plan.profiles[0].config_dir,
            &plan.profiles[0].state_dir,
        )
        .unwrap();
        for (field, value) in [
            ("window_id", serde_json::json!("")),
            ("loaded_protocol", serde_json::json!(2)),
        ] {
            let mut changed = original.clone();
            changed[field] = value;
            assert!(validate_handover_request(
                &parse(changed).unwrap(),
                &plan.profiles[0].config_dir,
                &plan.profiles[0].state_dir
            )
            .is_err());
        }
        let mut changed = original.clone();
        changed["target"]["state_dir"] = serde_json::json!("/another/profile");
        assert!(validate_handover_request(
            &parse(changed).unwrap(),
            &plan.profiles[0].config_dir,
            &plan.profiles[0].state_dir
        )
        .is_err());
        let mut changed = original.clone();
        changed["handover"]["token"] = serde_json::json!("B".repeat(64));
        assert!(validate_handover_request(
            &parse(changed).unwrap(),
            &plan.profiles[0].config_dir,
            &plan.profiles[0].state_dir
        )
        .is_err());
        let mut changed = original;
        changed["self_attested_revision"] = serde_json::json!("c".repeat(40));
        assert!(parse(changed).is_err());
    }
    #[test]
    fn handover_binding_selects_exact_window_and_rejects_stale_or_wrong_instance() {
        use std::os::unix::net::UnixListener;
        for (window, instance, fresh, generation, accepted) in [
            ("actual-window", "target-instance", true, 1, true),
            ("other-window", "target-instance", true, 1, false),
            ("actual-window", "other-instance", true, 1, false),
            ("actual-window", "target-instance", false, 1, false),
            ("actual-window", "target-instance", true, 0, false),
        ] {
            let (_temp, plan) = crate::migration::tests::fixture();
            let profile = &plan.profiles[0];
            let listener = UnixListener::bind(&profile.control_socket).unwrap();
            fs::set_permissions(&profile.control_socket, fs::Permissions::from_mode(0o600))
                .unwrap();
            let response = serde_json::json!({"version":1,"kind":"tern-migration-evidence","instance_id":instance,
                "window_id":window,"binding_epoch":1,"sequence":2,"local_host":"local","observation_generation":generation,
                "native_generation":1,"fresh":fresh});
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut byte = [0u8; 1];
                while stream.read_exact(&mut byte).is_ok() && byte[0] != b'\n' {
                    request.push(byte[0]);
                }
                let request: serde_json::Value = serde_json::from_slice(&request).unwrap();
                let mut bytes = serde_json::to_vec(&response).unwrap();
                bytes.push(b'\n');
                stream.write_all(&bytes).unwrap();
                request
            });
            assert_eq!(
                fresh_binding(profile, "target-instance", "actual-window").is_ok(),
                accepted
            );
            assert_eq!(server.join().unwrap()["window_id"], "actual-window");
        }
    }
    #[test]
    fn sdk_handover_requires_actual_atomic_canonical_target_without_native_kv_write() {
        let (_temp, plan) = delivery_fixture();
        let configuration = &plan.tern_delivery.as_ref().unwrap().configuration;
        let before = fs::read(&configuration.path).unwrap();
        let target = TernConnection {
            version: 1,
            executable: plan.target.image.path.clone(),
            config_dir: plan.profiles[0].config_dir.clone(),
            state_dir: plan.profiles[0].state_dir.clone(),
        };
        assert!(tern_config::verify_canonical_target(configuration, &target).is_err());
        assert_eq!(fs::read(&configuration.path).unwrap(), before);
        let legacy_only = serde_json::to_vec(&serde_json::json!({"executable":target.executable,"config_dir":target.config_dir,"state_dir":target.state_dir})).unwrap();
        protocol::write_private_bytes(&configuration.path, &legacy_only, 0o600).unwrap();
        assert!(tern_config::verify_canonical_target(configuration, &target).is_err());
        let sdk_value = serde_json::to_vec(&serde_json::json!({"connection":target})).unwrap();
        protocol::write_private_bytes(&configuration.path, &sdk_value, 0o600).unwrap();
        tern_config::verify_canonical_target(configuration, &target).unwrap();
        assert_eq!(fs::read(&configuration.path).unwrap(), sdk_value);
    }
    #[test]
    fn changed_cli_or_daemon_routing_cannot_issue_a_package_receipt() {
        let (_temp, plan) = delivery_fixture();
        let mut delivery = plan.tern_delivery.as_ref().unwrap().clone();
        delivery.program.sha256 = "f".repeat(64);
        assert!(verify_delivery_package(&delivery)
            .unwrap_err()
            .contains("changed"));
        let mut delivery = plan.tern_delivery.as_ref().unwrap().clone();
        delivery
            .environment
            .insert("TERN_DAEMON_SOCKET".into(), "/another/tern.sock".into());
        assert!(tern_config::validate_delivery(&plan, &delivery).is_err());
    }
    #[test]
    fn stop_in_any_original_profile_fences_fresh_consumer_delivery() {
        let (_temp, mut plan) = delivery_fixture();
        let mut secondary = plan.profiles[0].clone();
        secondary.state_dir = secondary.state_dir.join("second-state");
        secondary.config_dir = secondary.config_dir.join("second-config");
        secondary.pid += 1;
        secondary.instance_id = "source-secondary".into();
        secondary.control_socket = secondary.state_dir.join("control.sock");
        ensure_private_directory(&secondary.state_dir.join("updates-v4")).unwrap();
        ensure_private_directory(&secondary.config_dir).unwrap();
        plan.profiles.push(secondary);
        let intent = DeliveryResult {
            version: 1,
            operation_id: plan.operation_id.clone(),
            plan_sha256: "a".repeat(64),
            plan_path: Some(evidence_path(&plan)),
            original_record_sha256: None,
            current_image: plan.target.image.clone(),
            owner_upgrade: None,
            state_dir: plan.profiles[0].state_dir.clone(),
            stop_generation: 0,
            delivery: plan.tern_delivery.as_ref().unwrap().clone(),
            intent: true,
            settled: false,
            error: None,
        };
        check_delivery_stop(&intent, Some(&plan)).unwrap();
        protocol::write_private_new(
            &plan.profiles[1]
                .state_dir
                .join("updates-v4/stop-generation.json"),
            b"1",
            0o600,
        )
        .unwrap();
        assert!(check_delivery_stop(&intent, Some(&plan))
            .unwrap_err()
            .contains("user Stop"));
        assert!(!delivery_path(&plan).exists());
    }
    #[test]
    fn secondary_profile_uses_exact_central_approval_pointer_not_primary_success() {
        let (_temp, mut plan) = crate::migration::tests::fixture();
        let mut secondary = plan.profiles[0].clone();
        secondary.config_dir = secondary.config_dir.join("second-config");
        secondary.state_dir = secondary.state_dir.join("second-state");
        secondary.control_socket = secondary.state_dir.join("control.sock");
        secondary.pid += 1;
        secondary.instance_id = "source-secondary".into();
        ensure_private_directory(&secondary.config_dir).unwrap();
        ensure_private_directory(&secondary.state_dir).unwrap();
        plan.profiles.push(secondary);
        let mut record = crate::migration::tests::persist_fixture(&plan);
        let original_plan = fs::read(evidence_path(&plan)).unwrap();
        let original_journal = fs::read(journal_path(&plan)).unwrap();
        retain_profile_plans(&plan, &record).unwrap();
        let secondary = &plan.profiles[1];
        let pointer = secondary
            .state_dir
            .join("updates-v4")
            .join(format!("{}.migration-profile.json", plan.operation_id));
        assert_eq!(
            profile_plan_path(&pointer, &secondary.config_dir, &secondary.state_dir).unwrap(),
            evidence_path(&plan)
        );
        assert!(
            profile_plan_path(&pointer, &plan.profiles[0].config_dir, &secondary.state_dir)
                .is_err()
        );
        record.native_ready = true;
        record.target_committed = true;
        record.target_profiles.push(MigrationTargetProfile {
            config_dir: plan.profiles[0].config_dir.clone(),
            state_dir: plan.profiles[0].state_dir.clone(),
            pid: Some(2000),
            instance_id: Some("committed-primary".into()),
            image: plan.target.image.clone(),
            native_ready: true,
            commit_requested: true,
            committed: true,
        });
        assert!(committed_profile_image(&plan, &record, &plan.profiles[0]).is_ok());
        assert!(committed_profile_image(&plan, &record, secondary).is_err());
        assert_ne!(
            delivery_path_for(&plan, &plan.profiles[0].state_dir),
            delivery_path_for(&plan, &secondary.state_dir)
        );
        assert_eq!(fs::read(evidence_path(&plan)).unwrap(), original_plan);
        assert_eq!(fs::read(journal_path(&plan)).unwrap(), original_journal);
    }
    #[test]
    fn closed_same_image_historical_target_never_restarts_or_rewrites_old_operation() {
        let (_temp, plan) = crate::migration::tests::fixture();
        let mut record = crate::migration::tests::persist_fixture(&plan);
        record.phase = MigrationPhase::AwaitingReconnect;
        record.native_ready = true;
        record.target_committed = true;
        record.target_pid = Some(2000);
        record.target_instance = Some("closed-target".into());
        save(&plan, &record).unwrap();
        let old_plan = fs::read(evidence_path(&plan)).unwrap();
        let old_record = fs::read(journal_path(&plan)).unwrap();
        let mut context = crate::migration::tests::context(&plan);
        context.running = plan.target.image.clone();
        context.instance_id = "closed-target".into();
        context.version = crate::identity::neutral::VERSION.into();
        context.running_origin = Some(Box::new(plan.target.origin.clone()));
        assert!(observe_for_context(&plan, &context).is_err());
        assert!(refresh_committed_integrations(&plan).is_err());
        assert_eq!(fs::read(evidence_path(&plan)).unwrap(), old_plan);
        assert_eq!(fs::read(journal_path(&plan)).unwrap(), old_record);
        assert!(!forward_path(&plan, &plan.profiles[0].state_dir, &plan.target.image).exists());
        assert!(!delivery_path(&plan).exists());
    }
    #[test]
    fn historical_brew_retirement_requires_exact_successful_old_manager_not_missing_files() {
        let (_temp, mut plan) = crate::migration::tests::fixture();
        let brew = ExecutableIdentity {
            path: plan.profiles[0].state_dir.join("captured-brew"),
            sha256: "d".repeat(64),
        };
        plan.source_origin = LegacyOrigin::Homebrew {
            brew: brew.clone(),
            prefix: plan.profiles[0].state_dir.join("brew-prefix"),
            cellar_formula_root: plan.profiles[0].state_dir.join("brew-cellar"),
            formula: "hanbong5938/tap/herdr-desktop-pet".into(),
        };
        let mut record = make_record(&plan, &serde_json::to_vec(&plan).unwrap());
        record.phase = MigrationPhase::AwaitingReconnect;
        record.native_ready = true;
        record.target_committed = true;
        record.target_pid = Some(2000);
        record.target_instance = Some("old-committed".into());
        record.retirement_attempt = 1;
        assert!(verify_retained_commit(&plan, &record, &plan.profiles[0])
            .unwrap_err()
            .contains("retirement transaction"));
        record.manager = Some(ManagerEvidence {
            program: brew,
            args: vec!["uninstall".into(), "another/formula".into()],
            intent_at: 1,
            pid: Some(2001),
            pgid: Some(2001),
            exit_code: Some(0),
            quiescent: true,
        });
        assert!(verify_retained_commit(&plan, &record, &plan.profiles[0])
            .unwrap_err()
            .contains("retirement transaction"));
        record.manager.as_mut().unwrap().args[1] = "hanbong5938/tap/herdr-desktop-pet".into();
        record.manager.as_mut().unwrap().quiescent = false;
        assert!(verify_retained_commit(&plan, &record, &plan.profiles[0]).is_err());
    }
}
