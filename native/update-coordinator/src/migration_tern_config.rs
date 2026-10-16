//! Documented Tern KV is read-only migration evidence. Only Tern's own window
//! uses tern.kv.set to replace a route; native code never writes this store.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TernConnection {
    pub version: u32,
    pub executable: PathBuf,
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TernConfiguration {
    pub path: PathBuf,
    pub sha256: String,
    pub connection: TernConnection,
    device: u64,
    inode: u64,
    length: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    bound_source: ExecutableIdentity,
    source_binding_sha256: Option<String>,
}
fn roots(profile: &LegacyProfile) -> Result<(PathBuf, PathBuf), String> {
    if let Some(path) = profile.environment.get("TERN_CONFIG_DIR") {
        let path = PathBuf::from(path);
        crate::ensure_absolute(&path)?;
        return Ok((path.clone(), path));
    }
    let home = profile_home(&profile.environment, &crate::origin::os_home()?)?;
    #[cfg(target_os = "macos")]
    {
        let root = home.join("Library/Application Support/Tern");
        Ok((root.clone(), root))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok((
            profile
                .environment
                .get("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"))
                .join("tern"),
            profile
                .environment
                .get("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/state"))
                .join("tern"),
        ))
    }
}
fn connection(bytes: &[u8]) -> Result<Option<TernConnection>, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("Tern saved KV is invalid JSON: {e}"))?;
    let map = value.as_object().ok_or("Tern saved KV is not a JSON map")?;
    let parsed = if let Some(canonical) = map.get("connection") {
        // Presence, including malformed/null canonical data, forbids legacy fallback.
        serde_json::from_value(canonical.clone())
            .map_err(|e| format!("Tern canonical connection is invalid: {e}"))?
    } else {
        let fields = ["executable", "config_dir", "state_dir"];
        if fields.iter().all(|field| !map.contains_key(*field)) {
            return Ok(None);
        }
        if fields.iter().any(|field| !map.contains_key(*field)) {
            return Err(
                "Tern legacy connection is incomplete; no profile values were mixed".into(),
            );
        }
        serde_json::from_value(serde_json::json!({"version":1,"executable":map["executable"],"config_dir":map["config_dir"],"state_dir":map["state_dir"]})).map_err(|e| format!("Tern legacy connection is invalid: {e}"))?
    };
    let parsed: TernConnection = parsed;
    if parsed.version != 1 {
        return Err("Tern saved connection has an unsupported version".into());
    }
    crate::ensure_absolute(&parsed.config_dir)?;
    crate::ensure_absolute(&parsed.state_dir)?;
    if !parsed.executable.is_absolute() && parsed.executable != Path::new("herdr-desktop-pet") {
        return Err(
            "Tern saved connection is neither an absolute executable nor the actual legacy command"
                .into(),
        );
    }
    Ok(Some(parsed))
}
fn metadata(path: &Path) -> Result<fs::Metadata, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || metadata.mode() & 0o022 != 0
        || metadata.len() > MAX_PLAN
    {
        return Err("Tern KV evidence is not a bounded owned regular non-writable file".into());
    }
    Ok(metadata)
}
pub(super) fn verify_configuration(configuration: &TernConfiguration) -> Result<(), String> {
    let meta = metadata(&configuration.path)?;
    if meta.dev() != configuration.device
        || meta.ino() != configuration.inode
        || meta.len() != configuration.length
        || meta.mtime() != configuration.modified_seconds
        || meta.mtime_nsec() != configuration.modified_nanoseconds
        || format!(
            "{:x}",
            Sha256::digest(crate::owned_file(&configuration.path, MAX_PLAN)?)
        ) != configuration.sha256
    {
        return Err("Tern saved connection changed after update approval; no consumer or KV mutation was attempted".into());
    }
    Ok(())
}
fn capture(
    plan: &MigrationPlan,
    profile: &LegacyProfile,
    previous_target: Option<&ExecutableIdentity>,
    retained: Option<&TernConfiguration>,
    retained_plan: Option<&MigrationPlan>,
) -> Result<Option<TernConfiguration>, String> {
    let (_, state) = roots(profile)?;
    let path = state.join("plugin-data/tern-desktop-pet/kv.json");
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    owned_directory(&state)?;
    owned_directory(
        path.parent()
            .ok_or("Tern KV has no plugin data directory")?,
    )?;
    let meta = metadata(&path)?;
    let bytes = crate::owned_file(&path, MAX_PLAN)?;
    let Some(connection) = connection(&bytes)? else {
        return Ok(None);
    };
    if connection.config_dir != profile.config_dir || connection.state_dir != profile.state_dir {
        return Ok(None);
    }
    let mut source_binding_sha256 = None;
    let bound_source = if connection.executable == plan.target.image.path {
        plan.target.image.clone()
    } else if let Some(previous) =
        previous_target.filter(|image| connection.executable == image.path)
    {
        previous.clone()
    } else if let Some(retained) = retained.filter(|retained| retained.connection == connection) {
        // The exact approved SDK route/profile is ancestry even when its old
        // alias or physical image is now absent. No executable is reopened.
        source_binding_sha256 = retained.source_binding_sha256.clone();
        retained.bound_source.clone()
    } else if let Some(image) = ancestry::retained_source_route(
        retained_plan.unwrap_or(plan),
        &connection.executable,
        &connection.config_dir,
        &connection.state_dir,
    )? {
        image
    } else if connection.executable == Path::new("herdr-desktop-pet") {
        // A short legacy command is not installation authority. Only an actual
        // fresh source binding can disambiguate this exact saved command/pair.
        let live = status(profile)?;
        verify_status(plan, profile, &live, false)?;
        let bytes = exchange(
            &profile.control_socket,
            &serde_json::json!({"version":1,"kind":"tern-migration-evidence"}),
        )?;
        let binding: TernBindingEvidence =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if binding.version != 1
            || binding.kind != "tern-migration-evidence"
            || !binding.fresh
            || binding.instance_id != profile.instance_id
            || binding.window_id.is_empty()
            || binding.binding_epoch == 0
            || binding.sequence == 0
            || binding.local_host.is_empty()
            || binding.observation_generation == 0
            || binding.native_generation == 0
        {
            return Err(
                "Tern saved short command has no fresh confirmed original native binding".into(),
            );
        }
        source_binding_sha256 = Some(format!("{:x}", Sha256::digest(bytes)));
        plan.source_image.clone()
    } else {
        return Err("Tern saved executable has no exact approved source/owning-manager ancestry; its route was preserved".into());
    };
    let configuration = TernConfiguration {
        path,
        sha256: format!("{:x}", Sha256::digest(bytes)),
        connection,
        device: meta.dev(),
        inode: meta.ino(),
        length: meta.len(),
        modified_seconds: meta.mtime(),
        modified_nanoseconds: meta.mtime_nsec(),
        bound_source,
        source_binding_sha256,
    };
    verify_configuration(&configuration)?;
    Ok(Some(configuration))
}
pub(super) fn configure_delivery(plan: &mut MigrationPlan) {
    configure_delivery_with_previous(plan, None, None);
}
pub(super) fn configure_delivery_with_previous(
    plan: &mut MigrationPlan,
    previous_target: Option<&ExecutableIdentity>,
    retained_plan: Option<&MigrationPlan>,
) {
    let retained_deliveries = std::mem::take(&mut plan.tern_deliveries);
    let retained_primary = plan.tern_delivery.take();
    plan.tern_delivery_problem = None;
    let mut errors = Vec::new();
    for index in 0..plan.profiles.len() {
        let result = (|| -> Result<Option<TernDelivery>, String> {
            let profile = &plan.profiles[index];
            let retained = retained_deliveries
                .iter()
                .chain(retained_primary.iter())
                .find(|delivery| {
                    delivery.configuration.connection.config_dir == profile.config_dir
                        && delivery.configuration.connection.state_dir == profile.state_dir
                });
            let Some(configuration) = capture(
                plan,
                profile,
                previous_target,
                retained.map(|delivery| &delivery.configuration),
                retained_plan,
            )?
            else {
                if plan.profiles[index].tern_reconnect_required {
                    return Err("configured Tern provider has no authenticated saved original profile route; no plugin was changed".into());
                }
                return Ok(None);
            };
            plan.profiles[index].tern_reconnect_required = true;
            plan.tern_reconnect_required = true;
            // Even an already-current canonical route needs the qualified
            // consumer and a new same-window handover, not path equality.
            integrations::capture_tern_delivery(&plan.target, &plan.profiles[index], configuration)
                .map(Some)
        })();
        match result {
            Ok(Some(delivery)) => plan.tern_deliveries.push(delivery),
            Ok(None) => {}
            Err(error) => errors.push(format!(
                "{}: {error}",
                plan.profiles[index].state_dir.display()
            )),
        }
    }
    if !errors.is_empty() {
        plan.tern_delivery_problem = Some(errors.join("; "));
    }
}
pub(super) fn verify_cli_routing(
    profile: &LegacyProfile,
    program: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<PathBuf, String> {
    let (config, _) = roots(profile)?;
    if profile.environment.get("TERN_CONFIG_DIR") != environment.get("TERN_CONFIG_DIR")
        || profile.environment.get("TERN_DAEMON_SOCKET") != environment.get("TERN_DAEMON_SOCKET")
    {
        return Err(
            "actual Tern config/daemon routing was not preserved from the approved profile".into(),
        );
    }
    if let Some(socket) = environment.get("TERN_DAEMON_SOCKET") {
        crate::ensure_absolute(Path::new(socket))?;
    }
    let result: serde_json::Value =
        serde_json::from_str(&crate::origin::command_output_with_environment(
            program,
            &["plugin", "dir", "--json"],
            16384,
            10,
            environment,
        )?)
        .map_err(|e| e.to_string())?;
    if result["dir"].as_str().map(Path::new) != Some(config.join("plugins").as_path()) {
        return Err("actual Tern CLI plugin root differs from the documented captured configuration/state routing".into());
    }
    Ok(config.join("plugins/tern-desktop-pet"))
}
pub(super) fn verify_delivery_routing(delivery: &TernDelivery) -> Result<(), String> {
    if delivery
        .environment
        .get("TERN_DAEMON_SOCKET")
        .map(PathBuf::from)
        != delivery.daemon_socket
    {
        return Err("captured actual Tern daemon socket routing changed".into());
    }
    let installed = delivery
        .installed_package
        .as_deref()
        .ok_or("Tern delivery lacks captured CLI installation routing")?;
    let plugins = installed
        .parent()
        .ok_or("Tern delivery installation root has no parent")?;
    let result: serde_json::Value =
        serde_json::from_str(&crate::origin::command_output_with_environment(
            &delivery.program.path,
            &["plugin", "dir", "--json"],
            16384,
            10,
            &delivery.environment,
        )?)
        .map_err(|e| e.to_string())?;
    if result["dir"].as_str().map(Path::new) != Some(plugins) {
        return Err("actual captured Tern CLI config routing changed".into());
    }
    Ok(())
}
pub(super) fn approved_saved_route(
    plan: &MigrationPlan,
    executable: &Path,
    config: &Path,
    state: &Path,
) -> Result<bool, String> {
    // Immutable captured route ancestry survives the SDK's authorized KV cutover.
    // Current KV contents are checked separately at the handover boundary.
    Ok(plan
        .tern_deliveries
        .iter()
        .chain(plan.tern_delivery.iter())
        .any(|delivery| {
            let captured = &delivery.configuration.connection;
            captured.executable == executable
                && captured.config_dir == config
                && captured.state_dir == state
        }))
}
pub(super) fn verify_canonical_target(
    configuration: &TernConfiguration,
    target: &TernConnection,
) -> Result<(), String> {
    metadata(&configuration.path)?;
    let value: serde_json::Value =
        serde_json::from_slice(&crate::owned_file(&configuration.path, MAX_PLAN)?)
            .map_err(|e| e.to_string())?;
    let canonical: TernConnection = serde_json::from_value(
        value
            .get("connection")
            .ok_or("Tern SDK has not saved the canonical target connection")?
            .clone(),
    )
    .map_err(|e| e.to_string())?;
    if &canonical != target {
        return Err(
            "current Tern SDK canonical target/profile differs from the approved handover".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ambient_tern_environment_without_saved_profile_does_not_install_or_require_tern() {
        let (_temp, mut plan) = crate::migration::tests::fixture();
        let root = plan.profiles[0].state_dir.join("not-configured-tern");
        plan.profiles[0].environment.insert(
            "TERN_CONFIG_DIR".into(),
            root.to_string_lossy().into_owned(),
        );
        plan.profiles[0]
            .environment
            .insert("TERN_WINDOW_KEY".into(), "ambient-only".into());
        plan.profiles[0]
            .environment
            .insert("TERM_PROGRAM".into(), "Tern".into());
        configure_delivery(&mut plan);
        assert!(!plan.tern_reconnect_required);
        assert!(plan.tern_delivery.is_none());
        assert!(plan.tern_delivery_problem.is_none());
        assert!(!root.exists());
    }

    #[test]
    fn malformed_canonical_connection_never_falls_back_to_complete_legacy_values() {
        let bytes = br#"{"connection":{"version":1,"executable":"/old"},"executable":"/legacy","config_dir":"/config","state_dir":"/state"}"#;
        assert!(connection(bytes).is_err());
        assert!(connection(br#"{"connection":null,"executable":"/legacy","config_dir":"/config","state_dir":"/state"}"#).is_err());
        assert!(connection(br#"{}"#).unwrap().is_none());
    }
    #[test]
    fn actual_saved_profile_pair_is_preserved_without_mixing_partial_keys() {
        let parsed = connection(br#"{"executable":"/original/keg/herdr-desktop-pet","config_dir":"/original/config","state_dir":"/original/state"}"#).unwrap().unwrap();
        assert_eq!(parsed.config_dir, Path::new("/original/config"));
        assert_eq!(parsed.state_dir, Path::new("/original/state"));
        assert!(connection(
            br#"{"executable":"/original/keg/herdr-desktop-pet","config_dir":"/original/config"}"#
        )
        .is_err());
    }
    #[test]
    fn absent_old_alias_is_recovered_only_from_exact_retained_sdk_route_and_pair() {
        let (_temp, mut plan) = crate::migration::tests::fixture();
        let root = plan.profiles[0].state_dir.join("isolated-tern");
        plan.profiles[0].environment.insert(
            "TERN_CONFIG_DIR".into(),
            root.to_string_lossy().into_owned(),
        );
        let configuration = fixture_configuration(&plan);
        let alias = root.join("retired-bin/herdr-desktop-pet");
        assert!(!alias.exists());
        let saved = TernConnection {
            executable: alias.clone(),
            ..configuration.connection.clone()
        };
        protocol::write_private_bytes(
            &configuration.path,
            &serde_json::to_vec(&serde_json::json!({"connection":saved})).unwrap(),
            0o600,
        )
        .unwrap();
        plan.legacy_aliases.push(alias);
        let retained = capture(&plan, &plan.profiles[0], None, None, None)
            .unwrap()
            .unwrap();
        plan.legacy_aliases.clear();
        let before = fs::read(&configuration.path).unwrap();
        assert!(capture(&plan, &plan.profiles[0], None, None, None).is_err());
        let repaired = capture(
            &plan,
            &plan.profiles[0],
            Some(&plan.target.image),
            Some(&retained),
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(repaired.connection, retained.connection);
        assert_eq!(repaired.bound_source, plan.source_image);
        let mut other = plan.profiles[0].clone();
        other.config_dir = other.config_dir.join("different");
        assert!(capture(
            &plan,
            &other,
            Some(&plan.target.image),
            Some(&retained),
            None
        )
        .unwrap()
        .is_none());
        assert_eq!(fs::read(&configuration.path).unwrap(), before);
    }

    #[test]
    fn current_sdk_route_capture_uses_original_old_schema_retired_brew_authority() {
        let (_temp, original, mut record) = crate::migration::tests::retained_brew_fixture();
        let [native, _] = legacy_brew_cli_publications(&original.source_origin)
            .unwrap()
            .unwrap();
        let mut saved = fixture_configuration(&original);
        saved.connection.executable = native.clone();
        let kv_bytes =
            serde_json::to_vec(&serde_json::json!({"connection": saved.connection})).unwrap();
        protocol::write_private_bytes(&saved.path, &kv_bytes, 0o600).unwrap();
        let original_plan = fs::read(evidence_path(&original)).unwrap();
        let original_journal = fs::read(journal_path(&original)).unwrap();
        let mut current = original.clone();
        current.target.image.path = current
            .target
            .image
            .path
            .with_file_name("current-desktop-pet");
        current.target.image.sha256 = "e".repeat(64);
        current.target.release.version = crate::identity::neutral::VERSION.into();
        current.profiles[0].stop_generation = 4;
        current.profiles[0].instance_id = "current-instance".into();
        let captured = capture(
            &current,
            &current.profiles[0],
            Some(&original.target.image),
            None,
            Some(&original),
        )
        .unwrap()
        .unwrap();
        assert_eq!(captured.connection.executable, native);
        assert_eq!(captured.bound_source, original.source_image);
        assert!(captured.source_binding_sha256.is_none());
        verify_configuration(&captured).unwrap();
        assert_eq!(fs::read(&saved.path).unwrap(), kv_bytes);
        assert_eq!(fs::read(evidence_path(&original)).unwrap(), original_plan);
        assert_eq!(fs::read(journal_path(&original)).unwrap(), original_journal);
        record.target_committed = false;
        save(&original, &record).unwrap();
        assert!(capture(
            &current,
            &current.profiles[0],
            Some(&original.target.image),
            None,
            Some(&original),
        )
        .is_err());
        assert_eq!(fs::read(&saved.path).unwrap(), kv_bytes);
    }
}

pub(super) fn validate_configuration(
    plan: &MigrationPlan,
    configuration: &TernConfiguration,
) -> Result<(), String> {
    image_valid(&configuration.bound_source)?;
    if !hex(&configuration.sha256, 64)
        || configuration.length > MAX_PLAN
        || configuration
            .source_binding_sha256
            .as_ref()
            .is_some_and(|hash| !hex(hash, 64))
        || configuration.connection.version != 1
    {
        return Err("saved Tern configuration proof is structurally invalid".into());
    }
    let profile = plan
        .profiles
        .iter()
        .find(|profile| {
            profile.tern_reconnect_required
                && profile.config_dir == configuration.connection.config_dir
                && profile.state_dir == configuration.connection.state_dir
        })
        .ok_or("saved Tern proof does not belong to an approved original profile")?;
    let (_, state) = roots(profile)?;
    if configuration.path != state.join("plugin-data/tern-desktop-pet/kv.json") {
        return Err("saved Tern proof escaped the documented captured state routing".into());
    }
    Ok(())
}
pub(super) fn validate_delivery(
    plan: &MigrationPlan,
    delivery: &TernDelivery,
) -> Result<(), String> {
    validate_configuration(plan, &delivery.configuration)?;
    let profile = plan
        .profiles
        .iter()
        .find(|profile| {
            profile.config_dir == delivery.configuration.connection.config_dir
                && profile.state_dir == delivery.configuration.connection.state_dir
        })
        .ok_or("Tern delivery original profile is absent")?;
    let mut expected_environment = neutral_environment(profile);
    // Historical primary delivery evidence predates daemon-socket capture. It
    // may be decoded read-only, but cannot pass actual installation/handover.
    if delivery.installed_package.is_none()
        && !delivery.environment.contains_key("TERN_DAEMON_SOCKET")
    {
        expected_environment.remove("TERN_DAEMON_SOCKET");
    }
    if delivery.environment != expected_environment
        || delivery
            .environment
            .get("TERN_DAEMON_SOCKET")
            .map(PathBuf::from)
            != delivery.daemon_socket
    {
        return Err(
            "Tern delivery CLI environment does not preserve the exact original profile routing"
                .into(),
        );
    }
    if let Some(socket) = &delivery.daemon_socket {
        crate::ensure_absolute(socket)?;
    }
    if let Some(installed) = &delivery.installed_package {
        let (config, _) = roots(profile)?;
        if installed != &config.join("plugins/tern-desktop-pet") {
            return Err("Tern delivery escaped the actual approved CLI plugin root".into());
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn fixture_configuration(plan: &MigrationPlan) -> TernConfiguration {
    let root = PathBuf::from(
        plan.profiles[0]
            .environment
            .get("TERN_CONFIG_DIR")
            .expect("isolated test routing"),
    );
    assert!(root.starts_with(&plan.profiles[0].state_dir));
    let directory = root.join("plugin-data/tern-desktop-pet");
    ensure_private_directory(&directory).unwrap();
    let path = directory.join("kv.json");
    let connection = TernConnection {
        version: 1,
        executable: plan.source_image.path.clone(),
        config_dir: plan.profiles[0].config_dir.clone(),
        state_dir: plan.profiles[0].state_dir.clone(),
    };
    let bytes = serde_json::to_vec(&serde_json::json!({"connection":connection})).unwrap();
    protocol::write_private_new(&path, &bytes, 0o600).unwrap();
    let meta = metadata(&path).unwrap();
    TernConfiguration {
        path,
        sha256: format!("{:x}", Sha256::digest(bytes)),
        connection,
        device: meta.dev(),
        inode: meta.ino(),
        length: meta.len(),
        modified_seconds: meta.mtime(),
        modified_nanoseconds: meta.mtime_nsec(),
        bound_source: plan.source_image.clone(),
        source_binding_sha256: None,
    }
}
