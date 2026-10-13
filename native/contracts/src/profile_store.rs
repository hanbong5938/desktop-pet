use serde_json::{Map, Value};
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub const LIFECYCLE_FILE: &str = "lifecycle.json";
pub const LOCK_FILE: &str = "desktop-pet.lock";
const LOCK_MODE: u32 = 0o600;
const DIRECTORY_MODE: u32 = 0o700;

pub fn effective_uid() -> u32 {
    unsafe { libc::geteuid() }
}

/// Existing lifecycle directory validation: reject symlinks/non-directories
/// and foreign ownership, secure owned directories to 0700, then recheck.
pub fn validate_directory(path: &Path, create: bool) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("directory path is empty".to_owned());
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "directory {} must not be a symlink",
                    path.display()
                ));
            }
            if !metadata.is_dir() {
                return Err(format!("path {} is not a directory", path.display()));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound && create => {
            fs::create_dir_all(path)
                .map_err(|error| format!("cannot create directory {}: {error}", path.display()))?;
        }
        Err(error) => {
            return Err(format!(
                "cannot inspect directory {}: {error}",
                path.display()
            ))
        }
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect directory {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "path {} is not a regular directory",
            path.display()
        ));
    }
    if metadata.uid() != effective_uid() {
        return Err(format!(
            "directory {} is not owned by the current user",
            path.display()
        ));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        fs::set_permissions(path, Permissions::from_mode(DIRECTORY_MODE)).map_err(|error| {
            format!(
                "directory {} is not private and cannot be secured: {error}",
                path.display()
            )
        })?;
    }
    let secured = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect directory {}: {error}", path.display()))?;
    if secured.file_type().is_symlink()
        || !secured.is_dir()
        || secured.uid() != effective_uid()
        || secured.permissions().mode() & 0o077 != 0
    {
        return Err(format!(
            "directory {} is not private to the current user",
            path.display()
        ));
    }
    Ok(path.to_path_buf())
}

/// The existing profile-wide, nonblocking lifecycle flock. Keep it alive for
/// the lifecycle operation/daemon lifetime. Both owners share the SAME Arc of
/// this guard, whose writer mutex serializes in-process read-modify-write too.
/// It is not reacquired for each write by a daemon that already owns the flock.
#[derive(Debug)]
pub struct LifecycleLock {
    file: File,
    state_dir: PathBuf,
    writes: Mutex<()>,
}

impl LifecycleLock {
    pub fn acquire(state_dir: &Path) -> Result<Self, String> {
        validate_directory(state_dir, true)?;
        let path = state_dir.join(LOCK_FILE);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(format!(
                        "lifecycle lock {} must not be a symlink",
                        path.display()
                    ));
                }
                if !metadata.is_file() {
                    return Err(format!(
                        "lifecycle lock {} is not a regular file",
                        path.display()
                    ));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "cannot inspect lifecycle lock {}: {error}",
                    path.display()
                ))
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(LOCK_MODE)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|error| format!("cannot open lifecycle lock {}: {error}", path.display()))?;
        let metadata = file.metadata().map_err(|error| {
            format!("cannot inspect lifecycle lock {}: {error}", path.display())
        })?;
        if !metadata.is_file() {
            return Err(format!(
                "lifecycle lock {} is not a regular file",
                path.display()
            ));
        }
        if metadata.uid() != effective_uid() {
            return Err(format!(
                "lifecycle lock {} is not owned by the current user",
                path.display()
            ));
        }
        file.set_permissions(Permissions::from_mode(LOCK_MODE))
            .map_err(|error| format!("cannot secure lifecycle lock {}: {error}", path.display()))?;
        if file
            .metadata()
            .map_err(|error| error.to_string())?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err(format!("lifecycle lock {} is not private", path.display()));
        }
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let error = io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN)
            {
                return Err("another desktop-pet lifecycle operation is in progress".to_owned());
            }
            return Err(format!(
                "cannot lock lifecycle guard {}: {error}",
                path.display()
            ));
        }
        Ok(Self {
            file,
            state_dir: state_dir.to_path_buf(),
            writes: Mutex::new(()),
        })
    }

    pub fn try_acquire(state_dir: &Path) -> Result<Option<Self>, String> {
        match Self::acquire(state_dir) {
            Ok(lock) => Ok(Some(lock)),
            Err(error) if error.contains("lifecycle operation is in progress") => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }
}

impl Drop for LifecycleLock {
    fn drop(&mut self) {
        unsafe {
            let _ = libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

/// A shared profile writer and lifecycle object store, not a settings schema.
/// Unknown keys survive lifecycle updates because the callback mutates the
/// object read under the guard. Core and adapters must clone this store or its
/// lifecycle guard, never create independent writer locks for the same profile.
#[derive(Clone, Debug)]
pub struct ProfileStore {
    config_dir: PathBuf,
    lock: Arc<LifecycleLock>,
}

impl ProfileStore {
    pub fn acquire(config_dir: &Path, state_dir: &Path) -> Result<Self, String> {
        Self::with_lock(config_dir, Arc::new(LifecycleLock::acquire(state_dir)?))
    }

    pub fn with_lock(config_dir: &Path, lock: Arc<LifecycleLock>) -> Result<Self, String> {
        validate_directory(config_dir, true)?;
        Ok(Self {
            config_dir: config_dir.to_path_buf(),
            lock,
        })
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }
    pub fn lifecycle_lock(&self) -> &Arc<LifecycleLock> {
        &self.lock
    }

    /// Serializes a complete owner-defined read/validate/atomic-write
    /// transaction, including files other than lifecycle.json. This uses the
    /// same mutex as lifecycle reads and updates, even across store wrappers
    /// sharing the lifecycle guard.
    ///
    /// Poison returns Err without invoking the callback. The store performs no
    /// implicit write or rollback: validate and stage all changes before the
    /// callback's final atomic write so a rejected operation writes nothing.
    /// A panic poisons the writer; it is never silently recovered.
    ///
    /// No reentry: do not call this method, read_object_with_presence, or update
    /// on any store sharing the guard from the callback. Use low-level file
    /// helpers instead. Do not acquire the domain lock or perform host I/O
    /// while holding the writer; live application belongs after commit.
    pub fn with_writer_lock<T>(
        &self,
        operation: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _guard = self
            .lock
            .writes
            .lock()
            .map_err(|_| "profile writer lock poisoned".to_owned())?;
        operation()
    }

    pub fn read_object_with_presence(&self) -> Result<(Map<String, Value>, bool), String> {
        self.with_writer_lock(|| read_object_with_presence(&self.config_dir))
    }

    /// The callback changes only its owner's keys. Err or panic never writes
    /// the object; poison is subsequently surfaced, not silently recovered.
    /// Do not call this store again from inside its update callback.
    pub fn update<T>(
        &self,
        operation: impl FnOnce(&mut Map<String, Value>, bool) -> Result<T, String>,
    ) -> Result<T, String> {
        self.with_writer_lock(|| {
            let (mut object, exists) = read_object_with_presence(&self.config_dir)?;
            let result = operation(&mut object, exists)?;
            write_object(&self.config_dir, object)?;
            Ok(result)
        })
    }
}

fn validate_file(path: &Path, metadata: &fs::Metadata) -> Result<(), String> {
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "lifecycle state {} is not a regular file",
            path.display()
        ));
    }
    if metadata.uid() != effective_uid() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(format!(
            "lifecycle state {} is not private to the current user",
            path.display()
        ));
    }
    Ok(())
}

/// Low-level helper. A writer must hold the shared lifecycle guard/writer
/// transaction across this read and write_object; prefer ProfileStore::update.
/// Missing and an existing empty object are intentionally distinct inputs.
pub fn read_object_with_presence(config_dir: &Path) -> Result<(Map<String, Value>, bool), String> {
    let path = config_dir.join(LIFECYCLE_FILE);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((Map::new(), false)),
        Err(error) => {
            return Err(format!(
                "cannot inspect lifecycle state {}: {error}",
                path.display()
            ))
        }
    };
    validate_file(&path, &metadata)?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|error| format!("cannot read lifecycle state {}: {error}", path.display()))?;
    validate_file(&path, &file.metadata().map_err(|error| error.to_string())?)?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|error| format!("cannot read lifecycle state {}: {error}", path.display()))?;
    match serde_json::from_str::<Value>(&text)
        .map_err(|error| format!("invalid lifecycle state {}: {error}", path.display()))?
    {
        Value::Object(object) => Ok((object, true)),
        _ => Err(format!(
            "invalid lifecycle state {}: expected an object",
            path.display()
        )),
    }
}

pub fn read_object(config_dir: &Path) -> Result<Map<String, Value>, String> {
    read_object_with_presence(config_dir).map(|(object, _)| object)
}

pub fn setting_bool(
    config_dir: &Path,
    object: &Map<String, Value>,
    key: &str,
) -> Result<Option<bool>, String> {
    match object.get(key) {
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(format!(
            "invalid lifecycle state {}: {key} must be boolean",
            config_dir.join(LIFECYCLE_FILE).display()
        )),
        None => Ok(None),
    }
}

/// Low-level write; hold the shared guard across read-modify-write, or use
/// ProfileStore::update. Only supplied keys are interpreted by the caller.
pub fn write_object(config_dir: &Path, object: Map<String, Value>) -> Result<(), String> {
    validate_directory(config_dir, true)?;
    let bytes = serde_json::to_vec_pretty(&Value::Object(object))
        .map_err(|error| format!("cannot encode lifecycle state: {error}"))?;
    atomic_write(&config_dir.join(LIFECYCLE_FILE), &bytes)
}

/// Private create_new staging, file fsync, atomic rename, and directory fsync.
/// Reject an existing symlink, nonregular, nonowned, or nonprivate target.
/// Temporary bytes are removed on failure; the previous file is never deleted.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("lifecycle path {} has no parent", path.display()))?;
    validate_directory(parent, true)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) => validate_file(path, &metadata)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "cannot inspect lifecycle state {}: {error}",
                path.display()
            ))
        }
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id(),
        nonce
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(LOCK_MODE)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temp)
        .map_err(|error| {
            format!(
                "cannot create lifecycle temporary file {}: {error}",
                temp.display()
            )
        })?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|error| format!("cannot write lifecycle state {}: {error}", temp.display()))?;
        file.sync_all()
            .map_err(|error| format!("cannot flush lifecycle state {}: {error}", temp.display()))?;
        fs::rename(&temp, path).map_err(|error| {
            format!("cannot replace lifecycle state {}: {error}", path.display())
        })?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                format!(
                    "cannot flush lifecycle directory {}: {error}",
                    parent.display()
                )
            })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
