use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

pub const MAX_AGENT_RECORDS: usize = 16_384;
pub const MAX_SOURCES: usize = 64;
pub const MAX_COUNT: u32 = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
}

/// A normalized outcome, not a host metadata token parser.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AgentOutcome {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl AgentOutcome {
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
}

/// Validated source-owned outcome identity. Adapters validate wire authority,
/// token bounds, numeric ordering, and freshness before publishing it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutcomeReport {
    pub session: String,
    pub turn: String,
    pub outcome: AgentOutcome,
    pub at_unix_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceWorktreeInfo {
    pub repo_key: String,
    pub repo_name: String,
    pub repo_root: String,
    pub checkout_path: String,
    pub is_linked_worktree: bool,
    pub pane_count: usize,
    pub tab_count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionMetadata {
    pub title: Option<String>,
    pub agent: Option<String>,
    pub workspace_id: Option<String>,
    pub worktree: Option<Arc<WorkspaceWorktreeInfo>>,
    pub workspace_label: Option<String>,
    pub tab_id: Option<String>,
    pub tab_label: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentRecord {
    pub terminal_id: String,
    pub pane_id: String,
    pub status: AgentStatus,
    pub metadata: SessionMetadata,
    pub outcome_authoritative: bool,
    pub outcome: Option<OutcomeReport>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    pub panes: Vec<String>,
    pub agents: Vec<AgentRecord>,
}

impl Snapshot {
    pub fn pane_ids(&self) -> Vec<String> {
        let mut panes = self.panes.clone();
        panes.sort_unstable();
        panes.dedup();
        panes
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SnapshotCounts {
    pub sessions: usize,
    pub working: usize,
    pub blocked: usize,
    pub done: usize,
    pub unknown: usize,
}

/// A continuous active-to-Done transition observed for the same terminal and
/// pane by the identified source generation. This is not prompt acceptance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionObservation {
    pub source: String,
    pub generation: u64,
    pub terminal_id: String,
    pub pane_id: String,
    pub observed_at: Instant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutcomeObservation {
    pub source: String,
    pub generation: u64,
    pub terminal_id: String,
    pub pane_id: String,
    pub outcome: AgentOutcome,
    pub at_unix_ms: u64,
    pub observed_at: Instant,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceCounts {
    pub sessions: usize,
    pub working: usize,
    pub blocked: usize,
    pub done: usize,
    pub unknown: usize,
}

impl SourceCounts {
    pub fn disconnected(self) -> Self {
        let sessions = self.sessions.min(MAX_COUNT as usize);
        Self {
            sessions,
            working: 0,
            blocked: 0,
            done: 0,
            unknown: sessions,
        }
    }

    pub fn bounded(self) -> Self {
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

pub struct SessionStatusUpdate<'a> {
    pub source: &'a str,
    pub generation: u64,
    pub terminal_id: &'a str,
    pub pane_id: &'a str,
    pub status: AgentStatus,
    pub counts: SourceCounts,
    pub completion: bool,
    pub observed_at: Instant,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionKey {
    pub source_id: u64,
    pub generation: u64,
    pub terminal_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptTarget {
    pub source: String,
    pub pane_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromptTargetError {
    ReadOnly,
    Offline,
    Stale,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeRemoveTarget {
    pub key: SessionKey,
    pub source: String,
    pub pane_id: String,
    pub workspace_id: String,
    pub worktree: Arc<WorkspaceWorktreeInfo>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorktreeRemoveTargetError {
    ReadOnly,
    Offline,
    Stale,
    NotLinkedWorktree,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceAdmission {
    Allowed,
    Capacity,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestOrigin {
    Gui,
    Cli { operation_id: String },
}

#[derive(Debug)]
pub struct PromptSubmission {
    pub key: SessionKey,
    pub text: String,
    pub origin: RequestOrigin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PromptError {
    Empty,
    TooLarge,
    Busy,
    Offline,
    ReadOnly,
    StaleTarget,
    Blocked,
    NotReady,
    Unsupported,
    UnknownDelivery,
    Other(String),
}

impl From<PromptTargetError> for PromptError {
    fn from(error: PromptTargetError) -> Self {
        match error {
            PromptTargetError::ReadOnly => Self::ReadOnly,
            PromptTargetError::Offline => Self::Offline,
            PromptTargetError::Stale => Self::StaleTarget,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromptReceipt {
    /// Existing `agent_prompted`, committed:true semantics.
    Acknowledged,
    /// Existing `prompt_accepted`, committed:false semantics. Not completion.
    Accepted,
}

#[derive(Debug)]
pub struct PromptResult {
    pub submission: Arc<PromptSubmission>,
    pub result: Result<PromptReceipt, PromptError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorktreeRemoveError {
    Busy,
    Offline,
    ReadOnly,
    StaleTarget,
    NotLinkedWorktree,
    Unsupported,
    UnknownDelivery,
    Rejected { code: String, message: String },
    Other(String),
}

impl From<WorktreeRemoveTargetError> for WorktreeRemoveError {
    fn from(error: WorktreeRemoveTargetError) -> Self {
        match error {
            WorktreeRemoveTargetError::ReadOnly => Self::ReadOnly,
            WorktreeRemoveTargetError::Offline => Self::Offline,
            WorktreeRemoveTargetError::Stale => Self::StaleTarget,
            WorktreeRemoveTargetError::NotLinkedWorktree => Self::NotLinkedWorktree,
        }
    }
}

#[derive(Debug)]
pub struct WorktreeRemoveResult {
    pub target: WorktreeRemoveTarget,
    pub origin: RequestOrigin,
    pub result: Result<(), WorktreeRemoveError>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceLocality {
    Local,
    Remote,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceCapabilities {
    pub prompt: bool,
    pub remove_worktree: bool,
}

/// Sole domain authority for locality and operations. IDs and provider keys
/// are opaque; neither metadata paths nor ID prefixes grant capabilities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceDescriptor {
    pub id: String,
    pub provider: String,
    pub label: String,
    pub locality: SourceLocality,
    pub capabilities: SourceCapabilities,
}

/// Counts keyed by registered opaque provider ID. Serialized directly as the
/// existing host_connections JSON object, including registered zero counts.
pub type HostConnections = std::collections::BTreeMap<String, usize>;
