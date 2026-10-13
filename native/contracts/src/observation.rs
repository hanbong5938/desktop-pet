use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::str::FromStr;

pub const OBSERVATION_SELECTION_ENV: &str = "DESKTOP_PET_OBSERVATION_MODE";

/// New profiles select all registered providers. Provider keys are opaque;
/// registration, not this type, decides whether a provider is available.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ObservationSelection {
    #[default]
    Auto,
    Provider(String),
}

impl ObservationSelection {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Auto => "auto",
            Self::Provider(provider) => provider,
        }
    }

    pub fn includes_provider(&self, provider: &str) -> bool {
        match self {
            Self::Auto => true,
            Self::Provider(selected) => selected == provider,
        }
    }

    /// Only for previously persisted settings/captured legacy input. A missing
    /// host in an existing old profile meant Herdr; a new profile must instead
    /// use Default. The old simultaneous token is not a new-input alias.
    pub fn migrate_legacy_input(value: Option<&str>) -> Result<Self, String> {
        match value {
            None | Some("herdr") => Ok(Self::Provider("herdr".into())),
            Some("tern") => Ok(Self::Provider("tern".into())),
            Some("auto" | "both") => Ok(Self::Auto),
            Some(value) => Err(format!("invalid legacy observation mode: {value:?}")),
        }
    }
}

impl FromStr for ObservationSelection {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "" | "both" => Err(format!("invalid observation selection: {value:?}")),
            provider => Ok(Self::Provider(provider.to_owned())),
        }
    }
}

impl Serialize for ObservationSelection {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Self::Provider(provider) = self {
            if matches!(provider.as_str(), "" | "auto" | "both") {
                return Err(serde::ser::Error::custom(
                    "reserved observation provider key",
                ));
            }
        }
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ObservationSelection {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let token = String::deserialize(deserializer)?;
        token.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults_are_distinct_from_legacy_absence() {
        assert_eq!(ObservationSelection::default(), ObservationSelection::Auto);
        assert_eq!(
            ObservationSelection::migrate_legacy_input(None).unwrap(),
            ObservationSelection::Provider("herdr".into())
        );
        assert_eq!(
            ObservationSelection::migrate_legacy_input(Some("both")).unwrap(),
            ObservationSelection::Auto
        );
        assert!("both".parse::<ObservationSelection>().is_err());
        assert!(serde_json::from_str::<ObservationSelection>("\"both\"").is_err());
    }

    #[test]
    fn opaque_provider_selection_round_trips_without_fixed_host_fields() {
        let selection = ObservationSelection::Provider("other-provider".into());
        let encoded = serde_json::to_string(&selection).unwrap();
        assert_eq!(encoded, "\"other-provider\"");
        assert_eq!(
            serde_json::from_str::<ObservationSelection>(&encoded).unwrap(),
            selection
        );
        assert!(selection.includes_provider("other-provider"));
        assert!(!selection.includes_provider("herdr"));
        assert!(ObservationSelection::Auto.includes_provider("other-provider"));
        assert_eq!(
            serde_json::to_string(&ObservationSelection::Auto).unwrap(),
            "\"auto\""
        );
    }
}
