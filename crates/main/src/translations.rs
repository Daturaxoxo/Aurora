use log::*;
use serde_json::Value;
use slint::ComponentHandle;
use slint::{ModelRc, SharedString, VecModel};

use crate::{LogWindow, MainWindow};
use shared::config;

const TRANSLATIONS_JSON: &str = include_str!("../../../production/Langs/translations.json");

fn build_values(lang_code: &str) -> ModelRc<SharedString> {
    let entries: Vec<Value> = serde_json::from_str(TRANSLATIONS_JSON)
        .expect("translations.json is invalid JSON (should have been caught at build time)");

    let values: Vec<SharedString> = entries
        .iter()
        .map(|entry| {
            let key = entry["key"].as_str().unwrap_or("<unknown key>");

            let value = entry
                .get(lang_code)
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty())
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

    ModelRc::new(VecModel::from(values))
}

pub fn apply_language(ui: &MainWindow, lang_code: &str) {
    ui.global::<crate::Tr>().set_values(build_values(lang_code));
}

pub fn apply_language_to_log_window(ui: &LogWindow, lang_code: &str) {
    ui.global::<crate::Tr>().set_values(build_values(lang_code));
}

fn saved_language() -> String {
    config::get(config::key::LANGUAGE)
        .as_str()
        .unwrap_or("en")
        .to_string()
}

pub fn apply_saved_language(ui: &MainWindow) {
    apply_language(ui, &saved_language());
}

pub fn apply_saved_language_to_log_window(ui: &LogWindow) {
    apply_language_to_log_window(ui, &saved_language());
}
