use herdr_update_coordinator::ObservationMode;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

const REMOTE_PREFIX: &str = "machine:";
const MAX_MACHINES: usize = 64;
const MAX_MACHINE_ID_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct HostConnections {
    pub herdr: usize,
    pub tern: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SourceRoute {
    HerdrLocal(PathBuf),
    HerdrRemote {
        machine: String,
    },
    TernWindow {
        window: String,
        epoch: u64,
        host: String,
        observation_generation: u64,
    },
}

impl SourceRoute {
    /// Worktree removal requires a Herdr-local route, not local-looking metadata.
    pub(crate) fn can_remove_worktree(&self) -> bool {
        matches!(self, Self::HerdrLocal(_))
    }
}

fn default_local() -> bool {
    true
}

fn legacy_host() -> ObservationMode {
    ObservationMode::Herdr
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub(crate) struct ObservationPreferences {
    #[serde(default = "legacy_host")]
    pub host: ObservationMode,
    #[serde(default = "default_local")]
    pub local: bool,
    #[serde(default)]
    pub remote: bool,
    #[serde(default)]
    pub machines: Vec<String>,
}

impl Default for ObservationPreferences {
    fn default() -> Self {
        Self {
            host: ObservationMode::Both,
            local: true,
            remote: false,
            machines: Vec::new(),
        }
    }
}

impl ObservationPreferences {
    pub(crate) fn includes(&self, source: &str) -> bool {
        if source.starts_with("tern:") {
            return self.host.supports_tern();
        } else if !self.host.supports_herdr() {
            return false;
        }
        match remote_machine_id(source) {
            Some(id) => self.remote && self.machines.iter().any(|selected| selected == id),
            None if source.starts_with(REMOTE_PREFIX) => false,
            None => self.local,
        }
    }

    pub(crate) fn sanitize(&mut self) {
        let mut seen = HashSet::new();
        self.machines.retain(|id| {
            !id.is_empty()
                && id.len() <= MAX_MACHINE_ID_BYTES
                && !id.contains('\0')
                && seen.len() < MAX_MACHINES
                && seen.insert(id.clone())
        });
    }
}

pub(crate) fn remote_source(id: &str) -> String {
    format!("{REMOTE_PREFIX}{id}")
}

pub(crate) fn remote_machine_id(source: &str) -> Option<&str> {
    source
        .strip_prefix(REMOTE_PREFIX)
        .filter(|id| !id.is_empty())
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum MachineStatus {
    #[default]
    NotSelected,
    Connecting,
    Online,
    Offline,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MachineInfo {
    pub id: String,
    pub label: String,
    pub remote_session: String,
    pub enabled: bool,
    pub status: MachineStatus,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SourceCatalog {
    pub initialized: bool,
    pub machines: Vec<MachineInfo>,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_observation_defaults_are_auto_but_missing_disk_host_is_herdr() {
        assert_eq!(
            ObservationPreferences::default().host,
            ObservationMode::Both
        );
        let legacy: ObservationPreferences = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.host, ObservationMode::Herdr);
        assert!(legacy.local);
        assert!(!legacy.remote);
        for token in ["auto", "both"] {
            let migrated: ObservationPreferences =
                serde_json::from_str(&format!("{{\"host\":\"{token}\"}}")).unwrap();
            assert_eq!(migrated.host, ObservationMode::Both);
            assert_eq!(serde_json::to_value(migrated).unwrap()["host"], "auto");
        }
    }

    #[test]
    fn only_local_herdr_routes_authorize_worktree_removal() {
        let herdr = SourceRoute::HerdrLocal(PathBuf::from("/private/herdr.sock"));
        let remote = SourceRoute::HerdrRemote {
            machine: "machine-a".to_owned(),
        };
        let tern = SourceRoute::TernWindow {
            window: "window-a".to_owned(),
            epoch: 1,
            host: "0".into(),
            observation_generation: 1,
        };
        assert!(herdr.can_remove_worktree());
        assert!(!remote.can_remove_worktree());
        assert!(!tern.can_remove_worktree());
    }

    #[test]
    fn host_filter_keeps_provider_identity_and_saved_scope_separate() {
        let mut preferences = ObservationPreferences {
            local: true,
            remote: true,
            machines: vec!["east".into()],
            ..ObservationPreferences::default()
        };
        let tern = crate::tern::source_id("window-a", "0");
        let remote = remote_source("east");
        assert!(preferences.includes("/private/herdr.sock"));
        assert!(preferences.includes(&remote));
        assert!(preferences.includes(&tern));
        preferences.host = ObservationMode::Tern;
        assert!(!preferences.includes("/private/herdr.sock"));
        assert!(!preferences.includes(&remote));
        assert!(preferences.includes(&tern));
        preferences.host = ObservationMode::Herdr;
        assert!(preferences.includes(&remote));
        assert!(!preferences.includes(&tern));
        assert!(preferences.remote);
        assert_eq!(preferences.machines, vec!["east".to_owned()]);
    }

    #[test]
    fn opaque_machine_ids_survive_sanitization_without_remote_enabled() {
        let opaque = " machine:/🌲 /key=abc ".to_owned();
        let boundary = "x".repeat(MAX_MACHINE_ID_BYTES);
        let mut preferences = ObservationPreferences {
            host: ObservationMode::Herdr,
            local: false,
            remote: false,
            machines: vec![
                String::new(),
                opaque.clone(),
                opaque.clone(),
                "x".repeat(MAX_MACHINE_ID_BYTES + 1),
                boundary.clone(),
                "invalid\0id".to_owned(),
            ],
        };
        preferences.sanitize();
        assert_eq!(preferences.machines, vec![opaque.clone(), boundary]);
        assert!(!preferences.includes(&remote_source(&opaque)));
        preferences.remote = true;
        assert!(preferences.includes(&remote_source(&opaque)));
        assert!(!preferences.includes("machine:"));
        assert!(!preferences.includes("/tmp/local.sock"));
    }

    #[test]
    fn selection_cap_retains_first_distinct_machine_ids() {
        let mut preferences = ObservationPreferences {
            machines: (0..MAX_MACHINES + 2)
                .map(|index| format!("opaque-{index}"))
                .collect(),
            ..ObservationPreferences::default()
        };
        preferences.sanitize();
        assert_eq!(preferences.machines.len(), MAX_MACHINES);
        assert_eq!(
            preferences.machines.first().map(String::as_str),
            Some("opaque-0")
        );
        assert_eq!(
            preferences.machines.last().map(String::as_str),
            Some("opaque-63")
        );
    }
}
