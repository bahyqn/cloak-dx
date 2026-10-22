mod connection_card;
mod connectivity_tests;
mod network_activity;
mod proxy_modes;

use crate::services::subscription_manager::SubscriptionNode;
use dioxus::prelude::*;

use self::{
    connection_card::ConnectionCard, connectivity_tests::ConnectivityTests,
    network_activity::NetworkActivity, proxy_modes::ProxyModes,
};

#[component]
pub fn Dashboard(selected_node: Option<SubscriptionNode>) -> Element {
    rsx! {
        div { id: "dashboard-view", class: "view-section active",
            h2 { class: "page-title", "Dashboard" }
            div { class: "dashboard-grid",
                div { class: "dashboard-main-column",
                    ConnectionCard { selected_node }
                    NetworkActivity {}
                    ProxyModes {}
                }
                ConnectivityTests {}
            }
        }
    }
}
