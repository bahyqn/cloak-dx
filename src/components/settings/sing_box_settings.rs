use std::sync::Arc;

use dioxus::prelude::*;

use crate::{
    i18n::I18n,
    services::{
        database::Database,
        proxy_core::{local_lan_ipv4, ProxyCoreService},
        sing_box_settings::{
            SingBoxSettings, DNS_STRATEGIES, LOG_LEVELS, MULTIPLEX_PROTOCOLS,
            MUX_MAX_CONNECTIONS_OPTIONS, ROUTING_MODES, TUN_MTU_OPTIONS, TUN_STACKS,
        },
    },
};

/// sing-box settings, kept in their own module so changes to one engine can
/// never leak into the other. Every section here has its own tab and its own
/// `sing_box_*` storage keys.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SingBoxSettingsSection {
    General,
    Routing,
    Dns,
    Tun,
    Advanced,
}

impl SingBoxSettingsSection {
    /// Sections with their own tab. General is shown next to Appearance in the
    /// shared General tab, like Xray's General section.
    pub(super) const TABS: &'static [Self] = &[Self::Routing, Self::Dns, Self::Tun, Self::Advanced];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Routing => "Routing",
            Self::Dns => "DNS",
            Self::Tun => "TUN",
            Self::Advanced => "Advanced",
        }
    }

    pub(super) fn icon(self) -> &'static str {
        match self {
            Self::General => "fa-solid fa-sliders",
            Self::Routing => "fa-solid fa-route",
            Self::Dns => "fa-solid fa-server",
            Self::Tun => "fa-solid fa-network-wired",
            Self::Advanced => "fa-solid fa-gears",
        }
    }
}

/// sing-box options that feed the config generated for that engine.
///
/// The controls save immediately under `sing_box_*` keys. While sing-box is the
/// running core, saving also restarts it so the generated config matches the
/// values shown here.
#[component]
pub(super) fn SingBoxSettingsPanel(section: SingBoxSettingsSection) -> Element {
    let database = use_context::<Arc<Database>>();
    let proxy_core = use_context::<Arc<ProxyCoreService>>();
    let i18n = use_context::<I18n>();
    let mut settings = use_signal(|| SingBoxSettings::load(&database));
    let mut status = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let core_status = proxy_core.status();
    let core_running = core_status.running && core_status.engine.as_deref() == Some("sing-box");

    let persist_database = Arc::clone(&database);
    let persist_core = Arc::clone(&proxy_core);
    let grant_database = Arc::clone(&database);
    let grant_core = Arc::clone(&proxy_core);
    let persist = EventHandler::new(move |next: SingBoxSettings| {
        if busy() {
            return;
        }
        let previous = settings();
        busy.set(true);
        status.set(
            if core_running {
                i18n.t("Applying changes to the running core...")
            } else {
                i18n.t("Saving...")
            }
            .to_string(),
        );
        // `reload_running_subscription` waits for the core to bind its ports,
        // so the work is moved off the render thread like the Xray panel does.
        let database = Arc::clone(&persist_database);
        let proxy_core = Arc::clone(&persist_core);
        spawn(async move {
            match apply_sing_box_settings(&proxy_core, &database, &previous, &next) {
                Ok(message) => {
                    settings.set(next);
                    status.set(message);
                }
                Err(message) => {
                    // The previous values were written back when the restart
                    // failed, so the form keeps matching what is on disk.
                    settings.set(previous);
                    status.set(message);
                }
            }
            busy.set(false);
        });
    });

    let current = settings();
    let lan_address = local_lan_ipv4();
    let lan_host = lan_address
        .map(|address| address.to_string())
        .unwrap_or_else(|| i18n.t("LAN IP").to_string());
    // While sing-box runs, the status carries the port it actually bound; before
    // that, the configured port is what the next start will try first.
    let lan_port = if core_running {
        core_status.lan_socks_port.unwrap_or(current.lan_port)
    } else {
        current.lan_port
    };
    let heading = match section {
        SingBoxSettingsSection::General => "sing-box · General",
        SingBoxSettingsSection::Routing => "sing-box · Routing",
        SingBoxSettingsSection::Dns => "sing-box · DNS",
        SingBoxSettingsSection::Tun => "sing-box · TUN",
        SingBoxSettingsSection::Advanced => "sing-box · Advanced",
    };
    let lan_auth_partial =
        !current.lan_username.trim().is_empty() != !current.lan_password.is_empty();
    // Held in bindings rather than inline in the RSX: the Dioxus formatter
    // parser treats the colon inside these strings as a formatting separator.
    let reject_ads_description = i18n.t("Reject geosite:category-ads-all");
    let custom_rules_description = i18n.t("One rule per line: <domain-or-ip> <direct|proxy|block>. Rules run before the automatic bypass rules, so they can override them.");

    rsx! {
        section { class: "card settings-card",
            h3 {
                i { class: section.icon() }
                "{i18n.t(heading)}"
            }
            p { class: "settings-hint",
                i { class: "fa-solid fa-circle-info" }
                if core_running {
                    "{i18n.t(\"Changes apply immediately and restart the running core.\")}"
                } else {
                    "{i18n.t(\"These options are saved immediately and take effect the next time sing-box starts.\")}"
                }
            }

            match section {
                SingBoxSettingsSection::General => rsx! {
                    div { class: "setting-divider", "{i18n.t(\"Logging\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Log level\")}" }
                            span { "{i18n.t(\"Verbosity written to the core log\")}" }
                        }
                        select {
                            class: "setting-select",
                            value: "{current.log_level}",
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
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Log timestamps\")}" }
                            span { "{i18n.t(\"Prefix every log line with the time\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.log_timestamp,
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.log_timestamp = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }

                    div { class: "setting-divider", "{i18n.t(\"Network\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Enable IPv6\")}" }
                            span { "{i18n.t(\"Allow AAAA lookups and IPv6 addresses in the generated config\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.ipv6_enabled,
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.ipv6_enabled = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }

                    div { class: "setting-divider", "{i18n.t(\"Local Ports\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Mixed port\")}" }
                            span { "{i18n.t(\"Single loopback port serving both SOCKS and HTTP for apps and the system proxy\")}" }
                        }
                        input {
                            class: "setting-input",
                            r#type: "number",
                            min: "1",
                            max: "65535",
                            value: "{current.mixed_port}",
                            onchange: move |event| {
                                let mut next = settings();
                                match event.value().parse::<u16>() {
                                    Ok(value) if value != 0 => next.mixed_port = value,
                                    _ => {
                                        status.set(
                                            i18n.t("Enter a port between 1 and 65535.")
                                                .to_string(),
                                        );
                                        return;
                                    }
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
                                label { "{i18n.t(\"LAN port\")}" }
                                span { "{i18n.t(\"Port other devices connect to for mixed SOCKS/HTTP traffic\")}" }
                            }
                            input {
                                class: "setting-input",
                                r#type: "number",
                                min: "1",
                                max: "65535",
                                value: "{current.lan_port}",
                                onchange: move |event| {
                                    let mut next = settings();
                                    match event.value().parse::<u16>() {
                                        Ok(value) if value != 0 => next.lan_port = value,
                                        _ => {
                                            status.set(
                                                i18n.t("Enter a port between 1 and 65535.")
                                                    .to_string(),
                                            );
                                            return;
                                        }
                                    }
                                    persist.call(next);
                                },
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"LAN address\")}" }
                                span { "{i18n.t(\"Point other devices at this address with the port above\")}" }
                            }
                            div { class: "lan-port-values",
                                span { class: "lan-port-value",
                                    span { class: "lan-port-label", "MIXED" }
                                    code { "{lan_host}:{lan_port}" }
                                }
                            }
                        }
                        if lan_address.is_none() {
                            p { class: "settings-hint",
                                i { class: "fa-solid fa-circle-info" }
                                "{i18n.t(\"Could not detect this machine's LAN address. Use its network IP with the port above.\")}"
                            }
                        }
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
                        }
                    }
                },

                SingBoxSettingsSection::Routing => rsx! {
                    div { class: "setting-divider", "{i18n.t(\"Routing mode\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Routing mode\")}" }
                            span { "{i18n.t(\"Rule follows the routing rules below, Global proxies every connection, Direct disables the proxy\")}" }
                        }
                        select {
                            class: "setting-select",
                            value: "{current.routing_mode}",
                            onchange: move |event| {
                                let mut next = settings();
                                next.routing_mode = event.value();
                                persist.call(next);
                            },
                            for mode in ROUTING_MODES {
                                option {
                                    value: "{mode}",
                                    selected: current.routing_mode == *mode,
                                    "{i18n.t(routing_mode_label(mode))}"
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
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.auto_failover = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }

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
                            span { "{reject_ads_description}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.block_ads,
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
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.block_quic = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }

                    div { class: "setting-divider", "{i18n.t(\"Custom routing rules\")}" }
                    div { class: "setting-item setting-item-stacked",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Custom routing rules\")}" }
                            span { "{i18n.t(\"Send a domain or IP to the proxy, direct, or the blackhole\")}" }
                        }
                        textarea {
                            class: "setting-input setting-textarea",
                            rows: 5,
                            placeholder: "example.com block\n10.0.0.0/8 direct\ngeosite:google proxy",
                            onchange: move |event| {
                                let mut next = settings();
                                next.custom_rules = event.value();
                                persist.call(next);
                            },
                            "{current.custom_rules}"
                        }
                    }
                    p { class: "settings-hint",
                        i { class: "fa-solid fa-circle-info" }
                        "{custom_rules_description}"
                    }
                },

                SingBoxSettingsSection::Dns => rsx! {
                    div { class: "setting-divider", "{i18n.t(\"Resolution\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"DNS strategy\")}" }
                            span { "{i18n.t(\"Which address family sing-box prefers when a domain has both A and AAAA records\")}" }
                        }
                        select {
                            class: "setting-select",
                            value: "{current.dns_strategy}",
                            onchange: move |event| {
                                let mut next = settings();
                                next.dns_strategy = event.value();
                                persist.call(next);
                            },
                            for strategy in DNS_STRATEGIES {
                                option {
                                    value: "{strategy}",
                                    selected: current.dns_strategy == *strategy,
                                    "{i18n.t(dns_strategy_label(strategy))}"
                                }
                            }
                        }
                    }

                    div { class: "setting-divider", "{i18n.t(\"Resolvers\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Local DNS server\")}" }
                            span { "{i18n.t(\"Resolves bypassed domains (China, private and custom direct rules); empty uses the system resolver\")}" }
                        }
                        input {
                            class: "setting-input",
                            r#type: "text",
                            value: "{current.local_dns}",
                            placeholder: "1.1.1.1 or https://1.1.1.1/dns-query",
                            onchange: move |event| {
                                let mut next = settings();
                                next.local_dns = event.value().trim().to_string();
                                persist.call(next);
                            },
                        }
                    }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Remote DNS server\")}" }
                            span { "{i18n.t(\"Resolves proxied domains; the query is routed through the proxy, so it answers from the exit's region\")}" }
                        }
                        input {
                            class: "setting-input",
                            r#type: "text",
                            value: "{current.remote_dns}",
                            placeholder: "1.1.1.1 or https://1.1.1.1/dns-query",
                            onchange: move |event| {
                                let mut next = settings();
                                next.remote_dns = event.value().trim().to_string();
                                persist.call(next);
                            },
                        }
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
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"IPv4 range\")}" }
                                span { "{i18n.t(\"CIDR used for synthetic IPv4 answers, e.g. 198.18.0.0/15\")}" }
                            }
                            input {
                                class: "setting-input",
                                r#type: "text",
                                value: "{current.fake_dns_ipv4_pool}",
                                placeholder: "198.18.0.0/15",
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.fake_dns_ipv4_pool = event.value().trim().to_string();
                                    persist.call(next);
                                },
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"IPv6 range\")}" }
                                span { "{i18n.t(\"CIDR used for synthetic IPv6 answers, e.g. fc00::/18\")}" }
                            }
                            input {
                                class: "setting-input",
                                r#type: "text",
                                value: "{current.fake_dns_ipv6_pool}",
                                placeholder: "fc00::/18",
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.fake_dns_ipv6_pool = event.value().trim().to_string();
                                    persist.call(next);
                                },
                            }
                        }
                        p { class: "settings-hint",
                            i { class: "fa-solid fa-circle-info" }
                            "{i18n.t(\"Fake DNS needs sniffing, and is most useful with TUN mode enabled in the TUN tab.\")}"
                        }
                    }
                },

                SingBoxSettingsSection::Tun => rsx! {
                    div { class: "setting-divider", "{i18n.t(\"TUN Interface\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Enable TUN mode\")}" }
                            span { "{i18n.t(\"Route all system traffic through sing-box without app-level proxy settings\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.tun_enabled,
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
                        "{i18n.t(\"TUN needs administrator capabilities on the sing-box binary (CAP_NET_ADMIN, CAP_NET_RAW). Cloak DX asks for them automatically when sing-box starts with TUN enabled; you can also grant them now.\")}"
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
                            "{i18n.t(\"The permission is stored on the selected sing-box binary; grant it again after the core is updated.\")}"
                        }
                    }
                    if current.tun_enabled {
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Interface name\")}" }
                                span { "{i18n.t(\"Empty lets sing-box pick an available name\")}" }
                            }
                            input {
                                class: "setting-input",
                                r#type: "text",
                                value: "{current.tun_name}",
                                placeholder: "cloak0",
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
                                label { "{i18n.t(\"IPv4 address\")}" }
                                span { "{i18n.t(\"Address prefix assigned to the TUN interface\")}" }
                            }
                            input {
                                class: "setting-input",
                                r#type: "text",
                                value: "{current.tun_address}",
                                placeholder: "172.19.0.1/30",
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.tun_address = event.value().trim().to_string();
                                    persist.call(next);
                                },
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
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.tun_stack = event.value();
                                    persist.call(next);
                                },
                                for stack in TUN_STACKS {
                                    option {
                                        value: "{stack}",
                                        selected: current.tun_stack == *stack,
                                        "{i18n.t(stack_label(stack))}"
                                    }
                                }
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Auto route\")}" }
                                span { "{i18n.t(\"Add default routes so traffic enters the TUN interface\")}" }
                            }
                            label { class: "toggle-switch small",
                                input {
                                    r#type: "checkbox",
                                    checked: current.tun_auto_route,
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
                                    onchange: move |event| {
                                        let mut next = settings();
                                        next.tun_strict_route = event.checked();
                                        persist.call(next);
                                    },
                                }
                                span { class: "slider" }
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Auto redirect\")}" }
                                span { "{i18n.t(\"Use nftables to redirect traffic without adding routes (Linux only)\")}" }
                            }
                            label { class: "toggle-switch small",
                                input {
                                    r#type: "checkbox",
                                    checked: current.tun_auto_redirect,
                                    onchange: move |event| {
                                        let mut next = settings();
                                        next.tun_auto_redirect = event.checked();
                                        persist.call(next);
                                    },
                                }
                                span { class: "slider" }
                            }
                        }
                    }
                },

                SingBoxSettingsSection::Advanced => rsx! {
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
                                label { "{i18n.t(\"Override destination\")}" }
                                span { "{i18n.t(\"Connect to the sniffed domain instead of the original IP\")}" }
                            }
                            label { class: "toggle-switch small",
                                input {
                                    r#type: "checkbox",
                                    checked: current.sniff_override,
                                    onchange: move |event| {
                                        let mut next = settings();
                                        next.sniff_override = event.checked();
                                        persist.call(next);
                                    },
                                }
                                span { class: "slider" }
                            }
                        }
                    }

                    div { class: "setting-divider", "{i18n.t(\"Multiplexing\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Multiplex connections on the proxy outbound\")}" }
                            span { "{i18n.t(\"Reduce handshake overhead when the server supports it\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.mux_enabled,
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
                                label { "{i18n.t(\"Protocol\")}" }
                                span { "{i18n.t(\"Multiplex protocol used with the server\")}" }
                            }
                            select {
                                class: "setting-select",
                                value: "{current.mux_protocol}",
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.mux_protocol = event.value();
                                    persist.call(next);
                                },
                                for protocol in MULTIPLEX_PROTOCOLS {
                                    option {
                                        value: "{protocol}",
                                        selected: current.mux_protocol == *protocol,
                                        "{i18n.t(multiplex_protocol_label(protocol))}"
                                    }
                                }
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Max connections\")}" }
                                span { "{i18n.t(\"Maximum number of multiplexed connections kept open\")}" }
                            }
                            select {
                                class: "setting-select",
                                value: "{current.mux_max_connections}",
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.mux_max_connections = event
                                        .value()
                                        .parse()
                                        .unwrap_or_else(|_| settings().mux_max_connections);
                                    persist.call(next);
                                },
                                for connections in MUX_MAX_CONNECTIONS_OPTIONS {
                                    option {
                                        value: "{connections}",
                                        selected: current.mux_max_connections == *connections,
                                        "{connections}"
                                    }
                                }
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "{i18n.t(\"Padding\")}" }
                                span { "{i18n.t(\"Pad multiplexed traffic to hide its size\")}" }
                            }
                            label { class: "toggle-switch small",
                                input {
                                    r#type: "checkbox",
                                    checked: current.mux_padding,
                                    onchange: move |event| {
                                        let mut next = settings();
                                        next.mux_padding = event.checked();
                                        persist.call(next);
                                    },
                                }
                                span { class: "slider" }
                            }
                        }
                    }

                    div { class: "setting-divider", "{i18n.t(\"Connection\")}" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"TCP Fast Open\")}" }
                            span { "{i18n.t(\"Allow TCP Fast Open on outbound connections\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.tcp_fast_open,
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.tcp_fast_open = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "{i18n.t(\"Cache file\")}" }
                            span { "{i18n.t(\"Store DNS and selector state across restarts\")}" }
                        }
                        label { class: "toggle-switch small",
                            input {
                                r#type: "checkbox",
                                checked: current.cache_file_enabled,
                                onchange: move |event| {
                                    let mut next = settings();
                                    next.cache_file_enabled = event.checked();
                                    persist.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }
                },
            }

            if !status().is_empty() {
                p { class: "core-status-message", role: "status", "{status}" }
            }
        }
    }
}

fn routing_mode_label(mode: &str) -> &'static str {
    match mode {
        "global" => "Global",
        "direct" => "Direct",
        _ => "Rule",
    }
}

fn dns_strategy_label(strategy: &str) -> &'static str {
    match strategy {
        "prefer_ipv6" => "Prefer IPv6",
        "ipv4_only" => "IPv4 only",
        "ipv6_only" => "IPv6 only",
        _ => "Prefer IPv4",
    }
}

fn stack_label(stack: &str) -> &'static str {
    match stack {
        "system" => "System",
        "gvisor" => "gVisor",
        _ => "Mixed",
    }
}

fn multiplex_protocol_label(protocol: &str) -> &'static str {
    match protocol {
        "smux" => "smux",
        "yamux" => "yamux",
        _ => "h2mux",
    }
}

/// Saves sing-box settings and applies them to the running core.
///
/// Only a running sing-box core is restarted: the panel lives on the same page
/// as the Xray options, so editing sing-box values while Xray runs must not
/// interrupt the connection. When the new config is rejected, the previous
/// values are stored again so the form keeps matching the running core.
fn apply_sing_box_settings(
    proxy_core: &ProxyCoreService,
    database: &Database,
    previous: &SingBoxSettings,
    next: &SingBoxSettings,
) -> Result<String, String> {
    next.save(database)?;

    let status = proxy_core.status();
    if !status.running || status.engine.as_deref() != Some("sing-box") {
        return Ok("Saved. sing-box will use these settings when it starts.".to_string());
    }

    match proxy_core.reload_running_subscription(database) {
        Ok(Some(_)) => Ok(
            "Saved and applied. The running core was restarted with the new settings.".to_string(),
        ),
        Ok(None) => {
            Ok("Saved. No core is running; the settings apply at the next start.".to_string())
        }
        Err(error) => {
            let restored = previous
                .save(database)
                .and_then(|()| proxy_core.reload_running_subscription(database).map(|_| ()));
            Err(match restored {
                Ok(()) => format!(
                    "Could not apply the new settings: {error}. The previous settings were restored."
                ),
                Err(restore_error) => format!(
                    "Could not apply the new settings: {error}. Restoring the previous settings also failed: {restore_error}"
                ),
            })
        }
    }
}
