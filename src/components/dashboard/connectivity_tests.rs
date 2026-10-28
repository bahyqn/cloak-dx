use std::{collections::HashMap, sync::Arc};

use dioxus::prelude::*;

use crate::{
    components::{
        network::{
            google_ping_outcome, run_dns_test, run_google_test, run_ippure_test, run_service_tests,
            service_outcome, NetworkTestUi,
        },
        proxies::{start_test_run, ProxyTestUi, TestKind},
    },
    services::{
        app_log::AppLog,
        database::{Database, Subscription},
        network_test::{DnsLeakResult, DnsLeakVerdict},
        proxy_core::{ProxyCoreService, ProxyCoreStatus},
        subscription_manager::{subscription_node_key, SubscriptionNode},
    },
};

#[component]
pub(super) fn ConnectivityTests(
    selected_node: Option<SubscriptionNode>,
    subscription: Option<Subscription>,
) -> Element {
    let database = use_context::<Arc<Database>>();
    let proxy_core = use_context::<Arc<ProxyCoreService>>();
    let app_log = use_context::<AppLog>();
    let test_ui = use_context::<ProxyTestUi>();
    let network_ui = use_context::<NetworkTestUi>();
    let NetworkTestUi {
        google,
        google_running,
        ippure,
        ippure_running,
        dns,
        dns_running,
        provider,
        services,
        service_results,
        services_running,
        google_ping,
        owner,
        ..
    } = network_ui;
    // The DNS rows read the same app-scoped signals as the Network page, so a
    // test started there shows up here immediately (and vice versa).
    let dns_result = dns();
    let running = dns_running();
    let status = use_core_status(Arc::clone(&proxy_core));
    let socks_port = if status.running {
        status.socks_port
    } else {
        None
    };
    let (leak_text, leak_style, pollution_text, pollution_style) = dns_summary(
        dns_result.as_ref(),
        network_ui
            .exit_country(status.node_name.as_deref())
            .as_deref(),
        running,
    );
    let run_dns = move |_| {
        let Some(port) = socks_port else {
            return;
        };
        run_dns_test(port, dns, dns_running, provider(), owner);
    };

    // Latency and packet loss are measured by the Proxies page; this card just
    // mirrors the selected node's latest values (live signals first, then the
    // persisted result from an earlier session).
    let node_key = selected_node.as_ref().map(subscription_node_key);
    // Proxies runs the same function, so the spinner and the result are shared
    // between the two pages.
    let testing_snapshot = (test_ui.testing)();
    let latency_running = node_key
        .as_deref()
        .and_then(|key| testing_snapshot.get(key).copied())
        == Some(TestKind::Latency);
    let latency_snapshot = (test_ui.latencies)();
    let latency_text = if latency_running {
        "Testing...".to_string()
    } else {
        node_key
            .as_deref()
            .and_then(|node_key| latency_snapshot.get(node_key).cloned())
            .unwrap_or_else(|| "-- ms".to_string())
    };
    let latency_style = if latency_text == "Failed" {
        failure_style()
    } else {
        String::new()
    };
    let loss_snapshot = (test_ui.packet_losses)();
    let packet_loss = node_key
        .as_deref()
        .and_then(|node_key| loss_snapshot.get(node_key))
        .and_then(|text| text.trim_end_matches('%').trim().parse::<u8>().ok());
    let (loss_text, loss_style) = match packet_loss {
        Some(loss) => (format!("{loss} %"), packet_loss_style(loss)),
        None => ("-- %".to_string(), String::new()),
    };
    let google_ping_snapshot = google_ping();
    let (ping_text, ping_color) =
        google_ping_outcome(google_ping_snapshot.as_ref(), services_running());
    let selected_services = services();
    let service_snapshot = service_results();
    let running_services = services_running();
    let can_run_services = socks_port.is_some() && !running_services;
    let run_services = move |_| {
        let Some(port) = socks_port else {
            return;
        };
        run_service_tests(
            port,
            services,
            service_results,
            services_running,
            google_ping,
            owner,
        );
    };

    let can_test_latency = node_key.is_some() && subscription.is_some() && !latency_running;
    // Latency tests go through the Proxies pipeline, so the result lands in the
    // same shared signals and database row no matter which page started it.
    let latency_key = node_key.clone();
    let latency_name = selected_node.as_ref().map(|node| node.name.clone());
    let latency_subscription = subscription.clone();
    let latency_core = Arc::clone(&proxy_core);
    let latency_database = Arc::clone(&database);
    let run_latency = move |_| {
        run_selected_latency(
            latency_key.clone(),
            latency_name.clone(),
            latency_subscription.clone(),
            Arc::clone(&latency_core),
            Arc::clone(&latency_database),
            test_ui,
            app_log,
        );
    };

    // "Test All" adds the Network probes on top of the latency run. Those need
    // the running core's SOCKS inbound, so without it only latency is started.
    let all_key = node_key.clone();
    let all_name = selected_node.as_ref().map(|node| node.name.clone());
    let all_subscription = subscription.clone();
    let all_core = Arc::clone(&proxy_core);
    let all_database = Arc::clone(&database);
    let run_all = move |_| {
        run_selected_latency(
            all_key.clone(),
            all_name.clone(),
            all_subscription.clone(),
            Arc::clone(&all_core),
            Arc::clone(&all_database),
            test_ui,
            app_log,
        );
        let Some(port) = socks_port else {
            return;
        };
        run_google_test(port, google, google_running, owner);
        run_ippure_test(port, ippure, ippure_running, owner);
        run_dns_test(port, dns, dns_running, provider(), owner);
        run_service_tests(
            port,
            services,
            service_results,
            services_running,
            google_ping,
            owner,
        );
    };

    rsx! {
        section { class: "card test-card",
            div { class: "card-header", h3 { "Connectivity Test" } }
            div { class: "test-controls",
                button {
                    id: "dash-test-all-btn",
                    class: "btn-primary",
                    disabled: !can_test_latency,
                    onclick: run_all,
                    i { class: "fa-solid fa-check-double" }
                    "Test All"
                }
                button {
                    id: "dash-test-lat-btn",
                    class: "btn-secondary small",
                    disabled: !can_test_latency,
                    onclick: run_latency,
                    if latency_running {
                        i { class: "fa-solid fa-bolt fa-spin" }
                    } else {
                        i { class: "fa-solid fa-bolt" }
                    }
                    if latency_running { "Testing..." } else { "Latency" }
                }
            }
            div { class: "test-results", id: "test-results",
                div { class: "result-item",
                    span { class: "label", "Latency" }
                    span { class: "value", id: "ping-result", style: latency_style, "{latency_text}" }
                }
                div { class: "result-item",
                    span { class: "label", "Packet Loss" }
                    span { class: "value", id: "packet-loss", style: loss_style, "{loss_text}" }
                }
                div { class: "result-item",
                    span { class: "label", "Google Ping" }
                    span { class: "value", id: "google-ping", style: "color: {ping_color}", "{ping_text}" }
                }
            }

            div { class: "dns-header",
                div { class: "test-header-small", style: "margin: 0", "DNS Analysis" }
                button {
                    id: "dash-test-dns-btn",
                    class: "btn-secondary small",
                    disabled: socks_port.is_none() || running,
                    onclick: run_dns,
                    if running {
                        i { class: "fa-solid fa-rotate fa-spin" }
                    } else {
                        i { class: "fa-solid fa-play" }
                    }
                    if running {
                        "Running..."
                    } else if dns_result.is_some() {
                        "Re-run"
                    } else {
                        "Run"
                    }
                }
            }
            div { class: "test-results", id: "dns-results",
                div { class: "result-item",
                    span { class: "label", "DNS Leak" }
                    span { class: "value", id: "dns-leak", style: leak_style, "{leak_text}" }
                }
                div { class: "result-item",
                    span { class: "label", "DNS Pollution" }
                    span { class: "value", id: "dns-pollution", style: pollution_style, "{pollution_text}" }
                }
            }

            div { class: "dns-header",
                div { class: "test-header-small", style: "margin: 0", "Service Access" }
                button {
                    id: "dash-test-service-btn",
                    class: "btn-secondary small",
                    disabled: !can_run_services,
                    onclick: run_services,
                    if running_services {
                        i { class: "fa-solid fa-rotate fa-spin" }
                    } else {
                        i { class: "fa-solid fa-play" }
                    }
                    if running_services {
                        "Running..."
                    } else if service_snapshot.is_empty() {
                        "Run"
                    } else {
                        "Re-run"
                    }
                }
            }
            div { class: "test-results", id: "service-results",
                if selected_services.is_empty() {
                    div { class: "result-item",
                        span { class: "label", "Services" }
                        span { class: "value", "Pick up to three on the Network page" }
                    }
                } else {
                    for service in selected_services.iter() {
                        {
                            let (status, color) = service_outcome(service_snapshot.get(service), running_services);
                            rsx! {
                                div { key: "{service.id()}", class: "result-item",
                                    span { class: "label", "{service.label()}" }
                                    span { class: "value", style: "color: {color}", "{status}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Follows the core status so the DNS button enables as soon as the proxy is
/// started, without the dashboard having to be reopened.
fn use_core_status(proxy_core: Arc<ProxyCoreService>) -> ProxyCoreStatus {
    let mut core_status = use_signal(|| proxy_core.status());
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
    core_status()
}

/// Starts the Proxies latency pipeline for the node shown on the dashboard.
fn run_selected_latency(
    node_key: Option<String>,
    node_name: Option<String>,
    subscription: Option<Subscription>,
    proxy_core: Arc<ProxyCoreService>,
    database: Arc<Database>,
    test_ui: ProxyTestUi,
    logs: AppLog,
) {
    let (Some(node_key), Some(subscription)) = (node_key, subscription) else {
        return;
    };
    let node_names = Arc::new(
        node_name
            .map(|name| (node_key.clone(), name))
            .into_iter()
            .collect::<HashMap<String, String>>(),
    );
    start_test_run(
        TestKind::Latency,
        vec![node_key],
        proxy_core,
        database,
        subscription,
        false,
        test_ui,
        logs,
        node_names,
    );
}

fn failure_style() -> String {
    "color: var(--danger-color)".to_string()
}

fn packet_loss_style(loss: u8) -> String {
    let color = if loss == 0 {
        "var(--success-color)"
    } else if loss >= 100 {
        "var(--danger-color)"
    } else {
        "var(--warning-color)"
    };
    format!("color: {color}")
}

/// Text and color for the two dashboard DNS rows, derived from the same
/// resolver set the Network page's DNS card displays.
fn dns_summary(
    result: Option<&Result<DnsLeakResult, String>>,
    exit_country: Option<&str>,
    running: bool,
) -> (String, String, String, String) {
    let color = |color: &str| format!("color: {color}");
    match result {
        None if running => (
            "Testing...".to_string(),
            String::new(),
            "Testing...".to_string(),
            String::new(),
        ),
        None => (
            "--".to_string(),
            String::new(),
            "--".to_string(),
            String::new(),
        ),
        Some(Err(_)) => (
            "Check failed".to_string(),
            color("var(--danger-color)"),
            "Unknown".to_string(),
            String::new(),
        ),
        Some(Ok(data)) => {
            let verdict = data.verdict(exit_country);
            let (leak, pollution, tone) = match verdict {
                DnsLeakVerdict::Protected => {
                    ("Safe (No Leak)", "Clean", color("var(--success-color)"))
                }
                DnsLeakVerdict::Mixed => ("Unknown", "Unknown", color("var(--warning-color)")),
                DnsLeakVerdict::Leaking => ("Leaking", "Polluted", color("var(--danger-color)")),
            };
            (leak.to_string(), tone.clone(), pollution.to_string(), tone)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::dns_summary;
    use crate::services::network_test::{DnsLeakResult, DnsResolver};

    fn resolver(ip: &str, country: &str) -> DnsResolver {
        DnsResolver {
            ip: ip.to_string(),
            country_code: Some(country.to_string()),
            place: None,
            organization: None,
            sources: Vec::new(),
        }
    }

    fn result(resolvers: Vec<DnsResolver>) -> Result<DnsLeakResult, String> {
        Ok(DnsLeakResult {
            resolvers,
            fallback: None,
        })
    }

    #[test]
    fn dns_rows_stay_empty_until_the_probe_finishes() {
        assert_eq!(dns_summary(None, Some("HK"), false).0, "--");
        assert_eq!(dns_summary(None, Some("HK"), true).0, "Testing...");
    }

    #[test]
    fn resolvers_in_the_exit_country_show_a_clean_dns_path() {
        let value = result(vec![resolver("1.1.1.1", "HK"), resolver("2.2.2.2", "HK")]);

        let (leak, leak_style, pollution, pollution_style) =
            dns_summary(Some(&value), Some("HK"), false);

        assert_eq!(leak, "Safe (No Leak)");
        assert_eq!(pollution, "Clean");
        assert!(leak_style.contains("success-color"));
        assert!(pollution_style.contains("success-color"));
    }

    #[test]
    fn resolvers_outside_the_exit_country_show_a_leak() {
        let value = result(vec![resolver("1.1.1.1", "US")]);

        let (leak, leak_style, pollution, _) = dns_summary(Some(&value), Some("HK"), false);

        assert_eq!(leak, "Leaking");
        assert_eq!(pollution, "Polluted");
        assert!(leak_style.contains("danger-color"));
    }

    #[test]
    fn one_chinese_resolver_is_enough_to_report_a_leak() {
        let value = result(vec![
            resolver("1.1.1.1", "HK"),
            resolver("114.114.114.114", "CN"),
        ]);

        let (leak, _, pollution, _) = dns_summary(Some(&value), Some("HK"), false);

        assert_eq!(leak, "Leaking");
        assert_eq!(pollution, "Polluted");
    }

    #[test]
    fn a_failed_probe_is_reported_instead_of_showing_stale_data() {
        let value: Result<DnsLeakResult, String> = Err("timeout".to_string());

        let (leak, _, pollution, _) = dns_summary(Some(&value), Some("HK"), false);

        assert_eq!(leak, "Check failed");
        assert_eq!(pollution, "Unknown");
    }
}
