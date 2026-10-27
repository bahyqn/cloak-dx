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
    let latencies = use_signal(HashMap::<String, String>::new);
    let downloads = use_signal(HashMap::<String, String>::new);
    let uploads = use_signal(HashMap::<String, String>::new);
    let exits = use_signal(HashMap::<String, GoogleExitResult>::new);
    let testing = use_signal(HashMap::<String, TestKind>::new);
    let batch_running = use_signal(|| None::<TestKind>);
    let sort_mode = use_signal(|| SortMode::Latency);
    let toast_message = use_signal(String::new);
    let toast_generation = use_signal(|| 0_u64);
    let app_log_entries = use_signal(Vec::<AppLogEntry>::new);
    // Network results are app-scoped: leaving the page must not throw away a
    // finished test, and a running one must keep going while it is closed.
    let network_google = use_signal(|| None::<Result<GoogleExitResult, String>>);
    let network_ippure = use_signal(|| None::<Result<IppureResult, String>>);
    let network_dns = use_signal(|| None::<Result<DnsLeakResult, String>>);
    let network_google_running = use_signal(|| false);
    let network_ippure_running = use_signal(|| false);
    let network_dns_running = use_signal(|| false);
    let network_provider = use_signal(DnsLeakProvider::default);
    use_context_provider(|| AppLog {
        entries: app_log_entries,
    });
    use_context_provider(|| Arc::clone(&database));
    use_context_provider(|| Arc::clone(&proxy_core));
    use_context_provider(|| data_revision);
    use_context_provider(|| ProxyTestUi {
        latencies,
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
