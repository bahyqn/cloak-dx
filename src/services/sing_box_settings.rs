use super::database::Database;

pub const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error", "fatal", "panic"];
pub const DEFAULT_LOG_LEVEL: &str = "info";

pub const ROUTING_MODES: &[&str] = &["rules", "global", "direct"];
pub const DEFAULT_ROUTING_MODE: &str = "rules";

pub const DNS_STRATEGIES: &[&str] = &["prefer_ipv4", "prefer_ipv6", "ipv4_only", "ipv6_only"];
pub const DEFAULT_DNS_STRATEGY: &str = "prefer_ipv4";

pub const TUN_STACKS: &[&str] = &["system", "gvisor", "mixed"];
pub const DEFAULT_TUN_STACK: &str = "mixed";

pub const MULTIPLEX_PROTOCOLS: &[&str] = &["h2mux", "smux", "yamux"];
pub const DEFAULT_MULTIPLEX_PROTOCOL: &str = "h2mux";
pub const MUX_MAX_CONNECTIONS_OPTIONS: &[u16] = &[1, 2, 4, 8, 16, 32];
pub const DEFAULT_MUX_MAX_CONNECTIONS: u16 = 4;

pub const DEFAULT_MIXED_PORT: u16 = 2080;
pub const DEFAULT_LAN_PORT: u16 = 2090;
pub const DEFAULT_TUN_NAME: &str = "cloak0";
pub const DEFAULT_TUN_MTU: u32 = 1500;
pub const TUN_MTU_OPTIONS: &[u32] = &[1280, 1380, 1420, 1500, 9000];
pub const DEFAULT_TUN_ADDRESS: &str = "172.19.0.1/30";
pub const DEFAULT_FAKE_DNS_IPV4_POOL: &str = "198.18.0.0/15";
pub const DEFAULT_FAKE_DNS_IPV6_POOL: &str = "fc00::/18";

const KEY_LOG_LEVEL: &str = "sing_box_log_level";
const KEY_LOG_TIMESTAMP: &str = "sing_box_log_timestamp";
const KEY_IPV6_ENABLED: &str = "sing_box_ipv6_enabled";
const KEY_MIXED_PORT: &str = "sing_box_mixed_port";
const KEY_LAN_ENABLED: &str = "sing_box_lan_enabled";
const KEY_LAN_PORT: &str = "sing_box_lan_port";
const KEY_LAN_USERNAME: &str = "sing_box_lan_username";
const KEY_LAN_PASSWORD: &str = "sing_box_lan_password";
const KEY_ROUTING_MODE: &str = "sing_box_routing_mode";
const KEY_AUTO_FAILOVER: &str = "sing_box_auto_failover";
const KEY_BYPASS_CN: &str = "sing_box_bypass_cn";
const KEY_BYPASS_PRIVATE: &str = "sing_box_bypass_private";
const KEY_BLOCK_ADS: &str = "sing_box_block_ads";
const KEY_BLOCK_QUIC: &str = "sing_box_block_quic";
const KEY_CUSTOM_RULES: &str = "sing_box_custom_rules";
const KEY_DNS_STRATEGY: &str = "sing_box_dns_strategy";
const KEY_LOCAL_DNS: &str = "sing_box_local_dns";
const KEY_REMOTE_DNS: &str = "sing_box_remote_dns";
const KEY_FAKE_DNS_ENABLED: &str = "sing_box_fake_dns_enabled";
const KEY_FAKE_DNS_IPV4_POOL: &str = "sing_box_fake_dns_ipv4_pool";
const KEY_FAKE_DNS_IPV6_POOL: &str = "sing_box_fake_dns_ipv6_pool";
const KEY_TUN_ENABLED: &str = "sing_box_tun_enabled";
const KEY_TUN_NAME: &str = "sing_box_tun_name";
const KEY_TUN_MTU: &str = "sing_box_tun_mtu";
const KEY_TUN_ADDRESS: &str = "sing_box_tun_address";
const KEY_TUN_AUTO_ROUTE: &str = "sing_box_tun_auto_route";
const KEY_TUN_STRICT_ROUTE: &str = "sing_box_tun_strict_route";
const KEY_TUN_STACK: &str = "sing_box_tun_stack";
const KEY_TUN_AUTO_REDIRECT: &str = "sing_box_tun_auto_redirect";
const KEY_SNIFFING_ENABLED: &str = "sing_box_sniffing_enabled";
const KEY_SNIFF_OVERRIDE: &str = "sing_box_sniff_override";
const KEY_MUX_ENABLED: &str = "sing_box_mux_enabled";
const KEY_MUX_PROTOCOL: &str = "sing_box_mux_protocol";
const KEY_MUX_MAX_CONNECTIONS: &str = "sing_box_mux_max_connections";
const KEY_MUX_PADDING: &str = "sing_box_mux_padding";
const KEY_TCP_FAST_OPEN: &str = "sing_box_tcp_fast_open";
const KEY_CACHE_FILE_ENABLED: &str = "sing_box_cache_file_enabled";

/// sing-box options that feed the config the app will generate for that core.
///
/// The values are stored under their own `sing_box_*` keys, so changing an
/// Xray field or an Xray default can never alter sing-box behaviour and vice
/// versa. Saving them restarts a running sing-box core; when Xray is running
/// (or nothing is), they apply the next time sing-box starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SingBoxSettings {
    pub log_level: String,
    pub log_timestamp: bool,
    pub ipv6_enabled: bool,
    /// Loopback mixed inbound port, also used by the system proxy.
    pub mixed_port: u16,
    pub lan_enabled: bool,
    /// Mixed inbound port exposed to the local network.
    pub lan_port: u16,
    pub lan_username: String,
    pub lan_password: String,
    pub routing_mode: String,
    /// Use a URLTest outbound and keep traffic on the fastest reachable node.
    pub auto_failover: bool,
    pub bypass_cn: bool,
    pub bypass_private: bool,
    pub block_ads: bool,
    pub block_quic: bool,
    /// Extra `<domain-or-ip> <direct|proxy|block>` rules.
    pub custom_rules: String,
    pub dns_strategy: String,
    /// Resolver for direct traffic. Empty uses the system resolver.
    pub local_dns: String,
    /// Resolver for proxied traffic. Empty uses the system resolver.
    pub remote_dns: String,
    pub fake_dns_enabled: bool,
    pub fake_dns_ipv4_pool: String,
    pub fake_dns_ipv6_pool: String,
    pub tun_enabled: bool,
    pub tun_name: String,
    pub tun_mtu: u32,
    /// IPv4 prefix assigned to the TUN interface.
    pub tun_address: String,
    pub tun_auto_route: bool,
    pub tun_strict_route: bool,
    pub tun_stack: String,
    /// Redirect traffic with nftables instead of route entries (Linux only).
    pub tun_auto_redirect: bool,
    pub sniffing_enabled: bool,
    /// Connect to the hostname recovered by sniffing instead of the original IP.
    pub sniff_override: bool,
    pub mux_enabled: bool,
    pub mux_protocol: String,
    pub mux_max_connections: u16,
    pub mux_padding: bool,
    pub tcp_fast_open: bool,
    /// Persist DNS and selector state across restarts.
    pub cache_file_enabled: bool,
}

impl Default for SingBoxSettings {
    fn default() -> Self {
        Self {
            log_level: DEFAULT_LOG_LEVEL.to_string(),
            log_timestamp: true,
            ipv6_enabled: true,
            mixed_port: DEFAULT_MIXED_PORT,
            lan_enabled: false,
            lan_port: DEFAULT_LAN_PORT,
            lan_username: String::new(),
            lan_password: String::new(),
            routing_mode: DEFAULT_ROUTING_MODE.to_string(),
            auto_failover: false,
            bypass_cn: true,
            bypass_private: true,
            block_ads: false,
            block_quic: false,
            custom_rules: String::new(),
            dns_strategy: DEFAULT_DNS_STRATEGY.to_string(),
            local_dns: String::new(),
            remote_dns: String::new(),
            fake_dns_enabled: false,
            fake_dns_ipv4_pool: DEFAULT_FAKE_DNS_IPV4_POOL.to_string(),
            fake_dns_ipv6_pool: DEFAULT_FAKE_DNS_IPV6_POOL.to_string(),
            tun_enabled: false,
            tun_name: DEFAULT_TUN_NAME.to_string(),
            tun_mtu: DEFAULT_TUN_MTU,
            tun_address: DEFAULT_TUN_ADDRESS.to_string(),
            tun_auto_route: true,
            tun_strict_route: false,
            tun_stack: DEFAULT_TUN_STACK.to_string(),
            tun_auto_redirect: false,
            sniffing_enabled: true,
            sniff_override: true,
            mux_enabled: false,
            mux_protocol: DEFAULT_MULTIPLEX_PROTOCOL.to_string(),
            mux_max_connections: DEFAULT_MUX_MAX_CONNECTIONS,
            mux_padding: false,
            tcp_fast_open: false,
            cache_file_enabled: false,
        }
    }
}

impl SingBoxSettings {
    pub fn load(database: &Database) -> Self {
        Self::from_values(|key| database.get_setting(key).ok().flatten())
    }

    /// Rejects values that would make sing-box refuse to start.
    pub fn validate(&self) -> Result<(), String> {
        if !LOG_LEVELS.contains(&self.log_level.as_str()) {
            return Err(format!("Unknown sing-box log level: {}", self.log_level));
        }
        if self.mixed_port == 0 {
            return Err("The mixed port must be between 1 and 65535.".to_string());
        }
        if self.lan_port == 0 {
            return Err("The LAN port must be between 1 and 65535.".to_string());
        }
        if self.lan_port == self.mixed_port {
            return Err("The LAN port must differ from the mixed port.".to_string());
        }
        if !ROUTING_MODES.contains(&self.routing_mode.as_str()) {
            return Err(format!(
                "Unknown sing-box routing mode: {}",
                self.routing_mode
            ));
        }
        validate_custom_rules(&self.custom_rules)?;
        if !DNS_STRATEGIES.contains(&self.dns_strategy.as_str()) {
            return Err(format!(
                "Unknown sing-box DNS strategy: {}",
                self.dns_strategy
            ));
        }
        if !self.local_dns.trim().is_empty() && !looks_like_dns_server(self.local_dns.trim()) {
            return Err("Local DNS must be an IP address, IP:port, or a DNS URL.".to_string());
        }
        if !self.remote_dns.trim().is_empty() && !looks_like_dns_server(self.remote_dns.trim()) {
            return Err("Remote DNS must be an IP address, IP:port, or a DNS URL.".to_string());
        }
        if self.fake_dns_enabled {
            if !is_cidr(&self.fake_dns_ipv4_pool, IpVersion::V4) {
                return Err(
                    "The FakeDNS IPv4 range must be a CIDR such as 198.18.0.0/15.".to_string(),
                );
            }
            if !is_cidr(&self.fake_dns_ipv6_pool, IpVersion::V6) {
                return Err("The FakeDNS IPv6 range must be a CIDR such as fc00::/18.".to_string());
            }
        }
        if self.tun_enabled {
            if self.tun_name.chars().any(char::is_whitespace) {
                return Err("The TUN interface name must not contain spaces.".to_string());
            }
            if !(576..=9000).contains(&self.tun_mtu) {
                return Err("The TUN MTU must be between 576 and 9000.".to_string());
            }
            if !is_cidr(&self.tun_address, IpVersion::V4) {
                return Err(
                    "The TUN address must be an IPv4 CIDR such as 172.19.0.1/30.".to_string(),
                );
            }
            if !TUN_STACKS.contains(&self.tun_stack.as_str()) {
                return Err(format!("Unknown TUN stack: {}", self.tun_stack));
            }
        }
        if !MULTIPLEX_PROTOCOLS.contains(&self.mux_protocol.as_str()) {
            return Err(format!("Unknown multiplex protocol: {}", self.mux_protocol));
        }
        if !(1..=1024).contains(&self.mux_max_connections) {
            return Err("Multiplex connections must be between 1 and 1024.".to_string());
        }
        Ok(())
    }

    /// Persists every option so the UI and a future generated config never
    /// disagree about a partially saved state.
    pub fn save(&self, database: &Database) -> Result<(), String> {
        self.validate()?;

        let entries: [(&str, String); 37] = [
            (KEY_LOG_LEVEL, self.log_level.clone()),
            (KEY_LOG_TIMESTAMP, bool_value(self.log_timestamp)),
            (KEY_IPV6_ENABLED, bool_value(self.ipv6_enabled)),
            (KEY_MIXED_PORT, self.mixed_port.to_string()),
            (KEY_LAN_ENABLED, bool_value(self.lan_enabled)),
            (KEY_LAN_PORT, self.lan_port.to_string()),
            (KEY_LAN_USERNAME, self.lan_username.clone()),
            (KEY_LAN_PASSWORD, self.lan_password.clone()),
            (KEY_ROUTING_MODE, self.routing_mode.clone()),
            (KEY_AUTO_FAILOVER, bool_value(self.auto_failover)),
            (KEY_BYPASS_CN, bool_value(self.bypass_cn)),
            (KEY_BYPASS_PRIVATE, bool_value(self.bypass_private)),
            (KEY_BLOCK_ADS, bool_value(self.block_ads)),
            (KEY_BLOCK_QUIC, bool_value(self.block_quic)),
            (KEY_CUSTOM_RULES, self.custom_rules.clone()),
            (KEY_DNS_STRATEGY, self.dns_strategy.clone()),
            (KEY_LOCAL_DNS, self.local_dns.clone()),
            (KEY_REMOTE_DNS, self.remote_dns.clone()),
            (KEY_FAKE_DNS_ENABLED, bool_value(self.fake_dns_enabled)),
            (KEY_FAKE_DNS_IPV4_POOL, self.fake_dns_ipv4_pool.clone()),
            (KEY_FAKE_DNS_IPV6_POOL, self.fake_dns_ipv6_pool.clone()),
            (KEY_TUN_ENABLED, bool_value(self.tun_enabled)),
            (KEY_TUN_NAME, self.tun_name.clone()),
            (KEY_TUN_MTU, self.tun_mtu.to_string()),
            (KEY_TUN_ADDRESS, self.tun_address.clone()),
            (KEY_TUN_AUTO_ROUTE, bool_value(self.tun_auto_route)),
            (KEY_TUN_STRICT_ROUTE, bool_value(self.tun_strict_route)),
            (KEY_TUN_STACK, self.tun_stack.clone()),
            (KEY_TUN_AUTO_REDIRECT, bool_value(self.tun_auto_redirect)),
            (KEY_SNIFFING_ENABLED, bool_value(self.sniffing_enabled)),
            (KEY_SNIFF_OVERRIDE, bool_value(self.sniff_override)),
            (KEY_MUX_ENABLED, bool_value(self.mux_enabled)),
            (KEY_MUX_PROTOCOL, self.mux_protocol.clone()),
            (
                KEY_MUX_MAX_CONNECTIONS,
                self.mux_max_connections.to_string(),
            ),
            (KEY_MUX_PADDING, bool_value(self.mux_padding)),
            (KEY_TCP_FAST_OPEN, bool_value(self.tcp_fast_open)),
            (KEY_CACHE_FILE_ENABLED, bool_value(self.cache_file_enabled)),
        ];

        for (key, value) in entries {
            database
                .set_setting(key, &value)
                .map_err(|error| format!("Could not save sing-box settings: {error}"))?;
        }
        Ok(())
    }

    fn from_values(mut lookup: impl FnMut(&str) -> Option<String>) -> Self {
        let defaults = Self::default();

        let log_level = lookup(KEY_LOG_LEVEL)
            .filter(|value| is_known(value, LOG_LEVELS))
            .unwrap_or(defaults.log_level);
        let log_timestamp = parse_bool(lookup(KEY_LOG_TIMESTAMP), defaults.log_timestamp);
        let ipv6_enabled = parse_bool(lookup(KEY_IPV6_ENABLED), defaults.ipv6_enabled);
        let mixed_port = parse_port(lookup(KEY_MIXED_PORT), defaults.mixed_port);
        let lan_enabled = parse_bool(lookup(KEY_LAN_ENABLED), defaults.lan_enabled);
        let lan_port = match parse_port(lookup(KEY_LAN_PORT), defaults.lan_port) {
            port if port != mixed_port => port,
            _ => defaults.lan_port,
        };
        let lan_username = lookup(KEY_LAN_USERNAME)
            .map(|value| value.trim().to_string())
            .unwrap_or_default();
        let lan_password = lookup(KEY_LAN_PASSWORD).unwrap_or_default();
        let routing_mode = lookup(KEY_ROUTING_MODE)
            .filter(|value| is_known(value, ROUTING_MODES))
            .unwrap_or(defaults.routing_mode);
        let auto_failover = parse_bool(lookup(KEY_AUTO_FAILOVER), defaults.auto_failover);
        let bypass_cn = parse_bool(lookup(KEY_BYPASS_CN), defaults.bypass_cn);
        let bypass_private = parse_bool(lookup(KEY_BYPASS_PRIVATE), defaults.bypass_private);
        let block_ads = parse_bool(lookup(KEY_BLOCK_ADS), defaults.block_ads);
        let block_quic = parse_bool(lookup(KEY_BLOCK_QUIC), defaults.block_quic);
        let custom_rules = lookup(KEY_CUSTOM_RULES).unwrap_or_default();
        let dns_strategy = lookup(KEY_DNS_STRATEGY)
            .filter(|value| is_known(value, DNS_STRATEGIES))
            .unwrap_or(defaults.dns_strategy);
        let local_dns = lookup(KEY_LOCAL_DNS)
            .map(|value| value.trim().to_string())
            .filter(|value| value.is_empty() || looks_like_dns_server(value))
            .unwrap_or_default();
        let remote_dns = lookup(KEY_REMOTE_DNS)
            .map(|value| value.trim().to_string())
            .filter(|value| value.is_empty() || looks_like_dns_server(value))
            .unwrap_or_default();
        let fake_dns_enabled = parse_bool(lookup(KEY_FAKE_DNS_ENABLED), defaults.fake_dns_enabled);
        let fake_dns_ipv4_pool = lookup(KEY_FAKE_DNS_IPV4_POOL)
            .filter(|value| is_cidr(value, IpVersion::V4))
            .unwrap_or(defaults.fake_dns_ipv4_pool);
        let fake_dns_ipv6_pool = lookup(KEY_FAKE_DNS_IPV6_POOL)
            .filter(|value| is_cidr(value, IpVersion::V6))
            .unwrap_or(defaults.fake_dns_ipv6_pool);
        let tun_enabled = parse_bool(lookup(KEY_TUN_ENABLED), defaults.tun_enabled);
        let tun_name = lookup(KEY_TUN_NAME)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.chars().any(char::is_whitespace))
            .unwrap_or(defaults.tun_name);
        let tun_mtu = lookup(KEY_TUN_MTU)
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| (576..=9000).contains(value))
            .unwrap_or(defaults.tun_mtu);
        let tun_address = lookup(KEY_TUN_ADDRESS)
            .filter(|value| is_cidr(value, IpVersion::V4))
            .unwrap_or(defaults.tun_address);
        let tun_auto_route = parse_bool(lookup(KEY_TUN_AUTO_ROUTE), defaults.tun_auto_route);
        let tun_strict_route = parse_bool(lookup(KEY_TUN_STRICT_ROUTE), defaults.tun_strict_route);
        let tun_stack = lookup(KEY_TUN_STACK)
            .filter(|value| is_known(value, TUN_STACKS))
            .unwrap_or(defaults.tun_stack);
        let tun_auto_redirect =
            parse_bool(lookup(KEY_TUN_AUTO_REDIRECT), defaults.tun_auto_redirect);
        let sniffing_enabled = parse_bool(lookup(KEY_SNIFFING_ENABLED), defaults.sniffing_enabled);
        let sniff_override = parse_bool(lookup(KEY_SNIFF_OVERRIDE), defaults.sniff_override);
        let mux_enabled = parse_bool(lookup(KEY_MUX_ENABLED), defaults.mux_enabled);
        let mux_protocol = lookup(KEY_MUX_PROTOCOL)
            .filter(|value| is_known(value, MULTIPLEX_PROTOCOLS))
            .unwrap_or(defaults.mux_protocol);
        let mux_max_connections = lookup(KEY_MUX_MAX_CONNECTIONS)
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|value| (1..=1024).contains(value))
            .unwrap_or(defaults.mux_max_connections);
        let mux_padding = parse_bool(lookup(KEY_MUX_PADDING), defaults.mux_padding);
        let tcp_fast_open = parse_bool(lookup(KEY_TCP_FAST_OPEN), defaults.tcp_fast_open);
        let cache_file_enabled =
            parse_bool(lookup(KEY_CACHE_FILE_ENABLED), defaults.cache_file_enabled);

        Self {
            log_level,
            log_timestamp,
            ipv6_enabled,
            mixed_port,
            lan_enabled,
            lan_port,
            lan_username,
            lan_password,
            routing_mode,
            auto_failover,
            bypass_cn,
            bypass_private,
            block_ads,
            block_quic,
            custom_rules,
            dns_strategy,
            local_dns,
            remote_dns,
            fake_dns_enabled,
            fake_dns_ipv4_pool,
            fake_dns_ipv6_pool,
            tun_enabled,
            tun_name,
            tun_mtu,
            tun_address,
            tun_auto_route,
            tun_strict_route,
            tun_stack,
            tun_auto_redirect,
            sniffing_enabled,
            sniff_override,
            mux_enabled,
            mux_protocol,
            mux_max_connections,
            mux_padding,
            tcp_fast_open,
            cache_file_enabled,
        }
    }
}

fn is_known(value: &str, known: &[&str]) -> bool {
    known.contains(&value)
}

fn bool_value(value: bool) -> String {
    if value { "1" } else { "0" }.to_string()
}

fn parse_bool(value: Option<String>, default: bool) -> bool {
    match value.as_deref() {
        Some("1" | "true") => true,
        Some("0" | "false") => false,
        _ => default,
    }
}

fn parse_port(value: Option<String>, default: u16) -> u16 {
    value
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| *value != 0)
        .unwrap_or(default)
}

fn validate_custom_rules(value: &str) -> Result<(), String> {
    for (index, raw) in value.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let destination = parts.next();
        let target = parts.next();
        if destination.is_none() || !matches!(target, Some("direct" | "proxy" | "block")) {
            return Err(format!(
                "Routing rules line {} must look like `<domain-or-ip> <direct|proxy|block>`: {line}",
                index + 1
            ));
        }
        if parts.next().is_some() {
            return Err(format!(
                "Routing rules line {} must look like `<domain-or-ip> <direct|proxy|block>`: {line}",
                index + 1
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum IpVersion {
    V4,
    V6,
}

fn is_cidr(value: &str, version: IpVersion) -> bool {
    let Some((address, prefix)) = value.split_once('/') else {
        return false;
    };
    let prefix = prefix.parse::<u8>().ok();
    match version {
        IpVersion::V4 => {
            address.parse::<std::net::Ipv4Addr>().is_ok()
                && prefix.is_some_and(|prefix| prefix <= 32)
        }
        IpVersion::V6 => {
            address.parse::<std::net::Ipv6Addr>().is_ok()
                && prefix.is_some_and(|prefix| prefix <= 128)
        }
    }
}

/// Accepts `1.1.1.1`, `1.1.1.1:53`, `https://1.1.1.1/dns-query`, and plain
/// hostnames such as `dns.google:853`.
fn looks_like_dns_server(value: &str) -> bool {
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return false;
    }
    let address = match value.split_once("://") {
        Some((scheme, rest)) => {
            if !matches!(
                scheme.to_ascii_lowercase().as_str(),
                "tcp" | "udp" | "tls" | "quic" | "https"
            ) {
                return false;
            }
            rest
        }
        None => value,
    };
    let authority = address.split(['/', '?']).next().unwrap_or("");
    if authority.is_empty() {
        return false;
    }
    if authority.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, tail)) = rest.split_once(']') else {
            return false;
        };
        return host.parse::<std::net::Ipv6Addr>().is_ok()
            && (tail.is_empty() || (tail.starts_with(':') && tail[1..].parse::<u16>().is_ok()));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    if port.is_some_and(|port| port.parse::<u16>().is_err()) {
        return false;
    }
    !host.is_empty() && (host.parse::<std::net::Ipv4Addr>().is_ok() || is_domain_name(host))
}

fn is_domain_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && !value.starts_with('.')
        && !value.ends_with('.')
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '.' | '_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_documented_sing_box_values() {
        let settings = SingBoxSettings::default();
        assert_eq!(settings.log_level, "info");
        assert_eq!(settings.mixed_port, DEFAULT_MIXED_PORT);
        assert_eq!(settings.routing_mode, "rules");
        assert_eq!(settings.dns_strategy, "prefer_ipv4");
        assert_eq!(settings.tun_stack, "mixed");
        assert_eq!(settings.mux_protocol, "h2mux");
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn all_values_survive_a_database_round_trip() {
        let database = Database::in_memory().unwrap();
        let settings = SingBoxSettings {
            log_level: "debug".to_string(),
            log_timestamp: false,
            ipv6_enabled: false,
            mixed_port: 3080,
            lan_enabled: true,
            lan_port: 3090,
            lan_username: "alice".to_string(),
            lan_password: "secret".to_string(),
            routing_mode: "global".to_string(),
            auto_failover: true,
            bypass_cn: false,
            bypass_private: false,
            block_ads: true,
            block_quic: true,
            custom_rules: "example.com direct\n10.0.0.0/8 block".to_string(),
            dns_strategy: "ipv4_only".to_string(),
            local_dns: "1.1.1.1".to_string(),
            remote_dns: "https://8.8.8.8/dns-query".to_string(),
            fake_dns_enabled: true,
            fake_dns_ipv4_pool: "198.19.0.0/16".to_string(),
            fake_dns_ipv6_pool: "fd00::/48".to_string(),
            tun_enabled: true,
            tun_name: "sing0".to_string(),
            tun_mtu: 1420,
            tun_address: "172.20.0.1/30".to_string(),
            tun_auto_route: false,
            tun_strict_route: true,
            tun_stack: "gvisor".to_string(),
            tun_auto_redirect: true,
            sniffing_enabled: false,
            sniff_override: false,
            mux_enabled: true,
            mux_protocol: "smux".to_string(),
            mux_max_connections: 16,
            mux_padding: true,
            tcp_fast_open: true,
            cache_file_enabled: true,
        };
        settings.save(&database).unwrap();

        assert_eq!(SingBoxSettings::load(&database), settings);
    }

    #[test]
    fn corrupt_values_fall_back_to_defaults() {
        let database = Database::in_memory().unwrap();
        for (key, value) in [
            (KEY_LOG_LEVEL, "verbose"),
            (KEY_LOG_TIMESTAMP, "maybe"),
            (KEY_MIXED_PORT, "0"),
            (KEY_LAN_PORT, "2080"),
            (KEY_ROUTING_MODE, "sometimes"),
            (KEY_DNS_STRATEGY, "round_robin"),
            (KEY_LOCAL_DNS, "not a dns server"),
            (KEY_FAKE_DNS_IPV4_POOL, "not-a-cidr"),
            (KEY_TUN_MTU, "99999"),
            (KEY_TUN_ADDRESS, "10.0.0.1"),
            (KEY_TUN_STACK, "magic"),
            (KEY_MUX_PROTOCOL, "quic"),
            (KEY_MUX_MAX_CONNECTIONS, "0"),
        ] {
            database.set_setting(key, value).unwrap();
        }

        let settings = SingBoxSettings::load(&database);
        let defaults = SingBoxSettings::default();
        assert_eq!(settings.log_level, defaults.log_level);
        assert_eq!(settings.log_timestamp, defaults.log_timestamp);
        assert_eq!(settings.mixed_port, defaults.mixed_port);
        assert_eq!(settings.lan_port, defaults.lan_port);
        assert_eq!(settings.routing_mode, defaults.routing_mode);
        assert_eq!(settings.dns_strategy, defaults.dns_strategy);
        assert_eq!(settings.local_dns, defaults.local_dns);
        assert_eq!(settings.fake_dns_ipv4_pool, defaults.fake_dns_ipv4_pool);
        assert_eq!(settings.tun_mtu, defaults.tun_mtu);
        assert_eq!(settings.tun_address, defaults.tun_address);
        assert_eq!(settings.tun_stack, defaults.tun_stack);
        assert_eq!(settings.mux_protocol, defaults.mux_protocol);
        assert_eq!(settings.mux_max_connections, defaults.mux_max_connections);
    }

    #[test]
    fn validation_rejects_values_sing_box_cannot_use() {
        let settings = SingBoxSettings {
            mixed_port: 0,
            ..SingBoxSettings::default()
        };
        assert!(settings.validate().is_err());

        let defaults = SingBoxSettings::default();
        let settings = SingBoxSettings {
            lan_port: defaults.mixed_port,
            ..SingBoxSettings::default()
        };
        assert!(settings.validate().is_err());

        let settings = SingBoxSettings {
            custom_rules: "example.com sideways".to_string(),
            ..SingBoxSettings::default()
        };
        assert!(settings.validate().is_err());

        let settings = SingBoxSettings {
            fake_dns_enabled: true,
            fake_dns_ipv6_pool: "198.18.0.0/15".to_string(),
            ..SingBoxSettings::default()
        };
        assert!(settings.validate().is_err());

        let settings = SingBoxSettings {
            tun_enabled: true,
            tun_address: "fc00::1/64".to_string(),
            ..SingBoxSettings::default()
        };
        assert!(settings.validate().is_err());

        let settings = SingBoxSettings {
            mux_max_connections: 0,
            ..SingBoxSettings::default()
        };
        assert!(settings.validate().is_err());
    }

    #[test]
    fn saved_settings_do_not_touch_xray_keys() {
        use crate::services::xray_settings::XraySettings;

        let database = Database::in_memory().unwrap();
        let mut xray = XraySettings::load(&database);
        xray.log_level = "debug".to_string();
        xray.save(&database).unwrap();

        let sing_box = SingBoxSettings {
            log_level: "trace".to_string(),
            mixed_port: 3080,
            ..SingBoxSettings::default()
        };
        sing_box.save(&database).unwrap();

        assert_eq!(SingBoxSettings::load(&database), sing_box);
        assert_eq!(XraySettings::load(&database), xray);
    }
}
