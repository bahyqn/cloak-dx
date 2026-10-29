use std::sync::Arc;

use dioxus::prelude::*;

use crate::{
    app::AppPage,
    i18n::I18n,
    services::{
        app_log::{AppLog, LogLevel},
        database::Database,
        proxy_core::ProxyCoreService,
    },
};

const SIDEBAR_COLLAPSED_SETTING: &str = "sidebar_collapsed";

#[component]
pub fn Sidebar(mut current_page: Signal<AppPage>) -> Element {
    let i18n = use_context::<I18n>();
    let database = use_context::<Arc<Database>>();
    // Start with the state the window was closed with. The value lives in
    // `app_settings` so it survives a restart just like theme and language.
    let mut collapsed = use_signal(|| load_sidebar_collapsed(&database));
    let data_revision = use_context::<Signal<u64>>();
    let proxy_core = use_context::<Arc<ProxyCoreService>>();
    let app_log = use_context::<AppLog>();
    let mut core_running = use_signal(|| proxy_core.status().running);
    let mut core_engine = use_signal(|| proxy_core.status().engine);
    let mut core_busy = use_signal(|| false);
    // The core can also stop on its own (or be toggled from the dashboard), so
    // the indicator follows the service instead of only reacting to clicks.
    let status_probe = Arc::clone(&proxy_core);
    use_future(move || {
        let status_probe = Arc::clone(&status_probe);
        async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let status = status_probe.status();
                if core_running() != status.running {
                    core_running.set(status.running);
                }
                if core_engine() != status.engine {
                    core_engine.set(status.engine);
                }
            }
        }
    });
    // Until the core runs, show the engine picked in Settings. Changing that
    // selection bumps `data_revision`, which re-renders the sidebar.
    let _revision = data_revision();
    let selected_engine = database
        .core_installs()
        .ok()
        .and_then(|installs| installs.into_iter().find(|install| install.selected))
        .map(|install| install.engine);
    let engine = core_engine().or(selected_engine);
    let core_name = core_display_name(engine.as_deref());

    // Clicking the indicator toggles the core, matching the dashboard button.
    // Starting can take a few seconds, so the control is disabled meanwhile.
    let toggle_database = Arc::clone(&database);
    let toggle_core = Arc::clone(&proxy_core);
    let collapse_database = Arc::clone(&database);
    let on_toggle_core = move |_| {
        if core_busy() {
            return;
        }
        core_busy.set(true);
        let current = toggle_core.status();
        if current.running {
            toggle_core.stop();
            app_log.record(
                LogLevel::Info,
                "Proxy core",
                format!(
                    "{}: {}",
                    current.engine_name(),
                    i18n.t("Proxy core stopped.")
                ),
            );
        } else {
            match toggle_core.start_selected_subscription(&toggle_database) {
                Ok(status) => {
                    if let Some(warning) = status.warning.clone() {
                        app_log.record(
                            LogLevel::Error,
                            "Proxy core",
                            format!("{}: {warning}", status.engine_name()),
                        );
                    } else {
                        app_log.record(
                            LogLevel::Success,
                            "Proxy core",
                            format!(
                                "{}: {}",
                                status.engine_name(),
                                i18n.t("Proxy core started.")
                            ),
                        );
                    }
                }
                Err(error) => {
                    let status = toggle_core.status();
                    app_log.record(
                        LogLevel::Error,
                        "Proxy core",
                        format!("{}: {error}", status.engine_name()),
                    );
                }
            }
        }
        let status = toggle_core.status();
        core_running.set(status.running);
        core_engine.set(status.engine);
        core_busy.set(false);
    };
    let running = core_running();
    let toggle_label = if running {
        i18n.t("Stop proxy core")
    } else {
        i18n.t("Start proxy core")
    };

    rsx! {
        aside { class: if collapsed() { "sidebar collapsed" } else { "sidebar" },
            div { class: "sidebar-header",
                div { class: "logo",
                    i { class: "fa-solid fa-box-open" }
                    div { class: "logo-icon" }
                    span { "{core_name}" }
                }
                button {
                    id: "sidebar-toggle",
                    class: "sidebar-toggle",
                    "aria-label": i18n.t("Toggle sidebar"),
                    "aria-expanded": (!collapsed()).to_string(),
                    onclick: move |_| {
                        let next = !collapsed();
                        collapsed.set(next);
                        let _ = collapse_database.set_setting(
                            SIDEBAR_COLLAPSED_SETTING,
                            if next { "1" } else { "0" },
                        );
                    },
                    i { class: "fa-solid fa-bars" }
                }
            }
            nav { class: "nav-links",
                button {
                    r#type: "button",
                    class: if current_page() == AppPage::Dashboard { "nav-item active" } else { "nav-item" },
                    "aria-current": "page",
                    onclick: move |_| current_page.set(AppPage::Dashboard),
                    i { class: "fa-solid fa-house" }
                    span { "{i18n.t(\"Dashboard\")}" }
                }
                button {
                    r#type: "button",
                    class: if current_page() == AppPage::Proxies { "nav-item active" } else { "nav-item" },
                    onclick: move |_| current_page.set(AppPage::Proxies),
                    i { class: "fa-solid fa-server" }
                    span { "{i18n.t(\"Proxies\")}" }
                }
                button {
                    r#type: "button",
                    class: if current_page() == AppPage::Subscriptions { "nav-item active" } else { "nav-item" },
                    onclick: move |_| current_page.set(AppPage::Subscriptions),
                    i { class: "fa-solid fa-link" }
                    span { "{i18n.t(\"Subscribes\")}" }
                }
                button {
                    r#type: "button",
                    class: if current_page() == AppPage::Network { "nav-item active" } else { "nav-item" },
                    onclick: move |_| current_page.set(AppPage::Network),
                    i { class: "fa-solid fa-earth-americas" }
                    span { "{i18n.t(\"Network\")}" }
                }
                button {
                    r#type: "button",
                    class: if current_page() == AppPage::Logs { "nav-item active" } else { "nav-item" },
                    onclick: move |_| current_page.set(AppPage::Logs),
                    i { class: "fa-solid fa-list-ul" }
                    span { "{i18n.t(\"Logs\")}" }
                }
                button {
                    r#type: "button",
                    class: if current_page() == AppPage::Settings { "nav-item active" } else { "nav-item" },
                    onclick: move |_| current_page.set(AppPage::Settings),
                    i { class: "fa-solid fa-gear" }
                    span { "{i18n.t(\"Settings\")}" }
                }
            }
            button {
                r#type: "button",
                class: if running { "status-indicator connected" } else { "status-indicator" },
                title: "{toggle_label}",
                "aria-label": "{toggle_label}",
                disabled: core_busy(),
                onclick: on_toggle_core,
                span { class: "dot" }
                span { class: "text",
                    if running {
                        "{i18n.t(\"Connected\")}"
                    } else {
                        "{i18n.t(\"Disconnected\")}"
                    }
                }
                i {
                    class: if core_busy() {
                        "fa-solid fa-circle-notch fa-spin status-power"
                    } else {
                        "fa-solid fa-power-off status-power"
                    }
                }
            }
        }
    }
}

/// Reads the persisted collapse state. Missing values and unreadable rows mean
/// the sidebar starts expanded, which is the same default as a new install.
fn load_sidebar_collapsed(database: &Database) -> bool {
    database
        .get_setting(SIDEBAR_COLLAPSED_SETTING)
        .ok()
        .flatten()
        .is_some_and(|value| value == "1")
}

/// Short label for the engine shown in the sidebar header.
fn core_display_name(engine: Option<&str>) -> &str {
    match engine {
        Some("xray") => "xray",
        Some("sing-box") => "sing-box",
        Some(other) => other,
        None => "cloak-dx",
    }
}

#[cfg(test)]
mod tests {
    use super::{core_display_name, load_sidebar_collapsed, SIDEBAR_COLLAPSED_SETTING};
    use crate::services::database::Database;

    #[test]
    fn core_names_are_short_engine_labels() {
        assert_eq!(core_display_name(Some("xray")), "xray");
        assert_eq!(core_display_name(Some("sing-box")), "sing-box");
        assert_eq!(core_display_name(Some("other-core")), "other-core");
        assert_eq!(core_display_name(None), "cloak-dx");
    }

    #[test]
    fn sidebar_collapse_is_loaded_from_the_database() {
        let database = Database::in_memory().unwrap();
        assert!(!load_sidebar_collapsed(&database));

        database
            .set_setting(SIDEBAR_COLLAPSED_SETTING, "1")
            .unwrap();
        assert!(load_sidebar_collapsed(&database));

        database
            .set_setting(SIDEBAR_COLLAPSED_SETTING, "0")
            .unwrap();
        assert!(!load_sidebar_collapsed(&database));
    }
}
