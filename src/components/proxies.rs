use std::{sync::Arc, time::Duration};

use dioxus::prelude::*;

use crate::services::{
    database::Database,
    subscription_manager::{parse_subscription_nodes, subscription_node_key, SubscriptionNode},
};

#[component]
pub fn ProxiesPage() -> Element {
    let database = use_context::<Arc<Database>>();
    let data_revision = use_context::<Signal<u64>>();
    let _revision = data_revision();
    let selected = database.selected_subscription().ok().flatten();
    let nodes = selected
        .as_ref()
        .map(|subscription| parse_subscription_nodes(&subscription.raw_config))
        .unwrap_or_default();
    let mut hide_timeout = use_signal(|| false);
    let toast_message = use_signal(String::new);
    let toast_generation = use_signal(|| 0_u64);
    let mut hover_generation = use_signal(|| 0_u64);
    let selected_node_key = selected
        .as_ref()
        .and_then(|subscription| subscription.selected_node_key.as_deref())
        .map(str::to_string)
        .or_else(|| nodes.first().map(subscription_node_key));
    let subscription_id = selected.as_ref().map(|subscription| subscription.id);
    let node_cards = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let node_key = subscription_node_key(node);
            (
                index,
                node.clone(),
                selected_node_key.as_deref() == Some(node_key.as_str()),
            )
        })
        .collect::<Vec<_>>();

    rsx! {
        div { id: "proxies-view", class: "view-section active proxies-page",
            div { class: "view-header",
                h2 { class: "page-title", "Proxy Nodes" }
                div { class: "toolbar",
                    button {
                        id: "test-all-btn",
                        class: "btn-primary",
                        style: "width: auto; padding: 10px 20px",
                        onclick: move |_| {
                            hover_generation.with_mut(|generation| *generation += 1);
                            show_proxy_toast(
                                toast_message,
                                toast_generation,
                                "Latency testing requires a running core.",
                            );
                        },
                        onmouseenter: move |_| schedule_hover_toast(
                            toast_message,
                            toast_generation,
                            hover_generation,
                            "Latency testing requires a running core.",
                        ),
                        onmouseleave: move |_| hover_generation.with_mut(|generation| *generation += 1),
                        i { class: "fa-solid fa-bolt" }
                        "Test Latency"
                    }
                    button {
                        id: "speed-test-all-btn",
                        class: "btn-secondary",
                        style: "width: auto; padding: 10px 20px",
                        onclick: move |_| {
                            hover_generation.with_mut(|generation| *generation += 1);
                            show_proxy_toast(
                                toast_message,
                                toast_generation,
                                "Speed testing requires a running core.",
                            );
                        },
                        onmouseenter: move |_| schedule_hover_toast(
                            toast_message,
                            toast_generation,
                            hover_generation,
                            "Speed testing requires a running core.",
                        ),
                        onmouseleave: move |_| hover_generation.with_mut(|generation| *generation += 1),
                        i { class: "fa-solid fa-gauge-high" }
                        "Speed Test All"
                    }
                    label { class: "toggle-switch",
                        input {
                            r#type: "checkbox",
                            id: "hide-timeout-toggle",
                            checked: hide_timeout(),
                            onchange: move |event| hide_timeout.set(event.checked()),
                        }
                        span { class: "slider" }
                        span { class: "label-text", "Hide Timeout" }
                    }
                    if !toast_message().is_empty() {
                        div { class: "proxy-toast", role: "status", "{toast_message}" }
                    }
                }
            }
            if selected.is_none() {
                div { class: "empty-state", "No subscription selected. Select one in Subscribes." }
            } else if nodes.is_empty() {
                div { class: "empty-state",
                    "The selected subscription has no parsed nodes. Refresh it from Subscribes first."
                }
            } else {
                div { class: "node-grid", id: "node-grid",
                    for (index , node , is_selected) in node_cards {
                        SubscriptionNodeCard {
                            key: "{selected.as_ref().map(|subscription| subscription.id).unwrap_or_default()}-{index}",
                            node,
                            selected: is_selected,
                            subscription_id,
                        }
                    }
                }
            }
        }
    }
}

fn show_proxy_toast(mut message: Signal<String>, mut generation: Signal<u64>, text: &'static str) {
    let current = generation() + 1;
    generation.set(current);
    message.set(text.to_string());
    spawn(async move {
        tokio::time::sleep(Duration::from_millis(2800)).await;
        if generation() == current {
            message.set(String::new());
        }
    });
}

fn schedule_hover_toast(
    message: Signal<String>,
    generation: Signal<u64>,
    mut hover_generation: Signal<u64>,
    text: &'static str,
) {
    let hover_id = hover_generation() + 1;
    hover_generation.set(hover_id);
    spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        if hover_generation() == hover_id {
            show_proxy_toast(message, generation, text);
        }
    });
}

#[component]
fn SubscriptionNodeCard(
    node: SubscriptionNode,
    selected: bool,
    subscription_id: Option<i64>,
) -> Element {
    let database = use_context::<Arc<Database>>();
    let mut data_revision = use_context::<Signal<u64>>();
    let mut action_message = use_signal(String::new);
    let selected_node_key = subscription_node_key(&node);
    let card_database = Arc::clone(&database);
    let latency_database = Arc::clone(&database);
    let speed_database = Arc::clone(&database);
    let card_node_key = selected_node_key.clone();
    let latency_node_key = selected_node_key.clone();
    let speed_node_key = selected_node_key;
    let latency_node_name = node.name.clone();
    let speed_node_name = node.name.clone();
    rsx! {
        article {
            class: if selected { "node-card active" } else { "node-card" },
            onclick: move |_| {
                if let Some(subscription_id) = subscription_id {
                    if card_database
                        .select_subscription_node(subscription_id, &card_node_key)
                        .is_ok()
                    {
                        data_revision.with_mut(|revision| *revision += 1);
                    }
                }
            },
            div { class: "node-header",
                div { class: "google-exit-region",
                    i { class: "fa-solid fa-earth-americas" }
                    span { "Google exit: —" }
                }
                div { class: "node-latency-wrapper",
                    div { class: "node-latency", "-- ms" }
                    button {
                        class: "node-refresh-btn",
                        title: "Test Latency",
                        "aria-label": "Test latency for {node.name}",
                        onclick: move |event| {
                            event.stop_propagation();
                            if let Some(subscription_id) = subscription_id {
                                let _ = latency_database
                                    .select_subscription_node(subscription_id, &latency_node_key);
                                data_revision.with_mut(|revision| *revision += 1);
                            }
                            action_message
                                .set(
                                    format!(
                                        "Latency test for {} requires a running core.",
                                        latency_node_name,
                                    ),
                                );
                        },
                        i { class: "fa-solid fa-rotate" }
                    }
                }
            }
            div { class: "node-info",
                h3 { class: "node-name-scroll", title: "{node.name}", "{node.name}" }
                div { class: "node-meta-grid",
                    div { class: "meta-item",
                        i { class: "fa-solid fa-shield-halved" }
                        "{node.protocol}"
                    }
                    div { class: "meta-item",
                        i { class: "fa-solid fa-network-wired" }
                        "{node.transport}"
                    }
                    div { class: "meta-item",
                        i { class: "fa-solid fa-certificate" }
                        "{node.tls}"
                    }
                    div { class: "meta-item",
                        i { class: "fa-solid fa-location-dot" }
                        "Google region —"
                    }
                }
            }
            div { class: "node-speed-area",
                div { class: "speed-metrics",
                    div { class: "speed-item",
                        i { class: "fa-solid fa-arrow-down" }
                        span { "-- MB/s" }
                    }
                    div { class: "speed-item",
                        i { class: "fa-solid fa-arrow-up" }
                        span { "-- MB/s" }
                    }
                }
                button {
                    class: "node-speed-test-btn",
                    title: "Speed Test",
                    "aria-label": "Speed test for {node.name}",
                    onclick: move |event| {
                        event.stop_propagation();
                        if let Some(subscription_id) = subscription_id {
                            let _ = speed_database
                                .select_subscription_node(subscription_id, &speed_node_key);
                            data_revision.with_mut(|revision| *revision += 1);
                        }
                        action_message
                            .set(format!("Speed test for {} requires a running core.", speed_node_name));
                    },
                    i { class: "fa-solid fa-gauge-high" }
                }
            }
            div { class: "node-details",
                span { class: "cert-tag", "Google exit IP" }
                span { class: "ip-tag", "—" }
            }
            if !action_message().is_empty() {
                p { class: "node-action-message", role: "status", "{action_message}" }
            }
        }
    }
}
