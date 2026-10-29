use std::sync::Arc;

use dioxus::prelude::*;

use crate::components::proxies::{cancel_proxy_test_runs, ProxyTestUi};
use crate::i18n::I18n;
use crate::services::database::{Database, Subscription};
use crate::services::subscription_manager::refresh_subscription;

#[component]
pub fn SubscriptionsPage() -> Element {
    let database = use_context::<Arc<Database>>();
    let data_revision = use_context::<Signal<u64>>();
    let i18n = use_context::<I18n>();
    let _revision = data_revision();
    let mut show_import = use_signal(|| false);
    let subscriptions = database.subscriptions().unwrap_or_default();

    rsx! {
        div { id: "subscribes-view", class: "view-section active",
            div { class: "view-header",
                h2 { class: "page-title", "{i18n.t(\"Subscriptions\")}" }
                div { class: "toolbar",
                    button { id: "update-subs-btn", class: "btn-secondary",
                        i { class: "fa-solid fa-arrows-rotate" }
                        "{i18n.t(\"Update All\")}"
                    }
                    button {
                        id: "import-sub-btn",
                        class: "btn-primary",
                        style: "width: auto",
                        onclick: move |_| show_import.set(true),
                        i { class: "fa-solid fa-plus" }
                        "{i18n.t(\"Import\")}"
                    }
                }
            }
            div { class: "sub-grid", id: "sub-list",
                if subscriptions.is_empty() {
                    div {
                        class: "empty-state",
                        id: "sub-empty-state",
                        style: "grid-column: 1 / -1;",
                        "{i18n.t(\"No subscriptions added.\")}"
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
    let i18n = use_context::<I18n>();
    let test_ui = use_context::<ProxyTestUi>();
    let _revision = data_revision();
    let mut show_edit = use_signal(|| false);
    let mut refreshing = use_signal(|| false);
    let mut refresh_error = use_signal(String::new);
    let refresh_database = Arc::clone(&database);
    let refresh_record = subscription.clone();
    let last_updated = subscription
        .last_updated
        .as_deref()
        .map(format_last_updated)
        .unwrap_or_else(|| i18n.t("Never").to_string());
    let node_count_label = if subscription.last_updated.is_some() {
        format!(
            "{} {} · {} {}",
            i18n.t("Nodes"),
            subscription.node_count,
            i18n.t("Usable"),
            subscription.usable_node_count
        )
    } else {
        format!("{} — · {} —", i18n.t("Nodes"), i18n.t("Usable"))
    };
    let id = subscription.id;
    let is_selected = subscription.selected;
    let select_database = Arc::clone(&database);
    rsx! {
        article {
            class: if subscription.selected { "sub-card selected" } else { "sub-card" },
            onclick: move |_| {
                if !is_selected && select_database.select_subscription(id).is_ok() {
                    // Test runs belong to the subscription they were started
                    // for; stop them before the new one becomes active.
                    cancel_proxy_test_runs(test_ui);
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
                    "{i18n.t(\"Refresh this subscription to fetch its configuration and count nodes.\")}"
                }
            }
            div { class: "sub-traffic-grid",
                div { class: "traffic-stat",
                    span { class: "traffic-label", "{i18n.t(\"Upload\")}" }
                    span { class: "traffic-val up",
                        i { class: "fa-solid fa-arrow-up" }
                        " {subscription.upload}"
                    }
                }
                div { class: "traffic-stat",
                    span { class: "traffic-label", "{i18n.t(\"Download\")}" }
                    span { class: "traffic-val down",
                        i { class: "fa-solid fa-arrow-down" }
                        " {subscription.download}"
                    }
                }
                div { class: "traffic-stat full",
                    span { class: "traffic-label", "{i18n.t(\"Total Traffic\")}" }
                    span { class: "traffic-val", "{subscription.total}" }
                }
            }
            div { class: "sub-dates",
                div {
                    span { {i18n.t("Expire Time:")} }
                    strong { {format_timestamp(subscription.expire_at, i18n)} }
                }
                div {
                    span { {i18n.t("Reset Time:")} }
                    strong { {format_timestamp(subscription.reset_at, i18n)} }
                }
            }
            div { class: "sub-card-actions",
                button {
                    class: "btn-icon",
                    title: "{i18n.t(\"Edit subscription\")}",
                    "aria-label": "{i18n.t(\"Edit subscription\")}",
                    onclick: move |event| {
                        event.stop_propagation();
                        show_edit.set(true);
                    },
                    i { class: "fa-solid fa-pen-to-square" }
                }
                button {
                    class: "btn-icon",
                    title: "{i18n.t(\"Update Now\")}",
                    "aria-label": "{i18n.t(\"Update subscription now\")}",
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
                    title: "{i18n.t(\"Delete subscription\")}",
                    "aria-label": "{i18n.t(\"Delete subscription\")}",
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
                p { class: "subscription-error", role: "alert", "{i18n.t(\"Refresh failed\")}: {refresh_error}" }
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

fn format_timestamp(timestamp: Option<i64>, i18n: I18n) -> String {
    use chrono::{DateTime, Local, Utc};

    // Subscription servers send Unix timestamps; show them in the same time
    // zone as the rest of the user's desktop instead of UTC.
    timestamp
        .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
        .map(|date| date.with_timezone(&Local).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| i18n.t("N/A").to_string())
}

/// Parses the timestamp SQLite writes for `last_updated`.
///
/// `CURRENT_TIMESTAMP` is UTC and uses the `YYYY-MM-DD HH:MM:SS` format.
/// Existing rows may also carry fractional seconds, and a future writer may
/// store RFC 3339, so all of those are accepted.
fn parse_last_updated(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono::{DateTime, NaiveDateTime, Utc};

    let value = value.trim();
    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return Some(date.with_timezone(&Utc));
    }
    for format in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
    ] {
        if let Ok(date) = NaiveDateTime::parse_from_str(value, format) {
            return Some(date.and_utc());
        }
    }
    None
}

/// Converts SQLite's UTC refresh time into the given time zone.
fn format_last_updated_in<Tz>(value: &str, timezone: &Tz) -> String
where
    Tz: chrono::TimeZone,
    Tz::Offset: std::fmt::Display,
{
    let value = value.trim();
    parse_last_updated(value)
        .map(|date| {
            date.with_timezone(timezone)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| value.to_string())
}

/// Converts SQLite's UTC refresh time into the machine's local time zone.
fn format_last_updated(value: &str) -> String {
    format_last_updated_in(value, &chrono::Local)
}

#[component]
pub fn SubscriptionDialog(
    on_close: EventHandler<()>,
    #[props(default)] subscription: Option<Subscription>,
) -> Element {
    let database = use_context::<Arc<Database>>();
    let mut data_revision = use_context::<Signal<u64>>();
    let i18n = use_context::<I18n>();
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
                            "{i18n.t(\"Edit Subscription\")}"
                        } else {
                            "{i18n.t(\"Add Subscription\")}"
                        }
                    }
                    button {
                        class: "modal-close",
                        id: "close-modal-btn",
                        "aria-label": "{i18n.t(\"Close dialog\")}",
                        onclick: move |_| on_close.call(()),
                        i { class: "fa-solid fa-xmark" }
                    }
                }
                div { class: "modal-body",
                    div { class: "form-group",
                        label { class: "form-label", "{i18n.t(\"Name\")}" }
                        input {
                            r#type: "text",
                            id: "modal-sub-name",
                            class: "form-input",
                            placeholder: "{i18n.t(\"e.g. My Premium Proxy\")}",
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
                            label { class: "form-label", "{i18n.t(\"Auto Update\")}" }
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
                            label { class: "form-label", "{i18n.t(\"Update Interval\")}" }
                            div { class: "interval-input-wrap",
                                input {
                                    r#type: "number",
                                    min: "1",
                                    id: "modal-sub-interval",
                                    class: "form-input",
                                    value: "{interval}",
                                    oninput: move |event| interval.set(event.value()),
                                }
                                span { "{i18n.t(\"hr\")}" }
                            }
                        }
                    }
                    div { class: "form-group",
                        label { class: "form-label", "User-Agent" }
                        input {
                            r#type: "text",
                            id: "modal-sub-ua",
                            class: "form-input",
                            placeholder: "{i18n.t(\"Leave empty for default (Sing-Box)\")}",
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
                        "{i18n.t(\"Cancel\")}"
                    }
                    button {
                        class: "btn-primary",
                        id: "save-modal-btn",
                        onclick: move |_| {
                            let name_value = name().trim().to_string();
                            let url_value = url().trim().to_string();
                            if name_value.is_empty() || url_value.is_empty() {
                                error.set(i18n.t("Name and URL are required.").to_string());
                            } else if !url_value.starts_with("https://") && !url_value.starts_with("http://")
                            {
                                error.set(i18n.t("URL must start with http:// or https://.").to_string());
                            } else if interval()
                                .parse::<u32>()
                                .map_or(true, |hours| hours == 0)
                            {
                                error.set(i18n.t("Update interval must be a positive number of hours.").to_string());
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
                                        error.set(format!(
                                            "{}: {save_error}",
                                            i18n.t("Could not save subscription")
                                        ))
                                    }
                                }
                            }
                        },
                        if subscription.is_some() {
                            "{i18n.t(\"Save Changes\")}"
                        } else {
                            "{i18n.t(\"Save\")}"
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{format_last_updated_in, parse_last_updated};
    use chrono::FixedOffset;

    #[test]
    fn parses_sqlite_utc_timestamp() {
        let parsed = parse_last_updated("2026-10-29 03:04:05").unwrap();
        assert_eq!(parsed.to_rfc3339(), "2026-10-29T03:04:05+00:00");
    }

    #[test]
    fn parses_fractional_and_rfc3339_timestamps() {
        assert_eq!(
            parse_last_updated("2026-10-29 03:04:05.250")
                .unwrap()
                .to_rfc3339(),
            "2026-10-29T03:04:05.250+00:00"
        );
        assert_eq!(
            parse_last_updated("2026-10-29T11:04:05+08:00")
                .unwrap()
                .to_rfc3339(),
            "2026-10-29T03:04:05+00:00"
        );
    }

    #[test]
    fn formats_a_sqlite_timestamp_in_the_requested_zone() {
        let zone = FixedOffset::east_opt(8 * 3600).unwrap();
        assert_eq!(
            format_last_updated_in("2026-10-29 03:04:05", &zone),
            "2026-10-29 11:04:05"
        );
    }

    #[test]
    fn leaves_an_unknown_timestamp_visible() {
        let zone = FixedOffset::east_opt(0).unwrap();
        assert_eq!(
            format_last_updated_in("not a timestamp", &zone),
            "not a timestamp"
        );
    }
}
