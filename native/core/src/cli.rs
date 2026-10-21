use crate::character_service::execute_offline;
use crate::character_store::PackStore;
use crate::character_types::{validate_pack_id, PackAction, PackOperation, PackRequest};
use crate::control::{
    send_command, send_pack_list, send_pack_request, send_pack_status, ControlResponse,
    PackReplyEnvelope,
};
use crate::daemon::DaemonConfig;
use crate::lifecycle::Paths;
use crate::Composition;
use crate::{
    automation_cli, bubble, bundle, character_renderer, control, daemon, lifecycle, pack_authoring,
    pose, preferences, state,
};
use desktop_pet_contracts::{
    CliInvocation, LifecycleLock, ObservationSelection, ProfilePaths, ProfileStore,
    OBSERVATION_SELECTION_ENV,
};
use desktop_pet_update_coordinator::identity;
use desktop_pet_update_coordinator::protocol::{
    observe_latest_operation, observe_operation, record_user_stop, start_allowed,
    OperationObservation,
};
use desktop_pet_update_coordinator::{
    detect_origin, executable_identity, InstallOrigin, UpdateContext,
};
use std::cell::Cell;
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const STARTUP_TIMEOUT: Duration = Duration::from_secs(
    character_renderer::MAX_PREPARATION_TIMEOUT.as_secs()
        + 2 * character_renderer::SURFACE_PREPARATION_TIMEOUT.as_secs(),
);
const STOP_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);

// Retain parsed integration options without configuring extensions again when
// Settings launches the existing restart command.
static DAEMON_RESTART_OPTIONS: OnceLock<Vec<OsString>> = OnceLock::new();
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const LOG_MODE: u32 = 0o600;
const MAX_SOURCE_PATH_BYTES: usize = 4096;
const USAGE: &str = concat!(
    "Commands:\n",
    "    ensure              Start automatically if auto_start is on; otherwise skip\n",
    "    start               Start the daemon without changing lifecycle settings\n",
    "    stop                Stop the current daemon or pending startup\n",
    "    restart             Stop, then manually start without changing settings\n",
    "    settings            Open unified Settings; manually start saved profile if offline\n",
    "                        Keeps saved host/visibility; ignores --host and host env overrides\n",
    "    settings get        Show lifecycle settings as JSON\n",
    "    settings set KEY on|off  Set auto_start or a registered integration-owned setting\n",
    "    status              Show daemon, registration, data, and executable status\n",
    "    update-capabilities  Show read-only updater protocol support\n",
    "    profile show --json  Read selected absolute profile paths without starting or writing\n",
    "    profile adopt --config-dir ABS --state-dir ABS  Explicitly adopt an inactive owned profile\n",
    "    installation inspect --json  Verify this executable's actual distribution and owner\n",
    "    installation discover [--executable ABS] --json  Resolve a verified installation\n",
    "    update-status [OPID] Show offline durable updater outcome (--state-dir PATH)\n",
    "    update --yes        Approve the successor bridge's bounded automatic migration; requires both profile paths\n",
    "    legacy-migration-route  Authenticate a committed original integration route from bounded stdin JSON\n",
    "    migration-integration-handover  Confirm loaded consumer and fresh same-window binding from bounded stdin JSON\n",
    "    show                Show character; reattach visible bubble\n",
    "    hide                Hide character; leave enabled bubble standalone\n",
    "    toggle              Toggle character visibility, independently of bubble\n",
    "    passthrough         Toggle full-window click-through for both windows\n",
    "    alpha_passthrough   Toggle alpha-mask click-through for character only\n",
    "    show_bubble         Show bubble, including when character is hidden\n",
    "    hide_bubble         Hide bubble without changing character visibility\n",
    "    bubble_above        Place bubble above pet when attached\n",
    "    bubble_below        Place bubble below pet when attached\n",
    "    bubble_left         Place bubble to the left of pet when attached\n",
    "    bubble_right        Place bubble to the right of pet when attached\n",
    "    bubble_auto         Place bubble automatically when attached\n",
    "    reset               Reset character and standalone bubble positions; keep visibility\n",
    "    bigger              Increase renderer scale\n",
    "    smaller             Decrease renderer scale\n",
    "    presentation get    Show desired, effective/native and persisted presentation\n",
    "    presentation set    Set absolute presentation fields (see options below)\n",
    "    presentation reset  Reset pet/bubble positions without changing visibility\n",
    "    presentation status OPID --instance ID   Show an operation from this daemon instance\n",
    "    preferences get      Show saved and effective preferences from the running daemon\n",
    "    preferences set      Set --language system|ko|en|zh-Hans|ja, --theme warm_ivory|dusty_rose|moonlit_ink|custom,\n",
    "                         --surface/--text/--muted/--border/--accent #RRGGBB (partial palette preserves saved colors),\n",
    "                         --status-indicators on|off, --menu-bar always|recovery_only,\n",
    "                         --observation-host auto|PROVIDER; integration-owned options are listed below\n",
    "                         [--expected-revision N] [--operation-id ID] [--wait SECONDS|--no-wait]\n",
    "    preferences status   --instance ID --operation-id ID (preference operations only)\n",
    "    sessions list        --instance ID [--filter all|idle|working|waiting|completed|unknown|offline] [--limit 1..128]\n",
    "    sessions show        --instance ID --source N --generation N --terminal ID\n",
    "    sessions prompt      --instance ID --source N --generation N --terminal ID\n",
    "                         exactly one of --text TEXT, --file PATH, --stdin; [--operation-id ID] [--wait SECONDS|--no-wait]\n",
    "    sessions status      --instance ID --operation-id ID (prompt operations only)\n",
    "    dialogue list        List editor targets, identities, locales and slots\n",
    "    dialogue get         --target JSON --locale ko|en|zh-Hans|ja --slot KEY\n",
    "    dialogue set         --target JSON --locale ko|en|zh-Hans|ja --slot KEY exactly one of --text TEXT, --file PATH, --stdin\n",
    "    dialogue reset-entry --target JSON --locale ko|en|zh-Hans|ja --slot KEY\n",
    "    dialogue reset-character --target JSON\n",
    "                         --slot KEY: idle|running|waiting|unknown|head_tap|body_tap|pet|completion_observed\n",
    "                         --target is the serialized identity returned by dialogue list; omit --baseline to read fresh CAS first\n",
    "                         Mutations accept [--baseline JSON] [--operation-id ID] [--wait SECONDS|--no-wait]\n",
    "    dialogue status      --instance ID --operation-id ID (dialogue operations only)\n",
    "    worktree inspect     --instance ID --source N --generation N --terminal ID\n",
    "    worktree remove      --token TOKEN [--operation-id ID] [--wait SECONDS|--no-wait]\n",
    "    worktree status      --instance ID --operation-id ID (worktree operations only)\n",
    "    Worktree inspect issues an expiring one-use token; remove accepts that token only (no force/path).\n",
    "    pack list            List managed character packs\n",
    "    pack import         Import a character pack directory or archive (--path PATH)\n",
    "    pack validate       Validate and fully prepare a character source (--path PATH)\n",
    "    pack preview        Render a deterministic source-sized PNG (--path PATH --output PATH [--phase idle|running|waiting|unknown] [--time-ms N] [--reaction head_tap|body_tap|pet|completion_observed] [--reaction-age-ms N] [--hit-overlay])\n",
    "    pack export ID      Export a managed revision or builtin ([--revision N] --output PATH)\n",
    "    pack select ID      Select the newest revision of a pack\n",
    "    pack update ID      Add a revision from a directory (--path DIR)\n",
    "    pack restore ID     Restore a historic revision (--revision N)\n",
    "    pack remove ID      Remove a pack (active selection falls back to builtin)\n",
    "    pack status OPID    Show a pack operation\n\n",
    "Pack mutation options: [--operation-id ID] [--expected-generation N] [--wait SECONDS|--no-wait].\n",
    "Async/custom finite waits require a running daemon; offline default executes synchronously.\n\n",
    "Presentation set fields (at least one required):\n",
    "    --visible on|off  --passthrough on|off  --alpha-passthrough on|off\n",
    "    --bubble-visible on|off  --bubble-placement above|below|left|right|auto\n",
    "    --scale NUMBER       Absolute finite renderer scale\n",
    "Presentation set/reset options:\n",
    "    --expected-revision N   Reject a concurrent presentation change\n",
    "    --operation-id ID       Caller-chosen unique operation ID (otherwise generated)\n",
    "    --wait SECONDS         Wait up to SECONDS for applied (default 15; no mutation retry)\n",
    "    --no-wait              Return acknowledged accepted/pending state without waiting\n",
    "    An ACK confirms only acceptance, not native application or persistence.\n",
    "    On uncertain transmission use presentation status OPID --instance ID.\n\n",
    "Options:\n",
    "    --host auto|PROVIDER Startup provider (explicit > neutral env > saved; new profile: auto)\n",
    "    --assets PATH       Renderer assets directory (settings: offline launch only; not for pack/settings get|set/presentation/preferences/sessions/dialogue/worktree)\n",
    "    --config-dir PATH   Lifecycle and preference directory\n",
    "    --state-dir PATH    Runtime state directory\n",
    "    -h, --help          Show this help\n",
    "    -V, --version       Show version\n",
    "\nIndependent-model preview:\n",
    "    --pose NAME         waiting, writing, failed, cancelled, disconnected, bored,\n",
    "                        happy, head-tap, torso-tap, head-pet (v5 rig packs)\n"
);

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum CommandKind {
    Ensure,
    Start,
    Stop,
    Restart,
    Status,
    UpdateCapabilities,
    UpdateStatus,
    Profile,
    Installation,
    DistributionIdentity,
    LegacyMigrationRoute,
    MigrationIntegrationHandover,
    Update,
    Extension,
    Settings,
    Show,
    Hide,
    Toggle,
    Passthrough,
    AlphaPassthrough,
    ShowBubble,
    HideBubble,
    BubbleAbove,
    BubbleBelow,
    BubbleLeft,
    BubbleRight,
    BubbleAuto,
    Reset,
    Bigger,
    Smaller,
    Daemon,
    Presentation,
    Preferences,
    Sessions,
    Dialogue,
    Worktree,
    Pack,
    SettingsGet,
    SettingsSet,
}

impl CommandKind {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "ensure" => Self::Ensure,
            "start" => Self::Start,
            "stop" => Self::Stop,
            "restart" => Self::Restart,
            "status" => Self::Status,
            "update-capabilities" => Self::UpdateCapabilities,
            "profile" => Self::Profile,
            "installation" => Self::Installation,
            "distribution-identity" => Self::DistributionIdentity,
            "legacy-migration-route" => Self::LegacyMigrationRoute,
            "migration-integration-handover" => Self::MigrationIntegrationHandover,
            "update" => Self::Update,
            "update-status" => Self::UpdateStatus,
            "settings" => Self::Settings,
            "show" => Self::Show,
            "hide" => Self::Hide,
            "toggle" => Self::Toggle,
            "passthrough" => Self::Passthrough,
            "alpha_passthrough" => Self::AlphaPassthrough,
            "show_bubble" => Self::ShowBubble,
            "hide_bubble" => Self::HideBubble,
            "bubble_above" => Self::BubbleAbove,
            "bubble_below" => Self::BubbleBelow,
            "bubble_left" => Self::BubbleLeft,
            "bubble_right" => Self::BubbleRight,
            "bubble_auto" => Self::BubbleAuto,
            "reset" => Self::Reset,
            "bigger" => Self::Bigger,
            "smaller" => Self::Smaller,
            "daemon" => Self::Daemon,
            "pack" => Self::Pack,
            "presentation" => Self::Presentation,
            "preferences" => Self::Preferences,
            "sessions" => Self::Sessions,
            "dialogue" => Self::Dialogue,
            "worktree" => Self::Worktree,
            _ => return None,
        })
    }

    fn needs_assets(self) -> bool {
        self == Self::Daemon
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Ensure => "ensure",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::UpdateCapabilities => "update-capabilities",
            Self::UpdateStatus => "update-status",
            Self::Profile => "profile",
            Self::Installation => "installation",
            Self::DistributionIdentity => "distribution-identity",
            Self::LegacyMigrationRoute => "legacy-migration-route",
            Self::MigrationIntegrationHandover => "migration-integration-handover",
            Self::Update => "update",
            Self::Extension => "extension",
            Self::Restart => "restart",
            Self::Status => "status",
            Self::Show => "show",
            Self::Settings => "settings",
            Self::Hide => "hide",
            Self::Toggle => "toggle",
            Self::Passthrough => "passthrough",
            Self::AlphaPassthrough => "alpha_passthrough",
            Self::ShowBubble => "show_bubble",
            Self::HideBubble => "hide_bubble",
            Self::BubbleAbove => "bubble_above",
            Self::BubbleBelow => "bubble_below",
            Self::BubbleLeft => "bubble_left",
            Self::BubbleRight => "bubble_right",
            Self::BubbleAuto => "bubble_auto",
            Self::Reset => "reset",
            Self::Bigger => "bigger",
            Self::Smaller => "smaller",
            Self::Daemon => "daemon",
            Self::Pack => "pack",
            Self::Presentation => "presentation",
            Self::Preferences => "preferences",
            Self::Sessions => "sessions",
            Self::Dialogue => "dialogue",
            Self::Worktree => "worktree",
            Self::SettingsGet => "settings-get",
            Self::SettingsSet => "settings-set",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum PackCommand {
    List,
    Mutation {
        action: PackAction,
        operation_id: Option<String>,
        expected_generation: Option<u64>,
        wait: Option<Duration>,
        async_requested: bool,
    },
    Validate {
        path: PathBuf,
    },
    Preview {
        path: PathBuf,
        output: PathBuf,
        phase: pack_authoring::PreviewPhase,
        time_ms: u64,
        reaction: Option<pack_authoring::PreviewReaction>,
        reaction_age_ms: Option<u64>,
        pose: Option<pose::PoseKind>,
        hit_overlay: bool,
    },
    Export {
        id: String,
        revision: Option<u64>,
        output: PathBuf,
    },
    Status {
        operation_id: String,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum SettingsCommand {
    Get,
    Set(String, bool),
}

#[derive(Debug, Default)]
struct Cli {
    command: Option<CommandKind>,
    update_operation: Option<String>,
    pack: Option<PackCommand>,
    settings: Option<SettingsCommand>,
    presentation: Option<automation_cli::PresentationCommand>,
    automation: Option<automation_cli::DomainCommand>,
    host: Option<ObservationSelection>,
    extra_tokens: Vec<String>,
    extension_command: Option<String>,
    extension_options: Vec<OsString>,
    assets: Option<PathBuf>,
    config_dir: Option<PathBuf>,
    state_dir: Option<PathBuf>,
    version: bool,
    help: bool,
    approve_update: bool,
}

pub(crate) fn run(composition: Arc<Composition>) -> Result<(), String> {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    #[cfg(desktop_pet_legacy)]
    let arguments = desktop_pet_update_coordinator::legacy_entry::preprocess(arguments)?;
    let cli = parse_cli(decode_args(arguments)?, &composition)?;
    if cli.help {
        println!("Usage: {} [COMMAND] [OPTIONS]\n", identity::BINARY);
        print!("{USAGE}");
        for extension in &composition.extensions.cli {
            for option in extension.options() {
                println!("  {}  {}", option.name, option.help);
            }
            for command in extension.commands() {
                println!("  {command}");
            }
        }
        for extension in &composition.extensions.lifecycle {
            for option in extension.preference_options() {
                println!("  preferences set {}  {}", option.name, option.help);
            }
        }
        return Ok(());
    }
    if cli.version {
        println!("{} {}", identity::BINARY, identity::VERSION);
        return Ok(());
    }
    let command = cli.command.unwrap_or(CommandKind::Ensure);
    if cli.assets.is_some()
        && matches!(
            command,
            CommandKind::Pack
                | CommandKind::SettingsGet
                | CommandKind::SettingsSet
                | CommandKind::Presentation
                | CommandKind::Preferences
                | CommandKind::Sessions
                | CommandKind::Dialogue
                | CommandKind::Worktree
                | CommandKind::UpdateStatus
                | CommandKind::Profile
                | CommandKind::Installation
                | CommandKind::Extension
                | CommandKind::LegacyMigrationRoute
                | CommandKind::MigrationIntegrationHandover
                | CommandKind::Update
        )
    {
        return Err("--assets is not valid with this command".into());
    }
    if command == CommandKind::UpdateCapabilities {
        return print_json(&identity::capabilities());
    }
    if command == CommandKind::DistributionIdentity {
        return print_json(&identity::json());
    }
    if command == CommandKind::Profile {
        match cli
            .extra_tokens
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["show", "--json"] => {
                return print_json(
                    &Paths::resolve_readonly(cli.config_dir.as_deref(), cli.state_dir.as_deref())?
                        .record(),
                )
            }
            ["adopt"] | ["adopt", "--json"] => {
                let config = cli
                    .config_dir
                    .as_deref()
                    .ok_or("profile adopt requires --config-dir ABS")?;
                let state = cli
                    .state_dir
                    .as_deref()
                    .ok_or("profile adopt requires --state-dir ABS")?;
                return print_json(&lifecycle::adopt_profile(config, state)?);
            }
            _ => return Err(
                "expected profile show --json or profile adopt --config-dir ABS --state-dir ABS"
                    .into(),
            ),
        }
    }
    if command == CommandKind::Installation {
        let executable = bundle::executable().map_err(|e| e.to_string())?;
        return match cli.extra_tokens.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["inspect", "--json"] => print_json(&desktop_pet_update_coordinator::installation::inspect(&executable)?),
            ["discover", "--json"] => print_json(&desktop_pet_update_coordinator::installation::discover(None)?),
            ["discover", "--executable", path, "--json"] | ["discover", "--json", "--executable", path] => {
                print_json(&desktop_pet_update_coordinator::installation::discover(Some(Path::new(path)))?)
            }
            _ => Err("expected installation inspect --json or installation discover [--executable ABS] --json".into()),
        };
    }
    if command == CommandKind::UpdateStatus {
        let Some(state_dir) = lifecycle::update_status_directory(cli.state_dir.as_deref())? else {
            if let Some(id) = cli.update_operation.as_deref() {
                return Err(format!("no durable update operation found for {id}"));
            }
            return print_json(
                &serde_json::json!({"type":"update-status","record":null,"recovery":"No update operation recorded for this profile."}),
            );
        };
        return update_status(&state_dir, cli.update_operation.as_deref());
    }
    if command == CommandKind::Update {
        if !identity::LEGACY
            || !cli.approve_update
            || cli.config_dir.is_none()
            || cli.state_dir.is_none()
        {
            return Err("update --yes is an explicit successor bridge approval and requires both --config-dir ABS and --state-dir ABS".into());
        }
        let paths = Paths::resolve_readonly(cli.config_dir.as_deref(), cli.state_dir.as_deref())?;
        let (plan, path) = desktop_pet_update_coordinator::migration::approve_profile_update(
            &paths.config_dir,
            &paths.state_dir,
            identity::herdr_migration_revision(),
        )?;
        let helper = plan
            .target
            .staged_bundle
            .join("Contents/MacOS")
            .join(identity::neutral::HELPER);
        // Replace this legacy CLI process so real source-root retirement is not
        // obstructed by an otherwise idle waiter mapped from the retiring image.
        let error = Command::new(helper)
            .args(["migrate-legacy", "--plan"])
            .arg(path)
            .env_clear()
            .envs(&plan.profiles[0].environment)
            .stdin(Stdio::null())
            .exec();
        return Err(format!(
            "verified automatic update helper could not execute: {error}"
        ));
    }
    if command == CommandKind::LegacyMigrationRoute {
        if identity::LEGACY {
            return Err("legacy distributions cannot authorize neutral integration routes".into());
        }
        if cli.config_dir.is_none() || cli.state_dir.is_none() {
            return Err(
                "legacy-migration-route requires both original absolute profile directories".into(),
            );
        }
        let paths = Paths::resolve_readonly(cli.config_dir.as_deref(), cli.state_dir.as_deref())?;
        let mut input = Vec::new();
        io::stdin()
            .lock()
            .take(16385)
            .read_to_end(&mut input)
            .map_err(|e| e.to_string())?;
        if input.len() > 16384 {
            return Err("migration route request exceeds its bound".into());
        }
        let route = desktop_pet_update_coordinator::migration::committed_route(
            &paths.config_dir,
            &paths.state_dir,
            &input,
        )?;
        let executable = bundle::executable().map_err(|e| e.to_string())?;
        if route["target"]["executable"].as_str() != executable.to_str() {
            return Err("migration route target is not this verified executable".into());
        }
        return print_json(&route);
    }
    if command == CommandKind::MigrationIntegrationHandover {
        if identity::LEGACY {
            return Err("legacy distributions cannot confirm neutral integration handovers".into());
        }
        let (config_dir, state_dir) = migration_handover_profile(&cli)?;
        let mut input = Vec::new();
        io::stdin()
            .lock()
            .take(16385)
            .read_to_end(&mut input)
            .map_err(|e| e.to_string())?;
        if input.len() > 16384 {
            return Err("migration handover request exceeds its bound".into());
        }
        let response = desktop_pet_update_coordinator::migration::integration_handover(
            config_dir, state_dir, &input,
        )?;
        return print_json(&response);
    }
    let paths = if command == CommandKind::Extension || command == CommandKind::Status {
        Paths::resolve_readonly(cli.config_dir.as_deref(), cli.state_dir.as_deref())?
    } else {
        Paths::resolve(cli.config_dir.as_deref(), cli.state_dir.as_deref())?
    };
    if command == CommandKind::Extension {
        if cli.assets.is_some() {
            return Err("--assets is not valid with integration commands".into());
        }
        let command = cli
            .extension_command
            .as_deref()
            .ok_or("missing integration command")?;
        let extension = composition
            .extensions
            .cli
            .iter()
            .find(|e| e.commands().contains(&command))
            .ok_or("integration command is not registered")?;
        let args = cli
            .extra_tokens
            .iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let input_limit = extension.max_input_bytes(command);
        let reads_input = composition
            .cli_input_policies
            .get(command)
            .map(|policy| policy(&args))
            .transpose()?
            .unwrap_or(input_limit > 0);
        if reads_input && input_limit == 0 {
            return Err("registered command has no bounded input allowance".into());
        }
        let mut input = Vec::new();
        if reads_input {
            io::stdin()
                .lock()
                .take(input_limit as u64 + 1)
                .read_to_end(&mut input)
                .map_err(|e| e.to_string())?;
            if input.len() > input_limit {
                return Err("integration input exceeds frame limit".into());
            }
        }
        if control::control_socket_is_live(&paths.control_socket, CONTROL_TIMEOUT) {
            let current = send_command(&paths.control_socket, "status", None, CONTROL_TIMEOUT)?;
            require_profile_identity(&current, &paths)?;
        }
        let profile = ProfilePaths {
            config_dir: paths.config_dir,
            state_dir: paths.state_dir,
            control_socket: paths.control_socket,
        };
        let output = extension.execute(CliInvocation {
            command,
            arguments: &args,
            input: &input,
            profile: &profile,
        })?;
        io::stdout()
            .write_all(&output.stdout)
            .map_err(|e| e.to_string())?;
        io::stderr()
            .write_all(&output.stderr)
            .map_err(|e| e.to_string())?;
        if output.exit_code != 0 {
            std::process::exit(output.exit_code);
        }
        return Ok(());
    }
    let current = if command != CommandKind::Daemon
        && control::control_socket_is_live(&paths.control_socket, CONTROL_TIMEOUT)
    {
        Some(send_command(
            &paths.control_socket,
            "status",
            None,
            CONTROL_TIMEOUT,
        )?)
    } else {
        None
    };
    if let Some(current) = &current {
        require_profile_identity(current, &paths)?;
    }
    if command == CommandKind::Settings && current.is_some() {
        return open_settings(&paths);
    }
    let startup = matches!(
        command,
        CommandKind::Ensure | CommandKind::Start | CommandKind::Restart | CommandKind::Daemon
    );
    let override_mode = if startup {
        resolve_observation_override(cli.host, env::var(OBSERVATION_SELECTION_ENV))?
    } else {
        None
    };
    let observation_mode = select_observation_mode(override_mode, current.as_ref(), || {
        #[cfg(desktop_pet_legacy)]
        if let Some(selection) =
            desktop_pet_update_coordinator::legacy_entry::saved_observation_override(
                &paths.config_dir,
            )?
        {
            return Ok(selection);
        }
        preferences::Preferences::load(&paths.config_dir).map(|saved| saved.observation_host())
    })?;
    if let ObservationSelection::Provider(provider) = &observation_mode {
        if !composition.provider_labels.contains_key(provider) {
            return Err(format!(
                "provider {provider:?} is not registered in this executable"
            ));
        }
    }
    let assets_override = cli.assets.is_some();
    let assets = if command.needs_assets() {
        Some(resolve_assets(cli.assets.as_deref())?)
    } else {
        cli.assets
    };
    if command == CommandKind::Daemon {
        let (generation, token) = updater_start_authorization()?;
        admit_startup(
            &paths,
            generation,
            token.as_deref(),
            &observation_mode,
            &Cell::new(None),
        )?;
        let automatic_start = match env::var("DESKTOP_PET_AUTOMATIC_START") {
            Ok(value) if value == "1" => true,
            Ok(value) if value == "0" => false,
            Err(env::VarError::NotPresent) => false,
            _ => return Err("invalid DESKTOP_PET_AUTOMATIC_START".into()),
        };
        let mut restart_options = cli.extension_options;
        if assets_override {
            restart_options.push("--assets".into());
            restart_options.push(
                assets
                    .as_ref()
                    .ok_or("daemon assets unavailable")?
                    .as_os_str()
                    .to_owned(),
            );
        }
        let _ = DAEMON_RESTART_OPTIONS.set(restart_options);
        return daemon::run(DaemonConfig {
            paths,
            assets: assets.ok_or("daemon assets unavailable")?,
            assets_override,
            startup_token: env::var("DESKTOP_PET_STARTUP_TOKEN").ok(),
            automatic_start,
            observation_mode,
            composition,
        });
    }
    execute_command(
        command,
        paths,
        assets,
        assets_override,
        observation_mode,
        cli.pack,
        cli.settings,
        cli.presentation,
        cli.automation,
        composition,
        cli.extension_options,
    )
}

fn migration_handover_profile(cli: &Cli) -> Result<(&Path, &Path), String> {
    let config_dir = cli
        .config_dir
        .as_deref()
        .ok_or("migration-integration-handover requires --config-dir ABS")?;
    let state_dir = cli
        .state_dir
        .as_deref()
        .ok_or("migration-integration-handover requires --state-dir ABS")?;
    if !config_dir.is_absolute() || !state_dir.is_absolute() {
        return Err(
            "migration-integration-handover requires both original absolute profile directories"
                .into(),
        );
    }
    if cli.host.is_some() || cli.assets.is_some() || !cli.extension_options.is_empty() {
        return Err(
            "migration-integration-handover accepts only the original profile directories".into(),
        );
    }
    Ok((config_dir, state_dir))
}

fn require_profile_identity(response: &ControlResponse, paths: &Paths) -> Result<(), String> {
    let selected = bundle::executable().map_err(|e| e.to_string())?;
    if Path::new(&response.executable_path) != selected
        || Path::new(&response.config_dir) != paths.config_dir
        || Path::new(&response.state_dir) != paths.state_dir
    {
        return Err(format!("running daemon image/profile mismatch: executable={}, config_dir={}, state_dir={}; selected executable={}, config_dir={}, state_dir={}",
            response.executable_path, response.config_dir, response.state_dir, selected.display(), paths.config_dir.display(), paths.state_dir.display()));
    }
    Ok(())
}

fn resolve_observation_override(
    explicit: Option<ObservationSelection>,
    owned_environment: Result<String, env::VarError>,
) -> Result<Option<ObservationSelection>, String> {
    if explicit.is_some() {
        return Ok(explicit);
    }
    match owned_environment {
        Ok(value) => value.parse().map(Some),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(format!(
            "{OBSERVATION_SELECTION_ENV} must be auto or a registered provider"
        )),
    }
}

fn select_observation_mode(
    explicit: Option<ObservationSelection>,
    running: Option<&ControlResponse>,
    saved: impl FnOnce() -> Result<ObservationSelection, String>,
) -> Result<ObservationSelection, String> {
    if let Some(mode) = explicit {
        return Ok(mode);
    }
    match running {
        Some(response) => response.observation_mode.parse(),
        None => saved(),
    }
}

pub(crate) fn persist_observation_mode(
    directory: &Path,
    mode: ObservationSelection,
) -> Result<(), String> {
    let mut preferences = preferences::Preferences::load(directory)?;
    preferences.apply_patch(
        preferences::PreferencePatch {
            observation_host: Some(mode),
            ..Default::default()
        },
        directory,
    )?;
    Ok(())
}

fn execute_command(
    command: CommandKind,
    paths: Paths,
    assets: Option<PathBuf>,
    assets_override: bool,
    observation_mode: ObservationSelection,
    pack: Option<PackCommand>,
    settings: Option<SettingsCommand>,
    presentation: Option<automation_cli::PresentationCommand>,
    automation: Option<automation_cli::DomainCommand>,
    composition: Arc<Composition>,
    extension_options: Vec<OsString>,
) -> Result<(), String> {
    match command {
        CommandKind::Ensure | CommandKind::Start | CommandKind::Settings => ensure(
            paths,
            assets,
            assets_override,
            command,
            observation_mode,
            composition,
            extension_options,
        ),
        CommandKind::Restart => {
            stop(paths.clone())?;
            ensure(
                paths,
                assets,
                assets_override,
                CommandKind::Start,
                observation_mode,
                composition,
                extension_options,
            )
        }
        CommandKind::Stop => stop(paths),
        CommandKind::Status => status(paths, observation_mode, &composition),
        CommandKind::SettingsGet | CommandKind::SettingsSet => {
            let settings = match settings.ok_or("settings operation missing")? {
                SettingsCommand::Get => {
                    control::access_registered_lifecycle(&paths, None, &composition)?
                }
                SettingsCommand::Set(key, value) => {
                    control::access_registered_lifecycle(&paths, Some((&key, value)), &composition)?
                }
            };
            print_json(&serde_json::json!({"ok":true,"settings":settings}))
        }
        CommandKind::Show
        | CommandKind::Hide
        | CommandKind::Toggle
        | CommandKind::Passthrough
        | CommandKind::AlphaPassthrough
        | CommandKind::ShowBubble
        | CommandKind::HideBubble
        | CommandKind::BubbleAbove
        | CommandKind::BubbleBelow
        | CommandKind::BubbleLeft
        | CommandKind::BubbleRight
        | CommandKind::BubbleAuto
        | CommandKind::Reset
        | CommandKind::Bigger
        | CommandKind::Smaller => control_action(command, paths),
        CommandKind::Pack => execute_pack(pack.ok_or("pack requires an operation")?, paths),
        CommandKind::Presentation => automation_cli::execute(
            presentation.ok_or("presentation requires an operation")?,
            &paths.control_socket,
        ),
        CommandKind::Preferences
        | CommandKind::Sessions
        | CommandKind::Dialogue
        | CommandKind::Worktree => automation_cli::execute_domain(
            automation.ok_or("automation operation missing")?,
            &paths.control_socket,
        ),
        _ => unreachable!(),
    }
}

fn execute_pack(command: PackCommand, paths: Paths) -> Result<(), String> {
    match command {
        PackCommand::List => execute_pack_list(&paths),
        PackCommand::Mutation {
            action,
            operation_id,
            expected_generation,
            wait,
            async_requested,
        } => execute_pack_mutation(
            &paths,
            action,
            operation_id,
            expected_generation,
            wait,
            async_requested,
        ),
        PackCommand::Status { operation_id } => execute_pack_status(&paths, &operation_id),
        PackCommand::Validate { path } => pack_authoring::validate(&path),
        PackCommand::Preview {
            path,
            output,
            phase,
            time_ms,
            reaction,
            reaction_age_ms,
            pose,
            hit_overlay,
        } => pack_authoring::preview(
            &path,
            &output,
            phase,
            time_ms,
            reaction,
            reaction_age_ms,
            pose,
            hit_overlay,
        ),
        PackCommand::Export {
            id,
            revision,
            output,
        } => {
            let builtin_assets = if id == "default" {
                Some(resolve_assets(None)?)
            } else {
                None
            };
            pack_authoring::export(
                &paths.config_dir,
                builtin_assets.as_deref(),
                &id,
                revision,
                &output,
            )
        }
    }
}

fn execute_pack_list(paths: &Paths) -> Result<(), String> {
    let socket = &paths.control_socket;
    if control::control_socket_is_live(socket, CONTROL_TIMEOUT) {
        let reply = send_pack_list(socket, CONTROL_TIMEOUT)?;
        if !reply.ok {
            return Err(reply
                .error
                .unwrap_or_else(|| "pack listing failed".to_owned()));
        }
        let listing = reply
            .listing
            .ok_or_else(|| "pack listing is unavailable".to_owned())?;
        return print_json(&listing);
    }
    let lock = acquire_lock_until(paths, Instant::now() + CONTROL_TIMEOUT)?;
    if control::control_socket_is_live(socket, Duration::from_millis(150)) {
        drop(lock);
        let reply = send_pack_list(socket, CONTROL_TIMEOUT)?;
        if !reply.ok {
            return Err(reply
                .error
                .unwrap_or_else(|| "pack listing failed".to_owned()));
        }
        let listing = reply
            .listing
            .ok_or_else(|| "pack listing is unavailable".to_owned())?;
        return print_json(&listing);
    }
    let store = PackStore::new(paths.config_dir.clone(), None);
    let listing = store.list()?;
    drop(lock);
    print_json(&listing)
}
fn execute_pack_status(paths: &Paths, operation_id: &str) -> Result<(), String> {
    let socket = &paths.control_socket;
    if control::control_socket_is_live(socket, CONTROL_TIMEOUT) {
        let reply = send_pack_status(socket, operation_id, CONTROL_TIMEOUT)?;
        return print_pack_operation_reply(reply);
    }
    let lock = acquire_lock_until(paths, Instant::now() + CONTROL_TIMEOUT)?;
    if control::control_socket_is_live(socket, Duration::from_millis(150)) {
        drop(lock);
        let reply = send_pack_status(socket, operation_id, CONTROL_TIMEOUT)?;
        return print_pack_operation_reply(reply);
    }
    let store = PackStore::new(paths.config_dir.clone(), None);
    let operation = store.operation_status(operation_id)?;
    drop(lock);
    match operation {
        Some(operation) => print_pack_operation(&operation),
        None => {
            let operation = PackOperation {
                operation_id: operation_id.to_owned(),
                state: "unknown".to_owned(),
                committed: false,
                ui_applied: false,
                generation: None,
                error: Some("operation identity is not retained".to_owned()),
            };
            print_pack_operation(&operation)
        }
    }
}

fn execute_pack_mutation(
    paths: &Paths,
    action: PackAction,
    operation_id: Option<String>,
    expected_generation: Option<u64>,
    wait: Option<Duration>,
    async_requested: bool,
) -> Result<(), String> {
    let request = PackRequest {
        operation_id: operation_id.unwrap_or_else(new_operation_id),
        expected_generation,
        action,
    };
    let offline_origin = InstallOrigin::Unknown {
        reason: "offline pack admission".into(),
    };
    start_allowed(&offline_origin, &paths.state_dir, None, None)?;
    let socket = &paths.control_socket;
    if control::control_socket_is_live(socket, CONTROL_TIMEOUT) {
        return submit_live_pack(socket, request, wait);
    }
    let lock = acquire_lock_until(paths, Instant::now() + CONTROL_TIMEOUT)?;
    if control::control_socket_is_live(socket, Duration::from_millis(150)) {
        drop(lock);
        return submit_live_pack(socket, request, wait);
    }
    if async_requested {
        return Err("pack --no-wait or explicit --wait requires a running daemon; start daemon or use the synchronous offline command".into());
    }
    start_allowed(&offline_origin, &paths.state_dir, None, None)?;
    let builtin_assets = resolve_assets(None)?;
    let operation = execute_offline(paths.config_dir.clone(), builtin_assets, request)?;
    drop(lock);
    print_json(&operation)?;
    operation_result(&operation)
}

fn submit_live_pack(
    socket: &Path,
    request: PackRequest,
    wait: Option<Duration>,
) -> Result<(), String> {
    let operation_id = request.operation_id.clone();
    let reply = match send_pack_request(socket, request, CONTROL_TIMEOUT) {
        Ok(reply) => reply,
        Err(error) => return Err(format!("pack operation {operation_id} outcome is unknown after control disconnect: {error}; query pack status {operation_id}")),
    };
    let operation = reply.operation.ok_or_else(|| {
        reply
            .error
            .clone()
            .unwrap_or_else(|| "pack request was not accepted".to_owned())
    })?;
    if operation.operation_id != operation_id {
        return Err(format!("pack operation {operation_id} outcome is unknown after a mismatched submission identity; query pack status {operation_id}"));
    }
    print_json(&operation)?;
    if !reply.ok {
        return Err(operation
            .error
            .unwrap_or_else(|| "pack request failed".to_owned()));
    }
    if is_pack_terminal(&operation.state) {
        return operation_result(&operation);
    }
    let Some(wait) = wait else {
        return Ok(()); // A nonterminal submission reply is only an ACK.
    };
    let deadline = Instant::now() + wait;
    let mut operation = operation;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return pack_wait_expired(&operation);
        }
        thread::sleep(POLL_INTERVAL.min(remaining));
        if Instant::now() >= deadline {
            return pack_wait_expired(&operation);
        }
        let reply = match control::send_pack_status_until(
            socket,
            &operation_id,
            deadline.min(Instant::now() + CONTROL_TIMEOUT),
        ) {
            Ok(reply) => reply,
            Err(control::ClientDeadlineError::Elapsed) if Instant::now() >= deadline => {
                return pack_wait_expired(&operation);
            }
            Err(error) => return Err(format!("pack operation {operation_id} status is unknown after control error: {error}; query pack status {operation_id}")),
        };
        if Instant::now() >= deadline {
            return pack_wait_expired(&operation);
        }
        let latest = reply.operation.ok_or_else(|| {
            format!(
                "pack operation {operation_id} status is unknown: {}; query pack status {operation_id}",
                reply.error.unwrap_or_else(|| "operation status is unavailable".to_owned())
            )
        })?;
        if latest.operation_id != operation_id {
            return Err(format!("pack operation {operation_id} status is unknown after a mismatched operation identity; query pack status {operation_id}"));
        }
        if latest.state != operation.state {
            print_json(&latest)?;
        }
        operation = latest;
        if is_pack_terminal(&operation.state) {
            return operation_result(&operation);
        }
    }
}

fn pack_wait_expired(operation: &PackOperation) -> Result<(), String> {
    print_json(&serde_json::json!({"deadline_exceeded":true,"operation":operation}))?;
    Err(format!(
        "pack operation {} remains pending (last state {}); query pack status {}",
        operation.operation_id, operation.state, operation.operation_id
    ))
}

fn is_pack_terminal(state: &str) -> bool {
    matches!(
        state,
        "completed"
            | "failed"
            | "canceled"
            | "durability_unknown"
            | "committed_pending_apply"
            | "unknown"
    )
}

fn operation_result(operation: &PackOperation) -> Result<(), String> {
    if operation.state == "completed" {
        Ok(())
    } else {
        Err(operation
            .error
            .clone()
            .unwrap_or_else(|| format!("pack operation ended in {}", operation.state)))
    }
}

fn pack_status_result(operation: &PackOperation) -> Result<(), String> {
    if matches!(
        operation.state.as_str(),
        "accepted" | "preparing" | "applying"
    ) {
        Ok(())
    } else {
        operation_result(operation)
    }
}

fn print_pack_operation(operation: &PackOperation) -> Result<(), String> {
    print_json(operation)?;
    pack_status_result(operation)
}

fn print_pack_operation_reply(reply: PackReplyEnvelope) -> Result<(), String> {
    if let Some(operation) = reply.operation {
        print_pack_operation(&operation)
    } else {
        Err(reply
            .error
            .unwrap_or_else(|| "pack operation is unavailable".to_owned()))
    }
}

pub(crate) fn print_json<T: serde::Serialize>(value: &T) -> Result<(), String> {
    let text = serde_json::to_string(value)
        .map_err(|error| format!("cannot encode pack response: {error}"))?;
    println!("{text}");
    Ok(())
}

pub(crate) fn new_operation_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{nanos:x}-{:x}-{counter:x}", std::process::id())
}

/// `verified` is this launch's already verified installation owner. Owner classification fully
/// validates the app bundle, so it is not repeated for every admission (as in the daemon); the
/// context, migration authorization and updater fences are still rebuilt and re-read each time.
fn startup_origin(
    paths: &Paths,
    selection: ObservationSelection,
    verified: Option<InstallOrigin>,
) -> Result<InstallOrigin, String> {
    let executable = bundle::executable().map_err(|e| e.to_string())?;
    let context = UpdateContext {
        running: executable_identity(&executable)?,
        executable,
        version: identity::VERSION.into(),
        instance_id: "launcher".into(),
        config_dir: paths.config_dir.clone(),
        state_dir: paths.state_dir.clone(),
        #[cfg(desktop_pet_legacy)]
        host_plugin_config_dir:
            desktop_pet_update_coordinator::legacy_entry::host_plugin_config_dir(false),
        #[cfg(desktop_pet_legacy)]
        herdr_socket: desktop_pet_update_coordinator::legacy_entry::herdr_socket(),
        running_origin: None,
        assets_override: None,
        environment: desktop_pet_update_coordinator::native_runtime::captured_environment(
            selection,
            |key| env::var(key).ok(),
        )?,
        locale: "en".into(),
    };
    daemon::validate_migration_startup_context(&context)?;
    Ok(verified.unwrap_or_else(|| {
        detect_origin(&context).unwrap_or_else(|reason| InstallOrigin::Unknown { reason })
    }))
}

fn updater_start_authorization() -> Result<(Option<u64>, Option<String>), String> {
    let generation = env::var("DESKTOP_PET_UPDATER_GENERATION")
        .ok()
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| "invalid updater generation".to_owned())
        })
        .transpose()?;
    let token = env::var("DESKTOP_PET_UPDATER_TOKEN").ok();
    if generation.is_some() != token.is_some() {
        return Err("updater start needs both token and generation".into());
    }
    Ok((generation, token))
}

fn admit_startup(
    paths: &Paths,
    expected_generation: Option<u64>,
    updater_token: Option<&str>,
    observation_mode: &ObservationSelection,
    verified_origin: &Cell<Option<InstallOrigin>>,
) -> Result<(), String> {
    let origin = startup_origin(paths, observation_mode.clone(), verified_origin.take())?;
    let allowed = start_allowed(
        &origin,
        &paths.state_dir,
        expected_generation,
        updater_token,
    );
    // An unverified owner is re-detected next time so a transient validation failure never
    // relaxes later installation reservation checks.
    if !matches!(origin, InstallOrigin::Unknown { .. }) {
        verified_origin.set(Some(origin));
    }
    allowed
}

fn ensure(
    paths: Paths,
    assets: Option<PathBuf>,
    assets_override: bool,
    command: CommandKind,
    observation_mode: ObservationSelection,
    composition: Arc<Composition>,
    extension_options: Vec<OsString>,
) -> Result<(), String> {
    let automatic_start = command == CommandKind::Ensure;
    let settings_request = command == CommandKind::Settings;
    if settings_request && control::control_socket_is_live(&paths.control_socket, CONTROL_TIMEOUT) {
        return open_settings(&paths);
    }
    let (generation, token) = updater_start_authorization()?;
    let verified_origin = Cell::new(None);
    let admit = || {
        admit_startup(
            &paths,
            generation,
            token.as_deref(),
            &observation_mode,
            &verified_origin,
        )
    };
    admit()?;
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    let mut last_error = None;
    while Instant::now() < deadline {
        admit()?;
        if control::control_socket_is_live(&paths.control_socket, Duration::from_millis(150)) {
            match wait_for_ready(
                &paths,
                deadline,
                (!settings_request).then_some(observation_mode.clone()),
            ) {
                Ok(response) => {
                    require_profile_identity(&response, &paths)?;
                    admit()?;
                    if settings_request {
                        return open_settings(&paths);
                    }
                    attach_integrations(&composition, &paths, &observation_mode)?;
                    return print_response(&response);
                }
                Err(ReadyFailure::ModeMismatch(error)) => return Err(error),
                Err(ReadyFailure::Exiting(error)) => {
                    last_error = Some(error);
                    wait_for_finalization(&paths, deadline)?;
                    continue;
                }
                Err(ReadyFailure::TimedOut(error)) => {
                    if control::control_socket_is_live(
                        &paths.control_socket,
                        Duration::from_millis(150),
                    ) {
                        let response =
                            send_command(&paths.control_socket, "status", None, CONTROL_TIMEOUT)?;
                        if !response.shutdown {
                            return Err(error);
                        }
                    }
                    last_error = Some(error);
                    wait_for_finalization(&paths, deadline)?;
                    continue;
                }
            }
        }
        let Some(lock) = LifecycleLock::try_acquire(&paths.state_dir)? else {
            thread::sleep(POLL_INTERVAL);
            continue;
        };
        if control::control_socket_is_live(&paths.control_socket, Duration::from_millis(150)) {
            drop(lock);
            continue;
        }
        admit()?;
        let settings = lifecycle::read_settings(&paths.config_dir)?;
        if automatic_start && !settings.auto_start {
            if lifecycle::startup_pending(&paths.config_dir)? {
                drop(lock);
                thread::sleep(POLL_INTERVAL);
                continue;
            }
            return print_json(
                &serde_json::json!({"type":"ensure","ok":true,"running":false,"skipped":true,"auto_start":false,"observation_mode":observation_mode.as_str()}),
            );
        }
        let assets = resolve_assets(assets.as_deref())?;
        admit()?;
        let profile = ProfileStore::with_lock(&paths.config_dir, Arc::new(lock))?
            .with_preferences_validator(crate::preferences::Preferences::validate_committed_object);
        if !settings_request {
            profile.with_writer_lock(|| {
                persist_observation_mode(&paths.config_dir, observation_mode.clone())
            })?;
        }
        let Some(lease) = lifecycle::begin_startup(&profile, &composition.extensions.lifecycle)?
        else {
            drop(profile);
            thread::sleep(POLL_INTERVAL);
            continue;
        };
        if let Err(error) = admit() {
            let _ = profile
                .with_writer_lock(|| lifecycle::abandon_startup(&paths.config_dir, &lease.token));
            return Err(error);
        }
        let child = match spawn_daemon(
            &paths,
            Some(&assets),
            assets_override,
            &lease.token,
            automatic_start,
            &observation_mode,
            &extension_options,
        ) {
            Ok(child) => child,
            Err(error) => {
                let _ = profile.with_writer_lock(|| {
                    lifecycle::abandon_startup(&paths.config_dir, &lease.token)
                });
                return Err(error);
            }
        };
        profile.with_writer_lock(|| {
            lifecycle::set_startup_child(&paths.config_dir, &lease.token, child)
        })?;
        drop(profile);
        match wait_for_ready(
            &paths,
            deadline,
            (!settings_request).then_some(observation_mode.clone()),
        ) {
            Ok(response) => {
                require_profile_identity(&response, &paths)?;
                if settings_request {
                    return open_settings(&paths);
                }
                admit()?;
                attach_integrations(&composition, &paths, &observation_mode)?;
                return print_response(&response);
            }
            Err(failure) => {
                if let ReadyFailure::TimedOut(error) = &failure {
                    if control::control_socket_is_live(
                        &paths.control_socket,
                        Duration::from_millis(150),
                    ) {
                        let response =
                            send_command(&paths.control_socket, "status", None, CONTROL_TIMEOUT)?;
                        if !response.shutdown {
                            return Err(error.clone());
                        }
                    }
                }
                clear_startup_reservation(&paths, &lease.token, deadline)?;
                wait_for_finalization(&paths, deadline)?;
                return Err(failure.into_message());
            }
        }
    }
    Err(last_error.unwrap_or_else(|| "timed out waiting for desktop-pet readiness".into()))
}

fn attach_integrations(
    composition: &Composition,
    paths: &Paths,
    selection: &ObservationSelection,
) -> Result<(), String> {
    for frame in &composition.attach_frames {
        if let Some(frame) = frame(selection)? {
            let reply =
                control::send_raw_frame(&paths.control_socket, &frame, CONTROL_TIMEOUT, 16 * 1024)?;
            let value: serde_json::Value =
                serde_json::from_slice(&reply).map_err(|e| e.to_string())?;
            if value.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
                return Err(value
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("integration registration rejected")
                    .into());
            }
        }
    }
    Ok(())
}

pub(crate) fn acquire_lock_until(
    paths: &Paths,
    deadline: Instant,
) -> Result<LifecycleLock, String> {
    loop {
        match LifecycleLock::try_acquire(&paths.state_dir)? {
            Some(lock) => return Ok(lock),
            None if Instant::now() < deadline => thread::sleep(POLL_INTERVAL),
            None => return Err("timed out waiting for desktop-pet lifecycle lock".to_owned()),
        }
    }
}

fn clear_startup_reservation(paths: &Paths, token: &str, deadline: Instant) -> Result<(), String> {
    let lock = acquire_lock_until(paths, deadline)?;
    lifecycle::abandon_startup(&paths.config_dir, token)?;
    drop(lock);
    Ok(())
}

fn wait_for_finalization(paths: &Paths, deadline: Instant) -> Result<(), String> {
    loop {
        if control::control_socket_is_live(&paths.control_socket, Duration::from_millis(100)) {
            if Instant::now() >= deadline {
                return Err("timed out waiting for desktop-pet finalization".to_owned());
            }
            thread::sleep(POLL_INTERVAL);
            continue;
        }
        if let Some(lock) = LifecycleLock::try_acquire(&paths.state_dir)? {
            let pending = lifecycle::startup_pending(&paths.config_dir)?;
            if !pending {
                lifecycle::cancel_startup(&paths.config_dir)?;
                drop(lock);
                return Ok(());
            }
            drop(lock);
        }
        if Instant::now() >= deadline {
            return Err("timed out waiting for desktop-pet finalization".to_owned());
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn stop(paths: Paths) -> Result<(), String> {
    record_user_stop(&paths.state_dir)?;
    let deadline = Instant::now() + STOP_TIMEOUT;
    loop {
        if Instant::now() >= deadline {
            return Err("timed out stopping desktop-pet".to_owned());
        }
        if control::control_socket_is_live(&paths.control_socket, Duration::from_millis(150)) {
            match send_command(&paths.control_socket, "stop", None, CONTROL_TIMEOUT) {
                Ok(response) if response.ok || response.shutdown => {
                    wait_for_finalization(&paths, deadline)?;
                    return Ok(());
                }
                Ok(response) => {
                    return Err(response
                        .error
                        .unwrap_or_else(|| "cannot stop daemon".to_owned()))
                }
                Err(error)
                    if control::control_socket_is_live(
                        &paths.control_socket,
                        Duration::from_millis(100),
                    ) =>
                {
                    return Err(error)
                }
                Err(_) => continue,
            }
        }
        let Some(lock) = LifecycleLock::try_acquire(&paths.state_dir)? else {
            thread::sleep(POLL_INTERVAL);
            continue;
        };
        if control::control_socket_is_live(&paths.control_socket, Duration::from_millis(150)) {
            drop(lock);
            continue;
        }
        lifecycle::cancel_startup(&paths.config_dir)?;
        drop(lock);
        wait_for_finalization(&paths, deadline)?;
        return Ok(());
    }
}

fn update_status_payload(
    state_dir: &Path,
    operation_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    let record = if let Some(id) = operation_id {
        observe_operation(state_dir, id)?
            .ok_or_else(|| format!("no durable update operation found for {id}"))?
    } else {
        let Some(record) = observe_latest_operation(state_dir)? else {
            return Ok(serde_json::json!({
                "type": "update-status", "record": null, "recovery": "No update operation recorded for this profile."
            }));
        };
        record
    };
    let record = match record {
        OperationObservation::Update(record) => record,
        OperationObservation::StandaloneInstall(record) => {
            let plan = desktop_pet_update_coordinator::standalone_install::read_plan(
                state_dir,
                &record.operation_id,
            )?;
            record.validate(&plan)?;
            let helper = &plan.helper_image.path;
            let staged = fs::symlink_metadata(helper).ok().is_some_and(|metadata| {
                metadata.is_file()
                    && metadata.uid() == lifecycle::effective_uid()
                    && metadata.permissions().mode() & 0o077 == 0
            });
            return Ok(serde_json::json!({
                "type": "update-status",
                "kind": "standalone_install",
                "operation_id": record.operation_id,
                "record": record,
                "plan": plan,
                "live_process_evidence": "Not inspected by this offline journal command.",
                "staged_helper_present_private": staged,
                "staged_helper_signature_verified": false,
                "helper_status_command": staged.then(|| serde_json::json!([helper, "install-status", "--state-dir", state_dir, "--operation-id", record.operation_id])),
                "helper_recover_command": staged.then(|| serde_json::json!([helper, "install-recover", "--state-dir", state_dir, "--operation-id", record.operation_id])),
                "recovery": "Verify the retained private helper signature and identity against the immutable plan, then use install-status and install-recover. Recovery reconciles existing authority; it does not grant a new restart or repeat Stop."
            }));
        }
    };
    let plan = desktop_pet_update_coordinator::read_plan(state_dir, &record.operation_id);
    let helper = state_dir
        .join("updates")
        .join(format!("helper-{}", record.operation_id));
    let staged = fs::symlink_metadata(&helper).ok().is_some_and(|metadata| {
        metadata.is_file()
            && metadata.uid() == lifecycle::effective_uid()
            && metadata.permissions().mode() & 0o077 == 0
    });
    let helper_path = helper.display().to_string();
    let state_path = state_dir.display().to_string();
    let id = record.operation_id.as_str();
    let last_durable_phase = record.phase;
    Ok(serde_json::json!({
        "type": "update-status",
        "kind": "update",
        "operation_id": id,
        "record": record,
        "last_durable_phase": last_durable_phase,
        "live_process_evidence": "Not inspected by this offline journal command. The last durable phase is not proof the helper or installer remains running.",
        "plan": plan.as_ref().ok(),
        "plan_error": plan.as_ref().err(),
        "staged_helper_present_private": staged,
        "staged_helper_signature_verified": false,
        "helper_status_command": staged.then(|| serde_json::json!([helper_path, "status", "--state-dir", state_path, "--operation-id", id])),
        "helper_recover_command": staged.then(|| serde_json::json!([helper_path, "recover", "--state-dir", state_path, "--operation-id", id])),
        "recovery": "Verify the private retained helper's code signature, then run helper_status_command for crash-aware helper/manager/candidate evidence. Run helper_recover_command only after inspecting the outcome; recovery never automatically repeats the installer."
    }))
}
fn update_status(state_dir: &Path, operation_id: Option<&str>) -> Result<(), String> {
    print_json(&update_status_payload(state_dir, operation_id)?)
}

fn status(
    paths: Paths,
    observation_mode: ObservationSelection,
    composition: &Composition,
) -> Result<(), String> {
    let response = if control::control_socket_is_live(&paths.control_socket, CONTROL_TIMEOUT) {
        send_command(&paths.control_socket, "status", None, CONTROL_TIMEOUT)?
    } else {
        let settings = lifecycle::read_settings(&paths.config_dir)?;
        let executable = bundle::executable()
            .map_err(|error| format!("cannot resolve executable path: {error}"))?;
        ControlResponse {
            r#type: "status".to_owned(),
            version: 1,
            app_version: desktop_pet_update_coordinator::identity::VERSION.to_owned(),
            updater_protocol: identity::PROTOCOL,
            observation_capability: identity::OBSERVATION_CAPABILITY,
            integration_api: identity::INTEGRATION_API,
            update_ui_visible: false,
            update_operation_id: None,
            migration_operation: None,
            migration_gated: false,
            #[cfg(desktop_pet_legacy)]
            migration_source_environment: None,
            #[cfg(desktop_pet_legacy)]
            migration_source_host_plugin_config_dir: None,
            provider_fields: Default::default(),
            observation_mode: observation_mode.as_str().to_owned(),
            ok: true,
            command: "status".to_owned(),
            error: None,
            ready: false,
            running: false,
            phase: "unknown".to_owned(),
            ui_ready: false,
            control_ready: false,
            registration_accepted: false,
            data_connected: false,
            connected_sources: 0,
            disconnected_sources: 0,
            host_connections: composition
                .provider_labels
                .keys()
                .map(|id| (id.clone(), 0))
                .collect(),
            sessions: 0,
            working: 0,
            blocked: 0,
            done: 0,
            unknown: 0,
            visible: false,
            passthrough: false,
            alpha_passthrough: false,
            bubble_visible: true,
            bubble_placement: bubble::BubblePlacement::Above,
            scale: state::DEFAULT_SCALE,
            shutdown: false,
            auto_start: settings.auto_start,
            pid: 0,
            instance_id: String::new(),
            running_sha256: String::new(),
            assets_override: None,
            executable_path: executable.display().to_string(),
            config_dir: paths.config_dir.display().to_string(),
            state_dir: paths.state_dir.display().to_string(),
        }
    };
    print_response(&response)
}

fn control_action(command: CommandKind, paths: Paths) -> Result<(), String> {
    let response = send_command(
        &paths.control_socket,
        command.as_str(),
        None,
        CONTROL_TIMEOUT,
    )?;
    if !response.ok {
        return Err(response
            .error
            .unwrap_or_else(|| "control action failed".into()));
    }
    print_response(&response)
}

fn open_settings(paths: &Paths) -> Result<(), String> {
    let response = send_command(&paths.control_socket, "settings", None, CONTROL_TIMEOUT)?;
    if !response.ok {
        return Err(response
            .error
            .unwrap_or_else(|| "cannot open settings".to_owned()));
    }
    print_response(&response)
}

/// Why `wait_for_ready` gave up.
#[derive(Debug)]
enum ReadyFailure {
    /// The daemon reported shutdown, or its socket vanished after it had
    /// answered: waiting longer cannot make it ready.
    Exiting(String),
    /// `deadline` passed without a ready response.
    TimedOut(String),
    ModeMismatch(String),
}

impl ReadyFailure {
    fn into_message(self) -> String {
        match self {
            Self::Exiting(message) | Self::TimedOut(message) | Self::ModeMismatch(message) => {
                message
            }
        }
    }
}

fn wait_for_ready(
    paths: &Paths,
    deadline: Instant,
    observation_mode: Option<ObservationSelection>,
) -> Result<ControlResponse, ReadyFailure> {
    let mut last_error = None;
    let mut answered = false;
    while Instant::now() < deadline {
        match send_command(
            &paths.control_socket,
            "ready",
            None,
            CONTROL_TIMEOUT.min(Duration::from_millis(500)),
        ) {
            Ok(response) if response.shutdown => {
                return Err(ReadyFailure::Exiting(
                    response
                        .error
                        .unwrap_or_else(|| "desktop pet is shutting down".into()),
                ))
            }
            Ok(response) => {
                answered = true;
                require_profile_identity(&response, paths).map_err(ReadyFailure::ModeMismatch)?;
                if observation_mode
                    .as_ref()
                    .is_some_and(|mode| response.observation_mode != mode.as_str())
                {
                    return Err(ReadyFailure::ModeMismatch(format!(
                        "desktop-pet is already running with host {}; requested {}",
                        response.observation_mode,
                        observation_mode
                            .as_ref()
                            .expect("requested selection")
                            .as_str()
                    )));
                }
                if response.ok && response.ready && response.control_ready {
                    return Ok(response);
                }
                last_error = response.error;
            }
            Err(error) => {
                if answered
                    && fs::symlink_metadata(&paths.control_socket)
                        .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
                {
                    return Err(ReadyFailure::Exiting(error));
                }
                last_error = Some(error);
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
    Err(ReadyFailure::TimedOut(last_error.unwrap_or_else(|| {
        "timed out waiting for desktop-pet readiness".into()
    })))
}

fn spawn_daemon(
    paths: &Paths,
    assets: Option<&Path>,
    assets_override: bool,
    startup_token: &str,
    automatic_start: bool,
    observation_mode: &ObservationSelection,
    extension_options: &[OsString],
) -> Result<u32, String> {
    let executable = bundle::executable()
        .map_err(|error| format!("cannot resolve desktop-pet executable path: {error}"))?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(LOG_MODE)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&paths.log_file)
        .map_err(|error| {
            format!(
                "cannot open daemon log {}: {error}",
                paths.log_file.display()
            )
        })?;
    let log_stderr = log
        .try_clone()
        .map_err(|error| format!("cannot duplicate daemon log: {error}"))?;
    fs::set_permissions(&paths.log_file, Permissions::from_mode(LOG_MODE)).map_err(|error| {
        format!(
            "cannot secure daemon log {}: {error}",
            paths.log_file.display()
        )
    })?;
    let mut command = Command::new(&executable);
    command
        .arg("daemon")
        .arg("--host")
        .arg(observation_mode.as_str())
        .arg("--config-dir")
        .arg(&paths.config_dir)
        .arg("--state-dir")
        .arg(&paths.state_dir)
        .env("DESKTOP_PET_CONFIG_DIR", &paths.config_dir)
        .env("DESKTOP_PET_STATE_DIR", &paths.state_dir)
        .env(OBSERVATION_SELECTION_ENV, observation_mode.as_str())
        .env("DESKTOP_PET_STARTUP_TOKEN", startup_token)
        .env(
            "DESKTOP_PET_AUTOMATIC_START",
            if automatic_start { "1" } else { "0" },
        )
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_stderr));
    command.args(extension_options);
    if let (Some(generation), Some(token)) = updater_start_authorization()? {
        command
            .env("DESKTOP_PET_UPDATER_GENERATION", generation.to_string())
            .env("DESKTOP_PET_UPDATER_TOKEN", token);
    } else {
        command
            .env_remove("DESKTOP_PET_UPDATER_GENERATION")
            .env_remove("DESKTOP_PET_UPDATER_TOKEN");
    }
    if assets_override {
        command
            .arg("--assets")
            .arg(assets.ok_or_else(|| "explicit renderer assets are unavailable".to_owned())?);
    }
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
        .spawn()
        .map(|child| child.id())
        .map_err(|error| format!("cannot start detached desktop-pet daemon: {error}"))
}

pub(crate) fn spawn_restart(
    paths: &Paths,
    observation_mode: &ObservationSelection,
) -> Result<(), String> {
    let options = DAEMON_RESTART_OPTIONS
        .get()
        .ok_or("running daemon restart options are unavailable")?;
    let executable = bundle::executable()
        .map_err(|error| format!("cannot resolve running executable: {error}"))?;
    let mut command = Command::new(executable);
    command
        .arg("restart")
        .arg("--config-dir")
        .arg(&paths.config_dir)
        .arg("--state-dir")
        .arg(&paths.state_dir)
        .arg("--host")
        .arg(observation_mode.as_str())
        .args(options)
        .env("DESKTOP_PET_CONFIG_DIR", &paths.config_dir)
        .env("DESKTOP_PET_STATE_DIR", &paths.state_dir)
        .env(OBSERVATION_SELECTION_ENV, observation_mode.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Retain integration context, not a completed startup handshake or updater
    // authorization: restart's stop advances the user-stop generation.
    command
        .env_remove("DESKTOP_PET_STARTUP_TOKEN")
        .env_remove("DESKTOP_PET_UPDATER_GENERATION")
        .env_remove("DESKTOP_PET_UPDATER_TOKEN");
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot start detached restart command: {error}"))?;
    // The command owns stop/start. Never wait for it on AppKit's main thread.
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn print_response(response: &ControlResponse) -> Result<(), String> {
    let value = serde_json::to_value(response)
        .map_err(|error| format!("cannot encode status response: {error}"))?;
    #[cfg(desktop_pet_legacy)]
    let value = desktop_pet_update_coordinator::legacy_entry::encode_control_response(value)?;
    let text = serde_json::to_string(&value)
        .map_err(|error| format!("cannot encode status response: {error}"))?;
    println!("{text}");
    Ok(())
}
fn decode_args<I>(arguments: I) -> Result<Vec<String>, String>
where
    I: IntoIterator<Item = OsString>,
{
    arguments
        .into_iter()
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| "command-line arguments must be valid UTF-8".to_owned())
        })
        .collect()
}

fn parse_cli<I>(arguments: I, composition: &Composition) -> Result<Cli, String>
where
    I: IntoIterator<Item = String>,
{
    let mut cli = Cli::default();
    let mut arguments = arguments.into_iter();
    let mut pack_tokens = Vec::new();
    let mut settings_tokens = Vec::new();
    let mut presentation_tokens = Vec::new();
    let mut automation_tokens = Vec::new();
    let mut update_tokens = Vec::new();
    while let Some(argument) = arguments.next() {
        if argument == "--yes" && cli.command == Some(CommandKind::Update) {
            if cli.approve_update {
                return Err("--yes was supplied more than once".into());
            }
            cli.approve_update = true;
            continue;
        }
        if cli.command == Some(CommandKind::Preferences) {
            let is_extension_value = automation_tokens.last().is_some_and(|last: &String| {
                composition.extensions.lifecycle.iter().any(|extension| {
                    extension
                        .preference_options()
                        .iter()
                        .any(|option| option.name == last && option.takes_value)
                })
            });
            let is_extension_option = composition.extensions.lifecycle.iter().any(|extension| {
                extension.preference_options().iter().any(|option| {
                    argument == option.name
                        || argument
                            .strip_prefix(option.name)
                            .is_some_and(|tail| tail.starts_with('='))
                })
            });
            if is_extension_value || is_extension_option {
                automation_tokens.push(argument);
                continue;
            }
        }
        if matches!(
            cli.command,
            Some(
                CommandKind::Preferences
                    | CommandKind::Sessions
                    | CommandKind::Dialogue
                    | CommandKind::Worktree
            )
        ) && automation_tokens.last().is_some_and(|last: &String| {
            matches!(
                last.as_str(),
                "--text"
                    | "--target"
                    | "--locale"
                    | "--slot"
                    | "--baseline"
                    | "--token"
                    | "--instance"
                    | "--operation-id"
                    | "--wait"
                    | "--source"
                    | "--generation"
                    | "--terminal"
                    | "--file"
                    | "--filter"
                    | "--limit"
                    | "--language"
                    | "--theme"
                    | "--surface"
                    | "--muted"
                    | "--border"
                    | "--accent"
                    | "--status-indicators"
                    | "--menu-bar"
                    | "--observation-host"
                    | "--expected-revision"
            )
        }) {
            automation_tokens.push(argument);
            continue;
        }
        if cli.command == Some(CommandKind::Installation) && argument == "--executable" {
            cli.extra_tokens.push(argument);
            cli.extra_tokens.push(
                arguments
                    .next()
                    .ok_or("--executable requires an absolute path")?,
            );
            continue;
        }
        if matches!(
            cli.command,
            Some(CommandKind::Profile | CommandKind::Installation | CommandKind::Extension)
        ) && argument == "--json"
        {
            cli.extra_tokens.push(argument);
            continue;
        }
        if parse_global_option(&argument, &mut arguments, &mut cli, composition)? {
            continue;
        }
        if matches!(
            cli.command,
            Some(CommandKind::Profile | CommandKind::Installation | CommandKind::Extension)
        ) {
            cli.extra_tokens.push(argument);
            continue;
        }
        if cli.command == Some(CommandKind::Pack) {
            pack_tokens.push(argument);
            continue;
        }
        if cli.command == Some(CommandKind::UpdateStatus) {
            update_tokens.push(argument);
            continue;
        }
        if cli.command == Some(CommandKind::Settings) {
            settings_tokens.push(argument);
            continue;
        }
        if cli.command == Some(CommandKind::Presentation) {
            presentation_tokens.push(argument);
            continue;
        }
        if matches!(
            cli.command,
            Some(
                CommandKind::Preferences
                    | CommandKind::Sessions
                    | CommandKind::Dialogue
                    | CommandKind::Worktree
            )
        ) {
            automation_tokens.push(argument);
            continue;
        }
        let command = if let Some(command) = CommandKind::parse(&argument) {
            command
        } else {
            if composition
                .extensions
                .cli
                .iter()
                .any(|e| e.commands().contains(&argument.as_str()))
            {
                cli.extension_command = Some(argument);
                CommandKind::Extension
            } else {
                return Err(format!("unknown command {argument:?}\n\n{USAGE}"));
            }
        };
        if cli.command.replace(command).is_some() {
            return Err("only one command may be specified".to_owned());
        }
    }
    if cli.command == Some(CommandKind::UpdateStatus) {
        if update_tokens.len() > 1 {
            return Err("update-status accepts at most one operation ID".into());
        }
        if let Some(id) = update_tokens.pop() {
            desktop_pet_update_coordinator::protocol::validate_operation_id(&id)?;
            cli.update_operation = Some(id);
        }
    }
    if cli.command == Some(CommandKind::Pack) {
        if pack_tokens.is_empty() {
            if cli.help || cli.version {
                return Ok(cli);
            }
            return Err(format!("pack requires an operation\n\n{USAGE}"));
        }
        cli.pack = Some(parse_pack_tokens(&pack_tokens)?);
    }
    if cli.command == Some(CommandKind::Settings) && !settings_tokens.is_empty() {
        let operation = parse_settings_tokens(&settings_tokens)?;
        cli.command = Some(match &operation {
            SettingsCommand::Get => CommandKind::SettingsGet,
            SettingsCommand::Set(_, _) => CommandKind::SettingsSet,
        });
        cli.settings = Some(operation);
    }
    if cli.command == Some(CommandKind::Presentation) {
        if presentation_tokens.is_empty() && (cli.help || cli.version) {
            return Ok(cli);
        }
        cli.presentation = Some(automation_cli::parse(&presentation_tokens)?);
    }
    if matches!(
        cli.command,
        Some(
            CommandKind::Preferences
                | CommandKind::Sessions
                | CommandKind::Dialogue
                | CommandKind::Worktree
        )
    ) {
        if automation_tokens.is_empty() && (cli.help || cli.version) {
            return Ok(cli);
        }
        cli.automation = Some(automation_cli::parse_family_with_extensions(
            cli.command.unwrap().as_str(),
            &automation_tokens,
            &composition.extensions.lifecycle,
        )?);
    }
    Ok(cli)
}

fn parse_settings_tokens(tokens: &[String]) -> Result<SettingsCommand, String> {
    match tokens {
        [operation] if operation == "get" => Ok(SettingsCommand::Get),
        [operation, key, value] if operation == "set" => {
            if key.is_empty()
                || key.len() > 128
                || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err("invalid lifecycle setting key".into());
            }
            let value = match value.as_str() {
                "on" => true,
                "off" => false,
                _ => return Err("settings value must be on or off".to_owned()),
            };
            Ok(SettingsCommand::Set(key.clone(), value))
        }
        _ => Err("expected settings get or settings set KEY on|off".to_owned()),
    }
}

fn parse_host_option(value: &str) -> Result<ObservationSelection, String> {
    value
        .parse()
        .map_err(|e| format!("invalid --host provider selection: {e}"))
}

fn parse_global_option<I>(
    argument: &str,
    arguments: &mut I,
    cli: &mut Cli,
    composition: &Composition,
) -> Result<bool, String>
where
    I: Iterator<Item = String>,
{
    let (name, inline) = argument
        .split_once('=')
        .map_or((argument, None), |(name, value)| (name, Some(value)));
    if let Some((extension, option)) = composition.extensions.cli.iter().find_map(|extension| {
        extension
            .options()
            .iter()
            .find(|option| option.name == name)
            .map(|option| (extension, option))
    }) {
        let value = if option.takes_value {
            Some(
                inline
                    .map(str::to_owned)
                    .or_else(|| arguments.next())
                    .ok_or_else(|| format!("{name} requires a value"))?,
            )
        } else {
            if inline.is_some() {
                return Err(format!("{name} does not accept a value"));
            }
            None
        };
        extension.configure_option(name, value.as_deref().map(std::ffi::OsStr::new))?;
        cli.extension_options.push(OsString::from(name));
        if let Some(value) = value {
            cli.extension_options.push(OsString::from(value));
        }
        return Ok(true);
    }
    match argument {
        "--help" | "-h" => {
            if cli.help {
                return Err("duplicate --help".to_owned());
            }
            cli.help = true;
            Ok(true)
        }
        "--version" | "-V" => {
            if cli.version {
                return Err("duplicate --version".to_owned());
            }
            cli.version = true;
            Ok(true)
        }
        "--host" => {
            let value = arguments
                .next()
                .ok_or_else(|| "--host requires auto or a provider ID".to_owned())?;
            if cli.host.replace(parse_host_option(&value)?).is_some() {
                return Err("duplicate --host".to_owned());
            }
            Ok(true)
        }
        value if value.starts_with("--host=") => {
            if cli
                .host
                .replace(parse_host_option(&value["--host=".len()..])?)
                .is_some()
            {
                return Err("duplicate --host".to_owned());
            }
            Ok(true)
        }
        "--assets" => {
            set_unique_path(
                &mut cli.assets,
                next_path(arguments, "--assets")?,
                "--assets",
            )?;
            Ok(true)
        }
        "--config-dir" => {
            set_unique_path(
                &mut cli.config_dir,
                next_path(arguments, "--config-dir")?,
                "--config-dir",
            )?;
            Ok(true)
        }
        "--state-dir" => {
            set_unique_path(
                &mut cli.state_dir,
                next_path(arguments, "--state-dir")?,
                "--state-dir",
            )?;
            Ok(true)
        }
        value if value.starts_with("--assets=") => {
            set_unique_path(
                &mut cli.assets,
                nonempty_path(value.trim_start_matches("--assets="), "--assets")?,
                "--assets",
            )?;
            Ok(true)
        }
        value if value.starts_with("--config-dir=") => {
            set_unique_path(
                &mut cli.config_dir,
                nonempty_path(value.trim_start_matches("--config-dir="), "--config-dir")?,
                "--config-dir",
            )?;
            Ok(true)
        }
        value if value.starts_with("--state-dir=") => {
            set_unique_path(
                &mut cli.state_dir,
                nonempty_path(value.trim_start_matches("--state-dir="), "--state-dir")?,
                "--state-dir",
            )?;
            Ok(true)
        }
        "--path"
        | "--output"
        | "--revision"
        | "--phase"
        | "--time-ms"
        | "--reaction"
        | "--reaction-age-ms"
        | "--pose"
        | "--hit-overlay"
        | "--visible"
        | "--passthrough"
        | "--alpha-passthrough"
        | "--bubble-visible"
        | "--bubble-placement"
        | "--scale"
        | "--expected-revision"
        | "--operation-id"
        | "--instance"
        | "--wait"
        | "--expected-generation"
        | "--language"
        | "--theme"
        | "--surface"
        | "--text"
        | "--muted"
        | "--border"
        | "--accent"
        | "--status-indicators"
        | "--menu-bar"
        | "--observation-host"
        | "--source"
        | "--generation"
        | "--terminal"
        | "--filter"
        | "--limit"
        | "--file"
        | "--stdin"
        | "--target"
        | "--locale"
        | "--slot"
        | "--baseline"
        | "--token"
        | "--no-wait" => Ok(false),
        value
            if value.starts_with("--path=")
                || value.starts_with("--output=")
                || value.starts_with("--revision=")
                || value.starts_with("--phase=")
                || value.starts_with("--time-ms=")
                || value.starts_with("--reaction=")
                || value.starts_with("--pose=")
                || value.starts_with("--reaction-age-ms=")
                || value.starts_with("--visible=")
                || value.starts_with("--passthrough=")
                || value.starts_with("--alpha-passthrough=")
                || value.starts_with("--bubble-visible=")
                || value.starts_with("--bubble-placement=")
                || value.starts_with("--scale=")
                || value.starts_with("--expected-revision=")
                || value.starts_with("--operation-id=")
                || value.starts_with("--instance=")
                || value.starts_with("--wait=")
                || [
                    "--expected-generation=",
                    "--language=",
                    "--theme=",
                    "--surface=",
                    "--text=",
                    "--muted=",
                    "--border=",
                    "--accent=",
                    "--status-indicators=",
                    "--menu-bar=",
                    "--observation-host=",
                    "--source=",
                    "--generation=",
                    "--terminal=",
                    "--filter=",
                    "--limit=",
                    "--file=",
                    "--target=",
                    "--locale=",
                    "--slot=",
                    "--baseline=",
                    "--token=",
                ]
                .iter()
                .any(|prefix| value.starts_with(prefix)) =>
        {
            Ok(false)
        }
        value if value.starts_with('-') => Err(format!("unknown option {value:?}\n\n{USAGE}")),
        _ => Ok(false),
    }
}

pub(crate) fn is_common_command(command: &str) -> bool {
    CommandKind::parse(command).is_some()
}

pub(crate) fn is_common_option(option: &str) -> bool {
    matches!(
        option,
        "--help"
            | "-h"
            | "--version"
            | "-V"
            | "--host"
            | "--assets"
            | "--config-dir"
            | "--state-dir"
    )
}

pub(crate) fn is_common_preference_option(option: &str) -> bool {
    matches!(
        option,
        "--expected-revision"
            | "--operation-id"
            | "--language"
            | "--theme"
            | "--surface"
            | "--muted"
            | "--border"
            | "--accent"
            | "--status-indicators"
            | "--menu-bar"
            | "--observation-host"
            | "--visible"
            | "--passthrough"
            | "--alpha-passthrough"
            | "--bubble-visible"
            | "--bubble-placement"
            | "--scale"
            | "--position"
    )
}

fn set_unique_path(
    destination: &mut Option<PathBuf>,
    value: PathBuf,
    option: &str,
) -> Result<(), String> {
    if destination.replace(value).is_some() {
        return Err(format!("duplicate {option}"));
    }
    Ok(())
}

fn parse_pack_tokens(tokens: &[String]) -> Result<PackCommand, String> {
    let operation = tokens
        .first()
        .ok_or_else(|| format!("pack requires an operation\n\n{USAGE}"))?;
    let mut index = 1usize;
    let mut path = None;
    let mut output = None;
    let mut revision = None;
    let mut operation_id = None;
    let mut expected_generation = None;
    let mut wait = None;
    let mut async_requested = false;
    let mut phase = None;
    let mut time_ms = None;
    let mut reaction = None;
    let mut reaction_age_ms = None;
    let mut pose = None;
    let mut hit_overlay = false;
    let mut positional = Vec::new();
    while index < tokens.len() {
        let token = &tokens[index];
        match token.as_str() {
            "--operation-id" | "--expected-generation" | "--wait" | "--no-wait" => {
                let name = token.as_str();
                if name == "--no-wait" {
                    if async_requested || wait.is_some() {
                        return Err("duplicate or incompatible wait controls".into());
                    }
                    async_requested = true;
                } else {
                    index += 1;
                    let value = tokens
                        .get(index)
                        .ok_or_else(|| format!("{name} requires a value"))?;
                    match name {
                        "--operation-id" => {
                            if operation_id
                                .replace(nonempty_operation_id(value)?)
                                .is_some()
                            {
                                return Err("duplicate --operation-id".into());
                            }
                        }
                        "--expected-generation" => {
                            if expected_generation
                                .replace(parse_revision(value)?)
                                .is_some()
                            {
                                return Err("duplicate --expected-generation".into());
                            }
                        }
                        _ => {
                            if wait.replace(automation_cli::parse_wait(value)?).is_some()
                                || async_requested
                            {
                                return Err("duplicate or incompatible wait controls".into());
                            }
                            async_requested = true;
                        }
                    }
                }
            }
            value
                if value.starts_with("--operation-id=")
                    || value.starts_with("--expected-generation=")
                    || value.starts_with("--wait=") =>
            {
                let (name, value) = value.split_once('=').expect("matched prefix");
                match name {
                    "--operation-id" => {
                        if operation_id
                            .replace(nonempty_operation_id(value)?)
                            .is_some()
                        {
                            return Err("duplicate --operation-id".into());
                        }
                    }
                    "--expected-generation" => {
                        if expected_generation
                            .replace(parse_revision(value)?)
                            .is_some()
                        {
                            return Err("duplicate --expected-generation".into());
                        }
                    }
                    _ => {
                        if wait.replace(automation_cli::parse_wait(value)?).is_some()
                            || async_requested
                        {
                            return Err("duplicate or incompatible wait controls".into());
                        }
                        async_requested = true;
                    }
                }
            }
            "--path" => {
                if path.is_some() {
                    return Err("duplicate --path".to_owned());
                }
                index += 1;
                let value = tokens
                    .get(index)
                    .ok_or_else(|| "--path requires a directory".to_owned())?;
                path = Some(absolute_input_path(value)?);
            }
            value if value.starts_with("--path=") => {
                if path.is_some() {
                    return Err("duplicate --path".to_owned());
                }
                path = Some(absolute_input_path(value.trim_start_matches("--path="))?);
            }
            "--output" => {
                if output.is_some() {
                    return Err("duplicate --output".to_owned());
                }
                index += 1;
                let value = tokens
                    .get(index)
                    .ok_or_else(|| "--output requires a path".to_owned())?;
                output = Some(absolute_output_path(value)?);
            }
            value if value.starts_with("--output=") => {
                if output.is_some() {
                    return Err("duplicate --output".to_owned());
                }
                output = Some(absolute_output_path(value.trim_start_matches("--output="))?);
            }
            "--revision" => {
                if revision.is_some() {
                    return Err("duplicate --revision".to_owned());
                }
                index += 1;
                let value = tokens
                    .get(index)
                    .ok_or_else(|| "--revision requires a number".to_owned())?;
                revision = Some(parse_revision(value)?);
            }
            value if value.starts_with("--revision=") => {
                if revision.is_some() {
                    return Err("duplicate --revision".to_owned());
                }
                revision = Some(parse_revision(value.trim_start_matches("--revision="))?);
            }
            "--phase" => {
                if phase.is_some() {
                    return Err("duplicate --phase".to_owned());
                }
                index += 1;
                let value = tokens
                    .get(index)
                    .ok_or_else(|| "--phase requires a value".to_owned())?;
                phase = Some(pack_authoring::parse_phase(value)?);
            }
            value if value.starts_with("--phase=") => {
                if phase.is_some() {
                    return Err("duplicate --phase".to_owned());
                }
                phase = Some(pack_authoring::parse_phase(
                    value.trim_start_matches("--phase="),
                )?);
            }
            "--time-ms" => {
                if time_ms.is_some() {
                    return Err("duplicate --time-ms".to_owned());
                }
                index += 1;
                let value = tokens
                    .get(index)
                    .ok_or_else(|| "--time-ms requires a value".to_owned())?;
                time_ms = Some(pack_authoring::parse_millis(value, "--time-ms")?);
            }
            value if value.starts_with("--time-ms=") => {
                if time_ms.is_some() {
                    return Err("duplicate --time-ms".to_owned());
                }
                time_ms = Some(pack_authoring::parse_millis(
                    value.trim_start_matches("--time-ms="),
                    "--time-ms",
                )?);
            }
            "--reaction" => {
                if reaction.is_some() {
                    return Err("duplicate --reaction".to_owned());
                }
                index += 1;
                let value = tokens
                    .get(index)
                    .ok_or_else(|| "--reaction requires a value".to_owned())?;
                reaction = Some(pack_authoring::parse_reaction(value)?);
            }
            value if value.starts_with("--reaction=") => {
                if reaction.is_some() {
                    return Err("duplicate --reaction".to_owned());
                }
                reaction = Some(pack_authoring::parse_reaction(
                    value.trim_start_matches("--reaction="),
                )?);
            }
            "--reaction-age-ms" => {
                if reaction_age_ms.is_some() {
                    return Err("duplicate --reaction-age-ms".to_owned());
                }
                index += 1;
                let value = tokens
                    .get(index)
                    .ok_or_else(|| "--reaction-age-ms requires a value".to_owned())?;
                reaction_age_ms = Some(pack_authoring::parse_millis(value, "--reaction-age-ms")?);
            }
            value if value.starts_with("--reaction-age-ms=") => {
                if reaction_age_ms.is_some() {
                    return Err("duplicate --reaction-age-ms".to_owned());
                }
                reaction_age_ms = Some(pack_authoring::parse_millis(
                    value.trim_start_matches("--reaction-age-ms="),
                    "--reaction-age-ms",
                )?);
            }
            "--pose" => {
                if pose.is_some() {
                    return Err("duplicate --pose".to_owned());
                }
                index += 1;
                let value = tokens
                    .get(index)
                    .ok_or_else(|| "--pose requires a value".to_owned())?;
                pose = Some(
                    pose::PoseKind::parse(value)
                        .ok_or_else(|| format!("unknown pose {value:?}"))?,
                );
            }
            value if value.starts_with("--pose=") => {
                if pose.is_some() {
                    return Err("duplicate --pose".to_owned());
                }
                let value = value.trim_start_matches("--pose=");
                pose = Some(
                    pose::PoseKind::parse(value)
                        .ok_or_else(|| format!("unknown pose {value:?}"))?,
                );
            }
            "--hit-overlay" => {
                if hit_overlay {
                    return Err("duplicate --hit-overlay".to_owned());
                }
                hit_overlay = true;
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown pack option {value:?}\n\n{USAGE}"));
            }
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }
    let has_preview_options = phase.is_some()
        || time_ms.is_some()
        || reaction.is_some()
        || reaction_age_ms.is_some()
        || pose.is_some()
        || hit_overlay;
    if operation.as_str() == "preview" && reaction.is_none() && reaction_age_ms.is_some() {
        return Err("--reaction-age-ms requires --reaction".to_owned());
    }
    let has_mutation_controls =
        operation_id.is_some() || expected_generation.is_some() || async_requested;
    let mutation = |action| PackCommand::Mutation {
        action,
        operation_id,
        expected_generation,
        wait: if async_requested {
            wait
        } else {
            Some(STARTUP_TIMEOUT)
        },
        async_requested,
    };
    let parsed = match operation.as_str() {
        "list"
            if positional.is_empty()
                && path.is_none()
                && output.is_none()
                && revision.is_none()
                && !has_preview_options =>
        {
            Ok(PackCommand::List)
        }
        "import"
            if positional.is_empty()
                && path.is_some()
                && output.is_none()
                && revision.is_none()
                && !has_preview_options =>
        {
            Ok(mutation(PackAction::Import {
                path: path.expect("checked above"),
            }))
        }
        "validate"
            if positional.is_empty()
                && path.is_some()
                && output.is_none()
                && revision.is_none()
                && !has_preview_options =>
        {
            Ok(PackCommand::Validate {
                path: path.expect("checked above"),
            })
        }
        "preview"
            if positional.is_empty()
                && path.is_some()
                && output.is_some()
                && revision.is_none() =>
        {
            Ok(PackCommand::Preview {
                path: path.expect("checked above"),
                output: output.expect("checked above"),
                phase: phase.unwrap_or(pack_authoring::PreviewPhase::Idle),
                time_ms: time_ms.unwrap_or(0),
                reaction,
                reaction_age_ms,
                pose,
                hit_overlay,
            })
        }
        "export"
            if positional.len() == 1
                && path.is_none()
                && output.is_some()
                && !has_preview_options =>
        {
            Ok(PackCommand::Export {
                id: select_id(&positional[0])?,
                revision,
                output: output.expect("checked above"),
            })
        }
        "select"
            if positional.len() == 1
                && path.is_none()
                && output.is_none()
                && revision.is_none()
                && !has_preview_options =>
        {
            Ok(mutation(PackAction::Select {
                id: select_id(&positional[0])?,
            }))
        }
        "update"
            if positional.len() == 1
                && path.is_some()
                && output.is_none()
                && revision.is_none()
                && !has_preview_options =>
        {
            Ok(mutation(PackAction::Update {
                id: nonempty_id(&positional[0])?,
                path: path.expect("checked above"),
            }))
        }
        "restore"
            if positional.len() == 1
                && path.is_none()
                && output.is_none()
                && revision.is_some()
                && !has_preview_options =>
        {
            Ok(mutation(PackAction::Restore {
                id: nonempty_id(&positional[0])?,
                revision: revision.expect("checked above"),
            }))
        }
        "remove"
            if positional.len() == 1
                && path.is_none()
                && output.is_none()
                && revision.is_none()
                && !has_preview_options =>
        {
            Ok(mutation(PackAction::Remove {
                id: nonempty_id(&positional[0])?,
            }))
        }
        "status"
            if positional.len() == 1
                && path.is_none()
                && output.is_none()
                && revision.is_none()
                && !has_preview_options =>
        {
            Ok(PackCommand::Status {
                operation_id: nonempty_operation_id(&positional[0])?,
            })
        }
        _ => Err(format!("invalid pack operation arguments\n\n{USAGE}")),
    };
    let command = parsed?;
    if !matches!(command, PackCommand::Mutation { .. }) && has_mutation_controls {
        return Err(
            "pack operation controls require an import/select/update/restore/remove mutation"
                .into(),
        );
    }
    Ok(command)
}
fn parse_revision(value: &str) -> Result<u64, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("invalid revision {value:?}"));
    }
    value
        .parse::<u64>()
        .map_err(|_| format!("revision {value:?} is out of range"))
}

fn nonempty_id(value: &str) -> Result<String, String> {
    validate_pack_id(value)?;
    Ok(value.to_owned())
}

fn select_id(value: &str) -> Result<String, String> {
    if value == "default" {
        Ok(value.to_owned())
    } else {
        nonempty_id(value)
    }
}

fn nonempty_operation_id(value: &str) -> Result<String, String> {
    if value.is_empty()
        || value.len() > 128
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err("pack operation ID is invalid".to_owned());
    }
    Ok(value.to_owned())
}
fn validate_source_path(path: &Path) -> Result<(), String> {
    let bytes = path.as_os_str().as_bytes();
    if bytes.contains(&0) {
        return Err("pack source path must not contain NUL bytes".to_owned());
    }
    if bytes.len() > MAX_SOURCE_PATH_BYTES {
        return Err(format!(
            "pack source path must be at most {MAX_SOURCE_PATH_BYTES} bytes"
        ));
    }
    Ok(())
}

fn absolute_input_path(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.starts_with('-') {
        return Err("--path requires a directory".to_owned());
    }
    let path = PathBuf::from(value);
    let path = if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| {
                format!("cannot resolve --path relative to current directory: {error}")
            })?
    };
    validate_source_path(&path)?;
    Ok(path)
}

fn absolute_output_path(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.starts_with('-') {
        return Err("--output requires a non-empty path".to_owned());
    }
    let path = PathBuf::from(value);
    let path = if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| {
                format!("cannot resolve --output relative to current directory: {error}")
            })?
    };
    validate_source_path(&path).map_err(|error| error.replace("source path", "output path"))?;
    Ok(path)
}

fn next_path<I>(arguments: &mut I, option: &str) -> Result<PathBuf, String>
where
    I: Iterator<Item = String>,
{
    let value = arguments
        .next()
        .ok_or_else(|| format!("{option} requires a path\n\n{USAGE}"))?;
    nonempty_path(&value, option)
}

fn nonempty_path(value: &str, option: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.starts_with('-') {
        Err(format!("{option} requires a non-empty path\n\n{USAGE}"))
    } else {
        Ok(PathBuf::from(value))
    }
}

pub(crate) fn resolve_assets(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return validate_assets_path(path);
    }
    let candidate = match bundle::contents_dir() {
        Some(contents) => contents.join("Resources/default"),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/rubelia-default"),
    };
    validate_assets_path(&candidate)
}

fn validate_assets_path(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("assets path {} is unavailable: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("assets path {} is not a directory", path.display()));
    }
    let manifest = path.join("manifest.json");
    let manifest_metadata = fs::symlink_metadata(&manifest).map_err(|error| {
        format!(
            "assets manifest {} is unavailable: {error}",
            manifest.display()
        )
    })?;
    if manifest_metadata.file_type().is_symlink() || !manifest_metadata.is_file() {
        return Err(format!(
            "assets manifest {} is not a regular file",
            manifest.display()
        ));
    }
    fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve assets path {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(not(desktop_pet_legacy))]
    fn update_status_observes_standalone_without_normal_plan_or_start_authority() {
        use desktop_pet_update_coordinator::{
            installation::{InstallationKind, InstallationRecord},
            native_runtime::ProfilePaths,
            standalone_install::{
                self, InstallRestart, InstallSource, InstallStatus, StandaloneInstallPhase,
                StandaloneInstallPlan, StandaloneInstallRecord, StandaloneInstallResult,
            },
            ExecutableIdentity,
        };
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config");
        let state = temp.path().join("state");
        for path in [&config, &state] {
            fs::create_dir(path).unwrap();
            fs::set_permissions(path, Permissions::from_mode(0o700)).unwrap();
        }
        let bundle = desktop_pet_update_coordinator::installation::standalone_bundle().unwrap();
        let executable = bundle.join("Contents/MacOS/desktop-pet");
        let candidate = temp.path().join("candidate/DesktopPet.app");
        let helper = temp.path().join("helper");
        fs::write(&helper, b"private fixture; never executed").unwrap();
        fs::set_permissions(&helper, Permissions::from_mode(0o700)).unwrap();
        let profile = ProfilePaths {
            config_dir: config,
            state_dir: state.clone(),
        };
        let plan = StandaloneInstallPlan {
            version: 2,
            purpose: standalone_install::StandaloneInstallPurpose::Upgrade,
            operation_id: "a".repeat(32),
            original: InstallationRecord {
                version: 1,
                executable: executable.clone(),
                kind: InstallationKind::Standalone,
                app_version: "0.4.13-beta.1".into(),
                protocol: 4,
                observation_mode: 1,
                integration_api: 1,
                origin: InstallOrigin::Standalone { bundle },
                image: ExecutableIdentity {
                    path: executable,
                    sha256: "1".repeat(64),
                },
            },
            candidate_bundle: candidate.clone(),
            candidate_image: ExecutableIdentity {
                path: candidate.join("Contents/MacOS/desktop-pet"),
                sha256: "2".repeat(64),
            },
            candidate_version: "0.4.13-beta.5".into(),
            helper_image: ExecutableIdentity {
                path: helper.clone(),
                sha256: "3".repeat(64),
            },
            profile: profile.clone(),
            source: InstallSource::Stopped { profile },
            original_stop_generation: 0,
            control_token: "4".repeat(64),
        };
        standalone_install::write_plan(&plan).unwrap();
        let record = StandaloneInstallRecord {
            version: 1,
            operation_id: plan.operation_id.clone(),
            phase: StandaloneInstallPhase::Replacing,
            result: StandaloneInstallResult {
                version: 1,
                operation_id: plan.operation_id.clone(),
                state_dir: state.clone(),
                status: InstallStatus::Running,
                installation: None,
                installed: false,
                applied: false,
                restart: InstallRestart::NotRequested,
                retained_bundle: None,
                error: None,
            },
        };
        standalone_install::write_record(&state, &record).unwrap();
        for id in [None, Some(plan.operation_id.as_str())] {
            let value = update_status_payload(&state, id).unwrap();
            assert_eq!(value["kind"], "standalone_install");
            assert_eq!(value["record"], serde_json::to_value(&record).unwrap());
            assert_eq!(value["staged_helper_signature_verified"], false);
            assert_eq!(
                value["helper_status_command"],
                serde_json::json!([
                    helper,
                    "install-status",
                    "--state-dir",
                    state,
                    "--operation-id",
                    plan.operation_id
                ])
            );
            assert_eq!(
                value["helper_recover_command"],
                serde_json::json!([
                    helper,
                    "install-recover",
                    "--state-dir",
                    state,
                    "--operation-id",
                    plan.operation_id
                ])
            );
            assert!(desktop_pet_update_coordinator::read_plan(&state, &plan.operation_id).is_err());
        }
    }

    #[test]
    #[cfg(not(desktop_pet_legacy))]
    fn startup_capture_preserves_selected_opaque_provider() {
        for mode in [
            ObservationSelection::Auto,
            ObservationSelection::Provider("provider-x".into()),
        ] {
            let captured = desktop_pet_update_coordinator::native_runtime::captured_environment(
                mode.clone(),
                |key| (key == "HOME").then(|| "/tmp/home".into()),
            )
            .expect("capture");
            assert_eq!(captured.get("HOME").map(String::as_str), Some("/tmp/home"));
            assert_eq!(
                captured.get(OBSERVATION_SELECTION_ENV).map(String::as_str),
                Some(mode.as_str())
            );
        }
    }

    #[test]
    fn update_status_rejects_invalid_operation_identity_and_extra_arguments() {
        let valid = "0123456789abcdef0123456789abcdef";
        assert!(parse_cli(
            ["update-status", valid].map(str::to_owned),
            &Composition::default()
        )
        .is_ok());
        assert!(parse_cli(["update-status".to_owned()], &Composition::default()).is_ok());
        for invalid in [
            "operation-1",
            "../other",
            "0123456789ABCDEF0123456789ABCDEF",
            "0123456789abcdef0123456789abcde",
            "0123456789abcdef0123456789abcdef0",
        ] {
            assert!(parse_cli(
                ["update-status", invalid].map(str::to_owned),
                &Composition::default()
            )
            .is_err());
        }
        assert!(parse_cli(
            ["update-status", valid, valid].map(str::to_owned),
            &Composition::default()
        )
        .is_err());
    }

    #[test]
    fn offline_update_status_never_presents_stale_installing_phase_as_live() {
        use desktop_pet_update_coordinator::protocol::{
            write_operation, ExecutionFence, OperationPhase, OperationRecord, UPDATER_PROTOCOL,
        };
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "herdr-update-status-{}-{nonce}",
            std::process::id()
        ));
        lifecycle::validate_directory(&root, true).unwrap();
        write_operation(
            &root,
            &OperationRecord {
                version: UPDATER_PROTOCOL,
                operation_id: "0123456789abcdef0123456789abcdef".into(),
                phase: OperationPhase::Installing,
                execution_fence: ExecutionFence::ManagerIntent,
                detail: "installer result not yet observed".into(),
                installed: false,
                applied: false,
                candidate: None,
            },
        )
        .unwrap();
        let status = update_status_payload(&root, None).unwrap();
        assert_eq!(status["last_durable_phase"], "installing");
        assert_eq!(status["record"]["installed"], false);
        assert_eq!(status["record"]["applied"], false);
        assert!(status["phase"].is_null());
        assert!(status["plan_error"].is_string());
        assert!(status["helper_status_command"].is_null());
        assert!(update_status_payload(&root, Some("fedcba9876543210fedcba9876543210")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn settings_cli_rejects_ambiguous_or_invalid_mutations() {
        let args = |values: &[&str]| {
            parse_cli(
                values.iter().map(|value| (*value).to_owned()),
                &Composition::default(),
            )
        };
        for tokens in [
            vec!["settings", "set", "auto_start", "true"],
            vec!["settings", "set", "auto_start"],
            vec!["settings", "set", "owner_setting", "off", "extra"],
            vec!["settings", "get", "auto_start"],
            vec!["settings", "set", "owner_setting", "OFF"],
        ] {
            assert!(args(&tokens).is_err(), "{tokens:?}");
        }
        let parsed = args(&["settings", "set", "auto_start", "off"]).unwrap();
        assert_eq!(
            parsed.settings,
            Some(SettingsCommand::Set("auto_start".into(), false))
        );
    }

    #[test]
    fn cli_accepts_every_command_and_explicit_paths() {
        let cli = parse_cli(
            [
                "restart".to_owned(),
                "--assets".to_owned(),
                "/tmp/assets".to_owned(),
                "--config-dir".to_owned(),
                "/tmp/config".to_owned(),
                "--state-dir".to_owned(),
                "/tmp/state".to_owned(),
            ],
            &Composition::default(),
        )
        .expect("cli");
        assert_eq!(cli.command, Some(CommandKind::Restart));
        assert_eq!(cli.config_dir, Some(PathBuf::from("/tmp/config")));
    }

    #[test]
    fn host_option_is_strict_and_unique() {
        let parse = |args: &[&str]| {
            parse_cli(
                args.iter().map(|arg| (*arg).to_owned()),
                &Composition::default(),
            )
        };
        assert_eq!(
            parse(&["start", "--host=tern"]).unwrap().host,
            Some(ObservationSelection::Provider("tern".into()))
        );
        assert_eq!(
            parse(&["--host", "auto", "ensure"]).unwrap().host,
            Some(ObservationSelection::Auto)
        );
        for args in [
            vec!["start", "--host=both"],
            vec!["start", "--host", "both"],
            vec!["start", "--host="],
            vec!["start", "--host"],
            vec!["start", "--host=tern", "--host=herdr"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn preference_host_tokens_reach_the_domain_parser_without_becoming_startup_flags() {
        for args in [
            vec!["preferences", "set", "--observation-host", "auto"],
            vec!["preferences", "set", "--observation-host=tern"],
            vec![
                "preferences",
                "set",
                "--host",
                "herdr",
                "--observation-host",
                "tern",
            ],
        ] {
            let cli = parse_cli(
                args.iter().map(|arg| (*arg).to_owned()),
                &Composition::default(),
            )
            .unwrap();
            assert!(cli.automation.is_some());
            assert_eq!(
                cli.host,
                args.contains(&"--host")
                    .then_some(ObservationSelection::Provider("herdr".into()))
            );
        }
        assert!(parse_cli(
            ["preferences", "set", "--observation-host", "both"].map(str::to_owned),
            &Composition::default()
        )
        .is_err());
    }

    #[test]
    fn startup_host_precedence_is_explicit_then_owned_environment_then_saved() {
        let resolve = |explicit, owned: Option<&str>, saved| {
            let selected = resolve_observation_override(
                explicit,
                owned.map(str::to_owned).ok_or(env::VarError::NotPresent),
            )?;
            select_observation_mode(selected, None, || Ok(saved))
        };
        let a = ObservationSelection::Provider("a".into());
        let b = ObservationSelection::Provider("b".into());
        assert_eq!(
            resolve(Some(a.clone()), Some("b"), ObservationSelection::Auto).unwrap(),
            a
        );
        assert_eq!(resolve(None, Some("b"), a.clone()).unwrap(), b);
        assert_eq!(resolve(None, None, a.clone()).unwrap(), a);
        assert_eq!(
            resolve(None, None, ObservationSelection::Auto).unwrap(),
            ObservationSelection::Auto
        );
        assert!(resolve(None, Some("both"), ObservationSelection::Auto).is_err());
        assert_eq!(resolve(Some(b.clone()), Some("both"), a).unwrap(), b);
    }

    #[test]
    fn restart_and_unqualified_queries_preserve_live_mode_not_saved_or_inherited_defaults() {
        for mode in [
            ObservationSelection::Provider("a".into()),
            ObservationSelection::Provider("b".into()),
            ObservationSelection::Auto,
        ] {
            let mut response = control_reply(true, false, false);
            response.observation_mode = mode.as_str().into();
            assert_eq!(
                select_observation_mode(None, Some(&response), || Err(
                    "must not load saved mode".into()
                ))
                .unwrap(),
                mode
            );
            assert_eq!(
                select_observation_mode(
                    Some(ObservationSelection::Provider("b".into())),
                    Some(&response),
                    || { Err("explicit selection must not load saved mode".into()) }
                )
                .unwrap(),
                ObservationSelection::Provider("b".into())
            );
        }
        assert_eq!(
            select_observation_mode(None, None, || Ok(ObservationSelection::Provider(
                "a".into()
            )))
            .unwrap(),
            ObservationSelection::Provider("a".into())
        );
    }

    #[test]
    fn startup_saves_canonical_host_and_preserves_legacy_profile_preferences() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "herdr-cli-host-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        lifecycle::validate_directory(&root, true).unwrap();
        let path = root.join("preferences.json");
        let old = serde_json::json!({
            "visible": false, "passthrough": true, "scale": 0.875, "position": null,
            "observation": {"local": false, "remote": true, "machines": []},
            "future_setting": {"kept": true}
        });
        fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
        fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
        let saved = preferences::Preferences::load(&root).unwrap();
        assert_eq!(
            saved.observation_host(),
            ObservationSelection::Provider("herdr".into())
        );
        assert_eq!(
            select_observation_mode(None, None, || Ok(saved.observation_host())).unwrap(),
            ObservationSelection::Provider("herdr".into())
        );
        persist_observation_mode(&root, ObservationSelection::Auto).unwrap();
        let disk: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(disk["observation"]["host"], "auto");
        for key in [
            "visible",
            "passthrough",
            "scale",
            "position",
            "future_setting",
        ] {
            assert_eq!(disk[key], old[key], "{key}");
        }
        assert_eq!(disk["observation"]["local"], false);
        assert_eq!(disk["observation"]["remote"], true);
        fs::write(&path, b"invalid existing preferences").unwrap();
        assert!(
            persist_observation_mode(&root, ObservationSelection::Provider("b".into())).is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), b"invalid existing preferences");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn grouped_dialogue_and_worktree_accept_raw_value_tokens_without_global_interception() {
        let target = r#"{"target":{"kind":"character","id":"default"},"reference":{"id":"default","revision":0},"generation":3}"#;
        let dialogue = parse_cli(
            [
                "dialogue",
                "set",
                "--target",
                target,
                "--locale",
                "ko",
                "--slot",
                "idle",
                "--text",
                "--help",
                "--no-wait",
            ]
            .map(str::to_owned),
            &Composition::default(),
        )
        .unwrap();
        assert!(matches!(dialogue.automation,
            Some(automation_cli::DomainCommand::DialogueSet {
                text:automation_cli::PromptText::Literal(value), wait:None,..
            }) if value=="--help"));
        let worktree = parse_cli(
            ["worktree", "remove", "--token=opaque", "--no-wait"].map(str::to_owned),
            &Composition::default(),
        )
        .unwrap();
        assert!(matches!(
            worktree.automation,
            Some(automation_cli::DomainCommand::WorktreeRemove { wait: None, .. })
        ));
        assert!(parse_cli(
            [
                "preferences",
                "status",
                "--instance",
                "daemon",
                "--operation-id",
                "op"
            ]
            .map(str::to_owned),
            &Composition::default()
        )
        .is_ok());
        assert!(parse_cli(
            ["orchestrate", "status"].map(str::to_owned),
            &Composition::default()
        )
        .is_err());
    }

    #[test]
    fn assets_never_use_legacy_omp_paths() {
        let source = resolve_assets(Some(Path::new("/tmp/not-a-real-assets-path")));
        assert!(source.is_err());
    }

    #[test]
    fn pack_cli_is_grouped_strict_and_normalizes_relative_paths() {
        let cli = parse_cli(
            [
                "pack".to_owned(),
                "import".to_owned(),
                "--path".to_owned(),
                "packs/example".to_owned(),
            ],
            &Composition::default(),
        )
        .expect("pack import");
        assert_eq!(cli.command, Some(CommandKind::Pack));
        match cli.pack {
            Some(PackCommand::Mutation {
                action: PackAction::Import { path },
                ..
            }) => assert!(path.is_absolute()),
            other => panic!("unexpected pack command: {other:?}"),
        }
        let cli = parse_cli(
            ["pack".to_owned(), "select".to_owned(), "default".to_owned()],
            &Composition::default(),
        )
        .expect("builtin select");
        assert!(matches!(
            cli.pack,
            Some(PackCommand::Mutation { action: PackAction::Select { id }, .. }) if id == "default"
        ));
        assert!(parse_cli(
            ["pack".to_owned(), "select".to_owned(), "@png".to_owned()],
            &Composition::default()
        )
        .is_err());
        assert!(parse_cli(
            ["pack".to_owned(), "remove".to_owned(), "default".to_owned(),],
            &Composition::default()
        )
        .is_err());
        assert!(parse_cli(
            [
                "pack".to_owned(),
                "select".to_owned(),
                "my-pack".to_owned(),
                "my-other-pack".to_owned(),
            ],
            &Composition::default()
        )
        .is_err());
        assert!(parse_cli(
            [
                "pack".to_owned(),
                "import".to_owned(),
                "--path".to_owned(),
                "one".to_owned(),
                "--path".to_owned(),
                "two".to_owned(),
            ],
            &Composition::default()
        )
        .is_err());
    }

    #[test]
    fn every_pack_mutation_uses_one_command_with_controls() {
        let cases = [
            (
                vec!["import", "--path", "/tmp/example"],
                PackAction::Import {
                    path: PathBuf::from("/tmp/example"),
                },
            ),
            (
                vec!["select", "default"],
                PackAction::Select {
                    id: "default".to_owned(),
                },
            ),
            (
                vec!["update", "example", "--path", "/tmp/example"],
                PackAction::Update {
                    id: "example".to_owned(),
                    path: PathBuf::from("/tmp/example"),
                },
            ),
            (
                vec!["restore", "example", "--revision", "2"],
                PackAction::Restore {
                    id: "example".to_owned(),
                    revision: 2,
                },
            ),
            (
                vec!["remove", "example"],
                PackAction::Remove {
                    id: "example".to_owned(),
                },
            ),
        ];
        for (args, expected_action) in cases {
            let tokens = args
                .into_iter()
                .chain([
                    "--operation-id=mutation_1",
                    "--expected-generation=42",
                    "--wait=2",
                ])
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let command = parse_pack_tokens(&tokens).expect("pack mutation");
            assert_eq!(
                command,
                PackCommand::Mutation {
                    action: expected_action,
                    operation_id: Some("mutation_1".to_owned()),
                    expected_generation: Some(42),
                    wait: Some(Duration::from_secs(2)),
                    async_requested: true,
                }
            );
        }
    }

    #[test]
    fn pack_mutation_controls_are_strict_and_preserve_offline_synchronous_default() {
        let parse = |tokens: &[&str]| {
            parse_cli(
                tokens.iter().map(|value| (*value).to_owned()),
                &Composition::default(),
            )
        };
        let cli = parse(&[
            "pack",
            "select",
            "sample",
            "--operation-id",
            "chosen_1",
            "--expected-generation",
            "42",
            "--no-wait",
        ])
        .expect("controlled mutation");
        assert!(
            matches!(cli.pack,Some(PackCommand::Mutation { operation_id:Some(id),expected_generation:Some(42),wait:None,async_requested:true,.. }) if id=="chosen_1")
        );
        let cli = parse(&["pack", "select", "sample"]).expect("synchronous mutation");
        assert!(matches!(
            cli.pack,
            Some(PackCommand::Mutation {
                wait: Some(_),
                async_requested: false,
                ..
            })
        ));
        for tokens in [
            vec!["pack", "list", "--no-wait"],
            vec!["pack", "status", "op", "--operation-id", "other"],
            vec!["pack", "validate", "--path", "/tmp/example", "--wait", "2"],
            vec![
                "pack",
                "preview",
                "--path",
                "/tmp/example",
                "--output",
                "/tmp/example.png",
                "--no-wait",
            ],
            vec![
                "pack",
                "export",
                "default",
                "--output",
                "/tmp/example",
                "--expected-generation",
                "2",
            ],
            vec!["pack", "select", "sample", "--wait", "0"],
            vec!["pack", "select", "sample", "--wait", "nan"],
            vec!["pack", "select", "sample", "--wait", "1", "--no-wait"],
            vec!["pack", "select", "sample", "--expected-generation", "-1"],
        ] {
            assert!(parse(&tokens).is_err(), "{tokens:?}");
        }
    }

    #[test]
    fn pack_status_exit_boundaries_distinguish_progress_and_failure() {
        let operation = |state: &str| PackOperation {
            operation_id: "operation".to_owned(),
            state: state.to_owned(),
            committed: state != "accepted",
            ui_applied: false,
            generation: None,
            error: Some("operation failed".to_owned()),
        };
        for state in ["accepted", "preparing", "applying"] {
            assert!(pack_status_result(&operation(state)).is_ok());
        }
        for state in [
            "failed",
            "canceled",
            "unknown",
            "durability_unknown",
            "committed_pending_apply",
        ] {
            assert!(pack_status_result(&operation(state)).is_err());
        }
        assert!(pack_status_result(&operation("completed")).is_ok());
    }

    #[test]
    fn live_pack_submission_replays_terminal_verdict_before_no_wait_ack() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;

        static NEXT_PACK_PEER: AtomicU64 = AtomicU64::new(0);
        for (state, success) in [
            ("completed", true),
            ("failed", false),
            ("canceled", false),
            ("durability_unknown", false),
            ("committed_pending_apply", false),
            ("unknown", false),
            ("accepted", true),
            ("preparing", true),
            ("applying", true),
        ] {
            let root = std::env::temp_dir().join(format!(
                "herdr-pack-replay-{}-{}",
                std::process::id(),
                NEXT_PACK_PEER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, Permissions::from_mode(0o700)).unwrap();
            let socket = root.join("control.sock");
            let listener = UnixListener::bind(&socket).unwrap();
            fs::set_permissions(&socket, Permissions::from_mode(0o600)).unwrap();
            let operation_id = format!("replayed_{state}");
            let reply = PackReplyEnvelope {
                version: control::PACK_PROTOCOL_VERSION,
                kind: "pack".into(),
                command: "submit".into(),
                ok: true,
                listing: None,
                operation: Some(PackOperation {
                    operation_id: operation_id.clone(),
                    state: state.into(),
                    committed: state != "accepted",
                    ui_applied: state == "completed",
                    generation: None,
                    error: (!success).then(|| "retained operation failed".into()),
                }),
                error: None,
            };
            let peer = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(&stream).read_line(&mut request).unwrap();
                let submitted: control::PackRequestEnvelope =
                    serde_json::from_str(&request).unwrap();
                assert_eq!(submitted.command, "submit");
                assert_eq!(
                    submitted.request.unwrap().operation_id,
                    reply.operation.as_ref().unwrap().operation_id
                );
                serde_json::to_writer(&mut stream, &reply).unwrap();
                stream.write_all(b"\n").unwrap();
                listener
            });
            let result = submit_live_pack(
                &socket,
                PackRequest {
                    operation_id,
                    expected_generation: None,
                    action: PackAction::Select {
                        id: "sample".into(),
                    },
                },
                None,
            );
            let listener = peer.join().unwrap();
            listener.set_nonblocking(true).unwrap();
            assert!(
                matches!(listener.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock),
                "terminal submission must not poll status"
            );
            assert_eq!(result.is_ok(), success, "{state}: {result:?}");
            drop(listener);
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn live_pack_wait_starts_after_ack_and_short_wait_does_not_poll() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;

        static NEXT_PACK_WAIT_PEER: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "herdr-pack-wait-{}-{}",
            std::process::id(),
            NEXT_PACK_WAIT_PEER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, Permissions::from_mode(0o700)).unwrap();
        let socket = root.join("control.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        fs::set_permissions(&socket, Permissions::from_mode(0o600)).unwrap();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(&stream).read_line(&mut request).unwrap();
            let request: control::PackRequestEnvelope = serde_json::from_str(&request).unwrap();
            let operation_id = request.request.unwrap().operation_id;
            thread::sleep(Duration::from_millis(110));
            let reply = PackReplyEnvelope {
                version: control::PACK_PROTOCOL_VERSION,
                kind: "pack".into(),
                command: "submit".into(),
                ok: true,
                listing: None,
                operation: Some(PackOperation {
                    operation_id,
                    state: "accepted".into(),
                    committed: false,
                    ui_applied: false,
                    generation: None,
                    error: None,
                }),
                error: None,
            };
            serde_json::to_writer(&mut stream, &reply).unwrap();
            stream.write_all(b"\n").unwrap();
            listener
        });
        let started = Instant::now();
        let result = submit_live_pack(
            &socket,
            PackRequest {
                operation_id: "delayed_ack".into(),
                expected_generation: None,
                action: PackAction::Select {
                    id: "sample".into(),
                },
            },
            Some(Duration::from_millis(30)),
        );
        let listener = peer.join().unwrap();
        listener.set_nonblocking(true).unwrap();
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock)
        );
        assert!(result.unwrap_err().contains("remains pending"));
        assert!(started.elapsed() >= Duration::from_millis(110));
        drop(listener);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pack_status_accepts_opaque_operation_ids_but_rejects_controls() {
        let cli = parse_cli(
            [
                "pack".to_owned(),
                "status".to_owned(),
                "operation.123".to_owned(),
            ],
            &Composition::default(),
        )
        .expect("pack status");
        assert!(matches!(
            cli.pack,
            Some(PackCommand::Status { operation_id }) if operation_id == "operation.123"
        ));
        assert!(parse_cli(
            [
                "pack".to_owned(),
                "status".to_owned(),
                "operation\n123".to_owned(),
            ],
            &Composition::default()
        )
        .is_err());
    }
    #[test]
    fn non_utf8_arguments_are_rejected_without_panicking() {
        use std::os::unix::ffi::OsStringExt;

        let error = decode_args([OsString::from_vec(vec![b'p', 0xff])])
            .expect_err("non-UTF-8 argv must be rejected");
        assert!(error.contains("UTF-8"));
    }

    #[test]
    fn pack_source_paths_reject_nul_and_oversize_values() {
        assert!(absolute_input_path("/tmp/pack\0source").is_err());
        assert!(absolute_input_path(&format!("/{}", "x".repeat(4096))).is_err());
    }

    fn control_reply(ok: bool, registration_accepted: bool, shutdown: bool) -> ControlResponse {
        ControlResponse {
            r#type: "status".to_owned(),
            version: 1,
            app_version: desktop_pet_update_coordinator::identity::VERSION.to_owned(),
            updater_protocol: identity::PROTOCOL,
            observation_capability: identity::OBSERVATION_CAPABILITY,
            integration_api: identity::INTEGRATION_API,
            update_ui_visible: false,
            update_operation_id: None,
            migration_operation: None,
            migration_gated: false,
            #[cfg(desktop_pet_legacy)]
            migration_source_environment: None,
            #[cfg(desktop_pet_legacy)]
            migration_source_host_plugin_config_dir: None,
            provider_fields: Default::default(),
            observation_mode: "herdr".into(),
            ok,
            command: "register".to_owned(),
            error: shutdown.then(|| "desktop pet is stopping".to_owned()),
            ready: false,
            running: true,
            phase: "starting".to_owned(),
            ui_ready: false,
            control_ready: true,
            registration_accepted,
            data_connected: false,
            connected_sources: 0,
            disconnected_sources: 0,
            host_connections: Default::default(),
            sessions: 0,
            working: 0,
            blocked: 0,
            done: 0,
            unknown: 0,
            visible: false,
            passthrough: false,
            alpha_passthrough: false,
            bubble_visible: true,
            bubble_placement: bubble::BubblePlacement::Above,
            scale: state::DEFAULT_SCALE,
            shutdown,
            auto_start: true,
            pid: std::process::id(),
            instance_id: String::new(),
            running_sha256: String::new(),
            assets_override: None,
            executable_path: bundle::executable()
                .expect("fixture executable")
                .display()
                .to_string(),
            config_dir: String::new(),
            state_dir: String::new(),
        }
    }

    /// Binds a private control socket that answers exactly one request with
    /// `reply`; `vanish` unlinks the socket before answering, as an exiting
    /// daemon does.
    fn serve_one_reply(
        mut reply: ControlResponse,
        vanish: bool,
        expected_endpoint: Option<PathBuf>,
    ) -> (Paths, thread::JoinHandle<()>) {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;

        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from("/tmp").join(format!(
            "herdr-ready-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, Permissions::from_mode(0o700)).unwrap();
        let control_socket = root.join("control.sock");
        let listener = UnixListener::bind(&control_socket).unwrap();
        fs::set_permissions(&control_socket, Permissions::from_mode(0o600)).unwrap();
        let paths = Paths {
            config_dir: root.join("config"),
            state_dir: root.join("state"),
            control_socket: control_socket.clone(),
            log_file: root.join("daemon.log"),
        };
        reply.config_dir = paths.config_dir.display().to_string();
        reply.state_dir = paths.state_dir.display().to_string();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(&stream).read_line(&mut request).unwrap();
            if let Some(expected) = expected_endpoint {
                let request: serde_json::Value = serde_json::from_str(&request).unwrap();
                assert_eq!(request["command"], "ready");
                assert_eq!(request["endpoint"], expected.display().to_string());
            }
            if reply.observation_mode == "tern" {
                let request: serde_json::Value = serde_json::from_str(&request).unwrap();
                assert_eq!(request["command"], "ready");
                assert!(request["endpoint"].is_null());
            }
            if vanish {
                fs::remove_file(&control_socket).unwrap();
            }
            let mut bytes = serde_json::to_vec(&reply).unwrap();
            bytes.push(b'\n');
            stream.write_all(&bytes).unwrap();
        });
        (paths, server)
    }

    #[test]
    fn readiness_wait_stops_once_the_daemon_reports_shutdown() {
        let (paths, server) = serve_one_reply(control_reply(false, false, true), false, None);
        let start = Instant::now();
        let result = wait_for_ready(
            &paths,
            Instant::now() + STARTUP_TIMEOUT,
            Some(ObservationSelection::Provider("herdr".into())),
        );
        server.join().unwrap();
        let _ = fs::remove_dir_all(paths.control_socket.parent().unwrap());
        assert!(
            matches!(result, Err(ReadyFailure::Exiting(_))),
            "{result:?}"
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn readiness_wait_stops_once_an_answering_daemon_unlinks_its_socket() {
        let (paths, server) = serve_one_reply(control_reply(true, true, false), true, None);
        let start = Instant::now();
        let result = wait_for_ready(
            &paths,
            Instant::now() + STARTUP_TIMEOUT,
            Some(ObservationSelection::Provider("herdr".into())),
        );
        server.join().unwrap();
        let _ = fs::remove_dir_all(paths.control_socket.parent().unwrap());
        assert!(
            matches!(result, Err(ReadyFailure::Exiting(_))),
            "{result:?}"
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn zero_provider_readiness_needs_only_the_actual_ui_and_control() {
        let mut reply = control_reply(true, false, false);
        reply.observation_mode = "auto".into();
        reply.command = "ready".into();
        reply.ready = true;
        reply.ui_ready = true;
        let (paths, server) = serve_one_reply(reply, false, None);
        let response = wait_for_ready(
            &paths,
            Instant::now() + Duration::from_secs(2),
            Some(ObservationSelection::Auto),
        )
        .unwrap();
        server.join().unwrap();
        let _ = fs::remove_dir_all(paths.control_socket.parent().unwrap());
        assert!(response.ready && response.control_ready);
        assert!(response.host_connections.is_empty() && !response.data_connected);
    }
    #[test]
    fn tern_readiness_neither_requires_nor_registers_a_herdr_endpoint() {
        let mut reply = control_reply(true, false, false);
        reply.observation_mode = "tern".to_owned();
        reply.ready = true;
        reply.control_ready = true;
        let (paths, server) = serve_one_reply(reply, false, None);
        let response = wait_for_ready(
            &paths,
            Instant::now() + Duration::from_secs(2),
            Some(ObservationSelection::Provider("tern".into())),
        )
        .unwrap();
        server.join().unwrap();
        let _ = fs::remove_dir_all(paths.control_socket.parent().unwrap());
        assert_eq!(response.observation_mode, "tern");
        assert!(!response.registration_accepted);
    }

    #[test]
    fn readiness_rejects_existing_daemon_with_different_host() {
        let (paths, server) = serve_one_reply(control_reply(true, true, false), false, None);
        let result = wait_for_ready(
            &paths,
            Instant::now() + Duration::from_secs(2),
            Some(ObservationSelection::Provider("tern".into())),
        );
        server.join().unwrap();
        let _ = fs::remove_dir_all(paths.control_socket.parent().unwrap());
        assert!(
            matches!(result, Err(ReadyFailure::ModeMismatch(_))),
            "{result:?}"
        );
    }

    #[test]
    fn settings_readiness_requires_only_ui_not_host_or_registration() {
        let mut reply = control_reply(true, false, false);
        reply.observation_mode = "tern".into();
        reply.ready = true;
        reply.ui_ready = true;
        let (paths, server) = serve_one_reply(reply, false, None);
        let result = wait_for_ready(&paths, Instant::now() + Duration::from_secs(2), None);
        server.join().unwrap();
        let _ = fs::remove_dir_all(paths.control_socket.parent().unwrap());
        let response = result.unwrap();
        assert_eq!(response.observation_mode, "tern");
        assert!(response.ready && !response.registration_accepted);
    }

    #[test]
    fn settings_reuses_daemon_without_assets_profile_or_host_registration() {
        use std::io::{BufRead, BufReader};
        use std::os::unix::net::UnixListener;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from("/tmp").join(format!(
            "herdr-live-settings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        lifecycle::validate_directory(&root, true).unwrap();
        let paths = Paths {
            config_dir: root.join("absent-config"),
            state_dir: root.join("absent-state"),
            control_socket: root.join("control.sock"),
            log_file: root.join("daemon.log"),
        };
        let listener = UnixListener::bind(&paths.control_socket).unwrap();
        fs::set_permissions(&paths.control_socket, Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            for expected_command in ["ping", "settings"] {
                let deadline = Instant::now() + Duration::from_secs(2);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error)
                            if error.kind() == io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            thread::sleep(POLL_INTERVAL)
                        }
                        Err(error) => panic!("missing {expected_command} request: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(
                        deadline
                            .checked_duration_since(Instant::now())
                            .expect("settings fixture request deadline expired"),
                    ))
                    .unwrap();
                let mut request = String::new();
                BufReader::new(&stream).read_line(&mut request).unwrap();
                let request: serde_json::Value = serde_json::from_str(&request).unwrap();
                assert_eq!(request["command"], expected_command);
                assert!(request["endpoint"].is_null());
                let mut reply = control_reply(true, false, false);
                reply.observation_mode = "tern".into();
                reply.command = expected_command.into();
                reply.ready = true;
                reply.ui_ready = true;
                let mut bytes = serde_json::to_vec(&reply).unwrap();
                bytes.push(b'\n');
                stream.write_all(&bytes).unwrap();
            }
        });
        let result = ensure(
            paths.clone(),
            Some(root.join("absent-assets")),
            true,
            CommandKind::Settings,
            ObservationSelection::Auto,
            Arc::new(Composition::default()),
            Vec::new(),
        );
        server.join().unwrap();
        assert!(result.is_ok(), "{result:?}");
        assert!(!paths.config_dir.exists());
        assert!(!paths.state_dir.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn control_queries_the_running_host_and_tern_never_registers_herdr() {
        use std::io::{BufRead, BufReader};
        use std::os::unix::net::UnixListener;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from("/tmp").join(format!(
            "herdr-live-mode-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        lifecycle::validate_directory(&root, true).unwrap();
        let paths = Paths {
            config_dir: root.join("config"),
            state_dir: root.join("state"),
            control_socket: root.join("control.sock"),
            log_file: root.join("daemon.log"),
        };
        let listener = UnixListener::bind(&paths.control_socket).unwrap();
        fs::set_permissions(&paths.control_socket, Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            for expected_command in ["hide"] {
                let deadline = Instant::now() + Duration::from_secs(2);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error)
                            if error.kind() == io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            thread::sleep(POLL_INTERVAL)
                        }
                        Err(error) => panic!("missing {expected_command} request: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(
                        deadline
                            .checked_duration_since(Instant::now())
                            .expect("host control fixture request deadline expired"),
                    ))
                    .unwrap();
                let mut request = String::new();
                BufReader::new(&stream).read_line(&mut request).unwrap();
                let request: serde_json::Value = serde_json::from_str(&request).unwrap();
                assert_eq!(request["command"], expected_command);
                assert!(request["endpoint"].is_null());
                let mut reply = control_reply(true, false, false);
                reply.observation_mode = "tern".into();
                reply.command = expected_command.into();
                let mut bytes = serde_json::to_vec(&reply).unwrap();
                bytes.push(b'\n');
                stream.write_all(&bytes).unwrap();
            }
        });
        let result = control_action(CommandKind::Hide, paths);
        server.join().unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(result.is_ok(), "{result:?}");
    }
}
