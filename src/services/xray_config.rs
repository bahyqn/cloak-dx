use serde_json::{json, Map, Value};
use std::net::IpAddr;

use super::{
    geo_assets::geo_asset_directory,
    subscription_manager::{decode_subscription, parse_proxy_uri, subscription_node_key},
    xray_settings::{
        bypass_entries, outbound_rules, OutboundTarget, RuleMatcher, XraySettings,
        DEFAULT_FAKE_DNS_IPV6_POOL, TUN_IPV6_GATEWAY,
    },
};

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedConfig {
    pub config: Value,
    pub socks_port: Option<u16>,
    pub http_port: Option<u16>,
    /// Ports of the optional LAN inbounds, which sit next to the loopback ones
    /// so local clients never need the LAN credentials.
    pub lan_socks_port: Option<u16>,
    pub lan_http_port: Option<u16>,
    pub node_name: Option<String>,
    /// Present when the config carries every subscription node behind a
    /// balancer, so switching nodes only has to override that balancer.
    pub hot_switch: Option<HotSwitch>,
}

/// Runtime information needed to move the running core to another node.
#[derive(Clone, Debug, PartialEq)]
pub struct HotSwitch {
    /// Loopback port of the Xray API inbound used to apply the switch.
    pub api_port: u16,
    pub balancer: String,
    /// Tag the balancer is currently overridden to.
    pub selected_tag: String,
    pub nodes: Vec<HotNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HotNode {
    pub key: String,
    pub name: Option<String>,
    pub tag: String,
}

/// Prefix for the per-node outbounds; the balancer's selector matches it.
pub const HOT_NODE_TAG_PREFIX: &str = "cloak-node-";
pub const HOT_SWITCH_BALANCER: &str = "cloak-balancer";
/// Freedom outbound that splits the TLS ClientHello when fragmenting is on.
const FRAGMENT_TAG: &str = "cloak-fragment";
/// Outbound protocols that can be dialed through the fragment outbound.
const PROXY_PROTOCOLS: &[&str] = &["vless", "vmess", "trojan", "shadowsocks", "socks", "http"];

/// Builds a runnable Xray config from a stored subscription payload using the
/// default settings. Kept for tests; the app always supplies stored settings.
///
/// Full Xray JSON configs are passed through untouched (local inbounds are
/// added only when the config has none). Base64 / plain proxy URI lists are
/// converted into a single outbound for the selected node.
#[cfg(test)]
pub fn prepare(
    raw_subscription: &str,
    selected_node_key: Option<&str>,
    socks_port: u16,
    http_port: u16,
) -> Result<PreparedConfig, String> {
    prepare_with_settings(
        raw_subscription,
        selected_node_key,
        socks_port,
        http_port,
        None,
        None,
        &XraySettings::default(),
    )
}

/// Builds a runnable Xray config using the options configured in Settings.
///
/// `api_port` enables hot node switching: every node in the subscription gets
/// an outbound behind a balancer, and the API inbound lets the caller override
/// that balancer instead of restarting the core.
pub fn prepare_with_settings(
    raw_subscription: &str,
    selected_node_key: Option<&str>,
    socks_port: u16,
    http_port: u16,
    lan_ports: Option<(u16, u16)>,
    api_port: Option<u16>,
    settings: &XraySettings,
) -> Result<PreparedConfig, String> {
    let payload = decode_subscription(raw_subscription);

    if let Ok(value) = serde_json::from_str::<Value>(&payload) {
        if is_xray_config(&value) {
            return prepare_from_config(value, socks_port, http_port, lan_ports, settings);
        }
    }

    let uri = select_proxy_uri(&payload, selected_node_key)?;
    let (mut selected_outbound, node_name) = outbound_from_uri(&uri)?;
    apply_mux(&mut selected_outbound, settings);

    let (outbounds, hot_switch, node_name) = match api_port {
        Some(api_port) => {
            let (outbounds, nodes) = node_outbounds(&payload, settings);
            let selected = selected_node_key
                .and_then(|key| nodes.iter().find(|node| node.key == key))
                .or_else(|| nodes.first())
                .ok_or_else(|| {
                    "The subscription has no nodes that Xray-core can run.".to_string()
                })?;
            let selected_tag = selected.tag.clone();
            let node_name = selected.name.clone().or(node_name);
            (
                outbounds,
                Some(HotSwitch {
                    api_port,
                    balancer: HOT_SWITCH_BALANCER.to_string(),
                    selected_tag,
                    nodes,
                }),
                node_name,
            )
        }
        None => (vec![selected_outbound], None, node_name),
    };

    Ok(PreparedConfig {
        config: base_config(
            outbounds,
            socks_port,
            http_port,
            lan_ports,
            managed_geo_routing(settings, api_port.is_some()),
            settings,
            api_port,
        ),
        socks_port: Some(socks_port),
        http_port: Some(http_port),
        lan_socks_port: lan_ports.map(|ports| ports.0),
        lan_http_port: lan_ports.map(|ports| ports.1),
        node_name,
        hot_switch,
    })
}

/// Builds a config for a temporary test core.
///
/// Unlike [`prepare`], every inbound is rebound to a local ephemeral port. This
/// lets a test instance run next to the main core (or another test) even when
/// the subscription ships a full Xray config with fixed inbound ports.
pub fn prepare_for_test(
    raw_subscription: &str,
    selected_node_key: Option<&str>,
    socks_port: u16,
    http_port: u16,
) -> Result<PreparedConfig, String> {
    let payload = decode_subscription(raw_subscription);
    if let Ok(value) = serde_json::from_str::<Value>(&payload) {
        if is_xray_config(&value) {
            return prepare_test_config(value, socks_port, http_port);
        }
    }

    // Test cores only probe latency/throughput, so they skip the geo routing
    // used by a real node. That avoids loading the geo databases in every
    // temporary core (up to 30 at once).
    let uri = select_proxy_uri(&payload, selected_node_key)?;
    let (outbound, node_name) = outbound_from_uri(&uri)?;
    Ok(PreparedConfig {
        config: base_config(
            vec![outbound],
            socks_port,
            http_port,
            None,
            json!({"domainStrategy": "AsIs", "rules": []}),
            &XraySettings::default(),
            None,
        ),
        socks_port: Some(socks_port),
        http_port: Some(http_port),
        lan_socks_port: None,
        lan_http_port: None,
        node_name,
        hot_switch: None,
    })
}

fn is_xray_config(value: &Value) -> bool {
    value
        .get("outbounds")
        .and_then(Value::as_array)
        .is_some_and(|outbounds| {
            outbounds
                .iter()
                .any(|outbound| outbound.get("protocol").and_then(Value::as_str).is_some())
        })
}

fn prepare_from_config(
    mut value: Value,
    socks_port: u16,
    http_port: u16,
    lan_ports: Option<(u16, u16)>,
    settings: &XraySettings,
) -> Result<PreparedConfig, String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "The Xray config must be a JSON object.".to_string())?;

    // A full subscription config keeps its own routing and outbounds, but the
    // application-wide log level is still ours to apply.
    let log = object.entry("log".to_string()).or_insert_with(|| json!({}));
    if let Some(log) = log.as_object_mut() {
        log.insert("loglevel".to_string(), json!(settings.log_level));
    }

    let has_inbounds = object
        .get("inbounds")
        .and_then(Value::as_array)
        .is_some_and(|inbounds| !inbounds.is_empty());

    let (socks_port, http_port) = if has_inbounds {
        let socks_port = inbound_port(object, "socks");
        let http_port = inbound_port(object, "http");
        if let Some(ports) = lan_ports {
            // The subscription's own inbounds stay untouched; the LAN pair is
            // added next to them so its credentials never affect local apps.
            if let Some(inbounds) = object.get_mut("inbounds").and_then(Value::as_array_mut) {
                inbounds.extend(lan_inbounds(ports, settings));
            }
        }
        (socks_port, http_port)
    } else {
        object.insert(
            "inbounds".to_string(),
            local_inbounds(socks_port, http_port, lan_ports, settings),
        );
        (Some(socks_port), Some(http_port))
    };

    let node_name = object
        .get("outbounds")
        .and_then(Value::as_array)
        .and_then(|outbounds| outbounds.first())
        .and_then(|outbound| outbound.get("tag"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    Ok(PreparedConfig {
        config: value,
        socks_port,
        http_port,
        lan_socks_port: lan_ports.map(|ports| ports.0),
        lan_http_port: lan_ports.map(|ports| ports.1),
        node_name,
        hot_switch: None,
    })
}

fn prepare_test_config(
    mut value: Value,
    socks_port: u16,
    http_port: u16,
) -> Result<PreparedConfig, String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "The Xray config must be a JSON object.".to_string())?;

    let mut inbounds = object
        .get("inbounds")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut socks_ready = false;
    let mut http_ready = false;
    for inbound in inbounds.iter_mut() {
        let Some(inbound) = inbound.as_object_mut() else {
            continue;
        };
        let protocol = inbound
            .get("protocol")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if protocol == "socks" && !socks_ready {
            set_local_listener(inbound, socks_port);
            socks_ready = true;
        } else if protocol == "http" && !http_ready {
            set_local_listener(inbound, http_port);
            http_ready = true;
        } else if inbound.get("port").and_then(Value::as_u64).is_some() {
            if let Some(port) = ephemeral_port() {
                set_local_listener(inbound, port);
            }
        }
    }
    if !socks_ready {
        inbounds.push(socks_inbound(
            "cloak-socks-in",
            "127.0.0.1",
            socks_port,
            None,
        ));
    }
    if !http_ready {
        inbounds.push(http_inbound("cloak-http-in", "127.0.0.1", http_port, None));
    }
    object.insert("inbounds".to_string(), Value::Array(inbounds));

    let node_name = object
        .get("outbounds")
        .and_then(Value::as_array)
        .and_then(|outbounds| outbounds.first())
        .and_then(|outbound| outbound.get("tag"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    Ok(PreparedConfig {
        config: value,
        socks_port: Some(socks_port),
        http_port: Some(http_port),
        lan_socks_port: None,
        lan_http_port: None,
        node_name,
        hot_switch: None,
    })
}

fn set_local_listener(inbound: &mut Map<String, Value>, port: u16) {
    inbound.insert("listen".to_string(), Value::String("127.0.0.1".to_string()));
    inbound.insert("port".to_string(), Value::from(port));
}

fn ephemeral_port() -> Option<u16> {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .ok()?
        .local_addr()
        .ok()
        .map(|address| address.port())
}

fn inbound_port(object: &Map<String, Value>, protocol: &str) -> Option<u16> {
    object
        .get("inbounds")
        .and_then(Value::as_array)?
        .iter()
        .find(|inbound| inbound.get("protocol").and_then(Value::as_str) == Some(protocol))?
        .get("port")?
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
}

fn local_inbounds(
    socks_port: u16,
    http_port: u16,
    lan_ports: Option<(u16, u16)>,
    settings: &XraySettings,
) -> Value {
    let mut inbounds = vec![
        with_sniffing(
            socks_inbound("cloak-socks-in", "127.0.0.1", socks_port, None),
            settings,
        ),
        with_sniffing(
            http_inbound("cloak-http-in", "127.0.0.1", http_port, None),
            settings,
        ),
    ];
    if let Some(ports) = lan_ports {
        inbounds.extend(lan_inbounds(ports, settings));
    }
    Value::Array(inbounds)
}

/// LAN inbounds listen on every interface and carry the optional credentials.
/// Keeping them separate from the loopback pair means the system proxy and the
/// app's own probes never have to authenticate.
fn lan_inbounds((socks_port, http_port): (u16, u16), settings: &XraySettings) -> Vec<Value> {
    let account = settings.lan_account();
    vec![
        with_sniffing(
            socks_inbound("cloak-socks-lan", "0.0.0.0", socks_port, account),
            settings,
        ),
        with_sniffing(
            http_inbound("cloak-http-lan", "0.0.0.0", http_port, account),
            settings,
        ),
    ]
}

/// Adds sniffing to a proxy inbound when it is enabled.
///
/// Sniffing lets Xray recover the hostname from an HTTP/TLS/QUIC handshake, so
/// apps that resolve names themselves still match the domain rules.
fn with_sniffing(mut inbound: Value, settings: &XraySettings) -> Value {
    if let Some(sniffing) = sniffing_config(settings, None) {
        inbound["sniffing"] = sniffing;
    }
    inbound
}

/// Builds the `sniffing` object for an inbound.
///
/// `required` keeps sniffing on for TUN when FakeDNS needs it even if the user
/// turned sniffing off, because without it a fake IP cannot be mapped back to
/// its domain.
fn sniffing_config(settings: &XraySettings, required: Option<&str>) -> Option<Value> {
    if !settings.sniffing_enabled && required.is_none() {
        return None;
    }
    let mut dest_override = Vec::new();
    if settings.sniff_http {
        dest_override.push("http");
    }
    if settings.sniff_tls {
        dest_override.push("tls");
    }
    if settings.sniff_quic {
        dest_override.push("quic");
    }
    if let Some(protocol) = required {
        dest_override.push(protocol);
    }

    let mut sniffing = json!({
        "enabled": true,
        "destOverride": dest_override,
        "metadataOnly": false
    });
    if settings.sniff_route_only {
        sniffing["routeOnly"] = json!(true);
    }
    Some(sniffing)
}

fn socks_inbound(tag: &str, listen: &str, port: u16, account: Option<(&str, &str)>) -> Value {
    let mut settings = json!({"auth": "noauth", "udp": true});
    if let Some((user, pass)) = account {
        settings["auth"] = json!("password");
        settings["accounts"] = json!([{"user": user, "pass": pass}]);
    }
    json!({
        "tag": tag,
        "listen": listen,
        "port": port,
        "protocol": "socks",
        "settings": settings
    })
}

fn http_inbound(tag: &str, listen: &str, port: u16, account: Option<(&str, &str)>) -> Value {
    let mut settings = json!({});
    if let Some((user, pass)) = account {
        settings["accounts"] = json!([{"user": user, "pass": pass}]);
    }
    json!({
        "tag": tag,
        "listen": listen,
        "port": port,
        "protocol": "http",
        "settings": settings
    })
}

fn api_inbound(port: u16) -> Value {
    json!({
        "tag": "api",
        "listen": "127.0.0.1",
        "port": port,
        "protocol": "dokodemo-door",
        "settings": {"address": "127.0.0.1"}
    })
}

/// Routes the API inbound to Xray's handler and sends everything else that was
/// not matched by the bypass rules through the node balancer, whose selection
/// can be overridden at runtime.
fn add_hot_switch_routing(routing: &mut Value) {
    if !routing.is_object() {
        *routing = json!({});
    }
    let object = routing.as_object_mut().expect("routing is an object");
    let rules = object
        .entry("rules".to_string())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("routing rules are an array");
    rules.insert(
        0,
        json!({"type": "field", "inboundTag": ["api"], "outboundTag": "api"}),
    );
    // A field is required on every rule; `network` makes this one match all
    // proxied traffic without restricting protocols.
    rules.push(json!({
        "type": "field",
        "network": "tcp,udp",
        "balancerTag": HOT_SWITCH_BALANCER
    }));
    object.insert(
        "balancers".to_string(),
        json!([{
            "tag": HOT_SWITCH_BALANCER,
            "selector": [HOT_NODE_TAG_PREFIX]
        }]),
    );
}

fn base_config(
    outbounds: Vec<Value>,
    socks_port: u16,
    http_port: u16,
    lan_ports: Option<(u16, u16)>,
    mut routing: Value,
    settings: &XraySettings,
    api_port: Option<u16>,
) -> Value {
    let mut inbounds = local_inbounds(socks_port, http_port, lan_ports, settings);
    if settings.tun_enabled {
        if let Some(inbounds) = inbounds.as_array_mut() {
            inbounds.push(tun_inbound(settings, system_dns_takeover_available()));
        }
    }
    // The API inbound lets the app override the balancer without a restart.
    if let Some(api_port) = api_port {
        if let Some(inbounds) = inbounds.as_array_mut() {
            inbounds.push(api_inbound(api_port));
        }
    }

    let mut outbounds = outbounds;
    outbounds.push(json!({"tag": "direct", "protocol": "freedom"}));
    outbounds.push(json!({"tag": "block", "protocol": "blackhole"}));
    if settings.tun_enabled {
        // DNS queries captured by the TUN inbound are routed to this outbound
        // so the built-in DNS server (and FakeDNS) can answer them.
        outbounds.push(json!({"tag": "dns-out", "protocol": "dns"}));
    }
    if api_port.is_some() {
        outbounds.push(json!({"tag": "api", "protocol": "freedom"}));
        add_hot_switch_routing(&mut routing);
    }
    apply_fragment(&mut outbounds, settings);

    let mut config = json!({
        "log": {"loglevel": settings.log_level},
        "inbounds": inbounds,
        "outbounds": outbounds,
        "routing": routing
    });
    if api_port.is_some() {
        config["api"] = json!({
            "tag": "api",
            "services": ["HandlerService", "RoutingService"]
        });
    }

    if let Some(dns) = managed_dns(settings) {
        config["dns"] = dns;
    }
    if let Some(fake_dns) = fake_dns_config(settings) {
        config["fakedns"] = fake_dns;
    }
    config
}

/// FakeDNS only has an effect when TUN owns the client's DNS traffic and its
/// sniffing can turn a fake IP back into a domain.
fn fake_dns_active(settings: &XraySettings) -> bool {
    settings.tun_enabled && settings.fake_dns_enabled
}

/// Xray takes over the system resolver through systemd-resolved on Linux.
/// Asking for that takeover when the resolver is absent makes the whole TUN
/// start fail, so it is only requested when resolved is actually running.
fn system_dns_takeover_available() -> bool {
    cfg!(target_os = "linux")
        && std::path::Path::new("/run/systemd/resolve/stub-resolv.conf").exists()
}

fn tun_inbound(settings: &XraySettings, dns_takeover_available: bool) -> Value {
    let mut tun = Map::new();
    let name = settings.tun_name.trim();
    if !name.is_empty() {
        tun.insert("name".to_string(), json!(name));
    }
    tun.insert("mtu".to_string(), json!(settings.tun_mtu));

    let mut gateways = vec![json!(settings.tun_gateway.trim())];
    if settings.ipv6_enabled {
        gateways.push(json!(TUN_IPV6_GATEWAY));
    }
    tun.insert("gateway".to_string(), Value::Array(gateways));

    // This field only takes effect on Windows, where Xray owns the interface
    // DNS. On Linux the DNS hijack rule below handles TUN DNS traffic.
    if settings
        .local_dns
        .trim()
        .parse::<std::net::IpAddr>()
        .is_ok()
    {
        tun.insert("dns".to_string(), json!([settings.local_dns.trim()]));
    }

    if settings.tun_auto_route {
        // Strict routing uses two half-default routes: being more specific than
        // any existing 0.0.0.0/0 or ::/0, they win regardless of its metric.
        let mut routes = if settings.tun_strict_route {
            vec![json!("0.0.0.0/1"), json!("128.0.0.0/1")]
        } else {
            vec![json!("0.0.0.0/0")]
        };
        if settings.ipv6_enabled {
            if settings.tun_strict_route {
                routes.push(json!("::/1"));
                routes.push(json!("8000::/1"));
            } else {
                routes.push(json!("::/0"));
            }
        }
        tun.insert("autoSystemRoutingTable".to_string(), Value::Array(routes));
        // Without this Xray's own outbound traffic can be routed back into the
        // TUN device and loop forever.
        tun.insert("autoOutboundsInterface".to_string(), json!("auto"));
    }

    if fake_dns_active(settings) && dns_takeover_available {
        // The OS resolver usually lives on loopback and would bypass the TUN;
        // pointing it at the gateway lets FakeDNS see every query.
        tun.insert("autoSystemDnsToGateway".to_string(), json!(true));
    }

    let mut inbound = json!({
        "tag": "cloak-tun-in",
        "protocol": "tun",
        "settings": Value::Object(tun)
    });
    // FakeDNS cannot work without sniffing, so it stays on even when the user
    // disabled sniffing for the proxy inbounds.
    if let Some(sniffing) =
        sniffing_config(settings, fake_dns_active(settings).then_some("fakedns"))
    {
        inbound["sniffing"] = sniffing;
    }
    inbound
}

/// Chains every proxy outbound through a freedom outbound that splits its TLS
/// ClientHello into fragments, which some networks use to detect (and block)
/// proxy connections.
///
/// The fragment outbound is appended last so it never becomes the default
/// outbound; it is only reachable through `sockopt.dialerProxy`.
fn apply_fragment(outbounds: &mut Vec<Value>, settings: &XraySettings) {
    if !settings.fragment_enabled {
        return;
    }

    let mut applied = false;
    for outbound in outbounds.iter_mut() {
        let Some(object) = outbound.as_object_mut() else {
            continue;
        };
        let protocol = object
            .get("protocol")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !PROXY_PROTOCOLS.contains(&protocol) {
            continue;
        }
        let stream = object.entry("streamSettings").or_insert_with(|| json!({}));
        let Some(stream) = stream.as_object_mut() else {
            continue;
        };
        let sockopt = stream.entry("sockopt").or_insert_with(|| json!({}));
        let Some(sockopt) = sockopt.as_object_mut() else {
            continue;
        };
        // A subscription-provided dialer proxy wins over the built-in one.
        if sockopt.contains_key("dialerProxy") {
            continue;
        }
        sockopt.insert("dialerProxy".to_string(), json!(FRAGMENT_TAG));
        applied = true;
    }

    if applied {
        outbounds.push(json!({
            "tag": FRAGMENT_TAG,
            "protocol": "freedom",
            "settings": {
                "fragment": {
                    "packets": settings.fragment_packets.trim(),
                    "length": settings.fragment_length.trim(),
                    "interval": settings.fragment_interval.trim()
                }
            }
        }));
    }
}

fn managed_dns(settings: &XraySettings) -> Option<Value> {
    let has_geosite = geo_asset_directory().join("geosite.dat").is_file();
    dns_config(settings, has_geosite)
}

fn dns_config(settings: &XraySettings, has_geosite: bool) -> Option<Value> {
    let local_dns = settings.local_dns.trim();
    let remote_dns = settings.remote_dns.trim();
    let fake_dns = fake_dns_active(settings);

    // Keep the previous behaviour (system DNS, both address families) when no
    // custom DNS option is in play.
    if local_dns.is_empty() && remote_dns.is_empty() && !fake_dns && settings.ipv6_enabled {
        return None;
    }

    let mut bypass_domains: Vec<String> = Vec::new();
    if has_geosite {
        if settings.bypass_private {
            bypass_domains.push("geosite:private".to_string());
        }
        if settings.bypass_cn {
            bypass_domains.push("geosite:cn".to_string());
        }
    }
    bypass_domains.extend(bypass_entries(&settings.bypass_list).domains);
    dedupe(&mut bypass_domains);

    let mut servers = Vec::new();

    // Bypassed domains must resolve through the local resolver: the proxy's
    // DNS would answer with an address from the exit's region, which is wrong
    // for traffic that is about to connect directly.
    let scoped_local = if !local_dns.is_empty() {
        Some(local_dns)
    } else if fake_dns || !remote_dns.is_empty() {
        // With no local server configured, the system resolver still has to
        // answer bypassed domains instead of the fake/remote one.
        Some("localhost")
    } else {
        None
    };
    if let Some(resolver) = scoped_local {
        if !bypass_domains.is_empty() {
            servers.push(json!({
                "address": resolver,
                "domains": bypass_domains,
                "skipFallback": true
            }));
        } else if remote_dns.is_empty() {
            // Nothing to scope it to, so the local resolver answers everything.
            servers.push(json!(resolver));
        }
    }

    if fake_dns {
        // FakeDNS answers proxied domains itself; a remote resolver would never
        // be asked for them.
        servers.push(json!("fakedns"));
    } else if !remote_dns.is_empty() {
        servers.push(json!(remote_dns));
    }

    // Without a remote resolver the system resolver is the final fallback for
    // everything that was not bypassed. With one, using the system resolver as
    // a fallback would leak those queries.
    if remote_dns.is_empty() || fake_dns {
        servers.push(json!("localhost"));
    }

    Some(json!({
        "servers": servers,
        "queryStrategy": if settings.ipv6_enabled { "UseIP" } else { "UseIPv4" }
    }))
}

fn fake_dns_config(settings: &XraySettings) -> Option<Value> {
    if !fake_dns_active(settings) {
        return None;
    }
    let pool = |address: &str| {
        json!({
            "ipPool": address,
            "poolSize": settings.fake_dns_pool_size
        })
    };
    let ipv4 = pool(settings.fake_dns_ip_pool.trim());

    if settings.ipv6_enabled {
        Some(json!([ipv4, pool(DEFAULT_FAKE_DNS_IPV6_POOL)]))
    } else {
        Some(ipv4)
    }
}

/// Adds Xray's outbound multiplexing to a generated proxy outbound.
fn apply_mux(outbound: &mut Value, settings: &XraySettings) {
    let Some(object) = outbound.as_object_mut() else {
        return;
    };
    if settings.mux_enabled {
        object.insert(
            "mux".to_string(),
            json!({"enabled": true, "concurrency": settings.mux_concurrency}),
        );
    }
}

/// Removes repeated entries while keeping their order. Xray accepts
/// duplicates, but they make the generated config harder to read.
fn dedupe(values: &mut Vec<String>) {
    let mut seen = Vec::new();
    values.retain(|value| {
        if seen.contains(value) {
            false
        } else {
            seen.push(value.clone());
            true
        }
    });
}

/// Routing for a core started from this app: private and mainland-China
/// destinations go direct, everything else uses the selected node.
///
/// The rules are built from the GeoIP/GeoSite files managed in Settings. A
/// rule is only emitted for a database that exists because Xray refuses to
/// start when it references a missing `geoip.dat`/`geosite.dat`.
fn managed_geo_routing(settings: &XraySettings, hot_switch: bool) -> Value {
    let directory = geo_asset_directory();
    geo_routing(
        settings,
        directory.join("geosite.dat").is_file(),
        directory.join("geoip.dat").is_file(),
        hot_switch,
    )
}

fn geo_routing(
    settings: &XraySettings,
    has_geosite: bool,
    has_geoip: bool,
    hot_switch: bool,
) -> Value {
    let mut rules = Vec::new();

    // DNS traffic captured by the TUN device is answered by the built-in DNS
    // server. Without this rule queries would be proxied like normal traffic.
    if settings.tun_enabled {
        rules.push(json!({
            "type": "field",
            "inboundTag": ["cloak-tun-in"],
            "port": 53,
            "outboundTag": "dns-out"
        }));
    }

    // User rules come before the automatic bypass rules so an entry here can
    // override "Bypass mainland China" or the custom bypass list.
    for rule in outbound_rules(&settings.outbound_rules) {
        let mut value = Map::new();
        value.insert("type".to_string(), json!("field"));
        match rule.matcher {
            RuleMatcher::Domain(domain) => {
                value.insert("domain".to_string(), json!([domain]));
            }
            RuleMatcher::Ip(ip) => {
                value.insert("ip".to_string(), json!([ip]));
            }
        }
        if rule.target == OutboundTarget::Proxy && hot_switch {
            // The single proxy outbound is replaced by one outbound per node,
            // so the balancer is the tag that still means "the proxy".
            value.insert("balancerTag".to_string(), json!(HOT_SWITCH_BALANCER));
        } else {
            value.insert("outboundTag".to_string(), json!(rule.target.tag()));
        }
        rules.push(Value::Object(value));
    }

    // Ads are matched before the direct rules so a blocked domain is not sent
    // direct just because it is also part of a bypassed region list.
    if settings.block_ads && has_geosite {
        rules.push(json!({
            "type": "field",
            "outboundTag": "block",
            "domain": ["geosite:category-ads-all"]
        }));
    }

    // QUIC is blocked before the direct rules too: the point is to make every
    // app use TCP, including apps talking to a bypassed destination.
    if settings.block_quic {
        rules.push(json!({
            "type": "field",
            "network": "udp",
            "port": 443,
            "outboundTag": "block"
        }));
    }

    let custom = bypass_entries(&settings.bypass_list);

    let mut direct_domains: Vec<String> = Vec::new();
    if has_geosite {
        if settings.bypass_private {
            direct_domains.push("geosite:private".to_string());
        }
        if settings.bypass_cn {
            direct_domains.push("geosite:cn".to_string());
        }
    }
    direct_domains.extend(custom.domains);
    // A custom entry may repeat a geo list (for example `geoip:private` while
    // "Bypass private networks" is on), which would otherwise appear twice.
    dedupe(&mut direct_domains);
    if !direct_domains.is_empty() {
        rules.push(json!({
            "type": "field",
            "outboundTag": "direct",
            "domain": direct_domains
        }));
    }

    let mut direct_ips: Vec<String> = Vec::new();
    if has_geoip {
        if settings.bypass_private {
            direct_ips.push("geoip:private".to_string());
        }
        if settings.bypass_cn {
            direct_ips.push("geoip:cn".to_string());
        }
    }
    direct_ips.extend(custom.ips);
    dedupe(&mut direct_ips);
    if !direct_ips.is_empty() {
        rules.push(json!({
            "type": "field",
            "outboundTag": "direct",
            "ip": direct_ips
        }));
    }

    // With FakeDNS the destinations are synthetic addresses, so resolving
    // domains again for IP rules would route based on the fake pool.
    let domain_strategy = if fake_dns_active(settings) {
        "AsIs"
    } else {
        settings.domain_strategy.as_str()
    };

    json!({
        "domainStrategy": domain_strategy,
        "rules": rules
    })
}

/// Returns true when Xray's config loader would reject this node outbound.
///
/// Xray 26 prohibits VLESS and Trojan without TLS/reality unless the server
/// address is a hostname or a private/special IP, and it cannot build a
/// reality outbound without a public key. One such node in a subscription
/// would otherwise make the entire hot-switch config fail to load.
fn outbound_is_unbuildable(outbound: &Value) -> bool {
    let protocol = outbound
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let security = outbound
        .pointer("/streamSettings/security")
        .and_then(Value::as_str)
        .unwrap_or("none");

    if security == "reality" {
        let public_key = outbound
            .pointer("/streamSettings/realitySettings/publicKey")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return public_key.trim().is_empty();
    }
    if security == "tls" || !matches!(protocol, "vless" | "trojan") {
        return false;
    }

    let address = outbound
        .pointer("/settings/vnext/0/address")
        .or_else(|| outbound.pointer("/settings/servers/0/address"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    address
        .parse::<IpAddr>()
        .is_ok_and(|address| !is_private_address(address))
}

/// Mirrors the address ranges Xray treats as private when it checks whether a
/// plain-text node is allowed: hostnames are always fine, and these IP ranges
/// (including documentation, link-local, multicast and CGNAT) are exempt.
fn is_private_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let octets = address.octets();
            address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_broadcast()
                || address.is_documentation()
                || address.is_unspecified()
                || address.is_multicast()
                || octets[0] == 0
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                || (octets[0] == 198 && matches!(octets[1], 18 | 19))
                || octets[0] >= 240
        }
        IpAddr::V6(address) => {
            address.is_loopback()
                || address.is_unspecified()
                || address.is_unique_local()
                || address.is_unicast_link_local()
                || address.is_multicast()
        }
    }
}

/// Builds one outbound per distinct node so the balancer can switch between
/// them without restarting the core.
///
/// Nodes whose protocol Xray cannot run are skipped; the selected node was
/// already validated by [`select_proxy_uri`], so it is always present here.
fn node_outbounds(payload: &str, settings: &XraySettings) -> (Vec<Value>, Vec<HotNode>) {
    let mut outbounds = Vec::new();
    let mut nodes = Vec::new();
    let mut seen = Vec::new();

    for line in payload
        .lines()
        .map(|line| line.trim().trim_start_matches('\u{feff}'))
    {
        let Some(node) = parse_proxy_uri(line) else {
            continue;
        };
        let key = subscription_node_key(&node);
        if seen.contains(&key) {
            continue;
        }
        let Ok((mut outbound, _)) = outbound_from_uri(line) else {
            continue;
        };
        // Xray refuses to start with an outbound it cannot build, which would
        // take every other node down with it.
        if outbound_is_unbuildable(&outbound) {
            continue;
        }
        apply_mux(&mut outbound, settings);
        let tag = format!("{HOT_NODE_TAG_PREFIX}{}", outbounds.len());
        outbound["tag"] = serde_json::json!(tag);
        seen.push(key.clone());
        nodes.push(HotNode {
            key,
            name: Some(node.name),
            tag: tag.clone(),
        });
        outbounds.push(outbound);
    }

    (outbounds, nodes)
}

fn select_proxy_uri(payload: &str, selected_node_key: Option<&str>) -> Result<String, String> {
    let proxies = payload
        .lines()
        .map(|line| line.trim().trim_start_matches('\u{feff}'))
        .filter_map(|line| parse_proxy_uri(line).map(|node| (line.to_string(), node)))
        .collect::<Vec<_>>();

    if proxies.is_empty() {
        return Err(
            "The subscription contains no proxy links Xray can run. Xray JSON configs and \
             proxy URI lists are supported; Clash YAML subscriptions are not supported yet."
                .to_string(),
        );
    }

    if let Some(key) = selected_node_key {
        if let Some((uri, _)) = proxies
            .iter()
            .find(|(_, node)| subscription_node_key(node) == key)
        {
            return Ok(uri.clone());
        }
    }

    Ok(proxies[0].0.clone())
}

fn outbound_from_uri(uri: &str) -> Result<(Value, Option<String>), String> {
    let (scheme, _) = uri
        .split_once("://")
        .ok_or_else(|| format!("Invalid proxy link: {uri}"))?;
    match scheme.to_ascii_lowercase().as_str() {
        "vless" => vless_outbound(uri),
        "vmess" => vmess_outbound(uri),
        "trojan" => trojan_outbound(uri),
        "ss" => shadowsocks_outbound(uri),
        "socks" | "socks5" => socks_outbound(uri),
        "http" | "https" => http_outbound(uri),
        other => Err(format!("{other} nodes are not supported by Xray-core.")),
    }
}

struct LinkParts {
    userinfo: String,
    host: String,
    port: u16,
    query: Vec<(String, String)>,
    name: Option<String>,
}

fn parse_link(uri: &str) -> Result<LinkParts, String> {
    let (_, rest) = uri
        .split_once("://")
        .ok_or_else(|| format!("Invalid proxy link: {uri}"))?;
    let (rest, name) = match rest.split_once('#') {
        Some((rest, fragment)) => {
            let name = percent_decode(fragment);
            (rest, Some(name).filter(|name| !name.is_empty()))
        }
        None => (rest, None),
    };
    let (authority, query_text) = rest.split_once('?').unwrap_or((rest, ""));
    let (userinfo, host_port) = match authority.rsplit_once('@') {
        Some((userinfo, host_port)) => (percent_decode(userinfo), host_port),
        None => (String::new(), authority),
    };
    let (host, port) = split_host_port(host_port)?;
    Ok(LinkParts {
        userinfo,
        host,
        port,
        query: parse_query(query_text),
        name,
    })
}

fn split_host_port(authority: &str) -> Result<(String, u16), String> {
    let authority = authority.split('/').next().unwrap_or(authority);
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest
            .split_once(']')
            .ok_or_else(|| format!("Invalid IPv6 address in proxy link: {authority}"))?;
        let port = after
            .strip_prefix(':')
            .ok_or_else(|| format!("Missing port in proxy link: {authority}"))?
            .parse()
            .map_err(|_| format!("Invalid port in proxy link: {authority}"))?;
        Ok((host.to_string(), port))
    } else {
        let (host, port) = authority
            .rsplit_once(':')
            .ok_or_else(|| format!("Missing port in proxy link: {authority}"))?;
        let port = port
            .parse()
            .map_err(|_| format!("Invalid port in proxy link: {authority}"))?;
        Ok((host.to_string(), port))
    }
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect()
}

fn query_value<'a>(query: &'a [(String, String)], key: &str) -> Option<&'a str> {
    query
        .iter()
        .find(|(entry_key, _)| entry_key == key)
        .map(|(_, value)| value.as_str())
        .filter(|value| !value.is_empty())
}

fn percent_decode(value: &str) -> String {
    percent_encoding::percent_decode_str(value)
        .decode_utf8_lossy()
        .into_owned()
}

fn decode_base64(value: &str) -> Option<String> {
    use base64::Engine;

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

#[derive(Default)]
struct StreamOptions {
    network: String,
    security: String,
    host: Option<String>,
    path: Option<String>,
    sni: Option<String>,
    fingerprint: Option<String>,
    alpn: Option<String>,
    header_type: Option<String>,
    service_name: Option<String>,
    public_key: Option<String>,
    short_id: Option<String>,
    spider_x: Option<String>,
    ech: Option<String>,
}

fn stream_options_from_query(query: &[(String, String)]) -> StreamOptions {
    let security = query_value(query, "security").unwrap_or("none").to_string();
    StreamOptions {
        network: normalize_network(
            query_value(query, "type")
                .or_else(|| query_value(query, "network"))
                .unwrap_or("tcp"),
        ),
        security: normalize_security(&security),
        host: query_value(query, "host").map(str::to_owned),
        path: query_value(query, "path").map(str::to_owned),
        sni: query_value(query, "sni")
            .or_else(|| query_value(query, "peer"))
            .map(str::to_owned),
        fingerprint: query_value(query, "fp").map(str::to_owned),
        alpn: query_value(query, "alpn").map(str::to_owned),
        header_type: query_value(query, "headerType").map(str::to_owned),
        service_name: query_value(query, "serviceName").map(str::to_owned),
        public_key: query_value(query, "pbk").map(str::to_owned),
        short_id: query_value(query, "sid").map(str::to_owned),
        spider_x: query_value(query, "spx").map(str::to_owned),
        ech: query_value(query, "ech").map(str::to_owned),
    }
}

fn normalize_network(network: &str) -> String {
    match network {
        "" => "tcp".to_string(),
        "h2" => "http".to_string(),
        other => other.to_string(),
    }
}

fn normalize_security(security: &str) -> String {
    match security {
        "tls" | "reality" => security.to_string(),
        _ => "none".to_string(),
    }
}

fn build_stream_settings(options: &StreamOptions) -> Value {
    let mut settings = Map::new();
    settings.insert("network".to_string(), json!(options.network));
    settings.insert("security".to_string(), json!(options.security));

    match options.network.as_str() {
        "ws" => {
            let mut ws = Map::new();
            if let Some(path) = &options.path {
                ws.insert("path".to_string(), json!(path));
            }
            if let Some(host) = &options.host {
                ws.insert("headers".to_string(), json!({"Host": host}));
            }
            settings.insert("wsSettings".to_string(), Value::Object(ws));
        }
        "httpupgrade" => {
            let mut upgrade = Map::new();
            if let Some(path) = &options.path {
                upgrade.insert("path".to_string(), json!(path));
            }
            if let Some(host) = &options.host {
                upgrade.insert("host".to_string(), json!(host));
            }
            settings.insert("httpupgradeSettings".to_string(), Value::Object(upgrade));
        }
        "splithttp" => {
            let mut split = Map::new();
            if let Some(path) = &options.path {
                split.insert("path".to_string(), json!(path));
            }
            if let Some(host) = &options.host {
                split.insert("host".to_string(), json!(host));
            }
            settings.insert("splithttpSettings".to_string(), Value::Object(split));
        }
        "grpc" => {
            let service = options
                .service_name
                .clone()
                .or_else(|| options.path.clone())
                .unwrap_or_default();
            settings.insert("grpcSettings".to_string(), json!({"serviceName": service}));
        }
        "http" => {
            let mut http = Map::new();
            if let Some(path) = &options.path {
                http.insert("path".to_string(), json!(path));
            }
            if let Some(host) = &options.host {
                http.insert("host".to_string(), json!([host]));
            }
            settings.insert("httpSettings".to_string(), Value::Object(http));
        }
        "tcp" if options.header_type.as_deref() == Some("http") => {
            settings.insert(
                "tcpSettings".to_string(),
                json!({
                    "header": {
                        "type": "http",
                        "request": {
                            "path": [options.path.clone().unwrap_or_else(|| "/".to_string())],
                            "headers": {
                                "Host": [options.host.clone().unwrap_or_default()]
                            }
                        }
                    }
                }),
            );
        }
        _ => {}
    }

    match options.security.as_str() {
        "tls" => {
            let mut tls = Map::new();
            if let Some(sni) = &options.sni {
                tls.insert("serverName".to_string(), json!(sni));
            }
            if let Some(fingerprint) = &options.fingerprint {
                tls.insert("fingerprint".to_string(), json!(fingerprint));
            }
            if let Some(alpn) = &options.alpn {
                tls.insert("alpn".to_string(), json!(split_csv(alpn)));
            }
            // Xray resolves "domain+https://doh/dns-query" itself and injects
            // the ECH config into the ClientHello.
            if let Some(ech) = &options.ech {
                tls.insert("echConfigList".to_string(), json!(ech));
            }
            settings.insert("tlsSettings".to_string(), Value::Object(tls));
        }
        "reality" => {
            let mut reality = Map::new();
            if let Some(sni) = &options.sni {
                reality.insert("serverName".to_string(), json!(sni));
            }
            if let Some(fingerprint) = &options.fingerprint {
                reality.insert("fingerprint".to_string(), json!(fingerprint));
            }
            if let Some(public_key) = &options.public_key {
                reality.insert("publicKey".to_string(), json!(public_key));
            }
            if let Some(short_id) = &options.short_id {
                reality.insert("shortId".to_string(), json!(short_id));
            }
            if let Some(spider_x) = &options.spider_x {
                reality.insert("spiderX".to_string(), json!(spider_x));
            }
            settings.insert("realitySettings".to_string(), Value::Object(reality));
        }
        _ => {}
    }

    Value::Object(settings)
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

fn outbound(tag: &str, protocol: &str, settings: Value, stream: Option<Value>) -> Value {
    let mut outbound = Map::new();
    outbound.insert("tag".to_string(), json!(tag));
    outbound.insert("protocol".to_string(), json!(protocol));
    outbound.insert("settings".to_string(), settings);
    if let Some(stream) = stream {
        outbound.insert("streamSettings".to_string(), stream);
    }
    Value::Object(outbound)
}

fn vless_outbound(uri: &str) -> Result<(Value, Option<String>), String> {
    let link = parse_link(uri)?;
    if link.userinfo.is_empty() {
        return Err("VLESS link is missing its user id.".to_string());
    }

    let mut user = Map::new();
    user.insert("id".to_string(), json!(link.userinfo));
    user.insert(
        "encryption".to_string(),
        json!(query_value(&link.query, "encryption").unwrap_or("none")),
    );
    if let Some(flow) = query_value(&link.query, "flow") {
        user.insert("flow".to_string(), json!(flow));
    }

    let settings = json!({
        "vnext": [{
            "address": link.host,
            "port": link.port,
            "users": [Value::Object(user)]
        }]
    });
    let stream = build_stream_settings(&stream_options_from_query(&link.query));
    Ok((
        outbound("proxy", "vless", settings, Some(stream)),
        link.name,
    ))
}

fn vmess_outbound(uri: &str) -> Result<(Value, Option<String>), String> {
    let encoded = uri
        .split_once("://")
        .map(|(_, rest)| rest)
        .ok_or_else(|| format!("Invalid proxy link: {uri}"))?;
    let encoded = encoded.split(['#', '?']).next().unwrap_or(encoded);
    let decoded = decode_base64(encoded)
        .ok_or_else(|| "VMess link payload is not valid base64.".to_string())?;
    let payload: Value = serde_json::from_str(&decoded)
        .map_err(|error| format!("Invalid VMess link payload: {error}"))?;

    let address = string_field(&payload, "add")
        .ok_or_else(|| "VMess link is missing its server address.".to_string())?;
    let port = port_field(&payload, "port")?;
    let id = string_field(&payload, "id")
        .ok_or_else(|| "VMess link is missing its user id.".to_string())?;
    let alter_id = payload
        .get("aid")
        .map(|value| match value {
            Value::Number(number) => number.as_u64().unwrap_or(0),
            Value::String(text) => text.parse().unwrap_or(0),
            _ => 0,
        })
        .unwrap_or(0);
    let security = string_field(&payload, "scy").unwrap_or_else(|| "auto".to_string());
    let tls = string_field(&payload, "tls").unwrap_or_default();
    let host = string_field(&payload, "host");
    let options = StreamOptions {
        network: normalize_network(&string_field(&payload, "net").unwrap_or_else(|| "tcp".into())),
        security: normalize_security(if tls == "tls" || tls == "reality" {
            tls.as_str()
        } else {
            "none"
        }),
        host: host.clone(),
        path: string_field(&payload, "path"),
        sni: string_field(&payload, "sni").or(host),
        fingerprint: string_field(&payload, "fp"),
        alpn: string_field(&payload, "alpn"),
        header_type: string_field(&payload, "type"),
        service_name: string_field(&payload, "serviceName"),
        ..StreamOptions::default()
    };

    let settings = json!({
        "vnext": [{
            "address": address,
            "port": port,
            "users": [{
                "id": id,
                "alterId": alter_id,
                "security": security
            }]
        }]
    });
    let stream = build_stream_settings(&options);
    Ok((
        outbound("proxy", "vmess", settings, Some(stream)),
        string_field(&payload, "ps"),
    ))
}

fn trojan_outbound(uri: &str) -> Result<(Value, Option<String>), String> {
    let link = parse_link(uri)?;
    if link.userinfo.is_empty() {
        return Err("Trojan link is missing its password.".to_string());
    }

    let mut options = stream_options_from_query(&link.query);
    if query_value(&link.query, "security").is_none() {
        options.security = "tls".to_string();
    }
    if options.sni.is_none() {
        options.sni = Some(link.host.clone());
    }

    let settings = json!({
        "servers": [{
            "address": link.host,
            "port": link.port,
            "password": link.userinfo
        }]
    });
    let stream = build_stream_settings(&options);
    Ok((
        outbound("proxy", "trojan", settings, Some(stream)),
        link.name,
    ))
}

fn shadowsocks_outbound(uri: &str) -> Result<(Value, Option<String>), String> {
    let (_, rest) = uri
        .split_once("://")
        .ok_or_else(|| format!("Invalid proxy link: {uri}"))?;
    let (rest, name) = match rest.split_once('#') {
        Some((rest, fragment)) => {
            let name = percent_decode(fragment);
            (rest, Some(name).filter(|name| !name.is_empty()))
        }
        None => (rest, None),
    };
    let (authority, query_text) = rest.split_once('?').unwrap_or((rest, ""));
    if parse_query(query_text)
        .iter()
        .any(|(key, value)| key == "plugin" && !value.is_empty())
    {
        return Err("Shadowsocks plugin links are not supported by Xray-core yet.".to_string());
    }

    let (method, password, host, port) =
        if let Some((userinfo, host_port)) = authority.rsplit_once('@') {
            let credentials = decode_base64(userinfo)
                .filter(|decoded| decoded.contains(':'))
                .unwrap_or_else(|| percent_decode(userinfo));
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

    let settings = json!({
        "servers": [{
            "address": host,
            "port": port,
            "method": method,
            "password": password
        }]
    });
    Ok((outbound("proxy", "shadowsocks", settings, None), name))
}

fn socks_outbound(uri: &str) -> Result<(Value, Option<String>), String> {
    let link = parse_link(uri)?;
    let mut server = Map::new();
    server.insert("address".to_string(), json!(link.host));
    server.insert("port".to_string(), json!(link.port));
    if !link.userinfo.is_empty() {
        let (user, pass) = split_credentials(&link.userinfo);
        server.insert("users".to_string(), json!([{"user": user, "pass": pass}]));
    }
    let settings = json!({"servers": [Value::Object(server)]});
    Ok((outbound("proxy", "socks", settings, None), link.name))
}

fn http_outbound(uri: &str) -> Result<(Value, Option<String>), String> {
    let link = parse_link(uri)?;
    let mut server = Map::new();
    server.insert("address".to_string(), json!(link.host));
    server.insert("port".to_string(), json!(link.port));
    if !link.userinfo.is_empty() {
        let (user, pass) = split_credentials(&link.userinfo);
        server.insert("users".to_string(), json!([{"user": user, "pass": pass}]));
    }

    let is_tls = uri
        .split_once("://")
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("https"));
    let stream = is_tls.then(|| {
        json!({
            "network": "tcp",
            "security": "tls",
            "tlsSettings": {"serverName": link.host}
        })
    });
    let settings = json!({"servers": [Value::Object(server)]});
    Ok((outbound("proxy", "http", settings, stream), link.name))
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

fn string_field(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn port_field(value: &Value, key: &str) -> Result<u16, String> {
    match value
        .get(key)
        .ok_or_else(|| "VMess link is missing its server port.".to_string())?
    {
        Value::Number(number) => number
            .as_u64()
            .and_then(|port| u16::try_from(port).ok())
            .ok_or_else(|| "VMess link has an invalid port.".to_string()),
        Value::String(text) => text
            .parse()
            .map_err(|_| "VMess link has an invalid port.".to_string()),
        _ => Err("VMess link is missing its server port.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hot_switch_configs_put_every_node_behind_a_balancer() {
        let payload = "vless://id@a.example:443?security=none#A\n\
                       vless://id@b.example:443?security=none#B\n\
                       hysteria2://secret@c.example:443#C";
        let second = parse_proxy_uri("vless://id@b.example:443?security=none#B").unwrap();
        let second_key = subscription_node_key(&second);

        let prepared = prepare_with_settings(
            payload,
            Some(&second_key),
            2080,
            2081,
            None,
            Some(10085),
            &XraySettings::default(),
        )
        .unwrap();

        let hot = prepared.hot_switch.as_ref().unwrap();
        assert_eq!(hot.api_port, 10085);
        assert_eq!(hot.balancer, HOT_SWITCH_BALANCER);
        // The unsupported hysteria2 link is skipped instead of blocking the
        // whole subscription, and the selected node is tag 1.
        assert_eq!(hot.nodes.len(), 2);
        assert_eq!(hot.nodes[0].tag, "cloak-node-0");
        assert_eq!(hot.nodes[1].tag, "cloak-node-1");
        assert_eq!(hot.selected_tag, "cloak-node-1");

        let outbounds = prepared.config["outbounds"].as_array().unwrap();
        assert_eq!(outbounds[0]["tag"], "cloak-node-0");
        assert_eq!(outbounds[0]["protocol"], "vless");
        assert_eq!(outbounds[1]["tag"], "cloak-node-1");
        assert!(outbounds.iter().any(|outbound| outbound["tag"] == "direct"));
        assert!(outbounds.iter().any(|outbound| outbound["tag"] == "api"));

        assert_eq!(prepared.config["api"]["tag"], "api");
        let inbounds = prepared.config["inbounds"].as_array().unwrap();
        assert!(inbounds.iter().any(|inbound| {
            inbound["tag"] == "api"
                && inbound["protocol"] == "dokodemo-door"
                && inbound["port"] == 10085
                && inbound["listen"] == "127.0.0.1"
        }));

        let routing = &prepared.config["routing"];
        assert_eq!(routing["balancers"][0]["tag"], HOT_SWITCH_BALANCER);
        assert_eq!(
            routing["balancers"][0]["selector"],
            json!([HOT_NODE_TAG_PREFIX])
        );
        let rules = routing["rules"].as_array().unwrap();
        // The API rule comes first, and the balancer catches everything the
        // bypass rules above it did not match.
        assert_eq!(rules.first().unwrap()["inboundTag"], json!(["api"]));
        let last = rules.last().unwrap();
        assert_eq!(last["balancerTag"], HOT_SWITCH_BALANCER);
        assert_eq!(last["network"], "tcp,udp");
    }

    #[test]
    fn nodes_xray_cannot_build_are_skipped_instead_of_breaking_the_config() {
        let public = "vless://id@23.227.38.55:443?security=none#Public";
        let public_v6 =
            "vless://id@[2605:9880:200:401:135:7700:ef0a:e05]:54835?security=none#PublicV6";
        let plain_trojan = "trojan://secret@23.227.38.55:443?security=none#PlainTrojan";
        let no_key = "vless://id@no-key.example:443?security=reality#NoKey";
        let reality = "vless://id@reality.example:443?security=reality&pbk=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA&sni=reality.example#Reality";
        let private = "vless://id@10.0.0.5:443?security=none#Private";
        let payload =
            format!("{public}\n{public_v6}\n{plain_trojan}\n{no_key}\n{reality}\n{private}");
        // Selecting a node Xray refuses to build must not keep the core down.
        let selected = parse_proxy_uri(public).unwrap();
        let selected_key = subscription_node_key(&selected);

        let prepared = prepare_with_settings(
            &payload,
            Some(&selected_key),
            2080,
            2081,
            None,
            Some(10085),
            &XraySettings::default(),
        )
        .unwrap();
        let hot = prepared.hot_switch.as_ref().unwrap();

        let names = hot
            .nodes
            .iter()
            .map(|node| node.name.as_deref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["Reality", "Private"]);
        assert_eq!(hot.selected_tag, hot.nodes[0].tag);
        assert_eq!(prepared.node_name.as_deref(), Some("Reality"));

        let outbounds = prepared.config["outbounds"].as_array().unwrap();
        assert!(!outbounds
            .iter()
            .any(|outbound| outbound["settings"]["vnext"][0]["address"] == "23.227.38.55"));
    }

    #[test]
    fn a_subscription_with_no_runnable_nodes_is_reported() {
        let payload = "vless://id@23.227.38.55:443?security=none#Public\n\
                       trojan://secret@23.227.38.55:443?security=none#PlainTrojan";

        let error = prepare_with_settings(
            payload,
            None,
            2080,
            2081,
            None,
            Some(10085),
            &XraySettings::default(),
        )
        .unwrap_err();

        assert!(error.contains("no nodes that Xray-core can run"));
    }

    #[test]
    fn without_an_api_port_the_config_keeps_one_node_and_no_balancer() {
        let prepared = prepare(
            "vless://id@a.example:443?security=none#A\nvless://id@b.example:443?security=none#B",
            None,
            2080,
            2081,
        )
        .unwrap();

        assert!(prepared.hot_switch.is_none());
        assert_eq!(prepared.config["outbounds"][0]["tag"], "proxy");
        assert!(prepared.config.get("api").is_none());
        assert!(prepared.config["routing"].get("balancers").is_none());
        assert!(!prepared.config["routing"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule.get("balancerTag").is_some()));
    }

    #[test]
    fn converts_vless_ws_tls_link() {
        let uri = "vless://11111111-1111-1111-1111-111111111111@edge.example.com:443?encryption=none&security=tls&sni=cdn.example.com&type=ws&host=cdn.example.com&path=%2Fws#Tokyo";
        let prepared = prepare(uri, None, 2080, 2081).unwrap();
        let outbound = &prepared.config["outbounds"][0];

        assert_eq!(outbound["protocol"], "vless");
        assert_eq!(
            outbound["settings"]["vnext"][0]["address"],
            "edge.example.com"
        );
        assert_eq!(outbound["settings"]["vnext"][0]["port"], 443);
        assert_eq!(
            outbound["settings"]["vnext"][0]["users"][0]["id"],
            "11111111-1111-1111-1111-111111111111"
        );
        assert_eq!(outbound["streamSettings"]["network"], "ws");
        assert_eq!(outbound["streamSettings"]["security"], "tls");
        assert_eq!(outbound["streamSettings"]["wsSettings"]["path"], "/ws");
        assert_eq!(
            outbound["streamSettings"]["tlsSettings"]["serverName"],
            "cdn.example.com"
        );
        assert_eq!(prepared.node_name.as_deref(), Some("Tokyo"));
        assert_eq!(prepared.socks_port, Some(2080));
        assert_eq!(prepared.http_port, Some(2081));
    }

    #[test]
    fn converts_vless_ws_tls_link_with_ech_and_ignores_allow_insecure() {
        let uri = "vless://11111111-1111-1111-1111-111111111111@edge.example.com:443?encryption=none&security=tls&sni=cdn.example.com&type=ws&host=cdn.example.com&path=%2Fws&allowInsecure=1&ech=cloudflare-ech.com%2Bhttps%3A%2F%2Fdns.alidns.com%2Fdns-query#Tokyo";
        let prepared = prepare(uri, None, 2080, 2081).unwrap();
        let tls = &prepared.config["outbounds"][0]["streamSettings"]["tlsSettings"];

        assert_eq!(
            tls["echConfigList"],
            "cloudflare-ech.com+https://dns.alidns.com/dns-query"
        );
        assert!(tls.get("allowInsecure").is_none());
    }

    #[test]
    fn selects_the_requested_node_from_a_uri_list() {
        let raw = "vless://id-a@a.example:443?security=none#A\nvless://id-b@b.example:443?security=none#B";
        let second = parse_proxy_uri("vless://id-b@b.example:443?security=none#B").unwrap();
        let key = subscription_node_key(&second);

        let prepared = prepare(raw, Some(&key), 2080, 2081).unwrap();
        let outbound = &prepared.config["outbounds"][0];

        assert_eq!(outbound["settings"]["vnext"][0]["address"], "b.example");
        assert_eq!(prepared.node_name.as_deref(), Some("B"));
    }

    #[test]
    fn converts_vmess_base64_json_link() {
        use base64::Engine;

        let payload = json!({
            "v": "2",
            "ps": "Hong Kong",
            "add": "hk.example.com",
            "port": "8443",
            "id": "22222222-2222-2222-2222-222222222222",
            "aid": "0",
            "scy": "auto",
            "net": "ws",
            "type": "none",
            "host": "hk.example.com",
            "path": "/ray",
            "tls": "tls",
            "sni": "hk.example.com"
        })
        .to_string();
        let uri = format!(
            "vmess://{}",
            base64::engine::general_purpose::STANDARD.encode(payload)
        );

        let prepared = prepare(&uri, None, 2080, 2081).unwrap();
        let outbound = &prepared.config["outbounds"][0];

        assert_eq!(outbound["protocol"], "vmess");
        assert_eq!(
            outbound["settings"]["vnext"][0]["address"],
            "hk.example.com"
        );
        assert_eq!(outbound["settings"]["vnext"][0]["port"], 8443);
        assert_eq!(outbound["settings"]["vnext"][0]["users"][0]["alterId"], 0);
        assert_eq!(outbound["streamSettings"]["network"], "ws");
        assert_eq!(outbound["streamSettings"]["wsSettings"]["path"], "/ray");
        assert_eq!(outbound["streamSettings"]["security"], "tls");
        assert_eq!(prepared.node_name.as_deref(), Some("Hong Kong"));
    }

    #[test]
    fn converts_trojan_grpc_link_and_defaults_to_tls() {
        let uri = "trojan://secret@trojan.example.com:443?type=grpc&serviceName=svc#Trojan%20Node";
        let prepared = prepare(uri, None, 2080, 2081).unwrap();
        let outbound = &prepared.config["outbounds"][0];

        assert_eq!(outbound["protocol"], "trojan");
        assert_eq!(outbound["settings"]["servers"][0]["password"], "secret");
        assert_eq!(outbound["streamSettings"]["security"], "tls");
        assert_eq!(
            outbound["streamSettings"]["tlsSettings"]["serverName"],
            "trojan.example.com"
        );
        assert_eq!(
            outbound["streamSettings"]["grpcSettings"]["serviceName"],
            "svc"
        );
        assert_eq!(prepared.node_name.as_deref(), Some("Trojan Node"));
    }

    #[test]
    fn converts_sip002_shadowsocks_link() {
        use base64::Engine;

        let credentials = base64::engine::general_purpose::STANDARD.encode("aes-256-gcm:pass");
        let uri = format!("ss://{credentials}@ss.example.com:8388#SS");
        let prepared = prepare(&uri, None, 2080, 2081).unwrap();
        let server = &prepared.config["outbounds"][0]["settings"]["servers"][0];

        assert_eq!(prepared.config["outbounds"][0]["protocol"], "shadowsocks");
        assert_eq!(server["address"], "ss.example.com");
        assert_eq!(server["port"], 8388);
        assert_eq!(server["method"], "aes-256-gcm");
        assert_eq!(server["password"], "pass");
    }

    #[test]
    fn passes_a_full_xray_config_through_and_reads_existing_inbound_ports() {
        let config = json!({
            "inbounds": [{"protocol": "socks", "port": 1080}],
            "outbounds": [{"protocol": "freedom", "tag": "direct"}]
        });

        let prepared = prepare(&config.to_string(), None, 2080, 2081).unwrap();

        assert_eq!(prepared.socks_port, Some(1080));
        assert_eq!(prepared.config["inbounds"], config["inbounds"]);
        assert_eq!(prepared.config["outbounds"], config["outbounds"]);
    }

    #[test]
    fn test_configs_rebind_full_config_inbounds_away_from_fixed_ports() {
        let config = json!({
            "inbounds": [
                {"protocol": "socks", "port": 1080, "listen": "0.0.0.0"},
                {"protocol": "dokodemo-door", "port": 10085, "tag": "api"}
            ],
            "outbounds": [{"protocol": "freedom", "tag": "direct"}]
        });

        let prepared = prepare_for_test(&config.to_string(), None, 2080, 2081).unwrap();
        let inbounds = prepared.config["inbounds"].as_array().unwrap();

        assert_eq!(prepared.socks_port, Some(2080));
        assert_eq!(inbounds[0]["protocol"], "socks");
        assert_eq!(inbounds[0]["port"], 2080);
        assert_eq!(inbounds[0]["listen"], "127.0.0.1");
        assert_eq!(inbounds[1]["tag"], "api");
        assert_ne!(inbounds[1]["port"], 10085);
        assert_eq!(inbounds[2]["protocol"], "http");
        assert_eq!(inbounds[2]["port"], 2081);
    }

    #[test]
    fn adds_local_inbounds_to_an_xray_outbound_only_config() {
        let config = json!({
            "outbounds": [{"protocol": "vless", "tag": "proxy"}]
        });

        let prepared = prepare(&config.to_string(), None, 2080, 2081).unwrap();
        let inbounds = prepared.config["inbounds"].as_array().unwrap();

        assert_eq!(inbounds.len(), 2);
        assert_eq!(inbounds[0]["protocol"], "socks");
        assert_eq!(inbounds[0]["port"], 2080);
        assert_eq!(inbounds[1]["protocol"], "http");
        assert_eq!(inbounds[1]["port"], 2081);
        assert_eq!(prepared.http_port, Some(2081));
    }

    #[test]
    fn lan_inbounds_listen_on_all_interfaces_without_credentials() {
        let settings = XraySettings {
            lan_enabled: true,
            ..XraySettings::default()
        };
        let prepared = prepare_with_settings(
            "vless://id@a.example:443?security=none#A",
            None,
            2080,
            2081,
            Some((2090, 2091)),
            None,
            &settings,
        )
        .unwrap();
        let inbounds = prepared.config["inbounds"].as_array().unwrap();

        assert_eq!(inbounds.len(), 4);
        // The loopback pair used by the system proxy and probes is unchanged.
        assert_eq!(inbounds[0]["tag"], "cloak-socks-in");
        assert_eq!(inbounds[0]["listen"], "127.0.0.1");
        assert_eq!(inbounds[0]["settings"]["auth"], "noauth");
        assert!(inbounds[0]["settings"].get("accounts").is_none());
        assert_eq!(inbounds[1]["tag"], "cloak-http-in");
        assert_eq!(inbounds[1]["listen"], "127.0.0.1");

        assert_eq!(inbounds[2]["tag"], "cloak-socks-lan");
        assert_eq!(inbounds[2]["protocol"], "socks");
        assert_eq!(inbounds[2]["listen"], "0.0.0.0");
        assert_eq!(inbounds[2]["port"], 2090);
        assert_eq!(inbounds[2]["settings"]["auth"], "noauth");
        assert!(inbounds[2]["settings"].get("accounts").is_none());

        assert_eq!(inbounds[3]["tag"], "cloak-http-lan");
        assert_eq!(inbounds[3]["protocol"], "http");
        assert_eq!(inbounds[3]["listen"], "0.0.0.0");
        assert_eq!(inbounds[3]["port"], 2091);
        assert!(inbounds[3]["settings"].get("accounts").is_none());

        assert_eq!(prepared.lan_socks_port, Some(2090));
        assert_eq!(prepared.lan_http_port, Some(2091));
    }

    #[test]
    fn lan_credentials_only_apply_to_the_lan_inbounds() {
        let settings = XraySettings {
            lan_enabled: true,
            lan_username: "alice".to_string(),
            lan_password: "s3cret".to_string(),
            ..XraySettings::default()
        };
        let prepared = prepare_with_settings(
            "vless://id@a.example:443?security=none#A",
            None,
            2080,
            2081,
            Some((2090, 2091)),
            None,
            &settings,
        )
        .unwrap();
        let inbounds = prepared.config["inbounds"].as_array().unwrap();

        let socks_lan = &inbounds[2];
        assert_eq!(socks_lan["settings"]["auth"], "password");
        assert_eq!(
            socks_lan["settings"]["accounts"],
            json!([{"user": "alice", "pass": "s3cret"}])
        );
        let http_lan = &inbounds[3];
        assert_eq!(
            http_lan["settings"]["accounts"],
            json!([{"user": "alice", "pass": "s3cret"}])
        );

        // Local applications never see the password.
        assert_eq!(inbounds[0]["settings"]["auth"], "noauth");
        assert!(inbounds[0]["settings"].get("accounts").is_none());
        assert!(inbounds[1]["settings"].get("accounts").is_none());
    }

    #[test]
    fn lan_inbounds_are_added_to_full_configs_without_touching_their_own() {
        let config = json!({
            "inbounds": [{
                "tag": "subscription-socks",
                "protocol": "socks",
                "port": 1080,
                "settings": {"auth": "noauth"}
            }],
            "outbounds": [{"protocol": "freedom", "tag": "direct"}]
        });
        let settings = XraySettings {
            lan_enabled: true,
            ..XraySettings::default()
        };
        let prepared = prepare_with_settings(
            &config.to_string(),
            None,
            2080,
            2081,
            Some((2090, 2091)),
            None,
            &settings,
        )
        .unwrap();
        let inbounds = prepared.config["inbounds"].as_array().unwrap();

        assert_eq!(inbounds.len(), 3);
        assert_eq!(inbounds[0]["tag"], "subscription-socks");
        assert_eq!(inbounds[0]["port"], 1080);
        assert_eq!(inbounds[1]["tag"], "cloak-socks-lan");
        assert_eq!(inbounds[2]["tag"], "cloak-http-lan");
        // The reported local ports keep pointing at the subscription's own
        // inbound, not at the LAN pair.
        assert_eq!(prepared.socks_port, Some(1080));
    }

    #[test]
    fn rejects_protocols_xray_does_not_support() {
        let error =
            prepare("hysteria2://secret@hy.example.com:443#HY", None, 2080, 2081).unwrap_err();
        assert!(error.contains("not supported"));
    }

    #[test]
    fn rejects_clash_yaml_subscriptions_with_a_clear_message() {
        let error = prepare(
            "proxies:\n  - name: node-a\n    type: vless\n",
            None,
            2080,
            2081,
        )
        .unwrap_err();
        assert!(error.contains("Clash YAML"));
    }

    #[test]
    fn decodes_base64_encoded_uri_lists() {
        use base64::Engine;

        let encoded = base64::engine::general_purpose::STANDARD.encode(
            "vless://id@a.example:443?security=none#A\nvless://id@b.example:443?security=none#B",
        );
        let prepared = prepare(&encoded, None, 2080, 2081).unwrap();

        assert_eq!(
            prepared.config["outbounds"][0]["settings"]["vnext"][0]["address"],
            "a.example"
        );
    }

    #[test]
    fn geo_routing_uses_the_managed_databases() {
        let routing = geo_routing(&XraySettings::default(), true, true, false);

        assert_eq!(routing["domainStrategy"], "IPIfNonMatch");
        assert_eq!(
            routing["rules"][0]["domain"],
            json!(["geosite:private", "geosite:cn"])
        );
        assert_eq!(routing["rules"][0]["outboundTag"], "direct");
        assert_eq!(
            routing["rules"][1]["ip"],
            json!(["geoip:private", "geoip:cn"])
        );
        assert_eq!(routing["rules"][1]["outboundTag"], "direct");
    }

    #[test]
    fn geo_routing_skips_databases_that_are_not_downloaded() {
        let routing = geo_routing(&XraySettings::default(), false, false, false);
        assert_eq!(routing["rules"], json!([]));

        // A partial download is still usable: Xray must not see a reference
        // to a database that does not exist.
        let only_geosite = geo_routing(&XraySettings::default(), true, false, false);
        let rules = only_geosite["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 1);
        assert!(rules[0].get("ip").is_none());

        let only_geoip = geo_routing(&XraySettings::default(), false, true, false);
        let rules = only_geoip["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["ip"], json!(["geoip:private", "geoip:cn"]));
        assert!(rules[0].get("domain").is_none());
    }

    #[test]
    fn geo_routing_respects_the_route_toggles() {
        let settings = XraySettings {
            bypass_cn: false,
            bypass_private: false,
            ..XraySettings::default()
        };
        let routing = geo_routing(&settings, true, true, false);
        assert_eq!(routing["rules"], json!([]));

        let only_private = XraySettings {
            bypass_cn: false,
            ..XraySettings::default()
        };
        let routing = geo_routing(&only_private, true, true, false);
        let rules = routing["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["domain"], json!(["geosite:private"]));
        assert_eq!(rules[1]["ip"], json!(["geoip:private"]));
    }

    #[test]
    fn geo_routing_blocks_ads_before_bypassing_china() {
        let settings = XraySettings {
            block_ads: true,
            ..XraySettings::default()
        };
        let routing = geo_routing(&settings, true, true, false);
        let rules = routing["rules"].as_array().unwrap();

        assert_eq!(rules[0]["outboundTag"], "block");
        assert_eq!(rules[0]["domain"], json!(["geosite:category-ads-all"]));
        assert_eq!(rules[1]["outboundTag"], "direct");
    }

    #[test]
    fn custom_bypass_entries_become_direct_rules_without_geo_files() {
        let settings = XraySettings {
            bypass_cn: false,
            bypass_private: false,
            bypass_list: "example.com\n*.example.org\n10.0.0.0/8\ngeoip:private\n# comment\n"
                .to_string(),
            ..XraySettings::default()
        };
        let routing = geo_routing(&settings, false, false, false);
        let rules = routing["rules"].as_array().unwrap();

        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["outboundTag"], "direct");
        assert_eq!(
            rules[0]["domain"],
            json!(["domain:example.com", "domain:example.org"])
        );
        assert_eq!(rules[1]["outboundTag"], "direct");
        assert_eq!(rules[1]["ip"], json!(["10.0.0.0/8", "geoip:private"]));
    }

    #[test]
    fn blocking_quic_comes_before_the_bypass_rules() {
        let settings = XraySettings {
            block_quic: true,
            ..XraySettings::default()
        };
        let routing = geo_routing(&settings, true, true, false);
        let rules = routing["rules"].as_array().unwrap();

        assert_eq!(rules[0]["outboundTag"], "block");
        assert_eq!(rules[0]["network"], "udp");
        assert_eq!(rules[0]["port"], 443);
        assert_eq!(rules[1]["outboundTag"], "direct");
    }

    #[test]
    fn outbound_rules_run_before_the_bypass_rules() {
        let settings = XraySettings {
            outbound_rules: "example.com block\n10.0.0.0/8 direct\ntrusted.example proxy\n"
                .to_string(),
            bypass_list: "example.com".to_string(),
            ..XraySettings::default()
        };
        let routing = geo_routing(&settings, false, false, false);
        let rules = routing["rules"].as_array().unwrap();

        assert_eq!(rules.len(), 4);
        assert_eq!(rules[0]["domain"], json!(["domain:example.com"]));
        assert_eq!(rules[0]["outboundTag"], "block");
        assert_eq!(rules[1]["ip"], json!(["10.0.0.0/8"]));
        assert_eq!(rules[1]["outboundTag"], "direct");
        assert_eq!(rules[2]["domain"], json!(["domain:trusted.example"]));
        assert_eq!(rules[2]["outboundTag"], "proxy");
        // The custom bypass list still runs after the user's own rules.
        assert_eq!(rules[3]["outboundTag"], "direct");
    }

    #[test]
    fn proxy_outbound_rules_use_the_balancer_in_hot_switch_configs() {
        let settings = XraySettings {
            outbound_rules: "trusted.example proxy".to_string(),
            ..XraySettings::default()
        };
        let routing = geo_routing(&settings, false, false, true);
        let rule = &routing["rules"][0];

        assert_eq!(rule["domain"], json!(["domain:trusted.example"]));
        assert_eq!(rule["balancerTag"], HOT_SWITCH_BALANCER);
        assert!(rule.get("outboundTag").is_none());
    }

    #[test]
    fn sniffing_is_added_to_the_proxy_inbounds() {
        let prepared =
            prepare("vless://id@a.example:443?security=none#A", None, 2080, 2081).unwrap();
        let inbounds = prepared.config["inbounds"].as_array().unwrap();

        assert_eq!(
            inbounds[0]["sniffing"],
            json!({
                "enabled": true,
                "destOverride": ["http", "tls", "quic"],
                "metadataOnly": false
            })
        );
        assert_eq!(inbounds[1]["sniffing"]["enabled"], true);
    }

    #[test]
    fn sniffing_can_be_disabled_but_fake_dns_keeps_it_required() {
        let settings = XraySettings {
            sniffing_enabled: false,
            ..XraySettings::default()
        };
        let prepared = prepare_with_settings(
            "vless://id@a.example:443?security=none#A",
            None,
            2080,
            2081,
            None,
            None,
            &settings,
        )
        .unwrap();
        let inbounds = prepared.config["inbounds"].as_array().unwrap();
        assert!(inbounds[0].get("sniffing").is_none());
        assert!(inbounds[1].get("sniffing").is_none());

        let fake_dns = XraySettings {
            sniffing_enabled: false,
            sniff_http: false,
            sniff_tls: false,
            sniff_quic: false,
            tun_enabled: true,
            fake_dns_enabled: true,
            ..XraySettings::default()
        };
        let tun = tun_inbound(&fake_dns, false);
        assert_eq!(tun["sniffing"]["enabled"], true);
        assert_eq!(tun["sniffing"]["destOverride"], json!(["fakedns"]));
    }

    #[test]
    fn sniff_route_only_is_forwarded_to_the_inbounds() {
        let settings = XraySettings {
            sniff_route_only: true,
            ..XraySettings::default()
        };
        let prepared = prepare_with_settings(
            "vless://id@a.example:443?security=none#A",
            None,
            2080,
            2081,
            None,
            None,
            &settings,
        )
        .unwrap();

        assert_eq!(
            prepared.config["inbounds"][0]["sniffing"]["routeOnly"],
            true
        );
    }

    #[test]
    fn fragment_chains_every_node_through_the_fragment_outbound() {
        let settings = XraySettings {
            fragment_enabled: true,
            ..XraySettings::default()
        };
        let prepared = prepare_with_settings(
            "vless://id@a.example:443?security=tls#A\nvless://id@b.example:443?security=tls#B",
            None,
            2080,
            2081,
            None,
            Some(10085),
            &settings,
        )
        .unwrap();
        let outbounds = prepared.config["outbounds"].as_array().unwrap();

        assert_eq!(outbounds[0]["tag"], "cloak-node-0");
        assert_eq!(
            outbounds[0]["streamSettings"]["sockopt"]["dialerProxy"],
            FRAGMENT_TAG
        );
        assert_eq!(
            outbounds[1]["streamSettings"]["sockopt"]["dialerProxy"],
            FRAGMENT_TAG
        );
        // Direct/blackhole are never fragmented.
        assert!(outbounds
            .iter()
            .filter(|outbound| outbound["tag"] == "direct" || outbound["tag"] == "block")
            .all(|outbound| outbound.get("streamSettings").is_none()));

        let fragment = outbounds.last().unwrap();
        assert_eq!(fragment["tag"], FRAGMENT_TAG);
        assert_eq!(fragment["protocol"], "freedom");
        assert_eq!(
            fragment["settings"]["fragment"],
            json!({"packets": "tlshello", "length": "100-200", "interval": "10-20"})
        );
    }

    #[test]
    fn fragment_is_absent_unless_enabled() {
        let prepared =
            prepare("vless://id@a.example:443?security=none#A", None, 2080, 2081).unwrap();
        let outbounds = prepared.config["outbounds"].as_array().unwrap();

        assert!(outbounds
            .iter()
            .all(|outbound| outbound["tag"] != FRAGMENT_TAG));
        assert!(outbounds[0].pointer("/streamSettings/sockopt").is_none());
    }

    #[test]
    fn local_and_remote_dns_split_bypassed_from_proxied_domains() {
        let settings = XraySettings {
            local_dns: "223.5.5.5".to_string(),
            remote_dns: "1.1.1.1".to_string(),
            bypass_list: "example.com".to_string(),
            ..XraySettings::default()
        };
        let dns = dns_config(&settings, false).unwrap();
        let servers = dns["servers"].as_array().unwrap();

        assert_eq!(servers[0]["address"], "223.5.5.5");
        assert_eq!(servers[0]["domains"], json!(["domain:example.com"]));
        assert_eq!(servers[0]["skipFallback"], true);
        assert_eq!(servers[1], "1.1.1.1");
        // Everything else goes through the remote resolver, so the system
        // resolver must not be a fallback for it.
        assert!(!servers.iter().any(|server| server == "localhost"));
    }

    #[test]
    fn remote_dns_alone_answers_every_query() {
        let settings = XraySettings {
            remote_dns: "1.1.1.1".to_string(),
            bypass_cn: false,
            bypass_private: false,
            ..XraySettings::default()
        };
        let dns = dns_config(&settings, true).unwrap();

        assert_eq!(dns["servers"], json!(["1.1.1.1"]));
    }

    #[test]
    fn fake_dns_takes_over_proxied_domains_instead_of_remote_dns() {
        let settings = XraySettings {
            tun_enabled: true,
            fake_dns_enabled: true,
            remote_dns: "1.1.1.1".to_string(),
            ..XraySettings::default()
        };
        let dns = dns_config(&settings, true).unwrap();
        let servers = dns["servers"].as_array().unwrap();

        assert!(servers.iter().any(|server| server == "fakedns"));
        assert!(!servers.iter().any(|server| server == "1.1.1.1"));
    }

    #[test]
    fn settings_control_the_log_level_and_multiplexing() {
        let settings = XraySettings {
            log_level: "debug".to_string(),
            mux_enabled: true,
            mux_concurrency: 16,
            ..XraySettings::default()
        };
        let prepared = prepare_with_settings(
            "vless://id@a.example:443?security=none#A",
            None,
            2080,
            2081,
            None,
            None,
            &settings,
        )
        .unwrap();
        let outbound = &prepared.config["outbounds"][0];

        assert_eq!(prepared.config["log"]["loglevel"], "debug");
        assert_eq!(outbound["mux"]["enabled"], true);
        assert_eq!(outbound["mux"]["concurrency"], 16);
    }

    #[test]
    fn disabled_mux_is_not_added_to_the_outbound() {
        let prepared =
            prepare("vless://id@a.example:443?security=none#A", None, 2080, 2081).unwrap();
        assert!(prepared.config["outbounds"][0].get("mux").is_none());
    }

    #[test]
    fn full_configs_receive_the_configured_log_level() {
        let config = json!({
            "outbounds": [{"protocol": "freedom", "tag": "direct"}]
        });
        let settings = XraySettings {
            log_level: "error".to_string(),
            ..XraySettings::default()
        };

        let prepared =
            prepare_with_settings(&config.to_string(), None, 2080, 2081, None, None, &settings)
                .unwrap();
        assert_eq!(prepared.config["log"]["loglevel"], "error");
        // The subscription's own routing stays untouched.
        assert!(prepared.config.get("routing").is_none());
    }

    #[test]
    fn generated_configs_route_through_the_geo_rules() {
        let prepared =
            prepare("vless://id@a.example:443?security=none#A", None, 2080, 2081).unwrap();

        assert_eq!(prepared.config["routing"]["domainStrategy"], "IPIfNonMatch");
        assert!(prepared.config["routing"]["rules"].is_array());
    }

    #[test]
    fn ipv6_can_be_disabled_for_dns_queries() {
        let settings = XraySettings {
            ipv6_enabled: false,
            ..XraySettings::default()
        };
        let dns = dns_config(&settings, true).unwrap();
        assert_eq!(dns["queryStrategy"], "UseIPv4");
        assert_eq!(dns["servers"], json!(["localhost"]));

        // Nothing to configure with IPv6 on, no local DNS, and no FakeDNS.
        assert!(dns_config(&XraySettings::default(), true).is_none());
    }

    #[test]
    fn local_dns_serves_bypassed_domains() {
        let settings = XraySettings {
            local_dns: "223.5.5.5".to_string(),
            ..XraySettings::default()
        };
        let dns = dns_config(&settings, true).unwrap();
        let servers = dns["servers"].as_array().unwrap();

        assert_eq!(servers[0]["address"], "223.5.5.5");
        assert_eq!(
            servers[0]["domains"],
            json!(["geosite:private", "geosite:cn"])
        );
        assert_eq!(servers[0]["skipFallback"], true);
        assert_eq!(servers[1], "localhost");
    }

    #[test]
    fn fake_dns_requires_tun_and_uses_both_address_pools() {
        let without_tun = XraySettings {
            fake_dns_enabled: true,
            ..XraySettings::default()
        };
        assert!(fake_dns_config(&without_tun).is_none());
        // FakeDNS alone changes nothing, so no DNS section is emitted.
        assert!(dns_config(&without_tun, true).is_none());

        let settings = XraySettings {
            tun_enabled: true,
            fake_dns_enabled: true,
            ..XraySettings::default()
        };
        let fake_dns = fake_dns_config(&settings).unwrap();
        assert_eq!(fake_dns[0]["ipPool"], "198.18.0.0/15");
        assert_eq!(fake_dns[1]["ipPool"], "2001:2::/48");

        let dns = dns_config(&settings, true).unwrap();
        assert!(dns["servers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|server| server == "fakedns"));

        let routing = geo_routing(&settings, true, true, false);
        assert_eq!(routing["domainStrategy"], "AsIs");
        assert_eq!(routing["rules"][0]["outboundTag"], "dns-out");
        assert_eq!(routing["rules"][0]["port"], 53);
    }

    #[test]
    fn fake_dns_uses_a_single_pool_when_ipv6_is_disabled() {
        let settings = XraySettings {
            tun_enabled: true,
            fake_dns_enabled: true,
            ipv6_enabled: false,
            ..XraySettings::default()
        };
        let fake_dns = fake_dns_config(&settings).unwrap();
        assert_eq!(fake_dns["ipPool"], "198.18.0.0/15");
        assert_eq!(
            dns_config(&settings, true).unwrap()["queryStrategy"],
            "UseIPv4"
        );
    }

    #[test]
    fn tun_inbound_contains_gateway_routes_and_sniffing() {
        let settings = XraySettings {
            tun_enabled: true,
            fake_dns_enabled: true,
            ..XraySettings::default()
        };
        let inbound = tun_inbound(&settings, true);

        assert_eq!(inbound["protocol"], "tun");
        assert_eq!(inbound["settings"]["name"], "cloak0");
        assert_eq!(
            inbound["settings"]["gateway"],
            json!(["10.0.0.1/16", "fc00::1/64"])
        );
        assert_eq!(
            inbound["settings"]["autoSystemRoutingTable"],
            json!(["0.0.0.0/1", "128.0.0.0/1", "::/1", "8000::/1"])
        );
        assert_eq!(inbound["settings"]["autoSystemDnsToGateway"], true);
        assert_eq!(inbound["sniffing"]["enabled"], true);
        assert!(inbound["sniffing"]["destOverride"]
            .as_array()
            .unwrap()
            .iter()
            .any(|sniffer| sniffer == "fakedns"));
    }

    #[test]
    fn fake_dns_only_takes_over_the_system_resolver_when_available() {
        let settings = XraySettings {
            tun_enabled: true,
            fake_dns_enabled: true,
            ..XraySettings::default()
        };

        let without_takeover = tun_inbound(&settings, false);
        assert!(without_takeover["settings"]
            .get("autoSystemDnsToGateway")
            .is_none());

        let with_takeover = tun_inbound(&settings, true);
        assert_eq!(with_takeover["settings"]["autoSystemDnsToGateway"], true);
    }

    #[test]
    fn ipv6_disabled_removes_tun_ipv6_gateway_and_route() {
        let settings = XraySettings {
            tun_enabled: true,
            ipv6_enabled: false,
            ..XraySettings::default()
        };
        let inbound = tun_inbound(&settings, false);

        assert_eq!(inbound["settings"]["gateway"], json!(["10.0.0.1/16"]));
        assert_eq!(
            inbound["settings"]["autoSystemRoutingTable"],
            json!(["0.0.0.0/1", "128.0.0.0/1"])
        );
        assert!(inbound["settings"].get("autoSystemDnsToGateway").is_none());
    }

    #[test]
    fn non_strict_routing_uses_single_default_routes() {
        let settings = XraySettings {
            tun_enabled: true,
            tun_strict_route: false,
            ..XraySettings::default()
        };
        let inbound = tun_inbound(&settings, false);

        assert_eq!(
            inbound["settings"]["autoSystemRoutingTable"],
            json!(["0.0.0.0/0", "::/0"])
        );
    }

    #[test]
    fn strict_routing_needs_auto_route_to_be_effective() {
        let settings = XraySettings {
            tun_enabled: true,
            tun_auto_route: false,
            tun_strict_route: true,
            ..XraySettings::default()
        };
        let inbound = tun_inbound(&settings, false);

        assert!(inbound["settings"].get("autoSystemRoutingTable").is_none());
        assert!(inbound["settings"].get("autoOutboundsInterface").is_none());
    }

    #[test]
    fn tun_mode_adds_the_inbound_outbound_and_dns_hijack() {
        let settings = XraySettings {
            tun_enabled: true,
            fake_dns_enabled: true,
            ..XraySettings::default()
        };
        let prepared = prepare_with_settings(
            "vless://id@a.example:443?security=none#A",
            None,
            2080,
            2081,
            None,
            None,
            &settings,
        )
        .unwrap();

        assert!(prepared.config["inbounds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|inbound| inbound["protocol"] == "tun"));
        assert!(prepared.config["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|outbound| outbound["protocol"] == "dns"));
        assert_eq!(
            prepared.config["routing"]["rules"][0]["outboundTag"],
            "dns-out"
        );
        assert!(prepared.config["fakedns"].is_array());
        assert_eq!(prepared.config["dns"]["queryStrategy"], "UseIP");
    }

    #[test]
    fn test_cores_do_not_load_geo_databases() {
        let prepared =
            prepare_for_test("vless://id@a.example:443?security=none#A", None, 2080, 2081).unwrap();

        assert_eq!(prepared.config["routing"]["rules"], json!([]));
        assert_eq!(prepared.config["routing"]["domainStrategy"], "AsIs");
    }
}
