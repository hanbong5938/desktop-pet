use desktop_pet_contracts::profile_store::{self, LIFECYCLE_FILE, LOCK_FILE};
use desktop_pet_contracts::{LifecycleLock, ProfileStore};
use serde_json::json;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture {
    root: PathBuf,
    config: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "desktop-pet-contracts-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let config = root.join("config");
        let state = root.join("state");
        profile_store::validate_directory(&config, true).unwrap();
        profile_store::validate_directory(&state, true).unwrap();
        Self {
            root,
            config,
            state,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn presence_and_boolean_type_are_not_defaulted_silently() {
    let fixture = Fixture::new();
    let store = ProfileStore::acquire(&fixture.config, &fixture.state).unwrap();
    let (object, exists) = store.read_object_with_presence().unwrap();
    assert!(!exists);
    assert_eq!(
        profile_store::setting_bool(&fixture.config, &object, "auto_start").unwrap(),
        None
    );
    store
        .update(|object, exists| {
            assert!(!exists);
            object.insert("auto_start".into(), json!("true"));
            Ok(())
        })
        .unwrap();
    let (object, exists) = store.read_object_with_presence().unwrap();
    assert!(exists);
    assert!(profile_store::setting_bool(&fixture.config, &object, "auto_start").is_err());
}

#[test]
fn both_owners_share_read_modify_write_and_preserve_unrelated_keys() {
    let fixture = Fixture::new();
    let lock = Arc::new(LifecycleLock::acquire(&fixture.state).unwrap());
    let core = ProfileStore::with_lock(&fixture.config, lock.clone()).unwrap();
    let adapter = ProfileStore::with_lock(&fixture.config, lock).unwrap();
    core.update(|object, _| {
        object.insert("unknown".into(), json!({ "future": [1, 2, 3] }));
        object.insert("endpoints".into(), json!(["/kept/endpoint.sock"]));
        Ok(())
    })
    .unwrap();
    let barrier = Arc::new(Barrier::new(3));
    std::thread::scope(|scope| {
        for (store, key) in [(&core, "auto_start"), (&adapter, "exit_with_herdr")] {
            let barrier = barrier.clone();
            scope.spawn(move || {
                barrier.wait();
                for _ in 0..20 {
                    store
                        .update(|object, _| {
                            object.insert(key.into(), json!(true));
                            Ok(())
                        })
                        .unwrap();
                }
            });
        }
        barrier.wait();
    });
    let (object, _) = core.read_object_with_presence().unwrap();
    assert_eq!(object["auto_start"], json!(true));
    assert_eq!(object["exit_with_herdr"], json!(true));
    assert_eq!(object["unknown"], json!({ "future": [1, 2, 3] }));
    assert_eq!(object["endpoints"], json!(["/kept/endpoint.sock"]));
    assert_eq!(
        fs::metadata(fixture.config.join(LIFECYCLE_FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn failed_owner_mutation_preserves_original_bytes() {
    let fixture = Fixture::new();
    let store = ProfileStore::acquire(&fixture.config, &fixture.state).unwrap();
    store
        .update(|object, _| {
            object.insert("kept".into(), json!(true));
            Ok(())
        })
        .unwrap();
    let path = fixture.config.join(LIFECYCLE_FILE);
    let before = fs::read(&path).unwrap();
    let result: Result<(), String> = store.update(|object, _| {
        object.clear();
        Err("rejected".into())
    });
    assert!(result.is_err());
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn preference_owners_and_lifecycle_updates_share_one_writer() {
    let fixture = Fixture::new();
    let lock = Arc::new(LifecycleLock::acquire(&fixture.state).unwrap());
    let core = ProfileStore::with_lock(&fixture.config, lock.clone()).unwrap();
    let adapter = ProfileStore::with_lock(&fixture.config, lock).unwrap();
    let path = fixture.config.join("preferences.json");
    profile_store::atomic_write(
        &path,
        &serde_json::to_vec(&json!({
            "unknown": { "future": [1, 2, 3] },
            "common_count": 0,
            "owner_count": 0
        }))
        .unwrap(),
    )
    .unwrap();
    let barrier = Barrier::new(4);
    let active = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for (store, key) in [(&core, "common_count"), (&adapter, "owner_count")] {
            let path = &path;
            let barrier = &barrier;
            let active = &active;
            scope.spawn(move || {
                barrier.wait();
                for _ in 0..20 {
                    store
                        .with_writer_lock(|| {
                            assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                            let mut object: serde_json::Map<String, serde_json::Value> =
                                serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
                            std::thread::yield_now();
                            let next = object[key].as_u64().unwrap() + 1;
                            object.insert(key.into(), json!(next));
                            let bytes = serde_json::to_vec(&object).unwrap();
                            profile_store::atomic_write(path, &bytes).unwrap();
                            assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
                            Ok(())
                        })
                        .unwrap();
                }
            });
        }
        let barrier = &barrier;
        let active = &active;
        let core = &core;
        scope.spawn(move || {
            barrier.wait();
            for _ in 0..20 {
                core.update(|object, _| {
                    assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                    std::thread::yield_now();
                    object.insert("auto_start".into(), json!(true));
                    assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
                    Ok(())
                })
                .unwrap();
            }
        });
        barrier.wait();
    });
    let object: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(object["common_count"], json!(20));
    assert_eq!(object["owner_count"], json!(20));
    assert_eq!(object["unknown"], json!({ "future": [1, 2, 3] }));
    assert_eq!(
        core.read_object_with_presence().unwrap().0["auto_start"],
        json!(true)
    );
}

#[test]
fn rejected_preference_candidate_does_not_commit_other_owner_changes() {
    let fixture = Fixture::new();
    let store = ProfileStore::acquire(&fixture.config, &fixture.state).unwrap();
    let path = fixture.config.join("preferences.json");
    profile_store::atomic_write(&path, br#"{"common":false,"owner":["kept"],"unknown":7}"#)
        .unwrap();
    let before = fs::read(&path).unwrap();
    let result: Result<(), String> = store.with_writer_lock(|| {
        let mut candidate: serde_json::Map<String, serde_json::Value> =
            serde_json::from_slice(&fs::read(&path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        candidate.insert("common".into(), json!(true));
        // The second owner's validation rejects before the sole atomic write.
        Err("owner selection rejected".into())
    });
    assert!(result.is_err());
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn poisoned_writer_rejects_all_store_wrappers_without_invoking_callbacks() {
    let fixture = Fixture::new();
    let lock = Arc::new(LifecycleLock::acquire(&fixture.state).unwrap());
    let core = ProfileStore::with_lock(&fixture.config, lock.clone()).unwrap();
    let adapter = ProfileStore::with_lock(&fixture.config, lock).unwrap();
    core.update(|object, _| {
        object.insert("kept".into(), json!(true));
        Ok(())
    })
    .unwrap();
    let before = fs::read(fixture.config.join(LIFECYCLE_FILE)).unwrap();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Result<(), String> = core.with_writer_lock(|| panic!("owner panicked"));
    }));
    assert!(panic.is_err());
    let mut called = false;
    let result: Result<(), String> = adapter.with_writer_lock(|| {
        called = true;
        Ok(())
    });
    assert!(result.is_err());
    assert!(!called);
    assert!(core.read_object_with_presence().is_err());
    assert!(adapter.update(|_, _| Ok(())).is_err());
    assert_eq!(
        fs::read(fixture.config.join(LIFECYCLE_FILE)).unwrap(),
        before
    );
}

#[test]
fn lifecycle_guard_is_exclusive_and_released_on_drop() {
    let fixture = Fixture::new();
    let lock = LifecycleLock::acquire(&fixture.state).unwrap();
    assert!(LifecycleLock::try_acquire(&fixture.state)
        .unwrap()
        .is_none());
    drop(lock);
    assert!(LifecycleLock::try_acquire(&fixture.state)
        .unwrap()
        .is_some());
}

#[test]
fn symlinks_and_nonprivate_files_are_rejected_without_replacement() {
    let fixture = Fixture::new();
    let outside = fixture.root.join("outside.json");
    fs::write(&outside, b"{}").unwrap();
    let path = fixture.config.join(LIFECYCLE_FILE);
    symlink(&outside, &path).unwrap();
    assert!(profile_store::read_object_with_presence(&fixture.config).is_err());
    assert!(profile_store::write_object(&fixture.config, Default::default()).is_err());
    assert!(fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(&outside).unwrap(), b"{}");
    fs::remove_file(&path).unwrap();
    fs::write(&path, b"{}").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(profile_store::read_object_with_presence(&fixture.config).is_err());
    assert!(profile_store::write_object(&fixture.config, Default::default()).is_err());
    symlink(&outside, fixture.state.join(LOCK_FILE)).unwrap();
    assert!(LifecycleLock::acquire(&fixture.state).is_err());
}

#[test]
fn invalid_object_never_gets_overwritten_with_defaults() {
    let fixture = Fixture::new();
    let path = fixture.config.join(LIFECYCLE_FILE);
    profile_store::atomic_write(&path, b"[]").unwrap();
    let store = ProfileStore::acquire(&fixture.config, &fixture.state).unwrap();
    let mut called = false;
    assert!(store
        .update(|_, _| {
            called = true;
            Ok(())
        })
        .is_err());
    assert!(!called);
    assert_eq!(fs::read(path).unwrap(), b"[]");
}
