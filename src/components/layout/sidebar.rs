use dioxus::prelude::*;

use crate::app::AppPage;

#[component]
pub fn Sidebar(mut current_page: Signal<AppPage>) -> Element {
    let mut collapsed = use_signal(|| false);

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
            div { class: "status-indicator connected",
                span { class: "dot" }
                span { class: "text", "Connected" }
            }
        }
    }
}
