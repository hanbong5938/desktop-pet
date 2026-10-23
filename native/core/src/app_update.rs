//! App-side, read-only update discovery and explicit helper handoff.
//! AppKit never waits for a manager, hash, source probe, or helper stdout.
//! Relocation follows the helper's admitted operation, never a manual retry's older one-shot offer.
//! The successor bridge continues the authenticated original update approval.
//! No independent migration consent is required after that bounded approval.
//! Completed history admits policy changes after scoped settlement, without a network check.
//! Exact completed-history clearance supersedes older asynchronous recovery observations.
//! A fresh settled check is still required before retiring history for new update work.
//! Old ledgers remain owned by their exact retained helper. Explicit original
//! Stop consent runs only in today's authenticated bundled coordinator, after
//! source-side scoped admission; durable launch intent forbids uncertain replay.
use crate::i18n::{text, Message, UiLocale};
use crate::update_card::{UpdateCardModel, UpdateCardPrimaryAction};
use desktop_pet_update_coordinator::protocol::{
    historical_operation_plan, lock_operation, observe_latest_operation, observe_operation,
    private_updates, start_allowed, ExecutionFence, OperationObservation, OperationPhase,
    OperationRecord, UPDATER_PROTOCOL,
};
use desktop_pet_update_coordinator::standalone_install::{
    self, InstallStatus, StandaloneInstallRecord,
};
use desktop_pet_update_coordinator::standalone_relocation::{
    self, RelocationAdmission, SettledRelocationParent,
};
use desktop_pet_update_coordinator::{
    check, read_operation, read_plan, recheck_plan, stage_helper, write_plan, CheckSnapshot,
    CheckStatus, InstallOrigin, UpdateContext, UpdatePlan, UpdatePolicy,
};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RelocationTernObservation {
    captured: bool,
    windows: usize,
    pending: bool,
    detail: Option<String>,
}

const CHECK_INTERVAL: u64 = 24 * 60 * 60;

pub(super) enum Event {
    RelocationAdmission {
        context: UpdateContext,
        result: Result<RelocationAdmission, String>,
    },
    RelocationFinished {
        context: UpdateContext,
        operation_id: String,
        result: Result<(), String>,
    },
    RelocationTernObserved {
        context: UpdateContext,
        operation_id: String,
        result: Result<RelocationTernObservation, String>,
    },
    Checked {
        request_id: u64,
        policy_generation: u64,
        result: Result<CheckSnapshot, String>,
        settled: Option<SettledOperationProof>,
    },
    LaunchWaiting {
        operation_id: String,
    },
    Launched {
        operation_id: String,
        result: Result<(), LaunchFailure>,
    },
    Journal {
        operation_id: String,
        result: Result<Option<OperationObservation>, String>,
        prepared_recovery: Option<PreparedRecoveryProof>,
        completed_history: Option<CompletedHistoryProof>,
    },
    LatestJournal(Result<Option<OperationObservation>, String>),
    MigrationEligibility {
        context: UpdateContext,
        result: Result<(), String>,
    },
    MigrationHistory {
        context: UpdateContext,
        result: Result<
            Option<(
                desktop_pet_update_coordinator::migration::MigrationPlan,
                desktop_pet_update_coordinator::migration::MigrationRecord,
            )>,
            String,
        >,
    },
    AutomaticMigrationStarted {
        context: UpdateContext,
    },
    AutomaticMigration {
        context: UpdateContext,
        result: Result<Option<desktop_pet_update_coordinator::migration::MigrationPlan>, String>,
    },
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
pub(super) enum SettledOperationProof {
    Update(OperationRecord),
    StandaloneInstall(StandaloneInstallRecord),
}

// Policy admission is independent of network discovery and does not retire a
// journal or authorize another update. Only a worker may prove exact completed
// history has no helper lease or installation/profile reservation remaining.
// Installed-only history additionally requires the coordinator to authenticate
// this later ordinary fenced Start; it never changes the journal's applied bit.
pub(super) struct CompletedHistoryProof {
    context: UpdateContext,
    observation: OperationObservation,
}
// Constructed only by the journal worker while holding the finished helper's
// operation lease. Unlike fresh-check retirement, this must match the original
// prepared instance and its immutable plan, not a replacement check's context.
pub(super) struct PreparedRecoveryProof {
    record: OperationRecord,
    plan: UpdatePlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecoveryDisposition {
    Recover,
    RecoverStoppedOriginal,
    Start,
    Live(u32),
    Reconciled,
}

#[derive(Clone)]
struct RecoveryProjection {
    context: UpdateContext,
    record: OperationRecord,
    plan: UpdatePlan,
    helper: Option<desktop_pet_update_coordinator::ExecutableIdentity>,
    coordinator: Option<desktop_pet_update_coordinator::ExecutableIdentity>,
    live_helper: Option<desktop_pet_update_coordinator::ExecutableIdentity>,
    native_ui: bool,
    disposition: RecoveryDisposition,
}

enum RecoveryEvent {
    Observed {
        context: UpdateContext,
        operation_id: String,
        request: u64,
        result: Result<RecoveryProjection, String>,
    },
    Started {
        context: UpdateContext,
        operation_id: String,
        request: u64,
        pid: u32,
    },
    Finished {
        context: UpdateContext,
        operation_id: String,
        request: u64,
        result: Result<(), String>,
    },
    HandoffReleased {
        context: UpdateContext,
        operation_id: String,
        request: u64,
    },
    Focused {
        context: UpdateContext,
        operation_id: String,
        request: u64,
        record: OperationRecord,
        result: Result<(), String>,
    },
}

// This is the helper's immutable original/helper binding, not an authorization
// inferred from a diagnostic string or from today's installation locator.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryLifecycle {
    original_pid: u32,
    original_instance_id: String,
    original_image: desktop_pet_update_coordinator::ExecutableIdentity,
    original_config_dir: PathBuf,
    original_state_dir: PathBuf,
    original_socket: PathBuf,
    original_assets: Option<PathBuf>,
    helper_pid: u32,
    helper_image: desktop_pet_update_coordinator::ExecutableIdentity,
    stop_generation: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryLaunchReceipt {
    version: u32,
    operation_id: String,
    context: UpdateContext,
    process: RecoveryLaunchProcess,
    retained_helper_image: desktop_pet_update_coordinator::ExecutableIdentity,
    coordinator: bool,
    helper_image: desktop_pet_update_coordinator::ExecutableIdentity,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum RecoveryLaunchProcess {
    Launching,
    Spawned { pid: u32 },
    NoSpawn,
}

pub(super) struct AppUpdateService {
    context: Option<UpdateContext>,
    snapshot: Option<CheckSnapshot>,
    check_busy: bool,
    launching: Option<String>,
    launch_plan: Option<UpdatePlan>,
    launch_waiting: bool,
    prepared_plan: Option<UpdatePlan>,
    cancelled_preparation: Option<String>,
    operation: Option<OperationRecord>,
    previous_outcome: Option<OperationRecord>,
    standalone_operation: Option<StandaloneInstallRecord>,
    previous_standalone_outcome: Option<StandaloneInstallRecord>,
    observed_operation_id: Option<String>,
    retired_operation_id: Option<String>,
    next_journal_poll: Instant,
    journal_in_flight: Option<String>,
    problem: Option<String>,
    journal_error: Option<String>,
    journal_error_operation_id: Option<String>,
    sender: Sender<Event>,
    receiver: Receiver<Event>,
    last_checked: Option<u64>,
    auto_check: bool,
    policy: UpdatePolicy,
    policy_generation: u64,
    candidate_generation: u64,
    next_request_id: u64,
    pending_request: Option<u64>,
    launch_generation: Option<(u64, u64)>,
    next_auto_due: Option<u64>,
    scheduled_due_before: Option<Option<u64>>,
    auto_failure_paused: bool,
    revision: u64,
    migration_available: bool,
    migration_problem: Option<String>,
    migration_history_verified: bool,
    migration_blocked: bool,
    relocation_parent: Option<SettledRelocationParent>,
    relocation_busy: bool,
    relocation_polled: bool,
    next_relocation_parent_poll: Instant,
    relocation_auto_attempted: bool,
    relocation_waiting: bool,
    relocation_tern: Option<(String, RelocationTernObservation)>,
    relocation_tern_in_flight: bool,
    next_relocation_tern_poll: Instant,
    recovery: Option<RecoveryProjection>,
    recovery_error: Option<String>,
    recovery_observing: Option<(UpdateContext, String, u64, OperationRecord)>,
    recovery_request: u64,
    recovery_running: Option<(UpdateContext, String, u64, Option<u32>)>,
    recovery_handoff: Option<(UpdateContext, String, u64, bool)>,
    recovery_journal_authority: Option<(UpdateContext, String)>,
    completed_history: Option<CompletedHistoryProof>,
    recovery_sender: Sender<RecoveryEvent>,
    recovery_receiver: Receiver<RecoveryEvent>,
}

impl AppUpdateService {
    pub(super) fn new(
        context: Option<UpdateContext>,
        auto_check: bool,
        policy: UpdatePolicy,
        last_checked: Option<u64>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let (recovery_sender, recovery_receiver) = mpsc::channel();
        if let Some(context) = context.as_ref() {
            let sender = sender.clone();
            let state_dir = context.state_dir.clone();
            std::thread::spawn(move || {
                let _ = sender.send(Event::LatestJournal(observe_latest_operation(&state_dir)));
            });
        }
        let mut service = Self {
            context,
            snapshot: None,
            check_busy: false,
            launching: None,
            launch_plan: None,
            launch_waiting: false,
            prepared_plan: None,
            cancelled_preparation: None,
            previous_outcome: None,
            operation: None,
            standalone_operation: None,
            previous_standalone_outcome: None,
            observed_operation_id: None,
            retired_operation_id: None,
            next_journal_poll: Instant::now(),
            journal_in_flight: None,
            problem: None,
            journal_error: None,
            journal_error_operation_id: None,
            sender,
            receiver,
            last_checked,
            auto_check,
            policy,
            policy_generation: 0,
            candidate_generation: 0,
            next_request_id: 0,
            pending_request: None,
            launch_generation: None,
            next_auto_due: None,
            scheduled_due_before: None,
            auto_failure_paused: false,
            revision: 0,
            migration_available: false,
            migration_problem: None,
            migration_history_verified: false,
            migration_blocked: false,
            relocation_parent: None,
            relocation_busy: false,
            relocation_polled: false,
            next_relocation_parent_poll: Instant::now(),
            relocation_auto_attempted: false,
            relocation_waiting: false,
            relocation_tern: None,
            relocation_tern_in_flight: false,
            next_relocation_tern_poll: Instant::now(),
            recovery: None,
            recovery_error: None,
            recovery_observing: None,
            recovery_request: 0,
            recovery_running: None,
            recovery_handoff: None,
            recovery_journal_authority: None,
            completed_history: None,
            recovery_sender,
            recovery_receiver,
        };
        service.check_migration_eligibility();
        service.check_migration_history();
        service
    }
    pub(super) fn selected_executable(&self) -> Option<(&Path, bool)> {
        let context = self.context.as_ref()?;
        let candidate = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.plan.as_ref())
            .and_then(|plan| plan.candidate.as_ref());
        match candidate {
            Some(candidate) => Some((&candidate.path, candidate == &context.running)),
            None => Some((
                &context.executable,
                context.executable == context.running.path,
            )),
        }
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    fn at_relocation_home(&self) -> bool {
        self.context.as_ref().is_some_and(|context| {
            !desktop_pet_update_coordinator::identity::LEGACY
                && desktop_pet_update_coordinator::installation::os_home()
                    .ok()
                    .is_some_and(|home| {
                        context.running_origin.as_deref()
                            == Some(&InstallOrigin::Standalone {
                                bundle: home.join("Applications/DesktopPet.app"),
                            })
                    })
        })
    }

    /// Called only by the UI-ready default run loop, never a tracking/modal loop.
    pub(super) fn poll_relocation(&mut self) {
        self.poll_relocation_tern();
        if !self.at_relocation_home() || self.relocation_busy {
            return;
        }
        if self.journal_error.is_some()
            || self
                .operation
                .as_ref()
                .is_some_and(|record| !terminal_phase(record.phase))
            || self
                .standalone_operation
                .as_ref()
                .is_some_and(|record| !record.result.terminal())
        {
            return;
        }
        if self.relocation_parent.is_some() && !self.relocation_auto_attempted {
            self.relocation_auto_attempted = true;
            self.launch_relocation(false);
            return;
        }
        if self.relocation_polled || Instant::now() < self.next_relocation_parent_poll {
            return;
        }
        let Some(context) = self.context.clone() else {
            return;
        };
        self.relocation_polled = true;
        self.next_relocation_parent_poll = Instant::now() + Duration::from_secs(1);
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = standalone_relocation::resume_relocation_parent(&context);
            let _ = sender.send(Event::RelocationAdmission { context, result });
        });
    }

    pub(super) fn relocation_admission(
        &mut self,
        context: UpdateContext,
        result: Result<RelocationAdmission, String>,
    ) {
        if self.context.as_ref() != Some(&context) {
            return;
        }
        match result {
            Ok(RelocationAdmission::Eligible(parent)) => {
                self.relocation_parent = Some(parent);
                self.relocation_waiting = false;
            }
            Ok(RelocationAdmission::Waiting) => {
                self.relocation_waiting = true;
                self.relocation_polled = false;
            }
            Ok(RelocationAdmission::Superseded) => {
                self.relocation_waiting = false;
                self.relocation_auto_attempted = true;
            }
            Err(error) => {
                self.problem = Some(error);
                self.relocation_auto_attempted = true;
            }
        }
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn begin_standalone_relocation(&mut self) {
        self.launch_relocation(true);
    }

    fn launch_relocation(&mut self, manual: bool) {
        if !self.at_relocation_home()
            || self.relocation_busy
            || self.launching.is_some()
            || self.journal_error.is_some()
        {
            return;
        }
        let Some(context) = self.context.clone() else {
            return;
        };
        let parent = self.relocation_parent.clone();
        self.relocation_busy = true;
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let mut operation_id = String::new();
            let result = (|| {
                let parent = match parent {
                    Some(parent) => parent,
                    None if manual => standalone_relocation::discover_relocation_parent(&context)?
                        .ok_or(
                            "No authenticated completed home update is available for relocation.",
                        )?,
                    None => return Err("No automatic relocation parent is available.".into()),
                };
                let replay = !manual
                    && standalone_install::read_relocation_offer(
                        &context.state_dir,
                        &parent.operation_id,
                    )?
                    .is_some();
                if !replay {
                    standalone_relocation::validate_settled_parent(&context, &parent)?;
                }
                // A terminal result alone is not release evidence. Admission Settled
                // is written by the helper only after checking exact scope release.
                for id in if manual {
                    standalone_install::relocation_attempts(
                        &context.state_dir,
                        &parent.operation_id,
                    )?
                } else {
                    Vec::new()
                } {
                    let admission = standalone_install::read_admission(&context.state_dir, &id)?
                        .ok_or("relocation attempt has no admission")?;
                    if admission.phase != standalone_install::StandaloneAdmissionPhase::Settled {
                        operation_id = id.clone();
                        return Err(format!("Relocation {id} requires observation or recovery before another attempt."));
                    }
                }
                let request = standalone_relocation::RelocationRequest {
                    version: 1,
                    context: context.clone(),
                    parent,
                };
                let updates = private_updates(&context.state_dir)?;
                let nonce = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| e.to_string())?
                    .as_nanos();
                let path = updates.join(format!(
                    "relocation-request-{}-{nonce}.json",
                    std::process::id()
                ));
                desktop_pet_update_coordinator::protocol::write_private_atomic_new(
                    &path,
                    &serde_json::to_vec(&request).map_err(|e| e.to_string())?,
                    0o600,
                )?;
                let helper = context
                    .executable
                    .parent()
                    .ok_or("missing app executable directory")?
                    .join("desktop-pet-update-coordinator");
                let previous_attempts = if manual {
                    standalone_install::relocation_attempts(
                        &context.state_dir,
                        &request.parent.operation_id,
                    )?
                } else {
                    Vec::new()
                };
                let output = Command::new(helper)
                    .arg(if manual {
                        "install-relocate-manual"
                    } else {
                        "install-relocate"
                    })
                    .arg("--request")
                    .arg(path)
                    .env_clear()
                    .env(
                        "HOME",
                        desktop_pet_update_coordinator::installation::os_home()?,
                    )
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .spawn()
                    .map_err(|e| e.to_string())?;
                let (status, stdout) = read_relocation_helper(output)?;
                operation_id =
                    authenticate_relocation_outcome(&request, &previous_attempts, &stdout)?;
                if status.code()
                    == Some(
                        serde_json::from_slice::<standalone_install::StandaloneInstallResult>(
                            &stdout,
                        )
                        .map_err(|e| e.to_string())?
                        .exit_code(),
                    )
                {
                    Ok(())
                } else {
                    Err(format!("Relocation helper exited with {status}; observing its authenticated operation."))
                }
            })();
            let _ = sender.send(Event::RelocationFinished {
                context,
                operation_id,
                result,
            });
        });
    }

    pub(super) fn relocation_finished(
        &mut self,
        context: UpdateContext,
        operation_id: String,
        result: Result<(), String>,
    ) {
        if self.context.as_ref() != Some(&context) {
            return;
        }
        self.relocation_busy = false;
        if let Err(error) = result {
            self.problem = Some(error);
        }
        if !operation_id.is_empty()
            && self.launching.is_none()
            && self.launch_plan.is_none()
            && self.prepared_plan.is_none()
            && self.journal_error.is_none()
            && self
                .operation
                .as_ref()
                .is_none_or(|record| settled_terminal(record))
            && self.standalone_operation.as_ref().is_none_or(|record| {
                record.operation_id == operation_id
                    || matches!(
                        record.result.status,
                        InstallStatus::Cancelled
                            | InstallStatus::Failed
                            | InstallStatus::Applied
                            | InstallStatus::InstalledStopped
                    )
            })
        {
            if let Some(parent) = self.operation.take() {
                self.previous_outcome = Some(parent);
            }
            if self
                .standalone_operation
                .as_ref()
                .is_some_and(|record| record.operation_id != operation_id)
            {
                self.previous_standalone_outcome = self.standalone_operation.take();
            }
            self.observed_operation_id = Some(operation_id.clone());
            self.next_journal_poll = Instant::now();
            self.reload_journal(operation_id);
        }
        self.revision = self.revision.wrapping_add(1);
    }
    fn poll_relocation_tern(&mut self) {
        if self.relocation_tern_in_flight || Instant::now() < self.next_relocation_tern_poll {
            return;
        }
        let Some(record) = self
            .standalone_operation
            .as_ref()
            .or(self.previous_standalone_outcome.as_ref())
            .filter(|record| record.result.installed)
        else {
            return;
        };
        let Some(context) = self.context.clone() else {
            return;
        };
        let operation_id = record.operation_id.clone();
        let applied = record.result.applied;
        self.relocation_tern_in_flight = true;
        self.next_relocation_tern_poll = Instant::now() + Duration::from_secs(5);
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let plan = standalone_install::read_plan(&context.state_dir, &operation_id)?;
                let tern = match &plan.purpose {
                    standalone_install::StandaloneInstallPurpose::PrivateRelocation {
                        tern,
                        ..
                    } if applied => tern,
                    standalone_install::StandaloneInstallPurpose::ManualRecovery {
                        tern, ..
                    } => tern,
                    _ => {
                        return Ok(RelocationTernObservation {
                            captured: false,
                            windows: 0,
                            pending: false,
                            detail: None,
                        });
                    }
                };
                if tern.is_none() {
                    return Ok(RelocationTernObservation {
                        captured: false,
                        windows: 0,
                        pending: false,
                        detail: None,
                    });
                }
                let (statuses, status_error) =
                    match desktop_pet_update_coordinator::migration::observe_relocation_tern(
                        &context,
                        &operation_id,
                    ) {
                        Ok(statuses) => (statuses, None),
                        Err(error) => (Vec::new(), Some(error)),
                    };
                let count =
                    desktop_pet_update_coordinator::migration::observe_relocation_tern_windows(
                        &context,
                        &operation_id,
                    );
                let (windows, count_error) = match count {
                    Ok(Some(count)) => (count, None),
                    Ok(None) => (
                        0,
                        Some("Captured Tern configuration has no window observation.".to_owned()),
                    ),
                    Err(error) => (0, Some(error)),
                };
                let detail = count_error
                    .or(status_error)
                    .or_else(|| statuses.iter().find_map(|status| status.error.clone()));
                let pending = windows == 0
                    || statuses.iter().any(|status| !status.connected)
                    || detail.is_some();
                Ok(RelocationTernObservation {
                    captured: true,
                    windows,
                    pending,
                    detail,
                })
            })();
            let _ = sender.send(Event::RelocationTernObserved {
                context,
                operation_id,
                result,
            });
        });
    }

    pub(super) fn relocation_tern_observed(
        &mut self,
        context: UpdateContext,
        operation_id: String,
        result: Result<RelocationTernObservation, String>,
    ) {
        if self.context.as_ref() != Some(&context) {
            return;
        }
        self.relocation_tern_in_flight = false;
        let current = self
            .standalone_operation
            .as_ref()
            .or(self.previous_standalone_outcome.as_ref());
        if current
            .is_none_or(|record| record.operation_id != operation_id || !record.result.installed)
        {
            return;
        }
        let observation = result.unwrap_or_else(|error| RelocationTernObservation {
            captured: false,
            windows: 0,
            pending: true,
            detail: Some(error),
        });
        if self
            .relocation_tern
            .as_ref()
            .is_none_or(|(current_id, current)| {
                current_id != &operation_id || current != &observation
            })
        {
            self.relocation_tern = Some((operation_id, observation));
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub(super) fn policy(&self) -> UpdatePolicy {
        self.policy
    }

    pub(super) fn check_migration_eligibility(&mut self) {
        self.migration_available = false;
        self.migration_problem = None;
        if !desktop_pet_update_coordinator::identity::LEGACY {
            return;
        }
        let Some(context) = self.context.clone() else {
            return;
        };
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = desktop_pet_update_coordinator::migration::readonly_eligibility(&context);
            let _ = sender.send(Event::MigrationEligibility { context, result });
        });
    }

    pub(super) fn migration_eligibility(
        &mut self,
        context: UpdateContext,
        result: Result<(), String>,
    ) {
        if self.context.as_ref() != Some(&context) {
            return;
        }
        self.migration_available = result.is_ok();
        self.migration_problem = result.err();
        self.revision = self.revision.wrapping_add(1);
    }

    fn check_migration_history(&mut self) {
        self.migration_history_verified = false;
        let Some(context) = self.context.clone() else {
            return;
        };
        self.migration_blocked = true;
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            // Ui::new has not published readiness yet. Retain the operation
            // without delivery; the existing per-plan observer runs afterward.
            let result = desktop_pet_update_coordinator::migration::discover_for_context(&context);
            let continue_update =
                desktop_pet_update_coordinator::identity::LEGACY && matches!(&result, Ok(None));
            let _ = sender.send(Event::MigrationHistory {
                context: context.clone(),
                result,
            });
            if continue_update {
                let _ = sender.send(Event::AutomaticMigrationStarted {
                    context: context.clone(),
                });
                let result =
                    desktop_pet_update_coordinator::migration::prepare_automatic_bridge_plan(
                        &context,
                        desktop_pet_update_coordinator::identity::herdr_migration_revision(),
                    );
                let _ = sender.send(Event::AutomaticMigration { context, result });
            }
        });
    }

    pub(super) fn migration_history_checked(&mut self, context: &UpdateContext, verified: bool) {
        if self.context.as_ref() != Some(context) {
            return;
        }
        self.migration_history_verified = verified;
        self.migration_blocked = !verified;
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn migration_preparing(&mut self) {
        if !self.migration_blocked {
            self.migration_blocked = true;
            self.pending_request = None;
            self.check_busy = false;
            self.snapshot = None;
            self.candidate_generation = self.candidate_generation.wrapping_add(1);
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub(super) fn migration_observed(
        &mut self,
        record: &desktop_pet_update_coordinator::migration::MigrationRecord,
    ) {
        use desktop_pet_update_coordinator::migration::MigrationPhase;
        // Historical observations may expose Complete only through their separate
        // authenticated settlement receipt. No old journal is changed here.
        if record.phase != MigrationPhase::Complete {
            self.migration_preparing();
        } else if self.migration_blocked {
            self.migration_blocked = false;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub(super) fn settings_admitted(&self) -> bool {
        !self.migration_blocked
            && !self.relocation_busy
            && !self.check_busy
            && self.recovery_running.is_none()
            && !self
                .recovery_handoff
                .as_ref()
                .is_some_and(|(_, _, _, released)| !released)
            && !self.current_recovery().is_some_and(|projection| {
                matches!(projection.disposition, RecoveryDisposition::Live(_))
            })
            && self.launching.is_none()
            && self.launch_plan.is_none()
            && self.prepared_plan.is_none()
            && (self.observed_operation_id.is_none() || self.completed_history_admitted())
            && self.journal_error.is_none()
            && self
                .operation
                .as_ref()
                .is_none_or(|record| terminal_phase(record.phase))
    }

    fn completed_history_admitted(&self) -> bool {
        self.completed_history.as_ref().is_some_and(|proof| {
            self.context.as_ref() == Some(&proof.context)
                && self.observed_operation_id.as_deref() == Some(observation_id(&proof.observation))
                && match &proof.observation {
                    OperationObservation::Update(record) => self.operation.as_ref() == Some(record),
                    OperationObservation::StandaloneInstall(record) => {
                        self.standalone_operation.as_ref() == Some(record)
                    }
                }
        })
    }

    pub(super) fn reconcile_context(&mut self, context: Option<UpdateContext>) {
        if self.context == context {
            return;
        }
        self.context = context;
        self.recovery = None;
        self.recovery_error = None;
        self.recovery_observing = None;
        self.recovery_running = None;
        self.recovery_request = self.recovery_request.wrapping_add(1);
        self.recovery_journal_authority = None;
        self.completed_history = None;
        self.relocation_parent = None;
        self.relocation_busy = false;
        self.relocation_polled = false;
        self.next_relocation_parent_poll = Instant::now();
        self.relocation_auto_attempted = false;
        self.relocation_waiting = false;
        self.relocation_tern = None;
        self.relocation_tern_in_flight = false;
        self.next_relocation_tern_poll = Instant::now();
        self.check_migration_eligibility();
        self.check_migration_history();
        // Retire discovery and consent, not the immutable plan or journal owned
        // by an already launched helper. Native state gates host changes while
        // an actual preparation is active.
        self.pending_request = None;
        self.check_busy = false;
        self.snapshot = None;
        self.candidate_generation = self.candidate_generation.wrapping_add(1);
        self.problem = None;
        self.next_auto_due = None;
        self.scheduled_due_before = None;
        self.auto_failure_paused = false;
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn change_policy(&mut self, policy: UpdatePolicy, checked_at: Option<u64>) {
        if !self.settings_admitted() || self.policy == policy {
            return;
        }
        self.policy = policy;
        self.policy_generation = self.policy_generation.wrapping_add(1);
        self.pending_request = None;
        self.check_busy = false;
        self.snapshot = None;
        self.problem = None;
        self.last_checked = checked_at;
        self.next_auto_due = None;
        self.scheduled_due_before = None;
        self.auto_failure_paused = false;
        self.revision = self.revision.wrapping_add(1);
        if self.auto_check {
            if let Some(time) = now() {
                self.start_auto_check(time);
            }
        }
    }

    pub(super) fn accepts_checked(&self, request_id: u64, policy_generation: u64) -> bool {
        self.pending_request == Some(request_id) && self.policy_generation == policy_generation
    }
    pub(super) fn consent_generation(&self) -> (u64, u64) {
        (self.policy_generation, self.candidate_generation)
    }

    pub(super) fn consent_matches(&self, plan: &UpdatePlan, generation: (u64, u64)) -> bool {
        self.consent_generation() == generation && self.plan() == Some(plan)
    }
    pub(super) fn download_url(&self) -> Option<String> {
        if self.check_busy
            || self.launching.is_some()
            || self.prepared_plan.is_some()
            || self.problem.is_some()
            || self.journal_error.is_some()
            || self.observed_operation_id.is_some()
        {
            return None;
        }
        let snapshot = self.snapshot.as_ref()?;
        if snapshot.policy != self.policy
            || !matches!(
                snapshot.status,
                CheckStatus::Local | CheckStatus::Unsupported
            )
        {
            return None;
        }
        let release = snapshot.release.as_ref()?;
        release.validate().ok()?;
        Some(format!(
            "https://github.com/{}/releases/tag/{}",
            desktop_pet_update_coordinator::identity::REPOSITORY,
            release.tag
        ))
    }

    pub(super) fn checked_event(
        &mut self,
        request_id: u64,
        policy_generation: u64,
        result: Result<CheckSnapshot, String>,
        settled: Option<SettledOperationProof>,
    ) -> Option<u64> {
        if !self.accepts_checked(request_id, policy_generation) {
            return None;
        }
        self.pending_request = None;
        self.scheduled_due_before = None;
        self.checked(result, settled)
    }

    pub(super) fn checked_at_for_save(
        &self,
        request_id: u64,
        policy_generation: u64,
        result: &Result<CheckSnapshot, String>,
    ) -> Option<u64> {
        if !self.accepts_checked(request_id, policy_generation) {
            return None;
        }
        match result {
            Ok(snapshot)
                if snapshot.policy == self.policy && snapshot.status != CheckStatus::Failed =>
            {
                snapshot.checked_at.or_else(now)
            }
            _ => None,
        }
    }

    pub(super) fn mark_check_save_failure(&mut self, error: String) {
        if let Some(previous_due) = self.scheduled_due_before.take() {
            self.next_auto_due = previous_due;
        }
        self.auto_failure_paused = true;
        self.pending_request = None;
        self.check_busy = false;
        self.problem = Some(format!(
            "Update was checked, but the check time could not be saved: {error}"
        ));
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn set_auto_check(&mut self, enabled: bool) {
        if !self.settings_admitted() || self.auto_check == enabled {
            return;
        }
        self.auto_check = enabled;
        self.revision = self.revision.wrapping_add(1);
        if enabled {
            self.policy_generation = self.policy_generation.wrapping_add(1);
            self.pending_request = None;
            self.snapshot = None;
            self.problem = None;
            self.next_auto_due = None;
            self.scheduled_due_before = None;
            self.auto_failure_paused = false;
            if let Some(time) = now() {
                self.start_auto_check(time);
            }
        }
    }

    pub(super) fn check_due(&mut self) {
        let Some(time) = now() else { return };
        if self.auto_check
            && !self.auto_failure_paused
            && self.next_auto_due.is_none_or(|due| time >= due)
            && self
                .last_checked
                .is_none_or(|last| time.saturating_sub(last) >= CHECK_INTERVAL)
        {
            self.start_auto_check(time);
        }
    }

    fn start_auto_check(&mut self, time: u64) {
        let previous_due = self.next_auto_due;
        let previous_request = self.pending_request;
        self.check_now();
        if self.pending_request != previous_request {
            self.scheduled_due_before = Some(previous_due);
            self.next_auto_due = Some(time.saturating_add(CHECK_INTERVAL));
        }
    }

    /// Check admission is independent of policy-setting admission.
    pub(super) fn check_admitted(&self) -> bool {
        !(self.migration_blocked
            || self.relocation_busy
            || self.check_busy
            || self.recovery_unresolved()
            || self.launching.is_some()
            || (self.launch_plan.is_some()
                && !self
                    .operation
                    .as_ref()
                    .is_some_and(|record| terminal_phase(record.phase)))
            || (self.prepared_plan.is_some()
                && !self.operation.as_ref().is_some_and(|record| {
                    terminal_phase(record.phase)
                        && self.cancelled_preparation.as_deref()
                            == Some(record.operation_id.as_str())
                }))
            || self
                .operation
                .as_ref()
                .is_some_and(|record| !terminal_phase(record.phase))
            || (self.observed_operation_id.is_some()
                && !self
                    .operation
                    .as_ref()
                    .is_some_and(|record| terminal_phase(record.phase))
                && !self
                    .standalone_operation
                    .as_ref()
                    .is_some_and(|record| record.result.terminal()))
            || self.journal_error.is_some())
    }

    fn begin_check(&mut self) -> Option<(u64, u64, UpdateContext)> {
        if !self.check_admitted() {
            return None;
        }
        let Some(context) = self.context.clone() else {
            let problem = "Running app update context is unavailable";
            if self.problem.as_deref() != Some(problem) {
                self.problem = Some(problem.into());
                self.revision = self.revision.wrapping_add(1);
            }
            return None;
        };
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request_id = self.next_request_id;
        let policy_generation = self.policy_generation;
        self.pending_request = Some(request_id);
        self.auto_failure_paused = false;
        self.check_busy = true;
        self.problem = None;
        self.revision = self.revision.wrapping_add(1);
        Some((request_id, policy_generation, context))
    }

    pub(super) fn check_now(&mut self) {
        let Some((request_id, policy_generation, context)) = self.begin_check() else {
            return;
        };
        let policy = self.policy;
        let sender = self.sender.clone();
        let observed_operation_id = self.observed_operation_id.clone();
        std::thread::spawn(move || {
            let (result, settled) = match observe_latest_operation(&context.state_dir) {
                Err(error) => (Err(error), None),
                Ok(latest) => {
                    let prior_operation = observed_operation_id
                        .or_else(|| latest.map(|record| observation_id(&record).to_owned()));
                    let result = check(&context, policy);
                    match (result, prior_operation) {
                        (Ok(snapshot), Some(operation_id))
                            if snapshot.status != CheckStatus::Failed =>
                        {
                            match settlement_proof(&context, &snapshot, &operation_id) {
                                Ok(proof) => (Ok(snapshot), Some(proof)),
                                Err(error) => (Err(error), None),
                            }
                        }
                        (result, _) => (result, None),
                    }
                }
            };
            let _ = sender.send(Event::Checked {
                request_id,
                policy_generation,
                result,
                settled,
            });
        });
    }

    pub(super) fn next_event(&mut self) -> Option<Event> {
        while let Ok(event) = self.recovery_receiver.try_recv() {
            self.recovery_event(event);
        }
        self.receiver.try_recv().ok()
    }

    fn current_recovery(&self) -> Option<&RecoveryProjection> {
        self.recovery.as_ref().filter(|projection| {
            self.context.as_ref() == Some(&projection.context)
                && self.operation.as_ref() == Some(&projection.record)
                && self.observed_operation_id.as_deref()
                    == Some(projection.record.operation_id.as_str())
                && self.journal_error.is_none()
        })
    }

    fn recovery_unresolved(&self) -> bool {
        self.recovery_running.is_some()
            || self
                .recovery_handoff
                .as_ref()
                .is_some_and(|(_, _, _, released)| !released)
            || self.operation.as_ref().is_some_and(|record| {
                record.version == UPDATER_PROTOCOL
                    && !self.completed_history_admitted()
                    && !self.current_recovery().is_some_and(|projection| {
                        projection.disposition == RecoveryDisposition::Reconciled
                    })
            })
    }

    pub(super) fn recovery_operation_id(&self) -> Option<&str> {
        self.current_recovery()
            .map(|projection| projection.record.operation_id.as_str())
    }

    pub(super) fn recovery_start_requires_confirmation(&self) -> bool {
        self.current_recovery()
            .is_some_and(|projection| !projection.native_ui)
    }

    fn recovery_action(&self) -> Option<UpdateCardPrimaryAction> {
        if self.check_busy || self.standalone_operation.is_some() {
            return None;
        }
        if self.recovery_running.is_some()
            || self
                .recovery_handoff
                .as_ref()
                .is_some_and(|(_, _, _, released)| !released)
        {
            return Some(UpdateCardPrimaryAction::ShowProgress);
        }
        if let Some(projection) = self.current_recovery() {
            return match projection.disposition {
                RecoveryDisposition::Live(_) => Some(UpdateCardPrimaryAction::ShowProgress),
                RecoveryDisposition::Start => Some(UpdateCardPrimaryAction::RecoverStart),
                RecoveryDisposition::RecoverStoppedOriginal => {
                    Some(UpdateCardPrimaryAction::RecoverStoppedOriginal)
                }
                RecoveryDisposition::Recover => Some(UpdateCardPrimaryAction::Recover),
                RecoveryDisposition::Reconciled => None,
            };
        }
        // An unknown outcome can request read-only rediscovery, but cannot
        // authorize helper execution without an authenticated immutable plan.
        self.operation
            .as_ref()
            .filter(|record| {
                terminal_phase(record.phase) && record.phase != OperationPhase::Completed
            })
            .map(|_| UpdateCardPrimaryAction::Recover)
    }

    /// Cleanup may own retained preparation; only a matching authenticated
    /// projection permits execution. Unknown outcomes permit rediscovery only.
    pub(super) fn recovery_admitted(&self, action: UpdateCardPrimaryAction) -> bool {
        if self.context.is_none()
            || self.migration_blocked
            || self.relocation_busy
            || self.check_busy
            || self.launching.is_some()
            || self.journal_error.is_some()
            || self.standalone_operation.is_some()
            || self.recovery_running.is_some()
            || self
                .recovery_handoff
                .as_ref()
                .is_some_and(|(_, _, _, released)| !released)
        {
            return false;
        }
        if let Some(projection) = self.current_recovery() {
            return matches!(
                (action, &projection.disposition),
                (
                    UpdateCardPrimaryAction::Recover,
                    RecoveryDisposition::Recover
                ) | (
                    UpdateCardPrimaryAction::RecoverStart,
                    RecoveryDisposition::Start
                ) | (
                    UpdateCardPrimaryAction::RecoverStoppedOriginal,
                    RecoveryDisposition::RecoverStoppedOriginal
                )
            );
        }
        action == UpdateCardPrimaryAction::Recover
            && self.operation.as_ref().is_some_and(|record| {
                terminal_phase(record.phase)
                    && record.phase != OperationPhase::Completed
                    && self.observed_operation_id.as_deref() == Some(record.operation_id.as_str())
            })
    }

    fn observe_recovery(&mut self) {
        if self.recovery_observing.is_some()
            || self.standalone_operation.is_some()
            || self.completed_history_admitted()
        {
            return;
        }
        let (Some(context), Some(record)) = (self.context.clone(), self.operation.as_ref()) else {
            return;
        };
        let operation_id = record.operation_id.clone();
        if self.observed_operation_id.as_deref() != Some(operation_id.as_str()) {
            return;
        }
        self.recovery_request = self.recovery_request.wrapping_add(1);
        let request = self.recovery_request;
        self.recovery_observing = Some((
            context.clone(),
            operation_id.clone(),
            request,
            record.clone(),
        ));
        let running_pid = self
            .recovery_running
            .as_ref()
            .filter(|(owner, id, _, _)| owner == &context && id == &operation_id)
            .and_then(|(_, _, _, pid)| *pid);
        let sender = self.recovery_sender.clone();
        std::thread::spawn(move || {
            let result = recovery_projection(&context, &operation_id, running_pid);
            let _ = sender.send(RecoveryEvent::Observed {
                context,
                operation_id,
                request,
                result,
            });
        });
    }

    pub(super) fn recovery_stop_plan(&self) -> Option<UpdatePlan> {
        if !self.recovery_admitted(UpdateCardPrimaryAction::RecoverStoppedOriginal) {
            return None;
        }
        self.current_recovery()
            .map(|projection| projection.plan.clone())
    }

    pub(super) fn recover_stopped_original(&mut self) -> Option<(UpdateContext, String, u64)> {
        if !self.recovery_admitted(UpdateCardPrimaryAction::RecoverStoppedOriginal) {
            return None;
        }
        self.begin_recovery(false, true);
        let (context, id, request, _) = self.recovery_running.as_ref()?;
        Some((context.clone(), id.clone(), *request))
    }

    pub(super) fn legacy_recovery_handoff_active(
        &self,
        context: &UpdateContext,
        operation_id: &str,
        request: u64,
    ) -> bool {
        self.recovery_handoff
            .as_ref()
            .is_some_and(|(owner, id, generation, released)| {
                owner == context && id == operation_id && *generation == request && !released
            })
    }

    pub(super) fn legacy_recovery_handoff_releasable(
        &self,
        context: &UpdateContext,
        operation_id: &str,
        request: u64,
    ) -> bool {
        self.recovery_handoff
            .as_ref()
            .is_some_and(|(owner, id, generation, released)| {
                owner == context && id == operation_id && *generation == request && *released
            })
    }

    pub(super) fn recover(&mut self, start: bool) {
        self.begin_recovery(start, false);
    }

    fn begin_recovery(&mut self, start: bool, stop_original: bool) {
        let action = if stop_original {
            UpdateCardPrimaryAction::RecoverStoppedOriginal
        } else if start {
            UpdateCardPrimaryAction::RecoverStart
        } else {
            UpdateCardPrimaryAction::Recover
        };
        if !self.recovery_admitted(action) {
            return;
        }
        let Some(projection) = self.current_recovery().cloned() else {
            self.observe_recovery();
            self.revision = self.revision.wrapping_add(1);
            return;
        };
        // Admission matched this exact projection and requested disposition.
        self.recovery_request = self.recovery_request.wrapping_add(1);
        let request = self.recovery_request;
        let context = projection.context.clone();
        let operation_id = projection.record.operation_id.clone();
        self.recovery_running = Some((context.clone(), operation_id.clone(), request, None));
        self.recovery_error = None;
        if stop_original {
            self.recovery_handoff = Some((context.clone(), operation_id.clone(), request, false));
        }
        self.problem = None;
        self.pending_request = None;
        self.revision = self.revision.wrapping_add(1);
        let sender = self.recovery_sender.clone();
        std::thread::spawn(move || {
            let mut spawned = false;
            let mut child_exited = false;
            let result = launch_recovery(
                &projection,
                start,
                stop_original,
                request,
                &sender,
                &mut spawned,
                &mut child_exited,
            );
            if stop_original
                && (!spawned || child_exited)
                && recovery_handoff_quiescent(&projection).unwrap_or(false)
            {
                let _ = sender.send(RecoveryEvent::HandoffReleased {
                    context: context.clone(),
                    operation_id: operation_id.clone(),
                    request,
                });
            }
            let _ = sender.send(RecoveryEvent::Finished {
                context,
                operation_id,
                request,
                result,
            });
        });
    }

    pub(super) fn show_recovery_progress(&mut self) {
        let Some(projection) = self.current_recovery().cloned() else {
            self.observe_recovery();
            return;
        };
        let pid = self
            .recovery_running
            .as_ref()
            .filter(|(context, id, _, _)| {
                context == &projection.context && id == &projection.record.operation_id
            })
            .and_then(|(_, _, _, pid)| *pid)
            .or_else(|| match projection.disposition {
                RecoveryDisposition::Live(pid) => Some(pid),
                _ => None,
            });
        if let Some(pid) = pid {
            // Authentication and activation occur on a worker, never a timer
            // callback on AppKit's main thread. No duplicate helper is spawned.
            let sender = self.recovery_sender.clone();
            let request = self.recovery_request;
            std::thread::spawn(move || {
                let result = recovery_projection(
                    &projection.context,
                    &projection.record.operation_id,
                    Some(pid),
                )
                .and_then(|fresh| {
                    if fresh.disposition != RecoveryDisposition::Live(pid) {
                        return Err("recovery process no longer owns this operation".into());
                    }
                    let helper = fresh
                        .live_helper
                        .as_ref()
                        .ok_or("authenticated live recovery identity is unavailable")?;
                    focus_recovery_helper(
                        pid,
                        helper,
                        &fresh.plan,
                        fresh.coordinator.as_ref() == Some(helper),
                    )
                });
                let _ = sender.send(RecoveryEvent::Focused {
                    context: projection.context,
                    operation_id: projection.record.operation_id.clone(),
                    request,
                    record: projection.record,
                    result,
                });
            });
        } else if self.recovery_running.is_none() {
            self.observe_recovery();
        }
    }

    fn recovery_event(&mut self, event: RecoveryEvent) {
        match event {
            RecoveryEvent::HandoffReleased {
                context,
                operation_id,
                request,
            } => {
                if self.context.as_ref() == Some(&context)
                    && self.observed_operation_id.as_deref() == Some(operation_id.as_str())
                {
                    if let Some((owner, id, generation, released)) = self.recovery_handoff.as_mut()
                    {
                        if owner == &context && id == &operation_id && *generation == request {
                            *released = true;
                            self.revision = self.revision.wrapping_add(1);
                        }
                    }
                }
            }
            RecoveryEvent::Focused {
                context,
                operation_id,
                request,
                record,
                result,
            } => {
                if self.context.as_ref() == Some(&context)
                    && self.observed_operation_id.as_deref() == Some(operation_id.as_str())
                    && self.recovery_request == request
                    && self.operation.as_ref() == Some(&record)
                {
                    self.recovery_error = result.err();
                    self.revision = self.revision.wrapping_add(1);
                }
            }
            RecoveryEvent::Observed {
                context,
                operation_id,
                request,
                result,
            } => {
                if !self
                    .recovery_observing
                    .as_ref()
                    .is_some_and(|(owner, id, generation, _)| {
                        owner == &context && id == &operation_id && *generation == request
                    })
                {
                    return;
                }
                let (_, _, _, observed_record) = self.recovery_observing.take().unwrap();
                if self.context.as_ref() != Some(&context)
                    || self.observed_operation_id.as_deref() != Some(operation_id.as_str())
                    || self.operation.as_ref() != Some(&observed_record)
                {
                    return;
                }
                match result {
                    Ok(projection) => {
                        if self.operation.as_ref() != Some(&projection.record) {
                            if !terminal_phase(projection.record.phase)
                                || self.operation.as_ref().is_none_or(|prior| {
                                    prior.operation_id != operation_id
                                        || prior.phase == OperationPhase::UserStopped
                                })
                            {
                                self.next_journal_poll = Instant::now();
                                return;
                            }
                            // A reopened app can observe recovery performed by
                            // its authenticated retained helper, not only work
                            // launched during this particular UI lifetime.
                            self.recovery_journal_authority =
                                Some((context.clone(), operation_id.clone()));
                            self.update_journal(
                                &operation_id,
                                Ok(Some(projection.record.clone())),
                                None,
                            );
                        }
                        self.recovery = Some(projection);
                        self.recovery_error = None;
                    }
                    Err(error) => {
                        self.recovery = None;
                        self.recovery_error = Some(error);
                    }
                }
                self.revision = self.revision.wrapping_add(1);
            }
            RecoveryEvent::Started {
                context,
                operation_id,
                request,
                pid,
            } => {
                if self.context.as_ref() != Some(&context)
                    || self.observed_operation_id.as_deref() != Some(operation_id.as_str())
                {
                    return;
                }
                if let Some((owner, id, generation, running_pid)) = self.recovery_running.as_mut() {
                    if owner == &context && id == &operation_id && *generation == request {
                        *running_pid = Some(pid);
                        self.revision = self.revision.wrapping_add(1);
                    }
                }
            }
            RecoveryEvent::Finished {
                context,
                operation_id,
                request,
                result,
            } => {
                if !self
                    .recovery_running
                    .as_ref()
                    .is_some_and(|(owner, id, generation, _)| {
                        owner == &context && id == &operation_id && *generation == request
                    })
                    || self.context.as_ref() != Some(&context)
                    || self.observed_operation_id.as_deref() != Some(operation_id.as_str())
                {
                    return;
                }
                self.recovery_running = None;
                self.recovery_journal_authority = Some((context.clone(), operation_id.clone()));
                self.recovery_error = result.err();
                self.recovery = None;
                self.next_journal_poll = Instant::now();
                self.reload_journal(operation_id);
                self.observe_recovery();
                self.revision = self.revision.wrapping_add(1);
            }
        }
    }

    pub(super) fn checked(
        &mut self,
        result: Result<CheckSnapshot, String>,
        settled: Option<SettledOperationProof>,
    ) -> Option<u64> {
        if result
            .as_ref()
            .is_ok_and(|snapshot| snapshot.policy != self.policy)
        {
            self.check_busy = false;
            self.revision = self.revision.wrapping_add(1);
            return None;
        }
        self.candidate_generation = self.candidate_generation.wrapping_add(1);
        let mut changed = self.check_busy
            || match &result {
                Ok(next) => self.snapshot.as_ref().is_none_or(|old| {
                    old.status != next.status
                        || old.origin != next.origin
                        || old.policy != next.policy
                        || old.release != next.release
                        || old.installed_version != next.installed_version
                        || old.detail != next.detail
                        || old.plan != next.plan
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
                if let Some(SettledOperationProof::StandaloneInstall(record)) = settled.as_ref() {
                    if self.launching.is_none()
                        && self.launch_plan.is_none()
                        && self.prepared_plan.is_none()
                        && self.journal_error.is_none()
                        && ((self.observed_operation_id.as_deref()
                            == Some(record.operation_id.as_str())
                            && self.standalone_operation.as_ref() == Some(record))
                            || (self.observed_operation_id.is_none()
                                && self.operation.is_none()
                                && self.standalone_operation.is_none()))
                    {
                        self.previous_standalone_outcome = Some(record.clone());
                        self.retired_operation_id = Some(record.operation_id.clone());
                        self.observed_operation_id = None;
                        self.standalone_operation = None;
                        changed = true;
                    }
                }
                if let Some(SettledOperationProof::Update(record)) = settled {
                    let matching_observation = self.observed_operation_id.as_deref()
                        == Some(record.operation_id.as_str())
                        && self.operation.as_ref() == Some(&record);
                    let not_yet_observed =
                        self.observed_operation_id.is_none() && self.operation.is_none();
                    if self.launching.is_none()
                        && (self.launch_plan.is_none()
                            || (self.prepared_plan.is_none()
                                && matching_observation
                                && terminal_phase(record.phase)))
                        && (self.prepared_plan.is_none()
                            || self.cancelled_preparation.as_deref()
                                == Some(record.operation_id.as_str()))
                        && self.journal_error.is_none()
                        && (matching_observation || not_yet_observed)
                    {
                        // Keep the proven outcome for presentation even after the runtime
                        // observation retires; never delete or alter the durable journal.
                        self.previous_outcome = Some(record.clone());
                        changed = true;
                        self.observed_operation_id = None;
                        self.operation = None;
                        self.launch_plan = None;
                        self.prepared_plan = None;
                        self.cancelled_preparation = None;
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
        if self.migration_blocked
            || self.check_busy
            || self.launching.is_some()
            || self.launch_plan.is_some()
            || self.prepared_plan.is_some()
            || self.observed_operation_id.is_some()
            || self.operation.is_some()
            || self.problem.is_some()
            || self.journal_error.is_some()
        {
            return None;
        }
        let snapshot = self.snapshot.as_ref()?;
        if snapshot.policy != self.policy {
            return None;
        }
        snapshot.plan.as_ref().filter(|plan| {
            plan.target.as_ref().is_none_or(|target| {
                target.policy == self.policy && snapshot.release.as_ref() == Some(&target.release)
            })
        })
    }
    pub(super) fn preparing_plan(&self, operation_id: &str) -> Option<&UpdatePlan> {
        self.launch_plan.as_ref().filter(|plan| {
            plan.operation_id == operation_id
                && self.launching.as_deref() == Some(operation_id)
                && self.observed_operation_id.as_deref() == Some(operation_id)
                && !self
                    .operation
                    .as_ref()
                    .is_some_and(|record| terminal_phase(record.phase))
                && self.launch_generation == Some(self.consent_generation())
                && plan
                    .target
                    .as_ref()
                    .is_none_or(|target| target.policy == self.policy)
        })
    }

    pub(super) fn launch_waiting(&mut self, operation_id: &str) {
        if self.launching.as_deref() == Some(operation_id)
            && !self
                .operation
                .as_ref()
                .is_some_and(|record| terminal_phase(record.phase))
            && !self.launch_waiting
        {
            self.launch_waiting = true;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub(super) fn observing(&self, operation_id: &str) -> bool {
        self.observed_operation_id.as_deref() == Some(operation_id)
    }

    pub(super) fn preparation_cancelled(&mut self, operation_id: &str) {
        if self
            .launch_plan
            .as_ref()
            .is_some_and(|plan| plan.operation_id == operation_id)
            || self
                .prepared_plan
                .as_ref()
                .is_some_and(|plan| plan.operation_id == operation_id)
        {
            // Native state explicitly cancelled this request. Retain the
            // exact plan until independent, locked settlement proof.
            self.cancelled_preparation = Some(operation_id.to_owned());
        }
    }

    pub(super) fn recovery_reconciled(&mut self, operation_id: &str) {
        if self
            .prepared_plan
            .as_ref()
            .is_some_and(|plan| plan.operation_id == operation_id)
        {
            self.prepared_plan = None;
            self.cancelled_preparation = None;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub(super) fn launch(
        &mut self,
        plan: UpdatePlan,
        generation: (u64, u64),
    ) -> Result<(), String> {
        if !self.consent_matches(&plan, generation) {
            return Err("Update state changed; check again before applying".into());
        }
        self.launching = Some(plan.operation_id.clone());
        self.observed_operation_id = Some(plan.operation_id.clone());
        self.prepared_plan = None;
        self.cancelled_preparation = None;
        self.launch_waiting = false;
        self.launch_plan = Some(plan.clone());
        self.problem = None;
        self.launch_generation = Some(generation);
        self.revision = self.revision.wrapping_add(1);
        let sender = self.sender.clone();
        let notice = self.sender.clone();
        std::thread::spawn(move || {
            let result = stage_and_launch(&plan, || {
                let _ = notice.send(Event::LaunchWaiting {
                    operation_id: plan.operation_id.clone(),
                });
            });
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
        self.launch_waiting = false;
        if let Err(error) = result {
            self.launching = None;
            self.launch_generation = None;
            match error {
                LaunchFailure::NoSpawn(detail) => {
                    self.problem = Some(detail);
                    // Staging/spawn failure never gave this helper ownership.
                    // Keep any independently observed journal evidence.
                    if self.operation.is_none()
                        && (self.journal_error.is_none()
                            || self.journal_error_operation_id.as_deref() == Some(operation_id))
                    {
                        self.launch_plan = None;
                        self.observed_operation_id = None;
                        self.journal_error = None;
                        self.journal_error_operation_id = None;
                    }
                }
                LaunchFailure::Spawned(detail) => {
                    self.problem = Some(detail);
                    // The helper may have accepted native Prepare even if
                    // stdout failed before its final ownership reply.
                    self.prepared_plan = self.launch_plan.take();
                }
            }
            self.revision = self.revision.wrapping_add(1);
            return false;
        }
        // Accepted native Prepare precedes the final ACK. Preserve its exact
        // immutable plan for independently proved recovery, and consume this
        // launch so a duplicate final reply cannot be accepted.
        self.prepared_plan = self.launch_plan.take();
        self.launching = None;
        self.launch_generation = None;
        self.revision = self.revision.wrapping_add(1);
        self.next_journal_poll = Instant::now();
        true
    }
    pub(super) fn abort_launch(&mut self, operation_id: &str, detail: String) {
        if self.observed_operation_id.as_deref() != Some(operation_id)
            || (self.launching.as_deref() != Some(operation_id)
                && !self
                    .prepared_plan
                    .as_ref()
                    .is_some_and(|plan| plan.operation_id == operation_id))
        {
            return;
        }
        self.launching = None;
        self.launch_generation = None;
        self.launch_waiting = false;
        self.problem = Some(detail);
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn poll_operation(&mut self) {
        if Instant::now() < self.next_journal_poll {
            return;
        }
        let Some(operation_id) = self.observed_operation_id.clone() else {
            return;
        };
        self.next_journal_poll = Instant::now() + Duration::from_secs(5);
        self.observe_recovery();
        self.reload_journal(operation_id);
    }
    pub(super) fn reload_journal(&mut self, operation_id: String) {
        if self.observed_operation_id.as_deref() != Some(operation_id.as_str())
            || self.journal_in_flight.is_some()
        {
            return;
        }
        let Some(context) = self.context.as_ref() else {
            return;
        };
        self.journal_in_flight = Some(operation_id.clone());
        let context = context.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = observe_app_operation(&context, &operation_id);
            let prepared_recovery =
                result
                    .as_ref()
                    .ok()
                    .and_then(Option::as_ref)
                    .and_then(|record| match record {
                        OperationObservation::Update(record) => {
                            prepared_recovery_proof(&context, &operation_id, record).ok()
                        }
                        OperationObservation::StandaloneInstall(_) => None,
                    });
            let completed_history = result
                .as_ref()
                .ok()
                .and_then(Option::as_ref)
                .and_then(|record| completed_history_proof(&context, record).ok());
            let _ = sender.send(Event::Journal {
                operation_id,
                result,
                prepared_recovery,
                completed_history,
            });
        });
    }

    pub(super) fn journal(
        &mut self,
        operation_id: &str,
        result: Result<Option<OperationObservation>, String>,
        prepared_recovery: Option<PreparedRecoveryProof>,
        completed_history: Option<CompletedHistoryProof>,
    ) -> Option<UpdateContext> {
        if self.observed_operation_id.as_deref() == Some(operation_id) {
            let admitted = self.completed_history_admitted();
            self.completed_history = completed_history.filter(|proof| {
                self.context.as_ref() == Some(&proof.context)
                    && observation_id(&proof.observation) == operation_id
                    && result.as_ref().ok().and_then(Option::as_ref) == Some(&proof.observation)
            });
            if admitted != self.completed_history_admitted() {
                self.revision = self.revision.wrapping_add(1);
            }
        }
        let reconciled = match result {
            Ok(Some(OperationObservation::StandaloneInstall(record))) => {
                if self.journal_in_flight.as_deref() == Some(operation_id) {
                    self.journal_in_flight = None;
                }
                if self.observed_operation_id.as_deref() == Some(operation_id)
                    && record.operation_id == operation_id
                    && self.operation.is_none()
                {
                    self.observe_standalone(record);
                }
                None
            }
            Ok(Some(OperationObservation::Update(record)))
                if self.standalone_operation.is_none() =>
            {
                self.update_journal(operation_id, Ok(Some(record)), prepared_recovery)
            }
            Ok(Some(_)) => self.update_journal(
                operation_id,
                Err("operation kind changed during observation".into()),
                None,
            ),
            Ok(None) => self.update_journal(operation_id, Ok(None), None),
            Err(error) => self.update_journal(operation_id, Err(error), None),
        };
        if self.completed_history_admitted() {
            // The journal worker independently proved this exact completed
            // observation settled. A concurrent recovery observer may have
            // mistaken that worker's operation lease for a live helper. Its
            // earlier projection/error is superseded, not new Start authority.
            let had_projection = self.recovery.take().is_some();
            let had_error = self.recovery_error.take().is_some();
            let had_observer = self.recovery_observing.take().is_some();
            if had_projection || had_error || had_observer {
                self.recovery_request = self.recovery_request.wrapping_add(1);
                self.revision = self.revision.wrapping_add(1);
            }
        }
        reconciled
    }

    pub(super) fn latest_journal(&mut self, result: Result<Option<OperationObservation>, String>) {
        match result {
            Ok(Some(OperationObservation::StandaloneInstall(record))) => {
                if self.retired_operation_id.as_deref() == Some(record.operation_id.as_str())
                    || self.launching.is_some()
                    || self.launch_plan.is_some()
                    || self.prepared_plan.is_some()
                    || self
                        .operation
                        .as_ref()
                        .is_some_and(|record| !terminal_phase(record.phase))
                    || self.standalone_operation.as_ref().is_some_and(|prior| {
                        prior.operation_id != record.operation_id && !prior.result.terminal()
                    })
                {
                    return;
                }
                self.operation = None;
                self.observed_operation_id = Some(record.operation_id.clone());
                self.observe_standalone(record);
            }
            Ok(Some(OperationObservation::Update(record))) => {
                if let Some(prior) = self.standalone_operation.as_ref() {
                    if !prior.result.terminal() || terminal_phase(record.phase) {
                        return;
                    }
                    self.standalone_operation = None;
                    self.observed_operation_id = None;
                }
                self.update_latest_journal(Ok(Some(record)));
            }
            Ok(None) => self.update_latest_journal(Ok(None)),
            Err(error) => self.update_latest_journal(Err(error)),
        }
    }

    fn observe_standalone(&mut self, record: StandaloneInstallRecord) {
        if self.standalone_operation.as_ref().is_some_and(|prior| {
            prior.operation_id == record.operation_id
                && prior.result.terminal()
                && !record.result.terminal()
        }) {
            return;
        }
        // A standalone journal has no normal-update recovery action. Retire
        // callbacks/errors for the superseded normal operation, not its history
        // or any independently held execution/preparation gate.
        let had_projection = self.recovery.take().is_some();
        let had_error = self.recovery_error.take().is_some();
        let had_observer = self.recovery_observing.take().is_some();
        if had_projection || had_error || had_observer {
            self.recovery_request = self.recovery_request.wrapping_add(1);
            self.revision = self.revision.wrapping_add(1);
        }
        if record.result.terminal() {
            self.previous_standalone_outcome = Some(record.clone());
        }
        if self.standalone_operation.as_ref() != Some(&record) || self.journal_error.is_some() {
            self.revision = self.revision.wrapping_add(1);
        }
        self.standalone_operation = Some(record);
        self.journal_error = None;
        self.journal_error_operation_id = None;
    }

    fn update_journal(
        &mut self,
        operation_id: &str,
        result: Result<Option<OperationRecord>, String>,
        prepared_recovery: Option<PreparedRecoveryProof>,
    ) -> Option<UpdateContext> {
        if self.journal_in_flight.as_deref() == Some(operation_id) {
            self.journal_in_flight = None;
        }
        if self.observed_operation_id.as_deref() != Some(operation_id) {
            return None;
        }
        let recovery_owned =
            self.recovery_journal_authority
                .as_ref()
                .is_some_and(|(context, id)| {
                    self.context.as_ref() == Some(context) && id == operation_id
                })
                || self
                    .recovery_running
                    .as_ref()
                    .is_some_and(|(context, id, _, _)| {
                        self.context.as_ref() == Some(context) && id == operation_id
                    });
        if self.operation.as_ref().is_some_and(|record| {
            record.operation_id == operation_id
                && terminal_phase(record.phase)
                && matches!(&result, Ok(Some(next))
                    if next.operation_id == operation_id
                        && next.phase != record.phase
                        && next.phase != OperationPhase::UserStopped
                        && (record.phase == OperationPhase::UserStopped
                            || !recovery_owned || !terminal_phase(next.phase)))
        }) {
            return None;
        }
        let recovery_record_changed = recovery_owned
            && matches!(&result, Ok(Some(next))
            if self.operation.as_ref() != Some(next) && terminal_phase(next.phase));
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
            self.launch_waiting = false;
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
            self.launch_generation = None;
            // A terminal journal can precede the final ACK. Retain its exact
            // plan for locked recovery without treating a late ACK as success.
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
        if changed {
            self.revision = self.revision.wrapping_add(1);
            self.observe_recovery();
        }
        if recovery_record_changed && self.journal_error.is_none() {
            self.recovery_journal_authority = None;
        }
        reconciled
    }
    fn update_latest_journal(&mut self, result: Result<Option<OperationRecord>, String>) {
        // Startup discovery must not replace a locally owned launch/preparation
        // or an active operation. An unowned terminal startup observation can
        // be superseded by a subsequently observed active operation.
        if self.observed_operation_id.is_some()
            && !(self.launching.is_none()
                && self.launch_plan.is_none()
                && self.prepared_plan.is_none()
                && self
                    .operation
                    .as_ref()
                    .is_some_and(|record| terminal_phase(record.phase))
                && matches!(&result, Ok(Some(record))
                    if !terminal_phase(record.phase)
                        && self.observed_operation_id.as_deref()
                            != Some(record.operation_id.as_str())))
        {
            if let Err(error) = result {
                if self.operation.is_none() && self.journal_error.is_none() {
                    self.journal_error =
                        Some(format!("Update journal cannot be verified: {error}"));
                    self.journal_error_operation_id = None;
                    self.revision = self.revision.wrapping_add(1);
                }
            }
            return;
        }
        match result {
            Ok(Some(record))
                if self.retired_operation_id.as_deref() != Some(record.operation_id.as_str()) =>
            {
                self.observed_operation_id = Some(record.operation_id.clone());
                if terminal_phase(record.phase) {
                    self.previous_outcome = Some(record.clone());
                }
                self.operation = Some(record);
                self.journal_error = None;
                self.journal_error_operation_id = None;
                self.revision = self.revision.wrapping_add(1);
                self.observe_recovery();
            }
            Err(error) => {
                self.journal_error = Some(format!("Update journal cannot be verified: {error}"));
                self.journal_error_operation_id = None;
                self.revision = self.revision.wrapping_add(1);
            }
            Ok(_) => {}
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
        let mut history = outcome.map_or_else(String::new, |record| {
            format!(
                "{}: {}",
                text(locale, Message::AppUpdatePreviousOperation),
                text(locale, outcome_message(record))
            )
        });
        if let Some(record) = self.previous_standalone_outcome.as_ref() {
            let _ = write!(
                history,
                " · Standalone install {}: {:?}",
                record.operation_id, record.result.status
            );
        }
        let (mut status, mut status_message, mut detail_message) = if self.journal_error.is_some() {
            (
                CheckStatus::Failed,
                Message::AppUpdateOutcomeUnknown,
                Message::AppUpdateJournalHelp,
            )
        } else if let Some(record) = self.standalone_operation.as_ref().filter(|record| {
            !(record.result.status == InstallStatus::InstalledStopped
                && self.completed_history_admitted())
        }) {
            if !record.result.terminal() {
                (
                    CheckStatus::Checking,
                    Message::AppUpdateInstalling,
                    Message::AppUpdateActiveHelp,
                )
            } else if record.result.applied {
                (
                    CheckStatus::Applied,
                    Message::AppUpdateApplied,
                    Message::AppUpdateBlockedHelp,
                )
            } else if record.result.status == InstallStatus::InstalledStopped {
                (
                    CheckStatus::Installed,
                    Message::AppUpdateInstalled,
                    Message::AppUpdateBlockedHelp,
                )
            } else {
                (
                    CheckStatus::Failed,
                    Message::AppUpdateOutcomeUnknown,
                    Message::AppUpdateBlockedHelp,
                )
            }
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
        if self.recovery_running.is_some() {
            detail_message = if self
                .current_recovery()
                .is_some_and(|projection| projection.disposition == RecoveryDisposition::Start)
            {
                Message::AppUpdateStartingHelp
            } else {
                Message::AppUpdateRecoveryChecking
            };
        } else if let Some(projection) = self.current_recovery() {
            detail_message = match projection.disposition {
                RecoveryDisposition::Reconciled => Message::AppUpdateRecoveryComplete,
                RecoveryDisposition::Start => Message::AppUpdateInstalledHelp,
                RecoveryDisposition::Live(_) => Message::AppUpdateRecoveryProgress,
                RecoveryDisposition::Recover => Message::AppUpdateBlockedHelp,
                RecoveryDisposition::RecoverStoppedOriginal => Message::AppUpdateBlockedHelp,
            };
            if projection.disposition == RecoveryDisposition::Start {
                status = CheckStatus::Installed;
                status_message = Message::AppUpdateInstalled;
            } else if projection.disposition == RecoveryDisposition::Reconciled
                && projection.record.phase == OperationPhase::Failed
                && projection.record.execution_fence == ExecutionFence::NoSpawn
                && !projection.record.installed
                && !projection.record.applied
            {
                status_message = Message::AppUpdateRecoveryNotStarted;
            }
        } else if self.recovery_error.is_some() {
            detail_message = Message::AppUpdateRecoveryBlocked;
        } else if self.completed_history_admitted() {
            detail_message = Message::AppUpdateRecoveryComplete;
        } else if self.recovery_observing.is_some() && self.operation.is_some() {
            detail_message = Message::AppUpdateRecoveryChecking;
        }
        let plan = self.plan();
        let migration_available = desktop_pet_update_coordinator::identity::LEGACY
            && self.context.is_some()
            && self.migration_available
            && self.migration_history_verified
            && self.migration_problem.is_none()
            && self.settings_admitted();
        let primary_action = if let Some(action) = self.recovery_action() {
            Some(action)
        } else if plan.is_some() {
            Some(UpdateCardPrimaryAction::Update)
        } else if migration_available
            && self.problem.is_none()
            && self.journal_error.is_none()
            && self.pending_request.is_none()
            && self.operation.is_none()
            && self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.policy == self.policy && snapshot.plan.is_none())
            && matches!(status, CheckStatus::Current | CheckStatus::Applied)
        {
            status = CheckStatus::Available;
            status_message = Message::MigrationNextStep;
            detail_message = Message::MigrationNextStepHelp;
            Some(UpdateCardPrimaryAction::Migrate)
        } else {
            None
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
            if let Some(release) = snapshot.release.as_ref() {
                let _ = writeln!(
                    diagnostics,
                    "Official release: {} · {}",
                    release.version, release.tag
                );
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
        if let Some(record) = self
            .standalone_operation
            .as_ref()
            .or(self.previous_standalone_outcome.as_ref())
        {
            let _ = writeln!(
                diagnostics,
                "Standalone install: {} · phase {:?} · installed {} · applied {} · restart {:?}",
                record.operation_id,
                record.phase,
                record.result.installed,
                record.result.applied,
                record.result.restart
            );
            if let Some(path) = &record.result.retained_bundle {
                let _ = writeln!(diagnostics, "Retained original bundle: {}", path.display());
            }
            if let Some(error) = &record.result.error {
                let _ = writeln!(diagnostics, "Standalone outcome: {error}");
            }
            let _ = writeln!(diagnostics, "Use update-status --operation-id {} for the retained helper's install-status/install-recover commands. Normal update/start commands cannot recover this operation.", record.operation_id);
        }
        if self.launch_waiting && active.is_none() && self.launching.is_some() {
            let _ = writeln!(
                diagnostics,
                "{}: {}",
                text(locale, Message::AppUpdateLaunchPending),
                text(locale, Message::AppUpdateActiveHelp)
            );
        }
        if let Some(error) = self.problem.as_ref() {
            let _ = writeln!(diagnostics, "Check or launch error: {error}");
        }
        if let Some(error) = self.journal_error.as_ref() {
            let _ = writeln!(diagnostics, "Journal error: {error}");
        }
        if let Some(error) = self.recovery_error.as_ref() {
            let _ = writeln!(diagnostics, "Recovery: {error}");
        }
        if let Some(projection) = self.current_recovery() {
            let _ = writeln!(
                diagnostics,
                "Recovery evidence: {:?} · native UI {}",
                projection.disposition, projection.native_ui
            );
        }
        if let Some(error) = &self.migration_problem {
            let _ = writeln!(diagnostics, "Migration unavailable: {error}");
        }
        let relocation_tern = self.relocation_tern.as_ref().and_then(|(id, observation)| {
            self.standalone_operation
                .as_ref()
                .or(self.previous_standalone_outcome.as_ref())
                .filter(|record| &record.operation_id == id && record.result.installed)
                .and_then(|_| observation.captured.then_some(observation))
        });
        if let Some(detail) = self
            .relocation_tern
            .as_ref()
            .and_then(|(_, observation)| observation.detail.as_ref())
        {
            let _ = writeln!(diagnostics, "Tern relocation observation: {detail}");
        }
        let relocation_unresolved = self.journal_error.is_some()
            || self.standalone_operation.as_ref().is_some_and(|record| {
                matches!(
                    record.result.status,
                    InstallStatus::Accepted
                        | InstallStatus::Running
                        | InstallStatus::Unknown
                        | InstallStatus::InstalledNotApplied
                )
            });
        let relocation_available = self.at_relocation_home()
            && !self.relocation_waiting
            && !self.relocation_busy
            && !self.recovery_unresolved()
            && active.is_none()
            && !relocation_unresolved;
        let primary_action = if self.at_relocation_home()
            && active.is_none()
            && !relocation_unresolved
            && !self.recovery_unresolved()
        {
            if relocation_available {
                Some(UpdateCardPrimaryAction::Relocate)
            } else {
                None
            }
        } else {
            primary_action
        };
        UpdateCardModel {
            auto_check: self.auto_check,
            receive_beta_updates: self.policy == UpdatePolicy::StableAndBeta,
            check_enabled: self.context.is_some() && self.check_admitted(),
            recovery_enabled: primary_action.is_some_and(|action| self.recovery_admitted(action)),
            busy: self.migration_blocked
                || self.relocation_busy
                // Completed history is not live work, even while admission still
                // awaits independent settlement. A running helper remains busy.
                || self.recovery_running.is_some()
                || self.current_recovery().is_some_and(|projection| {
                    matches!(projection.disposition, RecoveryDisposition::Live(_))
                })
                || (self.recovery_unresolved()
                    && self.operation.as_ref().is_some_and(|record| {
                        record.phase != OperationPhase::Completed
                    }))
                || self.check_busy
                || self.launching.is_some()
                || self.launch_plan.is_some()
                || self.prepared_plan.is_some()
                || active.is_some()
                || self
                    .standalone_operation
                    .as_ref()
                    .is_some_and(|record| !record.result.terminal())
                || self.journal_error.is_some(),
            settings_locked: self.relocation_busy || !self.settings_admitted(),
            status,
            status_text: text(locale, status_message).into(),
            running_version: self
                .context
                .as_ref()
                .map_or_else(String::new, |context| context.version.clone()),
            source: origin.map_or_else(
                || text(locale, Message::AppUpdateSourceUnknown).to_owned(),
                |origin| origin.manager_label().to_owned(),
            ),
            detail: text(locale, detail_message).into(),
            history,
            diagnostics,
            last_checked: self.last_checked,
            action: plan.map(|plan| plan.action.clone()),
            primary_action,
            migration_visible: desktop_pet_update_coordinator::identity::LEGACY,
            migration_available,
            migration_title: text(locale, Message::MigrationTitle).to_owned(),
            relocation_available,
            relocation_title: if relocation_tern.is_some() {
                text(locale, Message::RelocationComplete).to_owned()
            } else if self.at_relocation_home() {
                text(locale, Message::RelocationAction).to_owned()
            } else {
                String::new()
            },
            relocation_summary: if matches!(status, CheckStatus::Failed | CheckStatus::Checking)
                || self.check_busy
                || self.launching.is_some()
                || self.launch_plan.is_some()
                || self.prepared_plan.is_some()
                || active.is_some()
                || relocation_unresolved
            {
                String::new()
            } else if self.at_relocation_home() {
                text(
                    locale,
                    if self.relocation_waiting {
                        Message::RelocationWaiting
                    } else if self.relocation_busy {
                        Message::RelocationPreparing
                    } else {
                        Message::RelocationReady
                    },
                )
                .to_owned()
            } else if let Some(observation) = relocation_tern {
                text(
                    locale,
                    if observation.pending {
                        Message::RelocationTernPending
                    } else {
                        Message::RelocationComplete
                    },
                )
                .to_owned()
            } else {
                String::new()
            },
            relocation_detail: if self.at_relocation_home()
                && active.is_none()
                && !relocation_unresolved
            {
                text(
                    locale,
                    if self.relocation_waiting {
                        Message::RelocationWaiting
                    } else if self.relocation_busy {
                        Message::RelocationPreparing
                    } else {
                        Message::RelocationReady
                    },
                )
                .to_owned()
            } else if let Some(observation) =
                relocation_tern.filter(|_| active.is_none() && !relocation_unresolved)
            {
                let count = text(locale, Message::RelocationTernWindows)
                    .replace("{}", &observation.windows.to_string());
                if observation.pending {
                    format!(
                        "{}\n{}\n{count}",
                        text(locale, Message::RelocationComplete),
                        text(locale, Message::RelocationTernPending)
                    )
                } else {
                    format!("{}\n{count}", text(locale, Message::RelocationComplete))
                }
            } else {
                String::new()
            },
            download_url: self.download_url(),
            release: self
                .snapshot
                .as_ref()
                .filter(|snapshot| {
                    snapshot.policy == self.policy && !self.check_busy && self.problem.is_none()
                })
                .and_then(|snapshot| snapshot.release.clone()),
        }
    }
}
// Read-only projection of the helper's launch. The admission hashes all launch
// bytes, including fields this observer does not interpret; this grants no Stop authority.
#[derive(Deserialize)]
struct RelocationLaunch {
    version: u32,
    operation_id: String,
    original: desktop_pet_update_coordinator::installation::InstallationRecord,
    profile: desktop_pet_update_coordinator::native_runtime::ProfilePaths,
    helper: desktop_pet_update_coordinator::ExecutableIdentity,
    request: desktop_pet_update_coordinator::installation::InstallRequest,
    purpose: standalone_install::StandaloneInstallPurpose,
    source: standalone_install::InstallSource,
}

fn relocation_evidence(
    context: &UpdateContext,
    id: &str,
) -> Result<
    (
        RelocationLaunch,
        standalone_install::StandaloneAdmissionRecord,
        standalone_relocation::RelocationRequest,
    ),
    String,
> {
    use sha2::{Digest, Sha256};
    desktop_pet_update_coordinator::protocol::validate_operation_id(id)?;
    let root = context
        .state_dir
        .join(desktop_pet_update_coordinator::identity::UPDATES_DIRECTORY)
        .join(format!("install-{id}.resources"));
    let bytes =
        desktop_pet_update_coordinator::owned_private_file(&root.join("launch.json"), 196608)?;
    let launch: RelocationLaunch =
        serde_json::from_slice(&bytes).map_err(|e| format!("invalid relocation launch: {e}"))?;
    let admission = standalone_install::read_admission(&context.state_dir, id)?
        .ok_or("relocation admission is missing")?;
    let request: standalone_relocation::RelocationRequest =
        serde_json::from_slice(&desktop_pet_update_coordinator::owned_private_file(
            &root.join("relocation-request.json"),
            16384,
        )?)
        .map_err(|e| format!("invalid durable relocation request: {e}"))?;
    request.parent.validate()?;
    let standalone_install::StandaloneInstallPurpose::PrivateRelocation {
        target,
        parent,
        source_closure_sha256,
        ..
    } = &launch.purpose
    else {
        return Err("helper outcome is not a relocation".into());
    };
    let standalone_install::InstallSource::Running {
        instance,
        context: source,
    } = &launch.source
    else {
        return Err("relocation launch has no running source".into());
    };
    let home = desktop_pet_update_coordinator::installation::os_home()?;
    let original = InstallOrigin::Standalone {
        bundle: home.join("Applications/DesktopPet.app"),
    };
    let expected_target = InstallOrigin::Standalone {
        bundle: desktop_pet_update_coordinator::installation::standalone_bundle()?,
    };
    if launch.version != 1
        || launch.operation_id != id
        || admission.launch_sha256 != format!("{:x}", Sha256::digest(&bytes))
        || request.version != 1
        || request.context != *source
        || request.parent != *parent
        || launch.profile != parent.profile
        || launch.profile.config_dir != context.config_dir
        || launch.profile.state_dir != context.state_dir
        || source.running != context.running
        || source.executable != context.executable
        || source.running_origin != context.running_origin
        || source.config_dir != context.config_dir
        || source.state_dir != context.state_dir
        || source.instance_id != instance.instance_id
        || source.running != instance.image
        || launch.original.version != 1
        || launch.original.origin != original
        || launch.original.image != source.running
        || launch.original.executable != source.executable
        || source.executable != home.join("Applications/DesktopPet.app/Contents/MacOS/desktop-pet")
        || launch.original.app_version != source.version
        || source.running_origin.as_deref() != Some(&original)
        || launch.original.kind
            != desktop_pet_update_coordinator::installation::InstallationKind::Standalone
        || parent.candidate_image != source.running
        || parent.source_closure_sha256 != *source_closure_sha256
        || parent.generation != admission.original_stop_generation
        || target != &expected_target
        || launch.helper.path != root.join("helper")
        || launch.request.version != 1
        || launch.request.staged_app != root.join("DesktopPet.app")
        || launch.request.kind
            != desktop_pet_update_coordinator::installation::InstallationKind::Standalone
        || launch.request.source_root.is_some()
    {
        return Err("relocation outcome source, profile, parent or admission differs".into());
    }
    Ok((launch, admission, request))
}

fn preplan_relocation_record(
    context: &UpdateContext,
    id: &str,
) -> Result<StandaloneInstallRecord, String> {
    for path in [
        standalone_install::stop_ack_path(&context.state_dir, id)?,
        standalone_install::replacement_path(&context.state_dir, id)?,
        standalone_install::relocation_commit_path(&context.state_dir, id)?,
    ] {
        if path.try_exists().map_err(|e| e.to_string())? {
            return Err("pre-plan relocation contains post-Stop evidence".into());
        }
    }
    let (_, admission, _) = relocation_evidence(context, id)?;
    let result = admission
        .result
        .ok_or("relocation has no terminal admission outcome")?;
    if admission.phase != standalone_install::StandaloneAdmissionPhase::Settled
        || !matches!(
            result.status,
            InstallStatus::Cancelled | InstallStatus::Failed
        )
        || result.installed
        || result.applied
        || result.restart != standalone_install::InstallRestart::NotRequested
    {
        return Err("pre-plan relocation has no cleanup-only settlement".into());
    }
    let path = context
        .state_dir
        .join(desktop_pet_update_coordinator::identity::UPDATES_DIRECTORY)
        .join(format!("install-{id}.resources/interrupted-result.json"));
    let durable: standalone_install::StandaloneInstallResult = serde_json::from_slice(
        &desktop_pet_update_coordinator::owned_private_file(&path, 32768)?,
    )
    .map_err(|e| format!("invalid interrupted relocation outcome: {e}"))?;
    if durable != result {
        return Err("pre-plan outcome differs from admission".into());
    }
    Ok(StandaloneInstallRecord {
        version: 1,
        operation_id: id.to_owned(),
        phase: standalone_install::StandaloneInstallPhase::Failed,
        result,
    })
}

fn observe_app_operation(
    context: &UpdateContext,
    id: &str,
) -> Result<Option<OperationObservation>, String> {
    if !standalone_install::plan_path(&context.state_dir, id)?
        .try_exists()
        .map_err(|e| e.to_string())?
        && standalone_install::read_admission(&context.state_dir, id)?.is_some()
    {
        return preplan_relocation_record(context, id)
            .map(|record| Some(OperationObservation::StandaloneInstall(record)));
    }
    observe_operation(&context.state_dir, id)
}

fn authenticate_relocation_outcome(
    request: &standalone_relocation::RelocationRequest,
    previous_attempts: &[String],
    bytes: &[u8],
) -> Result<String, String> {
    if bytes.len() > 32768 {
        return Err("relocation helper outcome exceeds bound".into());
    }
    let outcome: standalone_install::StandaloneInstallResult = serde_json::from_slice(bytes)
        .map_err(|e| format!("invalid relocation helper outcome: {e}"))?;
    outcome.validate()?;
    if outcome.state_dir != request.context.state_dir
        || previous_attempts.contains(&outcome.operation_id)
    {
        return Err(
            "relocation helper returned a different profile or an old manual attempt".into(),
        );
    }
    let id = &outcome.operation_id;
    let (launch, admission, durable_request) = relocation_evidence(&request.context, id)?;
    if durable_request.version != request.version
        || durable_request.context != request.context
        || durable_request.parent != request.parent
    {
        return Err("relocation helper returned a stale request".into());
    }
    let record = match observe_app_operation(&request.context, id)? {
        Some(OperationObservation::StandaloneInstall(record)) => record,
        _ => return Err("relocation helper has no matching standalone journal".into()),
    };
    if standalone_install::plan_path(&request.context.state_dir, id)?
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        let plan = standalone_install::read_plan(&request.context.state_dir, id)?;
        if plan.source != launch.source
            || plan.original != launch.original
            || plan.profile != launch.profile
            || plan.purpose != launch.purpose
            || plan.helper_image != launch.helper
            || plan.original_stop_generation != admission.original_stop_generation
        {
            return Err("relocation journal differs from admitted launch".into());
        }
    }
    // A lost child ACK can return an ephemeral Unknown envelope. It identifies
    // only this authenticated journal, never success or permission to release.
    if outcome != record.result && outcome.status != InstallStatus::Unknown {
        return Err("relocation helper outcome differs from durable journal".into());
    }
    if admission
        .result
        .as_ref()
        .is_some_and(|result| result != &record.result)
    {
        return Err("relocation journal differs from settled admission".into());
    }
    Ok(id.clone())
}

fn read_relocation_helper(
    mut child: std::process::Child,
) -> Result<(std::process::ExitStatus, Vec<u8>), String> {
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or("relocation helper stdout is missing")?
        .take(32769)
        .read_to_end(&mut bytes);
    let status = child.wait().map_err(|e| e.to_string())?;
    read.map_err(|e| e.to_string())?;
    if bytes.len() > 32768 {
        return Err("relocation helper outcome exceeds bound".into());
    }
    Ok((status, bytes))
}

fn observation_id(observation: &OperationObservation) -> &str {
    match observation {
        OperationObservation::Update(record) => &record.operation_id,
        OperationObservation::StandaloneInstall(record) => &record.operation_id,
    }
}

fn standalone_settlement(
    context: &UpdateContext,
    record: &StandaloneInstallRecord,
) -> Result<(), String> {
    if !standalone_install::plan_path(&context.state_dir, &record.operation_id)?
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        if preplan_relocation_record(context, &record.operation_id)? != *record {
            return Err("pre-plan relocation outcome changed".into());
        }
        let (launch, admission, _) = relocation_evidence(context, &record.operation_id)?;
        let standalone_install::StandaloneInstallPurpose::PrivateRelocation { target, .. } =
            &launch.purpose
        else {
            return Err("pre-plan settlement is not a relocation".into());
        };
        if desktop_pet_update_coordinator::protocol::stop_generation(&context.state_dir)?
            != admission.original_stop_generation
            || desktop_pet_update_coordinator::protocol::reservation_exists(
                &launch.original.origin,
                Some(target),
                &context.state_dir,
                &record.operation_id,
            )?
        {
            return Err("pre-plan relocation is not released at its original generation".into());
        }
        start_allowed(&launch.original.origin, &context.state_dir, None, None)?;
        return start_allowed(target, &context.state_dir, None, None);
    }
    let plan = standalone_install::read_plan(&context.state_dir, &record.operation_id)?;
    record.validate(&plan)?;
    if plan.profile.config_dir != context.config_dir || plan.profile.state_dir != context.state_dir
    {
        return Err("standalone operation belongs to a different profile".into());
    }
    desktop_pet_update_coordinator::migration::verify_standalone_settlement(
        &plan,
        record,
        Some(context),
    )
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
    if observe_latest_operation(&context.state_dir)?.as_ref()
        != Some(&OperationObservation::Update(record.clone()))
    {
        return Err("prepared operation is no longer the latest journal".into());
    }
    recovery_scopes_free(context, &plan)?;
    Ok(PreparedRecoveryProof { record, plan })
}

fn recovery_scopes_free(context: &UpdateContext, plan: &UpdatePlan) -> Result<(), String> {
    start_allowed(&plan.origin, &context.state_dir, None, None)?;
    if plan.target_origin() != &plan.origin {
        start_allowed(plan.target_origin(), &context.state_dir, None, None)?;
    }
    let running_origin = context
        .running_origin
        .as_deref()
        .ok_or("running installation source is unavailable")?;
    start_allowed(running_origin, &context.state_dir, None, None)
}

// Read-only worker projection. Holding the lease proves absence of a helper,
// not completion of a manager; only the retained helper may reconcile those.
fn recovery_projection(
    context: &UpdateContext,
    operation_id: &str,
    launched_pid: Option<u32>,
) -> Result<RecoveryProjection, String> {
    use desktop_pet_update_coordinator::{executable_identity, owned_private_file};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let plan = read_plan(&context.state_dir, operation_id)?;
    if plan.context.config_dir != context.config_dir
        || plan.context.state_dir != context.state_dir
        || matches!(plan.origin, InstallOrigin::Unknown { .. })
    {
        return Err("recovery operation belongs to a different profile or unknown source".into());
    }
    let record = read_operation(&context.state_dir, operation_id)?
        .ok_or("recovery operation journal is unavailable")?;
    if observe_latest_operation(&context.state_dir)?.as_ref()
        != Some(&OperationObservation::Update(record.clone()))
    {
        return Err("recovery operation is not the current durable operation".into());
    }
    let lease = desktop_pet_update_coordinator::protocol::try_lock_operation(
        &context.state_dir,
        operation_id,
    )?;
    if lease.is_some() && settled_terminal(&record) && recovery_scopes_free(context, &plan).is_ok()
    {
        return Ok(RecoveryProjection {
            context: context.clone(),
            record,
            plan,
            helper: None,
            coordinator: None,
            live_helper: None,
            native_ui: false,
            disposition: RecoveryDisposition::Reconciled,
        });
    }
    let updates = private_updates(&context.state_dir)?;
    let helper_path = updates.join(format!("helper-{operation_id}"));
    let metadata = std::fs::symlink_metadata(&helper_path).map_err(|e| e.to_string())?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > 64 * 1024 * 1024
    {
        return Err("retained helper is not a private owned executable".into());
    }
    let helper = executable_identity(&helper_path)?;
    let owner: RecoveryLifecycle = serde_json::from_slice(&owned_private_file(
        &updates.join(format!("{operation_id}.lifecycle.json")),
        4096,
    )?)
    .map_err(|e| e.to_string())?;
    #[cfg(desktop_pet_legacy)]
    let expected_socket = &plan.context.herdr_socket;
    #[cfg(not(desktop_pet_legacy))]
    let expected_socket = &plan.context.state_dir.join("control.sock");
    if owner.original_pid == 0
        || owner.helper_pid == 0
        || owner.original_instance_id != plan.context.instance_id
        || owner.original_image != plan.running
        || owner.original_config_dir != plan.context.config_dir
        || owner.original_state_dir != plan.context.state_dir
        || owner.original_socket != *expected_socket
        || owner.original_assets != plan.context.assets_override
        || owner.helper_image != helper
    {
        return Err("retained helper identity differs from the immutable operation".into());
    }
    #[cfg(target_os = "macos")]
    {
        let mut signature = Command::new("/usr/bin/codesign");
        signature.arg("--verify").arg("--strict").arg(&helper.path);
        recovery_inspect_command(signature, 4096)?;
    }
    let capabilities = recovery_command(&plan, &helper.path, "update-capabilities", false)?;
    let bytes = recovery_inspect_command(capabilities, 4096)?;
    let response = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    desktop_pet_update_coordinator::validate_helper_capabilities(response, &plan.context)?;
    let capabilities: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let native_ui = match capabilities.get("ui_recovery_contract") {
        None => false,
        Some(value) if value.as_u64() == Some(1) && value.is_u64() => true,
        Some(_) => return Err("retained helper has an unsupported UI recovery contract".into()),
    };
    if executable_identity(&helper.path)? != helper {
        return Err("retained helper changed during authentication".into());
    }
    // The coordinator is today's signed bundled helper, never the retained
    // executable masquerading as an implementation of a newer UI contract.
    let coordinator = if native_ui {
        None
    } else {
        authenticate_recovery_coordinator(context, &plan)?
    };
    let receipt_path = updates.join(format!("{operation_id}.app-recovery.json"));
    let receipt = match std::fs::symlink_metadata(&receipt_path) {
        Ok(_) => {
            let receipt: RecoveryLaunchReceipt =
                serde_json::from_slice(&owned_private_file(&receipt_path, 65536)?)
                    .map_err(|e| e.to_string())?;
            validate_recovery_receipt(&receipt, &plan, &helper, coordinator.as_ref())?;
            Some(receipt)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    let mut live = None;
    if let Some(receipt) = receipt.as_ref() {
        if let RecoveryLaunchProcess::Spawned { pid } = receipt.process {
            if recovery_process_scoped(pid, &receipt.helper_image, &plan, receipt.coordinator)? {
                live = Some((pid, receipt.helper_image.clone()));
            }
        }
    }
    if live.is_none() {
        // A callback PID is not authority for a new image or a different
        // operation. Only a matching durable receipt may identify a wrapper.
        for pid in launched_pid
            .into_iter()
            .chain(std::iter::once(owner.helper_pid))
        {
            if recovery_process_scoped(pid, &helper, &plan, false)? {
                live = Some((pid, helper.clone()));
                break;
            }
        }
    }
    let live_helper = live.as_ref().map(|(_, image)| image.clone());
    let disposition = if let Some((pid, _)) = live {
        RecoveryDisposition::Live(pid)
    } else if lease.is_none() {
        return Err(
            "operation lease is live but its helper cannot be authenticated; ownership retained"
                .into(),
        );
    } else {
        let status = recovery_command(&plan, &helper.path, "status", true)?;
        let bytes = recovery_inspect_command(status, 32768)?;
        let inspected: OperationRecord =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if inspected.operation_id != operation_id || inspected.version != plan.version {
            return Err("retained helper status differs from the immutable operation".into());
        }
        let candidate_verified = inspected.candidate.as_ref().is_some_and(|candidate| {
            desktop_pet_update_coordinator::validate_candidate_for_context(
                &candidate.path,
                &plan.context,
            )
            .as_ref()
                == Ok(candidate)
        });
        if recovery_start_eligible(&record, &inspected, candidate_verified)
            && desktop_pet_update_coordinator::protocol::stop_generation(&context.state_dir)?
                == owner.stop_generation
        {
            RecoveryDisposition::Start
        } else if !native_ui
            && coordinator.is_some()
            && recovery_stop_original_eligible(&record, &inspected)
            && context == &plan.context
            && owner.original_pid == std::process::id()
            && recovery_process_matches(owner.original_pid, &owner.original_image)?
            && desktop_pet_update_coordinator::protocol::stop_generation(&context.state_dir)?
                == owner.stop_generation
        {
            RecoveryDisposition::RecoverStoppedOriginal
        } else {
            RecoveryDisposition::Recover
        }
    };
    Ok(RecoveryProjection {
        context: context.clone(),
        record,
        plan,
        helper: Some(helper),
        coordinator,
        live_helper,
        native_ui,
        disposition,
    })
}

fn recovery_stop_original_eligible(durable: &OperationRecord, inspected: &OperationRecord) -> bool {
    durable == inspected
        && durable.phase == OperationPhase::Failed
        && durable.execution_fence == ExecutionFence::NoSpawn
        && !durable.installed
        && !durable.applied
}

fn authenticate_recovery_coordinator(
    context: &UpdateContext,
    plan: &UpdatePlan,
) -> Result<Option<desktop_pet_update_coordinator::ExecutableIdentity>, String> {
    use desktop_pet_update_coordinator::{executable_identity, helper_path_for_context};
    let path = helper_path_for_context(&context.running.path, context)?;
    let helper = executable_identity(&path)?;
    #[cfg(target_os = "macos")]
    {
        let mut signature = Command::new("/usr/bin/codesign");
        signature.arg("--verify").arg("--strict").arg(&helper.path);
        recovery_inspect_command(signature, 4096)?;
    }
    let command = recovery_command(plan, &helper.path, "update-capabilities", false)?;
    let bytes = recovery_inspect_command(command, 4096)?;
    let response = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    desktop_pet_update_coordinator::validate_helper_capabilities(response, context)?;
    let capabilities: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if !recovery_legacy_ui_capability(&capabilities)? {
        return Ok(None);
    }
    if executable_identity(&helper.path)? != helper {
        return Err("current bundled helper changed during authentication".into());
    }
    Ok(Some(helper))
}

fn recovery_legacy_ui_capability(capabilities: &serde_json::Value) -> Result<bool, String> {
    match capabilities.get("ui_recovery_legacy_contract") {
        None => Ok(false),
        Some(value) if value.is_u64() && value.as_u64() == Some(1) => Ok(true),
        Some(_) => Err("current bundled helper has an unsupported legacy recovery contract".into()),
    }
}

fn validate_recovery_receipt(
    receipt: &RecoveryLaunchReceipt,
    plan: &UpdatePlan,
    retained: &desktop_pet_update_coordinator::ExecutableIdentity,
    coordinator: Option<&desktop_pet_update_coordinator::ExecutableIdentity>,
) -> Result<(), String> {
    let expected = if receipt.coordinator {
        coordinator
    } else {
        Some(retained)
    };
    if receipt.version != 2
        || receipt.operation_id != plan.operation_id
        || receipt.context != plan.context
        || matches!(receipt.process, RecoveryLaunchProcess::Spawned { pid } if pid == 0 || pid > i32::MAX as u32)
        || &receipt.retained_helper_image != retained
        || expected != Some(&receipt.helper_image)
    {
        return Err("recovery launch receipt differs from its immutable operation or image".into());
    }
    if matches!(receipt.process, RecoveryLaunchProcess::Launching) {
        return Err(
            "recovery launch ownership is unresolved; duplicate launch is forbidden".into(),
        );
    }
    Ok(())
}

fn write_recovery_receipt(
    plan: &UpdatePlan,
    receipt: &RecoveryLaunchReceipt,
) -> Result<(), String> {
    desktop_pet_update_coordinator::protocol::write_private_bytes(
        &private_updates(&plan.context.state_dir)?
            .join(format!("{}.app-recovery.json", plan.operation_id)),
        &serde_json::to_vec(receipt).map_err(|e| e.to_string())?,
        0o600,
    )
}

fn recovery_start_eligible(
    durable: &OperationRecord,
    inspected: &OperationRecord,
    candidate_verified: bool,
) -> bool {
    durable.phase == OperationPhase::Failed
        && inspected.phase == OperationPhase::Failed
        && durable.execution_fence != ExecutionFence::ManagerIntent
        && inspected.execution_fence != ExecutionFence::ManagerIntent
        && durable.installed
        && inspected.installed
        && !durable.applied
        && !inspected.applied
        && durable.candidate.is_some()
        && durable.candidate == inspected.candidate
        && candidate_verified
}

fn recovery_command(
    plan: &UpdatePlan,
    helper: &Path,
    verb: &str,
    scoped: bool,
) -> Result<Command, String> {
    let mut command = Command::new(helper);
    command.arg(verb);
    if scoped {
        command
            .arg("--state-dir")
            .arg(&plan.context.state_dir)
            .arg("--operation-id")
            .arg(&plan.operation_id);
    }
    command
        .current_dir(private_updates(&plan.context.state_dir)?)
        .env_clear()
        .envs(&plan.context.environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(command)
}

// Capture a bounded diagnostic while draining every byte. Reaching the bound
// must never close a pipe early and strand the child behind a full stderr pipe.
fn drain_recovery_output(mut stream: impl Read, bound: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = stream.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            return Ok(bytes);
        }
        let keep = count.min(bound.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..keep]);
    }
}

fn recovery_inspect_command(mut command: Command, bound: usize) -> Result<Vec<u8>, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or("recovery inspection stdout is unavailable")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("recovery inspection stderr is unavailable")?;
    let out = std::thread::spawn(move || drain_recovery_output(stdout, bound));
    let err = std::thread::spawn(move || drain_recovery_output(stderr, 4096));
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            // This is our own read-only capability/status subprocess, never
            // the original operation helper or a recovery that may own state.
            let _ = child.kill();
            let _ = child.wait();
            return Err("retained helper inspection did not complete".into());
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    let bytes = out
        .join()
        .map_err(|_| "recovery inspection output worker failed")??;
    let error = err
        .join()
        .map_err(|_| "recovery inspection error worker failed")??;
    if !status.success() {
        return Err(format!(
            "retained helper inspection failed: {}",
            String::from_utf8_lossy(&error)
        ));
    }
    if bytes.len() >= bound {
        return Err("retained helper inspection exceeded its output bound".into());
    }
    Ok(bytes)
}

fn launch_recovery(
    approved: &RecoveryProjection,
    start: bool,
    stop_original: bool,
    request: u64,
    sender: &Sender<RecoveryEvent>,
    spawned: &mut bool,
    child_exited: &mut bool,
) -> Result<(), String> {
    // Serialize app-side launchers across UI lifetimes. The helper owns its
    // separate operation lease; this private lock prevents a second consent UI
    // even before the first child has acquired that lease.
    let _launch_lock = recovery_launch_lock(&approved.plan)?;
    let fresh = recovery_projection(&approved.context, &approved.record.operation_id, None)?;
    if fresh.plan != approved.plan
        || fresh.helper != approved.helper
        || fresh.coordinator != approved.coordinator
        || fresh.native_ui != approved.native_ui
        || fresh.record != approved.record
    {
        return Err("recovery state changed; review the current recovery action".into());
    }
    if let RecoveryDisposition::Live(pid) = fresh.disposition {
        let helper = fresh
            .live_helper
            .as_ref()
            .ok_or("live recovery helper missing")?;
        return focus_recovery_helper(
            pid,
            helper,
            &fresh.plan,
            fresh.coordinator.as_ref() == Some(helper),
        );
    }
    if fresh.disposition == RecoveryDisposition::Reconciled {
        return Ok(());
    }
    if start && fresh.disposition != RecoveryDisposition::Start {
        return Err("installed image is not verified for an explicit recovery start".into());
    }
    if stop_original != (fresh.disposition == RecoveryDisposition::RecoverStoppedOriginal) {
        return Err(
            "original instance is not eligible for explicit stopped-original recovery".into(),
        );
    }
    let retained = fresh
        .helper
        .as_ref()
        .ok_or("retained recovery helper is unavailable")?;
    let helper = if stop_original {
        fresh
            .coordinator
            .as_ref()
            .ok_or("current recovery coordinator is unavailable")?
    } else {
        retained
    };
    let verb = if stop_original {
        "recover-legacy-ui"
    } else if fresh.native_ui {
        "recover-ui"
    } else {
        "recover"
    };
    let mut command = recovery_command(&fresh.plan, &helper.path, verb, true)?;
    if start {
        command.arg("--start");
    }
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut receipt = RecoveryLaunchReceipt {
        version: 2,
        operation_id: fresh.record.operation_id.clone(),
        context: fresh.plan.context.clone(),
        process: RecoveryLaunchProcess::Launching,
        retained_helper_image: retained.clone(),
        coordinator: stop_original,
        helper_image: helper.clone(),
    };
    // Record intent before spawn. A crash in the spawn/receipt window remains
    // blocked rather than allowing an untracked second native consent window.
    write_recovery_receipt(&fresh.plan, &receipt)?;
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            receipt.process = RecoveryLaunchProcess::NoSpawn;
            write_recovery_receipt(&fresh.plan, &receipt)?;
            return Err(format!(
                "cannot open authenticated recovery helper: {error}"
            ));
        }
    };
    *spawned = true;
    receipt.process = RecoveryLaunchProcess::Spawned { pid: child.id() };
    let receipt_result = write_recovery_receipt(&fresh.plan, &receipt);
    let _ = sender.send(RecoveryEvent::Started {
        context: fresh.context.clone(),
        operation_id: fresh.record.operation_id.clone(),
        request,
        pid: child.id(),
    });
    let stdout = child
        .stdout
        .take()
        .ok_or("recovery stdout is unavailable")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("recovery stderr is unavailable")?;
    let out = std::thread::spawn(move || drain_recovery_output(stdout, 32768));
    let err = std::thread::spawn(move || drain_recovery_output(stderr, 8192));
    // Never kill or retry a launched recovery; its durable journal owns outcome.
    let status = child.wait().map_err(|e| e.to_string())?;
    *child_exited = true;
    let bytes = out.join().map_err(|_| "recovery output worker failed")??;
    let error = err.join().map_err(|_| "recovery error worker failed")??;
    receipt_result?;
    if !status.success() {
        return Err(format!(
            "recovery retained its operation evidence: {}",
            String::from_utf8_lossy(&error)
        ));
    }
    let reply: OperationRecord = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if reply.version != fresh.plan.version || reply.operation_id != fresh.record.operation_id {
        return Err("recovery reply does not match its operation".into());
    }
    // A successful transport is not update success. Poll the real journal and
    // prove both reservation scopes free before making Check available again.
    Ok(())
}

// Thaw only after an authenticated worker exit/no-spawn, no competing launcher
// or operation owner, and a ready response from the exact original kernel peer.
fn recovery_handoff_quiescent(projection: &RecoveryProjection) -> Result<bool, String> {
    let plan = &projection.plan;
    let _launcher = recovery_launch_lock(plan)?;
    let Some(_lease) = desktop_pet_update_coordinator::protocol::try_lock_operation(
        &plan.context.state_dir,
        &plan.operation_id,
    )?
    else {
        return Ok(false);
    };
    let (status, peer): (serde_json::Value, _) =
        desktop_pet_update_coordinator::native_runtime::exchange_observed(
            &plan.context.state_dir.join("control.sock"),
            &serde_json::json!({"version": 1, "command": "status"}),
            Duration::from_secs(3),
        )?;
    Ok(
        status.get("version").and_then(serde_json::Value::as_u64) == Some(1)
            && status.get("command").and_then(serde_json::Value::as_str) == Some("status")
            && ["ok", "running", "ready", "ui_ready", "control_ready"]
                .iter()
                .all(|key| status.get(*key).and_then(serde_json::Value::as_bool) == Some(true))
            && status.get("shutdown").and_then(serde_json::Value::as_bool) == Some(false)
            && status.get("pid").and_then(serde_json::Value::as_u64)
                == Some(std::process::id().into())
            && peer.incarnation().pid == std::process::id()
            && peer.path() == plan.running.path
            && status
                .get("executable_path")
                .and_then(serde_json::Value::as_str)
                == plan.running.path.to_str()
            && status.get("config_dir").and_then(serde_json::Value::as_str)
                == plan.context.config_dir.to_str()
            && status.get("state_dir").and_then(serde_json::Value::as_str)
                == plan.context.state_dir.to_str()
            && status
                .get("instance_id")
                .and_then(serde_json::Value::as_str)
                == Some(plan.context.instance_id.as_str())
            && status
                .get("running_sha256")
                .and_then(serde_json::Value::as_str)
                == Some(plan.running.sha256.as_str())
            && recovery_process_matches(std::process::id(), &plan.running)?,
    )
}

fn recovery_launch_lock(plan: &UpdatePlan) -> Result<std::fs::File, String> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(
            private_updates(&plan.context.state_dir)?
                .join(format!("{}.app-recovery.lock", plan.operation_id)),
        )
        .map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err("recovery launch lock is not private and owned".into());
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("recovery launch is already owned; rediscover its progress".into());
    }
    Ok(file)
}

fn recovery_process_scoped(
    pid: u32,
    helper: &desktop_pet_update_coordinator::ExecutableIdentity,
    plan: &UpdatePlan,
    coordinator: bool,
) -> Result<bool, String> {
    if !recovery_process_matches(pid, helper)? {
        return Ok(false);
    }
    #[cfg(target_os = "macos")]
    {
        let mut bytes = [0u8; 256 * 1024];
        let mut length = bytes.len();
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as i32];
        if unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                bytes.as_mut_ptr().cast(),
                &mut length,
                std::ptr::null_mut(),
                0,
            )
        } != 0
        {
            return Err("cannot authenticate recovery process arguments".into());
        }
        if length > bytes.len() {
            return Err("recovery process arguments exceeded the kernel bound".into());
        }
        let matches = recovery_arguments_match(&bytes[..length], helper, plan, coordinator)?;
        Ok(matches && recovery_process_matches(pid, helper)?)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (plan, coordinator);
        Err("recovery argument authentication requires macOS".into())
    }
}

fn recovery_arguments_match(
    bytes: &[u8],
    helper: &desktop_pet_update_coordinator::ExecutableIdentity,
    plan: &UpdatePlan,
    coordinator: bool,
) -> Result<bool, String> {
    use std::os::unix::ffi::OsStrExt;
    let count = i32::from_ne_bytes(
        bytes
            .get(..4)
            .ok_or("missing recovery argc")?
            .try_into()
            .map_err(|_| "invalid recovery argc")?,
    );
    if !(1..=16).contains(&count) {
        return Ok(false);
    }
    let payload = &bytes[4..];
    let executable_end = payload
        .iter()
        .position(|byte| *byte == 0)
        .ok_or("missing recovery executable terminator")?;
    let payload = &payload[executable_end..];
    let start = payload
        .iter()
        .position(|byte| *byte != 0)
        .ok_or("missing recovery argv")?;
    let mut remaining = &payload[start..];
    let mut args: [&[u8]; 16] = [&[]; 16];
    for arg in &mut args[..count as usize] {
        let end = remaining
            .iter()
            .position(|byte| *byte == 0)
            .ok_or("unterminated recovery argv")?;
        *arg = &remaining[..end];
        remaining = &remaining[end + 1..];
    }
    let args = &args[..count as usize];
    let verb_matches = if coordinator {
        args.get(1) == Some(&b"recover-legacy-ui".as_slice())
    } else {
        args.get(1)
            .is_some_and(|arg| [b"run".as_slice(), b"recover", b"recover-ui"].contains(arg))
    };
    Ok(verb_matches
        && args.first() == Some(&helper.path.as_os_str().as_bytes())
        && (args.len() == 6 || (!coordinator && args.len() == 7 && args[6] == b"--start"))
        && args.get(2) == Some(&b"--state-dir".as_slice())
        && args.get(3) == Some(&plan.context.state_dir.as_os_str().as_bytes())
        && args.get(4) == Some(&b"--operation-id".as_slice())
        && args.get(5) == Some(&plan.operation_id.as_bytes()))
}

fn recovery_process_matches(
    pid: u32,
    helper: &desktop_pet_update_coordinator::ExecutableIdentity,
) -> Result<bool, String> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Ok(false);
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut buffer = [0u8; 4096];
        let count = unsafe {
            libc::proc_pidpath(pid as i32, buffer.as_mut_ptr().cast(), buffer.len() as u32)
        };
        if count <= 0 {
            return Ok(false);
        }
        let end = buffer
            .iter()
            .position(|byte| *byte == 0)
            .ok_or("unterminated helper process path")?;
        let path = Path::new(std::ffi::OsStr::from_bytes(&buffer[..end]));
        Ok(path == helper.path
            && desktop_pet_update_coordinator::executable_identity(path)? == *helper)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = helper;
        Err("recovery process authentication requires macOS".into())
    }
}

fn focus_recovery_helper(
    pid: u32,
    helper: &desktop_pet_update_coordinator::ExecutableIdentity,
    plan: &UpdatePlan,
    coordinator: bool,
) -> Result<(), String> {
    if !recovery_process_scoped(pid, helper, plan, coordinator)? {
        return Err("running recovery helper identity could not be authenticated".into());
    }
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
        objc2::rc::autoreleasepool(|_| {
            let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32)
                .ok_or("authenticated helper has no native application")?;
            #[allow(deprecated)]
            let activated = app.activateWithOptions(
                NSApplicationActivationOptions::ActivateAllWindows
                    | NSApplicationActivationOptions::ActivateIgnoringOtherApps,
            );
            if activated {
                Ok(())
            } else {
                Err("recovery helper could not be brought forward".into())
            }
        })
    }
    #[cfg(not(target_os = "macos"))]
    Err("recovery helper activation requires macOS".into())
}

// Runs only on the check worker. A journal's terminal phase by itself is not
// reconciliation: verify its immutable plan, nonblocking helper lease and both
// installation/profile admission markers before retiring the runtime observation.
fn settlement_proof(
    context: &UpdateContext,
    snapshot: &CheckSnapshot,
    operation_id: &str,
) -> Result<SettledOperationProof, String> {
    if let Some(fresh_plan) = snapshot.plan.as_ref() {
        if fresh_plan.context != *context
            || fresh_plan.running != context.running
            || fresh_plan.origin != snapshot.origin
            || fresh_plan.operation_id == operation_id
        {
            return Err("fresh check plan is not for the running profile/source".into());
        }
    } else if context.running_origin.as_deref() != Some(&snapshot.origin) {
        // An up-to-date installation has no next update plan. Its checked source
        // must still match the startup capture before retiring prior ownership.
        return Err("fresh check source is not the captured running installation".into());
    }
    settled_operation_for_origin(context, &snapshot.origin, operation_id)
}

fn settled_operation_for_origin(
    context: &UpdateContext,
    origin: &InstallOrigin,
    operation_id: &str,
) -> Result<SettledOperationProof, String> {
    let _lease = lock_operation(&context.state_dir, operation_id)?;
    let observation = observe_app_operation(context, operation_id)?
        .ok_or("previous operation journal is missing")?;
    let preplan = !standalone_install::plan_path(&context.state_dir, operation_id)?
        .try_exists()
        .map_err(|e| e.to_string())?
        && matches!(&observation, OperationObservation::StandaloneInstall(_));
    if !preplan && observe_latest_operation(&context.state_dir)?.as_ref() != Some(&observation) {
        return Err("previous operation journal changed during admission".into());
    }
    let record = match observation {
        OperationObservation::Update(record) => record,
        OperationObservation::StandaloneInstall(record) => {
            standalone_settlement(context, &record)?;
            if !preplan
                && origin
                    != standalone_install::read_plan(&context.state_dir, operation_id)?.origin()
            {
                start_allowed(origin, &context.state_dir, None, None)?;
            }
            return Ok(SettledOperationProof::StandaloneInstall(record));
        }
    };
    if record.version == UPDATER_PROTOCOL {
        if !settled_terminal(&record) {
            return Err("previous operation is not a reconciled terminal outcome".into());
        }
        let prior_plan = read_plan(&context.state_dir, operation_id)?;
        if prior_plan.context.config_dir != context.config_dir {
            return Err("previous operation belongs to a different configuration profile".into());
        }
        start_allowed(&prior_plan.origin, &context.state_dir, None, None)?;
        if prior_plan.target_origin() != &prior_plan.origin {
            start_allowed(prior_plan.target_origin(), &context.state_dir, None, None)?;
        }
    } else {
        let scope = historical_operation_plan(&context.state_dir, &record)?;
        if scope.config_dir != context.config_dir {
            return Err("previous operation belongs to a different configuration profile".into());
        }
        desktop_pet_update_coordinator::protocol::historical_scope_settled(&scope)?;
    }
    if observe_latest_operation(&context.state_dir)?.as_ref()
        != Some(&OperationObservation::Update(record.clone()))
    {
        return Err("previous operation journal changed during admission".into());
    }
    start_allowed(origin, &context.state_dir, None, None)?;
    Ok(SettledOperationProof::Update(record))
}

fn completed_history_proof(
    context: &UpdateContext,
    observed: &OperationObservation,
) -> Result<CompletedHistoryProof, String> {
    let completed = match observed {
        OperationObservation::Update(record) => {
            record.phase == OperationPhase::Completed && record.installed && record.applied
        }
        OperationObservation::StandaloneInstall(record) => {
            record.phase == standalone_install::StandaloneInstallPhase::Completed
                && record.result.installed
                && match record.result.status {
                    InstallStatus::Applied => record.result.applied,
                    InstallStatus::InstalledStopped => {
                        !record.result.applied
                            && record.result.restart
                                == standalone_install::InstallRestart::NotRequested
                    }
                    _ => false,
                }
        }
    };
    if !completed {
        return Err("operation is not completed installed history".into());
    }
    let origin = context
        .running_origin
        .as_deref()
        .ok_or("running installation source is unavailable")?;
    let generation = desktop_pet_update_coordinator::protocol::stop_generation(&context.state_dir)?;
    let settled = settled_operation_for_origin(context, origin, observation_id(observed))?;
    let observation = match settled {
        SettledOperationProof::Update(record) => OperationObservation::Update(record),
        SettledOperationProof::StandaloneInstall(record) => {
            OperationObservation::StandaloneInstall(record)
        }
    };
    if &observation != observed
        || desktop_pet_update_coordinator::protocol::stop_generation(&context.state_dir)?
            != generation
    {
        return Err("completed history or Stop generation changed during admission".into());
    }
    Ok(CompletedHistoryProof {
        context: context.clone(),
        observation,
    })
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
    format!(
        "{} · {}",
        origin.manager_label(),
        serde_json::to_string(origin).unwrap_or_else(|error| error.to_string())
    )
}

fn now() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|time| time.as_secs())
}

fn stage_and_launch(plan: &UpdatePlan, on_waiting: impl FnOnce()) -> Result<(), LaunchFailure> {
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
    // The detached helper owns its journal after this process exits.
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
    // After spawn, ownership is determined by the journal; never kill on timeout.
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(read_ownership_ack(stdout));
    });
    await_ownership_ack(
        receiver,
        &plan.operation_id,
        Duration::from_secs(30),
        on_waiting,
    )
}

fn read_ownership_ack(stdout: impl Read) -> Result<OwnershipAck, String> {
    let mut line = String::new();
    let length = BufReader::new(stdout.take(4096))
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    if length == 0 || length >= 4096 {
        return Err("Coordinator ownership reply missing or oversized".into());
    }
    if !line.ends_with('\n') {
        return Err("Coordinator ownership reply ended before its final line".into());
    }
    serde_json::from_str(&line).map_err(|error| error.to_string())
}

fn await_ownership_ack(
    receiver: Receiver<Result<OwnershipAck, String>>,
    operation_id: &str,
    notice_after: Duration,
    on_waiting: impl FnOnce(),
) -> Result<(), LaunchFailure> {
    let reply = match receiver.recv_timeout(notice_after) {
        Ok(reply) => reply,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            on_waiting();
            receiver.recv().map_err(|_| {
                LaunchFailure::Spawned("Coordinator ownership channel disconnected".into())
            })?
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(LaunchFailure::Spawned(
                "Coordinator ownership channel disconnected".into(),
            ));
        }
    };
    let ack = reply.map_err(LaunchFailure::Spawned)?;
    if ack.protocol != UPDATER_PROTOCOL
        || ack.operation_id != operation_id
        || !ack.visible
        || !ack.prepared
        || ack.error.is_some()
    {
        return Err(LaunchFailure::Spawned(ack.error.unwrap_or_else(|| {
            "Coordinator did not confirm visible ownership and preparation".into()
        })));
    }
    Ok(())
}

/// Service-produced snapshots in Recover, Start, StoppedOriginal, Live, then
/// Reconciled order. Models own their data; no service or reservation is retained.
#[cfg(test)]
pub(crate) fn recovery_ui_regression_models(locale: UiLocale) -> [UpdateCardModel; 5] {
    [
        RecoveryDisposition::Recover,
        RecoveryDisposition::Start,
        RecoveryDisposition::RecoverStoppedOriginal,
        RecoveryDisposition::Live(123),
        RecoveryDisposition::Reconciled,
    ]
    .map(|disposition| tests::recovery_service(disposition).model(locale))
}

#[cfg(test)]
mod tests {
    use super::*;
    use desktop_pet_update_coordinator::{
        ExecutableIdentity, ReleaseCandidate, ReleaseChannel, UpdateAction,
    };
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    pub(super) fn recovery_service(disposition: RecoveryDisposition) -> AppUpdateService {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        let plan = local_check().plan.unwrap();
        let mut record = failed_record('a');
        record.operation_id = plan.operation_id.clone();
        service.context = Some(plan.context.clone());
        service.observed_operation_id = Some(record.operation_id.clone());
        service.operation = Some(record.clone());
        service.previous_outcome = Some(record.clone());
        service.recovery = Some(RecoveryProjection {
            context: plan.context.clone(),
            record,
            plan,
            helper: Some(ExecutableIdentity {
                path: "/isolated/retained-helper".into(),
                sha256: "a".repeat(64),
            }),
            coordinator: None,
            live_helper: None,
            native_ui: true,
            disposition,
        });
        service
    }

    #[test]
    fn recovery_capability_matches_only_the_exact_execution_disposition() {
        use UpdateCardPrimaryAction::{Recover, RecoverStart, RecoverStoppedOriginal};
        for (disposition, expected) in [
            (RecoveryDisposition::Recover, Some(Recover)),
            (RecoveryDisposition::Start, Some(RecoverStart)),
            (
                RecoveryDisposition::RecoverStoppedOriginal,
                Some(RecoverStoppedOriginal),
            ),
            (RecoveryDisposition::Live(123), None),
            (RecoveryDisposition::Reconciled, None),
        ] {
            let mut service = recovery_service(disposition);
            let model = service.model(UiLocale::En);
            for action in [Recover, RecoverStart, RecoverStoppedOriginal] {
                assert_eq!(service.recovery_admitted(action), Some(action) == expected);
                if Some(action) != expected {
                    match action {
                        Recover => service.recover(false),
                        RecoverStart => service.recover(true),
                        RecoverStoppedOriginal => {
                            assert!(service.recover_stopped_original().is_none());
                        }
                        _ => unreachable!(),
                    }
                    assert!(service.recovery_running.is_none());
                    assert!(service.recovery_observing.is_none());
                    assert_eq!(service.recovery_request, 0);
                }
            }
            assert_eq!(model.recovery_enabled, expected.is_some());
            assert_eq!(
                service.recovery_stop_plan().is_some(),
                expected == Some(RecoverStoppedOriginal)
            );
            if disposition == RecoveryDisposition::Live(123) {
                assert_eq!(
                    model.primary_action,
                    Some(UpdateCardPrimaryAction::ShowProgress)
                );
                assert!(model.busy);
            }
            assert!(!service.recovery_admitted(UpdateCardPrimaryAction::ShowProgress));
            if let Some(action) = expected {
                assert!(model.busy);
                assert_eq!(model.primary_action, Some(action));
                assert!(!model.check_enabled);
            } else {
                // Recovery execution never doubles as progress activation.
                service.recover(false);
                service.recover(true);
                assert!(service.recover_stopped_original().is_none());
                assert!(service.recovery_running.is_none());
                assert_eq!(service.recovery_request, 0);
            }
        }
    }

    #[test]
    fn recovery_execution_and_model_share_all_scope_guards() {
        let blockers: &[(&str, fn(&mut AppUpdateService))] = &[
            ("missing context", |s| s.context = None),
            ("migration", |s| s.migration_blocked = true),
            ("relocation", |s| s.relocation_busy = true),
            ("checking", |s| s.check_busy = true),
            ("launching", |s| s.launching = Some("a".repeat(32))),
            ("journal error", |s| {
                s.journal_error = Some("unreadable".into())
            }),
            ("standalone operation", |s| {
                s.standalone_operation = Some(standalone_observation(InstallStatus::Running))
            }),
            ("duplicate recovery", |s| {
                s.recovery_running = Some((
                    s.context.clone().unwrap(),
                    s.operation.as_ref().unwrap().operation_id.clone(),
                    7,
                    Some(123),
                ))
            }),
            ("unreleased handoff", |s| {
                s.recovery_handoff = Some((
                    s.context.clone().unwrap(),
                    s.operation.as_ref().unwrap().operation_id.clone(),
                    7,
                    false,
                ))
            }),
            ("stale observed ID", |s| {
                s.observed_operation_id = Some("f".repeat(32))
            }),
        ];
        for (name, block) in blockers {
            for disposition in [
                RecoveryDisposition::Recover,
                RecoveryDisposition::Start,
                RecoveryDisposition::RecoverStoppedOriginal,
            ] {
                let mut service = recovery_service(disposition);
                block(&mut service);
                for action in [
                    UpdateCardPrimaryAction::Recover,
                    UpdateCardPrimaryAction::RecoverStart,
                    UpdateCardPrimaryAction::RecoverStoppedOriginal,
                ] {
                    assert!(!service.recovery_admitted(action), "{name}: {action:?}");
                }
                assert!(!service.model(UiLocale::En).recovery_enabled, "{name}");
                if matches!(*name, "duplicate recovery" | "unreleased handoff") {
                    assert_eq!(
                        service.model(UiLocale::En).primary_action,
                        Some(UpdateCardPrimaryAction::ShowProgress)
                    );
                }
                assert!(service.recovery_stop_plan().is_none(), "{name}");
                let running = service.recovery_running.clone();
                let request = service.recovery_request;
                let revision = service.revision;
                service.recover(false);
                service.recover(true);
                assert!(service.recover_stopped_original().is_none(), "{name}");
                assert_eq!(service.recovery_running, running, "{name}");
                assert_eq!(service.recovery_request, request, "{name}");
                assert_eq!(service.revision, revision, "{name}");
                assert!(service.recovery_observing.is_none(), "{name}");
            }
        }
    }

    #[test]
    fn stale_projection_can_only_request_read_only_rediscovery() {
        for stale_context in [false, true] {
            let mut service = recovery_service(RecoveryDisposition::Start);
            let projection = service.recovery.as_mut().unwrap();
            if stale_context {
                projection.context.instance_id = "foreign-source".into();
            } else {
                projection.record.detail.push_str("; stale");
            }
            assert!(service.current_recovery().is_none());
            assert!(service.recovery_admitted(UpdateCardPrimaryAction::Recover));
            assert!(!service.recovery_admitted(UpdateCardPrimaryAction::RecoverStart));
            assert!(!service.recovery_admitted(UpdateCardPrimaryAction::RecoverStoppedOriginal));
            service.recover(true);
            assert!(service.recover_stopped_original().is_none());
            assert_eq!(service.recovery_request, 0);
            service.recover(false);
            assert!(service.recovery_running.is_none());
            let (context, id, _, record) = service.recovery_observing.as_ref().unwrap();
            assert_eq!(Some(context), service.context.as_ref());
            assert_eq!(Some(id), service.observed_operation_id.as_ref());
            assert_eq!(Some(record), service.operation.as_ref());
        }
    }

    #[test]
    fn projectionless_recovery_never_authorizes_start_or_unscoped_rediscovery() {
        let mut service = recovery_service(RecoveryDisposition::Recover);
        service.recovery = None;
        assert!(service.recovery_admitted(UpdateCardPrimaryAction::Recover));
        assert!(service.model(UiLocale::En).recovery_enabled);
        assert!(!service.recovery_admitted(UpdateCardPrimaryAction::RecoverStart));
        assert!(!service.recovery_admitted(UpdateCardPrimaryAction::RecoverStoppedOriginal));
        let revision = service.revision;
        service.recover(false);
        assert!(service.recovery_running.is_none());
        assert!(service.recovery_observing.is_some());
        assert_eq!(service.revision, revision.wrapping_add(1));
        let blockers: &[fn(&mut AppUpdateService)] = &[
            |s| s.operation = None,
            |s| s.operation.as_mut().unwrap().phase = OperationPhase::Completed,
            |s| s.operation.as_mut().unwrap().phase = OperationPhase::Preparing,
            |s| s.observed_operation_id = None,
            |s| s.observed_operation_id = Some("f".repeat(32)),
            |s| s.standalone_operation = Some(standalone_observation(InstallStatus::Failed)),
            |s| s.context = None,
            |s| s.journal_error = Some("unreadable".into()),
        ];
        for block in blockers {
            let mut service = recovery_service(RecoveryDisposition::Recover);
            service.recovery = None;
            block(&mut service);
            assert!(!service.recovery_admitted(UpdateCardPrimaryAction::Recover));
            service.recover(false);
            assert!(service.recovery_running.is_none());
            assert!(service.recovery_observing.is_none());
            assert_eq!(service.recovery_request, 0);
        }
    }

    #[test]
    fn retained_preparation_does_not_block_its_own_cleanup_or_unlock_installation() {
        let mut service = recovery_service(RecoveryDisposition::Recover);
        let plan = service.recovery.as_ref().unwrap().plan.clone();
        service.prepared_plan = Some(plan.clone());
        service.launch_plan = Some(plan);
        assert!(service.recovery_admitted(UpdateCardPrimaryAction::Recover));
        assert!(service.model(UiLocale::En).recovery_enabled);
        assert!(!service.settings_admitted());
        assert!(!service.check_admitted());
        assert!(service.plan().is_none());
        service.recovery.as_mut().unwrap().disposition = RecoveryDisposition::Reconciled;
        assert!(!service.check_admitted());
        assert!(service.begin_check().is_none());
        assert!(service.prepared_plan.is_some());
        assert!(service.launch_plan.is_some());
        assert!(!service.settings_admitted());
        assert!(service.plan().is_none());
    }

    #[test]
    fn released_handoff_allows_exact_recovery_without_treating_history_as_live_work() {
        let mut service = recovery_service(RecoveryDisposition::RecoverStoppedOriginal);
        service.recovery_handoff = Some((
            service.context.clone().unwrap(),
            service.operation.as_ref().unwrap().operation_id.clone(),
            7,
            true,
        ));
        assert!(service.recovery_admitted(UpdateCardPrimaryAction::RecoverStoppedOriginal));
        assert!(service.recovery_stop_plan().is_some());
        assert!(service.model(UiLocale::En).recovery_enabled);
    }

    #[test]
    fn stopped_original_offer_is_separate_from_new_helper_cleanup_and_start() {
        let mut service = recovery_service(RecoveryDisposition::Recover);
        assert!(service.recovery_stop_plan().is_none());
        assert!(service.recover_stopped_original().is_none());
        assert_eq!(service.recovery_request, 0);
        assert_eq!(
            service.recovery_action(),
            Some(UpdateCardPrimaryAction::Recover)
        );
        let projection = service.recovery.as_mut().unwrap();
        projection.native_ui = false;
        projection.disposition = RecoveryDisposition::RecoverStoppedOriginal;
        projection.coordinator = Some(ExecutableIdentity {
            path: "/isolated/current-helper".into(),
            sha256: "c".repeat(64),
        });
        let history = service.model(UiLocale::En).history;
        assert!(service.recovery_stop_plan().is_some());
        assert_eq!(
            service.recovery_action(),
            Some(UpdateCardPrimaryAction::RecoverStoppedOriginal),
        );
        service.recover(false);
        service.recover(true);
        assert!(service.recovery_running.is_none());
        assert_eq!(service.recovery_request, 0);
        assert_eq!(service.model(UiLocale::En).history, history);
        service.operation.as_mut().unwrap().installed = true;
        assert!(service.recovery_stop_plan().is_none());
        assert!(service.recover_stopped_original().is_none());
    }

    #[test]
    fn stopped_original_eligibility_requires_exact_no_spawn_failure_not_diagnostics() {
        let mut durable = failed_record('a');
        durable.detail = "legacy stale original; Stop approved; installation verified".into();
        assert!(recovery_stop_original_eligible(&durable, &durable));
        for (phase, fence, installed, applied) in [
            (
                OperationPhase::Unknown,
                ExecutionFence::NoSpawn,
                false,
                false,
            ),
            (
                OperationPhase::UserStopped,
                ExecutionFence::NoSpawn,
                false,
                false,
            ),
            (
                OperationPhase::Completed,
                ExecutionFence::NoSpawn,
                false,
                false,
            ),
            (
                OperationPhase::Failed,
                ExecutionFence::ManagerIntent,
                false,
                false,
            ),
            (
                OperationPhase::Failed,
                ExecutionFence::ManagerExited,
                false,
                false,
            ),
            (OperationPhase::Failed, ExecutionFence::NoSpawn, true, false),
            (OperationPhase::Failed, ExecutionFence::NoSpawn, false, true),
        ] {
            let mut inspected = durable.clone();
            inspected.phase = phase;
            inspected.execution_fence = fence;
            inspected.installed = installed;
            inspected.applied = applied;
            assert!(!recovery_stop_original_eligible(&inspected, &inspected));
            assert!(!recovery_stop_original_eligible(&durable, &inspected));
        }
        let mut inspected = durable.clone();
        inspected.detail.push_str("; changed observation");
        assert!(!recovery_stop_original_eligible(&durable, &inspected));
    }

    #[test]
    fn legacy_stop_ui_requires_its_own_explicit_integer_capability() {
        assert!(!recovery_legacy_ui_capability(&serde_json::json!({
            "ui_recovery_contract": 1
        }))
        .unwrap());
        assert!(recovery_legacy_ui_capability(&serde_json::json!({
            "ui_recovery_legacy_contract": 1
        }))
        .unwrap());
        for value in [
            serde_json::json!(true),
            serde_json::json!("1"),
            serde_json::json!(1.0),
            serde_json::json!(0),
            serde_json::json!(2),
            serde_json::Value::Null,
        ] {
            assert!(recovery_legacy_ui_capability(&serde_json::json!({
                "ui_recovery_legacy_contract": value
            }))
            .is_err());
        }
    }

    #[test]
    fn recovery_receipts_bind_retained_and_coordinator_images_separately() {
        let plan = local_check().plan.unwrap();
        let retained = ExecutableIdentity {
            path: "/isolated/retained-helper".into(),
            sha256: "a".repeat(64),
        };
        let coordinator = ExecutableIdentity {
            path: "/isolated/current-helper".into(),
            sha256: "b".repeat(64),
        };
        let receipt = RecoveryLaunchReceipt {
            version: 2,
            operation_id: plan.operation_id.clone(),
            context: plan.context.clone(),
            process: RecoveryLaunchProcess::Spawned { pid: 123 },
            retained_helper_image: retained.clone(),
            coordinator: true,
            helper_image: coordinator.clone(),
        };
        assert!(validate_recovery_receipt(&receipt, &plan, &retained, Some(&coordinator)).is_ok());
        assert!(validate_recovery_receipt(&receipt, &plan, &retained, None).is_err());
        let mut changed = receipt.clone();
        changed.coordinator = false;
        assert!(validate_recovery_receipt(&changed, &plan, &retained, Some(&coordinator)).is_err());
        changed = receipt.clone();
        changed.helper_image = retained.clone();
        assert!(validate_recovery_receipt(&changed, &plan, &retained, Some(&coordinator)).is_err());
        changed = receipt.clone();
        changed.retained_helper_image = coordinator.clone();
        assert!(validate_recovery_receipt(&changed, &plan, &retained, Some(&coordinator)).is_err());
        changed = receipt.clone();
        changed.operation_id = "f".repeat(32);
        assert!(validate_recovery_receipt(&changed, &plan, &retained, Some(&coordinator)).is_err());
        changed = receipt.clone();
        changed.context.instance_id = "other-original".into();
        assert!(validate_recovery_receipt(&changed, &plan, &retained, Some(&coordinator)).is_err());
        changed = receipt.clone();
        changed.process = RecoveryLaunchProcess::Spawned { pid: 0 };
        assert!(validate_recovery_receipt(&changed, &plan, &retained, Some(&coordinator)).is_err());
        changed.process = RecoveryLaunchProcess::Launching;
        assert!(validate_recovery_receipt(&changed, &plan, &retained, Some(&coordinator)).is_err());
        changed.process = RecoveryLaunchProcess::NoSpawn;
        assert!(validate_recovery_receipt(&changed, &plan, &retained, Some(&coordinator)).is_ok());
    }

    #[test]
    fn recovery_focus_requires_exact_process_image_verb_and_operation_arguments() {
        use std::os::unix::ffi::OsStrExt;
        let plan = local_check().plan.unwrap();
        let helper = ExecutableIdentity {
            path: "/isolated/current-helper".into(),
            sha256: "a".repeat(64),
        };
        let image = helper.path.as_os_str().as_bytes();
        let state = plan.context.state_dir.as_os_str().as_bytes();
        let id = plan.operation_id.as_bytes();
        let encode = |args: &[&[u8]]| {
            let mut bytes = (args.len() as i32).to_ne_bytes().to_vec();
            bytes.extend_from_slice(image);
            bytes.extend_from_slice(&[0, 0, 0]);
            for arg in args {
                bytes.extend_from_slice(arg);
                bytes.push(0);
            }
            bytes.extend_from_slice(b"UNTRUSTED_ENV=--operation-id other\0");
            bytes
        };
        let mut args: Vec<&[u8]> = vec![
            image,
            b"recover-legacy-ui",
            b"--state-dir",
            state,
            b"--operation-id",
            id,
        ];
        assert!(recovery_arguments_match(&encode(&args), &helper, &plan, true).unwrap());
        assert!(!recovery_arguments_match(&encode(&args), &helper, &plan, false).unwrap());
        args.push(b"--start");
        assert!(!recovery_arguments_match(&encode(&args), &helper, &plan, true).unwrap());
        args.pop();
        args[5] = b"another-operation";
        assert!(!recovery_arguments_match(&encode(&args), &helper, &plan, true).unwrap());
        args[5] = id;
        args[3] = b"/another/profile";
        assert!(!recovery_arguments_match(&encode(&args), &helper, &plan, true).unwrap());
        args[3] = state;
        args[0] = b"/another/helper";
        assert!(!recovery_arguments_match(&encode(&args), &helper, &plan, true).unwrap());
        args[0] = image;
        args[1] = b"run";
        assert!(recovery_arguments_match(&encode(&args), &helper, &plan, false).unwrap());
        assert!(!recovery_arguments_match(&encode(&args), &helper, &plan, true).unwrap());
        assert!(recovery_arguments_match(&[0, 1], &helper, &plan, true).is_err());
    }

    #[test]
    fn legacy_handoff_thaw_is_scoped_and_uncertainty_cannot_duplicate_or_claim_success() {
        let mut service = recovery_service(RecoveryDisposition::RecoverStoppedOriginal);
        let projection = service.recovery.as_ref().unwrap().clone();
        let context = projection.context.clone();
        let id = projection.record.operation_id.clone();
        let history = service.model(UiLocale::En).history;
        service.recovery_request = 7;
        service.recovery_handoff = Some((context.clone(), id.clone(), 7, false));
        let revision = service.revision;
        service.recovery_event(RecoveryEvent::HandoffReleased {
            context: context.clone(),
            operation_id: id.clone(),
            request: 6,
        });
        let mut foreign = context.clone();
        foreign.instance_id = "other-original".into();
        service.recovery_event(RecoveryEvent::HandoffReleased {
            context: foreign,
            operation_id: id.clone(),
            request: 7,
        });
        assert_eq!(service.revision, revision);
        assert!(service.legacy_recovery_handoff_active(&context, &id, 7));
        assert!(!service.legacy_recovery_handoff_releasable(&context, &id, 7));
        assert!(service.recover_stopped_original().is_none());
        service.recover(false);
        assert!(service.recovery_running.is_none());
        assert!(service.begin_check().is_none());
        assert_eq!(
            service.recovery_action(),
            Some(UpdateCardPrimaryAction::ShowProgress)
        );
        service.recovery_event(RecoveryEvent::HandoffReleased {
            context: context.clone(),
            operation_id: id.clone(),
            request: 7,
        });
        assert!(!service.legacy_recovery_handoff_active(&context, &id, 7));
        assert!(service.legacy_recovery_handoff_releasable(&context, &id, 7));
        assert!(!service.legacy_recovery_handoff_releasable(&context, &id, 6));
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Failed);
        assert_eq!(service.model(UiLocale::En).history, history);
    }

    #[test]
    fn late_focus_callback_cannot_overwrite_a_changed_request_or_journal() {
        let mut service = recovery_service(RecoveryDisposition::Live(123));
        let projection = service.recovery.as_ref().unwrap().clone();
        service.recovery_request = 8;
        let revision = service.revision;
        service.recovery_event(RecoveryEvent::Focused {
            context: projection.context.clone(),
            operation_id: projection.record.operation_id.clone(),
            request: 7,
            record: projection.record.clone(),
            result: Err("stale focus failure".into()),
        });
        assert_eq!(service.revision, revision);
        service
            .operation
            .as_mut()
            .unwrap()
            .detail
            .push_str("; changed observation");
        service.recovery_event(RecoveryEvent::Focused {
            context: projection.context,
            operation_id: projection.record.operation_id.clone(),
            request: 8,
            record: projection.record,
            result: Err("stale record failure".into()),
        });
        assert!(service.recovery_error.is_none());
        assert_eq!(service.revision, revision);
    }

    #[test]
    fn recovery_cleanup_keeps_failed_history_and_requires_proven_free_scopes_for_check() {
        let mut service = recovery_service(RecoveryDisposition::Recover);
        let history = service.model(UiLocale::En).history;
        assert!(service.begin_check().is_none());
        assert!(service.model(UiLocale::En).busy);
        assert!(!service.model(UiLocale::En).check_enabled);
        assert!(service.model(UiLocale::En).recovery_enabled);
        assert_eq!(
            service.model(UiLocale::En).primary_action,
            Some(UpdateCardPrimaryAction::Recover)
        );
        service.recovery.as_mut().unwrap().disposition = RecoveryDisposition::Reconciled;
        let model = service.model(UiLocale::En);
        assert_eq!(model.status, CheckStatus::Failed);
        assert_eq!(model.history, history);
        assert!(!model.busy);
        assert!(model.primary_action.is_none());
        assert!(!model.recovery_enabled);
        assert!(model.check_enabled);
        assert!(model.settings_locked);
        assert!(service.begin_check().is_some());
        assert!(!service.model(UiLocale::En).check_enabled);
        assert!(!service.model(UiLocale::En).recovery_enabled);
        assert!(service.plan().is_none());
    }

    #[test]
    fn check_capability_preserves_context_error_and_relocation_fencing() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        assert!(service.check_admitted());
        assert!(!service.model(UiLocale::En).check_enabled);
        assert!(service.begin_check().is_none());
        assert_eq!(
            service.problem.as_deref(),
            Some("Running app update context is unavailable")
        );
        service.context = Some(local_check().plan.unwrap().context);
        assert!(service.model(UiLocale::En).check_enabled);
        service.relocation_busy = true;
        assert!(!service.check_admitted());
        assert!(!service.model(UiLocale::En).check_enabled);
        assert!(service.begin_check().is_none());
        service.relocation_busy = false;
        service.migration_blocked = true;
        assert!(!service.check_admitted());
        assert!(!service.model(UiLocale::En).check_enabled);
        assert!(service.begin_check().is_none());
    }

    #[test]
    fn completed_history_requires_exact_clearance_then_a_settled_check_for_new_work() {
        let mut service = recovery_service(RecoveryDisposition::Recover);
        let mut projection = service.recovery.take().unwrap();
        projection.record.phase = OperationPhase::Completed;
        projection.record.execution_fence = ExecutionFence::ManagerExited;
        projection.record.installed = true;
        projection.record.applied = true;
        projection.disposition = RecoveryDisposition::Reconciled;
        service.operation = Some(projection.record.clone());
        service.previous_outcome = Some(projection.record.clone());
        let history = service.model(UiLocale::En).history;
        assert!(!service.model(UiLocale::En).busy);
        assert!(service.model(UiLocale::En).settings_locked);
        assert!(service.begin_check().is_none());
        assert!(service.plan().is_none());

        service.recovery_observing = Some((
            projection.context.clone(),
            projection.record.operation_id.clone(),
            1,
            projection.record.clone(),
        ));
        service.recovery_event(RecoveryEvent::Observed {
            context: projection.context.clone(),
            operation_id: projection.record.operation_id.clone(),
            request: 1,
            result: Ok(projection.clone()),
        });
        assert!(service.begin_check().is_some());
        service.checked(Ok(local_check()), None);
        assert!(service.plan().is_none());
        assert!(!service.settings_admitted());
        assert!(service.model(UiLocale::En).primary_action.is_none());
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::Update(projection.record)),
        );
        let model = service.model(UiLocale::En);
        assert!(!model.busy);
        assert!(service.settings_admitted());
        assert_eq!(model.primary_action, Some(UpdateCardPrimaryAction::Update));
        assert_eq!(model.history, history);
    }

    #[test]
    fn late_recovery_observation_cannot_regress_completed_or_retired_history() {
        let mut service = recovery_service(RecoveryDisposition::Recover);
        let mut projection = service.recovery.take().unwrap();
        projection.disposition = RecoveryDisposition::Reconciled;
        let id = projection.record.operation_id.clone();
        service.recovery_observing = Some((
            projection.context.clone(),
            id.clone(),
            1,
            projection.record.clone(),
        ));
        let mut completed = projection.record.clone();
        completed.phase = OperationPhase::Completed;
        completed.execution_fence = ExecutionFence::ManagerExited;
        completed.installed = true;
        completed.applied = true;
        service.operation = Some(completed.clone());
        service.previous_outcome = Some(completed.clone());
        service.recovery_event(RecoveryEvent::Observed {
            context: projection.context.clone(),
            operation_id: id.clone(),
            request: 1,
            result: Ok(projection.clone()),
        });
        assert_eq!(service.operation.as_ref(), Some(&completed));
        assert!(service.current_recovery().is_none());
        assert!(service.begin_check().is_none());
        assert!(service.recovery_journal_authority.is_none());

        service.recovery_observing =
            Some((projection.context.clone(), id.clone(), 2, completed.clone()));
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::Update(completed.clone())),
        );
        service.recovery_event(RecoveryEvent::Observed {
            context: projection.context.clone(),
            operation_id: id,
            request: 2,
            result: Ok(projection),
        });
        assert!(service.operation.is_none());
        assert!(service.current_recovery().is_none());
        assert_eq!(service.previous_outcome.as_ref(), Some(&completed));
        assert!(service.plan().is_some());
    }

    #[test]
    fn recovery_start_never_uses_detail_text_or_unknown_manager_as_authorization() {
        let mut durable = failed_record('b');
        durable.detail = "Installation verified; explicit recover --start required".into();
        durable.installed = true;
        durable.candidate = Some(ExecutableIdentity {
            path: "/isolated/candidate".into(),
            sha256: "b".repeat(64),
        });
        let inspected = durable.clone();
        assert!(recovery_start_eligible(&durable, &inspected, true));
        assert!(!recovery_start_eligible(&durable, &inspected, false));
        durable.execution_fence = ExecutionFence::ManagerIntent;
        assert!(!recovery_start_eligible(&durable, &inspected, true));
        durable.execution_fence = ExecutionFence::NoSpawn;
        durable.phase = OperationPhase::Unknown;
        assert!(!recovery_start_eligible(&durable, &inspected, true));
        durable.phase = OperationPhase::UserStopped;
        assert!(!recovery_start_eligible(&durable, &inspected, true));
        durable.phase = OperationPhase::Failed;
        durable.candidate = None;
        assert!(!recovery_start_eligible(&durable, &inspected, true));
    }

    #[test]
    fn recovery_actions_preserve_legacy_confirmation_and_reject_late_callbacks() {
        let mut service = recovery_service(RecoveryDisposition::Start);
        let projection = service.recovery.as_ref().unwrap().clone();
        assert_eq!(
            service.model(UiLocale::En).primary_action,
            Some(UpdateCardPrimaryAction::RecoverStart)
        );
        assert!(!service.recovery_start_requires_confirmation());
        service.recovery.as_mut().unwrap().native_ui = false;
        assert!(service.recovery_start_requires_confirmation());
        service.recovery_request = 7;
        service.recovery_running = Some((
            projection.context.clone(),
            projection.record.operation_id.clone(),
            7,
            None,
        ));
        service.recover(false);
        service.recover(true);
        assert_eq!(service.recovery_request, 7); // Duplicate clicks cannot launch work.
        assert_eq!(
            service.model(UiLocale::En).primary_action,
            Some(UpdateCardPrimaryAction::ShowProgress)
        );
        service.recovery_event(RecoveryEvent::Finished {
            context: projection.context.clone(),
            operation_id: projection.record.operation_id.clone(),
            request: 6,
            result: Err("late failure".into()),
        });
        assert!(service.recovery_running.is_some());
        assert!(service.recovery_error.is_none());
        let mut foreign = projection.context;
        foreign.instance_id = "other-instance".into();
        service.recovery_event(RecoveryEvent::Started {
            context: foreign,
            operation_id: projection.record.operation_id,
            request: 7,
            pid: 123,
        });
        assert!(service.recovery_running.as_ref().unwrap().3.is_none());
    }

    #[test]
    fn only_owned_recovery_can_advance_unknown_and_stop_remains_sticky() {
        let mut service = recovery_service(RecoveryDisposition::Recover);
        service.operation.as_mut().unwrap().phase = OperationPhase::Unknown;
        let id = service.operation.as_ref().unwrap().operation_id.clone();
        let mut next = service.operation.as_ref().unwrap().clone();
        next.phase = OperationPhase::Failed;
        service.update_journal(&id, Ok(Some(next.clone())), None);
        assert_eq!(
            service.operation.as_ref().unwrap().phase,
            OperationPhase::Unknown
        );
        service.recovery_journal_authority = Some((service.context.clone().unwrap(), id.clone()));
        service.update_journal(&id, Ok(Some(next.clone())), None);
        assert_eq!(
            service.operation.as_ref().unwrap().phase,
            OperationPhase::Failed
        );
        next.phase = OperationPhase::UserStopped;
        service.update_journal(&id, Ok(Some(next.clone())), None);
        service.recovery_journal_authority = Some((service.context.clone().unwrap(), id.clone()));
        next.phase = OperationPhase::Completed;
        next.installed = true;
        next.applied = true;
        service.update_journal(&id, Ok(Some(next)), None);
        assert_eq!(
            service.operation.as_ref().unwrap().phase,
            OperationPhase::UserStopped
        );
    }

    #[test]
    fn invalid_typed_marker_discovered_during_check_never_reuses_previous_candidate() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(root.path()).unwrap();
        let state = root.join("state");
        let config = root.join("config");
        for path in [&state, &config] {
            std::fs::create_dir(path).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let marker = private_updates(&state)
            .unwrap()
            .join("latest-managed-operation.json");
        let invalid = br#"{"version":1,"kind":"standalone_install","operation_id":"invalid"}"#;
        desktop_pet_update_coordinator::protocol::write_private_bytes(&marker, invalid, 0o600)
            .unwrap();
        let mut context = local_check().plan.unwrap().context;
        context.config_dir = config;
        context.state_dir = state;
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        service.context = Some(context.clone());
        service.checked(Ok(local_check_for_context(context)), None);
        assert!(service.plan().is_some());
        service.check_now();
        match service
            .receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
        {
            Event::Checked {
                request_id,
                policy_generation,
                result,
                settled,
            } => {
                assert!(result.is_err());
                assert!(settled.is_none());
                service.checked_event(request_id, policy_generation, result, settled);
            }
            _ => panic!("unexpected update event"),
        }
        assert!(service.plan().is_none());
        assert_eq!(std::fs::read(marker).unwrap(), invalid);
    }

    fn standalone_observation(status: InstallStatus) -> StandaloneInstallRecord {
        use desktop_pet_update_coordinator::standalone_install::{
            InstallRestart, StandaloneInstallPhase, StandaloneInstallResult,
        };
        StandaloneInstallRecord {
            version: 1,
            operation_id: "a".repeat(32),
            phase: if status == InstallStatus::Running {
                StandaloneInstallPhase::Replacing
            } else {
                StandaloneInstallPhase::Failed
            },
            result: StandaloneInstallResult {
                version: 1,
                operation_id: "a".repeat(32),
                state_dir: "/isolated/state".into(),
                status,
                installation: None,
                installed: false,
                applied: false,
                restart: InstallRestart::NotRequested,
                retained_bundle: None,
                error: None,
            },
        }
    }

    fn relocation_outcome_fixture(
        status: InstallStatus,
        preplan: bool,
    ) -> (
        tempfile::TempDir,
        standalone_relocation::RelocationRequest,
        StandaloneInstallRecord,
    ) {
        use desktop_pet_update_coordinator::{
            installation::*, native_runtime::ProfilePaths, standalone_install::*,
        };
        use sha2::{Digest, Sha256};
        use std::os::unix::fs::PermissionsExt;
        let temporary = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temporary.path()).unwrap();
        let state = root.join("state");
        let config = root.join("config");
        for path in [&state, &config] {
            std::fs::create_dir(path).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let home = os_home().unwrap();
        let origin = InstallOrigin::Standalone {
            bundle: home.join("Applications/DesktopPet.app"),
        };
        let executable = home.join("Applications/DesktopPet.app/Contents/MacOS/desktop-pet");
        let mut context = local_check().plan.unwrap().context;
        context.version = "0.4.13".into();
        context.executable = executable.clone();
        context.running = ExecutableIdentity {
            path: executable,
            sha256: "b".repeat(64),
        };
        context.running_origin = Some(Box::new(origin.clone()));
        context.config_dir = config;
        context.state_dir = state.clone();
        let profile = ProfilePaths {
            config_dir: context.config_dir.clone(),
            state_dir: state.clone(),
        };
        let parent = SettledRelocationParent {
            version: 1,
            operation_id: "d".repeat(32),
            plan_sha256: "e".repeat(64),
            generation: 0,
            candidate_image: context.running.clone(),
            source_closure_sha256: "f".repeat(64),
            profile: profile.clone(),
            result_sha256: "1".repeat(64),
        };
        let request = standalone_relocation::RelocationRequest {
            version: 1,
            context: context.clone(),
            parent: parent.clone(),
        };
        let id = "c".repeat(32);
        let operation = private_updates(&state)
            .unwrap()
            .join(format!("install-{id}.resources"));
        std::fs::create_dir(&operation).unwrap();
        std::fs::set_permissions(&operation, std::fs::Permissions::from_mode(0o700)).unwrap();
        let original = InstallationRecord {
            version: 1,
            executable: context.executable.clone(),
            kind: InstallationKind::Standalone,
            app_version: context.version.clone(),
            protocol: 4,
            observation_mode: 2,
            integration_api: 1,
            origin,
            image: context.running.clone(),
        };
        let helper = ExecutableIdentity {
            path: operation.join("helper"),
            sha256: "2".repeat(64),
        };
        let source = InstallSource::Running {
            instance: StandaloneOriginalInstance {
                pid: 42,
                start_sec: 1,
                start_usec: 0,
                instance_id: context.instance_id.clone(),
                image: context.running.clone(),
            },
            context: context.clone(),
        };
        let purpose = StandaloneInstallPurpose::PrivateRelocation {
            target: InstallOrigin::Standalone {
                bundle: standalone_bundle().unwrap(),
            },
            parent,
            source_closure_sha256: "f".repeat(64),
            tern: None,
        };
        let install_request = InstallRequest {
            version: 1,
            staged_app: operation.join("DesktopPet.app"),
            kind: InstallationKind::Standalone,
            source_root: None,
        };
        let launch = serde_json::to_vec(&serde_json::json!({
            "version":1, "operation_id":id, "original":original, "profile":profile,
            "helper":helper, "request":install_request, "source":source, "purpose":purpose,
            "locale":"en", "original_evidence": {
                "sidecar":"captured signed deployment", "locator":null,
                "image_device":1, "image_inode":2, "bundle_device":1, "bundle_inode":3
            },
        }))
        .unwrap();
        let persist = |path: &Path, bytes: &[u8]| {
            desktop_pet_update_coordinator::protocol::write_private_atomic_new(path, bytes, 0o600)
                .unwrap();
        };
        persist(&operation.join("launch.json"), &launch);
        persist(
            &operation.join("relocation-request.json"),
            &serde_json::to_vec(&request).unwrap(),
        );
        let mut record = standalone_observation(status);
        record.operation_id = id.clone();
        record.result.operation_id = id.clone();
        record.result.state_dir = state.clone();
        let admission = StandaloneAdmissionRecord {
            version: 1,
            operation_id: id.clone(),
            launch_sha256: format!("{:x}", Sha256::digest(&launch)),
            original_stop_generation: 0,
            phase: StandaloneAdmissionPhase::Settled,
            result: Some(record.result.clone()),
        };
        write_admission(&state, &admission).unwrap();
        if preplan {
            persist(
                &operation.join("interrupted-result.json"),
                &serde_json::to_vec(&record.result).unwrap(),
            );
        } else {
            let plan = StandaloneInstallPlan {
                version: 2,
                purpose,
                operation_id: id.clone(),
                original,
                candidate_bundle: install_request.staged_app.clone(),
                candidate_image: ExecutableIdentity {
                    path: install_request
                        .staged_app
                        .join("Contents/MacOS/desktop-pet"),
                    sha256: context.running.sha256.clone(),
                },
                candidate_version: context.version.clone(),
                helper_image: helper,
                profile,
                source,
                original_stop_generation: 0,
                control_token: "3".repeat(64),
            };
            plan.validate().unwrap();
            // This is read-only journal evidence, not a signed-app execution fixture.
            persist(
                &plan_path(&state, &id).unwrap(),
                &serde_json::to_vec(&plan).unwrap(),
            );
            write_record(&state, &record).unwrap();
        }
        let old_offer = RelocationOffer {
            version: 1,
            parent: request.parent.clone(),
            operation_id: "a".repeat(32),
        };
        persist(
            &relocation_offer_path(&state, &request.parent.operation_id).unwrap(),
            &serde_json::to_vec(&old_offer).unwrap(),
        );
        (temporary, request, record)
    }

    #[test]
    fn relocation_outcome_observes_manual_retry_not_immutable_automatic_offer() {
        for status in [InstallStatus::Cancelled, InstallStatus::Failed] {
            for preplan in [false, true] {
                let (_temporary, request, record) = relocation_outcome_fixture(status, preplan);
                let old = "a".repeat(32);
                let id = authenticate_relocation_outcome(
                    &request,
                    &[old.clone()],
                    &serde_json::to_vec(&record.result).unwrap(),
                )
                .unwrap();
                assert_eq!(id, record.operation_id);
                let mut service =
                    AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
                service.context = Some(request.context.clone());
                service.latest_journal(Ok(Some(OperationObservation::StandaloneInstall(
                    standalone_observation(InstallStatus::Cancelled),
                ))));
                service.relocation_finished(request.context.clone(), id.clone(), Ok(()));
                match service
                    .receiver
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                {
                    Event::Journal {
                        operation_id,
                        result,
                        prepared_recovery,
                        completed_history,
                    } => {
                        assert_eq!(operation_id, id);
                        service.journal(
                            &operation_id,
                            result,
                            prepared_recovery,
                            completed_history,
                        );
                    }
                    _ => panic!("expected exact retry journal"),
                }
                assert_eq!(service.standalone_operation.as_ref(), Some(&record));
                let mut snapshot = local_check_for_context(request.context.clone());
                snapshot.origin = *request.context.running_origin.clone().unwrap();
                snapshot.status = CheckStatus::Current;
                snapshot.plan = None;
                service.checked(
                    Ok(snapshot),
                    Some(SettledOperationProof::StandaloneInstall(record.clone())),
                );
                assert!(service.standalone_operation.is_none());
                assert_eq!(
                    service.model(UiLocale::En).relocation_available,
                    !desktop_pet_update_coordinator::identity::LEGACY,
                );
                assert_eq!(
                    standalone_install::read_relocation_offer(
                        &request.context.state_dir,
                        &request.parent.operation_id
                    )
                    .unwrap()
                    .unwrap()
                    .operation_id,
                    old
                );
            }
        }
    }

    #[test]
    fn relocation_outcome_refuses_stale_malformed_and_mismatched_evidence() {
        let (_temporary, request, record) =
            relocation_outcome_fixture(InstallStatus::Cancelled, false);
        let bytes = serde_json::to_vec(&record.result).unwrap();
        assert!(
            authenticate_relocation_outcome(&request, &[record.operation_id.clone()], &bytes)
                .is_err()
        );
        for invalid in [
            b"not json".as_slice(),
            br#"{"version":1}"#.as_slice(),
            &vec![b' '; 32769],
        ] {
            assert!(authenticate_relocation_outcome(&request, &[], invalid).is_err());
        }
        let mut stale = request.clone();
        stale.context.instance_id.push_str("-stale");
        assert!(authenticate_relocation_outcome(&stale, &[], &bytes).is_err());
        stale = request.clone();
        stale.parent.result_sha256 = "4".repeat(64);
        assert!(authenticate_relocation_outcome(&stale, &[], &bytes).is_err());
        for change in ["profile", "operation", "result"] {
            let mut wrong = record.result.clone();
            match change {
                "profile" => wrong.state_dir = "/foreign/state".into(),
                "operation" => wrong.operation_id = "b".repeat(32),
                "result" => wrong.status = InstallStatus::Failed,
                _ => unreachable!(),
            }
            assert!(authenticate_relocation_outcome(
                &request,
                &[],
                &serde_json::to_vec(&wrong).unwrap()
            )
            .is_err());
        }
        let mut admission =
            standalone_install::read_admission(&request.context.state_dir, &record.operation_id)
                .unwrap()
                .unwrap();
        admission.launch_sha256 = "5".repeat(64);
        desktop_pet_update_coordinator::protocol::write_private_bytes(
            &standalone_install::admission_path(&request.context.state_dir, &record.operation_id)
                .unwrap(),
            &serde_json::to_vec(&admission).unwrap(),
            0o600,
        )
        .unwrap();
        assert!(authenticate_relocation_outcome(&request, &[], &bytes).is_err());
    }

    #[test]
    fn relocation_outcome_rejects_substituted_launch_journal_and_unsettled_preplan() {
        use sha2::{Digest, Sha256};
        for change in [
            "source",
            "profile",
            "journal",
            "preplan_unsettled",
            "preplan_result",
        ] {
            let preplan = change.starts_with("preplan");
            let (_temporary, request, record) =
                relocation_outcome_fixture(InstallStatus::Failed, preplan);
            let state = &request.context.state_dir;
            let id = &record.operation_id;
            let operation = state
                .join(desktop_pet_update_coordinator::identity::UPDATES_DIRECTORY)
                .join(format!("install-{id}.resources"));
            let persist = |path: &Path, bytes: &[u8]| {
                desktop_pet_update_coordinator::protocol::write_private_bytes(path, bytes, 0o600)
                    .unwrap();
            };
            match change {
                "source" | "profile" => {
                    let launch_path = operation.join("launch.json");
                    let mut launch: serde_json::Value = serde_json::from_slice(
                        &desktop_pet_update_coordinator::owned_private_file(&launch_path, 196608)
                            .unwrap(),
                    )
                    .unwrap();
                    if change == "source" {
                        launch["source"]["context"]["instance_id"] = "substituted-source".into();
                    } else {
                        launch["profile"]["config_dir"] = "/foreign/config".into();
                    }
                    let bytes = serde_json::to_vec(&launch).unwrap();
                    persist(&launch_path, &bytes);
                    let mut admission = standalone_install::read_admission(state, id)
                        .unwrap()
                        .unwrap();
                    admission.launch_sha256 = format!("{:x}", Sha256::digest(&bytes));
                    persist(
                        &standalone_install::admission_path(state, id).unwrap(),
                        &serde_json::to_vec(&admission).unwrap(),
                    );
                }
                "journal" => {
                    let mut changed = record.clone();
                    changed.result.state_dir = "/foreign/state".into();
                    persist(
                        &standalone_install::record_path(state, id).unwrap(),
                        &serde_json::to_vec(&changed).unwrap(),
                    );
                }
                "preplan_unsettled" => {
                    let mut admission = standalone_install::read_admission(state, id)
                        .unwrap()
                        .unwrap();
                    admission.phase = standalone_install::StandaloneAdmissionPhase::Presenting;
                    admission.result = None;
                    persist(
                        &standalone_install::admission_path(state, id).unwrap(),
                        &serde_json::to_vec(&admission).unwrap(),
                    );
                }
                "preplan_result" => {
                    let mut changed = record.result.clone();
                    changed.status = InstallStatus::Cancelled;
                    persist(
                        &operation.join("interrupted-result.json"),
                        &serde_json::to_vec(&changed).unwrap(),
                    );
                }
                _ => unreachable!(),
            }
            assert!(
                authenticate_relocation_outcome(
                    &request,
                    &[],
                    &serde_json::to_vec(&record.result).unwrap()
                )
                .is_err(),
                "{change}"
            );
        }
    }

    #[test]
    fn installed_stopped_history_after_independent_start_is_not_updater_application_authority() {
        let mut service = recovery_service(RecoveryDisposition::Recover);
        let mut prior = service.operation.clone().unwrap();
        prior.phase = OperationPhase::Completed;
        prior.execution_fence = ExecutionFence::ManagerExited;
        prior.installed = true;
        prior.applied = true;
        service.operation = Some(prior.clone());
        service.previous_outcome = Some(prior.clone());
        let mut context = service.context.clone().unwrap();
        context.version = "0.4.17-beta.9".into();
        context.instance_id.push_str("-ordinary-start");
        context.running.sha256 = "9".repeat(64);
        service.context = Some(context.clone());
        service.recovery_error =
            Some("older completed operation recovery observation failed".into());
        service.recovery_observing = Some((
            context.clone(),
            prior.operation_id.clone(),
            1,
            prior.clone(),
        ));
        let mut installed = standalone_observation(InstallStatus::InstalledStopped);
        installed.phase = standalone_install::StandaloneInstallPhase::Completed;
        installed.result.installed = true;
        let observation = OperationObservation::StandaloneInstall(installed.clone());
        service.latest_journal(Ok(Some(observation.clone())));
        assert!(service.operation.is_none());
        assert_eq!(service.previous_outcome.as_ref(), Some(&prior));
        assert!(service.recovery_error.is_none());
        assert!(service.recovery_observing.is_none());
        assert!(service.current_recovery().is_none());
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Installed);
        assert!(!service.settings_admitted());

        let mut foreign = context.clone();
        foreign.instance_id.push_str("-foreign");
        service.journal(
            &installed.operation_id,
            Ok(Some(observation.clone())),
            None,
            Some(CompletedHistoryProof {
                context: foreign,
                observation: observation.clone(),
            }),
        );
        assert!(!service.completed_history_admitted());
        assert!(!service.settings_admitted());

        // The production worker constructs this only after
        // verify_standalone_settlement authenticates replacement evidence,
        // the current ordinary Start fence, profile, generation and free scopes.
        service.journal(
            &installed.operation_id,
            Ok(Some(observation.clone())),
            None,
            Some(CompletedHistoryProof {
                context: context.clone(),
                observation,
            }),
        );
        assert!(service.completed_history_admitted());
        assert!(service.settings_admitted());
        assert_eq!(service.standalone_operation.as_ref(), Some(&installed));
        assert!(
            !service
                .standalone_operation
                .as_ref()
                .unwrap()
                .result
                .applied
        );
        assert!(service.prepared_plan.is_none());
        assert!(service.plan().is_none());
        for locale in [UiLocale::En, UiLocale::Ko] {
            let model = service.model(locale);
            assert_eq!(model.status, CheckStatus::NotChecked);
            assert_eq!(model.running_version, context.version);
            assert_eq!(
                model.detail,
                text(locale, Message::AppUpdateRecoveryComplete)
            );
            assert!(model.history.contains("InstalledStopped"));
            assert!(model.primary_action.is_none());
            assert!(!model.recovery_enabled);
        }
        let history = service.model(UiLocale::En).history;
        let (request, generation, _) = service.begin_check().unwrap();
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Checking);
        let mut snapshot = local_check_for_context(context);
        snapshot.status = CheckStatus::Current;
        snapshot.plan = None;
        service.checked_event(request, generation, Ok(snapshot.clone()), None);
        assert!(service.standalone_operation.is_some());
        assert!(service.plan().is_none());
        service.checked(
            Ok(snapshot),
            Some(SettledOperationProof::StandaloneInstall(installed.clone())),
        );
        assert!(service.standalone_operation.is_none());
        assert_eq!(
            service.previous_standalone_outcome.as_ref(),
            Some(&installed)
        );
        assert!(
            !service
                .previous_standalone_outcome
                .as_ref()
                .unwrap()
                .result
                .applied
        );
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Current);
        assert_eq!(service.model(UiLocale::En).history, history);
    }

    #[test]
    fn standalone_observation_is_not_normal_preparation_or_restart_authority() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        service.context = Some(local_check().plan.unwrap().context);
        let active = standalone_observation(InstallStatus::Running);
        service.latest_journal(Ok(Some(OperationObservation::StandaloneInstall(
            active.clone(),
        ))));
        assert!(service.journal_error.is_none());
        assert!(service.operation.is_none());
        assert!(service.prepared_plan.is_none());
        assert!(service.plan().is_none());
        assert!(!service.settings_admitted());
        assert!(service.begin_check().is_none());
        let failed = standalone_observation(InstallStatus::Failed);
        assert!(service
            .journal(
                &failed.operation_id,
                Ok(Some(OperationObservation::StandaloneInstall(
                    failed.clone()
                ))),
                None,
                None,
            )
            .is_none());
        assert!(service.operation.is_none());
        assert!(service.prepared_plan.is_none());
        assert!(!service.settings_admitted());
        assert!(service.begin_check().is_some());
        service.check_busy = false;
        service.checked(Ok(local_check()), None);
        assert!(
            !service.settings_admitted(),
            "terminal phase alone cannot release ownership"
        );
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::StandaloneInstall(failed)),
        );
        assert!(service.settings_admitted());
        assert!(service.previous_standalone_outcome.is_some());
        assert!(service.operation.is_none());
    }

    #[test]
    fn active_standalone_cannot_be_replaced_by_normal_history_or_normal_recovery() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        let active = standalone_observation(InstallStatus::Running);
        service.latest_journal(Ok(Some(OperationObservation::StandaloneInstall(
            active.clone(),
        ))));
        let normal = failed_record('b');
        service.latest_journal(Ok(Some(OperationObservation::Update(normal.clone()))));
        assert_eq!(service.standalone_operation.as_ref(), Some(&active));
        assert!(service.operation.is_none());
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::Update(normal)),
        );
        assert_eq!(service.standalone_operation.as_ref(), Some(&active));
        assert!(!service.settings_admitted());
    }
    #[test]
    fn terminal_standalone_history_yields_to_new_active_normal_operation() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        let prior = standalone_observation(InstallStatus::Failed);
        service.latest_journal(Ok(Some(OperationObservation::StandaloneInstall(
            prior.clone(),
        ))));
        let mut next = failed_record('b');
        next.phase = OperationPhase::Installing;
        service.latest_journal(Ok(Some(OperationObservation::Update(next.clone()))));
        assert_eq!(service.operation.as_ref(), Some(&next));
        assert!(service.standalone_operation.is_none());
        assert_eq!(service.previous_standalone_outcome.as_ref(), Some(&prior));
        assert_eq!(
            service.observed_operation_id.as_deref(),
            Some(next.operation_id.as_str())
        );
        assert!(service.begin_check().is_none());
    }

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
            state_dir: PathBuf::from("/isolated/state"),
            #[cfg(desktop_pet_legacy)]
            host_plugin_config_dir: None,
            #[cfg(desktop_pet_legacy)]
            herdr_socket: PathBuf::from("/isolated/herdr.sock"),
            running: running.clone(),
            running_origin: None,
            assets_override: None,
            environment: BTreeMap::new(),
            locale: "en".into(),
        };
        let origin = InstallOrigin::Source { root };
        CheckSnapshot {
            status: CheckStatus::Local,
            policy: UpdatePolicy::StableOnly,
            release: None,
            origin: origin.clone(),
            installed_version: Some("0.2.0".into()),
            observed_revision: None,
            detail: "rebuild on-disk sources".into(),
            plan: Some(UpdatePlan {
                version: UPDATER_PROTOCOL,
                operation_id: "e".repeat(32),
                context,
                origin,
                action: UpdateAction::SourceRebuild,
                running,
                candidate: None,
                target: None,
            }),
            checked_at: Some(42),
        }
    }

    fn local_check_for_context(context: UpdateContext) -> CheckSnapshot {
        let mut snapshot = local_check();
        let plan = snapshot.plan.as_mut().unwrap();
        plan.running = context.running.clone();
        plan.context = context;
        snapshot
    }

    fn admitted_card_service(policy: UpdatePolicy) -> AppUpdateService {
        // Keep this fixture in memory: startup workers must not inspect a real profile.
        let mut service = AppUpdateService::new(None, false, policy, None);
        let context = local_check().plan.unwrap().context;
        service.context = Some(context.clone());
        service.migration_history_checked(&context, true);
        service
    }

    fn completed_history_model(service: &AppUpdateService, locale: UiLocale) -> UpdateCardModel {
        let before = service.model(locale);
        let mut after = before.clone();
        let operation_id = "c".repeat(32);
        after.retain_completed_migration(&operation_id, locale);
        assert!(!after.migration_visible);
        assert!(!after.migration_available);
        assert!(after.history.starts_with(&before.history));
        assert!(after.history[before.history.len()..].contains(&operation_id));
        assert_eq!(after.status, before.status);
        assert_eq!(after.status_text, before.status_text);
        assert_eq!(after.detail, before.detail);
        assert_eq!(after.action, before.action);
        assert_eq!(after.primary_action, before.primary_action);
        assert_eq!(after.release, before.release);
        assert_eq!(after.download_url, before.download_url);
        assert_eq!(after.busy, before.busy);
        assert_eq!(after.settings_locked, before.settings_locked);
        assert_eq!(after.auto_check, before.auto_check);
        assert_eq!(after.receive_beta_updates, before.receive_beta_updates);
        assert_eq!(after.running_version, before.running_version);
        assert_eq!(after.source, before.source);
        assert_eq!(after.diagnostics, before.diagnostics);
        assert_eq!(after.relocation_summary, before.relocation_summary);
        assert_eq!(after.relocation_detail, before.relocation_detail);
        assert_eq!(after.last_checked, before.last_checked);
        assert_eq!(after.migration_title, before.migration_title);
        after
    }

    #[test]
    fn completed_migration_history_preserves_live_service_states_and_actions() {
        for locale in [UiLocale::En, UiLocale::Ko] {
            let mut service = admitted_card_service(UpdatePolicy::StableOnly);
            service.previous_outcome = Some(failed_record('a'));
            for status in [
                CheckStatus::Current,
                CheckStatus::Available,
                CheckStatus::Installed,
            ] {
                let (request, generation, context) = service.begin_check().unwrap();
                let checking = completed_history_model(&service, locale);
                assert_eq!(checking.status, CheckStatus::Checking);
                assert!(checking.busy);
                assert!(checking.settings_locked);
                assert!(checking.action.is_none());
                assert!(checking.primary_action.is_none());

                let mut snapshot = local_check_for_context(context);
                snapshot.status = status;
                if status != CheckStatus::Available {
                    snapshot.plan = None;
                }
                service.checked_event(request, generation, Ok(snapshot), None);
                let model = completed_history_model(&service, locale);
                assert_eq!(model.status, status);
                assert!(!model.busy);
                assert!(!model.settings_locked);
                if status == CheckStatus::Available {
                    assert_eq!(model.action, Some(UpdateAction::SourceRebuild));
                    assert_eq!(model.primary_action, Some(UpdateCardPrimaryAction::Update));
                } else {
                    assert!(model.action.is_none());
                    assert!(model.primary_action.is_none());
                }
            }
            let (request, generation, _) = service.begin_check().unwrap();
            service.checked_event(
                request,
                generation,
                Err("check transport failed".into()),
                None,
            );
            let failed = completed_history_model(&service, locale);
            assert_eq!(failed.status, CheckStatus::Failed);
            assert!(!failed.busy);
            assert!(failed.action.is_none());
            assert!(failed.primary_action.is_none());
        }
    }

    #[cfg(not(desktop_pet_legacy))]
    fn manual_beta_check(context: UpdateContext) -> CheckSnapshot {
        let mut snapshot = local_check_for_context(context);
        snapshot.status = CheckStatus::Unsupported;
        snapshot.origin = InstallOrigin::Manual {
            locator: "/isolated/manual/DesktopPet.app/Contents/MacOS/desktop-pet".into(),
        };
        snapshot.policy = UpdatePolicy::StableAndBeta;
        snapshot.plan = None;
        let asset = "DesktopPet-v0.4.13-beta.2-macos-arm64.tar.gz";
        let base = "https://github.com/hanbong5938/desktop-pet/releases/download/v0.4.13-beta.2/";
        let release = ReleaseCandidate {
            version: "0.4.13-beta.2".into(),
            channel: ReleaseChannel::Beta,
            tag: "v0.4.13-beta.2".into(),
            release_id: 42,
            asset_name: asset.into(),
            archive_url: format!("{base}{asset}"),
            archive_size: 1234,
            sha256: "a".repeat(64),
            checksum_url: format!("{base}{asset}.sha256"),
        };
        release.validate().unwrap();
        snapshot.release = Some(release);
        snapshot
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn completed_migration_does_not_authorize_manual_installation_updates() {
        let mut service = admitted_card_service(UpdatePolicy::StableAndBeta);
        service.context.as_mut().unwrap().version = "0.4.13-beta.1".into();
        let (request, generation, context) = service.begin_check().unwrap();
        let snapshot = manual_beta_check(context);
        let release = snapshot.release.clone().unwrap();
        assert!(snapshot.plan.is_none());
        service.checked_event(request, generation, Ok(snapshot), None);
        for locale in [UiLocale::En, UiLocale::Ko] {
            let model = completed_history_model(&service, locale);
            assert_eq!(model.status, CheckStatus::Unsupported);
            assert_eq!(model.running_version, "0.4.13-beta.1");
            assert_eq!(model.release.as_ref(), Some(&release));
            assert_eq!(model.release.as_ref().unwrap().tag, "v0.4.13-beta.2");
            assert!(model.receive_beta_updates);
            assert!(model.action.is_none());
            assert!(model.primary_action.is_none());
            assert!(!model.busy);
            assert!(!model.settings_locked);
        }
        assert!(service.plan().is_none());
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn completed_history_cannot_resurrect_beta_after_error_or_policy_generation_change() {
        let mut service = admitted_card_service(UpdatePolicy::StableAndBeta);
        let (request, generation, context) = service.begin_check().unwrap();
        let beta = manual_beta_check(context);
        service.checked_event(request, generation, Ok(beta.clone()), None);
        assert!(completed_history_model(&service, UiLocale::En)
            .download_url
            .is_some());

        let assert_hidden = |service: &AppUpdateService, status| {
            let model = completed_history_model(service, UiLocale::En);
            assert_eq!(model.status, status);
            assert!(model.release.is_none());
            assert!(model.download_url.is_none());
            assert!(model.action.is_none());
            assert!(model.primary_action.is_none());
        };
        let (request, generation, _) = service.begin_check().unwrap();
        assert_hidden(&service, CheckStatus::Checking);
        service.checked_event(request, generation, Err("offline".into()), None);
        assert_hidden(&service, CheckStatus::Failed);

        let (request, generation, _) = service.begin_check().unwrap();
        assert_hidden(&service, CheckStatus::Checking);
        service.checked_event(request, generation, Ok(beta.clone()), None);
        assert!(completed_history_model(&service, UiLocale::En)
            .download_url
            .is_some());
        service.change_policy(UpdatePolicy::StableOnly, None);
        assert_hidden(&service, CheckStatus::NotChecked);
        let (old_request, old_generation, _) = service.begin_check().unwrap();
        let mut stable = beta.clone();
        stable.policy = UpdatePolicy::StableOnly;
        stable.status = CheckStatus::Current;
        stable.release = None;
        service.checked_event(old_request, old_generation, Ok(stable.clone()), None);
        assert_hidden(&service, CheckStatus::Current);
        service.change_policy(UpdatePolicy::StableAndBeta, None);
        assert_hidden(&service, CheckStatus::NotChecked);
        let (request, generation, _) = service.begin_check().unwrap();
        assert_ne!(old_generation, generation);
        assert_eq!(
            service.checked_event(old_request, old_generation, Ok(stable), None),
            None
        );
        assert_hidden(&service, CheckStatus::Checking);
        service.checked_event(request, generation, Ok(beta), None);
        assert!(completed_history_model(&service, UiLocale::En)
            .download_url
            .is_some());
    }

    #[test]
    fn completed_history_cannot_override_invalid_initial_history_admission() {
        let mut service = admitted_card_service(UpdatePolicy::StableOnly);
        let context = service.context.clone().unwrap();
        service.migration_history_checked(&context, false);
        service.set_problem("migration journal does not match immutable evidence".into());
        let model = completed_history_model(&service, UiLocale::En);
        assert_eq!(model.status, CheckStatus::Failed);
        assert!(model.busy);
        assert!(model.settings_locked);
        assert!(model.action.is_none());
        assert!(model.primary_action.is_none());
        assert!(service.begin_check().is_none());
        assert!(!service.settings_admitted());
        service.migration_history_checked(&context, true);
        assert!(service.begin_check().is_some());
        assert_eq!(
            completed_history_model(&service, UiLocale::En).status,
            CheckStatus::Checking
        );
    }

    #[test]
    fn unresolved_migration_fences_checks_and_stale_candidates_until_settlement() {
        use desktop_pet_update_coordinator::migration::{MigrationPhase, MigrationRecord};
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        let snapshot = local_check();
        let context = snapshot.plan.as_ref().unwrap().context.clone();
        service.context = Some(context);
        service.checked(Ok(snapshot.clone()), None);
        let consent = service.consent_generation();
        let plan = service.plan().unwrap().clone();
        let mut record: MigrationRecord = serde_json::from_value(serde_json::json!({
            "version":1,"operation_id":"a".repeat(32),"plan_sha256":"b".repeat(64),
            "phase":"integrating","prepared_profiles":[],"preparing_profiles":[],
            "acknowledged_profiles":[],"stopped_profiles":[],"target_pid":null,
            "target_instance":null,"locator_adopted":false,"default_adopted":false,
            "native_ready":true,"target_committed":true,"manager":null,
            "target_manager":null,"reconnected_profiles":[],"retirement_attempt":0,"error":null
        }))
        .unwrap();
        for phase in [
            MigrationPhase::Integrating,
            MigrationPhase::AwaitingReconnect,
            MigrationPhase::RetirementFailed,
            MigrationPhase::ManagerUnknown,
        ] {
            record.phase = phase;
            service.migration_observed(&record);
            assert!(service.begin_check().is_none());
            assert!(!service.settings_admitted());
            assert!(service.plan().is_none());
            assert!(!service.consent_matches(&plan, consent));
        }
        record.phase = MigrationPhase::Complete;
        service.migration_observed(&record);
        assert!(service.settings_admitted());
        assert!(
            service.plan().is_none(),
            "settlement must not resurrect a stale candidate"
        );
        assert!(service.begin_check().is_some());
    }

    #[test]
    fn initial_history_admission_blocks_update_until_the_exact_context_is_checked() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        let context = local_check().plan.unwrap().context;
        service.reconcile_context(Some(context.clone()));
        assert!(service.begin_check().is_none());
        assert!(!service.settings_admitted());
        let mut stale = context.clone();
        stale.instance_id = "other-instance".into();
        service.migration_history_checked(&stale, true);
        assert!(service.begin_check().is_none());
        service.migration_history_checked(&context, false);
        assert!(service.begin_check().is_none());
        service.migration_history_checked(&context, true);
        assert!(service.begin_check().is_some());
    }
    fn eligible_settled_bridge(status: CheckStatus) -> AppUpdateService {
        let mut snapshot = local_check();
        let mut context = snapshot.plan.take().unwrap().context;
        context.version = "0.3.16".into();
        snapshot.installed_version = Some(context.version.clone());
        snapshot.status = status;
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        service.context = Some(context.clone());
        let mut completed = failed_record('a');
        completed.phase = OperationPhase::Completed;
        completed.execution_fence = ExecutionFence::ManagerExited;
        completed.installed = true;
        completed.applied = true;
        service.update_latest_journal(Ok(Some(completed.clone())));
        service.checked(Ok(snapshot), Some(SettledOperationProof::Update(completed)));
        service.migration_eligibility(context.clone(), Ok(()));
        service.migration_history_checked(&context, true);
        service
    }

    #[test]
    fn terminal_journal_requires_check_settlement_before_migration_promotion() {
        let mut service = eligible_settled_bridge(CheckStatus::Current);
        let mut completed = failed_record('b');
        completed.phase = OperationPhase::Completed;
        completed.execution_fence = ExecutionFence::ManagerExited;
        completed.installed = true;
        completed.applied = true;
        service.update_latest_journal(Ok(Some(completed.clone())));
        let snapshot = service.snapshot.clone().unwrap();
        service.checked(Ok(snapshot.clone()), None);
        assert!(!service.settings_admitted());
        assert_eq!(service.model(UiLocale::En).primary_action, None);
        service.checked(Ok(snapshot), Some(SettledOperationProof::Update(completed)));
        assert!(service.settings_admitted());
        assert_eq!(
            service.model(UiLocale::En).primary_action,
            desktop_pet_update_coordinator::identity::LEGACY
                .then_some(UpdateCardPrimaryAction::Migrate),
        );
    }

    #[test]
    fn settled_eligible_bridge_promotes_only_legacy_current_or_applied() {
        for status in [CheckStatus::Current, CheckStatus::Applied] {
            let service = eligible_settled_bridge(status);
            assert!(service.settings_admitted());
            assert!(service.previous_outcome.is_some());
            let model = service.model(UiLocale::En);
            assert!(model.action.is_none());
            if desktop_pet_update_coordinator::identity::LEGACY {
                assert_eq!(model.primary_action, Some(UpdateCardPrimaryAction::Migrate));
                assert_eq!(model.status, CheckStatus::Available);
                assert!(model.migration_available);
            } else {
                assert_eq!(model.primary_action, None);
                assert_eq!(model.status, status);
                assert!(!model.migration_available);
            }
        }
    }

    #[test]
    fn ordinary_newer_update_takes_priority_over_migration() {
        let mut service = eligible_settled_bridge(CheckStatus::Current);
        let mut snapshot = local_check_for_context(service.context.clone().unwrap());
        snapshot.status = CheckStatus::Available;
        snapshot.plan.as_mut().unwrap().action = UpdateAction::HomebrewUpgrade;
        service.checked(Ok(snapshot), None);
        let model = service.model(UiLocale::En);
        assert_eq!(model.primary_action, Some(UpdateCardPrimaryAction::Update));
        assert_eq!(model.action, Some(UpdateAction::HomebrewUpgrade));
        assert_eq!(model.status, CheckStatus::Available);
    }

    #[test]
    fn migration_primary_is_blocked_by_unverified_or_unsettled_state() {
        let blockers: &[(&str, fn(&mut AppUpdateService))] = &[
            ("eligibility pending", |s| s.migration_available = false),
            ("eligibility failure", |s| {
                s.migration_problem = Some("ineligible".into())
            }),
            ("history pending or failed", |s| {
                s.migration_history_verified = false
            }),
            ("missing context", |s| s.context = None),
            ("checking", |s| s.check_busy = true),
            ("pending request", |s| s.pending_request = Some(9)),
            ("launch pending", |s| s.launching = Some("a".repeat(32))),
            ("reserved launch", |s| s.launch_plan = local_check().plan),
            ("prepared", |s| s.prepared_plan = local_check().plan),
            ("unsettled observation", |s| {
                s.observed_operation_id = Some("a".repeat(32))
            }),
            ("problem", |s| s.problem = Some("check failed".into())),
            ("journal error", |s| {
                s.journal_error = Some("unreadable".into())
            }),
            ("stale policy", |s| {
                s.snapshot.as_mut().unwrap().policy = UpdatePolicy::StableAndBeta
            }),
            ("unusable ordinary plan", |s| {
                s.snapshot.as_mut().unwrap().plan = local_check().plan
            }),
        ];
        for (name, block) in blockers {
            let mut service = eligible_settled_bridge(CheckStatus::Current);
            block(&mut service);
            assert_ne!(
                service.model(UiLocale::En).primary_action,
                Some(UpdateCardPrimaryAction::Migrate),
                "{name}",
            );
        }
        for phase in [
            OperationPhase::Starting,
            OperationPhase::Installing,
            OperationPhase::Unknown,
            OperationPhase::Failed,
            OperationPhase::UserStopped,
            OperationPhase::Completed,
        ] {
            let mut service = eligible_settled_bridge(CheckStatus::Current);
            let mut record = failed_record('b');
            record.phase = phase;
            service.observed_operation_id = Some(record.operation_id.clone());
            service.operation = Some(record);
            let model = service.model(UiLocale::En);
            assert_ne!(
                model.primary_action,
                Some(UpdateCardPrimaryAction::Migrate),
                "{phase:?}"
            );
            assert!(model.action.is_none(), "{phase:?}");
            assert!(service.plan().is_none(), "{phase:?}");
            assert!(!service.settings_admitted(), "{phase:?}");
            assert!(service.begin_check().is_none(), "{phase:?}");
            if matches!(
                phase,
                OperationPhase::Unknown | OperationPhase::Failed | OperationPhase::UserStopped
            ) {
                assert_eq!(model.status, CheckStatus::Failed);
                assert_eq!(
                    model.primary_action,
                    Some(UpdateCardPrimaryAction::Recover),
                    "{phase:?}"
                );
            } else {
                assert!(model.primary_action.is_none(), "{phase:?}");
            }
        }
        for status in [
            CheckStatus::NotChecked,
            CheckStatus::Checking,
            CheckStatus::Available,
            CheckStatus::Local,
            CheckStatus::Installed,
            CheckStatus::Failed,
            CheckStatus::Unsupported,
        ] {
            let service = eligible_settled_bridge(status);
            assert_eq!(
                service.model(UiLocale::En).primary_action,
                None,
                "{status:?}"
            );
        }
    }

    #[test]
    fn migration_results_for_stale_context_cannot_promote() {
        let mut service = eligible_settled_bridge(CheckStatus::Current);
        service.migration_available = false;
        service.migration_history_verified = false;
        let context = service.context.clone().unwrap();
        let mut stale = context.clone();
        stale.instance_id = "previous-instance".into();
        service.migration_eligibility(stale.clone(), Ok(()));
        service.migration_history_checked(&stale, true);
        assert_eq!(service.model(UiLocale::En).primary_action, None);
        service.migration_eligibility(context.clone(), Ok(()));
        service.migration_history_checked(&context, false);
        assert_eq!(service.model(UiLocale::En).primary_action, None);
        service.migration_history_checked(&context, true);
        assert_eq!(
            service.model(UiLocale::En).primary_action,
            desktop_pet_update_coordinator::identity::LEGACY
                .then_some(UpdateCardPrimaryAction::Migrate),
        );
    }

    #[test]
    fn host_commits_retire_old_checks_and_consent_and_capture_each_live_mode() {
        use desktop_pet_contracts::ObservationSelection;
        let mut state = crate::state::tests::observed_test_state(ObservationSelection::Provider(
            "herdr".into(),
        ));
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        service.reconcile_context(state.update_context().cloned());
        service.migration_history_checked(state.update_context().unwrap(), true);
        let history = failed_record('a');
        service.previous_outcome = Some(history.clone());
        let (request, policy, context) = service.begin_check().unwrap();
        let mut snapshot = local_check_for_context(context);
        assert_eq!(
            service.checked_event(request, policy, Ok(snapshot.clone()), None),
            Some(42)
        );
        for mode in [
            ObservationSelection::Provider("tern".into()),
            ObservationSelection::Auto,
            ObservationSelection::Provider("herdr".into()),
        ] {
            let old_plan = service.plan().unwrap().clone();
            let old_consent = service.consent_generation();
            let (old_request, old_policy, _) = service.begin_check().unwrap();
            let mut observation = state.observation_preferences().clone();
            observation.host = mode.clone();
            state.apply_observation_preferences(observation).unwrap();
            service.reconcile_context(state.update_context().cloned());
            assert!(!service.check_busy);
            assert!(service.plan().is_none());
            assert!(!service.consent_matches(&old_plan, old_consent));
            assert!(service.launch(old_plan, old_consent).is_err());
            assert_eq!(
                service.checked_at_for_save(old_request, old_policy, &Ok(snapshot.clone())),
                None
            );
            service.migration_history_checked(state.update_context().unwrap(), true);
            let (request, policy, context) = service.begin_check().unwrap();
            assert_eq!(context.observation_mode().unwrap(), mode);
            assert_eq!(
                service.checked_event(
                    old_request,
                    old_policy,
                    Ok(snapshot),
                    Some(SettledOperationProof::Update(history.clone())),
                ),
                None
            );
            assert_eq!(service.pending_request, Some(request));
            assert!(service.check_busy);
            assert_eq!(service.previous_outcome.as_ref(), Some(&history));
            snapshot = local_check_for_context(context.clone());
            assert_eq!(
                service.checked_event(request, policy, Ok(snapshot.clone()), None),
                Some(42)
            );
            let plan = service.plan().unwrap().clone();
            assert_eq!(plan.context, context);
            assert_eq!(plan.context.observation_mode().unwrap(), mode);
            let consent = service.consent_generation();
            assert!(service.consent_matches(&plan, consent));
            let revision = service.revision();
            service.reconcile_context(state.update_context().cloned());
            assert_eq!(service.revision(), revision);
            assert!(service.consent_matches(&plan, consent));
        }
    }

    #[test]
    fn prepared_admission_protects_host_context_and_original_helper_ownership() {
        use desktop_pet_contracts::ObservationSelection;
        let mut state = crate::state::tests::observed_test_state(ObservationSelection::Provider(
            "herdr".into(),
        ));
        // Capture the observation identity before launching, just as the live
        // state does before a check. Prepare must preserve this source context,
        // not repair an already-owned plan that omitted its observation mode.
        let mut source_context = local_check().plan.unwrap().context;
        source_context
            .set_observation_selection(&ObservationSelection::Provider("herdr".into()))
            .unwrap();
        state.set_update_context(source_context.clone()).unwrap();
        assert_eq!(state.update_context(), Some(&source_context));
        let (mut service, plan) = pending_launch(local_check_for_context(source_context));
        assert!(service.launch_result(&plan.operation_id, Ok(())));
        let mut active = failed_record('e');
        active.phase = OperationPhase::Installing;
        service.update_journal(&plan.operation_id, Ok(Some(active.clone())), None);
        service.journal_in_flight = Some(plan.operation_id.clone());
        let history = failed_record('a');
        service.previous_outcome = Some(history.clone());
        state.set_ui_ready();
        let (sender, receiver) = mpsc::channel();
        state
            .queue_update_prepare(plan.operation_id.clone(), "secret".into(), sender)
            .unwrap();
        state.take_update_prepare().unwrap();
        assert!(state.finish_update_prepare(&plan.operation_id, "secret", Ok(())));
        receiver.recv().unwrap().unwrap();
        assert!(state.update_mutation_allowed().is_err());
        let mut observation = state.observation_preferences().clone();
        observation.host = ObservationSelection::Provider("tern".into());
        assert!(state.apply_observation_preferences(observation).is_err());
        let mut replacement_context = plan.context.clone();
        replacement_context
            .set_observation_selection(&ObservationSelection::Provider("tern".into()))
            .unwrap();
        assert!(state.set_update_context(replacement_context).is_err());
        assert_eq!(
            state.observation_mode(),
            ObservationSelection::Provider("herdr".into())
        );
        assert_eq!(state.update_context(), Some(&plan.context));
        service.reconcile_context(state.update_context().cloned());
        assert_eq!(service.context.as_ref(), Some(&plan.context));
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert_eq!(service.operation.as_ref(), Some(&active));
        assert_eq!(service.previous_outcome.as_ref(), Some(&history));
        assert_eq!(
            service.journal_in_flight.as_deref(),
            Some(plan.operation_id.as_str())
        );
        assert!(service.observing(&plan.operation_id));
        assert!(!service.settings_admitted());
        assert!(service.begin_check().is_none());
        assert!(service.plan().is_none());
        let recovered = failed_record('e');
        assert_eq!(
            service.update_journal(
                &plan.operation_id,
                Ok(Some(recovered.clone())),
                Some(PreparedRecoveryProof {
                    record: recovered,
                    plan: plan.clone(),
                }),
            ),
            Some(plan.context.clone())
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        service.recovery_reconciled(&plan.operation_id);
        assert!(service.prepared_plan.is_none());
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
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        service.context = Some(plan.context.clone());
        service.launching = Some(record.operation_id.clone());
        service.launch_plan = Some(plan.clone());
        service.observed_operation_id = Some(record.operation_id.clone());
        service.launch_generation = Some(service.consent_generation());
        assert_eq!(
            service.update_journal(
                &record.operation_id,
                Ok(Some(record.clone())),
                Some(PreparedRecoveryProof {
                    record: record.clone(),
                    plan: plan.clone(),
                }),
            ),
            Some(plan.context.clone())
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        service.recovery_reconciled(&record.operation_id);
        assert!(service.prepared_plan.is_none());
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Failed);
        assert!(service.plan().is_none());
        assert!(service.launching.is_none());

        let mut newer = plan.clone();
        newer.operation_id = "b".repeat(32);
        service.launch_plan = Some(newer.clone());
        service.prepared_plan = Some(newer.clone());
        service.observed_operation_id = Some(newer.operation_id.clone());
        assert_eq!(
            service.update_journal(
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
            service.update_journal(&newer.operation_id, Ok(Some(blocked)), None),
            None
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&newer));
    }

    #[test]
    fn failed_phase_alone_or_a_different_prepared_image_cannot_thaw() {
        let record = failed_record('c');
        let mut plan = local_check().plan.unwrap();
        plan.operation_id = record.operation_id.clone();
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        service.context = Some(plan.context.clone());
        service.observed_operation_id = Some(record.operation_id.clone());
        service.prepared_plan = Some(plan.clone());
        assert_eq!(
            service.update_journal(&record.operation_id, Ok(Some(record.clone())), None),
            None
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        let mut other_image = plan.clone();
        other_image.context.running.sha256 = "different original".into();
        assert_eq!(
            service.update_journal(
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
        assert_eq!(
            service.update_journal(&operation_id, Ok(Some(intent)), None),
            None
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
    }

    #[test]
    fn fresh_check_retires_only_the_proven_reconciled_failure_and_exposes_rebuild() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        let prior = failed_record('a');
        service.update_latest_journal(Ok(Some(prior.clone())));
        assert!(service.plan().is_none());
        assert_eq!(
            service.checked(
                Ok(local_check()),
                Some(SettledOperationProof::Update(prior.clone()))
            ),
            Some(42)
        );
        assert_eq!(
            service.plan().map(|plan| &plan.action),
            Some(&UpdateAction::SourceRebuild)
        );
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Local);
        let history = service.model(UiLocale::En).history;
        assert!(!service.model(UiLocale::En).busy);
        assert_eq!(
            service.model(UiLocale::En).action,
            Some(UpdateAction::SourceRebuild)
        );
        service.update_latest_journal(Ok(Some(prior.clone())));
        let prior_id = prior.operation_id.clone();
        service.update_journal(&prior_id, Ok(Some(prior)), None);
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Local);
        assert_eq!(service.model(UiLocale::En).history, history);
        assert!(service.plan().is_some());
    }

    #[test]
    fn checked_settlement_keeps_late_startup_history_from_blocking_a_new_plan() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        let prior = failed_record('a');
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::Update(prior.clone())),
        );
        service.update_latest_journal(Ok(Some(prior)));
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Local);
        assert_eq!(
            service.plan().map(|plan| &plan.action),
            Some(&UpdateAction::SourceRebuild)
        );
    }

    #[test]
    fn late_unproven_startup_history_still_blocks_replacement() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        service.checked(Ok(local_check()), None);
        let mut prior = failed_record('a');
        prior.phase = OperationPhase::Unknown;
        service.update_latest_journal(Ok(Some(prior)));
        assert!(service.plan().is_none());
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Failed);
    }

    #[test]
    fn reconciled_user_stop_can_offer_a_new_explicit_plan() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        let mut prior = failed_record('f');
        prior.phase = OperationPhase::UserStopped;
        assert!(settled_terminal(&prior));
        service.update_latest_journal(Ok(Some(prior.clone())));
        assert!(service.plan().is_none());
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::Update(prior)),
        );
        assert_eq!(
            service.plan().map(|plan| &plan.action),
            Some(&UpdateAction::SourceRebuild)
        );
    }

    #[cfg(desktop_pet_legacy)]
    #[test]
    fn historical_completed_journal_allows_fresh_check_without_recovery() {
        use desktop_pet_update_coordinator::protocol::{
            latest_operation, reserve_install, write_operation, write_private_bytes,
            write_private_new,
        };
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempfile::tempdir().unwrap();
        let state = temporary.path().join("state");
        let root = temporary.path().join("source");
        fs::create_dir(&state).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(&root).unwrap();
        let mut snapshot = local_check();
        snapshot.origin = InstallOrigin::Homebrew {
            brew: root.join("bin/brew"),
            prefix: root.clone(),
            cellar_formula_root: root.join("Cellar/herdr-desktop-pet"),
            formula: "hanbong5938/tap/herdr-desktop-pet".into(),
            locator: root.join("opt/herdr-desktop-pet/app"),
        };
        let mut current_plan = snapshot.plan.take().unwrap();
        current_plan.origin = snapshot.origin.clone();
        current_plan.action = UpdateAction::HomebrewUpgrade;
        current_plan.context.state_dir = state.clone();
        current_plan.context.version = "0.3.16".into();
        current_plan.context.running_origin = Some(Box::new(snapshot.origin.clone()));
        current_plan.running.path = root.join("current-app");
        current_plan.running.sha256 = "c".repeat(64);
        current_plan.context.running = current_plan.running.clone();
        current_plan.context.executable = current_plan.running.path.clone();
        let context = current_plan.context.clone();
        snapshot.status = CheckStatus::Current;
        snapshot.installed_version = Some(context.version.clone());

        let mut historical = failed_record('a');
        historical.version = 2;
        historical.phase = OperationPhase::Completed;
        historical.execution_fence = ExecutionFence::ManagerExited;
        historical.installed = true;
        historical.applied = true;
        historical.candidate = Some(ExecutableIdentity {
            path: root.join("removed-0.3.5-image"),
            sha256: "b".repeat(64),
        });
        let mut historical_plan = current_plan.clone();
        historical_plan.version = 2;
        historical_plan.operation_id = historical.operation_id.clone();
        historical_plan.context.version = "0.3.4".into();
        historical_plan.context.instance_id = "original-historical-instance".into();
        historical_plan.running.path = root.join("removed-0.3.4-image");
        historical_plan.running.sha256 = "a".repeat(64);
        historical_plan.context.executable = historical_plan.running.path.clone();
        historical_plan.context.running = historical_plan.running.clone();
        let updates = private_updates(&state).unwrap();
        let plan_path = updates.join(format!("{}.plan.json", historical.operation_id));
        let result_path = updates.join(format!("{}.result.json", historical.operation_id));
        let latest_path = updates.join("latest-operation.json");
        let plan_bytes = serde_json::to_vec_pretty(&historical_plan).unwrap();
        let result_bytes = serde_json::to_vec_pretty(&historical).unwrap();
        let latest_bytes = serde_json::to_vec(&historical.operation_id).unwrap();
        write_private_new(&plan_path, &plan_bytes, 0o600).unwrap();
        write_private_new(&result_path, &result_bytes, 0o600).unwrap();
        write_private_new(&latest_path, &latest_bytes, 0o600).unwrap();
        assert!(historical_plan.candidate.is_none());
        assert!(!historical_plan.running.path.exists());
        assert!(!historical.candidate.as_ref().unwrap().path.exists());

        // Neither a terminal spelling nor an old version alone is admission.
        for (key, value) in [
            ("version", serde_json::json!(0)),
            ("version", serde_json::json!(UPDATER_PROTOCOL + 1)),
            ("operation_id", serde_json::json!("b".repeat(32))),
            ("phase", serde_json::json!("preparing")),
            ("phase", serde_json::json!("unknown")),
            ("execution_fence", serde_json::json!("manager_intent")),
            ("execution_fence", serde_json::json!("no_spawn")),
            ("installed", serde_json::json!(false)),
            ("applied", serde_json::json!(false)),
            ("detail", serde_json::json!("x".repeat(8193))),
            ("unrecognized", serde_json::json!(true)),
            (
                "candidate",
                serde_json::json!({"path":"relative","sha256":"b".repeat(64)}),
            ),
            (
                "candidate",
                serde_json::json!({"path":"/old/image","sha256":"not-a-hash"}),
            ),
        ] {
            let mut invalid = serde_json::to_value(&historical).unwrap();
            invalid[key] = value;
            let bytes = serde_json::to_vec(&invalid).unwrap();
            write_private_bytes(&result_path, &bytes, 0o600).unwrap();
            let mut blocked = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
            blocked.context = Some(context.clone());
            blocked.update_latest_journal(latest_operation(&state));
            assert!(blocked.begin_check().is_none(), "{key}: {invalid}");
            assert!(!blocked.settings_admitted());
            assert!(settlement_proof(&context, &snapshot, &historical.operation_id).is_err());
            assert!(write_operation(&state, &failed_record('e')).is_err());
            assert_eq!(fs::read(&latest_path).unwrap(), latest_bytes);
            assert_eq!(fs::read(&result_path).unwrap(), bytes);
        }
        for bytes in [b"{".to_vec(), vec![b' '; 16385]] {
            write_private_bytes(&result_path, &bytes, 0o600).unwrap();
            assert!(latest_operation(&state).is_err());
        }
        write_private_bytes(&result_path, &result_bytes, 0o600).unwrap();
        fs::set_permissions(&result_path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(latest_operation(&state).is_err());
        fs::set_permissions(&result_path, fs::Permissions::from_mode(0o600)).unwrap();

        for field in [
            "version",
            "operation_id",
            "state",
            "running",
            "config",
            "unknown",
        ] {
            let mut invalid = serde_json::to_value(&historical_plan).unwrap();
            match field {
                "version" => invalid["version"] = serde_json::json!(1),
                "operation_id" => invalid["operation_id"] = serde_json::json!("b".repeat(32)),
                "state" => invalid["context"]["state_dir"] = serde_json::json!("/foreign/state"),
                "running" => invalid["running"]["sha256"] = serde_json::json!("d".repeat(64)),
                "config" => invalid["context"]["config_dir"] = serde_json::json!("/foreign/config"),
                "unknown" => invalid["unrecognized"] = serde_json::json!(true),
                _ => unreachable!(),
            }
            write_private_bytes(&plan_path, &serde_json::to_vec(&invalid).unwrap(), 0o600).unwrap();
            assert!(
                settlement_proof(&context, &snapshot, &historical.operation_id).is_err(),
                "{field}"
            );
            assert!(
                completed_history_proof(
                    &context,
                    &OperationObservation::Update(historical.clone()),
                )
                .is_err(),
                "{field}"
            );
        }
        write_private_bytes(&plan_path, &plan_bytes, 0o600).unwrap();

        let lease = lock_operation(&state, &historical.operation_id).unwrap();
        assert!(settlement_proof(&context, &snapshot, &historical.operation_id).is_err());
        assert!(completed_history_proof(
            &context,
            &OperationObservation::Update(historical.clone()),
        )
        .is_err());
        drop(lease);
        let reserved =
            reserve_install(&snapshot.origin, None, &state, &historical.operation_id).unwrap();
        assert!(latest_operation(&state).is_err());
        assert!(settlement_proof(&context, &snapshot, &historical.operation_id).is_err());
        assert!(completed_history_proof(
            &context,
            &OperationObservation::Update(historical.clone()),
        )
        .is_err());
        reserved.release().unwrap();
        assert!(completed_history_proof(
            &context,
            &OperationObservation::Update(historical.clone()),
        )
        .is_ok());

        for phase in [OperationPhase::Failed, OperationPhase::UserStopped] {
            let mut stopped = historical.clone();
            stopped.phase = phase;
            stopped.installed = false;
            stopped.applied = false;
            stopped.candidate = None;
            write_private_bytes(&result_path, &serde_json::to_vec(&stopped).unwrap(), 0o600)
                .unwrap();
            assert_eq!(latest_operation(&state).unwrap(), Some(stopped.clone()));
            assert!(read_operation(&state, &stopped.operation_id).is_err());
            assert!(prepared_recovery_proof(&context, &stopped.operation_id, &stopped).is_err());
            let mut blocked = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
            blocked.context = Some(context.clone());
            blocked.update_latest_journal(latest_operation(&state));
            blocked.checked(Ok(snapshot.clone()), None);
            assert!(!blocked.settings_admitted());
            assert!(blocked.plan().is_none());
            assert!(!blocked.model(UiLocale::En).migration_available);
        }
        write_private_bytes(&result_path, &result_bytes, 0o600).unwrap();

        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        service.context = Some(context.clone());
        service.update_latest_journal(latest_operation(&state));
        service.reload_journal(historical.operation_id.clone());
        match service
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
        {
            Event::Journal {
                operation_id,
                result,
                prepared_recovery,
                completed_history,
            } => {
                assert!(prepared_recovery.is_none());
                assert_eq!(
                    result,
                    Ok(Some(OperationObservation::Update(historical.clone())))
                );
                assert!(service
                    .journal(&operation_id, result, prepared_recovery, completed_history)
                    .is_none());
            }
            _ => panic!("expected a journal observation"),
        }
        assert!(service.journal_error.is_none());
        assert!(service.settings_admitted());
        service.change_policy(UpdatePolicy::StableAndBeta, None);
        assert_eq!(service.policy(), UpdatePolicy::StableAndBeta);
        service.change_policy(UpdatePolicy::StableOnly, None);
        assert_eq!(service.policy(), UpdatePolicy::StableOnly);
        assert_eq!(service.previous_outcome.as_ref(), Some(&historical));
        let check_request = service.begin_check();
        assert!(check_request.is_some());
        let (request_id, policy_generation, _) = check_request.unwrap();
        assert!(!service.settings_admitted());
        assert!(read_operation(&state, &historical.operation_id).is_err());
        assert!(read_plan(&state, &historical.operation_id).is_err());
        assert!(recheck_plan(&historical_plan).is_err());
        assert!(prepared_recovery_proof(&context, &historical.operation_id, &historical).is_err());
        let proof = settlement_proof(&context, &snapshot, &historical.operation_id).unwrap();
        service.checked_event(
            request_id,
            policy_generation,
            Ok(snapshot.clone()),
            Some(proof),
        );
        assert!(service.settings_admitted());
        assert_eq!(service.previous_outcome.as_ref(), Some(&historical));
        service.migration_eligibility(context.clone(), Ok(()));
        service.migration_history_checked(&context, true);
        assert_eq!(
            service.model(UiLocale::En).primary_action,
            Some(UpdateCardPrimaryAction::Migrate)
        );
        assert_eq!(fs::read(&plan_path).unwrap(), plan_bytes);
        assert_eq!(fs::read(&result_path).unwrap(), result_bytes);
        assert_eq!(fs::read(&latest_path).unwrap(), latest_bytes);
        assert!(write_operation(&state, &historical).is_err());
        // The historical result schema has no action. A no-manager completion
        // becomes UI settlement evidence only with its exact retained ApplyInstalled plan.
        let mut managerless = historical.clone();
        managerless.execution_fence = ExecutionFence::NoSpawn;
        let mut applied_plan = historical_plan.clone();
        applied_plan.action = UpdateAction::ApplyInstalled;
        applied_plan.target = None;
        applied_plan.candidate = managerless.candidate.clone();
        let applied_bytes = serde_json::to_vec(&applied_plan).unwrap();
        let managerless_bytes = serde_json::to_vec(&managerless).unwrap();
        write_private_bytes(&plan_path, &applied_bytes, 0o600).unwrap();
        write_private_bytes(&result_path, &managerless_bytes, 0o600).unwrap();
        let mut observed = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        observed.context = Some(context.clone());
        observed.latest_journal(observe_latest_operation(&state));
        let (request, generation, _) = observed.begin_check().unwrap();
        let proof = settlement_proof(&context, &snapshot, &managerless.operation_id).unwrap();
        observed.checked_event(request, generation, Ok(snapshot.clone()), Some(proof));
        observed.migration_eligibility(context.clone(), Ok(()));
        observed.migration_history_checked(&context, true);
        assert!(observed.settings_admitted());
        assert_eq!(
            observed.model(UiLocale::En).primary_action,
            Some(UpdateCardPrimaryAction::Migrate)
        );
        assert!(read_plan(&state, &managerless.operation_id).is_err());
        assert!(
            prepared_recovery_proof(&context, &managerless.operation_id, &managerless).is_err()
        );
        assert_eq!(fs::read(&plan_path).unwrap(), applied_bytes);
        assert_eq!(fs::read(&result_path).unwrap(), managerless_bytes);
        assert_eq!(fs::read(&latest_path).unwrap(), latest_bytes);
        write_private_bytes(&plan_path, &plan_bytes, 0o600).unwrap();
        write_private_bytes(&result_path, &result_bytes, 0o600).unwrap();
        let mut fresh = failed_record('e');
        fresh.phase = OperationPhase::Preparing;
        write_operation(&state, &fresh).unwrap();
        assert_eq!(latest_operation(&state).unwrap(), Some(fresh));
        assert_eq!(fs::read(&plan_path).unwrap(), plan_bytes);
        assert_eq!(fs::read(&result_path).unwrap(), result_bytes);
    }

    #[test]
    fn clearance_rejects_busy_lease_legacy_journal_and_retained_profile_marker() {
        use desktop_pet_update_coordinator::protocol::{
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
    fn stopped_original_restoration_history_requires_independent_settlement_before_beta_admission()
    {
        use desktop_pet_update_coordinator::protocol::{
            reserve_install, write_operation, write_private_new,
        };
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempfile::tempdir().unwrap();
        let state_dir = temporary.path().join("state");
        fs::create_dir(&state_dir).unwrap();
        fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let root = temporary.path().join("source");
        fs::create_dir(&root).unwrap();
        let mut snapshot = local_check();
        snapshot.origin = InstallOrigin::Source { root: root.clone() };
        let next_plan = snapshot.plan.as_mut().unwrap();
        next_plan.origin = snapshot.origin.clone();
        next_plan.running.path = root.join("app");
        next_plan.context.running = next_plan.running.clone();
        next_plan.context.executable = next_plan.running.path.clone();
        next_plan.context.running_origin = Some(Box::new(snapshot.origin.clone()));
        next_plan.context.state_dir = state_dir.clone();
        let context = next_plan.context.clone();
        let mut stopped = failed_record('f');
        stopped.operation_id =
            desktop_pet_update_coordinator::protocol::installation_key(&snapshot.origin).unwrap()
                [..32]
                .to_owned();
        stopped.phase = OperationPhase::UserStopped;
        stopped.execution_fence = ExecutionFence::ManagerExited;
        stopped.candidate = Some(ExecutableIdentity {
            path: next_plan.running.path.clone(),
            sha256: "historical candidate image I".into(),
        });
        let mut prior_plan = next_plan.clone();
        prior_plan.operation_id = stopped.operation_id.clone();
        let updates = private_updates(&state_dir).unwrap();
        let plan_path = updates.join(format!("{}.plan.json", stopped.operation_id));
        write_private_new(&plan_path, &serde_json::to_vec(&prior_plan).unwrap(), 0o600).unwrap();
        let result_path = write_operation(&state_dir, &stopped).unwrap();
        let plan_bytes = fs::read(&plan_path).unwrap();
        let result_bytes = fs::read(&result_path).unwrap();

        let reservation =
            reserve_install(&prior_plan.origin, None, &state_dir, &stopped.operation_id).unwrap();
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        service.update_latest_journal(Ok(Some(stopped.clone())));
        assert!(!service.settings_admitted());
        assert!(settlement_proof(&context, &snapshot, &stopped.operation_id).is_err());
        service.checked(Ok(snapshot.clone()), None);
        service.change_policy(UpdatePolicy::StableAndBeta, None);
        assert_eq!(service.policy(), UpdatePolicy::StableOnly);
        assert!(!service.settings_admitted());

        reservation.release().unwrap();
        let operation_lease = lock_operation(&state_dir, &stopped.operation_id).unwrap();
        assert!(settlement_proof(&context, &snapshot, &stopped.operation_id).is_err());
        drop(operation_lease);
        let mut mismatched = snapshot.clone();
        mismatched.origin = InstallOrigin::Unknown {
            reason: "original source did not match fresh discovery".into(),
        };
        assert!(settlement_proof(&context, &mismatched, &stopped.operation_id).is_err());
        let proof = settlement_proof(&context, &snapshot, &stopped.operation_id).unwrap();
        service.checked(Ok(snapshot.clone()), Some(proof));
        assert!(service.settings_admitted());
        assert_eq!(
            service.retired_operation_id.as_deref(),
            Some(stopped.operation_id.as_str())
        );
        assert_eq!(service.previous_outcome.as_ref(), Some(&stopped));
        assert_eq!(
            service.plan().map(|plan| &plan.action),
            Some(&UpdateAction::SourceRebuild)
        );
        service.change_policy(UpdatePolicy::StableAndBeta, None);
        assert_eq!(service.policy(), UpdatePolicy::StableAndBeta);
        service.update_latest_journal(Ok(Some(stopped.clone())));
        assert!(service.settings_admitted());
        assert_eq!(fs::read(&plan_path).unwrap(), plan_bytes);
        assert_eq!(fs::read(&result_path).unwrap(), result_bytes);

        let old_proof = settlement_proof(&context, &snapshot, &stopped.operation_id).unwrap();
        let mut newer = failed_record('a');
        newer.phase = OperationPhase::Installing;
        newer.execution_fence = ExecutionFence::ManagerIntent;
        let mut newer_service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        newer_service.update_latest_journal(Ok(Some(stopped.clone())));
        newer_service.update_latest_journal(Ok(Some(newer.clone())));
        newer_service.checked(Ok(snapshot), Some(old_proof));
        assert_eq!(newer_service.operation.as_ref(), Some(&newer));
        assert!(!newer_service.settings_admitted());
        assert!(newer_service.retired_operation_id.is_none());
    }

    #[test]
    fn completed_history_clearance_supersedes_racing_recovery_failure_in_both_event_orders() {
        use desktop_pet_update_coordinator::protocol::{write_operation, write_private_new};
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempfile::tempdir().unwrap();
        let state_dir = temporary.path().join("state");
        fs::create_dir(&state_dir).unwrap();
        fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let root = temporary.path().join("source");
        fs::create_dir(&root).unwrap();
        let mut prior_plan = local_check().plan.unwrap();
        prior_plan.context.state_dir = state_dir.clone();
        prior_plan.origin = InstallOrigin::Source { root };
        prior_plan.context.running_origin = Some(Box::new(prior_plan.origin.clone()));
        let mut completed = failed_record('f');
        completed.operation_id = prior_plan.operation_id.clone();
        completed.phase = OperationPhase::Completed;
        completed.execution_fence = ExecutionFence::ManagerExited;
        completed.installed = true;
        completed.applied = true;
        completed.candidate = Some(prior_plan.running.clone());
        let updates = private_updates(&state_dir).unwrap();
        write_private_new(
            &updates.join(format!("{}.plan.json", completed.operation_id)),
            &serde_json::to_vec(&prior_plan).unwrap(),
            0o600,
        )
        .unwrap();
        write_operation(&state_dir, &completed).unwrap();
        let mut context = prior_plan.context.clone();
        context.instance_id.push_str("-successor");
        context.version = "0.4.17-beta.8".into();
        context.running.sha256 = "9".repeat(64);
        let observation = OperationObservation::Update(completed.clone());

        for failure_first in [true, false] {
            // The journal admission worker holds this same operation lease
            // while proving settlement. Recovery must not confuse that reader
            // with a retained helper and leave its older failure on the card.
            let lease = lock_operation(&state_dir, &completed.operation_id).unwrap();
            let result = recovery_projection(&context, &completed.operation_id, None);
            assert!(result.is_err());
            assert!(completed_history_proof(&context, &observation).is_err());
            drop(lease);
            assert_eq!(
                recovery_projection(&context, &completed.operation_id, None)
                    .unwrap()
                    .disposition,
                RecoveryDisposition::Reconciled
            );
            let proof = completed_history_proof(&context, &observation).unwrap();
            let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableAndBeta, None);
            service.context = Some(context.clone());
            // Install the pending observation before startup discovery so the
            // deterministic event ordering needs no timing or background I/O.
            service.recovery_request = 1;
            service.recovery_observing = Some((
                context.clone(),
                completed.operation_id.clone(),
                1,
                completed.clone(),
            ));
            service.latest_journal(Ok(Some(observation.clone())));
            let history = service.model(UiLocale::En).history;
            let event = RecoveryEvent::Observed {
                context: context.clone(),
                operation_id: completed.operation_id.clone(),
                request: 1,
                result,
            };
            let mut event = Some(event);
            if failure_first {
                service.recovery_event(event.take().unwrap());
                assert!(service.recovery_error.is_some());
                assert!(!service.check_admitted());
            }
            service.journal(
                &completed.operation_id,
                Ok(Some(observation.clone())),
                None,
                Some(proof),
            );
            if let Some(event) = event {
                service.recovery_event(event);
            }
            service.recovery_event(RecoveryEvent::Focused {
                context: context.clone(),
                operation_id: completed.operation_id.clone(),
                request: 1,
                record: completed.clone(),
                result: Err("late recovery focus failure".into()),
            });
            service.observe_recovery();
            assert!(service.recovery_error.is_none());
            assert!(service.recovery_observing.is_none());
            assert!(service.current_recovery().is_none());
            assert!(service.completed_history_admitted());
            assert!(service.check_admitted());
            assert!(service.settings_admitted());
            assert_eq!(service.operation.as_ref(), Some(&completed));
            assert!(service.snapshot.is_none());
            assert!(service.plan().is_none());
            for locale in [UiLocale::En, UiLocale::Ko] {
                let model = service.model(locale);
                assert_eq!(
                    model.detail,
                    text(locale, Message::AppUpdateRecoveryComplete)
                );
                assert!(model.primary_action.is_none());
                assert!(!model.recovery_enabled);
            }
            assert_eq!(service.model(UiLocale::En).history, history);
        }
    }

    #[test]
    fn completed_applied_history_admits_settings_without_a_discovery_check_only_after_scope_release(
    ) {
        use desktop_pet_update_coordinator::protocol::{
            reserve_install, write_operation, write_private_bytes, write_private_new,
        };
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempfile::tempdir().unwrap();
        let state_dir = temporary.path().join("state");
        fs::create_dir(&state_dir).unwrap();
        fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let root = temporary.path().join("source");
        fs::create_dir(&root).unwrap();
        let mut snapshot = local_check();
        snapshot.origin = InstallOrigin::Source { root };
        snapshot.status = CheckStatus::Current;
        snapshot.policy = UpdatePolicy::StableAndBeta;
        let mut prior_plan = snapshot.plan.take().unwrap();
        prior_plan.context.state_dir = state_dir.clone();
        prior_plan.context.running_origin = Some(Box::new(snapshot.origin.clone()));
        prior_plan.origin = snapshot.origin.clone();
        let mut completed = failed_record('f');
        completed.operation_id =
            desktop_pet_update_coordinator::protocol::installation_key(&prior_plan.origin).unwrap()
                [..32]
                .to_owned();
        completed.phase = OperationPhase::Completed;
        completed.execution_fence = ExecutionFence::ManagerExited;
        completed.installed = true;
        completed.applied = true;
        completed.candidate = Some(prior_plan.running.clone());
        prior_plan.operation_id = completed.operation_id.clone();
        let updates = private_updates(&state_dir).unwrap();
        let plan_path = updates.join(format!("{}.plan.json", completed.operation_id));
        write_private_new(&plan_path, &serde_json::to_vec(&prior_plan).unwrap(), 0o600).unwrap();
        write_operation(&state_dir, &completed).unwrap();
        let result_path = updates.join(format!("{}.result.json", completed.operation_id));
        let result_bytes = fs::read(&result_path).unwrap();
        let plan_bytes = fs::read(&plan_path).unwrap();
        let context = &prior_plan.context;
        let observation = OperationObservation::Update(completed.clone());
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableAndBeta, None);
        service.context = Some(context.clone());
        service.update_latest_journal(Ok(Some(completed.clone())));
        let history = service.model(UiLocale::En).history;
        assert!(!service.settings_admitted());
        service.change_policy(UpdatePolicy::StableOnly, None);
        service.set_auto_check(true);
        assert_eq!(service.policy(), UpdatePolicy::StableAndBeta);
        assert!(!service.auto_check);

        let lease = lock_operation(&state_dir, &completed.operation_id).unwrap();
        assert!(settlement_proof(context, &snapshot, &completed.operation_id).is_err());
        assert!(completed_history_proof(context, &observation).is_err());
        drop(lease);
        let reserved = reserve_install(
            &prior_plan.origin,
            None,
            &state_dir,
            &completed.operation_id,
        )
        .unwrap();
        assert!(completed_history_proof(context, &observation).is_err());
        service.journal(
            &completed.operation_id,
            Ok(Some(observation.clone())),
            None,
            None,
        );
        service.change_policy(UpdatePolicy::StableOnly, None);
        service.set_auto_check(true);
        assert_eq!(service.policy(), UpdatePolicy::StableAndBeta);
        assert!(!service.auto_check);
        reserved.release().unwrap();

        let profile_marker = updates.join("profile.reservation.json");
        write_private_bytes(&profile_marker, br#"{"version":1}"#, 0o600).unwrap();
        assert!(completed_history_proof(context, &observation).is_err());
        fs::remove_file(&profile_marker).unwrap();

        for (phase, fence) in [
            (OperationPhase::Unknown, ExecutionFence::ManagerExited),
            (OperationPhase::Installing, ExecutionFence::ManagerIntent),
            (OperationPhase::Completed, ExecutionFence::ManagerIntent),
        ] {
            let mut unsettled = completed.clone();
            unsettled.phase = phase;
            unsettled.execution_fence = fence;
            write_operation(&state_dir, &unsettled).unwrap();
            let unsettled = OperationObservation::Update(unsettled);
            assert!(completed_history_proof(context, &unsettled).is_err());
        }
        write_private_bytes(&result_path, &result_bytes, 0o600).unwrap();
        let policy_proof = completed_history_proof(context, &observation).unwrap();
        service.journal(
            &completed.operation_id,
            Ok(Some(observation.clone())),
            None,
            Some(policy_proof),
        );
        assert!(service.settings_admitted());
        assert!(!service.model(UiLocale::En).settings_locked);
        assert_eq!(service.model(UiLocale::En).history, history);
        assert!(service.snapshot.is_none());
        assert!(service.plan().is_none());
        service.change_policy(UpdatePolicy::StableOnly, None);
        assert_eq!(service.policy(), UpdatePolicy::StableOnly);
        let (request_id, policy_generation, _) = service.begin_check().unwrap();
        service.set_auto_check(true);
        service.change_policy(UpdatePolicy::StableAndBeta, None);
        assert!(!service.auto_check);
        assert_eq!(service.policy(), UpdatePolicy::StableOnly);
        service.checked_event(request_id, policy_generation, Err("offline".into()), None);
        service.set_auto_check(true);
        assert!(service.auto_check);
        service.checked(Err("offline".into()), None);
        service.set_auto_check(false);
        assert!(!service.auto_check);
        service.change_policy(UpdatePolicy::StableAndBeta, None);
        assert_eq!(service.policy(), UpdatePolicy::StableAndBeta);

        let stale = completed_history_proof(context, &observation).unwrap();
        let mut foreign = context.clone();
        foreign.instance_id = "different-original".into();
        service.context = Some(foreign);
        service.journal(
            &completed.operation_id,
            Ok(Some(observation.clone())),
            None,
            Some(stale),
        );
        assert!(!service.settings_admitted());
        service.context = Some(context.clone());
        let policy_proof = completed_history_proof(context, &observation).unwrap();
        service.journal(
            &completed.operation_id,
            Ok(Some(observation)),
            None,
            Some(policy_proof),
        );
        assert!(service.settings_admitted());
        service.journal(
            &completed.operation_id,
            Ok(Some(OperationObservation::Update(completed.clone()))),
            None,
            None,
        );
        assert!(!service.settings_admitted());
        let mut mismatched = snapshot.clone();
        mismatched.origin = InstallOrigin::Unknown {
            reason: "running source is not the checked source".into(),
        };
        assert!(settlement_proof(context, &mismatched, &completed.operation_id).is_err());
        let proof = settlement_proof(context, &snapshot, &completed.operation_id).unwrap();
        assert!(!service.settings_admitted());
        service.checked(Ok(snapshot), Some(proof));
        assert!(service.settings_admitted());
        assert!(service.plan().is_none());
        service.change_policy(UpdatePolicy::StableOnly, None);
        assert_eq!(service.policy(), UpdatePolicy::StableOnly);
        assert_eq!(service.previous_outcome.as_ref(), Some(&completed));
        assert_eq!(fs::read(&result_path).unwrap(), result_bytes);
        assert_eq!(fs::read(&plan_path).unwrap(), plan_bytes);
    }

    #[test]
    fn unknown_active_and_unreleased_outcomes_remain_blocked_after_check() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        let mut unknown = failed_record('a');
        unknown.phase = OperationPhase::Unknown;
        assert!(!settled_terminal(&unknown));
        service.update_latest_journal(Ok(Some(unknown)));
        service.checked(Ok(local_check()), None);
        assert!(service.plan().is_none());

        let mut active = failed_record('b');
        active.phase = OperationPhase::Installing;
        active.execution_fence = ExecutionFence::ManagerIntent;
        assert!(!settled_terminal(&active));
        service.update_latest_journal(Ok(Some(active)));
        service.checked(Ok(local_check()), None);
        assert!(service.plan().is_none());

        let failed_with_retained_marker = failed_record('c');
        service.update_latest_journal(Ok(Some(failed_with_retained_marker.clone())));
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
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        let old = failed_record('a');
        service.update_latest_journal(Ok(Some(old.clone())));
        let mut current = failed_record('b');
        current.phase = OperationPhase::Installing;
        current.execution_fence = ExecutionFence::ManagerIntent;
        service.update_latest_journal(Ok(Some(current.clone())));
        assert_eq!(service.previous_outcome.as_ref(), Some(&old));
        service.update_latest_journal(Ok(Some(old.clone())));
        assert_eq!(service.operation.as_ref(), Some(&current));
        service.checked(Ok(local_check()), Some(SettledOperationProof::Update(old)));
        assert_eq!(service.operation.as_ref(), Some(&current));
        assert_eq!(
            service.observed_operation_id.as_deref(),
            Some(current.operation_id.as_str())
        );
        assert!(service.retired_operation_id.is_none());
        assert!(service.plan().is_none());
    }

    #[test]
    fn failed_check_does_not_advance_successful_check_time() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, Some(11));
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
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, Some(11));
        service.update_latest_journal(Err("reserved operation has no valid journal".into()));
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
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        service.update_latest_journal(Ok(Some(OperationRecord {
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
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
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
        service.update_latest_journal(Ok(Some(record.clone())));
        let revision = service.revision();
        let model = service.model(UiLocale::En);
        assert_eq!(model.status, CheckStatus::NotChecked);
        assert!(!model.busy);
        assert!(model.settings_locked);
        assert!(!model.history.is_empty());
        assert!(model.primary_action.is_none());
        assert!(model.action.is_none());
        service.update_journal(operation_id, Ok(Some(record)), None);
        assert_eq!(service.revision(), revision);
    }

    #[test]
    fn completed_history_survives_check_retirement_and_never_replaces_current_discovery() {
        let snapshot = local_check();
        let context = snapshot.plan.as_ref().unwrap().context.clone();
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
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
        service.update_latest_journal(Ok(Some(completed.clone())));
        service.check_busy = true;
        let checking = service.model(UiLocale::Ko);
        assert!(checking.busy);
        assert_eq!(checking.status, CheckStatus::Checking);
        assert!(checking.action.is_none());
        let history = checking.history;

        service.checked(
            Ok(snapshot),
            Some(SettledOperationProof::Update(completed.clone())),
        );
        let discovered = service.model(UiLocale::Ko);
        assert!(!discovered.busy);
        assert_eq!(discovered.status, CheckStatus::Local);
        assert_eq!(discovered.action, Some(UpdateAction::SourceRebuild));
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
        service.update_latest_journal(Ok(Some(completed)));
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
    fn newer_completed_operation_replaces_retained_history() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        let prior = failed_record('a');
        service.update_latest_journal(Ok(Some(prior.clone())));
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::Update(prior)),
        );
        let prior_history = service.model(UiLocale::En).history;

        let mut newer = failed_record('b');
        newer.phase = OperationPhase::Starting;
        newer.execution_fence = ExecutionFence::ManagerIntent;
        service.update_latest_journal(Ok(Some(newer.clone())));
        let active = service.model(UiLocale::En);
        assert_eq!(active.status, CheckStatus::Checking);
        assert!(active.busy);
        assert!(active.action.is_none());
        assert_eq!(active.history, prior_history);

        newer.phase = OperationPhase::Completed;
        newer.execution_fence = ExecutionFence::ManagerExited;
        newer.installed = true;
        newer.applied = true;
        service.update_journal(&newer.operation_id, Ok(Some(newer.clone())), None);
        let completed = service.model(UiLocale::En);
        assert_eq!(completed.status, CheckStatus::Local);
        assert!(!completed.busy); // Completed history is not an active helper.
        assert!(completed.settings_locked); // Admission still requires settlement.
        assert!(completed.action.is_none());
        assert_ne!(completed.history, prior_history);

        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::Update(newer)),
        );
        let retired = service.model(UiLocale::En);
        assert_eq!(retired.status, CheckStatus::Local);
        assert_eq!(retired.action, Some(UpdateAction::SourceRebuild));
        assert_eq!(retired.history, completed.history);
    }

    #[test]
    fn unresolved_outcomes_remain_blocked_without_admission() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        let mut record = failed_record('a');
        record.phase = OperationPhase::Unknown;
        record.detail = "raw /isolated/uncertain".into();
        service.update_latest_journal(Ok(Some(record.clone())));
        service.checked(Ok(local_check()), None);
        let unknown = service.model(UiLocale::Ko);
        assert_eq!(unknown.status, CheckStatus::Failed);
        assert!(unknown.busy);
        assert!(unknown.action.is_none());
        assert_eq!(
            unknown.primary_action,
            Some(UpdateCardPrimaryAction::Recover)
        );
        assert!(service.begin_check().is_none());
        assert!(unknown.diagnostics.contains("raw /isolated/uncertain"));
        assert!(!unknown.detail.contains("/isolated/"));

        record.phase = OperationPhase::UserStopped;
        service.update_journal(&record.operation_id, Ok(Some(record.clone())), None);
        service.checked(Ok(local_check()), None);
        let stopped = service.model(UiLocale::Ko);
        assert_eq!(stopped.status, CheckStatus::Failed);
        assert!(stopped.action.is_none());
        assert!(!stopped.detail.contains("/isolated/"));

        record.phase = OperationPhase::Failed;
        service.update_journal(&record.operation_id, Ok(Some(record.clone())), None);
        assert_eq!(
            service.operation.as_ref().unwrap().phase,
            OperationPhase::UserStopped
        );
    }

    #[test]
    fn journal_transitions_advance_revision_without_duplicate_paints() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
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
        service.update_latest_journal(Ok(Some(record.clone())));
        let revision = service.revision();
        service.update_journal(operation_id, Ok(Some(record.clone())), None);
        assert_eq!(service.revision(), revision);
        record.phase = OperationPhase::Failed;
        record.detail = "apply failed".into();
        service.update_journal(operation_id, Ok(Some(record)), None);
        assert_ne!(service.revision(), revision);
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Failed);
    }
    #[test]
    fn ownership_ack_keeps_live_operation_busy_and_paints_pre_stop_journal() {
        let (mut service, plan) = pending_local_launch();
        let operation_id = plan.operation_id.as_str();
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
        service.update_journal(operation_id, Ok(Some(record.clone())), None);
        let card = service.model(UiLocale::En);
        assert!(card.busy);
        assert_eq!(card.status, CheckStatus::Checking);
        assert!(card.diagnostics.contains("awaiting original Stop"));
        assert!(!card.detail.contains("awaiting original Stop"));
        record.phase = OperationPhase::Failed;
        record.detail = "Stop reply failed".into();
        service.update_journal(operation_id, Ok(Some(record)), None);
        let failed = service.model(UiLocale::En);
        assert!(failed.busy); // The accepted preparation still owns its exact plan.
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert!(!service.settings_admitted());
        assert_eq!(failed.status, CheckStatus::Failed);
        assert!(failed.diagnostics.contains("Stop reply failed"));
        assert!(!failed.detail.contains("Stop reply failed"));
    }
    fn pending_local_launch() -> (AppUpdateService, UpdatePlan) {
        pending_launch(local_check())
    }

    fn pending_launch(snapshot: CheckSnapshot) -> (AppUpdateService, UpdatePlan) {
        let plan = snapshot.plan.as_ref().unwrap().clone();
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        service.context = Some(plan.context.clone());
        service.checked(Ok(snapshot), None);
        assert!(service.plan().is_some());
        service.launching = Some(plan.operation_id.clone());
        service.observed_operation_id = Some(plan.operation_id.clone());
        service.launch_plan = Some(plan.clone());
        service.launch_generation = Some(service.consent_generation());
        (service, plan)
    }

    #[test]
    fn failed_before_spawn_retires_only_its_missing_journal_and_fresh_check_admits_apply() {
        let (mut service, plan) = pending_local_launch();
        service.update_journal(&plan.operation_id, Ok(None), None);
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
            Some(UpdateAction::SourceRebuild)
        );
        service.update_journal(&plan.operation_id, Err("late poll".into()), None);
        assert_eq!(service.plan(), Some(&plan));
    }

    #[test]
    fn failed_after_spawn_keeps_uncertain_ownership_blocked_after_fresh_check() {
        let (mut service, plan) = pending_local_launch();
        service.update_journal(&plan.operation_id, Ok(None), None);
        assert!(!service.launch_result(
            &plan.operation_id,
            Err(LaunchFailure::Spawned("ownership reply lost".into())),
        ));
        service.checked(Ok(local_check()), None);
        assert!(service.plan().is_none());
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert!(service.launch_plan.is_none());
        assert_eq!(
            service.observed_operation_id.as_deref(),
            Some(plan.operation_id.as_str())
        );
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Failed);
    }

    #[test]
    fn spawned_stdout_failure_retains_accepted_prepare_for_later_locked_recovery() {
        let (mut service, plan) = pending_local_launch();
        service.launch_waiting(&plan.operation_id);
        assert_eq!(service.preparing_plan(&plan.operation_id), Some(&plan));
        assert!(!service.launch_result(
            &plan.operation_id,
            Err(LaunchFailure::Spawned("ownership stdout closed".into())),
        ));
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert!(service.launch_plan.is_none());
        let finished = failed_record('e');
        service.update_journal(&plan.operation_id, Ok(Some(finished.clone())), None);
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::Update(finished.clone())),
        );
        assert!(!service.settings_admitted());
        let mut wrong = plan.clone();
        wrong.running.sha256 = "another original".into();
        assert_eq!(
            service.update_journal(
                &plan.operation_id,
                Ok(Some(finished.clone())),
                Some(PreparedRecoveryProof {
                    record: finished.clone(),
                    plan: wrong,
                }),
            ),
            None
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert_eq!(
            service.update_journal(
                &plan.operation_id,
                Ok(Some(finished.clone())),
                Some(PreparedRecoveryProof {
                    record: finished.clone(),
                    plan: plan.clone(),
                }),
            ),
            Some(plan.context.clone())
        );
        service.recovery_reconciled(&plan.operation_id);
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::Update(finished)),
        );
        assert!(service.settings_admitted());
        assert_eq!(service.plan(), Some(&plan));
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
        assert!(service.launch_plan.is_none());
    }

    #[test]
    fn no_spawn_does_not_hide_an_independent_unverified_journal() {
        let (mut service, plan) = pending_local_launch();
        service.update_latest_journal(Err("other operation cannot be verified".into()));
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
            service.update_journal(&plan.operation_id, Ok(Some(unknown.clone())), None),
            None
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert!(service.plan().is_none());

        let recovered = unknown;
        let mut wrong_plan = plan.clone();
        wrong_plan.context.instance_id = "different original".into();
        assert_eq!(
            service.update_journal(
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
            service.update_journal(
                &plan.operation_id,
                Ok(Some(recovered.clone())),
                Some(PreparedRecoveryProof {
                    record: recovered,
                    plan: plan.clone(),
                }),
            ),
            Some(plan.context.clone())
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        service.recovery_reconciled(&plan.operation_id);
        assert!(service.prepared_plan.is_none());
    }
    #[test]
    fn stale_checked_result_cannot_replace_current_request_or_retire_an_operation() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, Some(19));
        let active = failed_record('a');
        service.update_latest_journal(Ok(Some(active.clone())));
        service.pending_request = Some(12);
        service.check_busy = true;
        let revision = service.revision();
        assert_eq!(
            service.checked_event(
                11,
                0,
                Ok(local_check()),
                Some(SettledOperationProof::Update(active.clone()))
            ),
            None
        );
        assert_eq!(service.pending_request, Some(12));
        assert!(service.check_busy);
        assert_eq!(service.last_checked, Some(19));
        assert!(service.snapshot.is_none());
        assert_eq!(service.operation.as_ref(), Some(&active));
        assert!(service.retired_operation_id.is_none());
        assert_eq!(service.revision(), revision);
        assert_eq!(
            service.checked_event(12, 0, Ok(local_check()), None),
            Some(42)
        );
        assert!(!service.check_busy);
    }

    #[test]
    fn direct_setting_admission_blocks_active_checks_preparation_and_unsettled_reservations() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, Some(19));
        let mut pending = failed_record('a');
        pending.phase = OperationPhase::Installing;
        for block in 0..4 {
            match block {
                0 => service.check_busy = true,
                1 => service.launching = Some(pending.operation_id.clone()),
                2 => service.prepared_plan = local_check().plan,
                _ => service.update_latest_journal(Ok(Some(pending.clone()))),
            }
            let before = service.policy();
            service.change_policy(UpdatePolicy::StableAndBeta, Some(29));
            service.set_auto_check(true);
            assert_eq!(service.policy(), before);
            assert!(!service.auto_check);
            service.check_busy = false;
            service.launching = None;
            service.prepared_plan = None;
            service.operation = None;
            service.observed_operation_id = None;
        }
    }

    #[test]
    fn confirmation_rejects_candidate_or_policy_changed_after_snapshot() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, Some(19));
        let original = local_check();
        let plan = original.plan.clone().unwrap();
        service.checked(Ok(original), None);
        let generation = service.consent_generation();
        assert!(service.consent_matches(&plan, generation));
        let mut replacement = local_check();
        let asset = "DesktopPet-v0.4.1-macos-arm64.tar.gz";
        let base = "https://github.com/hanbong5938/desktop-pet/releases/download/v0.4.1/";
        replacement.release = Some(ReleaseCandidate {
            version: "0.4.1".into(),
            channel: ReleaseChannel::Stable,
            tag: "v0.4.1".into(),
            release_id: 42,
            asset_name: asset.into(),
            archive_url: format!("{base}{asset}"),
            archive_size: 1234,
            sha256: "a".repeat(64),
            checksum_url: format!("{base}{asset}.sha256"),
        });
        service.checked(Ok(replacement.clone()), None);
        assert_eq!(service.plan(), Some(&plan));
        assert!(!service.consent_matches(&plan, generation));
        let refreshed_generation = service.consent_generation();
        replacement.plan.as_mut().unwrap().operation_id = "d".repeat(32);
        service.checked(Ok(replacement), None);
        assert!(!service.consent_matches(&plan, refreshed_generation));
        service.change_policy(UpdatePolicy::StableAndBeta, Some(23));
        assert!(service.plan().is_none());
        assert!(!service.consent_matches(&plan, generation));
        assert_eq!(service.last_checked, Some(23));
    }
    #[test]
    fn delayed_final_ack_notices_once_then_valid_reply_preserves_ownership() {
        let operation_id = "e".repeat(32);
        let valid = OwnershipAck {
            protocol: UPDATER_PROTOCOL,
            operation_id: operation_id.clone(),
            visible: true,
            prepared: true,
            error: None,
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut notices = 0;
        assert!(
            await_ownership_ack(receiver, &operation_id, Duration::ZERO, || {
                notices += 1;
                sender.send(Ok(valid)).unwrap();
            })
            .is_ok()
        );
        assert_eq!(notices, 1);

        let (sender, receiver) = mpsc::sync_channel(1);
        sender
            .send(Ok(OwnershipAck {
                protocol: UPDATER_PROTOCOL,
                operation_id: operation_id.clone(),
                visible: true,
                prepared: true,
                error: None,
            }))
            .unwrap();
        assert!(
            await_ownership_ack(receiver, &operation_id, Duration::ZERO, || {
                notices += 1;
            })
            .is_ok()
        );
        assert_eq!(notices, 1);
    }

    #[test]
    fn negative_or_broken_final_ack_is_spawned_failure_even_after_waiting() {
        let operation_id = "e".repeat(32);
        for (protocol, id, visible, prepared, error) in [
            (UPDATER_PROTOCOL + 1, operation_id.clone(), true, true, None),
            (UPDATER_PROTOCOL, "f".repeat(32), true, true, None),
            (UPDATER_PROTOCOL, operation_id.clone(), false, true, None),
            (UPDATER_PROTOCOL, operation_id.clone(), true, false, None),
            (
                UPDATER_PROTOCOL,
                operation_id.clone(),
                true,
                true,
                Some("rejected".into()),
            ),
        ] {
            let (sender, receiver) = mpsc::sync_channel(1);
            assert!(matches!(
                await_ownership_ack(receiver, &operation_id, Duration::ZERO, || {
                    sender
                        .send(Ok(OwnershipAck {
                            protocol,
                            operation_id: id,
                            visible,
                            prepared,
                            error,
                        }))
                        .unwrap();
                }),
                Err(LaunchFailure::Spawned(_))
            ));
        }
        for bytes in [
            Vec::new(),
            b"{\"protocol\":3".to_vec(),
            b"{invalid}\n".to_vec(),
            vec![b'a'; 4096],
        ] {
            assert!(read_ownership_ack(bytes.as_slice()).is_err());
        }
        let (sender, receiver) = mpsc::sync_channel::<Result<OwnershipAck, String>>(1);
        drop(sender);
        assert!(matches!(
            await_ownership_ack(receiver, &operation_id, Duration::ZERO, || {}),
            Err(LaunchFailure::Spawned(_))
        ));
    }

    #[test]
    fn waiting_still_accepts_prepare_and_transfers_only_on_current_final_ack() {
        let (mut service, plan) = pending_local_launch();
        service.launch_waiting(&plan.operation_id);
        assert!(service.launch_waiting);
        assert_eq!(service.preparing_plan(&plan.operation_id), Some(&plan));
        assert!(service.prepared_plan.is_none());
        assert!(!service.launch_result("f0000000000000000000000000000000", Ok(())));
        assert_eq!(service.launch_plan.as_ref(), Some(&plan));
        assert!(service.launch_result(&plan.operation_id, Ok(())));
        assert!(service.launch_plan.is_none());
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert!(service.launching.is_none());
        assert!(!service.launch_waiting);
        assert!(!service.launch_result(&plan.operation_id, Ok(())));
        assert!(service.preparing_plan(&plan.operation_id).is_none());
        assert!(!service.settings_admitted());
    }

    #[test]
    fn waiting_retains_exact_consent_and_terminal_disallows_prepare_or_late_ack() {
        let (mut service, plan) = pending_local_launch();
        let generation = service.launch_generation;
        service.launch_waiting("f0000000000000000000000000000000");
        assert!(!service.launch_waiting);
        service.launch_waiting(&plan.operation_id);
        assert!(service.launch_waiting);
        assert_eq!(service.launch_plan.as_ref(), Some(&plan));
        assert!(service.prepared_plan.is_none());
        assert_eq!(service.launch_generation, generation);
        assert_eq!(service.preparing_plan(&plan.operation_id), Some(&plan));
        assert!(!service.settings_admitted());
        let waiting = service.model(UiLocale::En);
        assert_eq!(waiting.status, CheckStatus::Checking);
        assert!(waiting.diagnostics.contains(&waiting.status_text));
        service.update_latest_journal(Ok(Some(failed_record('b'))));
        assert_eq!(
            service.observed_operation_id.as_deref(),
            Some(plan.operation_id.as_str())
        );
        assert_eq!(service.launch_plan.as_ref(), Some(&plan));

        let mut terminal = failed_record('e');
        terminal.phase = OperationPhase::UserStopped;
        service.update_journal(&plan.operation_id, Ok(Some(terminal.clone())), None);
        assert!(service.launch_plan.is_none());
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert!(service.preparing_plan(&plan.operation_id).is_none());
        assert!(!service.launch_waiting);
        assert!(!service.launch_result(&plan.operation_id, Ok(())));
        service.launch_waiting(&plan.operation_id);
        assert!(!service.launch_waiting);
        let mut old = terminal.clone();
        old.phase = OperationPhase::Preparing;
        service.update_journal(&plan.operation_id, Ok(Some(old)), None);
        assert_eq!(service.operation.as_ref(), Some(&terminal));
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert!(!service.settings_admitted());
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::Update(terminal.clone())),
        );
        assert!(!service.settings_admitted());
        service.preparation_cancelled(&plan.operation_id);
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::Update(terminal)),
        );
        assert!(service.settings_admitted());
        assert_eq!(service.plan(), Some(&plan));
    }

    #[test]
    fn terminal_before_ack_waits_for_a_later_exact_recovery_proof() {
        let (mut service, plan) = pending_local_launch();
        service.launch_waiting(&plan.operation_id);
        let finished = failed_record('e');
        assert_eq!(
            service.update_journal(&plan.operation_id, Ok(Some(finished.clone())), None),
            None
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert!(!service.launch_result(&plan.operation_id, Ok(())));
        assert!(service.preparing_plan(&plan.operation_id).is_none());
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::Update(finished.clone())),
        );
        assert!(!service.settings_admitted());
        assert!(service.plan().is_none());
        assert_eq!(
            service.update_journal(
                &plan.operation_id,
                Ok(Some(finished.clone())),
                Some(PreparedRecoveryProof {
                    record: finished.clone(),
                    plan: plan.clone(),
                }),
            ),
            Some(plan.context.clone())
        );
        assert!(!service.settings_admitted());
        service.recovery_reconciled(&plan.operation_id);
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::Update(finished)),
        );
        assert!(service.settings_admitted());
        assert_eq!(service.plan(), Some(&plan));
    }

    #[test]
    fn final_ack_promotes_once_and_late_proof_holds_settings_until_reconciled_current_check() {
        let (mut service, plan) = pending_local_launch();
        assert_eq!(service.preparing_plan(&plan.operation_id), Some(&plan));
        assert!(service.prepared_plan.is_none());
        assert_eq!(
            service.launch_generation,
            Some(service.consent_generation())
        );
        assert!(service.launch_result(&plan.operation_id, Ok(())));
        assert!(service.launch_plan.is_none());
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert!(service.launching.is_none());
        assert!(!service.launch_result(&plan.operation_id, Ok(())));
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        // A late startup discovery of another ID cannot unpin the owned plan.
        let mut other = failed_record('b');
        other.phase = OperationPhase::Installing;
        service.update_latest_journal(Ok(Some(other)));
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert_eq!(
            service.observed_operation_id.as_deref(),
            Some(plan.operation_id.as_str())
        );
        let finished = failed_record('e');
        service.update_journal(&plan.operation_id, Ok(Some(finished.clone())), None);
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::Update(finished.clone())),
        );
        assert!(!service.settings_admitted());
        assert_eq!(
            service.observed_operation_id.as_deref(),
            Some(plan.operation_id.as_str())
        );
        assert_eq!(
            service.update_journal(
                &plan.operation_id,
                Ok(Some(finished.clone())),
                Some(PreparedRecoveryProof {
                    record: finished.clone(),
                    plan: plan.clone(),
                }),
            ),
            Some(plan.context.clone())
        );
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        service.recovery_reconciled(&plan.operation_id);
        service.checked(
            Ok(local_check()),
            Some(SettledOperationProof::Update(finished)),
        );
        assert!(service.settings_admitted());
        assert_eq!(service.plan(), Some(&plan));
    }

    #[test]
    fn expired_final_ack_reports_failure_without_releasing_the_owned_plan() {
        let (mut service, plan) = pending_local_launch();
        assert!(service.launch_result(&plan.operation_id, Ok(())));
        service.abort_launch("f0000000000000000000000000000000", "wrong operation".into());
        assert!(service.problem.is_none());
        service.abort_launch(&plan.operation_id, "native preparation expired".into());
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert_eq!(
            service.observed_operation_id.as_deref(),
            Some(plan.operation_id.as_str())
        );
        assert_eq!(
            service.problem.as_deref(),
            Some("native preparation expired")
        );
        assert!(!service.settings_admitted());
        assert!(service.plan().is_none());
    }

    #[test]
    fn native_cancellation_retains_plan_until_exact_independent_settlement() {
        let (mut service, plan) = pending_local_launch();
        service.preparation_cancelled("f0000000000000000000000000000000");
        assert!(service.cancelled_preparation.is_none());
        service.preparation_cancelled(&plan.operation_id);
        assert_eq!(service.launch_plan.as_ref(), Some(&plan));
        assert_eq!(
            service.cancelled_preparation.as_deref(),
            Some(plan.operation_id.as_str())
        );
        assert!(service.launch_result(&plan.operation_id, Ok(())));
        let finished = failed_record('e');
        service.update_journal(&plan.operation_id, Ok(Some(finished.clone())), None);
        service.checked(Ok(local_check()), None);
        assert_eq!(service.prepared_plan.as_ref(), Some(&plan));
        assert!(!service.settings_admitted());
        let mut current = local_check();
        current.status = CheckStatus::Current;
        current.plan = None;
        service.checked(Ok(current), Some(SettledOperationProof::Update(finished)));
        assert!(service.prepared_plan.is_none());
        assert!(service.settings_admitted());
        assert_eq!(service.model(UiLocale::En).status, CheckStatus::Current);
    }

    #[test]
    fn changed_consent_and_inflight_poll_cannot_reauthorize_preparing() {
        let (mut service, plan) = pending_local_launch();
        service.journal_in_flight = Some(plan.operation_id.clone());
        service.next_journal_poll = Instant::now();
        service.poll_operation();
        assert_eq!(
            service.journal_in_flight.as_deref(),
            Some(plan.operation_id.as_str())
        );
        service.candidate_generation += 1;
        assert!(service.preparing_plan(&plan.operation_id).is_none());
        assert_eq!(service.launch_plan.as_ref(), Some(&plan));
        assert!(!service.settings_admitted());
    }

    #[test]
    fn failed_timestamp_commit_preserves_original_schedule_and_does_not_retire_journal() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableAndBeta, Some(19));
        let journal = failed_record('c');
        service.update_latest_journal(Ok(Some(journal.clone())));
        service.next_auto_due = Some(500);
        service.scheduled_due_before = Some(Some(90));
        service.pending_request = Some(7);
        service.check_busy = true;
        service.mark_check_save_failure("disk unavailable".into());
        assert_eq!(service.last_checked, Some(19));
        assert_eq!(service.next_auto_due, Some(90));
        assert!(service.auto_failure_paused);
        assert_eq!(service.operation.as_ref(), Some(&journal));
        assert!(service.retired_operation_id.is_none());
        assert!(service.snapshot.is_none());
        assert!(service.pending_request.is_none());
        assert!(!service.check_busy);
        assert!(service.plan().is_none());
    }

    #[test]
    fn relocation_observation_retires_terminal_parent_and_settles_cancelled_attempt() {
        let mut service = admitted_card_service(UpdatePolicy::StableOnly);
        let home = desktop_pet_update_coordinator::installation::os_home().unwrap();
        service.context.as_mut().unwrap().running_origin =
            Some(Box::new(InstallOrigin::Standalone {
                bundle: home.join("Applications/DesktopPet.app"),
            }));
        let context = service.context.clone().unwrap();
        let mut parent = failed_record('b');
        parent.phase = OperationPhase::Completed;
        parent.execution_fence = ExecutionFence::ManagerExited;
        parent.installed = true;
        parent.applied = true;
        service.latest_journal(Ok(Some(OperationObservation::Update(parent.clone()))));
        let cancelled = standalone_observation(InstallStatus::Cancelled);
        // Do not spawn an observer: deliver its real journal event below.
        service.journal_in_flight = Some(parent.operation_id.clone());
        service.relocation_finished(context.clone(), cancelled.operation_id.clone(), Ok(()));
        service.journal(
            &cancelled.operation_id,
            Ok(Some(OperationObservation::StandaloneInstall(
                cancelled.clone(),
            ))),
            None,
            None,
        );
        assert_eq!(service.standalone_operation.as_ref(), Some(&cancelled));
        assert_eq!(service.previous_outcome.as_ref(), Some(&parent));
        assert!(service.operation.is_none());
        assert!(service.begin_check().is_some());
        let mut snapshot = local_check_for_context(context);
        snapshot.origin = *service
            .context
            .as_ref()
            .unwrap()
            .running_origin
            .clone()
            .unwrap();
        snapshot.plan = None;
        snapshot.status = CheckStatus::Current;
        service.checked(
            Ok(snapshot),
            Some(SettledOperationProof::StandaloneInstall(cancelled.clone())),
        );
        assert!(service.standalone_operation.is_none());
        assert_eq!(
            service.previous_standalone_outcome.as_ref(),
            Some(&cancelled)
        );
        assert!(service.settings_admitted());
        assert_eq!(
            service.model(UiLocale::En).relocation_available,
            !desktop_pet_update_coordinator::identity::LEGACY,
        );
    }

    #[test]
    fn relocation_observation_does_not_displace_unresolved_parent_or_stale_context() {
        let mut service = admitted_card_service(UpdatePolicy::StableOnly);
        let context = service.context.clone().unwrap();
        let mut parent = failed_record('b');
        parent.phase = OperationPhase::Starting;
        service.latest_journal(Ok(Some(OperationObservation::Update(parent.clone()))));
        service.journal_in_flight = Some(parent.operation_id.clone());
        service.relocation_finished(context.clone(), "a".repeat(32), Ok(()));
        assert_eq!(
            service.observed_operation_id.as_deref(),
            Some(parent.operation_id.as_str())
        );
        assert_eq!(service.operation.as_ref(), Some(&parent));
        let mut stale = context;
        stale.instance_id.push_str("-stale");
        service.relocation_finished(stale, "c".repeat(32), Err("stale helper".into()));
        assert_eq!(
            service.observed_operation_id.as_deref(),
            Some(parent.operation_id.as_str())
        );
        assert!(service.problem.is_none());
    }

    #[test]
    fn relocation_observation_preserves_unresolved_standalone_priority() {
        for status in [
            InstallStatus::Running,
            InstallStatus::Unknown,
            InstallStatus::InstalledNotApplied,
        ] {
            let mut service = admitted_card_service(UpdatePolicy::StableOnly);
            let context = service.context.clone().unwrap();
            let unresolved = standalone_observation(status);
            service.latest_journal(Ok(Some(OperationObservation::StandaloneInstall(
                unresolved.clone(),
            ))));
            service.journal_in_flight = Some(unresolved.operation_id.clone());
            service.relocation_finished(context, "c".repeat(32), Ok(()));
            assert_eq!(
                service.observed_operation_id.as_deref(),
                Some(unresolved.operation_id.as_str())
            );
            assert_eq!(service.standalone_operation.as_ref(), Some(&unresolved));
        }
    }

    #[test]
    fn relocation_supersession_cancels_automatic_continuation() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        let context = local_check().plan.unwrap().context;
        service.context = Some(context.clone());
        service.relocation_waiting = true;
        service.relocation_admission(context, Ok(RelocationAdmission::Superseded));
        assert!(!service.relocation_waiting);
        assert!(service.relocation_auto_attempted);
        assert!(service.relocation_parent.is_none());
    }

    #[test]
    fn stale_relocation_events_cannot_change_current_admission() {
        let mut service = AppUpdateService::new(None, false, UpdatePolicy::StableOnly, None);
        let context = local_check().plan.unwrap().context;
        service.relocation_admission(context.clone(), Ok(RelocationAdmission::Waiting));
        assert!(!service.relocation_waiting);
        service.relocation_finished(context, "a".repeat(32), Err("stale".into()));
        assert!(service.problem.is_none());
        assert!(service.observed_operation_id.is_none());
    }

    #[test]
    fn claimed_automatic_relocation_is_not_launched_again_by_ui_ticks() {
        let mut service = admitted_card_service(UpdatePolicy::StableOnly);
        let home = desktop_pet_update_coordinator::installation::os_home().unwrap();
        service.context.as_mut().unwrap().running_origin =
            Some(Box::new(InstallOrigin::Standalone {
                bundle: home.join("Applications/DesktopPet.app"),
            }));
        let context = service.context.as_ref().unwrap();
        service.relocation_parent = Some(SettledRelocationParent {
            version: 1,
            operation_id: "a".repeat(32),
            plan_sha256: "b".repeat(64),
            generation: 0,
            candidate_image: context.running.clone(),
            source_closure_sha256: "c".repeat(64),
            profile: desktop_pet_update_coordinator::native_runtime::ProfilePaths {
                config_dir: context.config_dir.clone(),
                state_dir: context.state_dir.clone(),
            },
            result_sha256: "d".repeat(64),
        });
        service.relocation_auto_attempted = true;
        service.relocation_polled = true;
        for _ in 0..3 {
            service.poll_relocation();
        }
        assert!(!service.relocation_busy);
        assert!(service.receiver.try_recv().is_err());
    }

    #[cfg(not(desktop_pet_legacy))]
    #[test]
    fn relocation_summary_preserves_waiting_preparing_and_operation_priority() {
        let mut service = admitted_card_service(UpdatePolicy::StableOnly);
        let home = desktop_pet_update_coordinator::installation::os_home().unwrap();
        service.context.as_mut().unwrap().running_origin =
            Some(Box::new(InstallOrigin::Standalone {
                bundle: home.join("Applications/DesktopPet.app"),
            }));
        for locale in [UiLocale::Ko, UiLocale::En, UiLocale::Ja, UiLocale::ZhHans] {
            for (waiting, busy, expected) in [
                (false, false, Message::RelocationReady),
                (false, true, Message::RelocationPreparing),
                (true, false, Message::RelocationWaiting),
                (true, true, Message::RelocationWaiting),
            ] {
                service.relocation_waiting = waiting;
                service.relocation_busy = busy;
                let model = service.model(locale);
                assert_eq!(model.relocation_summary, text(locale, expected));
                assert_eq!(
                    completed_history_model(&service, locale).relocation_summary,
                    model.relocation_summary
                );
            }
            service.problem = Some("SETTINGS_RAW_SERVICE_ERROR".into());
            assert_eq!(service.model(locale).status, CheckStatus::Failed);
            assert!(service.model(locale).relocation_summary.is_empty());
            service.problem = None;
            service.check_busy = true;
            assert!(service.model(locale).relocation_summary.is_empty());
            service.check_busy = false;
            service.launching = Some("a".repeat(32));
            assert!(service.model(locale).relocation_summary.is_empty());
            service.launching = None;
            let mut active = failed_record('a');
            active.phase = OperationPhase::Starting;
            service.operation = Some(active);
            assert!(service.model(locale).relocation_summary.is_empty());
            service.operation = None;
            service.standalone_operation = Some(standalone_observation(InstallStatus::Running));
            assert!(service.model(locale).relocation_summary.is_empty());
            service.standalone_operation = None;
            service.journal_error = Some("SETTINGS_RAW_JOURNAL_ERROR".into());
            assert!(service.model(locale).relocation_summary.is_empty());
            service.journal_error = None;
        }
    }

    #[test]
    fn native_success_remains_success_when_tern_is_pending_then_counts_fresh_windows() {
        let mut service = admitted_card_service(UpdatePolicy::StableOnly);
        let context = service.context.clone().unwrap();
        let mut record = standalone_observation(InstallStatus::Applied);
        record.result.installed = true;
        record.result.applied = true;
        let id = record.operation_id.clone();
        service.standalone_operation = Some(record.clone());
        service.relocation_tern_observed(
            context.clone(),
            id.clone(),
            Ok(RelocationTernObservation {
                captured: true,
                windows: 0,
                pending: true,
                detail: Some("consumer disconnected".into()),
            }),
        );
        assert_eq!(service.standalone_operation.as_ref(), Some(&record));
        assert!(service.problem.is_none());
        let pending = service.model(UiLocale::Ko);
        assert_eq!(
            pending.relocation_summary,
            text(UiLocale::Ko, Message::RelocationTernPending)
        );
        assert!(pending
            .relocation_detail
            .contains(text(UiLocale::Ko, Message::RelocationComplete)));
        assert!(pending
            .relocation_detail
            .contains(text(UiLocale::Ko, Message::RelocationTernPending)));
        service.relocation_tern_observed(
            context.clone(),
            id.clone(),
            Ok(RelocationTernObservation {
                captured: true,
                windows: 2,
                pending: false,
                detail: None,
            }),
        );
        let confirmed = service.model(UiLocale::Ko);
        assert_eq!(
            confirmed.relocation_summary,
            text(UiLocale::Ko, Message::RelocationComplete)
        );
        service.check_busy = true;
        assert!(service.model(UiLocale::Ko).relocation_summary.is_empty());
        service.check_busy = false;
        service.launching = Some("f".repeat(32));
        assert!(service.model(UiLocale::Ko).relocation_summary.is_empty());
        service.launching = None;
        let mut active = failed_record('f');
        active.phase = OperationPhase::Starting;
        service.operation = Some(active);
        assert!(service.model(UiLocale::Ko).relocation_summary.is_empty());
        service.operation = None;
        service.journal_error = Some("SETTINGS_RAW_JOURNAL_ERROR".into());
        assert_eq!(service.model(UiLocale::Ko).status, CheckStatus::Failed);
        assert!(service.model(UiLocale::Ko).relocation_summary.is_empty());
        service.journal_error = None;
        assert!(confirmed
            .relocation_detail
            .contains(&text(UiLocale::Ko, Message::RelocationTernWindows).replace("{}", "2")));
        assert!(!confirmed
            .relocation_detail
            .contains(text(UiLocale::Ko, Message::RelocationTernPending)));
        assert_eq!(service.standalone_operation.as_ref(), Some(&record));
        let mut stale = context;
        stale.instance_id.push_str("-stale");
        service.relocation_tern_observed(
            stale,
            id,
            Ok(RelocationTernObservation {
                captured: true,
                windows: 99,
                pending: false,
                detail: None,
            }),
        );
        assert_eq!(service.relocation_tern.as_ref().unwrap().1.windows, 2);
    }

    #[test]
    fn waiting_and_eligible_events_never_launch_a_helper_from_event_delivery() {
        let mut service = admitted_card_service(UpdatePolicy::StableOnly);
        let context = service.context.clone().unwrap();
        service.relocation_admission(context.clone(), Ok(RelocationAdmission::Waiting));
        assert!(service.relocation_waiting);
        assert!(!service.relocation_busy);
        assert!(service.relocation_parent.is_none());
        assert!(service.receiver.try_recv().is_err());
        let parent = SettledRelocationParent {
            version: 1,
            operation_id: "a".repeat(32),
            plan_sha256: "b".repeat(64),
            generation: 0,
            candidate_image: context.running.clone(),
            source_closure_sha256: "c".repeat(64),
            profile: desktop_pet_update_coordinator::native_runtime::ProfilePaths {
                config_dir: context.config_dir.clone(),
                state_dir: context.state_dir.clone(),
            },
            result_sha256: "d".repeat(64),
        };
        service.relocation_admission(context, Ok(RelocationAdmission::Eligible(parent.clone())));
        assert_eq!(service.relocation_parent, Some(parent));
        assert!(!service.relocation_busy);
        assert!(!service.relocation_auto_attempted);
        assert!(service.receiver.try_recv().is_err());
    }
}
