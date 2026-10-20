use crate::automation::{new_automation, PresentationTarget};
use crate::character_service::PackService;
use crate::control::ControlServer;
use crate::lifecycle::{self, Paths};
use crate::preferences::Preferences;
use crate::state::AppState;
use crate::ui;
use crate::Composition;
#[cfg(test)]
use desktop_pet_contracts::OBSERVATION_SELECTION_ENV;
use desktop_pet_contracts::{DomainAccess, ObservationSelection, ProfileStore, RuntimePort};
use desktop_pet_update_coordinator::native_runtime::captured_environment;
use desktop_pet_update_coordinator::protocol::{register_active, start_allowed, unregister_active};
use desktop_pet_update_coordinator::{
    detect_origin, executable_identity, InstallOrigin, UpdateContext,
};
use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::time::Instant;

#[derive(Clone)]
pub struct DaemonConfig {
    pub paths: Paths,
    pub assets: PathBuf,
    pub assets_override: bool,
    pub startup_token: Option<String>,
    pub automatic_start: bool,
    pub observation_mode: ObservationSelection,
    pub composition: Arc<Composition>,
}

pub(crate) struct StateRuntime {
    pub shared: Arc<Mutex<AppState>>,
}

impl RuntimePort for StateRuntime {
    fn mutation_admission(&self) -> Result<(), String> {
        crate::control::shared_mutation_admission(&self.shared)
    }
    fn with_domain(&self, operation: &mut dyn FnMut(&mut dyn DomainAccess)) -> Result<(), String> {
        let mut state = self
            .shared
            .lock()
            .map_err(|_| "desktop-pet domain lock is poisoned".to_owned())?;
        operation(&mut *state);
        Ok(())
    }
    fn wake_ui(&self) {
        ui::wake();
    }
}

struct IntegrationWorkers {
    runtime: Arc<StateRuntime>,
    integrations: Vec<Arc<dyn desktop_pet_contracts::Integration>>,
    stopping: Arc<AtomicBool>,
    tick: Option<JoinHandle<()>>,
}

impl IntegrationWorkers {
    fn start(runtime: Arc<StateRuntime>, composition: &Composition) -> Result<Self, String> {
        let mut workers = Self {
            runtime,
            integrations: Vec::new(),
            stopping: Arc::new(AtomicBool::new(false)),
            tick: None,
        };
        for integration in &composition.integrations {
            workers.integrations.push(integration.clone());
            integration.start(workers.runtime.clone())?;
        }
        let integrations = workers.integrations.clone();
        let lifecycle_extensions = composition.extensions.lifecycle.clone();
        let stopping = workers.stopping.clone();
        let runtime = workers.runtime.clone();
        if !integrations.is_empty() {
            workers.tick = Some(
                thread::Builder::new()
                    .name("desktop-pet-integrations".into())
                    .spawn(move || {
                        while !stopping.load(Ordering::Acquire) {
                            let now = Instant::now();
                            // This is the sole integration clock; ticks can enter RuntimePort
                            // and must run outside the domain lock, independently of IPC traffic.
                            for integration in &integrations {
                                integration.tick(now);
                            }
                            let exit = if lifecycle_extensions.is_empty() {
                                false
                            } else {
                                let selection = runtime
                                    .shared
                                    .lock()
                                    .ok()
                                    .map(|state| state.observation_mode());
                                selection.is_some_and(|selection| {
                                    lifecycle_extensions
                                        .iter()
                                        .any(|extension| extension.should_exit(&selection, now))
                                })
                            };
                            if exit {
                                if let Ok(mut state) = runtime.shared.lock() {
                                    state.request_shutdown();
                                }
                                runtime.wake_ui();
                            }
                            thread::sleep(Duration::from_millis(100));
                        }
                    })
                    .map_err(|e| format!("cannot start integration clock: {e}"))?,
            );
        }
        Ok(workers)
    }
}

impl Drop for IntegrationWorkers {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Ok(mut state) = self.runtime.shared.lock() {
            state.retire_integrations();
        }
        if let Some(tick) = self.tick.take() {
            let _ = tick.join();
        }
        for integration in &self.integrations {
            integration.shutdown();
        }
    }
}

struct PackWorkerGuard(Arc<PackService>);

impl Drop for PackWorkerGuard {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

struct ActiveUpdateGuard {
    origin: InstallOrigin,
    state_dir: PathBuf,
    instance_id: String,
}

impl Drop for ActiveUpdateGuard {
    fn drop(&mut self) {
        if let Err(error) = unregister_active(
            &self.origin,
            &self.state_dir,
            std::process::id(),
            &self.instance_id,
        ) {
            eprintln!("desktop-pet: cannot unregister active installation: {error}");
        }
    }
}

fn update_context(
    config: &DaemonConfig,
    instance_id: String,
    executable: PathBuf,
    prefs: &Preferences,
) -> Result<UpdateContext, String> {
    let environment = captured_environment(prefs.observation_host(), |key| env::var(key).ok())?;
    let locale = ui::current_ui_locale(prefs.language()).tag();
    let context = UpdateContext {
        running: executable_identity(&executable)?,
        executable,
        version: desktop_pet_update_coordinator::identity::VERSION.to_owned(),
        instance_id,
        config_dir: config.paths.config_dir.clone(),
        #[cfg(desktop_pet_legacy)]
        host_plugin_config_dir:
            desktop_pet_update_coordinator::legacy_entry::host_plugin_config_dir(true),
        state_dir: config.paths.state_dir.clone(),
        #[cfg(desktop_pet_legacy)]
        herdr_socket: desktop_pet_update_coordinator::legacy_entry::herdr_socket(),
        running_origin: None,
        assets_override: config.assets_override.then(|| config.assets.clone()),
        environment,
        locale: locale.to_owned(),
    };
    Ok(context)
}

/// Run the sole native daemon instance in the foreground of its detached child.
///
/// The lifecycle lock belongs to the daemon for its entire lifetime.  Control
/// clients never mutate AppState while doing disk I/O, and all worker shutdown
/// joins happen after AppKit has returned.
pub fn run(config: DaemonConfig) -> Result<(), String> {
    let lock = Arc::new(crate::cli::acquire_lock_until(
        &config.paths,
        Instant::now() + crate::cli::STARTUP_TIMEOUT,
    )?);
    let profile = ProfileStore::with_lock(&config.paths.config_dir, lock.clone())?
        .with_preferences_validator(Preferences::validate_committed_object);
    let settings = lifecycle::read_settings(&config.paths.config_dir)?;
    let updater_generation = env::var("DESKTOP_PET_UPDATER_GENERATION")
        .ok()
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| "invalid updater generation".to_owned())
        })
        .transpose()?;
    let updater_token = env::var("DESKTOP_PET_UPDATER_TOKEN").ok();
    if updater_token.is_some() != updater_generation.is_some() {
        return Err("updater startup requires both token and generation".into());
    }
    if config.automatic_start && !settings.auto_start {
        if let Some(token) = config.startup_token.as_deref() {
            let _ = profile
                .with_writer_lock(|| lifecycle::abandon_startup(&config.paths.config_dir, token));
        }
        return Err("desktop-pet automatic start is disabled".to_owned());
    }

    let mut initial_state = AppState::new();
    initial_state.set_lifecycle_settings(settings);
    initial_state.set_integrations(
        config.composition.integrations.clone(),
        config.composition.provider_labels.clone(),
    );
    initial_state.set_lifecycle_extensions(
        config.composition.extensions.lifecycle.clone(),
        profile.clone(),
    );
    let migration_input = migration_environment()?;
    let mut prefs = if migration_input.is_some() {
        Preferences::load(&config.paths.config_dir)?
    } else {
        Preferences::load_for_daemon(&config.paths.config_dir)?
    };
    prefs.set_integration_extensions(
        profile.clone(),
        config.composition.extensions.lifecycle.clone(),
    );
    prefs.set_observation_host(config.observation_mode.clone());
    initial_state.set_observation_selection(config.observation_mode.clone())?;
    initial_state.set_preferences(&prefs);
    let automation =
        new_automation(PresentationTarget::from_scene(&initial_state.scene()), None)
            .map_err(|error| format!("cannot initialize presentation automation: {error}"))?;
    let instance_id = crate::automation::lock_automation(&automation)
        .instance()
        .to_owned();
    let executable = crate::bundle::executable()
        .map_err(|error| format!("cannot resolve desktop-pet executable path: {error}"))?;
    let mut context = update_context(&config, instance_id.clone(), executable, &prefs)?;
    let migration = migration_input
        .as_ref()
        .map(|(path, operation, token)| {
            desktop_pet_update_coordinator::migration::startup_authorization(
                path, operation, token, &context,
            )
        })
        .transpose()?;
    context.running_origin = Some(Box::new(
        detect_origin(&context).unwrap_or_else(|reason| InstallOrigin::Unknown { reason }),
    ));
    let origin = context
        .running_origin
        .as_deref()
        .expect("origin captured")
        .clone();
    let runtime_start_fence =
        desktop_pet_update_coordinator::protocol::capture_runtime_start_fence(
            &config.paths.state_dir,
        )?;
    start_allowed(
        &origin,
        &config.paths.state_dir,
        updater_generation,
        updater_token.as_deref(),
    )?;
    if let (Some(generation), Some(token)) = (updater_generation, updater_token.as_deref()) {
        // Provenance failure must never prevent the old helper observing ready.
        if let Err(error) =
            desktop_pet_update_coordinator::standalone_relocation::capture_relocation_parent(
                &context, generation, token,
            )
        {
            eprintln!("private relocation parent capture: {error}");
        }
    }
    config.composition.configure_profile(profile.clone())?;
    prefs.refresh_owner_snapshot(&config.paths.config_dir)?;
    if migration.is_none() {
        if let Some(token) = config.startup_token.as_deref() {
            profile
                .with_writer_lock(|| lifecycle::claim_startup(&config.paths.config_dir, token))?;
        }
        lifecycle::write_settings(&profile, &config.composition.extensions.lifecycle, settings)?;
    }
    let builtin_assets = if config.assets_override {
        crate::cli::resolve_assets(None).unwrap_or_else(|_| config.assets.clone())
    } else {
        config.assets.clone()
    };
    let override_assets = config.assets_override.then(|| config.assets.clone());
    let packs = PackService::new(
        config.paths.config_dir.clone(),
        builtin_assets,
        override_assets,
    );
    // Every early return joins the worker before releasing the lifecycle lock.
    let _pack_worker = PackWorkerGuard(Arc::clone(&packs));
    if migration
        .as_ref()
        .is_some_and(|migration| !migration.source)
    {
        packs.start_read_only()?;
    } else {
        packs.start()?;
    }
    // Token children already persisted the selected host under the launcher's
    // lifecycle lock. Direct daemon entry saves only after update admission.
    if config.startup_token.is_none() && migration.is_none() {
        profile.with_writer_lock(|| {
            crate::cli::persist_observation_mode(
                &config.paths.config_dir,
                config.observation_mode.clone(),
            )
        })?;
    }
    let _active = if matches!(origin, InstallOrigin::Unknown { .. }) {
        None
    } else {
        register_active(
            &origin,
            &config.paths.state_dir,
            std::process::id(),
            &instance_id,
            updater_generation,
            updater_token.as_deref(),
        )?;
        let guard = ActiveUpdateGuard {
            origin: origin.clone(),
            state_dir: config.paths.state_dir.clone(),
            instance_id,
        };
        desktop_pet_update_coordinator::protocol::retain_runtime_start(
            &context,
            std::process::id(),
            runtime_start_fence,
        )?;
        Some(guard)
    };
    let executable = context.running.path.clone();
    initial_state.set_update_context(context)?;
    initial_state.set_automation(automation.clone());
    if let Some(migration) = migration.as_ref().filter(|migration| !migration.source) {
        let (path, _, _) = migration_input
            .as_ref()
            .ok_or("missing authenticated migration input")?;
        initial_state.set_pending_migration_plan(
            desktop_pet_update_coordinator::migration::read_plan(path)?,
        )?;
        initial_state
            .set_migration_gate(migration.operation_id.clone(), migration.token.clone())?;
    }
    let shared = Arc::new(Mutex::new(initial_state));
    let runtime = Arc::new(StateRuntime {
        shared: shared.clone(),
    });
    let workers = IntegrationWorkers::start(runtime, &config.composition)?;
    {
        let mut state = shared
            .lock()
            .map_err(|_| "desktop-pet domain lock is poisoned")?;
        for integration in &config.composition.integrations {
            integration.set_enabled(
                config.observation_mode.includes_provider(integration.id()),
                &mut *state,
            );
        }
    }
    let mut control = ControlServer::bind(
        Arc::clone(&shared),
        config.paths.clone(),
        executable,
        Arc::clone(&packs),
        config.composition.extensions.control.clone(),
        config.composition.status_fields.clone(),
    )?;
    start_allowed(
        &origin,
        &config.paths.state_dir,
        updater_generation,
        updater_token.as_deref(),
    )?;
    if let Err(error) = control.start() {
        packs.shutdown();
        return Err(error);
    }
    if let Some(token) = config.startup_token.as_deref() {
        if let Err(error) =
            profile.with_writer_lock(|| lifecycle::finish_startup(&config.paths.config_dir, token))
        {
            control.shutdown();
            packs.shutdown();
            return Err(error);
        }
    }
    if updater_generation.is_some() {
        if let Err(error) = start_allowed(
            &origin,
            &config.paths.state_dir,
            updater_generation,
            updater_token.as_deref(),
        ) {
            control.shutdown();
            packs.shutdown();
            return Err(error);
        }
    }

    let ui_result = ui::run(
        Arc::clone(&shared),
        &config.assets,
        Arc::clone(&packs),
        prefs,
        config.paths.clone(),
    );
    crate::automation::lock_automation(&automation).shutdown();
    if ui_result.is_err() {
        let mut state = shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.request_shutdown();
        drop(state);
        ui::wake();
    }

    // AppKit has stopped before joining integration/control workers.
    drop(workers);
    control.shutdown();
    packs.shutdown();

    ui_result
}

fn migration_environment() -> Result<Option<(PathBuf, String, String)>, String> {
    use desktop_pet_update_coordinator::migration::{OPERATION_ENV, PLAN_ENV, TOKEN_ENV};
    let read_token = |key: &str| match env::var(key) {
        Ok(value) if !value.is_empty() => Ok(Some(value)),
        Ok(_) => Err(format!("{key} is empty")),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{key} is not UTF-8")),
    };
    match (
        env::var_os(PLAN_ENV),
        read_token(OPERATION_ENV)?,
        read_token(TOKEN_ENV)?,
    ) {
        (None, None, None) => Ok(None),
        (Some(path), Some(operation), Some(token))
            if !path.is_empty() && !operation.is_empty() && !token.is_empty() =>
        {
            Ok(Some((PathBuf::from(path), operation, token)))
        }
        _ => Err("migration startup requires exact plan, operation and token".into()),
    }
}

pub(crate) fn validate_migration_startup_context(context: &UpdateContext) -> Result<(), String> {
    if let Some((path, operation, token)) = migration_environment()? {
        desktop_pet_update_coordinator::migration::startup_authorization(
            &path, &operation, &token, context,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(not(desktop_pet_legacy))]
    fn neutral_environment_has_no_host_defaults() {
        let environment = captured_environment(ObservationSelection::Auto, |_| None)
            .expect("neutral environment");
        assert_eq!(
            environment
                .get(OBSERVATION_SELECTION_ENV)
                .map(String::as_str),
            Some("auto")
        );
        assert!(!environment.keys().any(|key| key.starts_with("HERDR")));
    }

    #[test]
    #[cfg(not(desktop_pet_legacy))]
    fn tern_delivery_keeps_exact_endpoint_and_rejects_invalid_explicit_routing() {
        let environment = captured_environment(ObservationSelection::Auto, |key| match key {
            "TERN_CONFIG_DIR" => Some("/isolated/tern/config".into()),
            "TERN_DAEMON_SOCKET" => Some("/isolated/tern/control.sock".into()),
            "TERN_WINDOW_KEY" => Some("not-routing-authority".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(
            environment.get("TERN_CONFIG_DIR").map(String::as_str),
            Some("/isolated/tern/config")
        );
        assert_eq!(
            environment.get("TERN_DAEMON_SOCKET").map(String::as_str),
            Some("/isolated/tern/control.sock")
        );
        assert!(!environment.contains_key("TERN_WINDOW_KEY"));
        for key in ["TERN_CONFIG_DIR", "TERN_DAEMON_SOCKET"] {
            for invalid in [
                "".to_owned(),
                "relative".into(),
                "/bad\0path".into(),
                format!("/{}", "x".repeat(4096)),
            ] {
                assert!(
                    captured_environment(ObservationSelection::Auto, |candidate| {
                        (candidate == key).then(|| invalid.clone())
                    })
                    .is_err(),
                    "{key} must not silently fall back from invalid explicit routing"
                );
            }
        }
    }
    #[test]
    fn domain_callback_is_once_and_poison_never_invokes_it() {
        let runtime = Arc::new(StateRuntime {
            shared: Arc::new(Mutex::new(AppState::new())),
        });
        let mut calls = 0;
        runtime.with_domain(&mut |_| calls += 1).expect("good lock");
        assert_eq!(calls, 1);
        let shared = runtime.shared.clone();
        let _ = thread::spawn(move || {
            let _guard = shared.lock().unwrap();
            panic!("poison fixture");
        })
        .join();
        assert!(runtime.with_domain(&mut |_| calls += 1).is_err());
        assert_eq!(calls, 1);
    }
}
