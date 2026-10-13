use crate::profile_store::effective_uid;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

const LOCK_MODE: u32 = 0o600;

const PREFERENCES_OWNER_LOCK: &str = "preferences-owner.lock";
const PREFERENCES_OWNER_BINDING: &str = "preferences-owner.json";

/// The config-wide preferences writer lease is independent of the state-scoped
/// lifecycle lease and of the optional Manager journal. Daemon and offline
/// availability writers both keep this locked descriptor until their last write.
#[derive(Debug)]
pub(crate) struct PreferencesOwner {
    config: File,
    state: File,
    _lock: File,
}

fn owner_directory(path: &Path) -> Result<File, String> {
    let absolute;
    let path = if path.is_absolute() {
        path
    } else {
        absolute = std::path::absolute(path).map_err(|error| error.to_string())?;
        absolute.as_path()
    };
    let path_name = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "invalid preferences owner directory".to_owned())?;
    let before = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if before.file_type().is_symlink() || !before.is_dir() {
        return Err("preferences owner directory must not be a symlink".into());
    }
    let fd = unsafe {
        libc::open(
            path_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(format!(
            "cannot open preferences owner directory: {}",
            io::Error::last_os_error()
        ));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let actual = file.metadata().map_err(|error| error.to_string())?;
    if !actual.is_dir()
        || actual.uid() != effective_uid()
        || actual.permissions().mode() & 0o077 != 0
        || (before.dev(), before.ino()) != (actual.dev(), actual.ino())
    {
        return Err("preferences owner directory is not a stable private profile".into());
    }
    owner_directory_stable(path, &file)?;
    Ok(file)
}

fn owner_directory_stable(path: &Path, file: &File) -> Result<(), String> {
    let path_meta = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    let fd_meta = file.metadata().map_err(|error| error.to_string())?;
    if !path_meta.is_dir()
        || path_meta.file_type().is_symlink()
        || path_meta.uid() != effective_uid()
        || path_meta.permissions().mode() & 0o077 != 0
        || (path_meta.dev(), path_meta.ino()) != (fd_meta.dev(), fd_meta.ino())
    {
        return Err("preferences owner profile changed while in use".into());
    }
    Ok(())
}

fn owner_file_at(parent: &File, filename: &str, create: bool) -> Result<Option<File>, String> {
    let component = std::ffi::CString::new(filename).map_err(|error| error.to_string())?;
    let flags = libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let mut fd = unsafe { libc::openat(parent.as_raw_fd(), component.as_ptr(), flags) };
    if fd < 0 && create && io::Error::last_os_error().kind() == io::ErrorKind::NotFound {
        fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                component.as_ptr(),
                flags | libc::O_CREAT | libc::O_EXCL,
                LOCK_MODE as libc::c_uint,
            )
        };
        if fd < 0 && io::Error::last_os_error().kind() == io::ErrorKind::AlreadyExists {
            fd = unsafe { libc::openat(parent.as_raw_fd(), component.as_ptr(), flags) };
        }
    }
    if fd < 0 {
        let error = io::Error::last_os_error();
        if !create && error.kind() == io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(format!(
            "cannot open preferences owner file {filename}: {error}"
        ));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let meta = file.metadata().map_err(|error| error.to_string())?;
    let mut at_meta: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            component.as_ptr(),
            &mut at_meta,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } < 0
        || !meta.is_file()
        || meta.uid() != effective_uid()
        || meta.nlink() != 1
        || meta.permissions().mode() & 0o777 != LOCK_MODE
        || (meta.dev(), meta.ino()) != (at_meta.st_dev as u64, at_meta.st_ino as u64)
    {
        return Err(format!(
            "preferences owner file {filename} is not private and stable"
        ));
    }
    Ok(Some(file))
}

fn owner_temporary_at(parent: &File, filename: &str) -> Result<File, String> {
    let component = std::ffi::CString::new(filename).map_err(|error| error.to_string())?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            component.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            LOCK_MODE as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(format!(
            "cannot create preferences owner temporary file: {}",
            io::Error::last_os_error()
        ));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let meta = file.metadata().map_err(|error| error.to_string())?;
    if !meta.is_file()
        || meta.uid() != effective_uid()
        || meta.nlink() != 1
        || meta.permissions().mode() & 0o777 != LOCK_MODE
    {
        return Err("preferences owner temporary file is not private".into());
    }
    Ok(file)
}

fn bind_preferences_owner(config: &File, state: &Path) -> Result<(), String> {
    let binding = owner_file_at(config, PREFERENCES_OWNER_BINDING, false)?;
    if let Some(file) = binding.as_ref() {
        let mut bytes = Vec::new();
        file.take(4097)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > 4096 {
            return Err("preferences owner binding exceeds size limit".into());
        }
        let previous: Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid preferences owner binding: {error}"))?;
        let old_state = previous
            .get("state_dir")
            .and_then(Value::as_str)
            .ok_or("invalid preferences owner state binding")?;
        if old_state == state.to_str().ok_or("invalid canonical state path")? {
            return Ok(());
        }
    }
    let mut random = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(|error| format!("cannot generate preferences owner record: {error}"))?;
    let temp = format!(
        ".preferences-owner-{:032x}.tmp",
        u128::from_be_bytes(random)
    );
    let mut file = owner_temporary_at(config, &temp)?;
    let result = (|| {
        let bytes = serde_json::to_vec(&serde_json::json!({"state_dir":state.to_str()
            .ok_or("invalid canonical state path")?}))
        .map_err(|error| error.to_string())?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("cannot persist preferences owner binding: {error}"))?;
        if let Some(previous) = binding.as_ref() {
            let current = owner_file_at(config, PREFERENCES_OWNER_BINDING, false)?
                .ok_or("preferences owner binding changed before replacement")?;
            let old = previous.metadata().map_err(|error| error.to_string())?;
            let new = current.metadata().map_err(|error| error.to_string())?;
            if (old.dev(), old.ino()) != (new.dev(), new.ino()) {
                return Err("preferences owner binding changed before replacement".into());
            }
        } else if owner_file_at(config, PREFERENCES_OWNER_BINDING, false)?.is_some() {
            return Err("preferences owner binding appeared before replacement".into());
        }
        let linked_temp = owner_file_at(config, &temp, false)?
            .ok_or("preferences owner temporary file changed")?;
        let written = file.metadata().map_err(|error| error.to_string())?;
        let linked = linked_temp.metadata().map_err(|error| error.to_string())?;
        if (written.dev(), written.ino()) != (linked.dev(), linked.ino()) {
            return Err("preferences owner temporary file changed before publication".into());
        }
        let from = std::ffi::CString::new(temp.as_str()).map_err(|error| error.to_string())?;
        let to =
            std::ffi::CString::new(PREFERENCES_OWNER_BINDING).map_err(|error| error.to_string())?;
        if unsafe {
            libc::renameat(
                config.as_raw_fd(),
                from.as_ptr(),
                config.as_raw_fd(),
                to.as_ptr(),
            )
        } < 0
        {
            return Err(format!(
                "cannot publish preferences owner binding: {}",
                io::Error::last_os_error()
            ));
        }
        config
            .sync_all()
            .map_err(|error| format!("cannot sync preferences owner profile: {error}"))
    })();
    if result.is_err() {
        if let Ok(name) = std::ffi::CString::new(temp) {
            let _ = unsafe { libc::unlinkat(config.as_raw_fd(), name.as_ptr(), 0) };
        }
    }
    result
}

impl PreferencesOwner {
    pub(crate) fn acquire(config_dir: &Path, state_dir: &Path) -> Result<Self, String> {
        let config = owner_directory(config_dir)?;
        let state = owner_directory(state_dir)?;
        let canonical_state = fs::canonicalize(state_dir).map_err(|error| error.to_string())?;
        let lock = owner_file_at(&config, PREFERENCES_OWNER_LOCK, true)?
            .ok_or("missing preferences owner lock")?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } < 0 {
            let error = io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN)
            {
                return Err("another preferences writer owns this config profile".into());
            }
            return Err(format!("cannot acquire preferences owner: {error}"));
        }
        let linked = owner_file_at(&config, PREFERENCES_OWNER_LOCK, false)?
            .ok_or("preferences owner lock changed")?;
        let held = lock.metadata().map_err(|error| error.to_string())?;
        let current = linked.metadata().map_err(|error| error.to_string())?;
        if (held.dev(), held.ino()) != (current.dev(), current.ino()) {
            return Err("preferences owner lock changed after acquisition".into());
        }
        owner_directory_stable(config_dir, &config)?;
        owner_directory_stable(state_dir, &state)?;
        bind_preferences_owner(&config, &canonical_state)?;
        owner_directory_stable(config_dir, &config)?;
        owner_directory_stable(state_dir, &state)?;
        Ok(Self {
            config,
            state,
            _lock: lock,
        })
    }

    pub(crate) fn verify(&self, config_dir: &Path, state_dir: &Path) -> Result<(), String> {
        owner_directory_stable(config_dir, &self.config)?;
        owner_directory_stable(state_dir, &self.state)
    }
}

impl Drop for PreferencesOwner {
    fn drop(&mut self) {
        unsafe {
            let _ = libc::flock(self._lock.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProfileStore;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "desktop-pet-config-owner-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            Self(fs::canonicalize(root).unwrap())
        }

        fn directory(&self, name: &str) -> PathBuf {
            let directory = self.0.join(name);
            fs::create_dir(&directory).unwrap();
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
            directory
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn config_owner_excludes_different_state_until_last_shared_writer_drops() {
        let fixture = Fixture::new();
        let config = fixture.directory("config");
        let state_a = fixture.directory("state-a");
        let state_b = fixture.directory("state-b");
        let first = ProfileStore::acquire(&config, &state_a).unwrap();
        let shared = ProfileStore::with_lock(&config, first.lifecycle_lock().clone()).unwrap();
        drop(first);
        assert!(ProfileStore::acquire(&config, &state_b).is_err());
        let binding: Value =
            serde_json::from_slice(&fs::read(config.join(PREFERENCES_OWNER_BINDING)).unwrap())
                .unwrap();
        assert_eq!(binding["state_dir"], state_a.to_str().unwrap());
        assert!(!config.join("plugin-manager").exists());
        drop(shared);
        let second = ProfileStore::acquire(&config, &state_b).unwrap();
        second.verify_owner().unwrap();
        let binding: Value =
            serde_json::from_slice(&fs::read(config.join(PREFERENCES_OWNER_BINDING)).unwrap())
                .unwrap();
        assert_eq!(binding["state_dir"], state_b.to_str().unwrap());
    }

    #[test]
    fn replaced_config_identity_rejects_writer_without_touching_replacement() {
        let fixture = Fixture::new();
        let config = fixture.directory("config");
        let state = fixture.directory("state");
        let owner = ProfileStore::acquire(&config, &state).unwrap();
        fs::rename(&config, fixture.0.join("original-config")).unwrap();
        fs::create_dir(&config).unwrap();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(owner
            .with_writer_lock(|| {
                fs::write(config.join("must-not-write"), b"wrong profile")
                    .map_err(|error| error.to_string())
            })
            .is_err());
        assert!(!config.join("must-not-write").exists());
    }

    #[test]
    fn alias_owner_lock_rejects_without_touching_unrelated_file() {
        let fixture = Fixture::new();
        let config = fixture.directory("config");
        let state = fixture.directory("state");
        let target = fixture.0.join("unrelated");
        fs::write(&target, b"unchanged").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        let lock = config.join(PREFERENCES_OWNER_LOCK);
        std::os::unix::fs::symlink(&target, &lock).unwrap();
        assert!(PreferencesOwner::acquire(&config, &state).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"unchanged");
        fs::remove_file(&lock).unwrap();
        fs::hard_link(&target, &lock).unwrap();
        assert!(PreferencesOwner::acquire(&config, &state).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"unchanged");
    }
}
