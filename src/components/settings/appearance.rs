use dioxus::prelude::*;

use crate::theme::Theme;

/// Theme picker shown in Settings → General. The selection is stored through
/// the app-level theme signal, which also persists it to SQLite.
#[component]
pub(super) fn AppearanceSettings() -> Element {
    let mut theme = use_context::<Signal<Theme>>();

    rsx! {
        section { class: "card settings-card",
            h3 {
                i { class: "fa-solid fa-palette" }
                "Appearance"
            }
            p { class: "settings-hint",
                i { class: "fa-solid fa-circle-info" }
                "The selected theme is saved and restored on the next launch."
            }

            div { class: "setting-divider", "Theme" }
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
                            strong { "{option.label()}" }
                            span { "{option.description()}" }
                        }
                    }
                }
            }
        }
    }
}
