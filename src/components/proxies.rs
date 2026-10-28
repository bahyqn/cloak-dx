use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        atomic::{AtomicUsize, Ordering as AtomicOrdering},
        mpsc::{self, TryRecvError},
        Arc, Mutex,
    },
    time::Duration,
};

use dioxus::{dioxus_core::spawn_forever, prelude::*};

use crate::services::{
    app_log::{AppLog, LogLevel},
    country::country_flag,
    database::{Database, NodeTestResult, Subscription},
    proxy_core::ProxyCoreService,
    proxy_test::{self, GoogleExitResult, SpeedTestResult},
    subscription_manager::{
        parse_subscription_nodes, subscription_node_country_code, subscription_node_key,
        SubscriptionNode,
    },
};

const TEST_CONCURRENCY: usize = 30;
const LATENCY_FAILED: &str = "Failed";
const SPEED_FAILED: &str = "Failed";
const HIDE_TIMEOUT_SETTING: &str = "hide_timeout";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TestKind {
    Latency,
    Speed,
    /// Only the Google exit probe, triggered from a node card.
    Exit,
}

/// How the node grid is ordered.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Latency,
    Region,
    Download,
}

impl SortMode {
    fn value(self) -> &'static str {
        match self {
            SortMode::Latency => "latency",
            SortMode::Region => "region",
            SortMode::Download => "download",
        }
    }

    fn from_value(value: &str) -> Self {
        match value {
            "region" => SortMode::Region,
            "download" => SortMode::Download,
            _ => SortMode::Latency,
        }
    }
}

enum TestMessage {
    NodeStarted {
        key: String,
        kind: TestKind,
    },
    LatencyResult {
        key: String,
        result: Result<proxy_test::LatencyTestResult, String>,
    },
    SpeedResult {
        key: String,
        result: Result<SpeedTestResult, String>,
    },
    ExitResult {
        key: String,
        result: Result<GoogleExitResult, String>,
    },
    Finished,
}

/// Test results live at the app level so they survive page and node changes.
#[derive(Clone, Copy)]
pub struct ProxyTestUi {
    pub latencies: Signal<HashMap<String, String>>,
    /// Packet loss measured by the same probes that produce `latencies`.
    pub packet_losses: Signal<HashMap<String, String>>,
    pub downloads: Signal<HashMap<String, String>>,
    pub uploads: Signal<HashMap<String, String>>,
    pub exits: Signal<HashMap<String, GoogleExitResult>>,
    pub testing: Signal<HashMap<String, TestKind>>,
    pub batch_running: Signal<Option<TestKind>>,
    pub sort_mode: Signal<SortMode>,
    pub toast_message: Signal<String>,
    pub toast_generation: Signal<u64>,
}

struct NodeCardData {
    index: usize,
    node: SubscriptionNode,
    selected: bool,
    latency: String,
    download_speed: String,
    upload_speed: String,
    country_code: Option<String>,
    google_exit: Option<GoogleExitResult>,
    failed: bool,
    testing_latency: bool,
    testing_speed: bool,
    testing_exit: bool,
}

#[component]
pub fn ProxiesPage() -> Element {
    let database = use_context::<Arc<Database>>();
    let proxy_core = use_context::<Arc<ProxyCoreService>>();
    let data_revision = use_context::<Signal<u64>>();
    let _revision = data_revision();
    let selected = database.selected_subscription().ok().flatten();
    let nodes = selected
        .as_ref()
        .map(|subscription| parse_subscription_nodes(&subscription.raw_config))
        .unwrap_or_default();
    let mut hide_timeout = {
        let database = Arc::clone(&database);
        use_signal(move || {
            database
                .get_setting(HIDE_TIMEOUT_SETTING)
                .ok()
                .flatten()
                .is_some_and(|value| value == "1")
        })
    };
    let test_ui = use_context::<ProxyTestUi>();
    let app_log = use_context::<AppLog>();
    let mut batch_running = test_ui.batch_running;
    let mut sort_mode = test_ui.sort_mode;
    let toast_message = test_ui.toast_message;
    let toast_generation = test_ui.toast_generation;

    let selected_node_key = selected
        .as_ref()
        .and_then(|subscription| subscription.selected_node_key.as_deref())
        .map(str::to_string)
        .or_else(|| nodes.first().map(subscription_node_key));
    let subscription_id = selected.as_ref().map(|subscription| subscription.id);
    let node_keys = nodes.iter().map(subscription_node_key).collect::<Vec<_>>();
    let node_names = Arc::new(
        nodes
            .iter()
            .map(|node| (subscription_node_key(node), node.name.clone()))
            .collect::<HashMap<_, _>>(),
    );

    // Persisted results are only read when the page mounts or the selected
    // subscription changes; live results come from the app-level signals.
    let persisted_results = {
        let database = Arc::clone(&database);
        use_memo(move || {
            let _ = data_revision();
            subscription_id
                .and_then(|subscription_id| database.node_test_results(subscription_id).ok())
                .unwrap_or_default()
        })
    };
    let persisted_results = persisted_results();
    let persisted_latencies = persisted_results
        .iter()
        .filter_map(|result| {
            result
                .latency_ms
                .map(|latency| (result.node_key.clone(), latency))
        })
        .collect::<HashMap<_, _>>();
    let persisted_failures = persisted_results
        .iter()
        // Rows written before failures were flagged still carry a NULL
        // latency; a row only ever exists after a test, so that is a failure.
        .filter(|result| result.latency_failed || result.latency_ms.is_none())
        .map(|result| result.node_key.clone())
        .collect::<HashSet<_>>();
    let persisted_exits = persisted_results
        .iter()
        .filter_map(|result| {
            exit_from_test_result(result).map(|exit| (result.node_key.clone(), exit))
        })
        .collect::<HashMap<_, _>>();

    let latency_snapshot = (test_ui.latencies)();
    let download_snapshot = (test_ui.downloads)();
    let upload_snapshot = (test_ui.uploads)();
    let exit_snapshot = (test_ui.exits)();
    let testing_snapshot = (test_ui.testing)();
    let mut node_cards = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let node_key = subscription_node_key(node);
            let country_code = subscription_node_country_code(node);
            let google_exit = exit_snapshot
                .get(&node_key)
                .cloned()
                .or_else(|| persisted_exits.get(&node_key).cloned());
            let latency = latency_display(
                latency_snapshot.get(&node_key).cloned(),
                persisted_failures.contains(&node_key),
                persisted_latencies.get(&node_key).copied(),
            );
            NodeCardData {
                index,
                node: node.clone(),
                selected: selected_node_key.as_deref() == Some(node_key.as_str()),
                failed: latency == LATENCY_FAILED,
                latency,
                download_speed: download_snapshot
                    .get(&node_key)
                    .cloned()
                    .unwrap_or_else(|| "-- MB/s".to_string()),
                upload_speed: upload_snapshot
                    .get(&node_key)
                    .cloned()
                    .unwrap_or_else(|| "-- MB/s".to_string()),
                country_code,
                google_exit,
                testing_latency: testing_snapshot.get(&node_key) == Some(&TestKind::Latency),
                testing_speed: testing_snapshot.get(&node_key) == Some(&TestKind::Speed),
                testing_exit: testing_snapshot.get(&node_key) == Some(&TestKind::Exit),
            }
        })
        .collect::<Vec<_>>();
    if hide_timeout() {
        remove_failed_nodes(&mut node_cards);
    }
    sort_node_cards(&mut node_cards, sort_mode());

    let node_service = Arc::clone(&proxy_core);
    let node_database = Arc::clone(&database);
    let node_subscription = selected.clone();
    let node_latency_names = Arc::clone(&node_names);
    let on_node_latency: EventHandler<String> = EventHandler::new(move |key| {
        let Some(subscription) = node_subscription.clone() else {
            return;
        };
        start_test_run(
            TestKind::Latency,
            vec![key],
            Arc::clone(&node_service),
            Arc::clone(&node_database),
            subscription,
            false,
            test_ui,
            app_log,
            Arc::clone(&node_latency_names),
        );
    });
    let node_speed_service = Arc::clone(&proxy_core);
    let node_speed_database = Arc::clone(&database);
    let node_speed_subscription = selected.clone();
    let node_speed_names = Arc::clone(&node_names);
    let on_node_speed: EventHandler<String> = EventHandler::new(move |key| {
        let Some(subscription) = node_speed_subscription.clone() else {
            return;
        };
        start_test_run(
            TestKind::Speed,
            vec![key],
            Arc::clone(&node_speed_service),
            Arc::clone(&node_speed_database),
            subscription,
            false,
            test_ui,
            app_log,
            Arc::clone(&node_speed_names),
        );
    });

    let node_exit_service = Arc::clone(&proxy_core);
    let node_exit_database = Arc::clone(&database);
    let node_exit_subscription = selected.clone();
    let node_exit_log = app_log;
    let node_exit_names = Arc::clone(&node_names);
    let on_node_exit: EventHandler<String> = EventHandler::new(move |key: String| {
        let Some(subscription) = node_exit_subscription.clone() else {
            return;
        };
        let name = log_node_name(&node_exit_names, &key);
        node_exit_log.record(
            LogLevel::Info,
            "Google exit",
            format!("Manual exit test started for {name}."),
        );
        start_test_run(
            TestKind::Exit,
            vec![key],
            Arc::clone(&node_exit_service),
            Arc::clone(&node_exit_database),
            subscription,
            false,
            test_ui,
            node_exit_log,
            Arc::clone(&node_exit_names),
        );
    });

    let batch_latency_service = Arc::clone(&proxy_core);
    let batch_latency_database = Arc::clone(&database);
    let batch_latency_subscription = selected.clone();
    let batch_latency_keys = node_keys.clone();
    let batch_latency_names = Arc::clone(&node_names);
    let batch_speed_service = Arc::clone(&proxy_core);
    let batch_speed_database = Arc::clone(&database);
    let batch_speed_subscription = selected.clone();
    let batch_speed_keys = node_keys.clone();
    let batch_speed_names = Arc::clone(&node_names);
    let hide_timeout_database = Arc::clone(&database);

    rsx! {
        div { id: "proxies-view", class: "view-section active proxies-page",
            div { class: "view-header",
                h2 { class: "page-title", "Proxy Nodes" }
                div { class: "toolbar",
                    select {
                        id: "proxy-sort-select",
                        class: "proxy-sort-select",
                        title: "Sort by latency, download speed, or address & Google exit",
                        "aria-label": "Sort nodes",
                        value: sort_mode().value(),
                        onchange: move |event| {
                            sort_mode.set(SortMode::from_value(&event.value()));
                        },
                        option {
                            value: SortMode::Latency.value(),
                            selected: sort_mode() == SortMode::Latency,
                            "Latency"
                        }
                        option {
                            value: SortMode::Download.value(),
                            selected: sort_mode() == SortMode::Download,
                            "Download speed"
                        }
                        option {
                            value: SortMode::Region.value(),
                            selected: sort_mode() == SortMode::Region,
                            "Address & exit"
                        }
                    }
                    button {
                        id: "test-all-btn",
                        class: "btn-primary",
                        style: "width: auto; padding: 10px 20px",
                        disabled: batch_running().is_some() || node_keys.is_empty(),
                        onclick: move |_| {
                            if batch_running().is_some() || batch_latency_keys.is_empty() {
                                return;
                            }
                            let Some(subscription) = batch_latency_subscription.clone() else {
                                return;
                            };
                            batch_running.set(Some(TestKind::Latency));
                            show_proxy_toast(
                                toast_message,
                                toast_generation,
                                "Testing latency and Google exit for all nodes...",
                            );
                            start_test_run(
                                TestKind::Latency,
                                batch_latency_keys.clone(),
                                Arc::clone(&batch_latency_service),
                                Arc::clone(&batch_latency_database),
                                subscription,
                                true,
                                test_ui,
                                app_log,
                                Arc::clone(&batch_latency_names),
                            );
                        },
                        if batch_running() == Some(TestKind::Latency) {
                            i { class: "fa-solid fa-spinner fa-spin" }
                        } else {
                            i { class: "fa-solid fa-bolt" }
                        }
                        "Test Latency"
                    }
                    button {
                        id: "speed-test-all-btn",
                        class: "btn-secondary",
                        style: "width: auto; padding: 10px 20px",
                        disabled: batch_running().is_some() || node_keys.is_empty(),
                        onclick: move |_| {
                            if batch_running().is_some() || batch_speed_keys.is_empty() {
                                return;
                            }
                            let Some(subscription) = batch_speed_subscription.clone() else {
                                return;
                            };
                            batch_running.set(Some(TestKind::Speed));
                            show_proxy_toast(
                                toast_message,
                                toast_generation,
                                "Running a speed test for all nodes...",
                            );
                            start_test_run(
                                TestKind::Speed,
                                batch_speed_keys.clone(),
                                Arc::clone(&batch_speed_service),
                                Arc::clone(&batch_speed_database),
                                subscription,
                                true,
                                test_ui,
                                app_log,
                                Arc::clone(&batch_speed_names),
                            );
                        },
                        if batch_running() == Some(TestKind::Speed) {
                            i { class: "fa-solid fa-spinner fa-spin" }
                        } else {
                            i { class: "fa-solid fa-gauge-high" }
                        }
                        "Speed Test All"
                    }
                    label { class: "toggle-switch",
                        input {
                            r#type: "checkbox",
                            id: "hide-timeout-toggle",
                            checked: hide_timeout(),
                            onchange: move |event| {
                                let enabled = event.checked();
                                hide_timeout.set(enabled);
                                let _ = hide_timeout_database.set_setting(
                                    HIDE_TIMEOUT_SETTING,
                                    if enabled { "1" } else { "0" },
                                );
                            },
                        }
                        span { class: "slider" }
                        span { class: "label-text", "Hide Timeout" }
                    }
                    if !toast_message().is_empty() {
                        div { class: "proxy-toast", role: "status", "{toast_message}" }
                    }
                }
            }
            if selected.is_none() {
                div { class: "empty-state", "No subscription selected. Select one in Subscribes." }
            } else if nodes.is_empty() {
                div { class: "empty-state",
                    "The selected subscription has no parsed nodes. Refresh it from Subscribes first."
                }
            } else if node_cards.is_empty() {
                div { class: "empty-state",
                    "All nodes failed their last test. Turn off Hide Timeout to show them."
                }
            } else {
                div { class: "node-grid", id: "node-grid",
                    for card in node_cards {
                        SubscriptionNodeCard {
                            key: "{subscription_id.unwrap_or_default()}-{card.index}",
                            node: card.node,
                            selected: card.selected,
                            subscription_id,
                            latency: card.latency,
                            download_speed: card.download_speed,
                            upload_speed: card.upload_speed,
                            country_code: card.country_code,
                            google_exit: card.google_exit,
                            testing_latency: card.testing_latency,
                            testing_speed: card.testing_speed,
                            testing_exit: card.testing_exit,
                            on_test_latency: on_node_latency,
                            on_test_speed: on_node_speed,
                            on_test_exit: on_node_exit,
                        }
                    }
                }
            }
        }
    }
}

struct WorkerGuard {
    remaining: Arc<AtomicUsize>,
    sender: mpsc::Sender<TestMessage>,
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if self.remaining.fetch_sub(1, AtomicOrdering::SeqCst) == 1 {
            let _ = self.sender.send(TestMessage::Finished);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn start_test_run(
    kind: TestKind,
    node_keys: Vec<String>,
    service: Arc<ProxyCoreService>,
    database: Arc<Database>,
    subscription: Subscription,
    is_batch: bool,
    ui: ProxyTestUi,
    logs: AppLog,
    node_names: Arc<HashMap<String, String>>,
) {
    let (sender, receiver) = mpsc::channel();
    let worker_count = node_keys.len().min(TEST_CONCURRENCY).max(1);
    let queue = Arc::new(Mutex::new(VecDeque::from(node_keys)));
    let remaining = Arc::new(AtomicUsize::new(worker_count));
    let subscription_id = subscription.id;
    let result_database = Arc::clone(&database);

    for _ in 0..worker_count {
        let queue = Arc::clone(&queue);
        let remaining = Arc::clone(&remaining);
        let sender = sender.clone();
        let service = Arc::clone(&service);
        let database = Arc::clone(&database);
        let subscription = subscription.clone();
        std::thread::spawn(move || {
            let _guard = WorkerGuard {
                remaining,
                sender: sender.clone(),
            };
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(async move {
                loop {
                    let Some(key) = queue
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .pop_front()
                    else {
                        break;
                    };
                    if sender
                        .send(TestMessage::NodeStarted {
                            key: key.clone(),
                            kind,
                        })
                        .is_err()
                    {
                        break;
                    }
                    match kind {
                        TestKind::Latency => {
                            let result =
                                proxy_test::test_latency(&service, &database, &subscription, &key)
                                    .await;
                            if sender
                                .send(TestMessage::LatencyResult { key, result })
                                .is_err()
                            {
                                break;
                            }
                        }
                        TestKind::Speed => {
                            let result =
                                proxy_test::test_speed(&service, &database, &subscription, &key)
                                    .await;
                            if sender
                                .send(TestMessage::SpeedResult { key, result })
                                .is_err()
                            {
                                break;
                            }
                        }
                        TestKind::Exit => {
                            let result = proxy_test::test_google_exit(
                                &service,
                                &database,
                                &subscription,
                                &key,
                            )
                            .await;
                            if sender
                                .send(TestMessage::ExitResult { key, result })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            });
        });
    }

    let mut latencies = ui.latencies;
    let mut packet_losses = ui.packet_losses;
    let mut downloads = ui.downloads;
    let mut uploads = ui.uploads;
    let mut exits = ui.exits;
    let mut testing = ui.testing;
    let mut batch_running = ui.batch_running;
    spawn_forever(async move {
        loop {
            let mut finished = false;
            loop {
                match receiver.try_recv() {
                    Ok(TestMessage::NodeStarted { key, kind }) => {
                        testing.write().insert(key, kind);
                    }
                    Ok(TestMessage::LatencyResult { key, result }) => {
                        testing.write().remove(&key);
                        match result {
                            Ok(result) => {
                                latencies
                                    .write()
                                    .insert(key.clone(), format!("{} ms", result.milliseconds));
                                packet_losses.write().insert(
                                    key.clone(),
                                    format!("{} %", result.packet_loss_percent),
                                );
                                let _ = result_database.record_node_latency(
                                    subscription_id,
                                    &key,
                                    Some(result.milliseconds),
                                    Some(result.packet_loss_percent),
                                );
                                if let Ok(exit) = result.google_exit {
                                    let _ = result_database.record_node_google_exit(
                                        subscription_id,
                                        &key,
                                        &exit.ip,
                                        exit.country_code.as_deref(),
                                        exit.country.as_deref(),
                                        exit.region.as_deref(),
                                    );
                                    exits.write().insert(key, exit);
                                }
                            }
                            Err(error) => {
                                latencies
                                    .write()
                                    .insert(key.clone(), LATENCY_FAILED.to_string());
                                packet_losses.write().remove(&key);
                                let _ = result_database.record_node_latency(
                                    subscription_id,
                                    &key,
                                    None,
                                    None,
                                );
                                show_proxy_toast(
                                    ui.toast_message,
                                    ui.toast_generation,
                                    format!("Latency test failed: {error}"),
                                );
                            }
                        }
                    }
                    Ok(TestMessage::SpeedResult { key, result }) => {
                        testing.write().remove(&key);
                        match result {
                            Ok(result) => {
                                downloads.write().insert(
                                    key.clone(),
                                    format!("{:.2} MB/s", result.download_mbps),
                                );
                                if let Some(upload_mbps) = result.upload_mbps {
                                    uploads
                                        .write()
                                        .insert(key, format!("{upload_mbps:.2} MB/s"));
                                }
                            }
                            Err(error) => {
                                downloads
                                    .write()
                                    .insert(key.clone(), SPEED_FAILED.to_string());
                                uploads.write().remove(&key);
                                show_proxy_toast(
                                    ui.toast_message,
                                    ui.toast_generation,
                                    format!("Speed test failed: {error}"),
                                );
                            }
                        }
                    }
                    Ok(TestMessage::ExitResult { key, result }) => {
                        testing.write().remove(&key);
                        match result {
                            Ok(exit) => {
                                let _ = result_database.record_node_google_exit(
                                    subscription_id,
                                    &key,
                                    &exit.ip,
                                    exit.country_code.as_deref(),
                                    exit.country.as_deref(),
                                    exit.region.as_deref(),
                                );
                                if kind == TestKind::Exit && !is_batch {
                                    let name = log_node_name(&node_names, &key);
                                    logs.record(
                                        LogLevel::Success,
                                        "Google exit",
                                        exit_log_message(&name, &exit),
                                    );
                                }
                                exits.write().insert(key, exit);
                            }
                            Err(error) => {
                                if kind == TestKind::Exit && !is_batch {
                                    let name = log_node_name(&node_names, &key);
                                    logs.record(
                                        LogLevel::Error,
                                        "Google exit",
                                        format!("{name}: {error}"),
                                    );
                                }
                                show_proxy_toast(
                                    ui.toast_message,
                                    ui.toast_generation,
                                    format!("Google exit test failed: {error}"),
                                );
                            }
                        }
                    }
                    Ok(TestMessage::Finished) => finished = true,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finished = true;
                        break;
                    }
                }
            }

            if finished {
                if is_batch {
                    batch_running.set(None);
                    show_proxy_toast(
                        ui.toast_message,
                        ui.toast_generation,
                        match kind {
                            TestKind::Latency => "Latency and Google exit tests finished.",
                            TestKind::Speed => "Speed test finished.",
                            TestKind::Exit => "Google exit tests finished.",
                        },
                    );
                }
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
}

fn show_proxy_toast(
    mut message: Signal<String>,
    mut generation: Signal<u64>,
    text: impl Into<String>,
) {
    let current = generation() + 1;
    generation.set(current);
    message.set(text.into());
    spawn_forever(async move {
        tokio::time::sleep(Duration::from_millis(2800)).await;
        if generation() == current {
            message.set(String::new());
        }
    });
}

#[component]
fn SubscriptionNodeCard(
    node: SubscriptionNode,
    selected: bool,
    subscription_id: Option<i64>,
    latency: String,
    download_speed: String,
    upload_speed: String,
    country_code: Option<String>,
    google_exit: Option<GoogleExitResult>,
    testing_latency: bool,
    testing_speed: bool,
    testing_exit: bool,
    on_test_latency: EventHandler<String>,
    on_test_speed: EventHandler<String>,
    on_test_exit: EventHandler<String>,
) -> Element {
    let database = use_context::<Arc<Database>>();
    let mut data_revision = use_context::<Signal<u64>>();
    let proxy_core = use_context::<Arc<ProxyCoreService>>();
    let test_ui = use_context::<ProxyTestUi>();
    let selected_node_key = subscription_node_key(&node);
    let card_database = Arc::clone(&database);
    let card_proxy_core = Arc::clone(&proxy_core);
    let card_node_key = selected_node_key.clone();
    let latency_node_key = selected_node_key.clone();
    let exit_node_key = selected_node_key.clone();
    let speed_node_key = selected_node_key;
    let exit_matches = google_exit_matches(country_code.as_deref(), google_exit.as_ref());
    let exit_country_label = google_exit
        .as_ref()
        .and_then(|exit| exit.country_code.as_deref())
        .map(|code| match country_flag(code) {
            Some(flag) => format!("{flag} {code}"),
            None => code.to_string(),
        })
        .unwrap_or_else(|| "—".to_string());
    let exit_region = google_exit
        .as_ref()
        .and_then(|exit| {
            exit.region
                .clone()
                .or_else(|| exit.country.clone())
                .or_else(|| exit.country_code.clone())
        })
        .unwrap_or_else(|| "—".to_string());
    let exit_ip = google_exit
        .as_ref()
        .map(|exit| exit.ip.clone())
        .unwrap_or_else(|| "—".to_string());
    rsx! {
        article {
            class: if selected { "node-card active" } else { "node-card" },
            onclick: move |_| {
                // Selecting the node that is already active must not throw away
                // the measurements that belong to it.
                if selected {
                    return;
                }
                if let Some(subscription_id) = subscription_id {
                    if card_database
                        .select_subscription_node(subscription_id, &card_node_key)
                        .is_ok()
                    {
                        data_revision.with_mut(|revision| *revision += 1);
                        // When this subscription is the one the core is
                        // serving, move it to the new node right away. The
                        // balancer override keeps the process (and every open
                        // connection) alive.
                        let status = card_proxy_core.status();
                        if status.running && status.subscription_id == Some(subscription_id) {
                            let switch_core = Arc::clone(&card_proxy_core);
                            let switch_database = Arc::clone(&card_database);
                            let switch_key = card_node_key.clone();
                            let toast_message = test_ui.toast_message;
                            let toast_generation = test_ui.toast_generation;
                            spawn_forever(async move {
                                // Restarting a config that cannot switch live
                                // waits for the core, so keep it off the UI
                                // thread.
                                let result = tokio::task::spawn_blocking(move || {
                                    switch_core.switch_to_node(
                                        &switch_database,
                                        subscription_id,
                                        &switch_key,
                                    )
                                })
                                .await;
                                match result {
                                    Ok(Ok((_, restarted))) => show_proxy_toast(
                                        toast_message,
                                        toast_generation,
                                        if restarted {
                                            "Node switched. This subscription format restarts the core."
                                        } else {
                                            "Node switched."
                                        },
                                    ),
                                    Ok(Err(error)) => show_proxy_toast(
                                        toast_message,
                                        toast_generation,
                                        format!("Could not switch node: {error}"),
                                    ),
                                    Err(error) => show_proxy_toast(
                                        toast_message,
                                        toast_generation,
                                        format!("Could not switch node: {error}"),
                                    ),
                                }
                            });
                        }
                    }
                }
            },
            div { class: "node-header",
                div {
                    class: if exit_matches { "google-exit-region matched" } else { "google-exit-region" },
                    title: "Google exit",
                    i { class: "fa-solid fa-earth-americas" }
                    span { "Google exit: {exit_country_label}" }
                }
                div { class: "node-latency-wrapper",
                    button {
                        class: "node-refresh-btn",
                        title: "Test Google exit",
                        "aria-label": "Test Google exit for {node.name}",
                        disabled: testing_exit,
                        onclick: move |event| {
                            event.stop_propagation();
                            on_test_exit.call(exit_node_key.clone());
                        },
                        if testing_exit {
                            i { class: "fa-solid fa-location-crosshairs fa-spin" }
                        } else {
                            i { class: "fa-solid fa-location-crosshairs" }
                        }
                    }
                    div { class: "node-latency", "{latency}" }
                    button {
                        class: "node-refresh-btn",
                        title: "Test latency & Google exit",
                        "aria-label": "Test latency and Google exit for {node.name}",
                        disabled: testing_latency,
                        onclick: move |event| {
                            event.stop_propagation();
                            on_test_latency.call(latency_node_key.clone());
                        },
                        if testing_latency {
                            i { class: "fa-solid fa-rotate fa-spin" }
                        } else {
                            i { class: "fa-solid fa-rotate" }
                        }
                    }
                }
            }
            div { class: "node-info",
                h3 { class: "node-name-scroll", title: "{node.name}", "{node.name}" }
                div { class: "node-meta-grid",
                    div { class: "meta-item",
                        i { class: "fa-solid fa-shield-halved" }
                        "{node.protocol}"
                    }
                    div { class: "meta-item",
                        i { class: "fa-solid fa-network-wired" }
                        "{node.transport}"
                    }
                    div { class: "meta-item",
                        i { class: "fa-solid fa-certificate" }
                        "{node.tls}"
                    }
                    div { class: "meta-item",
                        i { class: "fa-solid fa-location-dot" }
                        span { class: "meta-value", title: "Exit region: {exit_region}", "{exit_region}" }
                    }
                }
            }
            div { class: "node-speed-area",
                div { class: "speed-metrics",
                    div { class: "speed-item",
                        i { class: "fa-solid fa-arrow-down" }
                        span { "{download_speed}" }
                    }
                    div { class: "speed-item",
                        i { class: "fa-solid fa-arrow-up" }
                        span { "{upload_speed}" }
                    }
                }
                button {
                    class: "node-speed-test-btn",
                    title: "Speed Test",
                    "aria-label": "Speed test for {node.name}",
                    disabled: testing_speed,
                    onclick: move |event| {
                        event.stop_propagation();
                        on_test_speed.call(speed_node_key.clone());
                    },
                    if testing_speed {
                        i { class: "fa-solid fa-gauge-high fa-spin" }
                    } else {
                        i { class: "fa-solid fa-gauge-high" }
                    }
                }
            }
            div { class: "node-details",
                span { class: "cert-tag", "Google exit IP" }
                span { class: "ip-tag", title: "{exit_ip}", "{exit_ip}" }
            }
        }
    }
}

/// Looks up the display name for a node key captured when a test started.
fn log_node_name(names: &HashMap<String, String>, key: &str) -> String {
    names.get(key).cloned().unwrap_or_else(|| key.to_string())
}

/// Formats a manual exit result for the application log.
fn exit_log_message(name: &str, exit: &GoogleExitResult) -> String {
    let country = exit
        .country_code
        .as_deref()
        .map(|code| match country_flag(code) {
            Some(flag) => format!("{flag} {code}"),
            None => code.to_string(),
        });
    let place = match (exit.region.as_deref(), exit.country.as_deref()) {
        (Some(region), Some(country)) => format!("{region}, {country}"),
        (Some(region), None) => region.to_string(),
        (None, Some(country)) => country.to_string(),
        (None, None) => "unknown location".to_string(),
    };
    match country {
        Some(country) => format!("{name}: {country} · {place} · {}", exit.ip),
        None => format!("{name}: {place} · {}", exit.ip),
    }
}

/// Prefers the in-memory result, then falls back to the persisted one. A
/// stored failure is shown as "Failed" rather than "-- ms" so Hide Timeout
/// also removes nodes that failed before the last restart.
fn latency_display(
    live: Option<String>,
    persisted_failed: bool,
    persisted_latency_ms: Option<u64>,
) -> String {
    live.or_else(|| {
        if persisted_failed {
            Some(LATENCY_FAILED.to_string())
        } else {
            persisted_latency_ms.map(|latency| format!("{latency} ms"))
        }
    })
    .unwrap_or_else(|| "-- ms".to_string())
}

fn remove_failed_nodes(cards: &mut Vec<NodeCardData>) {
    cards.retain(|card| !card.failed);
}

fn sort_node_cards(cards: &mut [NodeCardData], mode: SortMode) {
    match mode {
        SortMode::Latency => cards.sort_by(|left, right| {
            latency_value(&left.latency)
                .unwrap_or(u64::MAX)
                .cmp(&latency_value(&right.latency).unwrap_or(u64::MAX))
                .then_with(|| left.index.cmp(&right.index))
        }),
        SortMode::Region => cards.sort_by(|left, right| {
            // Nodes whose Google exit agrees with their advertised region come
            // first, then matching regions are grouped by address so similar
            // nodes stay together.
            exit_match_rank(left)
                .cmp(&exit_match_rank(right))
                .then_with(|| {
                    compare_optional_text(
                        left.country_code.as_deref(),
                        right.country_code.as_deref(),
                    )
                })
                .then_with(|| left.node.address.cmp(&right.node.address))
                .then_with(|| left.node.name.cmp(&right.node.name))
                .then_with(|| left.index.cmp(&right.index))
        }),
        SortMode::Download => cards.sort_by(|left, right| {
            // Fastest download first; nodes without a successful speed test
            // (untested or failed) stay at the end.
            match (
                download_speed_value(&left.download_speed),
                download_speed_value(&right.download_speed),
            ) {
                (Some(left_speed), Some(right_speed)) => right_speed
                    .partial_cmp(&left_speed)
                    .unwrap_or(Ordering::Equal),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
            .then_with(|| left.index.cmp(&right.index))
        }),
    }
}

fn latency_value(display: &str) -> Option<u64> {
    display.strip_suffix(" ms")?.trim().parse().ok()
}

fn download_speed_value(display: &str) -> Option<f64> {
    display.strip_suffix(" MB/s")?.trim().parse().ok()
}

fn exit_match_rank(card: &NodeCardData) -> u8 {
    let exit_country = card
        .google_exit
        .as_ref()
        .and_then(|exit| exit.country_code.as_deref());
    match (card.country_code.as_deref(), exit_country) {
        (Some(node), Some(exit)) if node.eq_ignore_ascii_case(exit) => 0,
        (Some(_), Some(_)) => 1,
        _ => 2,
    }
}

fn compare_optional_text(left: Option<&str>, right: Option<&str>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn google_exit_matches(node_country: Option<&str>, exit: Option<&GoogleExitResult>) -> bool {
    match (
        node_country,
        exit.and_then(|exit| exit.country_code.as_deref()),
    ) {
        (Some(node), Some(exit)) => node.eq_ignore_ascii_case(exit),
        _ => false,
    }
}

fn exit_from_test_result(result: &NodeTestResult) -> Option<GoogleExitResult> {
    Some(GoogleExitResult {
        ip: result.exit_ip.clone()?,
        country_code: result.exit_country_code.clone(),
        country: result.exit_country.clone(),
        region: result.exit_region.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(
        index: usize,
        name: &str,
        address: &str,
        latency: &str,
        country_code: Option<&str>,
        exit_country: Option<&str>,
    ) -> NodeCardData {
        NodeCardData {
            index,
            node: SubscriptionNode {
                name: name.to_string(),
                protocol: "vless".to_string(),
                transport: "ws".to_string(),
                tls: "tls".to_string(),
                address: address.to_string(),
            },
            selected: false,
            failed: latency == LATENCY_FAILED,
            latency: latency.to_string(),
            download_speed: "-- MB/s".to_string(),
            upload_speed: "-- MB/s".to_string(),
            country_code: country_code.map(str::to_string),
            google_exit: exit_country.map(|country| GoogleExitResult {
                ip: "203.0.113.7".to_string(),
                country_code: Some(country.to_string()),
                country: None,
                region: None,
            }),
            testing_latency: false,
            testing_speed: false,
            testing_exit: false,
        }
    }

    #[test]
    fn hide_timeout_removes_only_failed_nodes() {
        let mut cards = vec![
            card(0, "fast", "a.example", "40 ms", None, None),
            card(1, "failed", "b.example", LATENCY_FAILED, None, None),
            card(2, "untested", "c.example", "-- ms", None, None),
        ];

        remove_failed_nodes(&mut cards);

        let names = cards
            .iter()
            .map(|card| card.node.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["fast", "untested"]);
    }

    #[test]
    fn persisted_failures_render_as_failed_and_are_hidden() {
        assert_eq!(latency_display(None, true, None), LATENCY_FAILED);
        // A fresh live result wins over a stale persisted failure.
        assert_eq!(
            latency_display(Some("35 ms".to_string()), true, None),
            "35 ms"
        );
        assert_eq!(latency_display(None, false, Some(120)), "120 ms");
        assert_eq!(latency_display(None, false, None), "-- ms");

        let mut cards = vec![
            card(
                0,
                "persisted-failure",
                "a.example",
                &latency_display(None, true, None),
                None,
                None,
            ),
            card(1, "untested", "b.example", "-- ms", None, None),
        ];

        remove_failed_nodes(&mut cards);

        let names = cards
            .iter()
            .map(|card| card.node.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["untested"]);
    }

    #[test]
    fn latency_sort_keeps_unknown_results_last() {
        let mut cards = vec![
            card(0, "unknown", "a.example", "-- ms", None, None),
            card(1, "slow", "b.example", "240 ms", None, None),
            card(2, "failed", "c.example", "Failed", None, None),
            card(3, "fast", "d.example", "40 ms", None, None),
        ];

        sort_node_cards(&mut cards, SortMode::Latency);

        let names = cards
            .iter()
            .map(|card| card.node.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["fast", "slow", "unknown", "failed"]);
    }

    #[test]
    fn download_sort_places_fastest_first_and_unknown_last() {
        let mut cards = vec![
            card(0, "untested", "a.example", "-- ms", None, None),
            card(1, "slow", "b.example", "-- ms", None, None),
            card(2, "fast", "c.example", "-- ms", None, None),
            card(3, "failed", "d.example", "-- ms", None, None),
            card(4, "medium", "e.example", "-- ms", None, None),
        ];
        cards[1].download_speed = "1.20 MB/s".to_string();
        cards[2].download_speed = "12.50 MB/s".to_string();
        cards[3].download_speed = SPEED_FAILED.to_string();
        cards[4].download_speed = "4.00 MB/s".to_string();

        sort_node_cards(&mut cards, SortMode::Download);

        let names = cards
            .iter()
            .map(|card| card.node.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["fast", "medium", "slow", "untested", "failed"]);
    }

    #[test]
    fn region_sort_puts_matching_google_exits_first() {
        let mut cards = vec![
            card(0, "mismatch", "a.example", "-- ms", Some("US"), Some("JP")),
            card(1, "match-us", "b.example", "-- ms", Some("US"), Some("US")),
            card(2, "no-exit", "c.example", "-- ms", Some("US"), None),
            card(3, "match-jp", "d.example", "-- ms", Some("JP"), Some("JP")),
        ];

        sort_node_cards(&mut cards, SortMode::Region);

        let names = cards
            .iter()
            .map(|card| card.node.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["match-jp", "match-us", "mismatch", "no-exit"]);
    }

    #[test]
    fn country_codes_become_flag_emojis() {
        assert_eq!(country_flag("US").as_deref(), Some("🇺🇸"));
        assert_eq!(country_flag("jp").as_deref(), Some("🇯🇵"));
        assert_eq!(country_flag("USA"), None);
        assert_eq!(country_flag("1A"), None);
    }

    #[test]
    fn exit_log_messages_name_the_node_and_place() {
        let exit = GoogleExitResult {
            ip: "203.0.113.7".to_string(),
            country_code: Some("JP".to_string()),
            country: Some("Japan".to_string()),
            region: Some("Tokyo".to_string()),
        };
        assert_eq!(
            exit_log_message("Tokyo 01", &exit),
            "Tokyo 01: 🇯🇵 JP · Tokyo, Japan · 203.0.113.7"
        );

        let country_only = GoogleExitResult {
            ip: "5.183.102.48".to_string(),
            country_code: Some("GB".to_string()),
            country: Some("United Kingdom".to_string()),
            region: None,
        };
        assert_eq!(
            exit_log_message("London 01", &country_only),
            "London 01: 🇬🇧 GB · United Kingdom · 5.183.102.48"
        );
    }

    #[test]
    fn exit_matches_only_when_both_regions_are_known() {
        let exit = GoogleExitResult {
            ip: "203.0.113.7".to_string(),
            country_code: Some("US".to_string()),
            country: None,
            region: None,
        };
        assert!(google_exit_matches(Some("us"), Some(&exit)));
        assert!(!google_exit_matches(Some("JP"), Some(&exit)));
        assert!(!google_exit_matches(None, Some(&exit)));
    }
}
