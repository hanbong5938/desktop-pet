use crate::herdr::Endpoint;
use crate::herdr_protocol;
use crate::sources::{remote_source, MachineInfo, MachineStatus, SourceCatalog};
use crate::state::AppState;
use std::collections::{HashMap, HashSet};
use std::env;
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const INTERVAL: Duration = Duration::from_secs(5);
const DEADLINE: Duration = Duration::from_secs(8);
const TICK: Duration = Duration::from_millis(50);
const CATALOG_BYTES: usize = 256 * 1024;
const STDERR_BYTES: usize = 1024;
/// Discarded stderr bytes per drain pass once the diagnostic prefix is full,
/// so an endless writer cannot starve the cancellation/deadline checks.
const STDERR_DISCARD_PER_PASS: usize = 64 * 1024;
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

type Shared = Arc<Mutex<AppState>>;

struct Job {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

/// Owns independent discovery and per-machine polling threads. Each command is
/// placed in its own process group so cancellation also stops SSH descendants.
pub(crate) struct RemoteWatchers {
    stop: Arc<AtomicBool>,
    coordinator: Option<JoinHandle<()>>,
}

impl RemoteWatchers {
    pub(crate) fn new(shared: Shared) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let cancellation = Arc::clone(&stop);
        let coordinator = thread::spawn(move || run_coordinator(shared, cancellation));
        Self {
            stop,
            coordinator: Some(coordinator),
        }
    }

    pub(crate) fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.coordinator.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for RemoteWatchers {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_coordinator(shared: Shared, stop: Arc<AtomicBool>) {
    let mut jobs: HashMap<String, Job> = HashMap::new();
    let mut selected = HashSet::<String>::new();
    let mut scratch = Vec::<String>::new();
    let mut retired = Vec::<JoinHandle<()>>::new();
    let mut catalog_job: Option<Job> = None;
    let mut next_catalog = Instant::now();
    let cli = resolve_cli();
    let mut observation_epoch = None;
    while !stop.load(Ordering::Acquire) {
        let (epoch, enabled) = {
            let state = shared.lock().unwrap_or_else(|p| p.into_inner());
            (
                state.observation_epoch(),
                state.observation_mode().supports_herdr(),
            )
        };
        if observation_epoch != Some(epoch) {
            observation_epoch = Some(epoch);
            for (_, job) in jobs.drain() {
                job.stop.store(true, Ordering::Release);
                retired.push(job.handle);
            }
            if let Some(job) = catalog_job.take() {
                job.stop.store(true, Ordering::Release);
                retired.push(job.handle);
            }
            selected.clear();
            next_catalog = Instant::now();
        }
        if catalog_job
            .as_ref()
            .is_some_and(|job| job.handle.is_finished())
        {
            let _ = catalog_job
                .take()
                .expect("finished discovery")
                .handle
                .join();
        }
        if enabled && Instant::now() >= next_catalog && catalog_job.is_none() {
            let catalog_shared = Arc::clone(&shared);
            let catalog_global = Arc::clone(&stop);
            let catalog_stop = Arc::new(AtomicBool::new(false));
            let catalog_cancellation = Arc::clone(&catalog_stop);
            let catalog_cli = cli.clone();
            let handle = thread::spawn(move || {
                discover(
                    &catalog_shared,
                    &catalog_global,
                    &catalog_cancellation,
                    catalog_cli.as_ref(),
                    epoch,
                )
            });
            catalog_job = Some(Job {
                stop: catalog_stop,
                handle,
            });
            next_catalog = Instant::now() + INTERVAL;
        }
        {
            let state = shared.lock().unwrap_or_else(|p| p.into_inner());
            let prefs = state.observation_preferences();
            let mut desired = state
                .observation_catalog()
                .machines
                .iter()
                .filter(|machine| {
                    state.observation_mode().supports_herdr()
                        && machine.enabled
                        && prefs.remote
                        && prefs.machines.contains(&machine.id)
                });
            let desired_count = desired.clone().count();
            if desired_count != selected.len()
                || desired.any(|machine| !selected.contains(&machine.id))
            {
                selected.clear();
                selected.extend(
                    state
                        .observation_catalog()
                        .machines
                        .iter()
                        .filter(|machine| {
                            state.observation_mode().supports_herdr()
                                && machine.enabled
                                && prefs.remote
                                && prefs.machines.contains(&machine.id)
                        })
                        .take(64)
                        .map(|machine| machine.id.clone()),
                );
            }
        }
        scratch.clear();
        scratch.extend(jobs.keys().filter(|id| !selected.contains(*id)).cloned());
        for id in scratch.drain(..) {
            if let Some(job) = jobs.remove(&id) {
                job.stop.store(true, Ordering::Release);
                retired.push(job.handle);
            }
            let epoch = shared
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .observation_epoch();
            set_status(&shared, &id, epoch, MachineStatus::NotSelected, None);
        }
        scratch.extend(
            jobs.iter()
                .filter(|(_, job)| job.handle.is_finished())
                .map(|(id, _)| id.clone()),
        );
        for id in scratch.drain(..) {
            if let Some(job) = jobs.remove(&id) {
                let _ = job.handle.join();
            }
        }
        for id in &selected {
            if jobs.contains_key(id) {
                continue;
            }
            let job_stop = Arc::new(AtomicBool::new(false));
            let machine_shared = Arc::clone(&shared);
            let machine_global = Arc::clone(&stop);
            let machine_stop = Arc::clone(&job_stop);
            let machine_cli = cli.clone();
            let machine_id = id.clone();
            let handle = thread::spawn(move || {
                poll_machine(
                    machine_shared,
                    machine_global,
                    machine_stop,
                    machine_cli,
                    machine_id,
                    epoch,
                )
            });
            jobs.insert(
                id.clone(),
                Job {
                    stop: job_stop,
                    handle,
                },
            );
        }
        let mut index = 0;
        while index < retired.len() {
            if retired[index].is_finished() {
                let _ = retired.swap_remove(index).join();
            } else {
                index += 1;
            }
        }
        thread::sleep(TICK);
    }
    for job in jobs.values() {
        job.stop.store(true, Ordering::Release);
    }
    for (_, job) in jobs {
        let _ = job.handle.join();
    }
    if let Some(job) = catalog_job {
        job.stop.store(true, Ordering::Release);
        let _ = job.handle.join();
    }
    for handle in retired {
        let _ = handle.join();
    }
}

fn discover(
    shared: &Shared,
    global: &AtomicBool,
    stop: &AtomicBool,
    cli: Option<&PathBuf>,
    epoch: u64,
) {
    let result = cli
        .ok_or_else(|| "Herdr CLI not found; install Herdr or add its binary to PATH".to_owned())
        .and_then(|cli| {
            run_cli_until(cli, &["machine", "list", "--json"], CATALOG_BYTES, || {
                cancelled(global, stop) || !observation_current(shared, epoch, None)
            })
        })
        .and_then(|bytes| parse_catalog(&bytes));
    if cancelled(global, stop) {
        return;
    }
    let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
    if !state.observation_mode().supports_herdr() || state.observation_epoch() != epoch {
        return;
    }
    update_catalog(&mut state, result);
    drop(state);
    crate::ui::wake();
}

fn update_catalog(state: &mut AppState, result: Result<Vec<MachineInfo>, String>) {
    let old = state.observation_catalog().clone();
    let catalog = match result {
        Ok(mut machines) => {
            for machine in &mut machines {
                if machine.enabled {
                    if let Some(previous) = old
                        .machines
                        .iter()
                        .find(|previous| previous.id == machine.id && previous.enabled)
                    {
                        machine.status = previous.status;
                        machine.error = previous.error.clone();
                    }
                }
            }
            SourceCatalog {
                initialized: true,
                machines,
                error: None,
            }
        }
        Err(error) => SourceCatalog {
            initialized: true,
            machines: old.machines,
            error: Some(error),
        },
    };
    state.set_observation_catalog(catalog);
}

fn parse_catalog(bytes: &[u8]) -> Result<Vec<MachineInfo>, String> {
    let rows: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| "Herdr machine list returned invalid JSON".to_owned())?;
    let rows = rows
        .as_array()
        .ok_or("Herdr machine list must return an array")?;
    if rows.len() > 64 {
        return Err("Herdr machine list exceeds 64 profiles".to_owned());
    }
    let mut ids = HashSet::new();
    rows.iter()
        .map(|row| {
            let object = row
                .as_object()
                .ok_or("Herdr machine list has invalid profile")?;
            let string = |key| {
                object
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .filter(|s| !s.is_empty() && s.len() <= 4096)
                    .ok_or_else(|| format!("Herdr machine profile has invalid {key}"))
            };
            let id = string("id")?.to_owned();
            if id.contains('\0') || !ids.insert(id.clone()) {
                return Err("Herdr machine list has invalid or duplicate IDs".to_owned());
            }
            let label = string("label")?.to_owned();
            let session = string("session")?.to_owned();
            // `target` and `selected` are part of the v0.9.2 CLI contract, but
            // neither SSH targets nor Herdr's active selection belong in app state.
            let _ = string("target")?;
            let _ = object
                .get("selected")
                .and_then(serde_json::Value::as_bool)
                .ok_or("Herdr machine profile has invalid selected")?;
            let enabled = object
                .get("enabled")
                .and_then(serde_json::Value::as_bool)
                .ok_or("Herdr machine profile has invalid enabled")?;
            Ok(MachineInfo {
                id,
                label,
                remote_session: session,
                enabled,
                status: MachineStatus::NotSelected,
                error: None,
            })
        })
        .collect()
}

fn poll_machine(
    shared: Shared,
    global: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    cli: Option<PathBuf>,
    id: String,
    epoch: u64,
) {
    let source = remote_source(&id);
    {
        let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
        if state.observation_epoch() != epoch || !state.observation_mode().supports_herdr() {
            return;
        }
        if let Some(generation) = state.source_generation(&source) {
            state.mark_source_offline(&source, generation);
        }
    }
    // A poller belongs to one observation epoch. The coordinator creates a
    // replacement rather than rebinding late work to a newly enabled host.
    let mut endpoint: Option<Endpoint> = None;
    let mut baseline = true;
    while !cancelled(&global, &stop) && observation_current(&shared, epoch, Some(&id)) {
        if baseline {
            set_status(&shared, &id, epoch, MachineStatus::Connecting, None);
        }
        let observed_at = Instant::now();
        let result = cli
            .as_ref()
            .ok_or_else(|| {
                "Herdr CLI not found; install Herdr or add its binary to PATH".to_owned()
            })
            .and_then(|cli| {
                run_cli_until(
                    cli,
                    &["--machine", &id, "api", "snapshot"],
                    herdr_protocol::MAX_FRAME_BYTES,
                    || cancelled(&global, &stop) || !observation_current(&shared, epoch, Some(&id)),
                )
            })
            .and_then(|bytes| {
                let line = std::str::from_utf8(&bytes)
                    .map_err(|_| "Herdr snapshot was not UTF-8".to_owned())?;
                let response = herdr_protocol::parse_response_line(line, "cli:api:snapshot")
                    .map_err(|e| e.to_string())?;
                herdr_protocol::parse_snapshot(response).map_err(|e| e.to_string())
            });
        if cancelled(&global, &stop) || !observation_current(&shared, epoch, Some(&id)) {
            break;
        }
        match result {
            Ok(snapshot) => {
                if endpoint.is_none() {
                    let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
                    if state.observation_epoch() != epoch
                        || !state.observation_mode().supports_herdr()
                    {
                        continue;
                    }
                    let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
                    let previous = state
                        .source_generation(&source)
                        .unwrap_or(0)
                        .max(state.retired_source_generation(&source).unwrap_or(0));
                    let Some(generation) = previous.checked_add(1).map(|next| generation.max(next))
                    else {
                        break;
                    };
                    if state.begin_source(source.clone(), generation) {
                        endpoint = Some(Endpoint::remote(source.clone(), generation, epoch));
                        drop(state);
                    } else {
                        drop(state);
                        set_status(
                            &shared,
                            &id,
                            epoch,
                            MachineStatus::Offline,
                            Some(
                                "Remote source limit reached or machine is unavailable".to_owned(),
                            ),
                        );
                    }
                }
                if let Some(endpoint) = &mut endpoint {
                    endpoint.remote_snapshot(&shared, snapshot, observed_at, baseline);
                    baseline = false;
                    set_status(&shared, &id, epoch, MachineStatus::Online, None);
                }
            }
            Err(error) => {
                if let Some(endpoint) = &mut endpoint {
                    endpoint.remote_offline(&shared);
                }
                endpoint = None;
                baseline = true;
                set_status(&shared, &id, epoch, MachineStatus::Offline, Some(error));
            }
        }
        let until = Instant::now() + INTERVAL;
        while Instant::now() < until
            && !cancelled(&global, &stop)
            && observation_current(&shared, epoch, Some(&id))
        {
            thread::sleep(TICK);
        }
    }
}

fn observation_current(shared: &Shared, epoch: u64, machine: Option<&str>) -> bool {
    let state = shared.lock().unwrap_or_else(|p| p.into_inner());
    state.observation_mode().supports_herdr()
        && state.observation_epoch() == epoch
        && machine.is_none_or(|id| {
            let prefs = state.observation_preferences();
            prefs.remote
                && prefs.machines.iter().any(|selected| selected == id)
                && state
                    .observation_catalog()
                    .machines
                    .iter()
                    .any(|machine| machine.id == id && machine.enabled)
        })
}

fn set_status(shared: &Shared, id: &str, epoch: u64, status: MachineStatus, error: Option<String>) {
    let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
    if state.observation_epoch() != epoch || !state.observation_mode().supports_herdr() {
        return;
    }
    if status != MachineStatus::NotSelected
        && (!state.observation_preferences().remote
            || !state
                .observation_preferences()
                .machines
                .iter()
                .any(|selected| selected == id))
    {
        return;
    }
    let error = error.map(|error| {
        error
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect::<String>()
    });
    let Some(previous) = state
        .observation_catalog()
        .machines
        .iter()
        .find(|machine| machine.id == id && machine.enabled)
    else {
        return;
    };
    if previous.status == status && previous.error == error {
        return;
    }
    let mut catalog = state.observation_catalog().clone();
    let machine = catalog
        .machines
        .iter_mut()
        .find(|machine| machine.id == id)
        .expect("machine matched before catalog clone");
    machine.status = status;
    machine.error = error;
    state.set_observation_catalog(catalog);
    drop(state);
    crate::ui::wake();
}

fn cancelled(global: &AtomicBool, stop: &AtomicBool) -> bool {
    global.load(Ordering::Acquire) || stop.load(Ordering::Acquire)
}

fn resolve_cli() -> Option<PathBuf> {
    let path = env::var_os("PATH").unwrap_or_default();
    let mut locations: Vec<PathBuf> = env::split_paths(&path)
        .filter(|path| path.is_absolute())
        .collect();
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        locations.push(home.join(".local/bin"));
        locations.push(home.join(".cargo/bin"));
    }
    locations.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
    ]);
    locations
        .into_iter()
        .map(|dir| dir.join("herdr"))
        .find(|path| {
            std::fs::metadata(path).is_ok_and(|meta| {
                meta.is_file()
                    && (std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0)
            })
        })
}

#[cfg(test)]
fn run_cli(
    cli: &PathBuf,
    args: &[&str],
    stop: &AtomicBool,
    limit: usize,
) -> Result<Vec<u8>, String> {
    run_cli_until(cli, args, limit, || stop.load(Ordering::Acquire))
}

fn run_cli_until(
    cli: &PathBuf,
    args: &[&str],
    limit: usize,
    mut should_cancel: impl FnMut() -> bool,
) -> Result<Vec<u8>, String> {
    if should_cancel() {
        return Err("Herdr command cancelled".to_owned());
    }
    let mut command = Command::new(cli);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Cannot launch Herdr CLI: {e}"))?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    set_nonblocking(&stdout).map_err(|e| {
        terminate(&mut child);
        e.to_string()
    })?;
    set_nonblocking(&stderr).map_err(|e| {
        terminate(&mut child);
        e.to_string()
    })?;
    let mut output = Vec::new();
    let mut errors = Vec::new();
    let start = Instant::now();
    let mut out_open = true;
    let mut err_open = true;
    loop {
        let cancellation = should_cancel();
        if cancellation || start.elapsed() >= DEADLINE {
            terminate(&mut child);
            return Err(if cancellation {
                "Herdr command cancelled"
            } else {
                "Herdr command timed out"
            }
            .to_owned());
        }
        match drain(
            &mut stdout,
            &mut output,
            limit,
            Overflow::Fail,
            &mut out_open,
        ) {
            Ok(()) => {}
            Err(e) => {
                terminate(&mut child);
                return Err(format!("Herdr stdout: {e}"));
            }
        }
        match drain(
            &mut stderr,
            &mut errors,
            STDERR_BYTES,
            Overflow::Truncate,
            &mut err_open,
        ) {
            Ok(()) => {}
            Err(e) => {
                terminate(&mut child);
                return Err(format!("Herdr stderr: {e}"));
            }
        }
        match child.try_wait() {
            Ok(Some(status)) if !status.success() => {
                terminate(&mut child);
                let detail = String::from_utf8_lossy(&errors);
                let detail = detail
                    .trim()
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(200)
                    .collect::<String>();
                return Err(if detail.is_empty() {
                    format!("Herdr command exited with {status}")
                } else {
                    format!("Herdr command failed: {detail}")
                });
            }
            Ok(Some(_)) if !out_open && !err_open => return Ok(output),
            Ok(_) => {}
            Err(e) => {
                terminate(&mut child);
                return Err(format!("Herdr command wait failed: {e}"));
            }
        }
        thread::sleep(TICK);
    }
}

fn set_nonblocking(file: &impl AsRawFd) -> io::Result<()> {
    let fd = file.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// What `drain` does with bytes past `limit`.
#[derive(Clone, Copy)]
enum Overflow {
    /// The payload is unusable once truncated.
    Fail,
    /// Keep the prefix and keep reading (so the child never blocks on a full
    /// pipe), discarding at most `STDERR_DISCARD_PER_PASS` bytes per call.
    Truncate,
}

fn drain(
    file: &mut impl Read,
    output: &mut Vec<u8>,
    limit: usize,
    overflow: Overflow,
    open: &mut bool,
) -> io::Result<()> {
    let mut chunk = [0; 4096];
    let mut discarded = 0;
    while *open {
        match file.read(&mut chunk) {
            Ok(0) => *open = false,
            Ok(n) if output.len() + n > limit => match overflow {
                Overflow::Fail => return Err(io::Error::other("response exceeded size limit")),
                Overflow::Truncate => {
                    let keep = limit.saturating_sub(output.len());
                    output.extend_from_slice(&chunk[..keep]);
                    discarded += n - keep;
                    if discarded >= STDERR_DISCARD_PER_PASS {
                        break;
                    }
                }
            },
            Ok(n) => output.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn terminate(child: &mut Child) {
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_errors_do_not_accept_partial_or_duplicate_profiles() {
        assert!(parse_catalog(br#"[{"id":"a","label":"A","target":"ssh","session":"default","enabled":true,"selected":false},{"id":"a","label":"B","target":"ssh","session":"default","enabled":true,"selected":false}]"#).is_err());
        assert!(parse_catalog(br#"[{"id":"a","label":"A","target":"ssh","session":"default","enabled":"yes","selected":false}]"#).is_err());
        assert!(parse_catalog(br#"{"machines":[]}"#).is_err());
        assert_eq!(parse_catalog(br#"[{"id":"a","label":"Renamed","target":"ssh","session":"work","enabled":false,"selected":true}]"#).unwrap()[0].remote_session, "work");
    }

    #[test]
    fn discovery_distinguishes_pending_empty_failure_and_recovery_without_losing_profiles() {
        let mut state = AppState::new();
        assert!(!state.observation_catalog().initialized);
        assert!(state.observation_catalog().machines.is_empty());
        let mut initially_failed = AppState::new();
        update_catalog(&mut initially_failed, Err("CLI unavailable".to_owned()));
        assert!(initially_failed.observation_catalog().initialized);
        assert!(initially_failed.observation_catalog().machines.is_empty());
        assert_eq!(
            initially_failed.observation_catalog().error.as_deref(),
            Some("CLI unavailable")
        );

        update_catalog(&mut state, Ok(Vec::new()));
        assert!(state.observation_catalog().initialized);
        assert!(state.observation_catalog().machines.is_empty());
        assert!(state.observation_catalog().error.is_none());

        let profiles = parse_catalog(
            br#"[{"id":"east","label":"East","target":"ssh","session":"work","enabled":true,"selected":false}]"#,
        )
        .unwrap();
        update_catalog(&mut state, Ok(profiles));
        let mut observed = state.observation_catalog().clone();
        observed.machines[0].status = MachineStatus::Online;
        observed.machines[0].error = Some("Previous connection error".to_owned());
        state.set_observation_catalog(observed);
        let previous = state.observation_catalog().machines.clone();

        update_catalog(&mut state, Err("machine list failed".to_owned()));
        let failed = state.observation_catalog();
        assert!(failed.initialized);
        assert_eq!(failed.error.as_deref(), Some("machine list failed"));
        assert_eq!(failed.machines, previous);

        let recovered = parse_catalog(
            br#"[{"id":"east","label":"Renamed","target":"ssh","session":"work","enabled":true,"selected":false}]"#,
        )
        .unwrap();
        update_catalog(&mut state, Ok(recovered));
        let catalog = state.observation_catalog();
        assert!(catalog.initialized);
        assert!(catalog.error.is_none());
        assert_eq!(catalog.machines[0].label, "Renamed");
        assert_eq!(catalog.machines[0].status, MachineStatus::Online);
        assert_eq!(
            catalog.machines[0].error.as_deref(),
            Some("Previous connection error")
        );
    }

    #[test]
    fn tern_dormancy_cancels_remote_processes_and_reenable_uses_a_new_generation() {
        use crate::sources::ObservationPreferences;
        use herdr_update_coordinator::ObservationMode;
        use std::os::unix::fs::PermissionsExt;

        fn start_poll(shared: &Shared, global: &Arc<AtomicBool>, cli: PathBuf) -> JoinHandle<()> {
            let epoch = shared
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .observation_epoch();
            let shared = Arc::clone(shared);
            let global = Arc::clone(global);
            thread::spawn(move || {
                poll_machine(
                    shared,
                    global,
                    Arc::new(AtomicBool::new(false)),
                    Some(cli),
                    "m".into(),
                    epoch,
                )
            })
        }

        fn await_connection(shared: &Shared) {
            let deadline = Instant::now() + DEADLINE + Duration::from_secs(2);
            while shared
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .host_connections()
                .herdr
                == 0
            {
                assert!(Instant::now() < deadline, "remote snapshot never connected");
                thread::sleep(TICK);
            }
        }

        let root = env::temp_dir().join(format!(
            "herdr-remote-host-{}-{}",
            std::process::id(),
            NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&root).unwrap();
        let counter = root.join("runs");
        let fast = root.join("fast");
        let slow = root.join("slow");
        let profile = r#"[{"id":"m","label":"Machine","target":"ssh","session":"default","enabled":true,"selected":false}]"#;
        let snapshot = r#"{"id":"cli:api:snapshot","result":{"type":"session_snapshot","snapshot":{"panes":[],"agents":[]}}}"#;
        std::fs::write(&fast, format!(
            "#!/bin/sh\necho run >> '{}'\nif [ \"$1\" = machine ]; then printf '%s\\n' '{}'; else printf '%s\\n' '{}'; fi\n",
            counter.display(), profile, snapshot,
        )).unwrap();
        std::fs::write(
            &slow,
            format!(
                "#!/bin/sh\necho run >> '{}'\nsleep 30\nprintf '%s\\n' '{}'\n",
                counter.display(),
                snapshot,
            ),
        )
        .unwrap();
        for script in [&fast, &slow] {
            std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut state = AppState::new();
        update_catalog(&mut state, parse_catalog(profile.as_bytes()));
        state.apply_observation_preferences(ObservationPreferences {
            host: ObservationMode::Tern,
            local: true,
            remote: true,
            machines: vec!["m".into()],
        });
        let shared = Arc::new(Mutex::new(state));
        let global = Arc::new(AtomicBool::new(false));
        let stop = AtomicBool::new(false);
        let dormant_epoch = shared
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .observation_epoch();
        discover(&shared, &global, &stop, Some(&fast), dormant_epoch);
        start_poll(&shared, &global, fast.clone()).join().unwrap();
        assert!(!counter.exists());
        assert!(shared
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .observation_catalog()
            .error
            .is_none());

        shared
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .set_observation_mode(ObservationMode::Both);
        let active_epoch = shared
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .observation_epoch();
        discover(&shared, &global, &stop, Some(&fast), active_epoch);
        let first = start_poll(&shared, &global, fast.clone());
        await_connection(&shared);
        let source = remote_source("m");
        let first_generation = shared
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .source_generation(&source)
            .unwrap();
        shared
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .set_observation_mode(ObservationMode::Herdr);
        first.join().unwrap();
        assert_eq!(
            shared
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .host_connections()
                .herdr,
            1
        );
        let dormant_runs = std::fs::read_to_string(&counter).unwrap();
        let inflight = start_poll(&shared, &global, slow);
        let deadline = Instant::now() + DEADLINE;
        while std::fs::read_to_string(&counter).unwrap() == dormant_runs {
            assert!(Instant::now() < deadline, "remote command never started");
            thread::sleep(TICK);
        }
        assert_eq!(
            shared
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .host_connections()
                .herdr,
            0
        );
        let cancelled_at = Instant::now();
        shared
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .set_observation_mode(ObservationMode::Tern);
        inflight.join().unwrap();
        assert!(cancelled_at.elapsed() < Duration::from_secs(2));
        assert_eq!(
            shared
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .source_generation(&source),
            None
        );
        let dormant_runs = std::fs::read_to_string(&counter).unwrap();
        discover(&shared, &global, &stop, Some(&fast), active_epoch);
        start_poll(&shared, &global, fast.clone()).join().unwrap();
        assert_eq!(std::fs::read_to_string(&counter).unwrap(), dormant_runs);
        shared
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .set_observation_mode(ObservationMode::Herdr);
        let resumed = start_poll(&shared, &global, fast);
        await_connection(&shared);
        assert!(
            shared
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .source_generation(&source)
                .unwrap()
                > first_generation
        );
        global.store(true, Ordering::Release);
        resumed.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cli_output_is_bounded_and_cancelled_process_group_is_reaped() {
        let stop = Arc::new(AtomicBool::new(false));
        assert!(run_cli(
            &PathBuf::from("/bin/sh"),
            &["-c", "while :; do printf x; done"],
            &stop,
            1024
        )
        .unwrap_err()
        .contains("size limit"));
        let signal = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            run_cli(
                &PathBuf::from("/bin/sh"),
                &["-c", "sleep 30 & wait"],
                &signal,
                1024,
            )
        });
        thread::sleep(Duration::from_millis(100));
        let start = Instant::now();
        stop.store(true, Ordering::Release);
        assert!(handle.join().unwrap().unwrap_err().contains("cancelled"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn stderr_overflow_is_truncated_without_failing_or_starving_cancellation() {
        let stop = Arc::new(AtomicBool::new(false));
        let chatty = "head -c 100000 /dev/zero | tr '\\0' e >&2";
        let succeeds = format!("{chatty}; printf ok");
        assert_eq!(
            run_cli(
                &PathBuf::from("/bin/sh"),
                &["-c", succeeds.as_str()],
                &stop,
                1024
            )
            .unwrap(),
            b"ok"
        );
        let fails = format!("{chatty}; exit 3");
        let failure = run_cli(
            &PathBuf::from("/bin/sh"),
            &["-c", fails.as_str()],
            &stop,
            1024,
        )
        .unwrap_err();
        assert!(failure.contains("Herdr command failed:"), "{failure}");
        assert!(!failure.contains("size limit"), "{failure}");

        let signal = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            run_cli(
                &PathBuf::from("/bin/sh"),
                &["-c", "while :; do printf eeeeeeee >&2; done"],
                &signal,
                1024,
            )
        });
        thread::sleep(Duration::from_millis(100));
        let start = Instant::now();
        stop.store(true, Ordering::Release);
        assert!(handle.join().unwrap().unwrap_err().contains("cancelled"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn rejected_remote_source_waits_for_the_poll_interval() {
        use crate::sources::ObservationPreferences;
        use std::os::unix::fs::PermissionsExt;

        let root = env::temp_dir().join(format!(
            "herdr-remote-reject-{}-{}",
            std::process::id(),
            NEXT_GENERATION.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let counter = root.join("runs");
        let script = root.join("herdr");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho run >> '{}'\nprintf '%s\\n' '{}'\n",
                counter.display(),
                r#"{"id":"cli:api:snapshot","result":{"type":"session_snapshot","snapshot":{"panes":[],"agents":[]}}}"#
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();

        let mut state = AppState::new();
        state.set_observation_catalog(SourceCatalog {
            machines: vec![MachineInfo {
                id: "m".to_owned(),
                label: "Machine".to_owned(),
                remote_session: "default".to_owned(),
                enabled: true,
                status: MachineStatus::Online,
                error: None,
            }],
            error: None,
            initialized: true,
        });
        state.apply_observation_preferences(ObservationPreferences {
            host: herdr_update_coordinator::ObservationMode::Herdr,
            local: true,
            remote: true,
            machines: vec!["m".to_owned()],
        });
        for index in 0..64 {
            assert!(state.begin_source(format!("/tmp/local-{index}.sock"), 1));
        }
        let shared = Arc::new(Mutex::new(state));
        let global = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let shared = Arc::clone(&shared);
            let global = Arc::clone(&global);
            let stop = Arc::clone(&stop);
            let script = script.clone();
            let epoch = shared
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .observation_epoch();
            thread::spawn(move || {
                poll_machine(shared, global, stop, Some(script), "m".to_owned(), epoch)
            })
        };
        // Await rejection before observing the retry interval; process startup
        // can exceed one second when the full suite runs concurrently.
        let deadline = Instant::now() + DEADLINE + Duration::from_secs(2);
        while Instant::now() < deadline
            && shared.lock().is_ok_and(|state| {
                state.observation_catalog().machines[0].status != MachineStatus::Offline
            })
        {
            thread::sleep(TICK);
        }
        thread::sleep(Duration::from_secs(1));
        stop.store(true, Ordering::Release);
        handle.join().unwrap();

        let runs = std::fs::read_to_string(&counter).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(runs.lines().count(), 1);
        let state = shared.lock().unwrap();
        let machine = &state.observation_catalog().machines[0];
        assert_eq!(machine.status, MachineStatus::Offline);
        assert!(machine.error.is_some());
    }
}
