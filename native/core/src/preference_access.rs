//! Common opaque preference access for adapter-owned native actions.
//! Live requests retain their actual CAS/result identity. Offline mutations use
//! the common selected-profile owner and updater admission, never adapter I/O.
use crate::automation::{DomainAction, DomainOperation, DomainOperationState, DomainRequest};
use crate::control::{
    self, AutomationReplyEnvelope, AutomationRequestEnvelope, ControlResponse,
    PresentationRequestEnvelope, CLIENT_TIMEOUT, MAX_AUTOMATION_REPLY_BYTES, MAX_FRAME_BYTES,
    PRESENTATION_PROTOCOL_VERSION, PROTOCOL_VERSION,
};
use crate::lifecycle::Paths;
use desktop_pet_contracts::{LifecycleExtension, PreferenceAccess, PreferenceCommit};
use serde::Serialize;
use serde_json::{Map, Value};
use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// A request-scoped in-process port. All operations share the transport's
/// original absolute deadline and the UI's existing operation ledger.
pub(crate) struct LivePreferences {
    shared: Arc<std::sync::Mutex<crate::state::AppState>>,
    deadline: Instant,
}

impl LivePreferences {
    pub(crate) fn new(
        shared: Arc<std::sync::Mutex<crate::state::AppState>>,
        deadline: Instant,
    ) -> Self {
        Self { shared, deadline }
    }

    fn submit(&self, action: DomainAction) -> Result<(crate::automation::SharedAutomation, String, String), String> {
        availability_timeout(self.deadline, CLIENT_TIMEOUT)?;
        let ledger = {
            let state = self.shared.lock().map_err(|_| "native state lock is poisoned")?;
            if !state.is_ui_ready() || state.scene().shutdown {
                return Err("desktop pet UI is not ready".into());
            }
            if matches!(&action, DomainAction::PreferencesSet { .. }) {
                state.update_mutation_allowed()?;
            }
            state.automation().ok_or("automation owner is unavailable")?
        };
        let id = crate::cli::new_operation_id();
        let instance = {
            let mut owner = crate::automation::lock_automation(&ledger);
            let instance = owner.instance().to_owned();
            owner.submit_local_preferences(DomainRequest {
                instance_id: instance.clone(), operation_id: id.clone(), action,
            })?;
            instance
        };
        crate::ui::wake();
        Ok((ledger, instance, id))
    }

    fn observe(&self, ledger: &crate::automation::SharedAutomation, instance: &str, id: &str)
        -> Result<DomainOperation, String>
    {
        loop {
            let operation = crate::automation::lock_automation(ledger).domain_status(instance, id)?;
            if operation.state.terminal() {
                return Ok(operation);
            }
            availability_timeout(self.deadline, CLIENT_TIMEOUT)?;
            thread::sleep(Duration::from_millis(5).min(self.deadline.saturating_duration_since(Instant::now())));
        }
    }

    fn read_revision(&self) -> Result<(Map<String, Value>, u64), String> {
        let (ledger, instance, id) = self.submit(DomainAction::PreferencesGet {})?;
        let operation = self.observe(&ledger, &instance, &id)?;
        if operation.state != DomainOperationState::Applied {
            return Err(operation.error.unwrap_or_else(|| "preference read failed".into()));
        }
        let saved = availability_from_result(operation.result.as_ref(), "persisted")?;
        let revision = operation.result.as_ref().and_then(|result| result.get("revision"))
            .and_then(Value::as_u64).ok_or("preference revision is missing")?;
        Ok((saved, revision))
    }
}

impl PreferenceAccess for LivePreferences {
    fn read(&self) -> Result<Map<String, Value>, String> {
        self.read_revision().map(|(saved, _)| saved)
    }

    fn apply(&self, patch: Map<String, Value>) -> Result<PreferenceCommit, String> {
        if patch.is_empty() {
            return Err("preference patch must contain at least one setting".into());
        }
        let (_, revision) = self.read_revision()?;
        let fields = patch.keys().cloned().collect::<Vec<_>>();
        let (ledger, instance, id) = self.submit(DomainAction::PreferencesSet {
            patch: serde_json::from_value(Value::Object(patch))
                .map_err(|error| format!("invalid preference patch: {error}"))?,
            expected_revision: Some(revision),
        })?;
        match self.observe(&ledger, &instance, &id) {
            Ok(operation) => availability_set_outcome(operation, &instance, &id, &fields),
            Err(_) => Ok(PreferenceCommit::Uncertain {
                instance_id: instance, operation_id: id, current: None,
            }),
        }
    }
}

pub(crate) struct ProfilePreferences {
    paths: Paths,
    extensions: Vec<Arc<dyn LifecycleExtension>>,
}
impl ProfilePreferences {
    pub(crate) fn new(
        paths: Paths,
        extensions: Vec<Arc<dyn LifecycleExtension>>,
    ) -> Arc<dyn PreferenceAccess> {
        Arc::new(Self { paths, extensions })
    }
}
impl PreferenceAccess for ProfilePreferences {
    fn read(&self) -> Result<Map<String, Value>, String> {
        match access_preferences_until(
            &self.paths,
            &self.extensions,
            None,
            Instant::now() + crate::cli::STARTUP_TIMEOUT,
        )? {
            PreferenceCommit::Committed(saved) => Ok(saved),
            PreferenceCommit::Uncertain { .. } => {
                Err("preference read unexpectedly uncertain".to_owned())
            }
        }
    }
    fn apply(&self, patch: Map<String, Value>) -> Result<PreferenceCommit, String> {
        if patch.is_empty() {
            return Err("preference patch must contain at least one setting".to_owned());
        }
        access_preferences_until(
            &self.paths,
            &self.extensions,
            Some(patch),
            Instant::now() + crate::cli::STARTUP_TIMEOUT,
        )
    }
}

fn availability_envelope(
    command: &str,
    request: Option<DomainRequest>,
    identity: Option<(&str, &str)>,
) -> AutomationRequestEnvelope {
    AutomationRequestEnvelope {
        version: PROTOCOL_VERSION,
        kind: "automation".to_owned(),
        command: command.to_owned(),
        request,
        instance_id: identity.map(|(instance, _)| instance.to_owned()),
        operation_id: identity.map(|(_, operation)| operation.to_owned()),
    }
}

fn availability_operation(
    reply: AutomationReplyEnvelope,
    instance: &str,
    id: &str,
    kind: &str,
) -> Result<DomainOperation, String> {
    if !reply.ok {
        return Err(reply
            .error
            .unwrap_or_else(|| "availability request rejected".to_owned()));
    }
    let operation = reply
        .operation
        .ok_or("availability operation result is missing")?;
    if reply.instance_id.as_deref() != Some(instance)
        || operation.instance_id != instance
        || operation.operation_id != id
        || operation.kind != kind
    {
        return Err("availability operation identity changed".to_owned());
    }
    Ok(operation)
}

fn await_availability_operation(
    socket: &Path,
    instance: &str,
    id: &str,
    kind: &str,
    mut operation: DomainOperation,
    deadline: Instant,
) -> Result<DomainOperation, AvailabilityRpcError> {
    while !operation.state.terminal() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("availability operation is still pending".to_owned().into());
        }
        thread::sleep(Duration::from_millis(40).min(remaining));
        let reply = availability_request_until(
            socket,
            &availability_envelope("status", None, Some((instance, id))),
            deadline.min(Instant::now() + CLIENT_TIMEOUT),
            MAX_AUTOMATION_REPLY_BYTES,
            decode_automation_reply,
        )?;
        operation = availability_operation(reply, instance, id, kind)?;
    }
    Ok(operation)
}

fn availability_from_result(
    result: Option<&Value>,
    field: &str,
) -> Result<Map<String, Value>, String> {
    result
        .and_then(|value| value.get(field))
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| format!("preference {field} result is missing or invalid"))
}

fn availability_timeout(deadline: Instant, cap: Duration) -> Result<Duration, String> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err("timed out waiting for desktop-pet lifecycle finalization".to_owned());
    }
    Ok(remaining.min(cap))
}

#[derive(Debug)]
enum AvailabilityRpcError {
    SocketAbsent(String),
    Transport(String),
    Permanent(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AvailabilityPing {
    Live,
    Absent,
}

impl From<String> for AvailabilityRpcError {
    fn from(error: String) -> Self {
        Self::Permanent(error)
    }
}

impl From<&str> for AvailabilityRpcError {
    fn from(error: &str) -> Self {
        Self::Permanent(error.to_owned())
    }
}

impl std::fmt::Display for AvailabilityRpcError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SocketAbsent(error) | Self::Transport(error) | Self::Permanent(error) => {
                formatter.write_str(error)
            }
        }
    }
}

fn availability_io_error(context: &str, error: io::Error) -> AvailabilityRpcError {
    let message = format!("{context}: {error}");
    match error.kind() {
        io::ErrorKind::NotFound
        | io::ErrorKind::ConnectionRefused
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::BrokenPipe
        | io::ErrorKind::UnexpectedEof
        | io::ErrorKind::TimedOut
        | io::ErrorKind::WouldBlock
        | io::ErrorKind::WriteZero => AvailabilityRpcError::Transport(message),
        _ => AvailabilityRpcError::Permanent(message),
    }
}

fn availability_connect_error(error: io::Error) -> AvailabilityRpcError {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
            AvailabilityRpcError::SocketAbsent(format!("cannot connect to control socket: {error}"))
        }
        _ => availability_io_error("cannot connect to control socket", error),
    }
}

fn availability_request_until<T: Serialize, R>(
    socket: &Path,
    request: &T,
    deadline: Instant,
    reply_limit: usize,
    decode: impl FnOnce(&[u8], &T) -> Result<R, String>,
) -> Result<R, AvailabilityRpcError> {
    availability_send_until(socket, request, deadline, reply_limit, &mut 0, decode)
}

fn availability_send_until<T: Serialize, R>(
    socket: &Path,
    request: &T,
    deadline: Instant,
    reply_limit: usize,
    submitted_bytes: &mut usize,
    decode: impl FnOnce(&[u8], &T) -> Result<R, String>,
) -> Result<R, AvailabilityRpcError> {
    availability_timeout(deadline, CLIENT_TIMEOUT)?;
    let bytes = control::bounded_json(request, MAX_FRAME_BYTES)
        .map_err(|error| AvailabilityRpcError::Permanent(error.to_string()))?;
    availability_timeout(deadline, CLIENT_TIMEOUT)?;
    let mut stream =
        control::connect_private_socket_io(socket, deadline).map_err(availability_connect_error)?;
    control::write_frame_counted(&mut stream, &bytes, deadline, submitted_bytes)
        .map_err(|error| availability_io_error("cannot send availability request", error))?;
    let frame = control::read_client_frame_checked(&mut stream, deadline, reply_limit, true)
        .map_err(|error| availability_io_error("cannot read availability response", error))?;
    let reply = decode(&frame, request)?;
    availability_timeout(deadline, CLIENT_TIMEOUT)?;
    Ok(reply)
}

fn availability_ping_until(
    socket: &Path,
    deadline: Instant,
) -> Result<AvailabilityPing, AvailabilityRpcError> {
    let reply: ControlResponse = match availability_request_until(
        socket,
        &serde_json::json!({"version": PROTOCOL_VERSION, "command": "ping"}),
        deadline,
        MAX_FRAME_BYTES,
        |frame, _| {
            control::reject_duplicate_json_keys(frame)?;
            serde_json::from_slice(frame)
                .map_err(|error| format!("invalid availability ping response: {error}"))
        },
    ) {
        Ok(reply) => reply,
        Err(AvailabilityRpcError::SocketAbsent(_)) => return Ok(AvailabilityPing::Absent),
        Err(error) => return Err(error),
    };
    let status = reply.r#type == "status" && reply.ok && reply.error.is_none();
    let stopping = reply.r#type == "error"
        && !reply.ok
        && reply.error.as_deref() == Some("desktop pet is stopping");
    if reply.version != PROTOCOL_VERSION
        || reply.command != "ping"
        || !reply.running
        || reply.pid == 0
        || reply.instance_id.is_empty()
        || !(status || stopping)
    {
        return Err("invalid live availability ping response".into());
    }
    Ok(AvailabilityPing::Live)
}

fn availability_socket_released(
    socket: &Path,
    deadline: Instant,
) -> Result<bool, AvailabilityRpcError> {
    availability_timeout(deadline, Duration::from_millis(100))?;
    availability_ping_until(
        socket,
        deadline.min(Instant::now() + Duration::from_millis(100)),
    )
    .map(|presence| presence == AvailabilityPing::Absent)
}

fn online_availability_owner(
    paths: &Paths,
    deadline: Instant,
) -> Result<ControlResponse, AvailabilityRpcError> {
    let reply: ControlResponse = availability_request_until(
        &paths.control_socket,
        &serde_json::json!({"version": PROTOCOL_VERSION, "command": "status"}),
        deadline.min(Instant::now() + CLIENT_TIMEOUT),
        MAX_FRAME_BYTES,
        |frame, _| {
            control::reject_duplicate_json_keys(frame)?;
            serde_json::from_slice(frame)
                .map_err(|error| format!("invalid control response: {error}"))
        },
    )?;
    if !reply.ok
        || !reply.running
        || reply.r#type != "status"
        || reply.command != "status"
        || reply.version != PROTOCOL_VERSION
        || reply.pid == 0
        || reply.instance_id.is_empty()
    {
        return Err("invalid live preferences owner status".into());
    }
    let config = fs::canonicalize(&paths.config_dir).map_err(|error| error.to_string())?;
    let state = fs::canonicalize(&paths.state_dir).map_err(|error| error.to_string())?;
    let owner_config = fs::canonicalize(&reply.config_dir)
        .map_err(|error| format!("cannot resolve live preferences config owner: {error}"))?;
    let owner_state = fs::canonicalize(&reply.state_dir)
        .map_err(|error| format!("cannot resolve live preferences state owner: {error}"))?;
    availability_timeout(deadline, CLIENT_TIMEOUT)?;
    if config != owner_config || state != owner_state {
        return Err("live desktop-pet owns a different config/state profile".into());
    }
    Ok(reply)
}

fn online_availability_instance(
    socket: &Path,
    deadline: Instant,
) -> Result<String, AvailabilityRpcError> {
    let reply = availability_request_until(
        socket,
        &PresentationRequestEnvelope {
            version: PRESENTATION_PROTOCOL_VERSION,
            kind: "presentation".to_owned(),
            command: "get".to_owned(),
            instance_id: None,
            operation_id: None,
            expected_revision: None,
            patch: None,
        },
        deadline.min(Instant::now() + CLIENT_TIMEOUT),
        MAX_FRAME_BYTES,
        |frame, request| control::decode_presentation_reply(frame, &request.command),
    )?;
    availability_timeout(deadline, CLIENT_TIMEOUT)?;
    if !reply.ok {
        return Err(reply
            .error
            .unwrap_or_else(|| "desktop-pet unavailable".to_owned())
            .into());
    }
    let snapshot = reply
        .snapshot
        .ok_or_else(|| "desktop-pet instance snapshot is missing".to_owned())?;
    if reply.instance_id.as_deref() != Some(snapshot.instance_id.as_str()) {
        return Err("desktop-pet instance changed".into());
    }
    Ok(snapshot.instance_id)
}

fn online_availability_get(
    socket: &Path,
    instance: &str,
    deadline: Instant,
) -> Result<(Map<String, Value>, u64), AvailabilityRpcError> {
    let id = crate::cli::new_operation_id();
    let request = availability_envelope(
        "request",
        Some(DomainRequest {
            instance_id: instance.to_owned(),
            operation_id: id.clone(),
            action: DomainAction::PreferencesGet {},
        }),
        None,
    );
    let reply = availability_request_until(
        socket,
        &request,
        deadline.min(Instant::now() + CLIENT_TIMEOUT),
        MAX_AUTOMATION_REPLY_BYTES,
        decode_automation_reply,
    )?;
    let operation = availability_operation(reply, instance, &id, "preferences_get")?;
    let operation = await_availability_operation(
        socket,
        instance,
        &id,
        "preferences_get",
        operation,
        deadline,
    )?;
    if operation.state != DomainOperationState::Applied {
        return Err(operation
            .error
            .unwrap_or_else(|| format!("availability read ended in {:?}", operation.state))
            .into());
    }
    let installed = availability_from_result(operation.result.as_ref(), "persisted")?;
    let revision = operation
        .result
        .as_ref()
        .and_then(|value| value.get("revision"))
        .and_then(Value::as_u64)
        .ok_or_else(|| "availability revision is missing".to_owned())?;
    Ok((installed, revision))
}

fn availability_set_outcome(
    operation: DomainOperation,
    instance: &str,
    id: &str,
    fields: &[String],
) -> Result<PreferenceCommit, String> {
    if operation.committed {
        return Ok(
            match availability_from_result(operation.result.as_ref(), "persisted").and_then(
                |saved| {
                    if fields.iter().all(|field| saved.contains_key(field)) {
                        Ok(saved)
                    } else {
                        Err("committed preference result is missing a requested field".to_owned())
                    }
                },
            ) {
                Ok(saved) => PreferenceCommit::Committed(saved),
                Err(_) => PreferenceCommit::Uncertain {
                    instance_id: instance.to_owned(),
                    operation_id: id.to_owned(),
                    current: None,
                },
            },
        );
    }
    Err(operation
        .error
        .unwrap_or_else(|| format!("availability change ended in {:?}", operation.state)))
}

fn recover_availability_set(
    socket: &Path,
    instance: &str,
    id: &str,
    fields: &[String],
) -> Result<PreferenceCommit, String> {
    let status = availability_envelope("status", None, Some((instance, id)));
    let observation = control::send_automation_request(socket, &status)
        .and_then(|reply| availability_operation(reply, instance, id, "preferences_set"))
        .and_then(|operation| {
            await_availability_operation(
                socket,
                instance,
                id,
                "preferences_set",
                operation,
                Instant::now() + crate::cli::STARTUP_TIMEOUT,
            )
            .map_err(|error| error.to_string())
        });
    match observation {
        Ok(operation) => availability_set_outcome(operation, instance, id, fields),
        Err(_) => Ok(PreferenceCommit::Uncertain {
            instance_id: instance.to_owned(),
            operation_id: id.to_owned(),
            current: None,
        }),
    }
}

fn online_availability_set(
    socket: &Path,
    instance: &str,
    revision: u64,
    patch: Map<String, Value>,
    pre_submit_deadline: Instant,
) -> Result<PreferenceCommit, String> {
    // Keep only names across conversion to the common typed wire patch. A
    // postcommit result omitting an owner field cannot prove its saved outcome.
    let fields: Vec<_> = patch.keys().cloned().collect();
    let id = crate::cli::new_operation_id();
    let request = availability_envelope(
        "request",
        Some(DomainRequest {
            instance_id: instance.to_owned(),
            operation_id: id.clone(),
            action: DomainAction::PreferencesSet {
                patch: serde_json::from_value(Value::Object(patch))
                    .map_err(|error| format!("invalid preference patch: {error}"))?,
                expected_revision: Some(revision),
            },
        }),
        None,
    );
    availability_timeout(pre_submit_deadline, CLIENT_TIMEOUT)?;
    // An uncertain acknowledgement cannot prove non-delivery. Query this exact
    // identity if possible; never submit the mutation again or save offline.
    let mut submitted_bytes = 0;
    let reply = match availability_send_until(
        socket,
        &request,
        pre_submit_deadline,
        MAX_AUTOMATION_REPLY_BYTES,
        &mut submitted_bytes,
        decode_automation_reply,
    ) {
        Ok(reply) => reply,
        Err(error) if submitted_bytes == 0 => return Err(error.to_string()),
        Err(_) => return recover_availability_set(socket, instance, &id, &fields),
    };
    let operation = match availability_operation(reply, instance, &id, "preferences_set") {
        Ok(operation) => operation,
        Err(error)
            if error == "availability operation identity changed"
                || error == "availability operation result is missing" =>
        {
            return recover_availability_set(socket, instance, &id, &fields);
        }
        Err(error) => return Err(error),
    };
    let operation = match await_availability_operation(
        socket,
        instance,
        &id,
        "preferences_set",
        operation,
        Instant::now() + crate::cli::STARTUP_TIMEOUT,
    ) {
        Ok(operation) => operation,
        Err(_) => {
            return Ok(PreferenceCommit::Uncertain {
                instance_id: instance.to_owned(),
                operation_id: id,
                current: None,
            });
        }
    };
    availability_set_outcome(operation, instance, &id, &fields)
}

fn access_preferences_until(
    paths: &Paths,
    extensions: &[Arc<dyn LifecycleExtension>],
    change: Option<Map<String, Value>>,
    deadline: Instant,
) -> Result<PreferenceCommit, String> {
    let mut expected_instance: Option<String> = None;
    loop {
        // No manager gate or profile writer is held across a socket RPC.
        availability_timeout(deadline, Duration::from_millis(150))?;
        match availability_ping_until(
            &paths.control_socket,
            deadline.min(Instant::now() + Duration::from_millis(150)),
        ) {
            Ok(AvailabilityPing::Live) => {
                let online = (|| -> Result<Option<(String, Map<String, Value>, u64)>, AvailabilityRpcError> {
                    let owner = online_availability_owner(paths, deadline)?;
                    if expected_instance.as_ref().is_some_and(|id| id != &owner.instance_id) {
                        return Err("live preferences owner changed before preference read".into());
                    }
                    expected_instance = Some(owner.instance_id.clone());
                    if !owner.ui_ready || owner.shutdown { return Ok(None); }
                    let instance = online_availability_instance(&paths.control_socket, deadline)?;
                    if owner.instance_id != instance {
                        return Err("live preferences owner changed before preference read".into());
                    }
                    let (current, revision) = online_availability_get(&paths.control_socket, &instance, deadline)?;
                    if change.is_some() {
                        let owner = online_availability_owner(paths, deadline)?;
                        if owner.instance_id != instance {
                            return Err("live preferences owner changed before preference write".into());
                        }
                        if !owner.ui_ready || owner.shutdown { return Ok(None); }
                    }
                    availability_timeout(deadline, CLIENT_TIMEOUT)?;
                    Ok(Some((instance, current, revision)))
                })();
                match online {
                    Ok(Some((instance, current, revision))) => {
                        // Never enter offline fallback after this call may offer bytes.
                        return match change {
                            Some(patch) => online_availability_set(
                                &paths.control_socket,
                                &instance,
                                revision,
                                patch,
                                deadline,
                            ),
                            None => Ok(PreferenceCommit::Committed(current)),
                        };
                    }
                    Ok(None) => {}
                    Err(AvailabilityRpcError::Permanent(error))
                        if error == "desktop pet UI is not ready"
                            || error == "desktop pet is stopping" => {}
                    Err(
                        error @ (AvailabilityRpcError::Transport(_)
                        | AvailabilityRpcError::SocketAbsent(_)),
                    ) => match availability_socket_released(&paths.control_socket, deadline) {
                        Ok(true) => {}
                        Ok(false) => return Err(error.to_string()),
                        Err(AvailabilityRpcError::Transport(_)) => {}
                        Err(probe_error) => return Err(probe_error.to_string()),
                    },
                    Err(error) => return Err(error.to_string()),
                }
            }
            Ok(AvailabilityPing::Absent) => {
                if let Some(saved) = control::access_profile_preferences_offline(
                    paths,
                    extensions,
                    change.clone(),
                    deadline,
                )? {
                    return Ok(PreferenceCommit::Committed(saved));
                }
            }
            Err(AvailabilityRpcError::Transport(_)) => {}
            Err(error) => return Err(error.to_string()),
        }
        thread::sleep(availability_timeout(deadline, Duration::from_millis(50))?);
    }
}

fn decode_automation_reply(
    frame: &[u8],
    request: &AutomationRequestEnvelope,
) -> Result<crate::control::AutomationReplyEnvelope, String> {
    control::reject_duplicate_json_keys(frame)?;
    let reply = serde_json::from_slice(frame)
        .map_err(|error| format!("invalid preference response: {error}"))?;
    control::validate_automation_reply(&reply, request)?;
    Ok(reply)
}

#[cfg(test)]
#[path = "preference_access_tests.rs"]
mod tests;

/// Exclusive offline admission's locked second probe. A connected but silent
/// peer is not absence; malformed/partial/foreign socket evidence is permanent
/// failure, not authorization to save behind a possible live owner.
pub(crate) fn profile_owner_absent_until(paths: &Paths, deadline: Instant) -> Result<bool, String> {
    availability_timeout(deadline, Duration::from_millis(150))?;
    match availability_ping_until(
        &paths.control_socket,
        deadline.min(Instant::now() + Duration::from_millis(150)),
    ) {
        Ok(AvailabilityPing::Absent) => Ok(true),
        Ok(AvailabilityPing::Live) | Err(AvailabilityRpcError::Transport(_)) => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}
