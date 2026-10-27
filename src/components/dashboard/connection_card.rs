use std::sync::Arc;

use crate::services::{
    database::{Database, Subscription},
    proxy_core::ProxyCoreService,
    subscription_manager::SubscriptionNode,
};
use dioxus::prelude::*;

const ADDRESS_MAX_CHARS: usize = 30;

#[component]
pub(super) fn ConnectionCard(
    selected_node: Option<SubscriptionNode>,
    subscription: Option<Subscription>,
) -> Element {
    let database = use_context::<Arc<Database>>();
    let proxy_core = use_context::<Arc<ProxyCoreService>>();
    let mut core_status = use_signal(|| proxy_core.status());
    let mut core_error = use_signal(String::new);
    let status_probe = Arc::clone(&proxy_core);
    use_future(move || {
        let status_probe = Arc::clone(&status_probe);
        async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let next = status_probe.status();
                if core_status() != next {
                    core_status.set(next);
                }
            }
        }
    });
    let database_for_toggle = Arc::clone(&database);
    let proxy_core_for_toggle = Arc::clone(&proxy_core);
    let on_toggle = move |_| {
        core_error.set(String::new());
        if core_status().running {
            proxy_core_for_toggle.stop();
            core_status.set(proxy_core_for_toggle.status());
        } else {
            match proxy_core_for_toggle.start_selected_subscription(&database_for_toggle) {
                Ok(status) => {
                    if let Some(warning) = status.warning.clone() {
                        core_error.set(warning);
                    }
                    core_status.set(status);
                }
                Err(error) => {
                    core_status.set(proxy_core_for_toggle.status());
                    core_error.set(error);
                }
            }
        }
    };
    let running = core_status().running;
    let node_name = selected_node
        .as_ref()
        .map(|node| node.name.as_str())
        .unwrap_or("Selecting...");
    let node_address_full = selected_node
        .as_ref()
        .map(|node| node.address.clone())
        .unwrap_or_else(|| "---.---.---.---".to_string());
    let node_address = truncate_address(&node_address_full, ADDRESS_MAX_CHARS);
    let reset_at = subscription
        .as_ref()
        .and_then(|subscription| subscription.reset_at);
    let expire_at = subscription
        .as_ref()
        .and_then(|subscription| subscription.expire_at);
    let used_traffic = subscription
        .as_ref()
        .map(|subscription| {
            format!(
                "{} / {}",
                format_used_traffic(&subscription.upload, &subscription.download),
                subscription.total
            )
        })
        .unwrap_or_else(|| "-- / --".to_string());

    rsx! {
        section { class: "card status-card",
            div { class: "status-content",
                div { class: "status-details",
                    h2 { id: "dashboard-node-name", "{node_name}" }
                    div { class: "status-meta",
                        span { class: "meta-item",
                            i { class: "fa-solid fa-network-wired" }
                            span { id: "dashboard-node-ip", title: "{node_address_full}", "{node_address}" }
                        }
                        span { class: "meta-item",
                            i { class: "fa-regular fa-clock" }
                            span { id: "uptime", "00:00:00" }
                        }
                    }
                    div { class: "sub-info",
                        span {
                            class: "status-icon subscription-flag",
                            id: "dashboard-flag",
                            "🇺🇸"
                        }
                        // span { class: "sub-item", title: "Reset Time",
                        //     i { class: "fa-solid fa-rotate" }
                        //     span { id: "sub-reset", "{format_dashboard_date(reset_at)}" }
                        // }
                        span { class: "sub-item", title: "Subscription Expiry",
                            i { class: "fa-regular fa-calendar-check" }
                            span { id: "sub-expiry", "{format_dashboard_date(expire_at)}" }
                        }
                        span { class: "sub-item", title: "Used / Total Traffic",
                            i { class: "fa-solid fa-chart-pie" }
                            span { id: "sub-data", "{used_traffic}" }
                        }
                    }
                    if !core_error().is_empty() {
                        p { class: "subscription-error", role: "alert", "{core_error}" }
                    }
                }
                button {
                    id: "toggle-btn",
                    class: if running { "btn-toggle stop" } else { "btn-toggle start" },
                    onclick: on_toggle,
                    if running { "Stop" } else { "Start" }
                }
            }
        }
    }
}

fn truncate_address(address: &str, max_chars: usize) -> String {
    if address.chars().count() <= max_chars {
        return address.to_string();
    }
    let truncated = address.chars().take(max_chars).collect::<String>();
    format!("{truncated}…")
}

fn format_dashboard_date(timestamp: Option<i64>) -> String {
    use chrono::{DateTime, Utc};

    timestamp
        .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
        .map(|date| date.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "--/--".to_string())
}

fn format_used_traffic(upload: &str, download: &str) -> String {
    let used_bytes = parse_traffic_bytes(upload).saturating_add(parse_traffic_bytes(download));
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut amount = used_bytes as f64;
    let mut unit = 0;
    while amount >= 1024.0 && unit < UNITS.len() - 1 {
        amount /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{used_bytes} B")
    } else {
        format!("{amount:.2} {}", UNITS[unit])
    }
}

fn parse_traffic_bytes(value: &str) -> u64 {
    let mut parts = value.split_whitespace();
    let Some(amount) = parts.next().and_then(|amount| amount.parse::<f64>().ok()) else {
        return 0;
    };
    let multiplier = match parts.next().unwrap_or("B").to_ascii_uppercase().as_str() {
        "KB" => 1024_f64,
        "MB" => 1024_f64.powi(2),
        "GB" => 1024_f64.powi(3),
        "TB" => 1024_f64.powi(4),
        _ => 1_f64,
    };
    (amount * multiplier).max(0.0) as u64
}

#[cfg(test)]
mod tests {
    use super::{format_used_traffic, truncate_address};

    #[test]
    fn dashboard_used_traffic_adds_upload_and_download() {
        assert_eq!(format_used_traffic("1.00 GB", "512.00 MB"), "1.50 GB");
    }

    #[test]
    fn dashboard_address_is_truncated_after_thirty_characters() {
        let short = "node.example.com";
        assert_eq!(truncate_address(short, 30), short);

        let long = "a".repeat(31);
        let truncated = truncate_address(&long, 30);
        assert_eq!(truncated, format!("{}…", "a".repeat(30)));
        assert_eq!(truncated.chars().count(), 31);
    }
}
