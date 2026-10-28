use dioxus::prelude::*;
use std::{collections::HashMap, sync::Arc};

use crate::{
    components::{
        dashboard::Dashboard,
        layout::Sidebar,
        logs::LogsPage,
        network::{NetworkPage, NetworkTestUi},
        proxies::{ProxiesPage, ProxyTestUi, SortMode, TestKind},
        settings::SettingsPage,
        subscriptions::SubscriptionsPage,
    },
    services::{
        app_log::{AppLog, AppLogEntry},
        database::Database,
        network_test::{DnsLeakProvider, DnsLeakResult, IppureResult},
        proxy_core::ProxyCoreService,
        proxy_test::GoogleExitResult,
        service_access::{ServiceCheck, ServiceId},
        subscription_manager::{parse_subscription_nodes, subscription_node_key},
    },
    theme::ThemeStyles,
};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum AppPage {
    Dashboard,
    Proxies,
    Subscriptions,
    Network,
    Logs,
    Settings,
}

#[component]
pub fn App() -> Element {
    let current_page = use_signal(|| AppPage::Dashboard);
    let database =
        use_hook(|| Arc::new(Database::open().expect("failed to open cloak-dx database")));
    let proxy_core = use_hook(|| Arc::new(ProxyCoreService::new()));
    let data_revision = use_signal(|| 0_u64);
    let mut latencies = use_signal(HashMap::<String, String>::new);
    let mut packet_losses = use_signal(HashMap::<String, String>::new);
    let downloads = use_signal(HashMap::<String, String>::new);
    let uploads = use_signal(HashMap::<String, String>::new);
    let mut exits = use_signal(HashMap::<String, GoogleExitResult>::new);
    let testing = use_signal(HashMap::<String, TestKind>::new);
    let batch_running = use_signal(|| None::<TestKind>);
    let sort_mode = use_signal(|| SortMode::Latency);
    let toast_message = use_signal(String::new);
    let toast_generation = use_signal(|| 0_u64);
    let app_log_entries = use_signal(Vec::<AppLogEntry>::new);
    // Network results are app-scoped: leaving the page must not throw away a
    // finished test, and a running one must keep going while it is closed.
    let mut network_google = use_signal(|| None::<Result<GoogleExitResult, String>>);
    let mut network_ippure = use_signal(|| None::<Result<IppureResult, String>>);
    let mut network_dns = use_signal(|| None::<Result<DnsLeakResult, String>>);
    let mut network_google_running = use_signal(|| false);
    let mut network_ippure_running = use_signal(|| false);
    let mut network_dns_running = use_signal(|| false);
    let network_provider = use_signal(DnsLeakProvider::default);
    // Service Access is shared by the Network card (where the three services
    // are picked) and the dashboard rows, so both views test the same list.
    let service_selection = database
        .get_setting("service_access")
        .ok()
        .flatten()
        .map(|value| ServiceId::parse_selection(&value))
        .filter(|selection| !selection.is_empty())
        .unwrap_or_else(ServiceId::default_selection);
    let services = use_signal(|| service_selection);
    let mut service_results = use_signal(HashMap::<ServiceId, Result<ServiceCheck, String>>::new);
    let mut services_running = use_signal(|| false);
    let mut google_ping = use_signal(|| None::<Result<u64, String>>);

    let _revision = data_revision();
    let active_subscription = database.selected_subscription().ok().flatten();
    let selected_node = active_subscription.as_ref().and_then(|subscription| {
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

    // Network results describe one node. The owner is the subscription plus
    // the selected node's key, so changing either drops every stored result
    // instead of showing the previous node's measurements.
    let network_owner_key = active_subscription
        .as_ref()
        .map(|subscription| {
            let node_key = selected_node
                .as_ref()
                .map(subscription_node_key)
                .unwrap_or_default();
            format!("{}:{node_key}", subscription.id)
        })
        .unwrap_or_default();
    let mut network_owner = use_signal(|| network_owner_key.clone());
    // Captured separately so the reset effect does not move `selected_node`,
    // which is still passed to the dashboard below.
    let selected_reset_key = selected_node.as_ref().map(subscription_node_key);

    use_context_provider(|| AppLog {
        entries: app_log_entries,
    });
    use_context_provider(|| Arc::clone(&database));
    use_context_provider(|| Arc::clone(&proxy_core));
    use_context_provider(|| data_revision);
    use_context_provider(|| ProxyTestUi {
        latencies,
        packet_losses,
        downloads,
        uploads,
        exits,
        testing,
        batch_running,
        sort_mode,
        toast_message,
        toast_generation,
    });
    use_context_provider(|| NetworkTestUi {
        google: network_google,
        ippure: network_ippure,
        dns: network_dns,
        google_running: network_google_running,
        ippure_running: network_ippure_running,
        dns_running: network_dns_running,
        provider: network_provider,
        owner: network_owner,
        services,
        service_results,
        services_running,
        google_ping,
    });

    // `data_revision` changes whenever the selected subscription or node
    // changes, which is exactly when the network results stop describing the
    // active connection.
    use_effect(move || {
        let _ = data_revision();
        let next = network_owner_key.clone();
        if network_owner() != next {
            network_owner.set(next);
            network_google.set(None);
            network_ippure.set(None);
            network_dns.set(None);
            network_google_running.set(false);
            network_ippure_running.set(false);
            network_dns_running.set(false);
            service_results.set(HashMap::new());
            services_running.set(false);
            google_ping.set(None);
            // The connectivity card must not keep numbers measured while the
            // previous node was active. Live values are dropped for the node
            // that just became selected; the Proxies page still reads the
            // persisted history from the database.
            if let Some(node_key) = selected_reset_key.clone() {
                latencies.with_mut(|values| {
                    values.remove(&node_key);
                });
                packet_losses.with_mut(|values| {
                    values.remove(&node_key);
                });
                exits.with_mut(|values| {
                    values.remove(&node_key);
                });
            }
        }
    });

    // Remember which services the user watches, so the dashboard rows and the
    // Network card start with the same list after a restart.
    let settings_database = Arc::clone(&database);
    use_effect(move || {
        let selection = services();
        let _ = settings_database.set_setting("service_access", &ServiceId::serialize(&selection));
    });

    // The desktop event loop exits the process without unwinding, so stop the
    // core (and restore the system proxy) while the window is still closing.
    #[cfg(feature = "desktop")]
    {
        let proxy_core = Arc::clone(&proxy_core);
        dioxus::desktop::use_wry_event_handler(move |event, _| {
            if matches!(
                event,
                dioxus::desktop::tao::event::Event::WindowEvent {
                    event: dioxus::desktop::WindowEvent::CloseRequested,
                    ..
                }
            ) {
                proxy_core.stop();
            }
        });
    }

    rsx! {
        ThemeStyles {}
        div { class: "app-container",
            Sidebar { current_page }
            main { class: if current_page() == AppPage::Proxies { "main-content proxies-main-content" } else { "main-content" },
                if current_page() == AppPage::Dashboard {
                    Dashboard {
                        selected_node: selected_node.clone(),
                        subscription: active_subscription.clone(),
                    }
                } else if current_page() == AppPage::Proxies {
                    ProxiesPage {}
                } else if current_page() == AppPage::Subscriptions {
                    SubscriptionsPage {}
                } else if current_page() == AppPage::Network {
                    NetworkPage {}
                } else if current_page() == AppPage::Logs {
                    LogsPage {}
                } else {
                    SettingsPage {}
                }
            }
        }
    }
}
