//! The app and its paired helper use this single compiled product identity.
#[cfg(desktop_pet_legacy)]
#[path = "../../../migration/legacy-0.3.8/identity.rs"]
mod selected;
#[cfg(desktop_pet_legacy)]
pub use selected::*;

/// Fixed neutral target authority is also needed by the one-time bridge staging
/// boundary; it does not change that bridge's own compiled identity.
pub mod neutral {
    pub const DISTRIBUTION: &str = "neutral";
    pub const LEGACY: bool = false;
    pub const VERSION: &str = "0.4.3";
    pub const BINARY: &str = "desktop-pet";
    pub const HELPER: &str = "desktop-pet-update-coordinator";
    pub const APP_NAME: &str = "DesktopPet.app";
    pub const BETA_APP_NAME: &str = "DesktopPetBeta.app";
    pub const DISPLAY_NAME: &str = "Desktop Pet";
    pub const BUNDLE_ID: &str = "io.github.hanbong5938.desktop-pet";
    pub const PROTOCOL: u32 = 4;
    pub const OBSERVATION_CAPABILITY: u32 = 2;
    pub const INTEGRATION_API: u32 = desktop_pet_contracts::INTEGRATION_API_VERSION;
    pub const REPOSITORY: &str = "hanbong5938/desktop-pet";
    pub const RELEASE_API: &str = "https://api.github.com/repos/hanbong5938/desktop-pet/releases";
    pub const RELEASE_DOWNLOAD: &str =
        "https://github.com/hanbong5938/desktop-pet/releases/download/";
    pub const ARCHIVE_PREFIX: &str = "DesktopPet";
    pub const STABLE_FORMULA: &str = "desktop-pet";
    pub const BETA_FORMULA: &str = "desktop-pet-beta";
    pub const UPDATES_DIRECTORY: &str = "updates-v4";
    pub const GLOBAL_UPDATES_DIRECTORY: &str = ".desktop-pet-updates";
    pub const OBSERVATION_ENV: &str = desktop_pet_contracts::OBSERVATION_SELECTION_ENV;
    pub const UPDATER_TOKEN_ENV: &str = "DESKTOP_PET_UPDATER_TOKEN";
    pub const UPDATER_GENERATION_ENV: &str = "DESKTOP_PET_UPDATER_GENERATION";
    pub const UI_READY_MARKER: &str = "DESKTOP_PET_UI_READY";
}
#[cfg(not(desktop_pet_legacy))]
pub use neutral::*;

pub const CORE_REVISION: &str = env!("DESKTOP_PET_COMPILED_CORE_REVISION");

pub fn capabilities() -> serde_json::Value {
    serde_json::json!({
        "protocol": PROTOCOL,
        "observation_mode": OBSERVATION_CAPABILITY,
        "integration_api": INTEGRATION_API,
    })
}

pub fn json() -> serde_json::Value {
    let value = serde_json::json!({
        "distribution": DISTRIBUTION,
        "app_name": APP_NAME,
        "executable": BINARY,
        "helper": HELPER,
        "display_name": DISPLAY_NAME,
        "bundle_id": BUNDLE_ID,
        "app_version": VERSION,
        "core_revision": CORE_REVISION,
        "protocol": PROTOCOL,
        "observation_mode": OBSERVATION_CAPABILITY,
        "integration_api": INTEGRATION_API,
    });
    #[cfg(desktop_pet_legacy)]
    let value = {
        let mut value = value;
        value
            .as_object_mut()
            .expect("compiled identity object")
            .insert(
                "herdr_migration_revision".into(),
                HERDR_MIGRATION_REVISION.into(),
            );
        value
    };
    value
}

pub fn herdr_migration_revision() -> Option<&'static str> {
    #[cfg(desktop_pet_legacy)]
    {
        Some(HERDR_MIGRATION_REVISION)
    }
    #[cfg(not(desktop_pet_legacy))]
    {
        None
    }
}
