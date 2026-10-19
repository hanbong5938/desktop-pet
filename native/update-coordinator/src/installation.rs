//! Verified public installation discovery and explicitly requested atomic installation.
//! The locator is a preference, never authority: every use re-proves its actual image and owner.
//! Migration approval may upgrade a verified older neutral owner without adopting its locator:
//! source/standalone replacement requires an inactive root, while Brew retains its old live keg.
use crate::{
    ensure_absolute, executable_identity, identity, origin, owned_file, owned_private_file,
    physical_install_root, protocol, ExecutableIdentity, InstallOrigin, ReleaseChannel,
    UpdateAction, UpdatePlan, VerifiedInstallation,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

const MAX_RECORD: u64 = 2 * 1024 * 1024;
const MAX_FILE: u64 = 512 * 1024 * 1024;
const MAX_ARCHIVE: u64 = 1024 * 1024 * 1024;
const MAX_FILES: usize = 32768;

const INSTALLATION_DIRECTORIES: [(&str, bool); 4] = [
    (".local", false),
    ("share", false),
    ("desktop-pet", true),
    ("libexec", true),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationKind {
    Homebrew,
    Standalone,
    Source,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallationRecord {
    pub version: u32,
    pub executable: PathBuf,
    pub kind: InstallationKind,
    pub app_version: String,
    pub protocol: u32,
    pub observation_mode: u32,
    pub integration_api: u32,
    #[serde(with = "crate::migration::neutral_origin_codec")]
    pub origin: InstallOrigin,
    pub image: ExecutableIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedApp {
    pub bundle: PathBuf,
    pub image: ExecutableIdentity,
    pub version: String,
    pub installation_client_contract: Option<u32>,
}

/// Durable, operation-specific stage. Recovery loads it; it never repeats a download or commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedApp {
    pub bundle: PathBuf,
    pub image: ExecutableIdentity,
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRequest {
    pub version: u32,
    pub staged_app: PathBuf,
    pub kind: InstallationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_root: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Locator {
    version: u32,
    executable: PathBuf,
    kind: InstallationKind,
}

fn require_neutral() -> Result<(), String> {
    if identity::LEGACY {
        return Err("public installation APIs require the neutral distribution".into());
    }
    Ok(())
}

/// Actual account home from the operating system; HOME and injected host directories are ignored.
pub fn os_home() -> Result<PathBuf, String> {
    let mut user: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0u8; 16384];
    let rc = unsafe {
        libc::getpwuid_r(
            libc::geteuid(),
            &mut user,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() || user.pw_dir.is_null() {
        return Err("cannot determine OS account home".into());
    }
    let path = PathBuf::from(std::ffi::OsStr::from_bytes(
        unsafe { CStr::from_ptr(user.pw_dir) }.to_bytes(),
    ));
    physical_directory(&path, false)?;
    Ok(path)
}

/// Fresh standalone destination, rooted in the actual OS account home, never HOME or XDG.
pub fn standalone_bundle() -> Result<PathBuf, String> {
    Ok(private_standalone_bundle_in(&os_home()?))
}

fn private_standalone_bundle_in(home: &Path) -> PathBuf {
    let mut private = home.to_path_buf();
    for (name, _) in INSTALLATION_DIRECTORIES {
        private.push(name);
    }
    private.push("DesktopPet.app");
    private
}

fn standalone_bundles_in(home: &Path) -> [PathBuf; 2] {
    [
        private_standalone_bundle_in(home),
        home.join("Applications/DesktopPet.app"),
    ]
}

pub(crate) fn standalone_bundles() -> Result<[PathBuf; 2], String> {
    Ok(standalone_bundles_in(&os_home()?))
}

// The historical owner is upgrade-only: neither its parent nor its bundle may be created here.
fn standalone_owner_parent_in(
    home: &Path,
    bundle: &Path,
    create_private: bool,
) -> Result<Option<PathBuf>, String> {
    let [private, original] = standalone_bundles_in(home);
    let parent = if bundle == private {
        owned_directory_tree(home, &INSTALLATION_DIRECTORIES, create_private)?
    } else if bundle == original {
        physical_directory(home, false)?;
        let parent = home.join("Applications");
        physical_directory(&parent, false)?;
        physical_directory(bundle, false)?;
        Some(parent)
    } else {
        return Err("standalone owner is outside the two supported OS-home locations".into());
    };
    if exists(bundle)? {
        physical_directory(bundle, false)?;
    }
    Ok(parent)
}

pub(crate) fn validate_standalone_owner(bundle: &Path) -> Result<(), String> {
    standalone_owner_parent_in(&os_home()?, bundle, false).map(|_| ())
}

fn standalone_candidate_in(home: &Path) -> Result<Option<PathBuf>, String> {
    let [private, original] = standalone_bundles_in(home);
    match (exists(&private)?, exists(&original)?) {
        (true, true) => Err(
            "multiple standalone owners exist; select an explicit executable or verified locator"
                .into(),
        ),
        (true, false) => Ok(Some(private)),
        (false, true) => Ok(Some(original)),
        (false, false) => Ok(None),
    }
}

fn standalone_origin_in(
    home: &Path,
    current: Option<&InstallationRecord>,
) -> Result<InstallOrigin, String> {
    if let Some(current) = current.filter(|c| c.kind == InstallationKind::Standalone) {
        if let InstallOrigin::Standalone { bundle } = &current.origin {
            standalone_owner_parent_in(home, bundle, false)?;
            return Ok(current.origin.clone());
        }
        return Err("standalone record differs from actual installation owner".into());
    }
    Ok(InstallOrigin::Standalone {
        bundle: private_standalone_bundle_in(home),
    })
}

pub fn locator_path() -> Result<PathBuf, String> {
    Ok(os_home()?.join(".local/share/desktop-pet/installation.json"))
}

fn exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn physical_directory(path: &Path, private: bool) -> Result<(), String> {
    ensure_absolute(path)?;
    let m = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !m.is_dir()
        || m.file_type().is_symlink()
        || m.uid() != unsafe { libc::geteuid() }
        || m.permissions().mode() & (if private { 0o077 } else { 0o022 }) != 0
        || fs::canonicalize(path).map_err(|e| e.to_string())? != path
    {
        return Err(format!(
            "unsafe nonphysical or nonowned installation directory: {}",
            path.display()
        ));
    }
    Ok(())
}

fn create_directory(path: &Path, private: bool) -> Result<(), String> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(format!("{}: {e}", path.display())),
    }
    physical_directory(path, private)
}

fn owned_directory_tree(
    root: &Path,
    components: &[(&str, bool)],
    create: bool,
) -> Result<Option<PathBuf>, String> {
    physical_directory(root, false)?;
    let mut path = root.to_path_buf();
    for &(name, private) in components {
        path.push(name);
        if create {
            create_directory(&path, private)?;
        } else if exists(&path)? {
            physical_directory(&path, private)?;
        } else {
            return Ok(None);
        }
    }
    Ok(Some(path))
}

fn locator_directory(create: bool) -> Result<Option<PathBuf>, String> {
    owned_directory_tree(&os_home()?, &INSTALLATION_DIRECTORIES[..3], create)
}

struct InstallLock {
    _file: File,
}
fn install_lock() -> Result<InstallLock, String> {
    let directory = locator_directory(true)?.ok_or("missing installation directory")?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(directory.join("install.lock"))
        .map_err(|e| e.to_string())?;
    let m = file.metadata().map_err(|e| e.to_string())?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || m.permissions().mode() & 0o077 != 0
    {
        return Err("unsafe installation lock".into());
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(format!(
            "installation is locked: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(InstallLock { _file: file })
}

fn read_locator() -> Result<Option<Locator>, String> {
    let Some(directory) = locator_directory(false)? else {
        return Ok(None);
    };
    let path = directory.join("installation.json");
    if !exists(&path)? {
        return Ok(None);
    }
    let record: Locator = serde_json::from_slice(&owned_private_file(&path, 16384)?)
        .map_err(|e| format!("invalid installation locator: {e}"))?;
    if record.version != 1 {
        return Err("unsupported installation locator version".into());
    }
    ensure_absolute(&record.executable)?;
    Ok(Some(record))
}

fn write_locator(record: &InstallationRecord) -> Result<(), String> {
    let locator = Locator {
        version: 1,
        executable: record.executable.clone(),
        kind: record.kind,
    };
    protocol::write_private_bytes(
        &locator_path()?,
        &serde_json::to_vec(&locator).map_err(|e| e.to_string())?,
        0o600,
    )
}

fn path_str(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| "installation path is not UTF-8".into())
}
fn tool(binary: &str, args: &[&str], max: usize, seconds: u64) -> Result<String, String> {
    origin::command_output(Path::new(binary), args, max, seconds)
}
fn installed_version(version: &str) -> Result<Version, String> {
    let parsed = Version::parse(version).map_err(|e| format!("invalid installed version: {e}"))?;
    let beta = parsed
        .pre
        .as_str()
        .strip_prefix("beta.")
        .is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        });
    if (!parsed.pre.is_empty() && !beta)
        || !parsed.build.is_empty()
        || parsed.to_string() != version
    {
        return Err("public installation requires a canonical stable or beta.N version".into());
    }
    Ok(parsed)
}
fn stable_version(version: &str) -> Result<Version, String> {
    let parsed = Version::parse(version).map_err(|e| format!("invalid installed version: {e}"))?;
    if !parsed.pre.is_empty() || !parsed.build.is_empty() || parsed.to_string() != version {
        return Err("public installation requires a canonical stable version, not beta".into());
    }
    Ok(parsed)
}
fn capabilities(binary: &Path) -> Result<(Option<u64>, Option<u32>), String> {
    let parsed: Value = serde_json::from_str(&origin::command_output(
        binary,
        &["update-capabilities"],
        4096,
        5,
    )?)
    .map_err(|e| format!("invalid installation capabilities: {e}"))?;
    decode_capabilities(&parsed)
}

fn decode_capabilities(parsed: &Value) -> Result<(Option<u64>, Option<u32>), String> {
    for (key, expected) in [
        ("protocol", 4),
        ("observation_mode", 2),
        ("integration_api", 1),
    ] {
        if parsed.get(key).and_then(Value::as_u64) != Some(expected) {
            return Err(format!(
                "installed executable lacks compatible {key}={expected}"
            ));
        }
    }
    Ok((
        crate::migration_handover_capability(parsed)?,
        crate::installation_client_contract(parsed)?,
    ))
}

fn files_under(
    directory: &Path,
    relative: &Path,
    files: &mut BTreeMap<String, PathBuf>,
    total: &mut u64,
) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let m = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if m.uid() != unsafe { libc::geteuid() }
            || m.permissions().mode() & 0o022 != 0
            || m.file_type().is_symlink()
        {
            return Err(format!("unsafe bundle resource: {}", path.display()));
        }
        let name = relative.join(entry.file_name());
        if name.components().count() > 32 {
            return Err("bundle resource depth exceeds bound".into());
        }
        if m.is_dir() {
            files_under(&path, &name, files, total)?;
        } else if m.is_file() && m.nlink() == 1 && m.len() <= MAX_FILE {
            *total = total.checked_add(m.len()).ok_or("bundle size overflow")?;
            if *total > 2 * MAX_ARCHIVE || files.len() >= MAX_FILES {
                return Err("bundle exceeds resource bounds".into());
            }
            files.insert(path_str(&name)?.to_owned(), path);
        } else {
            return Err(format!("unsupported bundle resource: {}", path.display()));
        }
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, String> {
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| e.to_string())?;
    let before = f.metadata().map_err(|e| e.to_string())?;
    if !before.is_file()
        || before.uid() != unsafe { libc::geteuid() }
        || before.nlink() != 1
        || before.len() > MAX_ARCHIVE
        || before.permissions().mode() & 0o022 != 0
    {
        return Err("unsafe installation hash input".into());
    }
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    let after = f.metadata().map_err(|e| e.to_string())?;
    let current = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let proof = |m: &fs::Metadata| (m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec());
    if proof(&before) != proof(&after) || proof(&after) != proof(&current) {
        return Err("installation file changed during validation".into());
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub(crate) fn sidecar_path(bundle: &Path) -> PathBuf {
    let mut name = bundle.as_os_str().to_os_string();
    name.push(".rig-native.json");
    PathBuf::from(name)
}

fn validate_deploy_record(
    bundle: &Path,
    version: &str,
    files: &BTreeMap<String, PathBuf>,
    snapshot: Option<&[u8]>,
) -> Result<(), String> {
    let owned;
    let bytes = match snapshot {
        Some(bytes) => bytes,
        None => {
            let sidecar = sidecar_path(bundle);
            let m = fs::symlink_metadata(&sidecar).map_err(|e| e.to_string())?;
            if m.file_type().is_symlink() || m.permissions().mode() & 0o022 != 0 {
                return Err("unsafe rig deployment record".into());
            }
            owned = owned_file(&sidecar, MAX_RECORD)?;
            &owned
        }
    };
    let record: Value =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid rig deployment record: {e}"))?;
    if record.get("version").and_then(Value::as_u64) != Some(1)
        || record.get("paths_relative_to").and_then(Value::as_str) != Some("Contents")
        || record.get("probe_faults").and_then(Value::as_bool) != Some(false)
        || !record
            .get("core_revision")
            .and_then(Value::as_str)
            .is_some_and(|r| r.len() == 40 && r.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("rig record has invalid deployment provenance".into());
    }
    let expected = [
        ("distribution", "neutral"),
        ("app_name", "DesktopPet.app"),
        ("executable", "desktop-pet"),
        ("helper", "desktop-pet-update-coordinator"),
        ("display_name", "Desktop Pet"),
        ("bundle_id", "io.github.hanbong5938.desktop-pet"),
        ("app_version", version),
    ];
    let distribution = record
        .get("distribution_identity")
        .ok_or("rig record lacks distribution identity")?;
    for (key, expected) in expected {
        if distribution.get(key).and_then(Value::as_str) != Some(expected) {
            return Err(format!("rig distribution identity mismatch: {key}"));
        }
    }
    for (key, expected) in [
        ("protocol", 4),
        ("observation_mode", 2),
        ("integration_api", 1),
    ] {
        if distribution.get(key).and_then(Value::as_u64) != Some(expected) {
            return Err(format!("rig distribution capability mismatch: {key}"));
        }
    }
    for (key, expected) in [
        ("dylib", "Frameworks/libherdr_rig.dylib"),
        ("worker", "MacOS/rig-decode-worker"),
        ("coordinator", "MacOS/desktop-pet-update-coordinator"),
        ("decoder", "Resources/rig/decoder.js"),
        ("limits", "Resources/rig/limits.json"),
        ("build_record", "Resources/rig-native-build.json"),
        ("install_name", "@rpath/libherdr_rig.dylib"),
        ("packaged_rpath", "@executable_path/../Frameworks"),
    ] {
        if record.get(key).and_then(Value::as_str) != Some(expected) {
            return Err(format!("rig deployment layout mismatch: {key}"));
        }
    }
    validate_signing_derivation(bundle, &record)?;
    let hashes = record
        .get("deployed_sha256")
        .and_then(Value::as_object)
        .ok_or("rig record lacks deployed hashes")?;
    if hashes.len() != files.len() {
        return Err("rig deployment file closure differs from bundle".into());
    }
    for (name, path) in files {
        if hashes.get(name).and_then(Value::as_str) != Some(hash_file(path)?.as_str()) {
            return Err(format!("rig deployed hash mismatch: {name}"));
        }
    }
    Ok(())
}

fn validate_signing_derivation(bundle: &Path, record: &Value) -> Result<(), String> {
    let Some(derivation) = record.get("signing_derivation") else {
        return Ok(());
    };
    #[cfg(desktop_pet_legacy)]
    {
        let _ = (bundle, derivation);
        return Err(
            "downloaded neutral migration stages cannot carry manager signing derivation".into(),
        );
    }
    #[cfg(not(desktop_pet_legacy))]
    {
        let image = bundle.join("Contents/MacOS/desktop-pet");
        if !matches!(
            origin::brew_origin_for_executable(&image)?,
            Some(InstallOrigin::Homebrew { .. })
        ) {
            return Err(
                "signing derivation requires actual captured Brew formula/root/receipt ownership"
                    .into(),
            );
        }
        let fields = derivation
            .as_object()
            .ok_or("invalid signing derivation object")?;
        if fields.len() != 3
            || derivation.get("kind").and_then(Value::as_str) != Some("homebrew_ad_hoc")
            || derivation.get("original_record").and_then(Value::as_str)
                != Some("DesktopPet.app.release-rig-native.json")
        {
            return Err("unapproved manager signing derivation".into());
        }
        let original_path = bundle
            .parent()
            .ok_or("derived bundle lacks parent")?
            .join("DesktopPet.app.release-rig-native.json");
        let m = fs::symlink_metadata(&original_path).map_err(|e| e.to_string())?;
        if m.file_type().is_symlink() || m.permissions().mode() & 0o022 != 0 {
            return Err("unsafe original release rig record".into());
        }
        let original_bytes = owned_file(&original_path, MAX_RECORD)?;
        let digest = format!("{:x}", Sha256::digest(&original_bytes));
        if derivation.get("original_sha256").and_then(Value::as_str) != Some(digest.as_str()) {
            return Err(
                "original release rig record raw SHA-256 differs from Brew derivation".into(),
            );
        }
        let original: Value = serde_json::from_slice(&original_bytes)
            .map_err(|e| format!("invalid original release rig record: {e}"))?;
        let original_metadata = original
            .as_object()
            .ok_or("invalid original release rig metadata")?;
        if original_metadata.contains_key("signing_derivation") {
            return Err("original release record may not itself be derived".into());
        }
        let derived_metadata = record.as_object().ok_or("invalid derived rig metadata")?;
        if derived_metadata.len() != original_metadata.len() + 1
            || original_metadata.iter().any(|(key, value)| {
                key != "deployed_sha256" && derived_metadata.get(key) != Some(value)
            })
        {
            return Err("Brew signing changed non-hash release metadata".into());
        }
        let original_hashes = original
            .get("deployed_sha256")
            .and_then(Value::as_object)
            .ok_or("original release lacks deployed hashes")?;
        let current_hashes = record
            .get("deployed_sha256")
            .and_then(Value::as_object)
            .ok_or("derived release lacks deployed hashes")?;
        if original_hashes.len() != current_hashes.len() {
            return Err("Brew signing changed the release resource closure".into());
        }
        for (name, digest) in current_hashes {
            let previous = original_hashes
                .get(name)
                .and_then(Value::as_str)
                .ok_or("Brew signing added an unapproved resource")?;
            if previous.len() != 64
                || !previous
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                return Err("original release resource digest is invalid".into());
            }
            if digest.as_str() != Some(previous)
                && !matches!(
                    name.as_str(),
                    "MacOS/desktop-pet"
                        | "MacOS/desktop-pet-update-coordinator"
                        | "MacOS/rig-decode-worker"
                        | "Frameworks/libherdr_rig.dylib"
                        | "_CodeSignature/CodeResources"
                )
            {
                return Err(format!("Brew signing changed resource payload: {name}"));
            }
        }
        Ok(())
    }
}

fn temporary_directory(parent: &Path, prefix: &str) -> Result<PathBuf, String> {
    let mut random = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut random))
        .map_err(|e| e.to_string())?;
    let path = parent.join(format!(
        "{prefix}-{}",
        random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .map_err(|e| e.to_string())?;
    physical_directory(&path, true)?;
    Ok(path)
}

fn entitlement_set(target: &Path, scratch: &Path) -> Result<Value, String> {
    let output = tool(
        "/usr/bin/codesign",
        &["-d", "--xml", "--entitlements", "-", path_str(target)?],
        16384,
        10,
    )?;
    if output.trim().is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    let start = output
        .find("<?xml")
        .or_else(|| output.find("<plist"))
        .ok_or("unreadable code entitlements")?;
    let path = scratch.join("entitlements.plist");
    atomic_owned_file(&path, output[start..].as_bytes())?;
    let json = tool(
        "/usr/bin/plutil",
        &["-convert", "json", "-o", "-", path_str(&path)?],
        16384,
        5,
    )?;
    serde_json::from_str(&json).map_err(|e| format!("invalid entitlement dictionary: {e}"))
}

/// Full real bundle validation, shared by discovery, installer, and standalone update staging.
/// Requires the external post-signing rig deployment record beside the bundle.
pub fn validate_staged_app(bundle: &Path) -> Result<ValidatedApp, String> {
    require_neutral()?;
    validate_neutral_bundle(bundle)
}

fn validate_neutral_bundle(bundle: &Path) -> Result<ValidatedApp, String> {
    validate_bundle_snapshot(bundle, None)
}

// Only immutable replacement evidence may supply a sidecar snapshot. This permits read-only
// verification at the crash boundary between the bundle exchange and its external metadata.
fn validate_bundle_snapshot(
    bundle: &Path,
    snapshot: Option<&[u8]>,
) -> Result<ValidatedApp, String> {
    if !cfg!(target_os = "macos") {
        return Err("public native installation requires macOS".into());
    }
    ensure_absolute(bundle)?;
    physical_directory(bundle, false)?;
    if bundle.file_name().is_none_or(|n| n != "DesktopPet.app") {
        return Err("expected neutral DesktopPet.app bundle".into());
    }
    let contents = bundle.join("Contents");
    physical_directory(&contents, false)?;
    let mut files = BTreeMap::new();
    files_under(&contents, Path::new(""), &mut files, &mut 0)?;
    let plist = contents.join("Info.plist");
    for (key, expected) in [
        ("CFBundleIdentifier", "io.github.hanbong5938.desktop-pet"),
        ("CFBundleExecutable", "desktop-pet"),
        ("CFBundlePackageType", "APPL"),
    ] {
        if tool(
            "/usr/bin/plutil",
            &["-extract", key, "raw", "-o", "-", path_str(&plist)?],
            4096,
            5,
        )?
        .trim()
            != expected
        {
            return Err(format!("bundle identity mismatch: {key}"));
        }
    }
    let version = tool(
        "/usr/bin/plutil",
        &[
            "-extract",
            "CFBundleShortVersionString",
            "raw",
            "-o",
            "-",
            path_str(&plist)?,
        ],
        4096,
        5,
    )?
    .trim()
    .to_owned();
    installed_version(&version)?;
    if tool(
        "/usr/bin/plutil",
        &[
            "-extract",
            "CFBundleVersion",
            "raw",
            "-o",
            "-",
            path_str(&plist)?,
        ],
        4096,
        5,
    )?
    .trim()
        != version
    {
        return Err("bundle build and release versions differ".into());
    }
    validate_deploy_record(bundle, &version, &files, snapshot)?;
    let image = executable_identity(&contents.join("MacOS/desktop-pet"))?;
    let helper = contents.join("MacOS/desktop-pet-update-coordinator");
    let worker = contents.join("MacOS/rig-decode-worker");
    let library = contents.join("Frameworks/libherdr_rig.dylib");
    for path in [&image.path, &helper, &worker, &library] {
        if tool("/usr/bin/lipo", &["-archs", path_str(path)?], 4096, 5)?.trim() != "arm64" {
            return Err("installation contains non-arm64-only native code".into());
        }
        tool(
            "/usr/bin/codesign",
            &["--verify", "--strict", path_str(path)?],
            4096,
            10,
        )?;
    }
    tool(
        "/usr/bin/codesign",
        &["--verify", "--deep", "--strict", path_str(bundle)?],
        4096,
        10,
    )?;
    if origin::command_output(&image.path, &["--version"], 4096, 5)?.trim()
        != format!("desktop-pet {version}")
    {
        return Err("bundle version differs from actual executable".into());
    }
    executable_identity(&helper)?;
    executable_identity(&worker)?;
    let app_capabilities = capabilities(&image.path)?;
    let helper_capabilities = capabilities(&helper)?;
    if app_capabilities != helper_capabilities {
        return Err("app and helper installation capabilities differ".into());
    }
    if app_capabilities.1.is_some() {
        let app_identity: Value = serde_json::from_str(&origin::command_output(
            &image.path,
            &["distribution-identity"],
            16384,
            5,
        )?)
        .map_err(|e| format!("invalid application build identity: {e}"))?;
        let helper_identity: Value = serde_json::from_str(&origin::command_output(
            &helper,
            &["distribution-identity"],
            16384,
            5,
        )?)
        .map_err(|e| format!("invalid helper build identity: {e}"))?;
        if app_identity != helper_identity
            || app_identity.get("distribution").and_then(Value::as_str) != Some("neutral")
            || app_identity.get("app_version").and_then(Value::as_str) != Some(version.as_str())
            || !app_identity
                .get("core_revision")
                .and_then(Value::as_str)
                .is_some_and(|revision| {
                    revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit())
                })
        {
            return Err(
                "installation client contract requires identical paired build identity".into(),
            );
        }
    }
    let load_commands = tool("/usr/bin/otool", &["-l", path_str(&helper)?], 65536, 5)?;
    if load_commands
        .lines()
        .any(|line| line.trim() == "cmd LC_RPATH")
    {
        return Err("coordinator contains loader search paths".into());
    }
    let dependencies = tool("/usr/bin/otool", &["-L", path_str(&helper)?], 16384, 5)?;
    let mut count = 0;
    for line in dependencies
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
    {
        let name = line
            .split_whitespace()
            .next()
            .ok_or("invalid coordinator dependency")?;
        if !(name.starts_with("/usr/lib/") || name.starts_with("/System/Library/")) {
            return Err("coordinator contains a non-system dependency".into());
        }
        count += 1;
    }
    if count == 0 {
        return Err("coordinator has no verifiable system dependency closure".into());
    }
    let manifest: Value = serde_json::from_slice(&owned_file(
        &contents.join("Resources/default/manifest.json"),
        MAX_RECORD,
    )?)
    .map_err(|e| format!("invalid default character manifest: {e}"))?;
    if manifest.get("id").and_then(Value::as_str) != Some("rubelia-at12-multi-pose") {
        return Err("bundled default is not Rubelia".into());
    }
    for name in [
        "Resources/LICENSE.txt",
        "Resources/rig/decoder.js",
        "Resources/rig/limits.json",
        "Resources/rig/NOTICE.txt",
        "Resources/rig/RigLimits.swift",
        "Resources/rig/RigLimits.rs",
        "Resources/rig-native-build.json",
    ] {
        if !files.contains_key(name) {
            return Err(format!("missing required rig resource: {name}"));
        }
    }
    let scratch = temporary_directory(Path::new("/private/tmp"), "desktop-pet-validation")?;
    let result = (|| {
        for path in [&image.path, &helper, &library, bundle] {
            if !entitlement_set(path, &scratch)?
                .as_object()
                .is_some_and(|m| m.is_empty())
            {
                return Err("unexpected application or coordinator entitlements".into());
            }
        }
        let worker_entitlements = entitlement_set(&worker, &scratch)?;
        if !worker_entitlements.as_object().is_some_and(|m| {
            m.len() == 1
                && m.get("com.apple.security.cs.allow-jit")
                    .and_then(Value::as_bool)
                    == Some(true)
        }) {
            return Err("decode worker must have only allow-jit=true".into());
        }
        let validation = origin::command_output(
            &image.path,
            &[
                "pack",
                "validate",
                "--path",
                path_str(&contents.join("Resources/default"))?,
                "--config-dir",
                path_str(&scratch.join("config"))?,
                "--state-dir",
                path_str(&scratch.join("state"))?,
            ],
            65536,
            30,
        )?;
        let parsed: Value = serde_json::from_str(&validation)
            .map_err(|e| format!("invalid native pack validation: {e}"))?;
        if parsed.get("backend").and_then(Value::as_str) != Some("rig") {
            return Err("Rubelia pack did not validate with the native rig renderer".into());
        }
        validate_deploy_record(bundle, &version, &files, snapshot)?;
        if executable_identity(&image.path)? != image {
            return Err("application changed during bundle validation".into());
        }
        Ok(ValidatedApp {
            bundle: bundle.to_path_buf(),
            image,
            version,
            installation_client_contract: app_capabilities.1,
        })
    })();
    let _ = fs::remove_dir_all(&scratch);
    result
}

/// Inspect an actual bundle executable, including manager/source proof; unsupported manual apps fail.
pub fn inspect(executable: &Path) -> Result<InstallationRecord, String> {
    require_neutral()?;
    #[cfg(not(desktop_pet_legacy))]
    {
        inspect_neutral(executable)
    }
    #[cfg(desktop_pet_legacy)]
    {
        let _ = executable;
        Err("legacy distribution cannot inspect public installations".into())
    }
}

#[cfg(not(desktop_pet_legacy))]
fn inspect_neutral(executable: &Path) -> Result<InstallationRecord, String> {
    ensure_absolute(executable)?;
    let canonical =
        fs::canonicalize(executable).map_err(|e| format!("{}: {e}", executable.display()))?;
    if canonical.file_name().is_none_or(|n| n != "desktop-pet") {
        return Err("not a desktop-pet bundle executable".into());
    }
    let bundle = canonical
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or("invalid executable bundle path")?;
    if canonical != bundle.join("Contents/MacOS/desktop-pet") {
        return Err("invalid executable layout".into());
    }
    let app = validate_staged_app(bundle)?;
    let actual = origin::origin_for_executable(&canonical, Some(&app))?;
    let kind = match &actual {
        InstallOrigin::Homebrew { .. } => InstallationKind::Homebrew,
        InstallOrigin::Source { .. } => InstallationKind::Source,
        InstallOrigin::Standalone { bundle: selected } if selected == &app.bundle => {
            InstallationKind::Standalone
        }
        _ => return Err("executable has no supported verified installation owner".into()),
    };
    if executable_identity(&canonical)? != app.image {
        return Err("installation changed during origin verification".into());
    }
    Ok(InstallationRecord {
        version: 1,
        executable: canonical,
        kind,
        app_version: app.version,
        protocol: 4,
        observation_mode: 2,
        integration_api: 1,
        origin: actual,
        image: app.image,
    })
}

fn path_candidates(name: &str) -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .filter(|p| p.is_absolute())
                .map(|p| p.join(name))
                .collect()
        })
        .unwrap_or_default()
}

/// Exact priority: explicit, private locator, PATH image, captured Brew prefix, OS-home apps.
/// Invalid explicit/locator selections and ambiguous standalone fallback owners fail closed.
pub fn discover(explicit: Option<&Path>) -> Result<Option<InstallationRecord>, String> {
    require_neutral()?;
    #[cfg(not(desktop_pet_legacy))]
    {
        discover_neutral(explicit)
    }
    #[cfg(desktop_pet_legacy)]
    {
        let _ = explicit;
        Err("legacy distribution cannot discover public installations".into())
    }
}

#[cfg(not(desktop_pet_legacy))]
fn discover_neutral(explicit: Option<&Path>) -> Result<Option<InstallationRecord>, String> {
    if let Some(path) = explicit {
        return inspect_neutral(path).map(Some);
    }
    if let Some(locator) = read_locator()? {
        if locator.kind == InstallationKind::Standalone {
            let bundle = locator
                .executable
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .ok_or("invalid standalone locator bundle")?;
            validate_standalone_owner(bundle)?;
            if locator.executable != bundle.join("Contents/MacOS/desktop-pet") {
                return Err("invalid standalone locator executable layout".into());
            }
        }
        let current = inspect_neutral(&locator.executable)
            .map_err(|e| format!("invalid installation locator target: {e}"))?;
        if current.kind != locator.kind {
            return Err("locator kind differs from actual installation owner".into());
        }
        return Ok(Some(current));
    }
    for candidate in path_candidates("desktop-pet") {
        if exists(&candidate)? {
            if let Ok(record) = inspect_neutral(&candidate) {
                return Ok(Some(record));
            }
        }
    }
    let mut managers = path_candidates("brew");
    managers.extend([
        PathBuf::from("/opt/homebrew/bin/brew"),
        PathBuf::from("/usr/local/bin/brew"),
    ]);
    let mut captured = Vec::new();
    for manager in managers {
        if !exists(&manager)? {
            continue;
        }
        // Capture the real manager before invoking it. A PATH wrapper cannot supply its own prefix.
        let Some(prefix) = manager
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == "bin"))
            .and_then(Path::parent)
        else {
            continue;
        };
        if ![Path::new("/opt/homebrew"), Path::new("/usr/local")].contains(&prefix) {
            continue;
        }
        let Ok(physical) = fs::canonicalize(&manager) else {
            continue;
        };
        let expected = prefix.join("Homebrew/bin/brew");
        if physical != manager && !fs::canonicalize(&expected).is_ok_and(|path| path == physical) {
            continue;
        }
        if captured.contains(&physical) {
            continue;
        }
        captured.push(physical.clone());
        let Ok(m) = fs::symlink_metadata(&physical) else {
            continue;
        };
        if !m.is_file()
            || m.uid() != unsafe { libc::geteuid() }
            || m.permissions().mode() & 0o022 != 0
        {
            continue;
        }
        let Ok(output) = origin::command_output(&physical, &["--prefix", "desktop-pet"], 4096, 15)
        else {
            continue;
        };
        let selected = PathBuf::from(output.trim());
        if ensure_absolute(&selected).is_err() {
            continue;
        }
        let candidate = selected.join("libexec/DesktopPet.app/Contents/MacOS/desktop-pet");
        if let Ok(record) = inspect_neutral(&candidate) {
            if matches!(&record.origin, InstallOrigin::Homebrew { brew, prefix: actual, .. }
                if fs::canonicalize(brew).is_ok_and(|path| path == physical) && actual == prefix)
            {
                return Ok(Some(record));
            }
        }
    }
    if let Some(bundle) = standalone_candidate_in(&os_home()?)? {
        return inspect_neutral(&bundle.join("Contents/MacOS/desktop-pet")).map(Some);
    }
    Ok(None)
}

pub fn install_request(path: &Path) -> Result<InstallationRecord, String> {
    let request: InstallRequest = serde_json::from_slice(&owned_private_file(path, 16384)?)
        .map_err(|e| format!("invalid private install request: {e}"))?;
    install(&request)
}

#[cfg(not(desktop_pet_legacy))]
fn desired_origin(
    request: &InstallRequest,
    current: Option<&InstallationRecord>,
) -> Result<InstallOrigin, String> {
    if request.version != 1 {
        return Err("unsupported install request version".into());
    }
    ensure_absolute(&request.staged_app)?;
    match (&request.kind, &request.source_root) {
        (InstallationKind::Standalone, None) => standalone_origin_in(&os_home()?, current),
        (InstallationKind::Source, Some(root)) => {
            origin::validate_source_root(root)?;
            if !request.staged_app.starts_with(root.join("dist")) { return Err("source stage must remain inside its exact core root/dist".into()); }
            Ok(InstallOrigin::Source { root: root.clone() })
        },
        _ => Err("install request requires standalone without source_root or source with exact source_root".into()),
    }
}

fn bundle_for_origin(origin: &InstallOrigin) -> Result<PathBuf, String> {
    match origin {
        InstallOrigin::Standalone { bundle } => {
            validate_standalone_owner(bundle)?;
            Ok(bundle.clone())
        }
        InstallOrigin::Source { root } => Ok(root.join("dist/DesktopPet.app")),
        _ => Err("only source or standalone bundles can be atomically installed".into()),
    }
}

// The bridge uses a neutral helper's verified owner proof; it must still compare the staged
// revision without applying its own legacy package identity to the neutral source checkout.
fn check_source_revision(app: &ValidatedApp, origin: &InstallOrigin) -> Result<(), String> {
    if let InstallOrigin::Source { root } = origin {
        #[cfg(not(desktop_pet_legacy))]
        origin::validate_source_root(root)?;
        #[cfg(desktop_pet_legacy)]
        physical_directory(root, false)?;
        let sidecar: Value =
            serde_json::from_slice(&owned_file(&sidecar_path(&app.bundle), MAX_RECORD)?)
                .map_err(|e| e.to_string())?;
        let revision = tool(
            "/usr/bin/git",
            &["-C", path_str(root)?, "rev-parse", "HEAD"],
            4096,
            5,
        )?;
        if sidecar.get("core_revision").and_then(Value::as_str) != Some(revision.trim()) {
            return Err(
                "source stage was not built from the selected exact core checkout revision".into(),
            );
        }
    }
    Ok(())
}

/// Install/reuse only after both global namespaces and all affected physical roots are fenced.
/// A healthy different owner is reusable at the same/higher version, never overwritten or taken over.
pub fn install(request: &InstallRequest) -> Result<InstallationRecord, String> {
    require_neutral()?;
    #[cfg(not(desktop_pet_legacy))]
    {
        install_neutral(request)
    }
    #[cfg(desktop_pet_legacy)]
    {
        let _ = request;
        Err("legacy distribution cannot install public applications".into())
    }
}

#[cfg(not(desktop_pet_legacy))]
fn install_neutral(request: &InstallRequest) -> Result<InstallationRecord, String> {
    let initial = desired_origin(request, None)?;
    let staged = validate_staged_app(&request.staged_app)?;
    check_source_revision(&staged, &initial)?;
    let mut admission = protocol::admit_installation_reuse(|| {
        let current = discover(None)?;
        let desired = desired_origin(request, current.as_ref())?;
        let mut roots = vec![physical_install_root(&desired)?.to_path_buf()];
        if let Some(current) = current {
            roots.push(physical_install_root(&current.origin)?.to_path_buf());
        }
        Ok(roots)
    })?;
    let mut _lock = install_lock()?;
    let mut current = discover(None)?;
    if let Some(selected) = &current {
        let root = physical_install_root(&selected.origin)?;
        if !admission.holds_root(root) {
            // A selection outside the initial lockset needs one root refresh, not an install retry.
            // Keep both gates, but release the install lock before reacquiring sorted root locks.
            drop(_lock);
            admission.fence_additional_root(root)?;
            _lock = install_lock()?;
            current = discover(None)?;
        }
    }
    // Selection is authoritative only after both the admission fences and installation lock.
    let desired = desired_origin(request, current.as_ref())?;
    if !admission.holds_root(physical_install_root(&desired)?) {
        return Err("desired installation owner changed outside the fenced roots".into());
    }
    if let Some(current) = &current {
        if !admission.holds_root(physical_install_root(&current.origin)?) {
            return Err("installation selection changed outside the fenced roots".into());
        }
        if installed_version(&current.app_version)? >= installed_version(&staged.version)? {
            // A new locator may adopt the verified selection; an existing owner is never rewritten.
            if read_locator()?.is_none() {
                write_locator(current)?;
            }
            return Ok(current.clone());
        }
        if current.origin != desired {
            return Err("healthy locator belongs to another installation owner; explicit manager transition is required".into());
        }
    }
    let target = bundle_for_origin(&desired)?;
    if exists(&target)? {
        let target_record = inspect(&target.join("Contents/MacOS/desktop-pet"))?;
        if target_record.origin != desired {
            return Err("target bundle has a different actual owner".into());
        }
        if installed_version(&target_record.app_version)? >= installed_version(&staged.version)? {
            if current.is_none() {
                write_locator(&target_record)?;
            }
            return Ok(target_record);
        }
    }
    // Healthy reuse never replaces an image and remains safe with a newly attached daemon.
    // Only the mutation path requires all affected installations/profiles to be inactive.
    admission.installation_inactive()?;
    if staged.bundle == target {
        let record = inspect(&staged.image.path)?;
        if record.origin != desired {
            return Err("source stage has changed ownership".into());
        }
        write_locator(&record)?;
        return Ok(record);
    }
    let staged = validate_staged_app(&request.staged_app)?;
    check_source_revision(&staged, &desired)?;
    replace_bundle(&staged, &desired, current.as_ref(), true)
}

fn atomic_owned_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > MAX_RECORD {
        return Err("installation metadata exceeds bound".into());
    }
    physical_directory(path.parent().ok_or("metadata has no parent")?, false)?;
    if exists(path)? {
        owned_file(path, MAX_RECORD)?;
    }
    let temporary = temporary_directory(
        path.parent().ok_or("metadata has no parent")?,
        ".desktop-pet-record",
    )?;
    let temp = temporary.join("record");
    let result = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        f.write_all(bytes)
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(&temp, path).map_err(|e| e.to_string())?;
        File::open(path.parent().ok_or("missing metadata parent")?)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    })();
    let _ = fs::remove_dir_all(&temporary);
    result
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), String> {
    create_directory(target, false)?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        let m = fs::symlink_metadata(&from).map_err(|e| e.to_string())?;
        if m.is_dir() && !m.file_type().is_symlink() {
            copy_tree(&from, &to)?;
        } else if m.is_file() && !m.file_type().is_symlink() && m.nlink() == 1 {
            let input = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&from)
                .map_err(|e| e.to_string())?;
            let opened = input.metadata().map_err(|e| e.to_string())?;
            if opened.dev() != m.dev() || opened.ino() != m.ino() {
                return Err("bundle changed while staging".into());
            }
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(m.permissions().mode() & 0o777)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&to)
                .map_err(|e| e.to_string())?;
            std::io::copy(&mut input.take(MAX_FILE + 1), &mut output).map_err(|e| e.to_string())?;
            output.sync_all().map_err(|e| e.to_string())?;
        } else {
            return Err("unsupported bundle resource while staging".into());
        }
    }
    File::open(target)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn rename_bundle(left: &Path, right: &Path, swap: bool) -> Result<(), String> {
    unsafe extern "C" {
        fn renamex_np(
            from: *const libc::c_char,
            to: *const libc::c_char,
            flags: libc::c_uint,
        ) -> libc::c_int;
    }
    let left = CString::new(left.as_os_str().as_bytes()).map_err(|_| "invalid bundle path")?;
    let right = CString::new(right.as_os_str().as_bytes()).map_err(|_| "invalid bundle path")?;
    if unsafe { renamex_np(left.as_ptr(), right.as_ptr(), if swap { 2 } else { 4 }) } != 0 {
        return Err(format!(
            "atomic bundle replacement: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn rename_bundle(_: &Path, _: &Path, _: bool) -> Result<(), String> {
    Err("atomic native installation requires macOS".into())
}

fn restore_sidecar(path: &Path, previous: Option<&[u8]>) -> Result<(), String> {
    if let Some(bytes) = previous {
        atomic_owned_file(path, bytes)
    } else {
        if exists(path)? {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        File::open(path.parent().ok_or("sidecar has no parent")?)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    }
}

/// Copy a fully verified bundle and its external deployment record into an absent, private
/// operation destination. No ownership acknowledgement may precede successful return.
pub fn copy_standalone_install_stage(
    source: &ValidatedApp,
    target: &Path,
) -> Result<ValidatedApp, String> {
    let actual = validate_staged_app(&source.bundle)?;
    if &actual != source {
        return Err("stage changed before durable copy".into());
    }
    physical_directory(target.parent().ok_or("stage has no parent")?, true)?;
    if exists(target)? || exists(&sidecar_path(target))? {
        return Err("durable stage destination already exists".into());
    }
    let sidecar = owned_file(&sidecar_path(&source.bundle), MAX_RECORD)?;
    copy_tree(&source.bundle, target)?;
    atomic_owned_file(&sidecar_path(target), &sidecar)?;
    let copy = validate_staged_app(target)?;
    if copy.image.sha256 != source.image.sha256
        || copy.version != source.version
        || copy.installation_client_contract != source.installation_client_contract
    {
        return Err("copied stage differs from approved app".into());
    }
    sync_directory(target.parent().ok_or("stage has no parent")?)?;
    Ok(copy)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReplacementDisposition {
    RequireInactive,
    RetainCompatibleClients,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSnapshot {
    device: u64,
    inode: u64,
    sha256: String,
}

impl FileSnapshot {
    fn capture(path: &Path) -> Result<Self, String> {
        let before = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if !before.is_file() || before.file_type().is_symlink() || before.nlink() != 1 {
            return Err("replacement executable is not a unique regular file".into());
        }
        let image = executable_identity(path)?;
        let after = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if (
            before.dev(),
            before.ino(),
            before.len(),
            before.mtime(),
            before.mtime_nsec(),
        ) != (
            after.dev(),
            after.ino(),
            after.len(),
            after.mtime(),
            after.mtime_nsec(),
        ) {
            return Err("replacement executable changed during snapshot".into());
        }
        Ok(Self {
            device: after.dev(),
            inode: after.ino(),
            sha256: image.sha256,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplacementIntent {
    version: u32,
    operation_id: String,
    disposition: ReplacementDisposition,
    #[serde(with = "crate::migration::neutral_origin_codec")]
    origin: InstallOrigin,
    original: Option<InstallationRecord>,
    original_file: Option<FileSnapshot>,
    candidate_file: FileSnapshot,
    candidate_version: String,
    work: PathBuf,
    work_device: u64,
    work_inode: u64,
    target: PathBuf,
    prepared: PathBuf,
    original_sidecar: Option<Vec<u8>>,
    candidate_sidecar: Vec<u8>,
    original_sidecar_sha256: Option<String>,
    candidate_sidecar_sha256: String,
    locator: PathBuf,
    original_locator: Option<Vec<u8>>,
    next_locator: Option<Vec<u8>>,
    update_locator: bool,
}

/// Successful retained replacement. The directory is deliberately never retired automatically:
/// process disappearance alone does not exclude a pending exec or an unknown resource mapping.
#[derive(Clone, Debug)]
pub struct RetainedReplacement {
    pub installation: InstallationRecord,
    pub retained_bundle: PathBuf,
    pub intent_path: PathBuf,
}

/// Read-only recovery evidence. Candidate is the actual prepared inode, wherever the exchange
/// left it. `installation` is present only for a fully committed disk/sidecar/locator outcome.
#[derive(Clone, Debug)]
pub struct RecoveredReplacement {
    pub original: ValidatedApp,
    pub candidate: ValidatedApp,
    pub installation: Option<InstallationRecord>,
    pub retained_bundle: Option<PathBuf>,
    original_scope: RecoveredScopeImage,
    candidate_scope: RecoveredScopeImage,
}

#[derive(Clone, Debug)]
pub(crate) struct RecoveredScopeImage {
    app: ValidatedApp,
    file: FileSnapshot,
    sidecar_sha256: String,
}

impl RecoveredScopeImage {
    pub(crate) fn app(&self) -> &ValidatedApp {
        &self.app
    }
    pub(crate) fn sidecar_sha256(&self) -> &str {
        &self.sidecar_sha256
    }
    pub(crate) fn file_identity(&self) -> (u64, u64) {
        (self.file.device, self.file.inode)
    }
}

impl RecoveredReplacement {
    pub(crate) fn scope_images(&self) -> (&RecoveredScopeImage, &RecoveredScopeImage) {
        (&self.original_scope, &self.candidate_scope)
    }
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn optional_bytes(path: &Path, private: bool) -> Result<Option<Vec<u8>>, String> {
    if !exists(path)? {
        return Ok(None);
    }
    if private {
        owned_private_file(path, MAX_RECORD).map(Some)
    } else {
        owned_file(path, MAX_RECORD).map(Some)
    }
}

fn check_metadata_boundary(intent: &ReplacementIntent) -> Result<(), String> {
    let sidecar = optional_bytes(&sidecar_path(&intent.target), false)?;
    if sidecar != intent.original_sidecar
        && sidecar.as_deref() != Some(intent.candidate_sidecar.as_slice())
    {
        return Err("replacement target sidecar differs from both immutable snapshots".into());
    }
    let prepared_sidecar = optional_bytes(&sidecar_path(&intent.prepared), false)?;
    if prepared_sidecar != intent.original_sidecar
        && prepared_sidecar.as_deref() != Some(intent.candidate_sidecar.as_slice())
    {
        return Err("replacement retained sidecar differs from both immutable snapshots".into());
    }
    let locator = optional_bytes(&intent.locator, true)?;
    if locator != intent.original_locator && locator != intent.next_locator {
        return Err("replacement locator changed outside the operation".into());
    }
    Ok(())
}

fn load_replacement_intent(path: &Path, id: &str) -> Result<ReplacementIntent, String> {
    protocol::validate_operation_id(id)?;
    let intent: ReplacementIntent = serde_json::from_slice(&owned_private_file(path, MAX_RECORD)?)
        .map_err(|e| format!("invalid immutable replacement intent: {e}"))?;
    if intent.version != 1
        || intent.operation_id != id
        || intent.target != bundle_for_origin(&intent.origin)?
        || intent.prepared != intent.work.join("DesktopPet.app")
        || intent.work.parent() != intent.target.parent()
        || !intent
            .work
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(".desktop-pet-install-"))
        || intent.locator != locator_path()?
        || intent.original_sidecar.as_deref().map(digest_bytes) != intent.original_sidecar_sha256
        || digest_bytes(&intent.candidate_sidecar) != intent.candidate_sidecar_sha256
    {
        return Err("replacement intent identity, paths, or snapshots are invalid".into());
    }
    physical_directory(&intent.work, true)?;
    let work = fs::symlink_metadata(&intent.work).map_err(|e| e.to_string())?;
    if (work.dev(), work.ino()) != (intent.work_device, intent.work_inode) {
        return Err("replacement work directory was substituted".into());
    }
    let expected_locator = if intent.update_locator {
        Some(
            serde_json::to_vec(&Locator {
                version: 1,
                executable: intent.target.join("Contents/MacOS/desktop-pet"),
                kind: match &intent.origin {
                    InstallOrigin::Standalone { .. } => InstallationKind::Standalone,
                    InstallOrigin::Source { .. } => InstallationKind::Source,
                    _ => return Err("unsupported replacement owner".into()),
                },
            })
            .map_err(|e| e.to_string())?,
        )
    } else {
        intent.original_locator.clone()
    };
    if expected_locator != intent.next_locator {
        return Err("replacement intent locator does not describe the approved owner".into());
    }
    check_metadata_boundary(&intent)?;
    Ok(intent)
}

fn replacement_swapped(intent: &ReplacementIntent) -> Result<bool, String> {
    let canonical = FileSnapshot::capture(&intent.target.join("Contents/MacOS/desktop-pet"))?;
    let prepared = FileSnapshot::capture(&intent.prepared.join("Contents/MacOS/desktop-pet"))?;
    match intent.original_file.as_ref() {
        Some(original) if &canonical == original && prepared == intent.candidate_file => Ok(false),
        Some(original) if canonical == intent.candidate_file && &prepared == original => Ok(true),
        _ => Err("replacement images do not match either exact atomic exchange outcome".into()),
    }
}

/// Reopen both signed closures using their immutable external records, never reconstructing old
/// identity from new canonical bytes. This reader does not mutate disk or authorize a restart.
pub fn recover_replacement(
    intent_path: &Path,
    operation_id: &str,
    original: &InstallationRecord,
) -> Result<RecoveredReplacement, String> {
    let intent = load_replacement_intent(intent_path, operation_id)?;
    if intent.disposition != ReplacementDisposition::RetainCompatibleClients
        || intent.original.as_ref() != Some(original)
        || intent.origin != original.origin
        || intent.original_file.as_ref().map(|f| &f.sha256) != Some(&original.image.sha256)
    {
        return Err("replacement intent differs from the original operation approval".into());
    }
    let swapped = replacement_swapped(&intent)?;
    let (old_path, new_path) = if swapped {
        (&intent.prepared, &intent.target)
    } else {
        (&intent.target, &intent.prepared)
    };
    let old = validate_bundle_snapshot(
        old_path,
        Some(
            intent
                .original_sidecar
                .as_deref()
                .ok_or("missing original sidecar snapshot")?,
        ),
    )?;
    let candidate = validate_bundle_snapshot(new_path, Some(&intent.candidate_sidecar))?;
    if old.image.sha256 != original.image.sha256
        || old.version != original.app_version
        || candidate.image.sha256 != intent.candidate_file.sha256
        || candidate.version != intent.candidate_version
    {
        return Err("replacement signed closures differ from immutable intent".into());
    }
    // Re-sample after executing validation queries; no hash-only inode reconstruction is allowed.
    if replacement_swapped(&intent)? != swapped {
        return Err("replacement changed during recovery inspection".into());
    }
    let complete = swapped
        && optional_bytes(&sidecar_path(&intent.target), false)?.as_deref()
            == Some(intent.candidate_sidecar.as_slice())
        && optional_bytes(&sidecar_path(&intent.prepared), false)? == intent.original_sidecar
        && optional_bytes(&intent.locator, true)? == intent.next_locator;
    let installation = if complete {
        let installed = inspect(&intent.target.join("Contents/MacOS/desktop-pet"))?;
        if installed.origin != intent.origin || installed.image != candidate.image {
            return Err("replacement actual owner differs from immutable intent".into());
        }
        Some(installed)
    } else {
        None
    };
    Ok(RecoveredReplacement {
        original: old.clone(),
        candidate: candidate.clone(),
        installation,
        retained_bundle: swapped.then(|| intent.prepared.clone()),
        original_scope: RecoveredScopeImage {
            app: old,
            file: intent
                .original_file
                .clone()
                .ok_or("missing original inode snapshot")?,
            sidecar_sha256: intent
                .original_sidecar_sha256
                .clone()
                .ok_or("missing original sidecar hash")?,
        },
        candidate_scope: RecoveredScopeImage {
            app: candidate,
            file: intent.candidate_file.clone(),
            sidecar_sha256: intent.candidate_sidecar_sha256.clone(),
        },
    })
}

fn reconcile_replacement_metadata(intent: &ReplacementIntent, swapped: bool) -> Result<(), String> {
    check_metadata_boundary(intent)?;
    if replacement_swapped(intent)? != swapped {
        return Err("replacement images changed before metadata reconciliation".into());
    }
    let (canonical, retained, locator) = if swapped {
        (
            Some(intent.candidate_sidecar.as_slice()),
            intent.original_sidecar.as_deref(),
            intent.next_locator.as_deref(),
        )
    } else {
        (
            intent.original_sidecar.as_deref(),
            Some(intent.candidate_sidecar.as_slice()),
            intent.original_locator.as_deref(),
        )
    };
    restore_sidecar(&sidecar_path(&intent.prepared), retained)?;
    restore_sidecar(&sidecar_path(&intent.target), canonical)?;
    restore_sidecar(&intent.locator, locator)?;
    if replacement_swapped(intent)? != swapped
        || optional_bytes(&sidecar_path(&intent.target), false)?.as_deref() != canonical
        || optional_bytes(&sidecar_path(&intent.prepared), false)?.as_deref() != retained
        || optional_bytes(&intent.locator, true)?.as_deref() != locator
    {
        return Err("replacement reconciliation did not establish its exact disk outcome".into());
    }
    Ok(())
}

/// Reconcile only metadata at a proven atomic exchange boundary. Caller owns the operation lease,
/// reservation and Stop fence. No exchange, Stop, token issuance or process launch is replayed.
pub fn reconcile_replacement(
    intent_path: &Path,
    operation_id: &str,
    original: &InstallationRecord,
    profile: &crate::native_runtime::ProfilePaths,
    scope: &mut crate::native_runtime::NativeInstallScope,
) -> Result<RecoveredReplacement, String> {
    let _lock = install_lock()?;
    let proof = recover_replacement(intent_path, operation_id, original)?;
    *scope = crate::native_runtime::recover_install_scope(&proof, profile)?;
    scope.admit(None)?;
    let intent = load_replacement_intent(intent_path, operation_id)?;
    let swapped = replacement_swapped(&intent)?;
    if swapped != proof.retained_bundle.is_some() {
        return Err("replacement changed before metadata reconciliation".into());
    }
    reconcile_replacement_metadata(&intent, swapped)?;
    recover_replacement(intent_path, operation_id, original)
}

pub fn validate_standalone_replacement(
    plan: &crate::standalone_install::StandaloneInstallPlan,
) -> Result<RetainedReplacement, String> {
    plan.validate()?;
    let path =
        crate::standalone_install::replacement_path(&plan.profile.state_dir, &plan.operation_id)?;
    let proof = recover_replacement(&path, &plan.operation_id, &plan.original)?;
    if proof.candidate.image.sha256 != plan.candidate_image.sha256
        || proof.candidate.version != plan.candidate_version
    {
        return Err("replacement candidate differs from standalone approval".into());
    }
    Ok(RetainedReplacement {
        installation: proof
            .installation
            .ok_or("standalone replacement disk outcome is incomplete")?,
        retained_bundle: proof
            .retained_bundle
            .ok_or("standalone replacement has not exchanged")?,
        intent_path: path,
    })
}

pub fn recover_standalone_scope(
    plan: &UpdatePlan,
) -> Result<crate::native_runtime::NativeInstallScope, String> {
    let path = protocol::private_updates(&plan.context.state_dir)?
        .join(format!("{}.bundle-replacement.json", plan.operation_id));
    let profile = crate::native_runtime::ProfilePaths {
        config_dir: plan.context.config_dir.clone(),
        state_dir: plan.context.state_dir.clone(),
    };
    if !exists(&path)? {
        let app = validate_staged_app(&bundle_for_origin(&plan.origin)?)?;
        if app.image != plan.running {
            return Err("original image changed without replacement intent".into());
        }
        return crate::native_runtime::NativeInstallScope::capture(&app, &profile);
    }
    let intent = load_replacement_intent(&path, &plan.operation_id)?;
    let original = intent
        .original
        .as_ref()
        .ok_or("replacement intent has no original")?;
    if original.image != plan.running || original.origin != plan.origin {
        return Err("replacement intent differs from update approval".into());
    }
    let proof = recover_replacement(&path, &plan.operation_id, original)?;
    crate::native_runtime::recover_install_scope(&proof, &profile)
}

/// Complete only interrupted metadata for a normal reserved standalone update.
/// A fully committed replacement is observation-only, including a live candidate.
pub fn reconcile_standalone_release(
    plan: &UpdatePlan,
    scope: &mut crate::native_runtime::NativeInstallScope,
) -> Result<(), String> {
    let path = protocol::private_updates(&plan.context.state_dir)?
        .join(format!("{}.bundle-replacement.json", plan.operation_id));
    if !exists(&path)? {
        return Ok(());
    }
    let intent = load_replacement_intent(&path, &plan.operation_id)?;
    let original = intent
        .original
        .as_ref()
        .ok_or("replacement intent has no original")?;
    if original.image != plan.running || original.origin != plan.origin {
        return Err("replacement intent differs from update approval".into());
    }
    let proof = recover_replacement(&path, &plan.operation_id, original)?;
    if proof.installation.is_none() {
        let profile = crate::native_runtime::ProfilePaths {
            config_dir: plan.context.config_dir.clone(),
            state_dir: plan.context.state_dir.clone(),
        };
        if !crate::native_runtime::profile_quiescent(&profile)? {
            return Err(
                "replacement metadata recovery requires finalized original lifecycle".into(),
            );
        }
        reconcile_replacement(&path, &plan.operation_id, original, &profile, scope)?;
    }
    Ok(())
}

pub fn replace_standalone_coordinated(
    staged: &ValidatedApp,
    current: &InstallationRecord,
    scope: &mut crate::native_runtime::NativeInstallScope,
    operation_id: &str,
    intent_path: &Path,
) -> Result<RetainedReplacement, String> {
    protocol::validate_operation_id(operation_id)?;
    if current.kind != InstallationKind::Standalone
        || !matches!(current.origin, InstallOrigin::Standalone { .. })
    {
        return Err("coordinated replacement requires the same standalone owner".into());
    }
    let _lock = install_lock()?;
    bundle_for_origin(&current.origin)?;
    let selected = discover(None)?;
    if selected
        .as_ref()
        .is_some_and(|selected| selected != current)
    {
        return Err("standalone replacement may not take over another locator owner".into());
    }
    let (installation, retained_bundle) = replace_bundle_inner(
        staged,
        &current.origin,
        Some(current),
        selected.is_some(),
        ReplacementDisposition::RetainCompatibleClients,
        Some(scope),
        Some((operation_id, intent_path)),
    )?;
    Ok(RetainedReplacement {
        installation,
        retained_bundle: retained_bundle.ok_or("retained replacement lost original")?,
        intent_path: intent_path.to_path_buf(),
    })
}

fn replace_bundle(
    staged: &ValidatedApp,
    desired: &InstallOrigin,
    current: Option<&InstallationRecord>,
    update_locator: bool,
) -> Result<InstallationRecord, String> {
    replace_bundle_inner(
        staged,
        desired,
        current,
        update_locator,
        ReplacementDisposition::RequireInactive,
        None,
        None,
    )
    .map(|(record, _)| record)
}

fn replace_bundle_inner(
    staged: &ValidatedApp,
    desired: &InstallOrigin,
    current: Option<&InstallationRecord>,
    update_locator: bool,
    disposition: ReplacementDisposition,
    mut scope: Option<&mut crate::native_runtime::NativeInstallScope>,
    operation: Option<(&str, &Path)>,
) -> Result<(InstallationRecord, Option<PathBuf>), String> {
    let target = bundle_for_origin(desired)?;
    let parent = target.parent().ok_or("bundle has no parent")?;
    if matches!(desired, InstallOrigin::Standalone { .. }) {
        let directory = standalone_owner_parent_in(&os_home()?, &target, true)?
            .ok_or("missing standalone installation directory")?;
        if directory != parent {
            return Err("standalone installation parent differs from selected owner".into());
        }
    } else {
        create_directory(parent, false)?;
    }
    let work = temporary_directory(parent, ".desktop-pet-install")?;
    let prepared = work.join("DesktopPet.app");
    let copy = copy_standalone_install_stage(staged, &prepared)?;
    match disposition {
        ReplacementDisposition::RetainCompatibleClients => {
            let scope = scope
                .as_deref_mut()
                .ok_or("coordinated replacement has no pinned scope")?;
            scope.pin_prepared_candidate(&copy)?;
            scope.admit(None)?;
            if current.is_none() || operation.is_none() {
                return Err(
                    "retained replacement requires an original and operation evidence".into(),
                );
            }
        }
        ReplacementDisposition::RequireInactive => {
            crate::processes::installation_roots_inactive(&[
                physical_install_root(desired)?.to_path_buf()
            ])?;
        }
    }
    let actual = if exists(&target)? {
        let actual = inspect(&target.join("Contents/MacOS/desktop-pet"))?;
        if &actual.origin != desired || current.is_some_and(|c| c != &actual) {
            return Err("target image or manager changed before atomic replacement".into());
        }
        Some(actual)
    } else {
        if current.is_some() {
            return Err("current bundle disappeared before atomic replacement".into());
        }
        None
    };
    let original_sidecar = optional_bytes(&sidecar_path(&target), false)?;
    let candidate_sidecar = owned_file(&sidecar_path(&prepared), MAX_RECORD)?;
    let locator = locator_path()?;
    let original_locator = optional_bytes(&locator, true)?;
    let kind = match desired {
        InstallOrigin::Standalone { .. } => InstallationKind::Standalone,
        InstallOrigin::Source { .. } => InstallationKind::Source,
        _ => return Err("unsupported atomic replacement owner".into()),
    };
    let next_locator = if update_locator {
        Some(
            serde_json::to_vec(&Locator {
                version: 1,
                executable: target.join("Contents/MacOS/desktop-pet"),
                kind,
            })
            .map_err(|e| e.to_string())?,
        )
    } else {
        original_locator.clone()
    };
    let work_meta = fs::symlink_metadata(&work).map_err(|e| e.to_string())?;
    let fallback_intent = work.join("replacement.json");
    let (id, intent_path) = operation.unwrap_or((
        work.file_name()
            .and_then(|s| s.to_str())
            .ok_or("invalid work path")?,
        fallback_intent.as_path(),
    ));
    let intent = ReplacementIntent {
        version: 1,
        operation_id: id.to_owned(),
        disposition,
        origin: desired.clone(),
        original_file: actual
            .as_ref()
            .map(|r| FileSnapshot::capture(&r.executable))
            .transpose()?,
        original: actual.clone(),
        candidate_file: FileSnapshot::capture(&copy.image.path)?,
        candidate_version: copy.version.clone(),
        work: work.clone(),
        work_device: work_meta.dev(),
        work_inode: work_meta.ino(),
        target: target.clone(),
        prepared: prepared.clone(),
        original_sidecar_sha256: original_sidecar.as_deref().map(digest_bytes),
        candidate_sidecar_sha256: digest_bytes(&candidate_sidecar),
        original_sidecar,
        candidate_sidecar,
        locator,
        original_locator,
        next_locator,
        update_locator,
    };
    // The immutable write is durable before the first external sidecar, locator or bundle mutation.
    protocol::write_private_new(
        intent_path,
        &serde_json::to_vec(&intent).map_err(|e| e.to_string())?,
        0o600,
    )?;
    let committed: Result<InstallationRecord, String> = (|| {
        // Recheck every captured input after durable intent publication.
        if optional_bytes(&sidecar_path(&target), false)? != intent.original_sidecar
            || optional_bytes(&intent.locator, true)? != intent.original_locator
            || FileSnapshot::capture(&copy.image.path)? != intent.candidate_file
            || actual
                .as_ref()
                .map(|r| FileSnapshot::capture(&r.executable))
                .transpose()?
                != intent.original_file
        {
            return Err("replacement input changed after durable intent".into());
        }
        match disposition {
            ReplacementDisposition::RetainCompatibleClients => {
                scope
                    .as_deref_mut()
                    .ok_or("missing retained replacement scope")?
                    .admit(None)?;
            }
            ReplacementDisposition::RequireInactive => {
                crate::processes::installation_roots_inactive(&[
                    physical_install_root(desired)?.to_path_buf()
                ])?;
            }
        }
        atomic_owned_file(&sidecar_path(&target), &intent.candidate_sidecar)?;
        rename_bundle(&prepared, &target, actual.is_some())?;
        sync_directory(parent)?;
        sync_directory(&work)?;
        if actual.is_some() {
            // The external record does not participate in the bundle exchange.
            restore_sidecar(&sidecar_path(&prepared), intent.original_sidecar.as_deref())?;
        }
        let result = inspect(&target.join("Contents/MacOS/desktop-pet"))?;
        if result.image.sha256 != copy.image.sha256
            || &result.origin != desired
            || FileSnapshot::capture(&result.executable)? != intent.candidate_file
        {
            return Err("committed image or owner differs from validated prepared copy".into());
        }
        if update_locator {
            restore_sidecar(&intent.locator, intent.next_locator.as_deref())?;
        }
        if optional_bytes(&intent.locator, true)? != intent.next_locator {
            return Err("replacement locator differs from committed snapshot".into());
        }
        Ok(result)
    })();
    match committed {
        Ok(record) => {
            if disposition == ReplacementDisposition::RequireInactive {
                // Only the strict path proved all installation users absent. Retained storage
                // never enters this branch, even after a later scan reports no processes.
                fs::remove_dir_all(&work).map_err(|e| e.to_string())?;
                sync_directory(parent)?;
                Ok((record, None))
            } else {
                Ok((record, Some(prepared)))
            }
        }
        Err(error) => {
            let rollback: Result<(), String> = (|| {
                check_metadata_boundary(&intent)?;
                // Do not exchange on an error report alone: inspect both actual inodes first.
                let at_target = if exists(&target)? {
                    Some(FileSnapshot::capture(
                        &target.join("Contents/MacOS/desktop-pet"),
                    )?)
                } else {
                    None
                };
                let at_prepared = if exists(&prepared)? {
                    Some(FileSnapshot::capture(
                        &prepared.join("Contents/MacOS/desktop-pet"),
                    )?)
                } else {
                    None
                };
                if at_target.as_ref() == Some(&intent.candidate_file) {
                    if at_prepared != intent.original_file {
                        return Err(
                            "uncertain previous-image identity; no rollback attempted".into()
                        );
                    }
                    if actual.is_some() {
                        rename_bundle(&prepared, &target, true)?;
                    } else {
                        rename_bundle(&target, &prepared, false)?;
                    }
                } else if at_target != intent.original_file
                    || at_prepared.as_ref() != Some(&intent.candidate_file)
                {
                    return Err("uncertain disk state; no rollback attempted".into());
                }
                sync_directory(parent)?;
                sync_directory(&work)?;
                restore_sidecar(&sidecar_path(&target), intent.original_sidecar.as_deref())?;
                restore_sidecar(&sidecar_path(&prepared), Some(&intent.candidate_sidecar))?;
                if update_locator {
                    restore_sidecar(&intent.locator, intent.original_locator.as_deref())?;
                }
                if let Some(original) = &actual {
                    if inspect(&original.executable)? != *original
                        || FileSnapshot::capture(&original.executable)?
                            != *intent
                                .original_file
                                .as_ref()
                                .ok_or("missing original inode")?
                    {
                        return Err(
                            "rollback did not restore the exact original installation".into()
                        );
                    }
                } else if exists(&target)? {
                    return Err("rollback left an unexpected target".into());
                }
                if FileSnapshot::capture(&prepared.join("Contents/MacOS/desktop-pet"))?
                    != intent.candidate_file
                    || optional_bytes(&sidecar_path(&target), false)? != intent.original_sidecar
                    || optional_bytes(&intent.locator, true)? != intent.original_locator
                {
                    return Err("rollback metadata or prepared inode verification failed".into());
                }
                Ok(())
            })();
            // Even verified rollback retains both closures and immutable evidence for recovery.
            Err(format!(
                "{error}; verified rollback={rollback:?}; immutable intent={}; retained stage={}",
                intent_path.display(),
                work.display()
            ))
        }
    }
}

fn stage_directory(plan: &UpdatePlan) -> Result<PathBuf, String> {
    protocol::validate_operation_id(&plan.operation_id)?;
    Ok(protocol::private_updates(&plan.context.state_dir)?
        .join(format!("standalone-{}", plan.operation_id)))
}

fn standalone_release(plan: &UpdatePlan) -> Result<&crate::ReleaseCandidate, String> {
    crate::validate_plan_target(plan)?;
    if plan.action != UpdateAction::StandaloneReplace
        || !matches!(plan.origin, InstallOrigin::Standalone { .. })
    {
        return Err("plan is not a standalone replacement".into());
    }
    let target = plan
        .target
        .as_ref()
        .ok_or("standalone replacement lacks approved release")?;
    target.release.validate()?;
    if !target.policy.allows_version(&target.release.version)? || target.origin != plan.origin {
        return Err(
            "standalone replacement must preserve owner and authorized release policy".into(),
        );
    }
    if target.release.archive_size > MAX_ARCHIVE {
        return Err("standalone archive exceeds bound".into());
    }
    Ok(&target.release)
}

fn download(url: &str, path: &Path, limit: u64) -> Result<(), String> {
    if !url.starts_with("https://github.com/hanbong5938/desktop-pet/releases/download/") {
        return Err("standalone download is not the approved repository".into());
    }
    tool(
        "/usr/bin/curl",
        &[
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "15",
            "--max-time",
            "300",
            "--max-filesize",
            &limit.to_string(),
            "--output",
            path_str(path)?,
            url,
        ],
        16384,
        315,
    )?;
    let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !m.is_file()
        || m.file_type().is_symlink()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || m.permissions().mode() & 0o022 != 0
        || m.len() > limit
    {
        return Err("unsafe or oversized download".into());
    }
    Ok(())
}

pub fn stage_standalone_release(plan: &UpdatePlan) -> Result<StagedApp, String> {
    crate::recheck_plan(plan)?;
    stage_release(standalone_release(plan)?, &stage_directory(plan)?)
}

fn stage_release(release: &crate::ReleaseCandidate, directory: &Path) -> Result<StagedApp, String> {
    release.validate_neutral()?;
    if release.archive_size > MAX_ARCHIVE {
        return Err("public native staging requires a bounded approved release".into());
    }
    if exists(directory)? {
        return load_release_stage(release, directory);
    }
    create_directory(directory, true)?;
    // The stage is durable on failure. Retrying never replays a possibly completed download.
    let archive = directory.join(&release.asset_name);
    download(&release.archive_url, &archive, release.archive_size)?;
    if fs::metadata(&archive).map_err(|e| e.to_string())?.len() != release.archive_size
        || hash_file(&archive)? != release.sha256
    {
        return Err("standalone archive size or SHA-256 differs from approved release".into());
    }
    let checksum = directory.join("archive.sha256");
    download(&release.checksum_url, &checksum, 4096)?;
    release.validate_neutral_checksum(
        std::str::from_utf8(&owned_file(&checksum, 4096)?).map_err(|e| e.to_string())?,
    )?;
    let listing = tool(
        "/usr/bin/tar",
        &["-tzf", path_str(&archive)?],
        4 * 1024 * 1024,
        30,
    )?;
    let mut count = 0;
    for name in listing.lines() {
        let name = name
            .strip_prefix("./")
            .unwrap_or(name)
            .trim_end_matches('/');
        if name.is_empty() {
            continue;
        }
        let path = Path::new(name);
        if path.is_absolute()
            || path
                .components()
                .any(|p| !matches!(p, Component::Normal(_)))
            || !path.starts_with("DesktopPet.app")
        {
            return Err("standalone archive contains an unsafe or unrelated path".into());
        }
        count += 1;
        if count > MAX_FILES {
            return Err("standalone archive entry count exceeds bound".into());
        }
    }
    if count == 0 {
        return Err("standalone archive is empty".into());
    }
    let details = tool(
        "/usr/bin/tar",
        &["-tvzf", path_str(&archive)?],
        8 * 1024 * 1024,
        30,
    )?;
    let mut unpacked = 0u64;
    for line in details.lines() {
        if !matches!(line.as_bytes().first(), Some(b'd' | b'-')) {
            return Err("standalone archive contains links or special entries".into());
        }
        // BSD tar verbose output: permissions, links, owner, group, uncompressed size, date, path.
        let size = line
            .split_whitespace()
            .nth(4)
            .ok_or("archive entry lacks size")?
            .parse::<u64>()
            .map_err(|_| "archive entry has invalid size")?;
        if size > MAX_FILE {
            return Err("standalone archive member exceeds bound".into());
        }
        unpacked = unpacked
            .checked_add(size)
            .ok_or("archive unpacked size overflow")?;
        if unpacked > 2 * MAX_ARCHIVE {
            return Err("standalone archive unpacked size exceeds bound".into());
        }
    }
    tool(
        "/usr/bin/tar",
        &[
            "-xzf",
            path_str(&archive)?,
            "-C",
            path_str(&directory)?,
            "--no-same-owner",
            "--no-same-permissions",
        ],
        16384,
        60,
    )?;
    let bundle = directory.join("DesktopPet.app");
    let base = release
        .archive_url
        .rsplit_once('/')
        .ok_or("invalid archive URL")?
        .0;
    let sidecar_name = format!("DesktopPet-v{}-rig-native.json", release.version);
    let sidecar_url = format!("{base}/{sidecar_name}");
    download(&sidecar_url, &sidecar_path(&bundle), MAX_RECORD)?;
    let sums = directory.join("SHA256SUMS");
    download(&format!("{base}/SHA256SUMS"), &sums, 65536)?;
    let sum_bytes = owned_file(&sums, 65536)?;
    let sum_text = std::str::from_utf8(&sum_bytes).map_err(|e| e.to_string())?;
    let digest = hash_file(&sidecar_path(&bundle))?;
    let mut matching = 0;
    for line in sum_text.lines() {
        let Some((expected, name)) = line.split_once(' ') else {
            continue;
        };
        if name.trim_start_matches([' ', '*']) == sidecar_name {
            if expected != digest {
                return Err("rig sidecar differs from release SHA256SUMS".into());
            }
            matching += 1;
        }
    }
    if matching != 1 {
        return Err("release SHA256SUMS lacks one exact rig sidecar checksum".into());
    }
    let app = validate_neutral_bundle(&bundle)?;
    if app.version != release.version {
        return Err("standalone stage version differs from approved release".into());
    }
    let stage = StagedApp {
        bundle: app.bundle,
        image: app.image,
        version: app.version,
    };
    protocol::write_private_new(
        &directory.join("stage.json"),
        &serde_json::to_vec(&stage_record(&stage, &release.sha256)).map_err(|e| e.to_string())?,
        0o600,
    )?;
    Ok(stage)
}

fn stage_record(stage: &StagedApp, archive_sha256: &str) -> Value {
    serde_json::json!({"version":1,"bundle":stage.bundle,"image":stage.image,"app_version":stage.version,"archive_sha256":archive_sha256})
}

pub fn load_staged_standalone_release(plan: &UpdatePlan) -> Result<StagedApp, String> {
    load_release_stage(standalone_release(plan)?, &stage_directory(plan)?)
}

fn load_release_stage(
    release: &crate::ReleaseCandidate,
    directory: &Path,
) -> Result<StagedApp, String> {
    physical_directory(directory, true)?;
    validate_release_evidence(release, directory, &directory.join("DesktopPet.app"))?;
    let record: Value =
        serde_json::from_slice(&owned_private_file(&directory.join("stage.json"), 16384)?)
            .map_err(|e| e.to_string())?;
    let app = validate_neutral_bundle(&directory.join("DesktopPet.app"))?;
    let stage = StagedApp {
        bundle: app.bundle,
        image: app.image,
        version: app.version,
    };
    if stage.version != release.version || record != stage_record(&stage, &release.sha256) {
        return Err("durable standalone stage differs from approved release/image".into());
    }
    Ok(stage)
}

/// Runner must retain its InstallationReservation and execution fence across this call.
/// This does not reacquire global gates; locator ownership and both disk images are rechecked.
pub fn commit_standalone_release(
    plan: &UpdatePlan,
    stage: &StagedApp,
    scope: &mut crate::native_runtime::NativeInstallScope,
) -> Result<VerifiedInstallation, String> {
    standalone_release(plan)?;
    let verified = load_staged_standalone_release(plan)?;
    if &verified != stage {
        return Err("standalone stage changed since prepare".into());
    }
    let _lock = install_lock()?;
    let current = inspect(&plan.running.path)?;
    if current.image != plan.running || current.origin != plan.origin {
        return Err("standalone current image or owner changed since consent".into());
    }
    let selected = discover(None)?;
    if selected.as_ref().is_some_and(|selected| {
        selected.origin != current.origin || selected.image != current.image
    }) {
        return Err("standalone update may not take over another locator owner".into());
    }
    let staged = validate_staged_app(&stage.bundle)?;
    if staged.image != stage.image || staged.version != stage.version {
        return Err("standalone stage changed before prepared-copy validation".into());
    }
    let intent = protocol::private_updates(&plan.context.state_dir)?
        .join(format!("{}.bundle-replacement.json", plan.operation_id));
    let (installed, _) = replace_bundle_inner(
        &staged,
        &plan.origin,
        Some(&current),
        selected.is_some(),
        ReplacementDisposition::RetainCompatibleClients,
        Some(scope),
        Some((&plan.operation_id, &intent)),
    )?;
    Ok(VerifiedInstallation {
        image: installed.image,
        origin: installed.origin,
    })
}

fn validate_release_evidence(
    release: &crate::ReleaseCandidate,
    directory: &Path,
    bundle: &Path,
) -> Result<(), String> {
    release.validate_neutral()?;
    let archive = directory.join(&release.asset_name);
    if fs::symlink_metadata(&archive)
        .map_err(|e| e.to_string())?
        .len()
        != release.archive_size
        || hash_file(&archive)? != release.sha256
    {
        return Err("retained archive differs from the approved release".into());
    }
    let checksum = owned_file(&directory.join("archive.sha256"), 4096)?;
    release
        .validate_neutral_checksum(std::str::from_utf8(&checksum).map_err(|e| e.to_string())?)?;
    let sums = owned_file(&directory.join("SHA256SUMS"), 65536)?;
    let sums = std::str::from_utf8(&sums).map_err(|e| e.to_string())?;
    let name = format!("DesktopPet-v{}-rig-native.json", release.version);
    let digest = hash_file(&sidecar_path(bundle))?;
    let mut matching = 0;
    for line in sums.lines() {
        let Some((expected, file)) = line.split_once(' ') else {
            continue;
        };
        if file.trim_start_matches([' ', '*']) == name {
            if expected != digest {
                return Err("retained rig sidecar checksum differs from release evidence".into());
            }
            matching += 1;
        }
    }
    if matching != 1 {
        return Err("retained release evidence lacks the exact sidecar checksum".into());
    }
    Ok(())
}

/// Commit the source manager's staged-only output under its retained reservation/execution fence.
pub fn commit_source_release(
    plan: &UpdatePlan,
    staged: &ValidatedApp,
) -> Result<VerifiedInstallation, String> {
    require_neutral()?;
    #[cfg(not(desktop_pet_legacy))]
    {
        commit_source_neutral(plan, staged)
    }
    #[cfg(desktop_pet_legacy)]
    {
        let _ = (plan, staged);
        Err("legacy distribution cannot rebuild a neutral source installation".into())
    }
}

#[cfg(not(desktop_pet_legacy))]
fn commit_source_neutral(
    plan: &UpdatePlan,
    staged: &ValidatedApp,
) -> Result<VerifiedInstallation, String> {
    if plan.action != UpdateAction::SourceRebuild
        || !matches!(&plan.origin, InstallOrigin::Source { .. })
    {
        return Err("plan is not an exact core source rebuild".into());
    }
    crate::validate_plan_target(plan)?;
    let stage = validate_staged_app(&staged.bundle)?;
    if &stage != staged {
        return Err("source stage changed after manager completion".into());
    }
    check_source_revision(&stage, &plan.origin)?;
    let _lock = install_lock()?;
    let current = inspect(&plan.running.path)?;
    if current.image != plan.running || current.origin != plan.origin {
        return Err("source current image or checkout changed since consent".into());
    }
    let selected = discover(None)?;
    if selected.as_ref().is_some_and(|selected| {
        selected.origin != current.origin || selected.image != current.image
    }) {
        return Err("source rebuild may not take over another locator owner".into());
    }
    let installed = replace_bundle(
        &stage,
        &plan.origin,
        Some(&current),
        read_locator()?.is_some(),
    )?;
    Ok(VerifiedInstallation {
        image: installed.image,
        origin: installed.origin,
    })
}

fn migration_target_path(origin: &InstallOrigin, version: &str) -> Result<PathBuf, String> {
    match origin {
        InstallOrigin::Standalone { .. } => {
            Ok(bundle_for_origin(origin)?.join("Contents/MacOS/desktop-pet"))
        }
        InstallOrigin::Source { root } => {
            Ok(root.join("dist/DesktopPet.app/Contents/MacOS/desktop-pet"))
        }
        InstallOrigin::Homebrew {
            cellar_formula_root,
            ..
        } => Ok(cellar_formula_root
            .join(version)
            .join("libexec/DesktopPet.app/Contents/MacOS/desktop-pet")),
        _ => Err("migration target has no supported verified installation owner".into()),
    }
}

/// A revisioned formula would install in a different keg than the exact approved target path.
/// Read cached metadata only; the engine rechecks the release after its captured Brew update.
fn check_migration_brew_revision(origin: &InstallOrigin) -> Result<(), String> {
    let InstallOrigin::Homebrew { brew, formula, .. } = origin else {
        return Ok(());
    };
    if formula != &format!("hanbong5938/tap/{}", identity::neutral::STABLE_FORMULA) {
        return Err("migration requires the official neutral stable Homebrew formula".into());
    }
    let output = origin::command_output(
        brew,
        &["info", "--json=v2", "--formula", formula],
        65536,
        15,
    )?;
    let metadata: Value = serde_json::from_str(&output)
        .map_err(|e| format!("invalid migration Homebrew metadata: {e}"))?;
    let formulae = metadata
        .get("formulae")
        .and_then(Value::as_array)
        .ok_or("migration Homebrew metadata lacks its exact formula")?;
    if formulae.len() != 1
        || formulae[0].get("name").and_then(Value::as_str)
            != Some(identity::neutral::STABLE_FORMULA)
        || formulae[0].get("full_name").and_then(Value::as_str) != Some(formula.as_str())
    {
        return Err("migration Homebrew metadata names a different formula/owner".into());
    }
    if formulae[0]
        .get("revision")
        .is_some_and(|revision| revision.as_u64() != Some(0))
    {
        return Err(
            "migration Homebrew formula revision does not match the approved unrevisioned keg path"
                .into(),
        );
    }
    Ok(())
}

#[cfg(not(desktop_pet_legacy))]
fn validate_migration_previous(target: &crate::migration::MigrationTarget) -> Result<(), String> {
    let Some(previous) = target.previous_installation.as_deref() else {
        return Ok(());
    };
    let kind_matches = matches!(
        (&previous.kind, &target.origin),
        (
            InstallationKind::Standalone,
            InstallOrigin::Standalone { .. }
        ) | (InstallationKind::Source, InstallOrigin::Source { .. })
            | (InstallationKind::Homebrew, InstallOrigin::Homebrew { .. })
    );
    if previous.version != 1
        || previous.protocol != 4
        || previous.observation_mode != 2
        || previous.integration_api != 1
        || previous.executable != previous.image.path
        || previous.origin != target.origin
        || !kind_matches
        || !matches!(previous.app_version.as_str(), "0.4.7" | "0.4.8")
        || stable_version(&previous.app_version)? >= stable_version(&target.release.version)?
        || target.image.path != migration_target_path(&target.origin, &target.release.version)?
    {
        return Err(
            "migration previous installation is not the exact supported older neutral owner".into(),
        );
    }
    Ok(())
}

/// Re-prove the approved old owner until replacement, or the exact final image after it.
/// A retained Brew keg stops being the active locator after upgrade and must not be rediscovered.
#[cfg(not(desktop_pet_legacy))]
fn migration_target_installed(
    plan: &crate::migration::MigrationPlan,
    stage: &StagedApp,
    image: &ExecutableIdentity,
) -> Result<bool, String> {
    let target = &plan.target;
    validate_migration_previous(target)?;
    if image.path != target.image.path {
        return Err("migration candidate path differs from the approved target".into());
    }
    if image != &target.image {
        if validate_installed_migration_candidate(plan, stage)? != *image {
            return Err(
                "installed migration candidate differs from the captured manager image".into(),
            );
        }
        return Ok(true);
    }
    if exists(&target.image.path)? {
        let installed = inspect(&target.image.path)?;
        if installed.image == target.image
            && installed.origin == target.origin
            && installed.app_version == target.release.version
        {
            if let Some(previous) = target.previous_installation.as_deref() {
                if matches!(&target.origin, InstallOrigin::Homebrew { .. })
                    && executable_identity(&previous.executable)? != previous.image
                {
                    return Err(
                        "retained previous Homebrew image changed during migration upgrade".into(),
                    );
                }
            }
            return Ok(true);
        }
        if target.previous_installation.as_deref() == Some(&installed) {
            return Ok(false);
        }
        return Err("existing migration target differs from consented image/owner".into());
    }
    if let Some(previous) = target.previous_installation.as_deref() {
        if inspect(&previous.executable)? != *previous {
            return Err("previous migration installation changed before owner upgrade".into());
        }
    }
    Ok(false)
}

/// Accept a different executable hash only for an actual Brew-derived installation whose retained
/// original record is byte-for-byte the authenticated public release record. `inspect` verifies the
/// receipt, active owner, signature, complete deployed closure and the declared signing-only changes.
#[cfg(not(desktop_pet_legacy))]
fn validate_installed_migration_candidate(
    plan: &crate::migration::MigrationPlan,
    stage: &StagedApp,
) -> Result<ExecutableIdentity, String> {
    let target = &plan.target;
    validate_migration_previous(target)?;
    let installed = inspect(&target.image.path)?;
    if installed.image.path != target.image.path
        || installed.origin != target.origin
        || installed.app_version != target.release.version
    {
        return Err(
            "installed migration candidate differs from the approved path/version/owner".into(),
        );
    }
    if target.previous_installation.is_some() && target.image.sha256 != stage.image.sha256 {
        return Err("approved migration upgrade image differs from its signed stage".into());
    }
    if installed.image != target.image {
        if !matches!(&target.origin, InstallOrigin::Homebrew { .. })
            || target.image.sha256 != stage.image.sha256
        {
            return Err(
                "only the approved raw release may have a Homebrew-derived migration image".into(),
            );
        }
        let bundle = installed
            .executable
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .ok_or("installed migration candidate lacks its bundle")?;
        let original = bundle
            .parent()
            .ok_or("installed migration bundle lacks its manager directory")?
            .join("DesktopPet.app.release-rig-native.json");
        let original_bytes = owned_file(&original, MAX_RECORD)?;
        let approved_bytes = owned_file(&sidecar_path(&stage.bundle), MAX_RECORD)?;
        if original_bytes != approved_bytes {
            return Err(
                "Homebrew migration original record differs from the signed public release".into(),
            );
        }
        let approved_digest = format!("{:x}", Sha256::digest(&approved_bytes));
        let deployed: Value =
            serde_json::from_slice(&owned_file(&sidecar_path(bundle), MAX_RECORD)?)
                .map_err(|e| format!("invalid installed migration deployment record: {e}"))?;
        if deployed
            .pointer("/signing_derivation/kind")
            .and_then(Value::as_str)
            != Some("homebrew_ad_hoc")
            || deployed
                .pointer("/signing_derivation/original_sha256")
                .and_then(Value::as_str)
                != Some(approved_digest.as_str())
        {
            return Err(
                "Homebrew migration candidate lacks its exact approved signing derivation".into(),
            );
        }
    }
    if let Some(previous) = target.previous_installation.as_deref() {
        if matches!(&target.origin, InstallOrigin::Homebrew { .. })
            && executable_identity(&previous.executable)? != previous.image
        {
            return Err("retained previous Homebrew image changed during migration upgrade".into());
        }
    }
    if executable_identity(&installed.executable)? != installed.image {
        return Err("installed migration candidate changed during verification".into());
    }
    Ok(installed.image)
}

/// Read-only candidate proof. The caller must bind any derived image to the successful, quiescent
/// captured manager transaction before using it for migration startup or Commit.
#[cfg(not(desktop_pet_legacy))]
pub fn installed_migration_candidate(
    plan: &crate::migration::MigrationPlan,
) -> Result<ExecutableIdentity, String> {
    let stage = load_migration_stage(plan)?;
    validate_installed_migration_candidate(plan, &stage)
}

/// Retain and revalidate the signed public release independently of the manager's deployed image.
#[cfg(not(desktop_pet_legacy))]
fn load_migration_stage(plan: &crate::migration::MigrationPlan) -> Result<StagedApp, String> {
    require_neutral()?;
    protocol::validate_operation_id(&plan.operation_id)?;
    let release = &plan.target.release;
    release.validate_neutral()?;
    if release.version != identity::neutral::MIGRATION_TARGET_VERSION
        || release.channel != ReleaseChannel::Stable
    {
        return Err(format!(
            "legacy migration target must be stable neutral {}",
            identity::neutral::MIGRATION_TARGET_VERSION
        ));
    }
    let bundle = &plan.target.staged_bundle;
    ensure_absolute(bundle)?;
    if bundle
        .file_name()
        .is_none_or(|name| name != "DesktopPet.app")
    {
        return Err("migration stage must be DesktopPet.app".into());
    }
    let directory = bundle.parent().ok_or("migration stage lacks parent")?;
    let stage = if exists(directory)? {
        physical_directory(directory, true)?;
        validate_release_evidence(release, directory, bundle)?;
        let app = validate_staged_app(bundle)?;
        StagedApp {
            bundle: app.bundle,
            image: app.image,
            version: app.version,
        }
    } else {
        physical_directory(
            directory
                .parent()
                .ok_or("migration stage lacks physical ancestor")?,
            true,
        )?;
        stage_release(release, directory)?
    };
    if stage.version != release.version {
        return Err("signed migration stage version differs from consented release".into());
    }
    Ok(stage)
}

/// Pre-manager staging accepts only the original immutable header image or exact previous owner.
#[cfg(not(desktop_pet_legacy))]
pub fn stage_migration_target(plan: &crate::migration::MigrationPlan) -> Result<StagedApp, String> {
    stage_migration_target_with_image(plan, &plan.target.image)
}

/// Post-manager staging binds the caller's exact captured image without rewriting approval scope.
#[cfg(not(desktop_pet_legacy))]
pub fn stage_migration_target_with_image(
    plan: &crate::migration::MigrationPlan,
    image: &ExecutableIdentity,
) -> Result<StagedApp, String> {
    let stage = load_migration_stage(plan)?;
    let installed = migration_target_installed(plan, &stage, image)?;
    if (plan.target.previous_installation.is_some() || !installed)
        && stage.image.sha256 != plan.target.image.sha256
    {
        return Err("signed migration stage differs from consented target image".into());
    }
    if !installed {
        if plan.target.previous_installation.is_some()
            && !matches!(&plan.target.origin, InstallOrigin::Homebrew { .. })
        {
            crate::processes::installation_roots_inactive(&[physical_install_root(
                &plan.target.origin,
            )?
            .to_path_buf()])?;
        }
        check_source_revision(
            &ValidatedApp {
                bundle: stage.bundle.clone(),
                image: stage.image.clone(),
                version: stage.version.clone(),
                installation_client_contract: None,
            },
            &plan.target.origin,
        )?;
    }
    Ok(stage)
}

/// Operation B's guard owns both namespaces and disjoint source/target roots already.
/// Installs an absent target or atomically upgrades its approved inactive owner; never switches locator.
#[cfg(not(desktop_pet_legacy))]
pub fn commit_migration_target(
    plan: &crate::migration::MigrationPlan,
    stage: &StagedApp,
    image: &ExecutableIdentity,
) -> Result<(), String> {
    let verified = stage_migration_target_with_image(plan, image)?;
    if &verified != stage {
        return Err("migration stage changed after source quiescence".into());
    }
    let _lock = install_lock()?;
    let target = &plan.target;
    if migration_target_installed(plan, stage, image)? {
        return Ok(());
    }
    if matches!(&target.origin, InstallOrigin::Homebrew { .. }) {
        return Err("migration Brew target requires the engine's captured manager transaction before commit".into());
    }
    let bundle = bundle_for_origin(&target.origin)?;
    if exists(&bundle)? && target.previous_installation.is_none() {
        return Err("migration must not replace an existing unconsented target bundle".into());
    }
    crate::processes::installation_roots_inactive(&[
        physical_install_root(&target.origin)?.to_path_buf()
    ])?;
    let app = validate_staged_app(&stage.bundle)?;
    if app.image != stage.image || app.version != stage.version {
        return Err("migration stage changed before prepared-copy validation".into());
    }
    check_source_revision(&app, &target.origin)?;
    let installed = replace_bundle(
        &app,
        &target.origin,
        target.previous_installation.as_deref(),
        false,
    )?;
    if installed.image != *image || installed.origin != target.origin {
        return Err("installed migration target differs from consented image/owner".into());
    }
    Ok(())
}

/// Fixed-neutral helper staging usable by the real bridge, without changing its compiled identity.
pub fn stage_neutral_migration_helper(
    state_dir: &Path,
    operation_id: &str,
    release: &crate::ReleaseCandidate,
) -> Result<StagedApp, String> {
    protocol::validate_operation_id(operation_id)?;
    release.validate_neutral()?;
    if release.version != identity::neutral::MIGRATION_TARGET_VERSION
        || release.channel != ReleaseChannel::Stable
    {
        return Err(format!(
            "bridge migration may stage only the approved neutral stable {} target",
            identity::neutral::MIGRATION_TARGET_VERSION,
        ));
    }
    let directory =
        protocol::private_updates(state_dir)?.join(format!("neutral-migration-{operation_id}"));
    stage_release(release, &directory)
}

/// Read-only official lookup + durable neutral stage; never adopts a locator or stops/starts an app.
pub fn stage_neutral_migration_release(
    state_dir: &Path,
    operation_id: &str,
    observation: &desktop_pet_contracts::ObservationSelection,
) -> Result<crate::migration::MigrationTarget, String> {
    // Capability two preserves opaque selections; it does not synthesize an enabled integration.
    let _ = observation.as_str();
    let release = crate::releases::fetch_neutral_migration_candidate()?;
    let stage = stage_neutral_migration_helper(state_dir, operation_id, &release)?;
    #[cfg(not(desktop_pet_legacy))]
    let selected = discover_neutral(None)?;
    #[cfg(desktop_pet_legacy)]
    let selected: Option<InstallationRecord> = {
        // Only the genuinely staged protocol-four helper can attest neutral ownership for a bridge.
        let helper = stage
            .bundle
            .join("Contents/MacOS/desktop-pet-update-coordinator");
        let response = origin::command_output(&helper, &["discover", "--json"], 16384, 180)?;
        let selected: Option<InstallationRecord> = serde_json::from_str(&response)
            .map_err(|e| format!("invalid neutral helper discovery proof: {e}"))?;
        if let Some(record) = &selected {
            if record.version != 1
                || record.protocol != 4
                || record.observation_mode != 2
                || record.integration_api != 1
                || record.executable != record.image.path
                || executable_identity(&record.executable)? != record.image
            {
                return Err(
                    "neutral helper returned an incompatible or changed installation proof".into(),
                );
            }
        }
        selected
    };
    let (image, origin, previous_installation) = if let Some(installed) = selected {
        let installed_version = stable_version(&installed.app_version)?;
        let release_version = stable_version(&release.version)?;
        if installed_version > release_version {
            return Err("existing public installation is newer than the approved migration release; downgrade is forbidden".into());
        }
        if installed_version == release_version {
            (installed.image, installed.origin, None)
        } else {
            if !matches!(installed.app_version.as_str(), "0.4.7" | "0.4.8") {
                return Err("existing public installation is not a supported neutral 0.4.7/0.4.8 migration owner".into());
            }
            if matches!(&installed.origin, InstallOrigin::Homebrew { .. }) {
                check_migration_brew_revision(&installed.origin)?;
            } else {
                crate::processes::installation_roots_inactive(&[physical_install_root(
                    &installed.origin,
                )?
                .to_path_buf()])?;
            }
            check_source_revision(
                &ValidatedApp {
                    bundle: stage.bundle.clone(),
                    image: stage.image.clone(),
                    version: stage.version.clone(),
                    installation_client_contract: None,
                },
                &installed.origin,
            )?;
            (
                ExecutableIdentity {
                    path: migration_target_path(&installed.origin, &release.version)?,
                    sha256: stage.image.sha256.clone(),
                },
                installed.origin.clone(),
                Some(Box::new(installed)),
            )
        }
    } else {
        return Err(
            "the pinned stable migration target requires an Applications installation; a first standalone migration must wait for a private-location-compatible stable release. No installation was changed"
                .into(),
        );
    };
    Ok(crate::migration::MigrationTarget {
        release,
        image,
        origin,
        staged_bundle: stage.bundle,
        previous_installation,
    })
}

#[cfg(test)]
mod version_tests {
    use super::{installed_version, stable_version};

    #[test]
    fn installation_capabilities_keep_historical_absence_but_reject_invalid_present_contracts() {
        let historical = serde_json::json!({
            "protocol": 4, "observation_mode": 2, "integration_api": 1,
        });
        assert_eq!(
            super::decode_capabilities(&historical).unwrap(),
            (None, None)
        );
        let mut current = historical.clone();
        current["migration_handover_protocol"] = 1.into();
        current["installation_client_contract"] = 1.into();
        assert_eq!(
            super::decode_capabilities(&current).unwrap(),
            (Some(1), Some(1))
        );
        assert_ne!(
            super::decode_capabilities(&historical).unwrap(),
            super::decode_capabilities(&current).unwrap()
        );
        for invalid in [
            serde_json::Value::Null,
            false.into(),
            0.into(),
            2.into(),
            "1".into(),
            serde_json::json!(1.0),
            serde_json::json!({}),
        ] {
            current["installation_client_contract"] = invalid;
            assert!(super::decode_capabilities(&current).is_err());
        }
        // Additive negotiation does not reject unrelated future capability keys.
        current["installation_client_contract"] = 1.into();
        current["future_optional_capability"] = true.into();
        assert_eq!(
            super::decode_capabilities(&current).unwrap(),
            (Some(1), Some(1))
        );
    }

    #[test]
    fn direct_install_accepts_canonical_beta_without_relaxing_migration() {
        let stable = installed_version("0.4.12").unwrap();
        let beta = installed_version("0.4.13-beta.1").unwrap();
        assert!(beta > stable);
        assert!(beta < installed_version("0.4.13").unwrap());
        assert!(stable_version("0.4.13-beta.1").is_err());
        for invalid in [
            "0.4.13-alpha.1",
            "0.4.13-beta",
            "0.4.13-beta.01",
            "0.4.13-beta.1.extra",
            "0.4.13-beta.1+build",
        ] {
            assert!(installed_version(invalid).is_err(), "{invalid}");
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod replacement_boundary_tests {
    use super::*;

    fn executable(bundle: &Path, bytes: &[u8]) -> PathBuf {
        let directory = bundle.join("Contents/MacOS");
        fs::create_dir_all(&directory).unwrap();
        fs::set_permissions(bundle, fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.join("desktop-pet");
        fs::write(&path, bytes).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(bundle.join("Contents/resource"), bytes).unwrap();
        path
    }

    fn fixture() -> (tempfile::TempDir, ReplacementIntent) {
        let temporary = tempfile::tempdir().unwrap();
        let home = fs::canonicalize(temporary.path()).unwrap();
        let root = home.join("Applications");
        create_directory(&root, false).unwrap();
        let target = root.join("DesktopPet.app");
        let work = temporary_directory(&root, ".desktop-pet-install").unwrap();
        let prepared = work.join("DesktopPet.app");
        let old = executable(&target, b"original executable");
        let new = executable(&prepared, b"candidate executable");
        let original_sidecar = b"original external sidecar, not the candidate's".to_vec();
        let candidate_sidecar = b"candidate external sidecar".to_vec();
        atomic_owned_file(&sidecar_path(&target), &original_sidecar).unwrap();
        atomic_owned_file(&sidecar_path(&prepared), &candidate_sidecar).unwrap();
        let locator = root.join("installation.json");
        let original_locator = br#"{"owner":"original"}"#.to_vec();
        let next_locator = br#"{"owner":"candidate"}"#.to_vec();
        atomic_owned_file(&locator, &original_locator).unwrap();
        let work_metadata = fs::metadata(&work).unwrap();
        let origin = InstallOrigin::Standalone {
            bundle: target.clone(),
        };
        let original = InstallationRecord {
            version: 1,
            executable: old.clone(),
            kind: InstallationKind::Standalone,
            app_version: "0.4.13-beta.1".into(),
            protocol: 4,
            observation_mode: 2,
            integration_api: 1,
            origin: origin.clone(),
            image: executable_identity(&old).unwrap(),
        };
        let intent = ReplacementIntent {
            version: 1,
            operation_id: "0123456789abcdef0123456789abcdef".into(),
            disposition: ReplacementDisposition::RetainCompatibleClients,
            origin,
            original: Some(original),
            original_file: Some(FileSnapshot::capture(&old).unwrap()),
            candidate_file: FileSnapshot::capture(&new).unwrap(),
            candidate_version: "0.4.13-beta.5".into(),
            work,
            work_device: work_metadata.dev(),
            work_inode: work_metadata.ino(),
            target,
            prepared,
            original_sidecar_sha256: Some(digest_bytes(&original_sidecar)),
            candidate_sidecar_sha256: digest_bytes(&candidate_sidecar),
            original_sidecar: Some(original_sidecar),
            candidate_sidecar,
            locator,
            original_locator: Some(original_locator),
            next_locator: Some(next_locator),
            update_locator: true,
        };
        (temporary, intent)
    }

    #[test]
    fn metadata_reconciliation_distinguishes_each_real_exchange_boundary() {
        // Each state is a distinct interruption point in the production mutation sequence.
        // This exercises actual inode exchange and the production metadata reconciler, not
        // a simulated exchange result or an assertion about implementation text.
        for boundary in 0..5 {
            let (_temporary, intent) = fixture();
            let home = fs::canonicalize(_temporary.path()).unwrap();
            assert_eq!(
                standalone_owner_parent_in(&home, &intent.target, true).unwrap(),
                intent.target.parent().map(Path::to_path_buf)
            );
            assert_eq!(
                standalone_origin_in(&home, intent.original.as_ref()).unwrap(),
                intent.origin
            );
            if boundary >= 1 {
                atomic_owned_file(&sidecar_path(&intent.target), &intent.candidate_sidecar)
                    .unwrap();
            }
            if boundary >= 2 {
                rename_bundle(&intent.prepared, &intent.target, true).unwrap();
            }
            if boundary >= 3 {
                restore_sidecar(
                    &sidecar_path(&intent.prepared),
                    intent.original_sidecar.as_deref(),
                )
                .unwrap();
            }
            if boundary >= 4 {
                restore_sidecar(&intent.locator, intent.next_locator.as_deref()).unwrap();
            }
            let swapped = boundary >= 2;
            assert_eq!(replacement_swapped(&intent).unwrap(), swapped);
            check_metadata_boundary(&intent).unwrap();
            reconcile_replacement_metadata(&intent, swapped).unwrap();
            assert_eq!(replacement_swapped(&intent).unwrap(), swapped);
            let expected_old = if swapped {
                &intent.prepared
            } else {
                &intent.target
            };
            let expected_new = if swapped {
                &intent.target
            } else {
                &intent.prepared
            };
            assert_eq!(
                fs::read(expected_old.join("Contents/resource")).unwrap(),
                b"original executable"
            );
            assert_eq!(
                fs::read(expected_new.join("Contents/resource")).unwrap(),
                b"candidate executable"
            );
            assert_eq!(
                owned_file(&sidecar_path(expected_old), MAX_RECORD).unwrap(),
                intent.original_sidecar.clone().unwrap()
            );
            assert_eq!(
                owned_file(&sidecar_path(expected_new), MAX_RECORD).unwrap(),
                intent.candidate_sidecar
            );
            assert_eq!(
                optional_bytes(&intent.locator, true).unwrap(),
                if swapped {
                    intent.next_locator.clone()
                } else {
                    intent.original_locator.clone()
                }
            );
            // Reconciliation is idempotent and does not retire the retained old closure.
            reconcile_replacement_metadata(&intent, swapped).unwrap();
            assert!(expected_old.join("Contents/resource").is_file());
            assert_eq!(
                standalone_owner_parent_in(&home, &intent.target, true).unwrap(),
                intent.target.parent().map(Path::to_path_buf)
            );
            assert_eq!(
                standalone_origin_in(&home, intent.original.as_ref()).unwrap(),
                intent.origin
            );
            assert!(!home.join(".local").exists());
        }
    }

    #[test]
    fn original_sidecar_survives_exchange_and_verified_reverse_exchange() {
        let (_temporary, intent) = fixture();
        atomic_owned_file(&sidecar_path(&intent.target), &intent.candidate_sidecar).unwrap();
        rename_bundle(&intent.prepared, &intent.target, true).unwrap();
        reconcile_replacement_metadata(&intent, true).unwrap();
        assert_eq!(
            owned_file(&sidecar_path(&intent.prepared), MAX_RECORD).unwrap(),
            intent.original_sidecar.clone().unwrap()
        );
        rename_bundle(&intent.prepared, &intent.target, true).unwrap();
        reconcile_replacement_metadata(&intent, false).unwrap();
        assert_eq!(
            FileSnapshot::capture(&intent.target.join("Contents/MacOS/desktop-pet")).unwrap(),
            intent.original_file.clone().unwrap()
        );
        assert_eq!(
            owned_file(&sidecar_path(&intent.target), MAX_RECORD).unwrap(),
            intent.original_sidecar.clone().unwrap()
        );
        assert!(intent.prepared.join("Contents/resource").is_file());
    }

    #[test]
    fn unknown_metadata_preserves_both_images_without_overwriting_evidence() {
        for tamper_locator in [false, true] {
            let (_temporary, intent) = fixture();
            let tampered = if tamper_locator {
                intent.locator.clone()
            } else {
                sidecar_path(&intent.target)
            };
            atomic_owned_file(&tampered, b"unrelated owner mutation").unwrap();
            let canonical_before =
                FileSnapshot::capture(&intent.target.join("Contents/MacOS/desktop-pet")).unwrap();
            let prepared_before =
                FileSnapshot::capture(&intent.prepared.join("Contents/MacOS/desktop-pet")).unwrap();
            assert!(reconcile_replacement_metadata(&intent, false).is_err());
            assert_eq!(fs::read(&tampered).unwrap(), b"unrelated owner mutation");
            assert_eq!(
                FileSnapshot::capture(&intent.target.join("Contents/MacOS/desktop-pet")).unwrap(),
                canonical_before
            );
            assert_eq!(
                FileSnapshot::capture(&intent.prepared.join("Contents/MacOS/desktop-pet")).unwrap(),
                prepared_before
            );
        }
    }

    #[test]
    fn same_hash_substituted_inode_is_not_a_recoverable_exchange() {
        let (_temporary, intent) = fixture();
        let original = intent.target.join("Contents/MacOS/desktop-pet");
        let replacement = intent.target.join("Contents/MacOS/substituted");
        fs::copy(&original, &replacement).unwrap();
        assert_eq!(
            hash_file(&original).unwrap(),
            hash_file(&replacement).unwrap()
        );
        fs::rename(&replacement, &original).unwrap();
        assert!(replacement_swapped(&intent).is_err());
        assert!(reconcile_replacement_metadata(&intent, false).is_err());
        assert!(intent.prepared.join("Contents/resource").is_file());
    }

    #[test]
    fn missing_original_locator_is_preserved_across_all_committed_boundaries() {
        let (_temporary, mut intent) = fixture();
        fs::remove_file(&intent.locator).unwrap();
        intent.original_locator = None;
        atomic_owned_file(&sidecar_path(&intent.target), &intent.candidate_sidecar).unwrap();
        rename_bundle(&intent.prepared, &intent.target, true).unwrap();
        reconcile_replacement_metadata(&intent, true).unwrap();
        assert_eq!(
            optional_bytes(&intent.locator, true).unwrap(),
            intent.next_locator
        );
        assert_eq!(
            owned_file(&sidecar_path(&intent.prepared), MAX_RECORD).unwrap(),
            intent.original_sidecar.unwrap()
        );
    }

    #[test]
    fn immutable_intent_cannot_be_rewritten_to_describe_a_later_inode() {
        let (_temporary, intent) = fixture();
        let path = intent.work.join("operation.bundle-replacement.json");
        let bytes = serde_json::to_vec(&intent).unwrap();
        protocol::write_private_new(&path, &bytes, 0o600).unwrap();
        assert!(protocol::write_private_new(&path, b"replacement proof", 0o600).is_err());
        assert_eq!(owned_private_file(&path, MAX_RECORD).unwrap(), bytes);
        assert_eq!(
            owned_file(&sidecar_path(&intent.target), MAX_RECORD).unwrap(),
            intent.original_sidecar.as_ref().unwrap().as_slice()
        );
        assert!(!replacement_swapped(&intent).unwrap());
    }
}

#[cfg(test)]
mod installation_directory_tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn absent_standalone_parent_is_created_privately_and_reused_without_replacement() {
        let temporary = tempfile::tempdir().unwrap();
        let home = fs::canonicalize(temporary.path()).unwrap();
        assert!(!home.join(".local").exists());
        let parent = owned_directory_tree(&home, &INSTALLATION_DIRECTORIES, true)
            .unwrap()
            .unwrap();
        assert_eq!(parent, home.join(".local/share/desktop-pet/libexec"));
        let mut path = home.clone();
        for (name, private) in INSTALLATION_DIRECTORIES {
            path.push(name);
            physical_directory(&path, private).unwrap();
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        let marker = parent.join("existing-owner-evidence");
        fs::write(&marker, b"preserved").unwrap();
        let original_inode = fs::metadata(&parent).unwrap().ino();
        assert_eq!(
            owned_directory_tree(&home, &INSTALLATION_DIRECTORIES, true).unwrap(),
            Some(parent.clone())
        );
        assert_eq!(fs::metadata(&parent).unwrap().ino(), original_inode);
        assert_eq!(fs::read(&marker).unwrap(), b"preserved");
    }

    #[test]
    fn standalone_parent_creation_rejects_symlinks_at_every_component() {
        for depth in 0..INSTALLATION_DIRECTORIES.len() {
            let temporary = tempfile::tempdir().unwrap();
            let home = fs::canonicalize(temporary.path()).unwrap();
            let prefix = owned_directory_tree(&home, &INSTALLATION_DIRECTORIES[..depth], true)
                .unwrap()
                .unwrap();
            let external = home.join("external");
            create_directory(&external, true).unwrap();
            let marker = external.join("existing-owner-evidence");
            fs::write(&marker, b"untouched").unwrap();
            let link = prefix.join(INSTALLATION_DIRECTORIES[depth].0);
            symlink(&external, &link).unwrap();
            assert!(owned_directory_tree(&home, &INSTALLATION_DIRECTORIES, true).is_err());
            assert!(fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink());
            assert_eq!(fs::read(&marker).unwrap(), b"untouched");
            assert_eq!(fs::read_dir(&external).unwrap().count(), 1);
        }
    }

    #[test]
    fn standalone_parent_creation_rejects_unsafe_existing_permissions_without_repair() {
        for depth in 0..INSTALLATION_DIRECTORIES.len() {
            let temporary = tempfile::tempdir().unwrap();
            let home = fs::canonicalize(temporary.path()).unwrap();
            let directory = owned_directory_tree(&home, &INSTALLATION_DIRECTORIES[..=depth], true)
                .unwrap()
                .unwrap();
            let mode = if INSTALLATION_DIRECTORIES[depth].1 {
                0o755
            } else {
                0o777
            };
            fs::set_permissions(&directory, fs::Permissions::from_mode(mode)).unwrap();
            assert!(owned_directory_tree(&home, &INSTALLATION_DIRECTORIES, true).is_err());
            assert_eq!(
                fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
                mode
            );
            assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        }
    }

    fn selected(bundle: PathBuf) -> InstallationRecord {
        let executable = bundle.join("Contents/MacOS/desktop-pet");
        InstallationRecord {
            version: 1,
            executable: executable.clone(),
            kind: InstallationKind::Standalone,
            app_version: "0.4.13-beta.7".into(),
            protocol: 4,
            observation_mode: 2,
            integration_api: 1,
            origin: InstallOrigin::Standalone { bundle },
            image: ExecutableIdentity {
                path: executable,
                sha256: "a".repeat(64),
            },
        }
    }

    #[test]
    fn standalone_selection_retains_exact_original_but_defaults_to_private() {
        let temporary = tempfile::tempdir().unwrap();
        let home = fs::canonicalize(temporary.path()).unwrap();
        let [private, original] = standalone_bundles_in(&home);
        assert_eq!(standalone_candidate_in(&home).unwrap(), None);
        assert_eq!(
            standalone_origin_in(&home, None).unwrap(),
            InstallOrigin::Standalone {
                bundle: private.clone()
            }
        );
        assert_eq!(
            standalone_bundle().unwrap(),
            os_home()
                .unwrap()
                .join(".local/share/desktop-pet/libexec/DesktopPet.app")
        );
        assert!(standalone_owner_parent_in(&home, &original, true).is_err());
        assert!(!home.join("Applications").exists());
        create_directory(&home.join("Applications"), false).unwrap();
        assert!(standalone_owner_parent_in(&home, &original, true).is_err());
        assert!(!original.exists());
        create_directory(&original, false).unwrap();
        fs::set_permissions(home.join("Applications"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&original, fs::Permissions::from_mode(0o755)).unwrap();
        let current = selected(original.clone());
        assert_eq!(
            standalone_candidate_in(&home).unwrap(),
            Some(original.clone())
        );
        assert_eq!(
            standalone_origin_in(&home, Some(&current)).unwrap(),
            current.origin
        );
        assert_eq!(
            standalone_owner_parent_in(&home, &original, true).unwrap(),
            Some(home.join("Applications"))
        );
        assert!(!home.join(".local").exists());
        let parent = standalone_owner_parent_in(&home, &private, true)
            .unwrap()
            .unwrap();
        create_directory(&private, false).unwrap();
        assert!(standalone_candidate_in(&home).is_err());
        // An already selected owner remains exact even when fallback discovery is ambiguous.
        assert_eq!(
            standalone_origin_in(&home, Some(&current)).unwrap(),
            current.origin
        );
        let private_owner = selected(private.clone());
        assert_eq!(
            standalone_origin_in(&home, Some(&private_owner)).unwrap(),
            private_owner.origin
        );
        fs::remove_dir(&original).unwrap();
        assert_eq!(standalone_candidate_in(&home).unwrap(), Some(private));
        assert_eq!(
            fs::metadata(parent).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn standalone_selection_rejects_other_homes_custom_system_and_mismatched_owners() {
        let temporary = tempfile::tempdir().unwrap();
        let home = fs::canonicalize(temporary.path()).unwrap();
        for bundle in [
            home.join("DesktopPet.app"),
            home.join("custom/DesktopPet.app"),
            home.join("Applications/Other.app"),
            home.join("another-home/Applications/DesktopPet.app"),
            PathBuf::from("/Applications/DesktopPet.app"),
            PathBuf::from("Applications/DesktopPet.app"),
        ] {
            assert!(standalone_owner_parent_in(&home, &bundle, true).is_err());
            assert!(standalone_origin_in(&home, Some(&selected(bundle))).is_err());
        }
        let mut mismatch = selected(standalone_bundles_in(&home)[0].clone());
        mismatch.origin = InstallOrigin::Source { root: home.clone() };
        assert!(standalone_origin_in(&home, Some(&mismatch)).is_err());
        assert_eq!(fs::read_dir(&home).unwrap().count(), 0);
    }

    #[test]
    fn original_owner_rejects_symlinked_or_writable_parent_and_bundle_without_repair() {
        for at_bundle in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let home = fs::canonicalize(temporary.path()).unwrap();
            let original = standalone_bundles_in(&home)[1].clone();
            let applications = home.join("Applications");
            let external = home.join("external");
            create_directory(&external, false).unwrap();
            let link = if at_bundle {
                create_directory(&applications, false).unwrap();
                original.clone()
            } else {
                applications.clone()
            };
            symlink(&external, &link).unwrap();
            assert!(standalone_owner_parent_in(&home, &original, true).is_err());
            assert!(fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink());
            assert_eq!(fs::read_dir(&external).unwrap().count(), 0);
            fs::remove_file(&link).unwrap();
            if !at_bundle {
                create_directory(&applications, false).unwrap();
            }
            create_directory(&original, false).unwrap();
            let unsafe_directory = if at_bundle { &original } else { &applications };
            fs::set_permissions(unsafe_directory, fs::Permissions::from_mode(0o777)).unwrap();
            assert!(standalone_owner_parent_in(&home, &original, true).is_err());
            assert_eq!(
                fs::metadata(unsafe_directory).unwrap().permissions().mode() & 0o777,
                0o777
            );
            assert!(!home.join(".local").exists());
        }
    }
}
