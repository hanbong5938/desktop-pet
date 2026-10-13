#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[cfg(not(target_os = "macos"))]
compile_error!("Desktop Pet requires a macOS target");

mod agent_outcome;
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
mod herdr;
mod herdr_protocol;
mod i18n;
mod interaction;
mod lifecycle;
mod lifecycle_settings_ui;
mod menu_bar_icon;
mod menu_panel;
mod official_catalog;
mod official_characters;
mod pack_authoring;
mod pointer_capture;
mod pose;
mod preferences;
mod remote;
mod rig_renderer;
mod session;
mod session_cards;
mod session_view;
mod socket;
mod sources;
mod state;
mod status_indicator;
mod tern;
#[cfg(test)]
mod transport_tests;
mod ui;
mod update_card;
mod worktree_confirmation;

pub fn run() -> Result<(), String> {
    cli::run()
}
