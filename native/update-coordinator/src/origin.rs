//! Neutral installation ownership. Integration provenance is never install authority.
use crate::{
    ensure_absolute, executable_identity, identity, installation::standalone_bundle, now,
    owned_file, releases, validate_candidate_for_context, CheckSnapshot, CheckStatus,
    ExecutableIdentity, InstallOrigin, ReleaseCandidate, ReleaseTarget, UpdateAction,
    UpdateContext, UpdatePlan, UpdatePolicy, VerifiedInstallation,
};
use semver::Version;
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_METADATA: u64 = 1024 * 1024;
const OFFICIAL_TAP: &str = "hanbong5938/tap/";
const STABLE_FORMULA: &str = identity::STABLE_FORMULA;
const BETA_FORMULA: &str = identity::BETA_FORMULA;

fn version(value: &str) -> Result<Version, String> {
    Version::parse(value.trim_start_matches('v')).map_err(|e| e.to_string())
}
fn normalized(path: &Path) -> Result<PathBuf, String> {
    ensure_absolute(path)?;
    fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))
}
fn app_binary(bundle: &Path) -> PathBuf {
    bundle.join("Contents/MacOS").join(identity::BINARY)
}
fn source_binary(root: &Path) -> PathBuf {
    app_binary(&root.join("dist").join(identity::APP_NAME))
}

fn context_valid(c: &UpdateContext) -> Result<(), String> {
    for path in [&c.executable, &c.running.path, &c.config_dir, &c.state_dir] {
        ensure_absolute(path)?;
    }
    if let Some(path) = &c.assets_override {
        ensure_absolute(path)?;
    }
    if c.instance_id.is_empty()
        || c.instance_id.len() > 128
        || c.instance_id
            .chars()
            .any(|ch| matches!(ch, '\n' | '\r' | '/'))
    {
        return Err("invalid instance id".into());
    }
    if c.running.sha256.len() != 64 || !c.running.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid immutable running identity".into());
    }
    version(&c.version)?;
    c.observation_mode()?;
    for (key, value) in &c.environment {
        if !matches!(
            key.as_str(),
            "DESKTOP_PET_CONFIG_DIR"
                | "DESKTOP_PET_STATE_DIR"
                | "XDG_CONFIG_HOME"
                | "XDG_STATE_HOME"
                | "TERN_CONFIG_DIR"
                | "TERN_DAEMON_SOCKET"
                | "HOME"
                | identity::OBSERVATION_ENV
        ) || value.len() > 4096
            || value.contains('\0')
        {
            return Err(format!("unapproved profile environment variable {key}"));
        }
        if matches!(key.as_str(), "TERN_CONFIG_DIR" | "TERN_DAEMON_SOCKET") {
            ensure_absolute(Path::new(value))?;
        }
    }
    if c.running_origin.is_none() && executable_identity(&c.executable)? != c.running {
        return Err("initial running executable does not match captured image".into());
    }
    Ok(())
}

pub(crate) fn validate_source_root(root: &Path) -> Result<(), String> {
    if normalized(root)? != root {
        return Err("source root is not physical".into());
    }
    let text = owned_file(&root.join("native/app/Cargo.toml"), MAX_METADATA)?;
    let doc: toml::Value = std::str::from_utf8(&text)
        .map_err(|e| e.to_string())?
        .parse()
        .map_err(|e| format!("invalid core manifest: {e}"))?;
    if doc
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(toml::Value::as_str)
        != Some(identity::BINARY)
    {
        return Err("not the desktop-pet core source package".into());
    }
    let root_arg = root.to_str().ok_or("non-UTF8 core checkout")?;
    let top = command_output(
        Path::new("/usr/bin/git"),
        &["-C", root_arg, "rev-parse", "--show-toplevel"],
        4096,
        5,
    )?;
    if Path::new(top.trim()) != root {
        return Err("source is not an exact core checkout root".into());
    }
    let remote = command_output(
        Path::new("/usr/bin/git"),
        &["-C", root_arg, "remote", "get-url", "origin"],
        4096,
        5,
    )?;
    if !matches!(
        remote.trim(),
        "https://github.com/hanbong5938/desktop-pet"
            | "https://github.com/hanbong5938/desktop-pet.git"
            | "git@github.com:hanbong5938/desktop-pet.git"
            | "ssh://git@github.com/hanbong5938/desktop-pet.git"
    ) {
        return Err("source checkout is not the fixed desktop-pet repository".into());
    }
    let script = root.join("scripts/install.sh");
    if normalized(&script)? != script {
        return Err("source installer escapes checkout".into());
    }
    owned_file(&script, MAX_METADATA)?;
    Ok(())
}

/// Validate bytes and product identity; staging need not be the installed OS-home bundle.
pub fn validate_standalone_bundle(
    bundle: &Path,
    expected_version: &str,
) -> Result<ExecutableIdentity, String> {
    let app = crate::installation::validate_staged_app(bundle)?;
    if app.version != expected_version {
        return Err("bundle version differs from approved version".into());
    }
    Ok(app.image)
}

fn brew_origin(path: &Path) -> Result<Option<InstallOrigin>, String> {
    let parts: Vec<_> = path
        .components()
        .map(|p| p.as_os_str().to_string_lossy().into_owned())
        .collect();
    let Some(index) = parts.iter().position(|p| p == "Cellar") else {
        return Ok(None);
    };
    let formula = parts.get(index + 1).ok_or("incomplete Cellar path")?;
    if formula != STABLE_FORMULA {
        return Ok(None);
    }
    let keg = parts.get(index + 2).ok_or("incomplete keg path")?;
    let prefix = PathBuf::from("/").join(parts[1..index].join("/"));
    let keg_root = prefix.join("Cellar").join(formula).join(keg);
    let expected = app_binary(&keg_root.join("libexec").join(identity::APP_NAME));
    if normalized(path)? != expected {
        return Err("binary is not the formula bundle inside its keg".into());
    }
    let receipt: Value = serde_json::from_slice(&owned_file(
        &keg_root.join("INSTALL_RECEIPT.json"),
        MAX_METADATA,
    )?)
    .map_err(|e| e.to_string())?;
    if receipt.pointer("/source/tap").and_then(Value::as_str) != Some("hanbong5938/tap")
        || receipt.pointer("/source/spec").and_then(Value::as_str) != Some("stable")
    {
        return Err("unrecognized formula source/channel".into());
    }
    owned_file(
        &keg_root.join(".brew").join(format!("{formula}.rb")),
        MAX_METADATA,
    )?;
    let brew = prefix.join("bin/brew");
    executable_identity(&brew)?;
    let locator = app_binary(
        &prefix
            .join("opt")
            .join(formula)
            .join("libexec")
            .join(identity::APP_NAME),
    );
    if normalized(&locator)? != path {
        return Err("running keg is not active formula locator".into());
    }
    Ok(Some(InstallOrigin::Homebrew {
        brew,
        cellar_formula_root: prefix.join("Cellar").join(formula),
        prefix,
        formula: format!("{OFFICIAL_TAP}{formula}"),
        locator,
    }))
}

/// `validated` is a full bundle validation the caller just performed for this executable; the
/// standalone owner reuses it only for that exact physical bundle instead of validating it twice.
pub(crate) fn origin_for_executable(
    path: &Path,
    validated: Option<&crate::installation::ValidatedApp>,
) -> Result<InstallOrigin, String> {
    let path = normalized(path)?;
    if let Some(origin) = brew_origin(&path)? {
        return Ok(origin);
    }
    let bundle = standalone_bundle()?;
    if app_binary(&bundle) == path && normalized(&bundle)? == bundle {
        let fresh;
        let proof = match validated {
            Some(app) if app.bundle == bundle => app,
            _ => {
                fresh = crate::installation::validate_staged_app(&bundle)?;
                &fresh
            }
        };
        if proof.image.path != path {
            return Err("standalone image escaped bundle".into());
        }
        return Ok(InstallOrigin::Standalone { bundle });
    }
    for root in path.ancestors() {
        if source_binary(root) == path {
            validate_source_root(root)?;
            return Ok(InstallOrigin::Source {
                root: root.to_path_buf(),
            });
        }
    }
    if path
        .ancestors()
        .any(|p| p.extension().is_some_and(|e| e == "app"))
    {
        return Ok(InstallOrigin::Manual { locator: path });
    }
    Ok(InstallOrigin::Unknown {
        reason: "running image has no verified neutral installation owner".into(),
    })
}

fn locator(origin: &InstallOrigin) -> Result<PathBuf, String> {
    match origin {
        InstallOrigin::Source { root } => Ok(source_binary(root)),
        InstallOrigin::Standalone { bundle } => Ok(app_binary(bundle)),
        InstallOrigin::Homebrew { locator, .. } => Ok(locator.clone()),
        _ => Err("manual or unknown installation is unsupported".into()),
    }
}

pub fn detect_origin(c: &UpdateContext) -> Result<InstallOrigin, String> {
    context_valid(c)?;
    let Some(captured) = &c.running_origin else {
        return origin_for_executable(&c.running.path, None);
    };
    let current = match captured.as_ref() {
        InstallOrigin::Homebrew {
            cellar_formula_root,
            ..
        } if c.running.path.starts_with(cellar_formula_root) => {
            origin_for_executable(&locator(captured)?, None)
        }
        InstallOrigin::Source { root } if source_binary(root) == c.running.path => {
            validate_source_root(root)?;
            Ok((**captured).clone())
        }
        InstallOrigin::Standalone { bundle } if app_binary(bundle) == c.running.path => {
            origin_for_executable(&locator(captured)?, None)
        }
        InstallOrigin::Manual { locator } if *locator == c.running.path => Ok((**captured).clone()),
        _ => Err("captured running image is outside its installation".into()),
    };
    Ok(match current {
        Ok(current) if current == **captured => current,
        _ => InstallOrigin::Unknown {
            reason: "installation ownership changed since daemon startup".into(),
        },
    })
}

fn changed_candidate(
    c: &UpdateContext,
    locator: &Path,
) -> Result<Option<ExecutableIdentity>, String> {
    let image = executable_identity(locator)?;
    if image.sha256 == c.running.sha256 {
        return Ok(None);
    }
    let verified = validate_candidate_for_context(locator, c)?;
    if image_version(&verified)? < version(&c.version)? {
        return Err("candidate would downgrade running app".into());
    }
    if executable_identity(locator)? != verified {
        return Err("candidate changed during validation".into());
    }
    Ok(Some(verified))
}
fn image_version(image: &ExecutableIdentity) -> Result<Version, String> {
    let output = command_output(&image.path, &["--version"], 4096, 5)?;
    let value = output
        .trim()
        .strip_prefix(&format!("{} ", identity::BINARY))
        .ok_or("wrong app product version response")?;
    version(value)
}

pub fn executable_version(image: &ExecutableIdentity) -> Result<String, String> {
    Ok(image_version(image)?.to_string())
}

pub fn check(c: &UpdateContext, policy: UpdatePolicy) -> Result<CheckSnapshot, String> {
    let origin = detect_origin(c)?;
    let mut view = CheckSnapshot {
        status: CheckStatus::Unsupported,
        origin: origin.clone(),
        installed_version: None,
        observed_revision: None,
        detail: String::new(),
        plan: None,
        policy,
        release: None,
        checked_at: Some(now()?),
    };
    if let Ok(candidates) = releases::fetch_candidates() {
        view.release = releases::select_candidate(&candidates, &c.version, policy)?;
    }
    if matches!(
        origin,
        InstallOrigin::Manual { .. } | InstallOrigin::Unknown { .. }
    ) {
        view.detail = match &origin {
            InstallOrigin::Unknown { reason } => reason.clone(),
            _ => "Manual installation is unsupported; use an explicitly verified core installer"
                .into(),
        };
        return Ok(view);
    }
    let installed = locator(&origin)?;
    let image = validate_candidate_for_context(&installed, c)?;
    view.installed_version = Some(image_version(&image)?.to_string());
    if c.running_origin.is_some() {
        if let Some(candidate) = changed_candidate(c, &installed)? {
            view.status = CheckStatus::Installed;
            view.detail = "A different verified image is installed, not applied; explicitly apply this exact image".into();
            view.plan = Some(new_plan(
                c,
                &origin,
                UpdateAction::ApplyInstalled,
                Some(candidate),
            )?);
            return Ok(view);
        }
    }
    if matches!(origin, InstallOrigin::Source { .. }) {
        view.status = CheckStatus::Local;
        view.detail = "Core source checkout: rebuild these exact on-disk sources with --source; never fetch, reset, or change refs".into();
        view.plan = Some(new_plan(c, &origin, UpdateAction::SourceRebuild, None)?);
        return Ok(view);
    }
    if let Some(release) = &view.release {
        // Beta is informational until a distinct maintained installation is approved.
        if release.channel != crate::ReleaseChannel::Stable {
            view.detail =
                "Beta release is informational; the approved neutral installation remains stable"
                    .into();
            return Ok(view);
        }
        let action = match origin {
            InstallOrigin::Homebrew { .. } => UpdateAction::HomebrewUpgrade,
            _ => UpdateAction::StandaloneReplace,
        };
        let mut plan = new_plan(c, &origin, action, None)?;
        plan.target = Some(ReleaseTarget {
            policy,
            release: release.clone(),
            origin: origin.clone(),
            resolved_commit: None,
        });
        validate_plan_target(&plan)?;
        view.status = CheckStatus::Available;
        view.detail = format!("Approved core release {} is available; installation and restart require explicit consent", release.version);
        view.plan = Some(plan);
    } else {
        view.status = CheckStatus::Current;
        view.detail = "No newer approved core release under the selected policy".into();
    }
    Ok(view)
}

#[derive(Clone, Debug)]
pub struct ManagerCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub environment: Vec<(String, String)>,
}

pub fn validate_plan_target(plan: &UpdatePlan) -> Result<(), String> {
    let Some(target) = &plan.target else {
        if matches!(
            plan.action,
            UpdateAction::StandaloneReplace | UpdateAction::HomebrewUpgrade
        ) {
            return Err("release installation requires an exact approved target".into());
        }
        return Ok(());
    };
    target.release.validate()?;
    if !target.policy.allows_version(&target.release.version)?
        || version(&target.release.version)?.cmp_precedence(&version(&plan.context.version)?)
            != std::cmp::Ordering::Greater
        || plan.candidate.is_some()
        || target.origin != plan.origin
        || target.resolved_commit.is_some()
    {
        return Err("release target changes policy, manager, physical root or precedence".into());
    }
    match (&plan.origin, &plan.action) {
        (InstallOrigin::Standalone { bundle }, UpdateAction::StandaloneReplace)
            if *bundle == standalone_bundle()?
                && target.release.channel == crate::ReleaseChannel::Stable =>
        {
            Ok(())
        }
        (InstallOrigin::Homebrew { formula, .. }, UpdateAction::HomebrewUpgrade)
            if *formula == format!("{OFFICIAL_TAP}{STABLE_FORMULA}")
                && target.release.channel == crate::ReleaseChannel::Stable =>
        {
            Ok(())
        }
        _ => Err("release target does not match neutral installation manager".into()),
    }
}

pub fn refresh_release_target(plan: &UpdatePlan) -> Result<(), String> {
    recheck_plan(plan)?;
    if let Some(target) = &plan.target {
        if !releases::fetch_candidates()?
            .iter()
            .any(|release| release == &target.release)
        {
            return Err("approved release metadata changed since consent".into());
        }
        if let InstallOrigin::Homebrew { brew, formula, .. } = &target.origin {
            command_output(brew, &["update"], 65536, 60)?;
            brew_matches_release(&brew_metadata(brew, formula)?, &target.release)?;
        }
    }
    recheck_plan(plan)
}

pub fn recheck_plan(plan: &UpdatePlan) -> Result<(), String> {
    crate::protocol::validate_operation_id(&plan.operation_id)?;
    if plan.version != crate::protocol::UPDATER_PROTOCOL {
        return Err("unsupported update plan protocol".into());
    }
    context_valid(&plan.context)?;
    if plan.running != plan.context.running
        || plan.context.running_origin.as_deref() != Some(&plan.origin)
    {
        return Err("plan differs from immutable captured running origin".into());
    }
    validate_plan_target(plan)?;
    if detect_origin(&plan.context)? != plan.origin {
        return Err("installation source or manager changed since check".into());
    }
    let root = crate::physical_install_root(&plan.origin)?;
    for path in [&plan.context.config_dir, &plan.context.state_dir] {
        if resolved_parent_path(path)?.starts_with(root) {
            return Err("profile path is inside replaceable installation".into());
        }
    }
    if let Some(path) = &plan.context.assets_override {
        if resolved_parent_path(path)?.starts_with(root) {
            return Err("asset override is inside replaceable installation".into());
        }
    }
    for key in ["HOME", "XDG_CONFIG_HOME", "XDG_STATE_HOME"] {
        if let Some(value) = plan.context.environment.get(key) {
            if resolved_parent_path(Path::new(value))?.starts_with(root) {
                return Err(format!("{key} is inside replaceable installation"));
            }
        }
    }
    let installed = locator(&plan.origin)?;
    if matches!(plan.action, UpdateAction::ApplyInstalled) {
        if changed_candidate(&plan.context, &installed)?.as_ref() != plan.candidate.as_ref()
            || plan.candidate.is_none()
        {
            return Err("installed candidate changed or is not pinned".into());
        }
    } else if plan.candidate.is_some()
        || executable_identity(&installed)?.sha256 != plan.running.sha256
    {
        return Err("installation changed outside updater; check and apply explicitly".into());
    }
    match (&plan.origin, &plan.action) {
        (
            InstallOrigin::Source { .. },
            UpdateAction::SourceRebuild | UpdateAction::ApplyInstalled,
        )
        | (
            InstallOrigin::Homebrew { .. },
            UpdateAction::HomebrewUpgrade | UpdateAction::ApplyInstalled,
        )
        | (
            InstallOrigin::Standalone { .. },
            UpdateAction::StandaloneReplace | UpdateAction::ApplyInstalled,
        ) => Ok(()),
        _ => Err("action does not match verified neutral manager".into()),
    }
}

pub fn installed_candidate(plan: &UpdatePlan) -> Result<VerifiedInstallation, String> {
    installed_image(plan, false)
}
pub fn restored_original(plan: &UpdatePlan) -> Result<VerifiedInstallation, String> {
    if matches!(plan.action, UpdateAction::ApplyInstalled) {
        return Err("installed candidate is not an original restoration".into());
    }
    let proof = installed_image(plan, true)?;
    if proof.image != plan.running || proof.origin != plan.origin {
        return Err("original image changed after installation".into());
    }
    Ok(proof)
}
fn installed_image(plan: &UpdatePlan, original: bool) -> Result<VerifiedInstallation, String> {
    crate::protocol::validate_operation_id(&plan.operation_id)?;
    if plan.running != plan.context.running
        || plan.context.running_origin.as_deref() != Some(&plan.origin)
    {
        return Err("candidate does not belong to captured live origin".into());
    }
    let expected = if original {
        &plan.origin
    } else {
        plan.target_origin()
    };
    let path = locator(expected)?;
    let actual = origin_for_executable(&path, None)?;
    if &actual != expected {
        return Err("installed candidate changed origin".into());
    }
    let image = validate_candidate_for_context(&path, &plan.context)?;
    let installed_version = image_version(&image)?;
    if installed_version < version(&plan.context.version)? {
        return Err("candidate would downgrade running app".into());
    }
    if let Some(target) = &plan.target {
        if !original && installed_version != version(&target.release.version)? {
            return Err("candidate differs from approved release version".into());
        }
        if !original {
            if let InstallOrigin::Homebrew { brew, formula, .. } = expected {
                let metadata = brew_metadata(brew, formula)?;
                brew_matches_release(&metadata, &target.release)?;
                if metadata
                    .pointer("/installed/0/version")
                    .and_then(Value::as_str)
                    != Some(target.release.version.as_str())
                {
                    return Err("installed Homebrew receipt differs from approved version".into());
                }
            }
        }
    }
    if !original
        && image.sha256 == plan.running.sha256
        && !matches!(plan.action, UpdateAction::SourceRebuild)
    {
        return Err("installed executable has not changed".into());
    }
    if matches!(plan.action, UpdateAction::ApplyInstalled)
        && plan.candidate.as_ref() != Some(&image)
    {
        return Err("pinned candidate changed after consent".into());
    }
    if executable_identity(&path)? != image {
        return Err("candidate changed during verification".into());
    }
    Ok(VerifiedInstallation {
        image,
        origin: actual,
    })
}

pub fn manager_command(plan: &UpdatePlan) -> Result<Option<ManagerCommand>, String> {
    recheck_plan(plan)?;
    let c = &plan.context;
    let mut env = c.environment.clone();
    let home = env
        .get("HOME")
        .ok_or("captured HOME required for manager")?;
    ensure_absolute(Path::new(home))?;
    env.insert(
        "PATH".into(),
        format!("{home}/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"),
    );
    env.insert(
        "DESKTOP_PET_CONFIG_DIR".into(),
        c.config_dir.to_string_lossy().into_owned(),
    );
    env.insert(
        "DESKTOP_PET_STATE_DIR".into(),
        c.state_dir.to_string_lossy().into_owned(),
    );
    match (&plan.origin, &plan.action) {
        (InstallOrigin::Source { root }, UpdateAction::SourceRebuild) => {
            validate_source_root(root)?;
            let stage = crate::protocol::private_updates(&c.state_dir)?
                .join(format!("{}.source", plan.operation_id));
            Ok(Some(ManagerCommand {
                program: "/bin/bash".into(),
                args: vec![
                    root.join("scripts/install.sh")
                        .to_string_lossy()
                        .into_owned(),
                    "--source".into(),
                    "--stage-only".into(),
                    stage.to_string_lossy().into_owned(),
                ],
                cwd: root.clone(),
                environment: env.into_iter().collect(),
            }))
        }
        (InstallOrigin::Homebrew { brew, formula, .. }, UpdateAction::HomebrewUpgrade) => {
            let release = &plan
                .target
                .as_ref()
                .ok_or("missing approved release")?
                .release;
            brew_matches_release(&brew_metadata(brew, formula)?, release)?;
            env.insert("HOMEBREW_NO_AUTO_UPDATE".into(), "1".into());
            env.insert("HOMEBREW_NO_ANALYTICS".into(), "1".into());
            Ok(Some(ManagerCommand {
                program: brew.clone(),
                args: vec!["upgrade".into(), "--formula".into(), formula.clone()],
                cwd: c.state_dir.clone(),
                environment: env.into_iter().collect(),
            }))
        }
        (InstallOrigin::Standalone { .. }, UpdateAction::StandaloneReplace)
        | (_, UpdateAction::ApplyInstalled) => Ok(None),
        _ => Err("unsupported manager command".into()),
    }
}

fn new_plan(
    c: &UpdateContext,
    origin: &InstallOrigin,
    action: UpdateAction,
    candidate: Option<ExecutableIdentity>,
) -> Result<UpdatePlan, String> {
    let mut random = [0u8; 16];
    let mut f = fs::File::open("/dev/urandom").map_err(|e| e.to_string())?;
    f.read_exact(&mut random).map_err(|e| e.to_string())?;
    Ok(UpdatePlan {
        version: crate::protocol::UPDATER_PROTOCOL,
        operation_id: random.iter().map(|b| format!("{b:02x}")).collect(),
        context: c.clone(),
        origin: origin.clone(),
        action,
        running: c.running.clone(),
        candidate,
        target: None,
    })
}

fn brew_metadata(brew: &Path, formula: &str) -> Result<Value, String> {
    let short = formula
        .strip_prefix(OFFICIAL_TAP)
        .ok_or("unofficial formula tap")?;
    if !matches!(short, STABLE_FORMULA | BETA_FORMULA) {
        return Err("unofficial formula".into());
    }
    let output = command_output(
        brew,
        &["info", "--json=v2", "--formula", formula],
        65536,
        15,
    )?;
    brew_metadata_details(short, formula, &output)
}

fn brew_metadata_details(short: &str, formula: &str, output: &str) -> Result<Value, String> {
    let json: Value = serde_json::from_str(output).map_err(|e| e.to_string())?;
    let formulae = json
        .get("formulae")
        .and_then(Value::as_array)
        .ok_or("brew has no formula metadata array")?;
    if formulae.len() != 1 {
        return Err("brew did not return one exact formula".into());
    }
    let details = &formulae[0];
    if details.get("name").and_then(Value::as_str) != Some(short)
        || details.get("full_name").and_then(Value::as_str) != Some(formula)
    {
        return Err("brew returned a different formula/tap".into());
    }
    Ok(details.clone())
}

fn brew_matches_release(details: &Value, release: &ReleaseCandidate) -> Result<(), String> {
    let version = details.pointer("/versions/stable").and_then(Value::as_str);
    let url = details.pointer("/urls/stable/url").and_then(Value::as_str);
    let digest = details
        .pointer("/urls/stable/checksum")
        .and_then(Value::as_str);
    if version != Some(release.version.as_str())
        || url != Some(release.archive_url.as_str())
        || digest != Some(release.sha256.as_str())
    {
        return Err(
            "cached Homebrew formula version, archive or checksum differs from approved release"
                .into(),
        );
    }
    Ok(())
}

#[cfg(not(desktop_pet_legacy))]
pub(crate) fn verify_migration_brew_release(
    brew: &Path,
    formula: &str,
    release: &ReleaseCandidate,
    environment: &std::collections::BTreeMap<String, String>,
) -> Result<(), String> {
    release.validate()?;
    if formula.strip_prefix(OFFICIAL_TAP) != Some(STABLE_FORMULA)
        || release.channel != crate::ReleaseChannel::Stable
    {
        return Err(
            "migration target must use the fixed neutral desktop-pet stable formula".into(),
        );
    }
    executable_identity(brew)?;
    let output = command_output_with_environment(
        brew,
        &["info", "--json=v2", "--formula", formula],
        65536,
        15,
        environment,
    )?;
    let details = brew_metadata_details(STABLE_FORMULA, formula, &output)?;
    if details.get("revision").and_then(Value::as_u64) != Some(0) {
        return Err("approved migration Brew target does not permit a revisioned keg path".into());
    }
    brew_matches_release(&details, release)
}

fn resolved_parent_path(path: &Path) -> Result<PathBuf, String> {
    ensure_absolute(path)?;
    let mut ancestor = path;
    let mut suffix = Vec::new();
    loop {
        match fs::canonicalize(ancestor) {
            Ok(mut real) => {
                for name in suffix.into_iter().rev() {
                    real.push(name);
                }
                return Ok(real);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // Only append genuinely absent components. A dangling symlink
                // must not hide a target inside the replaceable installation.
                match fs::symlink_metadata(ancestor) {
                    Ok(_) => return Err(error.to_string()),
                    Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => {}
                    Err(other) => return Err(other.to_string()),
                }
                suffix.push(ancestor.file_name().ok_or("path has no basename")?);
                ancestor = ancestor.parent().ok_or("path has no parent")?;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

pub(crate) fn command_output(
    binary: &Path,
    args: &[&str],
    max: usize,
    seconds: u64,
) -> Result<String, String> {
    command_output_impl(binary, args, max, seconds, None)
}

pub(crate) fn command_output_with_environment(
    binary: &Path,
    args: &[&str],
    max: usize,
    seconds: u64,
    environment: &std::collections::BTreeMap<String, String>,
) -> Result<String, String> {
    command_output_impl(binary, args, max, seconds, Some(environment))
}

fn command_output_impl(
    binary: &Path,
    args: &[&str],
    max: usize,
    seconds: u64,
    environment: Option<&std::collections::BTreeMap<String, String>>,
) -> Result<String, String> {
    fn nonblocking(fd: libc::c_int) -> Result<(), String> {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
    fn stop(mut child: std::process::Child) {
        let _ = unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
        // Never block the metadata deadline on a wedged process's wait().
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
    fn drain<R: Read>(
        pipe: &mut Option<R>,
        output: &mut Vec<u8>,
        max: usize,
    ) -> Result<(), String> {
        let Some(reader) = pipe.as_mut() else {
            return Ok(());
        };
        let mut buf = [0; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    *pipe = None;
                    return Ok(());
                }
                Ok(n) => {
                    if n > max.saturating_sub(output.len()) {
                        return Err("read-only metadata output exceeds bound".into());
                    }
                    output.extend_from_slice(&buf[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(format!("metadata pipe read: {e}")),
            }
        }
    }
    let mut command = Command::new(binary);
    if let Some(environment) = environment {
        command.env_clear().envs(environment);
    }
    let mut child = command
        .args(args)
        .process_group(0)
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .env("HOMEBREW_NO_ANALYTICS", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_ASKPASS", "/usr/bin/false")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    if let Err(error) =
        nonblocking(stdout.as_raw_fd()).and_then(|()| nonblocking(stderr.as_raw_fd()))
    {
        stop(child);
        return Err(error);
    }
    let mut stdout = Some(stdout);
    let mut stderr = Some(stderr);
    let mut out = Vec::new();
    let mut err = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut status = None;
    loop {
        if status.is_none() {
            status = match child.try_wait() {
                Ok(value) => value,
                Err(e) => {
                    stop(child);
                    return Err(format!("metadata process wait: {e}"));
                }
            };
        }
        let drained =
            drain(&mut stdout, &mut out, max).and_then(|()| drain(&mut stderr, &mut err, max));
        if let Err(error) = drained {
            if status.is_none() {
                stop(child);
            }
            return Err(error);
        }
        if let Some(exit) = status {
            if stdout.is_none() && stderr.is_none() {
                if !exit.success() {
                    return Err(format!(
                        "metadata query failed: {}",
                        String::from_utf8_lossy(&err)
                    ));
                }
                return String::from_utf8(out).map_err(|e| e.to_string());
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            if status.is_none() {
                stop(child);
            }
            return Err("read-only metadata command timed out before exit and pipe EOF".into());
        }
        let mut pollfds = [
            libc::pollfd {
                fd: stdout.as_ref().map_or(-1, |f| f.as_raw_fd()),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: stderr.as_ref().map_or(-1, |f| f.as_raw_fd()),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let timeout = remaining.as_millis().min(25) as libc::c_int;
        let rc =
            unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::nfds_t, timeout) };
        if rc < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            if status.is_none() {
                stop(child);
            }
            return Err("read-only metadata poll failed".into());
        }
    }
}

/// Real account home, never an injected HOME or integration profile.
pub(crate) fn os_home() -> Result<PathBuf, String> {
    use std::os::unix::ffi::OsStrExt;
    let mut user: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0u8; 65536];
    let rc = unsafe {
        libc::getpwuid_r(
            libc::geteuid(),
            &mut user,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() || user.pw_dir.is_null() {
        return Err("cannot determine actual account home".into());
    }
    let home = PathBuf::from(std::ffi::OsStr::from_bytes(
        unsafe { std::ffi::CStr::from_ptr(user.pw_dir) }.to_bytes(),
    ));
    ensure_absolute(&home)?;
    normalized(&home)
}

#[cfg(test)]
mod tests {
    use super::*;
    use desktop_pet_contracts::ObservationSelection;
    use std::collections::BTreeMap;

    fn context() -> UpdateContext {
        UpdateContext {
            executable: "/tmp/DesktopPet.app/Contents/MacOS/desktop-pet".into(),
            version: identity::VERSION.into(),
            instance_id: "original-instance".into(),
            config_dir: "/tmp/profile-config".into(),
            state_dir: "/tmp/profile-state".into(),
            running: ExecutableIdentity {
                path: "/tmp/DesktopPet.app/Contents/MacOS/desktop-pet".into(),
                sha256: "a".repeat(64),
            },
            running_origin: Some(Box::new(InstallOrigin::Manual {
                locator: "/tmp/DesktopPet.app/Contents/MacOS/desktop-pet".into(),
            })),
            assets_override: None,
            environment: BTreeMap::new(),
            locale: "en".into(),
        }
    }

    #[test]
    fn neutral_context_uses_opaque_contract_selection_without_host_authority() {
        let mut c = context();
        assert_eq!(c.observation_mode().unwrap(), ObservationSelection::Auto);
        for provider in ["herdr", "tern", "custom-provider"] {
            c.environment
                .insert(identity::OBSERVATION_ENV.into(), provider.into());
            assert!(context_valid(&c).is_ok());
            assert_eq!(
                c.observation_mode().unwrap(),
                ObservationSelection::Provider(provider.into())
            );
            let retained: UpdateContext =
                serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
            assert_eq!(retained, c);
        }
        c.environment
            .insert(identity::OBSERVATION_ENV.into(), "both".into());
        assert!(context_valid(&c).is_err());
        c.environment.remove(identity::OBSERVATION_ENV);
        for key in [
            "HERDR_BIN_PATH",
            "HERDR_PLUGIN_CONFIG_DIR",
            "HERDR_SOCKET_PATH",
        ] {
            c.environment.insert(key.into(), "/tmp/injected".into());
            assert!(context_valid(&c).is_err());
            c.environment.remove(key);
        }
    }

    #[test]
    fn immutable_context_preserves_exact_tern_routing_and_rejects_invalid_values() {
        let mut c = context();
        for key in ["TERN_CONFIG_DIR", "TERN_DAEMON_SOCKET"] {
            c.environment
                .insert(key.into(), "/isolated/tern/endpoint".into());
            assert!(context_valid(&c).is_ok());
            let bytes = serde_json::to_vec(&c).unwrap();
            let retained: UpdateContext = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(retained, c);
            for invalid in [
                "".to_owned(),
                "relative".into(),
                "/bad\0path".into(),
                format!("/{}", "x".repeat(4096)),
            ] {
                c.environment.insert(key.into(), invalid);
                assert!(context_valid(&c).is_err());
            }
            c.environment.remove(key);
        }
    }

    #[test]
    fn unsupported_manual_installation_cannot_gain_manager_authority() {
        let c = context();
        let origin = detect_origin(&c).unwrap();
        assert!(matches!(origin, InstallOrigin::Manual { .. }));
        assert!(!origin.supports_update());
        let plan = new_plan(
            &c,
            &origin,
            UpdateAction::ApplyInstalled,
            Some(c.running.clone()),
        )
        .unwrap();
        assert!(recheck_plan(&plan).is_err());
        assert!(manager_command(&plan).is_err());
    }

    #[test]
    fn standalone_installation_requires_an_approved_exact_target() {
        let c = context();
        let origin = InstallOrigin::Standalone {
            bundle: standalone_bundle().unwrap(),
        };
        let plan = new_plan(&c, &origin, UpdateAction::StandaloneReplace, None).unwrap();
        assert!(validate_plan_target(&plan).is_err());
    }

    #[test]
    fn missing_profile_components_do_not_hide_installation_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let physical = fs::canonicalize(dir.path()).unwrap();
        assert_eq!(
            resolved_parent_path(&physical.join("missing/control.sock")).unwrap(),
            physical.join("missing/control.sock")
        );
        let alias = physical.join("alias");
        std::os::unix::fs::symlink(physical.join("missing"), &alias).unwrap();
        assert!(resolved_parent_path(&alias.join("control.sock")).is_err());
    }
}

pub(crate) fn brew_origin_for_executable(path: &Path) -> Result<Option<InstallOrigin>, String> {
    brew_origin(&normalized(path)?)
}

#[cfg(test)]
mod captured_environment_tests {
    use super::*;

    #[test]
    fn bounded_metadata_command_uses_only_captured_routing_and_fixed_safety_vars() {
        let captured = std::collections::BTreeMap::from([
            ("HOME".into(), "/tmp/captured-manager-home".into()),
            ("DESKTOP_PET_METADATA_CAPTURE".into(), "approved".into()),
            ("GIT_ASKPASS".into(), "/tmp/unapproved-askpass".into()),
        ]);
        let output =
            command_output_with_environment(Path::new("/usr/bin/env"), &[], 16384, 5, &captured)
                .unwrap();
        let fields: std::collections::BTreeMap<_, _> = output
            .lines()
            .map(|line| line.split_once('=').unwrap())
            .collect();
        assert_eq!(fields.get("HOME"), Some(&"/tmp/captured-manager-home"));
        assert_eq!(
            fields.get("DESKTOP_PET_METADATA_CAPTURE"),
            Some(&"approved")
        );
        assert_eq!(fields.get("GIT_ASKPASS"), Some(&"/usr/bin/false"));
        assert_eq!(fields.get("HOMEBREW_NO_AUTO_UPDATE"), Some(&"1"));
        assert!(!fields.contains_key("PATH"));
    }
}

#[cfg(test)]
mod brew_metadata_tests {
    use super::*;

    #[test]
    fn migration_and_normal_updates_share_exact_single_formula_metadata_proof() {
        let formula = format!("{OFFICIAL_TAP}{STABLE_FORMULA}");
        let details = serde_json::json!({"name":STABLE_FORMULA, "full_name":formula});
        let one = serde_json::json!({"formulae":[details.clone()]});
        assert_eq!(
            brew_metadata_details(STABLE_FORMULA, &formula, &one.to_string()).unwrap(),
            details
        );
        for output in [
            serde_json::json!({"formulae":[]}),
            serde_json::json!({"formulae":[details.clone(), details.clone()]}),
            serde_json::json!({"formulae":{"0":details}}),
            serde_json::json!({"formulae":[{"name":STABLE_FORMULA,"full_name":"other/tap/desktop-pet"}]}),
            serde_json::json!({"formulae":[{"name":"other","full_name":formula}]}),
        ] {
            assert!(brew_metadata_details(STABLE_FORMULA, &formula, &output.to_string()).is_err());
        }
    }
}
