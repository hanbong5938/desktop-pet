use crate::{ObservationSelection, ProfileStore};
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

/// Core owns auto_start only. Adapter-owned lifecycle fields and defaults are
/// interpreted here and persisted through the SAME ProfileStore as core.
/// should_exit implements host-specific exit policy without a concrete host
/// branch in core. apply_setting must update its policy only after persistence
/// succeeds; calls run outside a domain transaction and may obtain one itself.
pub trait LifecycleExtension: Send + Sync {
    fn provider(&self) -> &'static str;
    fn settings(
        &self,
        store: &ProfileStore,
        selection: &ObservationSelection,
        locale: &str,
    ) -> Result<Vec<LifecycleSettingView>, String>;
    fn apply_setting(&self, store: &ProfileStore, key: &str, value: bool) -> Result<(), String>;
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
