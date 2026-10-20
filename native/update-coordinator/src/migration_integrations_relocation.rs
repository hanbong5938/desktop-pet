//! Standalone relocation is its own authority namespace, never a legacy Commit.
//! Historical links are proven from retained inodes/closures; only the head is live.
//!
//! Native relocation proves a Running source with Stop ACK/native consent;
//! manual recovery proves a Stopped source with exact SSH operator consent.
//! The atomic receipt establishes the H-to-C cutover. Every successor is a
//! completed normal updater edge with its retained predecessor inode and exact
//! external deployment sidecar. Missing derived owner records are rebuilt in
//! order from that history, never from a latest marker or today's version.
//! A terminal failed updater is excluded only after its exact candidate remains
//! staged and its original inode/complete closure is restored or retained by a
//! uniquely authenticated later successor. Unresolved rollback history blocks
//! reconstruction; a result flag alone never removes an intent from ancestry.
//!
//! Challenges are immutable per image/instance/original SDK tuple; receipts are
//! per image/instance/window. A saved H query and an already-saved C query cannot
//! replace one another's token. Tern alone writes KV, and unavailable consumers
//! or disconnected windows remain integration-pending after native success.
//! Full-digest challenge/receipt basenames live in a guarded private operation
//! directory so both final and atomic temporary names fit filesystem NAME_MAX.
use super::*;
use crate::installation::{self, FileSnapshot, ReplacementIntent, ReplacementLayout};
use crate::native_runtime::ProfilePaths;
use crate::standalone_install::{self, StandaloneInstallPlan, StandaloneInstallPurpose};
use crate::standalone_relocation::RelocationTernApproval;
use std::collections::BTreeSet;

const MAX_INTENT: u64 = 2 * 1024 * 1024;
const HISTORY_BOUND: usize = 1024;
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path, limit: u64) -> Result<T, String> {
    serde_json::from_slice(&owned_private_file(path, limit)?)
        .map_err(|e| format!("{}: {e}", path.display()))
}
fn exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}
fn plan_id(name: &str, prefix: &str) -> Result<Option<String>, String> {
    let Some(id) = name
        .strip_prefix(prefix)
        .and_then(|name| name.strip_suffix(".plan.json"))
    else {
        return Ok(None);
    };
    // Other dotted operation resources are not plan files in this namespace.
    if prefix.is_empty() && id.starts_with("install-") {
        return Ok(None);
    }
    protocol::validate_operation_id(id)?;
    Ok(Some(id.to_owned()))
}
pub(super) fn is_install_plan_path(state: &Path, path: &Path, id: &str) -> Result<bool, String> {
    if path == standalone_install::plan_path(state, id)? {
        return Ok(true);
    }
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("install-") && name.ends_with(".plan.json"))
    {
        return Err("standalone handover path differs from its exact operation namespace".into());
    }
    Ok(false)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Consent {
    version: u32,
    operation_id: String,
    plan_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Commit {
    version: u32,
    operation_id: String,
    intent_sha256: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelocationOwner {
    version: u32,
    relocation_id: String,
    relocation_plan_sha256: String,
    predecessor_sha256: Option<String>,
    previous_image: ExecutableIdentity,
    current_image: ExecutableIdentity,
    previous_closure_sha256: String,
    current_closure_sha256: String,
    update_operation_id: Option<String>,
    update_plan_sha256: Option<String>,
    update_result_sha256: Option<String>,
    #[serde(with = "crate::migration::neutral_origin_codec")]
    origin: InstallOrigin,
    profile: ProfilePaths,
}
struct Base {
    plan: StandaloneInstallPlan,
    approval: RelocationTernApproval,
    plan_sha256: String,
    receipt_sha256: String,
    result_sha256: String,
    intent: ReplacementIntent,
}
struct Link {
    plan: crate::UpdatePlan,
    result: protocol::OperationRecord,
    plan_sha256: String,
    result_sha256: String,
    intent: ReplacementIntent,
}
struct Authority {
    base: Base,
    owner_sha256: String,
    owner_operations: Vec<String>,
    live: crate::native_runtime::NativeInstance,
    generation: u64,
}
/// A normal startup is observed separately from InstalledStopped. This never
/// rewrites applied=false or grants bootstrap startup authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManualInstalledStoppedObservation {
    version: u32,
    operation_id: String,
    plan_sha256: String,
    result_sha256: String,
    image: ExecutableIdentity,
    instance_id: String,
    pid: Option<u32>,
    stop_generation: u64,
}
fn manual_observation_path(base: &Base) -> Result<PathBuf, String> {
    protocol::validate_operation_id(&base.plan.operation_id)?;
    Ok(base.plan.profile.state_dir.join("updates-v4").join(format!(
        "install-{}.manual-installed-stopped-observed.json",
        base.plan.operation_id,
    )))
}
fn validate_manual_observation(
    base: &Base,
    observed: &ManualInstalledStoppedObservation,
) -> Result<(), String> {
    let previous = original_context(&base.plan)?;
    if observed.version != 1
        || observed.operation_id != base.plan.operation_id
        || observed.plan_sha256 != base.plan_sha256
        || observed.result_sha256 != base.result_sha256
        || observed.image.path != base.intent.target.join("Contents/MacOS/desktop-pet")
        || observed.image.sha256 != base.plan.candidate_image.sha256
        || observed.instance_id.is_empty()
        || observed.instance_id == previous.instance_id
        || observed.pid == Some(0)
        || observed.stop_generation != base.plan.original_stop_generation
        || !matches!(
            base.plan.purpose,
            StandaloneInstallPurpose::ManualRecovery { .. }
        )
    {
        return Err("manual InstalledStopped observation does not prove a fresh unchanged-generation normal start".into());
    }
    Ok(())
}
fn validate_manual_start_receipt(
    base: &Base,
    observed: &ManualInstalledStoppedObservation,
) -> Result<(), String> {
    validate_manual_observation(base, observed)?;
    let (generation, context) = protocol::historical_runtime_start_context(
        base.plan.installed_origin(),
        &base.plan.profile.state_dir,
        observed.pid,
        &observed.instance_id,
        &observed.image,
    )?;
    let previous = original_context(&base.plan)?;
    if generation != observed.stop_generation
        || context.version != base.plan.candidate_version
        || context.config_dir != base.plan.profile.config_dir
        || context.state_dir != base.plan.profile.state_dir
        || context.assets_override != previous.assets_override
        || context.environment != previous.environment
        || context.observation_mode()? != previous.observation_mode()?
    {
        return Err(
            "manual initial normal startup changed the original profile or runtime context".into(),
        );
    }
    Ok(())
}
fn reconstruct_manual_start(base: &Base) -> Result<ManualInstalledStoppedObservation, String> {
    // These fields discover a candidate receipt, not authority. The shared
    // strict protocol reader below authenticates its entire canonical record.
    #[derive(Deserialize)]
    struct StartHint {
        pid: u32,
        instance_id: String,
        image: ExecutableIdentity,
        stop_generation: u64,
    }
    let expected = ExecutableIdentity {
        path: base.intent.target.join("Contents/MacOS/desktop-pet"),
        sha256: base.plan.candidate_image.sha256.clone(),
    };
    let mut selected: Option<ManualInstalledStoppedObservation> = None;
    let mut count = 0;
    for entry in
        fs::read_dir(base.plan.profile.state_dir.join("updates-v4")).map_err(|e| e.to_string())?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.ends_with(".runtime-start.json") {
            continue;
        }
        count += 1;
        if count > HISTORY_BOUND {
            return Err("manual runtime startup history exceeds bound".into());
        }
        let hint: StartHint = read_json(&entry.path(), MAX_PLAN)?;
        if hint.image != expected || hint.stop_generation != base.plan.original_stop_generation {
            continue;
        }
        if name != format!("{}.runtime-start.json", digest(hint.instance_id.as_bytes())) {
            return Err(
                "manual historical startup is not its exact canonical runtime receipt".into(),
            );
        }
        let observed = ManualInstalledStoppedObservation {
            version: 1,
            operation_id: base.plan.operation_id.clone(),
            plan_sha256: base.plan_sha256.clone(),
            result_sha256: base.result_sha256.clone(),
            image: hint.image,
            instance_id: hint.instance_id,
            pid: Some(hint.pid),
            stop_generation: hint.stop_generation,
        };
        validate_manual_start_receipt(base, &observed)?;
        // Multiple context-preserving real starts at the unchanged fence are
        // equivalent proof. Later settings are never used as initial evidence.
        if selected
            .as_ref()
            .is_none_or(|old| observed.instance_id < old.instance_id)
        {
            selected = Some(observed);
        }
    }
    selected.ok_or_else(|| {
        "manual recovery has no unchanged-generation context-bound normal startup receipt".into()
    })
}
fn observe_manual_start(base: &Base) -> Result<(), String> {
    if !matches!(
        base.plan.purpose,
        StandaloneInstallPurpose::ManualRecovery { .. }
    ) {
        return Ok(());
    }
    let path = manual_observation_path(base)?;
    let observed = if exists(&path)? {
        read_json(&path, MAX_PLAN)?
    } else {
        // Always reconstruct the actual initial C startup. A later legitimate
        // restart/updater may have changed settings and is only live-head proof.
        reconstruct_manual_start(base)?
    };
    validate_manual_start_receipt(base, &observed)?;
    publish_exact(&path, &observed)?;
    Ok(())
}

fn released(
    source: &InstallOrigin,
    target: Option<&InstallOrigin>,
    state: &Path,
    id: &str,
) -> Result<(), String> {
    if protocol::reservation_exists(source, target, state, id)? {
        return Err("Tern relocation is pending until the exact helper/root/profile reservation is released".into());
    }
    protocol::start_allowed(source, state, None, None)?;
    if let Some(target) = target {
        protocol::start_allowed(target, state, None, None)?;
    }
    Ok(())
}
fn validate_updater_approval(plan: &crate::UpdatePlan) -> Result<(), String> {
    let target = plan
        .target
        .as_ref()
        .ok_or("owner updater lacks explicit release approval")?;
    target.release.validate_neutral()?;
    let previous = semver::Version::parse(&plan.context.version).map_err(|e| e.to_string())?;
    let next = semver::Version::parse(&target.release.version).map_err(|e| e.to_string())?;
    if previous.to_string() != plan.context.version
        || !previous.build.is_empty()
        || !crate::UpdatePolicy::StableAndBeta.allows_version(&plan.context.version)?
        || previous.cmp_precedence(&next) != std::cmp::Ordering::Less
        || !target.policy.allows_version(&target.release.version)?
        || target.resolved_commit.is_some()
        || plan.context.running_origin.as_deref() != Some(&plan.origin)
        || plan.context.executable != plan.running.path
        || plan.running.path
            != crate::physical_install_root(&plan.origin)?.join("Contents/MacOS/desktop-pet")
    {
        return Err("owner updater release/profile/source approval is not canonical".into());
    }
    Ok(())
}
fn validate_parent(plan: &StandaloneInstallPlan) -> Result<(), String> {
    let StandaloneInstallPurpose::PrivateRelocation { parent, .. } = &plan.purpose else {
        return Err("upgrade approval has no Tern relocation authority".into());
    };
    let _lease = protocol::lock_operation(&plan.profile.state_dir, &parent.operation_id)?;
    let prior = crate::read_plan(&plan.profile.state_dir, &parent.operation_id)?;
    let directory = plan.profile.state_dir.join("updates-v4");
    let result_bytes = owned_private_file(
        &directory.join(format!("{}.result.json", parent.operation_id)),
        MAX_PLAN,
    )?;
    let result: protocol::OperationRecord =
        serde_json::from_slice(&result_bytes).map_err(|e| e.to_string())?;
    validate_updater_approval(&prior)?;
    if digest(&owned_private_file(
        &directory.join(format!("{}.plan.json", parent.operation_id)),
        MAX_PLAN,
    )?) != parent.plan_sha256
        || digest(&result_bytes) != parent.result_sha256
        || prior.version != 4
        || prior.operation_id != parent.operation_id
        || prior.action != crate::UpdateAction::StandaloneReplace
        || prior.target.is_none()
        || prior.origin != *plan.origin()
        || prior.target_origin() != plan.origin()
        || prior.context.config_dir != plan.profile.config_dir
        || prior.context.state_dir != plan.profile.state_dir
        || prior.target.as_ref().is_none_or(|target| {
            target.release.version != plan.original.app_version
                || target.release.channel != crate::ReleaseChannel::Stable
        })
        || result.version != 4
        || result.operation_id != parent.operation_id
        || result.phase != protocol::OperationPhase::Completed
        || !result.installed
        || !result.applied
        || result.execution_fence != protocol::ExecutionFence::ManagerExited
        || result.candidate.as_ref() != Some(&plan.original.image)
    {
        return Err("relocation has no unchanged settled exact H updater parent".into());
    }
    released(
        &prior.origin,
        Some(prior.target_origin()),
        &plan.profile.state_dir,
        &prior.operation_id,
    )
}
fn validate_closure(
    bundle: &Path,
    sidecar: &[u8],
    file: &FileSnapshot,
    version: &str,
) -> Result<installation::ValidatedApp, String> {
    if FileSnapshot::capture(&bundle.join("Contents/MacOS/desktop-pet"))? != *file {
        return Err(
            "retained replacement inode differs from immutable signed closure evidence".into(),
        );
    }
    let app = installation::validate_bundle_snapshot(bundle, Some(sidecar))?;
    if app.image.sha256 != file.sha256 || app.version != version {
        return Err(
            "retained replacement version/image differs from signed closure evidence".into(),
        );
    }
    if crate::owned_file(&installation::sidecar_path(bundle), MAX_INTENT)? != sidecar {
        return Err("retained external deployment sidecar changed".into());
    }
    if FileSnapshot::capture(&app.image.path)? != *file {
        return Err("retained replacement inode changed during closure validation".into());
    }
    Ok(app)
}
fn relocation_approval(plan: &StandaloneInstallPlan) -> Option<&Option<RelocationTernApproval>> {
    match &plan.purpose {
        StandaloneInstallPurpose::PrivateRelocation { tern, .. }
        | StandaloneInstallPurpose::ManualRecovery { tern, .. } => Some(tern),
        StandaloneInstallPurpose::Upgrade => None,
    }
}
fn original_context(plan: &StandaloneInstallPlan) -> Result<&UpdateContext, String> {
    match &plan.purpose {
        StandaloneInstallPurpose::ManualRecovery {
            previous_context, ..
        } => Ok(previous_context),
        _ => plan
            .running()
            .map(|(_, context)| context)
            .ok_or_else(|| "relocation has no original Running context".into()),
    }
}
fn validate_base_consent(plan: &StandaloneInstallPlan, plan_sha256: &str) -> Result<(), String> {
    let state = &plan.profile.state_dir;
    let id = &plan.operation_id;
    match &plan.purpose {
        StandaloneInstallPurpose::ManualRecovery { .. } => {
            let request = standalone_install::manual_operator_request_path(state, id)?;
            standalone_install::read_operator_consent(plan, &request)?
                .ok_or("manual recovery has no exact SSH operator consent")?;
            // An ACK file, including an invalid one, cannot become authority for
            // a source which was already stopped before this operation.
            if exists(&standalone_install::stop_ack_path(state, id)?)?
                || exists(&standalone_install::consent_path(state, id)?)?
            {
                return Err(
                    "stopped manual recovery unexpectedly claims native consent or Stop ACK".into(),
                );
            }
        }
        StandaloneInstallPurpose::PrivateRelocation { .. } => {
            let consent: Consent =
                read_json(&standalone_install::consent_path(state, id)?, MAX_PLAN)?;
            if consent.version != 1
                || consent.operation_id != *id
                || consent.plan_sha256 != plan_sha256
            {
                return Err(
                    "relocation Tern authority has no exact native affirmative consent".into(),
                );
            }
            standalone_install::read_stop_ack(plan)?
                .ok_or("relocation has no durable source Stop ACK")?
                .validate(plan)?;
        }
        StandaloneInstallPurpose::Upgrade => return Err("upgrade has no relocation consent".into()),
    }
    Ok(())
}
fn validate_source_retired(plan: &StandaloneInstallPlan) -> Result<(), String> {
    let source = crate::physical_install_root(plan.origin())?;
    if exists(source)? || exists(&installation::sidecar_path(source))? {
        return Err("relocation source bundle or external sidecar has not been retired".into());
    }
    Ok(())
}
fn base_result_ready(
    plan: &StandaloneInstallPlan,
    record: &standalone_install::StandaloneInstallRecord,
) -> bool {
    record.phase == standalone_install::StandaloneInstallPhase::Completed
        && record.result.installed
        && match &plan.purpose {
            StandaloneInstallPurpose::ManualRecovery { .. } => {
                !record.result.applied
                    && record.result.status == standalone_install::InstallStatus::InstalledStopped
                    && record.result.restart == standalone_install::InstallRestart::NotRequested
            }
            StandaloneInstallPurpose::PrivateRelocation { .. } => record.result.applied,
            StandaloneInstallPurpose::Upgrade => false,
        }
}
fn read_base(state: &Path, id: &str) -> Result<Option<Base>, String> {
    let plan = standalone_install::read_plan(state, id)?;
    let Some(tern) = relocation_approval(&plan) else {
        return Ok(None);
    };
    let Some(approval) = tern.clone() else {
        return Ok(None);
    };
    let context = original_context(&plan)?;
    tern_config::validate_relocation_configuration(context, &approval)?;
    let Some(record) = standalone_install::read_record(state, id)? else {
        return Ok(None);
    };
    if !base_result_ready(&plan, &record) {
        return Ok(None);
    }
    let _lease = protocol::lock_operation(state, id)?;
    if standalone_install::read_plan(state, id)? != plan
        || standalone_install::read_record(state, id)?.as_ref() != Some(&record)
    {
        return Err("relocation approval/result changed before helper settlement".into());
    }
    let result_sha256 = digest(&owned_private_file(
        &standalone_install::record_path(state, id)?,
        MAX_INTENT,
    )?);
    released(plan.origin(), plan.reservation_target(), state, id)?;
    let plan_sha256 = digest(&owned_private_file(
        &standalone_install::plan_path(state, id)?,
        MAX_PLAN,
    )?);
    validate_base_consent(&plan, &plan_sha256)?;
    if matches!(
        plan.purpose,
        StandaloneInstallPurpose::PrivateRelocation { .. }
    ) {
        validate_parent(&plan)?;
    }
    let path = standalone_install::replacement_path(state, id)?;
    let intent_bytes = owned_private_file(&path, installation::MAX_REPLACEMENT_INTENT)?;
    let intent = installation::load_historical_replacement_intent(&path, id)?;
    let receipt_path = standalone_install::relocation_commit_path(state, id)?;
    if !installation::relocation_committed(&path, &intent)? {
        return Err("relocation has no atomic disk commit receipt".into());
    }
    let receipt_bytes = owned_private_file(&receipt_path, MAX_PLAN)?;
    let receipt: Commit = serde_json::from_slice(&receipt_bytes).map_err(|e| e.to_string())?;
    validate_relocation_disk_evidence(&plan, &record, &receipt, &intent, &digest(&intent_bytes))?;
    let ReplacementLayout::Relocate {
        retained,
        source_sidecar,
        ..
    } = &intent.layout
    else {
        return Err("relocation approval has an exchange replacement intent".into());
    };
    validate_closure(
        retained,
        source_sidecar,
        intent
            .original_file
            .as_ref()
            .ok_or("relocation lacks source inode")?,
        &plan.original.app_version,
    )?;
    validate_source_retired(&plan)?;
    if let StandaloneInstallPurpose::ManualRecovery { release, .. } = &plan.purpose {
        // The candidate is an independently signed official release, not a
        // byte-identical copy of the stopped beta source.
        let candidate = installation::validate_staged_app(&plan.candidate_bundle)?;
        if candidate.image != plan.candidate_image
            || candidate.version != plan.candidate_version
            || crate::owned_file(
                &installation::sidecar_path(&plan.candidate_bundle),
                MAX_INTENT,
            )? != intent.candidate_sidecar
        {
            return Err(
                "manual recovery candidate differs from approved signed release closure".into(),
            );
        }
        installation::validate_release_evidence(
            release,
            plan.candidate_bundle
                .parent()
                .ok_or("manual candidate has no resources directory")?,
            &plan.candidate_bundle,
        )?;
    }
    Ok(Some(Base {
        plan,
        approval,
        plan_sha256,
        receipt_sha256: digest(&receipt_bytes),
        result_sha256,
        intent,
    }))
}
fn validate_relocation_disk_evidence(
    plan: &StandaloneInstallPlan,
    record: &standalone_install::StandaloneInstallRecord,
    receipt: &Commit,
    intent: &ReplacementIntent,
    intent_sha256: &str,
) -> Result<(), String> {
    record.validate(plan)?;
    installation::validate_relocation_intent_for_plan(intent, plan)?;
    let source_closure_sha256 = match &plan.purpose {
        StandaloneInstallPurpose::PrivateRelocation {
            source_closure_sha256,
            ..
        }
        | StandaloneInstallPurpose::ManualRecovery {
            source_closure_sha256,
            ..
        } => source_closure_sha256,
        StandaloneInstallPurpose::Upgrade => {
            return Err("upgrade has no relocation disk authority".into())
        }
    };
    let ReplacementLayout::Relocate {
        source,
        retained,
        source_sidecar,
        source_sidecar_sha256,
    } = &intent.layout
    else {
        return Err("relocation has no source retirement layout".into());
    };
    let installed = record
        .result
        .installation
        .as_ref()
        .ok_or("applied relocation lacks installed result")?;
    let id = &plan.operation_id;
    if !base_result_ready(plan, record)
        || receipt.version != 1
        || receipt.operation_id != *id
        || receipt.intent_sha256 != intent_sha256
        || intent.version != 2
        || intent.origin != *plan.installed_origin()
        || intent.operation_id != *id
        || intent.disposition != installation::ReplacementDisposition::RetainCompatibleClients
        || intent.original_file.as_ref().is_none_or(|file| {
            file.sha256 != plan.original.image.sha256
                || (file.device, file.inode)
                    == (intent.candidate_file.device, intent.candidate_file.inode)
        })
        || intent.original.as_ref() != Some(&plan.original)
        || source != crate::physical_install_root(plan.origin())?
        || intent.target != crate::physical_install_root(plan.installed_origin())?
        || retained != &intent.work.join("original/DesktopPet.app")
        || record.result.retained_bundle.as_ref() != Some(retained)
        || installed.origin != *plan.installed_origin()
        || installed.image.sha256 != plan.candidate_image.sha256
        || installed.executable != intent.target.join("Contents/MacOS/desktop-pet")
        || installed.app_version != plan.candidate_version
        || intent.candidate_file.sha256 != plan.candidate_image.sha256
        || intent.candidate_version != plan.candidate_version
        || matches!(
            plan.purpose,
            StandaloneInstallPurpose::PrivateRelocation { .. }
        ) && source_sidecar != &intent.candidate_sidecar
        || source_sidecar_sha256 != source_closure_sha256
        || digest(source_sidecar) != *source_closure_sha256
        || digest(&intent.candidate_sidecar) != intent.candidate_sidecar_sha256
        || matches!(
            plan.purpose,
            StandaloneInstallPurpose::PrivateRelocation { .. }
        ) && intent.candidate_sidecar_sha256 != *source_closure_sha256
        || intent.original_sidecar.is_some()
        || !intent.update_locator
    {
        return Err(
            "relocation result/consent/intent/atomic commit evidence is inconsistent".into(),
        );
    }
    Ok(())
}
fn normal_links(base: &Base) -> Result<(Vec<Link>, BTreeSet<String>), String> {
    let state = &base.plan.profile.state_dir;
    let directory = state.join("updates-v4");
    let mut links = Vec::new();
    let mut operations = BTreeSet::new();
    let mut history = BTreeSet::new();
    for entry in fs::read_dir(&directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(id) = name.strip_suffix(".operation.lock") {
            protocol::validate_operation_id(id)?;
            history.insert(id.to_owned());
            if history.len() > HISTORY_BOUND {
                return Err("Tern owner operation history exceeds bound".into());
            }
            // A staged child can hold its lease before its approval/reservation
            // is published. Absence of a plan is never evidence it did not run.
            if id != base.plan.operation_id {
                let _lease = protocol::lock_operation(state, id)?;
                operations.insert(id.to_owned());
            }
            continue;
        }
        if let Some(id) = plan_id(name, "install-")? {
            history.insert(id.clone());
            if history.len() > HISTORY_BOUND {
                return Err("Tern owner operation history exceeds bound".into());
            }
            let approval = standalone_install::read_plan(state, &id)?;
            if id != base.plan.operation_id && approval.profile == base.plan.profile {
                let _lease = protocol::lock_operation(state, &id)?;
                released(approval.origin(), approval.reservation_target(), state, &id)?;
                operations.insert(id);
            }
            continue;
        }
        let Some(id) = plan_id(name, "")? else {
            continue;
        };
        history.insert(id.clone());
        if history.len() > HISTORY_BOUND {
            return Err("Tern owner operation history exceeds bound".into());
        }
        let plan = crate::read_plan(state, &id)?;
        if plan.context.config_dir != base.plan.profile.config_dir
            || plan.context.state_dir != *state
            || plan.origin != *base.plan.installed_origin()
            || plan.target_origin() != base.plan.installed_origin()
        {
            continue;
        }
        let _lease = protocol::lock_operation(state, &id)?;
        released(
            &plan.origin,
            plan.target.as_ref().map(|target| &target.origin),
            state,
            &id,
        )?;
        operations.insert(id.clone());
        if plan.action != crate::UpdateAction::StandaloneReplace {
            continue;
        }
        let result_path = directory.join(format!("{id}.result.json"));
        let result_bytes = owned_private_file(&result_path, MAX_PLAN)?;
        let result: protocol::OperationRecord =
            serde_json::from_slice(&result_bytes).map_err(|e| e.to_string())?;
        // Every retained intent is classified from authenticated disk evidence
        // below, including failed attempts whose original inode was restored.
        let intent_path = directory.join(format!("{id}.bundle-replacement.json"));
        if !exists(&intent_path)? {
            if result.phase == protocol::OperationPhase::Completed && result.installed {
                return Err("completed owner update has no retained replacement intent".into());
            }
            continue;
        }
        let intent = installation::load_historical_replacement_intent(&intent_path, &id)?;
        links.push(Link {
            plan,
            result,
            plan_sha256: digest(&owned_private_file(
                &directory.join(format!("{id}.plan.json")),
                MAX_PLAN,
            )?),
            result_sha256: digest(&result_bytes),
            intent,
        });
    }
    Ok((links, operations))
}
fn validate_exchange_evidence(base: &Base, link: &Link) -> Result<(), String> {
    let original = link
        .intent
        .original
        .as_ref()
        .ok_or("owner updater intent has no original")?;
    validate_updater_approval(&link.plan)?;
    let expected = ExecutableIdentity {
        path: link.intent.target.join("Contents/MacOS/desktop-pet"),
        sha256: link.intent.candidate_file.sha256.clone(),
    };
    if link.plan.version != 4
        || link.result.version != 4
        || link.result.operation_id != link.plan.operation_id
        || link.plan.context.config_dir != base.plan.profile.config_dir
        || link.plan.context.state_dir != base.plan.profile.state_dir
        || link.plan.context.running != link.plan.running
        || link.plan.origin != *base.plan.installed_origin()
        || link.plan.target_origin() != base.plan.installed_origin()
        || original.app_version != link.plan.context.version
        || link
            .result
            .candidate
            .as_ref()
            .is_some_and(|candidate| candidate != &expected)
    {
        return Err("owner updater approval/result differs from exact replacement scope".into());
    }
    if link.intent.origin != *base.plan.installed_origin()
        || original.origin != link.intent.origin
        || original.image != link.plan.running
        || !matches!(link.intent.layout, ReplacementLayout::Exchange)
        || link.intent.target != crate::physical_install_root(base.plan.installed_origin())?
        || link
            .plan
            .target
            .as_ref()
            .is_none_or(|target| target.release.version != link.intent.candidate_version)
        || link.plan.action != crate::UpdateAction::StandaloneReplace
        || link.intent.operation_id != link.plan.operation_id
        || link.intent.disposition != installation::ReplacementDisposition::RetainCompatibleClients
        || link.intent.original_file.as_ref().is_none_or(|file| {
            file.sha256 != original.image.sha256
                || (file.device, file.inode)
                    == (
                        link.intent.candidate_file.device,
                        link.intent.candidate_file.inode,
                    )
        })
        || digest(&link.intent.candidate_sidecar) != link.intent.candidate_sidecar_sha256
        || link.intent.original_sidecar.as_deref().map(digest)
            != link.intent.original_sidecar_sha256
    {
        return Err("Tern owner intent is not an exact signed updater replacement".into());
    }
    Ok(())
}
fn validate_link_evidence(base: &Base, link: &Link) -> Result<(), String> {
    validate_exchange_evidence(base, link)?;
    let expected = ExecutableIdentity {
        path: link.intent.target.join("Contents/MacOS/desktop-pet"),
        sha256: link.intent.candidate_file.sha256.clone(),
    };
    verify_completed_owner_update(
        &link.plan,
        &link.result,
        base.plan.installed_origin(),
        &base.plan.profile,
        &expected,
    )
}
fn link_settled(base: &Base, link: &Link) -> Result<(), String> {
    let _lease = protocol::lock_operation(&base.plan.profile.state_dir, &link.plan.operation_id)?;
    validate_link_evidence(base, link)?;
    let original = link
        .intent
        .original
        .as_ref()
        .ok_or("owner updater intent has no original")?;
    released(
        &link.plan.origin,
        Some(link.plan.target_origin()),
        &base.plan.profile.state_dir,
        &link.plan.operation_id,
    )?;
    let sidecar = link
        .intent
        .original_sidecar
        .as_deref()
        .ok_or("owner updater lacks retained original sidecar")?;
    if link.intent.original_sidecar_sha256.as_deref() != Some(digest(sidecar).as_str()) {
        return Err("owner updater original sidecar digest changed".into());
    }
    validate_closure(
        &link.intent.prepared,
        sidecar,
        link.intent
            .original_file
            .as_ref()
            .ok_or("owner updater lacks original inode")?,
        &original.app_version,
    )?;
    Ok(())
}
fn publish_exact<T: Serialize + serde::de::DeserializeOwned + PartialEq>(
    path: &Path,
    value: &T,
) -> Result<Vec<u8>, String> {
    if !exists(path)? {
        let proposed = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        match protocol::write_private_atomic_new(path, &proposed, 0o600) {
            Ok(()) => {}
            Err(error) if !exists(path)? => return Err(error),
            Err(_) => {} // An exclusive-publication loser must authenticate the winner.
        }
    }
    let bytes = owned_private_file(path, MAX_INTENT)?;
    let winner: T = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if winner != *value {
        return Err(
            "immutable derived Tern owner evidence conflicts with authenticated history".into(),
        );
    }
    File::open(
        path.parent()
            .ok_or("derived owner evidence has no parent")?,
    )
    .and_then(|directory| directory.sync_all())
    .map_err(|e| e.to_string())?;
    Ok(bytes)
}
fn owner_path(base: &Base, image: &ExecutableIdentity) -> PathBuf {
    base.plan.profile.state_dir.join("updates-v4").join(format!(
        "install-{}.tern-relocation-owner-{}.json",
        base.plan.operation_id, image.sha256
    ))
}
#[derive(Default)]
struct OwnerAccess {
    #[cfg(test)]
    global: Option<PathBuf>,
    #[cfg(test)]
    closures: BTreeMap<PathBuf, (Vec<u8>, String)>,
}
impl OwnerAccess {
    fn lease(&self, state: &Path, id: &str) -> Result<protocol::OperationLease, String> {
        #[cfg(test)]
        if let Some(global) = &self.global {
            return protocol::test_scope::lock_operation_at(global, state, id)?
                .ok_or_else(|| "owner helper lease remains held".into());
        }
        protocol::lock_operation(state, id)
    }
    fn released(
        &self,
        source: &InstallOrigin,
        target: Option<&InstallOrigin>,
        state: &Path,
        id: &str,
    ) -> Result<(), String> {
        #[cfg(test)]
        if let Some(global) = &self.global {
            if protocol::test_scope::reservation_exists_at(global, source, target, state, id)? {
                return Err("owner operation remains reserved".into());
            }
            protocol::test_scope::start_allowed_at(global, source, state, None, None)?;
            if let Some(target) = target {
                protocol::test_scope::start_allowed_at(global, target, state, None, None)?;
            }
            return Ok(());
        }
        released(source, target, state, id)
    }
    fn closure(
        &self,
        bundle: &Path,
        sidecar: &[u8],
        file: &FileSnapshot,
        version: &str,
    ) -> Result<(), String> {
        #[cfg(test)]
        if let Some((signed_sidecar, signed_version)) = self.closures.get(bundle) {
            if sidecar != signed_sidecar
                || version != signed_version
                || FileSnapshot::capture(&bundle.join("Contents/MacOS/desktop-pet"))? != *file
                || crate::owned_file(&installation::sidecar_path(bundle), MAX_INTENT)? != sidecar
            {
                return Err("fixture validated signed-image boundary changed".into());
            }
            return Ok(());
        }
        validate_closure(bundle, sidecar, file, version).map(|_| ())
    }
}
fn validate_rolled_back(
    base: &Base,
    link: &Link,
    links: &[Link],
    access: &OwnerAccess,
) -> Result<(), String> {
    let state = &base.plan.profile.state_dir;
    let _lease = access.lease(state, &link.plan.operation_id)?;
    validate_exchange_evidence(base, link)?;
    if link.result.phase != protocol::OperationPhase::Failed
        || link.result.installed
        || link.result.applied
        || link.result.execution_fence != protocol::ExecutionFence::ManagerExited
    {
        return Err("owner replacement rollback lacks terminal execution evidence".into());
    }
    access.released(
        &link.plan.origin,
        Some(link.plan.target_origin()),
        state,
        &link.plan.operation_id,
    )?;
    let original = link
        .intent
        .original
        .as_ref()
        .ok_or("rollback lacks original installation")?;
    let file = link
        .intent
        .original_file
        .as_ref()
        .ok_or("rollback lacks original inode")?;
    let sidecar = link
        .intent
        .original_sidecar
        .as_deref()
        .ok_or("rollback lacks original closure")?;
    access.closure(
        &link.intent.prepared,
        &link.intent.candidate_sidecar,
        &link.intent.candidate_file,
        &link.intent.candidate_version,
    )?;
    // A later retry may have moved the restored original out of C. Only one
    // completed successor with the same immutable predecessor can retain it.
    let mut retainers = links.iter().filter(|next| {
        next.result.phase == protocol::OperationPhase::Completed
            && next.plan.running == link.plan.running
    });
    let retainer = retainers.next();
    if retainers.next().is_some() {
        return Err("rollback original has ambiguous retained successors".into());
    }
    if let Some(next) = retainer {
        let _next_lease = access.lease(state, &next.plan.operation_id)?;
        validate_link_evidence(base, next)?;
        access.released(
            &next.plan.origin,
            Some(next.plan.target_origin()),
            state,
            &next.plan.operation_id,
        )?;
        if next.intent.original_file.as_ref() != Some(file)
            || next.intent.original_sidecar.as_deref() != Some(sidecar)
            || next.intent.original.as_ref() != Some(original)
            || next.intent.original_locator != link.intent.original_locator
        {
            return Err(
                "later successor does not retain the exact restored rollback original".into(),
            );
        }
        access.closure(&next.intent.prepared, sidecar, file, &original.app_version)?;
    } else {
        access.closure(&link.intent.target, sidecar, file, &original.app_version)?;
        let locator = if exists(&link.intent.locator)? {
            Some(owned_private_file(&link.intent.locator, MAX_INTENT)?)
        } else {
            None
        };
        if locator != link.intent.original_locator {
            return Err("rollback did not restore original locator metadata".into());
        }
    }
    access.released(
        &link.plan.origin,
        Some(link.plan.target_origin()),
        state,
        &link.plan.operation_id,
    )
}
fn successor_chain<'a>(
    base: &Base,
    current: &ExecutableIdentity,
    links: &'a [Link],
) -> Result<Vec<&'a Link>, String> {
    successor_chain_with_access(base, current, links, &OwnerAccess::default())
}
fn successor_chain_with_access<'a>(
    base: &Base,
    current: &ExecutableIdentity,
    links: &'a [Link],
    access: &OwnerAccess,
) -> Result<Vec<&'a Link>, String> {
    for link in links {
        if link.result.phase != protocol::OperationPhase::Completed {
            validate_rolled_back(base, link, links, access)?;
        }
    }
    let mut selected = Vec::new();
    let mut image = ExecutableIdentity {
        path: base.intent.target.join("Contents/MacOS/desktop-pet"),
        sha256: base.intent.candidate_file.sha256.clone(),
    };
    let mut file = &base.intent.candidate_file;
    let mut sidecar = base.intent.candidate_sidecar.as_slice();
    let mut version = base.intent.candidate_version.as_str();
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(image.sha256.clone()) || visited.len() > HISTORY_BOUND {
            return Err("Tern owner ancestry has a cycle or exceeds bound".into());
        }
        let mut successors = links.iter().filter(|link| {
            link.plan.running == image && link.result.phase == protocol::OperationPhase::Completed
        });
        let next = successors.next();
        if successors.next().is_some() {
            return Err("Tern owner ancestry has ambiguous successor branches".into());
        }
        let Some(next) = next else {
            if &image != current {
                return Err("Tern owner ancestry is missing a signed updater successor".into());
            }
            break;
        };
        validate_link_evidence(base, next)?;
        if next.intent.original_file.as_ref() != Some(file)
            || next.intent.original_sidecar.as_deref() != Some(sidecar)
            || next
                .intent
                .original
                .as_ref()
                .is_none_or(|original| original.app_version != version)
        {
            return Err(
                "Tern owner successor substituted its predecessor inode/complete sidecar".into(),
            );
        }
        selected.push(next);
        image = ExecutableIdentity {
            path: next.intent.target.join("Contents/MacOS/desktop-pet"),
            sha256: next.intent.candidate_file.sha256.clone(),
        };
        file = &next.intent.candidate_file;
        sidecar = &next.intent.candidate_sidecar;
        version = &next.intent.candidate_version;
    }
    Ok(selected)
}
fn reconstruct_owners(
    base: &Base,
    live: &crate::native_runtime::NativeInstance,
) -> Result<(String, Vec<String>), String> {
    let (links, mut operations) = normal_links(base)?;
    let selected = successor_chain(base, &live.image, &links)?;
    for link in &selected {
        link_settled(base, link)?;
    }
    let (file, sidecar, version) = selected.last().map_or(
        (
            &base.intent.candidate_file,
            base.intent.candidate_sidecar.as_slice(),
            base.intent.candidate_version.as_str(),
        ),
        |link| {
            (
                &link.intent.candidate_file,
                link.intent.candidate_sidecar.as_slice(),
                link.intent.candidate_version.as_str(),
            )
        },
    );
    validate_closure(&base.intent.target, sidecar, file, version)?;
    if let StandaloneInstallPurpose::PrivateRelocation { parent, .. } = &base.plan.purpose {
        operations.insert(parent.operation_id.clone());
    }
    let mut leases = Vec::with_capacity(operations.len() + 1);
    leases.push(protocol::lock_operation(
        &base.plan.profile.state_dir,
        &base.plan.operation_id,
    )?);
    for id in &operations {
        leases.push(protocol::lock_operation(&base.plan.profile.state_dir, id)?);
    }
    released(
        base.plan.origin(),
        base.plan.reservation_target(),
        &base.plan.profile.state_dir,
        &base.plan.operation_id,
    )?;
    observe_manual_start(base)?;
    let hash = publish_owners(base, &selected)?;
    Ok((hash, operations.into_iter().collect()))
}
fn publish_owners(base: &Base, selected: &[&Link]) -> Result<String, String> {
    // Called only after the complete retained chain and live head are verified.
    let initial = ExecutableIdentity {
        path: base.intent.target.join("Contents/MacOS/desktop-pet"),
        sha256: base.intent.candidate_file.sha256.clone(),
    };
    let mut owner = RelocationOwner {
        version: 1,
        relocation_id: base.plan.operation_id.clone(),
        relocation_plan_sha256: base.plan_sha256.clone(),
        predecessor_sha256: None,
        previous_image: initial.clone(),
        current_image: initial,
        previous_closure_sha256: base.intent.candidate_sidecar_sha256.clone(),
        current_closure_sha256: base.intent.candidate_sidecar_sha256.clone(),
        update_operation_id: None,
        update_plan_sha256: None,
        update_result_sha256: None,
        origin: base.plan.installed_origin().clone(),
        profile: base.plan.profile.clone(),
    };
    let mut hash = digest(&publish_exact(
        &owner_path(base, &owner.current_image),
        &owner,
    )?);
    for link in selected {
        owner = RelocationOwner {
            version: 1,
            relocation_id: base.plan.operation_id.clone(),
            relocation_plan_sha256: base.plan_sha256.clone(),
            predecessor_sha256: Some(hash),
            previous_image: owner.current_image.clone(),
            current_image: ExecutableIdentity {
                path: link.intent.target.join("Contents/MacOS/desktop-pet"),
                sha256: link.intent.candidate_file.sha256.clone(),
            },
            previous_closure_sha256: owner.current_closure_sha256.clone(),
            current_closure_sha256: link.intent.candidate_sidecar_sha256.clone(),
            update_operation_id: Some(link.plan.operation_id.clone()),
            update_plan_sha256: Some(link.plan_sha256.clone()),
            update_result_sha256: Some(link.result_sha256.clone()),
            origin: base.plan.installed_origin().clone(),
            profile: base.plan.profile.clone(),
        };
        hash = digest(&publish_exact(
            &owner_path(base, &owner.current_image),
            &owner,
        )?);
    }
    Ok(hash)
}
fn authority(base: Base) -> Result<Authority, String> {
    let live =
        crate::native_runtime::capture_instance(base.plan.installed_origin(), &base.plan.profile)?;
    let generation = protocol::verified_runtime_start_fence(
        base.plan.installed_origin(),
        &base.plan.profile.state_dir,
        live.incarnation.pid,
        &live.instance_id,
        &live.image,
    )?;
    crate::native_runtime::verify_current_ready(&live, generation)?;
    let (owner_sha256, owner_operations) = reconstruct_owners(&base, &live)?;
    let result = Authority {
        base,
        owner_sha256,
        owner_operations,
        live,
        generation,
    };
    recheck(&result)?;
    Ok(result)
}
fn recheck(authority: &Authority) -> Result<Vec<protocol::OperationLease>, String> {
    let base = &authority.base;
    let mut leases = Vec::with_capacity(authority.owner_operations.len() + 1);
    leases.push(protocol::lock_operation(
        &base.plan.profile.state_dir,
        &base.plan.operation_id,
    )?);
    for id in &authority.owner_operations {
        leases.push(protocol::lock_operation(&base.plan.profile.state_dir, id)?);
    }
    released(
        base.plan.origin(),
        base.plan.reservation_target(),
        &base.plan.profile.state_dir,
        &base.plan.operation_id,
    )?;
    let state = &base.plan.profile.state_dir;
    let id = &base.plan.operation_id;
    let receipt_bytes = owned_private_file(
        &standalone_install::relocation_commit_path(state, id)?,
        MAX_PLAN,
    )?;
    let receipt: Commit = serde_json::from_slice(&receipt_bytes).map_err(|e| e.to_string())?;
    let owner_bytes = owned_private_file(&owner_path(base, &authority.live.image), MAX_PLAN)?;
    let owner: RelocationOwner = serde_json::from_slice(&owner_bytes).map_err(|e| e.to_string())?;
    if digest(&owned_private_file(
        &standalone_install::plan_path(state, id)?,
        MAX_PLAN,
    )?) != base.plan_sha256
        || digest(&owned_private_file(
            &standalone_install::record_path(state, id)?,
            MAX_INTENT,
        )?) != base.result_sha256
        || digest(&receipt_bytes) != base.receipt_sha256
        || receipt.intent_sha256
            != digest(&owned_private_file(
                &standalone_install::replacement_path(state, id)?,
                installation::MAX_REPLACEMENT_INTENT,
            )?)
        || digest(&owner_bytes) != authority.owner_sha256
        || owner.current_image != authority.live.image
        || owner.profile != base.plan.profile
        || owner.current_closure_sha256
            != digest(&crate::owned_file(
                &installation::sidecar_path(&base.intent.target),
                MAX_INTENT,
            )?)
    {
        return Err(
            "immutable relocation plan/result/consent/commit/owner evidence changed".into(),
        );
    }
    validate_base_consent(&base.plan, &base.plan_sha256)?;
    validate_source_retired(&base.plan)?;
    if matches!(
        base.plan.purpose,
        StandaloneInstallPurpose::ManualRecovery { .. }
    ) {
        let observed: ManualInstalledStoppedObservation =
            read_json(&manual_observation_path(base)?, MAX_PLAN)?;
        validate_manual_start_receipt(base, &observed)?;
    }
    if protocol::stop_generation(&base.plan.profile.state_dir)? != authority.generation
        || protocol::verified_runtime_start_fence(
            base.plan.installed_origin(),
            &base.plan.profile.state_dir,
            authority.live.incarnation.pid,
            &authority.live.instance_id,
            &authority.live.image,
        )? != authority.generation
    {
        return Err("Stop or current instance superseded Tern relocation handover".into());
    }
    crate::native_runtime::verify_current_ready(&authority.live, authority.generation)?;
    Ok(leases)
}
fn consumer(authority: &Authority) -> Result<(PathBuf, String, String), String> {
    tern_config::verify_relocation_routing(&authority.live.context, &authority.base.approval)?;
    let bundle = crate::physical_install_root(authority.base.plan.installed_origin())?;
    let app = installation::validate_staged_app(bundle)?;
    if app.image != authority.live.image {
        return Err("Tern consumer owner changed".into());
    }
    let expected_package = bundle.join("Contents/Resources/integrations/tern-desktop-pet");
    let installed = tern_config::relocation_installed_package(&authority.live.context.environment)?;
    let (expected, revision) = qualified_consumer(&expected_package, &installed)?;
    Ok((installed, expected, revision))
}
fn qualified_consumer(
    expected_package: &Path,
    installed: &Path,
) -> Result<(String, String), String> {
    let (expected, revision) = package_identity(expected_package)?;
    let (actual, actual_revision) = package_identity(installed)
        .map_err(|error| format!("Tern integration pending/incompatible: {error}"))?;
    if actual != expected || actual_revision != revision {
        return Err("Tern integration pending/incompatible: installed consumer differs from the signed current owner".into());
    }
    Ok((expected, revision))
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelocationHandoverChallenge {
    version: u32,
    handover: Handover,
    original: TernConnection,
    target: TernConnection,
    commit_receipt_sha256: String,
    owner_proof_sha256: String,
    current_image: ExecutableIdentity,
    stop_generation: u64,
    installed_package: PathBuf,
    package_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelocationHandoverReceipt {
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
    local_host: String,
}
fn evidence_directory(base: &Base) -> Result<PathBuf, String> {
    protocol::validate_operation_id(&base.plan.operation_id)?;
    let updates = protocol::private_updates(&base.plan.profile.state_dir)?;
    owned_directory(&updates)?;
    let directory = updates.join(format!(
        "install-{}.tern-relocation",
        base.plan.operation_id
    ));
    protocol::check_dir(&directory, true)?;
    File::open(&updates)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())?;
    owned_directory(&directory)?;
    Ok(directory)
}
fn challenge_path(
    base: &Base,
    image: &ExecutableIdentity,
    instance: &str,
    original: &TernConnection,
) -> Result<PathBuf, String> {
    let tuple = digest(&serde_json::to_vec(original).map_err(|e| e.to_string())?);
    Ok(evidence_directory(base)?.join(format!(
        "handover-{}-{}-{}.json",
        image.sha256,
        digest(instance.as_bytes()),
        tuple
    )))
}
fn receipt_path(
    base: &Base,
    image: &ExecutableIdentity,
    instance: &str,
    window: &str,
) -> Result<PathBuf, String> {
    Ok(evidence_directory(base)?.join(format!(
        "confirmed-{}-{}-{}.json",
        image.sha256,
        digest(instance.as_bytes()),
        digest(window.as_bytes())
    )))
}
fn route_tuple(base: &Base, original: &TernConnection) -> Result<(), String> {
    if original != &base.approval.original && original != &base.approval.target {
        return Err(
            "Tern relocation request is neither the captured H tuple nor the saved C tuple".into(),
        );
    }
    Ok(())
}
fn issue(authority: &Authority, original: &TernConnection) -> Result<Handover, String> {
    route_tuple(&authority.base, original)?;
    let (installed_package, package_sha256, revision) = consumer(authority)?;
    let path = challenge_path(
        &authority.base,
        &authority.live.image,
        &authority.live.instance_id,
        original,
    )?;
    let challenge = RelocationHandoverChallenge {
        version: 1,
        handover: Handover {
            operation_id: authority.base.plan.operation_id.clone(),
            plan_path: standalone_install::plan_path(
                &authority.base.plan.profile.state_dir,
                &authority.base.plan.operation_id,
            )?,
            plan_sha256: authority.base.plan_sha256.clone(),
            token: String::new(),
            consumer_revision: revision,
            target_instance_id: authority.live.instance_id.clone(),
        },
        original: original.clone(),
        target: authority.base.approval.target.clone(),
        commit_receipt_sha256: authority.base.receipt_sha256.clone(),
        owner_proof_sha256: authority.owner_sha256.clone(),
        current_image: authority.live.image.clone(),
        stop_generation: authority.generation,
        installed_package,
        package_sha256,
    };
    let _leases = recheck(authority)?;
    let handover = publish_challenge(&path, challenge)?;
    drop(_leases);
    recheck(authority)?;
    Ok(handover)
}
fn publish_challenge(
    path: &Path,
    mut challenge: RelocationHandoverChallenge,
) -> Result<Handover, String> {
    if !exists(path)? {
        challenge.handover.token = random_hex(32)?;
        match protocol::write_private_atomic_new(
            path,
            &serde_json::to_vec(&challenge).map_err(|e| e.to_string())?,
            0o600,
        ) {
            Ok(()) => {}
            Err(error) if !exists(path)? => return Err(error),
            Err(_) => {}
        }
    }
    let winner: RelocationHandoverChallenge = read_json(path, MAX_PLAN)?;
    if !hex(&winner.handover.token, 64) {
        return Err("Tern relocation challenge token is invalid".into());
    }
    challenge.handover.token = winner.handover.token.clone();
    if winner != challenge {
        return Err(
            "immutable Tern relocation challenge differs from current owner/tuple/instance".into(),
        );
    }
    File::open(path.parent().ok_or("relocation challenge has no parent")?)
        .and_then(|directory| directory.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(winner.handover)
}
/// A separately authenticated standalone cutover supersedes only the old
/// native owner at its exact source root/profile. Its SSH approval does not
/// inherit the old CLI/delivery authority, and never relaxes legacy handover.
pub(super) fn supersedes_legacy_route(
    response: &serde_json::Value,
    legacy: &MigrationPlan,
    record: &MigrationRecord,
    profile: &LegacyProfile,
) -> Result<bool, String> {
    let id = response
        .get("handover")
        .and_then(|h| h.get("operation_id"))
        .and_then(|id| id.as_str())
        .ok_or("standalone route has no operation identity")?;
    let plan = standalone_install::read_plan(&profile.state_dir, id)?;
    if response
        .get("handover")
        .and_then(|h| h.get("plan_sha256"))
        .and_then(|v| v.as_str())
        != Some(
            digest(&owned_private_file(
                &standalone_install::plan_path(&profile.state_dir, id)?,
                MAX_PLAN,
            )?)
            .as_str(),
        )
    {
        return Err("standalone route approval changed before legacy history selection".into());
    }
    if relocation_approval(&plan).is_none()
        || plan.profile.config_dir != profile.config_dir
        || plan.profile.state_dir != profile.state_dir
        || legacy.target.origin != *plan.origin()
    {
        return Ok(false);
    }
    verify_retained_commit(legacy, record, profile)?;
    Ok(true)
}
pub(super) fn committed_route(request: &RouteRequest) -> Result<Option<serde_json::Value>, String> {
    if crate::identity::LEGACY {
        return Ok(None);
    }
    let directory = request.state_dir.join("updates-v4");
    let original = TernConnection {
        version: 1,
        executable: request.executable.clone(),
        config_dir: request.config_dir.clone(),
        state_dir: request.state_dir.clone(),
    };
    let mut selected = None;
    let mut count = 0;
    for entry in fs::read_dir(&directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(id) = plan_id(name, "install-")? else {
            continue;
        };
        count += 1;
        if count > HISTORY_BOUND {
            return Err("Tern relocation history exceeds bound".into());
        }
        // The shared reader rejects malformed recognized evidence, including
        // forged v1 purpose, while valid v1/v2 Upgrades are explicitly skipped.
        let plan = standalone_install::read_plan(&request.state_dir, &id)?;
        if plan.purpose.is_upgrade() {
            continue;
        }
        if plan.profile.config_dir != request.config_dir {
            continue;
        }
        let Some(base) = read_base(&request.state_dir, &id)? else {
            continue;
        };
        if route_tuple(&base, &original).is_err() {
            continue;
        }
        let authority = authority(base)?;
        if selected.replace(authority).is_some() {
            return Err("ambiguous independent relocation Tern authorities".into());
        }
    }
    let Some(authority) = selected else {
        return Ok(None);
    };
    let handover = issue(&authority, &original)?;
    Ok(Some(
        serde_json::json!({"version":1,"kind":"legacy-migration-route","committed":true,"original":original,"target":authority.base.approval.target,"handover":handover}),
    ))
}
pub(super) fn integration_handover(
    config: &Path,
    state: &Path,
    request: &HandoverRequest,
) -> Result<serde_json::Value, String> {
    let base = read_base(state, &request.handover.operation_id)?
        .ok_or("standalone upgrade or incomplete relocation has no handover authority")?;
    if base.plan.profile.config_dir != config {
        return Err("Tern relocation handover selects another profile".into());
    }
    route_tuple(&base, &request.original)?;
    let authority = authority(base)?;
    let executing = executable_identity(&std::env::current_exe().map_err(|e| e.to_string())?)?;
    let process = crate::processes::capture_process(std::process::id())?;
    if process.path != authority.live.image.path
        || process.mapped_file != authority.live.mapped_file
    {
        return Err("Tern relocation handover is not executing the exact current C vnode".into());
    }
    if executing != authority.live.image
        || request.handover.target_instance_id != authority.live.instance_id
        || request.target != authority.base.approval.target
    {
        return Err("Tern relocation handover executing image/target/live instance changed".into());
    }
    let expected = issue(&authority, &request.original)?;
    if expected != request.handover {
        return Err("Tern relocation handover token or immutable approval changed".into());
    }
    let challenge_bytes = owned_private_file(
        &challenge_path(
            &authority.base,
            &authority.live.image,
            &authority.live.instance_id,
            &request.original,
        )?,
        MAX_PLAN,
    )?;
    let challenge: RelocationHandoverChallenge =
        serde_json::from_slice(&challenge_bytes).map_err(|e| e.to_string())?;
    tern_config::verify_canonical_target(&authority.base.approval.configuration, &request.target)?;
    let profile = binding_profile(&authority)?;
    let binding = fresh_binding(&profile, &authority.live.instance_id, &request.window_id)?;
    let _leases = recheck(&authority)?;
    consumer(&authority)?;
    tern_config::verify_canonical_target(&authority.base.approval.configuration, &request.target)?;
    let receipt = RelocationHandoverReceipt {
        version: 1,
        operation_id: authority.base.plan.operation_id.clone(),
        plan_sha256: authority.base.plan_sha256.clone(),
        challenge_sha256: digest(&challenge_bytes),
        current_image: authority.live.image.clone(),
        config_dir: config.to_path_buf(),
        state_dir: state.to_path_buf(),
        target_instance_id: authority.live.instance_id.clone(),
        window_id: request.window_id.clone(),
        stop_generation: authority.generation,
        binding_epoch: binding.binding_epoch,
        sequence: binding.sequence,
        observation_generation: binding.observation_generation,
        native_generation: binding.native_generation,
        consumer_revision: challenge.handover.consumer_revision,
        package_sha256: challenge.package_sha256,
        local_host: binding.local_host,
    };
    // Per-window replacement refreshes only that exact window's fresh receipt.
    protocol::write_private_bytes(
        &receipt_path(
            &authority.base,
            &authority.live.image,
            &authority.live.instance_id,
            &request.window_id,
        )?,
        &serde_json::to_vec(&receipt).map_err(|e| e.to_string())?,
        0o600,
    )?;
    Ok(
        serde_json::json!({"version":1,"kind":"migration-integration-handover","confirmed":true,"window_id":request.window_id,"target_instance_id":authority.live.instance_id}),
    )
}
fn binding_profile(authority: &Authority) -> Result<LegacyProfile, String> {
    let context = &authority.live.context;
    Ok(LegacyProfile {
        config_dir: context.config_dir.clone(),
        state_dir: context.state_dir.clone(),
        control_socket: context.state_dir.join("control.sock"),
        pid: authority.live.incarnation.pid,
        instance_id: authority.live.instance_id.clone(),
        stop_generation: authority.generation,
        observation: context.observation_mode()?,
        #[cfg(desktop_pet_legacy)]
        herdr_socket: Some(context.herdr_socket.clone()),
        #[cfg(not(desktop_pet_legacy))]
        herdr_socket: None,
        assets_override: context.assets_override.clone(),
        host_plugin_config_dir: None,
        tern_reconnect_required: true,
        environment: context.environment.clone(),
    })
}
fn observed_receipts(authority: &Authority) -> Result<usize, String> {
    let (_, package_sha256, revision) = consumer(authority)?;
    tern_config::verify_canonical_target(
        &authority.base.approval.configuration,
        &authority.base.approval.target,
    )?;
    let directory = evidence_directory(&authority.base)?;
    let prefix = format!(
        "confirmed-{}-{}-",
        authority.live.image.sha256,
        digest(authority.live.instance_id.as_bytes())
    );
    let profile = binding_profile(authority)?;
    let mut windows = BTreeSet::new();
    let mut count = 0;
    for entry in fs::read_dir(&directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(&prefix) || !name.ends_with(".json") {
            continue;
        }
        count += 1;
        if count > HISTORY_BOUND {
            return Err("Tern relocation window history exceeds bound".into());
        }
        let receipt: RelocationHandoverReceipt = read_json(&entry.path(), MAX_PLAN)?;
        let r = &receipt;
        if entry.path()
            != receipt_path(
                &authority.base,
                &authority.live.image,
                &authority.live.instance_id,
                &r.window_id,
            )?
            || r.version != 1
            || r.operation_id != authority.base.plan.operation_id
            || r.plan_sha256 != authority.base.plan_sha256
            || r.current_image != authority.live.image
            || r.target_instance_id != authority.live.instance_id
            || r.stop_generation != authority.generation
            || r.config_dir != profile.config_dir
            || r.state_dir != profile.state_dir
            || r.consumer_revision != revision
            || r.package_sha256 != package_sha256
        {
            return Err("Tern relocation window receipt differs from current head".into());
        }
        // Both captured-H and already-saved-C challenges remain independent.
        let mut matched = false;
        for tuple in [
            &authority.base.approval.original,
            &authority.base.approval.target,
        ] {
            let path = challenge_path(
                &authority.base,
                &authority.live.image,
                &authority.live.instance_id,
                tuple,
            )?;
            if !exists(&path)? {
                continue;
            }
            let bytes = owned_private_file(&path, MAX_PLAN)?;
            if digest(&bytes) != r.challenge_sha256 {
                continue;
            }
            let challenge: RelocationHandoverChallenge =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if issue(authority, tuple)? != challenge.handover {
                return Err("Tern relocation receipt challenge changed".into());
            }
            matched = true;
        }
        if !matched {
            return Err("Tern relocation receipt has no exact immutable tuple challenge".into());
        }
        if let Ok(binding) = fresh_binding(&profile, &authority.live.instance_id, &r.window_id) {
            if receipt_matches_binding(&receipt, &binding) {
                windows.insert(r.window_id.clone());
            }
        }
    }
    recheck(authority)?;
    Ok(windows.len())
}
/// Native installed/applied success is not conditional on an external Tern window.
/// Only exact currently fresh receipts count; old protocol IDs are never equated.
pub fn observe_relocation_tern(
    context: &UpdateContext,
    operation_id: &str,
) -> Result<Vec<MigrationIntegrationStatus>, String> {
    let plan = standalone_install::read_plan(&context.state_dir, operation_id)?;
    let tern =
        relocation_approval(&plan).ok_or("upgrade is not a relocation integration observation")?;
    if tern.is_none() {
        return Ok(Vec::new());
    }
    if plan.profile.config_dir != context.config_dir {
        return Err("relocation observation profile changed".into());
    }
    let result = (|| {
        let base = read_base(&context.state_dir, operation_id)?
            .ok_or("Tern integration pending: relocation has not settled")?;
        let authority = authority(base)?;
        if authority.live.image != context.running
            || authority.live.instance_id != context.instance_id
        {
            return Err("Tern integration observation belongs to another current instance".into());
        }
        let count = observed_receipts(&authority)?;
        if count == 0 {
            return Err("Tern integration pending: no currently fresh confirmed window".into());
        }
        Ok(())
    })();
    Ok(vec![MigrationIntegrationStatus {
        config_dir: context.config_dir.clone(),
        state_dir: context.state_dir.clone(),
        provider: "tern".into(),
        connected: result.is_ok(),
        error: result.err(),
    }])
}

pub fn observe_relocation_tern_windows(
    context: &UpdateContext,
    operation_id: &str,
) -> Result<Option<usize>, String> {
    let plan = standalone_install::read_plan(&context.state_dir, operation_id)?;
    let tern =
        relocation_approval(&plan).ok_or("upgrade is not a relocation integration observation")?;
    if plan.profile.config_dir != context.config_dir {
        return Err("relocation observation profile changed".into());
    }
    if tern.is_none() {
        return Ok(None);
    }
    let base = read_base(&context.state_dir, operation_id)?
        .ok_or("Tern integration pending: relocation has not settled")?;
    let authority = authority(base)?;
    if authority.live.image != context.running || authority.live.instance_id != context.instance_id
    {
        return Err("Tern integration observation belongs to another current instance".into());
    }
    Ok(Some(observed_receipts(&authority)?))
}

fn receipt_matches_binding(
    receipt: &RelocationHandoverReceipt,
    binding: &TernBindingEvidence,
) -> bool {
    binding.version == 1
        && binding.kind == "tern-migration-evidence"
        && binding.fresh
        && binding.instance_id == receipt.target_instance_id
        && binding.window_id == receipt.window_id
        && binding.local_host == receipt.local_host
        && !binding.local_host.is_empty()
        && binding.binding_epoch >= receipt.binding_epoch
        && receipt.binding_epoch != 0
        && binding.sequence >= receipt.sequence
        && receipt.sequence != 0
        && binding.observation_generation >= receipt.observation_generation
        && receipt.observation_generation != 0
        && binding.native_generation >= receipt.native_generation
        && receipt.native_generation != 0
}

#[cfg(all(test, not(desktop_pet_legacy)))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn private(path: &Path) {
        fs::create_dir_all(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fn image(bundle: &Path, hash: char) -> ExecutableIdentity {
        ExecutableIdentity {
            path: bundle.join("Contents/MacOS/desktop-pet"),
            sha256: hash.to_string().repeat(64),
        }
    }
    fn record(
        origin: &InstallOrigin,
        version: &str,
        hash: char,
    ) -> installation::InstallationRecord {
        let image = image(crate::physical_install_root(origin).unwrap(), hash);
        installation::InstallationRecord {
            version: 1,
            executable: image.path.clone(),
            kind: installation::InstallationKind::Standalone,
            app_version: version.into(),
            protocol: 4,
            observation_mode: 2,
            integration_api: 1,
            origin: origin.clone(),
            image,
        }
    }
    fn context(record: &installation::InstallationRecord, root: &Path) -> UpdateContext {
        UpdateContext {
            executable: record.executable.clone(),
            version: record.app_version.clone(),
            instance_id: "original-H-instance".into(),
            config_dir: root.join("config"),
            state_dir: root.join("state"),
            running: record.image.clone(),
            running_origin: Some(Box::new(record.origin.clone())),
            assets_override: None,
            environment: BTreeMap::from([(
                "TERN_CONFIG_DIR".into(),
                root.join("tern").display().to_string(),
            )]),
            locale: "en".into(),
        }
    }
    fn base(root: &Path) -> Base {
        let physical_root = fs::canonicalize(root).unwrap();
        let root = physical_root.as_path();
        let source = InstallOrigin::Standalone {
            bundle: installation::os_home()
                .unwrap()
                .join("Applications/DesktopPet.app"),
        };
        let target = InstallOrigin::Standalone {
            bundle: installation::standalone_bundle().unwrap(),
        };
        let original = record(&source, "0.4.13", 'a');
        let context = context(&original, root);
        private(&context.config_dir);
        private(&context.state_dir.join("updates-v4"));
        fs::set_permissions(&context.state_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let profile = ProfilePaths {
            config_dir: context.config_dir.clone(),
            state_dir: context.state_dir.clone(),
        };
        let original_tuple = TernConnection {
            version: 1,
            executable: original.executable.clone(),
            config_dir: profile.config_dir.clone(),
            state_dir: profile.state_dir.clone(),
        };
        let target_tuple = TernConnection {
            executable: crate::physical_install_root(&target)
                .unwrap()
                .join("Contents/MacOS/desktop-pet"),
            ..original_tuple.clone()
        };
        let configuration: TernConfiguration = serde_json::from_value(serde_json::json!({
            "path":root.join("tern/plugin-data/tern-desktop-pet/kv.json"),"sha256":"b".repeat(64),"connection":original_tuple,
            "device":1,"inode":2,"length":100,"modified_seconds":1,"modified_nanoseconds":0,
            "bound_source":original.image,"source_binding_sha256":null,
        })).unwrap();
        let approval = RelocationTernApproval {
            configuration,
            original: original_tuple,
            target: target_tuple,
        };
        let sidecar = b"complete-signed-closure-a".to_vec();
        let closure = digest(&sidecar);
        let id = "1".repeat(32);
        let work = root.join("relocation-work");
        let plan = StandaloneInstallPlan {
            version: 2,
            purpose: StandaloneInstallPurpose::PrivateRelocation {
                target: target.clone(),
                parent: crate::standalone_relocation::SettledRelocationParent {
                    version: 1,
                    operation_id: "0".repeat(32),
                    plan_sha256: "0".repeat(64),
                    generation: 7,
                    candidate_image: original.image.clone(),
                    source_closure_sha256: closure.clone(),
                    profile: profile.clone(),
                    result_sha256: "0".repeat(64),
                },
                source_closure_sha256: closure.clone(),
                tern: Some(approval.clone()),
            },
            operation_id: id.clone(),
            original: original.clone(),
            candidate_bundle: root.join("stage/DesktopPet.app"),
            candidate_image: image(&root.join("stage/DesktopPet.app"), 'a'),
            candidate_version: "0.4.13".into(),
            helper_image: ExecutableIdentity {
                path: root.join("helper"),
                sha256: "f".repeat(64),
            },
            profile,
            source: standalone_install::InstallSource::Running {
                instance: standalone_install::StandaloneOriginalInstance {
                    pid: 123,
                    start_sec: 1,
                    start_usec: 0,
                    instance_id: context.instance_id.clone(),
                    image: context.running.clone(),
                },
                context,
            },
            original_stop_generation: 7,
            control_token: "c".repeat(64),
        };
        let intent = ReplacementIntent {
            version: 2,
            operation_id: id,
            disposition: installation::ReplacementDisposition::RetainCompatibleClients,
            layout: ReplacementLayout::Relocate {
                source: crate::physical_install_root(&source).unwrap().to_path_buf(),
                retained: work.join("original/DesktopPet.app"),
                source_sidecar: sidecar.clone(),
                source_sidecar_sha256: closure.clone(),
            },
            origin: target.clone(),
            original: Some(original),
            original_file: Some(FileSnapshot {
                device: 1,
                inode: 10,
                sha256: "a".repeat(64),
            }),
            candidate_file: FileSnapshot {
                device: 1,
                inode: 20,
                sha256: "a".repeat(64),
            },
            candidate_version: "0.4.13".into(),
            work: work.clone(),
            work_device: 1,
            work_inode: 3,
            target: crate::physical_install_root(&target).unwrap().to_path_buf(),
            prepared: work.join("DesktopPet.app"),
            original_sidecar: None,
            candidate_sidecar: sidecar,
            original_sidecar_sha256: None,
            candidate_sidecar_sha256: closure,
            locator: root.join("installation.json"),
            original_locator: None,
            next_locator: None,
            update_locator: true,
        };
        Base {
            plan,
            approval,
            plan_sha256: "d".repeat(64),
            receipt_sha256: "e".repeat(64),
            result_sha256: "0".repeat(64),
            intent,
        }
    }
    fn release(version: &str) -> crate::ReleaseCandidate {
        let asset = format!(
            "{}-v{version}-macos-arm64.tar.gz",
            crate::identity::ARCHIVE_PREFIX
        );
        let url = format!(
            "https://github.com/hanbong5938/desktop-pet/releases/download/v{version}/{asset}"
        );
        crate::ReleaseCandidate {
            version: version.into(),
            channel: crate::ReleaseChannel::Stable,
            tag: format!("v{version}"),
            release_id: 1,
            asset_name: asset,
            archive_url: url.clone(),
            archive_size: 1,
            sha256: "9".repeat(64),
            checksum_url: format!("{url}.sha256"),
        }
    }
    fn successor(
        base: &Base,
        previous: &ReplacementIntent,
        id: char,
        previous_version: &str,
        version: &str,
        next_hash: char,
    ) -> Link {
        let original = record(
            base.plan.installed_origin(),
            previous_version,
            previous.candidate_file.sha256.chars().next().unwrap(),
        );
        let mut context = original_context(&base.plan).unwrap().clone();
        context.executable = original.executable.clone();
        context.running = original.image.clone();
        context.running_origin = Some(Box::new(original.origin.clone()));
        context.version = previous_version.into();
        let operation_id = id.to_string().repeat(32);
        let plan = crate::UpdatePlan {
            version: 4,
            operation_id: operation_id.clone(),
            context,
            origin: original.origin.clone(),
            action: crate::UpdateAction::StandaloneReplace,
            running: original.image.clone(),
            candidate: None,
            target: Some(crate::ReleaseTarget {
                policy: crate::UpdatePolicy::StableOnly,
                release: release(version),
                origin: original.origin.clone(),
                resolved_commit: None,
            }),
        };
        let candidate = image(&previous.target, next_hash);
        let work = base.plan.profile.state_dir.join(format!("work-{id}"));
        let sidecar = format!("complete-signed-closure-{next_hash}").into_bytes();
        let intent = ReplacementIntent {
            version: 2,
            operation_id: operation_id.clone(),
            disposition: installation::ReplacementDisposition::RetainCompatibleClients,
            layout: ReplacementLayout::Exchange,
            origin: original.origin.clone(),
            original: Some(original),
            original_file: Some(previous.candidate_file.clone()),
            candidate_file: FileSnapshot {
                device: 1,
                inode: previous.candidate_file.inode + 10,
                sha256: candidate.sha256.clone(),
            },
            candidate_version: version.into(),
            work: work.clone(),
            work_device: 1,
            work_inode: 4,
            target: previous.target.clone(),
            prepared: work.join("DesktopPet.app"),
            original_sidecar: Some(previous.candidate_sidecar.clone()),
            candidate_sidecar: sidecar.clone(),
            original_sidecar_sha256: Some(previous.candidate_sidecar_sha256.clone()),
            candidate_sidecar_sha256: digest(&sidecar),
            locator: previous.locator.clone(),
            original_locator: None,
            next_locator: None,
            update_locator: true,
        };
        let result = protocol::OperationRecord {
            version: 4,
            operation_id,
            phase: protocol::OperationPhase::Completed,
            execution_fence: protocol::ExecutionFence::ManagerExited,
            detail: "settled".into(),
            installed: true,
            applied: true,
            candidate: Some(candidate),
        };
        let plan_sha256 = digest(&serde_json::to_vec(&plan).unwrap());
        let result_sha256 = digest(&serde_json::to_vec(&result).unwrap());
        Link {
            plan,
            result,
            plan_sha256,
            result_sha256,
            intent,
        }
    }
    fn challenge(
        base: &Base,
        original: &TernConnection,
        instance: &str,
    ) -> RelocationHandoverChallenge {
        RelocationHandoverChallenge {
            version: 1,
            handover: Handover {
                operation_id: base.plan.operation_id.clone(),
                plan_path: standalone_install::plan_path(
                    &base.plan.profile.state_dir,
                    &base.plan.operation_id,
                )
                .unwrap(),
                plan_sha256: base.plan_sha256.clone(),
                token: random_hex(32).unwrap(),
                consumer_revision: "7".repeat(40),
                target_instance_id: instance.into(),
            },
            original: original.clone(),
            target: base.approval.target.clone(),
            commit_receipt_sha256: base.receipt_sha256.clone(),
            owner_proof_sha256: "8".repeat(64),
            current_image: ExecutableIdentity {
                path: base.intent.target.join("Contents/MacOS/desktop-pet"),
                sha256: base.plan.candidate_image.sha256.clone(),
            },
            stop_generation: 8,
            installed_package: base.plan.profile.state_dir.join("consumer"),
            package_sha256: "6".repeat(64),
        }
    }

    #[test]
    fn cancelled_relocation_settlement_accepts_released_h_and_rejects_c_or_reserved_roots() {
        let root = tempfile::tempdir().unwrap();
        let base = base(root.path());
        let plan = &base.plan;
        let state = &plan.profile.state_dir;
        let global = root.path().join(crate::identity::GLOBAL_UPDATES_DIRECTORY);
        private(&global);
        let access = SettlementAccess {
            global: Some(global.clone()),
        };
        protocol::write_private_new(
            &standalone_install::plan_path(state, &plan.operation_id).unwrap(),
            &serde_json::to_vec(plan).unwrap(),
            0o600,
        )
        .unwrap();
        let outcome = standalone_install::StandaloneInstallRecord {
            version: 1,
            operation_id: plan.operation_id.clone(),
            phase: standalone_install::StandaloneInstallPhase::Failed,
            result: standalone_install::StandaloneInstallResult {
                version: 1,
                operation_id: plan.operation_id.clone(),
                state_dir: state.clone(),
                status: standalone_install::InstallStatus::Cancelled,
                installation: None,
                installed: false,
                applied: false,
                restart: standalone_install::InstallRestart::NotRequested,
                retained_bundle: None,
                error: None,
            },
        };
        standalone_install::write_record(state, &outcome).unwrap();
        let context = plan.running().unwrap().1;
        assert!(standalone_settlement_preflight(plan, &outcome, Some(context), &access).is_ok());
        let mut wrong = context.clone();
        wrong.running_origin = Some(Box::new(plan.installed_origin().clone()));
        assert!(standalone_settlement_preflight(plan, &outcome, Some(&wrong), &access).is_err());
        wrong = context.clone();
        wrong.config_dir = root.path().join("another-config");
        assert!(standalone_settlement_preflight(plan, &outcome, Some(&wrong), &access).is_err());
        for origin in [plan.origin(), plan.installed_origin()] {
            let reservation = protocol::test_scope::reserve_install_at(
                &global,
                origin,
                None,
                state,
                &"9".repeat(32),
            )
            .unwrap();
            assert!(
                standalone_settlement_preflight(plan, &outcome, Some(context), &access).is_err()
            );
            reservation.release().unwrap();
            assert!(
                standalone_settlement_preflight(plan, &outcome, Some(context), &access).is_ok()
            );
        }
        let installed = standalone_install::StandaloneInstallRecord {
            phase: standalone_install::StandaloneInstallPhase::Completed,
            result: standalone_install::StandaloneInstallResult {
                installed: true,
                status: standalone_install::InstallStatus::InstalledStopped,
                installation: Some(record(
                    plan.installed_origin(),
                    &plan.candidate_version,
                    'a',
                )),
                ..outcome.result.clone()
            },
            ..outcome
        };
        standalone_install::write_record(state, &installed).unwrap();
        assert!(standalone_settlement_preflight(plan, &installed, Some(context), &access).is_err());
        wrong = context.clone();
        wrong.running_origin = Some(Box::new(plan.installed_origin().clone()));
        assert!(standalone_settlement_preflight(plan, &installed, Some(&wrong), &access).is_ok());
    }

    // Only the already-validated signed-image boundary is supplied. The files,
    // retained inode identity, external sidecars, graph and reservation are real.
    fn rollback_history(root: &Path) -> (Base, Vec<Link>, OwnerAccess) {
        fn closure(
            access: &mut OwnerAccess,
            bundle: &Path,
            bytes: &[u8],
            sidecar: &[u8],
            version: &str,
        ) -> FileSnapshot {
            private(&bundle.join("Contents/MacOS"));
            let path = bundle.join("Contents/MacOS/desktop-pet");
            fs::write(&path, bytes).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(installation::sidecar_path(bundle), sidecar).unwrap();
            access
                .closures
                .insert(bundle.to_owned(), (sidecar.to_owned(), version.into()));
            FileSnapshot::capture(&path).unwrap()
        }
        fn original(link: &mut Link, previous: &ReplacementIntent) {
            let image = ExecutableIdentity {
                path: previous.target.join("Contents/MacOS/desktop-pet"),
                sha256: previous.candidate_file.sha256.clone(),
            };
            link.plan.running = image.clone();
            link.plan.context.running = image.clone();
            link.intent.original.as_mut().unwrap().image = image;
            link.intent.original_file = Some(previous.candidate_file.clone());
            link.intent.original_sidecar = Some(previous.candidate_sidecar.clone());
            link.intent.original_sidecar_sha256 = Some(previous.candidate_sidecar_sha256.clone());
        }
        let mut base = base(root);
        let target = fs::canonicalize(root)
            .unwrap()
            .join("private/DesktopPet.app");
        private(target.parent().unwrap());
        base.intent.target = target.clone();
        let StandaloneInstallPurpose::PrivateRelocation { target: origin, .. } =
            &mut base.plan.purpose
        else {
            unreachable!()
        };
        *origin = InstallOrigin::Standalone { bundle: target };
        let global = root.join(crate::identity::GLOBAL_UPDATES_DIRECTORY);
        private(&global);
        let mut access = OwnerAccess {
            global: Some(global),
            closures: BTreeMap::new(),
        };
        let mut first = successor(&base, &base.intent, '2', "0.4.13", "0.4.14", 'b');
        base.intent.candidate_file = closure(
            &mut access,
            &first.intent.prepared,
            b"signed original a",
            &base.intent.candidate_sidecar,
            "0.4.13",
        );
        original(&mut first, &base.intent);
        let mut retry = successor(&base, &first.intent, '3', "0.4.14", "0.4.15", 'c');
        first.intent.candidate_file = closure(
            &mut access,
            &retry.intent.prepared,
            b"signed successor b",
            &first.intent.candidate_sidecar,
            "0.4.14",
        );
        first.result.candidate.as_mut().unwrap().sha256 =
            first.intent.candidate_file.sha256.clone();
        original(&mut retry, &first.intent);
        retry.intent.candidate_file = closure(
            &mut access,
            &retry.intent.target,
            b"signed successor c",
            &retry.intent.candidate_sidecar,
            "0.4.15",
        );
        retry.result.candidate.as_mut().unwrap().sha256 =
            retry.intent.candidate_file.sha256.clone();
        let mut failed = successor(&base, &first.intent, '4', "0.4.14", "0.4.15", 'd');
        original(&mut failed, &first.intent);
        failed.intent.candidate_file = closure(
            &mut access,
            &failed.intent.prepared,
            b"signed failed candidate d",
            &failed.intent.candidate_sidecar,
            "0.4.15",
        );
        failed.result.phase = protocol::OperationPhase::Failed;
        failed.result.installed = false;
        failed.result.applied = false;
        failed.result.candidate.as_mut().unwrap().sha256 =
            failed.intent.candidate_file.sha256.clone();
        (base, vec![first, failed, retry], access)
    }

    #[test]
    fn rolled_back_attempt_then_retry_reconstructs_unique_two_update_owner_chain() {
        let root = tempfile::tempdir().unwrap();
        let (base, links, access) = rollback_history(root.path());
        let current = links[2].result.candidate.as_ref().unwrap();
        let selected = successor_chain_with_access(&base, current, &links, &access).unwrap();
        assert_eq!(
            selected
                .iter()
                .map(|link| link.plan.operation_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                links[0].plan.operation_id.as_str(),
                links[2].plan.operation_id.as_str()
            ]
        );
        let hash = publish_owners(&base, &selected).unwrap();
        assert_eq!(hash, publish_owners(&base, &selected).unwrap());
        let head: RelocationOwner = read_json(&owner_path(&base, current), MAX_PLAN).unwrap();
        assert_eq!(
            head.update_operation_id.as_ref(),
            Some(&links[2].plan.operation_id)
        );
        assert!(!owner_path(&base, links[1].result.candidate.as_ref().unwrap()).exists());
    }

    #[test]
    fn rolled_back_attempt_requires_exact_inode_sidecars_terminal_fence_and_release() {
        for tamper in 0..9 {
            let root = tempfile::tempdir().unwrap();
            let (base, mut links, access) = rollback_history(root.path());
            let current = links[2].result.candidate.clone().unwrap();
            let mut reservation = None;
            let mut lease = None;
            match tamper {
                0 => {
                    let path = links[1].intent.prepared.join("Contents/MacOS/desktop-pet");
                    fs::rename(&path, path.with_extension("retained")).unwrap();
                    fs::write(&path, b"signed failed candidate d").unwrap();
                }
                1 => fs::write(
                    installation::sidecar_path(&links[1].intent.prepared),
                    b"substituted sidecar",
                )
                .unwrap(),
                2 => links[1].intent.original_file.as_mut().unwrap().inode += 1,
                3 => links[1].result.phase = protocol::OperationPhase::Unknown,
                4 => links[1].result.execution_fence = protocol::ExecutionFence::ManagerIntent,
                5 => {
                    reservation = Some(
                        protocol::test_scope::reserve_install_at(
                            access.global.as_ref().unwrap(),
                            base.plan.installed_origin(),
                            Some(base.plan.installed_origin()),
                            &base.plan.profile.state_dir,
                            &links[1].plan.operation_id,
                        )
                        .unwrap(),
                    )
                }
                6 => links[1].intent.original_sidecar.as_mut().unwrap().push(0),
                7 => {
                    lease = Some(
                        access
                            .lease(&base.plan.profile.state_dir, &links[1].plan.operation_id)
                            .unwrap(),
                    )
                }
                8 => links[1].intent.operation_id = "5".repeat(32),
                _ => unreachable!(),
            }
            assert!(
                successor_chain_with_access(&base, &current, &links, &access).is_err(),
                "tamper {tamper}"
            );
            if let Some(reservation) = reservation {
                reservation.release().unwrap();
            }
            drop(lease);
        }
    }

    #[test]
    fn rolled_back_attempt_without_retry_requires_restored_c_inode_and_locator() {
        let root = tempfile::tempdir().unwrap();
        let (base, mut links, mut access) = rollback_history(root.path());
        let retry = links.pop().unwrap();
        let current = links[0].result.candidate.clone().unwrap();
        fs::remove_file(retry.intent.target.join("Contents/MacOS/desktop-pet")).unwrap();
        fs::rename(
            retry.intent.prepared.join("Contents/MacOS/desktop-pet"),
            retry.intent.target.join("Contents/MacOS/desktop-pet"),
        )
        .unwrap();
        fs::rename(
            installation::sidecar_path(&retry.intent.prepared),
            installation::sidecar_path(&retry.intent.target),
        )
        .unwrap();
        let validated = access.closures.remove(&retry.intent.prepared).unwrap();
        access
            .closures
            .insert(retry.intent.target.clone(), validated);
        let selected = successor_chain_with_access(&base, &current, &links, &access).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].plan.operation_id, links[0].plan.operation_id);
        fs::write(&links[1].intent.locator, b"foreign locator").unwrap();
        fs::set_permissions(&links[1].intent.locator, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(successor_chain_with_access(&base, &current, &links, &access).is_err());
    }

    #[test]
    fn two_settled_updates_reconstruct_all_missing_derived_owner_records_idempotently() {
        let root = tempfile::tempdir().unwrap();
        let base = base(root.path());
        let first = successor(&base, &base.intent, '2', "0.4.13", "0.4.14", 'b');
        let second = successor(&base, &first.intent, '3', "0.4.14", "0.4.15", 'c');
        let links = [first, second];
        let current = links[1].result.candidate.clone().unwrap();
        let selected = successor_chain(&base, &current, &links).unwrap();
        assert_eq!(selected.len(), 2);
        let base_image = image(&base.intent.target, 'a');
        assert!(!owner_path(&base, &base_image).exists());
        assert!(!owner_path(&base, &links[0].result.candidate.clone().unwrap()).exists());
        let head = publish_owners(&base, &selected).unwrap();
        let initial_bytes = owned_private_file(&owner_path(&base, &base_image), MAX_PLAN).unwrap();
        let middle_path = owner_path(&base, &links[0].result.candidate.clone().unwrap());
        let middle_bytes = owned_private_file(&middle_path, MAX_PLAN).unwrap();
        let middle: RelocationOwner = serde_json::from_slice(&middle_bytes).unwrap();
        let last: RelocationOwner = read_json(&owner_path(&base, &current), MAX_PLAN).unwrap();
        assert_eq!(middle.predecessor_sha256, Some(digest(&initial_bytes)));
        assert_eq!(last.predecessor_sha256, Some(digest(&middle_bytes)));
        assert_eq!(last.previous_image, middle.current_image);
        assert_eq!(last.current_image, current);
        assert_eq!(publish_owners(&base, &selected).unwrap(), head);
        assert!(route_tuple(&base, &base.approval.original).is_ok());
        assert!(route_tuple(&base, &base.approval.target).is_ok());
        // Substitution of derived proof is never repaired by overwriting it.
        let mut substituted = middle;
        substituted.update_result_sha256 = Some("0".repeat(64));
        protocol::write_private_bytes(
            &middle_path,
            &serde_json::to_vec(&substituted).unwrap(),
            0o600,
        )
        .unwrap();
        assert!(publish_owners(&base, &selected).is_err());
    }
    #[test]
    fn missing_substituted_incomplete_and_branched_historical_links_are_not_successors() {
        let root = tempfile::tempdir().unwrap();
        let base = base(root.path());
        for tamper in 0..8 {
            let first = successor(&base, &base.intent, '2', "0.4.13", "0.4.14", 'b');
            let second = successor(&base, &first.intent, '3', "0.4.14", "0.4.15", 'c');
            let current = second.result.candidate.clone().unwrap();
            let mut links = vec![first, second];
            match tamper {
                0 => {
                    links.remove(0);
                }
                1 => links[1].intent.original_file.as_mut().unwrap().inode += 1,
                2 => links[1].intent.original_sidecar.as_mut().unwrap().push(0),
                3 => links[0].result.phase = protocol::OperationPhase::Starting,
                4 => links[1].result.applied = false,
                5 => {
                    links.push(successor(&base, &base.intent, '4', "0.4.13", "0.4.16", 'd'));
                }
                6 => links[1].intent.candidate_sidecar.push(0),
                7 => {
                    links[0].intent.candidate_file.sha256 =
                        base.intent.candidate_file.sha256.clone();
                    links[0].result.candidate.as_mut().unwrap().sha256 =
                        base.intent.candidate_file.sha256.clone();
                }
                _ => unreachable!(),
            }
            assert!(
                successor_chain(&base, &current, &links).is_err(),
                "tamper {tamper}"
            );
        }
    }
    #[test]
    fn h_and_saved_c_challenges_and_two_window_receipts_have_independent_namespaces() {
        let root = tempfile::tempdir().unwrap();
        let base = base(root.path());
        let current = image(&base.intent.target, 'a');
        let h_path =
            challenge_path(&base, &current, "instance-1", &base.approval.original).unwrap();
        let c_path = challenge_path(&base, &current, "instance-1", &base.approval.target).unwrap();
        assert_ne!(h_path, c_path);
        let h = publish_challenge(
            &h_path,
            challenge(&base, &base.approval.original, "instance-1"),
        )
        .unwrap();
        let c = publish_challenge(
            &c_path,
            challenge(&base, &base.approval.target, "instance-1"),
        )
        .unwrap();
        assert_ne!(h.token, c.token);
        assert_eq!(
            publish_challenge(
                &h_path,
                challenge(&base, &base.approval.original, "instance-1")
            )
            .unwrap(),
            h
        );
        assert_ne!(
            receipt_path(&base, &current, "instance-1", "new-window-A"),
            receipt_path(&base, &current, "instance-1", "new-window-B")
        );
        assert_ne!(
            h_path,
            challenge_path(&base, &current, "instance-2", &base.approval.original).unwrap()
        );
        assert_ne!(
            receipt_path(&base, &current, "instance-1", "new-window-A"),
            receipt_path(&base, &current, "instance-2", "new-window-A")
        );
        let mut wrong_generation = challenge(&base, &base.approval.original, "instance-1");
        wrong_generation.stop_generation += 1;
        assert!(publish_challenge(&h_path, wrong_generation).is_err());
        let mut wrong_package = challenge(&base, &base.approval.original, "instance-1");
        wrong_package.package_sha256 = "0".repeat(64);
        assert!(publish_challenge(&h_path, wrong_package).is_err());
    }
    #[test]
    fn full_digest_evidence_names_and_atomic_temps_fit_name_max() {
        let root = tempfile::tempdir().unwrap();
        let base = base(root.path());
        let current = image(&base.intent.target, 'a');
        let challenge_path =
            challenge_path(&base, &current, "instance-1", &base.approval.original).unwrap();
        let receipt_path = receipt_path(&base, &current, "instance-1", "window-1").unwrap();
        assert_eq!(challenge_path.file_name().unwrap().len(), 208);
        assert_eq!(receipt_path.file_name().unwrap().len(), 209);
        for path in [&challenge_path, &receipt_path] {
            assert!(path.components().all(|part| part.as_os_str().len() <= 255));
            protocol::write_private_atomic_new(path, b"complete", 0o600).unwrap();
            assert_eq!(owned_private_file(path, MAX_PLAN).unwrap(), b"complete");
        }
        // Atomic temporary names must also fit when the final name is NAME_MAX.
        let boundary = challenge_path.parent().unwrap().join("x".repeat(255));
        protocol::write_private_atomic_new(&boundary, b"boundary", 0o600).unwrap();
        assert_eq!(
            owned_private_file(&boundary, MAX_PLAN).unwrap(),
            b"boundary"
        );
    }
    #[test]
    fn simultaneous_same_tuple_requests_reuse_one_atomic_winner_token() {
        let root = tempfile::tempdir().unwrap();
        let base = base(root.path());
        let current = image(&base.intent.target, 'a');
        let path = challenge_path(&base, &current, "instance-1", &base.approval.original).unwrap();
        let handles = (0..8)
            .map(|_| {
                let path = path.clone();
                let proposed = challenge(&base, &base.approval.original, "instance-1");
                std::thread::spawn(move || publish_challenge(&path, proposed).unwrap())
            })
            .collect::<Vec<_>>();
        let winners = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert!(winners.iter().all(|winner| winner == &winners[0]));
        assert_eq!(
            fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    #[test]
    fn valid_upgrade_history_is_skipped_and_recognized_forged_relocation_is_error() {
        let root = tempfile::tempdir().unwrap();
        let base = base(root.path());
        for version in [1, 2] {
            let mut plan = base.plan.clone();
            plan.version = version;
            plan.purpose = StandaloneInstallPurpose::Upgrade;
            plan.operation_id = version.to_string().repeat(32);
            plan.original = record(
                if version == 1 {
                    base.plan.origin()
                } else {
                    base.plan.installed_origin()
                },
                "0.4.12",
                '0',
            );
            plan.candidate_version = "0.4.13".into();
            plan.source = standalone_install::InstallSource::Stopped {
                profile: plan.profile.clone(),
            };
            protocol::write_private_atomic_new(
                &standalone_install::plan_path(&plan.profile.state_dir, &plan.operation_id)
                    .unwrap(),
                &serde_json::to_vec(&plan).unwrap(),
                0o600,
            )
            .unwrap();
        }
        let request = RouteRequest {
            version: 1,
            kind: "legacy-migration-route".into(),
            executable: base.approval.original.executable.clone(),
            config_dir: base.plan.profile.config_dir.clone(),
            state_dir: base.plan.profile.state_dir.clone(),
        };
        assert!(committed_route(&request).unwrap().is_none());
        let mut forged = base.plan.clone();
        forged.version = 1;
        forged.operation_id = "3".repeat(32);
        protocol::write_private_atomic_new(
            &standalone_install::plan_path(&forged.profile.state_dir, &forged.operation_id)
                .unwrap(),
            &serde_json::to_vec(&forged).unwrap(),
            0o600,
        )
        .unwrap();
        assert!(committed_route(&request).is_err());
        assert!(read_base(&request.state_dir, &"2".repeat(32))
            .unwrap()
            .is_none());
    }
    #[test]
    fn strict_namespace_never_deserializes_arbitrary_plan_path_as_relocation() {
        let state = Path::new("/private/state");
        let id = "a".repeat(32);
        assert!(is_install_plan_path(
            state,
            &standalone_install::plan_path(state, &id).unwrap(),
            &id
        )
        .unwrap());
        assert!(is_install_plan_path(
            state,
            Path::new("/other/install-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.plan.json"),
            &id
        )
        .is_err());
        assert!(plan_id("install-forged.plan.json", "install-").is_err());
        assert_eq!(plan_id(&format!("{id}.plan.json"), "").unwrap(), Some(id));
    }
    fn package(path: &Path, script: &[u8]) {
        private(path);
        for (name, bytes) in [
            ("LICENSE.txt", b"license".as_slice()),
            (
                "plugin.toml",
                b"id = \"tern-desktop-pet\"\nwindow = \"window.luau\"\nversion = \"0.2.1\"\n"
                    .as_slice(),
            ),
            (
                "revision",
                b"7276c1b658a1728883d90207119df31bce118170\n".as_slice(),
            ),
            ("window.luau", script),
        ] {
            fs::write(path.join(name), bytes).unwrap();
            fs::set_permissions(path.join(name), fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
    #[test]
    fn missing_mismatched_consumer_is_pending_and_does_not_remove_saved_route_ancestry() {
        let root = tempfile::tempdir().unwrap();
        let base = base(root.path());
        let physical_root = fs::canonicalize(root.path()).unwrap();
        let expected = physical_root.join("signed-owner-consumer");
        let installed = physical_root.join("installed-consumer");
        package(&expected, b"qualified-consumer");
        assert!(qualified_consumer(&expected, &installed).is_err());
        assert!(route_tuple(&base, &base.approval.original).is_ok());
        package(&installed, b"substituted-consumer");
        assert!(qualified_consumer(&expected, &installed).is_err());
        package(&installed, b"qualified-consumer");
        assert_eq!(
            qualified_consumer(&expected, &installed).unwrap(),
            package_identity(&expected).unwrap()
        );
        fs::write(
            installed.join("revision"),
            b"0000000000000000000000000000000000000000\n",
        )
        .unwrap();
        assert!(qualified_consumer(&expected, &installed).is_err());
        assert!(route_tuple(&base, &base.approval.original).is_ok());
    }
    fn window_receipt(base: &Base, window: &str) -> RelocationHandoverReceipt {
        RelocationHandoverReceipt {
            version: 1,
            operation_id: base.plan.operation_id.clone(),
            plan_sha256: base.plan_sha256.clone(),
            challenge_sha256: "c".repeat(64),
            current_image: ExecutableIdentity {
                path: base.intent.target.join("Contents/MacOS/desktop-pet"),
                sha256: base.plan.candidate_image.sha256.clone(),
            },
            config_dir: base.plan.profile.config_dir.clone(),
            state_dir: base.plan.profile.state_dir.clone(),
            target_instance_id: "current-instance".into(),
            window_id: window.into(),
            stop_generation: 8,
            binding_epoch: 4,
            sequence: 5,
            observation_generation: 6,
            native_generation: 7,
            consumer_revision: "7".repeat(40),
            package_sha256: "8".repeat(64),
            local_host: "host-a".into(),
        }
    }
    fn binding(receipt: &RelocationHandoverReceipt) -> TernBindingEvidence {
        TernBindingEvidence {
            version: 1,
            kind: "tern-migration-evidence".into(),
            instance_id: receipt.target_instance_id.clone(),
            window_id: receipt.window_id.clone(),
            binding_epoch: receipt.binding_epoch,
            sequence: receipt.sequence,
            local_host: receipt.local_host.clone(),
            observation_generation: receipt.observation_generation,
            native_generation: receipt.native_generation,
            fresh: true,
        }
    }
    #[test]
    fn two_distinct_post_save_windows_are_counted_only_with_current_fresh_exact_bindings() {
        let root = tempfile::tempdir().unwrap();
        let base = base(root.path());
        let mut windows = BTreeSet::new();
        for window in ["post-save-new-id-A", "post-save-new-id-B"] {
            let receipt = window_receipt(&base, window);
            let path = receipt_path(
                &base,
                &receipt.current_image,
                &receipt.target_instance_id,
                &receipt.window_id,
            )
            .unwrap();
            protocol::write_private_bytes(&path, &serde_json::to_vec(&receipt).unwrap(), 0o600)
                .unwrap();
            let stored: RelocationHandoverReceipt = read_json(&path, MAX_PLAN).unwrap();
            assert_eq!(stored.local_host, "host-a");
            let evidence = binding(&stored);
            if receipt_matches_binding(&stored, &evidence) {
                windows.insert(stored.window_id.clone());
            }
            for tamper in 0..8 {
                let mut wrong = binding(&stored);
                match tamper {
                    0 => wrong.fresh = false,
                    1 => wrong.window_id = "other-window".into(),
                    2 => wrong.instance_id = "old-instance".into(),
                    3 => wrong.local_host = "other-host".into(),
                    4 => wrong.binding_epoch -= 1,
                    5 => wrong.sequence -= 1,
                    6 => wrong.observation_generation -= 1,
                    7 => wrong.native_generation -= 1,
                    _ => unreachable!(),
                }
                assert!(!receipt_matches_binding(&stored, &wrong), "tamper {tamper}");
            }
        }
        assert_eq!(windows.len(), 2);
        let old = window_receipt(&base, "pre-reset-protocol-id");
        let mut current = binding(&old);
        current.window_id = "post-reset-protocol-id".into();
        assert!(!receipt_matches_binding(&old, &current));
        // A receipt's additional native-only host field does not alter legacy
        // receipt decoding or the unchanged six-field wire handover.
        let mut unexpected =
            serde_json::to_value(window_receipt(&base, "post-save-new-id-A")).unwrap();
        unexpected["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<RelocationHandoverReceipt>(unexpected).is_err());
        let handover = challenge(&base, &base.approval.original, "current-instance").handover;
        assert_eq!(
            serde_json::to_value(handover)
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            6
        );
    }
    fn installed_result(base: &Base) -> standalone_install::StandaloneInstallRecord {
        let manual = matches!(
            base.plan.purpose,
            StandaloneInstallPurpose::ManualRecovery { .. }
        );
        standalone_install::StandaloneInstallRecord {
            version: 1,
            operation_id: base.plan.operation_id.clone(),
            phase: standalone_install::StandaloneInstallPhase::Completed,
            result: standalone_install::StandaloneInstallResult {
                version: 1,
                operation_id: base.plan.operation_id.clone(),
                state_dir: base.plan.profile.state_dir.clone(),
                status: if manual {
                    standalone_install::InstallStatus::InstalledStopped
                } else {
                    standalone_install::InstallStatus::Applied
                },
                installation: Some(record(
                    base.plan.installed_origin(),
                    &base.plan.candidate_version,
                    base.plan.candidate_image.sha256.chars().next().unwrap(),
                )),
                installed: true,
                applied: !manual,
                restart: if manual {
                    standalone_install::InstallRestart::NotRequested
                } else {
                    standalone_install::InstallRestart::Ready
                },
                retained_bundle: Some(base.intent.work.join("original/DesktopPet.app")),
                error: None,
            },
        }
    }
    #[test]
    fn relocation_authority_rejects_plan_result_intent_and_commit_substitution() {
        let root = tempfile::tempdir().unwrap();
        let base = base(root.path());
        let expected_digest = digest(&serde_json::to_vec(&base.intent).unwrap());
        let receipt = Commit {
            version: 1,
            operation_id: base.plan.operation_id.clone(),
            intent_sha256: expected_digest.clone(),
        };
        validate_relocation_disk_evidence(
            &base.plan,
            &installed_result(&base),
            &receipt,
            &base.intent,
            &expected_digest,
        )
        .unwrap();
        for tamper in 0..9 {
            let mut plan = base.plan.clone();
            let mut result = installed_result(&base);
            let mut intent = base.intent.clone();
            let mut receipt = Commit {
                version: 1,
                operation_id: base.plan.operation_id.clone(),
                intent_sha256: expected_digest.clone(),
            };
            match tamper {
                0 => receipt.intent_sha256 = "0".repeat(64),
                1 => receipt.operation_id = "f".repeat(32),
                2 => result.result.applied = false,
                3 => result.phase = standalone_install::StandaloneInstallPhase::Starting,
                4 => {
                    result.result.installation.as_mut().unwrap().origin = base.plan.origin().clone()
                }
                5 => intent.candidate_sidecar.push(0),
                6 => intent.layout = ReplacementLayout::Exchange,
                7 => intent.original_file.as_mut().unwrap().inode = intent.candidate_file.inode,
                8 => {
                    if let StandaloneInstallPurpose::PrivateRelocation {
                        source_closure_sha256,
                        ..
                    } = &mut plan.purpose
                    {
                        *source_closure_sha256 = "0".repeat(64);
                    }
                }
                _ => unreachable!(),
            }
            assert!(
                validate_relocation_disk_evidence(
                    &plan,
                    &result,
                    &receipt,
                    &intent,
                    &expected_digest
                )
                .is_err(),
                "tamper {tamper}"
            );
        }
    }
    #[test]
    fn relocation_requires_exact_source_bound_ack_and_preserves_wire_decoder_errors() {
        let root = tempfile::tempdir().unwrap();
        let base = base(root.path());
        let ack = standalone_install::BootstrapStopAck {
            version: 1,
            operation_id: base.plan.operation_id.clone(),
            origin: base.plan.origin().clone(),
            profile: base.plan.profile.clone(),
            original: base.plan.running().unwrap().0.clone(),
            original_stop_generation: base.plan.original_stop_generation,
            control_token: base.plan.control_token.clone(),
        };
        ack.validate(&base.plan).unwrap();
        for tamper in 0..3 {
            let mut wrong = ack.clone();
            match tamper {
                0 => wrong.origin = base.plan.installed_origin().clone(),
                1 => wrong.original_stop_generation += 1,
                2 => wrong.original.instance_id = "another-source-instance".into(),
                _ => unreachable!(),
            }
            assert!(wrong.validate(&base.plan).is_err());
        }
        let response = serde_json::json!({"version":1,"kind":"tern-migration-evidence","error":"actual backend error"});
        assert_eq!(
            decode_tern_binding_evidence(&serde_json::to_vec(&response).unwrap())
                .err()
                .unwrap(),
            "actual backend error"
        );
    }
    fn manual_base(root: &Path) -> Base {
        let mut base = base(root);
        let mut previous_context = original_context(&base.plan).unwrap().clone();
        base.plan.original.app_version = "0.4.13-beta.7".into();
        previous_context.version = base.plan.original.app_version.clone();
        previous_context.environment = crate::native_runtime::captured_environment(
            previous_context.observation_mode().unwrap(),
            |key| previous_context.environment.get(key).cloned(),
        )
        .unwrap();
        previous_context.assets_override =
            Some(base.plan.profile.config_dir.join("preserved-assets"));
        base.plan.candidate_version = "0.4.14".into();
        base.plan.candidate_bundle = standalone_install::manual_operator_request_path(
            &base.plan.profile.state_dir,
            &base.plan.operation_id,
        )
        .unwrap()
        .parent()
        .unwrap()
        .join("DesktopPet.app");
        base.plan.candidate_image = image(&base.plan.candidate_bundle, 'b');
        base.plan.purpose = StandaloneInstallPurpose::ManualRecovery {
            target: base.plan.installed_origin().clone(),
            release: release("0.4.14"),
            previous_context,
            source_closure_sha256: match &base.intent.layout {
                ReplacementLayout::Relocate {
                    source_sidecar_sha256,
                    ..
                } => source_sidecar_sha256.clone(),
                _ => unreachable!(),
            },
            tern: Some(base.approval.clone()),
        };
        base.plan.source = standalone_install::InstallSource::Stopped {
            profile: base.plan.profile.clone(),
        };
        base.intent.original = Some(base.plan.original.clone());
        base.intent.candidate_version = base.plan.candidate_version.clone();
        base.intent.candidate_file.sha256 = base.plan.candidate_image.sha256.clone();
        base.intent.candidate_sidecar = b"independently-signed-official-closure-b".to_vec();
        base.intent.candidate_sidecar_sha256 = digest(&base.intent.candidate_sidecar);
        base.plan.validate().unwrap();
        base
    }

    fn publish_manual_request(base: &Base) -> PathBuf {
        let plan_path =
            standalone_install::plan_path(&base.plan.profile.state_dir, &base.plan.operation_id)
                .unwrap();
        protocol::write_private_atomic_new(
            &plan_path,
            &serde_json::to_vec_pretty(&base.plan).unwrap(),
            0o600,
        )
        .unwrap();
        let StandaloneInstallPurpose::ManualRecovery {
            release,
            previous_context,
            ..
        } = &base.plan.purpose
        else {
            unreachable!()
        };
        let request = standalone_install::StandaloneManualRecoveryRequest {
            version: 1,
            context: previous_context.clone(),
            candidate_bundle: base.plan.candidate_bundle.clone(),
            release: release.clone(),
            confirmation: "replace_original_app_preserve_profile".into(),
        };
        let request_path = standalone_install::manual_operator_request_path(
            &base.plan.profile.state_dir,
            &base.plan.operation_id,
        )
        .unwrap();
        private(request_path.parent().unwrap());
        protocol::write_private_atomic_new(
            &request_path,
            &serde_json::to_vec_pretty(&request).unwrap(),
            0o600,
        )
        .unwrap();
        request_path
    }

    #[test]
    fn stopped_manual_cutover_binds_distinct_source_and_candidate_without_restart_claim() {
        let root = tempfile::tempdir().unwrap();
        let base = manual_base(root.path());
        let result = installed_result(&base);
        assert!(base.plan.running().is_none());
        assert!(!result.result.applied);
        let intent_sha256 = digest(&serde_json::to_vec(&base.intent).unwrap());
        let receipt = Commit {
            version: 1,
            operation_id: base.plan.operation_id.clone(),
            intent_sha256: intent_sha256.clone(),
        };
        validate_relocation_disk_evidence(
            &base.plan,
            &result,
            &receipt,
            &base.intent,
            &intent_sha256,
        )
        .unwrap();
        for tamper in 0..10 {
            let mut result = result.clone();
            let mut intent = base.intent.clone();
            let mut receipt = Commit {
                version: 1,
                operation_id: base.plan.operation_id.clone(),
                intent_sha256: intent_sha256.clone(),
            };
            match tamper {
                0 => result.result.applied = true,
                1 => result.result.restart = standalone_install::InstallRestart::Ready,
                2 => result.result.status = standalone_install::InstallStatus::Applied,
                3 => receipt.intent_sha256 = "0".repeat(64),
                4 => intent.original_file.as_mut().unwrap().inode = intent.candidate_file.inode,
                5 => intent.candidate_sidecar.push(0),
                6 => {
                    let ReplacementLayout::Relocate { source_sidecar, .. } = &mut intent.layout
                    else {
                        unreachable!()
                    };
                    *source_sidecar = intent.candidate_sidecar.clone();
                }
                7 => result.result.state_dir = root.path().join("foreign-profile"),
                8 => intent.origin = base.plan.origin().clone(),
                9 => intent.original.as_mut().unwrap().image.sha256 = "f".repeat(64),
                _ => unreachable!(),
            }
            assert!(
                validate_relocation_disk_evidence(
                    &base.plan,
                    &result,
                    &receipt,
                    &intent,
                    &intent_sha256,
                )
                .is_err(),
                "manual tamper {tamper}"
            );
        }
    }

    #[test]
    fn manual_authority_requires_actual_operator_bytes_and_rejects_native_ack_or_consent() {
        let root = tempfile::tempdir().unwrap();
        let base = manual_base(root.path());
        let request_path = publish_manual_request(&base);
        let plan_sha256 = digest(
            &owned_private_file(
                &standalone_install::plan_path(
                    &base.plan.profile.state_dir,
                    &base.plan.operation_id,
                )
                .unwrap(),
                MAX_PLAN,
            )
            .unwrap(),
        );
        assert!(validate_base_consent(&base.plan, &plan_sha256).is_err());
        let consent_path =
            standalone_install::write_operator_consent(&base.plan, &request_path).unwrap();
        let saved = owned_private_file(&consent_path, MAX_PLAN).unwrap();
        validate_base_consent(&base.plan, &plan_sha256).unwrap();
        for tamper in 0..4 {
            let mut consent: standalone_install::StandaloneOperatorConsent =
                serde_json::from_slice(&saved).unwrap();
            match tamper {
                0 => consent.method = "native_panel".into(),
                1 => consent.plan_sha256 = "0".repeat(64),
                2 => consent.request_sha256 = "0".repeat(64),
                3 => consent.operation_id = "f".repeat(32),
                _ => unreachable!(),
            }
            protocol::write_private_bytes(
                &consent_path,
                &serde_json::to_vec(&consent).unwrap(),
                0o600,
            )
            .unwrap();
            assert!(validate_base_consent(&base.plan, &plan_sha256).is_err());
        }
        protocol::write_private_bytes(&consent_path, &saved, 0o600).unwrap();
        for path in [
            standalone_install::stop_ack_path(
                &base.plan.profile.state_dir,
                &base.plan.operation_id,
            )
            .unwrap(),
            standalone_install::consent_path(&base.plan.profile.state_dir, &base.plan.operation_id)
                .unwrap(),
        ] {
            protocol::write_private_atomic_new(&path, b"{}", 0o600).unwrap();
            assert!(validate_base_consent(&base.plan, &plan_sha256).is_err());
            fs::remove_file(path).unwrap();
        }
        let request_bytes = owned_private_file(&request_path, MAX_PLAN).unwrap();
        let request: serde_json::Value = serde_json::from_slice(&request_bytes).unwrap();
        protocol::write_private_bytes(&request_path, &serde_json::to_vec(&request).unwrap(), 0o600)
            .unwrap();
        assert!(
            validate_base_consent(&base.plan, &plan_sha256).is_err(),
            "semantically equal but different request bytes cannot inherit consent"
        );
    }

    #[test]
    fn manual_atomic_commit_is_mandatory_and_bound_to_actual_intent_bytes() {
        let root = tempfile::tempdir().unwrap();
        let base = manual_base(root.path());
        let path = standalone_install::replacement_path(
            &base.plan.profile.state_dir,
            &base.plan.operation_id,
        )
        .unwrap();
        assert!(!installation::relocation_committed(&path, &base.intent).unwrap());
        let bytes = serde_json::to_vec_pretty(&base.intent).unwrap();
        protocol::write_private_atomic_new(&path, &bytes, 0o600).unwrap();
        let receipt_path = standalone_install::relocation_commit_path(
            &base.plan.profile.state_dir,
            &base.plan.operation_id,
        )
        .unwrap();
        let receipt = Commit {
            version: 1,
            operation_id: base.plan.operation_id.clone(),
            intent_sha256: digest(&bytes),
        };
        protocol::write_private_atomic_new(
            &receipt_path,
            &serde_json::to_vec(&receipt).unwrap(),
            0o600,
        )
        .unwrap();
        assert!(installation::relocation_committed(&path, &base.intent).unwrap());
        protocol::write_private_bytes(&path, &serde_json::to_vec(&base.intent).unwrap(), 0o600)
            .unwrap();
        assert!(installation::relocation_committed(&path, &base.intent).is_err());
    }

    #[test]
    fn manual_separate_start_observation_rejects_old_instance_generation_image_and_profile() {
        let root = tempfile::tempdir().unwrap();
        let base = manual_base(root.path());
        let observed = ManualInstalledStoppedObservation {
            version: 1,
            operation_id: base.plan.operation_id.clone(),
            plan_sha256: base.plan_sha256.clone(),
            result_sha256: base.result_sha256.clone(),
            image: image(&base.intent.target, 'b'),
            instance_id: "explicit-normal-C-start".into(),
            pid: Some(456),
            stop_generation: base.plan.original_stop_generation,
        };
        validate_manual_observation(&base, &observed).unwrap();
        for tamper in 0..7 {
            let mut wrong = observed.clone();
            match tamper {
                0 => wrong.stop_generation += 1,
                1 => wrong.instance_id = original_context(&base.plan).unwrap().instance_id.clone(),
                2 => wrong.image = base.plan.original.image.clone(),
                3 => wrong.pid = Some(0),
                4 => wrong.plan_sha256 = "0".repeat(64),
                5 => wrong.operation_id = "f".repeat(32),
                6 => wrong.result_sha256 = "f".repeat(64),
                _ => unreachable!(),
            }
            assert!(
                validate_manual_observation(&base, &wrong).is_err(),
                "start tamper {tamper}"
            );
        }
        assert!(
            !installed_result(&base).result.applied,
            "observing current readiness must not rewrite old InstalledStopped"
        );
    }

    #[test]
    fn manual_scopes_require_exact_helper_and_both_root_profile_release() {
        let root = tempfile::tempdir().unwrap();
        let base = manual_base(root.path());
        let global = root.path().join("global");
        private(&global);
        let access = OwnerAccess {
            global: Some(global),
            ..OwnerAccess::default()
        };
        let state = &base.plan.profile.state_dir;
        let id = &base.plan.operation_id;
        let lease = access.lease(state, id).unwrap();
        assert!(access.lease(state, id).is_err());
        drop(lease);
        let reservation = protocol::test_scope::reserve_install_at(
            access.global.as_ref().unwrap(),
            base.plan.origin(),
            base.plan.reservation_target(),
            state,
            id,
        )
        .unwrap();
        assert!(access
            .released(
                base.plan.origin(),
                base.plan.reservation_target(),
                state,
                id
            )
            .is_err());
        reservation.release().unwrap();
        access
            .released(
                base.plan.origin(),
                base.plan.reservation_target(),
                state,
                id,
            )
            .unwrap();
        let other_state = root.path().join("other-profile-state");
        private(&other_state);
        let foreign = protocol::test_scope::reserve_install_at(
            access.global.as_ref().unwrap(),
            base.plan.installed_origin(),
            None,
            &other_state,
            &"f".repeat(32),
        )
        .unwrap();
        assert!(access
            .released(
                base.plan.origin(),
                base.plan.reservation_target(),
                state,
                id
            )
            .is_err());
        foreign.release().unwrap();
    }

    #[test]
    fn manual_h_to_c_sdk_challenges_two_fresh_windows_and_restart_are_independent() {
        let root = tempfile::tempdir().unwrap();
        let base = manual_base(root.path());
        route_tuple(&base, &base.approval.original).unwrap();
        route_tuple(&base, &base.approval.target).unwrap();
        let mut foreign = base.approval.original.clone();
        foreign.state_dir = root.path().join("other-state");
        assert!(route_tuple(&base, &foreign).is_err());
        let current = image(&base.intent.target, 'b');
        let mut handovers = Vec::new();
        for tuple in [&base.approval.original, &base.approval.target] {
            let path = challenge_path(&base, &current, "manual-C-instance", tuple).unwrap();
            handovers.push(
                publish_challenge(&path, challenge(&base, tuple, "manual-C-instance")).unwrap(),
            );
            assert_ne!(
                path,
                challenge_path(&base, &current, "later-C-instance", tuple).unwrap()
            );
        }
        assert_ne!(handovers[0].token, handovers[1].token);
        assert_eq!(
            serde_json::to_value(&handovers[0])
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            6
        );
        let mut windows = BTreeSet::new();
        for window in ["post-save-A", "post-save-B"] {
            let receipt = window_receipt(&base, window);
            let fresh = binding(&receipt);
            assert!(receipt_matches_binding(&receipt, &fresh));
            windows.insert(receipt.window_id.clone());
            let mut stale = fresh;
            stale.instance_id = "previous-C-instance".into();
            assert!(!receipt_matches_binding(&receipt, &stale));
        }
        assert_eq!(windows.len(), 2);
    }

    #[test]
    fn manual_owner_chain_starts_at_candidate_not_beta_source_and_accepts_next_updates() {
        let root = tempfile::tempdir().unwrap();
        let base = manual_base(root.path());
        let first = successor(&base, &base.intent, '2', "0.4.14", "0.4.15", 'c');
        let second = successor(&base, &first.intent, '3', "0.4.15", "0.4.16", 'd');
        let links = [first, second];
        let current = links[1].result.candidate.clone().unwrap();
        let selected = successor_chain(&base, &current, &links).unwrap();
        publish_owners(&base, &selected).unwrap();
        let initial: RelocationOwner = read_json(
            &owner_path(&base, &image(&base.intent.target, 'b')),
            MAX_PLAN,
        )
        .unwrap();
        assert_eq!(initial.previous_image, initial.current_image);
        assert_eq!(
            initial.previous_image.sha256,
            base.plan.candidate_image.sha256
        );
        assert_eq!(
            initial.previous_closure_sha256,
            base.intent.candidate_sidecar_sha256
        );
        assert_eq!(
            initial.previous_closure_sha256,
            initial.current_closure_sha256
        );
        assert_ne!(
            initial.previous_image.sha256,
            base.plan.original.image.sha256
        );
        assert!(initial.update_operation_id.is_none());
        let head: RelocationOwner = read_json(&owner_path(&base, &current), MAX_PLAN).unwrap();
        assert_eq!(
            head.update_operation_id.as_ref(),
            Some(&links[1].plan.operation_id)
        );
    }
    fn manual_start_context(base: &Base, instance: &str) -> UpdateContext {
        let mut context = original_context(&base.plan).unwrap().clone();
        context.executable = base.intent.target.join("Contents/MacOS/desktop-pet");
        context.running = image(&base.intent.target, 'b');
        context.running_origin = Some(Box::new(base.plan.installed_origin().clone()));
        context.version = base.plan.candidate_version.clone();
        context.instance_id = instance.into();
        context
    }
    fn runtime_start_fixture(base: &Base, context: &UpdateContext, generation: u64) -> PathBuf {
        let path = base.plan.profile.state_dir.join("updates-v4").join(format!(
            "{}.runtime-start.json",
            digest(context.instance_id.as_bytes()),
        ));
        let bytes = serde_json::to_vec(&serde_json::json!({
            "version":2,
            "root_key":protocol::installation_key(base.plan.installed_origin()).unwrap(),
            "pid":456,
            "instance_id":context.instance_id,
            "image":context.running,
            "stop_generation":generation,
            "context":context,
        }))
        .unwrap();
        protocol::write_private_atomic_new(&path, &bytes, 0o600).unwrap();
        path
    }
    fn historical_start_fixture(base: &Base, instance: &str, generation: u64) -> PathBuf {
        runtime_start_fixture(base, &manual_start_context(base, instance), generation)
    }

    #[test]
    fn missing_manual_observation_reconstructs_old_start_after_restart_not_current_generation() {
        let root = tempfile::tempdir().unwrap();
        let base = manual_base(root.path());
        let mut later = manual_start_context(&base, "later-restart");
        later
            .set_observation_selection(&desktop_pet_contracts::ObservationSelection::Provider(
                "tern".into(),
            ))
            .unwrap();
        later.assets_override = Some(base.plan.profile.config_dir.join("later-assets"));
        later
            .environment
            .insert("XDG_CONFIG_HOME".into(), "/later/environment".into());
        runtime_start_fixture(&base, &later, base.plan.original_stop_generation + 1);
        assert!(
            reconstruct_manual_start(&base).is_err(),
            "a later fresh runtime cannot invent the original unchanged fence"
        );
        historical_start_fixture(
            &base,
            "initial-explicit-start",
            base.plan.original_stop_generation,
        );
        let observed = reconstruct_manual_start(&base).unwrap();
        validate_manual_observation(&base, &observed).unwrap();
        assert_eq!(observed.instance_id, "initial-explicit-start");
        assert_eq!(observed.stop_generation, base.plan.original_stop_generation);
        assert!(!manual_observation_path(&base).unwrap().exists());
        observe_manual_start(&base).unwrap();
        let persisted: ManualInstalledStoppedObservation =
            read_json(&manual_observation_path(&base).unwrap(), MAX_PLAN).unwrap();
        assert_eq!(persisted, observed);
        assert!(!installed_result(&base).result.applied);
    }

    #[test]
    fn cold_manual_reconstruction_accepts_first_normal_updater_from_changed_later_context() {
        let root = tempfile::tempdir().unwrap();
        let base = manual_base(root.path());
        historical_start_fixture(
            &base,
            "initial-explicit-start",
            base.plan.original_stop_generation,
        );
        let mut later = manual_start_context(&base, "later-restart-before-first-updater");
        later
            .set_observation_selection(&desktop_pet_contracts::ObservationSelection::Provider(
                "tern".into(),
            ))
            .unwrap();
        later
            .environment
            .insert("HOME".into(), "/changed/runtime/home".into());
        later.assets_override = Some(base.plan.profile.config_dir.join("later-assets"));
        runtime_start_fixture(&base, &later, base.plan.original_stop_generation + 1);
        let (later_generation, retained_later) = protocol::historical_runtime_start_context(
            base.plan.installed_origin(),
            &base.plan.profile.state_dir,
            Some(456),
            &later.instance_id,
            &later.running,
        )
        .unwrap();
        assert_eq!(later_generation, base.plan.original_stop_generation + 1);
        assert_eq!(retained_later, later);
        let mut first = successor(&base, &base.intent, '2', "0.4.14", "0.4.15", 'c');
        first.plan.context = retained_later;
        first.plan_sha256 = digest(&serde_json::to_vec(&first.plan).unwrap());
        let links = [first];
        let selected =
            successor_chain(&base, links[0].result.candidate.as_ref().unwrap(), &links).unwrap();
        assert_ne!(
            selected[0].plan.context.environment,
            original_context(&base.plan).unwrap().environment
        );
        assert!(!manual_observation_path(&base).unwrap().exists());
        observe_manual_start(&base).unwrap();
        publish_owners(&base, &selected).unwrap();
        let observed: ManualInstalledStoppedObservation =
            read_json(&manual_observation_path(&base).unwrap(), MAX_PLAN).unwrap();
        assert_eq!(observed.instance_id, "initial-explicit-start");
        assert_eq!(observed.stop_generation, base.plan.original_stop_generation);
        let head: RelocationOwner = read_json(
            &owner_path(&base, links[0].result.candidate.as_ref().unwrap()),
            MAX_PLAN,
        )
        .unwrap();
        assert_eq!(
            head.update_operation_id.as_ref(),
            Some(&links[0].plan.operation_id)
        );
        assert!(!installed_result(&base).result.applied);
    }

    #[test]
    fn manual_historical_start_rejects_foreign_scope_and_malformed_runtime_authority() {
        for tamper in 0..20 {
            let root = tempfile::tempdir().unwrap();
            let base = manual_base(root.path());
            let path = historical_start_fixture(
                &base,
                "initial-explicit-start",
                base.plan.original_stop_generation,
            );
            let mut value: serde_json::Value = read_json(&path, MAX_PLAN).unwrap();
            match tamper {
                0 => value["root_key"] = serde_json::json!("foreign-root"),
                1 => value["version"] = serde_json::json!(3),
                2 => value["pid"] = serde_json::json!(0),
                3 => value["unexpected"] = serde_json::json!(true),
                4 => value["instance_id"] = serde_json::json!("substituted-instance"),
                5 => {
                    value.as_object_mut().unwrap().remove("context");
                }
                6 => {
                    value["version"] = serde_json::json!(1);
                    value.as_object_mut().unwrap().remove("context");
                }
                7 => {
                    value["context"]["config_dir"] = serde_json::json!(base.plan.profile.state_dir)
                }
                8 => {
                    value["context"]["state_dir"] = serde_json::json!(base.plan.profile.config_dir)
                }
                9 => value["context"]["running"]["sha256"] = serde_json::json!("f".repeat(64)),
                10 => value["context"]["instance_id"] = serde_json::json!("foreign-instance"),
                11 => {
                    value["context"]["environment"]["DESKTOP_PET_UPDATER_TOKEN"] =
                        serde_json::json!("must-not-be-stored")
                }
                12 => value["context"]["assets_override"] = serde_json::json!("/changed/assets"),
                13 => {
                    value["context"]["environment"]
                        [desktop_pet_contracts::OBSERVATION_SELECTION_ENV] =
                        serde_json::json!("tern")
                }
                14 => value["context"]["environment"]["HOME"] = serde_json::json!("/changed/home"),
                15 => value["context"]["version"] = serde_json::json!("0.4.15"),
                16 => {
                    value["stop_generation"] =
                        serde_json::json!(base.plan.original_stop_generation + 1)
                }
                17 => value["image"]["sha256"] = serde_json::json!("f".repeat(64)),
                18 => {
                    value["context"]["running_origin"] =
                        serde_json::to_value(base.plan.origin()).unwrap()
                }
                19 => value["context"]["executable"] = serde_json::json!("/foreign/desktop-pet"),
                _ => unreachable!(),
            }
            protocol::write_private_bytes(&path, &serde_json::to_vec(&value).unwrap(), 0o600)
                .unwrap();
            assert!(
                reconstruct_manual_start(&base).is_err(),
                "runtime tamper {tamper}"
            );
            assert!(observe_manual_start(&base).is_err());
            assert!(!manual_observation_path(&base).unwrap().exists());
        }
    }

    #[test]
    fn persisted_manual_observation_cannot_replace_missing_or_substituted_initial_context() {
        let root = tempfile::tempdir().unwrap();
        let base = manual_base(root.path());
        let path = historical_start_fixture(
            &base,
            "initial-explicit-start",
            base.plan.original_stop_generation,
        );
        observe_manual_start(&base).unwrap();
        let observed_path = manual_observation_path(&base).unwrap();
        let observed_bytes = owned_private_file(&observed_path, MAX_PLAN).unwrap();
        let initial: serde_json::Value = read_json(&path, MAX_PLAN).unwrap();
        for missing in [false, true] {
            let mut wrong = initial.clone();
            if missing {
                wrong.as_object_mut().unwrap().remove("context");
                wrong["version"] = serde_json::json!(1);
            } else {
                wrong["context"]["assets_override"] = serde_json::json!("/substituted/assets");
            }
            protocol::write_private_bytes(&path, &serde_json::to_vec(&wrong).unwrap(), 0o600)
                .unwrap();
            assert!(observe_manual_start(&base).is_err());
            assert_eq!(
                owned_private_file(&observed_path, MAX_PLAN).unwrap(),
                observed_bytes
            );
        }
        assert!(!installed_result(&base).result.applied);
    }
    #[test]
    fn recognized_manual_namespace_rejects_forged_version_running_source_and_profile() {
        for tamper in 0..3 {
            let root = tempfile::tempdir().unwrap();
            let base = manual_base(root.path());
            let mut plan = base.plan.clone();
            match tamper {
                0 => plan.version = 1,
                1 => plan.source = self::base(root.path()).plan.source,
                2 => plan.profile.config_dir = root.path().join("foreign-config"),
                _ => unreachable!(),
            }
            protocol::write_private_atomic_new(
                &standalone_install::plan_path(&plan.profile.state_dir, &plan.operation_id)
                    .unwrap(),
                &serde_json::to_vec(&plan).unwrap(),
                0o600,
            )
            .unwrap();
            let request = RouteRequest {
                version: 1,
                kind: "legacy-migration-route".into(),
                executable: base.approval.original.executable.clone(),
                config_dir: base.plan.profile.config_dir.clone(),
                state_dir: base.plan.profile.state_dir.clone(),
            };
            assert!(
                committed_route(&request).is_err(),
                "namespace tamper {tamper}"
            );
        }
    }
}
