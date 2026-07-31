use log::*;
use serde_json::Value;
use slint::ComponentHandle;
use slint::{ModelRc, SharedString, VecModel};

use crate::MainWindow;
use shared::config;

const TRANSLATIONS_JSON: &str = include_str!("../../../production/Langs/translations.json");

pub fn apply_language(ui: &MainWindow, lang_code: &str) {
    let entries: Vec<Value> = serde_json::from_str(TRANSLATIONS_JSON)
        .expect("translations.json is invalid JSON (should have been caught at build time)");

    let values: Vec<SharedString> = entries
        .iter()
        .map(|entry| {
            let key = entry["key"].as_str().unwrap_or("<unknown key>");

            let value = entry
                .get(lang_code)
                .and_then(|v| v.as_str())
                .or_else(|| {
                    warn!(
                        "translations.json: key \"{key}\" has no \"{lang_code}\" value, falling back to \"en\""
                    );
                    entry["en"].as_str()
                })
                .unwrap_or_else(|| {
                    panic!("translations.json: key \"{key}\" is missing its required \"en\" value")
                });

            SharedString::from(value)
        })
        .collect();

    ui.global::<crate::Tr>()
        .set_values(ModelRc::new(VecModel::from(values)));
}

pub fn apply_saved_language(ui: &MainWindow) {
    let code = config::get(config::key::LANGUAGE)
        .as_str()
        .unwrap_or("en")
        .to_string();

    apply_language(ui, &code);
}