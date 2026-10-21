use crate::automation::{
    lock_automation, AutomationState, DomainAction, DomainOperation, DomainRequest,
    PresentationAction, PresentationOperation, PresentationPatch, PresentationRequest,
    PresentationSnapshot, SessionIdentity, SharedAutomation,
};
use crate::bubble::BubblePlacement;
use crate::character_service::PackService;
use crate::character_types::{PackAction, PackListing, PackOperation, PackRequest};
use crate::lifecycle::{self, LifecycleSetting, Paths};
use crate::preferences::Preferences;
use crate::session_view::{SessionCursor, SessionFilter, SessionPageCursor, SessionPageRequest};
use crate::socket;
use crate::state::AppState;
use crate::ui;
use desktop_pet_contracts::{ControlExtension, HostConnections, LifecycleLock};
use desktop_pet_update_coordinator::protocol::{
    record_user_stop, start_allowed, stop_generation, verify_control_for, UpdateCommand,
    UpdateControlReply, UpdateControlRequest, UPDATER_PROTOCOL,
};
use desktop_pet_update_coordinator::{
    read_plan, validate_plan_target, InstallOrigin, UpdateContext, UpdatePlan, UpdatePolicy,
};
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs::{self, Permissions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub(crate) const PROTOCOL_VERSION: u64 = 1;
pub(crate) const MAX_FRAME_BYTES: usize = 16 * 1024;
const MAX_PROMPT_FRAME_BYTES: usize = 512 * 1024 + 4096;
const MAX_SESSION_FRAME_BYTES: usize = 512 * 1024;
pub(crate) const MAX_AUTOMATION_REPLY_BYTES: usize = 68 * 1024;
const MAX_CONNECTIONS: usize = 32;
const PACK_LIST_PAGE_SIZE: usize = 8;
const MAX_PACK_LIST_RECORDS: usize = 32;
const MAX_PACK_LIST_ROUND_TRIPS: usize = MAX_PACK_LIST_RECORDS / PACK_LIST_PAGE_SIZE + 1;
const MAX_SOURCE_PATH_BYTES: usize = 4096;

#[derive(Debug, Deserialize)]
struct FrameKind {
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyRequest {
    #[serde(default)]
    version: Option<u64>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    endpoint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecycleRequest {
    version: u64,
    kind: String,
    operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecycleReply {
    version: u64,
    kind: String,
    ok: bool,
    settings: Option<Map<String, Value>>,
    error: Option<String>,
}

fn validate_lifecycle_request(request: &LifecycleRequest) -> Result<(), String> {
    if request.version != PROTOCOL_VERSION || request.kind != "lifecycle" {
        return Err("invalid lifecycle protocol".to_owned());
    }
    match request.operation.as_str() {
        "get" if request.key.is_none() && request.value.is_none() => Ok(()),
        "set"
            if request
                .key
                .as_ref()
                .is_some_and(|key| !key.is_empty() && key.len() <= 128)
                && request.value.is_some() =>
        {
            Ok(())
        }
        _ => Err("invalid lifecycle operation or key/value".to_owned()),
    }
}

fn lifecycle_reply(result: Result<Map<String, Value>, String>) -> LifecycleReply {
    match result {
        Ok(settings) => LifecycleReply {
            version: PROTOCOL_VERSION,
            kind: "lifecycle".to_owned(),
            ok: true,
            settings: Some(settings),
            error: None,
        },
        Err(error) => LifecycleReply {
            version: PROTOCOL_VERSION,
            kind: "lifecycle".to_owned(),
            ok: false,
            settings: None,
            error: Some(error),
        },
    }
}

pub(crate) const PRESENTATION_PROTOCOL_VERSION: u64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PresentationRequestEnvelope {
    pub(crate) version: u64,
    pub(crate) kind: String,
    pub(crate) command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) expected_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) patch: Option<PresentationPatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PresentationReplyEnvelope {
    pub(crate) version: u64,
    pub(crate) kind: String,
    pub(crate) ok: bool,
    pub(crate) error: Option<String>,
    pub(crate) instance_id: Option<String>,
    pub(crate) snapshot: Option<PresentationSnapshot>,
    pub(crate) operation: Option<PresentationOperation>,
}

fn validate_presentation_request(request: &PresentationRequestEnvelope) -> Result<(), String> {
    if request.version != PRESENTATION_PROTOCOL_VERSION {
        return Err("unsupported presentation protocol version".to_owned());
    }
    if request.kind != "presentation" {
        return Err("invalid presentation request kind".to_owned());
    }
    let has_instance = request
        .instance_id
        .as_ref()
        .is_some_and(|id| !id.is_empty());
    let has_operation = request
        .operation_id
        .as_ref()
        .is_some_and(|id| !id.is_empty());
    let valid = match request.command.as_str() {
        "get" => {
            request.instance_id.is_none()
                && request.operation_id.is_none()
                && request.expected_revision.is_none()
                && request.patch.is_none()
        }
        "set" => has_instance && has_operation && request.patch.is_some(),
        "reset" => has_instance && has_operation && request.patch.is_none(),
        "status" => {
            has_instance
                && has_operation
                && request.expected_revision.is_none()
                && request.patch.is_none()
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err("invalid presentation request fields".to_owned())
    }
}

fn decode_presentation_request(frame: &[u8]) -> Result<PresentationRequestEnvelope, String> {
    reject_duplicate_json_keys(frame)
        .map_err(|error| format!("invalid presentation request: {error}"))?;
    // Option<T> otherwise makes an explicit null indistinguishable from absence.
    // Reject forbidden fields even when sent as null.
    let fields: Map<String, Value> = serde_json::from_slice(frame)
        .map_err(|error| format!("invalid presentation request: {error}"))?;
    let forbidden = match fields.get("command").and_then(Value::as_str) {
        Some("get") => ["instance_id", "operation_id", "expected_revision", "patch"]
            .iter()
            .any(|field| fields.contains_key(*field)),
        Some("set") => false,
        Some("reset") => fields.contains_key("patch"),
        Some("status") => fields.contains_key("patch") || fields.contains_key("expected_revision"),
        _ => true,
    };
    if forbidden
        || fields
            .get("expected_revision")
            .is_some_and(|value| !value.is_u64())
    {
        return Err("invalid presentation request fields".to_owned());
    }
    let request: PresentationRequestEnvelope = serde_json::from_value(Value::Object(fields))
        .map_err(|error| format!("invalid presentation request: {error}"))?;
    validate_presentation_request(&request)?;
    Ok(request)
}

fn presentation_error(error: String) -> PresentationReplyEnvelope {
    PresentationReplyEnvelope {
        version: PRESENTATION_PROTOCOL_VERSION,
        kind: "presentation".to_owned(),
        ok: false,
        error: Some(error),
        instance_id: None,
        snapshot: None,
        operation: None,
    }
}

fn presentation_success(
    instance_id: String,
    snapshot: Option<PresentationSnapshot>,
    operation: Option<PresentationOperation>,
) -> PresentationReplyEnvelope {
    PresentationReplyEnvelope {
        version: PRESENTATION_PROTOCOL_VERSION,
        kind: "presentation".to_owned(),
        ok: true,
        error: None,
        instance_id: Some(instance_id),
        snapshot,
        operation,
    }
}

fn presentation_response_json(response: &PresentationReplyEnvelope) -> io::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(response)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if bytes.len() + 1 > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "presentation response exceeds frame limit",
        ));
    }
    bytes.push(b'\n');
    Ok(bytes)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutomationRequestEnvelope {
    pub(crate) version: u64,
    pub(crate) kind: String,
    pub(crate) command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) request: Option<DomainRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) operation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutomationReplyEnvelope {
    pub(crate) version: u64,
    pub(crate) kind: String,
    pub(crate) ok: bool,
    pub(crate) error: Option<String>,
    pub(crate) instance_id: Option<String>,
    pub(crate) operation: Option<DomainOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionsRequestEnvelope {
    pub(crate) version: u64,
    pub(crate) kind: String,
    pub(crate) command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) page: Option<SessionPageRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) identity: Option<SessionIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionsReplyEnvelope {
    pub(crate) version: u64,
    pub(crate) kind: String,
    pub(crate) ok: bool,
    pub(crate) error: Option<String>,
    pub(crate) result: Option<Value>,
}

fn validate_automation_request(request: &AutomationRequestEnvelope) -> Result<(), String> {
    if request.version != PROTOCOL_VERSION
        || !matches!(request.kind.as_str(), "automation" | "prompt")
    {
        return Err("invalid automation protocol".to_owned());
    }
    let valid = match request.command.as_str() {
        "request" => {
            request.instance_id.is_none()
                && request.operation_id.is_none()
                && request.request.as_ref().is_some_and(|domain| {
                    !domain.instance_id.is_empty()
                        && !domain.operation_id.is_empty()
                        && match (&*request.kind, &domain.action) {
                            ("prompt", DomainAction::SessionPrompt { .. }) => true,
                            ("automation", DomainAction::SessionPrompt { .. }) => false,
                            ("automation", _) => true,
                            _ => false,
                        }
                })
        }
        "status" => {
            request.kind == "automation"
                && request.request.is_none()
                && request
                    .instance_id
                    .as_ref()
                    .is_some_and(|id| !id.is_empty())
                && request
                    .operation_id
                    .as_ref()
                    .is_some_and(|id| !id.is_empty())
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err("invalid automation request fields".to_owned())
    }
}

fn decode_automation_request(frame: &[u8]) -> Result<AutomationRequestEnvelope, String> {
    reject_duplicate_json_keys(frame)?;
    let fields: Map<String, Value> = serde_json::from_slice(frame)
        .map_err(|error| format!("invalid automation request: {error}"))?;
    let forbidden = match fields.get("command").and_then(Value::as_str) {
        Some("request") => {
            fields.contains_key("instance_id")
                || fields.contains_key("operation_id")
                || !fields.get("request").is_some_and(Value::is_object)
        }
        Some("status") => {
            fields.contains_key("request")
                || !fields.get("instance_id").is_some_and(Value::is_string)
                || !fields.get("operation_id").is_some_and(Value::is_string)
        }
        _ => true,
    };
    if forbidden {
        return Err("invalid automation request fields".to_owned());
    }
    let request: AutomationRequestEnvelope = serde_json::from_value(Value::Object(fields))
        .map_err(|error| format!("invalid automation request: {error}"))?;
    validate_automation_request(&request)?;
    Ok(request)
}

fn validate_sessions_request(request: &SessionsRequestEnvelope) -> Result<(), String> {
    if request.version != PROTOCOL_VERSION || request.kind != "sessions" {
        return Err("invalid sessions protocol".to_owned());
    }
    let valid = match request.command.as_str() {
        "list" => {
            request.identity.is_none()
                && request.page.as_ref().is_some_and(|page| {
                    !page.instance_id.is_empty() && (1..=128).contains(&page.limit)
                })
        }
        "detail" => {
            request.page.is_none()
                && request.identity.as_ref().is_some_and(|identity| {
                    !identity.instance_id.is_empty() && !identity.terminal_id.is_empty()
                })
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err("invalid sessions request fields".to_owned())
    }
}

fn decode_sessions_request(frame: &[u8]) -> Result<SessionsRequestEnvelope, String> {
    reject_duplicate_json_keys(frame)?;
    let fields: Map<String, Value> = serde_json::from_slice(frame)
        .map_err(|error| format!("invalid sessions request: {error}"))?;
    let valid = match fields.get("command").and_then(Value::as_str) {
        Some("list") => {
            !fields.contains_key("identity") && fields.get("page").is_some_and(Value::is_object)
        }
        Some("detail") => {
            !fields.contains_key("page") && fields.get("identity").is_some_and(Value::is_object)
        }
        _ => false,
    };
    if !valid {
        return Err("invalid sessions request fields".to_owned());
    }
    let request: SessionsRequestEnvelope = serde_json::from_value(Value::Object(fields))
        .map_err(|error| format!("invalid sessions request: {error}"))?;
    validate_sessions_request(&request)?;
    Ok(request)
}

struct UniqueJsonKeys;

impl<'de> DeserializeSeed<'de> for UniqueJsonKeys {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueJsonVisitor)
    }
}

struct UniqueJsonVisitor;

impl<'de> Visitor<'de> for UniqueJsonVisitor {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut keys = HashSet::new();
        while let Some(key) = access.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(de::Error::custom("duplicate JSON object key"));
            }
            access.next_value_seed(UniqueJsonKeys)?;
        }
        Ok(())
    }

    fn visit_seq<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while access.next_element_seed(UniqueJsonKeys)?.is_some() {}
        Ok(())
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_string<E>(self, _value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(())
    }
}

pub const PACK_PROTOCOL_VERSION: u64 = 1;

/// Bounded pack-only IPC envelope.  The envelope intentionally contains only
/// validated metadata and paths; manifests and image bytes never cross the
/// control socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackRequestEnvelope {
    pub version: u64,
    pub kind: String,
    pub command: String,
    #[serde(default)]
    pub request: Option<PackRequest>,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub offset: Option<usize>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackReplyEnvelope {
    pub version: u64,
    pub kind: String,
    pub command: String,
    pub ok: bool,
    #[serde(default)]
    pub listing: Option<PackListing>,
    #[serde(default)]
    pub operation: Option<PackOperation>,
    #[serde(default)]
    pub error: Option<String>,
}

pub(crate) const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);
const SOCKET_MODE: u32 = 0o600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlResponse {
    pub r#type: String,
    pub version: u64,
    pub app_version: String,
    pub updater_protocol: u32,
    pub observation_capability: u32,
    pub integration_api: u32,
    pub update_ui_visible: bool,
    pub update_operation_id: Option<String>,
    pub migration_operation: Option<String>,
    pub migration_gated: bool,
    #[cfg(desktop_pet_legacy)]
    pub migration_source_environment: Option<std::collections::BTreeMap<String, String>>,
    #[cfg(desktop_pet_legacy)]
    pub migration_source_host_plugin_config_dir: Option<PathBuf>,
    pub ok: bool,
    pub command: String,
    pub error: Option<String>,
    pub ready: bool,
    pub running: bool,
    pub phase: String,
    #[serde(default = "default_observation_mode")]
    pub observation_mode: String,
    #[serde(default)]
    pub host_connections: HostConnections,
    pub ui_ready: bool,
    pub control_ready: bool,
    pub registration_accepted: bool,
    pub data_connected: bool,
    pub connected_sources: usize,
    pub disconnected_sources: usize,
    pub sessions: usize,
    pub working: usize,
    pub blocked: usize,
    pub done: usize,
    pub unknown: usize,
    pub visible: bool,
    pub passthrough: bool,
    #[serde(default)]
    pub alpha_passthrough: bool,
    #[serde(default = "default_bubble_visible")]
    pub bubble_visible: bool,
    #[serde(default)]
    pub bubble_placement: BubblePlacement,
    pub scale: f64,
    pub shutdown: bool,
    pub auto_start: bool,
    pub pid: u32,
    #[serde(default)]
    pub instance_id: String,
    #[serde(flatten)]
    pub provider_fields: Map<String, Value>,
    #[serde(default)]
    pub running_sha256: String,
    #[serde(default)]
    pub assets_override: Option<String>,
    pub executable_path: String,
    pub config_dir: String,
    pub state_dir: String,
}

fn default_bubble_visible() -> bool {
    true
}

fn default_observation_mode() -> String {
    "auto".to_owned()
}

impl ControlResponse {
    fn status(command: impl Into<String>, inner: &ControlInner) -> Self {
        let command = command.into();
        let state = lock_unpoisoned(&inner.shared);
        let context = state.update_context();
        let instance_id = context
            .map(|value| value.instance_id.clone())
            .unwrap_or_default();
        let running_sha256 = context
            .map(|value| value.running.sha256.clone())
            .unwrap_or_default();
        let assets_override = context
            .and_then(|value| value.assets_override.as_ref())
            .map(|value| value.display().to_string());
        let ui_ready = state.is_ui_ready();
        let scene = state.scene();
        let settings = state.lifecycle_settings();
        let observation_mode = state.observation_mode().as_str().to_owned();
        let host_connections = state.host_connections().clone();
        let update_ui_visible = state.update_ui_visible();
        let update_operation_id = state.update_ui_operation_id().map(str::to_owned);
        let migration_operation = state.migration_operation().map(str::to_owned);
        let migration_gated = state.migration_gated();
        #[cfg(desktop_pet_legacy)]
        let migration_source_environment = context.map(|context| {
            desktop_pet_update_coordinator::legacy_entry::migration_source_environment(
                &context.environment,
            )
        });
        #[cfg(desktop_pet_legacy)]
        let migration_source_host_plugin_config_dir =
            context.and_then(|context| context.host_plugin_config_dir.clone());
        drop(state);
        let mut response = Self {
            r#type: if command == "ready" {
                "ready"
            } else {
                "status"
            }
            .to_owned(),
            version: PROTOCOL_VERSION,
            app_version: desktop_pet_update_coordinator::identity::VERSION.to_owned(),
            updater_protocol: desktop_pet_update_coordinator::identity::PROTOCOL,
            observation_capability:
                desktop_pet_update_coordinator::identity::OBSERVATION_CAPABILITY,
            integration_api: desktop_pet_update_coordinator::identity::INTEGRATION_API,
            update_ui_visible,
            update_operation_id,
            migration_operation,
            migration_gated,
            #[cfg(desktop_pet_legacy)]
            migration_source_environment,
            #[cfg(desktop_pet_legacy)]
            migration_source_host_plugin_config_dir,
            ok: true,
            command,
            error: None,
            ready: ui_ready && !scene.shutdown,
            running: true,
            phase: scene.phase.as_str().to_owned(),
            observation_mode,
            data_connected: host_connections.values().any(|count| *count > 0),
            host_connections,
            ui_ready,
            control_ready: true,
            registration_accepted: true,
            connected_sources: scene.connected_sources,
            disconnected_sources: scene.disconnected_sources,
            sessions: scene.sessions,
            working: scene.working,
            blocked: scene.blocked,
            done: scene.done,
            unknown: scene.unknown,
            visible: scene.visible,
            passthrough: scene.passthrough,
            alpha_passthrough: scene.alpha_passthrough,
            bubble_visible: scene.bubble_visible,
            bubble_placement: scene.bubble_placement,
            scale: scene.scale,
            shutdown: scene.shutdown,
            auto_start: settings.auto_start,
            instance_id,
            running_sha256,
            assets_override,
            pid: inner.pid,
            executable_path: inner.executable_path.display().to_string(),
            config_dir: inner.paths.config_dir.display().to_string(),
            state_dir: inner.paths.state_dir.display().to_string(),
            provider_fields: Map::new(),
        };
        for fields in &inner.status_fields {
            match fields() {
                Ok(fields) => {
                    if fields.keys().any(|key| {
                        common_status_key(key) || response.provider_fields.contains_key(key)
                    }) {
                        response.ok = false;
                        response.error = Some(
                            "integration status attempted to replace common status fields".into(),
                        );
                        break;
                    }
                    response.provider_fields.extend(fields);
                }
                Err(error) => {
                    response.ok = false;
                    response.error = Some(error);
                    break;
                }
            }
        }
        response
    }

    fn error(command: impl Into<String>, inner: &ControlInner, message: impl Into<String>) -> Self {
        let mut response = Self::status(command, inner);
        response.r#type = "error".to_owned();
        response.ok = false;
        response.error = Some(message.into());
        response
    }
}

/// Serialize a local-control response as one bounded newline-delimited frame.
fn response_json(response: &ControlResponse) -> io::Result<Vec<u8>> {
    let value = serde_json::to_value(response)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    #[cfg(desktop_pet_legacy)]
    let value = desktop_pet_update_coordinator::legacy_entry::encode_control_response(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut bytes = serde_json::to_vec(&value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if bytes.len() + 1 > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "control response exceeds frame limit",
        ));
    }
    bytes.push(b'\n');
    Ok(bytes)
}

struct ControlInner {
    stopping: AtomicBool,
    shared: Arc<Mutex<AppState>>,
    extensions: Vec<Arc<dyn ControlExtension>>,
    status_fields: Vec<crate::StatusFields>,
    packs: Arc<PackService>,
    paths: Paths,
    lifecycle_writes: Mutex<()>,
    executable_path: PathBuf,
    pid: u32,
    clients: Mutex<Vec<JoinHandle<()>>>,
    active_clients: Mutex<usize>,
}

pub struct ControlServer {
    inner: Arc<ControlInner>,
    listener: Option<UnixListener>,
    accept_thread: Option<JoinHandle<()>>,
    started: bool,
    stopped: bool,
}

impl ControlServer {
    pub fn bind(
        shared: Arc<Mutex<AppState>>,
        paths: Paths,
        executable_path: PathBuf,
        packs: Arc<PackService>,
        extensions: Vec<Arc<dyn ControlExtension>>,
        status_fields: Vec<crate::StatusFields>,
    ) -> Result<Self, String> {
        desktop_pet_contracts::profile_store::validate_directory(&paths.state_dir, true)?;
        let listener = bind_private_socket(&paths.control_socket)?;
        let inner = Arc::new(ControlInner {
            stopping: AtomicBool::new(false),
            shared,
            extensions,
            status_fields,
            packs,
            paths,
            lifecycle_writes: Mutex::new(()),
            executable_path,
            pid: std::process::id(),
            clients: Mutex::new(Vec::new()),
            active_clients: Mutex::new(0),
        });
        Ok(Self {
            inner,
            listener: Some(listener),
            accept_thread: None,
            started: false,
            stopped: false,
        })
    }

    pub fn start(&mut self) -> Result<(), String> {
        if self.started {
            return Ok(());
        }
        let listener = self
            .listener
            .take()
            .ok_or_else(|| "local control listener is unavailable".to_owned())?;
        let inner = Arc::clone(&self.inner);
        self.accept_thread = Some(
            thread::Builder::new()
                .name("desktop-pet-control".to_owned())
                .spawn(move || accept_loop(listener, inner))
                .map_err(|error| format!("cannot start local control listener: {error}"))?,
        );
        self.started = true;
        Ok(())
    }

    pub fn shutdown(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        self.inner.stopping.store(true, Ordering::Release);
        wake_listener(&self.inner.paths.control_socket);
        if let Some(thread) = self.accept_thread.take() {
            let _ = thread.join();
        }
        let mut clients = lock_unpoisoned(&self.inner.clients);
        let handles = std::mem::take(&mut *clients);
        drop(clients);
        for handle in handles {
            let _ = handle.join();
        }
        let _ = remove_owned_socket(&self.inner.paths.control_socket);
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn accept_loop(listener: UnixListener, inner: Arc<ControlInner>) {
    loop {
        if inner.stopping.load(Ordering::Acquire) {
            return;
        }
        reap_finished_clients(&inner);
        let (stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(_) if inner.stopping.load(Ordering::Acquire) => return,
            Err(_) => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
        };
        if inner.stopping.load(Ordering::Acquire) || !verify_peer_uid(&stream) {
            continue;
        }
        // Darwin inherits the listener's nonblocking flag on accepted sockets.
        // Client I/O uses blocking syscalls bounded by the absolute deadline.
        if stream.set_nonblocking(false).is_err() {
            continue;
        }
        if !acquire_client_slot(&inner) {
            continue;
        }
        let client_inner = Arc::clone(&inner);
        let handle = thread::Builder::new()
            .name("desktop-pet-control-client".to_owned())
            .spawn(move || {
                handle_client(stream, client_inner.clone());
                release_client_slot(&client_inner);
            });
        match handle {
            Ok(handle) => lock_unpoisoned(&inner.clients).push(handle),
            Err(_) => release_client_slot(&inner),
        }
    }
}

fn reap_finished_clients(inner: &ControlInner) {
    let mut finished = Vec::new();
    {
        let mut clients = lock_unpoisoned(&inner.clients);
        let mut index = 0;
        while index < clients.len() {
            if clients[index].is_finished() {
                finished.push(clients.swap_remove(index));
            } else {
                index += 1;
            }
        }
    }
    for handle in finished {
        let _ = handle.join();
    }
}

fn acquire_client_slot(inner: &ControlInner) -> bool {
    reserve_client_slot(&mut lock_unpoisoned(&inner.active_clients))
}

fn reserve_client_slot(active: &mut usize) -> bool {
    if *active >= MAX_CONNECTIONS {
        return false;
    }
    *active += 1;
    true
}

fn release_client_slot(inner: &ControlInner) {
    let mut active = lock_unpoisoned(&inner.active_clients);
    *active = active.saturating_sub(1);
}

fn read_frame(
    stream: &mut UnixStream,
    output: &mut [u8; MAX_FRAME_BYTES],
    deadline: Instant,
) -> io::Result<usize> {
    let mut chunk = [0u8; 1024];
    let mut used = 0usize;
    loop {
        let read = socket::read_with_deadline(stream, &mut chunk, deadline)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "control peer closed before frame terminator",
            ));
        }
        let newline = chunk[..read].iter().position(|byte| *byte == b'\n');
        let payload_len = newline.unwrap_or(read);
        if used + payload_len > output.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "control frame exceeds limit",
            ));
        }
        output[used..used + payload_len].copy_from_slice(&chunk[..payload_len]);
        used += payload_len;
        if newline.is_some() {
            if used > 0 && output[used - 1] == b'\r' {
                used -= 1;
            }
            return Ok(used);
        }
    }
}

pub(crate) fn reject_duplicate_json_keys(bytes: &[u8]) -> Result<(), String> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    UniqueJsonKeys
        .deserialize(&mut deserializer)
        .map_err(|error| error.to_string())?;
    deserializer.end().map_err(|error| error.to_string())
}

fn write_frame(stream: &mut UnixStream, bytes: &[u8], deadline: Instant) -> io::Result<()> {
    write_frame_counted(stream, bytes, deadline, &mut 0)
}

pub(crate) fn write_frame_counted(
    stream: &mut UnixStream,
    bytes: &[u8],
    deadline: Instant,
    written: &mut usize,
) -> io::Result<()> {
    while *written < bytes.len() {
        stream.set_write_timeout(Some(socket::remaining(deadline)?))?;
        let count = stream.write(&bytes[*written..])?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "control peer closed while writing",
            ));
        }
        *written += count;
    }
    Ok(())
}

fn read_client_frame_with_size(
    stream: &mut UnixStream,
    deadline: Instant,
    limit: usize,
) -> io::Result<(Vec<u8>, usize)> {
    read_client_frame_with_size_checked(stream, deadline, limit, false)
}

fn read_client_frame_with_size_checked(
    stream: &mut UnixStream,
    deadline: Instant,
    limit: usize,
    reject_partial: bool,
) -> io::Result<(Vec<u8>, usize)> {
    let mut frame = Vec::with_capacity(MAX_FRAME_BYTES);
    let mut chunk = [0u8; 1024];
    loop {
        let read = socket::read_with_deadline(stream, &mut chunk, deadline).map_err(|error| {
            if !reject_partial || frame.is_empty() {
                error
            } else {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("control peer interrupted a partial frame: {error}"),
                )
            }
        })?;
        if read == 0 {
            return Err(io::Error::new(
                if !reject_partial || frame.is_empty() {
                    io::ErrorKind::UnexpectedEof
                } else {
                    io::ErrorKind::InvalidData
                },
                "control peer closed before frame terminator",
            ));
        }
        let newline = chunk[..read].iter().position(|byte| *byte == b'\n');
        let count = newline.unwrap_or(read);
        if frame.len() + count >= limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "control frame exceeds limit",
            ));
        }
        frame.extend_from_slice(&chunk[..count]);
        if newline.is_some() {
            let wire_bytes = frame.len() + 1;
            if frame.last() == Some(&b'\r') {
                frame.pop();
            }
            return Ok((frame, wire_bytes));
        }
    }
}

fn read_client_frame(
    stream: &mut UnixStream,
    deadline: Instant,
    limit: usize,
) -> io::Result<Vec<u8>> {
    read_client_frame_with_size(stream, deadline, limit).map(|(frame, _)| frame)
}

pub(crate) fn read_client_frame_checked(
    stream: &mut UnixStream,
    deadline: Instant,
    limit: usize,
    reject_partial: bool,
) -> io::Result<Vec<u8>> {
    read_client_frame_with_size_checked(stream, deadline, limit, reject_partial)
        .map(|(frame, _)| frame)
}

fn common_status_key(key: &str) -> bool {
    matches!(
        key,
        "type"
            | "version"
            | "app_version"
            | "updater_protocol"
            | "observation_capability"
            | "integration_api"
            | "update_ui_visible"
            | "update_operation_id"
            | "migration_operation"
            | "migration_gated"
            | "migration_source_environment"
            | "migration_source_host_plugin_config_dir"
            | "ok"
            | "command"
            | "error"
            | "ready"
            | "running"
            | "phase"
            | "observation_mode"
            | "host_connections"
            | "ui_ready"
            | "control_ready"
            | "registration_accepted"
            | "data_connected"
            | "connected_sources"
            | "disconnected_sources"
            | "sessions"
            | "working"
            | "blocked"
            | "done"
            | "unknown"
            | "visible"
            | "passthrough"
            | "alpha_passthrough"
            | "bubble_visible"
            | "bubble_placement"
            | "scale"
            | "shutdown"
            | "auto_start"
            | "pid"
            | "instance_id"
            | "running_sha256"
            | "assets_override"
            | "executable_path"
            | "config_dir"
            | "state_dir"
    )
}

#[cfg(desktop_pet_legacy)]
fn dispatch_legacy_registration(
    inner: &ControlInner,
    frame: &[u8],
    command: &str,
    authorized: bool,
    deadline: Instant,
) -> ControlResponse {
    let result: Result<bool, String> = (|| {
        if !authorized {
            return Err("unauthorized legacy registration".into());
        }
        let kind: FrameKind =
            serde_json::from_slice(frame).map_err(|e| format!("invalid legacy frame: {e}"))?;
        let extension = inner
            .extensions
            .iter()
            .find(|extension| Some(extension.kind()) == kind.kind.as_deref())
            .ok_or("legacy integration is not registered")?;
        if frame.len() + 1 > extension.max_request_bytes() {
            return Err("legacy registration exceeds frame limit".into());
        }
        let preferences =
            crate::preference_access::LivePreferences::new(inner.shared.clone(), deadline);
        let bytes = extension.handle_frame(frame, &preferences)?;
        if bytes.len() > extension.max_response_bytes() {
            return Err("legacy registration response exceeds frame limit".into());
        }
        reject_duplicate_json_keys(&bytes)?;
        let reply: Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("invalid legacy registration reply: {e}"))?;
        if reply.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err("legacy registration rejected".into());
        }
        reply
            .get("registration_accepted")
            .and_then(Value::as_bool)
            .ok_or_else(|| "legacy registration omitted readiness proof".into())
    })();
    match result {
        Ok(accepted) => {
            let mut response = ControlResponse::status(command, inner);
            response.registration_accepted = accepted;
            response
        }
        Err(error) => {
            let mut response = ControlResponse::error(command, inner, error);
            response.registration_accepted = false;
            response.ready = false;
            response
        }
    }
}

fn verify_migration_control(
    inner: &ControlInner,
    request: &desktop_pet_update_coordinator::migration::MigrationControlRequest,
) -> Result<UpdateContext, String> {
    let context = lock_unpoisoned(&inner.shared)
        .update_context()
        .cloned()
        .ok_or("migration context unavailable")?;
    let bridge = desktop_pet_update_coordinator::identity::LEGACY;
    desktop_pet_update_coordinator::migration::verify_live_control(
        &request.plan_path,
        request,
        &context,
        bridge,
    )?;
    use desktop_pet_update_coordinator::migration::MigrationCommand;
    match (bridge, request.command) {
        (true, MigrationCommand::Abort) => {
            desktop_pet_update_coordinator::protocol::verify_legacy_migration_abort(
                &context.state_dir,
                inner.pid,
                &context.instance_id,
                &request.operation_id,
                &request.token,
            )?
        }
        (true, _) => desktop_pet_update_coordinator::protocol::verify_legacy_migration_control(
            &context.state_dir,
            inner.pid,
            &context.instance_id,
            &request.operation_id,
            &request.token,
        )?,
        (false, MigrationCommand::Stop) => {
            desktop_pet_update_coordinator::protocol::verify_migration_target_stop(
                &context.state_dir,
                inner.pid,
                &context.instance_id,
                &request.operation_id,
                &request.token,
            )?
        }
        (false, _) => desktop_pet_update_coordinator::protocol::verify_migration_target(
            &context.state_dir,
            inner.pid,
            &context.instance_id,
            &request.operation_id,
            &request.token,
        )?,
    }
    Ok(context)
}

fn dispatch_migration(
    inner: &ControlInner,
    request: &desktop_pet_update_coordinator::migration::MigrationControlRequest,
    deadline: Instant,
) -> Result<bool, String> {
    use desktop_pet_update_coordinator::migration::{self, MigrationCommand};
    let context = verify_migration_control(inner, request)?;
    let delegated = migration::delegated_approval(&migration::read_plan(&request.plan_path)?)?;
    if inner.stopping.load(Ordering::Acquire) {
        return Err("daemon is stopping".into());
    }
    match request.command {
        MigrationCommand::Prepare => {
            let plan = migration::read_plan(&request.plan_path)?;
            let (sender, receiver) = mpsc::channel();
            {
                let _writer = lock_unpoisoned(&inner.lifecycle_writes);
                let mut state = lock_unpoisoned(&inner.shared);
                if !state.is_ui_ready() {
                    return Err("migration source UI is not ready".into());
                }
                if state.migration_authorized(&request.operation_id, &request.token)
                    && (delegated
                        || (state.update_ui_visible()
                            && state.update_ui_operation_id()
                                == Some(request.operation_id.as_str())))
                {
                    return Ok(false);
                }
                state.set_pending_migration_plan(plan)?;
                if let Err(error) = state.queue_update_prepare(
                    request.operation_id.clone(),
                    request.token.clone(),
                    sender,
                ) {
                    state.cancel_update_prepare(&request.operation_id, &request.token);
                    return Err(error);
                }
            }
            ui::wake();
            match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    lock_unpoisoned(&inner.shared)
                        .cancel_update_prepare(&request.operation_id, &request.token);
                    ui::wake();
                    return Err(error);
                }
                Err(_) => {
                    lock_unpoisoned(&inner.shared)
                        .cancel_update_prepare(&request.operation_id, &request.token);
                    ui::wake();
                    return Err("migration main-thread preparation timed out".into());
                }
            }
            let _writer = lock_unpoisoned(&inner.lifecycle_writes);
            if let Err(error) = verify_migration_control(inner, request) {
                lock_unpoisoned(&inner.shared)
                    .cancel_update_prepare(&request.operation_id, &request.token);
                ui::wake();
                return Err(error);
            }
            lock_unpoisoned(&inner.shared)
                .promote_prepared_migration(&request.operation_id, &request.token)?;
            Ok(false)
        }
        MigrationCommand::ConsentAck | MigrationCommand::DelegationAck => {
            if (request.command == MigrationCommand::DelegationAck) != delegated {
                return Err(
                    "migration authorization acknowledgement kind differs from approval".into(),
                );
            }
            {
                let state = lock_unpoisoned(&inner.shared);
                if !desktop_pet_update_coordinator::identity::LEGACY
                    || state.scene().shutdown
                    || !state.migration_authorized(&request.operation_id, &request.token)
                {
                    return Err(
                        "migration final consent is not authorized for this exact operation".into(),
                    );
                }
            }
            if !delegated {
                ui::confirm_migration_consent_visibility(&request.operation_id)?;
            }
            verify_migration_control(inner, request)?;
            let state = lock_unpoisoned(&inner.shared);
            if state.scene().shutdown
                || !state.migration_authorized(&request.operation_id, &request.token)
                || (!delegated
                    && (!state.update_ui_visible()
                        || state.update_ui_operation_id() != Some(request.operation_id.as_str())))
            {
                return Err(
                    "migration final consent is not visible for this exact operation".into(),
                );
            }
            Ok(false)
        }
        MigrationCommand::Stop => {
            if desktop_pet_update_coordinator::identity::LEGACY && !delegated {
                ui::confirm_migration_consent_visibility(&request.operation_id)?;
                verify_migration_control(inner, request)?;
            }
            let state = lock_unpoisoned(&inner.shared);
            if state.scene().shutdown
                || (desktop_pet_update_coordinator::identity::LEGACY
                    && (!state.migration_authorized(&request.operation_id, &request.token)
                        || (!delegated
                            && (!state.update_ui_visible()
                                || state.update_ui_operation_id()
                                    != Some(request.operation_id.as_str())))))
            {
                return Err(
                    "migration source is not visibly prepared for this authorization".into(),
                );
            }
            Ok(true)
        }
        MigrationCommand::Abort | MigrationCommand::Commit => {
            let _writer = lock_unpoisoned(&inner.lifecycle_writes);
            if !lock_unpoisoned(&inner.shared)
                .migration_authorized(&request.operation_id, &request.token)
            {
                return Err("migration gate is not owned by this authorization".into());
            }
            if request.command == MigrationCommand::Commit {
                desktop_pet_update_coordinator::protocol::with_migration_target_commit(
                    &context.state_dir,
                    inner.pid,
                    &context.instance_id,
                    &request.operation_id,
                    &request.token,
                    || {
                        lock_unpoisoned(&inner.shared)
                            .release_migration_gate(&request.operation_id, &request.token)
                    },
                )?;
                inner.packs.finish_read_only_startup()?;
            } else {
                verify_migration_control(inner, request)?;
                lock_unpoisoned(&inner.shared)
                    .release_migration_gate(&request.operation_id, &request.token)?;
            }
            ui::wake();
            Ok(false)
        }
    }
}

fn handle_client(mut stream: UnixStream, inner: Arc<ControlInner>) {
    let deadline = Instant::now() + CLIENT_TIMEOUT;
    let limit = inner
        .extensions
        .iter()
        .map(|extension| extension.max_request_bytes())
        .max()
        .unwrap_or(0)
        .max(MAX_PROMPT_FRAME_BYTES + 1);
    let (frame, wire_bytes) = match read_client_frame_with_size(&mut stream, deadline, limit) {
        Ok(frame) => frame,
        Err(_) => return,
    };
    if reject_duplicate_json_keys(&frame).is_err() {
        return;
    }
    #[cfg(desktop_pet_legacy)]
    let frame = match desktop_pet_update_coordinator::legacy_entry::decode_control_request(&frame) {
        Ok(desktop_pet_update_coordinator::legacy_entry::LegacyControlFrame::Common(frame)) => {
            frame
        }
        Ok(desktop_pet_update_coordinator::legacy_entry::LegacyControlFrame::Register {
            frame,
            command,
            ..
        }) => {
            let result = dispatch_legacy_registration(
                &inner,
                &frame,
                &command,
                verify_peer_uid(&stream),
                deadline,
            );
            let _ = write_response(&mut stream, &result, deadline);
            return;
        }
        Err(error) => {
            let _ = write_response(
                &mut stream,
                &ControlResponse::error("unknown", &inner, error),
                deadline,
            );
            return;
        }
    };
    let frame_kind: FrameKind = match serde_json::from_slice(&frame) {
        Ok(frame_kind) => frame_kind,
        Err(_) => {
            let response = ControlResponse::error("unknown", &inner, "invalid control request");
            let _ = write_response(&mut stream, &response, deadline);
            return;
        }
    };
    if reject_duplicate_json_keys(&frame).is_err() {
        let response = ControlResponse::error("unknown", &inner, "invalid control request");
        let _ = write_response(&mut stream, &response, deadline);
        return;
    }
    if matches!(
        frame_kind.kind.as_deref(),
        Some("legacy-migration-route" | "migration-integration-handover")
    ) {
        let response = ControlResponse::error(
            "unknown",
            &inner,
            "migration integration proofs require the bounded native CLI",
        );
        let _ = write_response(&mut stream, &response, deadline);
        return;
    }
    if frame_kind.kind.as_deref() == Some("legacy-migration-context") {
        if frame.len() + 1 > MAX_FRAME_BYTES || !verify_peer_uid(&stream) {
            return;
        }
        let result = (|| -> Result<serde_json::Value, String> {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Request {
                version: u32,
                kind: String,
                config_dir: PathBuf,
                state_dir: PathBuf,
            }
            let request: Request = serde_json::from_slice(&frame).map_err(|e| e.to_string())?;
            if request.version != 1 || request.kind != "legacy-migration-context" {
                return Err("unsupported migration context request".into());
            }
            #[cfg(desktop_pet_legacy)]
            {
                let state = lock_unpoisoned(&inner.shared);
                state.update_mutation_allowed()?;
                let mut context = state
                    .update_context()
                    .cloned()
                    .ok_or("source startup context is unavailable")?;
                if context.config_dir != request.config_dir
                    || context.state_dir != request.state_dir
                    || !state.is_ui_ready()
                    || state.scene().shutdown
                {
                    return Err("migration context is not the exact ready source profile".into());
                }
                context.environment =
                    desktop_pet_update_coordinator::legacy_entry::migration_source_environment(
                        &context.environment,
                    );
                Ok(
                    serde_json::json!({"version":1,"kind":"legacy-migration-context","context":context}),
                )
            }
            #[cfg(not(desktop_pet_legacy))]
            {
                let _ = request;
                Err("neutral applications cannot approve a legacy source update".into())
            }
        })();
        let response = result.unwrap_or_else(|error| serde_json::json!({"version":1,"kind":"legacy-migration-context","error":error}));
        let _ = bounded_json(&response, MAX_FRAME_BYTES)
            .and_then(|bytes| write_frame(&mut stream, &bytes, deadline));
        return;
    }
    if matches!(
        frame_kind.kind.as_deref(),
        Some("legacy-migration" | "update")
    ) {
        if frame.len() + 1 > MAX_FRAME_BYTES || !verify_peer_uid(&stream) {
            return;
        }
        let request: desktop_pet_update_coordinator::migration::MigrationControlRequest =
            match serde_json::from_slice(&frame) {
                Ok(request) => request,
                Err(_) => return,
            };
        let migration_deadline = Instant::now() + Duration::from_secs(15);
        let result = dispatch_migration(&inner, &request, migration_deadline);
        let stopping = result.as_ref().is_ok_and(|stop| *stop);
        let reply = desktop_pet_update_coordinator::migration::MigrationControlReply {
            version: request.version,
            kind: request.kind.clone(),
            command: request.command,
            operation_id: request.operation_id.clone(),
            instance_id: request.instance_id.clone(),
            token: request.token.clone(),
            plan_path: request.plan_path.clone(),
            source_image: request.source_image.clone(),
            config_dir: request.config_dir.clone(),
            state_dir: request.state_dir.clone(),
            stop_generation: request.stop_generation,
            accepted: result.is_ok(),
            error: result.err(),
        };
        let written = bounded_json(&reply, MAX_FRAME_BYTES)
            .and_then(|bytes| write_frame(&mut stream, &bytes, migration_deadline))
            .is_ok();
        if written && stopping {
            let delegated =
                desktop_pet_update_coordinator::migration::read_plan(&request.plan_path)
                    .and_then(|plan| {
                        desktop_pet_update_coordinator::migration::delegated_approval(&plan)
                    })
                    .unwrap_or(false);
            if desktop_pet_update_coordinator::identity::LEGACY
                && !delegated
                && ui::confirm_migration_consent_visibility(&request.operation_id).is_err()
            {
                return;
            }
            let _writer = lock_unpoisoned(&inner.lifecycle_writes);
            if verify_migration_control(&inner, &request).is_ok() {
                let mut state = lock_unpoisoned(&inner.shared);
                if !desktop_pet_update_coordinator::identity::LEGACY
                    || (state.migration_authorized(&request.operation_id, &request.token)
                        && (delegated
                            || (state.update_ui_visible()
                                && state.update_ui_operation_id()
                                    == Some(request.operation_id.as_str()))))
                {
                    state.request_shutdown();
                    drop(state);
                    ui::wake();
                }
            }
        } else if !written
            && request.command
                == desktop_pet_update_coordinator::migration::MigrationCommand::Prepare
        {
            let mut state = lock_unpoisoned(&inner.shared);
            state.cancel_update_prepare(&request.operation_id, &request.token);
            if state.migration_authorized(&request.operation_id, &request.token) {
                let _ = state.release_migration_gate(&request.operation_id, &request.token);
            }
            drop(state);
            ui::wake();
        }
        return;
    }
    if let Some(extension) = inner
        .extensions
        .iter()
        .find(|extension| Some(extension.kind()) == frame_kind.kind.as_deref())
    {
        if wire_bytes > extension.max_request_bytes() || !verify_peer_uid(&stream) {
            return;
        }
        let preferences =
            crate::preference_access::LivePreferences::new(inner.shared.clone(), deadline);
        match extension.handle_frame(&frame, &preferences) {
            Ok(bytes)
                if bytes.len() <= extension.max_response_bytes()
                    && bytes.last() == Some(&b'\n')
                    && !bytes[..bytes.len() - 1].contains(&b'\n') =>
            {
                let _ = write_frame(&mut stream, &bytes, deadline);
            }
            Ok(_) => {
                let response = ControlResponse::error(
                    extension.kind(),
                    &inner,
                    "extension response exceeds frame bounds",
                );
                let _ = write_response(&mut stream, &response, deadline);
            }
            Err(error) => {
                let response = ControlResponse::error(extension.kind(), &inner, error);
                let _ = write_response(&mut stream, &response, deadline);
            }
        }
        return;
    }
    if frame_kind.kind.as_deref() == Some("updater") {
        let updater_deadline = Instant::now() + Duration::from_secs(15);
        // Lock order: lifecycle_writes -> durable generation gate -> AppState.
        // Neither a state lock nor lifecycle_writes is retained across socket IO.
        let (response, claimed_stop) = if frame.len() > MAX_FRAME_BYTES
            || !verify_peer_uid(&stream)
            || reject_duplicate_json_keys(&frame).is_err()
        {
            (
                update_error("", "", "invalid or unauthorized updater request".into()),
                None,
            )
        } else {
            match serde_json::from_slice::<UpdateControlRequest>(&frame) {
                Ok(request) => {
                    let result = dispatch_update(&inner, &request, updater_deadline);
                    let accepted_generation = result.as_ref().ok().copied();
                    let reply = match result {
                        Ok(_) => update_reply(&request, true, "accepted".into()),
                        Err(error) => update_reply(&request, false, error),
                    };
                    // Prepare also thaws on a failed reply write, but Stop must
                    // additionally defer actual shutdown until the complete write.
                    (
                        reply,
                        accepted_generation.map(|generation| (request, generation)),
                    )
                }
                Err(error) => (
                    update_error("", "", format!("invalid updater request: {error}")),
                    None,
                ),
            }
        };
        let written = serde_json::to_vec(&response)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
            .and_then(|mut bytes| {
                bytes.push(b'\n');
                if bytes.len() > MAX_FRAME_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "updater reply exceeds frame limit",
                    ));
                }
                write_frame(&mut stream, &bytes, updater_deadline)
            })
            .is_ok();
        if let Some((request, generation)) = claimed_stop {
            if !written {
                lock_unpoisoned(&inner.shared)
                    .abort_update_stop(&request.operation_id, &request.token);
                ui::wake();
            } else if let Some(generation) = generation {
                let _lifecycle = lock_unpoisoned(&inner.lifecycle_writes);
                let current = stop_generation(&inner.paths.state_dir);
                let mut state = lock_unpoisoned(&inner.shared);
                if current.is_ok_and(|now| now == generation)
                    && state
                        .commit_update_stop(&request.operation_id, &request.token)
                        .is_ok()
                {
                    drop(state);
                    ui::wake();
                } else {
                    state.abort_update_stop(&request.operation_id, &request.token);
                    drop(state);
                    ui::wake();
                }
            }
        }
        return;
    }
    if frame_kind.kind.as_deref() == Some("presentation") {
        if frame.len() > MAX_FRAME_BYTES {
            return;
        }
        let response = decode_presentation_request(&frame)
            .and_then(|request| dispatch_presentation(&inner, request))
            .unwrap_or_else(presentation_error);
        let _ = write_presentation_response(&mut stream, &response, deadline);
        return;
    }
    if matches!(frame_kind.kind.as_deref(), Some("automation" | "prompt")) {
        if frame_kind.kind.as_deref() != Some("prompt") && frame.len() > MAX_FRAME_BYTES {
            let _ = write_automation_response(
                &mut stream,
                &automation_error("automation request exceeds frame limit".to_owned()),
                deadline,
            );
            return;
        }
        let response = decode_automation_request(&frame)
            .and_then(|request| dispatch_automation(&inner, request))
            .unwrap_or_else(automation_error);
        let _ = write_automation_response(&mut stream, &response, deadline);
        return;
    }
    if frame_kind.kind.as_deref() == Some("sessions") {
        if frame.len() > MAX_FRAME_BYTES {
            let _ = write_sessions_response(
                &mut stream,
                &sessions_error("sessions request exceeds frame limit".to_owned()),
                deadline,
            );
            return;
        }
        let response = decode_sessions_request(&frame)
            .and_then(|request| dispatch_sessions(&inner, request))
            .unwrap_or_else(sessions_error);
        let _ = write_sessions_response(&mut stream, &response, deadline);
        return;
    }
    if frame.len() > MAX_FRAME_BYTES {
        return;
    }
    if reject_duplicate_json_keys(&frame).is_err() {
        let response = ControlResponse::error("unknown", &inner, "invalid control request");
        let _ = write_response(&mut stream, &response, deadline);
        return;
    }
    if frame_kind.kind.as_deref() == Some("pack") {
        let envelope: PackRequestEnvelope = match serde_json::from_slice(&frame) {
            Ok(request) => request,
            Err(error) => {
                let response = pack_error("", format!("invalid pack request: {error}"));
                let _ = write_pack_response(&mut stream, &response, deadline);
                return;
            }
        };
        if envelope.version != PACK_PROTOCOL_VERSION {
            let response = pack_error(
                &envelope.command,
                "unsupported pack protocol version".to_owned(),
            );
            let _ = write_pack_response(&mut stream, &response, deadline);
            return;
        }
        if envelope.kind != "pack" || envelope.command.len() > 64 {
            let response = pack_error(&envelope.command, "pack request is invalid".to_owned());
            let _ = write_pack_response(&mut stream, &response, deadline);
            return;
        }
        let response = dispatch_pack(&inner, envelope);
        let _ = write_pack_response(&mut stream, &response, deadline);
        return;
    }
    if frame_kind.kind.as_deref() == Some("lifecycle") {
        let result = serde_json::from_slice::<LifecycleRequest>(&frame)
            .map_err(|error| format!("invalid lifecycle request: {error}"))
            .and_then(|request| {
                validate_lifecycle_request(&request)?;
                let (profile, extensions, selection) = {
                    let state = lock_unpoisoned(&inner.shared);
                    if inner.stopping.load(Ordering::Acquire) || state.scene().shutdown {
                        return Err("desktop pet is stopping".into());
                    }
                    (
                        state
                            .profile_store()
                            .cloned()
                            .ok_or("daemon profile store unavailable")?,
                        state.lifecycle_extensions().to_vec(),
                        state.observation_mode(),
                    )
                };
                if request.operation == "get" {
                    return lifecycle_values(&profile, &extensions, &selection);
                }
                let key = request.key.ok_or("missing lifecycle key")?;
                let value = request.value.ok_or("missing lifecycle value")?;
                ui::set_registered_lifecycle_on_main(None, key, value, deadline)
            });
        let mut bytes = match serde_json::to_vec(&lifecycle_reply(result)) {
            Ok(bytes) => bytes,
            Err(_) => return,
        };
        bytes.push(b'\n');
        if bytes.len() <= MAX_FRAME_BYTES {
            let _ = write_frame(&mut stream, &bytes, deadline);
        }
        return;
    }
    let request: LegacyRequest = match serde_json::from_slice(&frame) {
        Ok(request) => request,
        Err(_) => {
            let response = ControlResponse::error("unknown", &inner, "invalid control request");
            let _ = write_response(&mut stream, &response, deadline);
            return;
        }
    };
    if request.version != Some(PROTOCOL_VERSION) {
        let response =
            ControlResponse::error("unknown", &inner, "unsupported control protocol version");
        let _ = write_response(&mut stream, &response, deadline);
        return;
    }
    let command = match request.command.as_deref() {
        Some(command) if command.len() <= 64 => command,
        _ => {
            let response = ControlResponse::error("unknown", &inner, "control command is invalid");
            let _ = write_response(&mut stream, &response, deadline);
            return;
        }
    };
    let endpoint = request.endpoint.map(PathBuf::from);
    let response = dispatch(&inner, command, endpoint);
    let _ = write_response(&mut stream, &response, deadline);
}

fn update_error(instance_id: &str, operation_id: &str, detail: String) -> UpdateControlReply {
    UpdateControlReply {
        version: UPDATER_PROTOCOL,
        instance_id: instance_id.to_owned(),
        operation_id: operation_id.to_owned(),
        accepted: false,
        detail,
    }
}

fn update_reply(
    request: &UpdateControlRequest,
    accepted: bool,
    detail: String,
) -> UpdateControlReply {
    UpdateControlReply {
        version: UPDATER_PROTOCOL,
        instance_id: request.instance_id.clone(),
        operation_id: request.operation_id.clone(),
        accepted,
        detail,
    }
}
fn verify_running_plan(plan: &UpdatePlan, context: &UpdateContext) -> Result<(), String> {
    if plan.version != UPDATER_PROTOCOL {
        return Err("unsupported updater plan protocol".into());
    }
    if plan.context != *context
        || plan.origin
            != *context
                .running_origin
                .as_deref()
                .ok_or("missing startup origin")?
        || plan.running != context.running
    {
        return Err("updater plan does not match running executable, origin and profile".into());
    }
    validate_plan_target(plan)?;
    if let Some(target) = &plan.target {
        let selected = if Preferences::load(&context.config_dir)?.receive_beta_updates() {
            UpdatePolicy::StableAndBeta
        } else {
            UpdatePolicy::StableOnly
        };
        if target.policy != selected {
            return Err("selected update policy changed since consent".into());
        }
    }
    Ok(())
}

fn dispatch_update(
    inner: &ControlInner,
    request: &UpdateControlRequest,
    deadline: Instant,
) -> Result<Option<u64>, String> {
    if request.version != UPDATER_PROTOCOL
        || request.kind != "updater"
        || inner.stopping.load(Ordering::Acquire)
    {
        return Err("unsupported updater protocol or daemon stopping".into());
    }
    let context = {
        let state = lock_unpoisoned(&inner.shared);
        let context = state
            .update_context()
            .ok_or("updater context unavailable")?;
        if request.instance_id != context.instance_id
            || !state.is_ui_ready()
            || state.scene().shutdown
        {
            return Err("updater daemon instance is not ready or changed".into());
        }
        context.clone()
    };
    let origin = context
        .running_origin
        .as_deref()
        .ok_or("running installation origin unavailable")?;
    if matches!(origin, InstallOrigin::Unknown { .. }) {
        return Err("unknown installation cannot be reserved for update".into());
    }
    let plan = read_plan(&context.state_dir, &request.operation_id)?;
    verify_running_plan(&plan, &context)?;
    verify_control_for(
        origin,
        plan.target.as_ref().map(|target| &target.origin),
        &context.state_dir,
        &request.operation_id,
        &request.token,
    )?;
    match request.command {
        UpdateCommand::Prepare => {
            let (sender, receiver) = mpsc::channel();
            {
                let _lifecycle = lock_unpoisoned(&inner.lifecycle_writes);
                let mut state = lock_unpoisoned(&inner.shared);
                if !state.is_ui_ready() || state.scene().shutdown {
                    return Err("desktop pet stopped during updater preparation".into());
                }
                state.queue_update_prepare(
                    request.operation_id.clone(),
                    request.token.clone(),
                    sender,
                )?;
            }
            ui::wake();
            let remaining = deadline.saturating_duration_since(Instant::now());
            match receiver.recv_timeout(remaining) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(error),
                Err(_) => {
                    lock_unpoisoned(&inner.shared)
                        .cancel_update_prepare(&request.operation_id, &request.token);
                    ui::wake();
                    return Err("main-thread update preparation timed out".into());
                }
            }
            if let Err(error) = verify_control_for(
                origin,
                plan.target.as_ref().map(|target| &target.origin),
                &context.state_dir,
                &request.operation_id,
                &request.token,
            ) {
                lock_unpoisoned(&inner.shared)
                    .cancel_update_prepare(&request.operation_id, &request.token);
                ui::wake();
                return Err(error);
            }
            Ok(None)
        }
        UpdateCommand::Stop => {
            let _lifecycle = lock_unpoisoned(&inner.lifecycle_writes);
            // Verify and read the durable generation before taking AppState;
            // ordinary control Stop writes it under the same lifecycle lock.
            verify_control_for(
                origin,
                plan.target.as_ref().map(|target| &target.origin),
                &context.state_dir,
                &request.operation_id,
                &request.token,
            )?;
            let generation = stop_generation(&context.state_dir)?;
            lock_unpoisoned(&inner.shared)
                .claim_update_stop(&request.operation_id, &request.token)?;
            Ok(Some(generation))
        }
    }
}

fn presentation_admission(
    state: &AppState,
    stopping: bool,
    mutation: bool,
) -> Result<SharedAutomation, String> {
    if !state.is_ui_ready() {
        return Err("desktop pet UI is not ready".to_owned());
    }
    if mutation {
        state.update_mutation_allowed()?;
    }
    let handle = state
        .automation()
        .ok_or("presentation automation is unavailable")?;
    if mutation && (stopping || state.scene().shutdown) {
        return Err("desktop pet is stopping".to_owned());
    }
    Ok(handle)
}

fn dispatch_presentation(
    inner: &ControlInner,
    request: PresentationRequestEnvelope,
) -> Result<PresentationReplyEnvelope, String> {
    let mutation = matches!(request.command.as_str(), "set" | "reset");
    if mutation {
        mutation_admission(inner)?;
    }
    let state = lock_unpoisoned(&inner.shared);
    let handle = presentation_admission(&state, inner.stopping.load(Ordering::Acquire), mutation)?;
    let mut ledger = lock_automation(&handle);
    let reply = presentation_ledger_reply(&mut ledger, request)?;
    drop(ledger);
    drop(state);
    if mutation {
        ui::wake();
    }
    Ok(reply)
}
fn presentation_ledger_reply(
    ledger: &mut AutomationState,
    request: PresentationRequestEnvelope,
) -> Result<PresentationReplyEnvelope, String> {
    match request.command.as_str() {
        "get" => {
            let snapshot = ledger.snapshot();
            Ok(presentation_success(
                snapshot.instance_id.clone(),
                Some(snapshot),
                None,
            ))
        }
        "status" => {
            let operation = ledger.status(
                request
                    .instance_id
                    .as_deref()
                    .expect("validated instance ID"),
                request
                    .operation_id
                    .as_deref()
                    .expect("validated operation ID"),
            )?;
            Ok(presentation_success(
                operation.instance_id.clone(),
                None,
                Some(operation),
            ))
        }
        "set" | "reset" => {
            let action = if request.command == "set" {
                PresentationAction::Set {
                    patch: request.patch.expect("validated patch"),
                }
            } else {
                PresentationAction::ResetPosition
            };
            let operation = ledger.submit(PresentationRequest {
                instance_id: request.instance_id.expect("validated instance ID"),
                operation_id: request.operation_id.expect("validated operation ID"),
                expected_revision: request.expected_revision,
                action,
            })?;
            Ok(presentation_success(
                operation.instance_id.clone(),
                None,
                Some(operation),
            ))
        }
        _ => Err("invalid presentation command".to_owned()),
    }
}

fn automation_error(error: String) -> AutomationReplyEnvelope {
    AutomationReplyEnvelope {
        version: PROTOCOL_VERSION,
        kind: "automation".to_owned(),
        ok: false,
        error: Some(error),
        instance_id: None,
        operation: None,
    }
}

fn dispatch_automation(
    inner: &ControlInner,
    envelope: AutomationRequestEnvelope,
) -> Result<AutomationReplyEnvelope, String> {
    let mutation = envelope.command == "request";
    if mutation {
        mutation_admission(inner)?;
    }
    let state = lock_unpoisoned(&inner.shared);
    let handle = presentation_admission(&state, inner.stopping.load(Ordering::Acquire), mutation)?;
    let mut ledger = lock_automation(&handle);
    let operation = if let Some(request) = envelope.request {
        ledger.submit_domain(request)?
    } else {
        ledger.domain_status(
            envelope
                .instance_id
                .as_deref()
                .expect("validated instance ID"),
            envelope
                .operation_id
                .as_deref()
                .expect("validated operation ID"),
        )?
    };
    drop(ledger);
    drop(state);
    if mutation {
        ui::wake();
    }
    Ok(AutomationReplyEnvelope {
        version: PROTOCOL_VERSION,
        kind: "automation".to_owned(),
        ok: true,
        error: None,
        instance_id: Some(operation.instance_id.clone()),
        operation: Some(operation),
    })
}

fn sessions_error(error: String) -> SessionsReplyEnvelope {
    SessionsReplyEnvelope {
        version: PROTOCOL_VERSION,
        kind: "sessions".to_owned(),
        ok: false,
        error: Some(error),
        result: None,
    }
}

fn dispatch_sessions(
    inner: &ControlInner,
    envelope: SessionsRequestEnvelope,
) -> Result<SessionsReplyEnvelope, String> {
    let result = {
        let state = lock_unpoisoned(&inner.shared);
        if !state.is_ui_ready() {
            return Err("desktop pet UI is not ready".to_owned());
        }
        match envelope.command.as_str() {
            "list" => {
                state.automation_session_page(envelope.page.as_ref().expect("validated page"))?
            }
            "detail" => state.automation_session_detail(
                envelope.identity.as_ref().expect("validated identity"),
            )?,
            _ => return Err("invalid sessions command".to_owned()),
        }
    };
    let reply = SessionsReplyEnvelope {
        version: PROTOCOL_VERSION,
        kind: "sessions".to_owned(),
        ok: true,
        error: None,
        result: Some(result),
    };
    if envelope.command == "list" {
        Ok(budget_sessions_list_response(reply))
    } else {
        Ok(reply)
    }
}

#[derive(Serialize)]
struct SessionCursorRef<'a> {
    source_id: u64,
    terminal_id: &'a str,
}

#[derive(Serialize)]
struct SessionPageCursorRef<'a> {
    instance_id: &'a str,
    revision: u64,
    filter: SessionFilter,
    position: SessionCursorRef<'a>,
}

#[derive(Default)]
struct JsonByteCount(usize);

impl Write for JsonByteCount {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.checked_add(bytes.len()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "session JSON length overflow")
        })?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn json_byte_count<T: Serialize>(value: &T) -> io::Result<usize> {
    let mut counter = JsonByteCount::default();
    serde_json::to_writer(&mut counter, value).map_err(io::Error::other)?;
    Ok(counter.0)
}

fn budget_sessions_list_response(mut reply: SessionsReplyEnvelope) -> SessionsReplyEnvelope {
    fn budget(reply: &mut SessionsReplyEnvelope) -> io::Result<Option<&'static str>> {
        let page = reply
            .result
            .as_mut()
            .and_then(Value::as_object_mut)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing sessions page"))?;
        let original_has_more = !page.get("next_cursor").is_some_and(Value::is_null);
        let mut rows = std::mem::take(
            page.get_mut("rows")
                .and_then(Value::as_array_mut)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "missing sessions rows")
                })?,
        );
        page.insert("next_cursor".to_owned(), Value::Null);
        let base = json_byte_count(reply)?;
        let Some(base_frame) = base.checked_add(1) else {
            return Ok(Some("session page envelope exceeds 512KiB frame limit"));
        };
        if base_frame > MAX_SESSION_FRAME_BYTES {
            return Ok(Some("session page envelope exceeds 512KiB frame limit"));
        }
        let page = reply
            .result
            .as_ref()
            .and_then(Value::as_object)
            .expect("page");
        let instance_id = page
            .get("instance_id")
            .and_then(Value::as_str)
            .expect("instance");
        let revision = page
            .get("revision")
            .and_then(Value::as_u64)
            .expect("revision");
        let filter = match page.get("filter").and_then(Value::as_str) {
            Some("all") => SessionFilter::All,
            Some("idle") => SessionFilter::Idle,
            Some("working") => SessionFilter::Working,
            Some("waiting") => SessionFilter::Waiting,
            Some("completed") => SessionFilter::Completed,
            Some("unknown") => SessionFilter::Unknown,
            Some("offline") => SessionFilter::Offline,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid session filter",
                ))
            }
        };
        let mut selected = 0;
        let mut used_rows = 0usize;
        for row in &rows {
            let row_bytes = json_byte_count(row)?;
            let Some(next_rows) = used_rows
                .checked_add(usize::from(selected > 0))
                .and_then(|n| n.checked_add(row_bytes))
            else {
                break;
            };
            let needs_cursor = selected + 1 < rows.len() || original_has_more;
            let cursor_bytes = if needs_cursor {
                let key = row.get("key").expect("session key");
                let cursor = SessionPageCursorRef {
                    instance_id,
                    revision,
                    filter,
                    position: SessionCursorRef {
                        source_id: key
                            .get("source_id")
                            .and_then(Value::as_u64)
                            .expect("source"),
                        terminal_id: key
                            .get("terminal_id")
                            .and_then(Value::as_str)
                            .expect("terminal"),
                    },
                };
                json_byte_count(&cursor)?
            } else {
                4 // The base envelope already includes `null`.
            };
            let frame_bytes = base_frame
                .checked_add(next_rows)
                .and_then(|n| n.checked_sub(4))
                .and_then(|n| n.checked_add(cursor_bytes));
            if !frame_bytes.is_some_and(|size| size <= MAX_SESSION_FRAME_BYTES) {
                break;
            }
            selected += 1;
            used_rows = next_rows;
        }
        if selected == 0 && !rows.is_empty() {
            return Ok(Some("session row exceeds 512KiB frame limit"));
        }
        let cursor = if selected > 0 && (selected < rows.len() || original_has_more) {
            let key = rows[selected - 1].get("key").expect("session key");
            Some(SessionPageCursor {
                instance_id: instance_id.to_owned(),
                revision,
                filter,
                position: SessionCursor {
                    source_id: key
                        .get("source_id")
                        .and_then(Value::as_u64)
                        .expect("source"),
                    terminal_id: key
                        .get("terminal_id")
                        .and_then(Value::as_str)
                        .expect("terminal")
                        .to_owned(),
                },
            })
        } else {
            None
        };
        rows.truncate(selected);
        let page = reply
            .result
            .as_mut()
            .and_then(Value::as_object_mut)
            .expect("page");
        page.insert("rows".to_owned(), Value::Array(rows));
        page.insert(
            "next_cursor".to_owned(),
            serde_json::to_value(cursor).map_err(io::Error::other)?,
        );
        Ok(None)
    }
    match budget(&mut reply) {
        Ok(None) => reply,
        Ok(Some(message)) => sessions_error(message.to_owned()),
        Err(error) => sessions_error(format!("cannot budget session page: {error}")),
    }
}

fn pack_error(command: &str, error: String) -> PackReplyEnvelope {
    PackReplyEnvelope {
        version: PACK_PROTOCOL_VERSION,
        kind: "pack".to_owned(),
        command: command.to_owned(),
        ok: false,
        listing: None,
        operation: None,
        error: Some(error),
    }
}

fn validate_source_path(path: &Path) -> Result<(), String> {
    let bytes = path.as_os_str().as_bytes();
    if bytes.contains(&0) {
        return Err("pack source path must not contain NUL bytes".to_owned());
    }
    if bytes.is_empty() || !path.is_absolute() {
        return Err("pack source path must be a non-empty absolute path".to_owned());
    }
    if bytes.len() > MAX_SOURCE_PATH_BYTES {
        return Err(format!(
            "pack source path must be at most {MAX_SOURCE_PATH_BYTES} bytes"
        ));
    }
    Ok(())
}

pub(crate) fn validate_pack_request(request: &PackRequest) -> Result<(), String> {
    match &request.action {
        PackAction::Import { path } | PackAction::Update { path, .. } => validate_source_path(path),
        PackAction::ImportAndSelect { official } => {
            if request.expected_generation.is_none() {
                return Err("official import requires expected_generation".to_owned());
            }
            crate::character_types::validate_pack_id(&official.id)?;
            if official.version.is_empty()
                || official.version.len() > 64
                || !official
                    .version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-_+".contains(&b))
            {
                return Err("official version is invalid".to_owned());
            }
            if official.release_tag.is_empty()
                || official.release_tag.len() > 128
                || !official
                    .release_tag
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-_+".contains(&b))
            {
                return Err("official release tag is invalid".to_owned());
            }
            if official.sha256.len() != 64
                || !official.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("official sha256 is invalid".to_owned());
            }
            Ok(())
        }
        PackAction::Select { .. } | PackAction::Restore { .. } | PackAction::Remove { .. } => {
            Ok(())
        }
    }
}

fn mutation_admission(inner: &ControlInner) -> Result<(), String> {
    shared_mutation_admission(&inner.shared)
}

pub(crate) fn shared_mutation_admission(shared: &Arc<Mutex<AppState>>) -> Result<(), String> {
    let scope = {
        let state = lock_unpoisoned(shared);
        state.update_mutation_allowed()?;
        state.update_context().map(|context| {
            (
                context
                    .running_origin
                    .as_deref()
                    .cloned()
                    .unwrap_or(InstallOrigin::Unknown {
                        reason: "running origin unavailable".into(),
                    }),
                context.state_dir.clone(),
            )
        })
    };
    if let Some((origin, state_dir)) = scope {
        start_allowed(&origin, &state_dir, None, None)?;
    }
    lock_unpoisoned(shared).update_mutation_allowed()
}

fn dispatch_pack(inner: &ControlInner, request: PackRequestEnvelope) -> PackReplyEnvelope {
    if inner.stopping.load(Ordering::Acquire) {
        return pack_error(&request.command, "desktop pet is stopping".to_owned());
    }
    match request.command.as_str() {
        "list"
            if request.request.is_none()
                && request.operation_id.is_none()
                && (request.offset.is_some() || request.limit.is_some()) =>
        {
            let offset = request.offset.unwrap_or(0);
            let limit = request.limit.unwrap_or(1);
            if limit == 0 || limit > PACK_LIST_PAGE_SIZE {
                return pack_error("list", "pack list page size is invalid".to_owned());
            }
            match inner.packs.list() {
                Ok(mut listing) => {
                    if offset > listing.packs.len() {
                        return pack_error("list", "pack list offset is out of range".to_owned());
                    }
                    listing.packs = listing.packs.into_iter().skip(offset).take(limit).collect();
                    PackReplyEnvelope {
                        version: PACK_PROTOCOL_VERSION,
                        kind: "pack".to_owned(),
                        command: "list".to_owned(),
                        ok: true,
                        listing: Some(listing),
                        operation: None,
                        error: None,
                    }
                }
                Err(error) => pack_error("list", error),
            }
        }
        "list" if request.request.is_none() && request.operation_id.is_none() => {
            pack_error("list", "pack list pagination is required".to_owned())
        }
        "submit"
            if request.request.is_some()
                && request.operation_id.is_none()
                && request.offset.is_none()
                && request.limit.is_none() =>
        {
            let request = request.request.expect("checked above");
            if let Err(error) = validate_pack_request(&request) {
                return pack_error("submit", error);
            }
            if let Err(error) = mutation_admission(inner) {
                return pack_error("submit", error);
            }
            let state = lock_unpoisoned(&inner.shared);
            if let Err(error) = state.update_mutation_allowed() {
                return pack_error("submit", error);
            }
            let submission = inner.packs.submit(request);
            drop(state);
            match submission {
                Ok(operation) => PackReplyEnvelope {
                    version: PACK_PROTOCOL_VERSION,
                    kind: "pack".to_owned(),
                    command: "submit".to_owned(),
                    ok: true,
                    listing: None,
                    operation: Some(operation),
                    error: None,
                },
                Err(error) => pack_error("submit", error),
            }
        }
        "status"
            if request.request.is_none()
                && request.operation_id.is_some()
                && request.offset.is_none()
                && request.limit.is_none() =>
        {
            let operation_id = request.operation_id.expect("checked above");
            match inner.packs.status(&operation_id) {
                Ok(operation) => PackReplyEnvelope {
                    version: PACK_PROTOCOL_VERSION,
                    kind: "pack".to_owned(),
                    command: "status".to_owned(),
                    ok: true,
                    listing: None,
                    operation: Some(operation),
                    error: None,
                },
                Err(error) => pack_error("status", error),
            }
        }
        _ => pack_error(&request.command, "invalid pack request fields".to_owned()),
    }
}

fn pack_response_json(response: &PackReplyEnvelope) -> io::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(response)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if bytes.len() + 1 > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack response exceeds frame limit",
        ));
    }
    bytes.push(b'\n');
    Ok(bytes)
}

fn settings_admission(state: &AppState, stopping: bool) -> Result<(), String> {
    if stopping || state.scene().shutdown {
        return Err("desktop pet is stopping".to_owned());
    }
    if !state.is_ui_ready() {
        return Err("desktop pet UI is not ready".to_owned());
    }
    state.update_mutation_allowed()
}

fn dispatch(inner: &ControlInner, command: &str, endpoint: Option<PathBuf>) -> ControlResponse {
    if endpoint.is_some() {
        return ControlResponse::error(
            command,
            inner,
            "endpoint routing belongs to a registered integration",
        );
    }
    if inner.stopping.load(Ordering::Acquire) && command != "status" {
        return ControlResponse::error(command, inner, "desktop pet is stopping");
    }
    if !matches!(
        command,
        "status" | "ping" | "ready" | "stop" | "shutdown" | "ensure"
    ) {
        if let Err(error) = mutation_admission(inner) {
            return ControlResponse::error(command, inner, error);
        }
    }
    match command {
        "ready" | "ensure" => {
            let mut response = ControlResponse::status(command, inner);
            if response.shutdown && !response.ui_ready {
                response.ok = false;
                response.error = Some("desktop pet stopped before UI became ready".to_owned());
            }
            response
        }
        "status" | "ping" => ControlResponse::status(command, inner),
        "settings" => {
            let state = lock_unpoisoned(&inner.shared);
            if let Err(error) = settings_admission(&state, inner.stopping.load(Ordering::Acquire)) {
                drop(state);
                return ControlResponse::error(command, inner, error);
            }
            drop(state);
            ui::request_settings();
            ControlResponse::status(command, inner)
        }
        "stop" | "shutdown" => {
            let _lifecycle = lock_unpoisoned(&inner.lifecycle_writes);
            if let Err(error) = record_user_stop(&inner.paths.state_dir) {
                return ControlResponse::error(command, inner, error);
            }
            lock_unpoisoned(&inner.shared).request_shutdown();
            ui::wake();
            ControlResponse::status(command, inner)
        }
        "show"
        | "hide"
        | "toggle"
        | "passthrough"
        | "toggle_passthrough"
        | "alpha_passthrough"
        | "toggle_alpha_passthrough"
        | "show_bubble"
        | "hide_bubble"
        | "bubble_above"
        | "bubble_below"
        | "bubble_left"
        | "bubble_right"
        | "bubble_auto"
        | "reset"
        | "reset_position"
        | "bigger"
        | "smaller"
        | "scale_up"
        | "scale_down" => {
            let action = match command {
                "passthrough" => "toggle_passthrough",
                "reset" => "reset_position",
                "bigger" => "scale_up",
                "smaller" => "scale_down",
                other => other,
            };
            let result = {
                let mut state = lock_unpoisoned(&inner.shared);
                state.apply_control(action)
            };
            match result {
                Ok(()) => {
                    ui::wake();
                    ControlResponse::status(command, inner)
                }
                Err(error) => ControlResponse::error(command, inner, error),
            }
        }
        _ => ControlResponse::error(
            command,
            inner,
            format!("unsupported control command: {command}"),
        ),
    }
}

/// Offline counterpart of the common preference transaction. The client must
/// establish socket absence before calling; recheck under the actual state
/// lease and retain config-wide ownership through the sole atomic save.
pub(crate) fn access_profile_preferences_offline(
    paths: &Paths,
    extensions: &[Arc<dyn desktop_pet_contracts::LifecycleExtension>],
    change: Option<Map<String, Value>>,
    deadline: Instant,
) -> Result<Option<Map<String, Value>>, String> {
    if Instant::now() >= deadline {
        return Err("preference access deadline elapsed".into());
    }
    let Some(lease) = LifecycleLock::try_acquire(&paths.state_dir)? else {
        return Ok(None);
    };
    if !crate::preference_access::profile_owner_absent_until(paths, deadline)?
        || lifecycle::startup_pending(&paths.config_dir)?
    {
        return Ok(None);
    }
    if change.is_some() {
        start_allowed(
            &InstallOrigin::Unknown {
                reason: "offline preference transaction".into(),
            },
            &paths.state_dir,
            None,
            None,
        )?;
    }
    let profile =
        desktop_pet_contracts::ProfileStore::with_lock(&paths.config_dir, Arc::new(lease))?
            .with_preferences_validator(Preferences::validate_committed_object);
    profile.verify_owner()?;
    let mut preferences =
        profile.with_writer_lock(|| Preferences::load_committed(profile.config_dir()))?;
    preferences.set_integration_extensions(profile.clone(), extensions.to_vec());
    preferences.refresh_owner_snapshot(profile.config_dir())?;
    if let Some(patch) = change {
        if Instant::now() >= deadline {
            return Err("preference access deadline elapsed before save".into());
        }
        profile.verify_owner()?;
        preferences.apply_patch(
            serde_json::from_value(Value::Object(patch))
                .map_err(|error| format!("invalid preference patch: {error}"))?,
            profile.config_dir(),
        )?;
    }
    profile.verify_owner()?;
    match serde_json::to_value(preferences.snapshot()).map_err(|error| error.to_string())? {
        Value::Object(saved) => Ok(Some(saved)),
        _ => Err("common preferences snapshot is not an object".into()),
    }
}

pub(crate) fn lifecycle_values(
    profile: &desktop_pet_contracts::ProfileStore,
    extensions: &[Arc<dyn desktop_pet_contracts::LifecycleExtension>],
    selection: &desktop_pet_contracts::ObservationSelection,
) -> Result<Map<String, Value>, String> {
    let settings = lifecycle::read_settings(profile.config_dir())?;
    let mut values = Map::new();
    values.insert("auto_start".into(), Value::Bool(settings.auto_start));
    for extension in extensions {
        for setting in extension.settings(profile, selection, "en")? {
            if values
                .insert(setting.key, Value::Bool(setting.value))
                .is_some()
            {
                return Err("duplicate registered lifecycle setting".into());
            }
        }
    }
    Ok(values)
}

pub(crate) fn prepare_registered_lifecycle(
    profile: &desktop_pet_contracts::ProfileStore,
    extensions: &[Arc<dyn desktop_pet_contracts::LifecycleExtension>],
    selection: &desktop_pet_contracts::ObservationSelection,
    provider: Option<&str>,
    key: &str,
    value: bool,
) -> Result<Option<Box<dyn desktop_pet_contracts::PreparedPreferencePatch>>, String> {
    let mut owner = None;
    let mut keys = std::collections::BTreeSet::from(["auto_start".to_owned()]);
    for extension in extensions {
        for setting in extension.settings(profile, selection, "en")? {
            if !keys.insert(setting.key.clone()) {
                return Err("duplicate registered lifecycle setting".into());
            }
            if setting.key == key {
                owner = Some(extension);
            }
        }
    }
    if key == "auto_start" {
        if provider.is_some() {
            return Err("auto_start is owned by core".into());
        }
        lifecycle::update_setting(profile, extensions, LifecycleSetting::AutoStart, value)?;
        return Ok(None);
    }
    let owner = owner.ok_or_else(|| format!("unknown lifecycle setting: {key}"))?;
    if provider.is_some_and(|provider| provider != owner.provider()) {
        return Err("lifecycle setting belongs to another provider".into());
    }
    if let Some(prepared) = owner.prepare_preference_setting(profile, key, value)? {
        return Ok(Some(prepared));
    }
    owner.apply_setting(profile, key, value)?;
    Ok(None)
}

pub(crate) fn access_registered_lifecycle(
    paths: &Paths,
    change: Option<(&str, bool)>,
    composition: &crate::Composition,
) -> Result<Map<String, Value>, String> {
    if control_socket_is_live(&paths.control_socket, CLIENT_TIMEOUT) {
        let request = LifecycleRequest {
            version: PROTOCOL_VERSION,
            kind: "lifecycle".into(),
            operation: if change.is_some() { "set" } else { "get" }.into(),
            key: change.map(|(key, _)| key.into()),
            value: change.map(|(_, value)| value),
        };
        let frame = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
        let reply = send_raw_frame(
            &paths.control_socket,
            &frame,
            CLIENT_TIMEOUT,
            MAX_FRAME_BYTES,
        )?;
        let reply: LifecycleReply = serde_json::from_slice(&reply).map_err(|e| e.to_string())?;
        return match (reply.ok, reply.settings, reply.error) {
            (true, Some(settings), None)
                if reply.version == PROTOCOL_VERSION && reply.kind == "lifecycle" =>
            {
                Ok(settings)
            }
            (false, None, Some(error)) => Err(error),
            _ => Err("invalid registered lifecycle reply".into()),
        };
    }
    let guard = Arc::new(crate::cli::acquire_lock_until(
        paths,
        Instant::now() + crate::cli::STARTUP_TIMEOUT,
    )?);
    if control_socket_is_live(&paths.control_socket, Duration::from_millis(150))
        || lifecycle::startup_pending(&paths.config_dir)?
    {
        return Err("profile became active while accessing lifecycle settings".into());
    }
    let profile = desktop_pet_contracts::ProfileStore::with_lock(&paths.config_dir, guard)?
        .with_preferences_validator(Preferences::validate_committed_object);
    if change.is_some() {
        start_allowed(
            &InstallOrigin::Unknown {
                reason: "offline lifecycle settings".into(),
            },
            &paths.state_dir,
            None,
            None,
        )?;
    }
    composition.configure_profile(profile.clone())?;
    let mut preferences = Preferences::load_committed(&paths.config_dir)?;
    preferences
        .set_integration_extensions(profile.clone(), composition.extensions.lifecycle.clone());
    preferences.refresh_owner_snapshot(&paths.config_dir)?;
    let selection = preferences.observation_host();
    if let Some((key, value)) = change {
        if let Some(prepared) = prepare_registered_lifecycle(
            &profile,
            &composition.extensions.lifecycle,
            &selection,
            None,
            key,
            value,
        )? {
            preferences.apply_prepared_owner_patch(prepared, &paths.config_dir)?;
            if let Some(error) = preferences.postcommit_error() {
                return Err(format!(
                    "preference saved but adapter application failed: {error}"
                ));
            }
        }
    }
    lifecycle_values(&profile, &composition.extensions.lifecycle, &selection)
}

struct BoundedJson {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "control frame exceeds limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn bounded_json<T: Serialize>(value: &T, limit: usize) -> io::Result<Vec<u8>> {
    let mut writer = BoundedJson {
        bytes: Vec::new(),
        limit: limit - 1,
    };
    serde_json::to_writer(&mut writer, value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    writer.bytes.push(b'\n');
    Ok(writer.bytes)
}

#[derive(Debug)]
pub(crate) enum ClientDeadlineError {
    Elapsed,
    Other(String),
}

impl std::fmt::Display for ClientDeadlineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Elapsed => formatter.write_str("control request deadline elapsed"),
            Self::Other(error) => formatter.write_str(error),
        }
    }
}

fn client_deadline(deadline: Instant) -> Result<(), ClientDeadlineError> {
    if Instant::now() >= deadline {
        Err(ClientDeadlineError::Elapsed)
    } else {
        Ok(())
    }
}

fn client_io_error(context: &str, error: io::Error, deadline: Instant) -> ClientDeadlineError {
    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) && Instant::now() >= deadline
    {
        ClientDeadlineError::Elapsed
    } else {
        ClientDeadlineError::Other(format!("{context}: {error}"))
    }
}

pub(crate) fn send_automation_request(
    path: &Path,
    request: &AutomationRequestEnvelope,
) -> Result<AutomationReplyEnvelope, String> {
    send_automation_request_until(path, request, Instant::now() + CLIENT_TIMEOUT)
        .map_err(|error| error.to_string())
}

pub(crate) fn send_automation_request_until(
    path: &Path,
    request: &AutomationRequestEnvelope,
    deadline: Instant,
) -> Result<AutomationReplyEnvelope, ClientDeadlineError> {
    client_deadline(deadline)?;
    validate_automation_request(request).map_err(ClientDeadlineError::Other)?;
    let limit = if request.kind == "prompt" {
        MAX_PROMPT_FRAME_BYTES
    } else {
        MAX_FRAME_BYTES
    };
    client_deadline(deadline)?;
    let bytes = bounded_json(request, limit).map_err(|error| {
        ClientDeadlineError::Other(format!("automation request exceeds frame limit: {error}"))
    })?;
    client_deadline(deadline)?;
    let mut stream = connect_private_socket_io(path, deadline)
        .map_err(|error| client_io_error("cannot connect to control socket", error, deadline))?;
    client_deadline(deadline)?;
    write_frame(&mut stream, &bytes, deadline)
        .map_err(|error| client_io_error("cannot send automation request", error, deadline))?;
    let frame = read_client_frame(&mut stream, deadline, MAX_AUTOMATION_REPLY_BYTES)
        .map_err(|error| client_io_error("cannot read automation response", error, deadline))?;
    client_deadline(deadline)?;
    reject_duplicate_json_keys(&frame).map_err(|error| {
        ClientDeadlineError::Other(format!("invalid automation response: {error}"))
    })?;
    let reply: AutomationReplyEnvelope = serde_json::from_slice(&frame).map_err(|error| {
        ClientDeadlineError::Other(format!("invalid automation response: {error}"))
    })?;
    validate_automation_reply(&reply, request).map_err(ClientDeadlineError::Other)?;
    client_deadline(deadline)?;
    Ok(reply)
}

pub(crate) fn validate_automation_reply(
    reply: &AutomationReplyEnvelope,
    request: &AutomationRequestEnvelope,
) -> Result<(), String> {
    let identity = request
        .request
        .as_ref()
        .map(|domain| (domain.instance_id.as_str(), domain.operation_id.as_str()))
        .or_else(|| {
            request
                .instance_id
                .as_deref()
                .zip(request.operation_id.as_deref())
        });
    let valid = reply.version == PROTOCOL_VERSION
        && reply.kind == "automation"
        && if reply.ok {
            reply.error.is_none()
                && reply.operation.as_ref().is_some_and(|operation| {
                    reply.instance_id.as_deref() == Some(operation.instance_id.as_str())
                        && identity
                            == Some((
                                operation.instance_id.as_str(),
                                operation.operation_id.as_str(),
                            ))
                })
        } else {
            reply.error.as_ref().is_some_and(|error| !error.is_empty())
                && reply.instance_id.is_none()
                && reply.operation.is_none()
        };
    if valid {
        Ok(())
    } else {
        Err("invalid automation response fields".to_owned())
    }
}

pub(crate) fn send_sessions_request(
    path: &Path,
    request: &SessionsRequestEnvelope,
) -> Result<SessionsReplyEnvelope, String> {
    validate_sessions_request(request)?;
    let bytes = bounded_json(request, MAX_FRAME_BYTES)
        .map_err(|error| format!("sessions request exceeds frame limit: {error}"))?;
    let deadline = Instant::now() + CLIENT_TIMEOUT;
    let mut stream = connect_private_socket(path, deadline)?;
    write_frame(&mut stream, &bytes, deadline)
        .map_err(|error| format!("cannot send sessions request: {error}"))?;
    let frame = read_client_frame(&mut stream, deadline, MAX_SESSION_FRAME_BYTES)
        .map_err(|error| format!("cannot read sessions response: {error}"))?;
    reject_duplicate_json_keys(&frame)
        .map_err(|error| format!("invalid sessions response: {error}"))?;
    let reply: SessionsReplyEnvelope = serde_json::from_slice(&frame)
        .map_err(|error| format!("invalid sessions response: {error}"))?;
    validate_sessions_reply(&reply, request)?;
    Ok(reply)
}

fn validate_sessions_reply(
    reply: &SessionsReplyEnvelope,
    request: &SessionsRequestEnvelope,
) -> Result<(), String> {
    let expected = request
        .page
        .as_ref()
        .map(|page| page.instance_id.as_str())
        .or_else(|| {
            request
                .identity
                .as_ref()
                .map(|identity| identity.instance_id.as_str())
        });
    let actual = reply
        .result
        .as_ref()
        .and_then(|value| {
            if request.command == "list" {
                value.get("instance_id")
            } else {
                value.get("key").and_then(|key| key.get("instance_id"))
            }
        })
        .and_then(Value::as_str);
    let valid = reply.version == PROTOCOL_VERSION
        && reply.kind == "sessions"
        && if reply.ok {
            reply.error.is_none() && expected == actual
        } else {
            reply.error.as_ref().is_some_and(|error| !error.is_empty()) && reply.result.is_none()
        };
    if valid {
        Ok(())
    } else {
        Err("invalid sessions response fields".to_owned())
    }
}

pub fn send_command(
    socket_path: &Path,
    command: &str,
    endpoint: Option<&Path>,
    timeout: Duration,
) -> Result<ControlResponse, String> {
    if socket_path.as_os_str().is_empty() {
        return Err("control socket path is empty".to_owned());
    }
    let deadline = Instant::now() + timeout;
    let mut stream = connect_private_socket(socket_path, deadline)?;
    let mut request = Map::new();
    request.insert("version".to_owned(), Value::from(PROTOCOL_VERSION));
    request.insert("command".to_owned(), Value::String(command.to_owned()));
    if let Some(endpoint) = endpoint {
        request.insert(
            "endpoint".to_owned(),
            Value::String(endpoint.display().to_string()),
        );
    }
    let mut bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    if bytes.len() + 1 > MAX_FRAME_BYTES {
        return Err("control request exceeds frame limit".to_owned());
    }
    bytes.push(b'\n');
    write_frame(&mut stream, &bytes, deadline)
        .map_err(|error| format!("cannot send control request: {error}"))?;
    let mut response = [0u8; MAX_FRAME_BYTES];

    let length = read_frame(&mut stream, &mut response, deadline)
        .map_err(|error| format!("cannot read control response: {error}"))?;
    reject_duplicate_json_keys(&response[..length])?;
    let reply: ControlResponse = serde_json::from_slice(&response[..length])
        .map_err(|error| format!("invalid control response: {error}"))?;
    #[cfg(desktop_pet_legacy)]
    let reply = {
        let mut reply = reply;
        reply.observation_mode = desktop_pet_contracts::ObservationSelection::migrate_legacy_input(
            Some(&reply.observation_mode),
        )?
        .as_str()
        .into();
        reply
    };
    Ok(reply)
}
pub(crate) fn send_raw_frame(
    socket_path: &Path,
    frame: &[u8],
    timeout: Duration,
    response_limit: usize,
) -> Result<Vec<u8>, String> {
    reject_duplicate_json_keys(frame)?;
    let deadline = Instant::now() + timeout;
    let mut stream = connect_private_socket(socket_path, deadline)?;
    let mut bytes = frame.to_owned();
    if bytes.last() != Some(&b'\n') {
        bytes.push(b'\n');
    }
    write_frame(&mut stream, &bytes, deadline)
        .map_err(|e| format!("cannot send integration request: {e}"))?;
    let mut reply = read_client_frame(&mut stream, deadline, response_limit)
        .map_err(|e| format!("cannot read integration reply: {e}"))?;
    reject_duplicate_json_keys(&reply)?;
    reply.push(b'\n');
    if reply.len() > response_limit {
        return Err("integration reply exceeds frame bound".into());
    }
    Ok(reply)
}

pub(crate) fn send_presentation_request(
    socket_path: &Path,
    request: PresentationRequestEnvelope,
    timeout: Duration,
) -> Result<PresentationReplyEnvelope, String> {
    if socket_path.as_os_str().is_empty() {
        return Err("control socket path is empty".to_owned());
    }
    validate_presentation_request(&request)?;
    let mut bytes = serde_json::to_vec(&request)
        .map_err(|error| format!("cannot encode presentation request: {error}"))?;
    if bytes.len() + 1 > MAX_FRAME_BYTES {
        return Err("presentation request exceeds frame limit".to_owned());
    }
    bytes.push(b'\n');
    let deadline = Instant::now() + timeout;
    let mut stream = connect_private_socket(socket_path, deadline)?;
    write_frame(&mut stream, &bytes, deadline)
        .map_err(|error| format!("cannot send presentation request: {error}"))?;
    let mut frame = [0u8; MAX_FRAME_BYTES];
    let length = read_frame(&mut stream, &mut frame, deadline)
        .map_err(|error| format!("cannot read presentation response: {error}"))?;
    decode_presentation_reply(&frame[..length], &request.command)
}

pub(crate) fn decode_presentation_reply(
    frame: &[u8],
    command: &str,
) -> Result<PresentationReplyEnvelope, String> {
    reject_duplicate_json_keys(frame)
        .map_err(|error| format!("invalid presentation response: {error}"))?;
    let response_kind: FrameKind = serde_json::from_slice(frame)
        .map_err(|error| format!("invalid presentation response: {error}"))?;
    if response_kind.kind.is_none() {
        return Err(
            "running daemon does not support presentation automation; restart the daemon"
                .to_owned(),
        );
    }
    let reply: PresentationReplyEnvelope = serde_json::from_slice(frame)
        .map_err(|error| format!("invalid presentation response: {error}"))?;
    if reply.version != PRESENTATION_PROTOCOL_VERSION || reply.kind != "presentation" {
        return Err("unsupported presentation protocol response".to_owned());
    }
    let valid = if reply.ok {
        reply.error.is_none()
            && reply.instance_id.as_ref().is_some_and(|id| !id.is_empty())
            && match command {
                "get" => {
                    reply.snapshot.as_ref().is_some_and(|snapshot| {
                        reply.instance_id.as_deref() == Some(snapshot.instance_id.as_str())
                    }) && reply.operation.is_none()
                }
                "set" | "reset" | "status" => {
                    reply.operation.as_ref().is_some_and(|operation| {
                        reply.instance_id.as_deref() == Some(operation.instance_id.as_str())
                    }) && reply.snapshot.is_none()
                }
                _ => false,
            }
    } else {
        reply.error.as_ref().is_some_and(|error| !error.is_empty())
            && reply.instance_id.is_none()
            && reply.snapshot.is_none()
            && reply.operation.is_none()
    };
    if !valid {
        return Err("invalid presentation response fields".to_owned());
    }
    Ok(reply)
}

fn send_pack_envelope(
    socket_path: &Path,
    envelope: PackRequestEnvelope,
    timeout: Duration,
) -> Result<PackReplyEnvelope, String> {
    send_pack_envelope_until(socket_path, envelope, Instant::now() + timeout)
        .map_err(|error| error.to_string())
}

fn send_pack_envelope_until(
    socket_path: &Path,
    envelope: PackRequestEnvelope,
    deadline: Instant,
) -> Result<PackReplyEnvelope, ClientDeadlineError> {
    client_deadline(deadline)?;
    if socket_path.as_os_str().is_empty() {
        return Err(ClientDeadlineError::Other(
            "control socket path is empty".to_owned(),
        ));
    }
    if envelope.kind != "pack" {
        return Err(ClientDeadlineError::Other(
            "pack request kind is invalid".to_owned(),
        ));
    }
    if let Some(request) = envelope.request.as_ref() {
        validate_pack_request(request).map_err(ClientDeadlineError::Other)?;
    }
    client_deadline(deadline)?;
    let bytes = bounded_json(&envelope, MAX_FRAME_BYTES).map_err(|error| {
        ClientDeadlineError::Other(format!("pack request exceeds frame limit: {error}"))
    })?;
    client_deadline(deadline)?;
    let mut stream = connect_private_socket_io(socket_path, deadline)
        .map_err(|error| client_io_error("cannot connect to control socket", error, deadline))?;
    client_deadline(deadline)?;
    write_frame(&mut stream, &bytes, deadline)
        .map_err(|error| client_io_error("cannot send pack request", error, deadline))?;
    let mut response = [0u8; MAX_FRAME_BYTES];
    let length = read_frame(&mut stream, &mut response, deadline)
        .map_err(|error| client_io_error("cannot read pack response", error, deadline))?;
    client_deadline(deadline)?;
    let response: PackReplyEnvelope = serde_json::from_slice(&response[..length])
        .map_err(|error| ClientDeadlineError::Other(format!("invalid pack response: {error}")))?;
    if response.version != PACK_PROTOCOL_VERSION || response.kind != "pack" {
        return Err(ClientDeadlineError::Other(
            "invalid pack response envelope".to_owned(),
        ));
    }
    client_deadline(deadline)?;
    Ok(response)
}

fn collect_pack_list_pages<F>(mut request_page: F) -> Result<PackReplyEnvelope, String>
where
    F: FnMut(usize, usize) -> Result<PackReplyEnvelope, String>,
{
    let mut combined: Option<PackReplyEnvelope> = None;
    let mut seen_ids = HashSet::new();
    let mut offset = 0usize;

    for _ in 0..MAX_PACK_LIST_ROUND_TRIPS {
        let reply = request_page(offset, PACK_LIST_PAGE_SIZE)?;
        if !reply.ok {
            return Ok(reply);
        }
        let mut page = reply
            .listing
            .ok_or_else(|| "pack listing is unavailable".to_owned())?;
        if page.packs.len() > PACK_LIST_PAGE_SIZE {
            return Err("pack list page exceeds the page size".to_owned());
        }
        if offset.saturating_add(page.packs.len()) > MAX_PACK_LIST_RECORDS {
            return Err("pack listing exceeds the maximum record count".to_owned());
        }
        for record in &page.packs {
            if !seen_ids.insert(record.id.clone()) {
                return Err("pack listing contains duplicate records".to_owned());
            }
        }

        let page_len = page.packs.len();
        if let Some(current) = combined.as_mut() {
            let current_listing = current
                .listing
                .as_mut()
                .ok_or_else(|| "pack listing is unavailable".to_owned())?;
            if current_listing.generation != page.generation
                || current_listing.selected != page.selected
                || current_listing.active != page.active
                || current_listing.override_active != page.override_active
            {
                return Err("pack listing changed while it was being paged".to_owned());
            }
            current_listing.packs.append(&mut page.packs);
        } else {
            combined = Some(PackReplyEnvelope {
                version: PACK_PROTOCOL_VERSION,
                kind: "pack".to_owned(),
                command: "list".to_owned(),
                ok: true,
                listing: Some(page),
                operation: None,
                error: None,
            });
        }
        offset += page_len;
        if page_len < PACK_LIST_PAGE_SIZE {
            return combined.ok_or_else(|| "pack listing is unavailable".to_owned());
        }
    }
    Err("pack listing exceeded the pagination round-trip limit".to_owned())
}

pub fn send_pack_list(socket_path: &Path, timeout: Duration) -> Result<PackReplyEnvelope, String> {
    collect_pack_list_pages(|offset, limit| {
        send_pack_envelope(
            socket_path,
            PackRequestEnvelope {
                version: PACK_PROTOCOL_VERSION,
                kind: "pack".to_owned(),
                command: "list".to_owned(),
                request: None,
                operation_id: None,
                offset: Some(offset),
                limit: Some(limit),
            },
            timeout,
        )
    })
}

pub fn send_pack_request(
    socket_path: &Path,
    request: PackRequest,
    timeout: Duration,
) -> Result<PackReplyEnvelope, String> {
    send_pack_envelope(
        socket_path,
        PackRequestEnvelope {
            version: PACK_PROTOCOL_VERSION,
            kind: "pack".to_owned(),
            command: "submit".to_owned(),
            request: Some(request),
            operation_id: None,
            offset: None,
            limit: None,
        },
        timeout,
    )
}

pub fn send_pack_status(
    socket_path: &Path,
    operation_id: &str,
    timeout: Duration,
) -> Result<PackReplyEnvelope, String> {
    send_pack_status_until(socket_path, operation_id, Instant::now() + timeout)
        .map_err(|error| error.to_string())
}

pub(crate) fn send_pack_status_until(
    socket_path: &Path,
    operation_id: &str,
    deadline: Instant,
) -> Result<PackReplyEnvelope, ClientDeadlineError> {
    client_deadline(deadline)?;
    if operation_id.is_empty() {
        return Err(ClientDeadlineError::Other(
            "pack operation ID is empty".to_owned(),
        ));
    }
    send_pack_envelope_until(
        socket_path,
        PackRequestEnvelope {
            version: PACK_PROTOCOL_VERSION,
            kind: "pack".to_owned(),
            command: "status".to_owned(),
            request: None,
            operation_id: Some(operation_id.to_owned()),
            offset: None,
            limit: None,
        },
        deadline,
    )
}

pub fn control_socket_is_live(path: &Path, timeout: Duration) -> bool {
    send_command(path, "ping", None, timeout)
        .map(|response| response.ok)
        .unwrap_or(false)
}

fn bind_private_socket(path: &Path) -> Result<UnixListener, String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("control socket {} has no parent", path.display()))?;
    lifecycle::validate_directory(parent, true)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "control socket {} must not be a symlink",
                path.display()
            ));
        }
        if !metadata.file_type().is_socket() {
            return Err(format!(
                "control socket {} exists but is not a Unix socket",
                path.display()
            ));
        }
        if metadata.uid() != lifecycle::effective_uid()
            || metadata.permissions().mode() & 0o777 != SOCKET_MODE
        {
            return Err(format!(
                "control socket {} is not private to the current user",
                path.display()
            ));
        }
        match connect_private_socket(path, Instant::now() + Duration::from_millis(150)) {
            Ok(_) => return Err("another desktop-pet daemon is already running".to_owned()),
            Err(error) if is_stale_socket_error(&error) => {
                fs::remove_file(path).map_err(|remove| {
                    format!(
                        "cannot remove stale control socket {}: {remove}",
                        path.display()
                    )
                })?;
            }
            Err(error) => return Err(error),
        }
    }
    let listener = UnixListener::bind(path)
        .map_err(|error| format!("cannot bind control socket {}: {error}", path.display()))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("cannot make control listener nonblocking: {error}"))?;
    fs::set_permissions(path, Permissions::from_mode(SOCKET_MODE))
        .map_err(|error| format!("cannot secure control socket {}: {error}", path.display()))?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect control socket {}: {error}", path.display()))?;
    if metadata.uid() != lifecycle::effective_uid()
        || metadata.permissions().mode() & 0o777 != SOCKET_MODE
    {
        return Err(format!(
            "control socket {} is not private to the current user",
            path.display()
        ));
    }
    Ok(listener)
}

fn connect_private_socket(path: &Path, deadline: Instant) -> Result<UnixStream, String> {
    connect_private_socket_io(path, deadline).map_err(|error| {
        format!(
            "cannot connect to control socket {}: {error}",
            path.display()
        )
    })
}

pub(crate) fn connect_private_socket_io(path: &Path, deadline: Instant) -> io::Result<UnixStream> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink()
            || !metadata.file_type().is_socket()
            || metadata.uid() != lifecycle::effective_uid()
            || metadata.permissions().mode() & 0o777 != SOCKET_MODE
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "control socket {} is not a private user socket",
                    path.display()
                ),
            ));
        }
    }
    socket::connect(path, deadline)
}

fn is_stale_socket_error(error: &str) -> bool {
    error.contains("Connection refused")
        || error.contains("No such file")
        || error.contains("not found")
}

fn remove_owned_socket(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_socket()
        || metadata.uid() != lifecycle::effective_uid()
    {
        return Ok(());
    }
    fs::remove_file(path).map_err(|error| error.to_string())
}

fn wake_listener(path: &Path) {
    let _ = socket::connect(path, Instant::now() + Duration::from_millis(100));
}

#[cfg(target_os = "macos")]
fn verify_peer_uid(stream: &UnixStream) -> bool {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    unsafe {
        libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) == 0 && uid == libc::geteuid()
    }
}

#[cfg(not(target_os = "macos"))]
fn verify_peer_uid(_stream: &UnixStream) -> bool {
    false
}

fn write_response(
    stream: &mut UnixStream,
    response: &ControlResponse,
    deadline: Instant,
) -> io::Result<()> {
    let bytes = response_json(response)?;
    write_frame(stream, &bytes, deadline)
}

fn write_presentation_response(
    stream: &mut UnixStream,
    response: &PresentationReplyEnvelope,
    deadline: Instant,
) -> io::Result<()> {
    let bytes = match presentation_response_json(response) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::InvalidData => presentation_response_json(
            &presentation_error("presentation response exceeds frame limit".to_owned()),
        )?,
        Err(error) => return Err(error),
    };
    write_frame(stream, &bytes, deadline)
}

fn write_automation_response(
    stream: &mut UnixStream,
    response: &AutomationReplyEnvelope,
    deadline: Instant,
) -> io::Result<()> {
    let bytes = match bounded_json(response, MAX_AUTOMATION_REPLY_BYTES) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::InvalidData => bounded_json(
            &automation_error("automation response exceeds 68KiB frame limit".to_owned()),
            MAX_AUTOMATION_REPLY_BYTES,
        )?,
        Err(error) => return Err(error),
    };
    write_frame(stream, &bytes, deadline)
}

fn write_sessions_response(
    stream: &mut UnixStream,
    response: &SessionsReplyEnvelope,
    deadline: Instant,
) -> io::Result<()> {
    let bytes = match bounded_json(response, MAX_SESSION_FRAME_BYTES) {
        Ok(bytes) => bytes,
        // A successful list has already been byte-budgeted; a mismatch is an
        // internal failure, never a generic wire error hiding skipped rows.
        Err(error)
            if response.ok
                && response
                    .result
                    .as_ref()
                    .is_some_and(|page| page.get("rows").is_some()) =>
        {
            return Err(error);
        }
        Err(error) if error.kind() == io::ErrorKind::InvalidData => bounded_json(
            &sessions_error("sessions response exceeds 512KiB frame limit".to_owned()),
            MAX_SESSION_FRAME_BYTES,
        )?,
        Err(error) => return Err(error),
    };
    write_frame(stream, &bytes, deadline)
}

fn write_pack_response(
    stream: &mut UnixStream,
    response: &PackReplyEnvelope,
    deadline: Instant,
) -> io::Result<()> {
    let bytes = pack_response_json(response)?;
    write_frame(stream, &bytes, deadline)
}

fn lock_unpoisoned<'a, T>(mutex: &'a Mutex<T>) -> MutexGuard<'a, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character_types::{CharacterRef, PackRecord};
    #[test]
    fn frame_budget_counts_both_crlf_bytes_before_dispatch() {
        let wire = b"{\"kind\":\"inventory\"}\r\n";
        let (mut peer, mut server) = UnixStream::pair().expect("frame pair");
        peer.write_all(wire).expect("frame write");
        server
            .set_nonblocking(true)
            .expect("nonblocking frame read");
        let (frame, count) =
            read_client_frame_with_size(&mut server, Instant::now() + CLIENT_TIMEOUT, wire.len())
                .expect("exact bound");
        assert_eq!(count, wire.len());
        assert_eq!(frame, b"{\"kind\":\"inventory\"}");
        let (mut peer, mut server) = UnixStream::pair().expect("oversized frame pair");
        peer.write_all(wire).expect("oversized frame write");
        server
            .set_nonblocking(true)
            .expect("nonblocking oversized read");
        assert!(read_client_frame_with_size(
            &mut server,
            Instant::now() + CLIENT_TIMEOUT,
            wire.len() - 1
        )
        .is_err());
    }

    #[test]
    fn integration_frames_reject_nested_duplicate_keys_before_dispatch() {
        let duplicate = br#"{"kind":"inventory","hosts":[{"id":"a","id":"b"}]}"#;
        assert!(reject_duplicate_json_keys(duplicate).is_err());
        assert!(
            reject_duplicate_json_keys(br#"{"kind":"inventory","hosts":[{"id":"a"}]}"#).is_ok()
        );
    }

    #[test]
    fn zero_integration_readiness_has_no_host_authority() {
        let shared = Arc::new(Mutex::new(AppState::new()));
        lock_unpoisoned(&shared).set_ui_ready();
        let inner = ControlInner {
            stopping: AtomicBool::new(false),
            shared,
            extensions: Vec::new(),
            status_fields: Vec::new(),
            packs: PackService::new(
                PathBuf::from("/tmp/unused-profile"),
                PathBuf::from("/tmp/unused-assets"),
                None,
            ),
            paths: Paths {
                config_dir: PathBuf::from("/tmp/unused-profile"),
                state_dir: PathBuf::from("/tmp/unused-state"),
                control_socket: PathBuf::from("/tmp/unused-control.sock"),
                log_file: PathBuf::from("/tmp/unused-log"),
            },
            lifecycle_writes: Mutex::new(()),
            executable_path: PathBuf::from("/tmp/unused-executable"),
            pid: std::process::id(),
            clients: Mutex::new(Vec::new()),
            active_clients: Mutex::new(0),
        };
        let ready = dispatch(&inner, "ready", None);
        assert!(ready.ok && ready.ready && ready.control_ready);
        assert_eq!(ready.observation_mode, "auto");
        assert!(ready.host_connections.is_empty());
        assert!(!ready.data_connected && !ready.shutdown);
        assert!(!dispatch(&inner, "register", None).ok);
        assert!(!dispatch(&inner, "ready", Some(PathBuf::from("/tmp/host.sock"))).ok);
    }

    #[test]
    fn updater_plan_rejects_other_profile_image_assets_and_source() {
        use desktop_pet_update_coordinator::{ExecutableIdentity, UpdateAction};
        use std::collections::BTreeMap;
        let running = ExecutableIdentity {
            path: PathBuf::from("/tmp/DesktopPet.app/Contents/MacOS/desktop-pet"),
            sha256: "a".repeat(64),
        };
        let origin = InstallOrigin::Source {
            root: PathBuf::from("/tmp/local-checkout"),
        };
        let context = UpdateContext {
            executable: running.path.clone(),
            version: "0.2.0".into(),
            instance_id: "daemon-1".into(),
            config_dir: PathBuf::from("/tmp/profile/config"),
            #[cfg(desktop_pet_legacy)]
            host_plugin_config_dir: None,
            #[cfg(desktop_pet_legacy)]
            herdr_socket: PathBuf::from("/tmp/profile/herdr.sock"),
            state_dir: PathBuf::from("/tmp/profile/state"),
            running: running.clone(),
            running_origin: Some(Box::new(origin.clone())),
            assets_override: Some(PathBuf::from("/tmp/profile/assets")),
            environment: BTreeMap::new(),
            locale: "en".into(),
        };
        let plan = UpdatePlan {
            version: UPDATER_PROTOCOL,
            operation_id: "operation-1".into(),
            context: context.clone(),
            origin,
            action: UpdateAction::SourceRebuild,
            running,
            candidate: None,
            target: None,
        };
        assert!(verify_running_plan(&plan, &context).is_ok());
        let mut tampered = plan.clone();
        tampered.context.state_dir = PathBuf::from("/tmp/other-profile");
        assert!(verify_running_plan(&tampered, &context).is_err());
        tampered = plan.clone();
        tampered.context.assets_override = None;
        assert!(verify_running_plan(&tampered, &context).is_err());
        tampered = plan.clone();
        tampered.origin = InstallOrigin::Source {
            root: PathBuf::from("/tmp/other-checkout"),
        };
        assert!(verify_running_plan(&tampered, &context).is_err());
        tampered = plan.clone();
        tampered.running.sha256 = "b".repeat(64);
        assert!(verify_running_plan(&tampered, &context).is_err());
        tampered = plan;
        tampered.context.instance_id = "daemon-2".into();
        assert!(verify_running_plan(&tampered, &context).is_err());
    }

    #[test]
    fn official_import_control_rejects_missing_cas_and_unbounded_identity() {
        let mut request = PackRequest {
            operation_id: "official-import".to_owned(),
            expected_generation: None,
            action: PackAction::ImportAndSelect {
                official: crate::character_types::OfficialPackIdentity {
                    id: "official-cat".to_owned(),
                    version: "0.0.2".to_owned(),
                    release_tag: "v0.0.2".to_owned(),
                    sha256: "a".repeat(64),
                },
            },
        };
        assert!(validate_pack_request(&request)
            .unwrap_err()
            .contains("expected_generation"));
        request.expected_generation = Some(4);
        assert!(validate_pack_request(&request).is_ok());
        if let PackAction::ImportAndSelect { official } = &mut request.action {
            official.release_tag = "../evil".to_owned();
        }
        assert!(validate_pack_request(&request).is_err());
        if let PackAction::ImportAndSelect { official } = &mut request.action {
            official.release_tag = "v0.0.2".to_owned();
            official.sha256 = "a".repeat(65);
        }
        assert!(validate_pack_request(&request).is_err());
    }

    #[test]
    fn typed_requests_reject_duplicates_unknowns_null_combinations_and_wrong_kinds() {
        for frame in [
            r#"{"version":1,"kind":"automation","command":"request","request":{"instance_id":"i","operation_id":"o","action":{"action":"preferences_get","surprise":true}}}"#,
            r#"{"version":1,"kind":"automation","command":"request","request":{"instance_id":"i","operation_id":"o","action":{"action":"preferences_get"}},"instance_id":null}"#,
            r#"{"version":1,"kind":"automation","command":"status","instance_id":"i","operation_id":"o","request":null}"#,
            r#"{"version":1,"kind":"automation","command":"status","instance_id":"i","instance_id":"j","operation_id":"o"}"#,
            r#"{"version":1,"kind":"automation","command":"request","request":{"instance_id":"i","operation_id":"o","action":{"action":"session_prompt","key":{"instance_id":"i","source_id":1,"generation":1,"terminal_id":"t"},"text":"hi"}}}"#,
            r#"{"version":1,"kind":"prompt","command":"request","request":{"instance_id":"i","operation_id":"o","action":{"action":"preferences_get"}}}"#,
            r#"{"version":1,"kind":"prompt","command":"request","request":{"instance_id":"i","operation_id":"o","action":{"action":"dialogue_list"}}}"#,
            r#"{"version":1,"kind":"automation","command":"request","request":{"instance_id":"i","operation_id":"o","action":{"action":"dialogue_list","unexpected":true}}}"#,
            r#"{"version":1,"kind":"automation","command":"request","request":{"instance_id":"i","operation_id":"o","action":{"action":"dialogue_list","action":"worktree_remove","token":"t"}}}"#,
            r#"{"version":1,"kind":"automation","command":"request","request":{"instance_id":"i","operation_id":"o","action":{"action":"worktree_remove","token":"t","unexpected":true}}}"#,
            r#"{"version":1,"kind":"automation","command":"request","request":{"instance_id":"i","operation_id":"o","action":{"action":"worktree_inspect","key":{"instance_id":"i","source_id":1,"generation":1,"terminal_id":"t","unexpected":true}}}}"#,
            r#"{"version":1,"kind":"prompt","command":"status","instance_id":"i","operation_id":"o"}"#,
            r#"{"version":1,"kind":"automation","command":"request","request":{"instance_id":"i","operation_id":"o","action":{"action":"preferences_get"},"unexpected":1}}"#,
        ] {
            assert!(
                decode_automation_request(frame.as_bytes()).is_err(),
                "{frame}"
            );
        }
        for action in [
            r#"{"action":"dialogue_list"}"#,
            r#"{"action":"worktree_remove","token":"opaque"}"#,
            r#"{"action":"worktree_inspect","key":{"instance_id":"i","source_id":1,"generation":1,"terminal_id":"t"}}"#,
        ] {
            let frame = format!(
                r#"{{"version":1,"kind":"automation","command":"request","request":{{"instance_id":"i","operation_id":"o","action":{action}}}}}"#
            );
            assert!(decode_automation_request(frame.as_bytes()).is_ok());
        }
        for frame in [
            r#"{"version":1,"kind":"sessions","command":"detail","identity":null}"#,
            r#"{"version":1,"kind":"sessions","command":"detail","identity":{"instance_id":"i","source_id":1,"generation":1,"terminal_id":"t","unknown":1}}"#,
            r#"{"version":1,"kind":"sessions","command":"detail","identity":{"instance_id":"i","source_id":1,"generation":1,"terminal_id":"t"},"page":null}"#,
            r#"{"version":1,"kind":"sessions","command":"list","page":{"instance_id":"i","filter":"all","cursor":null,"limit":129}}"#,
            r#"{"version":1,"kind":"sessions","command":"list","page":{"instance_id":"i","filter":"all","cursor":null,"limit":32},"page":{"instance_id":"i","filter":"all","cursor":null,"limit":32}}"#,
        ] {
            assert!(
                decode_sessions_request(frame.as_bytes()).is_err(),
                "{frame}"
            );
        }
    }

    #[test]
    fn prompt_boundary_and_session_response_are_bounded() {
        let prompt = AutomationRequestEnvelope {
            version: 1,
            kind: "prompt".into(),
            command: "request".into(),
            request: Some(DomainRequest {
                instance_id: "instance".into(),
                operation_id: "op".into(),
                action: DomainAction::SessionPrompt {
                    key: SessionIdentity {
                        instance_id: "instance".into(),
                        source_id: 1,
                        generation: 2,
                        terminal_id: "terminal".into(),
                    },
                    text: "a".repeat(512 * 1024),
                },
            }),
            instance_id: None,
            operation_id: None,
        };
        let encoded =
            bounded_json(&prompt, MAX_PROMPT_FRAME_BYTES).expect("512KiB raw prompt fits envelope");
        assert!(encoded.len() > MAX_FRAME_BYTES);
        assert!(bounded_json(&prompt, MAX_FRAME_BYTES).is_err());
        assert!(matches!(
            decode_automation_request(&encoded[..encoded.len() - 1])
                .unwrap()
                .request
                .unwrap()
                .action,
            DomainAction::SessionPrompt { .. }
        ));
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        let expected_len = encoded.len() - 1;
        let sending = thread::spawn(move || writer.write_all(&encoded));
        let received = read_client_frame(
            &mut reader,
            Instant::now() + Duration::from_secs(5),
            MAX_PROMPT_FRAME_BYTES + 1,
        )
        .unwrap();
        assert_eq!(received.len(), expected_len);
        sending.join().unwrap().unwrap();
        let mut multiline = prompt;
        if let Some(DomainRequest {
            action: DomainAction::SessionPrompt { text, .. },
            ..
        }) = multiline.request.as_mut()
        {
            *text = "line one\nline two\r\n".repeat(10_000);
        }
        let encoded = bounded_json(&multiline, MAX_PROMPT_FRAME_BYTES).unwrap();
        assert!(encoded.len() > MAX_FRAME_BYTES);
        assert!(decode_automation_request(&encoded[..encoded.len() - 1]).is_ok());
        let reply = SessionsReplyEnvelope {
            version: 1,
            kind: "sessions".into(),
            ok: true,
            error: None,
            result: Some(Value::String("x".repeat(MAX_SESSION_FRAME_BYTES))),
        };
        assert!(bounded_json(&reply, MAX_SESSION_FRAME_BYTES).is_err());
        assert!(bounded_json(
            &sessions_error("sessions response exceeds 512KiB frame limit".into()),
            MAX_SESSION_FRAME_BYTES
        )
        .is_ok());
    }

    #[test]
    fn bounded_reader_does_not_extend_past_prompt_allowance() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        let sending = thread::spawn(move || {
            let bytes = vec![b'x'; MAX_PROMPT_FRAME_BYTES + 1024];
            let _ = writer.write_all(&bytes);
        });
        assert!(read_client_frame(
            &mut reader,
            Instant::now() + Duration::from_secs(5),
            MAX_PROMPT_FRAME_BYTES + 1
        )
        .is_err());
        drop(reader);
        sending.join().unwrap();
    }

    #[test]
    fn reply_identity_and_connection_admission_boundaries() {
        let request = AutomationRequestEnvelope {
            version: 1,
            kind: "automation".into(),
            command: "status".into(),
            request: None,
            instance_id: Some("current".into()),
            operation_id: Some("op".into()),
        };
        let reply = AutomationReplyEnvelope {
            version: 1,
            kind: "automation".into(),
            ok: true,
            error: None,
            instance_id: Some("stale".into()),
            operation: Some(DomainOperation {
                instance_id: "stale".into(),
                operation_id: "op".into(),
                kind: "preferences_get".into(),
                state: crate::automation::DomainOperationState::Accepted,
                committed: false,
                native_applied: false,
                result: None,
                error_code: None,
                error: None,
            }),
        };
        assert!(validate_automation_reply(&reply, &request).is_err());
        let session_request = SessionsRequestEnvelope {
            version: 1,
            kind: "sessions".into(),
            command: "detail".into(),
            page: None,
            identity: Some(SessionIdentity {
                instance_id: "current".into(),
                source_id: 1,
                generation: 2,
                terminal_id: "t".into(),
            }),
        };
        let session_reply = SessionsReplyEnvelope {
            version: 1,
            kind: "sessions".into(),
            ok: true,
            error: None,
            result: Some(
                serde_json::json!({"key":{"instance_id":"stale","source_id":1,"generation":2,"terminal_id":"t"}}),
            ),
        };
        assert!(validate_sessions_reply(&session_reply, &session_request).is_err());
        let mut active = 0;
        for _ in 0..MAX_CONNECTIONS {
            assert!(reserve_client_slot(&mut active));
        }
        assert_eq!(active, 32);
        assert!(!reserve_client_slot(&mut active));
        assert_eq!(active, 32);
    }

    #[test]
    fn automation_replies_retain_large_committed_settings_without_relaxing_requests() {
        let reply = AutomationReplyEnvelope {
            version: 1,
            kind: "automation".into(),
            ok: true,
            error: None,
            instance_id: Some("current".into()),
            operation: Some(DomainOperation {
                instance_id: "current".into(),
                operation_id: "op".into(),
                kind: "preferences_get".into(),
                state: crate::automation::DomainOperationState::Applied,
                committed: true,
                native_applied: true,
                result: Some(Value::String("a".repeat(64 * 1024 - 2))),
                error_code: None,
                error: None,
            }),
        };
        let encoded = bounded_json(&reply, MAX_AUTOMATION_REPLY_BYTES)
            .expect("large settings snapshot fits reply");
        assert!(encoded.len() > MAX_FRAME_BYTES);
        assert!(bounded_json(&reply, MAX_FRAME_BYTES).is_err());
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        let sending = thread::spawn(move || writer.write_all(&encoded));
        let received = read_client_frame(
            &mut reader,
            Instant::now() + Duration::from_secs(5),
            MAX_AUTOMATION_REPLY_BYTES,
        )
        .unwrap();
        assert!(serde_json::from_slice::<AutomationReplyEnvelope>(&received).is_ok());
        sending.join().unwrap().unwrap();
        let mut oversized = reply;
        oversized.operation.as_mut().unwrap().result =
            Some(Value::String("a".repeat(MAX_AUTOMATION_REPLY_BYTES)));
        assert!(bounded_json(&oversized, MAX_AUTOMATION_REPLY_BYTES).is_err());
    }

    #[test]
    fn presentation_wire_rejects_bad_fields_before_submission() {
        for frame in [
            r#"{"version":1,"kind":"presentation","command":"set","instance_id":"i","operation_id":"o","patch":{"visible":true,"visible":false}}"#,
            r#"{"version":1,"kind":"presentation","command":"set","instance_id":"i","operation_id":"o","patch":{"unknown":true}}"#,
            r#"{"version":1,"kind":"presentation","command":"set","instance_id":"i","operation_id":"o","patch":{"scale":"big"}}"#,
            r#"{"version":1,"kind":"presentation","command":"set","instance_id":"i","operation_id":"o","patch":null}"#,
            r#"{"version":1,"kind":"presentation","command":"set","instance_id":"i","operation_id":"o","expected_revision":null,"patch":{}}"#,
            r#"{"version":1,"kind":"presentation","command":"set","instance_id":0,"operation_id":"o","patch":{}}"#,
            r#"{"version":1,"kind":"presentation","command":"set","instance_id":"i","operation_id":"o","expected_revision":-1,"patch":{}}"#,
            r#"{"version":1,"kind":"presentation","command":"reset","instance_id":"i","operation_id":"o","patch":{}}"#,
            r#"{"version":1,"kind":"presentation","command":"reset","instance_id":"i","operation_id":"o","patch":null}"#,
            r#"{"version":1,"kind":"presentation","command":"status","instance_id":"i","operation_id":"o","expected_revision":0}"#,
            r#"{"version":1,"kind":"presentation","command":"status","instance_id":"","operation_id":"o"}"#,
            r#"{"version":1,"kind":"presentation","command":"get","instance_id":"i"}"#,
            r#"{"version":1,"kind":"presentation","command":"get","instance_id":null}"#,
            r#"{"version":1,"kind":"presentation","command":"launch"}"#,
            r#"{"version":2,"kind":"presentation","command":"get"}"#,
        ] {
            assert!(
                decode_presentation_request(frame.as_bytes()).is_err(),
                "{frame}"
            );
        }
    }

    #[test]
    fn settings_view_requires_ready_live_ui_and_rejects_update_freeze() {
        let mut state = AppState::new();
        assert_eq!(
            settings_admission(&state, false).unwrap_err(),
            "desktop pet UI is not ready"
        );
        state.set_ui_ready();
        let before = state.scene();
        assert!(settings_admission(&state, false).is_ok());
        assert!(settings_admission(&state, true).is_err());
        let (reply, _receiver) = mpsc::channel();
        state
            .queue_update_prepare("update".into(), "token".into(), reply)
            .unwrap();
        assert!(state.update_mutation_allowed().is_err());
        assert!(settings_admission(&state, false).is_err());
        let after = state.scene();
        assert_eq!(after.visible, before.visible);
        assert_eq!(after.bubble_visible, before.bubble_visible);
        state.request_shutdown();
        assert_eq!(
            settings_admission(&state, false).unwrap_err(),
            "desktop pet is stopping"
        );
    }

    #[test]
    fn presentation_admission_and_mailbox_read_only_status() {
        use crate::automation::{new_automation, OperationState, PresentationTarget};
        let mut state = AppState::new();
        assert!(presentation_admission(&state, false, false).is_err());
        state.set_ui_ready();
        assert!(presentation_admission(&state, false, false).is_err());
        let automation = new_automation(PresentationTarget::from_scene(&state.scene()), None)
            .expect("create instance ID");
        state.set_automation(Arc::clone(&automation));
        assert!(presentation_admission(&state, true, true).is_err());
        assert!(presentation_admission(&state, true, false).is_ok());
        let mut ledger = lock_automation(&automation);
        let instance = ledger.instance().to_owned();
        let get =
            decode_presentation_request(br#"{"version":1,"kind":"presentation","command":"get"}"#)
                .expect("get request");
        let get_reply = presentation_ledger_reply(&mut ledger, get).expect("get reply");
        assert_eq!(get_reply.snapshot.as_ref().expect("snapshot").revision, 0);
        assert_eq!(get_reply.instance_id.as_deref(), Some(instance.as_str()));
        let set = format!(
            r#"{{"version":1,"kind":"presentation","command":"set","instance_id":"{instance}","operation_id":"op","expected_revision":0,"patch":{{"visible":false}}}}"#
        );
        let set = decode_presentation_request(set.as_bytes()).expect("set request");
        let stale = format!(
            r#"{{"version":1,"kind":"presentation","command":"set","instance_id":"{instance}","operation_id":"stale","expected_revision":1,"patch":{{"visible":false}}}}"#
        );
        let stale = decode_presentation_request(stale.as_bytes()).expect("well-shaped request");
        assert!(presentation_ledger_reply(&mut ledger, stale).is_err());
        let wrong_instance = decode_presentation_request(
            br#"{"version":1,"kind":"presentation","command":"reset","instance_id":"previous-daemon","operation_id":"old"}"#,
        ).expect("well-shaped request");
        assert!(presentation_ledger_reply(&mut ledger, wrong_instance).is_err());
        assert_eq!(ledger.snapshot().revision, 0);
        let accepted = presentation_ledger_reply(&mut ledger, set).expect("accepted request");
        assert!(matches!(
            accepted.operation.expect("operation").state,
            OperationState::Accepted
        ));
        assert_eq!(ledger.snapshot().revision, 0);
        assert!(ledger.snapshot().desired.visible);
        let status = format!(
            r#"{{"version":1,"kind":"presentation","command":"status","instance_id":"{instance}","operation_id":"op"}}"#
        );
        let status = decode_presentation_request(status.as_bytes()).expect("status request");
        let status_reply = presentation_ledger_reply(&mut ledger, status).expect("status reply");
        assert!(matches!(
            status_reply.operation.expect("operation").state,
            OperationState::Accepted
        ));
        assert_eq!(ledger.drain_queued().len(), 1);
        state.request_shutdown();
        assert!(presentation_admission(&state, false, true).is_err());
        assert!(presentation_admission(&state, false, false).is_ok());
    }

    #[test]
    fn presentation_reply_is_bounded_and_old_daemon_explicit() {
        let reply = presentation_error("x".repeat(MAX_FRAME_BYTES));
        assert!(presentation_response_json(&reply).is_err());
        let error = decode_presentation_reply(
            br#"{"type":"status","version":1,"command":"unknown","ok":false}"#,
            "get",
        )
        .unwrap_err();
        assert!(error.contains("does not support presentation automation"));
        let bad = br#"{"version":1,"kind":"presentation","ok":true,"error":null,"instance_id":"i","snapshot":null,"operation":null}"#;
        assert!(decode_presentation_reply(bad, "get").is_err());
    }

    #[test]
    fn lifecycle_frames_reject_duplicate_unknown_and_wrong_typed_mutations() {
        for frame in [
            r#"{"version":1,"kind":"lifecycle","operation":"set","key":"auto_start","value":false,"value":true}"#,
            r#"{"version":1,"kind":"lifecycle","operation":"set","key":"auto_start","value":false,"unexpected":0}"#,
            r#"{"version":1,"kind":"lifecycle","operation":"set","key":"auto_start","value":"off"}"#,
            r#"{"version":1,"kind":"lifecycle","operation":"set","key":"","value":false}"#,
            r#"{"version":1,"kind":"lifecycle","operation":"set","key":"auto_start"}"#,
            r#"{"version":1,"kind":"lifecycle","operation":"get","key":"auto_start"}"#,
            r#"{"version":2,"kind":"lifecycle","operation":"get"}"#,
        ] {
            let parsed = reject_duplicate_json_keys(frame.as_bytes())
                .and_then(|()| {
                    serde_json::from_str::<LifecycleRequest>(frame)
                        .map_err(|error| error.to_string())
                })
                .and_then(|request| validate_lifecycle_request(&request));
            assert!(parsed.is_err(), "{frame}");
        }
    }

    #[test]
    fn lifecycle_dynamic_keys_require_one_admitted_owner_before_mutation() {
        use desktop_pet_contracts::{
            LifecycleExtension, LifecycleSettingView, ObservationSelection, ProfileStore,
        };
        use std::sync::atomic::AtomicUsize;

        struct SettingOwner {
            calls: AtomicUsize,
        }
        impl LifecycleExtension for SettingOwner {
            fn provider(&self) -> &'static str {
                "fixture"
            }
            fn should_exit(&self, _selection: &ObservationSelection, _now: Instant) -> bool {
                false
            }
            fn settings(
                &self,
                store: &ProfileStore,
                selection: &ObservationSelection,
                _locale: &str,
            ) -> Result<Vec<LifecycleSettingView>, String> {
                if selection != &ObservationSelection::Provider(self.provider().into()) {
                    return Ok(Vec::new());
                }
                let (saved, _) = store.read_object_with_presence()?;
                let value = desktop_pet_contracts::profile_store::setting_bool(
                    store.config_dir(),
                    &saved,
                    "fixture_policy",
                )?
                .unwrap_or(true);
                Ok(vec![LifecycleSettingView {
                    key: "enabled".into(),
                    label: "Fixture policy".into(),
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
                assert_eq!(key, "enabled");
                store.update(|saved, _| {
                    saved.insert("fixture_policy".into(), Value::Bool(value));
                    Ok(())
                })?;
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }

        let frame =
            br#"{"version":1,"kind":"lifecycle","operation":"set","key":"enabled","value":false}"#;
        reject_duplicate_json_keys(frame).unwrap();
        let request: LifecycleRequest = serde_json::from_slice(frame).unwrap();
        validate_lifecycle_request(&request).unwrap();
        let key = request.key.as_deref().unwrap();
        let value = request.value.unwrap();
        let config = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let profile = ProfileStore::acquire(config.path(), state.path()).unwrap();
        let selection = ObservationSelection::Provider("fixture".into());
        let owner = Arc::new(SettingOwner {
            calls: AtomicUsize::new(0),
        });
        let extensions: Vec<Arc<dyn LifecycleExtension>> = vec![owner.clone()];
        let before = profile.read_object_with_presence().unwrap();

        assert!(prepare_registered_lifecycle(&profile, &[], &selection, None, key, value).is_err());
        assert!(prepare_registered_lifecycle(
            &profile,
            &extensions,
            &ObservationSelection::Provider("other".into()),
            None,
            key,
            value,
        )
        .is_err());
        assert!(prepare_registered_lifecycle(
            &profile,
            &extensions,
            &selection,
            None,
            "unknown_setting",
            value
        )
        .is_err());
        let duplicate = Arc::new(SettingOwner {
            calls: AtomicUsize::new(0),
        });
        let duplicate_extensions: Vec<Arc<dyn LifecycleExtension>> =
            vec![owner.clone(), duplicate.clone()];
        assert!(lifecycle_values(&profile, &duplicate_extensions, &selection).is_err());
        assert!(prepare_registered_lifecycle(
            &profile,
            &duplicate_extensions,
            &selection,
            None,
            key,
            value
        )
        .is_err());
        assert_eq!(profile.read_object_with_presence().unwrap(), before);
        assert_eq!(owner.calls.load(Ordering::SeqCst), 0);
        assert_eq!(duplicate.calls.load(Ordering::SeqCst), 0);

        assert!(prepare_registered_lifecycle(
            &profile,
            &extensions,
            &selection,
            Some("other"),
            key,
            value
        )
        .is_err());
        assert_eq!(owner.calls.load(Ordering::SeqCst), 0);
        assert!(
            prepare_registered_lifecycle(&profile, &extensions, &selection, None, key, value)
                .unwrap()
                .is_none()
        );
        assert_eq!(owner.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            lifecycle_values(&profile, &extensions, &selection)
                .unwrap()
                .get(key),
            Some(&Value::Bool(false))
        );
        let (saved, present) = profile.read_object_with_presence().unwrap();
        assert!(present);
        assert_eq!(saved.get("fixture_policy"), Some(&Value::Bool(false)));
        assert!(!saved.contains_key("enabled"));
    }

    fn list_reply(packs: Vec<PackRecord>) -> PackReplyEnvelope {
        PackReplyEnvelope {
            version: PACK_PROTOCOL_VERSION,
            kind: "pack".to_owned(),
            command: "list".to_owned(),
            ok: true,
            listing: Some(PackListing {
                generation: 1,
                selected: CharacterRef::builtin(),
                active: None,
                override_active: false,
                packs,
                error: None,
            }),
            operation: None,
            error: None,
        }
    }

    fn list_record(index: usize) -> PackRecord {
        PackRecord {
            id: format!("pack-{index}"),
            name: format!("Pack {index}"),
            head: 1,
            revisions: vec![1],
        }
    }

    #[test]
    fn pack_envelope_rejects_duplicate_wire_fields() {
        let error = serde_json::from_slice::<PackRequestEnvelope>(
            br#"{"version":1,"kind":"pack","command":"list","request":null,"operation_id":null,"offset":0,"limit":1,"limit":1}"#,
        );
        assert!(error.is_err());
    }

    #[test]
    fn raw_frames_reject_duplicate_keys_before_dispatch() {
        assert!(
            reject_duplicate_json_keys(br#"{"version":1,"command":"show","command":"hide"}"#)
                .is_err()
        );
        assert!(reject_duplicate_json_keys(
            br#"{"version":1,"kind":"pack","command":"list","limit":1,"limit":2}"#
        )
        .is_err());
        assert!(reject_duplicate_json_keys(
            br#"{"version":1,"command":"show","endpoint":"/tmp/herdr.sock"}"#
        )
        .is_ok());
    }
    #[test]
    fn ipc_pack_source_paths_are_bounded() {
        let nul = PackRequest {
            operation_id: "op".to_owned(),
            expected_generation: None,
            action: PackAction::Import {
                path: PathBuf::from("/tmp/pack\0source"),
            },
        };
        assert!(validate_pack_request(&nul).is_err());

        let relative = PackRequest {
            operation_id: "op".to_owned(),
            expected_generation: None,
            action: PackAction::Import {
                path: PathBuf::from("relative/source"),
            },
        };
        assert!(validate_pack_request(&relative).is_err());

        let empty = PackRequest {
            operation_id: "op".to_owned(),
            expected_generation: None,
            action: PackAction::Import {
                path: PathBuf::new(),
            },
        };
        assert!(validate_pack_request(&empty).is_err());

        let oversized = PackRequest {
            operation_id: "op".to_owned(),
            expected_generation: None,
            action: PackAction::Update {
                id: "pack".to_owned(),
                path: PathBuf::from(format!("/{}", "x".repeat(MAX_SOURCE_PATH_BYTES))),
            },
        };
        assert!(validate_pack_request(&oversized).is_err());
    }

    #[test]
    fn pack_list_accepts_maximum_records_with_bounded_pages() {
        let records: Vec<_> = (0..MAX_PACK_LIST_RECORDS).map(list_record).collect();
        let mut offsets = Vec::new();
        let reply = collect_pack_list_pages(|offset, limit| {
            offsets.push(offset);
            let end = (offset + limit).min(records.len());
            Ok(list_reply(records[offset..end].to_vec()))
        })
        .expect("maximum valid listing");
        assert_eq!(offsets, vec![0, 8, 16, 24, 32]);
        assert_eq!(
            reply.listing.expect("listing").packs.len(),
            MAX_PACK_LIST_RECORDS
        );
    }

    #[test]
    fn pack_list_rejects_duplicate_and_overflow_pages() {
        let duplicate = collect_pack_list_pages(|offset, limit| {
            if offset == 0 {
                Ok(list_reply((0..limit).map(list_record).collect()))
            } else {
                Ok(list_reply(vec![list_record(0)]))
            }
        });
        assert!(duplicate.is_err());

        let overflow = collect_pack_list_pages(|offset, limit| {
            Ok(list_reply(
                (offset..offset + limit).map(list_record).collect(),
            ))
        });
        assert!(overflow.is_err());
    }

    #[test]
    fn pack_envelope_rejects_unknown_fields() {
        assert!(serde_json::from_str::<PackRequestEnvelope>(
            r#"{"version":1,"kind":"pack","command":"list","request":null,"operation_id":null,"offset":0,"limit":1,"extra":true}"#,
        )
        .is_err());
    }

    #[test]
    fn oversized_pack_reply_is_rejected_before_writing() {
        let listing = PackListing {
            generation: 1,
            selected: CharacterRef::builtin(),
            active: None,
            override_active: false,
            packs: (0..32)
                .map(|index| PackRecord {
                    id: format!("pack-{index}"),
                    name: "quote\\\"".repeat(128),
                    head: u64::MAX,
                    revisions: vec![u64::MAX; 8],
                })
                .collect(),
            error: None,
        };
        let response = PackReplyEnvelope {
            version: PACK_PROTOCOL_VERSION,
            kind: "pack".to_owned(),
            command: "list".to_owned(),
            ok: true,
            listing: Some(listing),
            operation: None,
            error: None,
        };
        assert!(pack_response_json(&response).is_err());
    }
    fn page_reply(rows: Vec<Value>, has_more: bool) -> SessionsReplyEnvelope {
        let next_cursor = if has_more {
            let key = rows.last().unwrap().get("key").unwrap();
            Some(SessionPageCursor {
                instance_id: "daemon".into(),
                revision: 91,
                filter: SessionFilter::Working,
                position: SessionCursor {
                    source_id: 7,
                    terminal_id: key["terminal_id"].as_str().unwrap().into(),
                },
            })
        } else {
            None
        };
        SessionsReplyEnvelope {
            version: PROTOCOL_VERSION,
            kind: "sessions".into(),
            ok: true,
            error: None,
            result: Some(serde_json::json!({
                "instance_id": "daemon",
                "revision": 91,
                "filter": "working",
                "total": 151,
                "matched": 140,
                "status_summary": {"working": 140, "idle": 11},
                "rows": rows,
                "next_cursor": next_cursor,
            })),
        }
    }

    fn page_row(index: usize, label: &str) -> Value {
        serde_json::json!({
            "key": {"instance_id":"daemon", "source_id":7, "generation":2,
                    "terminal_id":format!("term-{index:04}\\/雪\n")},
            "source_label":label,
            "metadata":{"title":"quoted \" NUL \u{0} 雪", "agent":"agent"},
            "availability":"live",
        })
    }

    #[test]
    fn session_wire_pages_collect_every_row_with_actual_escaped_byte_budget() {
        let label = "a".repeat(4096);
        let all: Vec<_> = (0..140).map(|index| page_row(index, &label)).collect();
        let mut collected = Vec::new();
        let mut first_page_len = 0;
        while collected.len() < all.len() {
            let start = collected.len();
            let end = (start + 128).min(all.len());
            let reply = budget_sessions_list_response(page_reply(
                all[start..end].to_vec(),
                end < all.len(),
            ));
            assert!(reply.ok, "{:?}", reply.error);
            let frame = bounded_json(&reply, MAX_SESSION_FRAME_BYTES).unwrap();
            assert!(frame.len() <= MAX_SESSION_FRAME_BYTES);
            let page = reply.result.unwrap();
            assert_eq!(page["matched"], 140);
            assert_eq!(page["total"], 151);
            assert_eq!(page["revision"], 91);
            assert_eq!(page["filter"], "working");
            assert_eq!(page["status_summary"]["idle"], 11);
            let rows = page["rows"].as_array().unwrap();
            assert!(!rows.is_empty());
            if start == 0 {
                first_page_len = rows.len();
            }
            for row in rows {
                assert_eq!(row, &all[collected.len()]);
                collected.push(row["key"]["terminal_id"].as_str().unwrap().to_owned());
            }
            if collected.len() < all.len() {
                assert_eq!(
                    page["next_cursor"]["position"]["terminal_id"],
                    collected.last().unwrap().as_str()
                );
            } else {
                assert!(page["next_cursor"].is_null());
            }
        }
        assert!(first_page_len > 0 && first_page_len < 128);
        assert_eq!(collected.len(), 140);
    }

    #[test]
    fn session_frame_exact_boundary_and_oversize_row_have_no_empty_continuation() {
        let plain = page_reply(vec![page_row(0, "")], false);
        let overhead = bounded_json(&plain, MAX_SESSION_FRAME_BYTES).unwrap().len();
        let label = "a".repeat(MAX_SESSION_FRAME_BYTES - overhead);
        let exact = budget_sessions_list_response(page_reply(vec![page_row(0, &label)], false));
        assert!(exact.ok);
        assert_eq!(
            bounded_json(&exact, MAX_SESSION_FRAME_BYTES).unwrap().len(),
            MAX_SESSION_FRAME_BYTES
        );
        let oversized = budget_sessions_list_response(page_reply(
            vec![page_row(0, &format!("{label}a"))],
            false,
        ));
        assert!(!oversized.ok);
        assert!(oversized.error.unwrap().contains("session row exceeds"));
        // The first row fits without a cursor, but cannot fit if another row
        // demands a continuation. It must fail explicitly rather than spin.
        let required_cursor = budget_sessions_list_response(page_reply(
            vec![page_row(0, &label), page_row(1, "next")],
            false,
        ));
        assert!(!required_cursor.ok);
        assert!(required_cursor
            .error
            .unwrap()
            .contains("session row exceeds"));
        let escaped = page_row(0, &"\\\"\n\u{0}雪".repeat(30_000));
        let reply = budget_sessions_list_response(page_reply(vec![escaped], false));
        assert!(reply.ok);
        assert!(bounded_json(&reply, MAX_SESSION_FRAME_BYTES).is_ok());
    }

    #[test]
    fn escaped_long_cursor_is_counted_at_the_emitted_prefix_boundary() {
        let mut first = page_row(0, "");
        first["key"]["terminal_id"] = Value::String("id\\\"\u{0}雪".repeat(1400));
        let overhead = bounded_json(
            &page_reply(vec![first.clone()], true),
            MAX_SESSION_FRAME_BYTES,
        )
        .unwrap()
        .len();
        let mut over = first.clone();
        first["source_label"] = Value::String("a".repeat(MAX_SESSION_FRAME_BYTES - overhead));
        over["source_label"] = Value::String("a".repeat(MAX_SESSION_FRAME_BYTES - overhead + 1));
        // Without a continuation cursor the short next row can fit, hiding
        // the escaped first-row cursor cost. Force a continuation by making
        // the second row too large even when the cursor is omitted.
        let first_without_cursor =
            json_byte_count(&page_reply(vec![first.clone()], false)).unwrap() + 1;
        assert!(first_without_cursor < MAX_SESSION_FRAME_BYTES);
        let next = page_row(
            1,
            &"n".repeat(MAX_SESSION_FRAME_BYTES - first_without_cursor + 1),
        );
        assert_eq!(
            json_byte_count(&page_reply(vec![first.clone()], true)).unwrap() + 1,
            MAX_SESSION_FRAME_BYTES
        );
        assert!(
            json_byte_count(&page_reply(vec![first.clone(), next.clone()], false)).unwrap() + 1
                > MAX_SESSION_FRAME_BYTES
        );
        assert!(
            json_byte_count(&page_reply(vec![over.clone()], true)).unwrap() + 1
                > MAX_SESSION_FRAME_BYTES
        );
        assert!(
            json_byte_count(&page_reply(vec![over.clone()], false)).unwrap() + 1
                <= MAX_SESSION_FRAME_BYTES
        );
        let response =
            budget_sessions_list_response(page_reply(vec![first.clone(), next.clone()], false));
        assert!(response.ok, "{:?}", response.error);
        assert_eq!(
            bounded_json(&response, MAX_SESSION_FRAME_BYTES)
                .unwrap()
                .len(),
            MAX_SESSION_FRAME_BYTES
        );
        let page = response.result.unwrap();
        assert_eq!(page["rows"].as_array().unwrap().len(), 1);
        assert_eq!(page["rows"][0], first);
        assert_eq!(
            page["next_cursor"]["position"]["terminal_id"],
            first["key"]["terminal_id"]
        );
        let continued = budget_sessions_list_response(page_reply(vec![next.clone()], false));
        assert!(continued.ok, "{:?}", continued.error);
        assert!(bounded_json(&continued, MAX_SESSION_FRAME_BYTES).is_ok());
        let continued_page = continued.result.unwrap();
        assert_eq!(continued_page["rows"][0], next);
        assert!(continued_page["next_cursor"].is_null());
        let too_big = budget_sessions_list_response(page_reply(vec![over, next], false));
        assert!(!too_big.ok);
        assert!(too_big.error.unwrap().contains("session row exceeds"));
    }

    #[test]
    fn client_deadline_rejects_late_trickled_reply_and_distinguishes_early_errors() {
        use std::io::Read;
        let directory = std::env::temp_dir().join(format!(
            "hw{:x}{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("peer.sock");
        let listener = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, Permissions::from_mode(SOCKET_MODE)).unwrap();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut byte = [0u8; 1];
            while stream.read(&mut byte).unwrap() == 1 && byte[0] != b'\n' {}
            stream
                .write_all(b"{\"version\":1,\"kind\":\"automation\"")
                .unwrap();
            thread::sleep(Duration::from_millis(500));
            let _ = stream.write_all(
                b",\"ok\":false,\"error\":\"late\",\"instance_id\":null,\"operation\":null}\n",
            );
        });
        let request = AutomationRequestEnvelope {
            version: PROTOCOL_VERSION,
            kind: "automation".into(),
            command: "status".into(),
            request: None,
            instance_id: Some("daemon".into()),
            operation_id: Some("op".into()),
        };
        let result = send_automation_request_until(
            &path,
            &request,
            Instant::now() + Duration::from_millis(250),
        );
        assert!(matches!(result, Err(ClientDeadlineError::Elapsed)));
        peer.join().unwrap();
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&directory).unwrap();
        assert!(matches!(
            client_io_error(
                "read",
                io::Error::from(io::ErrorKind::WouldBlock),
                Instant::now() + Duration::from_secs(1)
            ),
            ClientDeadlineError::Other(_)
        ));
        assert!(matches!(
            client_io_error(
                "read",
                io::Error::from(io::ErrorKind::WouldBlock),
                Instant::now() - Duration::from_secs(1)
            ),
            ClientDeadlineError::Elapsed
        ));
    }
    #[test]
    fn expired_status_does_not_connect_and_early_protocol_failure_is_not_elapsed() {
        use std::io::Read;
        let directory = std::env::temp_dir().join(format!(
            "hw{:x}{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("peer.sock");
        let listener = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, Permissions::from_mode(SOCKET_MODE)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let request = AutomationRequestEnvelope {
            version: PROTOCOL_VERSION,
            kind: "automation".into(),
            command: "status".into(),
            request: None,
            instance_id: Some("daemon".into()),
            operation_id: Some("op".into()),
        };
        assert!(matches!(
            send_automation_request_until(
                &path,
                &request,
                Instant::now() - Duration::from_millis(1)
            ),
            Err(ClientDeadlineError::Elapsed)
        ));
        assert!(matches!(
            send_pack_status_until(&path, "op", Instant::now() - Duration::from_millis(1)),
            Err(ClientDeadlineError::Elapsed)
        ));
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        listener.set_nonblocking(false).unwrap();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut byte = [0u8; 1];
            while stream.read(&mut byte).unwrap() == 1 && byte[0] != b'\n' {}
            let reply = AutomationReplyEnvelope {
                version: PROTOCOL_VERSION,
                kind: "automation".into(),
                ok: true,
                error: None,
                instance_id: Some("wrong".into()),
                operation: Some(DomainOperation {
                    instance_id: "wrong".into(),
                    operation_id: "op".into(),
                    kind: "preferences_get".into(),
                    state: crate::automation::DomainOperationState::Accepted,
                    committed: false,
                    native_applied: false,
                    result: None,
                    error_code: None,
                    error: None,
                }),
            };
            stream
                .write_all(&bounded_json(&reply, MAX_AUTOMATION_REPLY_BYTES).unwrap())
                .unwrap();
        });
        let error =
            send_automation_request_until(&path, &request, Instant::now() + Duration::from_secs(2))
                .unwrap_err();
        assert!(matches!(error, ClientDeadlineError::Other(_)));
        peer.join().unwrap();
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&directory).unwrap();
    }
    #[test]
    fn pack_status_trickle_cannot_extend_absolute_deadline() {
        use std::io::Read;
        let directory = std::env::temp_dir().join(format!(
            "hw{:x}{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("peer.sock");
        let listener = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, Permissions::from_mode(SOCKET_MODE)).unwrap();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut byte = [0u8; 1];
            while stream.read(&mut byte).unwrap() == 1 && byte[0] != b'\n' {}
            stream
                .write_all(b"{\"version\":1,\"kind\":\"pack\"")
                .unwrap();
            thread::sleep(Duration::from_millis(500));
            let _ = stream.write_all(b",\"command\":\"status\",\"ok\":false,\"error\":\"late\"}\n");
        });
        let reply = send_pack_status_until(
            &path,
            "operation",
            Instant::now() + Duration::from_millis(250),
        );
        assert!(matches!(reply, Err(ClientDeadlineError::Elapsed)));
        peer.join().unwrap();
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&directory).unwrap();
    }
}
