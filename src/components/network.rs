use std::{collections::HashMap, sync::Arc};

use dioxus::{dioxus_core::spawn_forever, prelude::*};

use crate::i18n::I18n;
use crate::services::{
    country::{country_flag, country_name},
    network_test::{
        self, DnsLeakProvider, DnsLeakResult, DnsLeakVerdict, DnsResolver, IppureResult,
    },
    proxy_core::ProxyCoreService,
    proxy_test::GoogleExitResult,
    service_access::{self, ServiceCheck, ServiceId},
    subscription_manager::country_code_from_name,
};

type GoogleState = Signal<Option<Result<GoogleExitResult, String>>>;
type IppureState = Signal<Option<Result<IppureResult, String>>>;
type DnsState = Signal<Option<Result<DnsLeakResult, String>>>;

/// Results and per-card progress for the Network page.
///
/// The state lives at the app scope, so switching to another view and back
/// keeps whatever finished earlier (and a test keeps running while the page is
/// closed).
#[derive(Clone, Copy)]
pub struct NetworkTestUi {
    pub google: GoogleState,
    pub ippure: IppureState,
    pub dns: DnsState,
    pub google_running: Signal<bool>,
    pub ippure_running: Signal<bool>,
    pub dns_running: Signal<bool>,
    pub provider: Signal<DnsLeakProvider>,
    /// The three services watched by the Service Access card and the
    /// dashboard. Shared so both views always show the same list.
    pub services: Signal<Vec<ServiceId>>,
    pub service_results: Signal<HashMap<ServiceId, Result<ServiceCheck, String>>>,
    pub services_running: Signal<bool>,
    /// Google round trip measured by the service checks, for the dashboard's
    /// Google Ping row.
    pub google_ping: Signal<Option<Result<u64, String>>>,
    /// Subscription + node the stored results describe. Results that finish
    /// after the selection changed are discarded instead of attributed to the
    /// new node.
    pub owner: Signal<String>,
}

impl NetworkTestUi {
    /// Country the active connection exits from, preferring Google's own
    /// localization over IP databases. The Network page and the dashboard's
    /// DNS summary both use it so they judge the same resolver set the same
    /// way.
    ///
    /// When neither probe has reported a country yet, the running node's flag
    /// is the intended exit (`node_name`), so a DNS test run on its own can
    /// still tell whether the resolvers sit outside the tunnel.
    pub(crate) fn exit_country(self, node_name: Option<&str>) -> Option<String> {
        (self.google)()
            .and_then(|result| result.ok())
            .and_then(|exit| exit.country_code)
            .or_else(|| {
                (self.ippure)()
                    .and_then(|result| result.ok())
                    .and_then(|ippure| ippure.country_code)
            })
            .or_else(|| node_name.and_then(country_code_from_name))
    }
}

/// Network page.
///
/// Every check runs through the core's SOCKS inbound, so the page describes the
/// connection that is actually serving traffic. Nothing runs on its own: the
/// header button tests everything, and each card can be re-tested on its own.
#[component]
pub fn NetworkPage() -> Element {
    let proxy_core = use_context::<Arc<ProxyCoreService>>();
    let i18n = use_context::<I18n>();
    let status = proxy_core.status();
    let socks_port = if status.running {
        status.socks_port
    } else {
        None
    };
    let node_label = status
        .node_name
        .clone()
        .unwrap_or_else(|| i18n.t("No active node").to_string());

    // App-scoped so results survive navigating away from this page.
    let network_ui = use_context::<NetworkTestUi>();
    let NetworkTestUi {
        google,
        ippure,
        mut dns,
        google_running,
        ippure_running,
        dns_running,
        mut provider,
        mut services,
        mut service_results,
        services_running,
        google_ping,
        owner,
    } = network_ui;

    let any_running = google_running() || ippure_running() || dns_running() || services_running();
    let google_result = google();
    let ippure_result = ippure();
    let dns_result = dns();
    let services_snapshot = services();
    let service_snapshot = service_results();
    let google_ping_snapshot = google_ping();
    let (ping_text, ping_color) = match google_ping_snapshot.as_ref() {
        None if services_running() => (i18n.t("Testing...").to_string(), "var(--text-secondary)"),
        None => ("-- ms".to_string(), "var(--text-secondary)"),
        Some(Ok(milliseconds)) => (format!("{milliseconds} ms"), "var(--success-color)"),
        Some(Err(_)) => (i18n.t("Failed").to_string(), "var(--danger-color)"),
    };

    // Google's own localization — the country domain it redirects to — is what
    // a browser experiences, and IP databases can disagree with it (a node
    // registered in one country can be routed to another). IPPure only fills in
    // when Google could not report a country.
    let exit_country = network_ui.exit_country(status.node_name.as_deref());
    let exit_label = exit_country
        .as_deref()
        .map(|code| {
            country_name(code)
                .map(str::to_string)
                .unwrap_or_else(|| code.to_ascii_uppercase())
        })
        .unwrap_or_else(|| i18n.t("the exit country").to_string());

    let endpoint = match socks_port {
        Some(port) => format!("{node_label} · SOCKS 127.0.0.1:{port}"),
        None => i18n.t("Proxy core is offline").to_string(),
    };

    rsx! {
        div { id: "network-view", class: "view-section active network-page",
            div { class: "settings-header",
                h2 { class: "page-title", "{i18n.t(\"Network\")}" }
                div { class: "settings-header-actions",
                    p { class: "settings-hint",
                        i { class: "fa-solid fa-circle-info" }
                        "{endpoint}"
                    }
                    button {
                        class: "btn-primary network-run-btn",
                        disabled: socks_port.is_none() || any_running,
                        onclick: move |_| {
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
                        },
                        if any_running {
                            i { class: "fa-solid fa-rotate fa-spin" }
                            "{i18n.t(\"Testing...\")}"
                        } else {
                            i { class: "fa-solid fa-play" }
                            "{i18n.t(\"Test\")}"
                        }
                    }
                }
            }

            if socks_port.is_none() {
                div { class: "network-banner",
                    i { class: "fa-solid fa-triangle-exclamation" }
                    div {
                        strong { "{i18n.t(\"Start the proxy core first\")}" }
                        span {
                            "{i18n.t(\"These checks run through the active node. Start the core from the Dashboard, then return here to inspect the connection.\")}"
                        }
                    }
                }
            }

            div { class: "network-grid",
                div { class: "settings-card network-card",
                    h3 {
                        span { class: "network-card-title",
                            i { class: "fa-brands fa-google" }
                            "{i18n.t(\"Google Exit\")}"
                        }
                        button {
                            class: "btn-secondary small",
                            title: "{i18n.t(\"Test Google exit\")}",
                            disabled: socks_port.is_none() || google_running(),
                            onclick: move |_| {
                                if let Some(port) = socks_port {
                                    run_google_test(port, google, google_running, owner);
                                }
                            },
                            if google_running() {
                                i { class: "fa-solid fa-rotate fa-spin" }
                            } else {
                                i { class: "fa-solid fa-rotate" }
                            }
                            if google_result.is_none() { "{i18n.t(\"Test\")}" } else { "{i18n.t(\"Re-test\")}" }
                        }
                    }
                    {google_card_body(i18n, google_result.as_ref(), google_running())}
                }

                div { class: "settings-card network-card",
                    h3 {
                        span { class: "network-card-title",
                            i { class: "fa-solid fa-shield-halved" }
                            "IPPure"
                        }
                        button {
                            class: "btn-secondary small",
                            title: "{i18n.t(\"Test with IPPure\")}",
                            disabled: socks_port.is_none() || ippure_running(),
                            onclick: move |_| {
                                if let Some(port) = socks_port {
                                    run_ippure_test(port, ippure, ippure_running, owner);
                                }
                            },
                            if ippure_running() {
                                i { class: "fa-solid fa-rotate fa-spin" }
                            } else {
                                i { class: "fa-solid fa-rotate" }
                            }
                            if ippure_result.is_none() { "{i18n.t(\"Test\")}" } else { "{i18n.t(\"Re-test\")}" }
                        }
                    }
                    {ippure_card_body(i18n, ippure_result.as_ref(), ippure_running())}
                }

                div { class: "settings-card network-card",
                    h3 {
                        span { class: "network-card-title",
                            i { class: "fa-solid fa-globe" }
                            "{i18n.t(\"Service Access\")}"
                        }
                        button {
                            class: "btn-secondary small",
                            title: "{i18n.t(\"Test the selected services\")}",
                            disabled: socks_port.is_none() || services_running(),
                            onclick: move |_| {
                                if let Some(port) = socks_port {
                                    run_service_tests(
                                        port,
                                        services,
                                        service_results,
                                        services_running,
                                        google_ping,
                                        owner,
                                    );
                                }
                            },
                            if services_running() {
                                i { class: "fa-solid fa-rotate fa-spin" }
                            } else {
                                i { class: "fa-solid fa-rotate" }
                            }
                            if service_snapshot.is_empty() { "{i18n.t(\"Test\")}" } else { "{i18n.t(\"Re-test\")}" }
                        }
                    }
                    div { class: "service-selects",
                        for slot in 0..ServiceId::SELECTED_LIMIT {
                            select {
                                class: "service-select",
                                title: "{i18n.t(\"Service slot\")} {slot + 1}",
                                "aria-label": "{i18n.t(\"Service slot\")} {slot + 1}",
                                value: services_snapshot
                                    .get(slot)
                                    .map(|service| service.id())
                                    .unwrap_or(""),
                                disabled: services_running(),
                                onchange: move |event| {
                                    let chosen = ServiceId::from_id(&event.value());
                                    let current = services();
                                    let next = ServiceId::with_slot(&current, slot, chosen);
                                    if next != current {
                                        services.set(next.clone());
                                        service_results.with_mut(|results| {
                                            results.retain(|service, _| next.contains(service));
                                        });
                                    }
                                },
                                option { value: "", "—" }
                                for service in ServiceId::ALL {
                                    option {
                                        value: service.id(),
                                        selected: services_snapshot.get(slot).copied() == Some(service),
                                        "{service.label()}"
                                    }
                                }
                            }
                        }
                    }
                    div { class: "network-kv-list",
                        if services_snapshot.is_empty() {
                            div { class: "network-empty",
                                i { class: "fa-solid fa-globe" }
                                p { "{i18n.t(\"Pick up to three services to check.\")}" }
                            }
                        } else {
                            for service in services_snapshot.iter() {
                                {service_row(i18n, *service, service_snapshot.get(service), services_running())}
                            }
                        }
                        div { class: "network-kv",
                            span { class: "network-k", "{i18n.t(\"Google ping\")}" }
                            span { class: "network-v", style: "color: {ping_color}", "{ping_text}" }
                        }
                    }
                }

                div { class: "settings-card network-card dns-card",
                    h3 {
                        span { class: "network-card-title",
                            i { class: "fa-solid fa-magnifying-glass-location" }
                            "{i18n.t(\"DNS Leak\")}"
                        }
                        div { class: "network-card-actions",
                            select {
                                class: "dns-provider-select",
                                title: "{i18n.t(\"DNS leak check provider\")}",
                                "aria-label": "{i18n.t(\"DNS leak check provider\")}",
                                value: provider().value(),
                                disabled: dns_running(),
                                onchange: move |event| {
                                    let next = DnsLeakProvider::from_value(&event.value());
                                    if next != provider() {
                                        provider.set(next);
                                        dns.set(None);
                                    }
                                },
                                option {
                                    value: DnsLeakProvider::Ipleak.value(),
                                    selected: provider() == DnsLeakProvider::Ipleak,
                                    "https://ipleak.net/"
                                }
                                option {
                                    value: DnsLeakProvider::BrowserLeaks.value(),
                                    selected: provider() == DnsLeakProvider::BrowserLeaks,
                                    "https://browserleaks.com/dns"
                                }
                            }
                            button {
                                class: "btn-secondary small",
                                title: "{i18n.t(\"Run the DNS leak check\")}",
                                disabled: socks_port.is_none() || dns_running(),
                                onclick: move |_| {
                                    if let Some(port) = socks_port {
                                        run_dns_test(port, dns, dns_running, provider(), owner);
                                    }
                                },
                                if dns_running() {
                                    i { class: "fa-solid fa-rotate fa-spin" }
                                } else {
                                    i { class: "fa-solid fa-rotate" }
                                }
                                if dns_result.is_none() { "{i18n.t(\"Test\")}" } else { "{i18n.t(\"Re-test\")}" }
                            }
                        }
                    }
                    {dns_card_body(i18n, dns_result.as_ref(), exit_country.as_deref(), &exit_label, dns_running())}
                }
            }
        }
    }
}

pub(crate) fn run_google_test(
    socks_port: u16,
    mut state: GoogleState,
    mut running: Signal<bool>,
    owner: Signal<String>,
) {
    let test_owner = owner();
    running.set(true);
    // Not tied to this page's scope: navigating away must not cancel the test.
    spawn_forever(async move {
        let result = network_test::test_google_exit(socks_port).await;
        // Results describe one node; a probe that finishes after the user
        // switched nodes is dropped instead of showing up under the new one.
        if owner() == test_owner {
            state.set(Some(result));
            running.set(false);
        }
    });
}

pub(crate) fn run_ippure_test(
    socks_port: u16,
    mut state: IppureState,
    mut running: Signal<bool>,
    owner: Signal<String>,
) {
    let test_owner = owner();
    running.set(true);
    spawn_forever(async move {
        let result = network_test::test_ippure(socks_port).await;
        if owner() == test_owner {
            state.set(Some(result));
            running.set(false);
        }
    });
}

/// Starts the DNS leak probe and writes into the app-scoped result signals.
///
/// The dashboard's DNS section runs the same probe against the same signals,
/// so both views always show one result.
pub(crate) fn run_dns_test(
    socks_port: u16,
    mut state: DnsState,
    mut running: Signal<bool>,
    provider: DnsLeakProvider,
    owner: Signal<String>,
) {
    let test_owner = owner();
    running.set(true);
    spawn_forever(async move {
        let result = network_test::test_dns_leak(socks_port, provider).await;
        if owner() == test_owner {
            state.set(Some(result));
            running.set(false);
        }
    });
}

/// Starts the Service Access checks and the Google ping probe.
///
/// Selected services are probed concurrently; the ping always runs so the
/// dashboard's Google Ping row is filled even when Google is not one of the
/// watched services. Both views read the same signals.
pub(crate) fn run_service_tests(
    socks_port: u16,
    services: Signal<Vec<ServiceId>>,
    mut results: Signal<HashMap<ServiceId, Result<ServiceCheck, String>>>,
    mut running: Signal<bool>,
    mut google_ping: Signal<Option<Result<u64, String>>>,
    owner: Signal<String>,
) {
    let test_owner = owner();
    let selected = services();
    running.set(true);
    spawn_forever(async move {
        let mut probes = Vec::with_capacity(selected.len());
        for service in selected {
            probes.push(tokio::spawn(async move {
                (
                    service,
                    service_access::test_service(socks_port, service).await,
                )
            }));
        }
        let ping = service_access::test_google_ping(socks_port).await;
        let mut checks = HashMap::new();
        for probe in probes {
            if let Ok((service, outcome)) = probe.await {
                checks.insert(service, outcome);
            }
        }
        // Like the other probes, a result that lands after the user switched
        // nodes belongs to the previous node and is thrown away.
        if owner() == test_owner {
            google_ping.set(Some(ping));
            results.set(checks);
            running.set(false);
        }
    });
}

/// Text and color for one service row, shared by the Network card and the
/// dashboard.
pub(crate) fn service_outcome(
    result: Option<&Result<ServiceCheck, String>>,
    running: bool,
) -> (String, &'static str) {
    match result {
        None if running => ("Testing...".to_string(), "var(--text-secondary)"),
        None => ("--".to_string(), "var(--text-secondary)"),
        Some(Ok(check)) => (
            format!("OK · {} ms", check.latency_ms),
            "var(--success-color)",
        ),
        Some(Err(_)) => ("Failed".to_string(), "var(--danger-color)"),
    }
}

/// Text and color for the dashboard's Google Ping row.
pub(crate) fn google_ping_outcome(
    result: Option<&Result<u64, String>>,
    running: bool,
) -> (String, &'static str) {
    match result {
        None if running => ("Testing...".to_string(), "var(--text-secondary)"),
        None => ("-- ms".to_string(), "var(--text-secondary)"),
        Some(Ok(milliseconds)) => (format!("{milliseconds} ms"), "var(--success-color)"),
        Some(Err(_)) => ("Failed".to_string(), "var(--danger-color)"),
    }
}

fn service_row(
    i18n: I18n,
    service: ServiceId,
    result: Option<&Result<ServiceCheck, String>>,
    running: bool,
) -> Element {
    let (status, color) = match result {
        None if running => (i18n.t("Testing...").to_string(), "var(--text-secondary)"),
        None => ("--".to_string(), "var(--text-secondary)"),
        Some(Ok(check)) => (
            format!("{} · {} ms", i18n.t("OK"), check.latency_ms),
            "var(--success-color)",
        ),
        Some(Err(_)) => (i18n.t("Failed").to_string(), "var(--danger-color)"),
    };
    let title = match result {
        Some(Ok(check)) => format!("HTTP {}", check.status),
        Some(Err(error)) => error.clone(),
        None => String::new(),
    };
    rsx! {
        div { key: "{service.id()}", class: "network-kv",
            span { class: "network-k", "{service.label()}" }
            span { class: "network-v", title: "{title}", style: "color: {color}", "{status}" }
        }
    }
}

fn google_card_body(
    i18n: I18n,
    result: Option<&Result<GoogleExitResult, String>>,
    running: bool,
) -> Element {
    match result {
        None => rsx! {
            div { class: "network-empty",
                if running {
                    i { class: "fa-solid fa-rotate fa-spin" }
                } else {
                    i { class: "fa-solid fa-globe" }
                }
                p { if running { "{i18n.t(\"Probing Google...\")}" } else { "{i18n.t(\"Not tested yet\")}" } }
                span { "{i18n.t(\"Shows the country and city Google infers from the active exit IP.\")}" }
            }
        },
        Some(Err(error)) => rsx! {
            div { class: "network-error",
                i { class: "fa-solid fa-triangle-exclamation" }
                p { "{i18n.t(\"Google exit probe failed\")}" }
                span { "{error}" }
            }
        },
        Some(Ok(exit)) => {
            let flag = exit
                .country_code
                .as_deref()
                .and_then(country_flag)
                .unwrap_or_else(|| "🌐".to_string());
            let code = exit.country_code.clone().unwrap_or_else(|| "—".to_string());
            let country = exit
                .country
                .clone()
                .or_else(|| {
                    exit.country_code
                        .as_deref()
                        .and_then(country_name)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| i18n.t("Unknown").to_string());
            let region = exit
                .region
                .clone()
                .unwrap_or_else(|| i18n.t("Unknown").to_string());
            rsx! {
                div { class: "network-hero",
                    span { class: "network-hero-flag", "{flag}" }
                    div { class: "network-hero-text",
                        span { class: "network-hero-value", "{country}" }
                        span { class: "network-hero-sub", "{code} · {region}" }
                    }
                }
                div { class: "network-kv-list",
                    div { class: "network-kv",
                        span { class: "network-k", "{i18n.t(\"Exit IP\")}" }
                        span { class: "network-v network-mono", title: "{exit.ip}", "{exit.ip}" }
                    }
                    div { class: "network-kv",
                        span { class: "network-k", "{i18n.t(\"Google country\")}" }
                        span { class: "network-v", "{country} · {code}" }
                    }
                    div { class: "network-kv",
                        span { class: "network-k", "{i18n.t(\"City / region\")}" }
                        span { class: "network-v", "{region}" }
                    }
                }
            }
        }
    }
}

fn ippure_card_body(
    i18n: I18n,
    result: Option<&Result<IppureResult, String>>,
    running: bool,
) -> Element {
    match result {
        None => rsx! {
            div { class: "network-empty",
                if running {
                    i { class: "fa-solid fa-rotate fa-spin" }
                } else {
                    i { class: "fa-solid fa-shield-halved" }
                }
                p { if running { "{i18n.t(\"Asking IPPure...\")}" } else { "{i18n.t(\"Not tested yet\")}" } }
                span { "{i18n.t(\"IPPure scores the exit IP for risk, ASN type, native/broadcast and automation.\")}" }
            }
        },
        Some(Err(error)) => rsx! {
            div { class: "network-error",
                i { class: "fa-solid fa-triangle-exclamation" }
                p { "{i18n.t(\"IPPure lookup failed\")}" }
                span { "{error}" }
            }
        },
        Some(Ok(data)) => {
            let (risk_label, risk_color) = data.risk_level().unwrap_or(("—", "#64748b"));
            let score = data
                .risk_score
                .map(|score| score.to_string())
                .unwrap_or_else(|| "—".to_string());
            let meter = data.risk_score.unwrap_or(0).min(100);
            let asn = data
                .asn_number
                .map(|number| format!("AS{number}"))
                .unwrap_or_else(|| "—".to_string());
            let organization = data
                .asn_organization
                .clone()
                .unwrap_or_else(|| i18n.t("Unknown").to_string());
            let flag = data
                .country_code
                .as_deref()
                .and_then(country_flag)
                .unwrap_or_else(|| "🌐".to_string());
            let place = data
                .place
                .clone()
                .or_else(|| {
                    data.country_code
                        .as_deref()
                        .and_then(country_name)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| i18n.t("Unknown").to_string());
            let ratios = match (data.human_ratio, data.bot_ratio) {
                (Some(human), Some(bot)) => format!(
                    "{human:.1}% {} · {bot:.1}% {}",
                    i18n.t("human"),
                    i18n.t("bot")
                ),
                (Some(human), None) => format!("{human:.1}% {}", i18n.t("human")),
                (None, Some(bot)) => format!("{bot:.1}% {}", i18n.t("bot")),
                (None, None) => "—".to_string(),
            };
            rsx! {
                div { class: "network-score",
                    span { class: "network-score-value", style: "color: {risk_color}", "{score}" }
                    div { class: "network-score-meta",
                        span { class: "network-score-label", style: "color: {risk_color}", "{risk_label}" }
                        span { class: "network-score-sub", "{i18n.t(\"IPPure risk score\")}" }
                    }
                }
                div { class: "network-meter",
                    div { class: "network-meter-fill", style: "width: {meter}%; background: {risk_color}" }
                }
                div { class: "network-badges",
                    span { class: "network-badge", "{data.ip_type()}" }
                    span { class: "network-badge", "{data.nature()}" }
                    if data.is_warp == Some(true) {
                        span { class: "network-badge warp", "WARP" }
                    }
                }
                div { class: "network-kv-list",
                    div { class: "network-kv",
                        span { class: "network-k", "{i18n.t(\"Exit IP\")}" }
                        span { class: "network-v network-mono", title: "{data.ip}", "{data.ip}" }
                    }
                    div { class: "network-kv",
                        span { class: "network-k", "ASN" }
                        span { class: "network-v", "{asn} · {organization}" }
                    }
                    div { class: "network-kv",
                        span { class: "network-k", "{i18n.t(\"Location\")}" }
                        span { class: "network-v", "{flag} {place}" }
                    }
                    div { class: "network-kv",
                        span { class: "network-k", "{i18n.t(\"Human / bot\")}" }
                        span { class: "network-v", "{ratios}" }
                    }
                }
            }
        }
    }
}

fn dns_card_body(
    i18n: I18n,
    result: Option<&Result<DnsLeakResult, String>>,
    exit_country: Option<&str>,
    exit_label: &str,
    running: bool,
) -> Element {
    match result {
        None => rsx! {
            div { class: "network-empty",
                if running {
                    i { class: "fa-solid fa-rotate fa-spin" }
                } else {
                    i { class: "fa-solid fa-magnifying-glass-location" }
                }
                p { if running { "{i18n.t(\"Probing public resolvers...\")}" } else { "{i18n.t(\"Not tested yet\")}" } }
                span { "{i18n.t(\"Random hostnames are resolved through the proxy, so the resolvers this connection actually uses show up here.\")}" }
            }
        },
        Some(Err(error)) => rsx! {
            div { class: "network-error",
                i { class: "fa-solid fa-triangle-exclamation" }
                p { "{i18n.t(\"DNS leak check failed\")}" }
                span { "{error}" }
            }
        },
        Some(Ok(data)) => {
            let verdict = data.verdict(exit_country);
            let (icon, summary) = match verdict {
                DnsLeakVerdict::Protected => (
                    "fa-solid fa-circle-check",
                    format!(
                        "{} {} ({} {}).",
                        i18n.t("Resolvers are consistent with"),
                        exit_label,
                        data.resolvers.len(),
                        i18n.t("seen")
                    ),
                ),
                DnsLeakVerdict::Mixed => {
                    let summary = if exit_country.is_none() {
                        i18n.t("The exit country is unknown, so resolver locations cannot be compared. Run the Google exit or IPPure test first.")
                            .to_string()
                    } else {
                        i18n.t("The resolvers have no location data, so they cannot be compared with the exit country.")
                            .to_string()
                    };
                    ("fa-solid fa-scale-balanced", summary)
                }
                DnsLeakVerdict::Leaking => {
                    let known = data
                        .resolvers
                        .iter()
                        .filter(|resolver| resolver.country_code.is_some())
                        .count();
                    let outside = data
                        .resolvers
                        .iter()
                        .filter(|resolver| {
                            resolver.country_code.as_deref().is_some_and(|code| {
                                !exit_country.is_some_and(|exit| {
                                    code.trim().eq_ignore_ascii_case(exit.trim())
                                })
                            })
                        })
                        .count();
                    (
                        "fa-solid fa-triangle-exclamation",
                        format!(
                            "{} {} {} {} {}.",
                            outside,
                            i18n.t("of"),
                            known,
                            i18n.t("resolvers sit outside"),
                            exit_label
                        ),
                    )
                }
            };
            rsx! {
                if let Some(fallback) = data.fallback {
                    div { class: "dns-fallback-note",
                        i { class: "fa-solid fa-circle-info" }
                        span {
                            "{i18n.t(\"ipleak.net did not see this connection's DNS lookups (the resolver answered the probe names from cache), so the resolvers below were measured with\")} {fallback.label()} {i18n.t(\"probes.\")}"
                        }
                    }
                }
                div { class: "dns-verdict {verdict.slug()}",
                    i { class: "{icon}" }
                    div { class: "dns-verdict-text",
                        span { class: "dns-verdict-title", "{i18n.t(verdict.label())}" }
                        span { class: "dns-verdict-sub", "{summary}" }
                    }
                }
                div { class: "dns-resolver-list",
                    for resolver in data.resolvers.iter() {
                        {dns_resolver_row(i18n, resolver)}
                    }
                }
            }
        }
    }
}

fn dns_resolver_row(i18n: I18n, resolver: &DnsResolver) -> Element {
    let flag = resolver
        .country_code
        .as_deref()
        .and_then(country_flag)
        .unwrap_or_else(|| "🌐".to_string());
    let place = resolver
        .place
        .clone()
        .or_else(|| {
            resolver
                .country_code
                .as_deref()
                .and_then(country_name)
                .map(str::to_string)
        })
        .unwrap_or_else(|| i18n.t("Unknown location").to_string());
    let organization = resolver.organization.clone().unwrap_or_default();
    rsx! {
        div { key: "{resolver.ip}", class: "dns-resolver-row",
            span { class: "dns-resolver-flag", "{flag}" }
            div { class: "dns-resolver-main",
                span { class: "dns-resolver-ip", "{resolver.ip}" }
                span { class: "dns-resolver-place", "{place}" }
            }
            span { class: "dns-resolver-org", title: "{organization}", "{organization}" }
            div { class: "dns-resolver-sources",
                for source in resolver.sources.iter() {
                    span { class: "dns-source", "{source}" }
                }
            }
        }
    }
}
