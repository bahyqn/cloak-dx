use dioxus::prelude::*;

use crate::services::app_log::AppLog;

#[derive(Clone, Copy, PartialEq, Eq)]
enum LogTab {
    Application,
    Core,
}

/// Logs page.
///
/// Application entries come from [`AppLog`] and live in memory for the session.
/// The proxy core produces a separate stream; its tab is reserved until that
/// capture is wired up.
#[component]
pub fn LogsPage() -> Element {
    let logs = use_context::<AppLog>();
    let mut tab = use_signal(|| LogTab::Application);
    let entries = (logs.entries)();
    let entry_count = entries.len();
    let count_label = if entry_count == 1 {
        "Application log: 1 entry".to_string()
    } else {
        format!("Application log: {entry_count} entries")
    };

    rsx! {
        div { id: "logs-view", class: "view-section active logs-page",
            div { class: "settings-header",
                h2 { class: "page-title", "Logs" }
                div { class: "settings-header-actions",
                    p { class: "settings-hint",
                        i { class: "fa-solid fa-circle-info" }
                        "{count_label}"
                    }
                    button {
                        class: "btn-secondary small",
                        title: "Clear application logs",
                        disabled: entry_count == 0 || tab() != LogTab::Application,
                        onclick: move |_| logs.clear(),
                        i { class: "fa-solid fa-trash-can" }
                        "Clear"
                    }
                }
            }
            div { class: "settings-tabs-nav",
                button {
                    class: if tab() == LogTab::Application { "s-tab-btn active" } else { "s-tab-btn" },
                    onclick: move |_| tab.set(LogTab::Application),
                    i { class: "fa-solid fa-desktop" }
                    "Application"
                }
                button {
                    class: if tab() == LogTab::Core { "s-tab-btn active" } else { "s-tab-btn" },
                    onclick: move |_| tab.set(LogTab::Core),
                    i { class: "fa-solid fa-microchip" }
                    "Proxy Core"
                }
            }
            if tab() == LogTab::Application {
                div { class: "s-tab-content active",
                    div { class: "log-console",
                        if entries.is_empty() {
                            div { class: "log-empty",
                                i { class: "fa-solid fa-list-ul" }
                                p { "No application logs yet" }
                                span {
                                    "Manual Google exit tests and other application events will appear here."
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
                        div { class: "log-empty",
                            i { class: "fa-solid fa-microchip" }
                            p { "Proxy core logs are not captured yet" }
                            span {
                                "The core's own output stays separate from application logs and will be shown here once capture is wired up."
                            }
                        }
                    }
                }
            }
        }
    }
}
