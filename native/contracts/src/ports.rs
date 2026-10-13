use crate::*;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;

/// Synchronous access to the single shared domain mutex. On successful lock
/// acquisition operation is called exactly once, before returning. A poisoned
/// mutex returns Err without calling operation; adapters surface that error as
/// source failure, never recover a poisoned guard or silently drop a publish.
///
/// Lock order is domain -> adapter registry. Never call with_domain while a
/// registry guard or a domain callback is held. Socket I/O, subprocess waits,
/// receipt waits, and worker joins run outside the domain lock. wake_ui does
/// not acquire the domain lock and may be called after a transaction commits.
pub trait RuntimePort: Send + Sync {
    fn with_domain(&self, operation: &mut dyn FnMut(&mut dyn DomainAccess)) -> Result<(), String>;
    fn wake_ui(&self);
    /// Durable common profile/update/migration admission. Called outside a
    /// domain/registry transaction before host work; implementations may read
    /// their actual common gates, never invent authority from adapter inputs.
    /// Final domain mutation_allowed and adapter epoch checks remain required
    /// in the separate linearizable handoff transaction.
    fn mutation_admission(&self) -> Result<(), String> {
        Err("runtime does not support durable mutation admission".to_owned())
    }
}

/// Host-neutral operations on the already-locked domain. All references are
/// limited to this transaction; adapters do not retain a DomainAccess pointer.
/// Snapshots are consumed by borrowed iteration, preserving shared Arc metadata.
///
/// The MAX_SOURCES budget is shared across providers. Capacity reclamation may
/// remove only a source whose generation and offline state are still current,
/// and whose owning adapter confirms expired/no pending work in the same
/// domain -> registry transaction. Live or pending sources are never evicted.
pub trait DomainAccess {
    fn instance_id(&self) -> &str;
    fn mutation_allowed(&self) -> Result<(), String>;
    fn source_generation(&self, source: &str) -> Option<u64>;
    fn retired_source_generation(&self, source: &str) -> Option<u64>;
    fn source_descriptor(&self, source: &str) -> Option<&SourceDescriptor>;
    /// Updates presentation data only when the source generation still matches.
    /// Never changes source identity, provider, locality, capabilities, observed
    /// session status/outcomes, or admission authority. Synchronizing session
    /// presentation labels is permitted. False means unsupported/missing/stale,
    /// with no mutation; a successful implementation preserves authority.
    fn set_source_label(&mut self, _source: &str, _generation: u64, _label: &str) -> bool {
        false
    }
    /// Changes observation visibility only when the source generation matches.
    /// Hidden sources continue ingesting snapshots and retain the same session
    /// keys. Core excludes them from observed presentation, aggregates, action
    /// admission, and reactions without changing descriptor authority, source
    /// generation, or live connection counts. Provider disable still retires;
    /// hiding is not retirement. False means unsupported/missing/stale and
    /// performs no mutation.
    fn set_source_observed(&mut self, _source: &str, _generation: u64, _observed: bool) -> bool {
        false
    }
    fn source_admission(&self, source: &str, generation: u64) -> SourceAdmission;
    /// Pressure-only admission check, BEFORE the requesting adapter takes its
    /// registry guard. A shared-domain implementation may ask all registered
    /// adapters to reclaim expired sources when admission is Capacity, then
    /// recheck admission. No host I/O or RuntimePort reentry is permitted.
    /// Adapters that already inspected a registry must release it first and
    /// reacquire only after this call, in the same domain transaction.
    fn ensure_source_capacity(
        &mut self,
        source: &str,
        generation: u64,
        _now: Instant,
    ) -> SourceAdmission {
        self.source_admission(source, generation)
    }
    /// Rechecks both generation and offline state before removing. The caller
    /// holds its registry guard after the domain lock and has checked no pending
    /// work; false leaves a mismatched/live source untouched.
    fn remove_offline_source(&mut self, source: &str, generation: u64) -> bool;
    fn begin_source(&mut self, descriptor: SourceDescriptor, generation: u64) -> bool;
    fn publish_source_snapshot(
        &mut self,
        source: &str,
        generation: u64,
        records: &mut dyn Iterator<Item = &AgentRecord>,
        counts: SourceCounts,
        pane_ids: Arc<[String]>,
        completions: &[(String, String)],
        observed_at: Instant,
    ) -> bool;
    fn update_session_status(&mut self, update: SessionStatusUpdate<'_>) -> bool;
    fn mark_source_offline(&mut self, source: &str, generation: u64) -> bool;
    fn remove_source(&mut self, source: &str, generation: u64) -> bool;
    fn invalidate_session_outcome(
        &mut self,
        source: &str,
        generation: u64,
        terminal_id: &str,
        pane_id: &str,
    );
    fn push_outcome(&mut self, observation: OutcomeObservation);
    fn push_completion(&mut self, observation: CompletionObservation);
    fn prompt_target(&self, key: &SessionKey) -> Result<PromptTarget, PromptError>;
    /// String is the registered opaque provider ID, never endpoint routing.
    fn prompt_route(&self, key: &SessionKey) -> Result<(PromptTarget, String), PromptError>;
    fn worktree_remove_target(
        &self,
        key: &SessionKey,
    ) -> Result<WorktreeRemoveTarget, WorktreeRemoveTargetError>;
}

/// An adapter, registered once by the executable composition root. Core owns
/// the original prompt, RequestOrigin, GUI/CLI shared single-flight senders,
/// and completion presentation. Adapters own routing and asynchronous I/O.
///
/// Submission admission is linearizable with Prepare, observation switching,
/// retire, and shutdown: submit_* is invoked OUTSIDE a domain callback; the
/// adapter uses its saved RuntimePort to perform final mutation_allowed,
/// instance/source/generation/coherence/capability/provider checks and queue
/// registration in ONE domain -> registry transaction. Preflight checks in a
/// sender are not admission. A worker never offers a queued operation retired
/// by that transaction. No I/O or receiver wait is performed under either lock.
/// Concurrent submits are subject to the sender's single-flight and adapter's
/// bounded queue; an admission failure returns Err without registering work.
///
/// Retire runs inside the same domain transaction that closes the mutation gate
/// or changes selection, then locks the registry: queued work is cancelled,
/// offered work completes UnknownDelivery and is never replayed in a new
/// binding. An accepted receipt is not a model completion. Unsupported/read-only
/// capabilities return existing Unsupported/ReadOnly errors, not fabricated
/// success. Once admitted, the receiver gets exactly one terminal result; loss
/// after offer is UnknownDelivery, not safe-to-retry.
pub trait Integration: Send + Sync {
    fn id(&self) -> &'static str;
    fn start(&self, runtime: Arc<dyn RuntimePort>) -> Result<(), String>;
    fn tick(&self, now: Instant);
    /// Already inside the domain transaction, with no requester registry held.
    /// Called only under shared source-capacity pressure. Lock domain -> own
    /// registry, reclaim only owned expired/offline/no-pending sources, and use
    /// remove_offline_source to recheck their current generation/offline state.
    /// Return actual removals; zero truthfully means no eligible sources.
    /// No RuntimePort reentry, host I/O, or copied candidate collection.
    fn reclaim_expired_sources(&self, _now: Instant, _domain: &mut dyn DomainAccess) -> usize {
        0
    }
    /// Already inside with_domain. Must not reenter RuntimePort.
    fn set_enabled(&self, enabled: bool, domain: &mut dyn DomainAccess);
    /// Already inside with_domain. Must not reenter RuntimePort.
    fn retire(&self, domain: &mut dyn DomainAccess);
    /// Joins workers only after retire and native UI termination, outside locks.
    fn shutdown(&self);
    fn submit_prompt(
        &self,
        key: SessionKey,
        text: String,
    ) -> Result<Receiver<Result<PromptReceipt, PromptError>>, PromptError>;
    fn submit_worktree_remove(
        &self,
        target: WorktreeRemoveTarget,
    ) -> Result<Receiver<Result<(), WorktreeRemoveError>>, WorktreeRemoveError>;
}
