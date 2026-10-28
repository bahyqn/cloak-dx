use std::sync::Arc;

use dioxus::prelude::*;

use crate::{
    i18n::I18n,
    services::{database::Database, proxy_core::ProxyCoreService, xray_settings::XraySettings},
};

#[component]
pub(super) fn ProxyModes() -> Element {
    let i18n = use_context::<I18n>();
    let proxy_core = use_context::<Arc<ProxyCoreService>>();
    let database = use_context::<Arc<Database>>();
    let mut system_proxy = use_signal(|| proxy_core.system_proxy_enabled());
    let mut system_proxy_error = use_signal(String::new);
    let mut xray = use_signal(|| XraySettings::load(&database));
    let mut busy = use_signal(|| false);
    let mut settings_error = use_signal(String::new);

    // The TUN options are the same persisted settings as the TUN tab under
    // Settings. Changes there and here go through one apply path, so the
    // running core is restarted consistently.
    let apply = {
        let database = Arc::clone(&database);
        let proxy_core = Arc::clone(&proxy_core);
        EventHandler::new(move |next: XraySettings| {
            if busy() {
                return;
            }
            let previous = xray();
            if previous == next {
                return;
            }
            busy.set(true);
            settings_error.set(String::new());
            let database = Arc::clone(&database);
            let proxy_core = Arc::clone(&proxy_core);
            spawn(async move {
                match proxy_core.apply_settings(&database, &previous, &next) {
                    Ok(_) => xray.set(next),
                    Err(message) => {
                        // `apply_settings` restored the previous values on disk
                        // when the restarted core refused the new config.
                        xray.set(previous);
                        settings_error.set(message);
                    }
                }
                busy.set(false);
            });
        })
    };

    let toggle_core = Arc::clone(&proxy_core);
    let current = xray();

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
                            checked: current.tun_enabled,
                            disabled: busy(),
                            onchange: move |event| {
                                let mut next = xray();
                                next.tun_enabled = event.checked();
                                apply.call(next);
                            },
                        }
                        span { class: "slider" }
                    }
                }
                div { class: "mode-sub-items", id: "tun-extras",
                    div { class: "sub-item-row",
                        span { "{i18n.t(\"Strict Route\")}" }
                        label { class: "toggle-switch extra-small",
                            input {
                                r#type: "checkbox",
                                id: "dash-strict-route",
                                checked: current.tun_strict_route,
                                disabled: busy() || !current.tun_auto_route,
                                onchange: move |event| {
                                    let mut next = xray();
                                    next.tun_strict_route = event.checked();
                                    apply.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
                    }
                    div { class: "sub-item-row",
                        span { "{i18n.t(\"Auto Route\")}" }
                        label { class: "toggle-switch extra-small",
                            input {
                                r#type: "checkbox",
                                id: "dash-auto-route",
                                checked: current.tun_auto_route,
                                disabled: busy(),
                                onchange: move |event| {
                                    let mut next = xray();
                                    next.tun_auto_route = event.checked();
                                    apply.call(next);
                                },
                            }
                            span { class: "slider" }
                        }
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
                div { class: "mode-divider" }
                div { class: "mode-selector",
                    button { class: "mode-btn active",
                        i { class: "fa-solid fa-filter" }
                        "{i18n.t(\"Rule\")}"
                    }
                    button { class: "mode-btn",
                        i { class: "fa-solid fa-globe" }
                        "{i18n.t(\"Global\")}"
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
