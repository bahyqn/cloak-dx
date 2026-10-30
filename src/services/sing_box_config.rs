//! Generates the JSON configuration sing-box runs with.
//!
//! This module is intentionally independent from [`super::xray_config`]: each
//! engine parses subscription links into its own outbound format, and a change
//! meant for one core cannot alter the config the other one starts with.

use std::{collections::HashMap, net::IpAddr, path::Path};

use base64::Engine;
use percent_encoding::percent_decode_str;
use serde_json::{json, Map, Value};

use super::{
    geo_assets::geo_asset_directory,
    sing_box_settings::SingBoxSettings,
    subscription_manager::{
        decode_subscription, parse_proxy_uri, subscription_node_key, SubscriptionNode,
    },
};

/// The subset of a generated config the runtime needs after validation.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedConfig {
    pub config: Value,
    /// Loopback mixed inbound port (SOCKS and HTTP on the same port).
    pub mixed_port: u16,
    /// Mixed inbound exposed to the LAN, when enabled.
    pub lan_port: Option<u16>,
    pub node_name: Option<String>,
    /// Loopback Clash API port used for traffic totals and live switching.
    pub api_port: Option<u16>,
    /// Present when a selector carrying every supported node is used.
    pub hot_switch: Option<HotSwitch>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HotSwitch {
    pub api_port: u16,
    /// Tag of the selector outbound controlled through the Clash API.
    pub selector: String,
    pub selected_tag: String,
    pub nodes: Vec<HotNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HotNode {
    pub key: String,
    pub name: Option<String>,
    pub tag: String,
}

const PROXY_TAG: &str = "proxy";
const DIRECT_TAG: &str = "direct";
const NODE_TAG_PREFIX: &str = "cloak-node-";
const GEOSITE_CN_FILE: &str = "geosite-cn.srs";
const GEOIP_CN_FILE: &str = "geoip-cn.srs";
const GEOSITE_ADS_FILE: &str = "geosite-category-ads-all.srs";

struct NodeCandidate {
    key: String,
    name: String,
    outbound: Value,
}

/// Builds the config of the main core.
///
/// With an `api_port` every supported node is kept in the outbounds list behind
/// a selector, so the running process can switch nodes through the Clash API.
/// Without one (temporary test cores) only the selected node is emitted.
pub fn prepare_with_settings(
    raw_subscription: &str,
    selected_node_key: Option<&str>,
    mixed_port: u16,
    lan_port: Option<u16>,
    api_port: Option<u16>,
    settings: &SingBoxSettings,
) -> Result<PreparedConfig, String> {
    let payload = decode_subscription(raw_subscription);
    let mut candidates = collect_candidates(&payload);
    apply_outbound_options(&mut candidates, settings);
    if candidates.is_empty() {
        return Err("The subscription has no nodes that sing-box can run.".to_string());
    }

    let selected_index = match selected_node_key {
        Some(key) => candidates
            .iter()
            .position(|node| node.key == key)
            .ok_or_else(|| "The selected node cannot be run by sing-box.".to_string())?,
        None => 0,
    };
    let node_name = Some(candidates[selected_index].name.clone());

    let (outbounds, hot_switch) = match api_port {
        Some(api_port) => {
            let mut outbounds = Vec::with_capacity(candidates.len() + 2);
            let mut hot_nodes = Vec::with_capacity(candidates.len());
            let mut selected_tag = String::new();
            for (index, node) in candidates.iter_mut().enumerate() {
                let tag = format!("{NODE_TAG_PREFIX}{index}");
                if index == selected_index {
                    selected_tag = tag.clone();
                }
                node.outbound["tag"] = json!(tag);
                hot_nodes.push(HotNode {
                    key: node.key.clone(),
                    name: Some(node.name.clone()),
                    tag: tag.clone(),
                });
                outbounds.push(node.outbound.clone());
            }

            let group = if settings.auto_failover {
                json!({
                    "type": "urltest",
                    "tag": PROXY_TAG,
                    "outbounds": hot_nodes.iter().map(|node| node.tag.clone()).collect::<Vec<_>>(),
                    "url": "https://www.gstatic.com/generate_204",
                    "interval": "3m",
                    "tolerance": 50
                })
            } else {
                json!({
                    "type": "selector",
                    "tag": PROXY_TAG,
                    "outbounds": hot_nodes.iter().map(|node| node.tag.clone()).collect::<Vec<_>>(),
                    "default": selected_tag
                })
            };
            outbounds.push(group);
            outbounds.push(json!({"type": "direct", "tag": DIRECT_TAG}));

            let hot_switch = (!settings.auto_failover).then(|| HotSwitch {
                api_port,
                selector: PROXY_TAG.to_string(),
                selected_tag,
                nodes: hot_nodes,
            });
            (outbounds, hot_switch)
        }
        None => {
            let mut outbound = candidates.swap_remove(selected_index).outbound;
            outbound["tag"] = json!(PROXY_TAG);
            (
                vec![outbound, json!({"type": "direct", "tag": DIRECT_TAG})],
                None,
            )
        }
    };

    Ok(PreparedConfig {
        config: build_config(
            settings,
            outbounds,
            mixed_port,
            lan_port,
            api_port,
            settings.routing_mode.as_str(),
        ),
        mixed_port,
        lan_port,
        node_name,
        api_port,
        hot_switch,
    })
}

/// Builds a minimal config for a single node test.
///
/// DNS, TUN, routing and the Clash API are omitted so dozens of temporary
/// cores can run side by side with no shared state or extra privileges.
pub fn prepare_for_test(
    raw_subscription: &str,
    selected_node_key: Option<&str>,
    mixed_port: u16,
    settings: &SingBoxSettings,
) -> Result<PreparedConfig, String> {
    let payload = decode_subscription(raw_subscription);
    let mut candidates = collect_candidates(&payload);
    apply_outbound_options(&mut candidates, settings);
    if candidates.is_empty() {
        return Err("The subscription has no nodes that sing-box can run.".to_string());
    }
    let index = match selected_node_key {
        Some(key) => candidates
            .iter()
            .position(|node| node.key == key)
            .ok_or_else(|| "The selected node cannot be run by sing-box.".to_string())?,
        None => 0,
    };
    let mut selected = candidates.swap_remove(index);
    selected.outbound["tag"] = json!(PROXY_TAG);
    let node_name = Some(selected.name);
    let config = json!({
        "log": {"level": settings.log_level, "timestamp": settings.log_timestamp},
        "dns": {
            "servers": [{"type": "local", "tag": "dns-local"}],
            "final": "dns-local"
        },
        "inbounds": [{
            "type": "mixed",
            "tag": "mixed-in",
            "listen": "127.0.0.1",
            "listen_port": mixed_port
        }],
        "outbounds": [
            selected.outbound,
            {"type": "direct", "tag": DIRECT_TAG}
        ],
        "route": {"final": PROXY_TAG, "default_domain_resolver": "dns-local"}
    });
    Ok(PreparedConfig {
        config,
        mixed_port,
        lan_port: None,
        node_name,
        api_port: None,
        hot_switch: None,
    })
}

fn build_config(
    settings: &SingBoxSettings,
    outbounds: Vec<Value>,
    mixed_port: u16,
    lan_port: Option<u16>,
    api_port: Option<u16>,
    mode: &str,
) -> Value {
    let mut inbounds = vec![json!({
        "type": "mixed",
        "tag": "mixed-in",
        "listen": "127.0.0.1",
        "listen_port": mixed_port
    })];

    if let Some(lan_port) = lan_port {
        let mut inbound = json!({
            "type": "mixed",
            "tag": "lan-in",
            "listen": "0.0.0.0",
            "listen_port": lan_port
        });
        if !settings.lan_username.trim().is_empty() && !settings.lan_password.is_empty() {
            inbound["users"] = json!([{
                "username": settings.lan_username.trim(),
                "password": settings.lan_password
            }]);
        }
        inbounds.push(inbound);
    }

    if settings.tun_enabled {
        let mut tun = json!({
            "type": "tun",
            "tag": "tun-in",
            "interface_name": settings.tun_name,
            "address": [settings.tun_address],
            "mtu": settings.tun_mtu,
            "stack": settings.tun_stack,
            "auto_route": settings.tun_auto_route,
            "strict_route": settings.tun_strict_route
        });
        // auto_redirect is rejected unless auto_route is also on.
        if settings.tun_auto_redirect && settings.tun_auto_route {
            tun["auto_redirect"] = json!(true);
        }
        inbounds.push(tun);
    }

    let rule_sets = route_rule_sets(settings);
    let rules = route_rules(settings, mode, &rule_sets);
    let final_outbound = match mode {
        "direct" => DIRECT_TAG,
        _ => PROXY_TAG,
    };
    let mut route = json!({
        "rules": rules,
        "final": final_outbound,
        "auto_detect_interface": true,
        // sing-box 1.14 rejects outbound domains when neither this nor a
        // per-outbound `domain_resolver` is set. Resolving proxy server names
        // locally avoids a chicken-and-egg lookup through the proxy itself.
        "default_domain_resolver": "dns-local"
    });
    if !rule_sets.is_empty() {
        route["rule_set"] = Value::Array(rule_sets.clone());
    }

    let mut config = Map::new();
    config.insert(
        "log".to_string(),
        json!({"level": settings.log_level, "timestamp": settings.log_timestamp}),
    );
    config.insert("dns".to_string(), build_dns(settings, mode, &rule_sets));
    config.insert("inbounds".to_string(), Value::Array(inbounds));
    config.insert("outbounds".to_string(), Value::Array(outbounds));
    config.insert("route".to_string(), route);

    let mut experimental = Map::new();
    if let Some(api_port) = api_port {
        experimental.insert(
            "clash_api".to_string(),
            json!({"external_controller": format!("127.0.0.1:{api_port}")}),
        );
    }
    if settings.cache_file_enabled {
        if let Ok(directory) = super::proxy_core::runtime_directory() {
            experimental.insert(
                "cache_file".to_string(),
                json!({
                    "enabled": true,
                    "path": directory.join("sing-box-cache.db").to_string_lossy()
                }),
            );
        }
    }
    if !experimental.is_empty() {
        config.insert("experimental".to_string(), Value::Object(experimental));
    }
    Value::Object(config)
}

fn build_dns(settings: &SingBoxSettings, mode: &str, rule_sets: &[Value]) -> Value {
    let mut servers = Vec::new();
    let local_tag = "dns-local";
    if settings.local_dns.trim().is_empty() {
        servers.push(json!({"type": "local", "tag": local_tag}));
    } else {
        let mut server = dns_server_from_url(local_tag, settings.local_dns.trim());
        // The local resolver must not be sent back through the proxy.
        if let Some(server) = server.as_object_mut() {
            server.remove("detour");
        }
        // A resolver reached by hostname needs a resolver of its own; using
        // itself would loop, so a plain system server bootstraps it.
        if dns_server_needs_bootstrap(&server) {
            servers.push(json!({"type": "local", "tag": "dns-bootstrap"}));
            server["domain_resolver"] = json!("dns-bootstrap");
        }
        servers.push(server);
    }

    let remote_tag = if settings.remote_dns.trim().is_empty() {
        None
    } else {
        let mut server = dns_server_from_url("dns-remote", settings.remote_dns.trim());
        if mode != "direct" {
            server["detour"] = json!(PROXY_TAG);
        }
        servers.push(server);
        Some("dns-remote")
    };

    if settings.fake_dns_enabled {
        servers.push(json!({
            "type": "fakeip",
            "tag": "dns-fakeip",
            "inet4_range": settings.fake_dns_ipv4_pool,
            "inet6_range": settings.fake_dns_ipv6_pool
        }));
    }

    let mut rules = Vec::new();
    if settings.bypass_cn {
        if rule_sets.iter().any(|set| set["tag"] == "geosite-cn") {
            rules.push(json!({"rule_set": ["geosite-cn"], "server": local_tag}));
        }
        rules.push(json!({"domain_suffix": [".cn"], "server": local_tag}));
    }
    // FakeIP is only useful for domains that leave through the proxy; direct
    // domains were already sent to the local resolver above.
    if settings.fake_dns_enabled && mode != "direct" {
        rules.push(json!({"query_type": ["A", "AAAA"], "server": "dns-fakeip"}));
    }

    let dns = json!({
        "servers": servers,
        "rules": rules,
        "final": remote_tag.unwrap_or(local_tag),
        "strategy": if settings.ipv6_enabled { settings.dns_strategy.as_str() } else { "ipv4_only" }
    });
    dns
}

fn dns_server_from_url(tag: &str, raw: &str) -> Value {
    let (scheme, rest) = match raw.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
        None => (String::new(), raw),
    };
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = split_host_port_or_default(authority, dns_default_port(&scheme));
    match scheme.as_str() {
        "https" | "h3" | "quic" | "tls" | "tcp" | "udp" => {
            let mut server = json!({
                "type": scheme,
                "tag": tag,
                "server": host,
                "server_port": port
            });
            if matches!(scheme.as_str(), "https" | "h3" | "quic" | "tls") {
                server["tls"] = json!({"enabled": true, "server_name": host});
            }
            if matches!(scheme.as_str(), "https" | "h3") {
                server["path"] = json!(if path.is_empty() {
                    "/dns-query".to_string()
                } else {
                    format!("/{path}")
                });
            }
            server
        }
        _ => json!({
            "type": "udp",
            "tag": tag,
            "server": host,
            "server_port": port
        }),
    }
}

fn dns_server_needs_bootstrap(server: &Value) -> bool {
    server
        .get("server")
        .and_then(Value::as_str)
        .is_some_and(|host| host.parse::<std::net::IpAddr>().is_err())
}

fn dns_default_port(scheme: &str) -> u16 {
    match scheme {
        "tls" | "quic" => 853,
        "https" | "h3" => 443,
        _ => 53,
    }
}

fn route_rule_sets(settings: &SingBoxSettings) -> Vec<Value> {
    let directory = geo_asset_directory();
    let mut sets = Vec::new();
    if settings.bypass_cn {
        push_local_rule_set(&mut sets, &directory, "geosite-cn", GEOSITE_CN_FILE);
        push_local_rule_set(&mut sets, &directory, "geoip-cn", GEOIP_CN_FILE);
    }
    if settings.block_ads {
        push_local_rule_set(&mut sets, &directory, "geosite-ads", GEOSITE_ADS_FILE);
    }
    sets
}

fn push_local_rule_set(sets: &mut Vec<Value>, directory: &Path, tag: &str, file: &str) {
    let path = directory.join(file);
    if !path.is_file() {
        return;
    }
    sets.push(json!({
        "type": "local",
        "tag": tag,
        "format": "binary",
        "path": path.to_string_lossy()
    }));
}

fn route_rules(settings: &SingBoxSettings, mode: &str, rule_sets: &[Value]) -> Vec<Value> {
    let mut rules = Vec::new();
    if settings.sniffing_enabled {
        rules.push(json!({"action": "sniff", "timeout": "300ms"}));
    }
    // DNS queries are answered by the DNS module even when a client reaches
    // it through TUN; sniffing is not required for the well-known port.
    rules.push(json!({"port": [53], "action": "hijack-dns"}));

    if mode == "rules" {
        rules.extend(custom_rules(&settings.custom_rules));
        if settings.block_quic {
            rules.push(json!({
                "network": ["udp"],
                "port": [443],
                "action": "reject"
            }));
        }
        if settings.block_ads && rule_sets.iter().any(|set| set["tag"] == "geosite-ads") {
            rules.push(json!({
                "rule_set": ["geosite-ads"],
                "action": "reject"
            }));
        }
        if settings.bypass_private {
            rules.push(json!({
                "ip_is_private": true,
                "action": "route",
                "outbound": DIRECT_TAG
            }));
        }
        if settings.bypass_cn {
            let mut rule = Map::new();
            let mut matches = Vec::new();
            if rule_sets.iter().any(|set| set["tag"] == "geosite-cn") {
                matches.push("geosite-cn");
            }
            if rule_sets.iter().any(|set| set["tag"] == "geoip-cn") {
                matches.push("geoip-cn");
            }
            if !matches.is_empty() {
                rule.insert("rule_set".to_string(), json!(matches));
            }
            rule.insert("domain_suffix".to_string(), json!([".cn"]));
            rule.insert("action".to_string(), json!("route"));
            rule.insert("outbound".to_string(), json!(DIRECT_TAG));
            rules.push(Value::Object(rule));
        }
    }
    rules
}

fn custom_rules(text: &str) -> Vec<Value> {
    let mut rules = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(destination) = parts.next() else {
            continue;
        };
        let Some(target) = parts.next() else {
            continue;
        };
        let mut rule = Map::new();
        if destination.contains('/') && destination.parse::<IpAddr>().is_err() {
            rule.insert("ip_cidr".to_string(), json!([destination]));
        } else if let Ok(address) = destination.parse::<IpAddr>() {
            let suffix = if address.is_ipv4() { "32" } else { "128" };
            rule.insert(
                "ip_cidr".to_string(),
                json!([format!("{address}/{suffix}")]),
            );
        } else {
            rule.insert("domain_suffix".to_string(), json!([destination]));
        }
        match target {
            "direct" => {
                rule.insert("action".to_string(), json!("route"));
                rule.insert("outbound".to_string(), json!(DIRECT_TAG));
            }
            "proxy" => {
                rule.insert("action".to_string(), json!("route"));
                rule.insert("outbound".to_string(), json!(PROXY_TAG));
            }
            "block" => {
                rule.insert("action".to_string(), json!("reject"));
            }
            _ => continue,
        }
        rules.push(Value::Object(rule));
    }
    rules
}

fn apply_outbound_options(candidates: &mut [NodeCandidate], settings: &SingBoxSettings) {
    for node in candidates {
        if settings.tcp_fast_open {
            node.outbound["tcp_fast_open"] = json!(true);
        }
        let supports_mux = matches!(
            node.outbound["type"].as_str(),
            Some("vless" | "vmess" | "trojan" | "shadowsocks" | "socks" | "http")
        );
        if settings.mux_enabled && supports_mux {
            node.outbound["multiplex"] = json!({
                "enabled": true,
                "protocol": settings.mux_protocol,
                "max_connections": settings.mux_max_connections,
                "padding": settings.mux_padding
            });
        }
    }
}

fn collect_candidates(payload: &str) -> Vec<NodeCandidate> {
    if let Ok(value) = serde_json::from_str::<Value>(payload) {
        return json_candidates(&value);
    }

    let mut candidates = Vec::new();
    for line in payload.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        if let Some(candidate) = uri_candidate(line) {
            candidates.push(candidate);
        }
    }
    if !candidates.is_empty() {
        return candidates;
    }

    yaml_candidates(payload)
}

fn json_candidates(value: &Value) -> Vec<NodeCandidate> {
    let entries = value
        .get("outbounds")
        .or_else(|| value.get("outbound"))
        .and_then(Value::as_array)
        .or_else(|| value.as_array());
    let Some(entries) = entries else {
        return Vec::new();
    };

    let mut candidates = Vec::new();
    for entry in entries {
        if entry.get("type").and_then(Value::as_str).is_some() {
            if let Some(candidate) = sing_box_json_candidate(entry) {
                candidates.push(candidate);
            }
        } else if entry.get("protocol").and_then(Value::as_str).is_some() {
            candidates.extend(xray_json_candidates(entry));
        }
    }
    candidates
}

fn sing_box_json_candidate(entry: &Value) -> Option<NodeCandidate> {
    let original_protocol = entry.get("type")?.as_str()?;
    let protocol = original_protocol.to_ascii_lowercase();
    if !is_sing_box_protocol(&protocol) {
        return None;
    }
    let address = entry
        .get("server")
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    if address.is_empty() {
        return None;
    }
    // `parse_subscription_nodes` derives the persisted node key from the same
    // tag/address pair, so both sides must name the node identically.
    let name = entry
        .get("tag")
        .or_else(|| entry.get("remarks"))
        .and_then(Value::as_str)
        .filter(|tag| !tag.is_empty())
        .unwrap_or(&address)
        .to_string();
    let key = node_key(original_protocol, &name, &address);
    let mut outbound = entry.clone();
    outbound["type"] = json!(protocol);
    Some(NodeCandidate {
        key,
        name,
        outbound,
    })
}

fn xray_json_candidates(entry: &Value) -> Vec<NodeCandidate> {
    let Some(original_protocol) = entry.get("protocol").and_then(Value::as_str) else {
        return Vec::new();
    };
    let protocol = original_protocol.to_ascii_lowercase();
    let stream = entry.get("streamSettings");
    let (tls, transport) = match xray_stream_to_sing_box(&protocol, stream) {
        Ok(parts) => parts,
        Err(()) => return Vec::new(),
    };
    let settings = entry.get("settings").unwrap_or(&Value::Null);

    let mut candidates = Vec::new();
    if let Some(servers) = settings.get("vnext").and_then(Value::as_array) {
        for server in servers {
            let Some(address) = server.get("address").and_then(Value::as_str) else {
                continue;
            };
            let Some(port) = json_port(server.get("port")) else {
                continue;
            };
            let users = server
                .get("users")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if users.is_empty() {
                continue;
            }
            for user in users {
                let mut outbound = Map::new();
                outbound.insert("type".to_string(), json!(protocol));
                outbound.insert("server".to_string(), json!(address));
                outbound.insert("server_port".to_string(), json!(port));
                match protocol.as_str() {
                    "vless" => {
                        let Some(id) = user.get("id").and_then(Value::as_str) else {
                            continue;
                        };
                        outbound.insert("uuid".to_string(), json!(id));
                        if let Some(flow) = user.get("flow").and_then(Value::as_str) {
                            outbound.insert("flow".to_string(), json!(flow));
                        }
                        if let Some(encoding) = user.get("packetEncoding").and_then(Value::as_str) {
                            outbound.insert("packet_encoding".to_string(), json!(encoding));
                        }
                    }
                    "vmess" => {
                        let Some(id) = user.get("id").and_then(Value::as_str) else {
                            continue;
                        };
                        outbound.insert("uuid".to_string(), json!(id));
                        outbound.insert(
                            "security".to_string(),
                            json!(user
                                .get("security")
                                .and_then(Value::as_str)
                                .unwrap_or("auto")),
                        );
                        outbound.insert(
                            "alter_id".to_string(),
                            json!(user.get("alterId").and_then(Value::as_u64).unwrap_or(0)),
                        );
                        if let Some(encoding) = user.get("packetEncoding").and_then(Value::as_str) {
                            outbound.insert("packet_encoding".to_string(), json!(encoding));
                        }
                    }
                    _ => continue,
                }
                if let Some(tls) = tls.clone() {
                    outbound.insert("tls".to_string(), tls);
                }
                if let Some(transport) = transport.clone() {
                    outbound.insert("transport".to_string(), transport);
                }
                let name = user
                    .get("email")
                    .and_then(Value::as_str)
                    .filter(|email| !email.is_empty())
                    .unwrap_or(address)
                    .to_string();
                let key = node_key(original_protocol, &name, address);
                candidates.push(NodeCandidate {
                    key,
                    name,
                    outbound: Value::Object(outbound),
                });
            }
        }
        return candidates;
    }

    if let Some(servers) = settings.get("servers").and_then(Value::as_array) {
        for server in servers {
            let Some(address) = server.get("address").and_then(Value::as_str) else {
                continue;
            };
            let Some(port) = json_port(server.get("port")) else {
                continue;
            };
            let mut outbound = Map::new();
            outbound.insert("type".to_string(), json!(protocol));
            outbound.insert("server".to_string(), json!(address));
            outbound.insert("server_port".to_string(), json!(port));
            match protocol.as_str() {
                "trojan" => {
                    let Some(password) = server.get("password").and_then(Value::as_str) else {
                        continue;
                    };
                    outbound.insert("password".to_string(), json!(password));
                }
                "shadowsocks" | "ss" => {
                    let Some(method) = server.get("method").and_then(Value::as_str) else {
                        continue;
                    };
                    let Some(password) = server.get("password").and_then(Value::as_str) else {
                        continue;
                    };
                    outbound.insert("type".to_string(), json!("shadowsocks"));
                    outbound.insert("method".to_string(), json!(method));
                    outbound.insert("password".to_string(), json!(password));
                }
                "socks" | "socks5" | "http" | "https" => {
                    let outbound_type = if protocol.starts_with("socks") {
                        "socks"
                    } else {
                        "http"
                    };
                    outbound.insert("type".to_string(), json!(outbound_type));
                    if let Some(users) = server.get("users").and_then(Value::as_array) {
                        if let Some(user) = users.first() {
                            if let Some(username) = user
                                .get("user")
                                .or_else(|| user.get("username"))
                                .and_then(Value::as_str)
                            {
                                outbound.insert("username".to_string(), json!(username));
                            }
                            if let Some(password) = user
                                .get("pass")
                                .or_else(|| user.get("password"))
                                .and_then(Value::as_str)
                            {
                                outbound.insert("password".to_string(), json!(password));
                            }
                        }
                    }
                }
                _ => continue,
            }
            if let Some(tls) = tls.clone() {
                outbound.insert("tls".to_string(), tls);
            }
            if let Some(transport) = transport.clone() {
                outbound.insert("transport".to_string(), transport);
            }
            let name = entry
                .get("tag")
                .or_else(|| entry.get("remarks"))
                .and_then(Value::as_str)
                .filter(|tag| !tag.is_empty())
                .unwrap_or(address)
                .to_string();
            let key = node_key(original_protocol, &name, address);
            candidates.push(NodeCandidate {
                key,
                name,
                outbound: Value::Object(outbound),
            });
        }
    }
    candidates
}

fn json_port(value: Option<&Value>) -> Option<u16> {
    match value? {
        Value::Number(number) => number.as_u64().and_then(|port| u16::try_from(port).ok()),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

fn xray_stream_to_sing_box(
    protocol: &str,
    stream: Option<&Value>,
) -> Result<(Option<Value>, Option<Value>), ()> {
    let Some(stream) = stream else {
        return Ok((None, None));
    };
    let network = stream
        .get("network")
        .and_then(Value::as_str)
        .unwrap_or("tcp")
        .to_ascii_lowercase();
    let transport = match network.as_str() {
        "tcp" | "raw" => None,
        "ws" => {
            let options = stream.get("wsSettings").unwrap_or(&Value::Null);
            let mut transport = json!({"type": "ws"});
            if let Some(path) = options.get("path").and_then(Value::as_str) {
                transport["path"] = json!(path);
            }
            if let Some(host) = options
                .get("headers")
                .and_then(|headers| headers.get("Host"))
                .and_then(Value::as_str)
            {
                transport["headers"] = json!({"Host": host});
            }
            Some(transport)
        }
        "grpc" => {
            let service = stream
                .get("grpcSettings")
                .and_then(|options| options.get("serviceName"))
                .and_then(Value::as_str)
                .unwrap_or("");
            Some(json!({"type": "grpc", "service_name": service}))
        }
        "http" | "h2" => {
            let options = stream.get("httpSettings").unwrap_or(&Value::Null);
            let mut transport = json!({"type": "http"});
            if let Some(path) = options.get("path").and_then(Value::as_str) {
                transport["path"] = json!(path);
            }
            if let Some(hosts) = options.get("host").and_then(Value::as_array) {
                transport["host"] = Value::Array(hosts.clone());
            }
            Some(transport)
        }
        "httpupgrade" => {
            let options = stream.get("httpupgradeSettings").unwrap_or(&Value::Null);
            let mut transport = json!({"type": "httpupgrade"});
            if let Some(path) = options.get("path").and_then(Value::as_str) {
                transport["path"] = json!(path);
            }
            if let Some(host) = options.get("host").and_then(Value::as_str) {
                transport["host"] = json!(host);
            }
            Some(transport)
        }
        _ => return Err(()),
    };

    let security = stream
        .get("security")
        .and_then(Value::as_str)
        .unwrap_or("none")
        .to_ascii_lowercase();
    let tls = match security.as_str() {
        "tls" | "reality" => {
            let options = stream
                .get(if security == "reality" {
                    "realitySettings"
                } else {
                    "tlsSettings"
                })
                .unwrap_or(&Value::Null);
            let mut tls = Map::new();
            tls.insert("enabled".to_string(), json!(true));
            if let Some(server_name) = options.get("serverName").and_then(Value::as_str) {
                tls.insert("server_name".to_string(), json!(server_name));
            }
            if options
                .get("allowInsecure")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                tls.insert("insecure".to_string(), json!(true));
            }
            if let Some(alpn) = options.get("alpn").and_then(Value::as_array) {
                tls.insert("alpn".to_string(), Value::Array(alpn.clone()));
            }
            if let Some(fingerprint) = options.get("fingerprint").and_then(Value::as_str) {
                tls.insert(
                    "utls".to_string(),
                    json!({"enabled": true, "fingerprint": fingerprint}),
                );
            }
            if security == "reality" {
                let mut reality = Map::new();
                reality.insert("enabled".to_string(), json!(true));
                if let Some(public_key) = options.get("publicKey").and_then(Value::as_str) {
                    reality.insert("public_key".to_string(), json!(public_key));
                }
                if let Some(short_id) = options.get("shortId").and_then(Value::as_str) {
                    reality.insert("short_id".to_string(), json!(short_id));
                }
                tls.insert("reality".to_string(), Value::Object(reality));
                ensure_utls_for_reality(&mut tls);
            }
            Some(Value::Object(tls))
        }
        _ if matches!(protocol, "trojan" | "hysteria2" | "tuic" | "anytls") => Some(json!({
            "enabled": true
        })),
        _ => None,
    };
    Ok((tls, transport))
}

fn uri_candidate(uri: &str) -> Option<NodeCandidate> {
    let display = parse_proxy_uri(uri)?;
    let outbound = outbound_from_uri(uri).ok()?;
    let key = subscription_node_key(&display);
    Some(NodeCandidate {
        key,
        name: display.name,
        outbound,
    })
}

fn outbound_from_uri(uri: &str) -> Result<Value, String> {
    let (scheme, _) = uri
        .split_once("://")
        .ok_or_else(|| format!("Invalid proxy link: {uri}"))?;
    match scheme.to_ascii_lowercase().as_str() {
        "vless" => vless_outbound(uri),
        "vmess" => vmess_outbound(uri),
        "trojan" => trojan_outbound(uri),
        "ss" => shadowsocks_outbound(uri),
        "hysteria2" | "hy2" => hysteria2_outbound(uri),
        "tuic" => tuic_outbound(uri),
        "anytls" => anytls_outbound(uri),
        "socks" | "socks5" => socks_outbound(uri),
        "http" | "https" => http_outbound(uri),
        other => Err(format!("{other} nodes are not supported by sing-box.")),
    }
}

struct LinkParts {
    userinfo: String,
    host: String,
    port: u16,
    query: HashMap<String, String>,
}

fn parse_link(uri: &str) -> Result<LinkParts, String> {
    let (_, rest) = uri
        .split_once("://")
        .ok_or_else(|| format!("Invalid proxy link: {uri}"))?;
    // The `#fragment` is the display name of the original link; node names come
    // from the subscription payload instead, so it can be discarded.
    let (rest, _) = rest.split_once('#').unwrap_or((rest, ""));
    let (authority, query_text) = rest.split_once('?').unwrap_or((rest, ""));
    let (userinfo, host_port) = match authority.rsplit_once('@') {
        Some((userinfo, host_port)) => (
            percent_decode_str(userinfo)
                .decode_utf8_lossy()
                .into_owned(),
            host_port,
        ),
        None => (String::new(), authority),
    };
    let (host, port) = split_host_port(host_port)?;
    Ok(LinkParts {
        userinfo,
        host,
        port,
        query: parse_query(query_text),
    })
}

fn split_host_port(authority: &str) -> Result<(String, u16), String> {
    split_host_port_or_default_result(authority, None)
}

fn split_host_port_or_default(authority: &str, default_port: u16) -> (String, u16) {
    split_host_port_or_default_result(authority, Some(default_port))
        .unwrap_or_else(|_| (authority.to_string(), default_port))
}

fn split_host_port_or_default_result(
    authority: &str,
    default_port: Option<u16>,
) -> Result<(String, u16), String> {
    let authority = authority.split('/').next().unwrap_or(authority);
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest
            .split_once(']')
            .ok_or_else(|| format!("Invalid IPv6 address in proxy link: {authority}"))?;
        let port = match after.strip_prefix(':') {
            Some(port) => port
                .parse()
                .map_err(|_| format!("Invalid port in proxy link: {authority}"))?,
            None => {
                default_port.ok_or_else(|| format!("Missing port in proxy link: {authority}"))?
            }
        };
        return Ok((host.to_string(), port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|character| character.is_ascii_digit()) => Ok((
            host.to_string(),
            port.parse()
                .map_err(|_| format!("Invalid port in proxy link: {authority}"))?,
        )),
        _ => default_port
            .map(|port| (authority.to_string(), port))
            .ok_or_else(|| format!("Missing port in proxy link: {authority}")),
    }
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            (
                percent_decode_str(key)
                    .decode_utf8_lossy()
                    .to_ascii_lowercase(),
                percent_decode_str(value).decode_utf8_lossy().into_owned(),
            )
        })
        .collect()
}

fn query<'a>(query: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    query
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
}

fn query_bool(query: &HashMap<String, String>, keys: &[&str]) -> bool {
    keys.iter().any(|key| {
        query
            .get(*key)
            .is_some_and(|value| matches!(value.as_str(), "1" | "true"))
    })
}

fn vless_outbound(uri: &str) -> Result<Value, String> {
    let link = parse_link(uri)?;
    if link.userinfo.is_empty() {
        return Err("VLESS link is missing its user id.".to_string());
    }
    let mut outbound = json!({
        "type": "vless",
        "server": link.host,
        "server_port": link.port,
        "uuid": link.userinfo
    });
    if let Some(flow) = query(&link.query, "flow") {
        outbound["flow"] = json!(flow);
    }
    if let Some(encoding) = query(&link.query, "packetencoding") {
        outbound["packet_encoding"] = json!(if encoding == "xudp" {
            "xudp"
        } else {
            "packetaddr"
        });
    }
    let transport = transport_from_query(&link.query)?;
    insert_tls_from_query(&mut outbound, &link.query, &link.host, false);
    insert_transport(&mut outbound, transport);
    Ok(outbound)
}

fn vmess_outbound(uri: &str) -> Result<Value, String> {
    let encoded = uri
        .split_once("://")
        .map(|(_, rest)| rest)
        .ok_or_else(|| format!("Invalid proxy link: {uri}"))?;
    let encoded = encoded.split('#').next().unwrap_or(encoded);
    let decoded = decode_base64(encoded)
        .ok_or_else(|| "VMess link payload is not valid base64.".to_string())?;
    let payload: Value = serde_json::from_str(&decoded)
        .map_err(|error| format!("Invalid VMess link payload: {error}"))?;
    let address = string_field(&payload, "add")
        .ok_or_else(|| "VMess link is missing its server address.".to_string())?;
    let port = json_port(payload.get("port"))
        .ok_or_else(|| "VMess link is missing its server port.".to_string())?;
    let id = string_field(&payload, "id")
        .ok_or_else(|| "VMess link is missing its user id.".to_string())?;
    let mut outbound = json!({
        "type": "vmess",
        "server": address,
        "server_port": port,
        "uuid": id,
        "security": string_field(&payload, "scy").unwrap_or_else(|| "auto".to_string()),
        "alter_id": payload
            .get("aid")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    });

    let network = string_field(&payload, "net").unwrap_or_else(|| "tcp".to_string());
    let host = string_field(&payload, "host");
    let path = string_field(&payload, "path");
    let transport = transport_from_parts(
        &network,
        path.as_deref(),
        host.as_deref(),
        string_field(&payload, "servicename").as_deref(),
        None,
        None,
    )?;
    let mut query = HashMap::new();
    if let Some(sni) = string_field(&payload, "sni") {
        query.insert("sni".to_string(), sni);
    } else if let Some(host) = host {
        query.insert("sni".to_string(), host);
    }
    if let Some(fingerprint) = string_field(&payload, "fp") {
        query.insert("fp".to_string(), fingerprint);
    }
    if let Some(alpn) = string_field(&payload, "alpn") {
        query.insert("alpn".to_string(), alpn);
    }
    let tls = string_field(&payload, "tls").unwrap_or_default();
    if tls == "tls" || tls == "reality" {
        query.insert("security".to_string(), tls);
    }
    insert_tls_from_query(&mut outbound, &query, &address, false);
    insert_transport(&mut outbound, transport);
    Ok(outbound)
}

fn trojan_outbound(uri: &str) -> Result<Value, String> {
    let link = parse_link(uri)?;
    if link.userinfo.is_empty() {
        return Err("Trojan link is missing its password.".to_string());
    }
    let mut outbound = json!({
        "type": "trojan",
        "server": link.host,
        "server_port": link.port,
        "password": link.userinfo
    });
    let mut query = link.query;
    if !query.contains_key("security") {
        query.insert("security".to_string(), "tls".to_string());
    }
    if !query.contains_key("sni") {
        query.insert("sni".to_string(), link.host.clone());
    }
    let transport = transport_from_query(&query)?;
    insert_tls_from_query(&mut outbound, &query, &link.host, true);
    insert_transport(&mut outbound, transport);
    Ok(outbound)
}

fn shadowsocks_outbound(uri: &str) -> Result<Value, String> {
    let (_, rest) = uri
        .split_once("://")
        .ok_or_else(|| format!("Invalid proxy link: {uri}"))?;
    let (rest, _) = rest.split_once('#').unwrap_or((rest, ""));
    let (authority, query_text) = rest.split_once('?').unwrap_or((rest, ""));
    let params = parse_query(query_text);
    let (method, password, host, port) =
        if let Some((userinfo, host_port)) = authority.rsplit_once('@') {
            let credentials = decode_base64(userinfo)
                .filter(|decoded| decoded.contains(':'))
                .unwrap_or_else(|| {
                    percent_decode_str(userinfo)
                        .decode_utf8_lossy()
                        .into_owned()
                });
            let (method, password) = credentials.split_once(':').ok_or_else(|| {
                "Shadowsocks link is missing its cipher and password.".to_string()
            })?;
            let (host, port) = split_host_port(host_port)?;
            (method.to_string(), password.to_string(), host, port)
        } else {
            let decoded = decode_base64(authority)
                .ok_or_else(|| "Shadowsocks link payload is not valid base64.".to_string())?;
            let (credentials, host_port) = decoded
                .rsplit_once('@')
                .ok_or_else(|| "Shadowsocks link is missing its server address.".to_string())?;
            let (method, password) = credentials.split_once(':').ok_or_else(|| {
                "Shadowsocks link is missing its cipher and password.".to_string()
            })?;
            let (host, port) = split_host_port(host_port)?;
            (method.to_string(), password.to_string(), host, port)
        };

    let mut outbound = json!({
        "type": "shadowsocks",
        "server": host,
        "server_port": port,
        "method": method,
        "password": password
    });
    if let Some(plugin) = params.get("plugin").filter(|plugin| !plugin.is_empty()) {
        let (name, options) = plugin.split_once(';').unwrap_or((plugin, ""));
        let plugin_name = match name {
            "obfs-local" | "simple-obfs" => "obfs-local",
            "v2ray-plugin" => "v2ray-plugin",
            _ => "",
        };
        if !plugin_name.is_empty() {
            outbound["plugin"] = json!(plugin_name);
            if !options.is_empty() {
                outbound["plugin_opts"] = json!(options);
            }
        }
    }
    Ok(outbound)
}

fn hysteria2_outbound(uri: &str) -> Result<Value, String> {
    let link = parse_link(uri)?;
    if link.userinfo.is_empty() {
        return Err("Hysteria2 link is missing its password.".to_string());
    }
    let mut outbound = json!({
        "type": "hysteria2",
        "server": link.host,
        "server_port": link.port,
        "password": link.userinfo
    });
    if let Some(obfs) = query(&link.query, "obfs") {
        if !obfs.is_empty() && obfs != "none" {
            outbound["obfs"] = json!({
                "type": obfs,
                "password": query(&link.query, "obfs-password").unwrap_or("")
            });
        }
    }
    if let Some(up) = query(&link.query, "upmbps").and_then(|value| value.parse::<u64>().ok()) {
        outbound["up_mbps"] = json!(up);
    }
    if let Some(down) = query(&link.query, "downmbps").and_then(|value| value.parse::<u64>().ok()) {
        outbound["down_mbps"] = json!(down);
    }
    insert_required_tls(&mut outbound, &link.query, &link.host);
    Ok(outbound)
}

fn tuic_outbound(uri: &str) -> Result<Value, String> {
    let link = parse_link(uri)?;
    let (uuid, password) = link
        .userinfo
        .split_once(':')
        .ok_or_else(|| "TUIC link is missing its uuid and password.".to_string())?;
    let mut outbound = json!({
        "type": "tuic",
        "server": link.host,
        "server_port": link.port,
        "uuid": uuid,
        "password": password
    });
    if let Some(control) = query(&link.query, "congestion_control") {
        outbound["congestion_control"] = json!(control);
    }
    if let Some(mode) = query(&link.query, "udp_relay_mode") {
        outbound["udp_relay_mode"] = json!(mode);
    }
    insert_required_tls(&mut outbound, &link.query, &link.host);
    Ok(outbound)
}

fn anytls_outbound(uri: &str) -> Result<Value, String> {
    let link = parse_link(uri)?;
    if link.userinfo.is_empty() {
        return Err("AnyTLS link is missing its password.".to_string());
    }
    let mut outbound = json!({
        "type": "anytls",
        "server": link.host,
        "server_port": link.port,
        "password": link.userinfo
    });
    insert_required_tls(&mut outbound, &link.query, &link.host);
    Ok(outbound)
}

fn socks_outbound(uri: &str) -> Result<Value, String> {
    let link = parse_link(uri)?;
    let mut outbound = json!({
        "type": "socks",
        "server": link.host,
        "server_port": link.port
    });
    if !link.userinfo.is_empty() {
        let (username, password) = split_credentials(&link.userinfo);
        outbound["username"] = json!(username);
        outbound["password"] = json!(password);
    }
    Ok(outbound)
}

fn http_outbound(uri: &str) -> Result<Value, String> {
    let link = parse_link(uri)?;
    let mut outbound = json!({
        "type": "http",
        "server": link.host,
        "server_port": link.port
    });
    if !link.userinfo.is_empty() {
        let (username, password) = split_credentials(&link.userinfo);
        outbound["username"] = json!(username);
        outbound["password"] = json!(password);
    }
    if uri
        .split_once("://")
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("https"))
    {
        outbound["tls"] = json!({"enabled": true, "server_name": link.host});
    }
    Ok(outbound)
}

fn insert_tls_from_query(
    outbound: &mut Value,
    query: &HashMap<String, String>,
    host: &str,
    default_on: bool,
) {
    let security = query
        .get("security")
        .map(String::as_str)
        .unwrap_or(if default_on { "tls" } else { "none" });
    if security != "tls" && security != "reality" {
        return;
    }
    let mut tls = Map::new();
    tls.insert("enabled".to_string(), json!(true));
    let server_name = query
        .get("sni")
        .or_else(|| query.get("peer"))
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(host);
    tls.insert("server_name".to_string(), json!(server_name));
    if query_bool(query, &["allowinsecure", "insecure", "skip-cert-verify"]) {
        tls.insert("insecure".to_string(), json!(true));
    }
    if let Some(alpn) = query.get("alpn").filter(|value| !value.is_empty()) {
        tls.insert(
            "alpn".to_string(),
            json!(alpn
                .split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()),
        );
    }
    if let Some(fingerprint) = query.get("fp").filter(|value| !value.is_empty()) {
        tls.insert(
            "utls".to_string(),
            json!({"enabled": true, "fingerprint": fingerprint}),
        );
    }
    if security == "reality" {
        let mut reality = Map::new();
        reality.insert("enabled".to_string(), json!(true));
        if let Some(public_key) = query.get("pbk").filter(|value| !value.is_empty()) {
            reality.insert("public_key".to_string(), json!(public_key));
        }
        if let Some(short_id) = query.get("sid").filter(|value| !value.is_empty()) {
            reality.insert("short_id".to_string(), json!(short_id));
        }
        tls.insert("reality".to_string(), Value::Object(reality));
        ensure_utls_for_reality(&mut tls);
    }
    outbound["tls"] = Value::Object(tls);
}

/// sing-box refuses to start a Reality outbound without uTLS, while a link or
/// subscription often omits `fp`. Fall back to the browser fingerprint the
/// official examples use.
fn ensure_utls_for_reality(tls: &mut Map<String, Value>) {
    if tls.contains_key("reality") && !tls.contains_key("utls") {
        tls.insert(
            "utls".to_string(),
            json!({"enabled": true, "fingerprint": "chrome"}),
        );
    }
}

fn insert_required_tls(outbound: &mut Value, query: &HashMap<String, String>, host: &str) {
    if outbound.get("tls").is_some() {
        return;
    }
    let mut tls = Map::new();
    tls.insert("enabled".to_string(), json!(true));
    tls.insert(
        "server_name".to_string(),
        json!(query.get("sni").map(String::as_str).unwrap_or(host)),
    );
    if query_bool(query, &["allowinsecure", "insecure", "skip-cert-verify"]) {
        tls.insert("insecure".to_string(), json!(true));
    }
    if let Some(alpn) = query.get("alpn").filter(|value| !value.is_empty()) {
        tls.insert(
            "alpn".to_string(),
            json!(alpn
                .split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()),
        );
    }
    outbound["tls"] = Value::Object(tls);
}

fn transport_from_query(query: &HashMap<String, String>) -> Result<Option<Value>, String> {
    let network = query
        .get("type")
        .or_else(|| query.get("network"))
        .map(String::as_str)
        .unwrap_or("tcp");
    transport_from_parts(
        network,
        query.get("path").map(String::as_str),
        query.get("host").map(String::as_str),
        query.get("servicename").map(String::as_str),
        None,
        None,
    )
}

fn transport_from_parts(
    network: &str,
    path: Option<&str>,
    host: Option<&str>,
    service_name: Option<&str>,
    header_type: Option<&str>,
    mode: Option<&str>,
) -> Result<Option<Value>, String> {
    let _ = (header_type, mode);
    let network = network.trim().to_ascii_lowercase();
    let transport = match network.as_str() {
        "" | "tcp" | "raw" => return Ok(None),
        "ws" | "websocket" => {
            let mut transport = json!({"type": "ws"});
            if let Some(path) = path {
                transport["path"] = json!(path);
            }
            if let Some(host) = host {
                transport["headers"] = json!({"Host": host});
            }
            transport
        }
        "grpc" => json!({
            "type": "grpc",
            "service_name": service_name
                .or(path)
                .unwrap_or("")
        }),
        "http" | "h2" => {
            let mut transport = json!({"type": "http"});
            if let Some(path) = path {
                transport["path"] = json!(path);
            }
            if let Some(host) = host {
                transport["host"] = json!([host]);
            }
            transport
        }
        "httpupgrade" => {
            let mut transport = json!({"type": "httpupgrade"});
            if let Some(path) = path {
                transport["path"] = json!(path);
            }
            if let Some(host) = host {
                transport["host"] = json!(host);
            }
            transport
        }
        other => {
            return Err(format!(
                "The {other} transport is not supported by sing-box."
            ))
        }
    };
    Ok(Some(transport))
}

fn insert_transport(outbound: &mut Value, transport: Option<Value>) {
    if let Some(transport) = transport {
        outbound["transport"] = transport;
    }
}

fn split_credentials(userinfo: &str) -> (String, String) {
    let decoded = if userinfo.contains(':') {
        userinfo.to_string()
    } else {
        decode_base64(userinfo).unwrap_or_else(|| userinfo.to_string())
    };
    let (user, pass) = decoded.split_once(':').unwrap_or((decoded.as_str(), ""));
    (user.to_string(), pass.to_string())
}

fn decode_base64(value: &str) -> Option<String> {
    let trimmed = value.trim();
    for engine in [
        &base64::engine::general_purpose::STANDARD,
        &base64::engine::general_purpose::URL_SAFE,
        &base64::engine::general_purpose::STANDARD_NO_PAD,
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Ok(bytes) = engine.decode(trimmed) {
            if let Ok(text) = String::from_utf8(bytes) {
                return Some(text);
            }
        }
    }
    None
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn is_sing_box_protocol(protocol: &str) -> bool {
    // Only outbound types the current sing-box still ships. A subscription
    // that carries anything else is skipped instead of being handed to the
    // core, which would refuse to start with an unknown outbound.
    matches!(
        protocol,
        "vless"
            | "vmess"
            | "trojan"
            | "shadowsocks"
            | "socks"
            | "http"
            | "hysteria2"
            | "tuic"
            | "anytls"
            | "ssh"
            | "wireguard"
            | "shadowtls"
    )
}

fn node_key(protocol: &str, name: &str, address: &str) -> String {
    let node = SubscriptionNode {
        name: name.to_string(),
        protocol: protocol.to_string(),
        transport: String::new(),
        tls: String::new(),
        address: address.to_string(),
    };
    subscription_node_key(&node)
}

fn yaml_candidates(payload: &str) -> Vec<NodeCandidate> {
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
        .filter_map(yaml_candidate)
        .collect::<Vec<_>>()
}

fn yaml_candidate(proxy: &serde_yaml::Value) -> Option<NodeCandidate> {
    let map = proxy.as_mapping()?;
    let original_protocol = yaml_str(map, "type")?;
    let protocol = original_protocol.to_ascii_lowercase();
    let name = yaml_str(map, "name").unwrap_or("Unknown").to_string();
    let address = yaml_str(map, "server").unwrap_or("-").to_string();
    let key = node_key(original_protocol, &name, &address);
    let outbound = yaml_outbound(map, &protocol)?;
    Some(NodeCandidate {
        key,
        name,
        outbound,
    })
}

fn yaml_outbound(map: &serde_yaml::Mapping, protocol: &str) -> Option<Value> {
    let server = yaml_str(map, "server")?;
    let port = yaml_u64(map, "port").and_then(|port| u16::try_from(port).ok())?;
    let mut outbound = Map::new();
    let outbound_type = match protocol {
        "ss" | "shadowsocks" => "shadowsocks",
        "socks5" | "socks" => "socks",
        "http" => "http",
        "vmess" => "vmess",
        "vless" => "vless",
        "trojan" => "trojan",
        "hysteria2" | "hy2" => "hysteria2",
        "tuic" => "tuic",
        "anytls" => "anytls",
        "wireguard" => "wireguard",
        "ssh" => "ssh",
        _ => return None,
    };
    outbound.insert("type".to_string(), json!(outbound_type));
    outbound.insert("server".to_string(), json!(server));
    outbound.insert("server_port".to_string(), json!(port));

    match outbound_type {
        "shadowsocks" => {
            outbound.insert("method".to_string(), json!(yaml_str(map, "cipher")?));
            outbound.insert("password".to_string(), json!(yaml_str(map, "password")?));
            if let Some(plugin) = yaml_str(map, "plugin") {
                let plugin_name = match plugin {
                    "obfs" => "obfs-local",
                    "v2ray-plugin" => "v2ray-plugin",
                    _ => "",
                };
                if !plugin_name.is_empty() {
                    outbound.insert("plugin".to_string(), json!(plugin_name));
                    if let Some(options) = yaml_str(map, "plugin-opts") {
                        outbound.insert("plugin_opts".to_string(), json!(options));
                    }
                }
            }
        }
        "vmess" => {
            outbound.insert("uuid".to_string(), json!(yaml_str(map, "uuid")?));
            outbound.insert(
                "security".to_string(),
                json!(yaml_str(map, "cipher").unwrap_or("auto")),
            );
            outbound.insert(
                "alter_id".to_string(),
                json!(yaml_u64(map, "alterId").unwrap_or(0)),
            );
        }
        "vless" => {
            outbound.insert("uuid".to_string(), json!(yaml_str(map, "uuid")?));
            if let Some(flow) = yaml_str(map, "flow") {
                outbound.insert("flow".to_string(), json!(flow));
            }
        }
        "trojan" => {
            outbound.insert("password".to_string(), json!(yaml_str(map, "password")?));
        }
        "hysteria2" => {
            outbound.insert("password".to_string(), json!(yaml_str(map, "password")?));
            if let Some(obfs) = yaml_str(map, "obfs") {
                if !obfs.is_empty() && obfs != "none" {
                    outbound.insert(
                        "obfs".to_string(),
                        json!({
                            "type": obfs,
                            "password": yaml_str(map, "obfs-password").unwrap_or("")
                        }),
                    );
                }
            }
            insert_yaml_tls(&mut outbound, map, true)?;
        }
        "tuic" => {
            outbound.insert("uuid".to_string(), json!(yaml_str(map, "uuid")?));
            outbound.insert("password".to_string(), json!(yaml_str(map, "password")?));
            if let Some(control) = yaml_str(map, "congestion-controller") {
                outbound.insert("congestion_control".to_string(), json!(control));
            }
            if let Some(mode) = yaml_str(map, "udp-relay-mode") {
                outbound.insert("udp_relay_mode".to_string(), json!(mode));
            }
            insert_yaml_tls(&mut outbound, map, true)?;
        }
        "anytls" => {
            outbound.insert("password".to_string(), json!(yaml_str(map, "password")?));
            insert_yaml_tls(&mut outbound, map, true)?;
        }
        "socks" => {
            if let Some(username) = yaml_str(map, "username") {
                outbound.insert("username".to_string(), json!(username));
            }
            if let Some(password) = yaml_str(map, "password") {
                outbound.insert("password".to_string(), json!(password));
            }
        }
        "http" => {
            if let Some(username) = yaml_str(map, "username") {
                outbound.insert("username".to_string(), json!(username));
            }
            if let Some(password) = yaml_str(map, "password") {
                outbound.insert("password".to_string(), json!(password));
            }
            insert_yaml_tls(&mut outbound, map, false)?;
        }
        "wireguard" => {
            outbound.insert(
                "private_key".to_string(),
                json!(yaml_str(map, "private-key")?),
            );
            if let Some(public_key) = yaml_str(map, "public-key") {
                outbound.insert("peer_public_key".to_string(), json!(public_key));
            }
            if let Some(ip) = yaml_str(map, "ip") {
                outbound.insert("local_address".to_string(), json!([ip]));
            }
        }
        "ssh" => {
            outbound.insert("user".to_string(), json!(yaml_str(map, "username")?));
            if let Some(password) = yaml_str(map, "password") {
                outbound.insert("password".to_string(), json!(password));
            }
        }
        _ => return None,
    }

    if matches!(outbound_type, "vmess" | "vless" | "trojan") {
        let network = yaml_str(map, "network").unwrap_or("tcp");
        let (path, host, service_name) = yaml_transport_options(map, network);
        let transport = transport_from_parts(
            network,
            path.as_deref(),
            host.as_deref(),
            service_name.as_deref(),
            None,
            None,
        )
        .ok()?;
        if let Some(transport) = transport {
            outbound.insert("transport".to_string(), transport);
        }
        insert_yaml_tls(&mut outbound, map, outbound_type == "trojan")?;
    }
    Some(Value::Object(outbound))
}

fn yaml_transport_options(
    map: &serde_yaml::Mapping,
    network: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    match network {
        "ws" => {
            let options = yaml_map(map, "ws-opts");
            let path = options.and_then(|options| yaml_str(options, "path"));
            let host = options
                .and_then(|options| yaml_map(options, "headers"))
                .and_then(|headers| {
                    yaml_str(headers, "Host").or_else(|| yaml_str(headers, "host"))
                });
            (path.map(str::to_string), host.map(str::to_string), None)
        }
        "grpc" => {
            let service = yaml_map(map, "grpc-opts")
                .and_then(|options| yaml_str(options, "grpc-service-name"));
            (None, None, service.map(str::to_string))
        }
        "http" | "h2" => {
            let options = yaml_map(map, "h2-opts");
            let path = options.and_then(|options| yaml_str(options, "path"));
            let host = options.and_then(|options| {
                options
                    .get(serde_yaml::Value::String("host".to_string()))
                    .and_then(serde_yaml::Value::as_sequence)
                    .and_then(|hosts| hosts.first())
                    .and_then(serde_yaml::Value::as_str)
                    .or_else(|| yaml_str(options, "host"))
            });
            (path.map(str::to_string), host.map(str::to_string), None)
        }
        _ => (None, None, None),
    }
}

fn insert_yaml_tls(
    outbound: &mut Map<String, Value>,
    map: &serde_yaml::Mapping,
    default_on: bool,
) -> Option<()> {
    let enabled = match map.get(serde_yaml::Value::String("tls".to_string())) {
        Some(serde_yaml::Value::Bool(enabled)) => *enabled,
        Some(serde_yaml::Value::String(value)) => !value.eq_ignore_ascii_case("none"),
        _ => default_on || yaml_map(map, "reality-opts").is_some(),
    };
    if !enabled {
        return Some(());
    }
    let mut tls = Map::new();
    tls.insert("enabled".to_string(), json!(true));
    let server_name = yaml_str(map, "servername")
        .or_else(|| yaml_str(map, "sni"))
        .or_else(|| yaml_str(map, "server"))
        .unwrap_or("");
    tls.insert("server_name".to_string(), json!(server_name));
    if yaml_bool(map, "skip-cert-verify").unwrap_or(false) {
        tls.insert("insecure".to_string(), json!(true));
    }
    if let Some(alpn) = map.get(serde_yaml::Value::String("alpn".to_string())) {
        let values = match alpn {
            serde_yaml::Value::Sequence(values) => values
                .iter()
                .filter_map(serde_yaml::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>(),
            serde_yaml::Value::String(value) => value
                .split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect(),
            _ => Vec::new(),
        };
        if !values.is_empty() {
            tls.insert("alpn".to_string(), json!(values));
        }
    }
    if let Some(fingerprint) = yaml_str(map, "client-fingerprint") {
        tls.insert(
            "utls".to_string(),
            json!({"enabled": true, "fingerprint": fingerprint}),
        );
    }
    if let Some(reality) = yaml_map(map, "reality-opts") {
        let mut options = Map::new();
        options.insert("enabled".to_string(), json!(true));
        if let Some(public_key) = yaml_str(reality, "public-key") {
            options.insert("public_key".to_string(), json!(public_key));
        }
        if let Some(short_id) = yaml_str(reality, "short-id") {
            options.insert("short_id".to_string(), json!(short_id));
        }
        tls.insert("reality".to_string(), Value::Object(options));
        ensure_utls_for_reality(&mut tls);
    }
    outbound.insert("tls".to_string(), Value::Object(tls));
    Some(())
}

fn yaml_str<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Option<&'a str> {
    map.get(serde_yaml::Value::String(key.to_string()))
        .and_then(serde_yaml::Value::as_str)
}

fn yaml_bool(map: &serde_yaml::Mapping, key: &str) -> Option<bool> {
    map.get(serde_yaml::Value::String(key.to_string()))
        .and_then(serde_yaml::Value::as_bool)
}

fn yaml_u64(map: &serde_yaml::Mapping, key: &str) -> Option<u64> {
    map.get(serde_yaml::Value::String(key.to_string()))
        .and_then(serde_yaml::Value::as_u64)
}

fn yaml_map<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Option<&'a serde_yaml::Mapping> {
    map.get(serde_yaml::Value::String(key.to_string()))
        .and_then(serde_yaml::Value::as_mapping)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::subscription_manager::{
        parse_proxy_uri, parse_subscription_nodes, subscription_node_key,
    };

    const NODES: &str = "\
vless://11111111-1111-1111-1111-111111111111@a.example:443?security=reality&pbk=public-key&sid=abcd&sni=a.example&type=grpc&serviceName=svc#Node A
trojan://secret@b.example:8443?type=ws&path=%2Fws&sni=b.example#Node B
hysteria2://pass@c.example:443?obfs=salamander&obfs-password=obfs-pass#Node C
ss://YWVzLTEyOC1nY206cGFzcw==@d.example:8388#Node D
";

    fn key_for(uri: &str) -> String {
        subscription_node_key(&parse_proxy_uri(uri).expect("the test link must parse"))
    }

    fn tags(config: &Value) -> Vec<String> {
        config["outbounds"]
            .as_array()
            .expect("outbounds")
            .iter()
            .map(|outbound| {
                outbound["tag"]
                    .as_str()
                    .expect("every outbound has a tag")
                    .to_string()
            })
            .collect()
    }

    fn outbound(config: &Value, tag: &str) -> Value {
        config["outbounds"]
            .as_array()
            .expect("outbounds")
            .iter()
            .find(|outbound| outbound["tag"] == tag)
            .cloned()
            .unwrap_or_else(|| panic!("no outbound tagged {tag}"))
    }

    #[test]
    fn every_supported_uri_becomes_a_sing_box_outbound() {
        let vless = outbound_from_uri(NODES.lines().next().unwrap()).unwrap();
        assert_eq!(vless["type"], "vless");
        assert_eq!(vless["server"], "a.example");
        assert_eq!(vless["server_port"], 443);
        assert_eq!(vless["uuid"], "11111111-1111-1111-1111-111111111111");
        assert_eq!(vless["tls"]["server_name"], "a.example");
        assert_eq!(vless["tls"]["reality"]["public_key"], "public-key");
        assert_eq!(vless["tls"]["reality"]["short_id"], "abcd");
        // sing-box refuses a Reality outbound without uTLS.
        assert_eq!(vless["tls"]["utls"]["enabled"], true);
        assert_eq!(vless["tls"]["utls"]["fingerprint"], "chrome");
        assert_eq!(vless["transport"]["type"], "grpc");
        assert_eq!(vless["transport"]["service_name"], "svc");

        let trojan = outbound_from_uri(NODES.lines().nth(1).unwrap()).unwrap();
        assert_eq!(trojan["type"], "trojan");
        assert_eq!(trojan["password"], "secret");
        assert_eq!(trojan["tls"]["server_name"], "b.example");
        assert_eq!(trojan["transport"]["type"], "ws");
        assert_eq!(trojan["transport"]["path"], "/ws");

        let hysteria2 = outbound_from_uri(NODES.lines().nth(2).unwrap()).unwrap();
        assert_eq!(hysteria2["type"], "hysteria2");
        assert_eq!(hysteria2["password"], "pass");
        assert_eq!(hysteria2["obfs"]["type"], "salamander");
        assert_eq!(hysteria2["tls"]["enabled"], true);

        let shadowsocks = outbound_from_uri(NODES.lines().nth(3).unwrap()).unwrap();
        assert_eq!(shadowsocks["type"], "shadowsocks");
        assert_eq!(shadowsocks["method"], "aes-128-gcm");
        assert_eq!(shadowsocks["password"], "pass");
    }

    #[test]
    fn protocols_sing_box_cannot_run_are_not_translated() {
        assert!(outbound_from_uri("ssr://c2VydmVyOjQ0Mw==#Node").is_err());
        assert!(outbound_from_uri("hysteria://pass@a.example:443#Node").is_err());
    }

    #[test]
    fn the_selected_node_is_the_default_of_the_selector() {
        let settings = SingBoxSettings::default();
        let selected = key_for(NODES.lines().nth(1).unwrap());
        let prepared =
            prepare_with_settings(NODES, Some(&selected), 2080, None, Some(9090), &settings)
                .expect("the subscription must build");

        assert_eq!(prepared.mixed_port, 2080);
        assert_eq!(prepared.node_name.as_deref(), Some("Node B"));
        assert_eq!(
            tags(&prepared.config),
            [
                "cloak-node-0",
                "cloak-node-1",
                "cloak-node-2",
                "cloak-node-3",
                "proxy",
                "direct"
            ]
        );
        let selector = outbound(&prepared.config, "proxy");
        assert_eq!(selector["type"], "selector");
        assert_eq!(selector["default"], "cloak-node-1");
        assert_eq!(
            prepared.config["experimental"]["clash_api"]["external_controller"],
            "127.0.0.1:9090"
        );

        let hot = prepared.hot_switch.expect("manual switching is available");
        assert_eq!(hot.selector, "proxy");
        assert_eq!(hot.selected_tag, "cloak-node-1");
        assert_eq!(hot.nodes.len(), 4);
        assert_eq!(hot.nodes[1].key, selected);
        assert_eq!(hot.nodes[1].name.as_deref(), Some("Node B"));
    }

    #[test]
    fn a_node_sing_box_cannot_run_is_rejected_instead_of_silently_swapped() {
        let settings = SingBoxSettings::default();
        let error =
            prepare_with_settings(NODES, Some("missing"), 2080, None, Some(9090), &settings)
                .expect_err("an unknown key must not start the wrong node");
        assert!(error.contains("cannot be run by sing-box"));
    }

    #[test]
    fn a_subscription_without_a_single_usable_node_is_rejected() {
        let settings = SingBoxSettings::default();
        let payload = "ssr://c2VydmVyOjQ0Mw==#Node\n";
        let error = prepare_with_settings(payload, None, 2080, None, None, &settings)
            .expect_err("ShadowsocksR is not a sing-box outbound");
        assert!(error.contains("no nodes that sing-box can run"));
    }

    #[test]
    fn auto_failover_uses_urltest_and_keeps_live_switching_off() {
        let settings = SingBoxSettings {
            auto_failover: true,
            ..Default::default()
        };
        let prepared = prepare_with_settings(NODES, None, 2080, None, Some(9090), &settings)
            .expect("the subscription must build");

        let group = outbound(&prepared.config, "proxy");
        assert_eq!(group["type"], "urltest");
        assert_eq!(group["outbounds"].as_array().unwrap().len(), 4);
        assert!(prepared.hot_switch.is_none());
        assert_eq!(prepared.node_name.as_deref(), Some("Node A"));
    }

    #[test]
    fn without_an_api_port_only_the_selected_node_is_written() {
        let settings = SingBoxSettings::default();
        let prepared = prepare_with_settings(NODES, None, 2080, None, None, &settings)
            .expect("the subscription must build");

        assert_eq!(tags(&prepared.config), ["proxy", "direct"]);
        assert!(prepared.config.get("experimental").is_none());
        assert!(prepared.hot_switch.is_none());
    }

    #[test]
    fn test_configs_skip_dns_tun_routing_and_the_clash_api() {
        let settings = SingBoxSettings::default();
        let prepared = prepare_for_test(NODES, None, 2099, &settings).expect("test config");

        assert_eq!(prepared.mixed_port, 2099);
        assert!(prepared.api_port.is_none());
        assert!(prepared.hot_switch.is_none());
        assert_eq!(tags(&prepared.config), ["proxy", "direct"]);
        let inbounds = prepared.config["inbounds"].as_array().unwrap();
        assert_eq!(inbounds.len(), 1);
        assert_eq!(inbounds[0]["type"], "mixed");
        assert_eq!(inbounds[0]["listen"], "127.0.0.1");
        assert_eq!(prepared.config["route"]["final"], "proxy");
        assert!(prepared.config.get("experimental").is_none());
    }

    #[test]
    fn tun_auto_redirect_is_dropped_without_auto_route() {
        let mut settings = SingBoxSettings {
            tun_enabled: true,
            tun_auto_route: true,
            tun_auto_redirect: true,
            ..Default::default()
        };
        let prepared = prepare_with_settings(NODES, None, 2080, None, None, &settings).unwrap();
        let tun = prepared.config["inbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|inbound| inbound["type"] == "tun")
            .cloned()
            .unwrap();
        assert_eq!(tun["auto_redirect"], true);

        settings.tun_auto_route = false;
        let prepared = prepare_with_settings(NODES, None, 2080, None, None, &settings).unwrap();
        let tun = prepared.config["inbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|inbound| inbound["type"] == "tun")
            .cloned()
            .unwrap();
        assert!(tun.get("auto_redirect").is_none());
    }

    #[test]
    fn the_lan_inbound_carries_the_configured_credentials() {
        let settings = SingBoxSettings {
            lan_username: "clash".to_string(),
            lan_password: "secret".to_string(),
            ..Default::default()
        };
        let prepared =
            prepare_with_settings(NODES, None, 2080, Some(2081), None, &settings).unwrap();

        let lan = prepared.config["inbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|inbound| inbound["tag"] == "lan-in")
            .cloned()
            .unwrap();
        assert_eq!(lan["listen"], "0.0.0.0");
        assert_eq!(lan["listen_port"], 2081);
        assert_eq!(lan["users"][0]["username"], "clash");
        assert_eq!(lan["users"][0]["password"], "secret");
    }

    #[test]
    fn the_remote_dns_server_is_reached_through_the_proxy() {
        let settings = SingBoxSettings {
            remote_dns: "https://dns.example/dns-query".to_string(),
            ..Default::default()
        };
        let prepared = prepare_with_settings(NODES, None, 2080, None, None, &settings).unwrap();

        let servers = prepared.config["dns"]["servers"].as_array().unwrap();
        let remote = servers
            .iter()
            .find(|server| server["tag"] == "dns-remote")
            .expect("the remote resolver is present");
        assert_eq!(remote["type"], "https");
        assert_eq!(remote["server"], "dns.example");
        assert_eq!(remote["server_port"], 443);
        assert_eq!(remote["path"], "/dns-query");
        assert_eq!(remote["detour"], "proxy");
        assert_eq!(prepared.config["dns"]["final"], "dns-remote");
    }

    #[test]
    fn a_hostname_local_resolver_is_bootstrapped_by_the_system() {
        let settings = SingBoxSettings {
            local_dns: "https://dns.example/dns-query".to_string(),
            ..Default::default()
        };
        let prepared = prepare_with_settings(NODES, None, 2080, None, None, &settings).unwrap();

        let servers = prepared.config["dns"]["servers"].as_array().unwrap();
        let local = servers
            .iter()
            .find(|server| server["tag"] == "dns-local")
            .expect("the local resolver is present");
        assert_eq!(local["server"], "dns.example");
        assert_eq!(local["domain_resolver"], "dns-bootstrap");
        assert!(servers
            .iter()
            .any(|server| server["tag"] == "dns-bootstrap" && server["type"] == "local"));
    }

    #[test]
    fn a_numeric_local_resolver_needs_no_bootstrap() {
        let settings = SingBoxSettings {
            local_dns: "1.1.1.1".to_string(),
            ..Default::default()
        };
        let prepared = prepare_with_settings(NODES, None, 2080, None, None, &settings).unwrap();

        let servers = prepared.config["dns"]["servers"].as_array().unwrap();
        let local = servers
            .iter()
            .find(|server| server["tag"] == "dns-local")
            .expect("the local resolver is present");
        assert!(local.get("domain_resolver").is_none());
        assert!(!servers
            .iter()
            .any(|server| server["tag"] == "dns-bootstrap"));
    }

    #[test]
    fn direct_mode_finishes_on_the_direct_outbound_and_skips_the_proxy_rules() {
        let settings = SingBoxSettings {
            routing_mode: "direct".to_string(),
            ..Default::default()
        };
        let prepared =
            prepare_with_settings(NODES, None, 2080, None, Some(9090), &settings).unwrap();

        assert_eq!(prepared.config["route"]["final"], "direct");
        let rules = prepared.config["route"]["rules"].as_array().unwrap();
        assert!(!rules.iter().any(|rule| rule["outbound"] == "proxy"));
    }

    #[test]
    fn custom_rules_are_split_into_rule_objects() {
        let settings = SingBoxSettings {
            custom_rules: "\
                example.com direct\n\
                10.0.0.0/8 block\n\
                1.1.1.1 proxy\n\
                ignored.invalid nowhere\n\
                # a comment\n"
                .to_string(),
            ..Default::default()
        };
        let prepared = prepare_with_settings(NODES, None, 2080, None, None, &settings).unwrap();

        let rules = prepared.config["route"]["rules"].as_array().unwrap();
        let domain = rules
            .iter()
            .find(|rule| rule["domain_suffix"] == json!(["example.com"]))
            .expect("domain rule");
        assert_eq!(domain["outbound"], "direct");

        let network = rules
            .iter()
            .find(|rule| rule["ip_cidr"] == json!(["10.0.0.0/8"]))
            .expect("cidr rule");
        assert_eq!(network["action"], "reject");

        let address = rules
            .iter()
            .find(|rule| rule["ip_cidr"] == json!(["1.1.1.1/32"]))
            .expect("single address rule");
        assert_eq!(address["outbound"], "proxy");

        assert!(!rules
            .iter()
            .any(|rule| rule["domain_suffix"] == json!(["ignored.invalid"])));
    }

    #[test]
    fn outbound_options_are_applied_to_the_protocols_that_support_them() {
        let settings = SingBoxSettings {
            tcp_fast_open: true,
            mux_enabled: true,
            mux_protocol: "h2mux".to_string(),
            mux_max_connections: 4,
            ..Default::default()
        };
        let prepared = prepare_with_settings(NODES, None, 2080, None, None, &settings).unwrap();

        let vless = outbound(&prepared.config, "proxy");
        assert_eq!(vless["tcp_fast_open"], true);
        assert_eq!(vless["multiplex"]["protocol"], "h2mux");
        assert_eq!(vless["multiplex"]["max_connections"], 4);
    }

    #[test]
    fn outbound_domains_have_a_default_resolver_and_no_deprecated_dns_options() {
        let settings = SingBoxSettings::default();
        let prepared = prepare_with_settings(NODES, None, 2080, None, None, &settings).unwrap();

        // sing-box 1.14 refuses outbounds whose domain cannot be resolved
        // through a dial field; the route-level default is the shared fix.
        assert_eq!(
            prepared.config["route"]["default_domain_resolver"],
            "dns-local"
        );
        let dns = prepared.config["dns"].as_object().unwrap();
        assert!(!dns.contains_key("independent_cache"));

        let test = prepare_for_test(NODES, None, 2080, &settings).unwrap();
        assert_eq!(test.config["route"]["default_domain_resolver"], "dns-local");
    }

    #[test]
    fn json_subscriptions_produce_the_same_keys_as_the_proxies_page() {
        let raw = r#"{"outbounds":[
            {"type":"vless","tag":"A","server":"a.example","server_port":443,"uuid":"u"},
            {"type":"hysteria2","tag":"B","server":"b.example","server_port":443,"password":"p"}
        ]}"#;
        let nodes = parse_subscription_nodes(raw);
        assert_eq!(nodes.len(), 2);
        let selected = subscription_node_key(&nodes[1]);

        let prepared = prepare_with_settings(
            raw,
            Some(&selected),
            2080,
            None,
            Some(9090),
            &sing_box_settings(),
        )
        .expect("JSON subscriptions are supported");
        assert_eq!(prepared.node_name.as_deref(), Some("B"));
        let hot = prepared.hot_switch.expect("hot switching");
        assert_eq!(hot.nodes[1].key, selected);
    }

    #[test]
    fn clash_subscriptions_produce_the_same_keys_as_the_proxies_page() {
        let raw = r#"proxies:
  - name: node-a
    type: vless
    server: a.example
    port: 443
    uuid: "1111"
    tls: true
  - name: node-b
    type: hysteria2
    server: b.example
    port: 443
    password: pass
"#;
        let nodes = parse_subscription_nodes(raw);
        assert_eq!(nodes.len(), 2);
        let selected = subscription_node_key(&nodes[1]);

        let prepared = prepare_with_settings(
            raw,
            Some(&selected),
            2080,
            None,
            Some(9090),
            &sing_box_settings(),
        )
        .expect("Clash subscriptions are supported");
        assert_eq!(prepared.node_name.as_deref(), Some("node-b"));
        assert_eq!(
            prepared.hot_switch.expect("hot switching").nodes[1].key,
            selected
        );
    }

    #[test]
    fn xray_json_subscriptions_produce_the_same_keys_as_the_proxies_page() {
        let raw = r#"{"outbounds":[{
            "protocol":"vmess",
            "tag":"subscription",
            "settings":{"vnext":[{
                "address":"edge.example",
                "port":443,
                "users":[{"id":"uuid","email":"node-a"}]
            }]},
            "streamSettings":{"network":"ws","security":"tls"}
        }]}"#;
        let nodes = parse_subscription_nodes(raw);
        assert_eq!(nodes.len(), 1);
        let selected = subscription_node_key(&nodes[0]);

        let prepared = prepare_with_settings(
            raw,
            Some(&selected),
            2080,
            None,
            Some(9090),
            &sing_box_settings(),
        )
        .expect("Xray JSON subscriptions are translated");
        assert_eq!(prepared.node_name.as_deref(), Some("node-a"));
        let node = outbound(&prepared.config, "cloak-node-0");
        assert_eq!(node["type"], "vmess");
        assert_eq!(node["transport"]["type"], "ws");
        assert_eq!(node["tls"]["enabled"], true);
    }

    fn sing_box_settings() -> SingBoxSettings {
        SingBoxSettings::default()
    }
}
