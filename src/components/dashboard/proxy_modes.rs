use std::sync::Arc;

use dioxus::prelude::*;

use crate::{
    i18n::I18n,
    services::{
        database::Database,
        proxy_core::{local_lan_ipv4, ProxyCoreService},
        sing_box_settings::{self, SingBoxSettings},
        xray_settings::{XraySettings, DEFAULT_LAN_HTTP_PORT, DEFAULT_LAN_SOCKS_PORT},
    },
};

/// The two engine-level switches this card owns. Both engines store TUN and
/// LAN settings separately, so the card applies the change to the engine that
/// is currently selected and leaves the other engine's settings untouched.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ModeToggle {
    Tun(bool),
    Lan(bool),
}

#[component]
pub(super) fn ProxyModes() -> Element {
    let i18n = use_context::<I18n>();
    let proxy_core = use_context::<Arc<ProxyCoreService>>();
    let database = use_context::<Arc<Database>>();
    let mut system_proxy = use_signal(|| proxy_core.system_proxy_enabled());
    let mut system_proxy_error = use_signal(String::new);
    let mut xray = use_signal(|| XraySettings::load(&database));
    let mut sing_box = use_signal(|| SingBoxSettings::load(&database));
    let mut busy = use_signal(|| false);
    let mut settings_error = use_signal(String::new);

    // The LAN address should show the ports the core actually bound, not the
    // configured ones, so follow the core like the connection card does.
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

    let engine = proxy_core.active_engine(&database);
    let sing_box_engine = engine == "sing-box";

    // TUN and LAN exist in both engines. The card reads and writes whichever
    // engine is selected, so a toggle can never edit the other engine's keys.
    let apply_engine = engine.clone();
    let apply = {
        let database = Arc::clone(&database);
        let proxy_core = Arc::clone(&proxy_core);
        EventHandler::new(move |toggle: ModeToggle| {
            if busy() {
                return;
            }
            settings_error.set(String::new());

            if apply_engine == "sing-box" {
                let previous = sing_box();
                let mut next = previous.clone();
                match toggle {
                    ModeToggle::Tun(enabled) => next.tun_enabled = enabled,
                    ModeToggle::Lan(enabled) => next.lan_enabled = enabled,
                }
                if previous == next {
                    return;
                }
                busy.set(true);
                let database = Arc::clone(&database);
                let proxy_core = Arc::clone(&proxy_core);
                spawn(async move {
                    match sing_box_settings::apply_settings(
                        &proxy_core,
                        &database,
                        &previous,
                        &next,
                    ) {
                        Ok(_) => sing_box.set(next),
                        Err(message) => {
                            // The previous values were written back when the
                            // restart failed, so the switch follows the disk.
                            sing_box.set(previous);
                            settings_error.set(message);
                        }
                    }
                    busy.set(false);
                });
            } else {
                let previous = xray();
                let mut next = previous.clone();
                match toggle {
                    ModeToggle::Tun(enabled) => next.tun_enabled = enabled,
                    ModeToggle::Lan(enabled) => next.lan_enabled = enabled,
                }
                if previous == next {
                    return;
                }
                busy.set(true);
                let database = Arc::clone(&database);
                let proxy_core = Arc::clone(&proxy_core);
                spawn(async move {
                    match proxy_core.apply_settings(&database, &previous, &next) {
                        Ok(_) => xray.set(next),
                        Err(message) => {
                            xray.set(previous);
                            settings_error.set(message);
                        }
                    }
                    busy.set(false);
                });
            }
        })
    };

    let toggle_core = Arc::clone(&proxy_core);
    let status = core_status();
    let xray_settings = xray();
    let sing_box_settings = sing_box();
    let tun_enabled = if sing_box_engine {
        sing_box_settings.tun_enabled
    } else {
        xray_settings.tun_enabled
    };
    let lan_enabled = if sing_box_engine {
        sing_box_settings.lan_enabled
    } else {
        xray_settings.lan_enabled
    };

    // Other devices need this machine's network address, not loopback.
    let lan_address = local_lan_ipv4();
    let lan_host = lan_address
        .map(|address| address.to_string())
        .unwrap_or_else(|| i18n.t("LAN IP").to_string());
    let sing_box_running = status.running && status.engine.as_deref() == Some("sing-box");
    let xray_running = status.running && status.engine.as_deref() == Some("xray");
    let sing_box_lan_port = if sing_box_running {
        status.lan_socks_port.unwrap_or(sing_box_settings.lan_port)
    } else {
        sing_box_settings.lan_port
    };
    // Xray does not let the user change its LAN ports; the running core may
    // still pick a free one when the defaults are busy.
    let xray_lan_socks_port = if xray_running {
        status.lan_socks_port.unwrap_or(DEFAULT_LAN_SOCKS_PORT)
    } else {
        DEFAULT_LAN_SOCKS_PORT
    };
    let xray_lan_http_port = if xray_running {
        status.lan_http_port.unwrap_or(DEFAULT_LAN_HTTP_PORT)
    } else {
        DEFAULT_LAN_HTTP_PORT
    };

    rsx! {
        section { class: "card modes-card",
            h3 { "{i18n.t(\"Proxy Modes\")}" }
            div { class: "modes-grid",
                div { class: "mode-item", id: "mode-tun",
                    div { class: "mode-icon", i { class: "fa-solid fa-network-wired" } }
                    div { class: "mode-info",
                        span { class: "mode-name", "{i18n.t(\"TUN Mode\")}" }
                        span { class: "mode-desc", "{i18n.t(\"Virtual Network Interface\")}" }
                    }
                    label { class: "toggle-switch small",
                        input {
                            r#type: "checkbox",
                            id: "tun-toggle",
                            checked: tun_enabled,
                            disabled: busy(),
                            onchange: move |event| apply.call(ModeToggle::Tun(event.checked())),
                        }
                        span { class: "slider" }
                    }
                }
                div { class: "mode-item", id: "mode-system",
                    div { class: "mode-icon", i { class: "fa-solid fa-desktop" } }
                    div { class: "mode-info",
                        span { class: "mode-name", "{i18n.t(\"System Proxy\")}" }
                        span { class: "mode-desc", "{i18n.t(\"Set System Settings\")}" }
                    }
                    label { class: "toggle-switch small",
                        input {
                            r#type: "checkbox",
                            id: "system-proxy-toggle",
                            checked: system_proxy(),
                            onchange: move |event| {
                                let enabled = event.checked();
                                system_proxy_error.set(String::new());
                                match toggle_core.set_system_proxy_enabled(enabled) {
                                    Ok(()) => system_proxy.set(enabled),
                                    Err(message) => {
                                        system_proxy.set(toggle_core.system_proxy_enabled());
                                        system_proxy_error.set(message);
                                    }
                                }
                            },
                        }
                        span { class: "slider" }
                    }
                }
                div { class: "mode-item", id: "mode-lan",
                    div { class: "mode-icon", i { class: "fa-solid fa-tower-broadcast" } }
                    div { class: "mode-info",
                        span { class: "mode-name", "{i18n.t(\"Allow LAN connections\")}" }
                        span { class: "mode-desc",
                            "{i18n.t(\"Let other devices on this network use this machine as a proxy\")}"
                        }
                    }
                    label { class: "toggle-switch small",
                        input {
                            r#type: "checkbox",
                            id: "lan-toggle",
                            checked: lan_enabled,
                            disabled: busy(),
                            onchange: move |event| apply.call(ModeToggle::Lan(event.checked())),
                        }
                        span { class: "slider" }
                    }
                }
                // The address row keeps its space while LAN is off, so turning
                // the switch on reveals the ports without moving anything on
                // the page.
                div {
                    class: if lan_enabled { "mode-lan-address" } else { "mode-lan-address is-placeholder" },
                    span { class: "mode-lan-label", "{i18n.t(\"LAN address\")}" }
                    div { class: "lan-port-values",
                        if sing_box_engine {
                            span { class: "lan-port-value",
                                span { class: "lan-port-label", "MIXED" }
                                code { "{lan_host}:{sing_box_lan_port}" }
                            }
                        } else {
                            span { class: "lan-port-value",
                                span { class: "lan-port-label", "SOCKS5" }
                                code { "{lan_host}:{xray_lan_socks_port}" }
                            }
                            span { class: "lan-port-value",
                                span { class: "lan-port-label", "HTTP" }
                                code { "{lan_host}:{xray_lan_http_port}" }
                            }
                        }
                    }
                }
            }
            if !system_proxy_error().is_empty() {
                p { class: "subscription-error", role: "alert", "{system_proxy_error}" }
            }
            if !settings_error().is_empty() {
                p { class: "subscription-error", role: "alert", "{settings_error}" }
            }
        }
    }
}
