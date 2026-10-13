use desktop_pet_contracts::profile_store::{self, LIFECYCLE_FILE, LOCK_FILE};
use desktop_pet_contracts::{LifecycleLock, ProfileStore};
use serde_json::json;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture {
    root: PathBuf,
    config: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "desktop-pet-contracts-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
