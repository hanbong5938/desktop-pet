use crate::character_store::PackStore;
use crate::lifecycle::{self, Paths};
use crate::preferences::Preferences;
use desktop_pet_contracts::profile_store::{atomic_write, validate_directory, LIFECYCLE_FILE};
use desktop_pet_contracts::{LifecycleExtension, ProfileStore};
use serde_json::{json, Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_SETTINGS_BYTES: u64 = 4 * 1024 * 1024;
const RECOVERY_DIR: &str = "settings-recovery";
const RESULT_FILE: &str = "last-result.json";

/// Captured before Stop. Only Stop's startup provenance removal is allowed to
/// differ at commit; no newer user preference or character selection is lost.
pub(crate) struct ExpectedSettings {
    preferences: Option<Vec<u8>>,
    lifecycle: Map<String, Value>,
    registry: Option<Vec<u8>>,
}

pub(crate) fn capture(paths: &Paths) -> Result<ExpectedSettings, String> {
    Ok(ExpectedSettings {
        preferences: read_private(
            &paths.config_dir.join("preferences.json"),
            MAX_SETTINGS_BYTES,
        )?,
        lifecycle: user_lifecycle(
            read_private(&paths.config_dir.join(LIFECYCLE_FILE), MAX_SETTINGS_BYTES)?.as_deref(),
        )?,
        registry: read_private(
            &paths.config_dir.join("characters/registry.json"),
            MAX_SETTINGS_BYTES,
        )?,
    })
}

fn user_lifecycle(bytes: Option<&[u8]>) -> Result<Map<String, Value>, String> {
    let mut object: Map<String, Value> = match bytes {
        Some(bytes) => {
            serde_json::from_slice(bytes).map_err(|e| format!("invalid lifecycle settings: {e}"))?
        }
        None => Map::new(),
    };
    object.remove("startup");
    Ok(object)
}

fn read_private(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, String> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > limit
    {
        return Err(format!(
            "{} is not a bounded private owned settings file",
            path.display()
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!("{} exceeds settings size limit", path.display()));
    }
    Ok(Some(bytes))
}

struct ResetFile {
    path: PathBuf,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

/// Offline only: the caller owns the exact stopped profile's lifecycle lease.
/// Adapter preparation runs before the writer; pure candidate construction and
/// every backup, CAS and publication run under both existing writer locks.
pub(crate) fn reset(
    paths: &Paths,
    profile: &ProfileStore,
    extensions: &[Arc<dyn LifecycleExtension>],
    expected: &ExpectedSettings,
    admit: impl Fn() -> Result<(), String>,
) -> Result<PathBuf, String> {
    if profile.config_dir() != fs::canonicalize(&paths.config_dir).map_err(|e| e.to_string())?
        || profile.lifecycle_lock().state_dir() != paths.state_dir
    {
        return Err("settings reset profile differs from captured selection".into());
    }
    admit()?;
    let saved = Preferences::load_committed(&paths.config_dir)?;
    let preferences = saved.settings_reset_object(&paths.config_dir, profile, extensions)?;
    Preferences::validate_committed_object(&preferences)?;
    let preference_bytes = serde_json::to_vec_pretty(&preferences).map_err(|e| e.to_string())?;
    profile.with_writer_lock(|| {
        admit()?;
        let current = capture(paths)?;
        if current.preferences != expected.preferences
            || current.lifecycle != expected.lifecycle
            || current.registry != expected.registry
        {
            return Err("settings changed after reset confirmation; nothing was reset".into());
        }
        let lifecycle_path = paths.config_dir.join(LIFECYCLE_FILE);
        let lifecycle_before = read_private(&lifecycle_path, MAX_SETTINGS_BYTES)?;
        let lifecycle_object = match lifecycle_before.as_deref() {
            Some(bytes) => serde_json::from_slice(bytes)
                .map_err(|e| format!("invalid lifecycle settings: {e}"))?,
            None => Map::new(),
        };
        let lifecycle_after =
            lifecycle::settings_reset_object(&paths.config_dir, lifecycle_object, extensions)?;
        let lifecycle_bytes =
            serde_json::to_vec_pretty(&lifecycle_after).map_err(|e| e.to_string())?;
        let store = PackStore::new(paths.config_dir.clone(), None);
        store.with_settings_reset_selection(|registry_path, registry_before, registry_after| {
            if registry_before != expected.registry.as_deref() {
                return Err(
                    "character selection changed after reset confirmation; nothing was reset"
                        .into(),
                );
            }
            let files = vec![
                ResetFile {
                    path: paths.config_dir.join("preferences.json"),
                    before: current.preferences,
                    after: Some(preference_bytes),
                },
                ResetFile {
                    path: lifecycle_path,
                    before: lifecycle_before,
                    after: Some(lifecycle_bytes),
                },
                ResetFile {
                    path: registry_path.to_owned(),
                    before: registry_before.map(<[u8]>::to_vec),
                    after: registry_after.map(<[u8]>::to_vec),
                },
            ];
            commit(paths, &files, &admit, &atomic_write)
        })
    })
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("cannot sync {}: {error}", path.display()))
}

fn recovery_root(paths: &Paths) -> Result<PathBuf, String> {
    let root = paths.state_dir.join(RECOVERY_DIR);
    validate_directory(&root, true)?;
    Ok(root)
}

fn commit(
    paths: &Paths,
    files: &[ResetFile],
    admit: &impl Fn() -> Result<(), String>,
    write: &impl Fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<PathBuf, String> {
    let root = recovery_root(paths)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let backup = root.join(format!("reset-{}-{nonce}", std::process::id()));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&backup)
        .map_err(|e| e.to_string())?;
    for (index, file) in files.iter().enumerate() {
        if let Some(bytes) = &file.before {
            atomic_write(&backup.join(format!("{index}.before.json")), bytes)?;
        }
        if let Some(bytes) = &file.after {
            atomic_write(&backup.join(format!("{index}.defaults.json")), bytes)?;
        }
    }
    let manifest = json!({
        "version": 1, "config_dir": paths.config_dir, "state_dir": paths.state_dir,
        "files": files.iter().enumerate().map(|(index, file)| json!({
            "path": file.path, "before": file.before.as_ref().map(|_| format!("{index}.before.json")),
            "defaults": file.after.as_ref().map(|_| format!("{index}.defaults.json")),
        })).collect::<Vec<_>>(),
        "status": "backed_up_before_write",
    });
    atomic_write(
        &backup.join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )?;
    sync_directory(&backup)?;
    sync_directory(&root)?;
    record_result(paths, "Settings reset has a complete backup; publication may be incomplete. Inspect manifest before restoring.", Some(&backup))?;
    // Do not start publication if a user Stop, updater reservation, or settings
    // change arrived while the backup was being made.
    admit()?;
    for file in files {
        if read_private(&file.path, MAX_SETTINGS_BYTES)? != file.before {
            return Err(format!(
                "settings changed during backup; nothing was reset; backup: {}",
                backup.display()
            ));
        }
    }
    let mut attempted = Vec::new();
    let operation = (|| {
        for file in files {
            if file.before == file.after {
                continue;
            }
            admit()?;
            attempted.push(file);
            if let Some(bytes) = &file.after {
                write(&file.path, bytes)?;
            }
        }
        admit()?;
        Ok::<(), String>(())
    })();
    if let Err(error) = operation {
        let mut failures = Vec::new();
        for file in attempted.into_iter().rev() {
            // A writer may fail after rename/directory fsync. Restore even the
            // attempted failing file, but never clobber an unrelated newer write.
            match read_private(&file.path, MAX_SETTINGS_BYTES) {
                Ok(current) if current == file.before => continue,
                Ok(current) if current == file.after => {}
                Ok(_) => {
                    failures.push(format!("{} changed concurrently", file.path.display()));
                    continue;
                }
                Err(e) => {
                    failures.push(e);
                    continue;
                }
            }
            let restored = match &file.before {
                Some(bytes) => atomic_write(&file.path, bytes),
                None => fs::remove_file(&file.path)
                    .map_err(|e| e.to_string())
                    .and_then(|()| sync_directory(file.path.parent().expect("settings parent"))),
            };
            if let Err(e) = restored {
                failures.push(e);
            }
        }
        let detail = if failures.is_empty() {
            format!(
                "Settings reset failed; original settings restored: {error}. Backup: {}",
                backup.display()
            )
        } else {
            format!("Settings reset partially failed: {error}; restoration errors: {}. Recover from backup: {}", failures.join("; "), backup.display())
        };
        let _ = record_result(paths, &detail, Some(&backup));
        return Err(detail);
    }
    record_result(paths, "Native settings reset committed. Downloaded/imported packs, integration routing, credentials and updater evidence were preserved. Restart pending.", Some(&backup))
        .map_err(|error| format!("Settings defaults committed, but the result receipt could not be saved: {error}; backup: {}", backup.display()))?;
    Ok(backup)
}

pub(crate) fn record_result(
    paths: &Paths,
    summary: &str,
    backup: Option<&Path>,
) -> Result<(), String> {
    let root = recovery_root(paths)?;
    let summary: String = summary.chars().take(6000).collect();
    // Keep the latest recoverable backup discoverable if a later preflight or
    // restart failure has no new backup of its own.
    let retained = if backup.is_none() {
        read_private(&root.join(RESULT_FILE), 16 * 1024)?.and_then(|bytes| {
            let value: Value = serde_json::from_slice(&bytes).ok()?;
            (value["config_dir"] == json!(paths.config_dir)
                && value["state_dir"] == json!(paths.state_dir))
            .then(|| value["backup"].as_str().map(PathBuf::from))
            .flatten()
        })
    } else {
        None
    };
    let backup = backup.or(retained.as_deref());
    let result = json!({ "version": 1, "config_dir": paths.config_dir, "state_dir": paths.state_dir, "summary": summary, "backup": backup });
    atomic_write(
        &root.join(RESULT_FILE),
        &serde_json::to_vec_pretty(&result).map_err(|e| e.to_string())?,
    )
}

pub(crate) fn load_last_result(paths: &Paths) -> Result<Option<String>, String> {
    let root = paths.state_dir.join(RECOVERY_DIR);
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
        Ok(metadata)
            if metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && metadata.uid() == unsafe { libc::geteuid() }
                && metadata.permissions().mode() & 0o077 == 0 => {}
        Ok(_) => return Err("settings reset evidence directory is not private and owned".into()),
    }
    let Some(bytes) = read_private(
        &paths.state_dir.join(RECOVERY_DIR).join(RESULT_FILE),
        16 * 1024,
    )?
    else {
        return Ok(None);
    };
    let result: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if result["version"] != 1
        || result["config_dir"] != json!(paths.config_dir)
        || result["state_dir"] != json!(paths.state_dir)
    {
        return Err("settings reset result belongs to a different profile".into());
    }
    let summary = result["summary"]
        .as_str()
        .ok_or("invalid settings reset result")?;
    let backup = result["backup"]
        .as_str()
        .map(|path| format!("\nLatest retained backup: {path}"))
        .unwrap_or_default();
    Ok(Some(format!("{summary}{backup}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn fixture() -> (tempfile::TempDir, Paths) {
        let root = tempfile::tempdir().unwrap();
        let canonical_root = root.path().canonicalize().unwrap();
        let paths = Paths::resolve(
            Some(&canonical_root.join("config")),
            Some(&canonical_root.join("state")),
        )
        .unwrap();
        (root, paths)
    }

    #[test]
    fn backups_precede_publication_and_are_private() {
        let (_root, paths) = fixture();
        let path = paths.config_dir.join("preferences.json");
        atomic_write(&path, b"old settings").unwrap();
        let files = [ResetFile {
            path: path.clone(),
            before: Some(b"old settings".to_vec()),
            after: Some(b"defaults".to_vec()),
        }];
        let backup = commit(&paths, &files, &|| Ok(()), &|target, bytes| {
            let summary = load_last_result(&paths).unwrap().unwrap();
            assert!(summary.contains("complete backup"));
            let root = paths.state_dir.join(RECOVERY_DIR);
            let directories: Vec<_> = fs::read_dir(root)
                .unwrap()
                .flatten()
                .filter(|entry| entry.file_type().unwrap().is_dir())
                .collect();
            let backup = directories[0].path();
            assert_eq!(
                fs::read(backup.join("0.before.json")).unwrap(),
                b"old settings"
            );
            assert_eq!(
                fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(backup.join("0.before.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            atomic_write(target, bytes)
        })
        .unwrap();
        assert!(backup.join("manifest.json").exists());
        assert_eq!(fs::read(path).unwrap(), b"defaults");
    }

    #[test]
    fn partial_failure_restores_even_post_rename_failure() {
        let (_root, paths) = fixture();
        let files: Vec<_> = ["one", "two"]
            .into_iter()
            .map(|name| {
                let path = paths.config_dir.join(name);
                atomic_write(&path, b"before").unwrap();
                ResetFile {
                    path,
                    before: Some(b"before".to_vec()),
                    after: Some(b"after".to_vec()),
                }
            })
            .collect();
        let calls = AtomicUsize::new(0);
        let error = commit(&paths, &files, &|| Ok(()), &|path, bytes| {
            atomic_write(path, bytes)?;
            if calls.fetch_add(1, Ordering::SeqCst) == 1 {
                Err("injected directory fsync failure".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(error.contains("original settings restored"));
        for file in files {
            assert_eq!(fs::read(file.path).unwrap(), b"before");
        }
        assert!(load_last_result(&paths)
            .unwrap()
            .unwrap()
            .contains("failed"));
    }

    #[test]
    fn stale_reset_does_not_overwrite_newer_settings() {
        let (_root, paths) = fixture();
        let path = paths.config_dir.join("preferences.json");
        atomic_write(&path, b"newer").unwrap();
        let files = [ResetFile {
            path: path.clone(),
            before: Some(b"stale".to_vec()),
            after: Some(b"defaults".to_vec()),
        }];
        assert!(commit(&paths, &files, &|| Ok(()), &|_, _| panic!(
            "stale reset wrote settings"
        ))
        .unwrap_err()
        .contains("changed"));
        assert_eq!(fs::read(path).unwrap(), b"newer");
    }

    #[test]
    fn backup_failure_never_writes_settings() {
        let (_root, paths) = fixture();
        atomic_write(&paths.state_dir.join(RECOVERY_DIR), b"not a directory").unwrap();
        let path = paths.config_dir.join("preferences.json");
        atomic_write(&path, b"original").unwrap();
        let files = [ResetFile {
            path: path.clone(),
            before: Some(b"original".to_vec()),
            after: Some(b"defaults".to_vec()),
        }];
        assert!(commit(&paths, &files, &|| Ok(()), &|_, _| panic!(
            "backup failure wrote settings"
        ))
        .is_err());
        assert_eq!(fs::read(path).unwrap(), b"original");
    }

    #[test]
    fn resetting_exact_profile_preserves_protocol_guards_and_other_profiles() {
        let (_root, paths) = fixture();
        let profile = ProfileStore::acquire(&paths.config_dir, &paths.state_dir).unwrap();
        let mut prefs = Preferences::default();
        prefs.set_visible(false);
        prefs.set_scale(2.0);
        prefs.set_receive_beta_updates(true);
        prefs.set_update_check_time(false, Some(123));
        let mut committed = Preferences::default();
        committed.save_candidate(prefs, &paths.config_dir).unwrap();
        let mut customized: Value =
            serde_json::from_slice(&fs::read(paths.config_dir.join("preferences.json")).unwrap())
                .unwrap();
        customized["language"] = json!("ja");
        customized["menu_bar_mode"] = json!("recovery_only");
        customized["menu_bar_icon"] = json!({"asset":"kept-icon.png"});
        customized["bubble_appearance"]["theme"] = json!("custom");
        customized["bubble_appearance"]["custom"]["accent"] = json!("#123456");
        customized["dialogue_overrides"] =
            json!({"characters":{"forest":{"ko":{"phases":{"idle":"custom dialogue"}}}}});
        customized["plugin_manager_installed"] = json!(true);
        atomic_write(
            &paths.config_dir.join("preferences.json"),
            &serde_json::to_vec(&customized).unwrap(),
        )
        .unwrap();
        let icon = paths.config_dir.join("kept-icon.png");
        atomic_write(&icon, b"retained icon source").unwrap();
        let registry_path = paths.config_dir.join("characters/registry.json");
        let registry = json!({"version":1,"generation":7,"selected":{"id":"forest","revision":1},
            "packs":[{"id":"forest","name":"Forest","head":1,"revisions":[1]}],
            "next_revision":2,"operations":[],"preserve_orphans":false});
        atomic_write(&registry_path, &serde_json::to_vec(&registry).unwrap()).unwrap();
        let pack = paths
            .config_dir
            .join("characters/packs/forest/1/source.png");
        atomic_write(&pack, b"retained imported character source").unwrap();
        atomic_write(
            &paths.config_dir.join(LIFECYCLE_FILE),
            br#"{"auto_start":false,"startup":{"token":"preserve"},"endpoints":["opaque"]}"#,
        )
        .unwrap();
        let guard = paths.state_dir.join("update-reservation.json");
        atomic_write(&guard, b"retained updater evidence").unwrap();
        let other = paths
            .config_dir
            .parent()
            .unwrap()
            .join("other-profile.json");
        atomic_write(&other, b"other profile").unwrap();
        let expected = capture(&paths).unwrap();
        reset(&paths, &profile, &[], &expected, || Ok(())).unwrap();
        let defaults = Preferences::load_committed(&paths.config_dir).unwrap();
        assert!(defaults.visible());
        assert_eq!(defaults.scale(), Preferences::default().scale());
        assert!(!defaults.receive_beta_updates());
        assert_eq!(defaults.update_check_time(false), Some(123));
        let lifecycle: Value =
            serde_json::from_slice(&fs::read(paths.config_dir.join(LIFECYCLE_FILE)).unwrap())
                .unwrap();
        assert_eq!(lifecycle["auto_start"], true);
        assert_eq!(lifecycle["startup"]["token"], "preserve");
        assert_eq!(lifecycle["endpoints"], json!(["opaque"]));
        let saved: Value =
            serde_json::from_slice(&fs::read(paths.config_dir.join("preferences.json")).unwrap())
                .unwrap();
        assert_eq!(saved["language"], "system");
        assert_eq!(saved["menu_bar_mode"], "always");
        assert!(saved.get("menu_bar_icon").is_none());
        assert_eq!(saved["bubble_appearance"]["theme"], "warm_ivory");
        assert_eq!(
            saved["dialogue_overrides"],
            serde_json::to_value(Preferences::default().dialogue_overrides()).unwrap()
        );
        assert_eq!(saved["plugin_manager_installed"], true);
        let reset_registry: Value =
            serde_json::from_slice(&fs::read(registry_path).unwrap()).unwrap();
        assert_eq!(
            reset_registry["selected"],
            json!({"id":"default","revision":0})
        );
        assert_eq!(reset_registry["generation"], 8);
        assert_eq!(reset_registry["packs"], registry["packs"]);
        assert_eq!(
            fs::read(pack).unwrap(),
            b"retained imported character source"
        );
        assert_eq!(fs::read(icon).unwrap(), b"retained icon source");
        assert_eq!(fs::read(guard).unwrap(), b"retained updater evidence");
        assert_eq!(fs::read(other).unwrap(), b"other profile");
    }

    #[test]
    fn engine_rejects_newer_profile_settings_without_a_write() {
        let (_root, paths) = fixture();
        let profile = ProfileStore::acquire(&paths.config_dir, &paths.state_dir).unwrap();
        let mut prefs = Preferences::default();
        prefs
            .save_candidate(Preferences::default(), &paths.config_dir)
            .unwrap();
        let expected = capture(&paths).unwrap();
        let mut newer = prefs.candidate();
        newer.set_visible(false);
        prefs.save_candidate(newer, &paths.config_dir).unwrap();
        let before = fs::read(paths.config_dir.join("preferences.json")).unwrap();
        let error = reset(&paths, &profile, &[], &expected, || Ok(())).unwrap_err();
        assert!(error.contains("settings changed"));
        assert_eq!(
            fs::read(paths.config_dir.join("preferences.json")).unwrap(),
            before
        );
        assert!(!paths.state_dir.join(RECOVERY_DIR).exists());
    }

    #[test]
    fn later_stop_admission_failure_rolls_back_partial_publication() {
        let (_root, paths) = fixture();
        let files: Vec<_> = ["one", "two"]
            .into_iter()
            .map(|name| {
                let path = paths.config_dir.join(name);
                atomic_write(&path, b"before").unwrap();
                ResetFile {
                    path,
                    before: Some(b"before".to_vec()),
                    after: Some(b"after".to_vec()),
                }
            })
            .collect();
        let admissions = AtomicUsize::new(0);
        let error = commit(
            &paths,
            &files,
            &|| {
                if admissions.fetch_add(1, Ordering::SeqCst) >= 2 {
                    Err("later user Stop superseded reset".into())
                } else {
                    Ok(())
                }
            },
            &atomic_write,
        )
        .unwrap_err();
        assert!(error.contains("later user Stop"));
        assert!(error.contains("original settings restored"));
        for file in files {
            assert_eq!(fs::read(file.path).unwrap(), b"before");
        }
    }
}
