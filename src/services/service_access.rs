//! Checks whether the active node can reach common services.
//!
//! Every request goes through the core's SOCKS inbound with `socks5h`, so the
//! node resolves the hostname and the answer describes what a browser on the
//! far side of the tunnel would see.

use std::time::{Duration, Instant};

use super::proxy_test::proxied_client;

const SERVICE_TIMEOUT: Duration = Duration::from_secs(15);
const GOOGLE_PING_URL: &str = "https://www.gstatic.com/generate_204";

/// Services the user can watch from the dashboard and the Network page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ServiceId {
    Google,
    YouTube,
    Gemini,
    Netflix,
    GitHub,
    ChatGpt,
    DisneyPlus,
    Telegram,
}

impl ServiceId {
    pub const ALL: [ServiceId; 8] = [
        ServiceId::Google,
        ServiceId::YouTube,
        ServiceId::Gemini,
        ServiceId::Netflix,
        ServiceId::GitHub,
        ServiceId::ChatGpt,
        ServiceId::DisneyPlus,
        ServiceId::Telegram,
    ];
    /// How many services the picker lets the user watch at once.
    pub const SELECTED_LIMIT: usize = 4;

    pub fn id(self) -> &'static str {
        match self {
            ServiceId::Google => "google",
            ServiceId::YouTube => "youtube",
            ServiceId::Gemini => "gemini",
            ServiceId::Netflix => "netflix",
            ServiceId::GitHub => "github",
            ServiceId::ChatGpt => "chatgpt",
            ServiceId::DisneyPlus => "disneyplus",
            ServiceId::Telegram => "telegram",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ServiceId::Google => "Google",
            ServiceId::YouTube => "YouTube",
            ServiceId::Gemini => "Gemini",
            ServiceId::Netflix => "Netflix",
            ServiceId::GitHub => "GitHub",
            ServiceId::ChatGpt => "ChatGPT",
            ServiceId::DisneyPlus => "Disney+",
            ServiceId::Telegram => "Telegram",
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|service| service.id() == value)
    }

    /// Endpoint used to decide whether the service answers through the node.
    fn url(self) -> &'static str {
        match self {
            ServiceId::Google => "https://www.google.com/generate_204",
            ServiceId::YouTube => "https://www.youtube.com/",
            ServiceId::Gemini => "https://gemini.google.com/",
            ServiceId::Netflix => "https://www.netflix.com/",
            ServiceId::GitHub => "https://github.com/",
            ServiceId::ChatGpt => "https://chatgpt.com/",
            ServiceId::DisneyPlus => "https://www.disneyplus.com/",
            ServiceId::Telegram => "https://web.telegram.org/",
        }
    }

    /// Services shown before the user changes the selection.
    pub fn default_selection() -> Vec<Self> {
        vec![ServiceId::Google, ServiceId::YouTube, ServiceId::Netflix]
    }

    /// Commas are enough here: ids are ASCII and never contain one.
    pub fn serialize(selection: &[Self]) -> String {
        selection
            .iter()
            .map(|service| service.id())
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn parse_selection(value: &str) -> Vec<Self> {
        let mut selection = Vec::new();
        for part in value.split(',') {
            let Some(service) = Self::from_id(part.trim()) else {
                continue;
            };
            if selection.contains(&service) {
                continue;
            }
            selection.push(service);
            if selection.len() == Self::SELECTED_LIMIT {
                break;
            }
        }
        selection
    }

    /// Adds or removes one service, keeping the selection within
    /// [`Self::SELECTED_LIMIT`]. Existing entries keep their order and the
    /// service being added goes to the end, so the rows stay stable while the
    /// user works through the picker.
    pub fn set_selected(selection: &[Self], service: Self, selected: bool) -> Vec<Self> {
        let mut next = selection
            .iter()
            .copied()
            .filter(|existing| *existing != service)
            .collect::<Vec<_>>();
        if selected && next.len() < Self::SELECTED_LIMIT {
            next.push(service);
        }
        next
    }
}

/// Result of reaching one service through the active node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceCheck {
    pub latency_ms: u64,
    pub status: u16,
}

/// Requests the service through the node. Any successful HTTP response means
/// the node can reach it; redirects are followed by the client first.
pub async fn test_service(socks_port: u16, service: ServiceId) -> Result<ServiceCheck, String> {
    let client = proxied_client(socks_port, SERVICE_TIMEOUT)?;
    let started = Instant::now();
    let response = client
        .get(service.url())
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let latency_ms = started.elapsed().as_millis() as u64;
    let status = response.status();
    if status.is_success() {
        Ok(ServiceCheck {
            latency_ms,
            status: status.as_u16(),
        })
    } else {
        Err(format!("HTTP {status}"))
    }
}

/// Round-trip to Google's 204 endpoint, used for the dashboard's Google Ping
/// row. It runs alongside the service checks so the row works even when Google
/// is not one of the watched services.
pub async fn test_google_ping(socks_port: u16) -> Result<u64, String> {
    let client = proxied_client(socks_port, SERVICE_TIMEOUT)?;
    let started = Instant::now();
    let response = client
        .get(GOOGLE_PING_URL)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if status.is_success() {
        Ok(started.elapsed().as_millis() as u64)
    } else {
        Err(format!("HTTP {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::ServiceId;

    #[test]
    fn service_ids_round_trip() {
        for service in ServiceId::ALL {
            assert_eq!(ServiceId::from_id(service.id()), Some(service));
        }
        assert_eq!(ServiceId::from_id("unknown"), None);
    }

    #[test]
    fn selection_serialization_skips_duplicates_and_extra_entries() {
        let selection = vec![ServiceId::Google, ServiceId::GitHub, ServiceId::Netflix];
        let encoded = ServiceId::serialize(&selection);
        assert_eq!(encoded, "google,github,netflix");
        assert_eq!(ServiceId::parse_selection(&encoded), selection);

        assert_eq!(
            ServiceId::parse_selection("google,google,youtube,netflix,github"),
            vec![
                ServiceId::Google,
                ServiceId::YouTube,
                ServiceId::Netflix,
                ServiceId::GitHub
            ]
        );
        assert!(ServiceId::parse_selection("").is_empty());
    }

    #[test]
    fn selecting_and_clearing_services_keeps_the_order_and_the_limit() {
        let initial = vec![ServiceId::Google, ServiceId::YouTube, ServiceId::Netflix];

        // Checking a service appends it without duplicating an existing entry.
        assert_eq!(
            ServiceId::set_selected(&initial, ServiceId::GitHub, true),
            vec![
                ServiceId::Google,
                ServiceId::YouTube,
                ServiceId::Netflix,
                ServiceId::GitHub
            ]
        );
        // Unchecking removes just that service.
        assert_eq!(
            ServiceId::set_selected(&initial, ServiceId::YouTube, false),
            vec![ServiceId::Google, ServiceId::Netflix]
        );
        // Re-checking an entry that is already selected is a no-op.
        assert_eq!(
            ServiceId::set_selected(&initial, ServiceId::Netflix, true),
            initial
        );
    }

    #[test]
    fn the_picker_never_holds_more_than_four_services() {
        let full = vec![
            ServiceId::Google,
            ServiceId::YouTube,
            ServiceId::Netflix,
            ServiceId::GitHub,
        ];

        assert_eq!(
            ServiceId::set_selected(&full, ServiceId::Telegram, true),
            full
        );
        // A fifth selection is accepted only after one is unchecked.
        let freed = ServiceId::set_selected(&full, ServiceId::YouTube, false);
        assert_eq!(
            ServiceId::set_selected(&freed, ServiceId::Telegram, true),
            vec![
                ServiceId::Google,
                ServiceId::Netflix,
                ServiceId::GitHub,
                ServiceId::Telegram
            ]
        );
    }
}
