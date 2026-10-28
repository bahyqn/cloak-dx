use std::sync::Arc;

use dioxus::prelude::*;

use crate::i18n::I18n;
use crate::services::{
    database::Database,
    proxy_core::{local_lan_ipv4, ProxyCoreService},
    xray_settings::{
        XraySettings, DEFAULT_LAN_HTTP_PORT, DEFAULT_LAN_SOCKS_PORT, DOMAIN_STRATEGIES,
        FAKE_DNS_POOL_SIZE_OPTIONS, LOG_LEVELS, MUX_CONCURRENCY_OPTIONS, MUX_PROXY_UDP443_OPTIONS,
        MUX_XUDP_CONCURRENCY_OPTIONS, REMOTE_DNS_PRESETS, TUN_MTU_OPTIONS, TUN_STACKS,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum XraySettingsSection {
    General,
    Routing,
    Dns,
    Tun,
    Advanced,
}

/// Xray options that apply to every config the app generates.
///
/// Each control saves immediately. If a core is running it is restarted in the
/// background with the new config, so changes take effect without restarting
/// the application or reconnecting manually.
#[component]
pub(super) fn XraySettingsPanel(section: XraySettingsSection) -> Element {
    let database = use_context::<Arc<Database>>();
    let proxy_core = use_context::<Arc<ProxyCoreService>>();
    let i18n = use_context::<I18n>();
    let mut settings = use_signal(|| XraySettings::load(&database));
    let mut status = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let core_status = proxy_core.status();
    let core_running = core_status.running;
    // While the core is stopped the last active ports are unknown, so show the
    // defaults that the next start will prefer.
    let lan_socks_port = core_status.lan_socks_port.unwrap_or(DEFAULT_LAN_SOCKS_PORT);
    let lan_http_port = core_status.lan_http_port.unwrap_or(DEFAULT_LAN_HTTP_PORT);
    // Other devices need this machine's network address, not loopback.
    let lan_address = local_lan_ipv4();

    let persist_database = Arc::clone(&database);
    let persist_core = Arc::clone(&proxy_core);
    let persist = EventHandler::new(move |next: XraySettings| {
        if busy() {
            return;
        }
        let previous = settings();
        busy.set(true);
        status.set(
            if persist_core.status().running {
                i18n.t("Applying changes to the running core...")
            } else {
                i18n.t("Saving...")
            }
            .to_string(),
        );
        let database = Arc::clone(&persist_database);
        let proxy_core = Arc::clone(&persist_core);
        spawn(async move {
            match proxy_core.apply_settings(&database, &previous, &next) {
                Ok(message) => {
                    settings.set(next);
                    status.set(message);
                }
                Err(message) => {
                    // `apply_settings` already restored the previous values on
                    // disk when the restarted core refused the new config.
                    settings.set(previous);
                    status.set(message);
                }
            }
            busy.set(false);
        });
    });

    let grant_database = Arc::clone(&database);
    let grant_core = Arc::clone(&proxy_core);

    let current = settings();
    let lan_host = lan_address
        .map(|address| address.to_string())
        .unwrap_or_else(|| i18n.t("LAN IP").to_string());
    let lan_username = current.lan_username.trim();
    let lan_auth_enabled = !lan_username.is_empty() && !current.lan_password.is_empty();
    let lan_auth_partial = !lan_username.is_empty() != !current.lan_password.is_empty();
    let heading = match section {
        XraySettingsSection::General => "Xray-core · General",
        XraySettingsSection::Routing => "Xray-core · Routing",
        XraySettingsSection::Dns => "Xray-core · DNS",
        XraySettingsSection::Tun => "Xray-core · TUN",
        XraySettingsSection::Advanced => "Xray-core · Advanced",
    };
    let icon = match section {
        XraySettingsSection::General => "fa-solid fa-sliders",
        XraySettingsSection::Routing => "fa-solid fa-route",
        XraySettingsSection::Dns => "fa-solid fa-server",
        XraySettingsSection::Tun => "fa-solid fa-network-wired",
        XraySettingsSection::Advanced => "fa-solid fa-gears",
    };

    rsx! {
        section { class: "card settings-card",
            h3 {
                i { class: "{icon}" }
                "{i18n.t(heading)}"
            }
            p { class: "settings-hint",
                i { class: "fa-solid fa-bolt" }
                if core_running {
                    "{i18n.t(\"Changes apply immediately and restart the running core.\")}"
                } else {
                    "{i18n.t(\"Changes are saved now and apply the next time a core starts.\")}"
                }
            }

            match section {
                XraySettingsSection::General => rsx! {
                    div { class: "setting-divider", "{i18n.t(\"Logging\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Log level\")}" }
                            span { "{i18n.t(\"Verbosity written to the core log\")}" }
                        }
                        select {
                            class: "setting-select",
                            value: "{current.log_level}",
                            disabled: busy(),
                            onchange: move |event| {
                                let mut next = settings();
                                next.log_level = event.value();
                                persist.call(next);
                            },
                            for level in LOG_LEVELS {
                                option {
                                    value: "{level}",
                                    selected: current.log_level == *level,
                                    "{level}"
                                }
                            }
                        }
                    }

                    div { class: "setting-divider", "{i18n.t(\"Multiplexing\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "Mux" }
                            span { "{i18n.t(\"Multiplex connections on the proxy outbound\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.mux_enabled,
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.mux_enabled = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }
                    if current.mux_enabled {
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Mux concurrency\")}" }
                                span { "{i18n.t(\"Maximum simultaneous multiplexed streams\")}" }
                            }
                            select {
                                class: "setting-select",
                                value: "{current.mux_concurrency}",
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.mux_concurrency = event
                                        .value()
                                        .parse()
                                        .unwrap_or_else(|_| settings().mux_concurrency);
                                    persist.call(next);
                                },
                                // Dioxus applies the select's `value` before
                                // its options are mounted, so on the first
                                // render no option would match. Marking the
                                // stored choice keeps it selected when the
                                // panel opens and after a restart.
                                for concurrency in MUX_CONCURRENCY_OPTIONS {
                                    option {
                                        value: "{concurrency}",
                                        selected: current.mux_concurrency == *concurrency,
                                        "{concurrency}"
                                    }
                                }
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"XUDP concurrency\")}" }
                                span { "{i18n.t(\"UDP sessions multiplexed over one connection. 0 disables XUDP.\")}" }
                            }
                            select {
                                class: "setting-select",
                                value: "{current.mux_xudp_concurrency}",
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.mux_xudp_concurrency = event
                                        .value()
                                        .parse()
                                        .unwrap_or_else(|_| settings().mux_xudp_concurrency);
                                    persist.call(next);
                                },
                                for concurrency in MUX_XUDP_CONCURRENCY_OPTIONS {
                                    option {
                                        value: "{concurrency}",
                                        selected: current.mux_xudp_concurrency == *concurrency,
                                        "{concurrency}"
                                    }
                                }
                            }
                        }
                        if current.mux_xudp_concurrency > 0 {
                            div { class: "setting-item",
                                div { class: "setting-info",
                                    label { "{i18n.t(\"QUIC (UDP 443) via mux\")}" }
                                    span { "{i18n.t(\"Reject asks apps to fall back to TCP, allow tunnels QUIC, skip bypasses mux for it\")}" }
                                }
                                select {
                                    class: "setting-select",
                                    value: "{current.mux_proxy_udp443}",
                                    disabled: busy(),
                                    onchange: move |event| {
                                        let mut next = settings();
                                        next.mux_proxy_udp443 = event.value();
                                        persist.call(next);
                                    },
                                    for policy in MUX_PROXY_UDP443_OPTIONS {
                                        option {
                                            value: "{policy}",
                                            selected: current.mux_proxy_udp443 == *policy,
                                            "{i18n.t(mux_udp443_label(policy))}"
                                        }
                                    }
                                }
                            }
                        }
                    }

                    div { class: "setting-divider", "{i18n.t(\"Node Selection\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Auto failover\")}" }
                            span { "{i18n.t(\"Probe every node and keep traffic on the fastest reachable one; manual node selection is ignored\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.auto_failover,
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.auto_failover = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }

                    div { class: "setting-divider", "{i18n.t(\"Local Ports\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"SOCKS port\")}" }
                            span { "{i18n.t(\"Loopback port used by apps and the system proxy\")}" }
                        }
                        input {
                            class: "setting-input",
                            r#type: "number",
                            min: "1",
                            max: "65535",
                            value: "{current.local_socks_port}",
                            disabled: busy(),
                            onchange: move |event| {
                                let mut next = settings();
                                match event.value().parse() {
                                    Ok(port) if port != 0 => next.local_socks_port = port,
                                    _ => return,
                                }
                                persist.call(next);
                            },
                        }
                    }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"HTTP port\")}" }
                            span { "{i18n.t(\"Must differ from the SOCKS port\")}" }
                        }
                        input {
                            class: "setting-input",
                            r#type: "number",
                            min: "1",
                            max: "65535",
                            value: "{current.local_http_port}",
                            disabled: busy(),
                            onchange: move |event| {
                                let mut next = settings();
                                match event.value().parse() {
                                    Ok(port) if port != 0 => next.local_http_port = port,
                                    _ => return,
                                }
                                persist.call(next);
                            },
                        }
                    }

                    div { class: "setting-divider", "{i18n.t(\"Local Network Access\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Allow LAN connections\")}" }
                            span { "{i18n.t(\"Let other devices on this network use this machine as a proxy\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.lan_enabled,
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.lan_enabled = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }
                    if current.lan_enabled {
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Username\")}" }
                                span { "{i18n.t(\"Leave both this and the password empty to accept connections without authentication\")}" }
                            }
                            input {
                                class: "setting-input",
                                r#type: "text",
                                value: "{current.lan_username}",
                                placeholder: "proxy",
                                autocomplete: "off",
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.lan_username = event.value().trim().to_string();
                                    persist.call(next);
                                },
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Password\")}" }
                                span { "{i18n.t(\"Must be set together with the username\")}" }
                            }
                            input {
                                class: "setting-input",
                                r#type: "password",
                                value: "{current.lan_password}",
                                placeholder: i18n.t("No password"),
                                autocomplete: "new-password",
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.lan_password = event.value();
                                    persist.call(next);
                                },
                            }
                        }
                        if lan_auth_partial {
                            p { class: "settings-hint",
                                i { class: "fa-solid fa-triangle-exclamation" }
                                "{i18n.t(\"Only one of the two fields is filled in, so authentication stays off. Set both a username and a password to require them.\")}"
                            }
                        } else if lan_auth_enabled {
                            p { class: "settings-hint",
                                i { class: "fa-solid fa-lock" }
                                "{i18n.t(\"LAN clients must send this username and password (SOCKS5 user/pass or HTTP basic auth).\")}"
                            }
                        } else {
                            p { class: "settings-hint",
                                i { class: "fa-solid fa-triangle-exclamation" }
                                "{i18n.t(\"Without a username and password, anyone who can reach this machine on the network can use the proxy. Only enable this on a trusted network.\")}"
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"LAN proxy ports\")}" }
                                span {
                                    if core_running && core_status.lan_socks_port.is_some() {
                                        "{i18n.t(\"Point other devices at this machine's LAN address with these ports\")}"
                                    } else {
                                        "{i18n.t(\"The next core start prefers these ports, and picks a free one if they are busy\")}"
                                    }
                                }
                            }
                            div { class: "lan-port-values",
                                span { class: "lan-port-value",
                                    span { class: "lan-port-label", "SOCKS5" }
                                    code { "{lan_host}:{lan_socks_port}" }
                                }
                                span { class: "lan-port-value",
                                    span { class: "lan-port-label", "HTTP" }
                                    code { "{lan_host}:{lan_http_port}" }
                                }
                            }
                        }
                        if lan_address.is_none() {
                            p { class: "settings-hint",
                                i { class: "fa-solid fa-circle-info" }
                                "{i18n.t(\"Could not detect this machine's LAN address. Use its network IP with the ports above.\")}"
                            }
                        }
                        p { class: "settings-hint",
                            i { class: "fa-solid fa-circle-info" }
                            "{i18n.t(\"Local applications keep using the loopback ports without a password, so the system proxy and built-in tests are unaffected.\")}"
                        }
                    }

                },
                XraySettingsSection::Routing => rsx! {
                    div { class: "setting-divider", "{i18n.t(\"Bypass\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Bypass mainland China\")}" }
                            span { "{i18n.t(\"Send Chinese domains and IPs direct\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.bypass_cn,
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.bypass_cn = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Bypass LAN and private networks\")}" }
                            span { "{i18n.t(\"Keep local addresses off the proxy\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.bypass_private,
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.bypass_private = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Block ads\")}" }
                            span { {i18n.t("Reject geosite:category-ads-all")} }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.block_ads,
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.block_ads = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Block QUIC (UDP 443)\")}" }
                            span { "{i18n.t(\"Make QUIC-capable apps fall back to TCP so the routing rules see their traffic\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.block_quic,
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.block_quic = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }

                    div { class: "setting-divider", "{i18n.t(\"Custom bypass\")}" }
                    div { class: "setting-item setting-item-stacked",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Bypass list\")}" }
                            span { "{i18n.t(\"Domains and IPs that connect directly, one entry per line\")}" }
                        }
                        textarea {
                            class: "setting-input setting-textarea",
                            rows: 5,
                            placeholder: "example.com\n*.example.org\n10.0.0.0/8\ngeoip:cn",
                            disabled: busy(),
                            onchange: move |event| {
                                let mut next = settings();
                                next.bypass_list = event.value();
                                persist.call(next);
                            },
                            "{current.bypass_list}"
                        }
                    }
                    p { class: "settings-hint",
                        i { class: "fa-solid fa-circle-info" }
                        {i18n.t("Accepts domains (example.com, *.example.com), IPs and CIDRs (10.0.0.0/8, 2001:db8::/32), and Xray prefixes such as domain:, full:, keyword:, regexp:, geosite: and geoip:. Lines starting with # are ignored.")}
                    }

                    div { class: "setting-divider", "{i18n.t(\"Outbound rules\")}" }
                    div { class: "setting-item setting-item-stacked",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Outbound rules\")}" }
                            span { "{i18n.t(\"Send a domain or IP to the proxy, direct, or the blackhole\")}" }
                        }
                        textarea {
                            class: "setting-input setting-textarea",
                            rows: 5,
                            placeholder: "example.com block\n10.0.0.0/8 direct\ngeosite:google proxy",
                            disabled: busy(),
                            onchange: move |event| {
                                let mut next = settings();
                                next.outbound_rules = event.value();
                                persist.call(next);
                            },
                            "{current.outbound_rules}"
                        }
                    }
                    p { class: "settings-hint",
                        i { class: "fa-solid fa-circle-info" }
                        {i18n.t("One rule per line: <domain-or-ip> <direct|proxy|block>. Rules run before the bypass rules above, so they can override them. Domain and IP syntax matches the bypass list.")}
                    }

                    div { class: "setting-divider", "{i18n.t(\"Resolution\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Domain strategy\")}" }
                            span { "{i18n.t(\"When Xray resolves a destination while routing\")}" }
                        }
                        select {
                            class: "setting-select",
                            value: "{current.domain_strategy}",
                            disabled: busy(),
                            onchange: move |event| {
                                let mut next = settings();
                                next.domain_strategy = event.value();
                                persist.call(next);
                            },
                            for strategy in DOMAIN_STRATEGIES {
                                option {
                                    value: "{strategy}",
                                    selected: current.domain_strategy == *strategy,
                                    "{strategy}"
                                }
                            }
                        }
                    }
                    p { class: "settings-hint",
                        i { class: "fa-solid fa-circle-info" }
                        "{i18n.t(\"Geo rules use the geodata downloaded in Core Management. Download Xray geodata there before enabling them.\")}"
                    }
                },
                XraySettingsSection::Dns => rsx! {
                    div { class: "setting-divider", "{i18n.t(\"Resolvers\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Local DNS server\")}" }
                            span { "{i18n.t(\"Resolves bypassed domains (China, private and the custom bypass list); empty uses the system resolver\")}" }
                        }
                        input {
                            class: "setting-input",
                            r#type: "text",
                            value: "{current.local_dns}",
                            placeholder: "223.5.5.5",
                            disabled: busy(),
                            onchange: move |event| {
                                let mut next = settings();
                                next.local_dns = event.value().trim().to_string();
                                persist.call(next);
                            },
                        }
                    }
                    div { class: "setting-item setting-item-stacked",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Remote DNS server\")}" }
                            span { "{i18n.t(\"Resolves proxied domains; the query is routed through the proxy, so it answers from the exit's region\")}" }
                        }
                        input {
                            class: "setting-input",
                            r#type: "text",
                            value: "{current.remote_dns}",
                            placeholder: i18n.t("1.1.1.1 or https://1.1.1.1/dns-query"),
                            disabled: busy(),
                            onchange: move |event| {
                                let mut next = settings();
                                next.remote_dns = event.value().trim().to_string();
                                persist.call(next);
                            },
                        }
                        div { class: "dns-presets",
                            for (label, address) in REMOTE_DNS_PRESETS.iter().copied() {
                                button {
                                    class: "btn-secondary small",
                                    disabled: busy(),
                                    onclick: move |_| {
                                        let mut next = settings();
                                        next.remote_dns = address.to_string();
                                        persist.call(next);
                                    },
                                    "{label}"
                                }
                            }
                        }
                    }
                    p { class: "settings-hint",
                        i { class: "fa-solid fa-circle-info" }
                        {i18n.t("Both resolvers can be enabled together: the local server answers bypassed domains and the remote server answers everything else. With only one set, it answers every query. While Fake DNS is on, proxied domains are answered by Fake DNS, so the remote server is not used.")}
                    }

                    div { class: "setting-divider", "{i18n.t(\"Fake DNS\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Enable Fake DNS\")}" }
                            span { "{i18n.t(\"Answer with synthetic IPs and recover the domain through sniffing\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.fake_dns_enabled,
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.fake_dns_enabled = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }
                    if current.fake_dns_enabled {
                        if !current.tun_enabled {
                            p { class: "settings-hint",
                                i { class: "fa-solid fa-triangle-exclamation" }
                                "{i18n.t(\"Fake DNS is only used while TUN mode is enabled in the TUN tab.\")}"
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Fake IP pool\")}" }
                                span { "{i18n.t(\"IPv4 CIDR used for synthetic answers\")}" }
                            }
                            input {
                                class: "setting-input",
                                r#type: "text",
                                value: "{current.fake_dns_ip_pool}",
                                placeholder: "198.18.0.0/15",
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.fake_dns_ip_pool = event.value().trim().to_string();
                                    persist.call(next);
                                },
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Pool size\")}" }
                                span { "{i18n.t(\"Maximum number of cached fake mappings\")}" }
                            }
                            select {
                                class: "setting-select",
                                value: "{current.fake_dns_pool_size}",
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.fake_dns_pool_size = event
                                        .value()
                                        .parse()
                                        .unwrap_or_else(|_| settings().fake_dns_pool_size);
                                    persist.call(next);
                                },
                                for size in FAKE_DNS_POOL_SIZE_OPTIONS {
                                    option {
                                        value: "{size}",
                                        selected: current.fake_dns_pool_size == *size,
                                        "{size}"
                                    }
                                }
                            }
                        }
                        p { class: "settings-hint",
                            i { class: "fa-solid fa-circle-info" }
                            {i18n.t("When IPv6 is enabled a 2001:2::/48 pool is added automatically.")}
                        }
                    }
                },
                XraySettingsSection::Tun => rsx! {
                    div { class: "setting-divider", "{i18n.t(\"TUN Interface\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Enable TUN mode\")}" }
                            span { "{i18n.t(\"Route all system traffic through Xray without app-level proxy settings\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.tun_enabled,
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.tun_enabled = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }
                    p { class: "settings-hint",
                        i { class: "fa-solid fa-triangle-exclamation" }
                        "{i18n.t(\"TUN needs network administrator privileges (CAP_NET_ADMIN). Without them the core cannot start; Cloak DX then restores the previous settings automatically.\")}"
                    }
                    if cfg!(target_os = "linux") {
                        div { class: "tun-privilege-row",
                            button {
                                class: "btn-secondary small",
                                r#type: "button",
                                disabled: busy(),
                                onclick: move |_| {
                                    if busy() {
                                        return;
                                    }
                                    busy.set(true);
                                    status.set(
                                        i18n.t("Waiting for administrator authorization...")
                                            .to_string(),
                                    );
                                    let database = Arc::clone(&grant_database);
                                    let proxy_core = Arc::clone(&grant_core);
                                    spawn(async move {
                                        match proxy_core.grant_tun_privileges(&database).await {
                                            Ok(()) => status.set(
                                                i18n.t("TUN permissions granted. Enable TUN mode if needed, then start the core.")
                                                    .to_string(),
                                            ),
                                            Err(error) => status.set(error),
                                        }
                                        busy.set(false);
                                    });
                                },
                                i { class: "fa-solid fa-shield-halved" }
                                "{i18n.t(\"Grant TUN permission\")}"
                            }
                        }
                        p { class: "settings-hint",
                            i { class: "fa-solid fa-circle-info" }
                            "{i18n.t(\"The permission is stored on the selected Xray binary; grant it again after the core is updated.\")}"
                        }
                    }

                    div { class: "setting-divider", "IPv6" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Enable IPv6\")}" }
                            span { "{i18n.t(\"Allow AAAA lookups and add IPv6 TUN addresses and routes\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.ipv6_enabled,
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.ipv6_enabled = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }

                    if current.tun_enabled {
                        div { class: "setting-divider", "{i18n.t(\"Interface\")}" }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Interface name\")}" }
                                span { "{i18n.t(\"Empty lets Xray pick an available name\")}" }
                            }
                            input {
                                class: "setting-input",
                                r#type: "text",
                                value: "{current.tun_name}",
                                placeholder: "cloak0",
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.tun_name = event.value().trim().to_string();
                                    persist.call(next);
                                },
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "MTU" }
                                span { "{i18n.t(\"Maximum packet size on the interface\")}" }
                            }
                            select {
                                class: "setting-select",
                                value: "{current.tun_mtu}",
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.tun_mtu = event
                                        .value()
                                        .parse()
                                        .unwrap_or_else(|_| settings().tun_mtu);
                                    persist.call(next);
                                },
                                for mtu in TUN_MTU_OPTIONS {
                                    option {
                                        value: "{mtu}",
                                        selected: current.tun_mtu == *mtu,
                                        "{mtu}"
                                    }
                                }
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Stack\")}" }
                                span { "{i18n.t(\"Network stack implementation\")}" }
                            }
                            select {
                                class: "setting-select",
                                value: "{current.tun_stack}",
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.tun_stack = event.value();
                                    persist.call(next);
                                },
                                for stack in TUN_STACKS {
                                    option {
                                        value: "{stack}",
                                        selected: current.tun_stack == *stack,
                                        "{stack}"
                                    }
                                }
                            }
                        }
                        p { class: "settings-hint",
                            i { class: "fa-solid fa-circle-info" }
                            "{i18n.t(\"Xray's TUN always uses its built-in stack; this preference applies to cores that support choosing one.\")}"
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"IPv4 gateway\")}" }
                                span { "{i18n.t(\"Address prefix assigned to the TUN interface\")}" }
                            }
                            input {
                                class: "setting-input",
                                r#type: "text",
                                value: "{current.tun_gateway}",
                                placeholder: "10.0.0.1/16",
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.tun_gateway = event.value().trim().to_string();
                                    persist.call(next);
                                },
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Auto route\")}" }
                                span { "{i18n.t(\"Add default routes and bind Xray outbounds to the physical interface\")}" }
                            }
                            label { class: "toggle-switch small",
                                input {
                                    r#type: "checkbox",
                                    checked: current.tun_auto_route,
                                    disabled: busy(),
                                    onchange: move |event| {
                                        let mut next = settings();
                                        next.tun_auto_route = event.checked();
                                        persist.call(next);
                                    },
                                }
                                span { class: "slider" }
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Strict route\")}" }
                                span { "{i18n.t(\"Use split default routes so existing routes cannot bypass the TUN\")}" }
                            }
                            label { class: "toggle-switch small",
                                input {
                                    r#type: "checkbox",
                                    checked: current.tun_strict_route,
                                    disabled: busy() || !current.tun_auto_route,
                                    onchange: move |event| {
                                        let mut next = settings();
                                        next.tun_strict_route = event.checked();
                                        persist.call(next);
                                    },
                                }
                                span { class: "slider" }
                            }
                        }
                        if !current.tun_auto_route {
                            p { class: "settings-hint",
                                i { class: "fa-solid fa-circle-info" }
                                "{i18n.t(\"Strict route only takes effect while Auto route is enabled.\")}"
                            }
                        }
                        p { class: "settings-hint",
                            i { class: "fa-solid fa-circle-info" }
                            {i18n.t("IPv6 gateway fc00::1/64 and its routes are added automatically while IPv6 is enabled above.")}
                        }
                    }
                },
                XraySettingsSection::Advanced => rsx! {
                    div { class: "setting-divider", "{i18n.t(\"Sniffing\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Enable sniffing\")}" }
                            span { "{i18n.t(\"Recover the hostname from HTTP, TLS and QUIC handshakes so domain rules also match apps that connect by IP\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.sniffing_enabled,
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.sniffing_enabled = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }
                    if current.sniffing_enabled {
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "HTTP" }
                                span { "{i18n.t(\"Detect plain HTTP requests\")}" }
                            }
                            label { class: "toggle-switch small",
                                input {
                                    r#type: "checkbox",
                                    checked: current.sniff_http,
                                    disabled: busy(),
                                    onchange: move |event| {
                                        let mut next = settings();
                                        next.sniff_http = event.checked();
                                        persist.call(next);
                                    },
                                }
                                span { class: "slider" }
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "TLS" }
                                span { "{i18n.t(\"Read the server name from the TLS handshake\")}" }
                            }
                            label { class: "toggle-switch small",
                                input {
                                    r#type: "checkbox",
                                    checked: current.sniff_tls,
                                    disabled: busy(),
                                    onchange: move |event| {
                                        let mut next = settings();
                                        next.sniff_tls = event.checked();
                                        persist.call(next);
                                    },
                                }
                                span { class: "slider" }
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "QUIC" }
                                span { "{i18n.t(\"Read the server name from QUIC Initial packets\")}" }
                            }
                            label { class: "toggle-switch small",
                                input {
                                    r#type: "checkbox",
                                    checked: current.sniff_quic,
                                    disabled: busy(),
                                    onchange: move |event| {
                                        let mut next = settings();
                                        next.sniff_quic = event.checked();
                                        persist.call(next);
                                    },
                                }
                                span { class: "slider" }
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Route only\")}" }
                                span { "{i18n.t(\"Use the sniffed hostname for routing only, keeping the original address for the connection\")}" }
                            }
                            label { class: "toggle-switch small",
                                input {
                                    r#type: "checkbox",
                                    checked: current.sniff_route_only,
                                    disabled: busy(),
                                    onchange: move |event| {
                                        let mut next = settings();
                                        next.sniff_route_only = event.checked();
                                        persist.call(next);
                                    },
                                }
                                span { class: "slider" }
                            }
                        }
                    }
                    p { class: "settings-hint",
                        i { class: "fa-solid fa-circle-info" }
                        "{i18n.t(\"TUN keeps sniffing on while FakeDNS is enabled, otherwise the fake addresses cannot be mapped back to their domains.\")}"
                    }

                    div { class: "setting-divider", "{i18n.t(\"Fragment\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Enable fragment\")}" }
                            span { "{i18n.t(\"Split the TLS ClientHello sent to the node into small pieces to evade DPI\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.fragment_enabled,
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.fragment_enabled = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }
                    if current.fragment_enabled {
                        div { class: "setting-item setting-item-stacked",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Packets\")}" }
                                span { {i18n.t("Which packets to split: tlshello or a packet range such as 1-3")} }
                            }
                            input {
                                class: "setting-input",
                                r#type: "text",
                                value: "{current.fragment_packets}",
                                placeholder: "tlshello",
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.fragment_packets = event.value().trim().to_string();
                                    persist.call(next);
                                },
                            }
                        }
                        div { class: "setting-item setting-item-stacked",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Length\")}" }
                                span { "{i18n.t(\"Bytes per fragment, as a count or a range such as 100-200\")}" }
                            }
                            input {
                                class: "setting-input",
                                r#type: "text",
                                value: "{current.fragment_length}",
                                placeholder: "100-200",
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.fragment_length = event.value().trim().to_string();
                                    persist.call(next);
                                },
                            }
                        }
                        div { class: "setting-item setting-item-stacked",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Interval\")}" }
                                span { "{i18n.t(\"Milliseconds between fragments, as a count or a range such as 10-20\")}" }
                            }
                            input {
                                class: "setting-input",
                                r#type: "text",
                                value: "{current.fragment_interval}",
                                placeholder: "10-20",
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.fragment_interval = event.value().trim().to_string();
                                    persist.call(next);
                                },
                            }
                        }
                    }
                    p { class: "settings-hint",
                        i { class: "fa-solid fa-triangle-exclamation" }
                        "{i18n.t(\"Some networks or nodes reject fragmented handshakes; turn fragment off if a node stops connecting. Fragment applies to node links; full Xray JSON subscriptions keep their own transport settings.\")}"
                    }
                },
            }

            if !status().is_empty() {
                p { class: "core-status-message", role: "status", "{status}" }
            }
        }
    }
}

/// Xray's `xudpProxyUDP443` values, named after what they do to QUIC traffic.
fn mux_udp443_label(policy: &str) -> &'static str {
    match policy {
        "allow" => "Allow",
        "skip" => "Skip mux",
        _ => "Reject",
    }
}
