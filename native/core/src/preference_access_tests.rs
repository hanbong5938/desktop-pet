//! Retained incoming preference ownership/CAS/delivery race tests, generalized
//! from one optional field to the common preference snapshot protocol.
use super::*;
use crate::automation::lock_automation;
use crate::preferences::{PreferencePatch, Preferences};
use crate::state::AppState;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
const SOCKET_MODE: u32 = 0o600;
fn flag_patch(value: bool) -> Map<String, Value> {
    serde_json::from_value(serde_json::json!({"show_status_indicators":value})).unwrap()
}
fn saved_flag(directory: &Path) -> Result<bool, String> {
    Ok(Preferences::load_committed(directory)?.show_status_indicators())
}
fn presentation_error(error: String) -> crate::control::PresentationReplyEnvelope {
    crate::control::PresentationReplyEnvelope {
        version: PRESENTATION_PROTOCOL_VERSION,
        kind: "presentation".to_owned(),
        ok: false,
        error: Some(error),
        instance_id: None,
        snapshot: None,
        operation: None,
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
fn write_frame(stream: &mut UnixStream, bytes: &[u8], deadline: Instant) -> io::Result<()> {
    control::write_frame_counted(stream, bytes, deadline, &mut 0)
}
fn bounded_json<T: Serialize>(value: &T, limit: usize) -> io::Result<Vec<u8>> {
    control::bounded_json(value, limit)
}
struct AvailabilityFixture {
    root: PathBuf,
    paths: Paths,
}

impl AvailabilityFixture {
    fn new() -> Self {
        let root = Path::new("/tmp").join(format!("av{}", crate::cli::new_operation_id()));
        let config_dir = root.join("config");
        let state_dir = root.join("state");
        for directory in [&root, &config_dir, &state_dir] {
            fs::create_dir(directory).unwrap();
            fs::set_permissions(directory, Permissions::from_mode(0o700)).unwrap();
        }
        let paths = Paths {
            config_dir,
            control_socket: state_dir
                .join(desktop_pet_contracts::profile_store::CONTROL_SOCKET_FILE),
            log_file: state_dir.join(crate::lifecycle::LOG_FILE),
            state_dir,
        };
        Preferences::default()
            .apply_patch(
                crate::preferences::PreferencePatch {
                    show_status_indicators: Some(false),
                    ..Default::default()
                },
                &paths.config_dir,
            )
            .unwrap();
        Self { root, paths }
    }

    fn serve(
        &self,
        mut hook: impl FnMut(&Value, &mut Value) + Send + 'static,
    ) -> AvailabilityServer {
        self.serve_frames(move |request, reply| {
            hook(request, reply);
            None
        })
    }

    fn serve_frames(
        &self,
        mut hook: impl FnMut(&Value, &mut Value) -> Option<Vec<u8>> + Send + 'static,
    ) -> AvailabilityServer {
        self.serve_frames_with_release(move |request, reply, _| {
            hook(request, reply).map(|bytes| (bytes, true))
        })
    }

    fn serve_frames_with_release(
        &self,
        mut hook: impl FnMut(&Value, &mut Value, &mut UnixStream) -> Option<(Vec<u8>, bool)>
            + Send
            + 'static,
    ) -> AvailabilityServer {
        use crate::automation::{new_automation, DomainOperationState, PresentationTarget};
        use std::io::BufRead;
        let listener = UnixListener::bind(&self.paths.control_socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        fs::set_permissions(
            &self.paths.control_socket,
            Permissions::from_mode(SOCKET_MODE),
        )
        .unwrap();
        let paths = self.paths.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            let automation = new_automation(
                PresentationTarget::from_scene(&AppState::new().scene()),
                None,
            )
            .unwrap();
            let ledger = lock_automation(&automation);
            let instance = ledger.instance().to_owned();
            let presentation = crate::control::PresentationReplyEnvelope {
                version: PRESENTATION_PROTOCOL_VERSION,
                kind: "presentation".to_owned(),
                ok: true,
                error: None,
                instance_id: Some(instance.clone()),
                snapshot: Some(ledger.snapshot()),
                operation: None,
            };
            drop(ledger);
            let mut revision = 7;
            let mut operations = Vec::<DomainOperation>::new();
            let mut requests = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(5);
            while !stopping.load(Ordering::Acquire) && Instant::now() < deadline {
                let (mut stream, _) = match listener.accept() {
                    Ok(peer) => peer,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("fixture accept failed: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut bytes = Vec::new();
                std::io::BufReader::new(&stream)
                    .read_until(b'\n', &mut bytes)
                    .unwrap();
                if bytes.is_empty() {
                    // An expired probe may close while queued behind a
                    // silent peer. It is not a new request.
                    continue;
                }
                let request: Value = serde_json::from_slice(&bytes).unwrap();
                let mut reply = match request["kind"].as_str() {
                    Some("presentation") => serde_json::to_value(&presentation).unwrap(),
                    Some("automation") => {
                        let envelope: AutomationRequestEnvelope =
                            serde_json::from_value(request.clone()).unwrap();
                        let operation = if let Some(domain) = envelope.request {
                            assert_eq!(domain.instance_id, instance);
                            let committed = match domain.action {
                                DomainAction::PreferencesGet {} => false,
                                DomainAction::PreferencesSet {
                                    patch,
                                    expected_revision,
                                } => {
                                    assert_eq!(expected_revision, Some(revision));
                                    let mut prefs =
                                        Preferences::load_committed(&paths.config_dir).unwrap();
                                    prefs.apply_patch(patch, &paths.config_dir).unwrap();
                                    revision += 1;
                                    true
                                }
                                _ => panic!("unexpected availability action"),
                            };
                            let operation = DomainOperation {
                                instance_id: instance.clone(),
                                operation_id: domain.operation_id,
                                kind: if committed {
                                    "preferences_set"
                                } else {
                                    "preferences_get"
                                }
                                .to_owned(),
                                state: DomainOperationState::Applied,
                                committed,
                                native_applied: committed,
                                result: Some(serde_json::json!({
                                    "revision":revision,
                                    "persisted":{"show_status_indicators":
                                        saved_flag(&paths.config_dir)
                                            .unwrap()}
                                })),
                                error_code: None,
                                error: None,
                            };
                            operations.push(operation.clone());
                            operation
                        } else {
                            assert_eq!(envelope.instance_id.as_deref(), Some(instance.as_str()));
                            operations
                                .iter()
                                .find(|op| {
                                    Some(op.operation_id.as_str())
                                        == envelope.operation_id.as_deref()
                                })
                                .expect("status must retain the original operation ID")
                                .clone()
                        };
                        serde_json::to_value(AutomationReplyEnvelope {
                            version: PROTOCOL_VERSION,
                            kind: "automation".to_owned(),
                            ok: true,
                            error: None,
                            instance_id: Some(instance.clone()),
                            operation: Some(operation),
                        })
                        .unwrap()
                    }
                    None => serde_json::json!({
                        "type":"status","version":PROTOCOL_VERSION,"app_version":"test",
                        "ok":true,"command":request["command"],"error":null,"ready":false,
                        "running":true,"phase":"loading","ui_ready":true,"control_ready":true,
                        "registration_accepted":false,"data_connected":false,
                        "connected_sources":0,"disconnected_sources":0,"sessions":0,
                        "working":0,"blocked":0,"done":0,"unknown":0,
                        "visible":true,"passthrough":false,"scale":1.0,"shutdown":false,
                        "auto_start":false,"updater_protocol":4,"observation_capability":1,"integration_api":1,
                        "update_ui_visible":false,"update_operation_id":null,"migration_operation":null,"migration_gated":false,
                        "pid":std::process::id(),"instance_id":instance,
                        "executable_path":"/bin/echo","config_dir":paths.config_dir,
                        "state_dir":paths.state_dir
                    }),
                    _ => panic!("unexpected availability protocol"),
                };
                let final_frame = hook(&request, &mut reply, &mut stream);
                requests.push(request);
                let release = final_frame.as_ref().is_some_and(|(_, release)| *release);
                let bytes = final_frame
                    .map(|(bytes, _)| bytes)
                    .unwrap_or_else(|| bounded_json(&reply, MAX_AUTOMATION_REPLY_BYTES).unwrap());
                if release {
                    // Delete the socket before EOF/error reaches the client.
                    // Permanent errors must still prohibit an offline save.
                    fs::remove_file(&paths.control_socket).unwrap();
                }
                let _ = write_frame(&mut stream, &bytes, Instant::now() + Duration::from_secs(1));
                if release {
                    break;
                }
            }
            drop(listener);
            if paths.control_socket.exists() {
                fs::remove_file(&paths.control_socket).unwrap();
            }
            requests
        });
        AvailabilityServer { stop, thread }
    }
}

impl Drop for AvailabilityFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

struct AvailabilityServer {
    stop: Arc<AtomicBool>,
    thread: thread::JoinHandle<Vec<Value>>,
}

impl AvailabilityServer {
    fn finish(self) -> Vec<Value> {
        self.stop.store(true, Ordering::Release);
        self.thread.join().unwrap()
    }
}

fn availability_set_count(requests: &[Value]) -> usize {
    requests
        .iter()
        .filter(|request| request["request"]["action"]["action"] == "preferences_set")
        .count()
}

#[test]
fn pre_submit_ui_and_stopping_races_retry_only_before_one_set() {
    for (kind, error) in [
        ("presentation", "desktop pet UI is not ready"),
        ("automation", "desktop pet is stopping"),
    ] {
        let fixture = AvailabilityFixture::new();
        let mut rejected = false;
        let server = fixture.serve(move |request, reply| {
            if !rejected && request["kind"] == kind {
                rejected = true;
                *reply = if kind == "presentation" {
                    serde_json::to_value(presentation_error(error.to_owned())).unwrap()
                } else {
                    serde_json::to_value(automation_error(error.to_owned())).unwrap()
                };
            }
        });
        assert_eq!(
            access_preferences_until(
                &fixture.paths,
                &[],
                Some(flag_patch(true)),
                Instant::now() + Duration::from_secs(2),
            )
            .unwrap(),
            PreferenceCommit::Committed(flag_patch(true))
        );
        let requests = server.finish();
        assert_eq!(availability_set_count(&requests), 1);
        assert!(
            requests
                .iter()
                .filter(|request| request["kind"] == kind)
                .count()
                >= 2
        );
        assert!(saved_flag(&fixture.paths.config_dir).unwrap());
    }
}

#[test]
fn mismatched_availability_instance_fails_before_reads_or_writes() {
    let fixture = AvailabilityFixture::new();
    let server = fixture.serve(|request, reply| {
        if request["kind"] == "presentation" {
            reply["instance_id"] = "another-owner".into();
            reply["snapshot"]["instance_id"] = "another-owner".into();
        }
    });
    assert!(access_preferences_until(
        &fixture.paths,
        &[],
        Some(flag_patch(true)),
        Instant::now() + Duration::from_secs(2),
    )
    .is_err());
    let requests = server.finish();
    assert_eq!(requests.len(), 3);
    assert!(!requests
        .iter()
        .any(|request| request["kind"] == "automation"));
    assert!(!saved_flag(&fixture.paths.config_dir).unwrap());
}

#[test]
fn changing_availability_owner_while_waiting_is_not_retried() {
    let fixture = AvailabilityFixture::new();
    let mut statuses = 0;
    let server = fixture.serve(move |request, reply| {
        if request["kind"].is_null() && request["command"] == "status" {
            statuses += 1;
            reply["ui_ready"] = false.into();
            if statuses > 1 {
                reply["instance_id"] = "replacement-owner".into();
            }
        }
    });
    assert!(access_preferences_until(
        &fixture.paths,
        &[],
        Some(flag_patch(true)),
        Instant::now() + Duration::from_secs(2),
    )
    .is_err());
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request["command"] == "status")
            .count(),
        2
    );
    assert!(!requests
        .iter()
        .any(|request| request["kind"] == "automation"));
    assert!(!saved_flag(&fixture.paths.config_dir).unwrap());
}

#[test]
fn slow_pre_submit_read_and_poll_share_deadline_and_never_send_a_late_set() {
    for poll in [false, true] {
        let fixture = AvailabilityFixture::new();
        let deadline = Instant::now() + Duration::from_millis(500);
        let server = fixture.serve(move |request, reply| {
            if request["kind"] == "automation" {
                if poll && request["command"] == "request" {
                    reply["operation"]["state"] = "accepted".into();
                    reply["operation"]["result"] = Value::Null;
                } else {
                    thread::sleep(
                        deadline.saturating_duration_since(Instant::now())
                            + Duration::from_millis(50),
                    );
                }
            }
        });
        assert!(
            access_preferences_until(&fixture.paths, &[], Some(flag_patch(true)), deadline,)
                .is_err()
        );
        let requests = server.finish();
        assert_eq!(availability_set_count(&requests), 0);
        assert!(requests
            .iter()
            .any(|request| request["request"]["action"]["action"] == "preferences_get"));
        if poll {
            assert!(requests
                .iter()
                .any(|request| request["kind"] == "automation" && request["command"] == "status"));
        }
        assert!(!saved_flag(&fixture.paths.config_dir).unwrap());
    }
}

#[test]
fn expired_pre_submit_deadline_does_not_send_preferences_set() {
    let fixture = AvailabilityFixture::new();
    let server = fixture.serve(|_, _| {});
    assert!(online_availability_set(
        &fixture.paths.control_socket,
        "unused-instance",
        7,
        flag_patch(true),
        Instant::now(),
    )
    .is_err());
    assert!(server.finish().is_empty());
    assert!(!saved_flag(&fixture.paths.config_dir).unwrap());
}

#[test]
fn invalid_pre_submit_replies_remain_closed_after_socket_disappears() {
    for stage in ["owner", "presentation", "get", "poll"] {
        for malformed in ["json", "truncated", "oversized", "protocol"] {
            let fixture = AvailabilityFixture::new();
            let before = fs::read(fixture.paths.config_dir.join("preferences.json")).unwrap();
            let server = fixture.serve_frames(move |request, reply| {
                let target = match stage {
                    "owner" => request["kind"].is_null() && request["command"] == "status",
                    "presentation" => request["kind"] == "presentation",
                    "get" => request["request"]["action"]["action"] == "preferences_get",
                    "poll" => request["kind"] == "automation" && request["command"] == "status",
                    _ => unreachable!(),
                };
                if stage == "poll" && request["request"]["action"]["action"] == "preferences_get" {
                    reply["operation"]["state"] = "accepted".into();
                    reply["operation"]["result"] = Value::Null;
                }
                target.then(|| match malformed {
                    "json" => b"not-json\n".to_vec(),
                    "truncated" => b"{\"ok\":true".to_vec(),
                    "oversized" => vec![b'x'; MAX_AUTOMATION_REPLY_BYTES + 1],
                    "protocol" => {
                        reply["version"] = 999.into();
                        bounded_json(reply, MAX_AUTOMATION_REPLY_BYTES).unwrap()
                    }
                    _ => unreachable!(),
                })
            });
            assert!(
                access_preferences_until(
                    &fixture.paths,
                    &[],
                    Some(flag_patch(true)),
                    Instant::now() + Duration::from_secs(2),
                )
                .is_err(),
                "{stage}/{malformed}"
            );
            let requests = server.finish();
            assert_eq!(availability_set_count(&requests), 0, "{stage}/{malformed}");
            assert_eq!(
                fs::read(fixture.paths.config_dir.join("preferences.json")).unwrap(),
                before,
                "{stage}/{malformed}",
            );

            assert!(!fixture.paths.control_socket.exists());
        }
    }
}

#[test]
fn deadline_expiring_during_set_encoding_sends_no_bytes() {
    struct DelayedSet {
        request: AutomationRequestEnvelope,
        deadline: Instant,
    }
    impl Serialize for DelayedSet {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            thread::sleep(
                self.deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(10),
            );
            self.request.serialize(serializer)
        }
    }
    let fixture = AvailabilityFixture::new();
    let before = fs::read(fixture.paths.config_dir.join("preferences.json")).unwrap();
    let server = fixture.serve(|_, _| {});
    let deadline = Instant::now() + Duration::from_millis(50);
    availability_timeout(deadline, CLIENT_TIMEOUT).unwrap();
    let request = DelayedSet {
        request: availability_envelope(
            "request",
            Some(DomainRequest {
                instance_id: "admitted-instance".into(),
                operation_id: crate::cli::new_operation_id(),
                action: DomainAction::PreferencesSet {
                    patch: PreferencePatch {
                        show_status_indicators: Some(true),
                        ..Default::default()
                    },
                    expected_revision: Some(7),
                },
            }),
            None,
        ),
        deadline,
    };
    let mut submitted_bytes = 0;
    let result = availability_send_until(
        &fixture.paths.control_socket,
        &request,
        deadline,
        MAX_AUTOMATION_REPLY_BYTES,
        &mut submitted_bytes,
        |frame, request| decode_automation_reply(frame, &request.request),
    );
    assert!(result.is_err());
    assert_eq!(submitted_bytes, 0);
    assert!(server.finish().is_empty());
    assert_eq!(
        fs::read(fixture.paths.config_dir.join("preferences.json")).unwrap(),
        before,
    );
}
#[test]
fn lost_availability_ack_retains_identity_without_resending_mutation() {
    use std::io::{BufRead, Write};
    use std::os::unix::fs::PermissionsExt;
    let directory = Path::new("/tmp").join(format!("ha{}", crate::cli::new_operation_id()));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, Permissions::from_mode(0o700)).unwrap();
    let path = directory.join("control.sock");
    let listener = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, Permissions::from_mode(SOCKET_MODE)).unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut bytes = Vec::new();
        std::io::BufReader::new(stream)
            .read_until(b'\n', &mut bytes)
            .unwrap();
        let envelope: AutomationRequestEnvelope = serde_json::from_slice(&bytes).unwrap();
        let request = envelope.request.unwrap();
        assert_eq!(request.instance_id, "running_instance");
        assert!(matches!(
            request.action,
            DomainAction::PreferencesSet {
                patch: PreferencePatch {
                    show_status_indicators: Some(false),
                    ..
                },
                expected_revision: Some(7),
            }
        ));
        // The socket accepted the mutation but closed before replying.
        // A status lookup is safe; a second PreferencesSet is not.
        let (mut stream, _) = listener.accept().unwrap();
        bytes.clear();
        std::io::BufReader::new(&mut stream)
            .read_until(b'\n', &mut bytes)
            .unwrap();
        let status: AutomationRequestEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(status.command, "status");
        assert!(status.request.is_none());
        assert_eq!(status.instance_id.as_deref(), Some("running_instance"));
        assert_eq!(
            status.operation_id.as_deref(),
            Some(request.operation_id.as_str())
        );
        let reply = AutomationReplyEnvelope {
            version: PROTOCOL_VERSION,
            kind: "automation".to_owned(),
            ok: false,
            error: Some("operation status unavailable".to_owned()),
            instance_id: None,
            operation: None,
        };
        stream
            .write_all(&serde_json::to_vec(&reply).unwrap())
            .unwrap();
        stream.write_all(b"\n").unwrap();
        request.operation_id
    });
    let outcome = online_availability_set(
        &path,
        "running_instance",
        7,
        flag_patch(false),
        Instant::now() + crate::cli::STARTUP_TIMEOUT,
    )
    .unwrap();
    let sent_id = server.join().unwrap();
    assert_eq!(
        outcome,
        PreferenceCommit::Uncertain {
            instance_id: "running_instance".to_owned(),
            operation_id: sent_id,
            current: None,
        }
    );
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn lost_availability_ack_can_recover_persisted_result_by_original_identity() {
    use std::io::{BufRead, Write};
    use std::os::unix::fs::PermissionsExt;
    let directory = Path::new("/tmp").join(format!("hr{}", crate::cli::new_operation_id()));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, Permissions::from_mode(0o700)).unwrap();
    let path = directory.join("control.sock");
    let listener = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, Permissions::from_mode(SOCKET_MODE)).unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut bytes = Vec::new();
        std::io::BufReader::new(stream)
            .read_until(b'\n', &mut bytes)
            .unwrap();
        let sent: AutomationRequestEnvelope = serde_json::from_slice(&bytes).unwrap();
        let original_id = sent.request.unwrap().operation_id;
        let (mut stream, _) = listener.accept().unwrap();
        bytes.clear();
        std::io::BufReader::new(&mut stream)
            .read_until(b'\n', &mut bytes)
            .unwrap();
        let status: AutomationRequestEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(status.command, "status");
        assert!(status.request.is_none());
        assert_eq!(status.instance_id.as_deref(), Some("running_instance"));
        assert_eq!(status.operation_id.as_deref(), Some(original_id.as_str()));
        let reply = AutomationReplyEnvelope {
            version: PROTOCOL_VERSION,
            kind: "automation".to_owned(),
            ok: true,
            error: None,
            instance_id: Some("running_instance".to_owned()),
            operation: Some(DomainOperation {
                instance_id: "running_instance".to_owned(),
                operation_id: original_id,
                kind: "preferences_set".to_owned(),
                state: crate::automation::DomainOperationState::Applied,
                committed: true,
                native_applied: true,
                result: Some(serde_json::json!({
                    "revision": 8,
                    "persisted": {"show_status_indicators": true}
                })),
                error_code: None,
                error: None,
            }),
        };
        stream
            .write_all(&serde_json::to_vec(&reply).unwrap())
            .unwrap();
        stream.write_all(b"\n").unwrap();
    });
    assert_eq!(
        online_availability_set(
            &path,
            "running_instance",
            7,
            flag_patch(true),
            Instant::now() + crate::cli::STARTUP_TIMEOUT,
        )
        .unwrap(),
        PreferenceCommit::Committed(flag_patch(true))
    );
    server.join().unwrap();
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn accepted_availability_ack_is_not_a_persisted_commit() {
    use std::io::{BufRead, Write};
    use std::os::unix::fs::PermissionsExt;
    let directory = Path::new("/tmp").join(format!("hc{}", crate::cli::new_operation_id()));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, Permissions::from_mode(0o700)).unwrap();
    let path = directory.join("control.sock");
    let listener = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, Permissions::from_mode(SOCKET_MODE)).unwrap();
    let server = thread::spawn(move || {
        for (state, committed) in [
            (crate::automation::DomainOperationState::Accepted, false),
            (crate::automation::DomainOperationState::Failed, false),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            std::io::BufReader::new(&mut stream)
                .read_until(b'\n', &mut bytes)
                .unwrap();
            let envelope: AutomationRequestEnvelope = serde_json::from_slice(&bytes).unwrap();
            let id = envelope
                .request
                .as_ref()
                .map(|request| request.operation_id.as_str())
                .or(envelope.operation_id.as_deref())
                .unwrap();
            let reply = AutomationReplyEnvelope {
                version: PROTOCOL_VERSION,
                kind: "automation".to_owned(),
                ok: true,
                error: None,
                instance_id: Some("running_instance".to_owned()),
                operation: Some(DomainOperation {
                    instance_id: "running_instance".to_owned(),
                    operation_id: id.to_owned(),
                    kind: "preferences_set".to_owned(),
                    state,
                    committed,
                    native_applied: false,
                    result: None,
                    error_code: (state == crate::automation::DomainOperationState::Failed)
                        .then(|| "persist_failed".to_owned()),
                    error: (state == crate::automation::DomainOperationState::Failed)
                        .then(|| "preferences could not be saved".to_owned()),
                }),
            };
            stream
                .write_all(&serde_json::to_vec(&reply).unwrap())
                .unwrap();
            stream.write_all(b"\n").unwrap();
        }
    });
    assert_eq!(
        online_availability_set(
            &path,
            "running_instance",
            7,
            flag_patch(true),
            Instant::now() + crate::cli::STARTUP_TIMEOUT,
        )
        .unwrap_err(),
        "preferences could not be saved"
    );
    server.join().unwrap();
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn committed_set_uses_final_visible_snapshot_and_missing_fields_stay_uncertain() {
    let operation = DomainOperation {
        instance_id: "observed-instance".to_owned(),
        operation_id: "observed-operation".to_owned(),
        kind: "preferences_set".to_owned(),
        state: DomainOperationState::Superseded,
        committed: true,
        native_applied: false,
        result: Some(serde_json::json!({
            "revision": 9, "persisted": {"show_status_indicators":false}
        })),
        error_code: None,
        error: None,
    };
    let fields = ["show_status_indicators".to_owned()];
    assert_eq!(
        availability_set_outcome(
            operation.clone(),
            "observed-instance",
            "observed-operation",
            &fields
        )
        .unwrap(),
        PreferenceCommit::Committed(flag_patch(false)),
    );
    let mut missing = operation;
    missing.result = Some(serde_json::json!({"revision":9,"persisted":{}}));
    assert_eq!(
        availability_set_outcome(missing, "observed-instance", "observed-operation", &fields)
            .unwrap(),
        PreferenceCommit::Uncertain {
            instance_id: "observed-instance".to_owned(),
            operation_id: "observed-operation".to_owned(),
            current: None,
        },
    );
}

#[test]
fn in_process_port_retains_exact_submitted_identity_after_absolute_deadline_without_replay() {
    use crate::automation::{new_automation, PresentationTarget};
    use crate::ui::automation_preferences::{commit_owner_mutation, PreferenceMutation};
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let store = desktop_pet_contracts::ProfileStore::acquire(&config, &root.path().join("state")).unwrap();
    let mut preferences = Preferences::default();
    preferences.set_integration_extensions(store, vec![]);
    preferences.apply_patch(PreferencePatch {
        show_status_indicators: Some(true), ..Default::default()
    }, &config).unwrap();
    let mut revision = 0;
    let mut state = AppState::new();
    state.set_ui_ready();
    let ledger = new_automation(PresentationTarget::from_scene(&state.scene()), None).unwrap();
    state.set_automation(ledger.clone());
    let shared = Arc::new(std::sync::Mutex::new(state));
    let port = LivePreferences::new(shared.clone(), Instant::now() + Duration::from_secs(1));
    let started = Arc::new(std::sync::Barrier::new(2));
    let worker_started = started.clone();
    let worker = thread::spawn(move || {
        worker_started.wait();
        port.apply(flag_patch(false))
    });
    started.wait();
    let take = || {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let mut requests = lock_automation(&ledger).drain_domain_requests();
            if let Some(request) = requests.pop() {
                assert!(requests.is_empty());
                return request;
            }
            assert!(Instant::now() < deadline, "preference request was not submitted");
            thread::sleep(Duration::from_millis(1));
        }
    };
    let get = take();
    assert!(matches!(get.action, DomainAction::PreferencesGet {}));
    lock_automation(&ledger).start_domain_request(&get.operation_id).unwrap();
    lock_automation(&ledger).finish_domain_operation(
        &get.operation_id, DomainOperationState::Applied, false, false,
        Some(serde_json::json!({"revision":revision,"persisted":preferences.snapshot()})),
        None, None,
    ).unwrap();
    let set = take();
    lock_automation(&ledger).start_domain_request(&set.operation_id).unwrap();
    let outcome = worker.join().unwrap().unwrap();
    let PreferenceCommit::Uncertain { instance_id, operation_id, current } = outcome else {
        panic!("a submitted, uncompleted mutation must remain uncertain");
    };
    assert_eq!(operation_id, set.operation_id);
    assert_eq!(instance_id, set.instance_id);
    assert!(current.is_none());
    assert!(lock_automation(&ledger).drain_domain_requests().is_empty());
    let DomainAction::PreferencesSet { patch, expected_revision } = set.action else {
        panic!("expected the single CAS mutation");
    };
    let commit = commit_owner_mutation(
        &mut preferences, &mut revision, PreferenceMutation::Patch(patch),
        expected_revision, &config, &shared, false,
    ).unwrap();
    assert!(commit.saved);
    assert_eq!(revision, 1);
    assert!(!saved_flag(&config).unwrap());
    // Persistence happened after the caller timed out. A missing AppKit owner
    // is an honest postcommit failure, not an invented native-application proof.
    lock_automation(&ledger).finish_domain_operation(
        &operation_id, DomainOperationState::Failed, true, false,
        Some(serde_json::json!({"revision":revision,"persisted":commit.target})),
        Some("native_apply_failed".into()), Some("headless fixture has no AppKit UI".into()),
    ).unwrap();
    let observed = lock_automation(&ledger).domain_status(&instance_id, &operation_id).unwrap();
    assert!(matches!(availability_set_outcome(observed, &instance_id, &operation_id,
        &["show_status_indicators".into()]).unwrap(), PreferenceCommit::Committed(_)));
    let stale = commit_owner_mutation(
        &mut preferences, &mut revision,
        PreferenceMutation::Patch(PreferencePatch { show_status_indicators: Some(true), ..Default::default() }),
        Some(0), &config, &shared, false,
    ).err().unwrap();
    assert_eq!(stale.code, "revision_conflict");
    assert_eq!(revision, 1);
}

#[test]
fn in_process_port_expired_or_gated_admission_never_submits_a_mutation() {
    use crate::automation::{new_automation, PresentationTarget};
    let mut state = AppState::new();
    state.set_ui_ready();
    let ledger = new_automation(PresentationTarget::from_scene(&state.scene()), None).unwrap();
    state.set_automation(ledger.clone());
    let shared = Arc::new(std::sync::Mutex::new(state));
    let expired = LivePreferences::new(shared.clone(), Instant::now());
    assert!(expired.apply(flag_patch(false)).is_err());
    assert!(lock_automation(&ledger).drain_domain_requests().is_empty());
    shared.lock().unwrap().set_migration_gate("migration".into(), "token".into()).unwrap();
    let port = LivePreferences::new(shared, Instant::now() + Duration::from_secs(1));
    assert!(port.submit(DomainAction::PreferencesSet {
        patch: PreferencePatch { show_status_indicators: Some(false), ..Default::default() },
        expected_revision: Some(0),
    }).is_err());
    assert!(lock_automation(&ledger).drain_domain_requests().is_empty());
}
