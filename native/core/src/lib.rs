#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[cfg(not(target_os = "macos"))]
compile_error!("Desktop Pet requires a macOS target");

mod alpha;
mod animation;
mod app_update;
mod assets;
mod automation;
mod automation_cli;
mod behavior;
mod bubble;
mod bundle;
mod character_browser;
mod character_menu;
mod character_preview;
mod character_renderer;
mod character_selection;
mod character_service;
mod character_store;
mod character_types;
mod cli;
mod composer_layout;
mod control;
mod daemon;
mod dialogue;
mod dialogue_automation;
mod dialogue_editor;
mod display_geometry;
mod i18n;
mod interaction;
mod lifecycle;
mod lifecycle_settings_ui;
mod menu_bar_icon;
mod menu_panel;
mod official_catalog;
mod official_characters;
mod operations;
mod pack_authoring;
mod pointer_capture;
mod pose;
mod preferences;
mod rig_renderer;
mod session_cards;
mod session_view;
mod socket;
mod sources;
mod state;
mod status_indicator;
#[cfg(test)]
mod transport_tests;
mod ui;
mod update_card;
mod worktree_confirmation;

use desktop_pet_contracts::{
    Integration, IntegrationExtensions, ObservationSelection, ProfileStore,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub type ProfileConfigurator = Arc<dyn Fn(ProfileStore) -> Result<(), String> + Send + Sync>;
pub type AttachFrame =
    Arc<dyn Fn(&ObservationSelection) -> Result<Option<Vec<u8>>, String> + Send + Sync>;
pub type StatusFields =
    Arc<dyn Fn() -> Result<serde_json::Map<String, serde_json::Value>, String> + Send + Sync>;
pub type CliInputPolicy = Arc<dyn Fn(&[std::ffi::OsString]) -> Result<bool, String> + Send + Sync>;

#[derive(Default)]
pub struct Composition {
    pub integrations: Vec<Arc<dyn Integration>>,
    pub extensions: IntegrationExtensions,
    pub provider_labels: BTreeMap<String, String>,
    pub profile_configurators: Vec<ProfileConfigurator>,
    pub attach_frames: Vec<AttachFrame>,
    pub status_fields: Vec<StatusFields>,
    pub cli_input_policies: BTreeMap<String, CliInputPolicy>,
}

impl Composition {
    fn validate(&self) -> Result<(), String> {
        let mut ids = BTreeSet::new();
        for integration in &self.integrations {
            if !ids.insert(integration.id()) {
                return Err(format!("duplicate integration: {}", integration.id()));
            }
            if !self.provider_labels.contains_key(integration.id()) {
                return Err(format!("missing provider label: {}", integration.id()));
            }
        }
        if self
            .provider_labels
            .keys()
            .any(|id| !ids.contains(id.as_str()))
        {
            return Err("provider label has no registered integration".into());
        }
        let mut kinds = BTreeSet::new();
        for extension in &self.extensions.control {
            if !kinds.insert(extension.kind()) {
                return Err(format!("duplicate control kind: {}", extension.kind()));
            }
            if matches!(
                extension.kind(),
                "lifecycle"
                    | "updater"
                    | "update"
                    | "legacy-migration"
                    | "pack"
                    | "presentation"
                    | "automation"
                    | "prompt"
                    | "sessions"
            ) || extension.max_request_bytes() == 0
                || extension.max_response_bytes() == 0
                || extension.max_request_bytes() > 1024 * 1024
                || extension.max_response_bytes() > 1024 * 1024
            {
                return Err(format!("invalid control extension: {}", extension.kind()));
            }
        }
        let mut commands = BTreeSet::new();
        let mut options = BTreeSet::new();
        for extension in &self.extensions.cli {
            for command in extension.commands() {
                if !commands.insert(*command) || cli::is_common_command(command) {
                    return Err(format!("duplicate CLI command: {command}"));
                }
            }
            for option in extension.options() {
                if !options.insert(option.name) || cli::is_common_option(option.name) {
                    return Err(format!("duplicate CLI option: {}", option.name));
                }
            }
        }
        for command in self.cli_input_policies.keys() {
            if !commands.contains(command.as_str()) {
                return Err(format!("input policy has no registered command: {command}"));
            }
        }
        let mut lifecycle = BTreeSet::new();
        let mut preference_keys = BTreeSet::new();
        let mut preference_options = BTreeSet::new();
        for extension in &self.extensions.lifecycle {
            if !ids.contains(extension.provider()) || !lifecycle.insert(extension.provider()) {
                return Err(format!(
                    "invalid lifecycle provider: {}",
                    extension.provider()
                ));
            }
            for key in extension.preference_patch_keys() {
                if !preference_keys.insert(*key) || preferences::is_common_patch_key(key) {
                    return Err(format!("duplicate preference key: {key}"));
                }
            }
            for option in extension.preference_options() {
                if !preference_options.insert(option.name)
                    || cli::is_common_preference_option(option.name)
                {
                    return Err(format!("duplicate preference option: {}", option.name));
                }
            }
        }
        Ok(())
    }

    fn configure_profile(&self, store: ProfileStore) -> Result<(), String> {
        for configure in &self.profile_configurators {
            configure(store.clone())?;
        }
        Ok(())
    }
}

pub fn run(composition: Composition) -> Result<(), String> {
    composition.validate()?;
    cli::run(Arc::new(composition))
}
