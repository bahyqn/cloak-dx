use std::collections::HashMap;

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
    country_code_from_name(&node.name)
}

/// Country advertised by a node name, used when the name carries a flag.
pub fn country_code_from_name(name: &str) -> Option<String> {
    let mut indicators = String::with_capacity(2);
    for character in name.chars() {
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
    decode_base64_text(trimmed).unwrap_or_else(|| trimmed.to_string())
}

/// Decodes one base64 blob, accepting the padded and unpadded standard and
/// URL-safe alphabets clients use for subscriptions and vmess payloads.
fn decode_base64_text(value: &str) -> Option<String> {
    use base64::Engine;
    let value = value.trim();
    [
        &base64::engine::general_purpose::STANDARD,
        &base64::engine::general_purpose::URL_SAFE,
        &base64::engine::general_purpose::STANDARD_NO_PAD,
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ]
    .into_iter()
    .find_map(|engine| {
        engine
            .decode(value)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    })
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
        "anytls://",
        "socks://",
        "socks5://",
        "http://",
        "ssh://",
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

/// Protocols Xray-core can dial from a subscription node.
///
/// Subscription links use their own short names (`ss`) while Clash and
/// sing-box configs use the long ones (`shadowsocks`), so both forms are
/// accepted. Protocols such as Hysteria, TUIC, AnyTLS, ShadowsocksR and SSH
/// have no Xray outbound and would only fail at runtime.
pub fn xray_supports_protocol(protocol: &str) -> bool {
    matches!(
        protocol.to_ascii_lowercase().as_str(),
        "vless"
            | "vmess"
            | "trojan"
            | "ss"
            | "shadowsocks"
            | "socks"
            | "socks5"
            | "http"
            | "https"
            | "wireguard"
    )
}

/// Drops the nodes the selected engine cannot run, so the Proxies page never
/// offers a node that is guaranteed to fail its test or start. Engines without
/// a protocol restriction keep every parsed node.
pub fn filter_nodes_for_engine(
    engine: Option<&str>,
    nodes: Vec<SubscriptionNode>,
) -> Vec<SubscriptionNode> {
    match engine {
        Some("xray") => nodes
            .into_iter()
            .filter(|node| xray_supports_protocol(&node.protocol))
            .collect(),
        _ => nodes,
    }
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
            let protocol_key = protocol.to_ascii_lowercase();
            let stream = outbound.get("streamSettings");

            if let Some(servers) = outbound
                .get("settings")
                .and_then(|settings| settings.get("vnext"))
                .and_then(serde_json::Value::as_array)
            {
                let transport = node_transport_value(
                    &protocol_key,
                    stream
                        .and_then(|settings| settings.get("network"))
                        .and_then(serde_json::Value::as_str),
                );
                let tls = node_security_value(
                    &protocol_key,
                    stream
                        .and_then(|settings| settings.get("security"))
                        .and_then(serde_json::Value::as_str),
                );
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
            let transport = node_transport_value(
                &protocol_key,
                transport_options
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| {
                        stream
                            .and_then(|settings| settings.get("network"))
                            .and_then(serde_json::Value::as_str)
                    }),
            );
            let stream_security = stream
                .and_then(|settings| settings.get("security"))
                .and_then(serde_json::Value::as_str);
            let tls = match options.get("enabled").and_then(serde_json::Value::as_bool) {
                Some(true) => {
                    let reality = options
                        .get("reality")
                        .and_then(|reality| reality.get("enabled"))
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
                        || stream_security
                            .is_some_and(|value| value.eq_ignore_ascii_case("reality"));
                    if reality {
                        "reality".to_string()
                    } else {
                        "tls".to_string()
                    }
                }
                Some(false) => "None".to_string(),
                None => node_security_value(&protocol_key, stream_security),
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
                    let protocol_key = protocol.to_ascii_lowercase();
                    let stream = entry.get("streamSettings");
                    Some(SubscriptionNode {
                        name: entry
                            .get("tag")
                            .or_else(|| entry.get("remarks"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(protocol)
                            .to_string(),
                        protocol: protocol.to_string(),
                        transport: node_transport_value(
                            &protocol_key,
                            stream
                                .and_then(|settings| settings.get("network"))
                                .and_then(serde_json::Value::as_str),
                        ),
                        tls: node_security_value(
                            &protocol_key,
                            stream
                                .and_then(|settings| settings.get("security"))
                                .and_then(serde_json::Value::as_str),
                        ),
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
            let field = |key: &str| {
                map.get(serde_yaml::Value::String(key.to_string()))
                    .and_then(serde_yaml::Value::as_str)
            };
            let protocol = field("type").unwrap_or("Unknown").to_string();
            let protocol_key = protocol.to_ascii_lowercase();
            let has_reality_options = map
                .get(serde_yaml::Value::String("reality-opts".to_string()))
                .is_some();
            // Clash stores `tls` as a boolean, while some other YAML dialects
            // store the mode as a string. Reality is also signalled by the
            // presence of `reality-opts`.
            let tls = match map.get(serde_yaml::Value::String("tls".to_string())) {
                Some(serde_yaml::Value::Bool(true)) => if has_reality_options {
                    "reality"
                } else {
                    "tls"
                }
                .to_string(),
                Some(serde_yaml::Value::Bool(false)) => "None".to_string(),
                Some(serde_yaml::Value::String(value)) => normalize_security(value),
                _ if has_reality_options => "reality".to_string(),
                _ => default_security(&protocol_key).to_string(),
            };
            Some(SubscriptionNode {
                name: field("name").unwrap_or("Unknown").to_string(),
                protocol,
                transport: node_transport_value(&protocol_key, field("network")),
                tls,
                address: field("server").unwrap_or("-").to_string(),
            })
        })
        .collect()
}

pub(crate) fn parse_proxy_uri(uri: &str) -> Option<SubscriptionNode> {
    if !is_proxy_uri(uri) {
        return None;
    }
    let (scheme, remainder) = uri.split_once("://")?;
    // Keep the scheme as the visible protocol; only fold it for the parsing
    // rules. Node keys include the protocol, so rewriting `hy2` to
    // `hysteria2` here would drop the persisted selection of existing nodes.
    let protocol = scheme.to_ascii_lowercase();
    match protocol.as_str() {
        // These formats carry their fields outside the normal URI layout:
        // vmess uses base64 JSON, SS may base64-encode the whole address and
        // SSR base64-encodes a colon-separated payload.
        "vmess" => parse_vmess_uri(remainder).or_else(|| parse_standard_uri(&protocol, remainder)),
        "ss" => parse_ss_uri(remainder).or_else(|| parse_standard_uri(&protocol, remainder)),
        "ssr" => parse_ssr_uri(remainder).or_else(|| parse_standard_uri(&protocol, remainder)),
        _ => parse_standard_uri(&protocol, remainder),
    }
}

/// Splits an authority/query/fragment remainder without letting the fragment
/// leak into the last query parameter.
fn uri_sections(remainder: &str) -> (&str, &str, Option<&str>) {
    let (before_fragment, fragment) = remainder.split_once('#').unwrap_or((remainder, ""));
    let (authority, query) = before_fragment
        .split_once('?')
        .unwrap_or((before_fragment, ""));
    (authority, query, (!fragment.is_empty()).then_some(fragment))
}

fn decode_uri_name(fragment: Option<&str>) -> Option<String> {
    fragment
        .map(|name| {
            percent_encoding::percent_decode_str(name)
                .decode_utf8_lossy()
                .into_owned()
        })
        .filter(|name| !name.is_empty())
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter_map(|entry| {
            let (key, value) = entry.split_once('=').unwrap_or((entry, ""));
            let key = key.trim().to_ascii_lowercase();
            if key.is_empty() {
                return None;
            }
            let value = percent_encoding::percent_decode_str(value)
                .decode_utf8_lossy()
                .into_owned();
            Some((key, value))
        })
        .collect()
}

fn parse_standard_uri(protocol: &str, remainder: &str) -> Option<SubscriptionNode> {
    let (authority, query, fragment) = uri_sections(remainder);
    let params = parse_query(query);
    Some(SubscriptionNode {
        name: decode_uri_name(fragment).unwrap_or_else(|| format!("{protocol} node")),
        protocol: protocol.to_string(),
        transport: node_transport(protocol, &params),
        tls: node_security(protocol, &params),
        address: address_from_authority(authority),
    })
}

/// Reads the host from `userinfo@host:port`, including bracketed IPv6 hosts.
fn address_from_authority(authority: &str) -> String {
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, host_port)| host_port)
        .unwrap_or(authority);
    address_from_host_port(host_port)
}

fn address_from_host_port(host_port: &str) -> String {
    let host_port = host_port.split('/').next().unwrap_or(host_port);
    if let Some(rest) = host_port.strip_prefix('[') {
        return rest
            .split(']')
            .next()
            .filter(|host| !host.is_empty())
            .unwrap_or("-")
            .to_string();
    }
    match host_port.rsplit_once(':') {
        Some((host, port))
            if !host.is_empty() && port.chars().all(|character| character.is_ascii_digit()) =>
        {
            host.to_string()
        }
        _ if !host_port.is_empty() => host_port.to_string(),
        _ => "-".to_string(),
    }
}

/// Transport explicitly named by the link, or the protocol's own default.
fn node_transport(protocol: &str, params: &HashMap<String, String>) -> String {
    for key in ["type", "network", "net"] {
        if let Some(value) = params
            .get(key)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("none"))
        {
            return normalize_transport(value);
        }
    }
    default_transport(protocol).to_string()
}

fn default_transport(protocol: &str) -> &'static str {
    match protocol {
        "hysteria" | "hysteria2" | "hy2" | "tuic" => "quic",
        "wireguard" => "udp",
        _ => "tcp",
    }
}

/// Transport named by a config field, or the protocol's default when the
/// config leaves it out.
fn node_transport_value(protocol: &str, value: Option<&str>) -> String {
    match value
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("none"))
    {
        Some(value) => normalize_transport(value),
        None => default_transport(protocol).to_string(),
    }
}

fn normalize_transport(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        // sing-box calls plain TCP `raw`; Xray and other clients call it `tcp`.
        "raw" | "tcp" => "tcp".to_string(),
        "websocket" => "ws".to_string(),
        other => other.to_string(),
    }
}

/// Security explicitly named by the link or config. A protocol that only works
/// over TLS (Trojan, Hysteria2, TUIC, AnyTLS) defaults to `tls`; every other
/// protocol defaults to `None` when no TLS or Reality setting is present.
///
/// Fields such as `fp`, `sni` and `alpn` are not proof of TLS on their own:
/// VMess links often carry `fp` while `tls` stays empty, and showing `tls` for
/// those nodes is wrong.
fn node_security(protocol: &str, params: &HashMap<String, String>) -> String {
    if let Some(security) = params
        .get("security")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return normalize_security(security);
    }
    if let Some(tls) = params
        .get("tls")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        match tls.to_ascii_lowercase().as_str() {
            "0" | "false" | "none" | "off" => return "None".to_string(),
            "1" | "true" | "tls" => return "tls".to_string(),
            "reality" => return "reality".to_string(),
            other => return other.to_string(),
        }
    }
    if has_reality_params(params) {
        return "reality".to_string();
    }
    default_security(protocol).to_string()
}

fn normalize_security(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" | "0" | "false" | "off" => "None".to_string(),
        "1" | "true" => "tls".to_string(),
        other => other.to_string(),
    }
}

fn default_security(protocol: &str) -> &'static str {
    match protocol {
        "trojan" | "hysteria" | "hysteria2" | "hy2" | "tuic" | "anytls" => "tls",
        _ => "None",
    }
}

/// Security named by a config field, or the protocol's default when the config
/// leaves it out.
fn node_security_value(protocol: &str, value: Option<&str>) -> String {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => normalize_security(value),
        None => default_security(protocol).to_string(),
    }
}

/// Reality links identify themselves with a public key, short id or spider
/// path even when the `security` parameter is missing.
fn has_reality_params(params: &HashMap<String, String>) -> bool {
    ["pbk", "publickey", "sid", "shortid", "spx"]
        .iter()
        .any(|key| {
            params
                .get(*key)
                .is_some_and(|value| !value.trim().is_empty())
        })
}

fn parse_ss_uri(remainder: &str) -> Option<SubscriptionNode> {
    let (authority, query, fragment) = uri_sections(remainder);
    let params = parse_query(query);
    // SIP002 keeps the host after `@`; the older format base64-encodes
    // `method:password@host:port` as the whole authority.
    let address = if authority.contains('@') {
        address_from_authority(authority)
    } else {
        decode_base64_text(authority)
            .map(|decoded| {
                let host_port = decoded
                    .rsplit_once('@')
                    .map(|(_, host_port)| host_port)
                    .unwrap_or(&decoded);
                address_from_host_port(host_port)
            })
            .unwrap_or_else(|| address_from_authority(authority))
    };
    let plugin = params
        .get("plugin")
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    let transport = if plugin.contains("v2ray-plugin") && plugin.contains("mode=websocket") {
        "ws".to_string()
    } else {
        node_transport("ss", &params)
    };
    let tls = if plugin.contains("v2ray-plugin") && plugin.contains("tls") {
        "tls".to_string()
    } else {
        node_security("ss", &params)
    };
    Some(SubscriptionNode {
        name: decode_uri_name(fragment).unwrap_or_else(|| "ss node".to_string()),
        protocol: "ss".to_string(),
        transport,
        tls,
        address,
    })
}

fn parse_ssr_uri(remainder: &str) -> Option<SubscriptionNode> {
    let (authority, _, fragment) = uri_sections(remainder);
    let decoded = decode_base64_text(authority)?;
    let (main, query) = decoded
        .split_once("/?")
        .or_else(|| decoded.split_once('?'))
        .unwrap_or((decoded.as_str(), ""));
    let params = parse_query(query);
    let address = main
        .split(':')
        .next()
        .filter(|host| !host.is_empty())
        .unwrap_or("-")
        .to_string();
    let name = decode_uri_name(fragment)
        .or_else(|| {
            params
                .get("remarks")
                .and_then(|remarks| decode_base64_text(remarks))
        })
        .unwrap_or_else(|| "ssr node".to_string());
    Some(SubscriptionNode {
        name,
        protocol: "ssr".to_string(),
        transport: "tcp".to_string(),
        tls: "None".to_string(),
        address,
    })
}

/// Parses the base64 JSON payload a `vmess://` link carries.
///
/// Unlike the other schemes, vmess keeps the remark, address and transport in
/// an object (`ps`, `add`, `net`, `tls`) rather than in a `#fragment`. Reading
/// it only as a generic URI turns every node into `vmess node` with the base64
/// blob as its address, which loses names such as
/// `若无HY节点-更换客户端即可`.
fn parse_vmess_uri(payload: &str) -> Option<SubscriptionNode> {
    let (encoded, fragment) = payload.split_once('#').unwrap_or((payload, ""));
    let decoded = decode_base64_text(encoded)?;
    let value = serde_json::from_str::<serde_json::Value>(&decoded).ok()?;
    let field = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };

    // Reuse the URI transport/security rules for the fields v2rayN stores as
    // JSON. `tls` is the authoritative switch: when it is empty, the node has
    // no TLS even if fields such as `fp` are present.
    let mut params = HashMap::new();
    for key in [
        "type", "network", "net", "security", "tls", "pbk", "sid", "spx",
    ] {
        if let Some(value) = field(key) {
            params.insert(key.to_string(), value.to_string());
        }
    }
    let name = field("ps")
        .map(str::to_string)
        .or_else(|| {
            (!fragment.is_empty())
                .then(|| decode_uri_name(Some(fragment)))
                .flatten()
        })
        .unwrap_or_else(|| "vmess node".to_string());
    let address = field("add").unwrap_or("-").to_string();

    Some(SubscriptionNode {
        name,
        protocol: "vmess".to_string(),
        transport: node_transport("vmess", &params),
        tls: node_security("vmess", &params),
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
    fn reads_vmess_nodes_from_their_base64_payload() {
        use base64::Engine;
        let payload = r#"{"v":"2","ps":"若无HY节点-更换客户端即可 ","add":"edge.example.com","port":"443","net":"ws","tls":"tls","id":"uuid"}"#;
        let uri = format!(
            "vmess://{}",
            base64::engine::general_purpose::STANDARD.encode(payload)
        );

        let nodes = parse_subscription_nodes(&uri);
        assert_eq!(nodes.len(), 1);
        let node = &nodes[0];
        assert_eq!(node.name, "若无HY节点-更换客户端即可");
        assert_eq!(node.address, "edge.example.com");
        assert_eq!(node.transport, "ws");
        assert_eq!(node.tls, "tls");
    }

    #[test]
    fn vmess_payloads_without_a_remark_or_tls_use_tcp_and_none() {
        use base64::Engine;
        let payload = r#"{"add":"edge.example.com","port":"443"}"#;
        let uri = format!(
            "vmess://{}",
            base64::engine::general_purpose::STANDARD.encode(payload)
        );

        let node = parse_proxy_uri(&uri).unwrap();
        assert_eq!(node.name, "vmess node");
        assert_eq!(node.address, "edge.example.com");
        assert_eq!(node.transport, "tcp");
        assert_eq!(node.tls, "None");
    }

    #[test]
    fn vmess_without_tls_is_not_marked_tls_by_its_fingerprint() {
        use base64::Engine;
        // Real node from the `bp` subscription: ws on port 80, empty `tls`,
        // but a `fp` field that must not be mistaken for TLS.
        let payload = r#"{"v":"2","ps":"安卓-clashmeta客户端","add":"edge.example.com","port":"80","id":"uuid","aid":"0","net":"ws","type":"ws","host":"","path":"/","tls":"","fp":"safari"}"#;
        let uri = format!(
            "vmess://{}",
            base64::engine::general_purpose::STANDARD.encode(payload)
        );

        let node = parse_proxy_uri(&uri).unwrap();
        assert_eq!(node.name, "安卓-clashmeta客户端");
        assert_eq!(node.transport, "ws");
        assert_eq!(node.tls, "None");
    }

    #[test]
    fn tls_hints_without_a_security_setting_do_not_imply_tls() {
        let node =
            parse_proxy_uri("vless://uuid@edge.example.com:443?type=ws&sni=x&fp=safari#NoTls")
                .unwrap();
        assert_eq!(node.transport, "ws");
        assert_eq!(node.tls, "None");
    }

    #[test]
    fn trojan_websocket_defaults_to_tls_when_security_is_missing() {
        let node = parse_proxy_uri(
            "trojan://secret@tw.example.com:443?type=ws&path=%2Fws#台湾家宽2-Gemini",
        )
        .unwrap();

        assert_eq!(node.name, "台湾家宽2-Gemini");
        assert_eq!(node.protocol, "trojan");
        assert_eq!(node.address, "tw.example.com");
        assert_eq!(node.transport, "ws");
        // Trojan is a TLS protocol; clients show TLS even though this link
        // omits `security=tls`.
        assert_eq!(node.tls, "tls");
    }

    #[test]
    fn explicit_trojan_security_overrides_the_protocol_default() {
        let none =
            parse_proxy_uri("trojan://secret@plain.example.com:80?type=ws&security=none#Plain")
                .unwrap();
        assert_eq!(none.tls, "None");
        assert_eq!(none.transport, "ws");

        let tls =
            parse_proxy_uri("trojan://secret@secure.example.com:443?type=ws&security=tls#Secure")
                .unwrap();
        assert_eq!(tls.tls, "tls");
        // The fragment must not leak into the last query value.
        assert_eq!(tls.name, "Secure");
    }

    #[test]
    fn vless_reality_and_websocket_tls_are_distinguished() {
        let reality = parse_proxy_uri(
            "vless://uuid@reality.example.com:443?security=reality&type=tcp&pbk=pubkey&sid=abcd#Reality",
        )
        .unwrap();
        assert_eq!(reality.transport, "tcp");
        assert_eq!(reality.tls, "reality");

        let websocket = parse_proxy_uri(
            "vless://uuid@ws.example.com:443?security=tls&type=ws&path=%2Fws#WebSocket",
        )
        .unwrap();
        assert_eq!(websocket.transport, "ws");
        assert_eq!(websocket.tls, "tls");
    }

    #[test]
    fn vless_reality_is_inferred_from_reality_parameters() {
        let node = parse_proxy_uri(
            "vless://uuid@reality.example.com:443?type=tcp&pbk=pubkey&sid=abcd#Inferred",
        )
        .unwrap();
        assert_eq!(node.tls, "reality");
    }

    #[test]
    fn plain_trojan_uses_tcp_and_tls_defaults() {
        let node = parse_proxy_uri("trojan://secret@example.com:443#Plain").unwrap();
        assert_eq!(node.transport, "tcp");
        assert_eq!(node.tls, "tls");
    }

    #[test]
    fn bracketed_ipv6_hosts_are_parsed_without_the_brackets() {
        let node = parse_proxy_uri("vless://uuid@[2001:db8::1]:443?security=tls#IPv6").unwrap();
        assert_eq!(node.address, "2001:db8::1");
    }

    #[test]
    fn parses_legacy_base64_shadowsocks_addresses() {
        use base64::Engine;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode("aes-256-gcm:password@ss.example.com:8388");
        let uri = format!("ss://{payload}#SS");

        let node = parse_proxy_uri(&uri).unwrap();
        assert_eq!(node.name, "SS");
        assert_eq!(node.address, "ss.example.com");
        assert_eq!(node.protocol, "ss");
        assert_eq!(node.transport, "tcp");
        assert_eq!(node.tls, "None");
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
    fn xray_support_hides_protocols_it_cannot_dial() {
        let raw = "vless://uuid@a.example.com:443#VLESS\n\
                   vmess://eyJhZGQiOiJiLmV4YW1wbGUuY29tIn0=\n\
                   trojan://secret@c.example.com:443#Trojan\n\
                   ss://YWVzLTI1Ni1nY206cGFzc0BkLmV4YW1wbGUuY29tOjgzODg=#SS\n\
                   hysteria2://secret@e.example.com:443#HY2\n\
                   tuic://uuid@f.example.com:443#TUIC\n\
                   anytls://secret@g.example.com:443#AnyTLS\n\
                   ssr://c3NyLW5vZGU=\n";
        let nodes = parse_subscription_nodes(raw);
        let filtered = filter_nodes_for_engine(Some("xray"), nodes.clone());
        let protocols = filtered
            .iter()
            .map(|node| node.protocol.as_str())
            .collect::<Vec<_>>();
        assert_eq!(protocols, ["vless", "vmess", "trojan", "ss"]);

        // Another engine keeps every node it parsed.
        assert_eq!(
            filter_nodes_for_engine(Some("sing-box"), nodes.clone()).len(),
            8
        );
        assert_eq!(filter_nodes_for_engine(None, nodes).len(), 8);
    }

    #[test]
    fn xray_support_accepts_long_protocol_names_from_configs() {
        for protocol in [
            "vless",
            "VMess",
            "trojan",
            "ss",
            "shadowsocks",
            "socks",
            "socks5",
            "http",
            "https",
            "wireguard",
        ] {
            assert!(xray_supports_protocol(protocol), "{protocol}");
        }
        for protocol in [
            "hysteria",
            "hysteria2",
            "hy2",
            "tuic",
            "anytls",
            "ssr",
            "ssh",
        ] {
            assert!(!xray_supports_protocol(protocol), "{protocol}");
        }
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
        assert_eq!(nodes[0].transport, "tcp");
        assert_eq!(nodes[0].tls, "tls");
        assert_eq!(nodes[1].tls, "tls");
    }

    #[test]
    fn sing_box_json_reports_the_security_mode_instead_of_the_server_name() {
        let raw = r#"{"outbounds":[
            {"type":"trojan","tag":"trojan-ws","server":"edge.example","tls":{"enabled":true,"server_name":"sni.example"},"transport":{"type":"ws"}},
            {"type":"vless","tag":"reality","server":"reality.example","tls":{"enabled":true,"reality":{"enabled":true}}},
            {"type":"vmess","tag":"plain","server":"plain.example"}
        ]}"#;
        let nodes = parse_subscription_nodes(raw);
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].transport, "ws");
        assert_eq!(nodes[0].tls, "tls");
        assert_eq!(nodes[1].transport, "tcp");
        assert_eq!(nodes[1].tls, "reality");
        assert_eq!(nodes[2].transport, "tcp");
        assert_eq!(nodes[2].tls, "None");
    }
}
