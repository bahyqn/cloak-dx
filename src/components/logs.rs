use std::sync::Arc;

use dioxus::prelude::*;

use crate::{
    i18n::I18n,
    services::{app_log::AppLog, proxy_core::ProxyCoreService},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum LogTab {
    Application,
    Core,
}

/// Logs page.
///
/// Application entries come from [`AppLog`] and live in memory for the session.
/// The proxy core writes its own log file; the Core tab polls its tail while
/// the page is open.
#[component]
pub fn LogsPage() -> Element {
    let logs = use_context::<AppLog>();
    let proxy_core = use_context::<Arc<ProxyCoreService>>();
    let i18n = use_context::<I18n>();
    let mut tab = use_signal(|| LogTab::Application);
    let mut core_log = use_signal(String::new);
    let log_probe = Arc::clone(&proxy_core);
    use_future(move || {
        let log_probe = Arc::clone(&log_probe);
        async move {
            loop {
                let probe = Arc::clone(&log_probe);
                let next = tokio::task::spawn_blocking(move || probe.core_log())
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                if core_log() != next {
                    core_log.set(next);
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    });
    let entries = (logs.entries)();
    let entry_count = entries.len();
    let count_label = if entry_count == 1 {
        format!("{}: 1 {}", i18n.t("Application log"), i18n.t("entry"))
    } else {
        format!(
            "{}: {entry_count} {}",
            i18n.t("Application log"),
            i18n.t("entries")
        )
    };

    rsx! {
        div { id: "logs-view", class: "view-section active logs-page",
            div { class: "settings-header",
                h2 { class: "page-title", "{i18n.t(\"Logs\")}" }
                div { class: "settings-header-actions",
                    p { class: "settings-hint",
                        i { class: "fa-solid fa-circle-info" }
                        "{count_label}"
                    }
                    button {
                        class: "btn-secondary small",
                        title: i18n.t("Clear application logs"),
                        disabled: entry_count == 0 || tab() != LogTab::Application,
                        onclick: move |_| logs.clear(),
                        i { class: "fa-solid fa-trash-can" }
                        "{i18n.t(\"Clear\")}"
                    }
                }
            }
            div { class: "settings-tabs-nav",
                button {
                    class: if tab() == LogTab::Application { "s-tab-btn active" } else { "s-tab-btn" },
                    onclick: move |_| tab.set(LogTab::Application),
                    i { class: "fa-solid fa-desktop" }
                    "{i18n.t(\"Application\")}"
                }
                button {
                    class: if tab() == LogTab::Core { "s-tab-btn active" } else { "s-tab-btn" },
                    onclick: move |_| tab.set(LogTab::Core),
                    i { class: "fa-solid fa-microchip" }
                    "{i18n.t(\"Proxy Core\")}"
                }
            }
            if tab() == LogTab::Application {
                div { class: "s-tab-content active",
                    div { class: "log-console",
                        if entries.is_empty() {
                            div { class: "log-empty",
                                i { class: "fa-solid fa-list-ul" }
                                p { "{i18n.t(\"No application logs yet\")}" }
                                span {
                                    "{i18n.t(\"Manual Google exit tests and other application events will appear here.\")}"
                                }
                            }
                        } else {
                            for entry in entries.iter() {
                                div {
                                    key: "{entry.id}",
                                    class: "log-row level-{entry.level.slug()}",
                                    span {
                                        class: "log-time",
                                        title: "{entry.date} {entry.time}",
                                        "{entry.time}"
                                    }
                                    i {
                                        class: "{entry.level.icon()} log-icon",
                                        title: "{entry.level.label()}",
                                    }
                                    span { class: "log-target", "{entry.target}" }
                                    span { class: "log-message", "{entry.message}" }
                                }
                            }
                        }
                    }
                }
            } else {
                div { class: "s-tab-content active",
                    div { class: "log-console",
                        if core_log().is_empty() {
                            div { class: "log-empty",
                                i { class: "fa-solid fa-microchip" }
                                p { "{i18n.t(\"No core logs yet\")}" }
                                span {
                                    "{i18n.t(\"Start the proxy core to capture its output.\")}"
                                }
                            }
                        } else {
                            pre { class: "log-core", "{core_log}" }
                        }
                    }
                }
            }
        }
    }
}
