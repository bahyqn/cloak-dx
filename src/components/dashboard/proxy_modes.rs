use dioxus::prelude::*;

#[component]
pub(super) fn ProxyModes() -> Element {
    rsx! {
        section { class: "card modes-card",
            h3 { "Proxy Modes" }
            div { class: "modes-grid",
                div { class: "mode-item", id: "mode-tun",
                    div { class: "mode-icon", i { class: "fa-solid fa-network-wired" } }
                    div { class: "mode-info",
                        span { class: "mode-name", "TUN Mode" }
                        span { class: "mode-desc", "Virtual Network Interface" }
                    }
                    label { class: "toggle-switch small",
                        input { r#type: "checkbox", id: "tun-toggle" }
                        span { class: "slider" }
                    }
                }
                div { class: "mode-sub-items", id: "tun-extras",
                    div { class: "sub-item-row",
                        span { "Strict Route" }
                        label { class: "toggle-switch extra-small",
                            input { r#type: "checkbox", id: "dash-strict-route", checked: true }
                            span { class: "slider" }
                        }
                    }
                    div { class: "sub-item-row",
                        span { "Auto Route" }
                        label { class: "toggle-switch extra-small",
                            input { r#type: "checkbox", id: "dash-auto-route", checked: true }
                            span { class: "slider" }
                        }
                    }
                }
                div { class: "mode-item", id: "mode-system",
                    div { class: "mode-icon", i { class: "fa-solid fa-desktop" } }
                    div { class: "mode-info",
                        span { class: "mode-name", "System Proxy" }
                        span { class: "mode-desc", "Set System Settings" }
                    }
                    label { class: "toggle-switch small",
                        input { r#type: "checkbox", id: "system-proxy-toggle" }
                        span { class: "slider" }
                    }
                }
                div { class: "mode-divider" }
                div { class: "mode-selector",
                    button { class: "mode-btn active",
                        i { class: "fa-solid fa-filter" }
                        "Rule"
                    }
                    button { class: "mode-btn",
                        i { class: "fa-solid fa-globe" }
                        "Global"
                    }
                }
            }
        }
    }
}
