use std::sync::Arc;

use crate::{
    components::proxies::ProxyTestUi,
    i18n::I18n,
    services::{
        country::country_flag,
        database::{Database, Subscription},
        proxy_core::ProxyCoreService,
        subscription_manager::{
            subscription_node_country_code, subscription_node_key, SubscriptionNode,
        },
    },
};
use dioxus::prelude::*;

const ADDRESS_MAX_CHARS: usize = 30;

#[component]
pub(super) fn ConnectionCard(
    selected_node: Option<SubscriptionNode>,
    subscription: Option<Subscription>,
) -> Element {
    let i18n = use_context::<I18n>();
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
    let uptime = format_uptime(core_status().uptime_seconds);
    // The flag describes where traffic actually exits, so it comes from the
    // node's Google exit probe. The subscription's own flag is only a
    // fallback until that node has been tested.
    let test_ui = use_context::<ProxyTestUi>();
    let exit_snapshot = (test_ui.exits)();
    let node_key = selected_node.as_ref().map(subscription_node_key);
    let exit_country_code = node_key.as_deref().and_then(|node_key| {
        exit_snapshot
            .get(node_key)
            .and_then(|exit| exit.country_code.clone())
            .or_else(|| {
                let subscription = subscription.as_ref()?;
                database
                    .node_test_results(subscription.id)
                    .ok()?
                    .into_iter()
                    .find(|result| result.node_key == node_key)?
                    .exit_country_code
            })
    });
    let exit_flag = exit_country_code
        .as_deref()
        .and_then(country_flag)
        .or_else(|| {
            selected_node
                .as_ref()
                .and_then(subscription_node_country_code)
                .and_then(|code| country_flag(&code))
        })
        .unwrap_or_else(|| "🌐".to_string());
    let node_name = selected_node
        .as_ref()
        .map(|node| node.name.as_str())
        .unwrap_or(i18n.t("Selecting..."));
    let node_address_full = selected_node
        .as_ref()
        .map(|node| node.address.clone())
        .unwrap_or_else(|| "---.---.---.---".to_string());
    let node_address = truncate_address(&node_address_full, ADDRESS_MAX_CHARS);
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
                            span { id: "uptime", "{uptime}" }
                        }
                    }
                    div { class: "sub-info",
                        span {
                            class: "status-icon subscription-flag",
                            id: "dashboard-flag",
                            "{exit_flag}"
                        }
                        // span { class: "sub-item", title: "Reset Time",
                        //     i { class: "fa-solid fa-rotate" }
                        //     span { id: "sub-reset", "{format_dashboard_date(reset_at)}" }
                        // }
                        span { class: "sub-item", title: i18n.t("Subscription Expiry"),
                            i { class: "fa-regular fa-calendar-check" }
                            span { id: "sub-expiry", "{format_dashboard_date(expire_at)}" }
                        }
                        span { class: "sub-item", title: i18n.t("Used / Total Traffic"),
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
                    if running { "{i18n.t(\"Stop\")}" } else { "{i18n.t(\"Start\")}" }
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

fn format_uptime(seconds: Option<u64>) -> String {
    let seconds = seconds.unwrap_or_default();
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn format_dashboard_date(timestamp: Option<i64>) -> String {
    use chrono::{DateTime, Local, Utc};

    timestamp
        .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
        .map(|date| date.with_timezone(&Local).format("%Y-%m-%d").to_string())
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
    use super::{format_uptime, format_used_traffic, truncate_address};

    #[test]
    fn dashboard_uptime_is_formatted_as_hours_minutes_seconds() {
        assert_eq!(format_uptime(None), "00:00:00");
        assert_eq!(format_uptime(Some(0)), "00:00:00");
        assert_eq!(format_uptime(Some(59)), "00:00:59");
        assert_eq!(format_uptime(Some(3661)), "01:01:01");
        assert_eq!(format_uptime(Some(100 * 3600)), "100:00:00");
    }

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
