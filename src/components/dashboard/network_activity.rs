use std::sync::Arc;

use dioxus::prelude::*;

use crate::{
    i18n::I18n,
    services::proxy_core::{ProxyCoreService, TrafficRates},
};

#[component]
pub(super) fn NetworkActivity() -> Element {
    let i18n = use_context::<I18n>();
    let proxy_core = use_context::<Arc<ProxyCoreService>>();
    let mut rates = use_signal(TrafficRates::default);
    let traffic_probe = Arc::clone(&proxy_core);
    use_future(move || {
        let traffic_probe = Arc::clone(&traffic_probe);
        async move {
            loop {
                let probe = Arc::clone(&traffic_probe);
                // `traffic_rates` runs the Xray CLI, so keep it off the async
                // runtime's worker threads.
                let next = tokio::task::spawn_blocking(move || probe.traffic_rates())
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                if rates() != next {
                    rates.set(next);
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    });
    let current = rates();

    rsx! {
        section { class: "card traffic-card",
            h3 { "{i18n.t(\"Network Activity\")}" }
            div { class: "traffic-stats-large",
                div { class: "traffic-item up",
                    i { class: "fa-solid fa-arrow-up" }
                    div {
                        span { class: "label", "{i18n.t(\"Upload\")}" }
                        span { class: "value", id: "upload-speed", "{format_speed(current.uplink_bps)}" }
                    }
                }
                div { class: "traffic-item down",
                    i { class: "fa-solid fa-arrow-down" }
                    div {
                        span { class: "label", "{i18n.t(\"Download\")}" }
                        span { class: "value", id: "download-speed", "{format_speed(current.downlink_bps)}" }
                    }
                }
            }
        }
    }
}

fn format_speed(bytes_per_second: u64) -> String {
    const UNITS: [&str; 4] = ["B/s", "KB/s", "MB/s", "GB/s"];
    let mut amount = bytes_per_second as f64;
    let mut unit = 0;
    while amount >= 1024.0 && unit < UNITS.len() - 1 {
        amount /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes_per_second} B/s")
    } else {
        format!("{amount:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::format_speed;

    #[test]
    fn speeds_use_a_readable_unit() {
        assert_eq!(format_speed(0), "0 B/s");
        assert_eq!(format_speed(1023), "1023 B/s");
        assert_eq!(format_speed(1536), "1.5 KB/s");
        assert_eq!(format_speed(5 * 1024 * 1024), "5.0 MB/s");
    }
}
