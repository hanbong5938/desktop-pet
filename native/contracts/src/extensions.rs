use crate::{ObservationSelection, ProfileStore};
use serde_json::{Map, Value};
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// Resolved common profile paths. Extension handlers must use the same selected
/// profile, not reinterpret a concrete host's injected environment in core.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilePaths {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub control_socket: PathBuf,
}

/// Adapter-owned raw-frame handler. Common transport owns frame boundaries,
/// timeouts, and recursive duplicate-key rejection before dispatch. It enforces
/// both byte limits (including the transport terminator) and never truncates a
/// response. The adapter owns wire schemas, validation-before-mutation, and
/// serialization. handle_frame is outside the domain lock; an adapter obtains
/// its RuntimePort transaction itself. There is no new IPC server here.
pub trait ControlExtension: Send + Sync {
    fn kind(&self) -> &'static str;
    fn max_request_bytes(&self) -> usize;
    fn max_response_bytes(&self) -> usize;
    fn handle_frame(&self, frame: &[u8]) -> Result<Vec<u8>, String>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CliOption {
    pub name: &'static str,
    pub takes_value: bool,
    pub help: &'static str,
}

pub struct CliInvocation<'a> {
    pub command: &'a str,
    pub arguments: &'a [OsString],
    pub input: &'a [u8],
    pub profile: &'a ProfilePaths,
}

#[derive(Debug, Eq, PartialEq)]
pub struct CliOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

/// Composition registers explicit adapter options and commands. Core owns
/// shared launch/profile/control commands and --host. An adapter may resolve
/// concrete socket/environment provenance in configure_option, but cannot
/// replace the selected profile. stdin is read only up to max_input_bytes.
pub trait CliExtension: Send + Sync {
    fn options(&self) -> &'static [CliOption];
    fn commands(&self) -> &'static [&'static str];
    fn configure_option(&self, name: &str, value: Option<&OsStr>) -> Result<(), String>;
    fn max_input_bytes(&self, command: &str) -> usize;
    fn execute(&self, invocation: CliInvocation<'_>) -> Result<CliOutput, String>;
}

/// AppKit-free lifecycle UI model. Key belongs to this extension; labels and
/// help are adapter-provided using the requested opaque locale identifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LifecycleSettingView {
    pub key: String,
    pub label: String,
    pub help: String,
    pub value: bool,
    pub enabled: bool,
}

/// Adapter-owned action presentation. Copy text is data, not a shell command
/// for core to execute. A disabled action must not be offered as actionable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LifecycleActionView {
    pub key: String,
    pub label: String,
    pub help: String,
    pub enabled: bool,
    pub copy_text: Option<String>,
}

/// Result from the common profile preference owner, not an adapter's desired
/// candidate. Uncertain retains the original submitted operation identity;
/// callers may observe that identity but must never replay it or save offline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreferenceCommit {
    Committed(Map<String, Value>),
    Uncertain {
        instance_id: String,
        operation_id: String,
        current: Option<Map<String, Value>>,
    },
}

/// Common selected-profile preference access, including live CAS/operation
/// observation and exclusive offline mutation admission. Native adapter UI
/// retains this port for workers rather than constructing another owner/store.
pub trait PreferenceAccess: Send + Sync {
    fn read(&self) -> Result<Map<String, Value>, String>;
    fn apply(&self, patch: Map<String, Value>) -> Result<PreferenceCommit, String>;
}

/// A registered option within the common `preferences set` command, distinct
/// from launch options. Names include their leading `--`; composition rejects
/// collisions with common options and other owners.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreferencePatchOption {
    pub name: &'static str,
    pub takes_value: bool,
    pub repeatable: bool,
    pub help: &'static str,
}

/// One parsed adapter preference option. Core validates arity/repetition from
/// the registration and preserves order; the adapter owns meaning, conflicting
/// options, values, and conversion to its opaque preference patch fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreferencePatchArgument {
    pub name: String,
    pub value: Option<OsString>,
}

/// Validated adapter patch with any required authority snapshot captured
/// before the profile writer transaction. Core stages all owners into one
/// candidate and retains its existing revision/CAS and operation semantics.
pub trait PreparedPreferencePatch: Send + Sync {
    /// Pure candidate transformation: only adapter-owned fields may change,
    /// and unknown fields must survive. No ProfileStore reentry, lock acquisition,
    /// host I/O, or live mutation. Err discards the entire candidate, including
    /// common settings, without a write or a call to committed.
    fn apply(&self, candidate: &mut Map<String, Value>) -> Result<(), String>;

    /// Called only after the sole atomic preference save succeeds and after the
    /// writer/domain locks are released. Apply live policy using the actual
    /// saved object, obtaining RuntimePort access here if needed. A failure is
    /// a postcommit error, not permission to roll back or report an unsaved
    /// candidate; the caller retains the committed revision/result semantics.
    fn committed(&self, saved: &Map<String, Value>) -> Result<(), String>;
}

/// Core owns auto_start only. Adapter-owned lifecycle fields and defaults are
/// interpreted here and persisted through the SAME ProfileStore as core.
/// should_exit implements host-specific exit policy without a concrete host
/// branch in core. apply_setting must update its policy only after persistence
/// succeeds; calls run outside a domain transaction and may obtain one itself.
pub trait LifecycleExtension: Send + Sync {
    fn provider(&self) -> &'static str;
    /// Pure initialization of this owner's defaults when lifecycle.json was
    /// absent at the start of a common writer transaction. Called before the
    /// sole atomic save, never for an existing object or a read-only query.
    /// Only owner keys may change; preserve other owners' and unknown fields.
    /// No store/domain reentry, lock acquisition, host I/O, or live mutation.
    /// Err discards the entire candidate without creating lifecycle.json.
    fn initialize_lifecycle(&self, _candidate: &mut Map<String, Value>) -> Result<(), String> {
        Ok(())
    }

    fn settings(
        &self,
        store: &ProfileStore,
        selection: &ObservationSelection,
        locale: &str,
    ) -> Result<Vec<LifecycleSettingView>, String>;
    fn apply_setting(&self, store: &ProfileStore, key: &str, value: bool) -> Result<(), String>;
    /// Returns only real adapter actions. Core may copy copy_text, but must
    /// never interpret it as executable host behavior.
    fn actions(
        &self,
        _store: &ProfileStore,
        _selection: &ObservationSelection,
        _locale: &str,
    ) -> Result<Vec<LifecycleActionView>, String> {
        Ok(Vec::new())
    }

    /// Explicit adapter-owned activation for an action without copy_text.
    /// Core rechecks the current enabled view before calling this outside its
    /// domain/UI locks. Copy actions never reach this method. Implementations
    /// own native presentation and host behavior; core interprets no commands.
    fn activate_action(
        &self,
        _store: &ProfileStore,
        _selection: &ObservationSelection,
        _key: &str,
        _locale: &str,
        _preferences: Arc<dyn PreferenceAccess>,
    ) -> Result<(), String> {
        Err(format!("{} does not support action activation", self.provider()))
    }

    /// Opaque top-level preference patch keys owned by this adapter. Core
    /// routes only registered keys and rejects unowned/duplicate ownership;
    /// it never interprets adapter settings or reconstructs their disk object.
    fn preference_patch_keys(&self) -> &'static [&'static str] {
        &[]
    }

    /// Pure projection from saved disk layout to registered opaque wire patch
    /// fields. Core does not know the adapter's storage schema. No store/domain
    /// reentry, lock acquisition, or host I/O; preserve validation errors rather
    /// than inventing defaults for malformed settings.
    fn preference_snapshot(
        &self,
        _saved: &Map<String, Value>,
    ) -> Result<Map<String, Value>, String> {
        if self.preference_patch_keys().is_empty() {
            Ok(Map::new())
        } else {
            Err(format!(
                "{} does not support preference snapshots",
                self.provider()
            ))
        }
    }

    fn preference_options(&self) -> &'static [PreferencePatchOption] {
        &[]
    }

    /// Converts registered CLI options to the same opaque fields accepted by
    /// the preference control request. Unsupported extensions reject, rather
    /// than consuming options and reporting a successful no-op.
    fn parse_preference_options(
        &self,
        _arguments: &[PreferencePatchArgument],
    ) -> Result<Map<String, Value>, String> {
        Err(format!(
            "{} does not support preference options",
            self.provider()
        ))
    }

    /// Runs outside the writer/domain locks using the caller's actual selected
    /// profile lease. Validate the complete owner patch and capture authority
    /// here; apply then operates only on the loaded candidate. Do not retain
    /// the supplied store in a long-lived adapter or persist/live-mutate here.
    fn prepare_preference_patch(
        &self,
        _store: &ProfileStore,
        _patch: &Map<String, Value>,
    ) -> Result<Box<dyn PreparedPreferencePatch>, String> {
        Err(format!(
            "{} does not support preference patches",
            self.provider()
        ))
    }
    fn should_exit(&self, selection: &ObservationSelection, now: Instant) -> bool;
}

/// One explicit registration bundle passed by composition alongside
/// Vec<Arc<dyn Integration>>. Duplicate kinds/commands/options/provider keys
/// must be rejected by composition rather than dispatched by registration order.
#[derive(Default)]
pub struct IntegrationExtensions {
    pub control: Vec<Arc<dyn ControlExtension>>,
    pub cli: Vec<Arc<dyn CliExtension>>,
    pub lifecycle: Vec<Arc<dyn LifecycleExtension>>,
}
