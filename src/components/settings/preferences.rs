use std::sync::Arc;

use dioxus::prelude::*;

use crate::{
    i18n::{I18n, Language},
    services::{
        database::Database,
        subscription_manager::{updates_via_proxy, UPDATE_VIA_PROXY_SETTING},
    },
    theme::Theme,
};

/// Preferences card shown at the top of Settings → General.
///
/// Theme and language used to be rendered as large option lists; both are
/// single-choice values, so they live in compact selects next to the update
/// preference instead of filling half the page. Every change is applied to the
/// app-level signals immediately, which also persists it to SQLite.
#[component]
pub(super) fn PreferencesSettings() -> Element {
    let mut theme = use_context::<Signal<Theme>>();
    let mut i18n = use_context::<I18n>();
    let database = use_context::<Arc<Database>>();
    let mut update_via_proxy = use_signal(|| updates_via_proxy(&database));
    let save_database = Arc::clone(&database);

    rsx! {
        section { class: "card settings-card",
            h3 {
                i { class: "fa-solid fa-sliders" }
                "{i18n.t(\"Preferences\")}"
            }
            p { class: "settings-hint",
                i { class: "fa-solid fa-circle-info" }
                "{i18n.t(\"Changes apply immediately and are restored on the next launch.\")}"
            }

            div { class: "setting-item",
                div { class: "setting-info",
                    label { "{i18n.t(\"Theme\")}" }
                    span { "{i18n.t(\"Colour scheme for the app window\")}" }
                }
                select {
                    class: "setting-select",
                    value: "{theme().key()}",
                    onchange: move |event| theme.set(Theme::from_key(&event.value())),
                    for theme_option in Theme::ALL {
                        option {
                            value: "{theme_option.key()}",
                            selected: theme() == theme_option,
                            "{i18n.t(theme_option.label())}"
                        }
                    }
                }
            }
            div { class: "setting-item",
                div { class: "setting-info",
                    label { "{i18n.t(\"Language\")}" }
                    span { "{i18n.t(\"Language of the interface\")}" }
                }
                select {
                    class: "setting-select",
                    value: "{i18n.language().key()}",
                    onchange: move |event| i18n.set_language(Language::from_key(&event.value())),
                    for language in Language::ALL {
                        option {
                            value: "{language.key()}",
                            selected: i18n.language() == language,
                            "{language.label()}"
                        }
                    }
                }
            }

            div { class: "setting-item",
                div { class: "setting-info",
                    label { "{i18n.t(\"Update through the proxy\")}" }
                    span { "{i18n.t(\"Download through the running core so a blocked subscription URL stays reachable. While the core is stopped, updates go out directly.\")}" }
                }
                label { class: "toggle-switch small",
                    input {
                        r#type: "checkbox",
                        id: "update-via-proxy-toggle",
                        checked: update_via_proxy(),
                        onchange: move |event| {
                            let enabled = event.checked();
                            update_via_proxy.set(enabled);
                            let _ = save_database.set_setting(
                                UPDATE_VIA_PROXY_SETTING,
                                if enabled { "1" } else { "0" },
                            );
                        },
                    }
                    span { class: "slider" }
                }
            }
        }
    }
}
