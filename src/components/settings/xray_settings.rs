use std::sync::Arc;

use dioxus::prelude::*;

use crate::services::{
    database::Database,
    proxy_core::ProxyCoreService,
    xray_settings::{
        XraySettings, DOMAIN_STRATEGIES, FAKE_DNS_POOL_SIZE_OPTIONS, LOG_LEVELS,
        MUX_CONCURRENCY_OPTIONS, TUN_MTU_OPTIONS, TUN_STACKS,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum XraySettingsSection {
    General,
    Routing,
    Dns,
    Tun,
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
    let mut settings = use_signal(|| XraySettings::load(&database));
    let mut status = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let core_running = proxy_core.status().running;

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
                "Applying changes to the running core..."
            } else {
                "Saving..."
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
    let heading = match section {
        XraySettingsSection::General => "Xray-core · General",
        XraySettingsSection::Routing => "Xray-core · Routing",
        XraySettingsSection::Dns => "Xray-core · DNS",
        XraySettingsSection::Tun => "Xray-core · TUN",
    };
    let icon = match section {
        XraySettingsSection::General => "fa-solid fa-sliders",
        XraySettingsSection::Routing => "fa-solid fa-route",
        XraySettingsSection::Dns => "fa-solid fa-server",
        XraySettingsSection::Tun => "fa-solid fa-network-wired",
    };

    rsx! {
        section { class: "card settings-card",
            h3 {
                i { class: "{icon}" }
                "{heading}"
            }
            p { class: "settings-hint",
                i { class: "fa-solid fa-bolt" }
                if core_running {
                    "Changes apply immediately and restart the running core."
                } else {
                    "Changes are saved now and apply the next time a core starts."
                }
            }

            match section {
                XraySettingsSection::General => rsx! {
                    div { class: "setting-divider", "Logging" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "Log level" }
                            span { "Verbosity written to the core log" }
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
                                option { value: "{level}", "{level}" }
                            }
                        }
                    }

                    div { class: "setting-divider", "Multiplexing" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "Mux" }
                            span { "Multiplex connections on the proxy outbound" }
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
                                label { "Mux concurrency" }
                                span { "Maximum simultaneous multiplexed streams" }
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
                                for concurrency in MUX_CONCURRENCY_OPTIONS {
                                    option { value: "{concurrency}", "{concurrency}" }
                                }
                            }
                        }
                    }

                },
                XraySettingsSection::Routing => rsx! {
                    div { class: "setting-divider", "Bypass" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "Bypass mainland China" }
                            span { "Send Chinese domains and IPs direct" }
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
                            label { "Bypass LAN and private networks" }
                            span { "Keep local addresses off the proxy" }
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
                            label { "Block ads" }
                            span { "Reject geosite:category-ads-all" }
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

                    div { class: "setting-divider", "Resolution" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "Domain strategy" }
                            span { "When Xray resolves a destination while routing" }
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
                                option { value: "{strategy}", "{strategy}" }
                            }
                        }
                    }
                    p { class: "settings-hint",
                        i { class: "fa-solid fa-circle-info" }
                        "Geo rules use the geodata downloaded in Core Management. Download Xray geodata there before enabling them."
                    }
                },
                XraySettingsSection::Dns => rsx! {
                    div { class: "setting-divider", "Local DNS" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "Local DNS server" }
                            span { "Resolves directly routed (China/private) domains; empty uses the system resolver" }
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

                    div { class: "setting-divider", "Fake DNS" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "Enable Fake DNS" }
                            span { "Answer with synthetic IPs and recover the domain through sniffing" }
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
                                "Fake DNS is only used while TUN mode is enabled in the TUN tab."
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "Fake IP pool" }
                                span { "IPv4 CIDR used for synthetic answers" }
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
                                label { "Pool size" }
                                span { "Maximum number of cached fake mappings" }
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
                                    option { value: "{size}", "{size}" }
                                }
                            }
                        }
                        p { class: "settings-hint",
                            i { class: "fa-solid fa-circle-info" }
                            "When IPv6 is enabled a 2001:2::/48 pool is added automatically."
                        }
                    }
                },
                XraySettingsSection::Tun => rsx! {
                    div { class: "setting-divider", "TUN Interface" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "Enable TUN mode" }
                            span { "Route all system traffic through Xray without app-level proxy settings" }
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
                        "TUN needs network administrator privileges (CAP_NET_ADMIN). Without them the core cannot start; Cloak DX then restores the previous settings automatically."
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
                                        "Waiting for administrator authorization...".to_string(),
                                    );
                                    let database = Arc::clone(&grant_database);
                                    let proxy_core = Arc::clone(&grant_core);
                                    spawn(async move {
                                        match proxy_core.grant_tun_privileges(&database).await {
                                            Ok(()) => status.set(
                                                "TUN permissions granted. Enable TUN mode if needed, then start the core."
                                                    .to_string(),
                                            ),
                                            Err(error) => status.set(error),
                                        }
                                        busy.set(false);
                                    });
                                },
                                i { class: "fa-solid fa-shield-halved" }
                                "Grant TUN permission"
                            }
                        }
                        p { class: "settings-hint",
                            i { class: "fa-solid fa-circle-info" }
                            "The permission is stored on the selected Xray binary; grant it again after the core is updated."
                        }
                    }

                    div { class: "setting-divider", "IPv6" }
                    div { class: "setting-item",
                        div { class: "setting-info",
                            label { "Enable IPv6" }
                            span { "Allow AAAA lookups and add IPv6 TUN addresses and routes" }
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
                        div { class: "setting-divider", "Interface" }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "Interface name" }
                                span { "Empty lets Xray pick an available name" }
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
                                span { "Maximum packet size on the interface" }
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
                                    option { value: "{mtu}", "{mtu}" }
                                }
                            }
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "Stack" }
                                span { "Network stack implementation" }
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
                                    option { value: "{stack}", "{stack}" }
                                }
                            }
                        }
                        p { class: "settings-hint",
                            i { class: "fa-solid fa-circle-info" }
                            "Xray's TUN always uses its built-in stack; this preference applies to cores that support choosing one."
                        }
                        div { class: "setting-item",
                            div { class: "setting-info",
                                label { "IPv4 gateway" }
                                span { "Address prefix assigned to the TUN interface" }
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
                                label { "Auto route" }
                                span { "Add default routes and bind Xray outbounds to the physical interface" }
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
                                label { "Strict route" }
                                span { "Use split default routes so existing routes cannot bypass the TUN" }
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
                                "Strict route only takes effect while Auto route is enabled."
                            }
                        }
                        p { class: "settings-hint",
                            i { class: "fa-solid fa-circle-info" }
                            "IPv6 gateway fc00::1/64 and its routes are added automatically while IPv6 is enabled above."
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
