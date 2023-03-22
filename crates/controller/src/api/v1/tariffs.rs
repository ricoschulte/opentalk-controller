// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use std::collections::{HashMap, HashSet};

use db_storage::tariffs::Tariff;
use serde::Serialize;
use types::core::TariffId;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct TariffResource {
    pub id: TariffId,
    pub name: String,
    pub quotas: HashMap<String, u32>,
    pub enabled_modules: HashSet<String>,
}

impl TariffResource {
    pub fn from_tariff(tariff: Tariff, available_modules: &[&str]) -> TariffResource {
        let disabled_modules: HashSet<&str> =
            HashSet::from_iter(tariff.disabled_modules.iter().map(String::as_str));
        let available_modules: HashSet<&str> =
            HashSet::from_iter(available_modules.iter().cloned());

        let enabled_modules = available_modules
            .difference(&disabled_modules)
            .map(ToString::to_string)
            .collect();

        TariffResource {
            id: tariff.id,
            name: tariff.name,
            quotas: tariff.quotas.0,
            enabled_modules,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn tariff_to_participant_tariff() {
        let tariff = Tariff {
            id: TariffId::nil(),
            name: "test".into(),
            created_at: Default::default(),
            updated_at: Default::default(),
            quotas: Default::default(),
            disabled_modules: vec![
                "whiteboard".to_string(),
                "timer".to_string(),
                "media".to_string(),
                "polls".to_string(),
            ],
        };
        let available_modules = vec!["chat", "media", "polls", "whiteboard", "timer"];

        let expected = json!({
            "id": "00000000-0000-0000-0000-000000000000",
            "name": "test",
            "quotas": {},
            "enabled_modules": ["chat"],
        });

        let actual =
            serde_json::to_value(TariffResource::from_tariff(tariff, &available_modules)).unwrap();

        assert_eq!(actual, expected);
    }
}
