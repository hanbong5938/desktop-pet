use desktop_pet_contracts::profile_store::{
    atomic_write, read_object, read_object_with_presence, setting_bool, write_object,
    CONTROL_SOCKET_FILE, LIFECYCLE_FILE,
};
use desktop_pet_contracts::{LifecycleExtension, ProfileStore};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub const LOG_FILE: &str = "desktop-pet.log";
const STARTUP_KEY: &str = "startup";
const STARTUP_STALE_AFTER_SECS: u64 = 45;
const PROFILE_FILE: &str = "default-profile.json";
const MAX_PROFILE_BYTES: u64 = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct LifecycleSettings {
    pub auto_start: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LifecycleSetting {
    AutoStart,
}

impl std::str::FromStr for LifecycleSetting {
    type Err = String;
    fn from_str(key: &str) -> Result<Self, String> {
        match key {
            "auto_start" => Ok(Self::AutoStart),
            _ => Err(format!("unknown lifecycle setting: {key}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StartupLease {
    pub token: String,
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub control_socket: PathBuf,
    pub log_file: PathBuf,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProfileRecord {
    pub version: u32,
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
}

impl Paths {
    pub fn resolve(config: Option<&Path>, state: Option<&Path>) -> Result<Self, String> {
        let paths = Self::resolve_readonly(config, state)?;
        desktop_pet_contracts::profile_store::validate_directory(&paths.config_dir, true)?;
        desktop_pet_contracts::profile_store::validate_directory(&paths.state_dir, true)?;
        Ok(paths)
    }

    pub fn resolve_readonly(config: Option<&Path>, state: Option<&Path>) -> Result<Self, String> {
        let configured = selected_path(config, "DESKTOP_PET_CONFIG_DIR")?;
        let stated = selected_path(state, "DESKTOP_PET_STATE_DIR")?;
        // A malformed unused pointer must not override an explicit complete profile.
        let record = if configured.is_none() || stated.is_none() {
            read_profile_record()?
        } else {
            None
        };
        let config_dir = configured
            .or_else(|| record.as_ref().map(|r| r.config_dir.clone()))
            .map_or_else(default_config_root, Ok)?;
        let state_dir = stated
            .or_else(|| record.as_ref().map(|r| r.state_dir.clone()))
            .map_or_else(default_state_root, Ok)?;
        validate_path_components(&config_dir, false)?;
        validate_path_components(&state_dir, false)?;
        Ok(Self {
            control_socket: state_dir.join(CONTROL_SOCKET_FILE),
            log_file: state_dir.join(LOG_FILE),
            config_dir,
            state_dir,
        })
    }

    pub(crate) fn record(&self) -> ProfileRecord {
        ProfileRecord {
            version: 1,
            config_dir: self.config_dir.clone(),
            state_dir: self.state_dir.clone(),
        }
    }
}

fn selected_path(explicit: Option<&Path>, variable: &str) -> Result<Option<PathBuf>, String> {
    if let Some(path) = explicit {
        return Ok(Some(path.to_owned()));
    }
    match env::var_os(variable) {
        Some(value) if value.is_empty() => Err(format!("{variable} is set to an empty path")),
        Some(value) => Ok(Some(PathBuf::from(value))),
        None => Ok(None),
    }
}

fn home() -> Result<PathBuf, String> {
    env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| "HOME must be an absolute path".into())
}

fn default_config_root() -> Result<PathBuf, String> {
    Ok(
        match env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
            Some(value) => PathBuf::from(value),
            None => home()?.join(".config"),
        }
        .join("desktop-pet"),
    )
}

fn default_state_root() -> Result<PathBuf, String> {
    Ok(
        match env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
            Some(value) => PathBuf::from(value),
            None => home()?.join(".local/state"),
        }
        .join("desktop-pet"),
    )
}

fn read_profile_record() -> Result<Option<ProfileRecord>, String> {
    let path = default_config_root()?.join(PROFILE_FILE);
    validate_path_components(path.parent().expect("profile parent"), false)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot inspect profile record: {e}")),
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != effective_uid()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > MAX_PROFILE_BYTES
    {
        return Err("default profile record is not a bounded private owned regular file".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|e| format!("cannot read profile record: {e}"))?;
    let opened = file.metadata().map_err(|e| e.to_string())?;
    if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
        return Err("default profile record changed during read".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_PROFILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_PROFILE_BYTES {
        return Err("profile record is too large".into());
    }
    let record: ProfileRecord = serde_json::from_slice(&bytes)
        .map_err(|e| format!("invalid default profile record: {e}"))?;
    if record.version != 1 {
        return Err("unsupported default profile record version".into());
    }
    validate_path_components(&record.config_dir, false)?;
    validate_path_components(&record.state_dir, false)?;
    Ok(Some(record))
}

/// Read-only validation also checks ancestors, so a symlink cannot hide in a parent.
fn validate_path_components(path: &Path, require_existing: bool) -> Result<(), String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!(
            "profile path {} must be absolute without parent traversal",
            path.display()
        ));
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return Err(format!(
                    "profile directory {} is not a nonsymlink directory",
                    current.display()
                ))
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound && !require_existing => break,
            Err(e) => {
                return Err(format!(
                    "cannot inspect profile directory {}: {e}",
                    current.display()
                ))
            }
        }
    }
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.uid() != effective_uid() {
            return Err(format!(
                "profile directory {} is not owned by current user",
                path.display()
            ));
        }
    }
    Ok(())
}

pub(crate) fn adopt_profile(config: &Path, state: &Path) -> Result<ProfileRecord, String> {
    validate_path_components(config, true)?;
    validate_path_components(state, true)?;
    let root = default_config_root()?;
    validate_path_components(&root, false)?;
    // Validate old pointer before any mutation; never repair corrupt evidence silently.
    let previous = read_profile_record()?;
    let mut profiles = vec![(config.to_owned(), state.to_owned())];
    let previous_paths = if let Some(previous) = &previous {
        Some((previous.config_dir.clone(), previous.state_dir.clone()))
    } else {
        // The first pointer replaces the implicit XDG profile too. Its live
        // daemon/reservations remain authoritative even without a pointer file.
        let state = default_state_root()?;
        validate_path_components(&state, false)?;
        match fs::symlink_metadata(&state) {
            Ok(_) => Some((root.clone(), state)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("cannot inspect previous default profile: {error}")),
        }
    };
    if let Some(previous_paths) = previous_paths {
        if previous_paths.0 != config || previous_paths.1 != state {
            profiles.push(previous_paths);
        }
    }
    let _admission =
        desktop_pet_update_coordinator::protocol::admit_profile_adoption_profiles(&profiles)?;
    if read_profile_record()? != previous {
        return Err("default profile changed while adoption was being admitted".into());
    }
    let record = ProfileRecord {
        version: 1,
        config_dir: config.to_owned(),
        state_dir: state.to_owned(),
    };
    let bytes = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
    desktop_pet_contracts::profile_store::validate_directory(&root, true)?;
    atomic_write(&root.join(PROFILE_FILE), &bytes)?;
    Ok(record)
}

pub(crate) fn update_status_directory(explicit: Option<&Path>) -> Result<Option<PathBuf>, String> {
    let paths = Paths::resolve_readonly(None, explicit)?;
    let path = paths.state_dir;
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
        Ok(metadata)
            if metadata.uid() == effective_uid() && metadata.permissions().mode() & 0o077 == 0 =>
        {
            Ok(Some(path))
        }
        Ok(_) => Err("update status directory must be private and owned".into()),
    }
}

fn settings_from_object(
    config: &Path,
    object: &Map<String, Value>,
    _exists: bool,
) -> Result<LifecycleSettings, String> {
    let legacy = setting_bool(config, object, "enabled")?;
    Ok(LifecycleSettings {
        auto_start: setting_bool(config, object, "auto_start")?
            .or(legacy)
            .unwrap_or(true),
    })
}

fn put_settings(object: &mut Map<String, Value>, settings: LifecycleSettings) {
    object.remove("enabled");
    object.insert("auto_start".into(), Value::Bool(settings.auto_start));
}

pub(crate) fn read_settings(config: &Path) -> Result<LifecycleSettings, String> {
    let (object, exists) = read_object_with_presence(config)?;
    settings_from_object(config, &object, exists)
}

fn initialize_owner_settings(
    object: &mut Map<String, Value>,
    exists: bool,
    extensions: &[Arc<dyn LifecycleExtension>],
) -> Result<(), String> {
    if !exists {
        for extension in extensions {
            extension.initialize_lifecycle(object)?;
        }
    }
    Ok(())
}

/// Reset only registered user defaults, retaining runtime provenance,
/// routing, and all unknown owner fields from the current lifecycle object.
pub(crate) fn settings_reset_object(
    config: &Path,
    mut object: Map<String, Value>,
    extensions: &[Arc<dyn LifecycleExtension>],
) -> Result<Map<String, Value>, String> {
    let mut defaults = Map::new();
    initialize_owner_settings(&mut defaults, false, extensions)?;
    let settings = settings_from_object(config, &Map::new(), false)?;
    object.extend(defaults);
    put_settings(&mut object, settings);
    Ok(object)
}

pub(crate) fn write_settings(
    profile: &ProfileStore,
    extensions: &[Arc<dyn LifecycleExtension>],
    settings: LifecycleSettings,
) -> Result<(), String> {
    profile.update(|object, exists| {
        settings_from_object(profile.config_dir(), object, exists)?;
        initialize_owner_settings(object, exists, extensions)?;
        put_settings(object, settings);
        Ok(())
    })
}

pub(crate) fn update_setting(
    profile: &ProfileStore,
    extensions: &[Arc<dyn LifecycleExtension>],
    key: LifecycleSetting,
    value: bool,
) -> Result<LifecycleSettings, String> {
    profile.update(|object, exists| {
        let mut settings = settings_from_object(profile.config_dir(), object, exists)?;
        match key {
            LifecycleSetting::AutoStart => settings.auto_start = value,
        }
        initialize_owner_settings(object, exists, extensions)?;
        put_settings(object, settings);
        Ok(settings)
    })
}

pub fn validate_directory(path: &Path, create: bool) -> Result<PathBuf, String> {
    desktop_pet_contracts::profile_store::validate_directory(path, create)
}

/// Reserve a durable startup handoff before spawning the detached daemon.
///
/// The normal lifecycle lock serializes writers, but it cannot be held across
/// `Command::spawn`: the child must acquire that lock itself.  This marker is
/// the durable ownership handoff that prevents a second ensure invocation from
/// spawning another child during that gap.
pub fn begin_startup(
    profile: &ProfileStore,
    extensions: &[Arc<dyn LifecycleExtension>],
) -> Result<Option<StartupLease>, String> {
    profile.with_writer_lock(|| reserve_startup(profile.config_dir(), extensions))
}

fn reserve_startup(
    config_dir: &Path,
    extensions: &[Arc<dyn LifecycleExtension>],
) -> Result<Option<StartupLease>, String> {
    let (mut object, exists) = read_object_with_presence(config_dir)?;
    if let Some(value) = object.get(STARTUP_KEY).cloned() {
        let marker = value.as_object().ok_or_else(|| {
            format!(
                "invalid lifecycle state {}: startup must be an object",
                config_dir.join(LIFECYCLE_FILE).display()
            )
        })?;
        let pid = marker
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok())
            .ok_or_else(|| {
                format!(
                    "invalid lifecycle state {}: startup pid is invalid",
                    config_dir.join(LIFECYCLE_FILE).display()
                )
            })?;
        let started = marker
            .get("started_unix_secs")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                format!(
                    "invalid lifecycle state {}: startup timestamp is invalid",
                    config_dir.join(LIFECYCLE_FILE).display()
                )
            })?;
        let now = unix_seconds();
        let claimed = marker
            .get("claimed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if now.saturating_sub(started) <= STARTUP_STALE_AFTER_SECS
            && ((claimed && pid_alive(pid) && pid != std::process::id())
                || (!claimed && unclaimed_owner_alive(marker, pid)))
        {
            return Ok(None);
        }
        // A dead launcher or an abandoned same-process attempt cannot own the
        // next startup.  Replacing its marker is safe under LifecycleLock.
        object.remove(STARTUP_KEY);
    }
    let settings = settings_from_object(config_dir, &object, exists)?;
    initialize_owner_settings(&mut object, exists, extensions)?;
    put_settings(&mut object, settings);
    let token = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let mut marker = Map::new();
    marker.insert(
        "pid".to_owned(),
        Value::Number(u64::from(std::process::id()).into()),
    );
    marker.insert("token".to_owned(), Value::String(token.clone()));
    marker.insert(
        "started_unix_secs".to_owned(),
        Value::Number(unix_seconds().into()),
    );
    object.insert(STARTUP_KEY.to_owned(), Value::Object(marker));
    write_object(config_dir, object)?;
    Ok(Some(StartupLease { token }))
}

/// Complete the daemon side of a startup handoff while retaining the marker
/// until the private control listener is bound.  Keeping the child pid in the
/// marker closes the gap where a second launcher could otherwise race startup.
pub fn claim_startup(config_dir: &Path, token: &str) -> Result<(), String> {
    if token.is_empty() {
        return Err("desktop-pet startup handoff token is empty".to_owned());
    }
    let mut object = read_object(config_dir)?;
    let Some(value) = object.get_mut(STARTUP_KEY) else {
        return Err("desktop-pet startup handoff was cancelled".to_owned());
    };
    let marker = value.as_object_mut().ok_or_else(|| {
        format!(
            "invalid lifecycle state {}: startup must be an object",
            config_dir.join(LIFECYCLE_FILE).display()
        )
    })?;
    if marker.get("token").and_then(Value::as_str) != Some(token) {
        return Err("desktop-pet startup handoff token does not match".to_owned());
    }
    marker.insert(
        "pid".to_owned(),
        Value::Number(u64::from(std::process::id()).into()),
    );
    marker.insert("claimed".to_owned(), Value::Bool(true));
    write_object(config_dir, object)
}

/// Record the child pid while the launcher still owns the lifecycle lock.
/// This keeps stop/finalization able to distinguish a spawned child from an
/// abandoned launcher even before the child claims the handoff.
pub fn set_startup_child(config_dir: &Path, token: &str, pid: u32) -> Result<(), String> {
    if token.is_empty() || pid == 0 {
        return Err("desktop-pet startup child is invalid".to_owned());
    }
    let mut object = read_object(config_dir)?;
    let Some(value) = object.get_mut(STARTUP_KEY) else {
        return Err("desktop-pet startup handoff was cancelled".to_owned());
    };
    let marker = value.as_object_mut().ok_or_else(|| {
        format!(
            "invalid lifecycle state {}: startup must be an object",
            config_dir.join(LIFECYCLE_FILE).display()
        )
    })?;
    if marker.get("token").and_then(Value::as_str) != Some(token) {
        return Err("desktop-pet startup handoff token does not match".to_owned());
    }
    marker.insert("child_pid".to_owned(), Value::Number(u64::from(pid).into()));
    write_object(config_dir, object)
}

/// Remove a claimed handoff after the control listener is accepting requests.
pub fn finish_startup(config_dir: &Path, token: &str) -> Result<(), String> {
    if token.is_empty() {
        return Err("desktop-pet startup handoff token is empty".to_owned());
    }
    let mut object = read_object(config_dir)?;
    let Some(value) = object.get(STARTUP_KEY) else {
        return Err("desktop-pet startup handoff was cancelled".to_owned());
    };
    let marker = value.as_object().ok_or_else(|| {
        format!(
            "invalid lifecycle state {}: startup must be an object",
            config_dir.join(LIFECYCLE_FILE).display()
        )
    })?;
    if marker.get("token").and_then(Value::as_str) != Some(token) {
        return Err("desktop-pet startup handoff token does not match".to_owned());
    }
    object.remove(STARTUP_KEY);
    write_object(config_dir, object)
}

/// Abandon a launch reservation after spawning fails or a bounded readiness
/// wait proves that this exact child never claimed the handoff.
pub fn abandon_startup(config_dir: &Path, token: &str) -> Result<(), String> {
    if token.is_empty() {
        return Ok(());
    }
    let mut object = read_object(config_dir)?;
    let remove = object
        .get(STARTUP_KEY)
        .and_then(Value::as_object)
        .and_then(|marker| marker.get("token"))
        .and_then(Value::as_str)
        == Some(token);
    if remove {
        object.remove(STARTUP_KEY);
        write_object(config_dir, object)?;
    }
    Ok(())
}

/// Return whether a non-expired launcher or daemon still owns a startup
/// handoff.  Callers use this while waiting for the lifecycle lock to become
/// quiescent; stale markers are reclaimed by `begin_startup`.
pub fn startup_pending(config_dir: &Path) -> Result<bool, String> {
    let object = read_object(config_dir)?;
    let Some(marker) = object.get(STARTUP_KEY).and_then(Value::as_object) else {
        return Ok(false);
    };
    let pid = marker
        .get("pid")
        .and_then(Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok())
        .unwrap_or(0);
    let started = marker
        .get("started_unix_secs")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let claimed = marker
        .get("claimed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let age_valid = unix_seconds().saturating_sub(started) <= STARTUP_STALE_AFTER_SECS;
    Ok(age_valid
        && if claimed {
            pid_alive(pid)
        } else {
            unclaimed_owner_alive(marker, pid)
        })
}

/// An unclaimed reservation stays owned while its recorded child (the only
/// process that can claim it) lives, or, before a child is recorded, while
/// its launcher lives.
fn unclaimed_owner_alive(marker: &Map<String, Value>, launcher: u32) -> bool {
    match marker
        .get("child_pid")
        .and_then(Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok())
    {
        Some(child) => pid_alive(child),
        None => pid_alive(launcher),
    }
}

/// Cancel a pending launch while holding the lifecycle lock. The child must
/// reject a missing handoff token before binding its control listener.
pub fn cancel_startup(config_dir: &Path) -> Result<(), String> {
    let mut object = read_object(config_dir)?;
    if object.remove(STARTUP_KEY).is_some() {
        write_object(config_dir, object)?;
    }
    Ok(())
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn effective_uid() -> u32 {
    desktop_pet_contracts::profile_store::effective_uid()
}

#[cfg(test)]
mod tests {
    use super::*;
    use desktop_pet_contracts::{LifecycleSettingView, ObservationSelection};
    use std::sync::atomic::{AtomicU64, Ordering};

    struct LifecycleOwner {
        initializations: AtomicU64,
        reject_initialization: bool,
    }

    impl LifecycleExtension for LifecycleOwner {
        fn provider(&self) -> &'static str {
            "fixture"
        }

        fn initialize_lifecycle(&self, candidate: &mut Map<String, Value>) -> Result<(), String> {
            self.initializations.fetch_add(1, Ordering::SeqCst);
            candidate.insert("owner_setting".into(), Value::Bool(true));
            candidate.insert("owner_future".into(), serde_json::json!({"token": 7}));
            if self.reject_initialization {
                return Err("owner initialization rejected".into());
            }
            Ok(())
        }

        fn settings(
            &self,
            store: &ProfileStore,
            _selection: &ObservationSelection,
            _locale: &str,
        ) -> Result<Vec<LifecycleSettingView>, String> {
            let (object, exists) = store.read_object_with_presence()?;
            let value =
                setting_bool(store.config_dir(), &object, "owner_setting")?.unwrap_or(!exists);
            Ok(vec![LifecycleSettingView {
                key: "owner_setting".into(),
                label: "Owner setting".into(),
                help: String::new(),
                value,
                enabled: true,
            }])
        }

        fn apply_setting(
            &self,
            store: &ProfileStore,
            key: &str,
            value: bool,
        ) -> Result<(), String> {
            if key != "owner_setting" {
                return Err(format!("unknown owner setting: {key}"));
            }
            store.update(|object, _| {
                object.insert(key.into(), Value::Bool(value));
                Ok(())
            })
        }

        fn should_exit(&self, _selection: &ObservationSelection, _now: std::time::Instant) -> bool {
            false
        }
    }

    fn owner(reject_initialization: bool) -> Arc<LifecycleOwner> {
        Arc::new(LifecycleOwner {
            initializations: AtomicU64::new(0),
            reject_initialization,
        })
    }

    #[test]
    fn reset_uses_registered_defaults_and_preserves_runtime_and_routing() {
        let fixture = Fixture::new();
        let extension = owner(false);
        let extensions: Vec<Arc<dyn LifecycleExtension>> = vec![extension.clone()];
        let original = serde_json::json!({
            "auto_start":false, "owner_setting":false,
            "startup":{"token":"live"}, "endpoints":["opaque"],
            "future_registration":{"installed":true},
        })
        .as_object()
        .unwrap()
        .clone();
        let reset = settings_reset_object(fixture.path(), original.clone(), &extensions).unwrap();
        assert_eq!(reset["auto_start"], true);
        assert_eq!(reset["owner_setting"], true);
        assert_eq!(reset["startup"], original["startup"]);
        assert_eq!(reset["endpoints"], original["endpoints"]);
        assert_eq!(
            reset["future_registration"],
            original["future_registration"]
        );
        assert_eq!(extension.initializations.load(Ordering::SeqCst), 1);
        assert!(!fixture.state().exists());
    }

    #[test]
    fn rejected_reset_defaults_do_not_publish_a_partial_lifecycle() {
        let fixture = Fixture::new();
        let extensions: Vec<Arc<dyn LifecycleExtension>> = vec![owner(true)];
        assert!(settings_reset_object(fixture.path(), Map::new(), &extensions).is_err());
        assert!(!fixture.state().exists());
    }

    fn first_common_write(
        profile: &ProfileStore,
        extensions: &[Arc<dyn LifecycleExtension>],
        operation: &str,
    ) -> Result<(), String> {
        match operation {
            "set" => {
                update_setting(profile, extensions, LifecycleSetting::AutoStart, false).map(|_| ())
            }
            "save" => write_settings(profile, extensions, LifecycleSettings { auto_start: false }),
            "startup" => begin_startup(profile, extensions).map(|_| ()),
            _ => Err(format!("unknown test lifecycle operation: {operation}")),
        }
    }

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = env::temp_dir().join(format!(
                "desktop-pet-lifecycle-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).expect("unique fixture");
            Self(root)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn state(&self) -> PathBuf {
            self.0.join(LIFECYCLE_FILE)
        }

        fn profile(&self) -> ProfileStore {
            ProfileStore::acquire(self.path(), self.path()).expect("profile writer")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("remove fixture");
        }
    }

    #[test]
    fn fresh_defaults_survive_every_first_common_write_and_startup_handoff() {
        for operation in ["set", "save", "startup"] {
            let fixture = Fixture::new();
            let profile = fixture.profile();
            let owner = owner(false);
            let extensions: Vec<Arc<dyn LifecycleExtension>> = vec![owner.clone()];
            assert!(read_settings(fixture.path()).unwrap().auto_start);
            assert!(
                owner
                    .settings(&profile, &ObservationSelection::Auto, "en")
                    .unwrap()[0]
                    .value
            );
            assert!(!fixture.state().exists());
            assert_eq!(owner.initializations.load(Ordering::SeqCst), 0);

            first_common_write(&profile, &extensions, operation).unwrap();
            let object = profile.read_object_with_presence().unwrap().0;
            assert_eq!(object["owner_setting"], true, "{operation}");
            assert_eq!(object["owner_future"], serde_json::json!({"token": 7}));
            assert_eq!(object["auto_start"], operation == "startup");

            let token = if operation == "startup" {
                object["startup"]["token"].as_str().unwrap().to_owned()
            } else {
                begin_startup(&profile, &extensions).unwrap().unwrap().token
            };
            profile
                .with_writer_lock(|| claim_startup(fixture.path(), &token))
                .unwrap();
            profile
                .with_writer_lock(|| finish_startup(fixture.path(), &token))
                .unwrap();
            assert!(
                owner
                    .settings(&profile, &ObservationSelection::Auto, "en")
                    .unwrap()[0]
                    .value
            );
            assert_eq!(owner.initializations.load(Ordering::SeqCst), 1);
            assert!(!startup_pending(fixture.path()).unwrap());
        }
    }

    #[test]
    fn existing_missing_or_explicit_owner_policy_is_not_initialized() {
        for explicit in [None, Some(false), Some(true)] {
            for operation in ["set", "save", "startup"] {
                let fixture = Fixture::new();
                let profile = fixture.profile();
                profile
                    .update(|object, _| {
                        object.insert("auto_start".into(), Value::Bool(false));
                        object.insert("future".into(), serde_json::json!({"kept": 9}));
                        if let Some(value) = explicit {
                            object.insert("owner_setting".into(), Value::Bool(value));
                        }
                        Ok(())
                    })
                    .unwrap();
                let owner = owner(false);
                let extensions: Vec<Arc<dyn LifecycleExtension>> = vec![owner.clone()];
                let before = fs::read(fixture.state()).unwrap();
                assert_eq!(
                    owner
                        .settings(&profile, &ObservationSelection::Auto, "en")
                        .unwrap()[0]
                        .value,
                    explicit.unwrap_or(false),
                );
                assert_eq!(fs::read(fixture.state()).unwrap(), before);

                first_common_write(&profile, &extensions, operation).unwrap();
                let object = profile.read_object_with_presence().unwrap().0;
                assert_eq!(
                    object.get("owner_setting").and_then(Value::as_bool),
                    explicit
                );
                assert_eq!(object["future"], serde_json::json!({"kept": 9}));
                assert!(!object.contains_key("owner_future"));
                assert_eq!(owner.initializations.load(Ordering::SeqCst), 0);
            }
        }
    }

    #[test]
    fn concurrent_first_common_writes_share_initialization_and_preserve_owner_data() {
        let fixture = Fixture::new();
        let profile = fixture.profile();
        let owner = owner(false);
        let extensions: Vec<Arc<dyn LifecycleExtension>> = vec![owner.clone()];
        let barrier = Arc::new(std::sync::Barrier::new(2));
        std::thread::scope(|scope| {
            let startup_profile = profile.clone();
            let startup_extensions = extensions.clone();
            let startup_barrier = barrier.clone();
            scope.spawn(move || {
                startup_barrier.wait();
                begin_startup(&startup_profile, &startup_extensions)
                    .unwrap()
                    .unwrap();
            });
            scope.spawn(|| {
                barrier.wait();
                update_setting(&profile, &extensions, LifecycleSetting::AutoStart, false).unwrap();
            });
        });
        let object = profile.read_object_with_presence().unwrap().0;
        assert_eq!(object["auto_start"], false);
        assert_eq!(object["owner_setting"], true);
        assert_eq!(object["owner_future"], serde_json::json!({"token": 7}));
        assert!(object["startup"]["token"].is_string());
        assert_eq!(owner.initializations.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn rejected_initialization_does_not_write_and_unregistered_owners_stay_absent() {
        for operation in ["set", "save", "startup"] {
            let fixture = Fixture::new();
            let profile = fixture.profile();
            let extensions: Vec<Arc<dyn LifecycleExtension>> = vec![owner(true)];
            assert!(first_common_write(&profile, &extensions, operation).is_err());
            assert!(!fixture.state().exists());
            first_common_write(&profile, &[], operation).unwrap();
            let object = profile.read_object_with_presence().unwrap().0;
            assert!(!object.contains_key("owner_setting"));
            assert!(!object.contains_key("owner_future"));
        }
    }

    #[test]
    fn explicit_readonly_profile_does_not_create_directories() {
        let fixture = Fixture::new();
        let root = fs::canonicalize(fixture.path()).expect("physical fixture");
        let config = root.join("absent-config");
        let state = root.join("absent-state");
        let paths = Paths::resolve_readonly(Some(&config), Some(&state)).expect("readonly paths");
        assert_eq!(paths.config_dir, config);
        assert_eq!(paths.state_dir, state);
        assert!(!paths.config_dir.exists() && !paths.state_dir.exists());
        assert_eq!(paths.record().version, 1);
    }

    #[test]
    fn adoption_rejects_invalid_and_symlink_target_before_any_mutation() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let root = fs::canonicalize(fixture.path()).expect("physical fixture");
        let config = root.join("config");
        let state = root.join("state");
        fs::create_dir(&config).expect("config fixture");
        fs::create_dir(&state).expect("state fixture");
        fs::set_permissions(&config, fs::Permissions::from_mode(0o755)).expect("fixture mode");
        let alias = root.join("alias");
        symlink(&config, &alias).expect("fixture alias");
        assert!(adopt_profile(Path::new("relative"), &state).is_err());
        assert!(adopt_profile(&alias, &state).is_err());
        assert_eq!(
            fs::metadata(&config)
                .expect("unchanged config")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert!(fs::read_dir(&config)
            .expect("config entries")
            .next()
            .is_none());
        assert!(fs::read_dir(&state)
            .expect("state entries")
            .next()
            .is_none());
    }

    #[test]
    fn common_lifecycle_write_preserves_integration_owned_keys() {
        let fixture = Fixture::new();
        validate_directory(fixture.path(), true).expect("private profile");
        atomic_write(&fixture.state(), br#"{"enabled":false,"owner_setting":true,"endpoints":["/opaque"],"future":{"kept":7}}"#).expect("initial profile");
        let profile = fixture.profile();
        update_setting(&profile, &[], LifecycleSetting::AutoStart, true).expect("common setting");
        let object = read_object(fixture.path()).expect("preserved profile");
        assert_eq!(object["auto_start"], true);
        assert_eq!(object["owner_setting"], true);
        assert_eq!(object["endpoints"], serde_json::json!(["/opaque"]));
        assert_eq!(object["future"], serde_json::json!({"kept":7}));
        assert!(!object.contains_key("enabled"));
    }

    #[test]
    fn startup_handoff_remains_owned_until_control_is_bound() {
        let fixture = Fixture::new();
        let profile = fixture.profile();
        let lease = begin_startup(&profile, &[])
            .expect("startup reservation")
            .expect("first launcher owns reservation");
        assert!(startup_pending(fixture.path()).expect("pending marker"));
        claim_startup(fixture.path(), &lease.token).expect("child claims reservation");
        assert!(startup_pending(fixture.path()).expect("claimed marker"));
        finish_startup(fixture.path(), &lease.token).expect("control listener bound");
        assert!(!startup_pending(fixture.path()).expect("finished marker"));
    }

    #[test]
    fn unclaimed_startup_is_owned_only_while_its_child_or_launcher_lives() {
        use std::process::Command;

        let fixture = Fixture::new();
        let profile = fixture.profile();
        let lease = begin_startup(&profile, &[])
            .expect("startup reservation")
            .expect("first launcher owns reservation");
        let mut dead = Command::new("/usr/bin/true").spawn().expect("spawn child");
        dead.wait().expect("reap child");
        set_startup_child(fixture.path(), &lease.token, dead.id()).expect("record child");
        assert!(!startup_pending(fixture.path()).expect("dead child marker"));
        let replacement = begin_startup(&profile, &[])
            .expect("startup reservation")
            .expect("dead child no longer owns reservation");
        assert_ne!(replacement.token, lease.token);

        let mut live = Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("spawn child");
        set_startup_child(fixture.path(), &replacement.token, live.id()).expect("record child");
        let pending = startup_pending(fixture.path()).expect("live child marker");
        let reserved = begin_startup(&profile, &[]).expect("startup reservation");
        let _ = live.kill();
        live.wait().expect("reap child");
        assert!(pending);
        assert!(reserved.is_none());

        fs::write(
            fixture.state(),
            format!(
                r#"{{"startup":{{"pid":{},"token":"orphan","started_unix_secs":{}}}}}"#,
                dead.id(),
                unix_seconds()
            ),
        )
        .expect("orphaned launcher marker");
        assert!(!startup_pending(fixture.path()).expect("dead launcher marker"));
    }

    #[test]
    fn cancelled_startup_cannot_be_claimed_or_published() {
        let fixture = Fixture::new();
        fs::write(fixture.state(), r#"{"enabled":false,"other":7}"#).expect("legacy state");
        fs::set_permissions(fixture.state(), fs::Permissions::from_mode(0o600))
            .expect("private legacy state");
        let profile = fixture.profile();
        let lease = begin_startup(&profile, &[])
            .expect("reserve")
            .expect("lease");
        cancel_startup(fixture.path()).expect("cancel");
        assert!(claim_startup(fixture.path(), &lease.token).is_err());
        assert!(set_startup_child(fixture.path(), &lease.token, std::process::id()).is_err());
        assert!(finish_startup(fixture.path(), &lease.token).is_err());
        assert!(!startup_pending(fixture.path()).expect("no pending lease"));
        assert!(
            !read_settings(fixture.path())
                .expect("preserved settings")
                .auto_start
        );
        assert_eq!(
            read_object(fixture.path()).expect("extra field")["other"],
            7
        );
    }
}
