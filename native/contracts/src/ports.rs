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
    fn source_admission(&self, source: &str, generation: u64) -> SourceAdmission;
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
