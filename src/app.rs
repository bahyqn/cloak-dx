use dioxus::prelude::*;
use std::sync::Arc;

use crate::{
    components::{
        dashboard::Dashboard, layout::Sidebar, proxies::ProxiesPage, settings::SettingsPage,
        subscriptions::SubscriptionsPage,
    },
    services::{
        database::Database,
        subscription_manager::{parse_subscription_nodes, subscription_node_key},
    },
    theme::ThemeStyles,
};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum AppPage {
    Dashboard,
    Proxies,
    Subscriptions,
    Settings,
}

#[component]
pub fn App() -> Element {
    let current_page = use_signal(|| AppPage::Dashboard);
    let database =
        use_hook(|| Arc::new(Database::open().expect("failed to open cloak-dx database")));
    let data_revision = use_signal(|| 0_u64);
    use_context_provider(|| Arc::clone(&database));
    use_context_provider(|| data_revision);
    let _revision = data_revision();
    let selected_node = database
        .selected_subscription()
        .ok()
        .flatten()
        .and_then(|subscription| {
            let nodes = parse_subscription_nodes(&subscription.raw_config);
            nodes
                .iter()
                .find(|node| {
                    subscription
                        .selected_node_key
                        .as_deref()
                        .is_some_and(|key| key == subscription_node_key(node))
                })
                .cloned()
                .or_else(|| nodes.first().cloned())
        });

    rsx! {
        ThemeStyles {}
        div { class: "app-container",
            Sidebar { current_page }
            main { class: if current_page() == AppPage::Proxies { "main-content proxies-main-content" } else { "main-content" },
                if current_page() == AppPage::Dashboard {
                    Dashboard { selected_node: selected_node.clone() }
                } else if current_page() == AppPage::Proxies {
                    ProxiesPage {}
                } else if current_page() == AppPage::Subscriptions {
                    SubscriptionsPage {}
                } else {
                    SettingsPage {}
                }
            }
        }
    }
}
