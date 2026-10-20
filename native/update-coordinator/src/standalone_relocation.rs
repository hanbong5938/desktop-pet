//! Immutable provenance for a separately approved H-to-private relocation.
//! These records carry no Stop, restart, or updater-token authority.
use crate::migration::{TernConfiguration, TernConnection};
use crate::native_runtime::ProfilePaths;
use crate::{ensure_absolute, protocol, ExecutableIdentity};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelocationParentCapture {
    pub version: u32,
    pub operation_id: String,
    pub plan_sha256: String,
    pub generation: u64,
    pub candidate_image: ExecutableIdentity,
    pub source_closure_sha256: String,
    pub profile: ProfilePaths,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettledRelocationParent {
    pub version: u32,
    pub operation_id: String,
    pub plan_sha256: String,
    pub generation: u64,
    pub candidate_image: ExecutableIdentity,
    pub source_closure_sha256: String,
    pub profile: ProfilePaths,
    pub result_sha256: String,
}

/// Saved-route approval, not evidence that a consumer is installed or connected.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelocationTernApproval {
    pub configuration: TernConfiguration,
    pub original: TernConnection,
    pub target: TernConnection,
}

pub(crate) fn validate_sha256(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("invalid relocation evidence SHA-256".into());
    }
    Ok(())
}

fn validate_parent(
    version: u32,
    operation_id: &str,
    plan_sha256: &str,
    candidate_image: &ExecutableIdentity,
    source_closure_sha256: &str,
    profile: &ProfilePaths,
) -> Result<(), String> {
    if version != 1 {
        return Err("unsupported relocation parent evidence".into());
    }
    protocol::validate_operation_id(operation_id)?;
    validate_sha256(plan_sha256)?;
    validate_sha256(&candidate_image.sha256)?;
    validate_sha256(source_closure_sha256)?;
    ensure_absolute(&candidate_image.path)?;
    ensure_absolute(&profile.config_dir)?;
    ensure_absolute(&profile.state_dir)
}

impl RelocationParentCapture {
    pub fn validate(&self) -> Result<(), String> {
        validate_parent(
            self.version,
            &self.operation_id,
            &self.plan_sha256,
            &self.candidate_image,
            &self.source_closure_sha256,
            &self.profile,
        )
    }
}

impl SettledRelocationParent {
    /// Structural validation only. Settlement admission must authenticate the
    /// immutable parent journal and its released reservation separately.
    pub fn validate(&self) -> Result<(), String> {
        validate_parent(
            self.version,
            &self.operation_id,
            &self.plan_sha256,
            &self.candidate_image,
            &self.source_closure_sha256,
            &self.profile,
        )?;
        validate_sha256(&self.result_sha256)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RelocationAdmission {
    Waiting,
    Eligible(SettledRelocationParent),
    Superseded,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelocationRequest {
    pub version: u32,
    pub context: crate::UpdateContext,
    pub parent: SettledRelocationParent,
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

fn home_origin() -> Result<crate::InstallOrigin, String> {
    Ok(crate::InstallOrigin::Standalone {
        bundle: crate::installation::os_home()?.join("Applications/DesktopPet.app"),
    })
}

// Production access is zero-sized. Fixtures substitute the OS account and
// already-validated signed-image boundary, not journal or protocol behavior.
#[derive(Default)]
struct ParentAccess {
    #[cfg(test)]
    fixture: Option<ParentFixtureAccess>,
}

#[cfg(test)]
struct ParentFixtureAccess {
    global: std::path::PathBuf,
    home: crate::InstallOrigin,
    app: crate::installation::ValidatedApp,
}

impl ParentAccess {
    fn home(&self) -> Result<crate::InstallOrigin, String> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return Ok(fixture.home.clone());
        }
        home_origin()
    }
    fn app(&self, bundle: &std::path::Path) -> Result<crate::installation::ValidatedApp, String> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            if fixture.app.bundle != bundle {
                return Err("fixture source bundle mismatch".into());
            }
            return Ok(fixture.app.clone());
        }
        crate::installation::validate_staged_app(bundle)
    }
    fn generation(&self, state: &std::path::Path) -> Result<u64, String> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return protocol::test_scope::stop_generation_at(&fixture.global, state);
        }
        protocol::stop_generation(state)
    }
    fn start_allowed(
        &self,
        origin: &crate::InstallOrigin,
        state: &std::path::Path,
        generation: Option<u64>,
        token: Option<&str>,
    ) -> Result<(), String> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return protocol::test_scope::start_allowed_at(
                &fixture.global,
                origin,
                state,
                generation,
                token,
            );
        }
        protocol::start_allowed(origin, state, generation, token)
    }
    fn snapshot(
        &self,
        plan: &crate::UpdatePlan,
    ) -> Result<Option<protocol::ReservationSnapshot>, String> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return protocol::test_scope::reservation_snapshot_at(
                &fixture.global,
                &plan.origin,
                Some(plan.target_origin()),
                &plan.context.state_dir,
                &plan.operation_id,
            );
        }
        protocol::reservation_snapshot(
            &plan.origin,
            Some(plan.target_origin()),
            &plan.context.state_dir,
            &plan.operation_id,
        )
    }
    fn reserved(&self, plan: &crate::UpdatePlan) -> Result<bool, String> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return protocol::test_scope::reservation_exists_at(
                &fixture.global,
                &plan.origin,
                Some(plan.target_origin()),
                &plan.context.state_dir,
                &plan.operation_id,
            );
        }
        protocol::reservation_exists(
            &plan.origin,
            Some(plan.target_origin()),
            &plan.context.state_dir,
            &plan.operation_id,
        )
    }
    fn lease(
        &self,
        state: &std::path::Path,
        id: &str,
    ) -> Result<Option<protocol::OperationLease>, String> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return protocol::test_scope::lock_operation_at(&fixture.global, state, id);
        }
        protocol::try_lock_operation(state, id)
    }
}

fn journal_bytes(state: &std::path::Path, id: &str, suffix: &str) -> Result<Vec<u8>, String> {
    protocol::validate_operation_id(id)?;
    crate::owned_private_file(
        &protocol::private_updates(state)?.join(format!("{id}.{suffix}.json")),
        65536,
    )
}

fn eligible_versions(previous: &str, target: &str) -> bool {
    let (Ok(previous_version), Ok(target_version)) = (
        semver::Version::parse(previous),
        semver::Version::parse(target),
    ) else {
        return false;
    };
    previous_version.to_string() == previous
        && target_version.to_string() == target
        && previous_version.build.is_empty()
        && target_version.build.is_empty()
        && target_version.pre.is_empty()
        && (previous_version.pre.is_empty()
            || previous_version
                .pre
                .as_str()
                .strip_prefix("beta.")
                .is_some_and(|number| {
                    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
                }))
        && previous_version < target_version
}

/// Read-only admission. In particular this never applies the new canonical-only
/// mutation policy to the immutable parent that installed the app at H.
fn parent_plan(
    context: &crate::UpdateContext,
    id: &str,
) -> Result<(crate::UpdatePlan, RelocationParentCapture), String> {
    parent_plan_with(context, id, &ParentAccess::default())
}

fn parent_plan_with(
    context: &crate::UpdateContext,
    id: &str,
    access: &ParentAccess,
) -> Result<(crate::UpdatePlan, RelocationParentCapture), String> {
    if crate::identity::LEGACY {
        return Err("private relocation requires the neutral product".into());
    }
    let plan = crate::read_plan(&context.state_dir, id)?;
    let home = access.home()?;
    let target = plan
        .target
        .as_ref()
        .ok_or("relocation parent has no explicit release target")?;
    target.release.validate_neutral()?;
    if plan.action != crate::UpdateAction::StandaloneReplace
        || plan.candidate.is_some()
        || target.resolved_commit.is_some()
        || plan.origin != home
        || target.origin != home
        || plan
            .context
            .running_origin
            .as_deref()
            .is_some_and(|origin| origin != &home)
        || context.running_origin.as_deref() != Some(&home)
        || plan.context.config_dir != context.config_dir
        || plan.context.state_dir != context.state_dir
        || plan.context.assets_override != context.assets_override
        || plan.context.executable != plan.running.path
        || plan.running.path != context.running.path
        || context.executable != context.running.path
        || !eligible_versions(&plan.context.version, &target.release.version)
        || target.release.version != crate::identity::VERSION
        || context.version != target.release.version
        || !target.policy.allows_version(&target.release.version)?
    {
        return Err("update is not an authenticated same-home stable relocation parent".into());
    }
    let bundle = crate::physical_install_root(&home)?;
    let app = access.app(bundle)?;
    if app.image != context.running || app.version != context.version {
        return Err("relocation parent candidate is not the current signed home image".into());
    }
    let result = protocol::read_operation_observation(&context.state_dir, id)?
        .ok_or("relocation parent has no candidate journal")?;
    if result.version != protocol::UPDATER_PROTOCOL
        || result.candidate.as_ref() != Some(&app.image)
        || result.execution_fence != protocol::ExecutionFence::ManagerExited
    {
        return Err("relocation source is not the installed parent candidate".into());
    }
    let sidecar = crate::read_owned_file(
        &crate::installation::sidecar_path(bundle),
        4 * 1024 * 1024,
        false,
    )?;
    let plan_bytes = journal_bytes(&context.state_dir, id, "plan")?;
    if serde_json::from_slice::<crate::UpdatePlan>(&plan_bytes).map_err(|e| e.to_string())? != plan
    {
        return Err("relocation parent plan changed during capture".into());
    }
    let capture = RelocationParentCapture {
        version: 1,
        operation_id: id.to_owned(),
        plan_sha256: digest(&plan_bytes),
        generation: access.generation(&context.state_dir)?,
        candidate_image: app.image,
        source_closure_sha256: digest(&sidecar),
        profile: ProfilePaths {
            config_dir: context.config_dir.clone(),
            state_dir: context.state_dir.clone(),
        },
    };
    Ok((plan, capture))
}

fn parent_ids(context: &crate::UpdateContext) -> Result<Vec<String>, String> {
    let mut ids = Vec::new();
    for entry in std::fs::read_dir(protocol::private_updates(&context.state_dir)?)
        .map_err(|e| e.to_string())?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if let Some(id) = name.to_str().and_then(|n| n.strip_suffix(".plan.json")) {
            if protocol::validate_operation_id(id).is_ok() {
                ids.push(id.to_owned());
            }
        }
        if ids.len() > 1024 {
            return Err("relocation parent history exceeds bound".into());
        }
    }
    Ok(ids)
}

pub fn capture_relocation_parent(
    context: &crate::UpdateContext,
    generation: u64,
    token: &str,
) -> Result<Option<RelocationParentCapture>, String> {
    capture_relocation_parent_with(context, generation, token, &ParentAccess::default())
}

fn capture_relocation_parent_with(
    context: &crate::UpdateContext,
    generation: u64,
    token: &str,
    access: &ParentAccess,
) -> Result<Option<RelocationParentCapture>, String> {
    if crate::identity::LEGACY || context.running_origin.as_deref() != Some(&access.home()?) {
        return Ok(None);
    }
    // start_allowed authenticates the startup token (not the control token)
    // and the current generation against the durable reservation.
    access.start_allowed(
        &access.home()?,
        &context.state_dir,
        Some(generation),
        Some(token),
    )?;
    let mut found = None;
    for id in parent_ids(context)? {
        let Ok((plan, capture)) = parent_plan_with(context, &id, access) else {
            continue;
        };
        let Ok(Some(snapshot)) = access.snapshot(&plan) else {
            continue;
        };
        if !snapshot.markers_complete
            || snapshot.startup_origin.as_ref() != Some(&access.home()?)
            || capture.generation != generation
        {
            return Err("relocation parent startup reservation is incomplete".into());
        }
        if found.is_some() {
            return Err("ambiguous relocation startup parent".into());
        }
        found = Some(capture);
    }
    if let Some(capture) = found.as_ref() {
        let path = protocol::private_updates(&context.state_dir)?
            .join(format!("relocation-parent-{}.json", capture.operation_id));
        let bytes = serde_json::to_vec(capture).map_err(|e| e.to_string())?;
        if path.try_exists().map_err(|e| e.to_string())? {
            if crate::owned_private_file(&path, 65536)? != bytes {
                return Err("relocation capture conflicts with immutable provenance".into());
            }
        } else {
            protocol::write_private_atomic_new(&path, &bytes, 0o600)?;
        }
    }
    Ok(found)
}

pub fn observe_relocation_parent(
    context: &crate::UpdateContext,
    capture: &RelocationParentCapture,
) -> Result<RelocationAdmission, String> {
    observe_relocation_parent_with(context, capture, &ParentAccess::default())
}

fn observe_relocation_parent_with(
    context: &crate::UpdateContext,
    capture: &RelocationParentCapture,
    access: &ParentAccess,
) -> Result<RelocationAdmission, String> {
    capture.validate()?;
    if access.generation(&context.state_dir)? != capture.generation {
        return Ok(RelocationAdmission::Superseded);
    }
    let Some(_lease) = access.lease(&context.state_dir, &capture.operation_id)? else {
        return Ok(RelocationAdmission::Waiting);
    };
    let (plan, current) = parent_plan_with(context, &capture.operation_id, access)?;
    if &current != capture {
        return Err("relocation parent capture no longer matches signed source".into());
    }
    let Some(result) =
        protocol::read_operation_observation(&context.state_dir, &capture.operation_id)?
    else {
        return Ok(RelocationAdmission::Waiting);
    };
    if result.phase != protocol::OperationPhase::Completed {
        return Ok(
            if matches!(
                result.phase,
                protocol::OperationPhase::Failed
                    | protocol::OperationPhase::Unknown
                    | protocol::OperationPhase::UserStopped
            ) {
                RelocationAdmission::Superseded
            } else {
                RelocationAdmission::Waiting
            },
        );
    }
    if !result.installed
        || !result.applied
        || result.execution_fence != protocol::ExecutionFence::ManagerExited
        || result.candidate.as_ref() != Some(&capture.candidate_image)
    {
        return Err("relocation parent did not apply the exact home candidate".into());
    }
    if access.reserved(&plan)? {
        return Ok(RelocationAdmission::Waiting);
    }
    if access
        .start_allowed(&access.home()?, &context.state_dir, None, None)
        .is_err()
    {
        return Ok(RelocationAdmission::Waiting);
    }
    let result_bytes = journal_bytes(&context.state_dir, &capture.operation_id, "result")?;
    if serde_json::from_slice::<protocol::OperationRecord>(&result_bytes)
        .map_err(|e| e.to_string())?
        != result
    {
        return Err("relocation parent result changed during settlement".into());
    }
    if access.generation(&context.state_dir)? != capture.generation {
        return Ok(RelocationAdmission::Superseded);
    }
    Ok(RelocationAdmission::Eligible(SettledRelocationParent {
        version: capture.version,
        operation_id: capture.operation_id.clone(),
        plan_sha256: capture.plan_sha256.clone(),
        generation: capture.generation,
        candidate_image: capture.candidate_image.clone(),
        source_closure_sha256: capture.source_closure_sha256.clone(),
        profile: capture.profile.clone(),
        result_sha256: digest(&result_bytes),
    }))
}

pub fn validate_settled_parent(
    context: &crate::UpdateContext,
    parent: &SettledRelocationParent,
) -> Result<(), String> {
    parent.validate()?;
    let capture = RelocationParentCapture {
        version: parent.version,
        operation_id: parent.operation_id.clone(),
        plan_sha256: parent.plan_sha256.clone(),
        generation: parent.generation,
        candidate_image: parent.candidate_image.clone(),
        source_closure_sha256: parent.source_closure_sha256.clone(),
        profile: parent.profile.clone(),
    };
    match observe_relocation_parent(context, &capture)? {
        RelocationAdmission::Eligible(current) if &current == parent => Ok(()),
        _ => Err("relocation parent is not settled and released".into()),
    }
}

pub fn discover_relocation_parent(
    context: &crate::UpdateContext,
) -> Result<Option<SettledRelocationParent>, String> {
    let mut found = None;
    for id in parent_ids(context)? {
        let Ok((_, capture)) = parent_plan(context, &id) else {
            continue;
        };
        if let RelocationAdmission::Eligible(parent) = observe_relocation_parent(context, &capture)?
        {
            if found.is_some() {
                return Err(
                    "multiple successful relocation parents; refusing to guess latest".into(),
                );
            }
            found = Some(parent);
        }
    }
    Ok(found)
}

pub fn resume_relocation_parent(
    context: &crate::UpdateContext,
) -> Result<RelocationAdmission, String> {
    let mut found = None;
    for entry in std::fs::read_dir(protocol::private_updates(&context.state_dir)?)
        .map_err(|e| e.to_string())?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|n| n.strip_prefix("relocation-parent-"))
            .and_then(|n| n.strip_suffix(".json"))
        else {
            continue;
        };
        protocol::validate_operation_id(id)?;
        let capture: RelocationParentCapture =
            serde_json::from_slice(&crate::owned_private_file(&entry.path(), 65536)?)
                .map_err(|e| e.to_string())?;
        if capture.operation_id != id {
            return Err("relocation capture pathname mismatch".into());
        }
        if found.is_some() {
            return Err("ambiguous persisted relocation capture".into());
        }
        if let Some(offer) =
            crate::standalone_install::read_relocation_offer(&context.state_dir, id)?
        {
            // This is only a request to reenter the already claimed operation,
            // never a new admission. Its helper owns cleanup/recovery under lease.
            let parent = offer.parent;
            let (_, current) = parent_plan(context, id)?;
            if current != capture
                || parent.plan_sha256 != capture.plan_sha256
                || parent.generation != capture.generation
                || parent.profile != capture.profile
                || parent.candidate_image != capture.candidate_image
                || parent.source_closure_sha256 != capture.source_closure_sha256
                || parent.result_sha256 != digest(&journal_bytes(&context.state_dir, id, "result")?)
            {
                return Err("claimed relocation offer conflicts with captured parent".into());
            }
            found = Some(RelocationAdmission::Eligible(parent));
        } else {
            found = Some(observe_relocation_parent(context, &capture)?);
        }
    }
    Ok(found.unwrap_or(RelocationAdmission::Superseded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beta7_and_previous_stable_are_valid_parent_versions() {
        assert!(eligible_versions("0.4.13-beta.7", "0.4.13"));
        assert!(eligible_versions("0.4.12", "0.4.13"));
    }

    #[test]
    fn equal_downgrade_build_and_other_prerelease_are_not_parent_versions() {
        for (previous, target) in [
            ("0.4.13", "0.4.13"),
            ("0.4.14", "0.4.13"),
            ("0.4.12+local", "0.4.13"),
            ("0.4.13-rc.1", "0.4.13"),
            ("0.4.12", "0.4.13-beta.1"),
            ("v0.4.12", "0.4.13"),
            ("0.4.13-beta.local", "0.4.13"),
            ("0.4.13-beta.7.extra", "0.4.13"),
        ] {
            assert!(
                !eligible_versions(previous, target),
                "{previous} -> {target}"
            );
        }
    }

    #[cfg(not(desktop_pet_legacy))]
    mod filesystem {
        use super::*;
        use crate::{InstallOrigin, UpdateAction, UpdateContext, UpdatePlan};
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};

        const ID: &str = "0123456789abcdef0123456789abcdef";

        struct Fixture {
            _directory: tempfile::TempDir,
            access: ParentAccess,
            context: UpdateContext,
            plan: UpdatePlan,
            reservation: Option<protocol::InstallationReservation>,
            startup_token: String,
            control_token: String,
        }

        fn directory(path: &Path) {
            std::fs::create_dir_all(path).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }

        impl Fixture {
            fn new(previous: &str) -> Self {
                let root = tempfile::tempdir().unwrap();
                let global = root.path().join(crate::identity::GLOBAL_UPDATES_DIRECTORY);
                let config = root.path().join("original-config");
                let state = root.path().join("original-state");
                let bundle = root.path().join("account/Applications/DesktopPet.app");
                for path in [&global, &config, &state, &bundle.join("Contents/MacOS")] {
                    directory(path);
                }
                let executable = bundle.join("Contents/MacOS/desktop-pet");
                let installed_bytes = b"fixture current signed image trust boundary";
                std::fs::write(&executable, installed_bytes).unwrap();
                std::fs::write(
                    crate::installation::sidecar_path(&bundle),
                    b"fixture complete signed deployment sidecar",
                )
                .unwrap();
                let home = InstallOrigin::Standalone {
                    bundle: bundle.clone(),
                };
                let image = crate::ExecutableIdentity {
                    path: executable.clone(),
                    sha256: digest(installed_bytes),
                };
                let context = UpdateContext {
                    executable: executable.clone(),
                    version: crate::identity::VERSION.into(),
                    instance_id: "fresh-stable-instance".into(),
                    config_dir: config,
                    state_dir: state,
                    running: image.clone(),
                    running_origin: Some(Box::new(home.clone())),
                    assets_override: None,
                    environment: Default::default(),
                    locale: "ko".into(),
                };
                let mut source_context = context.clone();
                source_context.version = previous.into();
                source_context.instance_id = "original-instance".into();
                source_context.running.sha256 = "a".repeat(64);
                let asset = format!(
                    "{}-v{}-macos-arm64.tar.gz",
                    crate::identity::neutral::ARCHIVE_PREFIX,
                    crate::identity::VERSION
                );
                let url = format!(
                    "{}v{}/{asset}",
                    crate::identity::neutral::RELEASE_DOWNLOAD,
                    crate::identity::VERSION
                );
                let plan = UpdatePlan {
                    version: protocol::UPDATER_PROTOCOL,
                    operation_id: ID.into(),
                    context: source_context.clone(),
                    origin: home.clone(),
                    action: UpdateAction::StandaloneReplace,
                    running: source_context.running,
                    candidate: None,
                    target: Some(crate::ReleaseTarget {
                        policy: crate::UpdatePolicy::StableOnly,
                        release: crate::ReleaseCandidate {
                            version: crate::identity::VERSION.into(),
                            channel: crate::ReleaseChannel::Stable,
                            tag: format!("v{}", crate::identity::VERSION),
                            release_id: 7,
                            asset_name: asset,
                            archive_url: url.clone(),
                            archive_size: 123,
                            sha256: "b".repeat(64),
                            checksum_url: format!("{url}.sha256"),
                        },
                        origin: home.clone(),
                        resolved_commit: None,
                    }),
                };
                let access = ParentAccess {
                    fixture: Some(ParentFixtureAccess {
                        global: global.clone(),
                        home: home.clone(),
                        app: crate::installation::ValidatedApp {
                            bundle,
                            image,
                            version: crate::identity::VERSION.into(),
                            installation_client_contract: Some(1),
                        },
                    }),
                };
                let mut fixture = Self {
                    _directory: root,
                    access,
                    context,
                    plan,
                    reservation: None,
                    startup_token: String::new(),
                    control_token: String::new(),
                };
                fixture.write_plan();
                fixture.write_result(protocol::OperationPhase::Starting);
                let reservation = protocol::test_scope::reserve_install_at(
                    &global,
                    &home,
                    Some(&home),
                    &fixture.context.state_dir,
                    ID,
                )
                .unwrap();
                fixture.control_token = reservation.control_token().to_owned();
                fixture.startup_token = reservation.authorize_start_for(&home).unwrap();
                fixture.reservation = Some(reservation);
                // Prove the fixture is admissible before negative tests mutate
                // it: capture deliberately skips invalid historical parents.
                parent_plan_with(&fixture.context, ID, &fixture.access)
                    .expect("fixture must contain valid historical parent authority");
                fixture
            }

            fn write_plan(&self) {
                protocol::write_private_bytes(
                    &protocol::private_updates(&self.context.state_dir)
                        .unwrap()
                        .join(format!("{ID}.plan.json")),
                    &serde_json::to_vec(&self.plan).unwrap(),
                    0o600,
                )
                .unwrap();
            }

            fn write_result(&self, phase: protocol::OperationPhase) {
                let record = protocol::OperationRecord {
                    version: protocol::UPDATER_PROTOCOL,
                    operation_id: ID.into(),
                    phase,
                    execution_fence: protocol::ExecutionFence::ManagerExited,
                    detail: "fixture parent journal".into(),
                    installed: true,
                    applied: phase == protocol::OperationPhase::Completed,
                    candidate: Some(self.context.running.clone()),
                };
                protocol::write_operation(&self.context.state_dir, &record).unwrap();
            }

            fn capture(&self) -> RelocationParentCapture {
                capture_relocation_parent_with(&self.context, 0, &self.startup_token, &self.access)
                    .expect("fixture startup authority must authenticate")
                    .expect("valid fixture parent must be captured")
            }

            fn observe(
                &self,
                capture: &RelocationParentCapture,
            ) -> Result<RelocationAdmission, String> {
                observe_relocation_parent_with(&self.context, capture, &self.access)
            }

            fn release(&mut self) {
                self.reservation.take().unwrap().release().unwrap();
            }
            fn global(&self) -> &Path {
                &self.access.fixture.as_ref().unwrap().global
            }
            fn updates(&self) -> PathBuf {
                protocol::private_updates(&self.context.state_dir).unwrap()
            }
        }

        #[test]
        fn beta7_and_previous_stable_wait_for_exact_parent_release() {
            for previous in ["0.4.13-beta.7", "0.4.12"] {
                let mut fixture = Fixture::new(previous);
                let capture = fixture.capture();
                assert_eq!(
                    fixture.observe(&capture).unwrap(),
                    RelocationAdmission::Waiting
                );
                fixture.write_result(protocol::OperationPhase::Completed);
                let held = fixture
                    .access
                    .lease(&fixture.context.state_dir, ID)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    fixture.observe(&capture).unwrap(),
                    RelocationAdmission::Waiting
                );
                drop(held);
                assert_eq!(
                    fixture.observe(&capture).unwrap(),
                    RelocationAdmission::Waiting
                );
                fixture.release();
                let RelocationAdmission::Eligible(parent) = fixture.observe(&capture).unwrap()
                else {
                    panic!("released exact parent must be eligible")
                };
                assert_eq!(parent.candidate_image, fixture.context.running);
                assert_eq!(
                    parent.plan_sha256,
                    digest(&journal_bytes(&fixture.context.state_dir, ID, "plan").unwrap())
                );
                assert_eq!(
                    parent.result_sha256,
                    digest(&journal_bytes(&fixture.context.state_dir, ID, "result").unwrap())
                );
                let saved = crate::owned_private_file(
                    &fixture
                        .updates()
                        .join(format!("relocation-parent-{ID}.json")),
                    65536,
                )
                .unwrap();
                let saved_text = String::from_utf8(saved).unwrap();
                assert!(!saved_text.contains(&fixture.startup_token));
                assert!(!saved_text.contains(&fixture.control_token));
            }
        }

        #[test]
        fn capture_authenticates_startup_not_control_token_and_exact_context() {
            let fixture = Fixture::new("0.4.13-beta.7");
            for token in [&fixture.control_token, &"f".repeat(64)] {
                assert!(capture_relocation_parent_with(
                    &fixture.context,
                    0,
                    token,
                    &fixture.access
                )
                .is_err());
            }
            assert!(capture_relocation_parent_with(
                &fixture.context,
                1,
                &fixture.startup_token,
                &fixture.access
            )
            .is_err());
            let mut wrong_profile = fixture.context.clone();
            wrong_profile.config_dir = fixture.context.config_dir.join("other");
            assert!(parent_plan_with(&wrong_profile, ID, &fixture.access).is_err());
            let mut wrong_state = fixture.context.clone();
            wrong_state.state_dir = fixture.context.state_dir.join("other");
            directory(&wrong_state.state_dir);
            assert!(capture_relocation_parent_with(
                &wrong_state,
                0,
                &fixture.startup_token,
                &fixture.access
            )
            .is_err());
            let mut wrong_candidate = fixture.context.clone();
            wrong_candidate.running.sha256 = "e".repeat(64);
            assert!(parent_plan_with(&wrong_candidate, ID, &fixture.access).is_err());
            assert!(!fixture
                .updates()
                .join(format!("relocation-parent-{ID}.json"))
                .exists());
        }

        #[test]
        fn wrong_action_target_metadata_and_versions_cannot_capture() {
            let mut fixture = Fixture::new("0.4.13-beta.7");
            let approved = fixture.plan.clone();
            fixture.plan.action = UpdateAction::ApplyInstalled;
            fixture.write_plan();
            assert!(parent_plan_with(&fixture.context, ID, &fixture.access).is_err());
            fixture.plan = approved.clone();
            fixture.plan.target.as_mut().unwrap().origin = InstallOrigin::Standalone {
                bundle: fixture._directory.path().join("different/DesktopPet.app"),
            };
            fixture.write_plan();
            assert!(parent_plan_with(&fixture.context, ID, &fixture.access).is_err());
            fixture.plan = approved.clone();
            fixture.plan.target.as_mut().unwrap().release.archive_url =
                "https://example.invalid/forged.tar.gz".into();
            fixture.write_plan();
            assert!(parent_plan_with(&fixture.context, ID, &fixture.access).is_err());
            for previous in [
                crate::identity::VERSION,
                "999.0.0",
                "0.4.13-rc.1",
                "0.4.12+local",
            ] {
                fixture.plan = approved.clone();
                fixture.plan.context.version = previous.into();
                fixture.write_plan();
                assert!(
                    parent_plan_with(&fixture.context, ID, &fixture.access).is_err(),
                    "{previous}"
                );
            }
            fixture.plan = approved;
            fixture.plan.target = None;
            fixture.write_plan();
            assert!(parent_plan_with(&fixture.context, ID, &fixture.access).is_err());
        }

        #[test]
        fn index_and_each_partial_marker_scope_remain_waiting_until_release() {
            let mut fixture = Fixture::new("0.4.13-beta.7");
            let capture = fixture.capture();
            fixture.write_result(protocol::OperationPhase::Completed);
            let profile = fixture.updates().join("profile.reservation.json");
            let profile_bytes = crate::owned_private_file(&profile, 16384).unwrap();
            let roots: Vec<_> = std::fs::read_dir(fixture.global())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| {
                    path.file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .ends_with(".reservation.json")
                })
                .collect();
            assert!(!roots.is_empty());
            std::fs::remove_file(&profile).unwrap();
            assert_eq!(
                fixture.observe(&capture).unwrap(),
                RelocationAdmission::Waiting
            );
            for root in &roots {
                std::fs::remove_file(root).unwrap();
            }
            assert_eq!(
                fixture.observe(&capture).unwrap(),
                RelocationAdmission::Waiting
            );
            protocol::write_private_new(&profile, &profile_bytes, 0o600).unwrap();
            assert_eq!(
                fixture.observe(&capture).unwrap(),
                RelocationAdmission::Waiting
            );
            // Release tolerates missing exact markers, but not substituted bytes.
            fixture.release();
            assert!(matches!(
                fixture.observe(&capture).unwrap(),
                RelocationAdmission::Eligible(_)
            ));
        }

        #[test]
        fn later_stop_supersedes_and_changed_candidate_or_closure_cannot_settle() {
            let mut fixture = Fixture::new("0.4.13-beta.7");
            let capture = fixture.capture();
            fixture.write_result(protocol::OperationPhase::Completed);
            fixture.release();
            let mut forged = capture.clone();
            forged.candidate_image.sha256 = "f".repeat(64);
            assert!(fixture.observe(&forged).is_err());
            std::fs::write(
                crate::installation::sidecar_path(
                    crate::physical_install_root(&fixture.plan.origin).unwrap(),
                ),
                b"foreign sidecar",
            )
            .unwrap();
            assert!(fixture.observe(&capture).is_err());
            protocol::write_private_bytes(
                &fixture.updates().join("stop-generation.json"),
                b"1",
                0o600,
            )
            .unwrap();
            assert_eq!(
                fixture.observe(&capture).unwrap(),
                RelocationAdmission::Superseded
            );
        }
    }
}
