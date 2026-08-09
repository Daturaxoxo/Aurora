use log::*;
use serde_json::Value;
use slint::ComponentHandle;
use slint::{ModelRc, SharedString, VecModel};

use crate::{LogWindow, MainWindow};
use shared::config;

const TRANSLATIONS_JSON: &str = include_str!("../../../production/Langs/translations.json");

fn entries() -> &'static [Value] {
    static ENTRIES: std::sync::OnceLock<Vec<Value>> = std::sync::OnceLock::new();
    ENTRIES.get_or_init(|| {
        serde_json::from_str(TRANSLATIONS_JSON)
            .expect("translations.json is invalid JSON (should have been caught at build time)")
    })
}

fn first_report(id: String) -> bool {
    static REPORTED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();

    REPORTED
        .get_or_init(Default::default)
        .lock()
        .is_ok_and(|mut reported| reported.insert(id))
}

fn value_for<'a>(entry: &'a Value, lang_code: &str) -> Option<&'a str> {
    entry
        .get(lang_code)
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
}

fn localized<'a>(entry: &'a Value, lang_code: &str) -> &'a str {
    let key = entry["key"].as_str().unwrap_or("<unknown key>");

    if let Some(value) = value_for(entry, lang_code) {
        return value;
    }

    if lang_code != "en" && first_report(format!("{lang_code}/{key}")) {
        warn!(
            "translations.json: key \"{key}\" has no \"{lang_code}\" value, falling back to \"en\""
        );
    }

    value_for(entry, "en").unwrap_or_else(|| {
        if first_report(format!("en/{key}")) {
            error!("translations.json: key \"{key}\" is missing its required \"en\" value");
        }
        key
    })
}

fn build_values(lang_code: &str) -> ModelRc<SharedString> {
    let values: Vec<SharedString> = entries()
        .iter()
        .map(|entry| SharedString::from(localized(entry, lang_code)))
        .collect();

    ModelRc::new(VecModel::from(values))
}

pub fn tr(key: &str) -> String {
    let Some(entry) = entries().iter().find(|e| e["key"].as_str() == Some(key)) else {
        error!("translations.json: no entry for key \"{key}\"");
        return key.to_string();
    };

    localized(entry, &saved_language()).to_string()
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
