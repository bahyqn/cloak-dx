mod appearance;
mod core_management;
mod sing_box_settings;
mod xray_settings;

use std::sync::Arc;

use dioxus::prelude::*;

use self::{
    appearance::AppearanceSettings,
    core_management::CoreManagement,
    sing_box_settings::{SingBoxSettingsPanel, SingBoxSettingsSection},
    xray_settings::{XraySettingsPanel, XraySettingsSection},
};
use crate::{i18n::I18n, services::database::Database};

/// Each engine owns its own tab variants. A section can only be selected while
/// that engine is the selected core, so adding sing-box pages cannot change how
/// the Xray pages render or which tab is active.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Xray(XraySettingsSection),
    SingBox(SingBoxSettingsSection),
    CoreManagement,
}

#[component]
pub fn SettingsPage() -> Element {
    let mut tab = use_signal(|| SettingsTab::General);
    let i18n = use_context::<I18n>();
    let database = use_context::<Arc<Database>>();
    let data_revision = use_context::<Signal<u64>>();
    let _revision = data_revision();
    let selected_engine = database.selected_engine().ok().flatten();
    // Keep the previous tab only while its engine is still the selected one.
    // Switching cores falls back to General instead of showing another
    // engine's options under the new core.
    let active_tab = match tab() {
        SettingsTab::Xray(_) if selected_engine.as_deref() != Some("xray") => SettingsTab::General,
        SettingsTab::SingBox(_) if selected_engine.as_deref() != Some("sing-box") => {
            SettingsTab::General
        }
        tab => tab,
    };
    let header_hint = match selected_engine.as_deref() {
        Some("xray") => i18n.t("Xray settings apply immediately to the running core"),
        Some("sing-box") => i18n.t("sing-box settings apply immediately to the running core"),
        _ => i18n.t("Select a core in Core Management to configure it"),
    };

    rsx! {
        div { id: "settings-view", class: "view-section active",
            div { class: "settings-header",
                h2 { class: "page-title", "{i18n.t(\"Settings\")}" }
                div { class: "settings-header-actions",
                    p { class: "settings-hint",
                        i { class: "fa-solid fa-circle-info" }
                        "{header_hint}"
                    }
                }
            }
            div { class: "settings-tabs-nav",
                button {
                    class: if active_tab == SettingsTab::General { "s-tab-btn active" } else { "s-tab-btn" },
                    onclick: move |_| tab.set(SettingsTab::General),
                    i { class: "fa-solid fa-sliders" }
                    "{i18n.t(\"General\")}"
                }
                if selected_engine.as_deref() == Some("xray") {
                    for section in XraySettingsSection::TABS.iter().copied() {
                        button {
                            class: if active_tab == SettingsTab::Xray(section) { "s-tab-btn active" } else { "s-tab-btn" },
                            onclick: move |_| tab.set(SettingsTab::Xray(section)),
                            i { class: section.icon() }
                            "{i18n.t(section.label())}"
                        }
                    }
                } else if selected_engine.as_deref() == Some("sing-box") {
                    for section in SingBoxSettingsSection::TABS.iter().copied() {
                        button {
                            class: if active_tab == SettingsTab::SingBox(section) { "s-tab-btn active" } else { "s-tab-btn" },
                            onclick: move |_| tab.set(SettingsTab::SingBox(section)),
                            i { class: section.icon() }
                            "{i18n.t(section.label())}"
                        }
                    }
                }
                button {
                    class: if active_tab == SettingsTab::CoreManagement { "s-tab-btn active" } else { "s-tab-btn" },
                    onclick: move |_| tab.set(SettingsTab::CoreManagement),
                    i { class: "fa-solid fa-microchip" }
                    "{i18n.t(\"Core Management\")}"
                }
            }

            match active_tab {
                SettingsTab::General => rsx! {
                    div { class: "s-tab-content active",
                        div { class: "settings-grid settings-grid-centered",
                            AppearanceSettings {}
                            match selected_engine.as_deref() {
                                Some("xray") => rsx! {
                                    XraySettingsPanel { section: XraySettingsSection::General }
                                },
                                Some("sing-box") => rsx! {
                                    SingBoxSettingsPanel { section: SingBoxSettingsSection::General }
                                },
                                _ => rsx! {},
                            }
                        }
                    }
                },
                SettingsTab::Xray(section) => rsx! {
                    div { class: "s-tab-content active",
                        div { class: "settings-grid settings-grid-centered",
                            XraySettingsPanel { section: section }
                        }
                    }
                },
                SettingsTab::SingBox(section) => rsx! {
                    div { class: "s-tab-content active",
                        div { class: "settings-grid settings-grid-centered",
                            SingBoxSettingsPanel { section: section }
                        }
                    }
                },
                SettingsTab::CoreManagement => rsx! {
                    div { class: "s-tab-content active",
                        div { class: "settings-grid core-management-grid", CoreManagement {} }
                    }
                },
            }
        }
    }
}
