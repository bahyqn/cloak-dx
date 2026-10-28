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
    /// The dashboard shows three rows; the Network card offers three selects.
    pub const SELECTED_LIMIT: usize = 3;

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

    /// Writes `chosen` into one of the three slots, removing it from any other
    /// slot so the same service cannot be watched twice.
    pub fn with_slot(selection: &[Self], slot: usize, chosen: Option<Self>) -> Vec<Self> {
        let mut slots = (0..Self::SELECTED_LIMIT)
            .map(|index| selection.get(index).copied())
            .collect::<Vec<_>>();
        if let Some(chosen) = chosen {
            for existing in &mut slots {
                if *existing == Some(chosen) {
                    *existing = None;
                }
            }
        }
        if slot < Self::SELECTED_LIMIT {
            slots[slot] = chosen;
        }
        slots.into_iter().flatten().collect()
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
/// is not one of the three watched services.
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
            vec![ServiceId::Google, ServiceId::YouTube, ServiceId::Netflix]
        );
        assert!(ServiceId::parse_selection("").is_empty());
    }

    #[test]
    fn choosing_a_service_moves_it_between_slots_without_duplicates() {
        let initial = vec![ServiceId::Google, ServiceId::YouTube, ServiceId::Netflix];

        // Picking a service that already sits in another slot moves it.
        assert_eq!(
            ServiceId::with_slot(&initial, 0, Some(ServiceId::Netflix)),
            vec![ServiceId::Netflix, ServiceId::YouTube]
        );
        // Clearing one slot removes just that service.
        assert_eq!(
            ServiceId::with_slot(&initial, 1, None),
            vec![ServiceId::Google, ServiceId::Netflix]
        );
        // Adding a fourth service into the freed slot keeps the limit.
        let freed = ServiceId::with_slot(&initial, 1, None);
        assert_eq!(
            ServiceId::with_slot(&freed, 2, Some(ServiceId::GitHub)),
            vec![ServiceId::Google, ServiceId::Netflix, ServiceId::GitHub]
        );
    }
}
