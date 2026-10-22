mod core_management;

use dioxus::prelude::*;

use self::core_management::CoreManagement;

#[component]
pub fn SettingsPage() -> Element {
    rsx! {
        div { id: "settings-view", class: "view-section active",
            div { class: "settings-header",
                h2 { class: "page-title", "Settings" }
                div { class: "settings-header-actions",
                    p { class: "settings-hint",
                        i { class: "fa-solid fa-circle-info" }
                        "Core changes take effect after restart"
                    }
                }
            }
            div { class: "settings-grid core-management-grid", CoreManagement {} }
        }
    }
}
