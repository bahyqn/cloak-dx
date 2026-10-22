use dioxus::prelude::*;

#[component]
pub(super) fn NetworkActivity() -> Element {
    rsx! {
        section { class: "card traffic-card",
            h3 { "Network Activity" }
            div { class: "traffic-stats-large",
                div { class: "traffic-item up",
                    i { class: "fa-solid fa-arrow-up" }
                    div {
                        span { class: "label", "Upload" }
                        span { class: "value", id: "upload-speed", "0 KB/s" }
                    }
                }
                div { class: "traffic-item down",
                    i { class: "fa-solid fa-arrow-down" }
                    div {
                        span { class: "label", "Download" }
                        span { class: "value", id: "download-speed", "0 KB/s" }
                    }
                }
            }
        }
    }
}
