//! Interface language support.
//!
//! Components never build a separate translation key space: the English
//! string itself is the key (`i18n.t("Settings")`). English renders the
//! string unchanged; Simplified Chinese looks it up in the catalogs below.
//!
//! Every catalog lives in its own module so translations stay grouped by the
//! screen they belong to. Catalogs not covering a string fall back to English.

use std::{collections::HashMap, sync::LazyLock};

use dioxus::prelude::*;

pub(crate) mod dashboard;
pub(crate) mod layout;
pub(crate) mod logs;
pub(crate) mod network;
pub(crate) mod proxies;
pub(crate) mod settings;
pub(crate) mod subscriptions;
pub(crate) mod xray_settings;

/// `app_settings` key that stores the selected interface language.
pub const LANGUAGE_SETTING_KEY: &str = "ui_language";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    English,
    Chinese,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::Chinese];

    pub fn key(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Chinese => "zh-CN",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Chinese => "简体中文",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Language::English => "English interface",
            Language::Chinese => "Simplified Chinese interface",
        }
    }

    /// Unknown values fall back to English so a database written by a newer
    /// build still opens with readable text.
    pub fn from_key(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "zh-cn" | "zh" | "zh_cn" | "zh-hans" => Language::Chinese,
            _ => Language::English,
        }
    }
}

static MESSAGES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for catalog in [
        dashboard::MESSAGES,
        layout::MESSAGES,
        logs::MESSAGES,
        network::MESSAGES,
        proxies::MESSAGES,
        settings::MESSAGES,
        subscriptions::MESSAGES,
        xray_settings::MESSAGES,
    ] {
        map.extend(catalog.iter().copied());
    }
    map
});

/// Returns the text in the requested language. Strings without a translation
/// are shown in English.
pub fn translate(language: Language, text: &str) -> &str {
    match language {
        Language::English => text,
        Language::Chinese => MESSAGES.get(text).copied().unwrap_or(text),
    }
}

/// Translation context provided by [`crate::app::App`]. Reading `t()` inside a
/// component subscribes it to language changes, so switching updates the whole
/// interface without a restart.
#[derive(Clone, Copy)]
pub struct I18n {
    language: Signal<Language>,
}

impl I18n {
    pub fn new(language: Signal<Language>) -> Self {
        Self { language }
    }

    pub fn language(&self) -> Language {
        (self.language)()
    }

    pub fn set_language(&mut self, language: Language) {
        self.language.set(language);
    }

    pub fn t(&self, text: &'static str) -> &'static str {
        translate(self.language(), text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_keys_round_trip() {
        for language in Language::ALL {
            assert_eq!(Language::from_key(language.key()), language);
        }
    }

    #[test]
    fn unknown_language_values_fall_back_to_english() {
        assert_eq!(Language::from_key(""), Language::English);
        assert_eq!(Language::from_key("fr"), Language::English);
        assert_eq!(Language::from_key("ZH-cn"), Language::Chinese);
    }

    #[test]
    fn english_returns_the_source_text_unchanged() {
        assert_eq!(translate(Language::English, "Appearance"), "Appearance");
        assert_eq!(
            translate(Language::English, "not in any catalog"),
            "not in any catalog"
        );
    }

    #[test]
    fn chinese_translates_known_strings_and_keeps_unknown_ones() {
        assert_eq!(translate(Language::Chinese, "Appearance"), "外观");
        assert_eq!(translate(Language::Chinese, "Language"), "语言");
        assert_eq!(
            translate(Language::Chinese, "not in any catalog"),
            "not in any catalog"
        );
    }

    #[test]
    fn no_catalog_entry_is_empty() {
        for catalog in [
            dashboard::MESSAGES,
            layout::MESSAGES,
            logs::MESSAGES,
            network::MESSAGES,
            proxies::MESSAGES,
            settings::MESSAGES,
            subscriptions::MESSAGES,
            xray_settings::MESSAGES,
        ] {
            for (source, target) in catalog {
                assert!(!source.is_empty(), "empty source string in catalog");
                assert!(!target.is_empty(), "empty translation for {source}");
            }
        }
    }
}
