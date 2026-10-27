use reqwest::{header::USER_AGENT, Client};

use super::database::{Database, Subscription};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeStatistics {
    pub node_count: u32,
    pub usable_node_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscriptionNode {
    pub name: String,
    pub protocol: String,
    pub transport: String,
    pub tls: String,
    pub address: String,
}

pub fn subscription_node_key(node: &SubscriptionNode) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        node.protocol.len(),
        node.protocol,
        node.name.len(),
        node.name,
        node.address
    )
}

/// Reads the two-letter country code from the flag emoji in a node name.
///
/// Subscriptions label their nodes with flags such as `🇺🇸美国A1`, which is the
/// only reliable region hint because many nodes share CDN addresses.
pub fn subscription_node_country_code(node: &SubscriptionNode) -> Option<String> {
    let mut indicators = String::with_capacity(2);
    for character in node.name.chars() {
        let codepoint = character as u32;
        if (0x1F1E6..=0x1F1FF).contains(&codepoint) {
            indicators.push(char::from(b'A' + (codepoint - 0x1F1E6) as u8));
            if indicators.len() == 2 {
                return Some(indicators);
            }
        } else if !indicators.is_empty() {
            break;
        }
    }
    None
}

pub trait NodeStatisticsProvider {
    fn statistics(&self, raw_subscription: &str) -> NodeStatistics;
}

pub struct ConfiguredNodeCounter;

impl NodeStatisticsProvider for ConfiguredNodeCounter {
    fn statistics(&self, raw_subscription: &str) -> NodeStatistics {
        let decoded = decode_subscription(raw_subscription);
        let node_count = count_proxy_nodes(&decoded);
        NodeStatistics {
            node_count,
            usable_node_count: node_count,
        }
    }
}

pub async fn refresh_subscription(
    database: &Database,
    subscription: &Subscription,
) -> Result<(), String> {
    let client = Client::builder()
        .user_agent(if subscription.user_agent.is_empty() {
            "cloak-dx"
        } else {
            &subscription.user_agent
        })
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(&subscription.url)
        .header(
            USER_AGENT,
            if subscription.user_agent.is_empty() {
                "cloak-dx"
            } else {
                &subscription.user_agent
            },
        )
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;
    let metadata = response
        .headers()
        .get("subscription-userinfo")
        .and_then(|value| value.to_str().ok())
        .map(parse_subscription_userinfo)
        .unwrap_or_default();
    let body = response.text().await.map_err(|error| error.to_string())?;
    let statistics = ConfiguredNodeCounter.statistics(&body);

    database
        .update_subscription_refresh(
            subscription.id,
            &body,
            &format_bytes(metadata.upload),
            &format_bytes(metadata.download),
            &format_bytes(metadata.total),
            metadata.expire,
            metadata.reset,
            statistics.node_count,
            statistics.usable_node_count,
        )
        .map_err(|error| error.to_string())
}

pub(crate) fn decode_subscription(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.lines().any(|line| is_proxy_uri(line.trim())) {
        return trimmed.to_string();
    }
    use base64::Engine;
    for engine in [
        &base64::engine::general_purpose::STANDARD,
        &base64::engine::general_purpose::URL_SAFE,
        &base64::engine::general_purpose::STANDARD_NO_PAD,
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Ok(bytes) = engine.decode(trimmed) {
            if let Ok(decoded) = String::from_utf8(bytes) {
                return decoded;
            }
        }
    }
    trimmed.to_string()
}

fn is_proxy_uri(line: &str) -> bool {
    [
        "ss://",
        "ssr://",
        "vmess://",
        "vless://",
        "trojan://",
        "hysteria://",
        "hysteria2://",
        "hy2://",
        "tuic://",
        "socks://",
        "socks5://",
        "http://",
    ]
    .iter()
    .any(|scheme| line.to_ascii_lowercase().starts_with(scheme))
}

fn count_proxy_nodes(payload: &str) -> u32 {
    parse_subscription_nodes(payload).len() as u32
}

pub fn parse_subscription_nodes(raw_subscription: &str) -> Vec<SubscriptionNode> {
    let payload = decode_subscription(raw_subscription);
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&payload) {
        return parse_json_nodes(&value);
    }

    let uri_nodes = payload
        .lines()
        .filter_map(|line| parse_proxy_uri(line.trim().trim_start_matches('\u{feff}')))
        .collect::<Vec<_>>();
    if !uri_nodes.is_empty() {
        return uri_nodes;
    }

    parse_yaml_nodes(&payload)
}

fn parse_json_nodes(value: &serde_json::Value) -> Vec<SubscriptionNode> {
    const PROXY_TYPES: &[&str] = &[
        "shadowsocks",
        "shadowsocksr",
        "vmess",
        "vless",
        "trojan",
        "hysteria",
        "hysteria2",
        "tuic",
        "socks",
        "http",
        "wireguard",
        "ssh",
        "anytls",
    ];

    let outbounds = value
        .get("outbounds")
        .or_else(|| value.get("outbound"))
        .and_then(serde_json::Value::as_array);
    if let Some(outbounds) = outbounds {
        let mut nodes = Vec::new();
        for outbound in outbounds {
            let Some(protocol) = outbound
                .get("type")
                .or_else(|| outbound.get("protocol"))
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            if !PROXY_TYPES.contains(&protocol.to_ascii_lowercase().as_str()) {
                continue;
            }

            if let Some(servers) = outbound
                .get("settings")
                .and_then(|settings| settings.get("vnext"))
                .and_then(serde_json::Value::as_array)
            {
                let transport = outbound
                    .get("streamSettings")
                    .and_then(|settings| settings.get("network"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("Unknown");
                let tls = outbound
                    .get("streamSettings")
                    .and_then(|settings| settings.get("security"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("None");
                for server in servers {
                    let address = server
                        .get("address")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("-");
                    let users = server.get("users").and_then(serde_json::Value::as_array);
                    if let Some(users) = users.filter(|users| !users.is_empty()) {
                        for user in users {
                            let name = user
                                .get("email")
                                .and_then(serde_json::Value::as_str)
                                .filter(|email| !email.is_empty())
                                .unwrap_or(address);
                            nodes.push(SubscriptionNode {
                                name: name.to_string(),
                                protocol: protocol.to_string(),
                                transport: transport.to_string(),
                                tls: tls.to_string(),
                                address: address.to_string(),
                            });
                        }
                    } else {
                        nodes.push(SubscriptionNode {
                            name: address.to_string(),
                            protocol: protocol.to_string(),
                            transport: transport.to_string(),
                            tls: tls.to_string(),
                            address: address.to_string(),
                        });
                    }
                }
                continue;
            }

            let options = outbound.get("tls").unwrap_or(&serde_json::Value::Null);
            let transport_options = outbound
                .get("transport")
                .unwrap_or(&serde_json::Value::Null);
            let transport = transport_options
                .get("type")
                .or_else(|| {
                    outbound
                        .get("streamSettings")
                        .and_then(|settings| settings.get("network"))
                })
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Unknown");
            let tls = if options.get("enabled").and_then(serde_json::Value::as_bool) == Some(true)
                || outbound
                    .get("streamSettings")
                    .and_then(|settings| settings.get("security"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|value| value != "none")
            {
                options
                    .get("server_name")
                    .or_else(|| options.get("serverName"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("TLS")
                    .to_string()
            } else {
                "None".to_string()
            };
            let address = outbound
                .get("server")
                .or_else(|| {
                    outbound
                        .get("settings")
                        .and_then(|settings| settings.get("vnext"))
                        .and_then(serde_json::Value::as_array)
                        .and_then(|servers| servers.first())
                        .and_then(|server| server.get("address"))
                })
                .and_then(serde_json::Value::as_str)
                .unwrap_or("-")
                .to_string();
            let name = outbound
                .get("tag")
                .and_then(serde_json::Value::as_str)
                .or_else(|| outbound.get("tag").and_then(serde_json::Value::as_str))
                .unwrap_or(protocol)
                .to_string();
            nodes.push(SubscriptionNode {
                name,
                protocol: protocol.to_string(),
                transport: transport.to_string(),
                tls,
                address,
            });
        }
        return nodes;
    }

    value
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let protocol = entry.get("protocol").and_then(serde_json::Value::as_str)?;
                    if !PROXY_TYPES.contains(&protocol.to_ascii_lowercase().as_str()) {
                        return None;
                    }
                    Some(SubscriptionNode {
                        name: entry
                            .get("tag")
                            .or_else(|| entry.get("remarks"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(protocol)
                            .to_string(),
                        protocol: protocol.to_string(),
                        transport: entry
                            .get("streamSettings")
                            .and_then(|settings| settings.get("network"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("Unknown")
                            .to_string(),
                        tls: entry
                            .get("streamSettings")
                            .and_then(|settings| settings.get("security"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("Unknown")
                            .to_string(),
                        address: entry
                            .get("settings")
                            .and_then(|settings| settings.get("vnext"))
                            .and_then(serde_json::Value::as_array)
                            .and_then(|servers| servers.first())
                            .and_then(|server| server.get("address"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("-")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_yaml_nodes(payload: &str) -> Vec<SubscriptionNode> {
    let Ok(document) = serde_yaml::from_str::<serde_yaml::Value>(payload) else {
        return Vec::new();
    };
    let Some(proxies) = document
        .get("proxies")
        .and_then(serde_yaml::Value::as_sequence)
    else {
        return Vec::new();
    };

    proxies
        .iter()
        .filter_map(|proxy| {
            let map = proxy.as_mapping()?;
            let value = |key: &str| {
                map.get(serde_yaml::Value::String(key.to_string()))
                    .and_then(serde_yaml::Value::as_str)
                    .unwrap_or("Unknown")
                    .to_string()
            };
            Some(SubscriptionNode {
                name: value("name"),
                protocol: value("type"),
                transport: value("network"),
                tls: if map
                    .get(serde_yaml::Value::String("tls".to_string()))
                    .and_then(serde_yaml::Value::as_bool)
                    == Some(false)
                {
                    "None".to_string()
                } else {
                    value("tls")
                },
                address: map
                    .get(serde_yaml::Value::String("server".to_string()))
                    .and_then(serde_yaml::Value::as_str)
                    .unwrap_or("-")
                    .to_string(),
            })
        })
        .collect()
}

pub(crate) fn parse_proxy_uri(uri: &str) -> Option<SubscriptionNode> {
    if !is_proxy_uri(uri) {
        return None;
    }
    let (scheme, remainder) = uri.split_once("://")?;
    let authority_and_name = remainder.split_once('#');
    let name = authority_and_name
        .map(|(_, name)| {
            percent_encoding::percent_decode_str(name)
                .decode_utf8_lossy()
                .into_owned()
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("{} node", scheme));
    let authority = authority_and_name
        .map(|(authority, _)| authority)
        .unwrap_or(remainder);
    let address = authority
        .rsplit('@')
        .next()
        .unwrap_or(authority)
        .split([':', '?', '/'])
        .next()
        .unwrap_or("-")
        .to_string();
    let query = uri.split_once('?').map(|(_, query)| query).unwrap_or("");
    let transport = query
        .split('&')
        .find_map(|part| {
            part.strip_prefix("type=")
                .or_else(|| part.strip_prefix("network="))
        })
        .unwrap_or("Unknown")
        .to_string();
    let tls = query
        .split('&')
        .find_map(|part| part.strip_prefix("security="))
        .unwrap_or("Unknown")
        .to_string();
    Some(SubscriptionNode {
        name,
        protocol: scheme.to_string(),
        transport,
        tls,
        address,
    })
}

#[derive(Default)]
struct SubscriptionInfo {
    upload: u64,
    download: u64,
    total: u64,
    expire: Option<i64>,
    reset: Option<i64>,
}

fn parse_subscription_userinfo(value: &str) -> SubscriptionInfo {
    let mut info = SubscriptionInfo::default();
    for entry in value.split(';') {
        let Some((key, value)) = entry.trim().split_once('=') else {
            continue;
        };
        match key.trim() {
            "upload" => info.upload = value.trim().parse().unwrap_or_default(),
            "download" => info.download = value.trim().parse().unwrap_or_default(),
            "total" => info.total = value.trim().parse().unwrap_or_default(),
            "expire" => info.expire = value.trim().parse().ok(),
            "reset" => info.reset = value.trim().parse().ok(),
            _ => {}
        }
    }
    info
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut amount = bytes as f64;
    let mut unit = 0;
    while amount >= 1024.0 && unit < UNITS.len() - 1 {
        amount /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{amount:.2} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_subscription_userinfo_header() {
        let info = parse_subscription_userinfo(
            "upload=1024; download=2048; total=4096; expire=1800000000; reset=1700000000",
        );
        assert_eq!(info.upload, 1024);
        assert_eq!(info.download, 2048);
        assert_eq!(info.total, 4096);
        assert_eq!(info.expire, Some(1_800_000_000));
        assert_eq!(info.reset, Some(1_700_000_000));
        assert_eq!(format_bytes(info.upload), "1.00 KB");
    }

    #[test]
    fn counts_plain_proxy_uris_and_defaults_usable_count_to_total() {
        let raw = "ss://first\nvless://second\nthis is not a node\ntrojan://third";
        let stats = ConfiguredNodeCounter.statistics(raw);
        assert_eq!(stats.node_count, 3);
        assert_eq!(stats.usable_node_count, 3);
    }

    #[test]
    fn counts_base64_encoded_proxy_uris() {
        use base64::Engine;
        let encoded =
            base64::engine::general_purpose::STANDARD.encode("vmess://one\nhysteria2://two");
        let stats = ConfiguredNodeCounter.statistics(&encoded);
        assert_eq!(stats.node_count, 2);
        assert_eq!(stats.usable_node_count, 2);
    }

    #[test]
    fn decodes_percent_encoded_unicode_uri_names() {
        let nodes = parse_subscription_nodes(
            "vless://uuid@example.com:443?type=ws#%E5%8F%B0%E5%8C%97%20%E8%8A%82%E7%82%B9",
        );
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "台北 节点");
    }

    #[test]
    fn reads_the_country_code_from_the_node_flag() {
        let node = SubscriptionNode {
            name: "🇺🇸美国A1-小火箭".to_string(),
            protocol: "vless".to_string(),
            transport: "ws".to_string(),
            tls: "tls".to_string(),
            address: "example.com".to_string(),
        };
        assert_eq!(subscription_node_country_code(&node).as_deref(), Some("US"));

        let unflagged = SubscriptionNode {
            name: "Youtube去广告1".to_string(),
            ..node
        };
        assert_eq!(subscription_node_country_code(&unflagged), None);
    }

    #[test]
    fn counts_sing_box_and_xray_json_outbounds_but_ignores_direct_outbounds() {
        let sing_box = r#"{"outbounds":[
            {"type":"selector","tag":"Proxy","outbounds":["node-a"]},
            {"type":"vless","tag":"node-a"},
            {"type":"shadowsocks","tag":"node-b"},
            {"type":"direct","tag":"direct"},
            {"type":"block","tag":"block"}
        ]}"#;
        assert_eq!(count_proxy_nodes(sing_box), 2);

        let xray = r#"{"outbounds":[
            {"protocol":"vmess"},
            {"protocol":"freedom"},
            {"protocol":"trojan"}
        ]}"#;
        assert_eq!(count_proxy_nodes(xray), 2);
    }

    #[test]
    fn expands_each_xray_vnext_server_and_user_as_a_node() {
        let xray = r#"{"outbounds":[{"protocol":"vmess","tag":"subscription","settings":{"vnext":[
            {"address":"edge-a.example","users":[{"email":"node-a"},{"email":"node-b"}]},
            {"address":"edge-b.example","users":[{"email":"node-c"}]}
        ]},"streamSettings":{"network":"ws","security":"tls"}}]}"#;
        let nodes = parse_subscription_nodes(xray);
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].name, "node-a");
        assert_eq!(nodes[1].address, "edge-a.example");
        assert_eq!(nodes[2].name, "node-c");
    }

    #[test]
    fn counts_clash_yaml_proxy_entries() {
        let clash = "proxies:\n  - name: node-a\n    type: vless\n  - name: node-b\n    type: trojan\nproxy-groups:\n  - name: auto\n";
        assert_eq!(count_proxy_nodes(clash), 2);

        let inline = "proxies:\n  - {name: node-a, type: vless, server: edge.example, tls: true}\n  - {name: node-b, type: trojan, server: edge2.example}\n";
        let nodes = parse_subscription_nodes(inline);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].name, "node-a");
        assert_eq!(nodes[0].address, "edge.example");
    }
}
