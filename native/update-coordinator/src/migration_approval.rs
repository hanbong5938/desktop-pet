//! One update approval delegates only the immutable, verified migration plan.
//! Historical updater evidence is read-only provenance, never replay authority.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Authority {
    UpdateApproval,
    SettledBridgeUpdate {
        operation_id: String,
        plan_sha256: String,
        result_sha256: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct Approval {
    version: u32,
    operation_id: String,
    plan_sha256: String,
    authority: Authority,
}
fn approval_path(plan: &MigrationPlan) -> PathBuf {
    plan.profiles[0]
        .state_dir
        .join("updates-v4")
        .join(format!("{}.migration-approval.json", plan.operation_id))
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn origin_root(value: &serde_json::Value) -> Result<PathBuf, String> {
    let kind = value
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or("original update origin has no kind")?;
    let field = match kind {
        "homebrew" => "cellar_formula_root",
        "herdr" => "checkout_root",
        "source" => "root",
        "manual" => "locator",
        _ => return Err("original update owner cannot delegate migration".into()),
    };
    let path = PathBuf::from(
        value
            .get(field)
            .and_then(|v| v.as_str())
            .ok_or("original update owner has no root")?,
    );
    crate::ensure_absolute(&path)?;
    if kind == "manual" {
        Ok(path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .ok_or("invalid original bundle")?
            .to_path_buf())
    } else {
        Ok(path)
    }
}
fn original_lease(base: &Path, operation: &str) -> Result<Option<File>, String> {
    let path = base.join(format!("{operation}.operation.lock"));
    owned_private_file(&path, 16384)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|e| e.to_string())?;
    use std::os::fd::AsRawFd;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            return Ok(None);
        }
        return Err(format!(
            "cannot authenticate original helper operation lease: {error}"
        ));
    }
    Ok(Some(file))
}
fn bridge_authority(plan: &MigrationPlan, operation: &str) -> Result<Authority, String> {
    protocol::validate_operation_id(operation)?;
    let profile = &plan.profiles[0];
    let base = profile.state_dir.join("updates");
    if base.join("profile.reservation.json").exists() {
        return Err(
            "original update still owns the profile; automatic migration waits for its helper"
                .into(),
        );
    }
    // This is the ORIGINAL protocol-three helper lease, not updates-v4.
    let _lease = original_lease(&base, operation)?
        .ok_or("original update helper still owns its operation lease")?;
    let latest: String = serde_json::from_slice(&owned_private_file(
        &base.join("latest-operation.json"),
        128,
    )?)
    .map_err(|e| e.to_string())?;
    if latest != operation {
        return Err("original update is no longer the latest operation".into());
    }
    let plan_bytes = owned_private_file(&base.join(format!("{operation}.plan.json")), 65536)?;
    let result_bytes = owned_private_file(&base.join(format!("{operation}.result.json")), 16384)?;
    let prior: serde_json::Value =
        serde_json::from_slice(&plan_bytes).map_err(|e| e.to_string())?;
    let result: protocol::OperationRecord =
        serde_json::from_slice(&result_bytes).map_err(|e| e.to_string())?;
    let context = &prior["context"];
    let prior_origin = prior
        .get("target")
        .filter(|target| !target.is_null())
        .map_or(&prior["origin"], |target| &target["origin"]);
    let candidate = result
        .candidate
        .as_ref()
        .ok_or("original update did not attest its applied candidate")?;
    let original_image: ExecutableIdentity =
        serde_json::from_value(prior["running"].clone()).map_err(|e| e.to_string())?;
    image_valid(&original_image)?;
    if result.version != 3
        || result.operation_id != operation
        || result.phase != protocol::OperationPhase::Completed
        || !result.installed
        || !result.applied
        || result.execution_fence != protocol::ExecutionFence::ManagerExited
        || prior["version"].as_u64() != Some(3)
        || prior["operation_id"].as_str() != Some(operation)
        || prior["running"] != context["running"]
        || !original_image
            .path
            .starts_with(origin_root(&prior["origin"])?)
        || context
            .get("running_origin")
            .filter(|origin| !origin.is_null())
            .is_some_and(|origin| origin != &prior["origin"])
        || !context["instance_id"].as_str().is_some_and(instance_valid)
        || context["version"].as_str() != Some("0.3.12")
        || context["config_dir"].as_str() != profile.config_dir.to_str()
        || context["state_dir"].as_str() != profile.state_dir.to_str()
        || candidate != &plan.source_image
        || origin_root(prior_origin)? != plan.source_origin.root()
        || prior
            .get("target")
            .filter(|target| !target.is_null())
            .is_some_and(|target| target["release"]["version"].as_str() != Some("0.3.13"))
        || plan.target.release.version != "0.4.9"
    {
        return Err("original update does not attest this exact successor bridge, profile and bounded core target".into());
    }
    // No old record, plan or helper is edited or executed by this continuation.
    Ok(Authority::SettledBridgeUpdate {
        operation_id: operation.into(),
        plan_sha256: digest(&plan_bytes),
        result_sha256: digest(&result_bytes),
    })
}
fn retain(plan: &MigrationPlan, authority: Authority) -> Result<(), String> {
    ensure_private_directory(&plan.profiles[0].state_dir.join("updates-v4"))?;
    let bytes = serde_json::to_vec(plan).map_err(|e| e.to_string())?;
    let approval = Approval {
        version: 1,
        operation_id: plan.operation_id.clone(),
        plan_sha256: digest(&bytes),
        authority,
    };
    protocol::write_private_new(
        &approval_path(plan),
        &serde_json::to_vec(&approval).map_err(|e| e.to_string())?,
        0o600,
    )
}
/// Called only after the single original update dialog approves this exact plan.
pub fn authorize_update_approval(plan: &MigrationPlan) -> Result<(), String> {
    validate_plan(plan)?;
    retain(plan, Authority::UpdateApproval)
}
/// A delegation is not a claim that an independent helper/source dialog was visible.
pub fn delegated_approval(plan: &MigrationPlan) -> Result<bool, String> {
    let path = approval_path(plan);
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(false);
    }
    let approval: Approval =
        serde_json::from_slice(&owned_private_file(&path, MAX_PLAN)?).map_err(|e| e.to_string())?;
    if approval.version != 1
        || approval.operation_id != plan.operation_id
        || approval.plan_sha256 != digest(&serde_json::to_vec(plan).map_err(|e| e.to_string())?)
    {
        return Err("delegated update approval does not match the immutable migration plan".into());
    }
    if let Authority::SettledBridgeUpdate { operation_id, .. } = &approval.authority {
        if bridge_authority(plan, operation_id)? != approval.authority {
            return Err("retained original update authority changed".into());
        }
    }
    Ok(true)
}
/// Continue a completed protocol-three update only to the published successor core.
/// A standalone launch, failed/unknown update, or different candidate grants nothing.
pub fn prepare_automatic_bridge_plan(
    context: &UpdateContext,
    integration: Option<&str>,
) -> Result<Option<MigrationPlan>, String> {
    if context.version != "0.3.13" {
        return Ok(None);
    }
    // This token/generation was already authenticated by the real native startup
    // path while the original updater owned its reservation. A manual later
    // launch must not resurrect an old approval after user Stop.
    let Some(startup_token) = std::env::var("HERDR_DESKTOP_PET_UPDATER_TOKEN").ok() else {
        return Ok(None);
    };
    if !hex(&startup_token, 64) {
        return Err("original updater startup token is malformed".into());
    }
    let startup_generation: u64 = std::env::var("HERDR_DESKTOP_PET_UPDATER_GENERATION")
        .map_err(|_| "original updater startup generation is unavailable")?
        .parse()
        .map_err(|_| "original updater startup generation is invalid")?;
    if migration_stop_generation(&context.state_dir)? != startup_generation {
        return Err(
            "user Stop superseded the original update continuation; no migration was started"
                .into(),
        );
    }
    let base = context.state_dir.join("updates");
    let latest = base.join("latest-operation.json");
    if !latest.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    let operation: String =
        serde_json::from_slice(&owned_private_file(&latest, 128)?).map_err(|e| e.to_string())?;
    // The old helper may publish readiness before releasing its reservation.
    let until = Instant::now() + Duration::from_secs(60);
    let completed = loop {
        let bytes = owned_private_file(&base.join(format!("{operation}.result.json")), 16384)?;
        let record: protocol::OperationRecord =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if record.phase == protocol::OperationPhase::Completed
            && !base.join("profile.reservation.json").exists()
        {
            if record.version != 3 || record.candidate.as_ref() != Some(&context.running) {
                return Ok(None);
            }
            if original_lease(&base, &operation)?.is_some() {
                break record;
            }
        }
        if matches!(
            record.phase,
            protocol::OperationPhase::Failed
                | protocol::OperationPhase::Unknown
                | protocol::OperationPhase::UserStopped
        ) {
            return Ok(None);
        }
        if Instant::now() >= until {
            return Err(
                "original update helper has not settled; no automatic migration was started".into(),
            );
        }
        thread::sleep(Duration::from_millis(200));
    };
    if !completed.installed
        || !completed.applied
        || completed.execution_fence != protocol::ExecutionFence::ManagerExited
    {
        return Err(
            "original update is not a certain successful bridge cutover; it will not be replayed"
                .into(),
        );
    }
    let plan = prepare_bridge_plan(context, integration)?;
    if migration_stop_generation(&context.state_dir)? != startup_generation {
        return Err("user Stop superseded the original update while its target was staging".into());
    }
    let authority = bridge_authority(&plan, &operation)?;
    retain(&plan, authority)?;
    Ok(Some(plan))
}

/// Explicit CLI update approval obtains the daemon's captured source context;
/// no preferences flag or fabricated historical operation grants authority.
pub fn approve_profile_update(
    config: &Path,
    state: &Path,
    integration: Option<&str>,
) -> Result<(MigrationPlan, PathBuf), String> {
    owned_directory(config)?;
    owned_directory(state)?;
    let response: serde_json::Value = serde_json::from_slice(&exchange(
        &state.join("control.sock"),
        &serde_json::json!({"version":1,"kind":"legacy-migration-context","config_dir":config,"state_dir":state}),
    )?).map_err(|e| e.to_string())?;
    if let Some(error) = response.get("error").and_then(|value| value.as_str()) {
        return Err(error.into());
    }
    if response["version"].as_u64() != Some(1)
        || response["kind"].as_str() != Some("legacy-migration-context")
    {
        return Err("source daemon returned an unsupported startup context proof".into());
    }
    let context: UpdateContext =
        serde_json::from_value(response["context"].clone()).map_err(|e| e.to_string())?;
    if context.config_dir != config || context.state_dir != state || context.version != "0.3.13" {
        return Err(
            "explicit update approval does not select the exact successor bridge profile".into(),
        );
    }
    let plan = prepare_bridge_plan(&context, integration)?;
    authorize_update_approval(&plan)?;
    let path = write_new_plan(&plan)?;
    Ok((plan, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn original(
        plan: &MigrationPlan,
        candidate: ExecutableIdentity,
    ) -> (String, PathBuf, protocol::OperationRecord) {
        let id = "a".repeat(32);
        let base = plan.profiles[0].state_dir.join("updates");
        ensure_private_directory(&base).unwrap();
        let running = ExecutableIdentity {
            path: plan
                .source_origin
                .root()
                .join("old-3.12/HerdrDesktopPet.app/Contents/MacOS/herdr-desktop-pet"),
            sha256: "e".repeat(64),
        };
        let prior = serde_json::json!({
            "version":3,"operation_id":id,"running":running,"candidate":null,"target":null,
            "origin":{"kind":"source","root":plan.source_origin.root()},
            "context":{"version":"0.3.12","instance_id":"original-approved-instance","running":running,
                "config_dir":plan.profiles[0].config_dir,"state_dir":plan.profiles[0].state_dir}
        });
        let result = protocol::OperationRecord {
            version: 3,
            operation_id: id.clone(),
            phase: protocol::OperationPhase::Completed,
            execution_fence: protocol::ExecutionFence::ManagerExited,
            detail: "genuine fixture bridge cutover".into(),
            installed: true,
            applied: true,
            candidate: Some(candidate),
        };
        for (name, bytes) in [
            (
                "latest-operation.json".into(),
                serde_json::to_vec(&id).unwrap(),
            ),
            (
                format!("{id}.plan.json"),
                serde_json::to_vec(&prior).unwrap(),
            ),
            (
                format!("{id}.result.json"),
                serde_json::to_vec(&result).unwrap(),
            ),
            (format!("{id}.operation.lock"), Vec::new()),
        ] {
            protocol::write_private_new(&base.join(name), &bytes, 0o600).unwrap();
        }
        (id, base, result)
    }

    #[test]
    fn one_approval_binds_every_exact_plan_field_and_is_single_use() {
        let (_temp, plan) = crate::migration::tests::fixture();
        assert!(!delegated_approval(&plan).unwrap());
        authorize_update_approval(&plan).unwrap();
        assert!(delegated_approval(&plan).unwrap());
        assert!(authorize_update_approval(&plan).is_err());
        let mut changed = plan.clone();
        changed.profiles[0].state_dir.push("another");
        // A different profile has no authority, even if an operation ID is reused.
        assert!(!delegated_approval(&changed).unwrap());
        changed = plan.clone();
        changed.target.image.sha256 = "f".repeat(64);
        assert!(delegated_approval(&changed).is_err());
    }

    #[test]
    fn completed_original_bridge_proof_is_read_only_and_exact_candidate_scoped() {
        let (_temp, plan) = crate::migration::tests::fixture();
        let (id, base, _) = original(&plan, plan.source_image.clone());
        let before_plan = fs::read(base.join(format!("{id}.plan.json"))).unwrap();
        let before_result = fs::read(base.join(format!("{id}.result.json"))).unwrap();
        let authority = bridge_authority(&plan, &id).unwrap();
        retain(&plan, authority).unwrap();
        assert!(delegated_approval(&plan).unwrap());
        assert_eq!(
            fs::read(base.join(format!("{id}.plan.json"))).unwrap(),
            before_plan
        );
        assert_eq!(
            fs::read(base.join(format!("{id}.result.json"))).unwrap(),
            before_result
        );
        let mut other = plan.clone();
        other.source_image.sha256 = "f".repeat(64);
        assert!(bridge_authority(&other, &id).is_err());
        other = plan.clone();
        other.profiles[0].config_dir.push("different");
        assert!(bridge_authority(&other, &id).is_err());
    }

    #[test]
    fn unknown_original_manager_and_unrelated_old_candidate_never_delegate_or_rewrite() {
        let (_temp, plan) = crate::migration::tests::fixture();
        let (id, base, mut result) = original(&plan, plan.source_image.clone());
        result.phase = protocol::OperationPhase::Unknown;
        result.execution_fence = protocol::ExecutionFence::ManagerIntent;
        let bytes = serde_json::to_vec(&result).unwrap();
        protocol::write_private_bytes(&base.join(format!("{id}.result.json")), &bytes, 0o600)
            .unwrap();
        assert!(bridge_authority(&plan, &id).is_err());
        assert_eq!(
            fs::read(base.join(format!("{id}.result.json"))).unwrap(),
            bytes
        );
        result.phase = protocol::OperationPhase::Completed;
        result.execution_fence = protocol::ExecutionFence::ManagerExited;
        result.candidate.as_mut().unwrap().sha256 = "0".repeat(64);
        protocol::write_private_bytes(
            &base.join(format!("{id}.result.json")),
            &serde_json::to_vec(&result).unwrap(),
            0o600,
        )
        .unwrap();
        let context = crate::migration::tests::context(&plan);
        assert!(prepare_automatic_bridge_plan(&context, None)
            .unwrap()
            .is_none());
        assert!(!approval_path(&plan).exists());
    }

    #[test]
    fn delegated_frame_never_claims_helper_or_source_visibility() {
        let (_temp, plan) = crate::migration::tests::fixture();
        authorize_update_approval(&plan).unwrap();
        let path = write_new_plan(&plan).unwrap();
        let mut frame = final_consent_frame(
            &plan,
            digest(&fs::read(&path).unwrap()),
            false,
            MigrationFinalConsentOutcome::DelegatedApproved,
        );
        assert!(!frame.helper_ui_visible && !frame.source_ui_visible);
        let mut bytes = serde_json::to_vec(&frame).unwrap();
        bytes.push(b'\n');
        assert_eq!(
            read_final_consent_outcome(&mut bytes.as_slice(), &plan, &path, false).unwrap(),
            MigrationFinalConsentOutcome::DelegatedApproved
        );
        frame.helper_ui_visible = true;
        let mut bytes = serde_json::to_vec(&frame).unwrap();
        bytes.push(b'\n');
        assert!(read_final_consent_outcome(&mut bytes.as_slice(), &plan, &path, false).is_err());
    }
}

/// Resolve a pre-bridge saved route through the exact retained original update,
/// never by basename, latest version, or reopening a retired source executable.
pub(super) fn original_source_route(
    plan: &MigrationPlan,
    executable: &Path,
    config: &Path,
    state: &Path,
) -> Result<Option<ExecutableIdentity>, String> {
    let selected = &plan.profiles[0];
    if selected.config_dir != config || selected.state_dir != state {
        return Ok(None);
    }
    let base = state.join("updates");
    let latest = base.join("latest-operation.json");
    if !latest.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    let operation: String =
        serde_json::from_slice(&owned_private_file(&latest, 128)?).map_err(|e| e.to_string())?;
    protocol::validate_operation_id(&operation)?;
    let result: protocol::OperationRecord = serde_json::from_slice(&owned_private_file(
        &base.join(format!("{operation}.result.json")),
        MAX_FRAME as u64,
    )?)
    .map_err(|e| e.to_string())?;
    if result.version != 3 || result.candidate.as_ref() != Some(&plan.source_image) {
        return Ok(None);
    }
    bridge_authority(plan, &operation)?;
    let original: serde_json::Value = serde_json::from_slice(&owned_private_file(
        &base.join(format!("{operation}.plan.json")),
        65536,
    )?)
    .map_err(|e| e.to_string())?;
    let source: ExecutableIdentity = serde_json::from_value(original["context"]["running"].clone())
        .map_err(|e| e.to_string())?;
    image_valid(&source)?;
    if source.path == executable
        || original["context"]["executable"].as_str().map(Path::new) == Some(executable)
        || original["origin"]["locator"].as_str().map(Path::new) == Some(executable)
    {
        return Ok(Some(source));
    }
    Ok(None)
}
