use desktop_pet_contracts::ObservationSelection;
use serde::{Deserialize, Deserializer, Serialize};

/// Common observation chooses registered providers only. Host-owned locality,
/// endpoints and machine selections are interpreted by adapter extensions.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub(crate) struct ObservationPreferences {
    #[serde(default = "legacy_selection", deserialize_with = "read_selection")]
    pub host: ObservationSelection,
}

fn legacy_selection() -> ObservationSelection {
    ObservationSelection::migrate_legacy_input(None).expect("known legacy default")
}

fn read_selection<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<ObservationSelection, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value == "both" {
        ObservationSelection::migrate_legacy_input(Some(&value)).map_err(serde::de::Error::custom)
    } else {
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl Default for ObservationPreferences {
    fn default() -> Self {
        Self {
            host: ObservationSelection::Auto,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_default_and_prior_missing_selection_are_distinct() {
        assert_eq!(
            ObservationPreferences::default().host,
            ObservationSelection::Auto
        );
        let prior: ObservationPreferences = serde_json::from_str("{}").unwrap();
        assert_eq!(prior.host.as_str(), "herdr");
        for input in ["auto", "both"] {
            let prior: ObservationPreferences =
                serde_json::from_str(&format!("{{\"host\":\"{input}\"}}")).unwrap();
            assert_eq!(prior.host, ObservationSelection::Auto);
            assert_eq!(serde_json::to_value(prior).unwrap()["host"], "auto");
        }
    }

    #[test]
    fn provider_selection_remains_opaque() {
        let preferences: ObservationPreferences = serde_json::from_str(
            r#"{"host":"custom-provider","local":false,"machines":["ignored-host-owned"]}"#,
        )
        .unwrap();
        assert_eq!(
            preferences.host,
            ObservationSelection::Provider("custom-provider".into())
        );
        assert_eq!(
            serde_json::to_value(preferences).unwrap(),
            serde_json::json!({"host":"custom-provider"})
        );
    }
}
