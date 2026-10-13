use super::*;
use crate::automation::{DomainAction, DomainOperationState, DomainRequest};
use crate::preferences::{PreferencePatch, PreferenceSnapshot};
use serde_json::{json, Value};

#[derive(Debug)]
pub(super) struct PreferenceMutationError {
    pub(super) code: &'static str,
    pub(super) detail: String,
    pub(super) committed: bool,
}

impl PreferenceMutationError {
    fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
            committed: false,
        }
    }
}

pub(super) struct PendingPreferenceOperation {
    pub(super) operation_id: String,
    pub(super) target: PreferenceSnapshot,
    pub(super) patch: PreferencePatch,
}

fn preference_target_state(
    target: &PreferenceSnapshot,
    desired: &PreferenceSnapshot,
    patch: &PreferencePatch,
    native_applied: bool,
) -> DomainOperationState {
    let changed = patch
        .language
        .is_some_and(|_| target.language != desired.language)
        || patch
            .bubble_appearance
            .is_some_and(|_| target.bubble_appearance != desired.bubble_appearance)
        || patch
            .show_status_indicators
            .is_some_and(|_| target.show_status_indicators != desired.show_status_indicators)
        || patch
            .menu_bar_mode
            .is_some_and(|_| target.menu_bar_mode != desired.menu_bar_mode)
        || patch
            .observation_host
            .as_ref()
            .is_some_and(|_| target.observation_host != desired.observation_host)
        || patch
            .owner
            .keys()
            .any(|key| target.owner.get(key) != desired.owner.get(key));
    if changed {
        DomainOperationState::Superseded
    } else if native_applied {
        DomainOperationState::Applied
    } else {
        DomainOperationState::Pending
    }
}

fn patch_changes(patch: &PreferencePatch, snapshot: &PreferenceSnapshot) -> Result<bool, String> {
    if patch.language.is_none()
        && patch.bubble_appearance.is_none()
        && patch.show_status_indicators.is_none()
        && patch.menu_bar_mode.is_none()
        && patch.observation_host.is_none()
        && patch.owner.is_empty()
    {
        return Err("preference patch must contain at least one setting".to_owned());
    }
    Ok(patch
        .language
        .is_some_and(|value| value != snapshot.language)
        || patch
            .bubble_appearance
            .is_some_and(|value| value != snapshot.bubble_appearance)
        || patch
            .show_status_indicators
            .is_some_and(|value| value != snapshot.show_status_indicators)
        || patch
            .menu_bar_mode
            .is_some_and(|value| value != snapshot.menu_bar_mode)
        || patch
            .observation_host
            .as_ref()
            .is_some_and(|value| value != &snapshot.observation_host)
        || !patch.owner.is_empty())
}

const MAX_PREFERENCE_RESULT_BYTES: usize = 60 * 1024;

#[derive(Default)]
struct JsonByteCounter(usize);

impl std::io::Write for JsonByteCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        if self.0 > MAX_PREFERENCE_RESULT_BYTES {
            return Err(std::io::Error::other(
                "preferences response exceeds operation limit",
            ));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn preference_result_fits(value: &Value) -> bool {
    serde_json::to_writer(JsonByteCounter::default(), value).is_ok()
}

impl Ui {
    pub(super) fn preference_result(&self) -> Value {
        let desired = self.prefs.snapshot();
        let mut effective = desired.clone();
        effective.language = self.effective_language;
        effective.bubble_appearance = self.native_bubble_appearance;
        if let Ok(state) = self.shared.lock() {
            effective.observation_host = state.observation_mode();
        }
        effective.show_status_indicators = self.native_show_status_indicators;
        effective.menu_bar_mode = self.native_menu_bar_mode;
        let mut pending_reasons = Vec::new();
        if self.prefs.postcommit_error().is_some() {
            effective.owner.clear();
            pending_reasons.push("adapter_apply_failed");
        }
        if self.pending_language.is_some() {
            pending_reasons.push("language_transition");
        }
        if self.composer_marked() {
            pending_reasons.push("ime_composition");
        }
        if self.explicit_gesture_active() {
            pending_reasons.push("gesture");
        }
        if self.status_menu_tracking || appkit_event_tracking_active() {
            pending_reasons.push("tracking");
        }
        if self.pending_bubble_content || self.pending_bubble_scene.is_some() {
            pending_reasons.push("native_deferred");
        }
        if self.native_menu_bar_mode != desired.menu_bar_mode {
            pending_reasons.push("menu_bar_tracking");
        }
        if desired != effective && pending_reasons.is_empty() {
            pending_reasons.push("native_apply");
        }
        let providers = self.shared.lock().ok().map(|state| {
            json!({
                "labels": state.provider_labels(),
                "connections": state.host_connections(),
            })
        });
        json!({
            "revision": self.preference_revision,
            "desired": desired,
            "persisted": desired,
            "effective": effective,
            "pending_reasons": pending_reasons,
            "status_item_visible": self._status_item.isVisible(),
            "effective_locale": self.locale.tag(),
            "registered_providers": providers,
            "owner_apply_error": self.prefs.postcommit_error(),
        })
    }

    // Candidate persistence is the sole settings commit boundary for GUI and CLI.
    pub(super) fn commit_preference_patch(
        &mut self,
        patch: PreferencePatch,
        expected_revision: Option<u64>,
    ) -> Result<PreferenceSnapshot, PreferenceMutationError> {
        self.commit_preference_candidate(patch, expected_revision)
    }

    pub(super) fn commit_gui_observation_patch(
        &mut self,
        patch: PreferencePatch,
    ) -> Result<PreferenceSnapshot, PreferenceMutationError> {
        self.commit_preference_candidate(patch, None)
    }

    fn commit_preference_candidate(
        &mut self,
        patch: PreferencePatch,
        expected_revision: Option<u64>,
    ) -> Result<PreferenceSnapshot, PreferenceMutationError> {
        if self.update_frozen.is_some()
            || self
                .shared
                .lock()
                .map_or(true, |state| state.update_mutation_allowed().is_err())
        {
            return Err(PreferenceMutationError::new(
                "update_preparing",
                "application update is preparing; retry preference changes after it finishes",
            ));
        }
        if expected_revision.is_some_and(|revision| revision != self.preference_revision) {
            return Err(PreferenceMutationError::new(
                "revision_conflict",
                "preference revision changed",
            ));
        }
        if let Some(selection) = &patch.observation_host {
            if let Some(mut context) = self
                .shared
                .lock()
                .map_err(|_| {
                    PreferenceMutationError::new(
                        "state_unavailable",
                        "native state lock is poisoned",
                    )
                })?
                .update_context()
                .cloned()
            {
                context
                    .set_observation_selection(selection)
                    .map_err(|error| PreferenceMutationError::new("invalid_preferences", error))?;
            }
        }
        let previous = self.prefs.snapshot();
        if !patch_changes(&patch, &previous)
            .map_err(|error| PreferenceMutationError::new("invalid_preferences", error))?
            && !(patch.observation_host.is_some() && self.preference_update_pending())
        {
            return Ok(previous);
        }
        let target = self
            .prefs
            .apply_patch(patch, &self.lifecycle_paths.config_dir)
            .map_err(|error| PreferenceMutationError::new("persist_failed", error))?;
        self.preference_revision = self
            .preference_revision
            .checked_add(1)
            .expect("preference revision exhausted");
        let error = self
            .apply_committed_preferences(&previous, &target)
            .err()
            .or_else(|| self.prefs.postcommit_error().map(str::to_owned));
        if let Some(detail) = error {
            return Err(PreferenceMutationError {
                code: "native_apply_failed",
                detail,
                committed: true,
            });
        }
        Ok(target)
    }

    fn apply_committed_preferences(
        &mut self,
        previous: &PreferenceSnapshot,
        target: &PreferenceSnapshot,
    ) -> Result<(), String> {
        if target.language != previous.language || self.pending_language.is_some() {
            self.schedule_committed_language(target.language);
        }
        if target.bubble_appearance != previous.bubble_appearance {
            self.apply_bubble_appearance(target.bubble_appearance);
        }
        if target.show_status_indicators != previous.show_status_indicators {
            self.menu_panel
                .set_show_status_indicators(target.show_status_indicators);
        }
        if target.show_status_indicators != self.native_show_status_indicators {
            self.cards.set_composition_active(self.composer_marked());
            self.bubble_content_dirty = true;
            let scene = self.last_scene.clone();
            self.refresh_bubble_content(&scene);
            if !self.pending_bubble_content && self.pending_bubble_scene.is_none() {
                self.native_show_status_indicators = target.show_status_indicators;
            }
        }
        if target.menu_bar_mode != previous.menu_bar_mode {
            self.menu_panel.set_menu_bar_mode(target.menu_bar_mode);
        }
        self.sync_committed_menu_bar_mode();
        let needs_observation = target.observation_host != previous.observation_host
            || target.owner != previous.owner
            || self.shared.lock().map_or(true, |state| {
                state.observation_mode() != target.observation_host
            });
        if needs_observation {
            let context = {
                let mut state = self
                    .shared
                    .lock()
                    .map_err(|_| "native state lock is poisoned")?;
                state.apply_observation_preferences(self.prefs.observation().clone())?;
                state.update_context().cloned()
            };
            self.app_update.reconcile_context(context);
            self.sync_observation_panel();
        }
        Ok(())
    }

    pub(super) fn sync_committed_menu_bar_mode(&mut self) {
        if self.status_menu_tracking {
            return;
        }
        let scene = self
            .shared
            .lock()
            .ok()
            .map(|state| state.scene())
            .unwrap_or_else(|| self.last_scene.clone());
        self.sync_status_menu(&scene);
        if self._status_item.isVisible() == menu_bar_visible(&scene, self.prefs.menu_bar_mode()) {
            self.native_menu_bar_mode = self.prefs.menu_bar_mode();
        }
    }

    pub(super) fn settle_preference_operations(&mut self) {
        if self.pending_preference_operations.is_empty() {
            return;
        }
        self.sync_committed_menu_bar_mode();
        let desired = self.prefs.snapshot();
        let mut pending = std::mem::take(&mut self.pending_preference_operations);
        for entry in pending.drain(..) {
            let native_applied = self.preference_native_applied(&entry.target, &entry.patch);
            let state =
                preference_target_state(&entry.target, &desired, &entry.patch, native_applied);
            if state == DomainOperationState::Pending {
                self.pending_preference_operations.push(entry);
                continue;
            }
            self.finish_preference_request(
                &entry.operation_id,
                state,
                true,
                state == DomainOperationState::Applied && native_applied,
                Some(self.preference_result()),
                None,
                None,
            );
        }
    }

    fn preference_native_applied(
        &self,
        target: &PreferenceSnapshot,
        patch: &PreferencePatch,
    ) -> bool {
        if !patch.owner.is_empty() && self.prefs.postcommit_error().is_some() {
            return false;
        }
        let observation_needed = patch.observation_host.is_some();
        patch.language.is_none_or(|_| {
            target.language == self.effective_language && self.pending_language.is_none()
        }) && patch
            .bubble_appearance
            .is_none_or(|_| target.bubble_appearance == self.native_bubble_appearance)
            && patch
                .show_status_indicators
                .is_none_or(|_| target.show_status_indicators == self.native_show_status_indicators)
            && patch.menu_bar_mode.is_none_or(|_| {
                target.menu_bar_mode == self.native_menu_bar_mode
                    && self._status_item.isVisible()
                        == menu_bar_visible(&self.last_scene, target.menu_bar_mode)
            })
            && (!observation_needed
                || self.shared.lock().is_ok_and(|state| {
                    patch
                        .observation_host
                        .as_ref()
                        .is_none_or(|_| target.observation_host == state.observation_mode())
                }))
    }

    pub(super) fn preference_update_pending(&self) -> bool {
        if !self.pending_preference_operations.is_empty() || self.pending_language.is_some() {
            return true;
        }
        let desired = self.prefs.snapshot();
        if desired.language != self.effective_language
            || desired.bubble_appearance != self.native_bubble_appearance
            || desired.show_status_indicators != self.native_show_status_indicators
            || desired.menu_bar_mode != self.native_menu_bar_mode
        {
            return true;
        }
        self.shared.lock().map_or(true, |state| {
            desired.observation_host != state.observation_mode()
        })
    }
    pub(super) fn drain_domain_requests(&mut self) {
        // Requests remain in the ledger until admitted; a prepared update
        // cannot silently drain or discard them.
        if self.update_frozen.is_some() {
            return;
        }
        let requests = lock_automation(&self.automation).drain_domain_requests();
        for request in requests {
            let id = request.operation_id.clone();
            if let Err(error) = lock_automation(&self.automation).start_domain_request(&id) {
                eprintln!("desktop-pet: domain request could not start: {error}");
                continue;
            }
            match request.action {
                DomainAction::PreferencesGet {} => {
                    let result = self.preference_result();
                    self.finish_preference_request(
                        &id,
                        DomainOperationState::Applied,
                        false,
                        false,
                        Some(result),
                        None,
                        None,
                    );
                }
                DomainAction::PreferencesSet {
                    patch,
                    expected_revision,
                } => {
                    let submitted = patch.clone();
                    match self.commit_preference_patch(patch, expected_revision) {
                        Ok(target) => {
                            if self.preference_native_applied(&target, &submitted) {
                                let result = self.preference_result();
                                self.finish_preference_request(
                                    &id,
                                    DomainOperationState::Applied,
                                    true,
                                    true,
                                    Some(result),
                                    None,
                                    None,
                                );
                            } else {
                                self.pending_preference_operations.push(
                                    PendingPreferenceOperation {
                                        operation_id: id.clone(),
                                        target,
                                        patch: submitted,
                                    },
                                );
                                self.queue_language_apply();
                                self.finish_preference_request(
                                    &id,
                                    DomainOperationState::Pending,
                                    true,
                                    false,
                                    Some(self.preference_result()),
                                    None,
                                    None,
                                );
                            }
                            self.cli_preference_feedback = Some(
                                crate::i18n::cli_preference_feedback(self.locale, true).to_owned(),
                            );
                        }
                        Err(error) => {
                            let title = if error.code == "revision_conflict" {
                                text(self.locale, Message::PreferenceRevisionConflict)
                            } else {
                                crate::i18n::cli_preference_feedback(self.locale, false)
                            };
                            self.cli_preference_feedback =
                                Some(format!("{title}: {}", error.detail));
                            self.finish_preference_request(
                                &id,
                                DomainOperationState::Failed,
                                error.committed,
                                false,
                                Some(self.preference_result()),
                                Some(error.code.to_owned()),
                                Some(error.detail),
                            );
                        }
                    }
                    self.bubble_content_dirty = true;
                }
                DomainAction::SessionPrompt { key, text } => {
                    self.handle_cli_session_prompt(DomainRequest {
                        instance_id: request.instance_id,
                        operation_id: id,
                        action: DomainAction::SessionPrompt { key, text },
                    });
                }
                action @ (DomainAction::DialogueList {}
                | DomainAction::DialogueRead { .. }
                | DomainAction::DialogueSet { .. }
                | DomainAction::DialogueResetEntry { .. }
                | DomainAction::DialogueResetCharacter { .. }) => {
                    self.handle_cli_dialogue(DomainRequest {
                        instance_id: request.instance_id,
                        operation_id: id,
                        action,
                    });
                }
                action @ (DomainAction::WorktreeInspect { .. }
                | DomainAction::WorktreeRemove { .. }) => {
                    self.handle_cli_worktree(DomainRequest {
                        instance_id: request.instance_id,
                        operation_id: id,
                        action,
                    });
                }
            }
        }
    }

    fn finish_preference_request(
        &self,
        id: &str,
        mut state: DomainOperationState,
        committed: bool,
        native_applied: bool,
        mut result: Option<Value>,
        mut error_code: Option<String>,
        mut error: Option<String>,
    ) {
        if result
            .as_ref()
            .is_some_and(|value| !preference_result_fits(value))
        {
            if committed {
                if let Some(value) = result.as_mut() {
                    if let Some(object) = value.as_object_mut() {
                        object.insert("registered_providers".to_owned(), Value::Null);
                        object.insert(
                            "providers_omitted_reason".to_owned(),
                            json!("response_limit"),
                        );
                    }
                }
                if result
                    .as_ref()
                    .is_some_and(|value| !preference_result_fits(value))
                {
                    result = Some(json!({
                        "revision": self.preference_revision,
                        "settings_omitted_reason": "response_limit",
                        "pending_reasons": ["response_limit"],
                    }));
                }
            } else {
                result = None;
                if state == DomainOperationState::Applied {
                    state = DomainOperationState::Failed;
                    error_code = Some("result_too_large".to_owned());
                    error = Some("preferences result exceeds operation limit".to_owned());
                }
            }
        }
        let mut ledger = lock_automation(&self.automation);
        if let Err(failure) = ledger.finish_domain_operation(
            id,
            state,
            committed,
            native_applied,
            result,
            error_code,
            error,
        ) {
            eprintln!("desktop-pet: domain preference result failed: {failure}");
            let _ = ledger.finish_domain_operation(
                id,
                state,
                committed,
                native_applied,
                None,
                Some("result_error".to_owned()),
                Some(failure),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_settings_track_only_requested_fields() {
        let mut target = Preferences::default().snapshot();
        target.language = LanguagePreference::Ko;
        let patch = PreferencePatch {
            language: Some(LanguagePreference::Ko),
            ..Default::default()
        };
        assert_eq!(
            preference_target_state(&target, &target, &patch, false),
            DomainOperationState::Pending
        );
        assert_eq!(
            preference_target_state(&target, &target, &patch, true),
            DomainOperationState::Applied
        );
        let mut unrelated = target.clone();
        unrelated.show_status_indicators = !target.show_status_indicators;
        assert_eq!(
            preference_target_state(&target, &unrelated, &patch, true),
            DomainOperationState::Applied
        );
        let mut replacement = target.clone();
        replacement.language = LanguagePreference::En;
        assert_eq!(
            preference_target_state(&target, &replacement, &patch, true),
            DomainOperationState::Superseded
        );
    }

    #[test]
    fn host_operations_track_host_without_superseding_unrelated_settings() {
        use desktop_pet_contracts::ObservationSelection;
        let mut target = Preferences::default().snapshot();
        target.observation_host = ObservationSelection::Provider("tern".into());
        let patch = PreferencePatch {
            observation_host: Some(ObservationSelection::Provider("tern".into())),
            ..Default::default()
        };
        assert!(!patch_changes(&patch, &target).unwrap());
        assert_eq!(
            preference_target_state(&target, &target, &patch, false),
            DomainOperationState::Pending
        );
        let mut unrelated = target.clone();
        unrelated
            .owner
            .insert("registered-setting".into(), json!(true));
        assert_eq!(
            preference_target_state(&target, &unrelated, &patch, true),
            DomainOperationState::Applied
        );
        let mut replacement = target.clone();
        replacement.observation_host = ObservationSelection::Auto;
        assert!(patch_changes(&patch, &replacement).unwrap());
        for native_applied in [false, true] {
            assert_eq!(
                preference_target_state(&target, &replacement, &patch, native_applied),
                DomainOperationState::Superseded
            );
        }
        let scope_patch = PreferencePatch {
            owner: serde_json::Map::from_iter([("registered-setting".into(), json!(true))]),
            ..Default::default()
        };
        assert_eq!(
            preference_target_state(&target, &replacement, &scope_patch, true),
            DomainOperationState::Applied
        );
    }

    #[test]
    fn host_save_first_keeps_live_mode_and_scope_when_persistence_fails() {
        use desktop_pet_contracts::ObservationSelection;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "herdr-host-save-first-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("preferences.json");
        let mut prefs = Preferences::default();
        let mut state = crate::state::tests::observed_test_state(ObservationSelection::Provider(
            "herdr".into(),
        ));
        let mut updater = AppUpdateService::new(
            None,
            false,
            desktop_pet_update_coordinator::UpdatePolicy::StableOnly,
            None,
        );
        updater.reconcile_context(state.update_context().cloned());
        prefs
            .apply_patch_in_directory(
                PreferencePatch {
                    observation_host: Some(ObservationSelection::Provider("tern".into())),
                    ..Default::default()
                },
                &directory,
            )
            .unwrap();
        state
            .apply_observation_preferences(prefs.observation().clone())
            .unwrap();
        updater.reconcile_context(state.update_context().cloned());
        let saved_context = state.update_context().cloned();
        let saved_consent = updater.consent_generation();
        let saved_update_revision = updater.revision();
        let saved = prefs.snapshot();
        let saved_bytes = std::fs::read(&path).unwrap();
        let backup = directory.join("saved.json");
        std::fs::rename(&path, &backup).unwrap();
        std::fs::create_dir(&path).unwrap();
        let result = prefs
            .apply_patch_in_directory(
                PreferencePatch {
                    observation_host: Some(ObservationSelection::Provider("herdr".into())),
                    ..Default::default()
                },
                &directory,
            )
            .map(|_| {
                state
                    .apply_observation_preferences(prefs.observation().clone())
                    .unwrap();
                updater.reconcile_context(state.update_context().cloned());
            });
        assert!(result.is_err());
        assert_eq!(prefs.snapshot(), saved);
        assert_eq!(
            state.observation_mode(),
            ObservationSelection::Provider("tern".into())
        );
        assert_eq!(state.observation_preferences(), prefs.observation());
        assert_eq!(state.update_context().cloned(), saved_context);
        assert_eq!(updater.consent_generation(), saved_consent);
        assert_eq!(updater.revision(), saved_update_revision);
        assert_eq!(std::fs::read(&backup).unwrap(), saved_bytes);
        std::fs::remove_dir_all(&path).unwrap();
        std::fs::rename(&backup, &path).unwrap();
        let target = prefs
            .apply_patch_in_directory(
                PreferencePatch {
                    observation_host: Some(ObservationSelection::Auto),
                    ..Default::default()
                },
                &directory,
            )
            .unwrap();
        state
            .apply_observation_preferences(prefs.observation().clone())
            .unwrap();
        updater.reconcile_context(state.update_context().cloned());
        assert_ne!(updater.consent_generation(), saved_consent);
        assert_eq!(
            state.update_context().unwrap().observation_mode().unwrap(),
            ObservationSelection::Auto
        );
        assert_eq!(state.observation_mode(), ObservationSelection::Auto);
        assert_eq!(state.observation_preferences(), prefs.observation());
        assert_eq!(Preferences::load_path(&path).unwrap().snapshot(), target);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn unchanged_settings_do_not_need_another_save() {
        let snapshot = Preferences::default().snapshot();
        assert!(patch_changes(&PreferencePatch::default(), &snapshot).is_err());
        assert!(!patch_changes(
            &PreferencePatch {
                language: Some(snapshot.language),
                show_status_indicators: Some(snapshot.show_status_indicators),
                ..Default::default()
            },
            &snapshot
        )
        .unwrap());
        assert!(patch_changes(
            &PreferencePatch {
                owner: serde_json::Map::from_iter([("registered-setting".into(), json!(true))]),
                ..Default::default()
            },
            &snapshot
        )
        .unwrap());
    }
}
