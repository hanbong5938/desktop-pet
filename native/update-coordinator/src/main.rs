mod helper_ui;
mod install_runner;
mod runner;

use desktop_pet_update_coordinator::{identity, installation, protocol, read_plan, OperationPhase};
use std::path::PathBuf;

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let command = args.next().ok_or("usage: desktop-pet-update-coordinator update-capabilities | distribution-identity | discover [--executable ABS] --json | install/install-coordinated/install-relocate/install-relocate-manual/install-manual-recovery --request ABS | install-status/install-recover --state-dir ABS --operation-id ID | migrate-legacy --plan ABS [--resume-retirement] | run/status/recover --state-dir PATH --operation-id ID [--start]")?;
    if command == "update-capabilities" || command == "distribution-identity" {
        if args.next().is_some() {
            return Err("identity commands accept no arguments".into());
        }
        let result = if command == "update-capabilities" {
            identity::capabilities()
        } else {
            identity::json()
        };
        println!(
            "{}",
            serde_json::to_string(&result).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if matches!(
        command.to_str(),
        Some(
            "install-coordinated"
                | "install-relocate"
                | "install-relocate-manual"
                | "install-manual-recovery"
                | "install-status"
                | "install-recover"
                | "install-run"
        )
    ) {
        let code = install_runner::dispatch(
            command.to_str().ok_or("invalid install command")?,
            args.collect(),
        )?;
        std::process::exit(code);
    }
    if command == "discover" {
        if identity::LEGACY {
            return Err("legacy distribution cannot discover neutral installations".into());
        }
        let mut executable = None;
        let mut json = false;
        while let Some(arg) = args.next() {
            if arg == "--executable" && executable.is_none() {
                executable = Some(PathBuf::from(
                    args.next()
                        .ok_or("--executable requires an absolute path")?,
                ));
            } else if arg == "--json" && !json {
                json = true;
            } else {
                return Err(format!(
                    "unexpected discovery argument: {}",
                    arg.to_string_lossy()
                ));
            }
        }
        if !json {
            return Err("discover requires --json".into());
        }
        let record = installation::discover(executable.as_deref())?;
        println!(
            "{}",
            serde_json::to_string(&record).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if command == "install" {
        if identity::LEGACY {
            return Err("legacy distribution cannot install neutral applications".into());
        }
        if args.next().as_deref() != Some(std::ffi::OsStr::new("--request")) {
            return Err("install requires --request ABS".into());
        }
        let request = PathBuf::from(args.next().ok_or("--request requires an absolute path")?);
        if args.next().is_some() {
            return Err("install accepts only --request ABS".into());
        }
        let record = installation::install_request(&request)?;
        println!(
            "{}",
            serde_json::to_string(&record).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if command == "migrate-legacy" {
        #[cfg(desktop_pet_legacy)]
        return Err("migration requires a verified neutral protocol-4 helper".into());
        #[cfg(not(desktop_pet_legacy))]
        {
            if args.next().as_deref() != Some(std::ffi::OsStr::new("--plan")) {
                return Err("migrate-legacy requires --plan ABS".into());
            }
            let plan = PathBuf::from(args.next().ok_or("--plan requires an absolute path")?);
            if !plan.is_absolute() {
                return Err("migration plan must be absolute".into());
            }
            let action = args.next();
            if args.next().is_some() {
                return Err(
                    "migration actions are mutually exclusive and accept no extra arguments".into(),
                );
            }
            use desktop_pet_update_coordinator::migration::{self, MigrationPresentation};
            // The helper owns this system-library-only native window independently
            // of both replaceable applications, on the process main thread.
            let approved = migration::read_plan(&plan)?;
            let resume = action.as_deref() == Some(std::ffi::OsStr::new("--resume-retirement"));
            if action.is_some() && !resume {
                return Err("unexpected migration argument".into());
            }
            if migration::delegated_approval(&approved)? {
                // The original update approval owns this exact automatic cutover.
                // No source/helper visibility or second consent is fabricated.
                let record = migration::run_delegated(&plan, resume)?;
                use std::io::Write;
                let mut output = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
                output.push(b'\n');
                let mut stdout = std::io::stdout().lock();
                if let Err(error) = stdout.write_all(&output).and_then(|_| stdout.flush()) {
                    if error.kind() != std::io::ErrorKind::BrokenPipe {
                        return Err(format!("cannot write migration result: {error}"));
                    }
                }
                return Ok(());
            }
            let panel = helper_ui::Panel::for_migration("en", &approved)?;
            // Retained operations without delegated approval retain their original UI.
            if !panel.confirm_migration(&approved, resume)? {
                migration::write_final_consent_cancelled(&approved, &plan, resume)?;
                panel.close();
                return Ok(());
            }
            let result = match action.as_deref() {
                None => migration::run_with_presentation(&plan, &panel),
                Some(arg) if arg == "--resume-retirement" => {
                    migration::resume_retirement_with_presentation(&plan, &panel)
                }
                Some(_) => unreachable!(),
            };
            match &result {
                Ok(record) => {
                    MigrationPresentation::show(&panel, record.phase, record.error.as_deref())
                }
                Err(error) => {
                    if let Ok(record) = migration::read_record(&approved) {
                        MigrationPresentation::show(&panel, record.phase, Some(error));
                    } else {
                        panel.show(
                            helper_ui::Phase::Attention(helper_ui::Attention::NotStarted),
                            Some(error),
                        );
                    }
                }
            }
            if result
                .as_ref()
                .is_ok_and(|record| record.phase == migration::MigrationPhase::Complete)
            {
                panel.close();
            } else {
                // A nonterminal/failed operation remains inspectable in the real
                // helper window. Closing hides it; it never replays a manager.
                while panel.visible() {
                    panel.pump();
                    std::thread::sleep(std::time::Duration::from_millis(120));
                }
            }
            let record = result?;
            // The source consent reader can exit after the authenticated ACK
            // and source Stop. Its closed output pipe is not a migration failure.
            // This exception applies only to the final journal result, never ACK.
            use std::io::Write;
            let mut output = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
            output.push(b'\n');
            let mut stdout = std::io::stdout().lock();
            if let Err(error) = stdout.write_all(&output).and_then(|_| stdout.flush()) {
                if error.kind() != std::io::ErrorKind::BrokenPipe {
                    return Err(format!("cannot write migration result: {error}"));
                }
            }
            return Ok(());
        }
    }
    let mut state_dir = None;
    let mut operation_id = None;
    let mut start = false;
    while let Some(arg) = args.next() {
        if arg == "--state-dir" && state_dir.is_none() {
            state_dir = Some(PathBuf::from(
                args.next().ok_or("--state-dir requires a path")?,
            ));
        } else if arg == "--operation-id" && operation_id.is_none() {
            operation_id = Some(
                args.next()
                    .ok_or("--operation-id requires a value")?
                    .into_string()
                    .map_err(|_| "invalid operation ID encoding")?,
            );
        } else if arg == "--start" && !start {
            start = true;
        } else {
            return Err(format!(
                "unexpected updater argument: {}",
                arg.to_string_lossy()
            ));
        }
    }
    let state_dir = state_dir.ok_or("missing --state-dir")?;
    let operation_id = operation_id.ok_or("missing --operation-id")?;
    protocol::validate_operation_id(&operation_id)?;
    if start && command != "recover" {
        return Err("--start is only valid with recover".into());
    }
    let plan = read_plan(&state_dir, &operation_id)?;
    match command.to_str().ok_or("invalid updater command encoding")? {
        "run" => runner::run(plan),
        "status" => {
            let status = runner::inspect(&plan)?;
            println!(
                "{}",
                serde_json::to_string(&status).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "recover" => {
            let result = runner::recover(plan, start)?;
            println!(
                "{}",
                serde_json::to_string(&result).map_err(|e| e.to_string())?
            );
            if result.phase == OperationPhase::Completed {
                Ok(())
            } else {
                Err(format!("update was not applied: {}", result.detail))
            }
        }
        _ => Err("unknown updater command".into()),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{}: {error}", identity::HELPER);
        std::process::exit(1);
    }
}
