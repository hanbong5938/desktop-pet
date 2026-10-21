//! Explicit, worker-free composition access to the production domain and owner.
use crate::automation::{lock_automation, new_automation, PresentationTarget};
use crate::preferences::Preferences;
use crate::state::AppState;
use crate::ui::automation_preferences::{
    commit_owner_mutation, PreferenceMutation, PreferenceMutationError,
};
use crate::Composition;
use desktop_pet_contracts::{
    DomainAccess, ObservationSelection, PreferenceAccess, PreferenceCommit, ProfileStore,
    RuntimePort,
};
use desktop_pet_update_coordinator::UpdateContext;
use serde_json::{json, Map, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};

struct Owner {
    preferences: Preferences,
    revision: u64,
}
struct Runtime {
    state: Arc<Mutex<AppState>>,
    wakes: AtomicUsize,
}
impl RuntimePort for Runtime {
    fn mutation_admission(&self) -> Result<(), String> {
        self.state
            .lock()
            .map_err(|_| "native state lock is poisoned")?
            .update_mutation_allowed()
    }
    fn with_domain(&self, operation: &mut dyn FnMut(&mut dyn DomainAccess)) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "native state lock is poisoned")?;
        operation(&mut *state);
        Ok(())
    }
    fn wake_ui(&self) {
        self.wakes.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Clone)]
pub struct HeadlessCore {
    owner: Arc<Mutex<Owner>>,
    state: Arc<Mutex<AppState>>,
    runtime: Arc<Runtime>,
    store: ProfileStore,
}
fn describe_mutation(error: PreferenceMutationError) -> String {
    format!(
        "{} (committed={}): {}",
        error.code, error.committed, error.detail
    )
}

impl HeadlessCore {
    pub fn new(
        composition: Composition,
        store: ProfileStore,
        selection: ObservationSelection,
    ) -> Result<Self, String> {
        composition.validate()?;
        let store = store.with_preferences_validator(Preferences::validate_committed_object);
        store.verify_owner()?;
        composition.configure_profile(store.clone())?;
        let mut preferences = Preferences::load_committed(store.config_dir())?;
        preferences
            .set_integration_extensions(store.clone(), composition.extensions.lifecycle.clone());
        preferences.refresh_owner_snapshot(store.config_dir())?;
        preferences.set_observation_host(selection.clone());
        let mut state = AppState::new();
        state.set_integrations(composition.integrations, composition.provider_labels);
        state.set_lifecycle_extensions(composition.extensions.lifecycle, store.clone());
        let automation = new_automation(PresentationTarget::from_scene(&state.scene()), None)
            .map_err(|error| error.to_string())?;
        let instance_id = lock_automation(&automation).instance().to_owned();
        let executable = std::env::current_exe()
            .map_err(|error| format!("cannot resolve fixture executable: {error}"))?;
        let running = desktop_pet_update_coordinator::executable_identity(&executable)?;
        state.set_update_context(UpdateContext {
            executable: running.path.clone(),
            version: desktop_pet_update_coordinator::identity::VERSION.to_owned(),
            instance_id,
            config_dir: store.config_dir().to_path_buf(),
            state_dir: store.lifecycle_lock().state_dir().to_path_buf(),
            #[cfg(desktop_pet_legacy)]
            host_plugin_config_dir: None,
            #[cfg(desktop_pet_legacy)]
            herdr_socket: store.config_dir().join("herdr.sock"),
            running,
            running_origin: None,
            assets_override: None,
            environment: Default::default(),
            locale: "en".into(),
        })?;
        state.set_automation(automation);
        state.set_ui_ready();
        state.set_observation_selection(selection)?;
        state.restore_selected_integrations_if_ungated();
        let state = Arc::new(Mutex::new(state));
        let runtime = Arc::new(Runtime {
            state: state.clone(),
            wakes: AtomicUsize::new(0),
        });
        Ok(Self {
            owner: Arc::new(Mutex::new(Owner {
                preferences,
                revision: 0,
            })),
            state,
            runtime,
            store,
        })
    }
    /// Explicitly reapply enablement after attaching worker-free adapter runtimes.
    pub fn enable_selected_integrations(&self) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "native state lock is poisoned")?;
        state.update_mutation_allowed()?;
        state.restore_selected_integrations_if_ungated();
        Ok(())
    }
    pub fn runtime(&self) -> Arc<dyn RuntimePort> {
        self.runtime.clone()
    }
    pub fn profile_store(&self) -> ProfileStore {
        self.store.clone()
    }
    pub fn preference_access(&self) -> Arc<dyn PreferenceAccess> {
        Arc::new(self.clone())
    }
    pub fn wake_count(&self) -> usize {
        self.runtime.wakes.load(Ordering::Relaxed)
    }
    pub fn snapshot(&self) -> Result<Value, String> {
        let owner = self
            .owner
            .lock()
            .map_err(|_| "preference owner is poisoned")?;
        let mut effective = owner.preferences.snapshot();
        effective.observation_host = self
            .state
            .lock()
            .map_err(|_| "native state lock is poisoned")?
            .observation_mode();
        if owner.preferences.postcommit_error().is_some() {
            effective.owner.clear();
        }
        Ok(
            json!({"revision": owner.revision, "persisted": owner.preferences.snapshot(),
            "effective": effective, "owner_apply_error": owner.preferences.postcommit_error()}),
        )
    }
    fn commit(
        &self,
        owner: &mut Owner,
        mutation: PreferenceMutation,
        expected: Option<u64>,
    ) -> Result<Value, PreferenceMutationError> {
        let Owner {
            preferences,
            revision,
        } = owner;
        let committed = commit_owner_mutation(
            preferences,
            revision,
            mutation,
            expected,
            self.store.config_dir(),
            &self.state,
            false,
        )?;
        if committed.saved {
            let applied = self
                .state
                .lock()
                .map_err(|_| "native state lock is poisoned".to_owned())
                .and_then(|mut state| {
                    state.apply_observation_preferences(preferences.observation().clone())
                });
            if let Err(detail) = applied {
                return Err(PreferenceMutationError {
                    code: "native_apply_failed",
                    detail,
                    committed: true,
                });
            }
        }
        if let Some(error) = preferences.postcommit_error() {
            return Err(PreferenceMutationError {
                code: "native_apply_failed",
                detail: error.to_owned(),
                committed: true,
            });
        }
        Ok(json!({"revision": *revision, "persisted": committed.target}))
    }
    pub fn compare_and_set(
        &self,
        patch: Map<String, Value>,
        expected_revision: Option<u64>,
    ) -> Result<Value, String> {
        let patch = serde_json::from_value(Value::Object(patch))
            .map_err(|error| format!("invalid_preferences: {error}"))?;
        let mut owner = self
            .owner
            .lock()
            .map_err(|_| "preference owner is poisoned")?;
        self.commit(
            &mut owner,
            PreferenceMutation::Patch(patch),
            expected_revision,
        )
        .map_err(describe_mutation)
    }
    pub fn apply_lifecycle_setting(
        &self,
        provider: Option<&str>,
        key: &str,
        value: bool,
    ) -> Result<Map<String, Value>, String> {
        let mut owner = self
            .owner
            .lock()
            .map_err(|_| "preference owner is poisoned")?;
        let (extensions, selection) = {
            let state = self
                .state
                .lock()
                .map_err(|_| "native state lock is poisoned")?;
            state.update_mutation_allowed()?;
            (
                state.lifecycle_extensions().to_vec(),
                state.observation_mode(),
            )
        };
        if let Some(prepared) = crate::control::prepare_registered_lifecycle(
            &self.store,
            &extensions,
            &selection,
            provider,
            key,
            value,
        )? {
            self.commit(&mut owner, PreferenceMutation::Prepared(prepared), None)
                .map_err(describe_mutation)?;
        }
        crate::control::lifecycle_values(
            &self.store,
            &extensions,
            &owner.preferences.observation_host(),
        )
    }
    pub fn queue_prepare(
        &self,
        operation: &str,
        token: &str,
    ) -> Result<mpsc::Receiver<Result<(), String>>, String> {
        let (sender, receiver) = mpsc::channel();
        self.state
            .lock()
            .map_err(|_| "native state lock is poisoned")?
            .queue_update_prepare(operation.into(), token.into(), sender)?;
        Ok(receiver)
    }
    pub fn take_prepare(&self) -> Option<(String, String)> {
        self.state
            .lock()
            .unwrap()
            .take_update_prepare()
            .map(|request| (request.operation_id, request.token))
    }
    pub fn finish_prepare(&self, operation: &str, token: &str, result: Result<(), String>) -> bool {
        self.state
            .lock()
            .unwrap()
            .finish_update_prepare(operation, token, result)
    }
    pub fn cancel_prepare(&self, operation: &str, token: &str) {
        self.state
            .lock()
            .unwrap()
            .cancel_update_prepare(operation, token);
    }
    pub fn expire_prepare(&self) {
        self.state.lock().unwrap().expire_update_prepare_for_test();
    }
    pub fn claim_stop(&self, operation: &str, token: &str) -> Result<(), String> {
        self.state
            .lock()
            .unwrap()
            .claim_update_stop(operation, token)
    }
    pub fn abort_stop(&self, operation: &str, token: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .abort_update_stop(operation, token)
    }
    pub fn commit_stop(&self, operation: &str, token: &str) -> Result<(), String> {
        self.state
            .lock()
            .unwrap()
            .commit_update_stop(operation, token)
    }
    pub fn set_update_context(&self, context: UpdateContext) -> Result<(), String> {
        self.state.lock().unwrap().set_update_context(context)
    }
    pub fn reconcile_prepare(&self, operation: &str, context: &UpdateContext) -> bool {
        self.state
            .lock()
            .unwrap()
            .reconcile_update_prepare(operation, context)
    }
    pub fn set_migration_gate(&self, operation: &str, token: &str) -> Result<(), String> {
        self.state
            .lock()
            .unwrap()
            .set_migration_gate(operation.into(), token.into())
    }
    pub fn release_migration_gate(&self, operation: &str, token: &str) -> Result<(), String> {
        self.state
            .lock()
            .unwrap()
            .release_migration_gate(operation, token)
    }
    pub fn shutdown(&self) {
        self.state.lock().unwrap().request_shutdown();
    }
    pub fn source_generation(&self, source: &str) -> Option<u64> {
        self.state.lock().unwrap().source_generation(source)
    }
    pub fn session_revision(&self) -> u64 {
        self.state.lock().unwrap().session_revision()
    }
    pub fn session_counts(&self) -> (usize, usize, usize) {
        let state = self.state.lock().unwrap();
        let scene = state.scene();
        (
            scene.sessions,
            scene.connected_sources,
            scene.disconnected_sources,
        )
    }
}
impl PreferenceAccess for HeadlessCore {
    fn read(&self) -> Result<Map<String, Value>, String> {
        let owner = self
            .owner
            .lock()
            .map_err(|_| "preference owner is poisoned")?;
        match serde_json::to_value(owner.preferences.snapshot())
            .map_err(|error| error.to_string())?
        {
            Value::Object(saved) => Ok(saved),
            _ => Err("preference snapshot is not an object".into()),
        }
    }
    fn apply(&self, patch: Map<String, Value>) -> Result<PreferenceCommit, String> {
        let revision = self
            .owner
            .lock()
            .map_err(|_| "preference owner is poisoned")?
            .revision;
        let patch = serde_json::from_value(Value::Object(patch))
            .map_err(|error| format!("invalid_preferences: {error}"))?;
        let mut owner = self
            .owner
            .lock()
            .map_err(|_| "preference owner is poisoned")?;
        let saved = match self.commit(&mut owner, PreferenceMutation::Patch(patch), Some(revision))
        {
            Ok(mut result) => match result.get_mut("persisted").map(Value::take) {
                Some(Value::Object(saved)) => saved,
                _ => return Err("persisted result is missing".into()),
            },
            Err(error) if error.committed => {
                match serde_json::to_value(owner.preferences.snapshot())
                    .map_err(|error| error.to_string())?
                {
                    Value::Object(saved) => saved,
                    _ => return Err("persisted result is not an object".into()),
                }
            }
            Err(error) => return Err(describe_mutation(error)),
        };
        Ok(PreferenceCommit::Committed(saved))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_context_and_domain_share_the_real_automation_instance() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let store = ProfileStore::acquire(&root.join("config"), &root.join("state")).unwrap();
        let core = HeadlessCore::new(
            Composition::default(),
            store.clone(),
            ObservationSelection::Auto,
        )
        .unwrap();
        let state = core.state.lock().unwrap();
        let automation = state.automation().unwrap();
        let instance = lock_automation(&automation).instance().to_owned();
        assert!(!instance.is_empty());
        assert_eq!(state.instance_id(), instance);
        let context = state.update_context().unwrap();
        assert_eq!(context.instance_id, instance);
        assert_eq!(context.config_dir, store.config_dir());
        assert_eq!(context.state_dir, store.lifecycle_lock().state_dir());
        assert_eq!(
            context.running,
            desktop_pet_update_coordinator::executable_identity(&std::env::current_exe().unwrap())
                .unwrap()
        );
        assert_eq!(
            context.observation_mode().unwrap(),
            ObservationSelection::Auto
        );
        drop(state);
        core.runtime()
            .with_domain(&mut |domain| assert_eq!(domain.instance_id(), instance))
            .unwrap();
    }
}
