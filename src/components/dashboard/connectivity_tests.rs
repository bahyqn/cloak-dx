use dioxus::prelude::*;

#[component]
pub(super) fn ConnectivityTests() -> Element {
    rsx! {
        section { class: "card test-card",
            div { class: "card-header", h3 { "Connectivity Test" } }
            div { class: "test-controls",
                button { id: "dash-test-all-btn", class: "btn-primary",
                    i { class: "fa-solid fa-check-double" }
                    "Test All"
                }
                button { id: "dash-test-lat-btn", class: "btn-secondary small",
                    i { class: "fa-solid fa-bolt" }
                    "Latency"
                }
            }
            div { class: "test-results", id: "test-results",
                div { class: "result-item",
                    span { class: "label", "Latency" }
                    span { class: "value", id: "ping-result", "-- ms" }
                }
                div { class: "result-item",
                    span { class: "label", "Packet Loss" }
                    span { class: "value", id: "packet-loss", "-- %" }
                }
                div { class: "result-item",
                    span { class: "label", "Google Ping" }
                    span { class: "value", id: "google-ping", "-- ms" }
                }
            }

            div { class: "dns-header",
                div { class: "test-header-small", style: "margin: 0", "DNS Analysis" }
                button { id: "dash-test-dns-btn", class: "btn-secondary small",
                    i { class: "fa-solid fa-play" }
                    "Run"
                }
            }
            div { class: "test-results", id: "dns-results",
                div { class: "result-item",
                    span { class: "label", "DNS Leak" }
                    span { class: "value", id: "dns-leak", "--" }
                }
                div { class: "result-item",
                    span { class: "label", "DNS Pollution" }
                    span { class: "value", id: "dns-pollution", "--" }
                }
            }

            div { class: "dns-header",
                div { class: "test-header-small", style: "margin: 0", "Service Access" }
                button { id: "dash-test-service-btn", class: "btn-secondary small",
                    i { class: "fa-solid fa-play" }
                    "Run"
                }
            }
            div { class: "test-results", id: "service-results",
                div { class: "result-item",
                    span { class: "label", "OpenAI (ChatGPT)" }
                    span { class: "value", id: "service-openai", "--" }
                }
                div { class: "result-item",
                    span { class: "label", "Netflix" }
                    span { class: "value", id: "service-netflix", "--" }
                }
                div { class: "result-item",
                    span { class: "label", "Disney+" }
                    span { class: "value", id: "service-disney", "--" }
                }
            }
        }
    }
}
