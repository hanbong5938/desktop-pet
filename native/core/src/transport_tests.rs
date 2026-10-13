use crate::socket::read_with_deadline;
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

fn enlarge_socket_buffers(stream: &UnixStream) {
    let size: libc::c_int = 64 * 1024;
    for option in [libc::SO_SNDBUF, libc::SO_RCVBUF] {
        let result = unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                option,
                (&size as *const libc::c_int).cast(),
                std::mem::size_of_val(&size) as libc::socklen_t,
            )
        };
        assert_eq!(
            result,
            0,
            "setsockopt({option}) failed: {}",
            io::Error::last_os_error()
        );
    }
}

fn is_blocking(stream: &UnixStream) -> bool {
    let flags = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFL) };
    assert!(
        flags >= 0,
        "fcntl(F_GETFL) failed: {}",
        io::Error::last_os_error()
    );
    flags & libc::O_NONBLOCK == 0
}

#[test]
fn drains_buffered_payload_after_peer_close_until_eof() {
    let (mut reader, mut writer) = UnixStream::pair().expect("UnixStream::pair");
    enlarge_socket_buffers(&reader);
    enlarge_socket_buffers(&writer);
    reader.set_nonblocking(false).expect("reader blocking mode");
    let payload: Vec<u8> = (0..20_000).map(|index| (index % 251) as u8).collect();
    writer.write_all(&payload).expect("preload payload");
    drop(writer);
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut received = Vec::with_capacity(payload.len());
    let mut chunk = [0u8; 8192];
    loop {
        let count =
            read_with_deadline(&mut reader, &mut chunk, deadline).expect("read payload or EOF");
        if count == 0 {
            break;
        }
        received.extend_from_slice(&chunk[..count]);
    }
    assert_eq!(received, payload);
    assert!(
        is_blocking(&reader),
        "successful read changed blocking mode"
    );
}

#[test]
fn timeout_is_bounded_and_restores_original_blocking_mode() {
    let (mut reader, _writer) = UnixStream::pair().expect("UnixStream::pair");
    reader.set_nonblocking(false).expect("reader blocking mode");
    assert!(is_blocking(&reader));
    let started = Instant::now();
    let error = read_with_deadline(
        &mut reader,
        &mut [0u8; 1],
        started + Duration::from_millis(120),
    )
    .expect_err("idle peer reaches deadline");
    let elapsed = started.elapsed();
    assert!(matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ));
    assert!(
        elapsed >= Duration::from_millis(60),
        "read returned early: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_millis(500),
        "read exceeded deadline: {elapsed:?}"
    );
    assert!(is_blocking(&reader), "timed-out read changed blocking mode");
}

#[test]
fn empty_buffer_returns_without_waiting() {
    let (mut reader, _writer) = UnixStream::pair().expect("UnixStream::pair");
    reader.set_nonblocking(false).expect("reader blocking mode");
    let started = Instant::now();
    assert_eq!(
        read_with_deadline(
            &mut reader,
            &mut [],
            Instant::now() + Duration::from_millis(250)
        )
        .expect("empty read"),
        0
    );
    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(is_blocking(&reader), "empty read changed blocking mode");
}

/// Deterministic registered integration for domain/dispatch races. Host wire,
/// endpoint, epoch and receipt validation fixtures live with their adapters.
pub(crate) mod fixture {
    use crate::session_view::SessionListOptions;
    use crate::state::AppState;
    use desktop_pet_contracts::*;
    use parking_lot::Mutex;
    use std::collections::BTreeMap;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Arc, Mutex as DomainMutex};
    use std::time::Instant;

    pub(crate) struct TestRuntime(pub Arc<DomainMutex<AppState>>);
    impl RuntimePort for TestRuntime {
        fn with_domain(
            &self,
            operation: &mut dyn FnMut(&mut dyn DomainAccess),
        ) -> Result<(), String> {
            let mut state = self.0.lock().map_err(|_| "poisoned domain".to_owned())?;
            operation(&mut *state);
            Ok(())
        }
        fn wake_ui(&self) {}
    }

    struct PendingPrompt {
        key: SessionKey,
        source: String,
        text: String,
        offered: bool,
        reply: Sender<Result<PromptReceipt, PromptError>>,
    }

    struct Registry {
        prompt: Option<PendingPrompt>,
        removal: Option<Sender<Result<(), WorktreeRemoveError>>>,
        sources: BTreeMap<String, u64>,
        expired: Vec<(String, u64)>,
        submissions: usize,
    }

    pub(crate) struct TestIntegration {
        id: &'static str,
        runtime: Mutex<Option<Arc<dyn RuntimePort>>>,
        registry: Mutex<Registry>,
        before_admission: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    impl TestIntegration {
        pub(crate) fn new(id: &'static str) -> Self {
            Self {
                id,
                runtime: Mutex::new(None),
                registry: Mutex::new(Registry {
                    prompt: None,
                    removal: None,
                    sources: BTreeMap::new(),
                    expired: Vec::new(),
                    submissions: 0,
                }),
                before_admission: Mutex::new(None),
            }
        }
        pub(crate) fn before_admission(&self, operation: impl FnOnce() + Send + 'static) {
            *self.before_admission.lock() = Some(Box::new(operation));
        }
        fn before(&self) {
            let operation = self.before_admission.lock().take();
            if let Some(operation) = operation {
                operation();
            }
        }
        fn runtime(&self) -> Result<Arc<dyn RuntimePort>, String> {
            self.runtime
                .lock()
                .clone()
                .ok_or_else(|| "integration not started".to_owned())
        }
        pub(crate) fn remember_source(&self, source: &str, generation: u64) {
            self.registry
                .lock()
                .sources
                .insert(source.to_owned(), generation);
        }
        pub(crate) fn expire(&self, source: &str, generation: u64) {
            self.registry
                .lock()
                .expired
                .push((source.to_owned(), generation));
        }
        pub(crate) fn offer(&self) {
            self.registry.lock().prompt.as_mut().unwrap().offered = true;
        }
        pub(crate) fn queued(&self) -> Option<(SessionKey, String)> {
            self.registry
                .lock()
                .prompt
                .as_ref()
                .map(|pending| (pending.key.clone(), pending.text.clone()))
        }
        pub(crate) fn submissions(&self) -> usize {
            self.registry.lock().submissions
        }
        pub(crate) fn complete(&self, result: Result<PromptReceipt, PromptError>) {
            self.registry
                .lock()
                .prompt
                .take()
                .unwrap()
                .reply
                .send(result)
                .unwrap();
        }
        pub(crate) fn disconnect(&self) {
            self.registry.lock().prompt.take();
        }
        pub(crate) fn complete_removal(&self, result: Result<(), WorktreeRemoveError>) {
            self.registry
                .lock()
                .removal
                .take()
                .unwrap()
                .send(result)
                .unwrap();
        }
        fn retire_with(&self, domain: &mut dyn DomainAccess, queued_error: PromptError) {
            let mut registry = self.registry.lock();
            if let Some(pending) = registry.prompt.take() {
                let _ = pending.reply.send(Err(if pending.offered {
                    PromptError::UnknownDelivery
                } else {
                    queued_error
                }));
            }
            if let Some(reply) = registry.removal.take() {
                let _ = reply.send(Err(WorktreeRemoveError::UnknownDelivery));
            }
            let sources = std::mem::take(&mut registry.sources);
            for (source, generation) in sources {
                domain.remove_source(&source, generation);
            }
        }
    }

    impl Integration for TestIntegration {
        fn id(&self) -> &'static str {
            self.id
        }
        fn start(&self, runtime: Arc<dyn RuntimePort>) -> Result<(), String> {
            *self.runtime.lock() = Some(runtime);
            Ok(())
        }
        fn tick(&self, _now: Instant) {}
        fn set_enabled(&self, enabled: bool, domain: &mut dyn DomainAccess) {
            if !enabled {
                self.retire_with(domain, PromptError::StaleTarget);
            }
        }
        fn retire(&self, domain: &mut dyn DomainAccess) {
            self.retire_with(domain, PromptError::Offline);
        }
        fn shutdown(&self) {}
        fn reclaim_expired_sources(&self, _now: Instant, domain: &mut dyn DomainAccess) -> usize {
            let mut registry = self.registry.lock();
            for index in 0..registry.expired.len() {
                let (source, generation) = &registry.expired[index];
                if registry
                    .prompt
                    .as_ref()
                    .is_some_and(|pending| &pending.source == source)
                {
                    continue;
                }
                if domain.remove_offline_source(source, *generation) {
                    let (source, _) = registry.expired.remove(index);
                    registry.sources.remove(&source);
                    return 1;
                }
            }
            0
        }
        fn submit_prompt(
            &self,
            key: SessionKey,
            text: String,
        ) -> Result<Receiver<Result<PromptReceipt, PromptError>>, PromptError> {
            self.before();
            let runtime = self.runtime().map_err(PromptError::Other)?;
            let (reply, receipt) = mpsc::channel();
            let mut queued = Some((key, text, reply));
            let mut admission = Err(PromptError::Offline);
            runtime
                .with_domain(&mut |domain| {
                    admission = (|| {
                        domain.mutation_allowed().map_err(PromptError::Other)?;
                        let (key, text, reply) = queued.take().unwrap();
                        let (target, provider) = domain.prompt_route(&key)?;
                        if provider != self.id {
                            return Err(PromptError::StaleTarget);
                        }
                        let mut registry = self.registry.lock();
                        if registry.prompt.is_some() {
                            return Err(PromptError::Busy);
                        }
                        registry.submissions += 1;
                        registry.prompt = Some(PendingPrompt {
                            key,
                            source: target.source,
                            text,
                            offered: false,
                            reply,
                        });
                        Ok(())
                    })();
                })
                .map_err(PromptError::Other)?;
            admission?;
            Ok(receipt)
        }
        fn submit_worktree_remove(
            &self,
            target: WorktreeRemoveTarget,
        ) -> Result<Receiver<Result<(), WorktreeRemoveError>>, WorktreeRemoveError> {
            self.before();
            let runtime = self.runtime().map_err(WorktreeRemoveError::Other)?;
            let (reply, receipt) = mpsc::channel();
            let mut reply = Some(reply);
            let mut admission = Err(WorktreeRemoveError::Offline);
            runtime
                .with_domain(&mut |domain| {
                    admission = (|| {
                        domain
                            .mutation_allowed()
                            .map_err(WorktreeRemoveError::Other)?;
                        if domain.worktree_remove_target(&target.key)? != target {
                            return Err(WorktreeRemoveError::StaleTarget);
                        }
                        if domain
                            .source_descriptor(&target.source)
                            .map(|descriptor| descriptor.provider.as_str())
                            != Some(self.id)
                        {
                            return Err(WorktreeRemoveError::StaleTarget);
                        }
                        let mut registry = self.registry.lock();
                        if registry.removal.is_some() {
                            return Err(WorktreeRemoveError::Busy);
                        }
                        registry.removal = reply.take();
                        Ok(())
                    })();
                })
                .map_err(WorktreeRemoveError::Other)?;
            admission?;
            Ok(receipt)
        }
    }

    pub(crate) fn descriptor(id: &str, provider: &str) -> SourceDescriptor {
        SourceDescriptor {
            id: id.into(),
            provider: provider.into(),
            label: "Fixture source".into(),
            locality: SourceLocality::Local,
            capabilities: SourceCapabilities {
                prompt: true,
                remove_worktree: true,
            },
        }
    }
    pub(crate) fn record() -> AgentRecord {
        AgentRecord {
            terminal_id: "same-terminal".into(),
            pane_id: "same-pane".into(),
            status: AgentStatus::Working,
            metadata: SessionMetadata::default(),
            outcome_authoritative: false,
            outcome: None,
        }
    }
    pub(crate) fn publish(
        state: &mut AppState,
        source: &str,
        generation: u64,
        row: &AgentRecord,
    ) -> SessionKey {
        assert!(state.publish_source_snapshot(
            source,
            generation,
            [row],
            SourceCounts {
                sessions: 1,
                working: 1,
                ..SourceCounts::default()
            },
            Arc::from(vec![row.pane_id.clone()]),
            &[],
            Instant::now()
        ));
        state
            .session_snapshot(&SessionListOptions::default(), None)
            .rows
            .into_iter()
            .find(|view| {
                view.key.terminal_id == row.terminal_id
                    && state
                        .prompt_target(&view.key)
                        .is_ok_and(|target| target.source == source)
            })
            .unwrap()
            .key
    }
    pub(crate) fn setup(
        id: &'static str,
    ) -> (Arc<DomainMutex<AppState>>, Arc<TestIntegration>, SessionKey) {
        let shared = Arc::new(DomainMutex::new(AppState::new()));
        let integration = Arc::new(TestIntegration::new(id));
        let registered: Arc<dyn Integration> = integration.clone();
        shared.lock().unwrap().set_integrations(
            vec![registered],
            BTreeMap::from([(id.into(), "Fixture".into())]),
        );
        integration
            .start(Arc::new(TestRuntime(Arc::clone(&shared))))
            .unwrap();
        let mut state = shared.lock().unwrap();
        assert!(state.begin_source(descriptor("opaque-source", id), 1));
        integration.remember_source("opaque-source", 1);
        let key = publish(&mut state, "opaque-source", 1, &record());
        drop(state);
        (shared, integration, key)
    }
}

mod operations {
    use super::fixture::*;
    use crate::operations::{PromptSender, WorktreeRemoveSender};
    use desktop_pet_contracts::*;
    use std::sync::{mpsc, Arc};

    #[test]
    fn shared_prompt_sender_preserves_original_text_origin_singleflight_and_receipt_meaning() {
        for receipt in [PromptReceipt::Acknowledged, PromptReceipt::Accepted] {
            let (shared, integration, key) = setup("opaque-provider");
            let mut sender = PromptSender::new(Arc::clone(&shared));
            let text = "  original\nsecond line\n ";
            let origin = RequestOrigin::Cli {
                operation_id: "request-id".into(),
            };
            sender
                .submit(key.clone(), text.into(), origin.clone())
                .unwrap();
            assert_eq!(integration.queued(), Some((key.clone(), text.into())));
            assert_eq!(
                sender.submit(key, "another".into(), RequestOrigin::Gui),
                Err(PromptError::Busy)
            );
            assert!(sender.try_result().is_none());
            assert!(
                shared.try_lock().is_ok(),
                "receipt wait retained domain lock"
            );
            integration.complete(Ok(receipt));
            let result = sender.try_result().unwrap();
            assert_eq!(result.result, Ok(receipt));
            assert_eq!(result.submission.text, text);
            assert_eq!(result.submission.origin, origin);
            assert!(!sender.is_pending());
            assert!(sender.try_result().is_none());
            assert!(shared.lock().unwrap().take_completions().is_empty());
        }
    }

    #[test]
    fn disconnected_adapter_receipt_is_unknown_delivery_and_never_replayed() {
        let (shared, integration, key) = setup("opaque-provider");
        let mut sender = PromptSender::new(shared);
        sender
            .submit(key, "one ask".into(), RequestOrigin::Gui)
            .unwrap();
        integration.disconnect();
        let result = sender.try_result().unwrap();
        assert_eq!(result.result, Err(PromptError::UnknownDelivery));
        assert_eq!(result.submission.text, "one ask");
        assert_eq!(integration.submissions(), 1);
        assert!(sender.try_result().is_none());
    }

    #[test]
    fn adapter_final_admission_serializes_with_prepare_after_sender_preflight() {
        let (shared, integration, key) = setup("opaque-provider");
        shared.lock().unwrap().set_ui_ready();
        let race = Arc::clone(&shared);
        let (reply, _receiver) = mpsc::channel();
        integration.before_admission(move || {
            race.lock()
                .unwrap()
                .queue_update_prepare("update".into(), "secret".into(), reply)
                .unwrap();
        });
        let mut sender = PromptSender::new(Arc::clone(&shared));
        assert!(matches!(
            sender.submit(key, "not admitted".into(), RequestOrigin::Gui),
            Err(PromptError::Other(_))
        ));
        assert_eq!(integration.submissions(), 0);
        assert!(!sender.is_pending());
    }

    #[test]
    fn selection_and_prepare_retire_queued_and_offered_work_without_resending() {
        for prepare in [false, true] {
            for offered in [false, true] {
                let (shared, integration, key) = setup("opaque-provider");
                let mut sender = PromptSender::new(Arc::clone(&shared));
                sender
                    .submit(key.clone(), "one ask".into(), RequestOrigin::Gui)
                    .unwrap();
                if offered {
                    integration.offer();
                }
                let mut state = shared.lock().unwrap();
                if prepare {
                    state.set_ui_ready();
                    let (reply, _receiver) = mpsc::channel();
                    state
                        .queue_update_prepare("update".into(), "secret".into(), reply)
                        .unwrap();
                } else {
                    state
                        .set_observation_selection(ObservationSelection::Provider("other".into()))
                        .unwrap();
                }
                drop(state);
                assert_eq!(
                    sender.try_result().unwrap().result,
                    Err(if offered {
                        PromptError::UnknownDelivery
                    } else if prepare {
                        PromptError::Offline
                    } else {
                        PromptError::StaleTarget
                    })
                );
                let mut state = shared.lock().unwrap();
                if prepare {
                    state.cancel_update_prepare("update", "secret");
                }
                state
                    .set_observation_selection(ObservationSelection::Auto)
                    .unwrap();
                assert!(!state.begin_source(descriptor("opaque-source", "opaque-provider"), 1));
                assert!(state.begin_source(descriptor("opaque-source", "opaque-provider"), 2));
                assert!(matches!(
                    state.prompt_target(&key),
                    Err(PromptError::StaleTarget)
                ));
                assert_eq!(integration.submissions(), 1);
                assert!(integration.queued().is_none());
            }
        }
    }

    #[test]
    fn confirmed_worktree_dispatch_keeps_origin_and_uses_post_admission_baseline() {
        let (shared, integration, key) = setup("opaque-provider");
        let mut row = record();
        row.metadata.workspace_id = Some("workspace".into());
        row.metadata.worktree = Some(Arc::new(WorkspaceWorktreeInfo {
            repo_key: "repo".into(),
            repo_name: "Project".into(),
            repo_root: "/repo".into(),
            checkout_path: "/checkout".into(),
            is_linked_worktree: true,
            pane_count: 1,
            tab_count: 1,
        }));
        let target = {
            let mut state = shared.lock().unwrap();
            publish(&mut state, "opaque-source", 1, &row);
            state.worktree_remove_target(&key).unwrap()
        };
        let before = shared
            .lock()
            .unwrap()
            .worktree_snapshot_revision(&target)
            .unwrap();
        let race = Arc::clone(&shared);
        integration.before_admission(move || {
            publish(&mut race.lock().unwrap(), "opaque-source", 1, &row);
        });
        let origin = RequestOrigin::Cli {
            operation_id: "remove-id".into(),
        };
        let mut sender = WorktreeRemoveSender::new(Arc::clone(&shared));
        let baseline = sender.submit(target.clone(), origin.clone()).unwrap();
        assert!(baseline > before);
        assert_eq!(
            sender.submit(target.clone(), origin.clone()),
            Err(WorktreeRemoveError::Busy)
        );
        assert!(sender.try_result().is_none());
        assert!(shared.try_lock().is_ok());
        integration.complete_removal(Ok(()));
        let result = sender.try_result().unwrap();
        assert_eq!(result.target, target);
        assert_eq!(result.origin, origin);
        assert_eq!(result.result, Ok(()));
        assert!(!sender.is_pending());
    }

    #[test]
    fn source_generation_changed_after_preflight_is_never_queued_or_replayed() {
        let (shared, integration, key) = setup("opaque-provider");
        let race = Arc::clone(&shared);
        integration.before_admission(move || {
            let mut state = race.lock().unwrap();
            assert!(state.begin_source(descriptor("opaque-source", "opaque-provider"), 2));
            publish(&mut state, "opaque-source", 2, &record());
        });
        let mut sender = PromptSender::new(shared);
        assert_eq!(
            sender.submit(key, "stale ask".into(), RequestOrigin::Gui),
            Err(PromptError::StaleTarget)
        );
        assert_eq!(integration.submissions(), 0);
        assert!(!sender.is_pending());
        assert!(integration.queued().is_none());
    }

    #[test]
    fn hidden_source_keeps_ingesting_and_restores_same_key_but_cannot_submit() {
        let (shared, integration, key) = setup("opaque-provider");
        let mut state = shared.lock().unwrap();
        assert!(state.set_source_observed("opaque-source", 1, false));
        assert_eq!(state.host_connections()["opaque-provider"], 1);
        assert_eq!(state.scene().sessions, 0);
        let mut row = record();
        row.metadata.title = Some("Updated while hidden".into());
        assert!(state.publish_source_snapshot(
            "opaque-source",
            1,
            [&row],
            SourceCounts {
                sessions: 1,
                working: 1,
                ..SourceCounts::default()
            },
            Arc::from(vec!["same-pane".into()]),
            &[],
            std::time::Instant::now()
        ));
        drop(state);
        let mut sender = PromptSender::new(Arc::clone(&shared));
        assert_eq!(
            sender.submit(key.clone(), "hidden".into(), RequestOrigin::Gui),
            Err(PromptError::StaleTarget)
        );
        let mut state = shared.lock().unwrap();
        assert!(!state.set_source_observed("opaque-source", 2, true));
        assert!(state.set_source_observed("opaque-source", 1, true));
        let view = state.session_view_for_key(&key).unwrap();
        assert_eq!(view.key, key);
        assert_eq!(view.metadata.title.as_deref(), Some("Updated while hidden"));
        assert_eq!(state.scene().sessions, 1);
        drop(state);
        sender
            .submit(key, "visible".into(), RequestOrigin::Gui)
            .unwrap();
        integration.complete(Ok(PromptReceipt::Acknowledged));
        assert_eq!(
            sender.try_result().unwrap().result,
            Ok(PromptReceipt::Acknowledged)
        );
    }
}
