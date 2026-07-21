//! Runtime translation loading.
//!
//! Pairs with the `TrKey` / `Tr` globals that `build.rs` generates into
//! `translations.slint` from `production/Langs/translations.json`. The Slint
//! side only ever reads `Tr.values[TrKey.some-key]`; this module is
//! responsible for putting the right strings into `Tr.values` for whichever
//! language is active, and for swapping them all at once when the user
//! changes languages so the whole UI updates in a single reactive step.
//!
//! Adjust `crate::MainWindow` / `crate::Tr` below if your generated window
//! component or import path differs.

use log::*;
use serde_json::Value;
use slint::{ModelRc, SharedString, VecModel};
use slint::ComponentHandle;

use shared::config;
use crate::MainWindow;

/// Embedded at compile time so the shipped binary doesn't depend on a loose
/// file sitting next to the exe. Must stay in the exact array order build.rs
/// saw when it generated `translations.slint` — `Tr.values` is looked up by
/// position (via `TrKey`'s int constants), not by key, so re-ordering this
/// file requires rebuilding.
const TRANSLATIONS_JSON: &str = include_str!("../../../production/Langs/translations.json");

/// Same source `lang-codes.json` that drives `Languages.names` in Slint, so
/// a dropdown index resolves to the same code the UI list was built from.
const LANG_CODES_JSON: &str = include_str!("../../../production/Langs/lang-codes.json");

/// Resolves a `Languages.names` dropdown index (what `language-index-changed`
/// hands you) to a language code, e.g. `1 -> "tr"`.
pub fn lang_code_for_index(index: usize) -> Option<String> {
    let entries: Vec<Value> = serde_json::from_str(LANG_CODES_JSON).ok()?;
    entries
        .get(index)?
        .get("code")?
        .as_str()
        .map(String::from)
}

/// Rebuilds the full `Tr.values` array for `lang_code` and swaps it into the
/// UI in one call. Every `Text` bound to `Tr.values[TrKey.*]` re-renders
/// immediately — no restart needed.
///
/// Falls back to the `"en"` value (with a warning) for any key missing the
/// requested language, matching the same fallback build.rs applies to the
/// compile-time defaults.
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

/// Call once at startup (after the window is constructed) using whatever
/// language code is already saved in config.
pub fn apply_saved_language(ui: &MainWindow) {
    let code = config::get(config::key::LANGUAGE)
        .as_str()
        .unwrap_or("en")
        .to_string();

    apply_language(ui, &code);
}

/// Wire this into your `language-index-changed` callback: saves the new
/// language choice to config, then applies it to the UI.
pub fn on_language_index_changed(ui: &MainWindow, index: usize) {
    let Some(code) = lang_code_for_index(index) else {
        error!("translations: no lang-codes.json entry at index {index}");
        return;
    };

    config::set(config::key::LANGUAGE, code.clone());
    apply_language(ui, &code);
}