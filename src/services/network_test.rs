//! Diagnostics for the connection currently served by the proxy core.
//!
//! Exit and geo probes run through the core's SOCKS inbound with `socks5h`, so
//! the node resolves the destination and the answer describes what a website
//! sees. DNS-leak probes use the same client for the same reason: the probe
//! names are resolved by the connection under test, so the authorities list the
//! resolvers proxied traffic really goes through — the same answer a browser
//! behind this proxy gets. Resolving them on this machine instead would only
//! ever report this machine's own ISP resolver, which is not the path the app's
//! proxied traffic takes.

use std::{
    fmt::Write as _,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hmac::{Hmac, Mac};
use reqwest::{
    header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, ORIGIN, REFERER},
    Client, StatusCode,
};
use serde_json::Value;
use sha2::Sha256;

use super::proxy_test::{self, text_field, GoogleExitResult};

const IPPURE_BASE: &str = "https://api.ippure.com";
const IPPURE_ORIGIN: &str = "https://ippure.com";
/// ipleak.net answers on `<session>-<label>.ipleak.net` with every resolver
/// that looked up the session's names, and describes each resolver under
/// `/json/<ip>`.
const IPLEAK_DETECTION_URL: &str = "https://{session}-{label}.ipleak.net/dnsdetection/";
const IPLEAK_JSON_URL: &str = "https://ipleak.net/json/{}";
/// ipleak.net reuses one 40-character session for a whole test run and a fresh
/// label for every request.
const IPLEAK_SESSION_LEN: usize = 40;
const IPLEAK_SESSION_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
/// BrowserLeaks keys its answer by resolver IP, so each probe needs a unique
/// subdomain.
const DNS_BROWSERLEAKS_V4_URL: &str = "https://{}.dns4.browserleaks.net/";
const DNS_BROWSERLEAKS_V6_URL: &str = "https://{}.dns6.browserleaks.net/";
const DNS_TIMEOUT: Duration = Duration::from_secs(15);
const IPPURE_TIMEOUT: Duration = Duration::from_secs(20);

/// Leak-check site whose probes the DNS card runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DnsLeakProvider {
    #[default]
    Ipleak,
    BrowserLeaks,
}

impl DnsLeakProvider {
    /// Stable value used by the UI select.
    pub fn value(self) -> &'static str {
        match self {
            DnsLeakProvider::Ipleak => "ipleak",
            DnsLeakProvider::BrowserLeaks => "browserleaks",
        }
    }

    pub fn from_value(value: &str) -> Self {
        match value {
            "browserleaks" => DnsLeakProvider::BrowserLeaks,
            _ => DnsLeakProvider::Ipleak,
        }
    }

    /// Short name attached to every resolver the provider reported.
    pub fn label(self) -> &'static str {
        match self {
            DnsLeakProvider::Ipleak => "ipleak.net",
            DnsLeakProvider::BrowserLeaks => "BrowserLeaks",
        }
    }
}

/// Runs the same Google exit probe the node cards use, but through the core
/// that is already serving traffic instead of a temporary one.
pub async fn test_google_exit(socks_port: u16) -> Result<GoogleExitResult, String> {
    proxy_test::probe_google_exit(socks_port).await
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DnsLeakVerdict {
    /// The resolver set is consistent with the country the proxy exits from.
    Protected,
    /// Only part of the resolver set matches the exit country.
    Mixed,
    /// No resolver matches the exit country.
    Leaking,
}

impl DnsLeakVerdict {
    pub fn slug(self) -> &'static str {
        match self {
            DnsLeakVerdict::Protected => "protected",
            DnsLeakVerdict::Mixed => "mixed",
            DnsLeakVerdict::Leaking => "leaking",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DnsLeakVerdict::Protected => "No DNS leak",
            DnsLeakVerdict::Mixed => "DNS status unknown",
            DnsLeakVerdict::Leaking => "DNS leak detected",
        }
    }
}

/// One DNS resolver observed by the leak-check authorities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DnsResolver {
    pub ip: String,
    pub country_code: Option<String>,
    pub place: Option<String>,
    pub organization: Option<String>,
    /// Authorities that saw this resolver, in the order they were queried.
    pub sources: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DnsLeakResult {
    pub resolvers: Vec<DnsResolver>,
    /// Set when the selected site could not see the DNS queries and the
    /// resolver list was measured by another probe set instead.
    pub fallback: Option<DnsLeakProvider>,
}

impl DnsLeakResult {
    /// Compares every resolver with the country the proxy exits from.
    ///
    /// The probe names are resolved by the node, so these are the resolvers the
    /// proxied connection uses. A resolver outside the exit country means part
    /// of the DNS path leaves somewhere else — for example a node whose
    /// upstream resolver sits in a different country, or a tunnel that sends
    /// queries to a public resolver. A single foreign resolver is enough to
    /// report it. Resolvers without geo data (a failed lookup on ipleak.net)
    /// are ignored, because they say nothing either way, and without a known
    /// exit country the resolver locations cannot be judged at all.
    pub fn verdict(&self, exit_country: Option<&str>) -> DnsLeakVerdict {
        let Some(expected) = exit_country
            .filter(|code| !code.trim().is_empty())
            .map(|code| code.trim().to_ascii_uppercase())
        else {
            // Without knowing where traffic leaves, a resolver's location says
            // nothing about whether it leaked.
            return DnsLeakVerdict::Mixed;
        };
        let mut known = 0_usize;
        let mut matching = 0_usize;
        for resolver in &self.resolvers {
            let Some(code) = resolver.country_code.as_deref() else {
                continue;
            };
            known += 1;
            if code.eq_ignore_ascii_case(&expected) {
                matching += 1;
            }
        }
        if known == 0 {
            return DnsLeakVerdict::Mixed;
        }
        if matching == known {
            DnsLeakVerdict::Protected
        } else {
            DnsLeakVerdict::Leaking
        }
    }

    fn merge(&mut self, resolver: DnsResolver) {
        if let Some(existing) = self
            .resolvers
            .iter_mut()
            .find(|existing| existing.ip.eq_ignore_ascii_case(&resolver.ip))
        {
            for source in resolver.sources {
                if !existing
                    .sources
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(&source))
                {
                    existing.sources.push(source);
                }
            }
            if existing.country_code.is_none() {
                existing.country_code = resolver.country_code;
            }
            if existing.place.is_none() {
                existing.place = resolver.place;
            }
            if existing.organization.is_none() {
                existing.organization = resolver.organization;
            }
            return;
        }
        self.resolvers.push(resolver);
    }
}

/// Runs the DNS leak check of `provider` through the active connection.
///
/// The probe names are resolved by the node, so the authorities list the
/// resolvers the proxied connection really uses — the same answer a browser
/// behind the same proxy gets. ipleak.net can only list resolvers that queried
/// its authoritative DNS; a resolver that answers `*.ipleak.net` from a wildcard
/// cache (Google Public DNS, for example) never shows up, in which case the
/// BrowserLeaks probe set measures the same resolver path and the result
/// records the substitution.
pub async fn test_dns_leak(
    socks_port: u16,
    provider: DnsLeakProvider,
) -> Result<DnsLeakResult, String> {
    // The probe names are resolved by the node, so the authorities list the
    // resolvers this connection really uses.
    let client = proxy_test::proxied_client(socks_port, DNS_TIMEOUT)?;
    let result = match provider {
        DnsLeakProvider::Ipleak => match collect_ipleak(&client).await {
            Ok(result) => result,
            Err(ipleak_error) => {
                let mut fallback = collect_browserleaks(&client).await;
                if fallback.resolvers.is_empty() {
                    return Err(format!(
                        "{ipleak_error} The BrowserLeaks fallback also reported nothing."
                    ));
                }
                fallback.fallback = Some(DnsLeakProvider::BrowserLeaks);
                fallback
            }
        },
        DnsLeakProvider::BrowserLeaks => collect_browserleaks(&client).await,
    };
    if result.resolvers.is_empty() {
        return Err(format!("{} reported no DNS resolvers.", provider.label()));
    }
    Ok(result)
}

/// ipleak.net aggregates a whole session: every request on the same
/// 40-character session answers with the resolvers seen so far. A few probes
/// therefore capture the set without running the site's 300-request loop.
async fn collect_ipleak(client: &Client) -> Result<DnsLeakResult, String> {
    let session = ipleak_session();
    let mut handles = Vec::new();
    for step in 1..=3u32 {
        // The label has to be new for every request but short: the whole
        // `<session>-<label>` prefix still has to fit in one DNS label.
        let url = IPLEAK_DETECTION_URL
            .replace("{session}", &session)
            .replace("{label}", &step.to_string());
        let client = client.clone();
        // Probes run together so a node whose resolver swallows the probe
        // names costs one timeout, not three.
        handles.push(tokio::spawn(async move { fetch_text(&client, &url).await }));
    }

    let mut result = DnsLeakResult::default();
    let mut responded = false;
    let mut last_error = None;
    for handle in handles {
        match handle.await {
            Ok(Ok(body)) => {
                let resolvers = parse_ipleak_detection(&body);
                if resolvers.is_empty() {
                    continue;
                }
                responded = true;
                for resolver in resolvers {
                    result.merge(resolver);
                }
            }
            Ok(Err(error)) => last_error = Some(error),
            Err(_) => {}
        }
    }
    if !responded {
        return Err(match last_error {
            Some(error) => {
                format!("ipleak.net saw no DNS lookups for this connection (probe error: {error}).")
            }
            None => "ipleak.net saw no DNS lookups for this connection: the resolver answered \
                     the probe names from cache."
                .to_string(),
        });
    }
    enrich_ipleak_resolvers(client, &mut result).await;
    Ok(result)
}

/// Looks each resolver up in ipleak.net's geo database. The lookup only
/// decorates the list, so a failure leaves the resolver in place without geo
/// data instead of failing the whole check.
async fn enrich_ipleak_resolvers(client: &Client, result: &mut DnsLeakResult) {
    let mut handles = Vec::new();
    for (index, resolver) in result.resolvers.iter().enumerate() {
        let client = client.clone();
        let url = IPLEAK_JSON_URL.replace("{}", &resolver.ip);
        handles.push((
            index,
            tokio::spawn(async move { fetch_text(&client, &url).await }),
        ));
    }
    for (index, handle) in handles {
        let Ok(Ok(body)) = handle.await else {
            continue;
        };
        let Some(geo) = parse_ipleak_geo(&body) else {
            continue;
        };
        let resolver = &mut result.resolvers[index];
        resolver.country_code = geo.country_code;
        resolver.place = geo.place;
        resolver.organization = geo.organization;
    }
}

/// BrowserLeaks detects one resolver per random subdomain, so the probes run
/// in parallel and their answers are merged.
async fn collect_browserleaks(client: &Client) -> DnsLeakResult {
    let mut probes = Vec::new();
    for _ in 0..2 {
        probes.push(DNS_BROWSERLEAKS_V4_URL.replace("{}", &random_label()));
    }
    probes.push(DNS_BROWSERLEAKS_V6_URL.replace("{}", &random_label()));

    let mut handles = Vec::new();
    for url in probes {
        let client = client.clone();
        handles.push(tokio::spawn(async move { fetch_text(&client, &url).await }));
    }

    let mut result = DnsLeakResult::default();
    for handle in handles {
        let Ok(Ok(body)) = handle.await else {
            continue;
        };
        for resolver in parse_browserleaks(&body, DnsLeakProvider::BrowserLeaks.label()) {
            result.merge(resolver);
        }
    }
    result
}

async fn fetch_text(client: &Client, url: &str) -> Result<String, String> {
    client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .text()
        .await
        .map_err(|error| error.to_string())
}

/// Returns a per-request label. The value only has to be unique: the authority
/// answers with whichever resolvers looked it up.
fn random_label() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos() as u64)
        .unwrap_or_default();
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{:x}{:x}",
        nanos ^ (u64::from(std::process::id()) << 32),
        sequence.wrapping_add(1)
    )
}

/// Builds the 40-character alphanumeric session ipleak.net expects.
fn ipleak_session() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or_default();
    let mut state =
        nanos ^ (u64::from(std::process::id()) << 32) ^ COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut session = String::with_capacity(IPLEAK_SESSION_LEN);
    for _ in 0..IPLEAK_SESSION_LEN {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        session.push(
            IPLEAK_SESSION_ALPHABET[(state >> 33) as usize % IPLEAK_SESSION_ALPHABET.len()] as char,
        );
    }
    session
}

/// ipleak.net answers with
/// `{"session": "...", "ip": {"<resolver>": <hits>}}`.
fn parse_ipleak_detection(body: &str) -> Vec<DnsResolver> {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let Some(Value::Object(resolvers)) = value.get("ip") else {
        return Vec::new();
    };
    resolvers
        .keys()
        .map(|ip| ip.trim())
        .filter(|ip| !ip.is_empty())
        .map(|ip| DnsResolver {
            ip: ip.to_string(),
            country_code: None,
            place: None,
            organization: None,
            sources: vec![DnsLeakProvider::Ipleak.label().to_string()],
        })
        .collect()
}

/// Geo and network data from ipleak.net's `/json/<ip>`.
struct IpleakGeo {
    country_code: Option<String>,
    place: Option<String>,
    organization: Option<String>,
}

/// ipleak.net describes an address with MaxMind data. City and region are
/// often null for resolvers, so the country name is the last resort for a
/// readable place.
fn parse_ipleak_geo(body: &str) -> Option<IpleakGeo> {
    let value = serde_json::from_str::<Value>(body).ok()?;
    if text_field(&value, "ip").is_none() && text_field(&value, "query_text").is_none() {
        return None;
    }
    Some(IpleakGeo {
        country_code: text_field(&value, "country_code")
            .map(|code| code.to_ascii_uppercase())
            .filter(|code| code.len() == 2),
        place: text_field(&value, "city_name")
            .or_else(|| text_field(&value, "region_name"))
            .or_else(|| text_field(&value, "country_name")),
        organization: text_field(&value, "isp_name"),
    })
}

/// BrowserLeaks keys its answer by resolver IP and describes each one as
/// `[country code, "Country, City", organization]`.
fn parse_browserleaks(body: &str, source: &str) -> Vec<DnsResolver> {
    let Ok(Value::Object(resolvers)) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    resolvers
        .iter()
        .filter_map(|(ip, entry)| {
            let fields = entry.as_array()?;
            Some(DnsResolver {
                ip: ip.clone(),
                country_code: fields
                    .first()
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|code| !code.is_empty())
                    .map(str::to_string),
                place: fields
                    .get(1)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|place| !place.is_empty())
                    .map(str::to_string),
                organization: fields
                    .get(2)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|organization| !organization.is_empty())
                    .map(str::to_string),
                sources: vec![source.to_string()],
            })
        })
        .filter(|resolver| !resolver.ip.is_empty())
        .collect()
}

/// What IPPure makes of the exit address.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IppureResult {
    pub ip: String,
    /// IPPure's risk coefficient, 0-100.
    pub risk_score: Option<u32>,
    pub asn_number: Option<u64>,
    pub asn_organization: Option<String>,
    pub asn_type: Option<String>,
    pub is_residential: Option<bool>,
    pub is_broadcast: Option<bool>,
    pub is_warp: Option<bool>,
    pub country_code: Option<String>,
    pub place: Option<String>,
    pub human_ratio: Option<f64>,
    pub bot_ratio: Option<f64>,
}

impl IppureResult {
    pub fn ip_type(&self) -> &'static str {
        ip_type_label(self.asn_type.as_deref(), self.is_residential)
    }

    pub fn nature(&self) -> &'static str {
        ip_nature(self.is_broadcast)
    }

    pub fn risk_level(&self) -> Option<(&'static str, &'static str)> {
        self.risk_score.map(risk_level)
    }
}

/// Maps IPPure's coefficient to a label and a colour from the app palette.
pub fn risk_level(score: u32) -> (&'static str, &'static str) {
    match score {
        0..=15 => ("极度纯净", "#10b981"),
        16..=25 => ("纯净", "#34d399"),
        26..=40 => ("中性", "#3b82f6"),
        41..=50 => ("轻度风险", "#f59e0b"),
        51..=70 => ("中度风险", "#f97316"),
        _ => ("极度风险", "#ef4444"),
    }
}

/// IPPure's ASN class, translated to the labels its own dashboard uses.
pub fn ip_type_label(asn_type: Option<&str>, is_residential: Option<bool>) -> &'static str {
    match asn_type.map(str::to_ascii_lowercase).as_deref() {
        Some("isp") => "住宅IP",
        Some("enterprise") => "商业宽带",
        Some("education") => "教育IP",
        Some("government") => "政府IP",
        Some("hosting") | Some("business") => "机房IP",
        _ if is_residential == Some(true) => "住宅IP",
        _ => "机房IP",
    }
}

/// Broadcast addresses are announced by a provider other than their user's;
/// everything else is treated as native to its ASN.
pub fn ip_nature(is_broadcast: Option<bool>) -> &'static str {
    if is_broadcast == Some(true) {
        "广播IP"
    } else {
        "原生IP"
    }
}

/// Looks up the active exit IP, then asks IPPure for its ASN, geo and risk
/// data. The risk and bot classification endpoints are best-effort.
pub async fn test_ippure(socks_port: u16) -> Result<IppureResult, String> {
    let client = proxy_test::proxied_client(socks_port, IPPURE_TIMEOUT)?;
    let exit = proxy_test::exit_from_cloudflare(&client).await?;
    let ip = exit.ip;

    let basic = ippure_get(&client, &format!("/api/info/ip-basic/{ip}")).await?;
    let mut result = parse_ip_basic(&basic);
    result.ip = ip;

    if let Ok(risk) = ippure_get(&client, &format!("/api/info/ip-risk/{}", result.ip)).await {
        result.risk_score = risk
            .get("data")
            .and_then(|data| data.get("risk_score"))
            .and_then(Value::as_f64)
            .map(|score| score.round().clamp(0.0, 100.0) as u32);
    }

    if let Some(asn_number) = result.asn_number {
        if let Ok(classification) =
            ippure_get(&client, &format!("/api/info/asn/botclass/{asn_number}")).await
        {
            if let Some(data) = classification.get("data") {
                result.bot_ratio = data.get("bot").and_then(Value::as_f64);
                result.human_ratio = data.get("human").and_then(Value::as_f64);
            }
        }
    }

    Ok(result)
}

/// Performs IPPure's challenge: the first request returns the signing key and
/// timestamp, and the signed request has to travel over the same TCP
/// connection. `reqwest` keeps it in the pool because the unsigned response is
/// fully read before the signed one is sent.
async fn ippure_get(client: &Client, path: &str) -> Result<Value, String> {
    let url = format!("{IPPURE_BASE}{path}");
    let response = client
        .get(&url)
        .headers(ippure_headers())
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let key = response_header(&response, "x-k");
    let stamp = response_header(&response, "x-t");
    let unsigned_status = response.status();
    let unsigned_body = response.text().await.map_err(|error| error.to_string())?;

    let (Some(key), Some(stamp)) = (key, stamp) else {
        return parse_ippure_body(unsigned_status, &unsigned_body);
    };

    let signature = sign_ippure(&key, &url, &stamp)?;
    let response = client
        .get(&url)
        .headers(ippure_headers())
        .header("x-k", key)
        .header("x-t", format!("{stamp}-{signature}"))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|error| error.to_string())?;
    parse_ippure_body(status, &body)
}

fn ippure_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/plain, */*"),
    );
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    headers.insert(ORIGIN, HeaderValue::from_static(IPPURE_ORIGIN));
    headers.insert(REFERER, HeaderValue::from_static("https://ippure.com/"));
    headers
}

fn response_header(response: &reqwest::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn sign_ippure(key: &str, url: &str, stamp: &str) -> Result<String, String> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())
        .map_err(|error| format!("IPPure's signing key is unusable: {error}"))?;
    mac.update(format!("GET-{url}--{stamp}").as_bytes());
    Ok(hex_lower(&mac.finalize().into_bytes()))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn parse_ippure_body(status: StatusCode, body: &str) -> Result<Value, String> {
    let value = serde_json::from_str::<Value>(body)
        .map_err(|_| format!("IPPure returned an unreadable response (HTTP {status})."))?;
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(format!("IPPure rejected the request: {message}"));
    }
    if !status.is_success() {
        return Err(format!("IPPure returned HTTP {status}."));
    }
    Ok(value)
}

fn parse_ip_basic(value: &Value) -> IppureResult {
    let data = value.get("data").unwrap_or(value);
    let asn = data.get("asn");
    let (country_code, place) = geo_fields(data);
    IppureResult {
        ip: text_field(data, "ip").unwrap_or_default(),
        risk_score: None,
        asn_number: asn
            .and_then(|asn| asn.get("number"))
            .and_then(Value::as_u64),
        asn_organization: asn.and_then(|asn| text_field(asn, "organization")),
        asn_type: asn.and_then(|asn| text_field(asn, "type")),
        is_residential: asn
            .and_then(|asn| asn.get("is_residential"))
            .and_then(Value::as_bool),
        is_broadcast: data
            .get("traits")
            .and_then(|traits| traits.get("is_broadcast"))
            .and_then(Value::as_bool),
        is_warp: data
            .get("traits")
            .and_then(|traits| traits.get("is_warp"))
            .and_then(Value::as_bool),
        country_code,
        place,
        human_ratio: None,
        bot_ratio: None,
    }
}

/// Picks the first geo database that knows the address, preferring the one
/// IPPure itself shows first.
fn geo_fields(data: &Value) -> (Option<String>, Option<String>) {
    for database in ["IP2Location", "DB-IP", "MaxMind"] {
        let Some(geo) = data.get("geo").and_then(|geo| geo.get(database)) else {
            continue;
        };
        let country = text_field(geo, "country_code");
        let place = join_place(text_field(geo, "city"), text_field(geo, "subdivisions"));
        if country.is_some() || place.is_some() {
            return (country, place);
        }
    }
    (None, None)
}

/// Joins a city and a subdivision without repeating the same name, which is
/// how city-states such as Singapore are reported by geo databases.
fn join_place(city: Option<String>, subdivision: Option<String>) -> Option<String> {
    match (city, subdivision) {
        (Some(city), Some(subdivision)) if !city.eq_ignore_ascii_case(&subdivision) => {
            Some(format!("{city}, {subdivision}"))
        }
        (Some(city), _) => Some(city),
        (None, subdivision) => subdivision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ipleak_detection_answers_become_resolvers() {
        let body = r#"{
            "session": "c246d927616c806b2b0766a33427f19a631d2f42",
            "ip": {
                "203.0.113.9": 4,
                "2409:806c:2000:4200::2004": 2
            }
        }"#;
        let mut resolvers = parse_ipleak_detection(body);
        resolvers.sort_by(|left, right| left.ip.cmp(&right.ip));
        assert_eq!(resolvers.len(), 2);
        assert_eq!(resolvers[0].ip, "203.0.113.9");
        assert_eq!(resolvers[1].ip, "2409:806c:2000:4200::2004");
        assert!(resolvers
            .iter()
            .all(|resolver| resolver.sources == ["ipleak.net"]));
    }

    #[test]
    fn ipleak_geo_describes_a_resolver() {
        let body = r#"{
            "country_code": "JP",
            "country_name": "Japan",
            "city_name": "Tokyo",
            "region_name": "Tokyo",
            "isp_name": "Example Networks",
            "ip": "203.0.113.9"
        }"#;
        let geo = parse_ipleak_geo(body).expect("geo payload parses");
        assert_eq!(geo.country_code.as_deref(), Some("JP"));
        assert_eq!(geo.place.as_deref(), Some("Tokyo"));
        assert_eq!(geo.organization.as_deref(), Some("Example Networks"));
    }

    #[test]
    fn ipleak_geo_falls_back_to_the_country_and_rejects_errors() {
        let body = r#"{
            "country_code": "US",
            "country_name": "United States",
            "city_name": null,
            "ip": "203.0.113.9"
        }"#;
        let geo = parse_ipleak_geo(body).expect("geo payload parses");
        assert_eq!(geo.place.as_deref(), Some("United States"));
        assert!(parse_ipleak_geo(r#"{"error": "Invalid IP address"}"#).is_none());
    }

    #[test]
    fn ipleak_sessions_are_forty_alphanumeric_characters() {
        let first = ipleak_session();
        let second = ipleak_session();
        assert_eq!(first.len(), IPLEAK_SESSION_LEN);
        assert!(first
            .chars()
            .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit()));
        assert_ne!(first, second);
    }

    #[test]
    fn provider_values_round_trip() {
        assert_eq!(
            DnsLeakProvider::from_value("ipleak"),
            DnsLeakProvider::Ipleak
        );
        assert_eq!(
            DnsLeakProvider::from_value("browserleaks"),
            DnsLeakProvider::BrowserLeaks
        );
        assert_eq!(
            DnsLeakProvider::from_value("unknown"),
            DnsLeakProvider::Ipleak
        );
        assert_eq!(DnsLeakProvider::default(), DnsLeakProvider::Ipleak);
        assert_eq!(DnsLeakProvider::BrowserLeaks.value(), "browserleaks");
    }

    #[test]
    fn browserleaks_rows_become_resolvers() {
        let body = r#"{
            "198.51.100.4": ["NL", "Netherlands, Amsterdam", "Example Networks B.V."]
        }"#;
        let resolvers = parse_browserleaks(body, "BrowserLeaks");
        assert_eq!(resolvers.len(), 1);
        assert_eq!(resolvers[0].country_code.as_deref(), Some("NL"));
        assert_eq!(
            resolvers[0].place.as_deref(),
            Some("Netherlands, Amsterdam")
        );
        assert_eq!(
            resolvers[0].organization.as_deref(),
            Some("Example Networks B.V.")
        );
    }

    #[test]
    fn merging_keeps_one_row_per_resolver_and_collects_sources() {
        let mut result = DnsLeakResult::default();
        result.merge(DnsResolver {
            ip: "203.0.113.9".to_string(),
            country_code: Some("US".to_string()),
            place: Some("Los Angeles".to_string()),
            organization: Some("Example ISP".to_string()),
            sources: vec!["Surfshark".to_string()],
        });
        result.merge(DnsResolver {
            ip: "203.0.113.9".to_string(),
            country_code: Some("US".to_string()),
            place: None,
            organization: None,
            sources: vec!["BrowserLeaks".to_string()],
        });
        result.merge(DnsResolver {
            ip: "203.0.113.10".to_string(),
            country_code: Some("US".to_string()),
            place: None,
            organization: None,
            sources: vec!["BrowserLeaks".to_string()],
        });

        assert_eq!(result.resolvers.len(), 2);
        assert_eq!(
            result.resolvers[0].sources,
            vec!["Surfshark", "BrowserLeaks"]
        );
        assert_eq!(result.resolvers[0].place.as_deref(), Some("Los Angeles"));
    }

    #[test]
    fn verdict_compares_resolvers_with_the_exit_country() {
        let resolvers = vec![
            DnsResolver {
                ip: "203.0.113.9".to_string(),
                country_code: Some("US".to_string()),
                place: None,
                organization: None,
                sources: vec!["Surfshark".to_string()],
            },
            DnsResolver {
                ip: "203.0.113.10".to_string(),
                country_code: Some("CA".to_string()),
                place: None,
                organization: None,
                sources: vec!["Surfshark".to_string()],
            },
        ];
        let all_us = DnsLeakResult {
            resolvers: resolvers[..1].to_vec(),
            fallback: None,
        };
        let two_countries = DnsLeakResult {
            resolvers,
            fallback: None,
        };

        assert_eq!(all_us.verdict(Some("us")), DnsLeakVerdict::Protected);
        assert_eq!(two_countries.verdict(Some("US")), DnsLeakVerdict::Leaking);
        assert_eq!(two_countries.verdict(Some("JP")), DnsLeakVerdict::Leaking);
        assert_eq!(two_countries.verdict(None), DnsLeakVerdict::Mixed);
        assert_eq!(all_us.verdict(None), DnsLeakVerdict::Mixed);
    }

    #[test]
    fn verdict_flags_a_minority_resolver_outside_the_exit_country() {
        // One resolver in the exit country does not excuse the others: every
        // resolver outside it answered some queries through the leaky path.
        let resolvers = vec![
            DnsResolver {
                ip: "203.0.113.9".to_string(),
                country_code: Some("HK".to_string()),
                place: None,
                organization: None,
                sources: vec!["ipleak.net".to_string()],
            },
            DnsResolver {
                ip: "114.114.114.114".to_string(),
                country_code: Some("CN".to_string()),
                place: None,
                organization: None,
                sources: vec!["ipleak.net".to_string()],
            },
        ];
        let result = DnsLeakResult {
            resolvers,
            fallback: None,
        };

        assert_eq!(result.verdict(Some("HK")), DnsLeakVerdict::Leaking);
        assert_eq!(result.verdict(None), DnsLeakVerdict::Mixed);
    }

    #[test]
    fn verdict_ignores_resolvers_without_geo_data() {
        let resolver = |ip: &str, country: Option<&str>| DnsResolver {
            ip: ip.to_string(),
            country_code: country.map(str::to_string),
            place: None,
            organization: None,
            sources: vec!["ipleak.net".to_string()],
        };
        let partial = DnsLeakResult {
            resolvers: vec![
                resolver("203.0.113.9", Some("JP")),
                resolver("203.0.113.10", None),
            ],
            fallback: None,
        };
        let unknown = DnsLeakResult {
            resolvers: vec![resolver("203.0.113.11", None)],
            fallback: None,
        };

        assert_eq!(partial.verdict(Some("JP")), DnsLeakVerdict::Protected);
        assert_eq!(partial.verdict(Some("US")), DnsLeakVerdict::Leaking);
        assert_eq!(unknown.verdict(Some("JP")), DnsLeakVerdict::Mixed);
    }

    #[test]
    fn ip_basic_payload_names_the_network() {
        let value = json!({
            "ok": true,
            "data": {
                "ip": "45.196.216.166",
                "asn": {
                    "number": 401434,
                    "organization": "Hytron Network Services (US) LLC",
                    "is_residential": false,
                    "type": "hosting"
                },
                "geo": {
                    "IP2Location": {
                        "country_code": "HK",
                        "subdivisions": "Hong Kong",
                        "city": "Hong Kong",
                        "postal_code": "-"
                    },
                    "MaxMind": { "country_code": "HK", "city": "" }
                },
                "traits": { "is_broadcast": true, "is_warp": false }
            }
        });
        let result = parse_ip_basic(&value);
        assert_eq!(result.ip, "45.196.216.166");
        assert_eq!(result.asn_number, Some(401_434));
        assert_eq!(
            result.asn_organization.as_deref(),
            Some("Hytron Network Services (US) LLC")
        );
        assert_eq!(result.country_code.as_deref(), Some("HK"));
        assert_eq!(result.place.as_deref(), Some("Hong Kong"));
        assert_eq!(result.ip_type(), "机房IP");
        assert_eq!(result.nature(), "广播IP");
        assert!(!result.is_warp.unwrap_or(true));
    }

    #[test]
    fn risk_scores_map_to_labels_and_app_colours() {
        assert_eq!(risk_level(0).0, "极度纯净");
        assert_eq!(risk_level(15).0, "极度纯净");
        assert_eq!(risk_level(16).0, "纯净");
        assert_eq!(risk_level(26).0, "中性");
        assert_eq!(risk_level(41).0, "轻度风险");
        assert_eq!(risk_level(51).0, "中度风险");
        assert_eq!(risk_level(100).0, "极度风险");
        assert_eq!(risk_level(100).1, "#ef4444");
    }

    #[test]
    fn asn_types_map_to_the_badge_labels() {
        assert_eq!(ip_type_label(Some("isp"), Some(false)), "住宅IP");
        assert_eq!(ip_type_label(Some("hosting"), None), "机房IP");
        assert_eq!(ip_type_label(Some("business"), None), "机房IP");
        assert_eq!(ip_type_label(Some("enterprise"), None), "商业宽带");
        assert_eq!(ip_type_label(Some("education"), None), "教育IP");
        assert_eq!(ip_type_label(Some("government"), None), "政府IP");
        assert_eq!(ip_type_label(None, Some(true)), "住宅IP");
        assert_eq!(ip_type_label(None, None), "机房IP");
        assert_eq!(ip_nature(Some(true)), "广播IP");
        assert_eq!(ip_nature(Some(false)), "原生IP");
        assert_eq!(ip_nature(None), "原生IP");
    }

    #[test]
    fn signing_matches_the_hmac_ippure_expects() {
        // HMAC-SHA256("key", "GET-https://api.ippure.com/api/info/ip-basic/8.8.8.8--1700000000000")
        let signature = sign_ippure(
            "key",
            "https://api.ippure.com/api/info/ip-basic/8.8.8.8",
            "1700000000000",
        )
        .expect("signing works");
        assert_eq!(
            signature,
            "8ebdd67a3d3ace86ef58f472526b57e54234d59d2b707f68c88f592b465c71d6"
        );
    }
}
