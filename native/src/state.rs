use crate::automation::{
    PresentationAction, PresentationPatch, PresentationTarget, SessionIdentity, SharedAutomation,
};
use crate::bubble::BubblePlacement;
use crate::lifecycle::LifecycleSettings;
use crate::session::{AgentRecord, AgentStatus, CompletionObservation, OutcomeObservation};
use crate::session_view::{
    CardDisplay, PromptTarget, PromptTargetError, SessionKey, SessionListOptions,
    SessionPageCursor, SessionPageError, SessionPageRequest, SessionSnapshot, SessionStore,
    SessionView, SourceAdmission, WorktreeRemoveTarget, WorktreeRemoveTargetError,
};
use crate::sources::{
    remote_machine_id, remote_source, HostConnections, ObservationPreferences, SourceCatalog,
    SourceRoute,
};
use crate::tern::{
    TernExchangeErrorCode, TernExchangeReply, TernExchangeRequest, TernRegistry, Window,
};
use herdr_update_coordinator::{ObservationMode, UpdateContext, OBSERVATION_MODE_ENV};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_COMPLETIONS: usize = 64;
const UPDATE_PREPARE_LIFETIME: Duration = Duration::from_secs(20);

#[derive(Clone, Debug)]
pub(crate) struct UpdatePrepare {
    pub operation_id: String,
    pub token: String,
}

#[derive(Debug)]
struct PendingUpdate {
    request: UpdatePrepare,
    reply: Option<mpsc::Sender<Result<(), String>>>,
    taken: bool,
    prepared: bool,
    stop_claimed: bool,
    expires: Instant,
}

impl PendingUpdate {
    fn matches(&self, operation_id: &str, token: &str) -> bool {
        self.request.operation_id == operation_id && self.request.token == token
    }
}

const COMPLETION_MAX_AGE: Duration = Duration::from_secs(30);
/// Only export stored, displayable metadata. In particular, raw source paths,
/// full working directories, and worktree root/checkout paths are not session wire data.
fn automation_session_row(instance_id: &str, view: &SessionView) -> Value {
    use crate::agent_outcome::AgentOutcome;
    let status = match view.status {
        AgentStatus::Idle => "idle",
        AgentStatus::Working => "working",
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Unknown => "unknown",
    };
    let outcome = view.outcome.map(|outcome| match outcome {
        AgentOutcome::Running => "running",
        AgentOutcome::Succeeded => "succeeded",
        AgentOutcome::Failed => "failed",
        AgentOutcome::Cancelled => "cancelled",
    });
    json!({
        "key": {
            "instance_id": instance_id,
            "source_id": view.key.source_id,
            "generation": view.key.generation,
            "terminal_id": view.key.terminal_id,
        },
        "source_label": view.source_label,
        "is_local": view.is_local,
        "pane_id": view.pane_id,
        "availability": view.availability,
        "status": status,
        "outcome": outcome,
        "display_status": view.display_status(),
        "metadata": {
            "title": view.metadata.title,
            "agent": view.metadata.agent,
            "workspace_id": view.metadata.workspace_id,
            "workspace_label": view.metadata.workspace_label,
            "tab_id": view.metadata.tab_id,
            "tab_label": view.metadata.tab_label,
        },
    })
}

fn completion_is_fresh(observed_at: Instant, now: Instant) -> bool {
    now.checked_duration_since(observed_at)
        .is_none_or(|age| age <= COMPLETION_MAX_AGE)
}
fn completion_matches_source(
    observation: &CompletionObservation,
    source: &SourceState,
    now: Instant,
) -> bool {
    source.connected
        && source.generation == observation.generation
        && completion_is_fresh(observation.observed_at, now)
}

fn purge_completions_for_source(completions: &mut VecDeque<CompletionObservation>, source: &str) {
    completions.retain(|observation| observation.source != source);
}

pub(crate) const MAX_COUNT: u32 = 1_000_000;
pub(crate) const DEFAULT_SCALE: f64 = 0.65;
pub(crate) const MIN_SCALE: f64 = 0.35;
pub(crate) const MAX_SCALE: f64 = 1.25;

pub(crate) fn normalize_scale(scale: f64) -> Option<f64> {
    scale.is_finite().then(|| scale.clamp(MIN_SCALE, MAX_SCALE))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Running,
    Waiting,
    Unknown,
}

impl Phase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub phase: Phase,
    pub sessions: usize,
    pub working: usize,
    pub blocked: usize,
    pub done: usize,
    pub unknown: usize,
    pub connected_sources: usize,
    pub disconnected_sources: usize,
    pub visible: bool,
    pub passthrough: bool,
    pub alpha_passthrough: bool,
    pub bubble_visible: bool,
    pub bubble_placement: BubblePlacement,
    pub scale: f64,
    pub reset_position_revision: u64,
    pub shutdown: bool,
}

impl Scene {
    pub(crate) fn needs_recovery_entry(&self) -> bool {
        !self.shutdown && (self.passthrough || (!self.visible && !self.bubble_visible))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct SourceCounts {
    pub sessions: usize,
    pub working: usize,
    pub blocked: usize,
    pub done: usize,
    pub unknown: usize,
}

impl SourceCounts {
    pub(crate) fn disconnected(self) -> Self {
        let sessions = self.sessions.min(MAX_COUNT as usize);
        Self {
            sessions,
            working: 0,
            blocked: 0,
            done: 0,
            unknown: sessions,
        }
    }

    fn bounded(self) -> Self {
        let max = MAX_COUNT as usize;
        Self {
            sessions: self.sessions.min(max),
            working: self.working.min(max),
            blocked: self.blocked.min(max),
            done: self.done.min(max),
            unknown: self.unknown.min(max),
        }
    }
}
pub(crate) struct SessionStatusUpdate<'a> {
    pub(crate) source: &'a str,
    pub(crate) generation: u64,
    pub(crate) terminal_id: &'a str,
    pub(crate) pane_id: &'a str,
    pub(crate) status: AgentStatus,
    pub(crate) counts: SourceCounts,
    pub(crate) completion: bool,
    pub(crate) observed_at: Instant,
}

#[derive(Clone, Debug)]
struct SourceState {
    generation: u64,
    connected: bool,
    coherent: bool,
    offline_since: Option<Instant>,
    counts: SourceCounts,
    snapshot_revision: Option<u64>,
    // Populated only by a complete, accepted snapshot for this generation.
    pane_ids: Option<Arc<[String]>>,
}

#[derive(Debug)]
pub struct AppState {
    visible: bool,
    passthrough: bool,
    alpha_passthrough: bool,
    bubble_visible: bool,
    bubble_placement: BubblePlacement,
    scale: f64,
    reset_position_revision: u64,
    automation: Option<SharedAutomation>,
    shutdown: bool,
    updater_stopping: bool,
    update_context: Option<UpdateContext>,
    pending_update: Option<PendingUpdate>,
    cancelled_updates: VecDeque<String>,
    lifecycle_settings: LifecycleSettings,
    ui_ready: bool,
    /// Global across sources and reconnects; only complete accepted snapshots advance it.
    coherent_snapshot_revision: u64,
    sources: HashMap<String, SourceState>,
    observation: ObservationPreferences,
    observation_mode: ObservationMode,
    tern: TernRegistry,
    observation_catalog: SourceCatalog,
    observation_epoch: u64,
    retired_generations: HashMap<String, u64>,
    session_store: SessionStore,
    completions: VecDeque<CompletionObservation>,
    outcomes: VecDeque<OutcomeObservation>,
    phase: Phase,
    sessions: usize,
    working: usize,
    blocked: usize,
    done: usize,
    unknown: usize,
    connected_sources: usize,
    disconnected_sources: usize,
    host_connections: HostConnections,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            visible: true,
            passthrough: false,
            alpha_passthrough: false,
            bubble_visible: true,
            bubble_placement: BubblePlacement::default(),
            scale: DEFAULT_SCALE,
            reset_position_revision: 0,
            automation: None,
            shutdown: false,
            updater_stopping: false,
            update_context: None,
            pending_update: None,
            cancelled_updates: VecDeque::new(),
            coherent_snapshot_revision: 0,
            lifecycle_settings: LifecycleSettings {
                auto_start: true,
                exit_with_herdr: true,
            },
            ui_ready: false,
            sources: HashMap::new(),
            observation: ObservationPreferences::default(),
            observation_mode: ObservationMode::Both,
            tern: TernRegistry::default(),
            observation_catalog: SourceCatalog::default(),
            observation_epoch: 0,
            retired_generations: HashMap::new(),
            session_store: SessionStore::new(),
            completions: VecDeque::new(),
            outcomes: VecDeque::new(),
            phase: Phase::Unknown,
            sessions: 0,
            working: 0,
            blocked: 0,
            done: 0,
            unknown: 0,
            connected_sources: 0,
            disconnected_sources: 0,
            host_connections: HostConnections::default(),
        }
    }
    #[cfg(test)]
    pub(crate) fn set_observation_mode(&mut self, mode: ObservationMode) {
        if self.observation_mode == mode || self.update_mutation_allowed().is_err() {
            return;
        }
        let Some(epoch) = self.observation_epoch.checked_add(1) else {
            return;
        };
        self.observation.host = mode;
        self.finish_observation_change(epoch);
    }

    pub(crate) fn observation_mode(&self) -> ObservationMode {
        self.observation_mode
    }

    pub(crate) fn set_update_context(&mut self, mut context: UpdateContext) {
        if self.update_mutation_allowed().is_err() {
            return;
        }
        Self::set_context_host(&mut context, self.observation_mode);
        self.update_context = Some(context);
    }

    fn set_context_host(context: &mut UpdateContext, mode: ObservationMode) {
        if mode == ObservationMode::Herdr {
            context.environment.remove(OBSERVATION_MODE_ENV);
        } else if context
            .environment
            .get(OBSERVATION_MODE_ENV)
            .is_none_or(|value| value != mode.as_str())
        {
            context
                .environment
                .insert(OBSERVATION_MODE_ENV.to_owned(), mode.as_str().to_owned());
        }
    }

    pub(crate) fn update_context(&self) -> Option<&UpdateContext> {
        self.update_context.as_ref()
    }

    pub(crate) fn updater_stopping(&self) -> bool {
        self.updater_stopping
    }

    pub(crate) fn update_prepared(&self) -> bool {
        self.pending_update
            .as_ref()
            .is_some_and(|pending| pending.prepared)
    }

    pub(crate) fn update_mutation_allowed(&self) -> Result<(), String> {
        if self.pending_update.is_some() {
            Err("application update is preparing or in progress; retry later".to_owned())
        } else {
            Ok(())
        }
    }

    pub(crate) fn queue_update_prepare(
        &mut self,
        operation_id: String,
        token: String,
        reply: mpsc::Sender<Result<(), String>>,
    ) -> Result<(), String> {
        if !self.ui_ready || self.shutdown {
            return Err("desktop pet is not ready for an update".into());
        }
        if self.pending_update.is_some() {
            return Err("another application update is preparing".into());
        }
        self.pending_update = Some(PendingUpdate {
            request: UpdatePrepare {
                operation_id,
                token,
            },
            reply: Some(reply),
            taken: false,
            prepared: false,
            stop_claimed: false,
            expires: Instant::now() + UPDATE_PREPARE_LIFETIME,
        });
        Ok(())
    }

    pub(crate) fn take_update_prepare(&mut self) -> Option<UpdatePrepare> {
        self.expire_update_prepare();
        let pending = self.pending_update.as_mut()?;
        if pending.taken {
            return None;
        }
        pending.taken = true;
        Some(pending.request.clone())
    }

    pub(crate) fn finish_update_prepare(
        &mut self,
        operation_id: &str,
        token: &str,
        result: Result<(), String>,
    ) -> bool {
        self.expire_update_prepare();
        if !self.pending_update.as_ref().is_some_and(|pending| {
            pending.matches(operation_id, token) && pending.taken && !pending.prepared
        }) {
            return false;
        }
        let pending = self
            .pending_update
            .as_mut()
            .expect("matched pending update");
        let success = result.is_ok();
        let delivered = pending
            .reply
            .take()
            .is_some_and(|reply| reply.send(result).is_ok());
        if success && delivered {
            pending.prepared = true;
        } else {
            self.pending_update = None;
        }
        success && delivered
    }

    /// Accept only a background-proven recovery for this exact prepared
    /// original. A claimed Stop or ordinary Quit wins over a late journal.
    pub(crate) fn reconcile_update_prepare(
        &mut self,
        operation_id: &str,
        context: &UpdateContext,
    ) -> bool {
        if self.shutdown
            || self.updater_stopping
            || self.update_context.as_ref() != Some(context)
            || !self.pending_update.as_ref().is_some_and(|pending| {
                pending.request.operation_id == operation_id
                    && pending.prepared
                    && !pending.stop_claimed
            })
        {
            return false;
        }
        self.pending_update = None;
        true
    }

    pub(crate) fn cancel_update_prepare(&mut self, operation_id: &str, token: &str) {
        if self
            .pending_update
            .as_ref()
            .is_some_and(|p| p.matches(operation_id, token))
        {
            let pending = self.pending_update.take().expect("matched pending update");
            if let Some(reply) = pending.reply {
                let _ = reply.send(Err("update preparation cancelled".into()));
            }
            if pending.prepared {
                self.cancelled_updates
                    .push_back(pending.request.operation_id);
            }
        }
    }

    pub(crate) fn take_cancelled_update_prepare(&mut self) -> Option<String> {
        self.expire_update_prepare();
        self.cancelled_updates.pop_front()
    }

    /// Own this exact Stop while the socket writer sends the accepted reply.
    /// The UI remains alive and all mutation ingress remains frozen.
    pub(crate) fn claim_update_stop(
        &mut self,
        operation_id: &str,
        token: &str,
    ) -> Result<(), String> {
        self.expire_update_prepare();
        if self.shutdown
            || !self
                .pending_update
                .as_ref()
                .is_some_and(|p| p.matches(operation_id, token) && p.prepared && !p.stop_claimed)
        {
            return Err("updater Stop requires a live unclaimed prepared operation".into());
        }
        self.pending_update
            .as_mut()
            .expect("matched prepared update")
            .stop_claimed = true;
        Ok(())
    }

    /// Only a fully written reply may commit a matching Stop. An ordinary Quit
    /// invalidates the claim instead of being reclassified as an updater exit.
    pub(crate) fn commit_update_stop(
        &mut self,
        operation_id: &str,
        token: &str,
    ) -> Result<(), String> {
        if self.shutdown
            || !self
                .pending_update
                .as_ref()
                .is_some_and(|p| p.matches(operation_id, token) && p.prepared && p.stop_claimed)
        {
            return Err("updater Stop claim changed before reply completion".into());
        }
        self.pending_update = None;
        self.updater_stopping = true;
        self.tern
            .reject_all_pending(crate::herdr::PromptError::Offline);
        self.shutdown = true;
        Ok(())
    }

    fn expire_update_prepare(&mut self) {
        if self
            .pending_update
            .as_ref()
            .is_some_and(|p| !p.stop_claimed && Instant::now() >= p.expires)
        {
            let pending = self.pending_update.take().expect("expired pending update");
            if let Some(reply) = pending.reply {
                let _ = reply.send(Err("update preparation timed out".into()));
            }
            if pending.prepared {
                self.cancelled_updates
                    .push_back(pending.request.operation_id);
            }
        }
    }

    /// Return the last watcher aggregate without allocating or walking source
    /// records.  Watcher updates maintain these counters off the UI hot path.
    pub fn scene(&self) -> Scene {
        Scene {
            phase: self.phase,
            sessions: self.sessions,
            working: self.working,
            blocked: self.blocked,
            done: self.done,
            unknown: self.unknown,
            connected_sources: self.connected_sources,
            disconnected_sources: self.disconnected_sources,
            visible: self.visible,
            passthrough: self.passthrough,
            alpha_passthrough: self.alpha_passthrough,
            bubble_visible: self.bubble_visible,
            bubble_placement: self.bubble_placement,
            scale: self.scale,
            reset_position_revision: self.reset_position_revision,
            shutdown: self.shutdown,
        }
    }
    pub(crate) fn observation_preferences(&self) -> &ObservationPreferences {
        &self.observation
    }

    pub(crate) fn observation_epoch(&self) -> u64 {
        self.observation_epoch
    }

    pub(crate) fn host_connections(&self) -> HostConnections {
        self.host_connections
    }

    pub(crate) fn observation_catalog(&self) -> &SourceCatalog {
        &self.observation_catalog
    }

    fn supports_source(&self, source: &str) -> bool {
        if source.starts_with("tern:") {
            self.observation_mode.supports_tern()
        } else {
            self.observation_mode.supports_herdr()
        }
    }

    fn accepts_source(&self, source: &str) -> bool {
        self.supports_source(source)
            && (remote_machine_id(source).is_none() || self.includes_source(source))
    }

    fn includes_source(&self, source: &str) -> bool {
        if !self.observation.includes(source) {
            return false;
        }
        remote_machine_id(source).is_none_or(|id| {
            self.observation_catalog
                .machines
                .iter()
                .any(|machine| machine.id == id && machine.enabled)
        })
    }

    fn reconcile_observation(&mut self) {
        let retired_sources: Vec<(String, u64)> = self
            .sources
            .iter()
            .filter(|(source, _)| !self.accepts_source(source))
            .map(|(source, state)| (source.clone(), state.generation))
            .collect();
        for (source, generation) in retired_sources {
            let _ = self.remove_source(&source, generation);
        }
        for source in self.sources.keys() {
            let included = self.includes_source(source);
            self.session_store.set_visibility(source, included);
        }
        if !self.observation_mode.supports_tern() {
            self.tern.retire_all();
        }
        self.completions.retain(|observation| {
            self.observation.includes(&observation.source)
                && remote_machine_id(&observation.source).is_none_or(|id| {
                    self.observation_catalog
                        .machines
                        .iter()
                        .any(|machine| machine.id == id && machine.enabled)
                })
        });
        self.outcomes.retain(|observation| {
            self.observation.includes(&observation.source)
                && remote_machine_id(&observation.source).is_none_or(|id| {
                    self.observation_catalog
                        .machines
                        .iter()
                        .any(|machine| machine.id == id && machine.enabled)
                })
        });
        self.recompute_aggregate();
    }

    pub(crate) fn apply_observation_preferences(
        &mut self,
        mut preferences: ObservationPreferences,
    ) {
        preferences.sanitize();
        if self.observation == preferences || self.update_mutation_allowed().is_err() {
            return;
        }
        let Some(epoch) = self.observation_epoch.checked_add(1) else {
            return;
        };
        self.observation = preferences;
        self.finish_observation_change(epoch);
    }

    fn finish_observation_change(&mut self, epoch: u64) {
        self.observation_mode = self.observation.host;
        if let Some(context) = self.update_context.as_mut() {
            Self::set_context_host(context, self.observation_mode);
        }
        self.observation_epoch = epoch;
        self.reconcile_observation();
        self.session_store.invalidate();
    }

    pub(crate) fn set_observation_catalog(&mut self, catalog: SourceCatalog) {
        if self.observation_catalog == catalog {
            return;
        }
        self.observation_catalog = catalog;
        self.reconcile_observation();
        for machine in &self.observation_catalog.machines {
            self.session_store
                .set_label(&remote_source(&machine.id), &machine.label);
        }
    }

    /// A selected remote source is healthy after a coherent snapshot, even if
    /// it has no agents. Local source visibility does not govern daemon exit.
    pub(crate) fn has_healthy_observed_remote(&self) -> bool {
        self.observation.remote
            && self.sources.iter().any(|(source, state)| {
                remote_machine_id(source).is_some()
                    && state.connected
                    && state.coherent
                    && self.includes_source(source)
            })
    }

    pub(crate) fn session_revision(&self) -> u64 {
        self.session_store.revision()
    }

    pub(crate) fn session_snapshot(
        &self,
        options: &SessionListOptions<'_>,
        selected: Option<&SessionKey>,
    ) -> SessionSnapshot {
        self.session_store.snapshot(options, selected)
    }
    pub(crate) fn session_display_for_key(
        &self,
        locale: crate::i18n::UiLocale,
        key: &SessionKey,
    ) -> Option<CardDisplay> {
        self.session_store.display_for_key(locale, key)
    }
    pub(crate) fn session_displays_for_keys(
        &self,
        locale: crate::i18n::UiLocale,
        keys: &[&SessionKey],
    ) -> Vec<Option<CardDisplay>> {
        self.session_store.displays_for_keys(locale, keys)
    }
    pub(crate) fn session_view_for_key(&self, key: &SessionKey) -> Option<SessionView> {
        self.session_store.view_for_key(key)
    }

    /// Paginate retained rows without the GUI's snapshot cap or selection state.
    pub(crate) fn automation_session_page(
        &self,
        request: &SessionPageRequest,
    ) -> Result<Value, String> {
        let automation = self.automation.as_ref().ok_or("automation unavailable")?;
        let instance = crate::automation::lock_automation(automation)
            .instance()
            .to_owned();
        if request.instance_id != instance {
            return Err("daemon instance changed".into());
        }
        let revision = self.session_store.revision();
        let after = match request.cursor.as_ref() {
            Some(cursor) => {
                if cursor.instance_id != instance {
                    return Err("cursor daemon instance changed".into());
                }
                if cursor.revision != revision {
                    return Err("session revision changed".into());
                }
                if cursor.filter != request.filter {
                    return Err("cursor filter changed".into());
                }
                Some(&cursor.position)
            }
            None => None,
        };
        let page = self
            .session_store
            .page(request.filter, revision, after, request.limit)
            .map_err(|error| match error {
                SessionPageError::StaleRevision => "session revision changed",
                SessionPageError::InvalidLimit => "session page limit must be between 1 and 128",
            })?;
        let next_cursor = page.next_cursor.map(|position| SessionPageCursor {
            instance_id: instance.clone(),
            revision: page.revision,
            filter: request.filter,
            position,
        });
        let rows: Vec<_> = page
            .rows
            .iter()
            .map(|view| automation_session_row(&instance, view))
            .collect();
        Ok(json!({
            "instance_id": instance,
            "revision": page.revision,
            "filter": request.filter,
            "rows": rows,
            "total": page.total,
            "matched": page.matched,
            "status_summary": page.status_summary,
            "next_cursor": next_cursor,
        }))
    }

    /// Resolve an exact identity, including a cached offline row, without
    /// consulting or mutating GUI selection.
    pub(crate) fn automation_session_detail(
        &self,
        identity: &SessionIdentity,
    ) -> Result<Value, String> {
        let automation = self.automation.as_ref().ok_or("automation unavailable")?;
        let instance = crate::automation::lock_automation(automation)
            .instance()
            .to_owned();
        if identity.instance_id != instance {
            return Err("daemon instance changed".into());
        }
        let key = SessionKey {
            source_id: identity.source_id,
            generation: identity.generation,
            terminal_id: identity.terminal_id.clone(),
        };
        let view = self
            .session_store
            .view_for_key(&key)
            .ok_or("stale session key")?;
        Ok(automation_session_row(&instance, &view))
    }

    pub(crate) fn prompt_target(
        &self,
        key: &SessionKey,
    ) -> Result<PromptTarget, crate::herdr::PromptError> {
        self.prompt_route(key).map(|(target, _)| target)
    }

    pub(crate) fn prompt_route(
        &self,
        key: &SessionKey,
    ) -> Result<(PromptTarget, SourceRoute), crate::herdr::PromptError> {
        use crate::herdr::PromptError;
        if self.shutdown {
            return Err(PromptError::Offline);
        }
        let target = self
            .session_store
            .prompt_target(key)
            .map_err(|error| match error {
                PromptTargetError::ReadOnly => PromptError::ReadOnly,
                PromptTargetError::Offline => PromptError::Offline,
                PromptTargetError::Stale => PromptError::StaleTarget,
            })?;
        if !self.includes_source(&target.source) {
            return Err(PromptError::StaleTarget);
        }
        let route = self
            .source_route(&target.source, key.generation)
            .ok_or(PromptError::StaleTarget)?;
        if !self.sources.get(&target.source).is_some_and(|source| {
            source.generation == key.generation && source.connected && source.coherent
        }) {
            return Err(PromptError::Offline);
        }
        Ok((target, route))
    }

    fn source_route(&self, source: &str, generation: u64) -> Option<SourceRoute> {
        if source.starts_with("tern:") {
            return self
                .tern
                .route(source, generation)
                .filter(|_| self.observation_mode.supports_tern());
        }
        if let Some(machine) = remote_machine_id(source) {
            return self
                .observation_mode
                .supports_herdr()
                .then(|| SourceRoute::HerdrRemote {
                    machine: machine.to_owned(),
                });
        }
        (self.observation_mode.supports_herdr()
            && std::path::Path::new(source).is_absolute()
            && self
                .sources
                .get(source)
                .is_some_and(|entry| entry.generation == generation))
        .then(|| SourceRoute::HerdrLocal(std::path::PathBuf::from(source)))
    }

    pub(crate) fn worktree_remove_target(
        &self,
        key: &SessionKey,
    ) -> Result<WorktreeRemoveTarget, WorktreeRemoveTargetError> {
        if self.shutdown {
            return Err(WorktreeRemoveTargetError::Offline);
        }
        let target = self.session_store.worktree_remove_target(key)?;
        if !self.includes_source(&target.source) {
            return Err(WorktreeRemoveTargetError::Stale);
        }
        if !std::path::Path::new(&target.source).is_absolute() {
            return Err(WorktreeRemoveTargetError::Stale);
        }
        let route = self
            .source_route(&target.source, key.generation)
            .ok_or(WorktreeRemoveTargetError::Stale)?;
        if !route.can_remove_worktree() {
            return Err(WorktreeRemoveTargetError::ReadOnly);
        }
        if !self.sources.get(&target.source).is_some_and(|source| {
            source.generation == key.generation && source.connected && source.coherent
        }) {
            return Err(WorktreeRemoveTargetError::Offline);
        }
        Ok(target)
    }
    /// Baseline for a dispatch against the currently resolved, live card.
    /// Status transitions and reconnects cannot supply a new full-snapshot proof.
    pub(crate) fn worktree_snapshot_revision(&self, target: &WorktreeRemoveTarget) -> Option<u64> {
        if self.worktree_remove_target(&target.key).ok().as_ref() != Some(target) {
            return None;
        }
        let source = self.sources.get(&target.source)?;
        if source.generation != target.key.generation || !source.coherent || !source.connected {
            return None;
        }
        source.pane_ids.as_ref()?;
        source.snapshot_revision
    }

    /// The watcher supplies complete pane IDs independently of agent rows:
    /// an exited agent can leave a shell pane behind. A reconnect can prove
    /// absence only if the registered source ID/path is unchanged and its new
    /// generation has provided another coherent full snapshot.
    pub(crate) fn automation_worktree_observation(
        &self,
        target: &WorktreeRemoveTarget,
    ) -> Option<Value> {
        if self.shutdown
            || remote_machine_id(&target.source).is_some()
            || !std::path::Path::new(&target.source).is_absolute()
            || !self.includes_source(&target.source)
        {
            return None;
        }
        let source = self.sources.get(&target.source)?;
        let revision = source.snapshot_revision?;
        if !source.connected || !source.coherent || source.generation < target.key.generation {
            return None;
        }
        let panes = source.pane_ids.as_ref()?;
        let session_absent = self.session_store.worktree_session_absent(
            &target.source,
            target.key.source_id,
            source.generation,
            &target.key.terminal_id,
        )?;
        Some(json!({
            "source_id": target.key.source_id,
            "source": target.source,
            "snapshot_revision": revision,
            "generation": source.generation,
            "pane_absent": !panes.iter().any(|id| id == &target.pane_id),
            "session_absent": session_absent,
        }))
    }

    pub(crate) fn prompt_available(
        &self,
        key: &SessionKey,
    ) -> Result<(), crate::herdr::PromptError> {
        self.prompt_target(key).map(|_| ())
    }

    /// Consume observations that still belong to a connected current source.
    /// The empty path does not inspect source records or allocate.
    pub fn take_completions(&mut self) -> Vec<CompletionObservation> {
        if self.completions.is_empty() {
            return Vec::new();
        }
        let now = Instant::now();
        let mut observations = Vec::with_capacity(self.completions.len());
        while let Some(observation) = self.completions.pop_front() {
            if !self.includes_source(&observation.source) {
                continue;
            }
            let Some(source) = self.sources.get(&observation.source) else {
                continue;
            };
            if completion_matches_source(&observation, source, now) {
                observations.push(observation);
            }
        }
        observations
    }

    /// Queue one watcher observation after checking its source generation.
    /// Queue ownership remains in AppState so source disconnects can discard
    /// pending UI reactions atomically with their status transition.
    pub(crate) fn push_completion(&mut self, observation: CompletionObservation) {
        if !self.includes_source(&observation.source) {
            return;
        }
        let Some(source) = self.sources.get(&observation.source) else {
            return;
        };
        if !completion_matches_source(&observation, source, Instant::now()) {
            return;
        }
        if self.completions.len() >= MAX_COMPLETIONS {
            let _ = self.completions.pop_front();
        }
        self.completions.push_back(observation);
    }

    pub(crate) fn take_outcomes(&mut self) -> Vec<OutcomeObservation> {
        if self.outcomes.is_empty() {
            return Vec::new();
        }
        let now = Instant::now();
        let mut observations = Vec::with_capacity(self.outcomes.len());
        while let Some(observation) = self.outcomes.pop_front() {
            if !self.includes_source(&observation.source) {
                continue;
            }
            if self.sources.get(&observation.source).is_some_and(|source| {
                source.connected
                    && source.generation == observation.generation
                    && completion_is_fresh(observation.observed_at, now)
            }) {
                observations.push(observation);
            }
        }
        observations
    }

    pub(crate) fn push_outcome(&mut self, observation: OutcomeObservation) {
        if !self.includes_source(&observation.source) {
            return;
        }
        if !self.sources.get(&observation.source).is_some_and(|source| {
            source.connected
                && source.coherent
                && source.generation == observation.generation
                && completion_is_fresh(observation.observed_at, Instant::now())
        }) {
            return;
        }
        let _ = self.session_store.accept_outcome(
            &observation.source,
            observation.generation,
            &observation.terminal_id,
            &observation.pane_id,
            observation.outcome,
            observation.at_unix_ms,
        );
        if self.outcomes.len() >= MAX_COMPLETIONS {
            let _ = self.outcomes.pop_front();
        }
        self.outcomes.push_back(observation);
    }

    pub(crate) fn automation(&self) -> Option<SharedAutomation> {
        self.automation.clone()
    }

    pub(crate) fn set_automation(&mut self, automation: SharedAutomation) {
        self.automation = Some(automation);
    }

    pub(crate) fn apply_presentation(
        &mut self,
        action: &PresentationAction,
    ) -> Result<PresentationTarget, String> {
        let target = PresentationTarget::from_scene(&self.scene()).applying(action)?;
        self.visible = target.visible;
        self.passthrough = target.passthrough;
        self.alpha_passthrough = target.alpha_passthrough;
        self.bubble_visible = target.bubble_visible;
        self.bubble_placement = target.bubble_placement;
        self.scale = target.scale;
        self.reset_position_revision = target.reset_position_revision;
        Ok(target)
    }

    fn presentation_changed(&self) {
        if let Some(automation) = &self.automation {
            crate::automation::lock_automation(automation)
                .ui_change(PresentationTarget::from_scene(&self.scene()));
        }
    }

    /// Restore an interactive character without changing the bubble or other settings.
    pub(crate) fn recover_interaction(&mut self) {
        self.apply_presentation(&PresentationAction::Set {
            patch: PresentationPatch {
                visible: Some(true),
                passthrough: Some(false),
                ..PresentationPatch::default()
            },
        })
        .expect("recovery target is valid");
        self.presentation_changed();
    }

    pub fn apply_control(&mut self, action: &str) -> Result<(), String> {
        self.update_mutation_allowed()?;
        let mut patch = PresentationPatch::default();
        let action = match action {
            "show" => {
                patch.visible = Some(true);
                PresentationAction::Set { patch }
            }
            "hide" => {
                patch.visible = Some(false);
                PresentationAction::Set { patch }
            }
            "toggle" => {
                patch.visible = Some(!self.visible);
                PresentationAction::Set { patch }
            }
            "toggle_passthrough" | "passthrough" => {
                patch.passthrough = Some(!self.passthrough);
                PresentationAction::Set { patch }
            }
            "alpha_passthrough" | "toggle_alpha_passthrough" => {
                patch.alpha_passthrough = Some(!self.alpha_passthrough);
                PresentationAction::Set { patch }
            }
            "show_bubble" => {
                patch.bubble_visible = Some(true);
                PresentationAction::Set { patch }
            }
            "hide_bubble" => {
                patch.bubble_visible = Some(false);
                PresentationAction::Set { patch }
            }
            "bubble_above" => {
                patch.bubble_placement = Some(BubblePlacement::Above);
                PresentationAction::Set { patch }
            }
            "bubble_below" => {
                patch.bubble_placement = Some(BubblePlacement::Below);
                PresentationAction::Set { patch }
            }
            "bubble_left" => {
                patch.bubble_placement = Some(BubblePlacement::Left);
                PresentationAction::Set { patch }
            }
            "bubble_right" => {
                patch.bubble_placement = Some(BubblePlacement::Right);
                PresentationAction::Set { patch }
            }
            "bubble_auto" => {
                patch.bubble_placement = Some(BubblePlacement::Auto);
                PresentationAction::Set { patch }
            }
            "scale_up" => {
                patch.scale = Some(self.scale + 0.05);
                PresentationAction::Set { patch }
            }
            "scale_down" => {
                patch.scale = Some(self.scale - 0.05);
                PresentationAction::Set { patch }
            }
            "reset_position" => PresentationAction::ResetPosition,
            "status" => PresentationAction::Set { patch },
            _ => return Err(format!("unsupported control action: {action}")),
        };
        self.apply_presentation(&action)?;
        self.presentation_changed();
        Ok(())
    }

    pub fn set_scale(&mut self, scale: f64) -> bool {
        let Some(scale) = normalize_scale(scale) else {
            return false;
        };
        if self.scale == scale {
            return false;
        }
        self.apply_presentation(&PresentationAction::Set {
            patch: PresentationPatch {
                scale: Some(scale),
                ..PresentationPatch::default()
            },
        })
        .expect("normalized scale is valid");
        self.presentation_changed();
        true
    }

    pub fn set_preferences(&mut self, prefs: &crate::preferences::Preferences) {
        self.scale = normalize_scale(prefs.scale()).unwrap_or(self.scale);
        self.visible = prefs.visible();
        self.passthrough = prefs.passthrough();
        self.alpha_passthrough = prefs.alpha_passthrough();
        self.bubble_visible = prefs.bubble_visible();
        self.bubble_placement = prefs.bubble_placement();
    }

    pub fn set_ui_ready(&mut self) {
        self.ui_ready = true;
    }

    pub fn is_ui_ready(&self) -> bool {
        self.ui_ready
    }

    pub(crate) fn lifecycle_settings(&self) -> LifecycleSettings {
        self.lifecycle_settings
    }

    pub(crate) fn set_lifecycle_settings(&mut self, settings: LifecycleSettings) {
        self.lifecycle_settings = settings;
    }

    /// Request an orderly daemon shutdown without changing persisted settings.
    pub fn request_shutdown(&mut self) {
        if let Some(pending) = self.pending_update.take() {
            if let Some(reply) = pending.reply {
                let _ = reply.send(Err("update preparation cancelled".into()));
            }
            if pending.prepared {
                self.cancelled_updates
                    .push_back(pending.request.operation_id);
            }
        }
        self.updater_stopping = false;
        self.tern
            .reject_all_pending(crate::herdr::PromptError::Offline);
        self.shutdown = true;
    }
    /// Freshness and pending delivery finalization, without retention pruning.
    pub(crate) fn expire_tern(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for (source, generation, since) in self.tern.expire(now) {
            changed |= self.mark_source_offline_at(&source, generation, since);
        }
        changed
    }

    /// The control tick owns time-based retirement; exchanges only age freshness.
    pub(crate) fn maintain_tern(&mut self, now: Instant) -> bool {
        let mut changed = self.expire_tern(now);
        let candidates: Vec<_> = self
            .sources
            .iter()
            .filter(|(source, entry)| {
                source.starts_with("tern:")
                    && !entry.connected
                    && entry.offline_since.is_some_and(|since| {
                        now.saturating_duration_since(since) >= crate::tern::OFFLINE_GRACE
                    })
                    && self
                        .session_store
                        .is_offline_generation(source, entry.generation)
                    && self.tern.allows_offline_removal(source, entry.generation)
            })
            .map(|(source, entry)| (source.clone(), entry.generation))
            .collect();
        for (source, generation) in candidates {
            changed |= self.remove_source(&source, generation);
        }
        let parents: Vec<_> = self
            .tern
            .expired_registrations(now)
            .filter(|(_, window)| {
                window.hosts.is_empty()
                    && window.offline_since.is_some_and(|since| {
                        now.saturating_duration_since(since) >= crate::tern::OFFLINE_GRACE
                    })
            })
            .map(|(id, _)| id.to_owned())
            .collect();
        for parent in parents {
            self.tern.retire_window(&parent);
            changed = true;
        }
        changed
    }
    pub(crate) fn queue_tern_prompt(
        &mut self,
        key: SessionKey,
        window: &str,
        epoch: u64,
        host: &str,
        observation_generation: u64,
        pane: &str,
        text: &str,
    ) -> Result<
        (
            String,
            mpsc::Receiver<Result<crate::herdr::PromptReceipt, crate::herdr::PromptError>>,
        ),
        crate::herdr::PromptError,
    > {
        self.update_mutation_allowed()
            .map_err(crate::herdr::PromptError::Other)?;
        let (target, route) = self.prompt_route(&key)?;
        if target.pane_id != pane
            || !matches!(&route, SourceRoute::TernWindow { window: current, epoch: current_epoch, host: current_host, observation_generation: current_observation }
            if current.as_str() == window && *current_epoch == epoch && current_host == host && *current_observation == observation_generation)
        {
            return Err(crate::herdr::PromptError::StaleTarget);
        }
        let instance_id = self
            .update_context
            .as_ref()
            .map(|context| context.instance_id.as_str())
            .ok_or(crate::herdr::PromptError::NotReady)?;
        self.tern.queue(
            window,
            epoch,
            host,
            observation_generation,
            key,
            pane,
            text,
            instance_id,
        )
    }

    pub(crate) fn cancel_tern_prompt(&mut self, window: &str, operation_id: &str) {
        self.tern.cancel(window, operation_id);
    }

    pub(crate) fn tern_exchange(&mut self, request: TernExchangeRequest) -> TernExchangeReply {
        self.tern_exchange_at(request, Instant::now())
    }

    fn tern_exchange_at(
        &mut self,
        request: TernExchangeRequest,
        now: Instant,
    ) -> TernExchangeReply {
        use crate::tern::{Host, Observation, TernHostReply, TernHostStatus, MAX_SAFE_INTEGER};
        let instance_id = self
            .update_context
            .as_ref()
            .map(|context| context.instance_id.clone())
            .unwrap_or_default();
        let error =
            |code, message: String| TernExchangeReply::error(instance_id.clone(), code, message);
        if !self.observation_mode.supports_tern() || self.shutdown || instance_id.is_empty() {
            return error(
                TernExchangeErrorCode::Rejected,
                "Tern observation is unavailable".into(),
            );
        }
        let snapshots = match request.normalize() {
            Ok(snapshots) => snapshots,
            Err(detail) => return error(TernExchangeErrorCode::Rejected, detail),
        };
        if self
            .tern
            .retired_epoch(&request.window)
            .is_some_and(|epoch| request.epoch <= epoch)
        {
            return error(
                TernExchangeErrorCode::SourceRetired,
                "stale retired Tern window epoch".into(),
            );
        }
        let previous = self.tern.get(&request.window);
        if previous.is_none() && request.instance_id.is_some() {
            return error(
                TernExchangeErrorCode::SourceRetired,
                "Tern window is retired".into(),
            );
        }
        if previous.is_some() && request.instance_id.is_none() {
            return error(
                TernExchangeErrorCode::BindingRequired,
                "missing bound daemon instance".into(),
            );
        }
        if request
            .instance_id
            .as_deref()
            .is_some_and(|id| id != instance_id)
        {
            return error(
                TernExchangeErrorCode::InstanceChanged,
                "daemon instance changed".into(),
            );
        }
        let new_epoch = previous.is_none_or(|previous| request.epoch > previous.epoch);
        if let Some(previous) = previous {
            if request.epoch < previous.epoch
                || (request.epoch == previous.epoch && request.sequence <= previous.sequence)
            {
                return error(
                    TernExchangeErrorCode::Rejected,
                    "stale Tern window epoch or sequence".into(),
                );
            }
        }
        // A receipt cannot establish a new parent epoch. Reject the envelope before
        // receipt matching so the captured current binding and pending remain intact.
        if new_epoch && !request.results.is_empty() {
            return error(
                TernExchangeErrorCode::Rejected,
                "Tern results require the currently bound window epoch".into(),
            );
        }
        if !new_epoch {
            if let Err(detail) = self.tern.validate_result(&request) {
                return error(TernExchangeErrorCode::ReceiptRejected, detail);
            }
        }
        // All structural, binding and receipt checks precede lifecycle mutation.
        self.expire_tern(now);
        if new_epoch {
            if self.tern.get(&request.window).is_none() && !self.ensure_tern_parent_capacity(now) {
                return error(
                    TernExchangeErrorCode::Rejected,
                    "Tern window registry is full".into(),
                );
            }
            let old: Vec<_> = self
                .tern
                .get(&request.window)
                .into_iter()
                .flat_map(|window| window.hosts.values())
                .map(|host| (host.source.clone(), host.generation))
                .collect();
            for (source, generation) in old {
                self.mark_source_offline_at(&source, generation, now);
            }
            self.tern
                .reject_pending(&request.window, crate::herdr::PromptError::StaleTarget);
            let mut next = Window::new(request.epoch, now);
            if let Some(previous) = self.tern.get_mut(&request.window) {
                next.hosts = std::mem::take(&mut previous.hosts);
                for host in next.hosts.values_mut() {
                    host.offline = true;
                }
            }
            self.tern.insert(request.window.clone(), next);
        }
        // Complete host-list omission is an observed loss, independent of other hosts' freshness.
        let omitted: Vec<_> = self
            .tern
            .get(&request.window)
            .unwrap()
            .observations
            .keys()
            .filter(|id| !request.hosts.iter().any(|host| &host.host == *id))
            .cloned()
            .collect();
        for id in omitted {
            self.lose_tern_host(&request.window, &id, now);
        }
        // Existing children first; new local before numeric remote IDs. Replies retain request order.
        let mut order: Vec<_> = request
            .hosts
            .iter()
            .enumerate()
            .map(|(index, host)| {
                (
                    !self
                        .tern
                        .get(&request.window)
                        .unwrap()
                        .hosts
                        .contains_key(&host.host),
                    !host.is_local,
                    host.host.parse::<u64>().expect("validated host ID"),
                    index,
                )
            })
            .collect();
        order.sort_unstable();
        let mut hosts: Vec<_> = request
            .hosts
            .iter()
            .map(|inventory| {
                let generation = self
                    .tern
                    .get(&request.window)
                    .unwrap()
                    .hosts
                    .get(&inventory.host)
                    .map_or(0, |host| host.generation);
                TernHostReply::new(inventory, generation, None)
            })
            .collect();
        for (_, _, _, index) in order {
            let inventory = &request.hosts[index];
            let (records, counts) = &snapshots[index];
            let window = self.tern.get(&request.window).unwrap();
            let child = window.hosts.get(&inventory.host);
            let current_generation = child.map_or(0, |host| host.generation);
            let watermark = window.observations.get(&inventory.host);
            let invalid = watermark.is_some_and(|observed| {
                inventory.observation_generation < observed.generation
                    || (inventory.observation_generation == observed.generation
                        && (inventory.is_local != observed.is_local
                            || (observed.requires_new
                                && inventory.status == TernHostStatus::Available)))
            });
            if invalid {
                hosts[index].reject("stale Tern host observation or locality".into());
                continue;
            }
            if inventory.status != TernHostStatus::Available {
                self.tern
                    .get_mut(&request.window)
                    .unwrap()
                    .observations
                    .insert(
                        inventory.host.clone(),
                        Observation {
                            generation: inventory.observation_generation,
                            is_local: inventory.is_local,
                            requires_new: true,
                        },
                    );
                self.lose_tern_host(&request.window, &inventory.host, now);
                continue;
            }
            let replacement = child.is_none_or(|host| {
                host.offline || host.observation_generation != inventory.observation_generation
            });
            let source = crate::tern::source_id(&request.window, &inventory.host);
            let generation = if replacement {
                self.source_generation(&source)
                    .into_iter()
                    .chain(self.retired_source_generation(&source))
                    .max()
                    .unwrap_or(0)
                    .checked_add(1)
                    .filter(|generation| *generation <= MAX_SAFE_INTEGER)
            } else {
                Some(current_generation)
            };
            let Some(generation) = generation else {
                hosts[index].reject("Tern source generation exhausted".into());
                continue;
            };
            if self.coherent_snapshot_revision.checked_add(1).is_none()
                || (replacement && !self.begin_source(source.clone(), generation))
            {
                hosts[index].reject("cannot register or publish Tern host".into());
                continue;
            }
            if replacement {
                self.tern
                    .get_mut(&request.window)
                    .unwrap()
                    .reject_host_pending(&inventory.host, crate::herdr::PromptError::StaleTarget);
            }
            if !self.session_store.set_locality(&source, inventory.is_local) {
                hosts[index].reject("Tern host locality changed without a new observation".into());
                continue;
            }
            let pane_ids: Arc<[String]> = records
                .iter()
                .map(|record| record.pane_id.clone())
                .collect();
            if !self.publish_source_snapshot(
                &source,
                generation,
                records.iter(),
                *counts,
                pane_ids,
                &[],
                now,
            ) {
                hosts[index].reject("cannot publish Tern host snapshot".into());
                continue;
            }
            self.session_store.set_label(
                &source,
                if inventory.label.trim().is_empty() {
                    &inventory.host
                } else {
                    &inventory.label
                },
            );
            let window = self.tern.get_mut(&request.window).unwrap();
            window.observations.insert(
                inventory.host.clone(),
                Observation {
                    generation: inventory.observation_generation,
                    is_local: inventory.is_local,
                    requires_new: false,
                },
            );
            window.hosts.insert(
                inventory.host.clone(),
                Host {
                    source,
                    generation,
                    observation_generation: inventory.observation_generation,
                    is_local: inventory.is_local,
                    last_seen: now,
                    offline: false,
                },
            );
            hosts[index].generation = generation;
        }
        let host_bytes = match crate::tern::host_reply_bytes(&hosts) {
            Ok(bytes) => bytes,
            Err(detail) => return error(TernExchangeErrorCode::Rejected, format!("{detail:?}")),
        };
        let window = self.tern.get_mut(&request.window).unwrap();
        window.sequence = request.sequence;
        window.last_seen = now;
        window.offline = false;
        window.offline_since = None;
        window.last_host_reply_bytes = host_bytes;
        window.bound = true;
        self.tern.settle(&request.window, request.results.first());
        let command = request.instance_id.as_ref().and_then(|_| {
            self.tern
                .pending_target(&request.window)
                .and_then(|(key, pane)| match self.prompt_route(&key) {
                    Ok((
                        target,
                        SourceRoute::TernWindow {
                            window,
                            epoch,
                            host,
                            observation_generation,
                        },
                    )) if window == request.window
                        && epoch == request.epoch
                        && target.pane_id == pane
                        && request.hosts.iter().any(|inventory| {
                            inventory.host == host
                                && inventory.is_local
                                && inventory.status == TernHostStatus::Available
                                && inventory.observation_generation == observation_generation
                        })
                        && hosts
                            .iter()
                            .any(|reply| reply.host == host && reply.error.is_none())
                        && self.update_mutation_allowed().is_ok() =>
                    {
                        self.tern.offer(&request.window, &instance_id)
                    }
                    _ => {
                        self.tern.reject_pending(
                            &request.window,
                            crate::herdr::PromptError::StaleTarget,
                        );
                        None
                    }
                })
        });
        TernExchangeReply {
            ok: true,
            instance_id,
            hosts,
            command,
            error: None,
            error_code: None,
        }
    }

    fn lose_tern_host(&mut self, window: &str, id: &str, now: Instant) {
        let entry = self.tern.get_mut(window).expect("registered parent");
        if let Some(observed) = entry.observations.get_mut(id) {
            observed.requires_new = true;
        }
        entry.reject_host_pending(id, crate::herdr::PromptError::Offline);
        let source = entry.hosts.get_mut(id).map(|host| {
            host.offline = true;
            (host.source.clone(), host.generation)
        });
        if let Some((source, generation)) = source {
            self.mark_source_offline_at(&source, generation, now);
        }
    }

    fn ensure_tern_parent_capacity(&mut self, now: Instant) -> bool {
        while self.tern.len() >= crate::tern::MAX_WINDOWS {
            let candidate = self
                .tern
                .expired_registrations(now)
                .filter(|(_, window)| {
                    window.hosts.values().all(|host| {
                        host.offline
                            && self.sources.get(&host.source).is_some_and(|source| {
                                !source.connected && source.generation == host.generation
                            })
                            && self
                                .session_store
                                .is_offline_generation(&host.source, host.generation)
                    })
                })
                .min_by_key(|(_, window)| window.last_seen)
                .map(|(id, window)| {
                    (
                        id.to_owned(),
                        window
                            .hosts
                            .values()
                            .map(|host| (host.source.clone(), host.generation))
                            .collect::<Vec<_>>(),
                    )
                });
            let Some((id, children)) = candidate else {
                return false;
            };
            for (source, generation) in children {
                if !self.remove_source(&source, generation) {
                    return false;
                }
            }
            self.tern.retire_window(&id);
        }
        true
    }

    /// Reclaim only children whose registry, aggregate and retained rows agree
    /// on the expired offline generation. Never retire their parent binding.
    fn ensure_source_capacity(&mut self, source: &str, generation: u64, now: Instant) -> bool {
        if self.session_store.source_admission(source, generation) == SourceAdmission::Rejected {
            return false;
        }
        if self.session_store.source_admission(source, generation) == SourceAdmission::Allowed {
            return true;
        }
        self.expire_tern(now);
        loop {
            if self.session_store.source_admission(source, generation) == SourceAdmission::Allowed {
                return true;
            }
            let retired = self
                .tern
                .expired_hosts(now)
                .filter(|host| {
                    self.sources.get(&host.source).is_some_and(|current| {
                        current.generation == host.generation && !current.connected
                    }) && self
                        .session_store
                        .is_offline_generation(&host.source, host.generation)
                })
                .min_by_key(|host| host.last_seen)
                .map(|host| (host.source.clone(), host.generation));
            let Some((retired_source, retired_generation)) = retired else {
                return false;
            };
            if !self.remove_source(&retired_source, retired_generation) {
                return false;
            }
        }
    }

    /// Inspect registration without beginning or resetting its generation.
    pub(crate) fn source_generation(&self, source: &str) -> Option<u64> {
        self.sources.get(source).map(|state| state.generation)
    }

    pub(crate) fn retired_source_generation(&self, source: &str) -> Option<u64> {
        self.retired_generations.get(source).copied()
    }

    /// Begin a new connection generation for a canonical endpoint.  Any
    /// previous live counts become unknown until the new generation provides a
    /// coherent snapshot.
    pub(crate) fn begin_source(&mut self, source: String, generation: u64) -> bool {
        if !self.accepts_source(&source)
            || self
                .retired_generations
                .get(&source)
                .is_some_and(|retired| generation <= *retired)
        {
            return false;
        }
        if generation == 0 {
            return false;
        }
        if let Some(existing) = self.sources.get(&source) {
            if generation <= existing.generation {
                return false;
            }
        }
        if !self.ensure_source_capacity(&source, generation, Instant::now()) {
            return false;
        }
        if !self.session_store.begin_source(&source, generation) {
            return false;
        }
        self.session_store
            .set_visibility(&source, self.includes_source(&source));
        if let Some(id) = remote_machine_id(&source) {
            if let Some(machine) = self
                .observation_catalog
                .machines
                .iter()
                .find(|machine| machine.id == id)
            {
                self.session_store.set_label(&source, &machine.label);
            }
        }
        purge_completions_for_source(&mut self.completions, &source);
        self.outcomes
            .retain(|observation| observation.source != source);
        let counts = self
            .sources
            .get(&source)
            .map(|existing| existing.counts.disconnected())
            .unwrap_or_default();
        self.sources.insert(
            source,
            SourceState {
                generation,
                connected: false,
                coherent: false,
                offline_since: None,
                counts,
                pane_ids: None,
                snapshot_revision: None,
            },
        );
        self.recompute_aggregate();
        true
    }

    /// Publish one endpoint's complete, already-normalized aggregate.  A
    /// callback from an older connection generation is ignored.  A connected
    /// source's records are published only through `publish_source_snapshot`,
    /// after the watcher has a coherent snapshot.
    pub(crate) fn update_source(
        &mut self,
        source: &str,
        generation: u64,
        connected: bool,
        counts: SourceCounts,
    ) -> bool {
        if !self.accepts_source(source) {
            return false;
        }
        {
            let Some(existing) = self.sources.get_mut(source) else {
                return false;
            };
            if existing.generation != generation {
                return false;
            }
            if !connected && existing.connected && existing.offline_since.is_none() {
                existing.offline_since = Some(Instant::now());
            }
            existing.connected = connected;
            existing.coherent = if connected { existing.coherent } else { false };
            if !connected {
                existing.pane_ids = None;
            }
            existing.counts = if connected {
                counts.bounded()
            } else {
                counts.disconnected()
            };
        }
        if !connected {
            let _ = self.session_store.mark_offline(source, generation);
            purge_completions_for_source(&mut self.completions, source);
            self.outcomes
                .retain(|observation| observation.source != source);
        }
        self.recompute_aggregate();
        true
    }

    pub(crate) fn mark_source_offline(&mut self, source: &str, generation: u64) -> bool {
        self.mark_source_offline_at(source, generation, Instant::now())
    }

    fn mark_source_offline_at(&mut self, source: &str, generation: u64, since: Instant) -> bool {
        if !self.accepts_source(source) {
            return false;
        }
        let Some(existing) = self.sources.get_mut(source) else {
            return false;
        };
        if existing.generation != generation {
            return false;
        }
        let deadline_changed = existing.offline_since.is_none_or(|before| since < before);
        existing.offline_since = Some(
            existing
                .offline_since
                .map_or(since, |before| before.min(since)),
        );
        if !existing.connected && !deadline_changed {
            return false;
        }
        let counts = existing.counts;
        self.update_source(source, generation, false, counts)
    }

    /// Revoke only the current source generation's matching card result;
    /// existing outcome reactions remain owned by the watcher queue.
    pub(crate) fn invalidate_session_outcome(
        &mut self,
        source: &str,
        generation: u64,
        terminal_id: &str,
        pane_id: &str,
    ) {
        self.session_store
            .invalidate_outcome(source, generation, terminal_id, pane_id);
    }

    /// Publish a coherent snapshot, its complete pane list and any completions
    /// observed while converging it. All facts change in one AppState mutation.
    pub(crate) fn publish_source_snapshot<'a, I>(
        &mut self,
        source: &str,
        generation: u64,
        records: I,
        counts: SourceCounts,
        pane_ids: Arc<[String]>,
        completions: &[(String, String)],
        observed_at: Instant,
    ) -> bool
    where
        I: IntoIterator<Item = &'a AgentRecord>,
    {
        if !self.accepts_source(source) {
            return false;
        }
        let Some(existing) = self.sources.get(source) else {
            return false;
        };
        if existing.generation != generation {
            return false;
        }
        let Some(next_revision) = self.coherent_snapshot_revision.checked_add(1) else {
            return false;
        };
        if !self
            .session_store
            .replace_source(source, generation, records)
        {
            return false;
        }
        self.coherent_snapshot_revision = next_revision;
        if let Some(existing) = self.sources.get_mut(source) {
            existing.connected = true;
            existing.coherent = true;
            existing.offline_since = None;
            existing.snapshot_revision = Some(next_revision);
            if existing
                .pane_ids
                .as_ref()
                .is_none_or(|before| !Arc::ptr_eq(before, &pane_ids))
            {
                existing.pane_ids = Some(pane_ids);
            }
            existing.counts = counts.bounded();
        }
        self.recompute_aggregate();
        for (terminal_id, pane_id) in completions {
            self.push_completion(CompletionObservation {
                source: source.to_owned(),
                generation,
                terminal_id: terminal_id.clone(),
                pane_id: pane_id.clone(),
                observed_at,
            });
        }
        true
    }

    /// Apply one unique live status transition without rebuilding the source
    /// record set.  The watcher supplies aggregate counts maintained from its
    /// authoritative records and a completion marker for active-to-Done.
    pub(crate) fn update_session_status(&mut self, update: SessionStatusUpdate<'_>) -> bool {
        let SessionStatusUpdate {
            source,
            generation,
            terminal_id,
            pane_id,
            status,
            counts,
            completion,
            observed_at,
        } = update;
        if !self.accepts_source(source) {
            return false;
        }
        let Some(existing) = self.sources.get(source) else {
            return false;
        };
        if existing.generation != generation || !existing.connected || !existing.coherent {
            return false;
        }
        if !self
            .session_store
            .update_status(source, generation, terminal_id, pane_id, status)
        {
            return false;
        }
        if let Some(existing) = self.sources.get_mut(source) {
            existing.counts = counts.bounded();
        }
        self.recompute_aggregate();
        if completion {
            self.push_completion(CompletionObservation {
                source: source.to_owned(),
                generation,
                terminal_id: terminal_id.to_owned(),
                pane_id: pane_id.to_owned(),
                observed_at,
            });
        }
        true
    }

    /// Remove a confirmed disabled/unlinked endpoint.  Stale callbacks from a
    /// prior generation cannot detach a newly registered endpoint.
    pub(crate) fn remove_source(&mut self, source: &str, generation: u64) -> bool {
        let Some(existing) = self.sources.get(source) else {
            return false;
        };
        if existing.generation != generation {
            return false;
        }
        if self
            .tern
            .source_generation(source)
            .is_some_and(|current| current != generation)
        {
            return false;
        }
        if !self.session_store.remove_source(source, generation) {
            return false;
        }
        self.tern.remove_source(source, generation);
        purge_completions_for_source(&mut self.completions, source);
        self.outcomes
            .retain(|observation| observation.source != source);
        self.retired_generations
            .entry(source.to_owned())
            .and_modify(|retired| *retired = (*retired).max(generation))
            .or_insert(generation);
        self.sources.remove(source);
        self.recompute_aggregate();
        true
    }

    fn recompute_aggregate(&mut self) {
        let mut aggregate = SourceCounts::default();
        let mut connected_sources = 0usize;
        let mut disconnected_sources = 0usize;
        let mut host_connections = HostConnections::default();
        for (source_id, source) in &self.sources {
            if source.connected && source.coherent && self.supports_source(source_id) {
                if source_id.starts_with("tern:") {
                    host_connections.tern = host_connections.tern.saturating_add(1);
                } else {
                    host_connections.herdr = host_connections.herdr.saturating_add(1);
                }
            }
            if !self.includes_source(source_id) {
                continue;
            }
            if source.connected {
                connected_sources = connected_sources.saturating_add(1);
            } else {
                disconnected_sources = disconnected_sources.saturating_add(1);
            }
            aggregate.sessions = aggregate
                .sessions
                .saturating_add(source.counts.sessions)
                .min(MAX_COUNT as usize);
            aggregate.working = aggregate
                .working
                .saturating_add(source.counts.working)
                .min(MAX_COUNT as usize);
            aggregate.blocked = aggregate
                .blocked
                .saturating_add(source.counts.blocked)
                .min(MAX_COUNT as usize);
            aggregate.done = aggregate
                .done
                .saturating_add(source.counts.done)
                .min(MAX_COUNT as usize);
            aggregate.unknown = aggregate
                .unknown
                .saturating_add(source.counts.unknown)
                .min(MAX_COUNT as usize);
        }
        self.sessions = aggregate.sessions;
        self.working = aggregate.working;
        self.blocked = aggregate.blocked;
        self.done = aggregate.done;
        self.unknown = aggregate.unknown;
        self.connected_sources = connected_sources;
        self.disconnected_sources = disconnected_sources;
        self.host_connections = host_connections;
        self.phase = if connected_sources == 0 {
            Phase::Unknown
        } else if aggregate.blocked > 0 {
            Phase::Waiting
        } else if aggregate.working > 0 {
            Phase::Running
        } else if aggregate.unknown > 0 {
            Phase::Unknown
        } else {
            Phase::Idle
        };
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::session_view::SessionFilter;
    use std::time::{Duration, Instant};
    #[test]
    fn updater_prepare_admission_requires_main_thread_acceptance_and_matching_stop() {
        let mut state = AppState::new();
        state.set_ui_ready();
        let (sender, receiver) = mpsc::channel();
        state
            .queue_update_prepare("operation-a".into(), "secret-a".into(), sender)
            .unwrap();
        assert!(state.update_mutation_allowed().is_err());
        assert!(state.claim_update_stop("operation-a", "secret-a").is_err());
        let prepared = state.take_update_prepare().unwrap();
        assert_eq!(prepared.operation_id, "operation-a");
        assert!(!state.finish_update_prepare("operation-a", "wrong", Ok(())));
        assert!(state.finish_update_prepare("operation-a", "secret-a", Ok(())));
        assert!(receiver.recv().unwrap().is_ok());
        assert!(state.claim_update_stop("operation-b", "secret-a").is_err());
        assert!(state.claim_update_stop("operation-a", "wrong").is_err());
        state.claim_update_stop("operation-a", "secret-a").unwrap();
        assert!(!state.scene().shutdown);
        assert!(!state.updater_stopping());
        assert!(state.update_mutation_allowed().is_err());
        assert!(state.claim_update_stop("operation-a", "secret-a").is_err());
        assert!(state.commit_update_stop("operation-a", "wrong").is_err());
        state.pending_update.as_mut().unwrap().expires = Instant::now() - Duration::from_secs(1);
        assert!(state.take_cancelled_update_prepare().is_none());
        state.commit_update_stop("operation-a", "secret-a").unwrap();
        assert!(state.scene().shutdown && state.updater_stopping());
        assert!(state.commit_update_stop("operation-a", "secret-a").is_err());
        state.request_shutdown();
        assert!(!state.updater_stopping());
    }

    #[test]
    fn expired_prepare_cannot_ack_late_or_stop_and_unfreezes() {
        let mut state = AppState::new();
        state.set_ui_ready();
        let (sender, receiver) = mpsc::channel();
        state
            .queue_update_prepare("operation-a".into(), "secret-a".into(), sender)
            .unwrap();
        let pending = state.take_update_prepare().unwrap();
        state.pending_update.as_mut().unwrap().expires = Instant::now() - Duration::from_secs(1);
        assert!(!state.finish_update_prepare(&pending.operation_id, &pending.token, Ok(())));
        assert!(receiver.recv().unwrap().is_err());
        assert!(state.update_mutation_allowed().is_ok());
        assert!(state.claim_update_stop("operation-a", "secret-a").is_err());
    }

    #[test]
    fn cancelled_prepared_update_unfreezes_without_reviving_stop() {
        let mut state = AppState::new();
        state.set_ui_ready();
        let (sender, receiver) = mpsc::channel();
        state
            .queue_update_prepare("operation-a".into(), "secret-a".into(), sender)
            .unwrap();
        state.take_update_prepare().unwrap();
        assert!(state.finish_update_prepare("operation-a", "secret-a", Ok(())));
        assert!(receiver.recv().unwrap().is_ok());
        state.cancel_update_prepare("operation-a", "secret-a");
        assert_eq!(
            state.take_cancelled_update_prepare().as_deref(),
            Some("operation-a")
        );
        assert!(state.claim_update_stop("operation-a", "secret-a").is_err());
        assert!(state.update_mutation_allowed().is_ok());
    }

    #[test]
    fn recovered_prepare_only_releases_exact_live_original_and_never_a_claimed_stop() {
        use herdr_update_coordinator::ExecutableIdentity;
        use std::collections::BTreeMap;
        use std::path::PathBuf;

        let image = ExecutableIdentity {
            path: PathBuf::from("/isolated/app"),
            sha256: "original image".into(),
        };
        let context = UpdateContext {
            executable: image.path.clone(),
            version: "0.2.0".into(),
            instance_id: "original".into(),
            config_dir: PathBuf::from("/isolated/profile"),
            host_plugin_config_dir: None,
            state_dir: PathBuf::from("/isolated/state"),
            herdr_socket: PathBuf::from("/isolated/original.sock"),
            running: image,
            running_origin: None,
            assets_override: None,
            environment: BTreeMap::new(),
            locale: "en".into(),
        };
        let mut state = AppState::new();
        state.set_observation_mode(ObservationMode::Herdr);
        state.set_ui_ready();
        state.set_update_context(context.clone());
        let (sender, receiver) = mpsc::channel();
        state
            .queue_update_prepare("old".into(), "secret".into(), sender)
            .unwrap();
        state.take_update_prepare().unwrap();
        assert!(state.finish_update_prepare("old", "secret", Ok(())));
        receiver.recv().unwrap().unwrap();
        assert!(state.update_mutation_allowed().is_err());
        let mut other = context.clone();
        other.running.sha256 = "new image".into();
        assert!(!state.reconcile_update_prepare("old", &other));
        assert!(!state.reconcile_update_prepare("new", &context));
        assert!(state.update_mutation_allowed().is_err());
        assert!(state.reconcile_update_prepare("old", &context));
        assert!(state.update_mutation_allowed().is_ok());
        assert!(state.claim_update_stop("old", "secret").is_err());

        let (sender, receiver) = mpsc::channel();
        state
            .queue_update_prepare("new".into(), "new-secret".into(), sender)
            .unwrap();
        state.take_update_prepare().unwrap();
        assert!(state.finish_update_prepare("new", "new-secret", Ok(())));
        receiver.recv().unwrap().unwrap();
        assert!(!state.reconcile_update_prepare("old", &context));
        assert!(state.update_mutation_allowed().is_err());
        state.claim_update_stop("new", "new-secret").unwrap();
        assert!(!state.reconcile_update_prepare("new", &context));
        assert!(state.update_mutation_allowed().is_err());
        state.request_shutdown();
        assert!(!state.reconcile_update_prepare("new", &context));
    }

    #[test]
    fn failed_stop_reply_thaws_and_user_quit_beats_claim() {
        let mut state = AppState::new();
        state.set_ui_ready();
        let (sender, receiver) = mpsc::channel();
        state
            .queue_update_prepare("failed-reply".into(), "token".into(), sender)
            .unwrap();
        state.take_update_prepare().unwrap();
        assert!(state.finish_update_prepare("failed-reply", "token", Ok(())));
        receiver.recv().unwrap().unwrap();
        state.claim_update_stop("failed-reply", "token").unwrap();
        assert!(!state.scene().shutdown);
        state.cancel_update_prepare("failed-reply", "token");
        assert!(state.update_mutation_allowed().is_ok());
        assert!(!state.updater_stopping());
        assert!(state.commit_update_stop("failed-reply", "token").is_err());

        let (sender, receiver) = mpsc::channel();
        state
            .queue_update_prepare("user-quit".into(), "token".into(), sender)
            .unwrap();
        state.take_update_prepare().unwrap();
        assert!(state.finish_update_prepare("user-quit", "token", Ok(())));
        receiver.recv().unwrap().unwrap();
        state.claim_update_stop("user-quit", "token").unwrap();
        state.request_shutdown();
        assert!(state.scene().shutdown && !state.updater_stopping());
        assert!(state.commit_update_stop("user-quit", "token").is_err());
    }

    fn connect(state: &mut AppState, source: &str, generation: u64) {
        assert!(state.begin_source(source.to_owned(), generation));
    }

    fn observation(source: &str, generation: u64, observed_at: Instant) -> CompletionObservation {
        CompletionObservation {
            source: source.to_owned(),
            generation,
            terminal_id: "terminal-a".to_owned(),
            pane_id: "pane-a".to_owned(),
            observed_at,
        }
    }
    fn record(terminal_id: &str, pane_id: &str, status: AgentStatus) -> AgentRecord {
        AgentRecord {
            terminal_id: terminal_id.to_owned(),
            pane_id: pane_id.to_owned(),
            status,
            metadata: crate::session::SessionMetadata::default(),
            outcome_authoritative: false,
            outcome: None,
        }
    }

    fn policy(local: bool, remote: bool, ids: &[&str]) -> ObservationPreferences {
        ObservationPreferences {
            host: herdr_update_coordinator::ObservationMode::Herdr,
            local,
            remote,
            machines: ids.iter().map(|id| (*id).to_owned()).collect(),
        }
    }

    fn catalog(profiles: &[(&str, &str, bool)]) -> SourceCatalog {
        SourceCatalog {
            initialized: true,
            machines: profiles
                .iter()
                .map(|(id, label, enabled)| crate::sources::MachineInfo {
                    id: (*id).to_owned(),
                    label: (*label).to_owned(),
                    remote_session: "session".to_owned(),
                    enabled: *enabled,
                    status: crate::sources::MachineStatus::Online,
                    error: None,
                })
                .collect(),
            error: None,
        }
    }

    fn publish(state: &mut AppState, source: &str, generation: u64, status: AgentStatus) {
        let records = [record("same-terminal", "same-pane", status)];
        let mut counts = SourceCounts {
            sessions: 1,
            ..SourceCounts::default()
        };
        match status {
            AgentStatus::Working => counts.working = 1,
            AgentStatus::Blocked => counts.blocked = 1,
            AgentStatus::Done => counts.done = 1,
            _ => counts.unknown = 1,
        }
        assert!(state.publish_source_snapshot(
            source,
            generation,
            records.iter(),
            counts,
            Arc::from(vec!["same-pane".to_owned()]),
            &[],
            Instant::now()
        ));
    }

    fn session_request(filter: SessionFilter, limit: usize) -> SessionPageRequest {
        SessionPageRequest {
            instance_id: "test".into(),
            filter,
            cursor: None,
            limit,
        }
    }

    #[test]
    fn session_wire_rejects_unknown_keys_and_invalid_filter_cursor_shapes() {
        assert!(serde_json::from_str::<SessionPageRequest>(
            r#"{"instance_id":"test","filter":"all","cursor":null,"limit":32,"unexpected":true}"#
        )
        .is_err());
        assert!(serde_json::from_str::<SessionPageRequest>(
            r#"{"instance_id":"test","filter":"invented","cursor":null,"limit":32}"#
        )
        .is_err());
        assert!(serde_json::from_str::<SessionPageCursor>(
            r#"{"instance_id":"test","revision":1,"filter":"all","position":{"source_id":0,"terminal_id":"t","extra":1}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<SessionKey>(
            r#"{"source_id":0,"generation":1,"terminal_id":"t","extra":1}"#
        )
        .is_err());
    }

    #[test]
    fn automation_pages_traverse_beyond_gui_cap_and_bind_cursor_to_instance_filter_revision() {
        let mut state = AppState::new();
        let automation =
            crate::automation::test_automation(PresentationTarget::from_scene(&state.scene()));
        state.set_automation(automation);
        connect(&mut state, "local.sock", 1);
        let records: Vec<_> = (0..140)
            .map(|index| {
                record(
                    &format!("terminal-{index:03}"),
                    &format!("pane-{index:03}"),
                    AgentStatus::Working,
                )
            })
            .collect();
        assert!(state.publish_source_snapshot(
            "local.sock",
            1,
            &records,
            SourceCounts {
                sessions: 140,
                working: 140,
                ..SourceCounts::default()
            },
            Arc::from(
                records
                    .iter()
                    .map(|record| record.pane_id.clone())
                    .collect::<Vec<_>>()
            ),
            &[],
            Instant::now(),
        ));
        assert_eq!(
            state
                .session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None)
                .rows
                .len(),
            128
        );
        let mut request = session_request(SessionFilter::All, 32);
        let mut ids = Vec::new();
        let first = state.automation_session_page(&request).unwrap();
        assert_eq!(first["matched"], 140);
        let cursor: SessionPageCursor =
            serde_json::from_value(first["next_cursor"].clone()).unwrap();
        let mut altered = request.clone();
        altered.cursor = Some(cursor.clone());
        altered.instance_id = "old-daemon".into();
        assert!(state.automation_session_page(&altered).is_err());
        altered.instance_id = "test".into();
        altered.filter = SessionFilter::Waiting;
        assert!(state.automation_session_page(&altered).is_err());
        altered.filter = SessionFilter::All;
        altered.cursor.as_mut().unwrap().instance_id = "old-daemon".into();
        assert!(state.automation_session_page(&altered).is_err());
        loop {
            let page = state.automation_session_page(&request).unwrap();
            ids.extend(page["rows"].as_array().unwrap().iter().map(|row| {
                assert_eq!(row["key"]["instance_id"], "test");
                row["key"]["terminal_id"].as_str().unwrap().to_owned()
            }));
            if page["next_cursor"].is_null() {
                break;
            }
            request.cursor = Some(serde_json::from_value(page["next_cursor"].clone()).unwrap());
        }
        assert_eq!(ids.len(), 140);
        assert_eq!(ids.first().unwrap(), "terminal-000");
        assert_eq!(ids.last().unwrap(), "terminal-139");
        let mut invalid = session_request(SessionFilter::All, 0);
        assert!(state.automation_session_page(&invalid).is_err());
        invalid.limit = 129;
        assert!(state.automation_session_page(&invalid).is_err());
        assert!(state.update_source(
            "local.sock",
            1,
            false,
            SourceCounts {
                sessions: 140,
                working: 140,
                ..SourceCounts::default()
            },
        ));
        assert!(state.automation_session_page(&request).is_err());
    }

    #[test]
    fn automation_detail_retains_offline_metadata_and_rejects_hidden_or_reconnected_key() {
        use crate::agent_outcome::{AgentOutcome, OutcomeReport};
        let mut state = AppState::new();
        let automation =
            crate::automation::test_automation(PresentationTarget::from_scene(&state.scene()));
        state.set_automation(automation);
        connect(&mut state, "local.sock", 1);
        let mut row = record("same-terminal", "same-pane", AgentStatus::Done);
        row.metadata.title = Some("Real title".into());
        row.metadata.cwd = Some("/private/secret".into());
        row.outcome_authoritative = true;
        row.outcome = Some(OutcomeReport {
            session: "current".into(),
            turn: "1".into(),
            outcome: AgentOutcome::Succeeded,
            at_unix_ms: 1,
        });
        assert!(state.publish_source_snapshot(
            "local.sock",
            1,
            [&row],
            SourceCounts {
                sessions: 1,
                done: 1,
                ..SourceCounts::default()
            },
            Arc::from(vec!["same-pane".to_owned()]),
            &[],
            Instant::now(),
        ));
        let key = state
            .session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None)
            .rows[0]
            .key
            .clone();
        let identity = SessionIdentity {
            instance_id: "test".into(),
            source_id: key.source_id,
            generation: key.generation,
            terminal_id: key.terminal_id.clone(),
        };
        let detail = state.automation_session_detail(&identity).unwrap();
        assert_eq!(detail["status"], "done");
        assert_eq!(detail["metadata"]["title"], "Real title");
        assert!(detail["metadata"].get("cwd").is_none());
        assert_eq!(detail["outcome"], Value::Null);
        assert_eq!(
            state
                .session_snapshot(
                    &SessionListOptions::with_filter(SessionFilter::All),
                    Some(&key)
                )
                .selected,
            Some(key.clone())
        );
        state.push_outcome(outcome("local.sock", 1));
        let completed = state.automation_session_detail(&identity).unwrap();
        assert_eq!(completed["outcome"], "succeeded");
        assert_eq!(completed["display_status"], "succeeded");
        assert_eq!(
            state
                .session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None)
                .rows[0]
                .outcome,
            Some(AgentOutcome::Succeeded)
        );
        assert_eq!(
            state
                .session_snapshot(
                    &SessionListOptions::with_filter(SessionFilter::All),
                    Some(&key)
                )
                .selected,
            Some(key.clone())
        );
        assert!(state.update_source(
            "local.sock",
            1,
            false,
            SourceCounts {
                sessions: 1,
                done: 1,
                ..SourceCounts::default()
            },
        ));
        let offline = state.automation_session_detail(&identity).unwrap();
        assert_eq!(offline["availability"], "offline");
        assert_eq!(offline["status"], "done");
        assert_eq!(offline["metadata"]["title"], "Real title");
        assert!(offline["metadata"].get("cwd").is_none());
        assert_eq!(offline["outcome"], Value::Null);
        assert_eq!(
            state
                .session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None)
                .rows[0]
                .outcome,
            None
        );
        assert_eq!(offline["display_status"], "offline");
        state.apply_observation_preferences(policy(false, false, &[]));
        assert!(state.automation_session_detail(&identity).is_err());
        state.apply_observation_preferences(policy(true, false, &[]));
        assert!(state.automation_session_detail(&identity).is_ok());
        connect(&mut state, "local.sock", 2);
        assert!(state.automation_session_detail(&identity).is_err());
        let mut wrong = identity;
        wrong.instance_id = "old-daemon".into();
        assert!(state.automation_session_detail(&wrong).is_err());
    }

    fn outcome(source: &str, generation: u64) -> OutcomeObservation {
        OutcomeObservation {
            source: source.to_owned(),
            generation,
            terminal_id: "same-terminal".to_owned(),
            pane_id: "same-pane".to_owned(),
            outcome: crate::agent_outcome::AgentOutcome::Succeeded,
            at_unix_ms: 1,
            observed_at: Instant::now(),
        }
    }

    #[test]
    fn display_bridge_observes_cached_offline_row_independently_of_prompt_readiness() {
        let mut state = AppState::new();
        connect(&mut state, "local.sock", 1);
        publish(&mut state, "local.sock", 1, AgentStatus::Working);
        let key = state
            .session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None)
            .rows[0]
            .key
            .clone();
        assert_eq!(
            state.session_view_for_key(&key),
            Some(
                state
                    .session_snapshot(&SessionListOptions::default(), None)
                    .rows[0]
                    .clone()
            )
        );
        assert!(state.update_source(
            "local.sock",
            1,
            false,
            SourceCounts {
                sessions: 1,
                working: 1,
                ..SourceCounts::default()
            }
        ));
        assert_eq!(
            state.session_view_for_key(&key).unwrap().display_status(),
            crate::session_view::DisplayStatus::Offline
        );
        assert!(state.prompt_available(&key).is_err());
        assert!(state.begin_source("local.sock".to_owned(), 2));
        assert_eq!(state.session_view_for_key(&key), None);
    }

    #[test]
    fn worktree_target_requires_current_included_coherent_absolute_local_source() {
        let mut state = AppState::new();
        let source = "/private/local.sock";
        let mut row = record("t", "p", AgentStatus::Working);
        row.metadata.workspace_id = Some("w".into());
        row.metadata.worktree = Some(std::sync::Arc::new(crate::session::WorkspaceWorktreeInfo {
            repo_key: "repo".into(),
            repo_name: "Project".into(),
            repo_root: "/repo".into(),
            checkout_path: "/checkout".into(),
            is_linked_worktree: true,
            pane_count: 1,
            tab_count: 1,
        }));
        connect(&mut state, source, 1);
        let counts = SourceCounts {
            sessions: 1,
            working: 1,
            ..SourceCounts::default()
        };
        assert!(state.publish_source_snapshot(
            source,
            1,
            [&row],
            counts,
            Arc::from(vec!["p".to_owned()]),
            &[],
            Instant::now()
        ));
        let key = state
            .session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None)
            .rows[0]
            .key
            .clone();
        assert_eq!(
            state.worktree_remove_target(&key).unwrap().workspace_id,
            "w"
        );
        state.apply_observation_preferences(policy(false, false, &[]));
        assert_eq!(
            state.worktree_remove_target(&key),
            Err(WorktreeRemoveTargetError::Stale)
        );
        state.apply_observation_preferences(policy(true, false, &[]));
        assert!(state.update_source(source, 1, false, counts));
        assert_eq!(
            state.worktree_remove_target(&key),
            Err(WorktreeRemoveTargetError::Offline)
        );
        connect(&mut state, source, 2);
        assert_eq!(
            state.worktree_remove_target(&key),
            Err(WorktreeRemoveTargetError::Stale)
        );
        assert!(state.publish_source_snapshot(
            source,
            2,
            [&row],
            counts,
            Arc::from(vec!["p".to_owned()]),
            &[],
            Instant::now()
        ));
        let new_key = state
            .session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None)
            .rows[0]
            .key
            .clone();
        state.request_shutdown();
        assert_eq!(
            state.worktree_remove_target(&new_key),
            Err(WorktreeRemoveTargetError::Offline)
        );

        let mut relative = AppState::new();
        connect(&mut relative, "local.sock", 1);
        assert!(relative.publish_source_snapshot(
            "local.sock",
            1,
            [&row],
            counts,
            Arc::from(vec!["p".to_owned()]),
            &[],
            Instant::now()
        ));
        let relative_key = relative
            .session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None)
            .rows[0]
            .key
            .clone();
        assert_eq!(
            relative.worktree_remove_target(&relative_key),
            Err(WorktreeRemoveTargetError::Stale)
        );
    }

    #[test]
    fn worktree_observation_requires_live_full_panes_and_same_registered_source() {
        let source = "/private/worktree.sock";
        let mut state = AppState::new();
        let mut row = record("target-terminal", "target-pane", AgentStatus::Working);
        row.metadata.workspace_id = Some("workspace".into());
        row.metadata.worktree = Some(Arc::new(crate::session::WorkspaceWorktreeInfo {
            repo_key: "repo".into(),
            repo_name: "Repo".into(),
            repo_root: "/repo".into(),
            checkout_path: "/repo/linked".into(),
            is_linked_worktree: true,
            pane_count: 1,
            tab_count: 1,
        }));
        connect(&mut state, source, 1);
        let live = SourceCounts {
            sessions: 1,
            working: 1,
            ..SourceCounts::default()
        };
        assert!(state.publish_source_snapshot(
            source,
            1,
            [&row],
            live,
            Arc::from(vec!["target-pane".to_owned()]),
            &[],
            Instant::now(),
        ));
        let key = state
            .session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None)
            .rows[0]
            .key
            .clone();
        let target = state.worktree_remove_target(&key).unwrap();
        assert_eq!(state.worktree_snapshot_revision(&target), Some(1));
        assert!(state.update_source(source, 1, true, live));
        assert_eq!(state.worktree_snapshot_revision(&target), Some(1));
        let mut different_source = target.clone();
        different_source.source = "/private/another.sock".into();
        assert_eq!(state.worktree_snapshot_revision(&different_source), None);
        assert_eq!(
            state.automation_worktree_observation(&target),
            Some(
                json!({"source_id":key.source_id,"source":source,"generation":1,
                        "snapshot_revision":1,"pane_absent":false,"session_absent":false})
            )
        );
        let empty: [AgentRecord; 0] = [];
        assert!(state.publish_source_snapshot(
            source,
            1,
            empty.iter(),
            SourceCounts::default(),
            Arc::from(vec!["target-pane".to_owned()]),
            &[],
            Instant::now(),
        ));
        // The agent exited, but the containing shell pane remains.
        assert_eq!(
            state.automation_worktree_observation(&target),
            Some(
                json!({"source_id":key.source_id,"source":source,"generation":1,
                        "snapshot_revision":2,"pane_absent":false,"session_absent":true})
            )
        );
        assert!(state.publish_source_snapshot(
            source,
            1,
            [&row],
            live,
            Arc::from([]),
            &[],
            Instant::now(),
        ));
        assert_eq!(
            state.automation_worktree_observation(&target),
            Some(
                json!({"source_id":key.source_id,"source":source,"generation":1,
                        "snapshot_revision":3,"pane_absent":true,"session_absent":false})
            )
        );
        assert!(state.update_source(source, 1, false, live));
        assert_eq!(state.worktree_snapshot_revision(&target), None);
        assert_eq!(state.automation_worktree_observation(&target), None);
        connect(&mut state, source, 2);
        assert_eq!(state.automation_worktree_observation(&target), None);
        assert!(!state.publish_source_snapshot(
            source,
            1,
            empty.iter(),
            SourceCounts::default(),
            Arc::from([]),
            &[],
            Instant::now(),
        ));
        assert_eq!(state.automation_worktree_observation(&target), None);
        assert!(state.publish_source_snapshot(
            source,
            2,
            empty.iter(),
            SourceCounts::default(),
            Arc::from([]),
            &[],
            Instant::now(),
        ));
        assert_eq!(
            state.automation_worktree_observation(&target),
            Some(
                json!({"source_id":key.source_id,"source":source,"generation":2,
                        "snapshot_revision":4,"pane_absent":true,"session_absent":true})
            )
        );
        assert!(state.remove_source(source, 2));
        connect(&mut state, source, 3);
        assert!(state.publish_source_snapshot(
            source,
            3,
            empty.iter(),
            SourceCounts::default(),
            Arc::from([]),
            &[],
            Instant::now(),
        ));
        assert_eq!(state.automation_worktree_observation(&target), None);
        let other = "/private/another.sock";
        connect(&mut state, other, 1);
        assert!(state.publish_source_snapshot(
            other,
            1,
            [&row],
            live,
            Arc::from(vec!["target-pane".to_owned()]),
            &[],
            Instant::now(),
        ));
        let other_key = state
            .session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None)
            .rows[0]
            .key
            .clone();
        let other_target = state.worktree_remove_target(&other_key).unwrap();
        assert_eq!(state.worktree_snapshot_revision(&other_target), Some(6));
    }

    #[test]
    fn remote_health_requires_selected_coherent_source_even_with_no_sessions() {
        let mut state = AppState::new();
        let source = remote_source("east");
        state.set_observation_catalog(catalog(&[("east", "East", true)]));
        state.apply_observation_preferences(policy(false, true, &["east"]));
        connect(&mut state, &source, 1);
        assert!(!state.has_healthy_observed_remote());
        let empty: [AgentRecord; 0] = [];
        assert!(state.publish_source_snapshot(
            &source,
            1,
            empty.iter(),
            SourceCounts::default(),
            Arc::from([]),
            &[],
            Instant::now(),
        ));
        assert!(state.has_healthy_observed_remote());
        assert_eq!(state.scene().phase, Phase::Idle);
        state.apply_observation_preferences(policy(false, false, &["east"]));
        assert!(!state.has_healthy_observed_remote());
        state.apply_observation_preferences(policy(false, true, &["east"]));
        assert!(!state.has_healthy_observed_remote());
        connect(&mut state, &source, 2);
        assert!(!state.has_healthy_observed_remote());
        assert!(state.publish_source_snapshot(
            &source,
            2,
            empty.iter(),
            SourceCounts::default(),
            Arc::from([]),
            &[],
            Instant::now(),
        ));
        state.set_observation_catalog(catalog(&[("east", "East", false)]));
        assert!(!state.has_healthy_observed_remote());
    }

    #[test]
    fn accepted_outcome_mirrors_only_eligible_current_source_without_consuming_reaction() {
        use crate::agent_outcome::{AgentOutcome, OutcomeReport};
        use crate::session_view::DisplayStatus;
        let mut state = AppState::new();
        connect(&mut state, "local.sock", 1);
        let mut row = record("same-terminal", "same-pane", AgentStatus::Working);
        row.outcome_authoritative = true;
        row.outcome = Some(OutcomeReport {
            session: "current".into(),
            turn: "1".into(),
            outcome: AgentOutcome::Succeeded,
            at_unix_ms: 1,
        });
        let counts = SourceCounts {
            sessions: 1,
            working: 1,
            ..SourceCounts::default()
        };
        assert!(state.publish_source_snapshot(
            "local.sock",
            1,
            [&row],
            counts,
            Arc::from(vec!["same-pane".to_owned()]),
            &[],
            Instant::now()
        ));
        let revision = state.session_revision();
        state.push_outcome(outcome("local.sock", 1));
        assert!(state.session_revision() > revision);
        assert_eq!(
            state
                .session_snapshot(&SessionListOptions::default(), None)
                .rows[0]
                .display_status(),
            DisplayStatus::Succeeded
        );
        assert_eq!(state.take_outcomes().len(), 1);
        let revision = state.session_revision();
        let mut stale = outcome("local.sock", 1);
        stale.pane_id = "wrong".into();
        state.push_outcome(stale);
        assert_eq!(state.session_revision(), revision);
        let active = SessionStatusUpdate {
            source: "local.sock",
            generation: 1,
            terminal_id: "same-terminal",
            pane_id: "same-pane",
            status: AgentStatus::Working,
            counts,
            completion: false,
            observed_at: Instant::now(),
        };
        assert!(state.update_session_status(active));
        assert_eq!(
            state
                .session_snapshot(&SessionListOptions::default(), None)
                .rows[0]
                .display_status(),
            DisplayStatus::Running
        );
        state.update_source("local.sock", 1, false, counts);
        assert_eq!(
            state
                .session_snapshot(&SessionListOptions::default(), None)
                .rows[0]
                .display_status(),
            DisplayStatus::Offline
        );
        state.push_outcome(outcome("local.sock", 1));
        assert!(state.take_outcomes().is_empty());
    }

    #[test]
    fn policy_filters_phase_rows_selection_and_pending_effects_without_discarding_local() {
        let mut state = AppState::new();
        let remote = remote_source("east");
        state.set_observation_catalog(catalog(&[("east", "East", true)]));
        assert!(!state.begin_source(remote.clone(), 1));
        connect(&mut state, "local.sock", 1);
        publish(&mut state, "local.sock", 1, AgentStatus::Working);
        state.apply_observation_preferences(policy(true, true, &["east"]));
        connect(&mut state, &remote, 1);
        publish(&mut state, &remote, 1, AgentStatus::Blocked);
        let both =
            state.session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None);
        assert_eq!(both.total, 2);
        assert_eq!(state.scene().phase, Phase::Waiting);
        assert_ne!(both.rows[0].key, both.rows[1].key);
        let local_key = both.rows[0].key.clone();
        state.push_completion(observation(&remote, 1, Instant::now()));
        state.push_outcome(outcome(&remote, 1));
        let revision = state.session_revision();
        state.apply_observation_preferences(policy(true, false, &["east"]));
        assert!(state.session_revision() > revision);
        assert_eq!(state.scene().phase, Phase::Running);
        let local = state.session_snapshot(
            &SessionListOptions::with_filter(SessionFilter::Waiting),
            Some(&local_key),
        );
        assert_eq!((local.total, local.matched, local.omitted), (1, 0, 0));
        assert_eq!(local.selected, Some(local_key.clone()));
        assert!(state.take_completions().is_empty());
        state.push_completion(observation(&remote, 1, Instant::now()));
        state.push_outcome(outcome(&remote, 1));
        assert!(state.take_completions().is_empty());
        assert!(state.take_outcomes().is_empty());
        state.apply_observation_preferences(policy(false, false, &["east"]));
        assert_eq!(state.scene().phase, Phase::Unknown);
        assert_eq!(
            state
                .session_snapshot(&SessionListOptions::default(), Some(&local_key))
                .selected,
            None
        );
        assert_eq!(
            state
                .session_snapshot(&SessionListOptions::default(), None)
                .total,
            0
        );
        publish(&mut state, "local.sock", 1, AgentStatus::Done);
        state.apply_observation_preferences(policy(true, false, &["east"]));
        assert_eq!(
            state
                .session_snapshot(&SessionListOptions::default(), None)
                .rows[0]
                .status,
            AgentStatus::Done
        );
    }

    #[test]
    fn catalog_disable_delete_rename_and_reinclude_reject_old_generations() {
        let mut state = AppState::new();
        let east = remote_source("east");
        let west = remote_source("west");
        state.set_observation_catalog(catalog(&[("east", "East", true), ("west", "West", true)]));
        state.apply_observation_preferences(policy(false, true, &["east", "west"]));
        connect(&mut state, &east, 1);
        connect(&mut state, &west, 1);
        publish(&mut state, &east, 1, AgentStatus::Working);
        publish(&mut state, &west, 1, AgentStatus::Blocked);
        let rows = state
            .session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None)
            .rows;
        assert_eq!(
            (rows[0].source_label.as_str(), rows[1].source_label.as_str()),
            ("East", "West")
        );
        assert_ne!(rows[0].key, rows[1].key);
        let previous = state.session_revision();
        state.set_observation_catalog(catalog(&[
            ("east", "Renamed", true),
            ("west", "West", true),
        ]));
        assert!(state.session_revision() > previous);
        assert_eq!(
            state
                .session_snapshot(&SessionListOptions::default(), None)
                .rows[0]
                .source_label,
            "Renamed"
        );
        state.set_observation_catalog(catalog(&[("east", "Renamed", false)]));
        assert_eq!(
            state
                .session_snapshot(&SessionListOptions::default(), None)
                .total,
            0
        );
        assert_eq!(state.scene().phase, Phase::Unknown);
        assert!(!state.begin_source(east.clone(), 1));
        let empty: [AgentRecord; 0] = [];
        assert!(!state.publish_source_snapshot(
            &west,
            1,
            empty.iter(),
            SourceCounts::default(),
            Arc::from([]),
            &[],
            Instant::now()
        ));
        state.set_observation_catalog(catalog(&[("east", "Renamed", true)]));
        assert!(!state.begin_source(east.clone(), 1));
        state.push_completion(observation(&east, 1, Instant::now()));
        state.push_outcome(outcome(&east, 1));
        assert!(state.take_completions().is_empty());
        assert!(state.take_outcomes().is_empty());
        connect(&mut state, &east, 2);
        publish(&mut state, &east, 2, AgentStatus::Done);
        assert!(!state.update_source(&east, 1, true, SourceCounts::default()));
        assert!(!state.update_session_status(SessionStatusUpdate {
            source: &east,
            generation: 1,
            terminal_id: "same-terminal",
            pane_id: "same-pane",
            status: AgentStatus::Blocked,
            counts: SourceCounts::default(),
            completion: true,
            observed_at: Instant::now(),
        }));
        let revision = state.session_revision();
        publish(&mut state, &east, 2, AgentStatus::Done);
        assert_eq!(state.session_revision(), revision);
        assert_eq!(
            state
                .session_snapshot(&SessionListOptions::default(), None)
                .total,
            1
        );
        assert!(state.take_completions().is_empty());
    }

    #[test]
    fn session_rows_remain_offline_until_coherent_replacement_or_empty_snapshot() {
        let mut state = AppState::new();
        connect(&mut state, "one.sock", 1);
        let initial_records = [record("terminal-a", "pane-a", AgentStatus::Working)];
        let initial_counts = SourceCounts {
            sessions: 1,
            working: 1,
            ..SourceCounts::default()
        };
        assert!(state.publish_source_snapshot(
            "one.sock",
            1,
            initial_records.iter(),
            initial_counts,
            Arc::from(vec!["pane-a".to_owned()]),
            &[],
            Instant::now(),
        ));
        let live =
            state.session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None);
        assert_eq!(live.rows.len(), 1);
        assert_eq!(
            live.rows[0].availability,
            crate::session_view::Availability::Live
        );
        assert_eq!(live.rows[0].pane_id, "pane-a");

        assert!(state.update_source("one.sock", 1, false, initial_counts));
        let offline =
            state.session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None);
        assert_eq!(offline.rows.len(), 1);
        assert_eq!(
            offline.rows[0].availability,
            crate::session_view::Availability::Offline
        );
        assert_eq!(offline.rows[0].status, AgentStatus::Working);

        assert!(state.begin_source("one.sock".to_owned(), 2));
        let restarted =
            state.session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None);
        assert_eq!(restarted.rows.len(), 1);
        assert_eq!(restarted.rows[0].key.generation, 2);
        assert_eq!(
            restarted.rows[0].availability,
            crate::session_view::Availability::Offline
        );
        let in_progress = [record("terminal-a", "pane-a", AgentStatus::Done)];
        assert!(!state.update_session_status(SessionStatusUpdate {
            source: "one.sock",
            generation: 2,
            terminal_id: "terminal-a",
            pane_id: "pane-a",
            status: AgentStatus::Done,
            counts: SourceCounts {
                sessions: 1,
                done: 1,
                ..SourceCounts::default()
            },
            completion: true,
            observed_at: Instant::now(),
        }));
        let still_offline =
            state.session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None);
        assert_eq!(still_offline.rows[0].status, AgentStatus::Working);
        assert_eq!(
            still_offline.rows[0].availability,
            crate::session_view::Availability::Offline
        );

        assert!(state.publish_source_snapshot(
            "one.sock",
            2,
            in_progress.iter(),
            SourceCounts {
                sessions: 1,
                done: 1,
                ..SourceCounts::default()
            },
            Arc::from(vec!["pane-a".to_owned()]),
            &[],
            Instant::now(),
        ));
        let replaced =
            state.session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None);
        assert_eq!(replaced.rows[0].status, AgentStatus::Done);
        assert_eq!(
            replaced.rows[0].availability,
            crate::session_view::Availability::Live
        );

        let before_rejected =
            state.session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None);
        let before_revision = state.session_revision();
        let before_scene = state.scene();
        assert!(!state.update_session_status(SessionStatusUpdate {
            source: "one.sock",
            generation: 2,
            terminal_id: "terminal-a",
            pane_id: "wrong-pane",
            status: AgentStatus::Working,
            counts: SourceCounts {
                sessions: 1,
                working: 1,
                ..SourceCounts::default()
            },
            completion: true,
            observed_at: Instant::now(),
        }));
        assert_eq!(state.session_revision(), before_revision);
        assert_eq!(
            state.session_snapshot(&SessionListOptions::default(), None),
            before_rejected
        );
        let after_scene = state.scene();
        assert_eq!(after_scene.sessions, before_scene.sessions);
        assert_eq!(after_scene.working, before_scene.working);
        assert_eq!(after_scene.done, before_scene.done);
        assert!(state.take_completions().is_empty());

        assert!(state.begin_source("one.sock".to_owned(), 3));
        let empty: [AgentRecord; 0] = [];
        assert!(state.publish_source_snapshot(
            "one.sock",
            3,
            empty.iter(),
            SourceCounts::default(),
            Arc::from([]),
            &[],
            Instant::now(),
        ));
        let cleared =
            state.session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None);
        assert!(cleared.rows.is_empty());
        assert_eq!(cleared.total, 0);
        assert_eq!(cleared.matched, 0);
    }
    #[test]
    fn completions_are_consumed_once_and_purged_on_reconnect_or_disconnect() {
        let mut state = AppState::new();
        connect(&mut state, "one.sock", 1);
        assert!(state.update_source("one.sock", 1, true, SourceCounts::default()));
        state.push_completion(observation("one.sock", 1, Instant::now()));
        assert_eq!(state.take_completions().len(), 1);
        state.push_completion(observation("one.sock", 1, Instant::now()));
        assert!(state.begin_source("one.sock".to_owned(), 2));
        assert!(state.take_completions().is_empty());
        assert!(state.update_source("one.sock", 2, true, SourceCounts::default()));
        state.push_completion(observation("one.sock", 2, Instant::now()));
        assert!(state.update_source("one.sock", 2, false, SourceCounts::default()));
        assert!(state.take_completions().is_empty());
    }

    #[test]
    fn stale_completion_observations_are_discarded() {
        let mut state = AppState::new();
        connect(&mut state, "one.sock", 1);
        assert!(state.update_source("one.sock", 1, true, SourceCounts::default()));
        state.push_completion(observation(
            "one.sock",
            1,
            Instant::now() - Duration::from_secs(31),
        ));
        assert!(state.take_completions().is_empty());
    }

    #[test]
    fn initial_scene_has_no_ready_connection() {
        let scene = AppState::new().scene();
        assert_eq!(scene.phase, Phase::Unknown);
        assert_eq!(scene.sessions, 0);
        assert_eq!(scene.connected_sources, 0);
        assert_eq!(scene.disconnected_sources, 0);
    }

    #[test]
    fn connected_zero_agents_is_idle_but_disconnect_is_unknown() {
        let mut state = AppState::new();
        connect(&mut state, "herdr.sock", 1);
        assert!(state.update_source("herdr.sock", 1, true, SourceCounts::default()));
        assert_eq!(state.scene().phase, Phase::Idle);
        assert!(state.update_source(
            "herdr.sock",
            1,
            true,
            SourceCounts {
                sessions: 1,
                working: 1,
                ..SourceCounts::default()
            },
        ));
        assert_eq!(state.scene().phase, Phase::Running);
        assert!(state.update_source(
            "herdr.sock",
            1,
            false,
            SourceCounts {
                sessions: 1,
                working: 1,
                ..SourceCounts::default()
            },
        ));
        let scene = state.scene();
        assert_eq!(scene.phase, Phase::Unknown);
        assert_eq!(scene.sessions, 1);
        assert_eq!(scene.unknown, 1);
        assert_eq!(scene.connected_sources, 0);
        assert_eq!(scene.disconnected_sources, 1);
    }

    #[test]
    fn stale_generation_cannot_commit_or_remove_new_source() {
        let mut state = AppState::new();
        connect(&mut state, "one.sock", 1);
        assert!(state.update_source(
            "one.sock",
            1,
            true,
            SourceCounts {
                sessions: 1,
                working: 1,
                ..SourceCounts::default()
            },
        ));
        assert!(state.begin_source("one.sock".to_owned(), 2));
        assert!(!state.update_source(
            "one.sock",
            1,
            true,
            SourceCounts {
                sessions: 1,
                blocked: 1,
                ..SourceCounts::default()
            },
        ));
        assert!(!state.remove_source("one.sock", 1));
        assert!(state.update_source(
            "one.sock",
            2,
            true,
            SourceCounts {
                sessions: 1,
                done: 1,
                ..SourceCounts::default()
            },
        ));
        assert_eq!(state.scene().done, 1);
        assert_eq!(state.scene().working, 0);
    }

    #[test]
    fn repeated_generations_keep_last_known_sessions_until_new_snapshot() {
        let mut state = AppState::new();
        connect(&mut state, "one.sock", 1);
        assert!(state.update_source(
            "one.sock",
            1,
            true,
            SourceCounts {
                sessions: 2,
                working: 2,
                ..SourceCounts::default()
            },
        ));
        assert!(state.begin_source("one.sock".to_owned(), 2));
        assert!(state.update_source(
            "one.sock",
            2,
            false,
            SourceCounts {
                sessions: 2,
                unknown: 2,
                ..SourceCounts::default()
            },
        ));
        assert!(state.begin_source("one.sock".to_owned(), 3));
        let scene = state.scene();
        assert_eq!(scene.sessions, 2);
        assert_eq!(scene.unknown, 2);
        assert_eq!(scene.phase, Phase::Unknown);
        assert!(state.update_source("one.sock", 3, true, SourceCounts::default(),));
        assert_eq!(state.scene().sessions, 0);
        assert_eq!(state.scene().phase, Phase::Idle);
    }

    #[test]
    fn independent_sources_with_equal_terminal_ids_are_counted_independently() {
        let mut state = AppState::new();
        connect(&mut state, "one.sock", 1);
        connect(&mut state, "two.sock", 1);
        let counts = SourceCounts {
            sessions: 1,
            working: 1,
            ..SourceCounts::default()
        };
        assert!(state.update_source("one.sock", 1, true, counts));
        assert!(state.update_source("two.sock", 1, true, counts));
        let scene = state.scene();
        assert_eq!(scene.sessions, 2);
        assert_eq!(scene.working, 2);
        assert_eq!(scene.connected_sources, 2);
    }

    #[test]
    fn remote_snapshot_preserves_title_provenance_and_source_on_status_and_rename() {
        let mut state = AppState::new();
        let east = remote_source("east");
        let west = remote_source("west");
        state.set_observation_catalog(catalog(&[("east", "East", true), ("west", "West", true)]));
        state.apply_observation_preferences(policy(false, true, &["east", "west"]));
        connect(&mut state, &east, 1);
        connect(&mut state, &west, 1);
        let mut east_record = record("shared", "pane-east", AgentStatus::Working);
        east_record.metadata.title = Some("Build east".into());
        east_record.metadata.cwd = Some("/projects/east".into());
        east_record.metadata.tab_label = Some("2".into());
        let mut west_record = record("shared", "pane-west", AgentStatus::Blocked);
        west_record.metadata.title = Some("Build west".into());
        let counts = SourceCounts {
            sessions: 1,
            working: 1,
            ..SourceCounts::default()
        };
        assert!(state.publish_source_snapshot(
            &east,
            1,
            [&east_record],
            counts,
            Arc::from(vec!["pane-east".to_owned()]),
            &[],
            Instant::now()
        ));
        let waiting = SourceCounts {
            sessions: 1,
            blocked: 1,
            ..SourceCounts::default()
        };
        assert!(state.publish_source_snapshot(
            &west,
            1,
            [&west_record],
            waiting,
            Arc::from(vec!["pane-west".to_owned()]),
            &[],
            Instant::now()
        ));
        let rows = state
            .session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None)
            .rows;
        assert_ne!(rows[0].key, rows[1].key);
        assert_eq!(rows[0].metadata, east_record.metadata);
        assert_eq!(rows[1].metadata.title.as_deref(), Some("Build west"));
        assert_eq!(rows[0].source_label, "East");
        assert_eq!(rows[1].source_label, "West");

        assert!(state.update_session_status(SessionStatusUpdate {
            source: &east,
            generation: 1,
            terminal_id: "shared",
            pane_id: "pane-east",
            status: AgentStatus::Done,
            counts: SourceCounts {
                sessions: 1,
                done: 1,
                ..SourceCounts::default()
            },
            completion: false,
            observed_at: Instant::now(),
        }));
        state.set_observation_catalog(catalog(&[
            ("east", "Renamed", true),
            ("west", "West", true),
        ]));
        let rows = state
            .session_snapshot(&SessionListOptions::with_filter(SessionFilter::All), None)
            .rows;
        assert_eq!(rows[0].metadata, east_record.metadata);
        assert_eq!(rows[0].source_label, "Renamed");
        assert_eq!(rows[0].status, AgentStatus::Done);
        assert_eq!(rows[1].metadata, west_record.metadata);
        assert_eq!(
            state
                .session_snapshot(
                    &SessionListOptions::with_filter(SessionFilter::Completed),
                    None
                )
                .matched,
            1
        );
        assert_eq!(
            state
                .session_snapshot(
                    &SessionListOptions::with_filter(SessionFilter::Waiting),
                    None
                )
                .matched,
            1
        );
    }

    #[test]
    fn legacy_and_recovery_revisions_precede_automation_reduction() {
        use crate::automation::{
            lock_automation, test_automation, PresentationAction, PresentationPatch,
            PresentationRequest, PresentationTarget,
        };

        let mut state = AppState::new();
        let automation = test_automation(PresentationTarget::from_scene(&state.scene()));
        state.set_automation(automation.clone());
        let stale = PresentationRequest {
            instance_id: "test".into(),
            operation_id: "stale".into(),
            expected_revision: Some(0),
            action: PresentationAction::Set {
                patch: PresentationPatch {
                    bubble_visible: Some(false),
                    ..PresentationPatch::default()
                },
            },
        };
        lock_automation(&automation).submit(stale.clone()).unwrap();
        state.apply_control("hide").unwrap();
        assert_eq!(lock_automation(&automation).snapshot().revision, 1);
        let stale_target = PresentationTarget::from_scene(&state.scene())
            .applying(&stale.action)
            .unwrap();
        assert!(lock_automation(&automation)
            .start_request("stale", stale_target)
            .is_err());
        assert!(state.scene().bubble_visible);

        let current = PresentationRequest {
            operation_id: "current".into(),
            expected_revision: Some(1),
            ..stale
        };
        lock_automation(&automation)
            .submit(current.clone())
            .unwrap();
        let current_target = PresentationTarget::from_scene(&state.scene())
            .applying(&current.action)
            .unwrap();
        lock_automation(&automation)
            .start_request("current", current_target)
            .unwrap();
        state.apply_presentation(&current.action).unwrap();
        assert_eq!(lock_automation(&automation).snapshot().revision, 2);
        state.apply_control("hide").unwrap();
        assert_eq!(lock_automation(&automation).snapshot().revision, 2);
        state.recover_interaction();
        assert_eq!(lock_automation(&automation).snapshot().revision, 3);
        assert!(state.scene().visible);
    }

    #[test]
    fn controls_preserve_scale_and_stop_semantics() {
        let mut state = AppState::new();
        assert!(state.apply_control("toggle").is_ok());
        assert!(!state.scene().visible);
        assert!(state.apply_control("toggle_passthrough").is_ok());
        assert!(state.scene().passthrough);
        assert!(state.set_scale(0.873125));
        assert!(!state.set_scale(f64::NAN));
        assert_eq!(state.scene().scale, 0.873125);
        state.request_shutdown();
        assert!(state.scene().shutdown);
    }

    #[test]
    fn full_and_alpha_passthrough_remain_independent() {
        let mut state = AppState::new();
        assert!(state.apply_control("alpha_passthrough").is_ok());
        assert!(state.scene().alpha_passthrough);
        assert!(!state.scene().passthrough);

        assert!(state.apply_control("toggle_passthrough").is_ok());
        assert!(state.scene().passthrough);
        assert!(state.scene().alpha_passthrough);

        assert!(state.apply_control("toggle_alpha_passthrough").is_ok());
        assert!(!state.scene().alpha_passthrough);
        assert!(state.scene().passthrough);

        assert!(state.apply_control("toggle_passthrough").is_ok());
        assert!(!state.scene().passthrough);
        assert!(!state.scene().alpha_passthrough);
    }

    #[test]
    fn bubble_controls_update_visibility_and_placement_without_touching_passthrough() {
        let mut state = AppState::new();
        assert!(state.apply_control("hide_bubble").is_ok());
        assert!(!state.scene().bubble_visible);
        assert!(state.apply_control("bubble_left").is_ok());
        assert_eq!(state.scene().bubble_placement, BubblePlacement::Left);
        assert!(state.apply_control("show_bubble").is_ok());
        assert!(state.scene().bubble_visible);
        assert!(state.apply_control("bubble_auto").is_ok());
        assert_eq!(state.scene().bubble_placement, BubblePlacement::Auto);
        assert!(!state.scene().passthrough);
        assert!(!state.scene().alpha_passthrough);
    }

    #[test]
    fn recovery_entry_requires_missing_interactive_surfaces_or_full_passthrough() {
        for (visible, bubble_visible, passthrough, shutdown, expected) in [
            (true, true, false, false, false),
            (true, false, false, false, false),
            (false, true, false, false, false),
            (false, false, false, false, true),
            (true, true, true, false, true),
            (true, false, true, false, true),
            (false, true, true, false, true),
            (false, false, true, false, true),
            (true, true, false, true, false),
            (false, false, false, true, false),
            (false, true, true, true, false),
        ] {
            for alpha_passthrough in [false, true] {
                let mut state = AppState::new();
                state.visible = visible;
                state.bubble_visible = bubble_visible;
                state.passthrough = passthrough;
                state.alpha_passthrough = alpha_passthrough;
                state.shutdown = shutdown;
                assert_eq!(
                    state.scene().needs_recovery_entry(),
                    expected,
                    "visible={visible}, bubble_visible={bubble_visible}, passthrough={passthrough}, shutdown={shutdown}, alpha_passthrough={alpha_passthrough}"
                );
            }
        }
    }

    #[test]
    fn recovery_restores_character_interaction_without_changing_other_settings() {
        for bubble_visible in [false, true] {
            let mut state = AppState::new();
            state.visible = false;
            state.passthrough = true;
            state.alpha_passthrough = true;
            state.bubble_visible = bubble_visible;
            state.bubble_placement = BubblePlacement::Left;
            state.scale = 0.873125;
            state.reset_position_revision = 7;
            let lifecycle = LifecycleSettings {
                auto_start: false,
                exit_with_herdr: false,
            };
            state.set_lifecycle_settings(lifecycle);

            state.recover_interaction();

            let scene = state.scene();
            assert!(scene.visible);
            assert!(!scene.passthrough);
            assert_eq!(scene.bubble_visible, bubble_visible);
            assert!(scene.alpha_passthrough);
            assert_eq!(scene.bubble_placement, BubblePlacement::Left);
            assert_eq!(scene.scale, 0.873125);
            assert_eq!(scene.reset_position_revision, 7);
            assert_eq!(state.lifecycle_settings(), lifecycle);
        }
    }

    pub(crate) fn tern_test_state(mode: ObservationMode) -> AppState {
        use herdr_update_coordinator::ExecutableIdentity;
        use std::collections::BTreeMap;
        use std::path::PathBuf;
        let binary = ExecutableIdentity {
            path: PathBuf::from("/isolated/pet"),
            sha256: "binary".into(),
        };
        let mut state = AppState::new();
        state.set_observation_mode(mode);
        state.set_update_context(UpdateContext {
            executable: binary.path.clone(),
            version: "0.2.0".into(),
            instance_id: "daemon-a".into(),
            config_dir: PathBuf::from("/isolated/config"),
            host_plugin_config_dir: None,
            state_dir: PathBuf::from("/isolated/state"),
            herdr_socket: PathBuf::from("/isolated/herdr.sock"),
            running: binary,
            running_origin: None,
            assets_override: None,
            environment: BTreeMap::new(),
            locale: "en".into(),
        });
        state
    }

    pub(crate) fn tern_test_request(window: &str) -> TernExchangeRequest {
        use crate::tern::{TernAgent, TernAgentState, TernHostInventory, TernHostStatus};
        TernExchangeRequest {
            version: 2,
            kind: "tern".into(),
            window: window.into(),
            epoch: 1,
            sequence: 1,
            instance_id: None,
            hosts: vec![TernHostInventory {
                host: "0".into(),
                label: "Tern".into(),
                is_local: true,
                observation_generation: 1,
                status: TernHostStatus::Available,
                agents: vec![TernAgent {
                    pane: "pane-a".into(),
                    title: window.into(),
                    cwd: "/isolated/repo".into(),
                    session_id: Some("session-a".into()),
                    session_label: Some("Session".into()),
                    tab_id: None,
                    tab_label: None,
                    state: TernAgentState::Working,
                    delivery_error: None,
                }],
            }],
            results: vec![],
        }
    }

    pub(crate) fn age_tern_test_source(state: &mut AppState, window: &str) {
        state.tern.get_mut(window).unwrap().last_seen =
            Instant::now() - crate::tern::FRESHNESS - Duration::from_millis(1);
    }

    pub(crate) fn set_tern_test_last_seen(
        state: &mut AppState,
        window: &str,
        host: Option<&str>,
        now: Instant,
    ) {
        let entry = state.tern.get_mut(window).unwrap();
        if let Some(host) = host {
            entry.hosts.get_mut(host).unwrap().last_seen = now;
        } else {
            entry.last_seen = now;
            for host in entry.hosts.values_mut() {
                host.last_seen = now;
            }
        }
    }

    pub(crate) fn tern_test_exchange_at(
        state: &mut AppState,
        request: TernExchangeRequest,
        now: Instant,
    ) -> TernExchangeReply {
        state.tern_exchange_at(request, now)
    }

    fn tern_test_key(state: &AppState, window: &str) -> SessionKey {
        state
            .session_snapshot(&SessionListOptions::default(), None)
            .rows
            .into_iter()
            .find(|row| row.metadata.title.as_deref() == Some(window))
            .expect("registered Tern row")
            .key
    }

    #[test]
    fn auto_observes_both_providers_without_conflating_matching_sessions() {
        let mut state = tern_test_state(ObservationMode::Both);
        assert_eq!(AppState::new().observation_mode(), ObservationMode::Both);
        assert_eq!(state.observation_preferences().host, ObservationMode::Both);
        let local = "/isolated/herdr.sock";
        let request = tern_test_request("shared-window");
        let (records, counts) = request.normalize().unwrap().remove(0);
        connect(&mut state, local, 1);
        assert!(state.update_source(local, 1, true, counts));
        assert_eq!(state.host_connections(), HostConnections::default());
        assert!(state.publish_source_snapshot(
            local,
            1,
            records.iter(),
            counts,
            Arc::from(vec!["pane-a".to_owned()]),
            &[],
            Instant::now(),
        ));
        assert!(state.tern_exchange(request).ok);
        assert_eq!(
            state.host_connections(),
            HostConnections { herdr: 1, tern: 1 }
        );
        let snapshot = state.session_snapshot(&SessionListOptions::default(), None);
        assert_eq!(snapshot.total, 2);
        assert_eq!(state.scene().sessions, 2);
        assert_eq!(snapshot.rows[0].metadata, snapshot.rows[1].metadata);
        assert_eq!(
            snapshot.rows[0].key.terminal_id,
            snapshot.rows[1].key.terminal_id
        );
        assert_ne!(
            snapshot.rows[0].key.source_id,
            snapshot.rows[1].key.source_id
        );
        assert!(matches!(
            state.prompt_route(&snapshot.rows[0].key).unwrap().1,
            SourceRoute::HerdrLocal(_)
        ));
        assert!(matches!(
            state.prompt_route(&snapshot.rows[1].key).unwrap().1,
            SourceRoute::TernWindow { .. }
        ));
        let epoch = state.observation_epoch();
        state.set_observation_mode(ObservationMode::Herdr);
        assert_eq!(state.observation_epoch(), epoch + 1);
        assert_eq!(
            state.host_connections(),
            HostConnections { herdr: 1, tern: 0 }
        );
        assert_eq!(state.scene().sessions, 1);
        assert!(state.session_view_for_key(&snapshot.rows[1].key).is_none());
        assert!(state.prompt_route(&snapshot.rows[0].key).is_ok());
        assert!(!state.scene().shutdown);
        assert!(!state.mark_source_offline(local, 2));
        assert_eq!(
            state.host_connections(),
            HostConnections { herdr: 1, tern: 0 }
        );
        assert!(state.mark_source_offline(local, 1));
        assert_eq!(state.host_connections(), HostConnections::default());
        assert_eq!(state.scene().sessions, 1);
        assert_eq!(state.scene().unknown, 1);
        assert_eq!(
            state
                .session_view_for_key(&snapshot.rows[0].key)
                .unwrap()
                .availability,
            crate::session_view::Availability::Offline
        );
        assert_eq!(
            state.prompt_route(&snapshot.rows[0].key),
            Err(crate::herdr::PromptError::Offline)
        );
    }

    #[test]
    fn rapid_host_transitions_fence_local_generations_and_tern_incarnations() {
        use crate::herdr::PromptError;
        let mut state = tern_test_state(ObservationMode::Both);
        let local = "/isolated/herdr.sock";
        connect(&mut state, local, 41);
        publish(&mut state, local, 41, AgentStatus::Working);
        let mut request = tern_test_request("window-a");
        assert!(state.tern_exchange(request.clone()).ok);
        let key = tern_test_key(&state, "window-a");
        state.set_observation_mode(ObservationMode::Tern);
        assert_eq!(state.source_generation(local), None);
        assert_eq!(state.retired_source_generation(local), Some(41));
        assert_eq!(state.scene().sessions, 1);
        assert!(!state.begin_source(local.to_owned(), 42));
        assert!(!state.publish_source_snapshot(
            local,
            41,
            std::iter::empty::<&AgentRecord>(),
            SourceCounts::default(),
            Arc::from(Vec::<String>::new()),
            &[],
            Instant::now(),
        ));
        state.set_observation_mode(ObservationMode::Herdr);
        assert_eq!(state.host_connections(), HostConnections::default());
        assert_eq!(state.scene().sessions, 0);
        assert_eq!(state.prompt_available(&key), Err(PromptError::StaleTarget));
        state.set_observation_mode(ObservationMode::Both);
        assert!(!state.begin_source(local.to_owned(), 41));
        connect(&mut state, local, 42);
        publish(&mut state, local, 42, AgentStatus::Working);
        assert!(!state.update_source(local, 41, true, SourceCounts::default()));
        assert!(!state.remove_source(local, 41));
        request.instance_id = Some("daemon-a".into());
        request.sequence = 2;
        assert_eq!(
            state.tern_exchange(request.clone()).error_code,
            Some(TernExchangeErrorCode::SourceRetired)
        );
        request.instance_id = None;
        assert_eq!(
            state.tern_exchange(request.clone()).error_code,
            Some(TernExchangeErrorCode::SourceRetired)
        );
        request.epoch = 2;
        request.sequence = 1;
        let fresh = state.tern_exchange(request.clone());
        assert!(fresh.ok && fresh.command.is_none());
        assert_eq!(fresh.hosts[0].generation, 2);
        let fresh_key = tern_test_key(&state, "window-a");
        assert_ne!(fresh_key.source_id, key.source_id);
        assert!(matches!(
            state.queue_tern_prompt(key, "window-a", 2, "0", 1, "pane-a", "stale ask"),
            Err(PromptError::StaleTarget)
        ));
        assert_eq!(
            state.host_connections(),
            HostConnections { herdr: 1, tern: 1 }
        );
        assert_eq!(state.scene().sessions, 2);
    }

    #[test]
    fn host_disable_finalizes_pending_tern_truthfully_without_reoffer_or_replay() {
        use crate::herdr::PromptError;
        for offered in [false, true] {
            let mut state = tern_test_state(ObservationMode::Both);
            let mut request = tern_test_request("window-a");
            assert!(state.tern_exchange(request.clone()).ok);
            let key = tern_test_key(&state, "window-a");
            let (_, receipt) = state
                .queue_tern_prompt(key.clone(), "window-a", 1, "0", 1, "pane-a", "one ask")
                .unwrap();
            request.instance_id = Some("daemon-a".into());
            request.sequence = 2;
            if offered {
                assert!(state.tern_exchange(request.clone()).command.is_some());
            }
            state.set_observation_mode(ObservationMode::Herdr);
            assert_eq!(
                receipt.try_recv(),
                Ok(Err(if offered {
                    PromptError::UnknownDelivery
                } else {
                    PromptError::StaleTarget
                }))
            );
            state.set_observation_mode(ObservationMode::Both);
            assert_eq!(
                state.tern_exchange(request.clone()).error_code,
                Some(TernExchangeErrorCode::SourceRetired)
            );
            request.instance_id = None;
            request.epoch = 2;
            request.sequence = 1;
            assert!(state.tern_exchange(request.clone()).command.is_none());
            request.instance_id = Some("daemon-a".into());
            request.sequence = 2;
            let poll = state.tern_exchange(request);
            assert!(poll.ok && poll.command.is_none());
            assert_eq!(state.prompt_available(&key), Err(PromptError::StaleTarget));
            assert!(receipt.try_recv().is_err());
        }
    }

    #[test]
    fn scope_pause_and_shutdown_preserve_honest_tern_prompt_outcomes() {
        use crate::herdr::PromptError;
        for offered in [false, true] {
            for shutdown in [false, true] {
                let mut state = tern_test_state(ObservationMode::Both);
                let mut poll = tern_test_request("window-a");
                assert!(state.tern_exchange(poll.clone()).ok);
                let key = tern_test_key(&state, "window-a");
                let (_, receipt) = state
                    .queue_tern_prompt(key.clone(), "window-a", 1, "0", 1, "pane-a", "one ask")
                    .unwrap();
                poll.instance_id = Some("daemon-a".into());
                poll.sequence = 2;
                if offered {
                    assert!(state.tern_exchange(poll.clone()).command.is_some());
                }
                if shutdown {
                    state.request_shutdown();
                } else {
                    let mut preferences = state.observation_preferences().clone();
                    preferences.local = false;
                    state.apply_observation_preferences(preferences.clone());
                    assert!(receipt.try_recv().is_err());
                    poll.sequence += 1;
                    if offered {
                        let command = state.tern.get("window-a").unwrap();
                        assert!(!command.offline);
                        // Missing ACK remains uncertain, but scope changes never revoke it.
                        let reply = state.tern_exchange(poll);
                        assert!(reply.ok && reply.command.is_none());
                        assert_eq!(receipt.try_recv(), Ok(Err(PromptError::UnknownDelivery)));
                    } else {
                        let reply = state.tern_exchange(poll);
                        assert!(reply.ok && reply.command.is_some());
                        assert!(receipt.try_recv().is_err());
                    }
                    assert_eq!(state.prompt_available(&key), Ok(()));
                    continue;
                }
                assert_eq!(
                    receipt.try_recv(),
                    Ok(Err(if offered {
                        PromptError::UnknownDelivery
                    } else {
                        PromptError::Offline
                    }))
                );
                assert!(receipt.try_recv().is_err());
            }
        }
    }

    #[test]
    fn host_and_scope_apply_once_preserving_choices_and_actual_connection_evidence() {
        let mut state = tern_test_state(ObservationMode::Both);
        state.set_observation_catalog(catalog(&[("east", "East", true)]));
        let mut preferences = ObservationPreferences {
            remote: true,
            machines: vec!["east".into()],
            ..ObservationPreferences::default()
        };
        state.apply_observation_preferences(preferences.clone());
        let remote = remote_source("east");
        connect(&mut state, &remote, 1);
        assert!(state.publish_source_snapshot(
            &remote,
            1,
            std::iter::empty::<&AgentRecord>(),
            SourceCounts::default(),
            Arc::from(Vec::<String>::new()),
            &[],
            Instant::now(),
        ));
        assert_eq!(
            state.host_connections(),
            HostConnections { herdr: 1, tern: 0 }
        );
        assert!(state.tern_exchange(tern_test_request("window-a")).ok);
        let epoch = state.observation_epoch();
        preferences.host = ObservationMode::Tern;
        preferences.local = false;
        state.apply_observation_preferences(preferences.clone());
        assert_eq!(state.observation_epoch(), epoch + 1);
        assert_eq!(state.observation_mode(), ObservationMode::Tern);
        assert_eq!(state.observation_preferences(), &preferences);
        assert_eq!(state.scene().sessions, 1);
        assert_eq!(
            state.host_connections(),
            HostConnections { herdr: 0, tern: 1 }
        );
        assert_eq!(state.source_generation(&remote), None);
        state.set_observation_mode(ObservationMode::Both);
        assert!(!state.observation_preferences().local);
        assert!(state.observation_preferences().remote);
        assert_eq!(
            state.observation_preferences().machines,
            vec!["east".to_owned()]
        );
        assert!(!state.begin_source(remote.clone(), 1));
        connect(&mut state, &remote, 2);
        preferences.host = ObservationMode::Both;
        preferences.local = true;
        state.apply_observation_preferences(preferences);
        assert_eq!(state.scene().sessions, 1);
        assert_eq!(
            state.host_connections(),
            HostConnections { herdr: 0, tern: 1 }
        );
    }

    #[test]
    fn observation_transition_updates_canonical_restart_context_only_after_mutation_admission() {
        let mut state = tern_test_state(ObservationMode::Both);
        for mode in [
            ObservationMode::Tern,
            ObservationMode::Herdr,
            ObservationMode::Both,
        ] {
            state.set_observation_mode(mode);
            let context = state.update_context().unwrap();
            assert_eq!(context.observation_mode().unwrap(), mode);
            assert_eq!(
                context
                    .environment
                    .get(OBSERVATION_MODE_ENV)
                    .map(String::as_str),
                (mode != ObservationMode::Herdr).then_some(mode.as_str())
            );
        }
        state.set_ui_ready();
        let context = state.update_context().unwrap().clone();
        let preferences = state.observation_preferences().clone();
        let epoch = state.observation_epoch();
        let (sender, receiver) = mpsc::channel();
        state
            .queue_update_prepare("operation".into(), "secret".into(), sender)
            .unwrap();
        state.set_observation_mode(ObservationMode::Tern);
        let mut changed = preferences.clone();
        changed.host = ObservationMode::Herdr;
        changed.local = false;
        state.apply_observation_preferences(changed.clone());
        let mut changed_context = context.clone();
        changed_context.instance_id = "different".into();
        state.set_update_context(changed_context);
        assert_eq!(state.observation_mode(), ObservationMode::Both);
        assert_eq!(state.observation_preferences(), &preferences);
        assert_eq!(state.observation_epoch(), epoch);
        assert_eq!(state.update_context(), Some(&context));
        state.take_update_prepare().unwrap();
        assert!(state.finish_update_prepare("operation", "secret", Ok(())));
        receiver.recv().unwrap().unwrap();
        state.apply_observation_preferences(changed.clone());
        assert_eq!(state.update_context(), Some(&context));
        state.cancel_update_prepare("operation", "secret");
        state.apply_observation_preferences(changed);
        assert_eq!(state.observation_mode(), ObservationMode::Herdr);
        assert_eq!(state.observation_epoch(), epoch + 1);
        assert_eq!(
            state.update_context().unwrap().observation_mode().unwrap(),
            ObservationMode::Herdr
        );
    }

    #[test]
    fn tern_inventory_receipts_epochs_and_expiry_are_scoped() {
        use crate::herdr::{PromptError, PromptReceipt};
        use crate::tern::{TernResult, TernResultStatus};
        let mut state = tern_test_state(ObservationMode::Tern);
        let mut request = tern_test_request("window-a");
        let first = state.tern_exchange(request.clone());
        assert!(first.ok);
        assert_eq!(first.hosts[0].generation, 1);
        assert!(first.command.is_none());
        let snapshot = state.session_snapshot(&SessionListOptions::default(), None);
        assert_eq!(snapshot.rows.len(), 1);
        assert_eq!(snapshot.rows[0].source_label, "Tern");
        let key = snapshot.rows[0].key.clone();
        assert_eq!(state.prompt_available(&key), Ok(()));

        request.instance_id = Some("daemon-a".into());
        request.sequence = 2;
        let duplicate = request.hosts[0].agents[0].clone();
        request.hosts[0].agents.push(duplicate);
        let revision = state.session_revision();
        assert!(!state.tern_exchange(request.clone()).ok);
        assert_eq!(state.session_revision(), revision);
        request.hosts[0].agents.pop();
        let (operation_id, receipt) = state
            .queue_tern_prompt(key.clone(), "window-a", 1, "0", 1, "pane-a", "question")
            .unwrap();
        assert!(matches!(
            state.queue_tern_prompt(key.clone(), "window-a", 1, "0", 1, "pane-a", "another"),
            Err(PromptError::Busy)
        ));
        let offered = state.tern_exchange(request.clone());
        assert!(offered.ok);
        let command = offered.command.expect("one command offered");
        assert_eq!(command.operation_id, operation_id);
        assert_eq!(command.pane, "pane-a");
        assert_eq!(command.text, "question");
        request.sequence = 3;
        request.results.push(TernResult {
            operation_id,
            pane: "pane-a".into(),
            status: TernResultStatus::Accepted,
            error: None,
        });
        assert!(state.tern_exchange(request.clone()).ok);
        assert_eq!(receipt.try_recv(), Ok(Ok(PromptReceipt::Accepted)));
        request.sequence = 4;
        let revision = state.session_revision();
        assert!(!state.tern_exchange(request.clone()).ok);
        assert_eq!(state.session_revision(), revision);
        request.results.clear();
        request.epoch = 2;
        request.sequence = 1;
        let renewed = state.tern_exchange(request.clone());
        assert!(renewed.ok);
        assert_eq!(renewed.hosts[0].generation, 2);
        assert_eq!(state.prompt_available(&key), Err(PromptError::StaleTarget));
        let current_key = state
            .session_snapshot(&SessionListOptions::default(), None)
            .rows[0]
            .key
            .clone();
        let (lost_operation, lost_receipt) = state
            .queue_tern_prompt(current_key, "window-a", 2, "0", 1, "pane-a", "second")
            .unwrap();
        request.sequence = 2;
        assert!(state.tern_exchange(request.clone()).command.is_some());
        request.sequence = 3;
        assert!(state.tern_exchange(request.clone()).ok);
        assert_eq!(
            lost_receipt.try_recv(),
            Ok(Err(PromptError::UnknownDelivery))
        );
        request.sequence = 4;
        request.results.push(TernResult {
            operation_id: lost_operation,
            pane: "pane-a".into(),
            status: TernResultStatus::Accepted,
            error: None,
        });
        assert!(!state.tern_exchange(request.clone()).ok);
        request.results.clear();
        request.epoch = 1;
        request.sequence = 5;
        assert!(!state.tern_exchange(request).ok);
        state.tern.get_mut("window-a").unwrap().last_seen =
            Instant::now() - crate::tern::FRESHNESS - Duration::from_millis(1);
        state.expire_tern(Instant::now());
        let snapshot = state.session_snapshot(&SessionListOptions::default(), None);
        assert_eq!(
            snapshot.rows[0].availability,
            crate::session_view::Availability::Offline
        );
        assert_eq!(snapshot.rows[0].outcome, None);
    }

    #[test]
    fn tern_lost_binding_and_retired_sources_never_mutate_or_replay() {
        use crate::herdr::{PromptError, PromptReceipt};
        use crate::tern::{TernResult, TernResultStatus};
        let mut state = tern_test_state(ObservationMode::Tern);
        let mut request = tern_test_request("lost-registration");
        let first = state.tern_exchange(request.clone());
        assert!(first.ok && first.command.is_none());
        assert!(first.error_code.is_none());
        let key = tern_test_key(&state, &request.window);
        let (operation_id, receipt) = state
            .queue_tern_prompt(key.clone(), &request.window, 1, "0", 1, "pane-a", "one ask")
            .unwrap();
        let revision = state.session_revision();
        let last_seen = state.tern.get(&request.window).unwrap().last_seen;
        request.sequence = 2;
        let lost = state.tern_exchange(request.clone());
        assert_eq!(
            lost.error_code,
            Some(TernExchangeErrorCode::BindingRequired)
        );
        assert!(lost.hosts.is_empty());
        assert!(lost.command.is_none());
        assert_eq!(state.session_revision(), revision);
        assert_eq!(
            state.tern.get(&request.window).unwrap().last_seen,
            last_seen
        );
        assert!(receipt.try_recv().is_err());

        request.instance_id = Some("different-daemon".into());
        assert_eq!(
            state.tern_exchange(request.clone()).error_code,
            Some(TernExchangeErrorCode::InstanceChanged)
        );
        request.instance_id = Some("daemon-a".into());
        let command = state.tern_exchange(request.clone()).command.unwrap();
        assert_eq!(command.operation_id, operation_id);
        assert_eq!(command.text, "one ask");
        request.sequence = 3;
        request.results.push(TernResult {
            operation_id: "wrong-operation".into(),
            pane: "pane-a".into(),
            status: TernResultStatus::Accepted,
            error: None,
        });
        let revision = state.session_revision();
        let last_seen = state.tern.get(&request.window).unwrap().last_seen;
        assert_eq!(
            state.tern_exchange(request.clone()).error_code,
            Some(TernExchangeErrorCode::ReceiptRejected)
        );
        assert_eq!(state.session_revision(), revision);
        assert_eq!(
            state.tern.get(&request.window).unwrap().last_seen,
            last_seen
        );
        assert_eq!(state.tern.get(&request.window).unwrap().sequence, 2);
        assert!(receipt.try_recv().is_err());
        request.results[0].operation_id = operation_id;
        request.results[0].pane = "wrong-pane".into();
        assert_eq!(
            state.tern_exchange(request.clone()).error_code,
            Some(TernExchangeErrorCode::ReceiptRejected)
        );
        request.results[0].pane = "pane-a".into();
        request.epoch = 2;
        assert_eq!(
            state.tern_exchange(request.clone()).error_code,
            Some(TernExchangeErrorCode::Rejected)
        );
        assert_eq!(state.session_revision(), revision);
        let binding = state.tern.get(&request.window).unwrap();
        assert_eq!(binding.epoch, 1);
        assert_eq!(binding.sequence, 2);
        assert_eq!(binding.last_seen, last_seen);
        assert_eq!(tern_test_key(&state, &request.window), key);
        assert!(receipt.try_recv().is_err());
        request.epoch = 1;
        request.sequence = 2;
        assert_eq!(
            state.tern_exchange(request.clone()).error_code,
            Some(TernExchangeErrorCode::Rejected)
        );
        request.sequence = 3;
        assert!(state.tern_exchange(request.clone()).ok);
        assert_eq!(receipt.try_recv(), Ok(Ok(PromptReceipt::Accepted)));
        assert!(!state.remove_source(&crate::tern::source_id(&request.window, "0"), 2));
        assert!(state.tern.get(&request.window).is_some());
        assert!(state.remove_source(&crate::tern::source_id(&request.window, "0"), 1));
        assert!(state.tern.get(&request.window).is_some());
        state.set_observation_mode(ObservationMode::Herdr);
        state.set_observation_mode(ObservationMode::Tern);
        assert!(state.tern.get(&request.window).is_none());
        request.sequence = 4;
        let revision = state.session_revision();
        for results in [request.results.clone(), vec![]] {
            request.results = results;
            assert_eq!(
                state.tern_exchange(request.clone()).error_code,
                Some(TernExchangeErrorCode::SourceRetired)
            );
            assert_eq!(state.session_revision(), revision);
            assert!(state.tern.get(&request.window).is_none());
            assert_eq!(state.prompt_available(&key), Err(PromptError::StaleTarget));
        }
        let fresh = tern_test_request("fresh-registration");
        assert!(state.tern_exchange(fresh.clone()).ok);
        assert_ne!(
            tern_test_key(&state, &fresh.window).source_id,
            key.source_id
        );
    }

    #[test]
    fn tern_shared_capacity_reclaims_expired_only_and_keeps_live_pending_and_herdr() {
        use crate::herdr::PromptError;
        let mut state = tern_test_state(ObservationMode::Both);
        connect(&mut state, "/isolated/live-herdr.sock", 1);
        publish(
            &mut state,
            "/isolated/live-herdr.sock",
            1,
            AgentStatus::Working,
        );
        let herdr_key = state
            .session_snapshot(&SessionListOptions::default(), None)
            .rows[0]
            .key
            .clone();
        for index in 0..63 {
            assert!(
                state
                    .tern_exchange(tern_test_request(&format!("window-{index}")))
                    .ok
            );
        }
        let retired_key = tern_test_key(&state, "window-0");
        let live_key = tern_test_key(&state, "window-1");
        let (_, pending) = state
            .queue_tern_prompt(
                live_key.clone(),
                "window-1",
                1,
                "0",
                1,
                "pane-a",
                "keep pending",
            )
            .unwrap();
        let now = Instant::now();
        state.tern.get_mut("window-0").unwrap().last_seen = now - crate::tern::FRESHNESS;
        let mut malformed = tern_test_request("malformed");
        let duplicate = malformed.hosts[0].agents[0].clone();
        malformed.hosts[0].agents.push(duplicate);
        let revision = state.session_revision();
        assert!(!state.tern_exchange(malformed).ok);
        assert_eq!(state.session_revision(), revision);
        assert!(state.tern.get("window-0").is_some());
        assert!(!state.begin_source("/isolated/invalid.sock".into(), 0));
        assert!(state.tern.get("window-0").is_some());
        let mut stale = tern_test_request("window-1");
        stale.instance_id = Some("daemon-a".into());
        assert!(!state.tern_exchange(stale).ok);
        assert!(state.tern.get("window-0").is_some());
        assert!(state.tern_exchange(tern_test_request("replacement")).ok);
        assert!(state.tern.get("window-0").unwrap().hosts.is_empty());
        assert_eq!(state.tern.len(), 64);
        assert_eq!(
            state.prompt_available(&retired_key),
            Err(PromptError::StaleTarget)
        );
        assert_eq!(state.prompt_available(&live_key), Ok(()));
        assert_eq!(state.prompt_available(&herdr_key), Ok(()));
        assert!(pending.try_recv().is_err());
        assert_ne!(
            tern_test_key(&state, "replacement").source_id,
            retired_key.source_id
        );
        let denied = state.tern_exchange(tern_test_request("all-live-full"));
        assert!(denied.ok);
        assert_eq!(
            denied.hosts[0].error_code,
            Some(TernExchangeErrorCode::Rejected)
        );
        assert!(state.tern.get("window-1").is_some());
        let mut retired = tern_test_request("window-0");
        retired.instance_id = Some("daemon-a".into());
        assert_eq!(
            state.tern_exchange(retired).error_code,
            Some(TernExchangeErrorCode::SourceRetired)
        );
        state.cancel_tern_prompt("window-1", "not-the-operation");
        assert!(pending.try_recv().is_err());
    }

    #[test]
    fn reclamation_requires_matching_registry_store_and_aggregate_generations() {
        for newer_store in [false, true] {
            let mut state = tern_test_state(ObservationMode::Both);
            for index in 0..crate::tern::MAX_WINDOWS {
                assert!(
                    state
                        .tern_exchange(tern_test_request(&format!("window-{index}")))
                        .ok
                );
            }
            let source = crate::tern::source_id("window-0", "0");
            state.tern.get_mut("window-0").unwrap().last_seen =
                Instant::now() - crate::tern::FRESHNESS;
            if newer_store {
                assert!(state.update_source(&source, 1, false, SourceCounts::default()));
                assert!(state.session_store.begin_source(&source, 2));
            } else {
                state.sources.get_mut(&source).unwrap().generation = 2;
            }
            assert!(!state.begin_source("/isolated/new-herdr.sock".into(), 1));
            assert_eq!(state.tern.len(), crate::tern::MAX_WINDOWS);
            assert!(state.tern.get("window-0").is_some());
            assert!(state.sources.contains_key(&source));
            assert!(!state.remove_source(&source, 1));
            assert!(state.tern.get("window-0").is_some());
        }
    }

    #[test]
    fn exhausted_tern_generation_rejects_only_its_host_without_refreshing_snapshot() {
        let mut state = tern_test_state(ObservationMode::Tern);
        let mut request = tern_test_request("window-a");
        assert!(state.tern_exchange(request.clone()).ok);
        let source = crate::tern::source_id("window-a", "0");
        state.sources.get_mut(&source).unwrap().generation = crate::tern::MAX_SAFE_INTEGER;
        state
            .tern
            .get_mut("window-a")
            .unwrap()
            .hosts
            .get_mut("0")
            .unwrap()
            .generation = crate::tern::MAX_SAFE_INTEGER;
        request.instance_id = Some("daemon-a".into());
        request.sequence = 2;
        request.hosts[0].observation_generation = 2;
        let snapshot = state.session_snapshot(&SessionListOptions::default(), None);
        let last_seen = state.tern.get("window-a").unwrap().hosts["0"].last_seen;
        let reply = state.tern_exchange(request);
        assert!(reply.ok);
        assert_eq!(
            reply.hosts[0].error_code,
            Some(TernExchangeErrorCode::Rejected)
        );
        assert_eq!(
            state.session_snapshot(&SessionListOptions::default(), None),
            snapshot
        );
        assert_eq!(
            state.tern.get("window-a").unwrap().hosts["0"].last_seen,
            last_seen
        );
    }

    #[test]
    fn herdr_admission_finalizes_expired_tern_pending_before_reclamation() {
        use crate::herdr::PromptError;
        let mut state = tern_test_state(ObservationMode::Both);
        for index in 0..crate::tern::MAX_WINDOWS {
            assert!(
                state
                    .tern_exchange(tern_test_request(&format!("window-{index}")))
                    .ok
            );
        }
        let key = tern_test_key(&state, "window-0");
        let (_, unoffered) = state
            .queue_tern_prompt(key, "window-0", 1, "0", 1, "pane-a", "not offered")
            .unwrap();
        let key = tern_test_key(&state, "window-1");
        let (_, offered) = state
            .queue_tern_prompt(key, "window-1", 1, "0", 1, "pane-a", "offered once")
            .unwrap();
        let mut poll = tern_test_request("window-1");
        poll.instance_id = Some("daemon-a".into());
        poll.sequence = 2;
        assert!(state.tern_exchange(poll).command.is_some());
        let now = Instant::now();
        state.tern.get_mut("window-0").unwrap().last_seen =
            now - crate::tern::FRESHNESS - Duration::from_millis(1);
        state.tern.get_mut("window-1").unwrap().last_seen = now - crate::tern::FRESHNESS;
        assert!(state.begin_source("/isolated/new-herdr.sock".into(), 1));
        assert_eq!(unoffered.try_recv(), Ok(Err(PromptError::Offline)));
        assert_eq!(offered.try_recv(), Ok(Err(PromptError::UnknownDelivery)));
        assert!(state.tern.get("window-0").unwrap().hosts.is_empty());
        assert!(state.tern.get("window-1").unwrap().offline);
        assert_eq!(state.tern.len(), crate::tern::MAX_WINDOWS);
        assert!(state.tern.get("window-2").is_some());
        assert!(!state.begin_source("/isolated/new-herdr.sock".into(), 1));
        assert!(state.tern.get("window-1").is_some());
    }

    #[test]
    fn tern_freshness_revokes_authorization_before_presentation_tick() {
        use crate::herdr::PromptError;
        let mut state = tern_test_state(ObservationMode::Tern);
        assert!(state.tern_exchange(tern_test_request("window-a")).ok);
        let key = tern_test_key(&state, "window-a");
        let now = Instant::now();
        state.tern.get_mut("window-a").unwrap().last_seen = now - crate::tern::FRESHNESS;
        assert_eq!(state.prompt_available(&key), Err(PromptError::StaleTarget));
        assert!(matches!(
            state.queue_tern_prompt(key, "window-a", 1, "0", 1, "pane-a", "stale"),
            Err(PromptError::StaleTarget)
        ));
        assert!(state.expire_tern(now));
        assert_eq!(
            state
                .session_snapshot(&SessionListOptions::default(), None)
                .rows[0]
                .availability,
            crate::session_view::Availability::Offline
        );
    }

    #[test]
    fn tern_large_ascii_and_escaped_utf8_use_the_actual_response_budget() {
        use crate::herdr::PromptError;
        let mut state = tern_test_state(ObservationMode::Tern);
        let mut poll = tern_test_request("window-a");
        assert!(state.tern_exchange(poll.clone()).ok);
        let key = tern_test_key(&state, "window-a");
        let oversized = "\"\\\n한🪶".repeat(50_000);
        assert!(matches!(
            state.queue_tern_prompt(key.clone(), "window-a", 1, "0", 1, "pane-a", &oversized,),
            Err(PromptError::TooLarge)
        ));
        let ascii = "a".repeat(300 * 1024);
        let (operation_id, receipt) = state
            .queue_tern_prompt(key.clone(), "window-a", 1, "0", 1, "pane-a", &ascii)
            .unwrap();
        poll.instance_id = Some("daemon-a".into());
        poll.sequence = 2;
        let reply = state.tern_exchange(poll.clone());
        let encoded = crate::tern::serialize_response(&reply).unwrap();
        assert!(encoded.len() <= crate::tern::MAX_RESPONSE_BYTES);
        assert_eq!(reply.command.as_ref().unwrap().text, ascii);
        assert_eq!(reply.command.as_ref().unwrap().operation_id, operation_id);
        poll.sequence = 3;
        assert!(state.tern_exchange(poll.clone()).command.is_none());
        assert_eq!(receipt.try_recv(), Ok(Err(PromptError::UnknownDelivery)));
        let utf8 = "\"\\\n한🪶".repeat(10_000);
        state
            .queue_tern_prompt(key, "window-a", 1, "0", 1, "pane-a", &utf8)
            .unwrap();
        poll.sequence = 4;
        let reply = state.tern_exchange(poll);
        let encoded = crate::tern::serialize_response(&reply).unwrap();
        assert_eq!(reply.command.unwrap().text, utf8);
        assert!(encoded.len() <= crate::tern::MAX_RESPONSE_BYTES);
    }

    fn tern_multi_request(window: &str) -> TernExchangeRequest {
        let mut request = tern_test_request(window);
        request.hosts[0].label = "host-0".into();
        for id in ["1", "2"] {
            let mut host = request.hosts[0].clone();
            host.host = id.into();
            host.label = format!("host-{id}");
            host.is_local = false;
            request.hosts.push(host);
        }
        request
    }

    fn tern_host_key(state: &AppState, id: &str) -> SessionKey {
        state
            .session_snapshot(&SessionListOptions::default(), None)
            .rows
            .into_iter()
            .find(|row| row.source_label == format!("host-{id}"))
            .unwrap()
            .key
    }

    #[test]
    fn tern_remote_loss_matrix_preserves_other_keys_and_local_receipts() {
        use crate::herdr::{PromptError, PromptReceipt};
        use crate::session_view::Availability;
        use crate::tern::{TernHostStatus, TernResult, TernResultStatus};
        for loss in [
            Some(TernHostStatus::Disconnected),
            Some(TernHostStatus::Unavailable),
            None,
        ] {
            let mut state = tern_test_state(ObservationMode::Both);
            let mut preferences = state.observation_preferences().clone();
            preferences.local = false;
            preferences.remote = false;
            preferences.machines.clear();
            state.apply_observation_preferences(preferences);
            let mut request = tern_multi_request("window-a");
            assert!(state.tern_exchange(request.clone()).ok);
            assert_eq!(state.host_connections().tern, 3);
            assert_eq!(state.scene().sessions, 3);
            let local = tern_host_key(&state, "0");
            let remote = tern_host_key(&state, "1");
            let other = tern_host_key(&state, "2");
            assert_ne!(local, remote);
            assert_ne!(remote, other);
            assert!(state.session_view_for_key(&local).unwrap().is_local);
            assert!(!state.session_view_for_key(&remote).unwrap().is_local);
            assert_eq!(state.prompt_available(&remote), Err(PromptError::ReadOnly));
            assert!(matches!(
                state.queue_tern_prompt(
                    remote.clone(),
                    "window-a",
                    1,
                    "1",
                    1,
                    "pane-a",
                    "forbidden"
                ),
                Err(PromptError::ReadOnly)
            ));
            let (operation, receipt) = state
                .queue_tern_prompt(
                    local.clone(),
                    "window-a",
                    1,
                    "0",
                    1,
                    "pane-a",
                    "one local ask",
                )
                .unwrap();
            let saved = request.hosts[1].clone();
            match loss {
                Some(status) => {
                    request.hosts[1].status = status;
                    request.hosts[1].agents.clear();
                }
                None => {
                    request.hosts.remove(1);
                }
            }
            request.instance_id = Some("daemon-a".into());
            request.sequence = 2;
            let reply = state.tern_exchange(request.clone());
            assert!(reply.ok);
            assert_eq!(reply.command.unwrap().operation_id, operation);
            assert!(receipt.try_recv().is_err());
            assert_eq!(
                state.session_view_for_key(&remote).unwrap().availability,
                Availability::Offline
            );
            for key in [&local, &other] {
                assert_eq!(
                    state.session_view_for_key(key).unwrap().availability,
                    Availability::Live
                );
            }
            if loss.is_none() {
                request.hosts.insert(1, saved.clone());
            } else {
                request.hosts[1] = saved.clone();
            }
            request.sequence = 3;
            request.results.push(TernResult {
                operation_id: operation,
                pane: "pane-a".into(),
                status: TernResultStatus::Accepted,
                error: None,
            });
            let reply = state.tern_exchange(request.clone());
            assert!(reply.ok && reply.command.is_none());
            assert_eq!(
                reply.hosts[1].error_code,
                Some(TernExchangeErrorCode::Rejected)
            );
            assert_eq!(receipt.try_recv(), Ok(Ok(PromptReceipt::Accepted)));
            assert_eq!(
                state.session_view_for_key(&remote).unwrap().availability,
                Availability::Offline
            );
            assert_eq!(tern_host_key(&state, "0"), local);
            assert_eq!(tern_host_key(&state, "2"), other);
            request.results.clear();
            request.sequence = 4;
            request.hosts[1].observation_generation = 2;
            let reply = state.tern_exchange(request.clone());
            assert!(reply.ok && reply.hosts[1].error.is_none());
            let recovered = tern_host_key(&state, "1");
            assert!(recovered.generation > remote.generation);
            assert_eq!(
                state.prompt_available(&remote),
                Err(PromptError::StaleTarget)
            );
            assert!(state.take_completions().is_empty());
            request.sequence = 5;
            request.hosts[1].agents.clear();
            assert!(state.tern_exchange(request).ok);
            assert_eq!(state.host_connections().tern, 3);
            assert_eq!(state.scene().sessions, 2);
            assert!(state.session_view_for_key(&recovered).is_none());
        }
    }

    #[test]
    fn tern_native_only_child_expiry_allows_same_observation_but_not_old_key() {
        use crate::session_view::Availability;
        let mut state = tern_test_state(ObservationMode::Tern);
        let mut request = tern_multi_request("window-a");
        assert!(state.tern_exchange(request.clone()).ok);
        let local = tern_host_key(&state, "0");
        let remote = tern_host_key(&state, "1");
        let other = tern_host_key(&state, "2");
        let (_, receipt) = state
            .queue_tern_prompt(local.clone(), "window-a", 1, "0", 1, "pane-a", "pending")
            .unwrap();
        let now = Instant::now();
        state
            .tern
            .get_mut("window-a")
            .unwrap()
            .hosts
            .get_mut("1")
            .unwrap()
            .last_seen = now - crate::tern::FRESHNESS;
        assert!(state.expire_tern(now));
        assert!(receipt.try_recv().is_err());
        assert_eq!(
            state.session_view_for_key(&remote).unwrap().availability,
            Availability::Offline
        );
        assert_eq!(
            state.session_view_for_key(&local).unwrap().availability,
            Availability::Live
        );
        assert_eq!(
            state.session_view_for_key(&other).unwrap().availability,
            Availability::Live
        );
        request.instance_id = Some("daemon-a".into());
        request.sequence = 2;
        let reply = state.tern_exchange(request.clone());
        assert!(reply.command.is_some());
        assert!(reply.hosts[1].generation > remote.generation);
        assert_eq!(request.hosts[1].observation_generation, 1);
        assert_eq!(tern_host_key(&state, "0"), local);
        assert_eq!(tern_host_key(&state, "2"), other);
        assert!(state.session_view_for_key(&remote).is_none());
        assert!(state.take_completions().is_empty());
    }

    #[test]
    fn tern_locality_is_sealed_per_observation_and_rename_keeps_identity() {
        use crate::herdr::PromptError;
        let mut state = tern_test_state(ObservationMode::Tern);
        let mut request = tern_multi_request("window-a");
        assert!(state.tern_exchange(request.clone()).ok);
        let local = tern_host_key(&state, "0");
        let remote = tern_host_key(&state, "1");
        request.instance_id = Some("daemon-a".into());
        request.sequence = 2;
        request.hosts[0].is_local = false;
        request.hosts[1].is_local = true;
        let reply = state.tern_exchange(request.clone());
        assert!(reply.ok);
        assert!(reply.hosts[0].error.is_some() && reply.hosts[1].error.is_some());
        assert!(state.session_view_for_key(&local).unwrap().is_local);
        assert!(!state.session_view_for_key(&remote).unwrap().is_local);
        assert_eq!(state.prompt_available(&remote), Err(PromptError::ReadOnly));
        request.sequence = 3;
        request.hosts[0].observation_generation = 2;
        request.hosts[1].observation_generation = 2;
        let reply = state.tern_exchange(request.clone());
        assert!(reply.hosts.iter().all(|host| host.error.is_none()));
        let next_local = tern_host_key(&state, "1");
        let next_remote = tern_host_key(&state, "0");
        assert_eq!(state.prompt_available(&next_local), Ok(()));
        assert_eq!(
            state.prompt_available(&next_remote),
            Err(PromptError::ReadOnly)
        );
        assert!(state.session_view_for_key(&local).is_none());
        assert!(state.session_view_for_key(&remote).is_none());
        request.sequence = 4;
        request.hosts[1].label = "Renamed".into();
        assert!(state.tern_exchange(request).ok);
        let renamed = state.session_view_for_key(&next_local).unwrap();
        assert_eq!(renamed.key, next_local);
        assert_eq!(renamed.source_label, "Renamed");
    }

    #[test]
    fn tern_parent_capacity_and_disable_retire_empty_bindings_independently() {
        use crate::tern::TernHostStatus;
        let mut state = tern_test_state(ObservationMode::Tern);
        for index in 0..crate::tern::MAX_WINDOWS {
            let mut request = tern_test_request(&format!("empty-{index}"));
            request.hosts[0].status = TernHostStatus::Unavailable;
            request.hosts[0].agents.clear();
            let reply = state.tern_exchange(request);
            assert!(reply.ok && reply.hosts[0].generation == 0);
        }
        assert_eq!(state.host_connections().tern, 0);
        assert!(!state.tern_exchange(tern_test_request("overflow")).ok);
        assert_eq!(state.scene().sessions, 0);
        state.tern.get_mut("empty-0").unwrap().last_seen = Instant::now() - crate::tern::FRESHNESS;
        assert!(state.tern_exchange(tern_test_request("replacement")).ok);
        assert!(state.tern.get("empty-0").is_none());
        let mut stale = tern_test_request("empty-0");
        assert_eq!(
            state.tern_exchange(stale.clone()).error_code,
            Some(TernExchangeErrorCode::SourceRetired)
        );
        state.set_observation_mode(ObservationMode::Herdr);
        assert_eq!(state.tern.len(), 0);
        state.set_observation_mode(ObservationMode::Tern);
        stale.window = "empty-1".into();
        assert_eq!(
            state.tern_exchange(stale.clone()).error_code,
            Some(TernExchangeErrorCode::SourceRetired)
        );
        stale.epoch = 2;
        assert!(state.tern_exchange(stale).ok);
    }

    #[test]
    fn tern_new_host_admission_is_local_then_numeric_and_existing_receipts_survive_full_capacity() {
        use crate::herdr::PromptReceipt;
        use crate::tern::{TernResult, TernResultStatus};
        let mut state = tern_test_state(ObservationMode::Both);
        for index in 0..62 {
            connect(&mut state, &format!("/isolated/herdr-{index}.sock"), 1);
        }
        let mut request = tern_multi_request("window-a");
        request.hosts[1].host = "10".into();
        request.hosts[1].label = "host-10".into();
        request.hosts.swap(0, 2);
        let reply = state.tern_exchange(request.clone());
        assert!(reply.ok);
        assert_eq!(
            reply
                .hosts
                .iter()
                .map(|host| host.host.as_str())
                .collect::<Vec<_>>(),
            vec!["2", "10", "0"]
        );
        assert!(reply.hosts[0].error.is_none() && reply.hosts[2].error.is_none());
        assert_eq!(reply.hosts[1].generation, 0);
        assert_eq!(
            reply.hosts[1].error_code,
            Some(TernExchangeErrorCode::Rejected)
        );
        let local = tern_host_key(&state, "0");
        let other = tern_host_key(&state, "2");
        let (operation_id, receipt) = state
            .queue_tern_prompt(local.clone(), "window-a", 1, "0", 1, "pane-a", "ask once")
            .unwrap();
        request.instance_id = Some("daemon-a".into());
        request.sequence = 2;
        assert!(state.tern_exchange(request.clone()).command.is_some());
        request.sequence = 3;
        request.results.push(TernResult {
            operation_id,
            pane: "pane-a".into(),
            status: TernResultStatus::Accepted,
            error: None,
        });
        let reply = state.tern_exchange(request);
        assert!(reply.ok && reply.command.is_none() && reply.hosts[1].error.is_some());
        assert_eq!(receipt.try_recv(), Ok(Ok(PromptReceipt::Accepted)));
        assert_eq!(tern_host_key(&state, "0"), local);
        assert_eq!(tern_host_key(&state, "2"), other);
    }

    #[test]
    fn tern_structural_failures_do_not_expire_or_mutate_any_existing_host() {
        use crate::tern::TernHostStatus;
        let mut state = tern_test_state(ObservationMode::Tern);
        assert!(state.tern_exchange(tern_multi_request("window-a")).ok);
        let before = state.session_snapshot(&SessionListOptions::default(), None);
        let mut base = tern_multi_request("window-a");
        base.instance_id = Some("daemon-a".into());
        base.sequence = 2;
        let mut invalid = Vec::new();
        let mut request = base.clone();
        request.version = 1;
        invalid.push(request);
        let mut request = base.clone();
        request.hosts[1].host = "01".into();
        invalid.push(request);
        let mut request = base.clone();
        request.hosts[1].host = "0".into();
        invalid.push(request);
        let mut request = base.clone();
        request.hosts[1].is_local = true;
        invalid.push(request);
        let mut request = base.clone();
        request.hosts[0].is_local = false;
        invalid.push(request);
        let mut request = base.clone();
        request.hosts[1].status = TernHostStatus::Unavailable;
        invalid.push(request);
        let mut request = base.clone();
        request.hosts[1].observation_generation = 0;
        invalid.push(request);
        let mut request = base.clone();
        request.hosts[1].label = "x".repeat(4097);
        invalid.push(request);
        let mut request = base.clone();
        for index in 1..129 {
            let mut agent = request.hosts[1].agents[0].clone();
            agent.pane = index.to_string();
            request.hosts[1].agents.push(agent);
        }
        invalid.push(request);
        for request in invalid {
            let reply = state.tern_exchange(request);
            assert!(!reply.ok && reply.hosts.is_empty() && reply.command.is_none());
            assert_eq!(
                state.session_snapshot(&SessionListOptions::default(), None),
                before
            );
            assert_eq!(state.tern.get("window-a").unwrap().sequence, 1);
        }
    }

    #[test]
    fn tern_reclaimed_child_keeps_parent_binding_and_observation_high_water() {
        let mut state = tern_test_state(ObservationMode::Both);
        for index in 0..61 {
            connect(&mut state, &format!("/isolated/herdr-{index}.sock"), 1);
        }
        let mut request = tern_multi_request("window-a");
        assert!(state.tern_exchange(request.clone()).ok);
        let old = tern_host_key(&state, "1");
        let local = tern_host_key(&state, "0");
        let saved = request.hosts.remove(1);
        let mut newcomer = saved.clone();
        newcomer.host = "3".into();
        newcomer.label = "host-3".into();
        request.hosts.push(newcomer);
        state
            .tern
            .get_mut("window-a")
            .unwrap()
            .hosts
            .get_mut("1")
            .unwrap()
            .last_seen = Instant::now() - crate::tern::FRESHNESS;
        request.instance_id = Some("daemon-a".into());
        request.sequence = 2;
        let reply = state.tern_exchange(request.clone());
        assert!(reply.ok && reply.hosts.iter().all(|host| host.error.is_none()));
        assert!(state.tern.get("window-a").unwrap().hosts.get("1").is_none());
        assert_eq!(state.tern.get("window-a").unwrap().epoch, 1);
        assert!(state.tern.retired_epoch("window-a").is_none());
        assert!(state.session_view_for_key(&old).is_none());
        assert_eq!(tern_host_key(&state, "0"), local);
        request.hosts.push(saved);
        request.sequence = 3;
        let reply = state.tern_exchange(request.clone());
        assert_eq!(
            reply.hosts[3].error_code,
            Some(TernExchangeErrorCode::Rejected)
        );
        assert_eq!(reply.hosts[3].generation, 0);
        state
            .tern
            .get_mut("window-a")
            .unwrap()
            .hosts
            .get_mut("3")
            .unwrap()
            .last_seen = Instant::now() - crate::tern::FRESHNESS;
        request.hosts.remove(2);
        request.hosts[2].observation_generation = 2;
        request.sequence = 4;
        let reply = state.tern_exchange(request);
        assert!(reply.ok && reply.hosts[2].error.is_none());
        assert!(reply.hosts[2].generation > old.generation);
        assert_eq!(tern_host_key(&state, "0"), local);
        assert!(state.take_completions().is_empty());
    }

    #[test]
    fn tern_parent_epoch_replaces_all_children_and_finalizes_only_its_pending() {
        use crate::herdr::PromptError;
        for offered in [false, true] {
            let mut state = tern_test_state(ObservationMode::Tern);
            let mut request = tern_multi_request("window-a");
            assert!(state.tern_exchange(request.clone()).ok);
            let local = tern_host_key(&state, "0");
            let remote = tern_host_key(&state, "1");
            let (_, receipt) = state
                .queue_tern_prompt(local.clone(), "window-a", 1, "0", 1, "pane-a", "one ask")
                .unwrap();
            request.instance_id = Some("daemon-a".into());
            request.sequence = 2;
            if offered {
                assert!(state.tern_exchange(request.clone()).command.is_some());
            }
            request.epoch = 2;
            request.sequence = 1;
            let reply = state.tern_exchange(request);
            assert!(reply.ok && reply.command.is_none());
            assert!(reply.hosts.iter().all(|host| host.generation == 2));
            assert_eq!(
                receipt.try_recv(),
                Ok(Err(if offered {
                    PromptError::UnknownDelivery
                } else {
                    PromptError::StaleTarget
                }))
            );
            assert!(state.session_view_for_key(&local).is_none());
            assert!(state.session_view_for_key(&remote).is_none());
            assert!(state.take_completions().is_empty());
        }
    }

    #[test]
    fn tern_prompt_sender_uses_response_budget_before_starting_delivery() {
        use crate::herdr::{PromptError, PromptSender, RequestOrigin};
        use crate::tern::{TernResult, TernResultStatus};
        let mut state = tern_test_state(ObservationMode::Tern);
        let mut poll = tern_test_request("sender-window");
        assert!(state.tern_exchange(poll.clone()).ok);
        let key = tern_test_key(&state, "sender-window");
        let shared = Arc::new(std::sync::Mutex::new(state));
        let mut sender = PromptSender::new(Arc::clone(&shared));
        assert_eq!(
            sender.submit(key.clone(), "\"\\\n한🪶".repeat(50_000), RequestOrigin::Gui),
            Err(PromptError::TooLarge)
        );
        assert!(!sender.is_pending());
        let text = "a".repeat(500 * 1024);
        assert_eq!(sender.submit(key, text.clone(), RequestOrigin::Gui), Ok(()));
        poll.instance_id = Some("daemon-a".into());
        poll.sequence = 2;
        let mut state = match shared.lock() {
            Ok(state) => state,
            Err(_) => panic!("prompt worker poisoned shared state"),
        };
        let reply = state.tern_exchange(poll.clone());
        assert!(
            crate::tern::serialize_response(&reply).unwrap().len()
                <= crate::tern::MAX_RESPONSE_BYTES
        );
        let command = reply.command.unwrap();
        assert_eq!(command.text, text);
        poll.sequence = 3;
        poll.results.push(TernResult {
            operation_id: command.operation_id,
            pane: command.pane,
            status: TernResultStatus::Accepted,
            error: None,
        });
        let settled = state.tern_exchange(poll);
        assert!(settled.ok && settled.command.is_none());
    }

    #[test]
    fn tern_offline_deadlines_use_freshness_not_the_first_late_tick() {
        use crate::tern::{FRESHNESS, OFFLINE_GRACE};
        for late_first_tick in [false, true] {
            let mut state = tern_test_state(ObservationMode::Tern);
            let base = Instant::now();
            let request = tern_test_request("deadline");
            assert!(tern_test_exchange_at(&mut state, request, base).ok);
            let source = crate::tern::source_id("deadline", "0");
            let key = tern_test_key(&state, "deadline");
            assert!(!state.maintain_tern(base + FRESHNESS - Duration::from_nanos(1)));
            assert_eq!(state.sources[&source].offline_since, None);
            if !late_first_tick {
                assert!(state.expire_tern(base + FRESHNESS));
                assert_eq!(state.sources[&source].offline_since, Some(base + FRESHNESS));
                assert_eq!(
                    state.tern.get("deadline").unwrap().offline_since,
                    Some(base + FRESHNESS)
                );
                assert!(!state
                    .maintain_tern(base + FRESHNESS + OFFLINE_GRACE - Duration::from_nanos(1)));
                // Freshness-only expiry never performs TTL cleanup, even when overdue.
                assert!(!state.expire_tern(base + FRESHNESS + OFFLINE_GRACE));
                assert!(state.session_view_for_key(&key).is_some());
            }
            let deadline = base + FRESHNESS + OFFLINE_GRACE;
            assert!(state.maintain_tern(deadline));
            assert!(!state.sources.contains_key(&source));
            assert!(state.session_view_for_key(&key).is_none());
            assert_eq!(state.scene().sessions, 0);
            assert_eq!(state.scene().unknown, 0);
            assert_eq!(state.scene().disconnected_sources, 0);
            assert!(state.tern.get("deadline").is_none());
            assert_eq!(state.retired_source_generation(&source), Some(1));
            assert_eq!(state.tern.retired_epoch("deadline"), Some(1));
            assert!(!state.maintain_tern(deadline + OFFLINE_GRACE));
        }
    }

    #[test]
    fn tern_explicit_loss_repeats_do_not_extend_and_recovery_starts_a_new_episode() {
        use crate::tern::{TernHostStatus, OFFLINE_GRACE};
        for loss in [
            Some(TernHostStatus::Disconnected),
            Some(TernHostStatus::Unavailable),
            None,
        ] {
            let mut state = tern_test_state(ObservationMode::Tern);
            let base = Instant::now();
            let mut request = tern_multi_request("episodes");
            assert!(tern_test_exchange_at(&mut state, request.clone(), base).ok);
            let saved = request.hosts[1].clone();
            let remote = crate::tern::source_id("episodes", "1");
            let local = tern_host_key(&state, "0");
            let old_key = tern_host_key(&state, "1");
            request.instance_id = Some("daemon-a".into());
            request.sequence = 2;
            match loss {
                Some(status) => {
                    request.hosts[1].status = status;
                    request.hosts[1].agents.clear();
                }
                None => {
                    request.hosts.remove(1);
                }
            }
            let first_loss = base + Duration::from_secs(1);
            assert!(tern_test_exchange_at(&mut state, request.clone(), first_loss).ok);
            assert_eq!(state.sources[&remote].offline_since, Some(first_loss));
            request.sequence = 3;
            let repeated_loss = base + Duration::from_secs(25);
            set_tern_test_last_seen(&mut state, "episodes", None, repeated_loss);
            assert!(tern_test_exchange_at(&mut state, request.clone(), repeated_loss).ok);
            assert_eq!(state.sources[&remote].offline_since, Some(first_loss));

            // Earlier valid loss evidence wins; a stale native generation cannot alter it.
            let earlier_loss = base + Duration::from_millis(500);
            assert!(state.mark_source_offline_at(&remote, 1, earlier_loss));
            assert!(!state.mark_source_offline_at(&remote, 2, base));
            assert_eq!(state.sources[&remote].offline_since, Some(earlier_loss));
            let mut recovered = saved;
            recovered.observation_generation = 2;
            if loss.is_none() {
                request.hosts.insert(1, recovered);
            } else {
                request.hosts[1] = recovered;
            }
            request.sequence = 4;
            let recovery = base + Duration::from_secs(29);
            assert!(tern_test_exchange_at(&mut state, request.clone(), recovery).ok);
            assert_eq!(state.sources[&remote].offline_since, None);
            assert_eq!(state.sources[&remote].generation, 2);
            assert!(state.session_view_for_key(&old_key).is_none());
            assert_eq!(tern_host_key(&state, "0"), local);
            request.sequence = 5;
            request.hosts[1].status = TernHostStatus::Disconnected;
            request.hosts[1].agents.clear();
            let second_loss = base + Duration::from_secs(30);
            assert!(tern_test_exchange_at(&mut state, request, second_loss).ok);
            assert_eq!(state.sources[&remote].offline_since, Some(second_loss));
            assert!(!state.maintain_tern(first_loss + OFFLINE_GRACE));
            let second_deadline = second_loss + OFFLINE_GRACE;
            set_tern_test_last_seen(&mut state, "episodes", None, second_deadline);
            assert!(state.maintain_tern(second_deadline));
            assert!(!state.sources.contains_key(&remote));
            let parent = state.tern.get("episodes").unwrap();
            assert!(parent.bound && !parent.offline);
            assert!(parent.observations["1"].requires_new);
            assert!(!parent.hosts.contains_key("1"));
            assert_eq!(tern_host_key(&state, "0"), local);
        }
    }

    #[test]
    fn tern_disconnected_initialization_has_no_loss_clock_and_generation_resets_it() {
        use crate::tern::OFFLINE_GRACE;
        let mut state = tern_test_state(ObservationMode::Tern);
        let source = crate::tern::source_id("unregistered", "0");
        let base = Instant::now();
        assert!(state.begin_source(source.clone(), 1));
        assert!(state.update_source(&source, 1, false, SourceCounts::default()));
        assert_eq!(state.sources[&source].offline_since, None);
        assert!(!state.maintain_tern(base + OFFLINE_GRACE));
        assert!(state.mark_source_offline_at(&source, 1, base));
        assert_eq!(state.sources[&source].offline_since, Some(base));
        assert!(state.begin_source(source.clone(), 2));
        assert_eq!(state.sources[&source].offline_since, None);
        assert!(!state.mark_source_offline_at(&source, 1, base));
        assert!(!state.update_source(&source, 1, false, SourceCounts::default()));
        assert!(!state.publish_source_snapshot(
            &source,
            1,
            std::iter::empty::<&AgentRecord>(),
            SourceCounts::default(),
            Arc::from(Vec::<String>::new()),
            &[],
            base,
        ));
        assert_eq!(state.sources[&source].offline_since, None);
        assert!(!state.maintain_tern(base + OFFLINE_GRACE));
        assert!(state.mark_source_offline_at(&source, 2, base));
        assert!(state.maintain_tern(base + OFFLINE_GRACE));
        assert_eq!(state.source_generation(&source), None);
        assert_eq!(state.retired_source_generation(&source), Some(2));
        assert!(!state.begin_source(source, 2));
    }

    #[test]
    fn tern_epoch_omissions_keep_prior_deadlines_and_age_newly_lost_hosts() {
        use crate::tern::OFFLINE_GRACE;
        let mut state = tern_test_state(ObservationMode::Tern);
        let base = Instant::now();
        let mut request = tern_multi_request("epoch-omission");
        assert!(tern_test_exchange_at(&mut state, request.clone(), base).ok);
        let first = crate::tern::source_id("epoch-omission", "1");
        let second = crate::tern::source_id("epoch-omission", "2");
        request.hosts.remove(1);
        request.instance_id = Some("daemon-a".into());
        request.sequence = 2;
        let first_loss = base + Duration::from_secs(1);
        assert!(tern_test_exchange_at(&mut state, request.clone(), first_loss).ok);
        request.hosts.truncate(1);
        request.epoch = 2;
        request.sequence = 1;
        let epoch_loss = base + Duration::from_secs(2);
        assert!(tern_test_exchange_at(&mut state, request, epoch_loss).ok);
        assert_eq!(state.sources[&first].offline_since, Some(first_loss));
        assert_eq!(state.sources[&second].offline_since, Some(epoch_loss));
        assert!(state.tern.get("epoch-omission").unwrap().hosts["1"].offline);
        assert!(state.tern.get("epoch-omission").unwrap().hosts["2"].offline);
        let deadline = first_loss + OFFLINE_GRACE;
        set_tern_test_last_seen(&mut state, "epoch-omission", None, deadline);
        assert!(state.maintain_tern(deadline));
        assert!(!state.sources.contains_key(&first));
        assert!(state.sources.contains_key(&second));
        assert!(state.maintain_tern(epoch_loss + OFFLINE_GRACE));
        assert!(!state.sources.contains_key(&second));
        let parent = state.tern.get("epoch-omission").unwrap();
        assert_eq!(parent.epoch, 2);
        assert!(!parent.offline);
        assert_eq!(parent.hosts.len(), 1);
        assert_eq!(parent.hosts["0"].generation, 2);
        assert_eq!(state.tern.retired_epoch("epoch-omission"), None);
    }

    #[test]
    fn tern_maintenance_skips_mismatched_sources_but_removes_other_and_orphan_sources() {
        use crate::tern::{FRESHNESS, OFFLINE_GRACE};
        for mismatch in ["aggregate", "store", "registry", "live-registry"] {
            let mut state = tern_test_state(ObservationMode::Tern);
            let base = Instant::now();
            assert!(tern_test_exchange_at(&mut state, tern_multi_request("mismatch"), base,).ok);
            assert!(state.expire_tern(base + FRESHNESS));
            let matching = crate::tern::source_id("mismatch", "0");
            let blocked = crate::tern::source_id("mismatch", "1");
            let orphan = crate::tern::source_id("mismatch", "2");
            state.tern.get_mut("mismatch").unwrap().hosts.remove("2");
            let deadline = base + FRESHNESS + OFFLINE_GRACE;
            match mismatch {
                "aggregate" => state.sources.get_mut(&blocked).unwrap().generation = 2,
                "store" => assert!(state.session_store.begin_source(&blocked, 2)),
                "registry" => {
                    state
                        .tern
                        .get_mut("mismatch")
                        .unwrap()
                        .hosts
                        .get_mut("1")
                        .unwrap()
                        .generation = 2;
                }
                "live-registry" => {
                    let parent = state.tern.get_mut("mismatch").unwrap();
                    parent.offline = false;
                    parent.offline_since = None;
                    parent.last_seen = deadline;
                    let host = parent.hosts.get_mut("1").unwrap();
                    host.offline = false;
                    host.last_seen = deadline;
                }
                _ => unreachable!(),
            }
            assert!(state.maintain_tern(deadline));
            assert_eq!(state.source_generation(&matching), None);
            assert_eq!(state.source_generation(&orphan), None);
            assert!(state.source_generation(&blocked).is_some());
            assert_eq!(state.retired_source_generation(&matching), Some(1));
            assert_eq!(state.retired_source_generation(&orphan), Some(1));
            assert_eq!(state.retired_source_generation(&blocked), None);
            let parent = state.tern.get("mismatch").unwrap();
            assert_eq!(parent.hosts.len(), 1);
            assert!(parent.hosts.contains_key("1"));
            assert_eq!(state.tern.retired_epoch("mismatch"), None);
            assert!(!state.maintain_tern(deadline));
        }
    }

    #[test]
    fn tern_remote_ttl_cleanup_keeps_live_parent_local_pending_and_receipt() {
        use crate::herdr::PromptReceipt;
        use crate::tern::{TernResult, TernResultStatus, OFFLINE_GRACE};
        for offered in [false, true] {
            let mut state = tern_test_state(ObservationMode::Tern);
            let base = Instant::now();
            let mut request = tern_multi_request("local-pending");
            assert!(tern_test_exchange_at(&mut state, request.clone(), base).ok);
            let local = tern_host_key(&state, "0");
            let remote = tern_host_key(&state, "1");
            let other = tern_host_key(&state, "2");
            request.instance_id = Some("daemon-a".into());
            request.sequence = 2;
            request.hosts.remove(1);
            let loss = base + Duration::from_secs(1);
            assert!(tern_test_exchange_at(&mut state, request.clone(), loss).ok);
            let (operation_id, receipt) = state
                .queue_tern_prompt(
                    local.clone(),
                    "local-pending",
                    1,
                    "0",
                    1,
                    "pane-a",
                    "ask once",
                )
                .unwrap();
            if offered {
                let command = state.tern.offer("local-pending", "daemon-a").unwrap();
                assert_eq!(command.operation_id, operation_id);
            }
            let deadline = loss + OFFLINE_GRACE;
            set_tern_test_last_seen(&mut state, "local-pending", None, deadline);
            assert!(state.maintain_tern(deadline));
            assert!(state.session_view_for_key(&remote).is_none());
            assert_eq!(tern_host_key(&state, "0"), local);
            assert_eq!(tern_host_key(&state, "2"), other);
            assert_eq!(state.scene().sessions, 2);
            assert_eq!(state.scene().unknown, 0);
            assert!(receipt.try_recv().is_err());
            assert!(state.tern.get("local-pending").unwrap().observations["1"].requires_new);
            request.sequence = 3;
            if !offered {
                let poll = tern_test_exchange_at(&mut state, request.clone(), deadline);
                assert_eq!(poll.command.unwrap().operation_id, operation_id);
                request.sequence += 1;
            }
            request.results.push(TernResult {
                operation_id,
                pane: "pane-a".into(),
                status: TernResultStatus::Accepted,
                error: None,
            });
            let acknowledged = tern_test_exchange_at(&mut state, request.clone(), deadline);
            assert!(acknowledged.ok && acknowledged.command.is_none());
            assert_eq!(receipt.try_recv(), Ok(Ok(PromptReceipt::Accepted)));
            request.sequence += 1;
            request.results.clear();
            let next = tern_test_exchange_at(&mut state, request, deadline);
            assert!(next.ok && next.command.is_none());
            assert!(receipt.try_recv().is_err());
        }
    }

    #[test]
    fn tern_explicit_local_loss_finalizes_pending_without_replaying_after_cleanup() {
        use crate::herdr::PromptError;
        use crate::tern::{TernHostStatus, OFFLINE_GRACE};
        for offered in [false, true] {
            for loss in [TernHostStatus::Disconnected, TernHostStatus::Unavailable] {
                let mut state = tern_test_state(ObservationMode::Tern);
                let base = Instant::now();
                let mut request = tern_test_request("explicit-pending");
                assert!(tern_test_exchange_at(&mut state, request.clone(), base).ok);
                let key = tern_test_key(&state, "explicit-pending");
                let (_, receipt) = state
                    .queue_tern_prompt(key, "explicit-pending", 1, "0", 1, "pane-a", "one ask")
                    .unwrap();
                if offered {
                    assert!(state.tern.offer("explicit-pending", "daemon-a").is_some());
                }
                request.instance_id = Some("daemon-a".into());
                request.sequence = 2;
                request.hosts[0].status = loss;
                request.hosts[0].agents.clear();
                let lost = base + Duration::from_secs(1);
                let reply = tern_test_exchange_at(&mut state, request.clone(), lost);
                assert!(reply.ok && reply.command.is_none());
                assert_eq!(
                    receipt.try_recv(),
                    Ok(Err(if offered {
                        PromptError::UnknownDelivery
                    } else {
                        PromptError::Offline
                    }))
                );
                let deadline = lost + OFFLINE_GRACE;
                set_tern_test_last_seen(&mut state, "explicit-pending", None, deadline);
                assert!(state.maintain_tern(deadline));
                request.sequence = 3;
                request.hosts[0].status = TernHostStatus::Available;
                request.hosts[0].observation_generation = 2;
                let recovered = tern_test_exchange_at(&mut state, request, deadline);
                assert!(recovered.ok && recovered.command.is_none());
                assert_eq!(recovered.hosts[0].generation, 2);
                assert!(receipt.try_recv().is_err());
            }
        }
    }

    #[test]
    fn tern_ttl_cleanup_does_not_change_offline_herdr_rows_or_counts() {
        use crate::tern::{FRESHNESS, OFFLINE_GRACE};
        let mut state = tern_test_state(ObservationMode::Both);
        let local = "/isolated/retained-herdr.sock";
        connect(&mut state, local, 1);
        publish(&mut state, local, 1, AgentStatus::Working);
        let key = state
            .session_snapshot(&SessionListOptions::default(), None)
            .rows[0]
            .key
            .clone();
        let base = Instant::now();
        assert!(state.mark_source_offline_at(local, 1, base));
        let retained = state.session_view_for_key(&key).unwrap();
        assert!(tern_test_exchange_at(&mut state, tern_test_request("gone-tern"), base).ok);
        assert!(state.maintain_tern(base + FRESHNESS + OFFLINE_GRACE));
        assert_eq!(state.source_generation(local), Some(1));
        assert_eq!(state.retired_source_generation(local), None);
        assert_eq!(state.session_view_for_key(&key).unwrap(), retained);
        assert_eq!(state.scene().sessions, 1);
        assert_eq!(state.scene().unknown, 1);
        assert_eq!(state.scene().disconnected_sources, 1);
        assert_eq!(
            state
                .session_snapshot(&SessionListOptions::default(), None)
                .total,
            1
        );
    }

    #[test]
    fn tern_maintenance_finalizes_queued_and_offered_once_and_never_replays_late_ack() {
        use crate::herdr::PromptError;
        use crate::tern::{TernResult, TernResultStatus, FRESHNESS, OFFLINE_GRACE};
        for offered in [false, true] {
            let mut state = tern_test_state(ObservationMode::Tern);
            let base = Instant::now();
            let mut request = tern_test_request("pending-retirement");
            assert!(tern_test_exchange_at(&mut state, request.clone(), base).ok);
            let key = tern_test_key(&state, "pending-retirement");
            let (operation_id, receipt) = state
                .queue_tern_prompt(
                    key.clone(),
                    "pending-retirement",
                    1,
                    "0",
                    1,
                    "pane-a",
                    "one ask",
                )
                .unwrap();
            request.instance_id = Some("daemon-a".into());
            request.sequence = 2;
            if offered {
                assert!(tern_test_exchange_at(&mut state, request.clone(), base)
                    .command
                    .is_some());
                request.sequence = 3;
            }
            assert!(state.maintain_tern(base + FRESHNESS));
            assert_eq!(
                receipt.try_recv(),
                Ok(Err(if offered {
                    PromptError::UnknownDelivery
                } else {
                    PromptError::Offline
                }))
            );
            let source = crate::tern::source_id("pending-retirement", "0");
            let since = state.sources[&source].offline_since;
            request.results.push(TernResult {
                operation_id,
                pane: "pane-a".into(),
                status: TernResultStatus::Accepted,
                error: None,
            });
            assert_eq!(
                tern_test_exchange_at(&mut state, request.clone(), base + FRESHNESS).error_code,
                Some(TernExchangeErrorCode::ReceiptRejected)
            );
            assert_eq!(state.sources[&source].offline_since, since);
            let deadline = base + FRESHNESS + OFFLINE_GRACE;
            assert!(state.maintain_tern(deadline));
            assert_eq!(state.prompt_available(&key), Err(PromptError::StaleTarget));
            assert_eq!(
                tern_test_exchange_at(&mut state, request.clone(), deadline).error_code,
                Some(TernExchangeErrorCode::SourceRetired)
            );
            request.results.clear();
            request.epoch = 2;
            request.sequence = 1;
            request.instance_id = None;
            let rebound = tern_test_exchange_at(&mut state, request.clone(), deadline);
            assert!(rebound.ok && rebound.command.is_none());
            assert_eq!(rebound.hosts[0].generation, 2);
            request.sequence = 2;
            request.instance_id = Some("daemon-a".into());
            let poll = tern_test_exchange_at(&mut state, request, deadline);
            assert!(poll.ok && poll.command.is_none());
            assert!(receipt.try_recv().is_err());
        }
    }

    #[test]
    fn tern_grace_lock_order_allows_coherent_recovery_only_before_pruning() {
        use crate::tern::{FRESHNESS, OFFLINE_GRACE};
        for prune_first in [false, true] {
            let mut state = tern_test_state(ObservationMode::Tern);
            let base = Instant::now();
            let mut request = tern_test_request("lock-order");
            assert!(tern_test_exchange_at(&mut state, request.clone(), base).ok);
            assert!(state.expire_tern(base + FRESHNESS));
            request.instance_id = Some("daemon-a".into());
            request.sequence = 2;
            let now = base + FRESHNESS + OFFLINE_GRACE + Duration::from_secs(1);
            if prune_first {
                assert!(state.maintain_tern(now));
                let revision = state.session_revision();
                assert_eq!(
                    tern_test_exchange_at(&mut state, request, now).error_code,
                    Some(TernExchangeErrorCode::SourceRetired)
                );
                assert_eq!(state.session_revision(), revision);
                assert_eq!(state.scene().sessions, 0);
            } else {
                let recovered = tern_test_exchange_at(&mut state, request, now);
                assert!(recovered.ok && recovered.command.is_none());
                assert_eq!(recovered.hosts[0].generation, 2);
                let source = crate::tern::source_id("lock-order", "0");
                assert_eq!(state.sources[&source].offline_since, None);
                assert_eq!(state.tern.get("lock-order").unwrap().offline_since, None);
                assert!(!state.maintain_tern(now));
                assert_eq!(state.scene().sessions, 1);
                assert_eq!(state.host_connections().tern, 1);
                assert_eq!(state.tern.retired_epoch("lock-order"), None);
            }
        }
    }

    #[test]
    fn tern_empty_live_sources_survive_and_offline_empty_sources_and_parents_retire() {
        use crate::tern::{TernHostStatus, FRESHNESS, OFFLINE_GRACE};
        let mut state = tern_test_state(ObservationMode::Tern);
        let base = Instant::now();
        let mut live = tern_test_request("empty-live");
        live.hosts[0].agents.clear();
        assert!(tern_test_exchange_at(&mut state, live.clone(), base).ok);
        let live_source = crate::tern::source_id("empty-live", "0");
        assert_eq!(state.host_connections().tern, 1);
        let mut empty_parent = tern_test_request("empty-parent");
        empty_parent.hosts[0].status = TernHostStatus::Unavailable;
        empty_parent.hosts[0].agents.clear();
        assert!(tern_test_exchange_at(&mut state, empty_parent, base).ok);
        assert!(state.tern.get("empty-parent").unwrap().hosts.is_empty());
        let deadline = base + FRESHNESS + OFFLINE_GRACE;
        set_tern_test_last_seen(&mut state, "empty-live", None, deadline);
        assert!(state.maintain_tern(deadline));
        assert!(state.sources[&live_source].connected);
        assert_eq!(state.host_connections().tern, 1);
        assert!(state.tern.get("empty-live").is_some());
        assert!(state.tern.get("empty-parent").is_none());
        assert_eq!(state.tern.retired_epoch("empty-parent"), Some(1));
        live.instance_id = Some("daemon-a".into());
        live.sequence = 2;
        live.hosts[0].status = TernHostStatus::Disconnected;
        assert!(tern_test_exchange_at(&mut state, live, deadline).ok);
        set_tern_test_last_seen(&mut state, "empty-live", None, deadline + OFFLINE_GRACE);
        assert!(state.maintain_tern(deadline + OFFLINE_GRACE));
        assert!(!state.sources.contains_key(&live_source));
        assert!(state.tern.get("empty-live").is_some());
        assert_eq!(state.scene().sessions, 0);
        assert_eq!(state.scene().disconnected_sources, 0);
        assert_eq!(state.host_connections().tern, 0);
    }

    #[test]
    fn tern_invalid_envelopes_cannot_refresh_offline_clocks_or_run_pruning() {
        use crate::tern::{TernResult, TernResultStatus, FRESHNESS, OFFLINE_GRACE};
        let mut state = tern_test_state(ObservationMode::Tern);
        let base = Instant::now();
        let mut request = tern_test_request("fail-closed-clocks");
        assert!(tern_test_exchange_at(&mut state, request.clone(), base).ok);
        assert!(state.expire_tern(base + FRESHNESS));
        let source = crate::tern::source_id("fail-closed-clocks", "0");
        request.instance_id = Some("daemon-a".into());
        request.sequence = 2;
        let mut invalid = Vec::new();
        let mut malformed = request.clone();
        malformed.version = 1;
        invalid.push(malformed);
        let mut wrong_instance = request.clone();
        wrong_instance.instance_id = Some("other-daemon".into());
        invalid.push(wrong_instance);
        let mut stale_sequence = request.clone();
        stale_sequence.sequence = 1;
        invalid.push(stale_sequence);
        let mut bad_receipt = request;
        bad_receipt.results.push(TernResult {
            operation_id: "not-offered".into(),
            pane: "pane-a".into(),
            status: TernResultStatus::Accepted,
            error: None,
        });
        invalid.push(bad_receipt);
        let deadline = base + FRESHNESS + OFFLINE_GRACE;
        let revision = state.session_revision();
        for request in invalid {
            let reply = tern_test_exchange_at(&mut state, request, deadline);
            assert!(!reply.ok && reply.command.is_none());
            assert_eq!(state.session_revision(), revision);
            assert_eq!(state.sources[&source].offline_since, Some(base + FRESHNESS));
            let parent = state.tern.get("fail-closed-clocks").unwrap();
            assert_eq!(parent.offline_since, Some(base + FRESHNESS));
            assert_eq!(parent.last_seen, base);
            assert_eq!(parent.hosts["0"].last_seen, base);
        }
        assert!(state.maintain_tern(deadline));
        assert_eq!(state.source_generation(&source), None);
    }

    #[test]
    fn show_control_does_not_disable_full_passthrough() {
        let mut state = AppState::new();
        state.visible = false;
        state.passthrough = true;
        state.bubble_visible = false;

        assert!(state.apply_control("show").is_ok());

        let scene = state.scene();
        assert!(scene.visible);
        assert!(scene.passthrough);
        assert!(!scene.bubble_visible);
        assert!(scene.needs_recovery_entry());
    }
}
