mod appearance;
mod core_management;
mod xray_settings;

use dioxus::prelude::*;

use self::{
    appearance::AppearanceSettings,
    core_management::CoreManagement,
    xray_settings::{XraySettingsPanel, XraySettingsSection},
};
use crate::i18n::I18n;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Routing,
    Dns,
    Tun,
    Advanced,
    CoreManagement,
}

#[component]
pub fn SettingsPage() -> Element {
    let mut tab = use_signal(|| SettingsTab::General);
    let i18n = use_context::<I18n>();

    rsx! {
        div { id: "settings-view", class: "view-section active",
            div { class: "settings-header",
                h2 { class: "page-title", "{i18n.t(\"Settings\")}" }
                div { class: "settings-header-actions",
                    p { class: "settings-hint",
                        i { class: "fa-solid fa-circle-info" }
                        "{i18n.t(\"Xray settings apply immediately to the running core\")}"
                    }
                }
            }
            div { class: "settings-tabs-nav",
                button {
                    class: if tab() == SettingsTab::General { "s-tab-btn active" } else { "s-tab-btn" },
                    onclick: move |_| tab.set(SettingsTab::General),
                    i { class: "fa-solid fa-sliders" }
                    "{i18n.t(\"General\")}"
                }
                button {
                    class: if tab() == SettingsTab::Routing { "s-tab-btn active" } else { "s-tab-btn" },
                    onclick: move |_| tab.set(SettingsTab::Routing),
                    i { class: "fa-solid fa-route" }
                    "{i18n.t(\"Routing\")}"
                }
                button {
                    class: if tab() == SettingsTab::Dns { "s-tab-btn active" } else { "s-tab-btn" },
                    onclick: move |_| tab.set(SettingsTab::Dns),
                    i { class: "fa-solid fa-server" }
                    "DNS"
                }
                button {
                    class: if tab() == SettingsTab::Tun { "s-tab-btn active" } else { "s-tab-btn" },
                    onclick: move |_| tab.set(SettingsTab::Tun),
                    i { class: "fa-solid fa-network-wired" }
                    "TUN"
                }
                button {
                    class: if tab() == SettingsTab::Advanced { "s-tab-btn active" } else { "s-tab-btn" },
                    onclick: move |_| tab.set(SettingsTab::Advanced),
                    i { class: "fa-solid fa-gears" }
                    "{i18n.t(\"Advanced\")}"
                }
                button {
                    class: if tab() == SettingsTab::CoreManagement { "s-tab-btn active" } else { "s-tab-btn" },
                    onclick: move |_| tab.set(SettingsTab::CoreManagement),
                    i { class: "fa-solid fa-microchip" }
                    "{i18n.t(\"Core Management\")}"
                }
            }

            if tab() == SettingsTab::General {
                div { class: "s-tab-content active",
                    div { class: "settings-grid settings-grid-centered",
                        AppearanceSettings {}
                        XraySettingsPanel { section: XraySettingsSection::General }
                    }
                }
            } else if tab() == SettingsTab::Routing {
                div { class: "s-tab-content active",
                    div { class: "settings-grid settings-grid-centered",
                        XraySettingsPanel { section: XraySettingsSection::Routing }
                    }
                }
            } else if tab() == SettingsTab::Dns {
                div { class: "s-tab-content active",
                    div { class: "settings-grid settings-grid-centered",
                        XraySettingsPanel { section: XraySettingsSection::Dns }
                    }
                }
            } else if tab() == SettingsTab::Tun {
                div { class: "s-tab-content active",
                    div { class: "settings-grid settings-grid-centered",
                        XraySettingsPanel { section: XraySettingsSection::Tun }
                    }
                }
            } else if tab() == SettingsTab::Advanced {
                div { class: "s-tab-content active",
                    div { class: "settings-grid settings-grid-centered",
                        XraySettingsPanel { section: XraySettingsSection::Advanced }
                    }
                }
            } else {
                div { class: "s-tab-content active",
                    div { class: "settings-grid core-management-grid", CoreManagement {} }
                }
            }
        }
    }
}
