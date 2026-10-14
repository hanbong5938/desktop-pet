//! Compiled distribution envelope for the one-time protocol-3 bridge.
pub const DISTRIBUTION: &str = "legacy-0.3.10";
pub const LEGACY: bool = true;
pub const VERSION: &str = "0.3.10";
pub const BINARY: &str = "herdr-desktop-pet";
pub const HELPER: &str = "herdr-update-coordinator";
pub const APP_NAME: &str = "HerdrDesktopPet.app";
pub const BETA_APP_NAME: &str = "HerdrDesktopPetBeta.app";
pub const DISPLAY_NAME: &str = "Herdr Desktop Pet";
pub const BUNDLE_ID: &str = "dev.herdr.desktop-pet";
pub const PROTOCOL: u32 = 3;
pub const OBSERVATION_CAPABILITY: u32 = 1;
pub const INTEGRATION_API: u32 = 1;
pub const REPOSITORY: &str = "hanbong5938/herdr-desktop-pet";
pub const RELEASE_API: &str = "https://api.github.com/repos/hanbong5938/herdr-desktop-pet/releases";
pub const RELEASE_DOWNLOAD: &str =
    "https://github.com/hanbong5938/herdr-desktop-pet/releases/download/";
pub const ARCHIVE_PREFIX: &str = "HerdrDesktopPet";
pub const STABLE_FORMULA: &str = "herdr-desktop-pet";
pub const BETA_FORMULA: &str = "herdr-desktop-pet-beta";
pub const UPDATES_DIRECTORY: &str = "updates";
pub const GLOBAL_UPDATES_DIRECTORY: &str = ".herdr-updates";
pub const OBSERVATION_ENV: &str = "HERDR_PET_OBSERVATION_MODE";
pub const UPDATER_TOKEN_ENV: &str = "HERDR_DESKTOP_PET_UPDATER_TOKEN";
pub const UPDATER_GENERATION_ENV: &str = "HERDR_DESKTOP_PET_UPDATER_GENERATION";
pub const HERDR_MIGRATION_REVISION: &str = env!("DESKTOP_PET_COMPILED_HERDR_MIGRATION_REVISION");
pub const UI_READY_MARKER: &str = "HERDR_DESKTOP_PET_UI_READY";
