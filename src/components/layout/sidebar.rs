use std::sync::Arc;

use dioxus::prelude::*;

use crate::{app::AppPage, services::proxy_core::ProxyCoreService};

#[component]
pub fn Sidebar(mut current_page: Signal<AppPage>) -> Element {
    let mut collapsed = use_signal(|| false);
    let proxy_core = use_context::<Arc<ProxyCoreService>>();
    let mut core_running = use_signal(|| proxy_core.status().running);
    // The core can also stop on its own (or be toggled from the dashboard), so
    // the indicator follows the service instead of only reacting to clicks.
    let status_probe = Arc::clone(&proxy_core);
    use_future(move || {
        let status_probe = Arc::clone(&status_probe);
        async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let running = status_probe.status().running;
                if core_running() != running {
                    core_running.set(running);
                }
            }
        }
    });

    rsx! {
        aside { class: if collapsed() { "sidebar collapsed" } else { "sidebar" },
            div { class: "sidebar-header",
                div { class: "logo",
                    i { class: "fa-solid fa-box-open" }
                    div { class: "logo-icon" }
                    span { "SingBox" }
                }
                button {
                    id: "sidebar-toggle",
                    class: "sidebar-toggle",
                    "aria-label": "Toggle sidebar",
                    "aria-expanded": (!collapsed()).to_string(),
                    onclick: move |_| collapsed.with_mut(|value| *value = !*value),
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
                    span { "Dashboard" }
                }
                button {
                    r#type: "button",
                    class: if current_page() == AppPage::Proxies { "nav-item active" } else { "nav-item" },
                    onclick: move |_| current_page.set(AppPage::Proxies),
                    i { class: "fa-solid fa-server" }
                    span { "Proxies" }
                }
                button {
                    r#type: "button",
                    class: if current_page() == AppPage::Subscriptions { "nav-item active" } else { "nav-item" },
                    onclick: move |_| current_page.set(AppPage::Subscriptions),
                    i { class: "fa-solid fa-link" }
                    span { "Subscribes" }
                }
                button {
                    r#type: "button",
                    class: if current_page() == AppPage::Network { "nav-item active" } else { "nav-item" },
                    onclick: move |_| current_page.set(AppPage::Network),
                    i { class: "fa-solid fa-earth-americas" }
                    span { "Network" }
                }
                button {
                    r#type: "button",
                    class: if current_page() == AppPage::Logs { "nav-item active" } else { "nav-item" },
                    onclick: move |_| current_page.set(AppPage::Logs),
                    i { class: "fa-solid fa-list-ul" }
                    span { "Logs" }
                }
                button {
                    r#type: "button",
                    class: if current_page() == AppPage::Settings { "nav-item active" } else { "nav-item" },
                    onclick: move |_| current_page.set(AppPage::Settings),
                    i { class: "fa-solid fa-gear" }
                    span { "Settings" }
                }
            }
            div {
                class: if core_running() { "status-indicator connected" } else { "status-indicator" },
                span { class: "dot" }
                span { class: "text", if core_running() { "Connected" } else { "Disconnected" } }
            }
        }
    }
}
