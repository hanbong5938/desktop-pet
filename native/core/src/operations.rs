use crate::state::AppState;
use desktop_pet_contracts::{
    PromptError, PromptReceipt, PromptResult, PromptSubmission, RequestOrigin, SessionKey,
    WorktreeRemoveError, WorktreeRemoveResult, WorktreeRemoveTarget,
};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

/// GUI and automation use the same single-flight owner. Completion belongs to
/// the adapter; an accepted delivery is never promoted to an acknowledgement.
pub(crate) struct PromptSender {
    shared: Arc<Mutex<AppState>>,
    pending: Option<(
        Receiver<Result<PromptReceipt, PromptError>>,
        Arc<PromptSubmission>,
    )>,
}

impl PromptSender {
    pub(crate) fn new(shared: Arc<Mutex<AppState>>) -> Self {
        Self {
            shared,
            pending: None,
        }
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn submit(
        &mut self,
        key: SessionKey,
        text: String,
        origin: RequestOrigin,
    ) -> Result<(), PromptError> {
        if self.is_pending() {
            return Err(PromptError::Busy);
        }
        if text.trim().is_empty() {
            return Err(PromptError::Empty);
        }
        let integration = {
            let state = self.shared.lock().map_err(|_| PromptError::Offline)?;
            state
                .update_mutation_allowed()
                .map_err(PromptError::Other)?;
            let (_, provider) = state.prompt_route(&key)?;
            state
                .integration(&provider)
                .ok_or(PromptError::Unsupported)?
        };
        let submission = Arc::new(PromptSubmission { key, text, origin });
        // Final admission and queue registration are a single adapter-owned
        // domain transaction. Never enter an adapter while holding this lock.
        let receipt = integration.submit_prompt(submission.key.clone(), submission.text.clone())?;
        self.pending = Some((receipt, submission));
        Ok(())
    }

    pub(crate) fn try_result(&mut self) -> Option<PromptResult> {
        let (receiver, _) = self.pending.as_ref()?;
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(PromptError::UnknownDelivery),
        };
        let (_, submission) = self.pending.take()?;
        Some(PromptResult { submission, result })
    }
}

/// Keeps the frozen confirmation target and request origin until the actual
/// adapter receipt is consumed, including an uncertain delivery.
pub(crate) struct WorktreeRemoveSender {
    shared: Arc<Mutex<AppState>>,
    pending: Option<(
        Receiver<Result<(), WorktreeRemoveError>>,
        WorktreeRemoveTarget,
        RequestOrigin,
    )>,
}

impl WorktreeRemoveSender {
    pub(crate) fn new(shared: Arc<Mutex<AppState>>) -> Self {
        Self {
            shared,
            pending: None,
        }
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn submit(
        &mut self,
        target: WorktreeRemoveTarget,
        origin: RequestOrigin,
    ) -> Result<u64, WorktreeRemoveError> {
        if self.is_pending() {
            return Err(WorktreeRemoveError::Busy);
        }
        let (integration, baseline_revision) = {
            let state = self
                .shared
                .lock()
                .map_err(|_| WorktreeRemoveError::Offline)?;
            state
                .update_mutation_allowed()
                .map_err(WorktreeRemoveError::Other)?;
            if state.worktree_remove_target(&target.key)? != target {
                return Err(WorktreeRemoveError::StaleTarget);
            }
            let descriptor = state
                .source_descriptor(&target.source)
                .ok_or(WorktreeRemoveError::StaleTarget)?;
            let integration = state
                .integration(&descriptor.provider)
                .ok_or(WorktreeRemoveError::Unsupported)?;
            let baseline = state
                .worktree_snapshot_revision(&target)
                .ok_or(WorktreeRemoveError::Offline)?;
            (integration, baseline)
        };
        let receipt = integration.submit_worktree_remove(target.clone())?;
        // A publication between preflight and adapter admission must not be
        // treated as subsequent authoritative evidence. Sampling afterward is
        // conservative; a disappearing target still retains its real receipt.
        let baseline_revision = self
            .shared
            .lock()
            .ok()
            .and_then(|state| state.worktree_snapshot_revision(&target))
            .unwrap_or(baseline_revision);
        self.pending = Some((receipt, target, origin));
        Ok(baseline_revision)
    }

    pub(crate) fn try_result(&mut self) -> Option<WorktreeRemoveResult> {
        let (receiver, _, _) = self.pending.as_ref()?;
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(WorktreeRemoveError::UnknownDelivery),
        };
        let (_, target, origin) = self.pending.take()?;
        Some(WorktreeRemoveResult {
            target,
            origin,
            result,
        })
    }
}
