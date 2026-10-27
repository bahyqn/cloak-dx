use std::sync::Arc;

use dioxus::{dioxus_core::spawn_forever, prelude::*};

use crate::services::{
    country::{country_flag, country_name},
    network_test::{
        self, DnsLeakProvider, DnsLeakResult, DnsLeakVerdict, DnsResolver, IppureResult,
    },
    proxy_core::ProxyCoreService,
    proxy_test::GoogleExitResult,
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
}

/// Network page.
///
/// Every check runs through the core's SOCKS inbound, so the page describes the
/// connection that is actually serving traffic. Nothing runs on its own: the
/// header button tests everything, and each card can be re-tested on its own.
#[component]
pub fn NetworkPage() -> Element {
    let proxy_core = use_context::<Arc<ProxyCoreService>>();
    let status = proxy_core.status();
    let socks_port = if status.running {
        status.socks_port
    } else {
        None
    };
    let node_label = status
        .node_name
        .clone()
        .unwrap_or_else(|| "No active node".to_string());

    // App-scoped so results survive navigating away from this page.
    let NetworkTestUi {
        google,
        ippure,
        mut dns,
        google_running,
        ippure_running,
        dns_running,
        mut provider,
    } = use_context::<NetworkTestUi>();

    let any_running = google_running() || ippure_running() || dns_running();
    let google_result = google();
    let ippure_result = ippure();
    let dns_result = dns();

    // Google's own localization — the country domain it redirects to — is what
    // a browser experiences, and IP databases can disagree with it (a node
    // registered in one country can be routed to another). IPPure only fills in
    // when Google could not report a country.
    let exit_country = google_result
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|exit| exit.country_code.clone())
        .or_else(|| {
            ippure_result
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .and_then(|ippure| ippure.country_code.clone())
        });
    let exit_label = exit_country
        .as_deref()
        .map(|code| {
            country_name(code)
                .map(str::to_string)
                .unwrap_or_else(|| code.to_ascii_uppercase())
        })
        .unwrap_or_else(|| "the exit country".to_string());

    let endpoint = match socks_port {
        Some(port) => format!("{node_label} · SOCKS 127.0.0.1:{port}"),
        None => "Proxy core is offline".to_string(),
    };

    rsx! {
        div { id: "network-view", class: "view-section active network-page",
            div { class: "settings-header",
                h2 { class: "page-title", "Network" }
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
                            run_google_test(port, google, google_running);
                            run_ippure_test(port, ippure, ippure_running);
                            run_dns_test(port, dns, dns_running, provider());
                        },
                        if any_running {
                            i { class: "fa-solid fa-rotate fa-spin" }
                            "Testing..."
                        } else {
                            i { class: "fa-solid fa-play" }
                            "Test"
                        }
                    }
                }
            }

            if socks_port.is_none() {
                div { class: "network-banner",
                    i { class: "fa-solid fa-triangle-exclamation" }
                    div {
                        strong { "Start the proxy core first" }
                        span {
                            "These checks run through the active node. Start the core from the Dashboard, then return here to inspect the connection."
                        }
                    }
                }
            }

            div { class: "network-grid",
                div { class: "settings-card network-card",
                    h3 {
                        span { class: "network-card-title",
                            i { class: "fa-brands fa-google" }
                            "Google Exit"
                        }
                        button {
                            class: "btn-secondary small",
                            title: "Test Google exit",
                            disabled: socks_port.is_none() || google_running(),
                            onclick: move |_| {
                                if let Some(port) = socks_port {
                                    run_google_test(port, google, google_running);
                                }
                            },
                            if google_running() {
                                i { class: "fa-solid fa-rotate fa-spin" }
                            } else {
                                i { class: "fa-solid fa-rotate" }
                            }
                            if google_result.is_none() { "Test" } else { "Re-test" }
                        }
                    }
                    {google_card_body(google_result.as_ref(), google_running())}
                }

                div { class: "settings-card network-card",
                    h3 {
                        span { class: "network-card-title",
                            i { class: "fa-solid fa-shield-halved" }
                            "IPPure"
                        }
                        button {
                            class: "btn-secondary small",
                            title: "Test with IPPure",
                            disabled: socks_port.is_none() || ippure_running(),
                            onclick: move |_| {
                                if let Some(port) = socks_port {
                                    run_ippure_test(port, ippure, ippure_running);
                                }
                            },
                            if ippure_running() {
                                i { class: "fa-solid fa-rotate fa-spin" }
                            } else {
                                i { class: "fa-solid fa-rotate" }
                            }
                            if ippure_result.is_none() { "Test" } else { "Re-test" }
                        }
                    }
                    {ippure_card_body(ippure_result.as_ref(), ippure_running())}
                }

                div { class: "settings-card network-card dns-card",
                    h3 {
                        span { class: "network-card-title",
                            i { class: "fa-solid fa-magnifying-glass-location" }
                            "DNS Leak"
                        }
                        div { class: "network-card-actions",
                            select {
                                class: "dns-provider-select",
                                title: "DNS leak check provider",
                                "aria-label": "DNS leak check provider",
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
                                title: "Run the DNS leak check",
                                disabled: socks_port.is_none() || dns_running(),
                                onclick: move |_| {
                                    if let Some(port) = socks_port {
                                        run_dns_test(port, dns, dns_running, provider());
                                    }
                                },
                                if dns_running() {
                                    i { class: "fa-solid fa-rotate fa-spin" }
                                } else {
                                    i { class: "fa-solid fa-rotate" }
                                }
                                if dns_result.is_none() { "Test" } else { "Re-test" }
                            }
                        }
                    }
                    {dns_card_body(dns_result.as_ref(), exit_country.as_deref(), &exit_label, dns_running())}
                }
            }
        }
    }
}

fn run_google_test(socks_port: u16, mut state: GoogleState, mut running: Signal<bool>) {
    running.set(true);
    // Not tied to this page's scope: navigating away must not cancel the test.
    spawn_forever(async move {
        let result = network_test::test_google_exit(socks_port).await;
        state.set(Some(result));
        running.set(false);
    });
}

fn run_ippure_test(socks_port: u16, mut state: IppureState, mut running: Signal<bool>) {
    running.set(true);
    spawn_forever(async move {
        let result = network_test::test_ippure(socks_port).await;
        state.set(Some(result));
        running.set(false);
    });
}

fn run_dns_test(
    socks_port: u16,
    mut state: DnsState,
    mut running: Signal<bool>,
    provider: DnsLeakProvider,
) {
    running.set(true);
    spawn_forever(async move {
        let result = network_test::test_dns_leak(socks_port, provider).await;
        state.set(Some(result));
        running.set(false);
    });
}

fn google_card_body(result: Option<&Result<GoogleExitResult, String>>, running: bool) -> Element {
    match result {
        None => rsx! {
            div { class: "network-empty",
                if running {
                    i { class: "fa-solid fa-rotate fa-spin" }
                } else {
                    i { class: "fa-solid fa-globe" }
                }
                p { if running { "Probing Google..." } else { "Not tested yet" } }
                span { "Shows the country and city Google infers from the active exit IP." }
            }
        },
        Some(Err(error)) => rsx! {
            div { class: "network-error",
                i { class: "fa-solid fa-triangle-exclamation" }
                p { "Google exit probe failed" }
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
                .unwrap_or_else(|| "Unknown".to_string());
            let region = exit.region.clone().unwrap_or_else(|| "Unknown".to_string());
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
                        span { class: "network-k", "Exit IP" }
                        span { class: "network-v network-mono", title: "{exit.ip}", "{exit.ip}" }
                    }
                    div { class: "network-kv",
                        span { class: "network-k", "Google country" }
                        span { class: "network-v", "{country} · {code}" }
                    }
                    div { class: "network-kv",
                        span { class: "network-k", "City / region" }
                        span { class: "network-v", "{region}" }
                    }
                }
            }
        }
    }
}

fn ippure_card_body(result: Option<&Result<IppureResult, String>>, running: bool) -> Element {
    match result {
        None => rsx! {
            div { class: "network-empty",
                if running {
                    i { class: "fa-solid fa-rotate fa-spin" }
                } else {
                    i { class: "fa-solid fa-shield-halved" }
                }
                p { if running { "Asking IPPure..." } else { "Not tested yet" } }
                span { "IPPure scores the exit IP for risk, ASN type, native/broadcast and automation." }
            }
        },
        Some(Err(error)) => rsx! {
            div { class: "network-error",
                i { class: "fa-solid fa-triangle-exclamation" }
                p { "IPPure lookup failed" }
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
                .unwrap_or_else(|| "Unknown".to_string());
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
                .unwrap_or_else(|| "Unknown".to_string());
            let ratios = match (data.human_ratio, data.bot_ratio) {
                (Some(human), Some(bot)) => format!("{human:.1}% human · {bot:.1}% bot"),
                (Some(human), None) => format!("{human:.1}% human"),
                (None, Some(bot)) => format!("{bot:.1}% bot"),
                (None, None) => "—".to_string(),
            };
            rsx! {
                div { class: "network-score",
                    span { class: "network-score-value", style: "color: {risk_color}", "{score}" }
                    div { class: "network-score-meta",
                        span { class: "network-score-label", style: "color: {risk_color}", "{risk_label}" }
                        span { class: "network-score-sub", "IPPure risk score" }
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
                        span { class: "network-k", "Exit IP" }
                        span { class: "network-v network-mono", title: "{data.ip}", "{data.ip}" }
                    }
                    div { class: "network-kv",
                        span { class: "network-k", "ASN" }
                        span { class: "network-v", "{asn} · {organization}" }
                    }
                    div { class: "network-kv",
                        span { class: "network-k", "Location" }
                        span { class: "network-v", "{flag} {place}" }
                    }
                    div { class: "network-kv",
                        span { class: "network-k", "Human / bot" }
                        span { class: "network-v", "{ratios}" }
                    }
                }
            }
        }
    }
}

fn dns_card_body(
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
                p { if running { "Probing public resolvers..." } else { "Not tested yet" } }
                span { "Random hostnames are resolved with this machine's own DNS, so a leaking system resolver shows up here." }
            }
        },
        Some(Err(error)) => rsx! {
            div { class: "network-error",
                i { class: "fa-solid fa-triangle-exclamation" }
                p { "DNS leak check failed" }
                span { "{error}" }
            }
        },
        Some(Ok(data)) => {
            let verdict = data.verdict(exit_country);
            let (icon, summary) = match verdict {
                DnsLeakVerdict::Protected => (
                    "fa-solid fa-circle-check",
                    format!(
                        "Resolvers are consistent with {exit_label} ({} seen).",
                        data.resolvers.len()
                    ),
                ),
                DnsLeakVerdict::Mixed => (
                    "fa-solid fa-scale-balanced",
                    format!("Some resolvers sit outside {exit_label}."),
                ),
                DnsLeakVerdict::Leaking => (
                    "fa-solid fa-triangle-exclamation",
                    format!("No resolver sits in {exit_label}."),
                ),
            };
            rsx! {
                if let Some(fallback) = data.fallback {
                    div { class: "dns-fallback-note",
                        i { class: "fa-solid fa-circle-info" }
                        span {
                            "ipleak.net did not see this machine's DNS lookups (the resolver answered the probe names from cache), so the resolvers below were measured with {fallback.label()} probes."
                        }
                    }
                }
                div { class: "dns-verdict {verdict.slug()}",
                    i { class: "{icon}" }
                    div { class: "dns-verdict-text",
                        span { class: "dns-verdict-title", "{verdict.label()}" }
                        span { class: "dns-verdict-sub", "{summary}" }
                    }
                }
                div { class: "dns-resolver-list",
                    for resolver in data.resolvers.iter() {
                        {dns_resolver_row(resolver)}
                    }
                }
            }
        }
    }
}

fn dns_resolver_row(resolver: &DnsResolver) -> Element {
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
        .unwrap_or_else(|| "Unknown location".to_string());
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
