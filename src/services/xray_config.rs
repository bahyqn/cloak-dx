use serde_json::{json, Map, Value};

use super::{
    geo_assets::geo_asset_directory,
    subscription_manager::{decode_subscription, parse_proxy_uri, subscription_node_key},
    xray_settings::{XraySettings, DEFAULT_FAKE_DNS_IPV6_POOL, TUN_IPV6_GATEWAY},
};

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedConfig {
    pub config: Value,
    pub socks_port: Option<u16>,
    pub http_port: Option<u16>,
    pub node_name: Option<String>,
}

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
        &XraySettings::default(),
    )
}

/// Builds a runnable Xray config using the options configured in Settings.
pub fn prepare_with_settings(
    raw_subscription: &str,
    selected_node_key: Option<&str>,
    socks_port: u16,
    http_port: u16,
    settings: &XraySettings,
) -> Result<PreparedConfig, String> {
    let payload = decode_subscription(raw_subscription);

    if let Ok(value) = serde_json::from_str::<Value>(&payload) {
        if is_xray_config(&value) {
            return prepare_from_config(value, socks_port, http_port, settings);
        }
    }

    let uri = select_proxy_uri(&payload, selected_node_key)?;
    let (mut outbound, node_name) = outbound_from_uri(&uri)?;
    apply_mux(&mut outbound, settings);
    Ok(PreparedConfig {
        config: base_config(
            outbound,
            socks_port,
            http_port,
            managed_geo_routing(settings),
            settings,
        ),
        socks_port: Some(socks_port),
        http_port: Some(http_port),
        node_name,
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
            outbound,
            socks_port,
            http_port,
            json!({"domainStrategy": "AsIs", "rules": []}),
            &XraySettings::default(),
        ),
        socks_port: Some(socks_port),
        http_port: Some(http_port),
        node_name,
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
        (inbound_port(object, "socks"), inbound_port(object, "http"))
    } else {
        object.insert(
            "inbounds".to_string(),
            local_inbounds(socks_port, http_port),
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
        node_name,
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
        inbounds.push(socks_inbound(socks_port));
    }
    if !http_ready {
        inbounds.push(http_inbound(http_port));
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
        node_name,
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

fn local_inbounds(socks_port: u16, http_port: u16) -> Value {
    json!([socks_inbound(socks_port), http_inbound(http_port)])
}

fn socks_inbound(port: u16) -> Value {
    json!({
        "tag": "cloak-socks-in",
        "listen": "127.0.0.1",
        "port": port,
        "protocol": "socks",
        "settings": {"auth": "noauth", "udp": true}
    })
}

fn http_inbound(port: u16) -> Value {
    json!({
        "tag": "cloak-http-in",
        "listen": "127.0.0.1",
        "port": port,
        "protocol": "http",
        "settings": {}
    })
}

fn base_config(
    outbound: Value,
    socks_port: u16,
    http_port: u16,
    routing: Value,
    settings: &XraySettings,
) -> Value {
    let mut inbounds = local_inbounds(socks_port, http_port);
    if settings.tun_enabled {
        if let Some(inbounds) = inbounds.as_array_mut() {
            inbounds.push(tun_inbound(settings, system_dns_takeover_available()));
        }
    }

    let mut outbounds = vec![
        outbound,
        json!({"tag": "direct", "protocol": "freedom"}),
        json!({"tag": "block", "protocol": "blackhole"}),
    ];
    if settings.tun_enabled {
        // DNS queries captured by the TUN inbound are routed to this outbound
        // so the built-in DNS server (and FakeDNS) can answer them.
        outbounds.push(json!({"tag": "dns-out", "protocol": "dns"}));
    }

    let mut config = json!({
        "log": {"loglevel": settings.log_level},
        "inbounds": inbounds,
        "outbounds": outbounds,
        "routing": routing
    });

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

    let mut dest_override = vec!["http", "tls", "quic"];
    if fake_dns_active(settings) {
        // Required for FakeDNS: maps the fake destination IP back to a domain.
        dest_override.push("fakedns");
    }

    json!({
        "tag": "cloak-tun-in",
        "protocol": "tun",
        "settings": Value::Object(tun),
        "sniffing": {
            "enabled": true,
            "destOverride": dest_override,
            "metadataOnly": false
        }
    })
}

fn managed_dns(settings: &XraySettings) -> Option<Value> {
    let has_geosite = geo_asset_directory().join("geosite.dat").is_file();
    dns_config(settings, has_geosite)
}

fn dns_config(settings: &XraySettings, has_geosite: bool) -> Option<Value> {
    let local_dns = settings.local_dns.trim();
    let fake_dns = fake_dns_active(settings);

    // Keep the previous behaviour (system DNS, both address families) when no
    // custom DNS option is in play.
    if local_dns.is_empty() && !fake_dns && settings.ipv6_enabled {
        return None;
    }

    let bypass_domains = {
        let mut domains = Vec::new();
        if settings.bypass_private {
            domains.push("geosite:private");
        }
        if settings.bypass_cn {
            domains.push("geosite:cn");
        }
        domains
    };

    let mut servers = Vec::new();
    if !local_dns.is_empty() {
        if has_geosite && !bypass_domains.is_empty() {
            // Direct traffic resolves through the local resolver; everything
            // else is sent to the remote proxy by domain anyway.
            servers.push(json!({
                "address": local_dns,
                "domains": bypass_domains,
                "skipFallback": true
            }));
        } else if !fake_dns {
            // With no domain list to scope it to, use the local resolver first.
            servers.push(json!(local_dns));
        }
    } else if fake_dns && has_geosite && settings.bypass_private {
        // Never hand LAN hostnames a fake address: sniffing cannot recover a
        // domain for plain LAN traffic.
        servers.push(json!({
            "address": "localhost",
            "domains": ["geosite:private"],
            "skipFallback": true
        }));
    }

    if fake_dns {
        servers.push(json!("fakedns"));
    }
    // Final fallback for everything the configured servers did not answer.
    servers.push(json!("localhost"));

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

/// Routing for a core started from this app: private and mainland-China
/// destinations go direct, everything else uses the selected node.
///
/// The rules are built from the GeoIP/GeoSite files managed in Settings. A
/// rule is only emitted for a database that exists because Xray refuses to
/// start when it references a missing `geoip.dat`/`geosite.dat`.
fn managed_geo_routing(settings: &XraySettings) -> Value {
    let directory = geo_asset_directory();
    geo_routing(
        settings,
        directory.join("geosite.dat").is_file(),
        directory.join("geoip.dat").is_file(),
    )
}

fn geo_routing(settings: &XraySettings, has_geosite: bool, has_geoip: bool) -> Value {
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

    // Ads are matched before the direct rules so a blocked domain is not sent
    // direct just because it is also part of a bypassed region list.
    if settings.block_ads && has_geosite {
        rules.push(json!({
            "type": "field",
            "outboundTag": "block",
            "domain": ["geosite:category-ads-all"]
        }));
    }

    let mut direct_domains = Vec::new();
    if settings.bypass_private {
        direct_domains.push("geosite:private");
    }
    if settings.bypass_cn {
        direct_domains.push("geosite:cn");
    }
    if has_geosite && !direct_domains.is_empty() {
        rules.push(json!({
            "type": "field",
            "outboundTag": "direct",
            "domain": direct_domains
        }));
    }

    let mut direct_ips = Vec::new();
    if settings.bypass_private {
        direct_ips.push("geoip:private");
    }
    if settings.bypass_cn {
        direct_ips.push("geoip:cn");
    }
    if has_geoip && !direct_ips.is_empty() {
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
        let routing = geo_routing(&XraySettings::default(), true, true);

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
        let routing = geo_routing(&XraySettings::default(), false, false);
        assert_eq!(routing["rules"], json!([]));

        // A partial download is still usable: Xray must not see a reference
        // to a database that does not exist.
        let only_geosite = geo_routing(&XraySettings::default(), true, false);
        let rules = only_geosite["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 1);
        assert!(rules[0].get("ip").is_none());

        let only_geoip = geo_routing(&XraySettings::default(), false, true);
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
        let routing = geo_routing(&settings, true, true);
        assert_eq!(routing["rules"], json!([]));

        let only_private = XraySettings {
            bypass_cn: false,
            ..XraySettings::default()
        };
        let routing = geo_routing(&only_private, true, true);
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
        let routing = geo_routing(&settings, true, true);
        let rules = routing["rules"].as_array().unwrap();

        assert_eq!(rules[0]["outboundTag"], "block");
        assert_eq!(rules[0]["domain"], json!(["geosite:category-ads-all"]));
        assert_eq!(rules[1]["outboundTag"], "direct");
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
            prepare_with_settings(&config.to_string(), None, 2080, 2081, &settings).unwrap();
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

        let routing = geo_routing(&settings, true, true);
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
