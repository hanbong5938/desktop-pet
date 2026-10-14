//! Only the compiled bridge accepts and emits the historical observation wire.
use desktop_pet_contracts::ObservationSelection;
use serde::{Deserialize, Deserializer, Serializer};
use std::collections::BTreeMap;

pub fn observation_from_environment(
    environment: &BTreeMap<String, String>,
) -> Result<ObservationSelection, String> {
    ObservationSelection::migrate_legacy_input(
        environment
            .get(super::identity::OBSERVATION_ENV)
            .map(String::as_str),
    )
}

pub fn observation_token(selection: &ObservationSelection) -> Result<&str, String> {
    match selection {
        ObservationSelection::Auto => Ok("both"),
        ObservationSelection::Provider(provider)
            if matches!(provider.as_str(), "herdr" | "tern") =>
        {
            Ok(provider)
        }
        _ => Err("legacy observation codec cannot represent this provider".into()),
    }
}

pub fn serialize_observation<S: Serializer>(
    selection: &ObservationSelection,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(observation_token(selection).map_err(serde::ser::Error::custom)?)
}

pub fn deserialize_observation<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<ObservationSelection, D::Error> {
    let value = String::deserialize(deserializer)?;
    ObservationSelection::migrate_legacy_input(Some(&value)).map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_wire_keeps_both_at_the_boundary() {
        assert_eq!(
            observation_token(&ObservationSelection::Auto).unwrap(),
            "both"
        );
        assert_eq!(
            ObservationSelection::migrate_legacy_input(Some("both")).unwrap(),
            ObservationSelection::Auto
        );
        assert!(observation_token(&ObservationSelection::Provider("other".into())).is_err());
    }
}
