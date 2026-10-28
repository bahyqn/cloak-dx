use dioxus::prelude::*;

use crate::{
    i18n::{I18n, Language},
    theme::Theme,
};

/// Theme and language pickers shown in Settings → General. Both selections are
/// stored through app-level signals, which persist them to SQLite; reading the
/// language signal here also re-renders the picker itself on change.
#[component]
pub(super) fn AppearanceSettings() -> Element {
    let mut theme = use_context::<Signal<Theme>>();
    let mut i18n = use_context::<I18n>();

    rsx! {
        section { class: "card settings-card",
            h3 {
                i { class: "fa-solid fa-palette" }
                "{i18n.t(\"Appearance\")}"
            }
            p { class: "settings-hint",
                i { class: "fa-solid fa-circle-info" }
                "{i18n.t(\"Theme and language are saved and restored on the next launch.\")}"
            }

            div { class: "setting-divider", "{i18n.t(\"Theme\")}" }
            div { class: "theme-options",
                for option in Theme::ALL {
                    button {
                        class: if theme() == option { "theme-option active" } else { "theme-option" },
                        onclick: move |_| theme.set(option),
                        span {
                            class: if option == Theme::Dark {
                                "theme-swatch theme-swatch-dark"
                            } else {
                                "theme-swatch theme-swatch-gnome"
                            },
                        }
                        span { class: "theme-option-text",
                            strong { "{i18n.t(option.label())}" }
                            span { "{i18n.t(option.description())}" }
                        }
                    }
                }
            }

            div { class: "setting-divider", "{i18n.t(\"Language\")}" }
            div { class: "theme-options",
                for language in Language::ALL {
                    button {
                        class: if i18n.language() == language { "theme-option active" } else { "theme-option" },
                        onclick: move |_| i18n.set_language(language),
                        span { class: "language-badge",
                            if language == Language::Chinese { "中" } else { "EN" }
                        }
                        span { class: "theme-option-text",
                            strong { "{language.label()}" }
                            span { "{i18n.t(language.description())}" }
                        }
                    }
                }
            }
        }
    }
}
