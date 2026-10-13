use crate::herdr::{PromptError, PromptReceipt};
use crate::session::{AgentRecord, AgentStatus, SessionMetadata, MAX_AGENT_RECORDS};
use crate::session_view::SessionKey;
use crate::sources::SourceRoute;
use crate::state::SourceCounts;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};
pub(crate) const MAX_WINDOWS: usize = 64;
pub(crate) const MAX_HOSTS: usize = 64;
pub(crate) const MAX_HOST_AGENTS: usize = 128;
pub(crate) const MAX_HOST_ERROR_BYTES: usize = 512;
// Request JSON only; the optional line terminator is not part of this budget.
pub(crate) const MAX_EXCHANGE_BYTES: usize = 256 * 1024;
pub(crate) const MAX_RESPONSE_BYTES: usize = 512 * 1024;
pub(crate) const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub(crate) const FRESHNESS: Duration = Duration::from_secs(5);
pub(crate) const OFFLINE_GRACE: Duration = Duration::from_secs(30);
const MAX_FIELD_BYTES: usize = 4096;
static NEXT_COMMAND: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TernExchangeRequest {
    pub version: u64,
    pub kind: String,
    pub window: String,
    pub epoch: u64,
    pub sequence: u64,
    pub instance_id: Option<String>,
    pub hosts: Vec<TernHostInventory>,
    pub results: Vec<TernResult>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TernHostInventory {
    pub host: String,
    pub label: String,
    pub is_local: bool,
    pub observation_generation: u64,
    pub status: TernHostStatus,
    pub agents: Vec<TernAgent>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TernHostStatus {
    Available,
    Unavailable,
    Disconnected,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TernHostReply {
    pub host: String,
    pub observation_generation: u64,
    pub generation: u64,
    pub error: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_error_code",
        skip_serializing_if = "Option::is_none"
    )]
    pub error_code: Option<TernExchangeErrorCode>,
}

impl TernHostReply {
    pub(crate) fn new(host: &TernHostInventory, generation: u64, error: Option<String>) -> Self {
        let error = error.map(|mut value| {
            truncate_utf8(&mut value, MAX_HOST_ERROR_BYTES);
            value
        });
        Self {
            host: host.host.clone(),
            observation_generation: host.observation_generation,
            generation,
            error_code: error.as_ref().map(|_| TernExchangeErrorCode::Rejected),
            error,
        }
    }
    pub(crate) fn reject(&mut self, mut error: String) {
        truncate_utf8(&mut error, MAX_HOST_ERROR_BYTES);
        self.error = Some(error);
        self.error_code = Some(TernExchangeErrorCode::Rejected);
    }
}

fn truncate_utf8(value: &mut String, maximum: usize) {
    if value.len() > maximum {
        let mut end = maximum;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TernAgent {
    pub pane: String,
    pub title: String,
    pub cwd: String,
    pub session_id: Option<String>,
    pub session_label: Option<String>,
    pub tab_id: Option<String>,
    pub tab_label: Option<String>,
    pub state: TernAgentState,
    pub delivery_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TernAgentState {
    Idle,
    Working,
    WaitingInput,
    Exited,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TernResult {
    pub operation_id: String,
    pub pane: String,
    pub status: TernResultStatus,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TernResultStatus {
    Accepted,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TernCommand {
    pub operation_id: String,
    pub pane: String,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TernExchangeErrorCode {
    BindingRequired,
    InstanceChanged,
    SourceRetired,
    ReceiptRejected,
    Rejected,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TernExchangeReply {
    pub ok: bool,
    pub instance_id: String,
    pub hosts: Vec<TernHostReply>,
    pub command: Option<TernCommand>,
    pub error: Option<String>,
    #[serde(default, deserialize_with = "deserialize_error_code")]
    pub error_code: Option<TernExchangeErrorCode>,
}

impl TernExchangeReply {
    pub(crate) fn error(
        instance_id: String,
        code: TernExchangeErrorCode,
        mut error: String,
    ) -> Self {
        truncate_utf8(&mut error, MAX_FIELD_BYTES);
        Self {
            ok: false,
            instance_id,
            hosts: Vec::new(),
            command: None,
            error: Some(error),
            error_code: Some(code),
        }
    }
}

fn deserialize_error_code<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<TernExchangeErrorCode>, D::Error> {
    TernExchangeErrorCode::deserialize(deserializer).map(Some)
}

pub(crate) fn serialize_response(reply: &TernExchangeReply) -> serde_json::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(reply)?;
    bytes.push(b'\n');
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(serde_json::Error::io(std::io::Error::other(
            "Tern response size admission invariant failed",
        )));
    }
    Ok(bytes)
}

#[derive(Serialize)]
struct CommandRef<'a> {
    operation_id: &'a str,
    pane: &'a str,
    text: &'a str,
}

#[derive(Serialize)]
struct ReplyRef<'a> {
    ok: bool,
    instance_id: &'a str,
    hosts: &'a [TernHostReply],
    command: Option<CommandRef<'a>>,
    error: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<TernExchangeErrorCode>,
}

impl Serialize for TernExchangeReply {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ReplyRef {
            ok: self.ok,
            instance_id: &self.instance_id,
            hosts: &self.hosts,
            command: self.command.as_ref().map(|command| CommandRef {
                operation_id: &command.operation_id,
                pane: &command.pane,
                text: &command.text,
            }),
            error: self.error.as_deref(),
            error_code: self.error_code,
        }
        .serialize(serializer)
    }
}

#[derive(Default)]
struct ResponseCounter {
    bytes: usize,
    too_large: bool,
}

impl std::io::Write for ResponseCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > (MAX_RESPONSE_BYTES - 1).saturating_sub(self.bytes) {
            self.too_large = true;
            return Err(std::io::Error::other("Tern response is too large"));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Count the same borrowed envelope used by the real reply serializer, including
/// the helper's trailing LF, without allocating a second copy of the text.
pub(crate) fn check_command_response(
    instance_id: &str,
    host_replies_json_bytes: usize,
    operation_id: &str,
    pane: &str,
    text: &str,
) -> Result<(), PromptError> {
    let reply = ReplyRef {
        ok: true,
        instance_id,
        hosts: &[],
        command: Some(CommandRef {
            operation_id,
            pane,
            text,
        }),
        error: None,
        error_code: None,
    };
    let mut counter = ResponseCounter::default();
    let result = serde_json::to_writer(&mut counter, &reply);
    if counter.too_large {
        return Err(PromptError::TooLarge);
    }
    result.map_err(|error| PromptError::Other(error.to_string()))?;
    let bytes = host_replies_json_bytes
        .checked_sub(2)
        .and_then(|extra| counter.bytes.checked_add(extra))
        .and_then(|size| size.checked_add(1))
        .ok_or(PromptError::TooLarge)?;
    if bytes > MAX_RESPONSE_BYTES {
        Err(PromptError::TooLarge)
    } else {
        Ok(())
    }
}

pub(crate) fn host_reply_bytes(hosts: &[TernHostReply]) -> Result<usize, PromptError> {
    let mut counter = ResponseCounter::default();
    serde_json::to_writer(&mut counter, hosts).map_err(|error| {
        if counter.too_large {
            PromptError::TooLarge
        } else {
            PromptError::Other(error.to_string())
        }
    })?;
    Ok(counter.bytes)
}

pub(crate) fn canonical_host(value: &str) -> bool {
    !value.is_empty()
        && (value == "0" || !value.starts_with('0'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|id| id <= MAX_SAFE_INTEGER)
}

fn bounded(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_FIELD_BYTES && !value.contains('\0')
}
fn optional_bounded(value: &Option<String>) -> bool {
    value.as_ref().is_none_or(|value| bounded(value))
}
fn optional_sized(value: &Option<String>) -> bool {
    value
        .as_ref()
        .is_none_or(|value| value.len() <= MAX_FIELD_BYTES && !value.contains('\0'))
}

impl TernExchangeRequest {
    /// Validate inventory shape before touching any source or receipt.
    /// Production ingress checks the raw JSON byte limit before decoding.
    pub(crate) fn normalize(&self) -> Result<Vec<(Vec<AgentRecord>, SourceCounts)>, String> {
        if self.version != 2
            || self.kind != "tern"
            || !bounded(&self.window)
            || self.epoch == 0
            || self.epoch > MAX_SAFE_INTEGER
            || self.sequence == 0
            || self.sequence > MAX_SAFE_INTEGER
            || !optional_bounded(&self.instance_id)
            || self.hosts.is_empty()
            || self.hosts.len() > MAX_HOSTS
            || self.hosts.iter().filter(|host| host.is_local).count() != 1
            || self
                .hosts
                .iter()
                .map(|host| host.agents.len())
                .sum::<usize>()
                > MAX_AGENT_RECORDS
            || self.results.len() > 1
        {
            return Err("invalid Tern exchange fields".into());
        }
        let mut hosts = HashSet::with_capacity(self.hosts.len());
        let mut snapshots = Vec::with_capacity(self.hosts.len());
        for host in &self.hosts {
            if !canonical_host(&host.host)
                || !hosts.insert(host.host.as_str())
                || host.label.len() > MAX_FIELD_BYTES
                || host.label.contains('\0')
                || host.observation_generation == 0
                || host.observation_generation > MAX_SAFE_INTEGER
                || host.agents.len() > MAX_HOST_AGENTS
                || (host.status != TernHostStatus::Available && !host.agents.is_empty())
            {
                return Err("invalid or duplicate Tern host".into());
            }
            let mut panes = HashSet::with_capacity(host.agents.len());
            let mut records = Vec::with_capacity(host.agents.len());
            let mut counts = SourceCounts::default();
            for agent in &host.agents {
                if !bounded(&agent.pane)
                    || agent.title.len() > MAX_FIELD_BYTES
                    || agent.cwd.len() > MAX_FIELD_BYTES
                    || agent.title.contains('\0')
                    || agent.cwd.contains('\0')
                    || !optional_sized(&agent.session_id)
                    || !optional_sized(&agent.session_label)
                    || !optional_sized(&agent.tab_id)
                    || !optional_sized(&agent.tab_label)
                    || !optional_sized(&agent.delivery_error)
                    || !panes.insert(agent.pane.as_str())
                {
                    return Err("invalid or duplicate Tern agent pane".into());
                }
                // Pane identity is scoped by this window and host, never session metadata.
                let terminal_id = agent.pane.clone();
                let status = match agent.state {
                    TernAgentState::Idle => AgentStatus::Idle,
                    TernAgentState::Working => AgentStatus::Working,
                    TernAgentState::WaitingInput => AgentStatus::Blocked,
                    // No completion signal or outcome is declared by the installed API.
                    TernAgentState::Exited => AgentStatus::Unknown,
                };
                counts.sessions += 1;
                match status {
                    AgentStatus::Working => counts.working += 1,
                    AgentStatus::Blocked => counts.blocked += 1,
                    AgentStatus::Unknown => counts.unknown += 1,
                    AgentStatus::Idle | AgentStatus::Done => {}
                }
                records.push(AgentRecord {
                    terminal_id,
                    pane_id: agent.pane.clone(),
                    status,
                    metadata: SessionMetadata {
                        title: Some(agent.title.clone()),
                        agent: Some("Tern".into()),
                        workspace_id: agent.session_id.clone(),
                        workspace_label: agent.session_label.clone(),
                        tab_id: agent.tab_id.clone(),
                        tab_label: agent.tab_label.clone(),
                        cwd: Some(agent.cwd.clone()),
                        ..SessionMetadata::default()
                    },
                    outcome_authoritative: false,
                    outcome: None,
                });
            }
            snapshots.push((records, counts));
        }
        for result in &self.results {
            if !bounded(&result.operation_id)
                || !bounded(&result.pane)
                || !optional_sized(&result.error)
                || matches!(result.status, TernResultStatus::Accepted) && result.error.is_some()
            {
                return Err("invalid Tern command result".into());
            }
        }
        Ok(snapshots)
    }
}

pub(crate) fn source_id(window: &str, host: &str) -> String {
    let mut source = String::with_capacity(6 + (window.len() + host.len()) * 2);
    source.push_str("tern:");
    for (index, component) in [window, host].into_iter().enumerate() {
        if index != 0 {
            source.push(':');
        }
        for byte in component.bytes() {
            use std::fmt::Write;
            let _ = write!(source, "{byte:02x}");
        }
    }
    source
}

#[derive(Debug)]
pub(crate) struct TernRegistry {
    windows: HashMap<String, Window>,
    retired_epochs: HashMap<String, u64>,
}

#[derive(Debug)]
pub(crate) struct Window {
    pub(crate) epoch: u64,
    pub(crate) sequence: u64,
    pub(crate) hosts: HashMap<String, Host>,
    pub(crate) observations: HashMap<String, Observation>,
    pub(crate) last_host_reply_bytes: usize,
    pub(crate) bound: bool,
    pub(crate) last_seen: Instant,
    pub(crate) offline: bool,
    pub(crate) offline_since: Option<Instant>,
    pending: Option<Pending>,
}

#[derive(Debug)]
pub(crate) struct Host {
    pub(crate) source: String,
    pub(crate) generation: u64,
    pub(crate) observation_generation: u64,
    pub(crate) is_local: bool,
    pub(crate) last_seen: Instant,
    pub(crate) offline: bool,
}

#[derive(Debug)]
pub(crate) struct Observation {
    pub(crate) generation: u64,
    pub(crate) is_local: bool,
    // Explicit loss requires a newer plugin observation. Native expiry alone does not.
    pub(crate) requires_new: bool,
}

#[derive(Debug)]
struct Pending {
    key: SessionKey,
    host: String,
    observation_generation: u64,
    command: TernCommand,
    sender: SyncSender<Result<PromptReceipt, PromptError>>,
    offered: bool,
}

impl Pending {
    fn reject(self, error: PromptError) {
        let error = if self.offered {
            PromptError::UnknownDelivery
        } else {
            error
        };
        let _ = self.sender.send(Err(error));
    }
}

impl Default for TernRegistry {
    fn default() -> Self {
        Self {
            windows: HashMap::new(),
            retired_epochs: HashMap::new(),
        }
    }
}

impl TernRegistry {
    pub(crate) fn get(&self, window: &str) -> Option<&Window> {
        self.windows.get(window)
    }
    pub(crate) fn get_mut(&mut self, window: &str) -> Option<&mut Window> {
        self.windows.get_mut(window)
    }
    pub(crate) fn len(&self) -> usize {
        self.windows.len()
    }
    pub(crate) fn retired_epoch(&self, window: &str) -> Option<u64> {
        self.retired_epochs.get(window).copied()
    }
    pub(crate) fn insert(&mut self, window: String, entry: Window) {
        self.windows.insert(window, entry);
    }
    pub(crate) fn expired_registrations(
        &self,
        now: Instant,
    ) -> impl Iterator<Item = (&str, &Window)> {
        self.windows.iter().filter_map(move |(window, entry)| {
            (entry.offline
                && entry.pending.is_none()
                && now.saturating_duration_since(entry.last_seen) >= FRESHNESS)
                .then_some((window.as_str(), entry))
        })
    }
    pub(crate) fn expired_hosts(&self, now: Instant) -> impl Iterator<Item = &Host> {
        self.windows.values().flat_map(move |entry| {
            let parent_expired =
                entry.offline && now.saturating_duration_since(entry.last_seen) >= FRESHNESS;
            entry.hosts.values().filter(move |host| {
                host.offline
                    && (parent_expired
                        || now.saturating_duration_since(host.last_seen) >= FRESHNESS)
            })
        })
    }
    pub(crate) fn source_generation(&self, source: &str) -> Option<u64> {
        self.windows
            .values()
            .flat_map(|entry| entry.hosts.values())
            .find_map(|host| (host.source == source).then_some(host.generation))
    }
    /// A missing registry child is safe; a live or newer child is not.
    pub(crate) fn allows_offline_removal(&self, source: &str, generation: u64) -> bool {
        self.windows.values().all(|entry| {
            entry.hosts.values().all(|host| {
                host.source != source || (host.generation == generation && host.offline)
            })
        })
    }
    pub(crate) fn remove_source(&mut self, source: &str, generation: u64) {
        for entry in self.windows.values_mut() {
            let id = entry.hosts.iter().find_map(|(id, host)| {
                (host.source == source && host.generation == generation).then(|| id.clone())
            });
            if let Some(id) = id {
                entry.reject_host_pending(&id, PromptError::StaleTarget);
                entry.hosts.remove(&id);
                // Keep observation high water marks and the parent binding.
                break;
            }
        }
    }
    pub(crate) fn retire_window(&mut self, window: &str) {
        if let Some(mut entry) = self.windows.remove(window) {
            if let Some(pending) = entry.pending.take() {
                pending.reject(PromptError::StaleTarget);
            }
            self.retired_epochs
                .entry(window.to_owned())
                .and_modify(|epoch| *epoch = (*epoch).max(entry.epoch))
                .or_insert(entry.epoch);
        }
    }
    pub(crate) fn retire_all(&mut self) {
        while let Some(window) = self.windows.keys().next().cloned() {
            self.retire_window(&window);
        }
    }
    pub(crate) fn route(&self, source: &str, generation: u64) -> Option<SourceRoute> {
        self.windows.iter().find_map(|(window, entry)| {
            if entry.offline || entry.last_seen.elapsed() >= FRESHNESS {
                return None;
            }
            entry.hosts.iter().find_map(|(id, host)| {
                (host.source == source
                    && host.generation == generation
                    && host.is_local
                    && !host.offline
                    && host.last_seen.elapsed() < FRESHNESS)
                    .then(|| SourceRoute::TernWindow {
                        window: window.clone(),
                        epoch: entry.epoch,
                        host: id.clone(),
                        observation_generation: host.observation_generation,
                    })
            })
        })
    }
    pub(crate) fn expire(&mut self, now: Instant) -> Vec<(String, u64, Instant)> {
        let mut offline = Vec::new();
        for entry in self.windows.values_mut() {
            let parent_deadline = entry.last_seen + FRESHNESS;
            let parent_expired = now >= parent_deadline;
            if parent_expired {
                entry.offline = true;
                entry.offline_since = Some(
                    entry
                        .offline_since
                        .map_or(parent_deadline, |since| since.min(parent_deadline)),
                );
            }
            let mut pending_lost = false;
            for (id, host) in &mut entry.hosts {
                let host_deadline = host.last_seen + FRESHNESS;
                if parent_expired || now >= host_deadline {
                    if !host.offline {
                        host.offline = true;
                        let since = if parent_expired {
                            host_deadline.min(parent_deadline)
                        } else {
                            host_deadline
                        };
                        offline.push((host.source.clone(), host.generation, since));
                    }
                    pending_lost |= entry
                        .pending
                        .as_ref()
                        .is_some_and(|pending| pending.host == *id);
                }
            }
            if pending_lost || parent_expired {
                if let Some(pending) = entry.pending.take() {
                    pending.reject(PromptError::Offline);
                }
            }
        }
        offline
    }
    pub(crate) fn queue(
        &mut self,
        window: &str,
        epoch: u64,
        host: &str,
        observation_generation: u64,
        key: SessionKey,
        pane: &str,
        text: &str,
        instance_id: &str,
    ) -> Result<(String, Receiver<Result<PromptReceipt, PromptError>>), PromptError> {
        let entry = self.windows.get_mut(window).ok_or(PromptError::Offline)?;
        let child = entry.hosts.get(host).ok_or(PromptError::StaleTarget)?;
        if !child.is_local {
            return Err(PromptError::ReadOnly);
        }
        if entry.epoch != epoch
            || child.generation != key.generation
            || child.observation_generation != observation_generation
            || entry.offline
            || child.offline
            || !entry.bound
            || entry.last_seen.elapsed() >= FRESHNESS
            || child.last_seen.elapsed() >= FRESHNESS
        {
            return Err(PromptError::StaleTarget);
        }
        if entry.pending.is_some() {
            return Err(PromptError::Busy);
        }
        let operation_id = format!(
            "tern-prompt-{}-{}",
            std::process::id(),
            NEXT_COMMAND
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .map_err(|_| PromptError::Other("Tern command IDs exhausted".into()))?
        );
        check_command_response(
            instance_id,
            entry.last_host_reply_bytes,
            &operation_id,
            pane,
            text,
        )?;
        let (tx, rx) = mpsc::sync_channel(1);
        entry.pending = Some(Pending {
            key,
            host: host.to_owned(),
            observation_generation,
            command: TernCommand {
                operation_id: operation_id.clone(),
                pane: pane.to_owned(),
                text: text.to_owned(),
            },
            sender: tx,
            offered: false,
        });
        Ok((operation_id, rx))
    }
    pub(crate) fn cancel(&mut self, window: &str, operation_id: &str) {
        if let Some(entry) = self.windows.get_mut(window) {
            if entry
                .pending
                .as_ref()
                .is_some_and(|pending| pending.command.operation_id == operation_id)
            {
                if let Some(pending) = entry.pending.take() {
                    pending.reject(PromptError::Offline);
                }
            }
        }
    }
    pub(crate) fn validate_result(&self, request: &TernExchangeRequest) -> Result<(), String> {
        let pending = self
            .windows
            .get(&request.window)
            .and_then(|window| window.pending.as_ref());
        for result in &request.results {
            if !pending.is_some_and(|pending| {
                pending.offered
                    && pending.command.operation_id == result.operation_id
                    && pending.command.pane == result.pane
                    && request.hosts.iter().any(|host| {
                        host.host == pending.host
                            && host.is_local
                            && host.observation_generation == pending.observation_generation
                    })
            }) {
                return Err("stale or unrecognized Tern command result".into());
            }
        }
        Ok(())
    }
    pub(crate) fn settle(&mut self, window: &str, result: Option<&TernResult>) {
        let Some(entry) = self.windows.get_mut(window) else {
            return;
        };
        if let Some(pending) = entry.pending.as_ref() {
            if pending.offered {
                let pending = entry.pending.take().expect("offered command");
                let receipt = match result {
                    Some(TernResult {
                        status: TernResultStatus::Accepted,
                        ..
                    }) => Ok(PromptReceipt::Accepted),
                    Some(TernResult {
                        status: TernResultStatus::Failed,
                        error,
                        ..
                    }) => Err(PromptError::Other(
                        error.clone().unwrap_or_else(|| "Tern rejected ask".into()),
                    )),
                    None => Err(PromptError::UnknownDelivery),
                };
                let _ = pending.sender.send(receipt);
            }
        }
    }
    pub(crate) fn pending_target(&self, window: &str) -> Option<(SessionKey, String)> {
        let pending = self.windows.get(window)?.pending.as_ref()?;
        (!pending.offered).then(|| (pending.key.clone(), pending.command.pane.clone()))
    }
    pub(crate) fn offer(&mut self, window: &str, instance_id: &str) -> Option<TernCommand> {
        let entry = self.windows.get(window)?;
        let pending = entry.pending.as_ref()?;
        if pending.offered {
            return None;
        }
        let child = entry.hosts.get(&pending.host);
        let admission = if child.is_some_and(|child| !child.is_local) {
            Err(PromptError::ReadOnly)
        } else if entry.offline
            || entry.last_seen.elapsed() >= FRESHNESS
            || child.is_none_or(|child| {
                child.offline
                    || child.last_seen.elapsed() >= FRESHNESS
                    || child.generation != pending.key.generation
                    || child.observation_generation != pending.observation_generation
            })
        {
            Err(PromptError::StaleTarget)
        } else {
            check_command_response(
                instance_id,
                entry.last_host_reply_bytes,
                &pending.command.operation_id,
                &pending.command.pane,
                &pending.command.text,
            )
        };
        if let Err(error) = admission {
            self.reject_pending(window, error);
            return None;
        }
        let pending = self.windows.get_mut(window)?.pending.as_mut()?;
        pending.offered = true;
        Some(TernCommand {
            operation_id: pending.command.operation_id.clone(),
            pane: pending.command.pane.clone(),
            text: std::mem::take(&mut pending.command.text),
        })
    }
    pub(crate) fn reject_pending(&mut self, window: &str, error: PromptError) {
        if let Some(pending) = self
            .windows
            .get_mut(window)
            .and_then(|entry| entry.pending.take())
        {
            pending.reject(error);
        }
    }
    pub(crate) fn reject_all_pending(&mut self, error: PromptError) {
        for entry in self.windows.values_mut() {
            if let Some(pending) = entry.pending.take() {
                pending.reject(error.clone());
            }
        }
    }
}

impl Window {
    pub(crate) fn new(epoch: u64, now: Instant) -> Self {
        Self {
            epoch,
            sequence: 0,
            hosts: HashMap::new(),
            observations: HashMap::new(),
            last_host_reply_bytes: 2,
            bound: false,
            last_seen: now,
            offline: false,
            offline_since: None,
            pending: None,
        }
    }
    pub(crate) fn reject_host_pending(&mut self, host: &str, error: PromptError) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.host == host)
        {
            if let Some(pending) = self.pending.take() {
                pending.reject(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(text: String) -> TernExchangeReply {
        TernExchangeReply {
            ok: true,
            instance_id: "daemon-a".into(),
            hosts: (0..MAX_HOSTS)
                .map(|id| TernHostReply {
                    host: id.to_string(),
                    observation_generation: 1,
                    generation: MAX_SAFE_INTEGER,
                    error: Some("\"\\\n한".repeat(70)),
                    error_code: Some(TernExchangeErrorCode::Rejected),
                })
                .collect(),
            command: Some(TernCommand {
                operation_id: "boundary-operation".into(),
                pane: "pane-한".into(),
                text,
            }),
            error: None,
            error_code: None,
        }
    }

    #[test]
    fn real_reply_serializer_admits_exact_utf8_and_escaped_boundaries() {
        let escaped = "\"\\\n\t한🪶";
        let empty = serialize_response(&reply(String::new())).unwrap().len();
        let escaped_len = serde_json::to_string(escaped).unwrap().len() - 2;
        let text = format!(
            "{escaped}{}",
            "a".repeat(MAX_RESPONSE_BYTES - empty - escaped_len)
        );
        let exact = reply(text.clone());
        let bytes = serialize_response(&exact).unwrap();
        assert_eq!(bytes.len(), MAX_RESPONSE_BYTES);
        assert_eq!(bytes.last(), Some(&b'\n'));
        let decoded: TernExchangeReply = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.command.unwrap().text, text);
        assert_eq!(
            check_command_response(
                "daemon-a",
                host_reply_bytes(&exact.hosts).unwrap(),
                "boundary-operation",
                "pane-한",
                &text,
            ),
            Ok(())
        );
        let mut above = text.clone();
        above.push('a');
        assert_eq!(
            check_command_response(
                "daemon-a",
                host_reply_bytes(&exact.hosts).unwrap(),
                "boundary-operation",
                "pane-한",
                &above,
            ),
            Err(PromptError::TooLarge)
        );
        assert!(serialize_response(&reply(above)).is_err());
        let mut below = text;
        below.pop();
        assert_eq!(
            serialize_response(&reply(below.clone())).unwrap().len(),
            MAX_RESPONSE_BYTES - 1
        );
        assert_eq!(
            check_command_response(
                "daemon-a",
                host_reply_bytes(&exact.hosts).unwrap(),
                "boundary-operation",
                "pane-한",
                &below,
            ),
            Ok(())
        );
    }

    #[test]
    fn preoffer_size_invariant_fails_definitely_before_offered_state() {
        let mut registry = TernRegistry::default();
        let mut window = test_window(MAX_SAFE_INTEGER, Instant::now());
        let (sender, receipt) = mpsc::sync_channel(1);
        window.pending = Some(Pending {
            key: SessionKey {
                source_id: 0,
                generation: MAX_SAFE_INTEGER,
                terminal_id: "pane-한".into(),
            },
            host: "0".into(),
            observation_generation: 1,
            command: reply("a".repeat(MAX_RESPONSE_BYTES)).command.unwrap(),
            sender,
            offered: false,
        });
        registry.insert("window-a".into(), window);
        assert!(registry.offer("window-a", "daemon-a").is_none());
        assert_eq!(receipt.try_recv(), Ok(Err(PromptError::TooLarge)));
        assert!(registry.get("window-a").unwrap().pending.is_none());
        let key = SessionKey {
            source_id: 0,
            generation: MAX_SAFE_INTEGER,
            terminal_id: "pane-한".into(),
        };
        let (_, accepted) = registry
            .queue(
                "window-a",
                1,
                "0",
                1,
                key,
                "pane-한",
                "next valid ask",
                "daemon-a",
            )
            .unwrap();
        let command = registry.offer("window-a", "daemon-a").unwrap();
        assert_eq!(command.text, "next valid ask");
        assert!(registry.offer("window-a", "daemon-a").is_none());
        registry.settle("window-a", None);
        assert_eq!(accepted.try_recv(), Ok(Err(PromptError::UnknownDelivery)));
    }

    #[test]
    fn freshness_boundary_finalizes_pending_before_selecting_registration() {
        let mut registry = TernRegistry::default();
        let now = Instant::now();
        registry.insert("window-a".into(), test_window(1, now));
        let key = SessionKey {
            source_id: 0,
            generation: 1,
            terminal_id: "pane".into(),
        };
        let (_, receipt) = registry
            .queue(
                "window-a",
                1,
                "0",
                1,
                key.clone(),
                "pane",
                "ask",
                "daemon-a",
            )
            .unwrap();
        registry.get_mut("window-a").unwrap().last_seen = now - FRESHNESS;
        assert!(registry.route(&source_id("window-a", "0"), 1).is_none());
        assert!(matches!(
            registry.queue("window-a", 1, "0", 1, key, "pane", "stale", "daemon-a",),
            Err(PromptError::StaleTarget)
        ));
        assert_eq!(registry.expired_registrations(now).count(), 0);
        assert_eq!(
            registry.expire(now),
            vec![(source_id("window-a", "0"), 1, now)]
        );
        assert_eq!(receipt.try_recv(), Ok(Err(PromptError::Offline)));
        assert_eq!(registry.expired_registrations(now).count(), 1);
    }

    #[test]
    fn canonical_error_codes_are_bounded_and_success_uses_host_shape() {
        let failure = TernExchangeReply::error(
            "daemon-a".into(),
            TernExchangeErrorCode::ReceiptRejected,
            "한".repeat(4096),
        );
        assert!(failure.error.as_ref().unwrap().len() <= MAX_FIELD_BYTES);
        let failure: serde_json::Value =
            serde_json::from_slice(&serialize_response(&failure).unwrap()).unwrap();
        assert_eq!(failure["error_code"], "receipt_rejected");
        assert_eq!(failure["hosts"], serde_json::json!([]));
        assert!(failure.get("generation").is_none());
        assert!(failure["command"].is_null());
        let mut success = reply("ask".into());
        success.command = None;
        let bytes = serialize_response(&success).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value.get("error_code").is_none());
        assert!(serde_json::from_slice::<TernExchangeReply>(&bytes).is_ok());
        value["error_code"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<TernExchangeReply>(value).is_err());
    }

    fn test_window(generation: u64, now: Instant) -> Window {
        let mut window = Window::new(1, now);
        window.bound = true;
        window.hosts.insert(
            "0".into(),
            Host {
                source: source_id("window-a", "0"),
                generation,
                observation_generation: 1,
                is_local: true,
                last_seen: now,
                offline: false,
            },
        );
        window
    }

    #[test]
    fn natural_expiry_uses_the_earliest_child_or_parent_deadline_and_keeps_parent_clock() {
        for (parent_age, host_age) in [(0, 1), (1, 0)] {
            let mut registry = TernRegistry::default();
            let base = Instant::now();
            let parent_seen = base + Duration::from_secs(parent_age);
            let host_seen = base + Duration::from_secs(host_age);
            let mut window = test_window(1, parent_seen);
            window.hosts.get_mut("0").unwrap().last_seen = host_seen;
            registry.insert("window-a".into(), window);
            let now = base + FRESHNESS + OFFLINE_GRACE;
            assert_eq!(
                registry.expire(now),
                vec![(source_id("window-a", "0"), 1, base + FRESHNESS)]
            );
            assert_eq!(
                registry.get("window-a").unwrap().offline_since,
                Some(parent_seen + FRESHNESS)
            );
            assert!(registry.expire(now + OFFLINE_GRACE).is_empty());
            assert_eq!(
                registry.get("window-a").unwrap().offline_since,
                Some(parent_seen + FRESHNESS)
            );
        }
    }

    #[test]
    fn tern_host_id_and_error_boundaries_do_not_alias_or_overflow() {
        for valid in ["0", "1", "9007199254740991"] {
            assert!(canonical_host(valid));
        }
        for invalid in [
            "",
            "00",
            "01",
            "-1",
            "+1",
            "1.0",
            "9007199254740992",
            "18446744073709551616",
        ] {
            assert!(!canonical_host(invalid));
        }
        assert_eq!(source_id("a:b", "0"), "tern:613a62:30");
        assert_ne!(source_id("a", "12"), source_id("a", "21"));
        let mut text = "한".repeat(4096);
        truncate_utf8(&mut text, MAX_HOST_ERROR_BYTES);
        assert_eq!(text.len(), 510);
        assert_eq!(
            check_command_response("daemon-a", usize::MAX, "o", "p", "text"),
            Err(PromptError::TooLarge)
        );
        assert_eq!(
            check_command_response("daemon-a", 1, "o", "p", "text"),
            Err(PromptError::TooLarge)
        );
    }
    #[test]
    fn tern_registry_route_queue_and_offer_independently_deny_remote_hosts() {
        let mut registry = TernRegistry::default();
        let mut window = test_window(1, Instant::now());
        window.hosts.get_mut("0").unwrap().is_local = false;
        registry.insert("window-a".into(), window);
        let key = SessionKey {
            source_id: 0,
            generation: 1,
            terminal_id: "pane".into(),
        };
        assert!(registry.route(&source_id("window-a", "0"), 1).is_none());
        assert!(matches!(
            registry.queue(
                "window-a",
                1,
                "0",
                1,
                key.clone(),
                "pane",
                "never",
                "daemon-a"
            ),
            Err(PromptError::ReadOnly)
        ));
        registry
            .get_mut("window-a")
            .unwrap()
            .hosts
            .get_mut("0")
            .unwrap()
            .is_local = true;
        let (_, receipt) = registry
            .queue("window-a", 1, "0", 1, key, "pane", "never", "daemon-a")
            .unwrap();
        registry
            .get_mut("window-a")
            .unwrap()
            .hosts
            .get_mut("0")
            .unwrap()
            .is_local = false;
        assert!(registry.offer("window-a", "daemon-a").is_none());
        assert_eq!(receipt.try_recv(), Ok(Err(PromptError::ReadOnly)));
    }
}
