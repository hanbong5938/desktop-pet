//! Startup compatibility is compiled only into the genuine legacy envelope.
use std::ffi::OsString;
use std::path::PathBuf;

const CAPTURED_HOST_CONFIG: &str = "HERDR_DESKTOP_PET_CAPTURED_HOST_PLUGIN_CONFIG_DIR";

fn root(variable: &str, suffix: &str) -> PathBuf {
    if let Some(value) = std::env::var_os(variable).filter(|value| !value.is_empty()) {
        return PathBuf::from(value).join("herdr");
    }
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|home| home.join(suffix).join("herdr"))
        .unwrap_or_else(|| {
            std::env::temp_dir().join(if variable == "XDG_STATE_HOME" {
                "herdr-state"
            } else {
                "herdr"
            })
        })
}

pub fn herdr_socket() -> PathBuf {
    std::env::var_os("HERDR_SOCKET_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| root("XDG_CONFIG_HOME", ".config").join("herdr.sock"))
}

/// A migration environment marker only selects untrusted captured input here.
/// Core must authenticate that candidate against startup_authorization before
/// origin detection or any host I/O; this function never grants startup authority.
pub fn host_plugin_config_dir(daemon_child: bool) -> Option<PathBuf> {
    if std::env::var("HERDR_PLUGIN_ID").ok().as_deref() != Some("desktop-pet")
        || std::env::var("HERDR_ENV")
            .ok()
            .is_none_or(|value| value.is_empty())
        || std::env::var("HERDR_BIN_PATH")
            .ok()
            .is_none_or(|value| value.is_empty())
    {
        return None;
    }
    let path = if daemon_child
        && (std::env::var_os("HERDR_DESKTOP_PET_STARTUP_TOKEN").is_some()
            || std::env::var_os(crate::migration::PLAN_ENV).is_some())
    {
        std::env::var_os(CAPTURED_HOST_CONFIG)
    } else if daemon_child {
        std::env::var_os("HERDR_PLUGIN_CONFIG_DIR")
    } else {
        std::env::var_os(CAPTURED_HOST_CONFIG)
            .or_else(|| std::env::var_os("HERDR_PLUGIN_CONFIG_DIR"))
    }?;
    let path = PathBuf::from(path);
    if !path.is_absolute()
        || path.file_name()? != "desktop-pet"
        || path.parent()?.file_name()? != "config"
        || path.parent()?.parent()?.file_name()? != "plugins"
    {
        return None;
    }
    Some(path)
}

/// Invoke once before workers start. Old injection priority is preserved by
/// removing conflicting explicit profile options, not by changing core rules.
pub fn preprocess(arguments: Vec<OsString>) -> Result<Vec<OsString>, String> {
    let config = std::env::var_os("HERDR_PLUGIN_CONFIG_DIR");
    let state = std::env::var_os("HERDR_PLUGIN_STATE_DIR");
    for (name, value) in [
        ("HERDR_PLUGIN_CONFIG_DIR", &config),
        ("HERDR_PLUGIN_STATE_DIR", &state),
    ] {
        if value.as_ref().is_some_and(|value| value.is_empty()) {
            return Err(format!("{name} is set to an empty path"));
        }
    }
    let mut result = Vec::with_capacity(arguments.len() + 8);
    let mut args = arguments.into_iter();
    let mut explicit_config = false;
    let mut explicit_state = false;
    let mut explicit_socket: Option<OsString> = None;
    while let Some(argument) = args.next() {
        let token = argument.to_str();
        if token == Some("--config-dir") || token == Some("--state-dir") {
            let value = args
                .next()
                .ok_or("legacy profile option is missing its path")?;
            let is_config = token == Some("--config-dir");
            if (is_config && explicit_config) || (!is_config && explicit_state) {
                return Err("legacy profile path option may only be supplied once".into());
            }
            if value.is_empty() {
                return Err("legacy profile option has an empty path".into());
            }
            if is_config {
                explicit_config = true;
            } else {
                explicit_state = true;
            }
            if (is_config && config.is_none()) || (!is_config && state.is_none()) {
                result.push(argument);
                result.push(value);
            }
        } else if token.is_some_and(|value| {
            value.starts_with("--config-dir=") || value.starts_with("--state-dir=")
        }) {
            let is_config = token.unwrap().starts_with("--config-dir=");
            if (is_config && explicit_config) || (!is_config && explicit_state) {
                return Err("legacy profile path option may only be supplied once".into());
            }
            if token
                .unwrap()
                .split_once('=')
                .is_none_or(|(_, value)| value.is_empty())
            {
                return Err("legacy profile option has an empty path".into());
            }
            if is_config {
                explicit_config = true;
            } else {
                explicit_state = true;
            }
            if (is_config && config.is_none()) || (!is_config && state.is_none()) {
                result.push(argument);
            }
        } else if matches!(token, Some("--socket" | "--herdr-socket")) {
            let value = args.next().ok_or("legacy socket option requires a path")?;
            if value.is_empty() || explicit_socket.is_some() {
                return Err("legacy socket requires one nonempty path".into());
            }
            explicit_socket = Some(value.clone());
            result.push("--herdr-socket".into());
            result.push(value);
        } else if let Some(value) = token.and_then(|value| {
            value
                .strip_prefix("--socket=")
                .or_else(|| value.strip_prefix("--herdr-socket="))
        }) {
            if value.is_empty() || explicit_socket.is_some() {
                return Err("legacy socket requires one nonempty path".into());
            }
            explicit_socket = Some(value.into());
            result.push(format!("--herdr-socket={value}").into());
        } else if token == Some("--host") {
            let host = args.next().ok_or("--host requires a value")?;
            result.push(argument);
            result.push(if host == "both" { "auto".into() } else { host });
        } else if token == Some("--host=both") {
            result.push("--host=auto".into());
        } else {
            result.push(argument);
        }
    }
    if let Some(value) = config {
        result.push("--config-dir".into());
        result.push(value);
    } else if !explicit_config {
        result.push("--config-dir".into());
        result.push(
            root("XDG_CONFIG_HOME", ".config")
                .join("plugins/config/desktop-pet")
                .into(),
        );
    }
    if let Some(value) = state {
        result.push("--state-dir".into());
        result.push(value);
    } else if !explicit_state {
        result.push("--state-dir".into());
        result.push(
            root("XDG_STATE_HOME", ".local/state")
                .join("plugins/desktop-pet")
                .into(),
        );
    }
    if let Some(socket) = explicit_socket {
        std::env::set_var("HERDR_SOCKET_PATH", socket);
    }
    if let Some(value) = std::env::var_os("HERDR_PET_OBSERVATION_MODE") {
        let token = value
            .to_str()
            .ok_or("legacy observation environment is not UTF-8")?;
        let selection =
            desktop_pet_contracts::ObservationSelection::migrate_legacy_input(Some(token))?;
        std::env::set_var(
            desktop_pet_contracts::OBSERVATION_SELECTION_ENV,
            selection.as_str(),
        );
    }
    for (old, new) in [
        (
            "HERDR_DESKTOP_PET_STARTUP_TOKEN",
            "DESKTOP_PET_STARTUP_TOKEN",
        ),
        (
            "HERDR_DESKTOP_PET_UPDATER_TOKEN",
            "DESKTOP_PET_UPDATER_TOKEN",
        ),
        (
            "HERDR_DESKTOP_PET_UPDATER_GENERATION",
            "DESKTOP_PET_UPDATER_GENERATION",
        ),
    ] {
        if let Some(value) = std::env::var_os(old) {
            if std::env::var_os(new).is_some_and(|canonical| canonical != value) {
                return Err(format!(
                    "conflicting legacy and canonical authorization variables: {old}, {new}"
                ));
            }
            std::env::set_var(new, value);
        }
    }
    Ok(result)
}

/// Preserve the protocol-3 normal status spelling without altering the internal
/// contracts model or normal protocol-4 serialization.
pub fn encode_control_response(mut value: serde_json::Value) -> Result<serde_json::Value, String> {
    if let Some(observation) = value.get_mut("observation_mode") {
        if let Some(token) = observation.as_str() {
            let selection = token.parse::<desktop_pet_contracts::ObservationSelection>()?;
            *observation = serde_json::Value::String(
                super::legacy_codec::observation_token(&selection)?.into(),
            );
        }
    }
    Ok(value)
}

pub enum LegacyControlFrame {
    Common(Vec<u8>),
    Register {
        frame: Vec<u8>,
        command: String,
        readiness_only: bool,
    },
}

/// Core performs common framing/recursive duplicate-key validation first.
/// This boundary then preserves old endpoint commands via the registered
/// adapter's real admission/readiness implementation.
pub fn decode_control_request(frame: &[u8]) -> Result<LegacyControlFrame, String> {
    #[derive(serde::Deserialize)]
    struct Kind<'a> {
        #[serde(default, borrow)]
        kind: Option<&'a str>,
    }
    let kind: Kind<'_> = serde_json::from_slice(frame).map_err(|error| error.to_string())?;
    if kind.kind.is_some() {
        return Ok(LegacyControlFrame::Common(frame.to_vec()));
    }
    if frame.len() > 16 * 1024 {
        return Err("legacy control request exceeds size limit".into());
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        version: u64,
        command: String,
        #[serde(default)]
        endpoint: Option<PathBuf>,
    }
    let request: Request = serde_json::from_slice(frame).map_err(|error| error.to_string())?;
    if request.version != 1 || request.command.len() > 64 {
        return Err("invalid legacy control version or command".into());
    }
    if matches!(request.command.as_str(), "register" | "ensure")
        || request.command == "ready" && request.endpoint.is_some()
    {
        let endpoint = request.endpoint.unwrap_or_else(herdr_socket);
        crate::ensure_absolute(&endpoint)?;
        if endpoint.as_os_str().len() > 4096 {
            return Err("legacy endpoint exceeds size limit".into());
        }
        let readiness_only = request.command == "ready";
        let frame = serde_json::to_vec(&serde_json::json!({
            "kind":"register", "endpoint":endpoint, "readiness_only":readiness_only,
        }))
        .map_err(|error| error.to_string())?;
        Ok(LegacyControlFrame::Register {
            frame,
            command: request.command,
            readiness_only,
        })
    } else {
        let frame = serde_json::to_vec(&serde_json::json!({
            "version":request.version,"command":request.command,
        }))
        .map_err(|error| error.to_string())?;
        Ok(LegacyControlFrame::Common(frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_endpoint_readiness_is_a_query_not_registration() {
        for command in ["register", "ensure", "ready"] {
            let request = serde_json::to_vec(&serde_json::json!({
                "version":1,"command":command,"endpoint":"/tmp/legacy-herdr.sock",
            }))
            .unwrap();
            let LegacyControlFrame::Register {
                frame,
                command: retained,
                readiness_only,
            } = decode_control_request(&request).unwrap()
            else {
                panic!("endpoint command was lost")
            };
            assert_eq!(retained, command);
            assert_eq!(readiness_only, command == "ready");
            let forwarded: serde_json::Value = serde_json::from_slice(&frame).unwrap();
            assert_eq!(forwarded["kind"], "register");
            assert_eq!(forwarded["endpoint"], "/tmp/legacy-herdr.sock");
            assert_eq!(forwarded["readiness_only"], command == "ready");
        }
        assert!(matches!(
            decode_control_request(br#"{"version":1,"command":"ready"}"#).unwrap(),
            LegacyControlFrame::Common(_)
        ));
    }

    #[test]
    fn unsupported_legacy_control_evidence_is_not_silently_admitted() {
        for frame in [
            br#"{"version":3,"command":"ready"}"#.as_slice(),
            br#"{"version":1,"command":"register","endpoint":"relative.sock"}"#.as_slice(),
            br#"{"version":1,"command":"status","unknown":true}"#.as_slice(),
        ] {
            assert!(decode_control_request(frame).is_err());
        }
    }

    #[test]
    fn normal_status_wire_keeps_legacy_both_spelling() {
        let value = encode_control_response(serde_json::json!({
            "observation_mode":"auto","app_version":"0.3.12",
        }))
        .unwrap();
        assert_eq!(value["observation_mode"], "both");
        assert_eq!(value["app_version"], "0.3.12");
    }
}

const LEGACY_ROUTING_KEYS: &[&str] = &[
    "HERDR_BIN_PATH",
    "HERDR_PLUGIN_ID",
    "HERDR_ENV",
    "HERDR_PLUGIN_CONFIG_DIR",
    "HERDR_PLUGIN_STATE_DIR",
    "HERDR_SOCKET_PATH",
    "XDG_CONFIG_HOME",
    "XDG_STATE_HOME",
    "HOME",
    "HERDR_PET_OBSERVATION_MODE",
];

pub fn capture_environment(
    selection: &desktop_pet_contracts::ObservationSelection,
    mut lookup: impl FnMut(&str) -> Option<String>,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut environment = std::collections::BTreeMap::new();
    for &key in LEGACY_ROUTING_KEYS {
        if key == super::identity::OBSERVATION_ENV {
            continue;
        }
        if let Some(value) = lookup(key) {
            if value.len() > 4096 || value.contains('\0') {
                return Err(format!("invalid captured legacy routing variable {key}"));
            }
            environment.insert(key.to_owned(), value);
        }
    }
    environment.insert(
        super::identity::OBSERVATION_ENV.into(),
        super::legacy_codec::observation_token(selection)?.into(),
    );
    Ok(environment)
}

/// Migration captures only original routing provenance. Private updater/startup
/// authorization tokens are never disclosed by status or reconstructed here.
pub fn migration_source_environment(
    environment: &std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeMap<String, String> {
    environment
        .iter()
        .filter(|(key, _)| LEGACY_ROUTING_KEYS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Only an absent prior legacy settings file supplies the historical default.
/// Existing bytes, including old both, remain owned by the normal loader.
/// The caller invokes this at saved-selection priority, after explicit/env.
pub fn saved_observation_override(
    config_dir: &std::path::Path,
) -> Result<Option<desktop_pet_contracts::ObservationSelection>, String> {
    crate::ensure_absolute(config_dir)?;
    let path = config_dir.join("preferences.json");
    match std::fs::symlink_metadata(&path) {
        Ok(_) => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            desktop_pet_contracts::ObservationSelection::migrate_legacy_input(None).map(Some)
        }
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

#[cfg(test)]
mod saved_observation_tests {
    use super::*;

    #[test]
    fn historical_absence_does_not_override_existing_selection_bytes() {
        let config = tempfile::tempdir().unwrap();
        assert_eq!(
            saved_observation_override(config.path()).unwrap(),
            Some(desktop_pet_contracts::ObservationSelection::Provider(
                "herdr".into()
            ))
        );
        let path = config.path().join("preferences.json");
        let bytes = br#"{"observation_mode":"both"}"#;
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(saved_observation_override(config.path()).unwrap(), None);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert!(saved_observation_override(std::path::Path::new("relative")).is_err());
    }
}
