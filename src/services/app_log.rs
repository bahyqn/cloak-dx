//! In-memory application log shown on the Logs page.
//!
//! The proxy core keeps its own output stream, which the Logs page shows in a
//! separate tab. This buffer only holds events produced by the application
//! itself, so the two never mix.

use dioxus::prelude::*;

/// Older entries are dropped once the buffer is full; a window that stays open
/// for days should not grow without bound.
pub const APP_LOG_CAPACITY: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Success,
    Error,
}

impl LogLevel {
    /// Human-readable name used by the entry's tooltip.
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Success => "Success",
            Self::Error => "Error",
        }
    }

    /// Font Awesome icon shown next to the entry.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Info => "fa-solid fa-circle-info",
            Self::Success => "fa-solid fa-circle-check",
            Self::Error => "fa-solid fa-circle-xmark",
        }
    }

    /// CSS modifier for the row (`.level-info`, `.level-success`, ...).
    pub fn slug(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Success => "success",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppLogEntry {
    /// Increases monotonically so the list can use stable keys.
    pub id: u64,
    pub date: String,
    pub time: String,
    pub level: LogLevel,
    /// Short source label, such as `Google exit`.
    pub target: String,
    pub message: String,
}

/// Handle to the shared log buffer.
///
/// It is provided once at the root so any page can append entries and the Logs
/// page re-renders without extra plumbing.
#[derive(Clone, Copy)]
pub struct AppLog {
    pub entries: Signal<Vec<AppLogEntry>>,
}

impl AppLog {
    /// Adds an entry at the top of the list.
    pub fn record(
        mut self,
        level: LogLevel,
        target: impl Into<String>,
        message: impl Into<String>,
    ) {
        let now = chrono::Local::now();
        // Compute the id under the same guard as the insert so concurrent
        // appends cannot hand out the same key.
        let mut entries = self.entries.write();
        let id = entries.first().map_or(1, |entry| entry.id + 1);
        let entry = AppLogEntry {
            id,
            date: now.format("%Y-%m-%d").to_string(),
            time: now.format("%H:%M:%S").to_string(),
            level,
            target: target.into(),
            message: message.into(),
        };
        entries.insert(0, entry);
        entries.truncate(APP_LOG_CAPACITY);
    }

    pub fn clear(mut self) {
        self.entries.write().clear();
    }
}
