//! App-side, read-only update discovery and explicit helper handoff.
//! AppKit never waits for a manager, hash, source probe, or helper stdout.
use crate::i18n::{text, Message, UiLocale};
use crate::update_card::UpdateCardModel;
use herdr_update_coordinator::protocol::{
    latest_operation, lock_operation, private_updates, start_allowed, ExecutionFence,
    OperationPhase, OperationRecord, UPDATER_PROTOCOL,
};
use herdr_update_coordinator::{
    check, read_operation, read_plan, recheck_plan, stage_helper, write_plan, CheckSnapshot,
    CheckStatus, InstallOrigin, UpdateContext, UpdatePlan,
};
use serde::Deserialize;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CHECK_INTERVAL: u64 = 24 * 60 * 60;

pub(super) enum Event {
    Checked {
        result: Result<CheckSnapshot, String>,
        settled: Option<SettledOperationProof>,
    },
    Launched {
        operation_id: String,
        result: Result<(), LaunchFailure>,
    },
    Journal {
        operation_id: String,
        result: Result<Option<OperationRecord>, String>,
        prepared_recovery: Option<PreparedRecoveryProof>,
    },
    LatestJournal(Result<Option<OperationRecord>, String>),
}

#[derive(Deserialize)]
struct OwnershipAck {
    protocol: u32,
    operation_id: String,
    visible: bool,
    prepared: bool,
    #[serde(default)]
    error: Option<String>,
}

// Only the staging worker can prove whether Command::spawn was never reached
// (or failed). Once spawn succeeds, even a missing reply cannot release ownership.
pub(super) enum LaunchFailure {
    NoSpawn(String),
    Spawned(String),
}

// Only a background admission check can construct this proof. It identifies the exact
// journal observation that a fresh check may retire; durable evidence is never removed.
pub(super) struct SettledOperationProof(OperationRecord);
// Constructed only by the journal worker while holding the finished helper's
// operation lease. Unlike fresh-check retirement, this must match the original
// prepared instance and its immutable plan, not a replacement check's context.
pub(super) struct PreparedRecoveryProof {
    record: OperationRecord,
    plan: UpdatePlan,
}

pub(super) struct AppUpdateService {
    context: Option<UpdateContext>,
    snapshot: Option<CheckSnapshot>,
    check_busy: bool,
    launching: Option<String>,
    launch_plan: Option<UpdatePlan>,
    prepared_plan: Option<UpdatePlan>,
    operation: Option<OperationRecord>,
    previous_outcome: Option<OperationRecord>,
    observed_operation_id: Option<String>,
    retired_operation_id: Option<String>,
    next_journal_poll: Instant,
    problem: Option<String>,
    journal_error: Option<String>,
    journal_error_operation_id: Option<String>,
    sender: Sender<Event>,
    receiver: Receiver<Event>,
    last_checked: Option<u64>,
    auto_check: bool,
    next_auto_due: Option<u64>,
    revision: u64,
}

impl AppUpdateService {
    pub(super) fn new(
        context: Option<UpdateContext>,
        auto_check: bool,
        last_checked: Option<u64>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        if let Some(context) = context.as_ref() {
            let sender = sender.clone();
            let state_dir = context.state_dir.clone();
            std::thread::spawn(move || {
                let _ = sender.send(Event::LatestJournal(latest_operation(&state_dir)));
            });
        }
        let mut service = Self {
            context,
            snapshot: None,
            check_busy: false,
            launching: None,
            launch_plan: None,
            prepared_plan: None,
            previous_outcome: None,
            operation: None,
            observed_operation_id: None,
            retired_operation_id: None,
            next_journal_poll: Instant::now(),
            problem: None,
            journal_error: None,
            journal_error_operation_id: None,
            sender,
            receiver,
            last_checked,
            auto_check,
            next_auto_due: None,
            revision: 0,
        };
        service.check_due();
        service
    }
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn set_auto_check(&mut self, enabled: bool) {
        if self.auto_check != enabled {
            self.auto_check = enabled;
            self.revision = self.revision.wrapping_add(1);
        }
        if enabled {
            self.check_due();
        }
    }

    pub(super) fn check_due(&mut self) {
        let Some(time) = now() else { return };
        if self.auto_check
            && self.next_auto_due.is_none_or(|due| time >= due)
            && self
                .last_checked
                .is_none_or(|last| time.saturating_sub(last) >= CHECK_INTERVAL)
        {
            self.next_auto_due = Some(time.saturating_add(CHECK_INTERVAL));
            self.check_now();
        }
    }

    pub(super) fn check_now(&mut self) {
        if self.check_busy || self.launching.is_some() {
            return;
        }
        let Some(context) = self.context.clone() else {
            let problem = "Running app update context is unavailable";
            if self.problem.as_deref() != Some(problem) {
                self.problem = Some(problem.into());
                self.revision = self.revision.wrapping_add(1);
            }
            return;
        };
        self.check_busy = true;
        self.problem = None;
        self.revision = self.revision.wrapping_add(1);
        let sender = self.sender.clone();
        let observed_operation_id = self.observed_operation_id.clone();
        std::thread::spawn(move || {
            let result = check(&context);
            let prior_operation = observed_operation_id.or_else(|| {
                latest_operation(&context.state_dir)
                    .ok()
                    .flatten()
                    .map(|record| record.operation_id)
            });
            let settled = result.as_ref().ok().and_then(|snapshot| {
                prior_operation.as_deref().and_then(|operation_id| {
                    settlement_proof(&context, snapshot, operation_id).ok()
                })
            });
            let _ = sender.send(Event::Checked { result, settled });
        });
    }

    pub(super) fn next_event(&self) -> Option<Event> {
        self.receiver.try_recv().ok()
    }

    pub(super) fn checked(
        &mut self,
        result: Result<CheckSnapshot, String>,
        settled: Option<SettledOperationProof>,
    ) -> Option<u64> {
        let mut changed = self.check_busy
            || match &result {
                Ok(next) => self.snapshot.as_ref().is_none_or(|old| {
                    old.status != next.status
                        || old.origin != next.origin
                        || old.installed_version != next.installed_version
                        || old.detail != next.detail
                        || old.plan.as_ref().map(|plan| &plan.action)
                            != next.plan.as_ref().map(|plan| &plan.action)
                }),
                Err(_) => false,
            };
        self.check_busy = false;
        let checked_at = match result {
            Ok(snapshot) if snapshot.status != CheckStatus::Failed => {
                let checked_at = snapshot.checked_at.or_else(now);
                changed |= self.last_checked != checked_at || self.problem.is_some();
                self.last_checked = checked_at;
                self.next_auto_due = checked_at.map(|time| time.saturating_add(CHECK_INTERVAL));
                self.problem = None;
                if let Some(SettledOperationProof(record)) = settled {
                    let matching_observation = self.observed_operation_id.as_deref()
                        == Some(record.operation_id.as_str())
                        && self.operation.as_ref() == Some(&record);
                    let not_yet_observed =
                        self.observed_operation_id.is_none() && self.operation.is_none();
                    if self.launching.is_none()
                        && self.journal_error.is_none()
                        && (matching_observation || not_yet_observed)
                    {
                        // Keep the proven outcome for presentation even after the runtime
                        // observation retires; never delete or alter the durable journal.
                        self.previous_outcome = Some(record.clone());
                        changed = true;
                        self.observed_operation_id = None;
                        self.operation = None;
                        if self
                            .prepared_plan
                            .as_ref()
                            .is_some_and(|plan| plan.operation_id == record.operation_id)
                        {
                            self.prepared_plan = None;
                        }
                        self.retired_operation_id = Some(record.operation_id);
                    }
                }
                self.snapshot = Some(snapshot);
                checked_at
            }
            Ok(snapshot) => {
                changed |= self.problem.as_deref() != Some(snapshot.detail.as_str());
                self.problem = Some(snapshot.detail.clone());
                self.snapshot = Some(snapshot);
                None
            }
            Err(error) => {
                changed |= self.problem.as_deref() != Some(error.as_str());
                self.problem = Some(error);
                None
            }
        };
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        checked_at
    }
    pub(super) fn plan(&self) -> Option<&UpdatePlan> {
        if self.check_busy
            || self.launching.is_some()
            || self.observed_operation_id.is_some()
            || self.operation.is_some()
            || self.problem.is_some()
            || self.journal_error.is_some()
        {
            return None;
        }
        self.snapshot.as_ref()?.plan.as_ref()
    }
    pub(super) fn preparing_plan(&self, operation_id: &str) -> Option<&UpdatePlan> {
        self.launch_plan
            .as_ref()
            .filter(|plan| plan.operation_id == operation_id)
    }

    pub(super) fn launch(&mut self, plan: UpdatePlan) -> Result<(), String> {
        if self.check_busy || self.launching.is_some() || self.plan() != Some(&plan) {
            return Err("Update state changed; check again before applying".into());
        }
        self.launching = Some(plan.operation_id.clone());
        self.observed_operation_id = Some(plan.operation_id.clone());
        self.prepared_plan = None;
        self.launch_plan = Some(plan.clone());
        self.problem = None;
        self.revision = self.revision.wrapping_add(1);
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = stage_and_launch(&plan);
            let _ = sender.send(Event::Launched {
                operation_id: plan.operation_id,
                result,
            });
        });
        Ok(())
    }

    pub(super) fn launch_result(
        &mut self,
        operation_id: &str,
        result: Result<(), LaunchFailure>,
    ) -> bool {
        if self.launching.as_deref() != Some(operation_id) {
            return false;
        }
        if let Err(error) = result {
            self.launching = None;
            match error {
                LaunchFailure::NoSpawn(detail) => {
                    self.problem = Some(detail);
                    // A staging/spawn failure proves this attempt never gave a
                    // helper ownership. Do not retire independent journal evidence.
                    if self.operation.is_none()
                        && (self.journal_error.is_none()
                            || self.journal_error_operation_id.as_deref() == Some(operation_id))
                    {
                        self.observed_operation_id = None;
                        self.journal_error = None;
                        self.journal_error_operation_id = None;
                        self.launch_plan = None;
                    } else {
                        self.prepared_plan = self.launch_plan.take();
                    }
                }
                LaunchFailure::Spawned(detail) => {
                    self.problem = Some(detail);
                    self.prepared_plan = self.launch_plan.take();
                }
            }
            self.revision = self.revision.wrapping_add(1);
            return false;
        }
        // Keep the original plan to authenticate an eventual recovery proof.
        // The helper may fail after Prepare but before its ownership reply.
        self.prepared_plan = self.launch_plan.take();
        self.next_journal_poll = Instant::now() + Duration::from_secs(5);
        true
    }
    pub(super) fn abort_launch(&mut self, operation_id: &str, detail: String) {
        if self.launching.as_deref() != Some(operation_id) {
            return;
        }
        self.launching = None;
        self.prepared_plan = self.launch_plan.take();
        self.problem = Some(detail);
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn poll_operation(&mut self) {
        if self.launch_plan.is_some() || Instant::now() < self.next_journal_poll {
            return;
        }
        let Some(operation_id) = self.observed_operation_id.clone() else {
            return;
        };
        self.next_journal_poll = Instant::now() + Duration::from_secs(5);
        self.reload_journal(operation_id);
    }
    pub(super) fn reload_journal(&self, operation_id: String) {
        let Some(context) = self.context.as_ref() else {
            return;
        };
        let context = context.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = read_operation(&context.state_dir, &operation_id);
            let prepared_recovery = result
                .as_ref()
                .ok()
                .and_then(Option::as_ref)
                .and_then(|record| prepared_recovery_proof(&context, &operation_id, record).ok());
            let _ = sender.send(Event::Journal {
                operation_id,
                result,
                prepared_recovery,
            });
        });
    }

    pub(super) fn journal(
        &mut self,
        operation_id: &str,
        result: Result<Option<OperationRecord>, String>,
        prepared_recovery: Option<PreparedRecoveryProof>,
    ) -> Option<UpdateContext> {
        if self.observed_operation_id.as_deref() != Some(operation_id) {
            return None;
        }
        let old_record = self.operation.as_ref();
        let old_error = self.journal_error.as_ref();
        let mut changed = match &result {
            Ok(Some(record)) if record.operation_id == operation_id => {
                old_record != Some(record) || old_error.is_some()
            }
            Ok(_) => {
                old_error.map(String::as_str)
                    != Some("Coordinator operation journal is not available; outcome is unknown")
            }
            Err(error) => {
                old_error.map(String::as_str)
                    != Some(
                        format!("Coordinator operation journal cannot be read: {error}").as_str(),
                    )
            }
        };
        let terminal = matches!(&result, Ok(Some(record))
        if record.operation_id == operation_id
            && matches!(
                record.phase,
                OperationPhase::Completed
                    | OperationPhase::Failed
                    | OperationPhase::Unknown
                    | OperationPhase::UserStopped
            ));
        match result {
            Ok(Some(record)) if record.operation_id == operation_id => {
                self.journal_error = None;
                self.journal_error_operation_id = None;
                self.operation = Some(record);
            }
            Ok(_) => {
                self.journal_error = Some(
                    "Coordinator operation journal is not available; outcome is unknown".into(),
                );
                self.journal_error_operation_id = Some(operation_id.into());
            }
            Err(error) => {
                self.journal_error = Some(format!(
                    "Coordinator operation journal cannot be read: {error}"
                ));
                self.journal_error_operation_id = Some(operation_id.into());
            }
        }
        if terminal {
            if let Some(record) = self.operation.as_ref() {
                if self.previous_outcome.as_ref() != Some(record) {
                    self.previous_outcome = Some(record.clone());
                    changed = true;
                }
            }
        }
        if terminal && self.launching.as_deref() == Some(operation_id) {
            self.launching = None;
            if let Some(plan) = self.launch_plan.take() {
                self.prepared_plan = Some(plan);
            }
            changed = true;
        }
        let reconciled = prepared_recovery.and_then(|proof| {
            let context = self.context.as_ref()?;
            if self.prepared_plan.as_ref() == Some(&proof.plan)
                && proof.plan.context == *context
                && self.operation.as_ref() == Some(&proof.record)
                && self.journal_error.is_none()
            {
                Some(context.clone())
            } else {
                None
            }
        });
        if reconciled.is_some() {
            self.prepared_plan = None;
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        reconciled
    }
    pub(super) fn latest_journal(&mut self, result: Result<Option<OperationRecord>, String>) {
        let old_record = self.operation.as_ref();
        let old_error = self.journal_error.as_ref();
        let changed = match &result {
            Ok(Some(record))
                if self
                    .launching
                    .as_ref()
                    .is_none_or(|id| id == &record.operation_id)
                    && self.retired_operation_id.as_deref()
                        != Some(record.operation_id.as_str()) =>
            {
                old_record != Some(record) || old_error.is_some()
            }
            Err(error) => {
                old_error.map(String::as_str)
                    != Some(format!("Update journal cannot be verified: {error}").as_str())
            }
            _ => false,
        };
        match result {
            Ok(Some(record))
                if self
                    .launching
                    .as_ref()
                    .is_none_or(|id| id == &record.operation_id)
                    && self.retired_operation_id.as_deref()
                        != Some(record.operation_id.as_str()) =>
            {
                self.journal_error = None;
                self.journal_error_operation_id = None;
                self.observed_operation_id = Some(record.operation_id.clone());
                if terminal_phase(record.phase) && self.previous_outcome.as_ref() != Some(&record) {
                    self.previous_outcome = Some(record.clone());
                }
                self.operation = Some(record);
            }
            Ok(_) => {}
            Err(error) => {
                self.journal_error = Some(format!("Update journal cannot be verified: {error}"));
                self.journal_error_operation_id = None;
            }
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub(super) fn saved_check_failure(&mut self, previous: Option<u64>, error: String) {
        let old_last = self.last_checked;
        let old_problem = self.problem.as_deref();
        let changed = old_last != previous
            || old_problem
                != Some(
                    format!("Update was checked, but the check time could not be saved: {error}")
                        .as_str(),
                );
        self.last_checked = previous;
        self.problem = Some(format!(
            "Update was checked, but the check time could not be saved: {error}"
        ));
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
    }
    pub(super) fn set_problem(&mut self, problem: String) {
        if self.problem.as_ref() != Some(&problem) {
            self.revision = self.revision.wrapping_add(1);
        }
        self.problem = Some(problem);
    }

    pub(super) fn model(&self, locale: UiLocale) -> UpdateCardModel {
        let origin = self
            .snapshot
            .as_ref()
            .map(|snapshot| &snapshot.origin)
            .or_else(|| self.context.as_ref()?.running_origin.as_deref());
        let active = self
            .operation
            .as_ref()
            .filter(|record| !terminal_phase(record.phase));
        let outcome = self
            .operation
            .as_ref()
            .filter(|record| terminal_phase(record.phase))
            .or(self.previous_outcome.as_ref());
        let history = outcome.map_or_else(String::new, |record| {
            format!(
                "{}: {}",
                text(locale, Message::AppUpdatePreviousOperation),
                text(locale, outcome_message(record))
            )
        });
        let (status, status_message, detail_message) = if self.journal_error.is_some() {
            (
                CheckStatus::Failed,
                Message::AppUpdateOutcomeUnknown,
                Message::AppUpdateJournalHelp,
            )
        } else if self.problem.is_some() && !self.check_busy {
            (
                CheckStatus::Failed,
                Message::AppUpdateFailed,
                Message::AppUpdateFailedHelp,
            )
        } else if let Some(record) = active {
            (
                CheckStatus::Checking,
                phase_message(record.phase),
                if record.phase == OperationPhase::Starting {
                    Message::AppUpdateStartingHelp
                } else {
                    Message::AppUpdateActiveHelp
                },
            )
        } else if self.launching.is_some() {
            (
                CheckStatus::Checking,
                Message::AppUpdateLaunchPending,
                Message::AppUpdateActiveHelp,
            )
        } else if self.check_busy {
            (
                CheckStatus::Checking,
                Message::AppUpdateChecking,
                Message::AppUpdateCheckingHelp,
            )
        } else if self.operation.as_ref().is_some_and(|record| {
            matches!(
                record.phase,
                OperationPhase::Unknown | OperationPhase::Failed | OperationPhase::UserStopped
            )
        }) {
            let record = self.operation.as_ref().expect("terminal operation");
            (
                CheckStatus::Failed,
                outcome_message(record),
                match record.phase {
                    OperationPhase::Unknown => Message::AppUpdateUnknownHelp,
                    OperationPhase::UserStopped => Message::AppUpdateStoppedHelp,
                    _ => Message::AppUpdateBlockedHelp,
                },
            )
        } else {
            let status = self
                .snapshot
                .as_ref()
                .map_or(CheckStatus::NotChecked, |snapshot| snapshot.status);
            let detail = if self.observed_operation_id.is_some() || self.prepared_plan.is_some() {
                Message::AppUpdateBlockedHelp
            } else {
                check_detail_message(status)
            };
            (status, check_status_message(status), detail)
        };
        let mut diagnostics = String::new();
        if let Some(context) = self.context.as_ref() {
            let _ = writeln!(
                diagnostics,
                "Running executable: {}",
                context.running.path.display()
            );
            let _ = writeln!(diagnostics, "Running SHA-256: {}", context.running.sha256);
        }
        if let Some(origin) = origin {
            let _ = writeln!(
                diagnostics,
                "Running source: {}",
                source_description(origin)
            );
        }
        if let Some(snapshot) = self.snapshot.as_ref() {
            let _ = writeln!(
                diagnostics,
                "Check: {:?} · {}",
                snapshot.status, snapshot.detail
            );
            if let Some(version) = snapshot.installed_version.as_ref() {
                let _ = writeln!(diagnostics, "Installed package: {version}");
            }
            if let Some(plan) = snapshot.plan.as_ref() {
                let _ = writeln!(diagnostics, "Candidate operation: {}", plan.operation_id);
                if let Some(candidate) = plan.candidate.as_ref() {
                    let _ = writeln!(
                        diagnostics,
                        "Candidate: {} · SHA-256 {}",
                        candidate.path.display(),
                        candidate.sha256
                    );
                }
            }
        }
        if let Some(record) = self.operation.as_ref() {
            append_operation_diagnostics(&mut diagnostics, "Observed operation", record);
        }
        if let Some(record) = self.previous_outcome.as_ref() {
            if self.operation.as_ref() != Some(record) {
                append_operation_diagnostics(&mut diagnostics, "Previous operation", record);
            }
        }
        if let Some(error) = self.problem.as_ref() {
            let _ = writeln!(diagnostics, "Check or launch error: {error}");
        }
        if let Some(error) = self.journal_error.as_ref() {
            let _ = writeln!(diagnostics, "Journal error: {error}");
        }
        UpdateCardModel {
            auto_check: self.auto_check,
            busy: self.check_busy || self.launching.is_some() || active.is_some(),
            status,
            status_text: text(locale, status_message).into(),
            running_version: self
                .context
                .as_ref()
                .map_or_else(String::new, |context| context.version.clone()),
            source: match origin {
                Some(InstallOrigin::Homebrew { formula, .. }) => {
                    format!(
                        "{} · {formula}",
                        text(locale, Message::AppUpdateSourceHomebrew)
                    )
                }
                _ => text(
                    locale,
                    origin.map_or(Message::AppUpdateSourceUnknown, source_message),
                )
                .into(),
            },
            detail: text(locale, detail_message).into(),
            history,
            diagnostics,
            last_checked: self.last_checked,
            action: self.plan().map(|plan| plan.action.clone()),
        }
    }
}

// Journal worker only. The helper must have exited, recovered NoSpawn with no
// applied/installed result, and released *both* reservation scopes. A mere
// Failed journal or a different original instance is never a thaw signal.
fn prepared_recovery_proof(
    context: &UpdateContext,
    operation_id: &str,
    observed: &OperationRecord,
) -> Result<PreparedRecoveryProof, String> {
    if observed.version != UPDATER_PROTOCOL
        || observed.phase != OperationPhase::Failed
        || observed.execution_fence != ExecutionFence::NoSpawn
        || observed.installed
        || observed.applied
    {
        return Err("prepared update has no reconciled NoSpawn failure".into());
    }
    let _lease = lock_operation(&context.state_dir, operation_id)?;
    let record = read_operation(&context.state_dir, operation_id)?
        .ok_or("prepared operation journal is missing")?;
    if &record != observed {
        return Err("prepared operation journal changed during reconciliation".into());
    }
    let plan = read_plan(&context.state_dir, operation_id)?;
    if plan.context != *context
        || plan.running != context.running
        || matches!(plan.origin, InstallOrigin::Unknown { .. })
    {
        return Err("prepared operation belongs to a different running instance or source".into());
    }
    if latest_operation(&context.state_dir)?.as_ref() != Some(&record) {
        return Err("prepared operation is no longer the latest journal".into());
    }
    start_allowed(&plan.origin, &context.state_dir, None, None)?;
    Ok(PreparedRecoveryProof { record, plan })
}

// Runs only on the check worker. A journal's terminal phase by itself is not
// reconciliation: verify its immutable plan, nonblocking helper lease and both
// installation/profile admission markers before retiring the runtime observation.
fn settlement_proof(
    context: &UpdateContext,
    snapshot: &CheckSnapshot,
    operation_id: &str,
) -> Result<SettledOperationProof, String> {
    let fresh_plan = snapshot
        .plan
        .as_ref()
        .ok_or("fresh check has no applicable plan")?;
    if fresh_plan.context != *context
        || fresh_plan.running != context.running
        || fresh_plan.origin != snapshot.origin
        || fresh_plan.operation_id == operation_id
    {
        return Err("fresh check plan is not for the running profile/source".into());
    }
    let _lease = lock_operation(&context.state_dir, operation_id)?;
    let record = read_operation(&context.state_dir, operation_id)?
        .ok_or("previous operation journal is missing")?;
    if !settled_terminal(&record) {
        return Err("previous operation is not a reconciled terminal outcome".into());
    }
    let prior_plan = read_plan(&context.state_dir, operation_id)?;
    if prior_plan.context.config_dir != context.config_dir {
        return Err("previous operation belongs to a different configuration profile".into());
    }
    let latest = latest_operation(&context.state_dir)?
        .ok_or("previous operation is no longer the latest journal")?;
    if latest != record {
        return Err("previous operation journal changed during admission".into());
    }
    start_allowed(&prior_plan.origin, &context.state_dir, None, None)?;
    if fresh_plan.origin != prior_plan.origin {
        start_allowed(&fresh_plan.origin, &context.state_dir, None, None)?;
    }
    Ok(SettledOperationProof(record))
}

fn settled_terminal(record: &OperationRecord) -> bool {
    record.version == UPDATER_PROTOCOL
        && record.execution_fence != ExecutionFence::ManagerIntent
        && match record.phase {
            OperationPhase::Completed => record.installed && record.applied,
            OperationPhase::Failed | OperationPhase::UserStopped => !record.applied,
            _ => false,
        }
}

fn terminal_phase(phase: OperationPhase) -> bool {
    matches!(
        phase,
        OperationPhase::Completed
            | OperationPhase::Failed
            | OperationPhase::Unknown
            | OperationPhase::UserStopped
    )
}

fn phase_message(phase: OperationPhase) -> Message {
    match phase {
        OperationPhase::Preparing => Message::AppUpdatePreparing,
        OperationPhase::Stopping => Message::AppUpdateStopping,
        OperationPhase::Installing => Message::AppUpdateInstalling,
        OperationPhase::Validating => Message::AppUpdateValidating,
        OperationPhase::Starting => Message::AppUpdateStarting,
        OperationPhase::Completed => Message::AppUpdateApplied,
        OperationPhase::Failed => Message::AppUpdateFailed,
        OperationPhase::Unknown => Message::AppUpdateOutcomeUnknown,
        OperationPhase::UserStopped => Message::AppUpdateUserStopped,
    }
}

fn outcome_message(record: &OperationRecord) -> Message {
    match record.phase {
        OperationPhase::Completed if record.installed && record.applied => {
            Message::AppUpdateApplied
        }
        OperationPhase::Failed => Message::AppUpdateFailed,
        OperationPhase::UserStopped => Message::AppUpdateUserStopped,
        _ => Message::AppUpdateOutcomeUnknown,
    }
}

fn source_message(origin: &InstallOrigin) -> Message {
    match origin {
        InstallOrigin::Herdr { .. } => Message::AppUpdateSourceHerdr,
        InstallOrigin::Local { .. } => Message::AppUpdateSourceLocal,
        InstallOrigin::Homebrew { .. } => Message::AppUpdateSourceHomebrew,
        InstallOrigin::Manual { .. } => Message::AppUpdateSourceManual,
        InstallOrigin::Unknown { .. } => Message::AppUpdateSourceUnknown,
    }
}

fn check_status_message(status: CheckStatus) -> Message {
    match status {
        CheckStatus::NotChecked => Message::AppUpdateNotChecked,
        CheckStatus::Checking => Message::AppUpdateChecking,
        CheckStatus::Current => Message::AppUpdateCurrent,
        CheckStatus::Applied => Message::AppUpdateApplied,
        CheckStatus::Available => Message::AppUpdateAvailable,
        CheckStatus::Installed => Message::AppUpdateInstalled,
        CheckStatus::Local => Message::AppUpdateLocal,
        CheckStatus::Unsupported => Message::AppUpdateUnsupported,
        CheckStatus::Failed => Message::AppUpdateFailed,
    }
}

fn check_detail_message(status: CheckStatus) -> Message {
    match status {
        CheckStatus::NotChecked => Message::AppUpdateNotCheckedHelp,
        CheckStatus::Checking => Message::AppUpdateCheckingHelp,
        CheckStatus::Current | CheckStatus::Applied => Message::AppUpdateCurrentHelp,
        CheckStatus::Available => Message::AppUpdateAvailableHelp,
        CheckStatus::Installed => Message::AppUpdateInstalledHelp,
        CheckStatus::Local => Message::AppUpdateLocalHelp,
        CheckStatus::Unsupported => Message::AppUpdateUnsupportedHelp,
        CheckStatus::Failed => Message::AppUpdateFailedHelp,
    }
}

fn append_operation_diagnostics(diagnostics: &mut String, label: &str, record: &OperationRecord) {
    let _ = writeln!(
        diagnostics,
        "{label}: {} · phase {:?} · installed {} · applied {} · fence {:?}",
        record.operation_id, record.phase, record.installed, record.applied, record.execution_fence
    );
    let _ = writeln!(diagnostics, "Operation detail: {}", record.detail);
    if let Some(candidate) = record.candidate.as_ref() {
        let _ = writeln!(
            diagnostics,
            "Candidate: {} · SHA-256 {}",
            candidate.path.display(),
            candidate.sha256
        );
    }
}

pub(super) fn source_description(origin: &InstallOrigin) -> String {
    match origin {
        InstallOrigin::Herdr {
            host,
            host_config_dir,
            checkout_root,
            plugin_root,
            source,
            requested_ref,
            resolved_commit,
            enabled,
        } => format!(
            "Herdr · host {} · host config {} · checkout {} · plugin {} · {source} · ref {} · commit {} · {}",
            host.display(),
            host_config_dir.display(),
            checkout_root.display(),
            plugin_root.display(),
            requested_ref.as_deref().unwrap_or("default"),
            resolved_commit.as_deref().unwrap_or("not observed"),
            if *enabled { "enabled" } else { "disabled" }
        ),
        InstallOrigin::Local { root } => format!("Local checkout · {}", root.display()),
        InstallOrigin::Homebrew {
            brew,
            formula,
            cellar_formula_root,
            prefix,
            locator,
        } => format!(
            "Homebrew · {formula} · brew {} · prefix {} · formula root {} · app {}",
            brew.display(),
            prefix.display(),
            cellar_formula_root.display(),
            locator.display()
        ),
        InstallOrigin::Manual { locator } => format!("Manual · {}", locator.display()),
        InstallOrigin::Unknown { reason } => format!("Unknown · {reason}"),
    }
}

fn now() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|time| time.as_secs())
}

fn stage_and_launch(plan: &UpdatePlan) -> Result<(), LaunchFailure> {
    // The plan remains bound to the startup image and captured install policy.
    recheck_plan(plan).map_err(LaunchFailure::NoSpawn)?;
    let helper = stage_helper(plan).map_err(LaunchFailure::NoSpawn)?;
    if let Err(error) = write_plan(plan) {
        let _ = std::fs::remove_file(&helper);
        return Err(LaunchFailure::NoSpawn(error));
    }
    let cwd = private_updates(&plan.context.state_dir).map_err(LaunchFailure::NoSpawn)?;
    let mut command = Command::new(helper);
    command
        .arg("run")
        .arg("--state-dir")
        .arg(&plan.context.state_dir)
        .arg("--operation-id")
        .arg(&plan.operation_id)
        .current_dir(cwd)
        .env_clear()
        .envs(&plan.context.environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(host_config_dir) = plan.context.host_plugin_config_dir.as_ref() {
        command.env(
            "HERDR_DESKTOP_PET_CAPTURED_HOST_PLUGIN_CONFIG_DIR",
            host_config_dir,
        );
    }
    // The helper owns its own AppKit loop and journal after this process exits.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|error| {
        LaunchFailure::NoSpawn(format!("Cannot launch update coordinator: {error}"))
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| LaunchFailure::Spawned("Coordinator ownership channel missing".into()))?;
    // The helper owns its journal and UI; after spawn, never kill it on timeout.
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout.take(4096))
            .read_line(&mut line)
            .map_err(|error| error.to_string())
            .and_then(|length| {
                if length == 0 || length >= 4096 {
                    Err("Coordinator ownership reply missing or oversized".into())
                } else {
                    serde_json::from_str::<OwnershipAck>(&line).map_err(|error| error.to_string())
                }
            });
        let _ = sender.send(result);
    });
    let ack = receiver
        .recv_timeout(Duration::from_secs(30))
        .map_err(|_| {
            LaunchFailure::Spawned(
                "Coordinator ownership confirmation timed out; check durable operation status before retrying"
                    .into(),
            )
        })?
        .map_err(LaunchFailure::Spawned)?;
    if ack.protocol != UPDATER_PROTOCOL
        || ack.operation_id != plan.operation_id
        || !ack.visible
        || !ack.prepared
    {
        return Err(LaunchFailure::Spawned(ack.error.unwrap_or_else(|| {
            "Coordinator did not confirm visible ownership and preparation".into()
        })));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use herdr_update_coordinator::{ExecutableIdentity, UpdateAction};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn local_check() -> CheckSnapshot {
        let root = PathBuf::from("/isolated/local-source");
        let running = ExecutableIdentity {
            path: root.join("app"),
            sha256: "current image".into(),
        };
        let context = UpdateContext {
            executable: running.path.clone(),
            version: "0.2.0".into(),
            instance_id: "isolated".into(),
            config_dir: PathBuf::from("/isolated/config"),
            host_plugin_config_dir: None,
            state_dir: PathBuf::from("/isolated/state"),
            herdr_socket: PathBuf::from("/isolated/socket"),
            running: running.clone(),
            running_origin: None,
            assets_override: None,
            environment: BTreeMap::new(),
            locale: "en".into(),
        };
        let origin = InstallOrigin::Local { root };
        CheckSnapshot {
            status: CheckStatus::Local,
            origin: origin.clone(),
            installed_version: Some("0.2.0".into()),
            observed_revision: None,
            detail: "rebuild on-disk sources".into(),
            plan: Some(UpdatePlan {
                version: UPDATER_PROTOCOL,
                operation_id: "e".repeat(32),
                context,
                origin,
                action: UpdateAction::LocalRebuild,
                running,
                candidate: None,
            }),
            checked_at: Some(42),
        }
    }

    fn failed_record(id: char) -> OperationRecord {
        OperationRecord {
            version: UPDATER_PROTOCOL,
            operation_id: id.to_string().repeat(32),
            phase: OperationPhase::Failed,
            execution_fence: ExecutionFence::NoSpawn,
            detail: "no manager ran; recovery released both reservations".into(),
            installed: false,
            applied: false,
            candidate: None,
        }
    }

    #[test]
    fn exact_recovered_prepared_failure_preserves_failed_card_and_thaws_only_that_operation() {
        let record = failed_record('a');
        let mut plan = local_check().plan.unwrap();
        plan.operation_id = record.operation_id.clone();
        let mut service = AppUpdateService::new(None, false, None);
        service.context = Some(plan.context.clone());
        service.launching = Some(record.operation_id.clone());
        service.launch_plan = Some(plan.clone());
        service.observed_operation_id = Some(record.operation_id.clone());
        assert_eq!(
            service.journal(
                &record.operation_id,
                Ok(Some(record.clone())),
                Some(PreparedRecoveryProof {
                    record: record.clone(),
                    plan: plan.clone(),
                }),
            ),
            Some(plan.context.clone())
        );
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Failed);
        assert!(service.plan().is_none());
        assert!(service.launching.is_none());

        let mut newer = plan.clone();
        newer.operation_id = "b".repeat(32);
        service.launch_plan = Some(newer.clone());
        service.prepared_plan = Some(newer.clone());
        service.observed_operation_id = Some(newer.operation_id.clone());
        assert_eq!(
            service.journal(
                &record.operation_id,
                Ok(Some(record.clone())),
                Some(PreparedRecoveryProof {
                    record: record.clone(),
                    plan,
                }),
            ),
            None
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&newer));
        let mut blocked = record;
        blocked.operation_id = newer.operation_id.clone();
        blocked.phase = OperationPhase::Unknown;
        assert_eq!(
            service.journal(&newer.operation_id, Ok(Some(blocked)), None),
            None
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&newer));
    }

    #[test]
    fn failed_phase_alone_or_a_different_prepared_image_cannot_thaw() {
        let record = failed_record('c');
        let mut plan = local_check().plan.unwrap();
        plan.operation_id = record.operation_id.clone();
        let mut service = AppUpdateService::new(None, false, None);
        service.context = Some(plan.context.clone());
        service.observed_operation_id = Some(record.operation_id.clone());
        service.prepared_plan = Some(plan.clone());
        assert_eq!(
            service.journal(&record.operation_id, Ok(Some(record.clone())), None),
            None
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        let mut other_image = plan.clone();
        other_image.context.running.sha256 = "different original".into();
        assert_eq!(
            service.journal(
                &record.operation_id,
                Ok(Some(record.clone())),
                Some(PreparedRecoveryProof {
                    record: record.clone(),
                    plan: other_image,
                }),
            ),
            None
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        let mut intent = record;
        intent.execution_fence = ExecutionFence::ManagerIntent;
        let operation_id = intent.operation_id.clone();
        assert_eq!(service.journal(&operation_id, Ok(Some(intent)), None), None);
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
    }

    #[test]
    fn fresh_check_retires_only_the_proven_reconciled_failure_and_exposes_rebuild() {
        let mut service = AppUpdateService::new(None, false, None);
        let prior = failed_record('a');
        service.latest_journal(Ok(Some(prior.clone())));
        assert!(service.plan().is_none());
        assert_eq!(
            service.checked(
                Ok(local_check()),
                Some(SettledOperationProof(prior.clone()))
            ),
            Some(42)
        );
        assert_eq!(
            service.plan().map(|plan| &plan.action),
            Some(&UpdateAction::LocalRebuild)
        );
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Local);
        let history = service.model(UiLocale::En).history;
        assert!(!service.model(UiLocale::En).busy);
        assert_eq!(
            service.model(UiLocale::En).action,
            Some(UpdateAction::LocalRebuild)
        );
        service.latest_journal(Ok(Some(prior.clone())));
        let prior_id = prior.operation_id.clone();
        service.journal(&prior_id, Ok(Some(prior)), None);
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Local);
        assert_eq!(service.model(UiLocale::En).history, history);
        assert!(service.plan().is_some());
    }

    #[test]
    fn checked_settlement_keeps_late_startup_history_from_blocking_a_new_plan() {
        let mut service = AppUpdateService::new(None, false, None);
        let prior = failed_record('a');
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof(prior.clone())),
        );
        service.latest_journal(Ok(Some(prior)));
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Local);
        assert_eq!(
            service.plan().map(|plan| &plan.action),
            Some(&UpdateAction::LocalRebuild)
        );
    }

    #[test]
    fn late_unproven_startup_history_still_blocks_replacement() {
        let mut service = AppUpdateService::new(None, false, None);
        service.checked(Ok(local_check()), None);
        let mut prior = failed_record('a');
        prior.phase = OperationPhase::Unknown;
        service.latest_journal(Ok(Some(prior)));
        assert!(service.plan().is_none());
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Failed);
    }

    #[test]
    fn reconciled_user_stop_can_offer_a_new_explicit_plan() {
        let mut service = AppUpdateService::new(None, false, None);
        let mut prior = failed_record('f');
        prior.phase = OperationPhase::UserStopped;
        assert!(settled_terminal(&prior));
        service.latest_journal(Ok(Some(prior.clone())));
        assert!(service.plan().is_none());
        service.checked(Ok(local_check()), Some(SettledOperationProof(prior)));
        assert_eq!(
            service.plan().map(|plan| &plan.action),
            Some(&UpdateAction::LocalRebuild)
        );
    }

    #[test]
    fn clearance_rejects_busy_lease_legacy_journal_and_retained_profile_marker() {
        use herdr_update_coordinator::protocol::{
            write_operation, write_private_bytes, write_private_new,
        };
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempfile::tempdir().unwrap();
        let state_dir = temporary.path().join("state");
        fs::create_dir(&state_dir).unwrap();
        fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let updates = private_updates(&state_dir).unwrap();
        let mut snapshot = local_check();
        let old = failed_record('f');
        let mut old_plan = snapshot.plan.as_ref().unwrap().clone();
        old_plan.operation_id = old.operation_id.clone();
        old_plan.context.state_dir = state_dir.clone();
        snapshot.plan.as_mut().unwrap().context.state_dir = state_dir.clone();
        write_private_new(
            &updates.join(format!("{}.plan.json", old.operation_id)),
            &serde_json::to_vec(&old_plan).unwrap(),
            0o600,
        )
        .unwrap();
        write_operation(&state_dir, &old).unwrap();

        let lease = lock_operation(&state_dir, &old.operation_id).unwrap();
        assert!(settlement_proof(&old_plan.context, &snapshot, &old.operation_id).is_err());
        assert!(prepared_recovery_proof(&old_plan.context, &old.operation_id, &old).is_err());
        drop(lease);

        let mut wrong_original = old_plan.context.clone();
        wrong_original.instance_id = "other-original".into();
        assert!(prepared_recovery_proof(&wrong_original, &old.operation_id, &old).is_err());
        for (phase, fence) in [
            (OperationPhase::Unknown, ExecutionFence::NoSpawn),
            (OperationPhase::Failed, ExecutionFence::ManagerIntent),
        ] {
            let mut unsafe_outcome = old.clone();
            unsafe_outcome.phase = phase;
            unsafe_outcome.execution_fence = fence;
            assert!(
                prepared_recovery_proof(&old_plan.context, &old.operation_id, &unsafe_outcome)
                    .is_err()
            );
        }
        let mut legacy = old.clone();
        legacy.version = 1;
        write_private_bytes(
            &updates.join(format!("{}.result.json", old.operation_id)),
            &serde_json::to_vec(&legacy).unwrap(),
            0o600,
        )
        .unwrap();
        assert!(settlement_proof(&old_plan.context, &snapshot, &old.operation_id).is_err());
        assert!(prepared_recovery_proof(&old_plan.context, &old.operation_id, &old).is_err());

        write_private_bytes(
            &updates.join(format!("{}.result.json", old.operation_id)),
            &serde_json::to_vec(&old).unwrap(),
            0o600,
        )
        .unwrap();
        write_private_bytes(
            &updates.join("profile.reservation.json"),
            br#"{"version":1}"#,
            0o600,
        )
        .unwrap();
        assert!(settlement_proof(&old_plan.context, &snapshot, &old.operation_id).is_err());
        assert!(prepared_recovery_proof(&old_plan.context, &old.operation_id, &old).is_err());
    }

    #[test]
    fn unknown_active_and_unreleased_outcomes_remain_blocked_after_check() {
        let mut service = AppUpdateService::new(None, false, None);
        let mut unknown = failed_record('a');
        unknown.phase = OperationPhase::Unknown;
        assert!(!settled_terminal(&unknown));
        service.latest_journal(Ok(Some(unknown)));
        service.checked(Ok(local_check()), None);
        assert!(service.plan().is_none());

        let mut active = failed_record('b');
        active.phase = OperationPhase::Installing;
        active.execution_fence = ExecutionFence::ManagerIntent;
        assert!(!settled_terminal(&active));
        service.latest_journal(Ok(Some(active)));
        service.checked(Ok(local_check()), None);
        assert!(service.plan().is_none());

        let failed_with_retained_marker = failed_record('c');
        service.latest_journal(Ok(Some(failed_with_retained_marker.clone())));
        // A failed proof (retained profile/global marker, busy lease or legacy bytes)
        // does not supply a settlement token, even though the journal is terminal.
        service.checked(Ok(local_check()), None);
        assert!(service.plan().is_none());
        let mut intent = failed_with_retained_marker;
        intent.execution_fence = ExecutionFence::ManagerIntent;
        assert!(!settled_terminal(&intent));
    }

    #[test]
    fn late_settlement_cannot_retire_a_newer_operation() {
        let mut service = AppUpdateService::new(None, false, None);
        let old = failed_record('a');
        service.latest_journal(Ok(Some(old.clone())));
        let mut current = failed_record('b');
        current.phase = OperationPhase::Installing;
        current.execution_fence = ExecutionFence::ManagerIntent;
        service.latest_journal(Ok(Some(current.clone())));
        service.checked(Ok(local_check()), Some(SettledOperationProof(old)));
        assert_eq!(service.operation.as_ref(), Some(&current));
        assert_eq!(
            service.observed_operation_id.as_deref(),
            Some(current.operation_id.as_str())
        );
        assert!(service.plan().is_none());
    }

    #[test]
    fn failed_check_does_not_advance_successful_check_time() {
        let mut service = AppUpdateService::new(None, false, Some(11));
        assert_eq!(
            service.checked(Err("untranslated /isolated/check-failure".into()), None),
            None
        );
        let ko = service.model(UiLocale::Ko);
        assert_eq!(ko.last_checked, Some(11));
        assert_eq!(ko.status, CheckStatus::Failed);
        assert!(!ko.detail.contains("untranslated"));
        assert!(!ko.status_text.contains("/isolated/"));
        assert!(ko
            .diagnostics
            .contains("untranslated /isolated/check-failure"));
    }

    #[test]
    fn unresolved_active_journal_outweighs_a_successful_read_only_check() {
        let mut service = AppUpdateService::new(None, false, Some(11));
        service.latest_journal(Err("reserved operation has no valid journal".into()));
        service.checked(Ok(local_check()), None);
        let model = service.model(UiLocale::En);
        assert_eq!(model.status, CheckStatus::Failed);
        assert!(model
            .diagnostics
            .contains("reserved operation has no valid journal"));
        assert!(!model.detail.contains("reserved operation"));
        assert!(service.plan().is_none());
    }

    #[test]
    fn installation_during_restart_is_not_reported_as_applied() {
        let mut service = AppUpdateService::new(None, false, None);
        service.latest_journal(Ok(Some(OperationRecord {
            version: UPDATER_PROTOCOL,
            execution_fence: ExecutionFence::ManagerExited,
            operation_id: "a".repeat(32),
            phase: OperationPhase::Starting,
            detail: "new instance not yet verified".into(),
            installed: true,
            applied: false,
            candidate: None,
        })));
        let model = service.model(UiLocale::En);
        assert_eq!(model.status, CheckStatus::Checking);
        assert!(model.busy);
        assert!(model.action.is_none());
        assert!(model.history.is_empty());
        assert!(!model.detail.contains("new instance not yet verified"));
        assert!(model.diagnostics.contains("new instance not yet verified"));
    }

    #[test]
    fn completed_applied_is_a_historical_result_not_latest_or_restart_pending() {
        let mut service = AppUpdateService::new(None, false, None);
        let operation_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let record = OperationRecord {
            operation_id: operation_id.into(),
            version: UPDATER_PROTOCOL,
            execution_fence: ExecutionFence::ManagerExited,
            phase: OperationPhase::Completed,
            detail: "replacement confirmed".into(),
            installed: true,
            applied: true,
            candidate: None,
        };
        service.latest_journal(Ok(Some(record.clone())));
        let revision = service.revision();
        let model = service.model(UiLocale::En);
        assert_eq!(model.status, CheckStatus::NotChecked);
        assert!(!model.busy);
        assert!(model.action.is_none());
        service.journal(operation_id, Ok(Some(record)), None);
        assert_eq!(service.revision(), revision);
    }

    #[test]
    fn completed_history_survives_check_retirement_and_never_replaces_current_discovery() {
        let snapshot = local_check();
        let context = snapshot.plan.as_ref().unwrap().context.clone();
        let mut service = AppUpdateService::new(None, false, None);
        service.context = Some(context);
        let mut completed = failed_record('b');
        completed.phase = OperationPhase::Completed;
        completed.execution_fence = ExecutionFence::ManagerExited;
        completed.installed = true;
        completed.applied = true;
        completed.detail = "private /isolated/operation/result".into();
        completed.candidate = Some(ExecutableIdentity {
            path: PathBuf::from("/isolated/secret/new-app"),
            sha256: "private candidate sha".into(),
        });
        service.latest_journal(Ok(Some(completed.clone())));
        service.check_busy = true;
        let checking = service.model(UiLocale::Ko);
        assert!(checking.busy);
        assert_eq!(checking.status, CheckStatus::Checking);
        assert!(checking.action.is_none());
        let history = checking.history;

        service.checked(Ok(snapshot), Some(SettledOperationProof(completed.clone())));
        let discovered = service.model(UiLocale::Ko);
        assert!(!discovered.busy);
        assert_eq!(discovered.status, CheckStatus::Local);
        assert_eq!(discovered.action, Some(UpdateAction::LocalRebuild));
        assert_eq!(discovered.history, history);
        assert_eq!(
            discovered.running_version,
            service.context.as_ref().unwrap().version
        );
        for summary in [
            &discovered.status_text,
            &discovered.source,
            &discovered.detail,
            &discovered.history,
        ] {
            assert!(!summary.contains("/isolated/"));
            assert!(!summary.contains("private"));
        }
        assert!(discovered
            .diagnostics
            .contains("/isolated/local-source/app"));
        assert!(discovered.diagnostics.contains("/isolated/secret/new-app"));
        assert!(discovered.diagnostics.contains("private candidate sha"));
        assert!(discovered
            .diagnostics
            .contains("private /isolated/operation/result"));
        service.latest_journal(Ok(Some(completed)));
        assert!(service.plan().is_some());
        assert_eq!(service.model(UiLocale::Ko).history, history);

        let mut current = local_check();
        current.status = CheckStatus::Current;
        current.plan = None;
        service.checked(Ok(current), None);
        let current = service.model(UiLocale::Ko);
        assert_eq!(current.status, CheckStatus::Current);
        assert!(!current.busy);
        assert!(current.action.is_none());
        assert_eq!(current.history, history);
    }

    #[test]
    fn homebrew_source_shows_formula_without_exposing_install_paths() {
        let mut snapshot = local_check();
        let origin = InstallOrigin::Homebrew {
            brew: PathBuf::from("/isolated/private/bin/brew"),
            formula: "example/tap/herdr-pet".into(),
            cellar_formula_root: PathBuf::from("/isolated/private/Cellar/herdr-pet"),
            prefix: PathBuf::from("/isolated/private/prefix"),
            locator: PathBuf::from("/isolated/private/app"),
        };
        snapshot.origin = origin.clone();
        snapshot.plan = None;
        let mut service = AppUpdateService::new(None, false, None);
        service.checked(Ok(snapshot), None);
        for locale in [UiLocale::En, UiLocale::Ko] {
            let model = service.model(locale);
            assert_eq!(model.source, "Homebrew · example/tap/herdr-pet");
            assert!(!model.source.contains("/isolated/"));
            assert!(model.diagnostics.contains("/isolated/private/bin/brew"));
            assert!(model.diagnostics.contains("/isolated/private/app"));
        }
        assert!(source_description(&origin).contains("/isolated/private/bin/brew"));
        assert!(source_description(&origin).contains("/isolated/private/app"));
    }

    #[test]
    fn newer_completed_operation_replaces_retained_history() {
        let mut service = AppUpdateService::new(None, false, None);
        let prior = failed_record('a');
        service.latest_journal(Ok(Some(prior.clone())));
        service.checked(Ok(local_check()), Some(SettledOperationProof(prior)));
        let prior_history = service.model(UiLocale::En).history;

        let mut newer = failed_record('b');
        newer.phase = OperationPhase::Starting;
        newer.execution_fence = ExecutionFence::ManagerIntent;
        service.latest_journal(Ok(Some(newer.clone())));
        let active = service.model(UiLocale::En);
        assert_eq!(active.status, CheckStatus::Checking);
        assert!(active.busy);
        assert!(active.action.is_none());
        assert_eq!(active.history, prior_history);

        newer.phase = OperationPhase::Completed;
        newer.execution_fence = ExecutionFence::ManagerExited;
        newer.installed = true;
        newer.applied = true;
        service.latest_journal(Ok(Some(newer.clone())));
        let completed = service.model(UiLocale::En);
        assert_eq!(completed.status, CheckStatus::Local);
        assert!(!completed.busy);
        assert!(completed.action.is_none());
        assert_ne!(completed.history, prior_history);

        service.checked(Ok(local_check()), Some(SettledOperationProof(newer)));
        let retired = service.model(UiLocale::En);
        assert_eq!(retired.status, CheckStatus::Local);
        assert_eq!(retired.action, Some(UpdateAction::LocalRebuild));
        assert_eq!(retired.history, completed.history);
    }

    #[test]
    fn unresolved_outcomes_remain_blocked_without_admission() {
        let mut service = AppUpdateService::new(None, false, None);
        let mut record = failed_record('a');
        record.phase = OperationPhase::Unknown;
        record.detail = "raw /isolated/uncertain".into();
        service.latest_journal(Ok(Some(record.clone())));
        service.checked(Ok(local_check()), None);
        let unknown = service.model(UiLocale::Ko);
        assert_eq!(unknown.status, CheckStatus::Failed);
        assert!(!unknown.busy);
        assert!(unknown.action.is_none());
        assert!(unknown.diagnostics.contains("raw /isolated/uncertain"));
        assert!(!unknown.detail.contains("/isolated/"));

        record.phase = OperationPhase::UserStopped;
        service.latest_journal(Ok(Some(record.clone())));
        service.checked(Ok(local_check()), None);
        let stopped = service.model(UiLocale::Ko);
        assert_eq!(stopped.status, CheckStatus::Failed);
        assert!(stopped.action.is_none());
        assert!(!stopped.detail.contains("/isolated/"));

        record.phase = OperationPhase::Failed;
        service.latest_journal(Ok(Some(record)));
        service.checked(Ok(local_check()), None);
        let failed = service.model(UiLocale::Ko);
        assert_eq!(failed.status, CheckStatus::Failed);
        assert!(failed.action.is_none());
    }

    #[test]
    fn journal_transitions_advance_revision_without_duplicate_paints() {
        let mut service = AppUpdateService::new(None, false, None);
        let operation_id = "cccccccccccccccccccccccccccccccc";
        let mut record = OperationRecord {
            operation_id: operation_id.into(),
            version: UPDATER_PROTOCOL,
            execution_fence: ExecutionFence::ManagerIntent,
            phase: OperationPhase::Installing,
            detail: "working".into(),
            installed: false,
            applied: false,
            candidate: None,
        };
        service.latest_journal(Ok(Some(record.clone())));
        let revision = service.revision();
        service.journal(operation_id, Ok(Some(record.clone())), None);
        assert_eq!(service.revision(), revision);
        record.phase = OperationPhase::Failed;
        record.detail = "apply failed".into();
        service.journal(operation_id, Ok(Some(record)), None);
        assert_ne!(service.revision(), revision);
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Failed);
    }
    #[test]
    fn ownership_ack_keeps_live_operation_busy_and_paints_pre_stop_journal() {
        let mut service = AppUpdateService::new(None, false, None);
        let operation_id = "dddddddddddddddddddddddddddddddd";
        service.launching = Some(operation_id.into());
        service.observed_operation_id = Some(operation_id.into());
        assert!(service.launch_result(operation_id, Ok(())));
        assert!(service.model(UiLocale::En).busy);
        let mut record = OperationRecord {
            version: UPDATER_PROTOCOL,
            execution_fence: ExecutionFence::NoSpawn,
            operation_id: operation_id.into(),
            phase: OperationPhase::Preparing,
            detail: "awaiting original Stop".into(),
            installed: false,
            applied: false,
            candidate: None,
        };
        service.journal(operation_id, Ok(Some(record.clone())), None);
        let card = service.model(UiLocale::En);
        assert!(card.busy);
        assert_eq!(card.status, CheckStatus::Checking);
        assert!(card.diagnostics.contains("awaiting original Stop"));
        assert!(!card.detail.contains("awaiting original Stop"));
        record.phase = OperationPhase::Failed;
        record.detail = "Stop reply failed".into();
        service.journal(operation_id, Ok(Some(record)), None);
        let failed = service.model(UiLocale::En);
        assert!(!failed.busy);
        assert_eq!(failed.status, CheckStatus::Failed);
        assert!(failed.diagnostics.contains("Stop reply failed"));
        assert!(!failed.detail.contains("Stop reply failed"));
    }
    fn pending_local_launch() -> (AppUpdateService, UpdatePlan) {
        let snapshot = local_check();
        let plan = snapshot.plan.as_ref().unwrap().clone();
        let mut service = AppUpdateService::new(None, false, None);
        service.context = Some(plan.context.clone());
        service.checked(Ok(snapshot), None);
        assert!(service.plan().is_some());
        service.launching = Some(plan.operation_id.clone());
        service.observed_operation_id = Some(plan.operation_id.clone());
        service.launch_plan = Some(plan.clone());
        (service, plan)
    }

    #[test]
    fn failed_before_spawn_retires_only_its_missing_journal_and_fresh_check_admits_apply() {
        let (mut service, plan) = pending_local_launch();
        service.journal(&plan.operation_id, Ok(None), None);
        assert!(service.plan().is_none());
        assert!(!service.launch_result(
            &plan.operation_id,
            Err(LaunchFailure::NoSpawn("staging failed".into())),
        ));
        assert!(service.plan().is_none()); // Error remains visible until a fresh check.
        service.checked(Ok(local_check()), None);
        assert_eq!(service.plan(), Some(&plan));
        assert_eq!(
            service.model(UiLocale::En).action,
            Some(UpdateAction::LocalRebuild)
        );
        service.journal(&plan.operation_id, Err("late poll".into()), None);
        assert_eq!(service.plan(), Some(&plan));
    }

    #[test]
    fn failed_after_spawn_keeps_uncertain_ownership_blocked_after_fresh_check() {
        let (mut service, plan) = pending_local_launch();
        service.journal(&plan.operation_id, Ok(None), None);
        assert!(!service.launch_result(
            &plan.operation_id,
            Err(LaunchFailure::Spawned("ownership reply lost".into())),
        ));
        service.checked(Ok(local_check()), None);
        assert!(service.plan().is_none());
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert_eq!(
            service.observed_operation_id.as_deref(),
            Some(plan.operation_id.as_str())
        );
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Failed);
    }

    #[test]
    fn spawned_without_any_journal_still_cannot_apply_after_successful_check() {
        let (mut service, plan) = pending_local_launch();
        assert!(!service.launch_result(
            &plan.operation_id,
            Err(LaunchFailure::Spawned("no ownership confirmation".into())),
        ));
        service.checked(Ok(local_check()), None);
        assert!(service.plan().is_none());
        assert_eq!(service.model(UiLocale::En).action, None);
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
    }

    #[test]
    fn no_spawn_does_not_hide_an_independent_unverified_journal() {
        let (mut service, plan) = pending_local_launch();
        service.latest_journal(Err("other operation cannot be verified".into()));
        assert!(!service.launch_result(
            &plan.operation_id,
            Err(LaunchFailure::NoSpawn("staging failed".into())),
        ));
        service.checked(Ok(local_check()), None);
        assert!(service.plan().is_none());
        let model = service.model(UiLocale::En);
        assert_eq!(model.status, CheckStatus::Failed);
        assert!(model
            .diagnostics
            .contains("other operation cannot be verified"));
        assert!(!model.detail.contains("other operation cannot be verified"));
    }

    #[test]
    fn ack_then_unknown_then_exact_recovery_keeps_original_plan_until_verified() {
        let (mut service, plan) = pending_local_launch();
        assert!(service.launch_result(&plan.operation_id, Ok(())));
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        let mut unknown = failed_record('e');
        unknown.phase = OperationPhase::Unknown;
        assert_eq!(unknown.operation_id, plan.operation_id);
        assert_eq!(
            service.journal(&plan.operation_id, Ok(Some(unknown)), None),
            None
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert!(service.plan().is_none());

        let recovered = failed_record('e');
        let mut wrong_plan = plan.clone();
        wrong_plan.context.instance_id = "different original".into();
        assert_eq!(
            service.journal(
                &plan.operation_id,
                Ok(Some(recovered.clone())),
                Some(PreparedRecoveryProof {
                    record: recovered.clone(),
                    plan: wrong_plan,
                }),
            ),
            None
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert!(service.plan().is_none());
        assert_eq!(
            service.journal(
                &plan.operation_id,
                Ok(Some(recovered.clone())),
                Some(PreparedRecoveryProof {
                    record: recovered,
                    plan: plan.clone(),
                }),
            ),
            Some(plan.context.clone())
        );
        assert!(service.prepared_plan.is_none());
    }
}
