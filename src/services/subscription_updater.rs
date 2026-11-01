//! Automatic refresh for subscriptions with auto update enabled.
//!
//! A refresh only rewrites the stored payload (and the traffic counters) in
//! SQLite. It never restarts the running core and never regenerates its config,
//! so the node carrying the current connections keeps working while the data
//! behind the node list changes. New nodes become hot-switchable the next time
//! the core starts, and a node that disappeared from the provider stays in use
//! until it is switched away from.

use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use dioxus::prelude::{Signal, WritableExt};

use super::{
    app_log::{AppLog, LogLevel},
    database::{Database, Subscription},
    proxy_core::ProxyCoreService,
    subscription_manager::{parse_last_updated, refresh_subscription},
};

/// How often the loop looks for subscriptions that are due.
const CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// Delay before the first pass so startup is not competing with the first
/// render (and, when the core auto-starts, with its port wait).
const STARTUP_DELAY: Duration = Duration::from_secs(5);

/// Interval used when a stored `update_interval` cannot be parsed.
const DEFAULT_INTERVAL_HOURS: i64 = 12;

/// Wait before retrying a refresh that failed. A failed refresh leaves
/// `last_updated` untouched, so without this the loop would retry it every
/// minute and pile up identical errors in the log.
const FAILURE_RETRY_MINUTES: i64 = 5;

/// Reads the interval written by the subscription dialog (`"12 hr"`, legacy
/// `"12h"`, or a bare number) as hours. Invalid or zero values fall back to
/// [`DEFAULT_INTERVAL_HOURS`].
pub fn interval_hours(subscription: &Subscription) -> i64 {
    let raw = subscription.update_interval.trim();
    let digits = raw
        .strip_suffix("hr")
        .or_else(|| raw.strip_suffix('h'))
        .unwrap_or(raw)
        .trim();
    digits
        .parse::<i64>()
        .ok()
        .filter(|hours| *hours > 0)
        .unwrap_or(DEFAULT_INTERVAL_HOURS)
}

/// Whether a subscription is now due for its next automatic refresh.
///
/// A subscription that has never been fetched is due immediately: Auto Update
/// is on by default, so a freshly added subscription should fill itself in
/// without a manual refresh.
pub fn is_due(subscription: &Subscription, now: DateTime<Utc>) -> bool {
    if !subscription.auto_update || subscription.url.trim().is_empty() {
        return false;
    }
    // An unreadable timestamp is treated like a missing one rather than
    // blocking updates forever.
    let Some(last_updated) = subscription
        .last_updated
        .as_deref()
        .and_then(parse_last_updated)
    else {
        return true;
    };
    now.signed_duration_since(last_updated) >= chrono::Duration::hours(interval_hours(subscription))
}

/// Background task started once by the app root.
///
/// Every [`CHECK_INTERVAL`] the stored subscriptions are read again, so edits
/// on the Subscribes page are picked up without restarting the app. The
/// `data_revision` signal is bumped after a successful refresh so open pages
/// re-read the database.
pub async fn run_auto_updates(
    database: Arc<Database>,
    proxy_core: Arc<ProxyCoreService>,
    logs: AppLog,
    data_revision: Signal<u64>,
) {
    tokio::time::sleep(STARTUP_DELAY).await;
    // Subscription id → earliest time a failed refresh may be retried.
    let mut retry_after = HashMap::new();
    loop {
        refresh_due_subscriptions(
            &database,
            &proxy_core,
            logs,
            data_revision,
            &mut retry_after,
        )
        .await;
        tokio::time::sleep(CHECK_INTERVAL).await;
    }
}

async fn refresh_due_subscriptions(
    database: &Database,
    proxy_core: &ProxyCoreService,
    logs: AppLog,
    mut data_revision: Signal<u64>,
    retry_after: &mut HashMap<i64, DateTime<Utc>>,
) {
    let now = Utc::now();
    let subscriptions = match database.subscriptions() {
        Ok(subscriptions) => subscriptions,
        Err(error) => {
            logs.record(
                LogLevel::Error,
                "Subscriptions",
                format!("Could not read the subscription list: {error}"),
            );
            return;
        }
    };

    let due = subscriptions
        .into_iter()
        .filter(|subscription| {
            is_due(subscription, now)
                && retry_after
                    .get(&subscription.id)
                    .is_none_or(|retry_at| *retry_at <= now)
        })
        .collect::<Vec<_>>();

    for subscription in due {
        // The port is read right before each request: the core may be started
        // or stopped between two subscriptions in the same pass.
        let socks_port = proxy_core.status().socks_port;
        match refresh_subscription(database, &subscription, socks_port).await {
            Ok(()) => {
                retry_after.remove(&subscription.id);
                logs.record(
                    LogLevel::Success,
                    "Subscriptions",
                    format!("Auto-updated {}.", subscription.name),
                );
                data_revision.with_mut(|revision| *revision += 1);
            }
            Err(error) => {
                retry_after.insert(
                    subscription.id,
                    now + chrono::Duration::minutes(FAILURE_RETRY_MINUTES),
                );
                logs.record(
                    LogLevel::Error,
                    "Subscriptions",
                    format!("Auto-update failed for {}: {error}", subscription.name),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{interval_hours, is_due};
    use crate::services::{
        database::{Database, Subscription},
        subscription_manager::{updates_via_proxy, UPDATE_VIA_PROXY_SETTING},
    };
    use chrono::{TimeZone, Utc};

    fn subscription(
        auto_update: bool,
        update_interval: &str,
        last_updated: Option<&str>,
    ) -> Subscription {
        Subscription {
            id: 1,
            name: "test".to_string(),
            url: "https://example.com/sub".to_string(),
            auto_update,
            update_interval: update_interval.to_string(),
            user_agent: String::new(),
            raw_config: String::new(),
            upload: "0 B".to_string(),
            download: "0 B".to_string(),
            total: "0 B".to_string(),
            expire_at: None,
            reset_at: None,
            last_updated: last_updated.map(str::to_string),
            node_count: 0,
            usable_node_count: 0,
            selected: false,
            selected_node_key: None,
        }
    }

    #[test]
    fn intervals_are_read_in_every_stored_form() {
        assert_eq!(interval_hours(&subscription(true, "12 hr", None)), 12);
        assert_eq!(interval_hours(&subscription(true, "6h", None)), 6);
        assert_eq!(interval_hours(&subscription(true, "24", None)), 24);
        // Invalid or zero values keep the loop running on the default cadence.
        assert_eq!(interval_hours(&subscription(true, "", None)), 12);
        assert_eq!(interval_hours(&subscription(true, "0 hr", None)), 12);
        assert_eq!(interval_hours(&subscription(true, "soon", None)), 12);
    }

    #[test]
    fn never_updated_subscriptions_are_due() {
        let now = Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap();
        assert!(is_due(&subscription(true, "12 hr", None), now));
    }

    #[test]
    fn the_interval_decides_when_a_subscription_is_due() {
        let now = Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap();

        assert!(!is_due(
            &subscription(true, "12 hr", Some("2026-11-01 00:01:00")),
            now
        ));
        assert!(is_due(
            &subscription(true, "12 hr", Some("2026-10-31 23:59:00")),
            now
        ));
        // A corrupt timestamp is treated as missing instead of pinning the
        // subscription forever.
        assert!(is_due(
            &subscription(true, "12 hr", Some("not a timestamp")),
            now
        ));
    }

    #[test]
    fn disabled_auto_update_and_empty_urls_are_never_due() {
        let now = Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap();
        assert!(!is_due(&subscription(false, "1 hr", None), now));

        let mut empty_url = subscription(true, "1 hr", None);
        empty_url.url = "  ".to_string();
        assert!(!is_due(&empty_url, now));
    }

    #[test]
    fn refreshes_use_the_proxy_unless_the_setting_is_off() {
        let database = Database::in_memory().unwrap();
        // Default: unset means "use the proxy".
        assert!(updates_via_proxy(&database));

        database.set_setting(UPDATE_VIA_PROXY_SETTING, "0").unwrap();
        assert!(!updates_via_proxy(&database));

        database.set_setting(UPDATE_VIA_PROXY_SETTING, "1").unwrap();
        assert!(updates_via_proxy(&database));
    }
}
