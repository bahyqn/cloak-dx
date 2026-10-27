use std::sync::Arc;

use dioxus::prelude::*;

use crate::services::database::{Database, Subscription};
use crate::services::subscription_manager::refresh_subscription;

#[component]
pub fn SubscriptionsPage() -> Element {
    let database = use_context::<Arc<Database>>();
    let data_revision = use_context::<Signal<u64>>();
    let _revision = data_revision();
    let mut show_import = use_signal(|| false);
    let subscriptions = database.subscriptions().unwrap_or_default();

    rsx! {
        div { id: "subscribes-view", class: "view-section active",
            div { class: "view-header",
                h2 { class: "page-title", "Subscriptions" }
                div { class: "toolbar",
                    button { id: "update-subs-btn", class: "btn-secondary",
                        i { class: "fa-solid fa-arrows-rotate" }
                        "Update All"
                    }
                    button {
                        id: "import-sub-btn",
                        class: "btn-primary",
                        style: "width: auto",
                        onclick: move |_| show_import.set(true),
                        i { class: "fa-solid fa-plus" }
                        "Import"
                    }
                }
            }
            div { class: "sub-grid", id: "sub-list",
                if subscriptions.is_empty() {
                    div {
                        class: "empty-state",
                        id: "sub-empty-state",
                        style: "grid-column: 1 / -1;",
                        "No subscriptions added."
                    }
                } else {
                    for subscription in subscriptions {
                        SubscriptionCard { key: "{subscription.id}", subscription }
                    }
                }
            }
            if show_import() {
                SubscriptionDialog { on_close: move |_| show_import.set(false) }
            }
        }
    }
}

#[component]
fn SubscriptionCard(subscription: Subscription) -> Element {
    let database = use_context::<Arc<Database>>();
    let mut data_revision = use_context::<Signal<u64>>();
    let _revision = data_revision();
    let mut show_edit = use_signal(|| false);
    let mut refreshing = use_signal(|| false);
    let mut refresh_error = use_signal(String::new);
    let refresh_database = Arc::clone(&database);
    let refresh_record = subscription.clone();
    let last_updated = subscription
        .last_updated
        .clone()
        .unwrap_or_else(|| "Never".to_string());
    let node_count_label = if subscription.last_updated.is_some() {
        format!(
            "Nodes {} · Usable {}",
            subscription.node_count, subscription.usable_node_count
        )
    } else {
        "Nodes — · Usable —".to_string()
    };
    let id = subscription.id;
    let is_selected = subscription.selected;
    let select_database = Arc::clone(&database);
    rsx! {
        article {
            class: if subscription.selected { "sub-card selected" } else { "sub-card" },
            onclick: move |_| {
                if !is_selected && select_database.select_subscription(id).is_ok() {
                    data_revision.with_mut(|revision| *revision += 1);
                }
            },
            div { class: "sub-card-header",
                div { class: "sub-card-heading",
                    h3 { class: "sub-card-title", "{subscription.name}" }
                    div { class: "sub-card-url", "{subscription.url}" }
                }
            }
            div { class: "sub-card-meta",
                span { class: "sub-meta-item",
                    i { class: "fa-solid fa-server" }
                    "{node_count_label}"
                }
                span { class: "sub-meta-item",
                    i { class: "fa-solid fa-clock-rotate-left" }
                    "{subscription.update_interval}"
                }
                span { class: "sub-meta-item",
                    i { class: "fa-solid fa-check-double" }
                    "{last_updated}"
                }
            }
            if subscription.last_updated.is_none() {
                p { class: "subscription-refresh-hint",
                    "Refresh this subscription to fetch its configuration and count nodes."
                }
            }
            div { class: "sub-traffic-grid",
                div { class: "traffic-stat",
                    span { class: "traffic-label", "Upload" }
                    span { class: "traffic-val up",
                        i { class: "fa-solid fa-arrow-up" }
                        " {subscription.upload}"
                    }
                }
                div { class: "traffic-stat",
                    span { class: "traffic-label", "Download" }
                    span { class: "traffic-val down",
                        i { class: "fa-solid fa-arrow-down" }
                        " {subscription.download}"
                    }
                }
                div { class: "traffic-stat full",
                    span { class: "traffic-label", "Total Traffic" }
                    span { class: "traffic-val", "{subscription.total}" }
                }
            }
            div { class: "sub-dates",
                div {
                    span { "Expire Time:" }
                    strong { "{format_timestamp(subscription.expire_at)}" }
                }
                div {
                    span { "Reset Time:" }
                    strong { "{format_timestamp(subscription.reset_at)}" }
                }
            }
            div { class: "sub-card-actions",
                button {
                    class: "btn-icon",
                    title: "Edit subscription",
                    "aria-label": "Edit subscription",
                    onclick: move |event| {
                        event.stop_propagation();
                        show_edit.set(true);
                    },
                    i { class: "fa-solid fa-pen-to-square" }
                }
                button {
                    class: "btn-icon",
                    title: "Update Now",
                    "aria-label": "Update subscription now",
                    disabled: refreshing(),
                    onclick: move |event| {
                        event.stop_propagation();
                        if refreshing() {
                            return;
                        }
                        refreshing.set(true);
                        refresh_error.set(String::new());
                        let database = Arc::clone(&refresh_database);
                        let record = refresh_record.clone();
                        spawn(async move {
                            match refresh_subscription(&database, &record).await {
                                Ok(()) => data_revision.with_mut(|revision| *revision += 1),
                                Err(error) => refresh_error.set(error),
                            }
                            refreshing.set(false);
                        });
                    },
                    if refreshing() {
                        i { class: "fa-solid fa-rotate fa-spin" }
                    } else {
                        i { class: "fa-solid fa-rotate" }
                    }
                }
                button {
                    class: "btn-icon",
                    title: "Delete subscription",
                    "aria-label": "Delete subscription",
                    onclick: move |event| {
                        event.stop_propagation();
                        if database.remove_subscription(id).is_ok() {
                            data_revision.with_mut(|revision| *revision += 1);
                        }
                    },
                    i { class: "fa-solid fa-trash" }
                }
            }
            if !refresh_error().is_empty() {
                p { class: "subscription-error", role: "alert", "Refresh failed: {refresh_error}" }
            }
        }
        if show_edit() {
            SubscriptionDialog {
                subscription: Some(subscription.clone()),
                on_close: move |_| show_edit.set(false),
            }
        }
    }
}

fn format_timestamp(timestamp: Option<i64>) -> String {
    use chrono::{DateTime, Local, Utc};

    // Subscription servers send Unix timestamps; show them in the same time
    // zone as the rest of the user's desktop instead of UTC.
    timestamp
        .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
        .map(|date| date.with_timezone(&Local).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "N/A".to_string())
}

#[component]
pub fn SubscriptionDialog(
    on_close: EventHandler<()>,
    #[props(default)] subscription: Option<Subscription>,
) -> Element {
    let database = use_context::<Arc<Database>>();
    let mut data_revision = use_context::<Signal<u64>>();
    let initial = subscription.clone();
    let mut name = use_signal(|| {
        initial
            .as_ref()
            .map_or_else(String::new, |item| item.name.clone())
    });
    let mut url = use_signal(|| {
        initial
            .as_ref()
            .map_or_else(String::new, |item| item.url.clone())
    });
    let mut auto_update = use_signal(|| initial.as_ref().is_none_or(|item| item.auto_update));
    let mut interval = use_signal(|| {
        initial.as_ref().map_or_else(
            || "12".to_string(),
            |item| {
                item.update_interval
                    .trim_end_matches(" hr")
                    .trim_end_matches('h')
                    .to_string()
            },
        )
    });
    let mut user_agent = use_signal(|| {
        initial
            .as_ref()
            .map_or_else(String::new, |item| item.user_agent.clone())
    });
    let mut error = use_signal(String::new);

    rsx! {
        div { class: "modal-overlay active", id: "import-modal",
            div { class: "modal",
                div { class: "modal-header",
                    h3 {
                        if subscription.is_some() {
                            "Edit Subscription"
                        } else {
                            "Add Subscription"
                        }
                    }
                    button {
                        class: "modal-close",
                        id: "close-modal-btn",
                        "aria-label": "Close dialog",
                        onclick: move |_| on_close.call(()),
                        i { class: "fa-solid fa-xmark" }
                    }
                }
                div { class: "modal-body",
                    div { class: "form-group",
                        label { class: "form-label", "Name" }
                        input {
                            r#type: "text",
                            id: "modal-sub-name",
                            class: "form-input",
                            placeholder: "e.g. My Premium Proxy",
                            value: "{name}",
                            oninput: move |event| name.set(event.value()),
                        }
                    }
                    div { class: "form-group",
                        label { class: "form-label", "URL" }
                        input {
                            r#type: "text",
                            id: "modal-sub-url",
                            class: "form-input",
                            placeholder: "https://example.com/sub",
                            value: "{url}",
                            oninput: move |event| url.set(event.value()),
                        }
                    }
                    div { class: "form-row",
                        div { class: "form-group",
                            label { class: "form-label", "Auto Update" }
                            label { class: "toggle-switch",
                                input {
                                    r#type: "checkbox",
                                    id: "modal-sub-auto",
                                    checked: auto_update(),
                                    onchange: move |event| auto_update.set(event.checked()),
                                }
                                span { class: "slider" }
                            }
                        }
                        div { class: "form-group", style: "flex: 1;",
                            label { class: "form-label", "Update Interval" }
                            div { class: "interval-input-wrap",
                                input {
                                    r#type: "number",
                                    min: "1",
                                    id: "modal-sub-interval",
                                    class: "form-input",
                                    value: "{interval}",
                                    oninput: move |event| interval.set(event.value()),
                                }
                                span { "hr" }
                            }
                        }
                    }
                    div { class: "form-group",
                        label { class: "form-label", "User-Agent" }
                        input {
                            r#type: "text",
                            id: "modal-sub-ua",
                            class: "form-input",
                            placeholder: "Leave empty for default (Sing-Box)",
                            value: "{user_agent}",
                            oninput: move |event| user_agent.set(event.value()),
                        }
                    }
                    if !error().is_empty() {
                        p { class: "core-status-message error", role: "alert", "{error}" }
                    }
                }
                div { class: "modal-footer",
                    button {
                        class: "btn-secondary",
                        id: "cancel-modal-btn",
                        onclick: move |_| on_close.call(()),
                        "Cancel"
                    }
                    button {
                        class: "btn-primary",
                        id: "save-modal-btn",
                        onclick: move |_| {
                            let name_value = name().trim().to_string();
                            let url_value = url().trim().to_string();
                            if name_value.is_empty() || url_value.is_empty() {
                                error.set("Name and URL are required.".to_string());
                            } else if !url_value.starts_with("https://") && !url_value.starts_with("http://")
                            {
                                error.set("URL must start with http:// or https://.".to_string());
                            } else if interval()
                                .parse::<u32>()
                                .map_or(true, |hours| hours == 0)
                            {
                                error.set("Update interval must be a positive number of hours.".to_string());
                            } else {
                                let interval_value = format!("{} hr", interval());
                                let save_result = if let Some(subscription) = subscription.as_ref() {
                                    database
                                        .update_subscription(
                                            subscription.id,
                                            &name_value,
                                            &url_value,
                                            auto_update(),
                                            &interval_value,
                                            &user_agent(),
                                        )
                                } else {
                                    database
                                        .add_subscription(
                                            &name_value,
                                            &url_value,
                                            auto_update(),
                                            &interval_value,
                                            &user_agent(),
                                        )
                                };
                                match save_result {
                                    Ok(()) => {
                                        data_revision.with_mut(|revision| *revision += 1);
                                        on_close.call(());
                                    }
                                    Err(save_error) => {
                                        error.set(format!("Could not save subscription: {save_error}"))
                                    }
                                }
                            }
                        },
                        if subscription.is_some() {
                            "Save Changes"
                        } else {
                            "Save"
                        }
                    }
                }
            }
        }
    }
}
