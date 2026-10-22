use crate::services::subscription_manager::SubscriptionNode;
use dioxus::prelude::*;

#[component]
pub(super) fn ConnectionCard(selected_node: Option<SubscriptionNode>) -> Element {
    let node_name = selected_node
        .as_ref()
        .map(|node| node.name.as_str())
        .unwrap_or("Selecting...");
    let node_address = selected_node
        .as_ref()
        .map(|node| node.address.as_str())
        .unwrap_or("---.---.---.---");
    let node_protocol = selected_node
        .as_ref()
        .map(|node| node.protocol.as_str())
        .unwrap_or("--");

    rsx! {
        section { class: "card status-card",
            div { class: "status-content",
                div { class: "status-icon", id: "dashboard-flag", "🇺🇸" }
                div { class: "status-details",
                    h2 { id: "dashboard-node-name", "{node_name}" }
                    div { class: "status-meta",
                        span { class: "meta-item",
                            i { class: "fa-solid fa-network-wired" }
                            span { id: "dashboard-node-ip", "{node_address}" }
                        }
                        span { class: "meta-item",
                            i { class: "fa-regular fa-clock" }
                            span { id: "uptime", "00:00:00" }
                        }
                    }
                    div { class: "sub-info",
                        span { class: "sub-item", title: "Subscription Expiry",
                            i { class: "fa-regular fa-calendar-check" }
                            span { id: "sub-expiry", "--/--" }
                        }
                        span { class: "sub-item", title: "Data Usage",
                            i { class: "fa-solid fa-chart-pie" }
                            span { id: "sub-data", "{node_protocol}" }
                        }
                    }
                }
                button { id: "toggle-btn", class: "btn-toggle start", "Start" }
            }
        }
    }
}
