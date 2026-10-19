use desktop_pet_contracts::{ObservationSelection, ProfileStore};
use desktop_pet_core::{test_support::HeadlessCore, Composition};
use desktop_pet_herdr::{test_support::{attach_runtime, seed_machines, FixtureEndpoint, ManagerFence}, HerdrIntegration};
use serde_json::{json, Map, Value};
use std::sync::Arc;

struct Fixture {
    _root: tempfile::TempDir,
    core: HeadlessCore,
    herdr: Arc<HerdrIntegration>,
    endpoint: FixtureEndpoint,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let physical_root = root.path().canonicalize().unwrap();
        let config = physical_root.join("config");
        let state = physical_root.join("state");
        let store = ProfileStore::acquire(&config, &state).unwrap();
        let herdr = HerdrIntegration::new();
        let mut composition = Composition::default();
        composition.integrations.push(herdr.clone());
        composition.provider_labels.insert("herdr".into(), "Herdr".into());
        composition.extensions = herdr.extensions();
        let configured = herdr.clone();
        composition.profile_configurators.push(Arc::new(move |store| configured.configure_profile(store)));
        let core = HeadlessCore::new(composition, store, ObservationSelection::Auto).unwrap();
        attach_runtime(&herdr, core.runtime()).unwrap();
        core.enable_selected_integrations().unwrap();
        let endpoint = FixtureEndpoint::new(&herdr, root.path().join("herdr.sock"));
        Self { _root: root, core, herdr, endpoint }
    }
    fn observation(&self, patch: Value) -> Result<Value, String> {
        let handler = self.herdr.extensions().control.into_iter().find(|handler| handler.kind() == "herdr-observation").unwrap();
        let frame = serde_json::to_vec(&json!({"kind":"herdr-observation","command":"set","patch":patch})).unwrap();
        let port = self.core.preference_access();
        serde_json::from_slice(&handler.handle_frame(&frame, &*port)?).map_err(|error| error.to_string())
    }
}
fn patch(value: Value) -> Map<String, Value> { value.as_object().unwrap().clone() }

fn records() -> Vec<desktop_pet_contracts::AgentRecord> {
    vec![desktop_pet_contracts::AgentRecord {
        terminal_id: "terminal".into(), pane_id: "pane".into(),
        status: desktop_pet_contracts::AgentStatus::Working,
        metadata: desktop_pet_contracts::SessionMetadata::default(),
        outcome_authoritative: false, outcome: None,
    }]
}

#[test]
fn precise_prepare_ungating_restores_new_publications_but_never_old_fences() {
    for cut in ["failure", "lost-receiver", "cancel-before", "cancel-after", "expiry", "expiry-after"] {
        let mut fixture = Fixture::new();
        let generation = fixture.endpoint.publish(records()).unwrap();
        assert_eq!(fixture.core.session_counts().0, 1);
        let old_fence = ManagerFence::capture(&fixture.herdr).unwrap();
        let receiver = fixture.core.queue_prepare("prepare", "token").unwrap();
        assert!(fixture.endpoint.publish(vec![]).is_err(), "{cut}");
        assert!(old_fence.offer().is_err());
        fixture.core.cancel_prepare("prepare", "wrong");
        assert!(fixture.endpoint.publish(vec![]).is_err());
        match cut {
            "cancel-before" => fixture.core.cancel_prepare("prepare", "token"),
            "expiry" => fixture.core.expire_prepare(),
            "failure" => {
                fixture.core.take_prepare().unwrap();
                assert!(!fixture.core.finish_prepare("prepare", "token", Err("native preparation failed".into())));
            }
            "lost-receiver" => {
                drop(receiver);
                fixture.core.take_prepare().unwrap();
                assert!(!fixture.core.finish_prepare("prepare", "token", Ok(())));
            }
            "cancel-after" | "expiry-after" => {
                fixture.core.take_prepare().unwrap();
                assert!(fixture.core.finish_prepare("prepare", "token", Ok(())));
                receiver.recv().unwrap().unwrap();
                if cut == "expiry-after" {
                    fixture.core.expire_prepare();
                } else {
                    fixture.core.cancel_prepare("prepare", "token");
                }
            }
            _ => unreachable!(),
        }
        let restored = fixture.endpoint.publish(records()).unwrap();
        assert_eq!(fixture.core.session_counts().0, 1);
        assert!(restored > generation, "{cut}");
        assert!(old_fence.offer().is_err(), "old manager authority must remain stale: {cut}");
        ManagerFence::capture(&fixture.herdr).unwrap().offer().unwrap();
    }
}

#[test]
fn claimed_stop_and_shutdown_never_reenable_herdr() {
    for shutdown in [false, true] {
        let mut fixture = Fixture::new();
        fixture.endpoint.publish(vec![]).unwrap();
        let receiver = fixture.core.queue_prepare("prepare", "token").unwrap();
        fixture.core.take_prepare().unwrap();
        assert!(fixture.core.finish_prepare("prepare", "token", Ok(())));
        receiver.recv().unwrap().unwrap();
        fixture.core.claim_stop("prepare", "token").unwrap();
        if shutdown { fixture.core.shutdown(); }
        fixture.core.cancel_prepare("prepare", "token");
        fixture.core.expire_prepare();
        assert!(fixture.endpoint.publish(vec![]).is_err());
    }
}

#[test]
fn lifecycle_observation_and_common_cas_share_one_revision_and_persisted_projection() {
    let fixture = Fixture::new();
    let initial = fixture.core.snapshot().unwrap();
    let old = initial["revision"].as_u64().unwrap();
    fixture.core.apply_lifecycle_setting(Some("herdr"), "observation_local", false).unwrap();
    let lifecycle = fixture.core.snapshot().unwrap();
    assert_eq!(lifecycle["revision"], old + 1);
    assert_eq!(lifecycle["persisted"]["observation_local"], false);
    assert_eq!(lifecycle["effective"]["observation_local"], false);
    assert!(fixture.core.compare_and_set(patch(json!({"language":"en"})), Some(old)).unwrap_err().contains("revision_conflict"));
    let reply = fixture.observation(json!({"observation_local":true})).unwrap();
    assert_eq!(reply["ok"], true);
    let observation = fixture.core.snapshot().unwrap();
    assert_eq!(observation["revision"], old + 2);
    assert_eq!(observation["persisted"]["observation_local"], true);
    assert!(fixture.core.compare_and_set(patch(json!({"language":"en"})), Some(old + 1)).unwrap_err().contains("revision_conflict"));
    fixture.core.compare_and_set(patch(json!({"language":"en"})), Some(old + 2)).unwrap();
    assert_eq!(fixture.core.snapshot().unwrap()["revision"], old + 3);
    let disk: Value = serde_json::from_slice(&std::fs::read(fixture.core.profile_store().config_dir().join("preferences.json")).unwrap()).unwrap();
    assert_eq!(disk["language"], "en");
    fixture.observation(json!({})).unwrap();
    assert_eq!(fixture.core.snapshot().unwrap()["revision"], old + 3);
    for forbidden in [json!({"language":"ko"}), json!({"plugin_manager_installed":true})] {
        assert!(fixture.observation(forbidden).is_err());
        assert_eq!(fixture.core.snapshot().unwrap()["revision"], old + 3);
    }
}

#[test]
fn real_owner_preserves_large_machine_selection_and_lifecycle_delta() {
    let fixture = Fixture::new();
    let machines: Vec<String> = (0..64).map(|index| format!("{index:02}{}", "x".repeat(4080))).collect();
    seed_machines(&fixture.herdr, machines.iter().map(|id| (id.clone(), true)).collect()).unwrap();
    let handler = fixture.herdr.extensions().control.into_iter()
        .find(|handler| handler.kind() == "herdr-observation").unwrap();
    let mut frame = serde_json::to_vec(&json!({
        "kind":"herdr-observation", "command":"set", "patch":{"observation_machines":machines}
    })).unwrap();
    assert!(frame.len() < 256 * 1024);
    // Match the transport's inclusive terminator budget, without shrinking IDs.
    frame.resize(256 * 1024 - 1, b' ');
    let port = fixture.core.preference_access();
    let reply: Value = serde_json::from_slice(&handler.handle_frame(&frame, &*port).unwrap()).unwrap();
    assert_eq!(reply["ok"], true);
    let saved = fixture.core.snapshot().unwrap();
    assert_eq!(saved["persisted"]["observation_machines"].as_array().unwrap().len(), 64);
    let revision = saved["revision"].as_u64().unwrap();
    // Catalog disappearance is not authority to erase already selected IDs.
    seed_machines(&fixture.herdr, vec![(machines[0].clone(), true)]).unwrap();
    fixture.core.apply_lifecycle_setting(Some("herdr"), &format!("observation_machine:{}", machines[0]), false).unwrap();
    let updated = fixture.core.snapshot().unwrap();
    assert_eq!(updated["revision"], revision + 1);
    assert_eq!(updated["persisted"]["observation_machines"].as_array().unwrap().len(), 63);
    assert!(fixture.core.compare_and_set(patch(json!({"observation_remote":false})), Some(revision)).unwrap_err().contains("revision_conflict"));
}

#[test]
fn prepare_cancel_preserves_current_provider_selection_and_migration_gate() {
    let mut fixture = Fixture::new();
    fixture.endpoint.publish(vec![]).unwrap();
    fixture.core.compare_and_set(patch(json!({"observation_host":"tern"})), None).unwrap();
    let _receiver = fixture.core.queue_prepare("prepare", "token").unwrap();
    fixture.core.cancel_prepare("prepare", "token");
    assert!(fixture.endpoint.publish(vec![]).is_err(), "unselected Herdr must remain disabled");
    fixture.core.compare_and_set(patch(json!({"observation_host":"auto"})), None).unwrap();
    fixture.endpoint.publish(vec![]).unwrap();
    fixture.core.set_migration_gate("migration", "secret").unwrap();
    fixture.core.cancel_prepare("prepare", "token");
    assert!(fixture.endpoint.publish(vec![]).is_err());
    assert!(fixture.core.release_migration_gate("migration", "wrong").is_err());
    assert!(fixture.endpoint.publish(vec![]).is_err());
    fixture.core.release_migration_gate("migration", "secret").unwrap();
    fixture.endpoint.publish(vec![]).unwrap();
}

#[test]
fn exact_context_reconciliation_restores_publication_without_reanimating_old_authority() {
    use desktop_pet_update_coordinator::{ExecutableIdentity, UpdateContext};
    let mut fixture = Fixture::new();
    let store = fixture.core.profile_store();
    let image = ExecutableIdentity { path: store.config_dir().join("pet"), sha256: "a".repeat(64) };
    let context = UpdateContext {
        executable: image.path.clone(), version: "0.4.8".into(), instance_id: "fixture".into(),
        config_dir: store.config_dir().to_path_buf(),
        state_dir: store.lifecycle_lock().state_dir().to_path_buf(),
        #[cfg(desktop_pet_legacy)]
        host_plugin_config_dir: None,
        #[cfg(desktop_pet_legacy)]
        herdr_socket: store.config_dir().join("herdr.sock"),
        running: image, running_origin: None, assets_override: None,
        environment: Default::default(), locale: "en".into(),
    };
    fixture.core.set_update_context(context.clone()).unwrap();
    let initial = fixture.endpoint.publish(vec![]).unwrap();
    let fence = ManagerFence::capture(&fixture.herdr).unwrap();
    let receiver = fixture.core.queue_prepare("prepare", "token").unwrap();
    fixture.core.take_prepare().unwrap();
    assert!(fixture.core.finish_prepare("prepare", "token", Ok(())));
    receiver.recv().unwrap().unwrap();
    let mut wrong = context.clone();
    wrong.running.sha256 = "b".repeat(64);
    assert!(!fixture.core.reconcile_prepare("prepare", &wrong));
    assert!(!fixture.core.reconcile_prepare("other", &context));
    assert!(fixture.endpoint.publish(vec![]).is_err());
    // AppState normalizes the selected observation context before storing it.
    let mut exact = context;
    exact.set_observation_selection(&ObservationSelection::Auto).unwrap();
    assert!(fixture.core.reconcile_prepare("prepare", &exact));
    assert!(fixture.endpoint.publish(vec![]).unwrap() > initial);
    assert!(fence.offer().is_err());
    assert!(!fixture.core.reconcile_prepare("prepare", &exact));
}

#[test]
fn failed_stop_reply_thaws_preference_owner_without_reenabling_retired_integrations() {
    let mut fixture = Fixture::new();
    fixture.endpoint.publish(records()).unwrap();
    let fence = ManagerFence::capture(&fixture.herdr).unwrap();
    let receiver = fixture.core.queue_prepare("prepare", "token").unwrap();
    fixture.core.take_prepare().unwrap();
    assert!(fixture.core.finish_prepare("prepare", "token", Ok(())));
    receiver.recv().unwrap().unwrap();
    fixture.core.claim_stop("prepare", "token").unwrap();
    fixture.core.cancel_prepare("prepare", "token");
    assert!(fixture.core.compare_and_set(patch(json!({"language":"en"})), Some(0)).is_err());
    assert!(!fixture.core.abort_stop("prepare", "wrong"));
    assert!(fixture.core.abort_stop("prepare", "token"));
    fixture.core.compare_and_set(patch(json!({"language":"en"})), Some(0)).unwrap();
    assert_eq!(fixture.core.snapshot().unwrap()["revision"], 1);
    assert!(fixture.endpoint.publish(records()).is_err());
    assert!(fence.offer().is_err());
    assert!(fixture.core.commit_stop("prepare", "token").is_err());
}

#[test]
fn headless_composition_exposes_a_real_bound_domain_instance() {
    let fixture = Fixture::new();
    let mut instance = String::new();
    fixture.core.runtime().with_domain(&mut |domain| {
        instance = domain.instance_id().to_owned();
    }).unwrap();
    assert_eq!(instance.len(), 32);
    assert!(instance.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let fence = ManagerFence::capture(&fixture.herdr).unwrap();
    fence.offer().unwrap();
}
