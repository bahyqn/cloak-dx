use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use reqwest::Client;
use serde_json::Value;

use super::{
    country as country_names,
    database::{Database, Subscription},
    proxy_core::ProxyCoreService,
};

/// Google-owned endpoint used for latency. It is much faster to reach than
/// the search homepage, which can take more than ten seconds on some nodes.
const LATENCY_URL: &str = "https://www.gstatic.com/generate_204";
/// Google's own IP geolocation, shown in the "my location" answer card. It is
/// noticeably more accurate than the databases behind generic geo IP services.
const GOOGLE_SEARCH_URL: &str = "https://www.google.com/search?q=my+location&hl=en&pws=0";
/// YouTube runs on the same GeoIP lookup and writes its result into the page.
const GOOGLE_GEO_URL: &str = "https://www.youtube.com/";
/// Requesting the generic homepage makes Google redirect to the country site
/// it decided this IP belongs to (`google.com.hk`, `google.co.uk`, ...). That
/// redirect is the same decision a browser sees.
const GOOGLE_HOME_URL: &str = "https://www.google.com/";
/// Answering Google's consent prompt keeps European exits on the real page
/// instead of a consent screen without any location.
const GOOGLE_CONSENT_COOKIE: &str = "SOCS=CAI";
/// Cloudflare reports the connecting exit IP and its country code.
const EXIT_TRACE_URL: &str = "https://www.cloudflare.com/cdn-cgi/trace";
/// Fallback for the exit IP when the Cloudflare trace endpoint is unavailable.
const EXIT_INFO_URL: &str = "https://ipwho.is/";
/// Google serves a consent page or a stripped-down page to unknown clients, so
/// the probe presents itself as a regular desktop browser.
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
const DOWNLOAD_URL: &str = "https://speed.cloudflare.com/__down?bytes=26214400";
const UPLOAD_URL: &str = "https://speed.cloudflare.com/__up";
const UPLOAD_BYTES: usize = 1024 * 1024;
/// Download throughput is sampled for a fixed window instead of waiting for a
/// whole payload. Slow nodes would otherwise time out before reporting
/// anything, which is why every speed test used to fail.
const DOWNLOAD_BUDGET: Duration = Duration::from_secs(8);
const DOWNLOAD_LIMIT: usize = 25 * 1024 * 1024;
const NODE_TIMEOUT: Duration = Duration::from_secs(10);
const SPEED_TIMEOUT: Duration = Duration::from_secs(30);
/// A latency test sends this many probes so it can report packet loss next to
/// the round trip. Four requests against a 204 endpoint stay cheap.
const LATENCY_SAMPLES: usize = 4;
/// Individual samples give up sooner than the whole test, so one dead probe
/// cannot hold the other three for the full node timeout.
const LATENCY_SAMPLE_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeedTestResult {
    pub download_mbps: f64,
    pub upload_mbps: Option<f64>,
}

/// Where a node's traffic comes out when it talks to Google.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoogleExitResult {
    pub ip: String,
    pub country_code: Option<String>,
    pub country: Option<String>,
    pub region: Option<String>,
}

/// Latency plus the Google exit observed during the same probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LatencyTestResult {
    /// Average round trip of the probes that succeeded.
    pub milliseconds: u64,
    /// Share of the latency probes that never answered (`0..=100`).
    pub packet_loss_percent: u8,
    /// The exit probe is best-effort: latency is still reported when the geo
    /// service cannot be reached through the node.
    pub google_exit: Result<GoogleExitResult, String>,
}

/// Measures the round-trip latency to Google and records the node's exit.
///
/// Both checks share one temporary Xray instance so a single test costs one
/// core start instead of two.
pub async fn test_latency(
    service: &ProxyCoreService,
    database: &Database,
    subscription: &Subscription,
    node_key: &str,
) -> Result<LatencyTestResult, String> {
    let core = service.start_temporary_core(database, subscription, node_key)?;
    let socks_port = core
        .socks_port()
        .ok_or_else(|| "The Xray config does not expose a SOCKS inbound.".to_string())?;
    let client = proxied_client(socks_port, NODE_TIMEOUT)?;

    // The exit lookup is the slowest part of the probe, so run it alongside
    // the latency request instead of adding the two round trips together.
    let exit_client = client.clone();
    let exit_task = tokio::spawn(async move { google_exit(&exit_client).await });

    // Sequential samples give packet loss a meaning: each probe either
    // answered or did not by the time the request timeout expired.
    let mut samples = Vec::with_capacity(LATENCY_SAMPLES);
    let mut last_error = String::new();
    for _ in 0..LATENCY_SAMPLES {
        let started = Instant::now();
        match client
            .get(LATENCY_URL)
            .timeout(LATENCY_SAMPLE_TIMEOUT)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                samples.push(started.elapsed().as_millis() as u64);
            }
            Ok(response) => {
                last_error = format!(
                    "The Google latency probe returned HTTP {}.",
                    response.status()
                );
            }
            Err(error) => last_error = error.to_string(),
        }
    }
    let Some((milliseconds, packet_loss_percent)) = summarize_samples(&samples, LATENCY_SAMPLES)
    else {
        exit_task.abort();
        return Err(last_error);
    };
    let google_exit = match exit_task.await {
        Ok(result) => result,
        Err(error) => Err(format!("The Google exit probe did not finish: {error}")),
    };

    Ok(LatencyTestResult {
        milliseconds,
        packet_loss_percent,
        google_exit,
    })
}

/// Averages the answered probes and derives the share that timed out.
fn summarize_samples(samples: &[u64], attempts: usize) -> Option<(u64, u8)> {
    if samples.is_empty() || attempts == 0 {
        return None;
    }
    let average = samples.iter().sum::<u64>() / samples.len() as u64;
    let lost = attempts.saturating_sub(samples.len());
    let loss = (lost * 100 / attempts) as u8;
    Some((average, loss))
}

/// Runs only the Google exit probe for a node. The card's manual test button
/// uses this so it does not also pay for a latency measurement.
pub async fn test_google_exit(
    service: &ProxyCoreService,
    database: &Database,
    subscription: &Subscription,
    node_key: &str,
) -> Result<GoogleExitResult, String> {
    let core = service.start_temporary_core(database, subscription, node_key)?;
    let socks_port = core
        .socks_port()
        .ok_or_else(|| "The Xray config does not expose a SOCKS inbound.".to_string())?;
    let client = proxied_client(socks_port, NODE_TIMEOUT)?;
    google_exit(&client).await
}

/// Runs the Google exit probe through an already running core, which is what
/// the Network page uses: the answer describes the connection that is actually
/// serving traffic rather than a detached test instance.
pub async fn probe_google_exit(socks_port: u16) -> Result<GoogleExitResult, String> {
    let client = proxied_client(socks_port, NODE_TIMEOUT)?;
    google_exit(&client).await
}

/// Measures download (and best-effort upload) throughput for a node.
pub async fn test_speed(
    service: &ProxyCoreService,
    database: &Database,
    subscription: &Subscription,
    node_key: &str,
) -> Result<SpeedTestResult, String> {
    let core = service.start_temporary_core(database, subscription, node_key)?;
    let socks_port = core
        .socks_port()
        .ok_or_else(|| "The Xray config does not expose a SOCKS inbound.".to_string())?;
    let client = proxied_client(socks_port, SPEED_TIMEOUT)?;

    let mut response = client
        .get(DOWNLOAD_URL)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;

    // The server keeps sending until its byte count is reached, so stop after
    // a short sampling window and derive the rate from what arrived.
    let started = Instant::now();
    let deadline = started + DOWNLOAD_BUDGET;
    let mut downloaded = 0_usize;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, response.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                downloaded += chunk.len();
                if downloaded >= DOWNLOAD_LIMIT {
                    break;
                }
            }
            Ok(Ok(None)) => break,
            Ok(Err(error)) => {
                if downloaded == 0 {
                    return Err(error.to_string());
                }
                break;
            }
            Err(_) => break,
        }
    }
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    if downloaded == 0 {
        return Err("The speed test download returned no data.".to_string());
    }
    let download_mbps = downloaded as f64 / 1_048_576.0 / elapsed;

    Ok(SpeedTestResult {
        download_mbps,
        upload_mbps: measure_upload(&client).await.ok(),
    })
}

async fn measure_upload(client: &Client) -> Result<f64, String> {
    let payload = vec![0_u8; UPLOAD_BYTES];
    let started = Instant::now();
    let response = client
        .post(UPLOAD_URL)
        .body(payload)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;
    let _ = response.bytes().await.map_err(|error| error.to_string())?;
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    Ok(UPLOAD_BYTES as f64 / 1_048_576.0 / elapsed)
}

/// Resolves the exit IP and the location Google infers for it.
///
/// Google Search builds its "my location" answer card with JavaScript, so a
/// plain HTTP client normally receives only the interstitial Google sends
/// before that script runs. The footer of every search page, however, is
/// server-rendered and names the place Google assigned to the IP, city
/// included. The probe reads that line first and falls back to the answer
/// card, YouTube's server-rendered country decision (`"countryCode"`/`"gl"`),
/// and the exit IP's geo service. A probe that cannot read a location fails
/// instead of overwriting the node's last stored result.
pub(crate) async fn google_exit(client: &Client) -> Result<GoogleExitResult, String> {
    let search_client = client.clone();
    let search_task = tokio::spawn(async move { google_location(&search_client).await });
    let (trace, geo) = tokio::join!(exit_from_cloudflare(client), exit_from_ipwho(client));
    let trace = trace.ok();
    let geo = geo.ok();

    let mut location = match search_task.await {
        Ok(result) => result?,
        Err(error) => return Err(format!("The Google exit probe did not finish: {error}")),
    };

    let ip = trace
        .as_ref()
        .map(|trace| trace.ip.clone())
        .or_else(|| geo.as_ref().map(|geo| geo.ip.clone()))
        .ok_or_else(|| "The exit probe could not determine an IP.".to_string())?;

    if let Some(geo) = geo {
        if location.country_code.is_none() {
            location.country_code = geo.country_code.clone();
        }
        if location.country.is_none() {
            location.country = geo.country.clone();
        }
        // A city from a database that disagrees about the country would put
        // the node in two places at once, so it is only used when both agree.
        let same_country = match (&location.country_code, &geo.country_code) {
            (Some(google), Some(database)) => google.eq_ignore_ascii_case(database),
            _ => true,
        };
        if location.region.is_none() && same_country {
            location.region = geo.region;
        }
    }

    Ok(GoogleExitResult {
        ip,
        country_code: location.country_code,
        country: location.country,
        region: location.region,
    })
}

/// The place Google believes the connecting IP is in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct GoogleLocation {
    country_code: Option<String>,
    country: Option<String>,
    region: Option<String>,
    /// The region came from the footer Google renders from its own GeoIP
    /// lookup. It describes the place Google resolved for the IP, so it stays
    /// valid even when the country Google localizes the site to disagrees —
    /// the case that matters for Hong Kong–routed exits in mainland China.
    region_from_footer: bool,
}

/// Prefers the country Google localizes its homepage to, then the country it
/// reports on its own video site, then the search answer card.
///
/// The redirect is ranked first because it is the behavior a browser actually
/// experiences: a node that Google routes to `google.com.hk` is in Hong Kong
/// for Google even when an IP database still labels it as the country it is
/// registered in.
async fn google_location(client: &Client) -> Result<GoogleLocation, String> {
    let first = google_location_once(client).await;
    if first.is_ok() {
        return first;
    }
    // Residential exits drop a connection now and then, and one dropped body
    // used to fail the whole probe even though a retry answers normally. A
    // second attempt separates that from a node that cannot reach Google.
    google_location_once(client).await
}

async fn google_location_once(client: &Client) -> Result<GoogleLocation, String> {
    let (search, country, redirect) = tokio::join!(
        google_search_card(client),
        youtube_google_country(client),
        google_redirect_country(client)
    );
    if let (Err(search_error), Err(country_error), Err(redirect_error)) =
        (&search, &country, &redirect)
    {
        return Err(format!(
            "Google did not report a location ({search_error}; {country_error}; {redirect_error})."
        ));
    }
    let search = search.ok();
    let country = country.ok();
    let redirect = redirect.ok();

    let country_code = redirect.or_else(|| country).or_else(|| {
        search
            .as_ref()
            .and_then(|location| location.country_code.clone())
    });
    let country_name = country_code
        .as_deref()
        .and_then(country_names::country_name)
        .map(str::to_string)
        .or_else(|| {
            search
                .as_ref()
                .and_then(|location| location.country.clone())
        });
    // A city from a card that disagrees about the country would put the node
    // in two places at once, so it is only used when both agree. Google's own
    // footer location is exempt: it is the place Google resolved for the IP,
    // and describing a Hong Kong–localized exit with its real mainland city is
    // exactly what it is for.
    let region = search
        .as_ref()
        .and_then(|location| region_for_country(location, country_code.as_deref()));

    Ok(GoogleLocation {
        country_code,
        country: country_name,
        region,
        // Only `parse_google_search_location` produces footer regions; the
        // flag has served its purpose by now.
        region_from_footer: false,
    })
}

/// Picks the region reported by the search page, dropping answer-card cities
/// whose country disagrees with the country Google localized the site to.
///
/// Footer regions are kept unconditionally: Google derives them from the same
/// GeoIP lookup that decides the site's country, so "Google localized this IP
/// to Hong Kong but names Shenzhen as the place" is a real result rather than
/// a parsing mistake.
fn region_for_country(location: &GoogleLocation, country_code: Option<&str>) -> Option<String> {
    if !location.region_from_footer {
        let agrees = match (country_code, location.country_code.as_deref()) {
            (Some(expected), Some(reported)) => expected.eq_ignore_ascii_case(reported),
            _ => true,
        };
        if !agrees {
            return None;
        }
    }
    location.region.clone()
}

/// Reads the country domain Google redirects the generic homepage to.
async fn google_redirect_country(client: &Client) -> Result<String, String> {
    let response = client
        .get(GOOGLE_HOME_URL)
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .header(reqwest::header::COOKIE, GOOGLE_CONSENT_COOKIE)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Google returned HTTP {status}"));
    }
    country_from_google_host(response.url().host_str().unwrap_or_default())
        .ok_or_else(|| "Google did not localize its homepage".to_string())
}

/// Maps a Google country domain onto a country code.
///
/// `www.google.com.hk` becomes `HK` and `www.google.co.uk` becomes `GB`.
/// `www.google.com` is Google's unlocalized fallback and carries no signal.
fn country_from_google_host(host: &str) -> Option<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let suffix = host.strip_prefix("google.")?;
    let label = suffix.rsplit('.').next().unwrap_or_default();
    if label.len() != 2 || !label.bytes().all(|byte| byte.is_ascii_alphabetic()) || label == "com" {
        return None;
    }
    let code = if label == "uk" { "GB" } else { label };
    let code = code.to_ascii_uppercase();
    country_names::is_country_code(&code).then_some(code)
}

/// Reads Google's IP-derived location from the `my location` search result.
async fn google_search_card(client: &Client) -> Result<GoogleLocation, String> {
    let response = client
        .get(GOOGLE_SEARCH_URL)
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .header(reqwest::header::COOKIE, GOOGLE_CONSENT_COOKIE)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Google Search returned HTTP {status}"));
    }
    let body = response.text().await.map_err(|error| error.to_string())?;

    parse_google_search_location(&body)
        .ok_or_else(|| "Google Search did not include a location card".to_string())
}

/// Extracts the place name from the answer card Google renders for
/// "my location". The card uses a handful of stable class names; the location
/// can be a single `"Tokyo, Japan"` line or a city line plus a country line.
///
/// The footer line is preferred for the city when present: it is always
/// server-rendered, while the card itself usually needs JavaScript.
fn parse_google_search_location(html: &str) -> Option<GoogleLocation> {
    const ANSWER_CLASSES: &[&str] = &["Z0LcW", "iAIpCb", "LrzXr"];
    const ANSWER_ATTRIDS: &[&str] = &["location", "address"];

    let mut texts = Vec::new();
    for class in ANSWER_CLASSES {
        texts.extend(element_texts(html, |tag| tag_has_class(tag, class)));
    }
    for attrid in ANSWER_ATTRIDS {
        texts.extend(element_texts(html, |tag| {
            tag_attribute(tag, "data-attrid")
                .is_some_and(|value| value.eq_ignore_ascii_case(attrid))
        }));
    }
    texts.retain(|text| is_location_text(text));
    texts.dedup_by(|left, right| left.eq_ignore_ascii_case(right));

    let country = texts
        .iter()
        .find_map(|text| country_names::country_from_name(text));
    let mut region_parts = Vec::new();
    for text in &texts {
        if let Some(remainder) = region_from_place_name(text) {
            if !region_parts
                .iter()
                .any(|part: &String| part.eq_ignore_ascii_case(&remainder))
            {
                region_parts.push(remainder);
            }
        }
    }
    let card_region = (!region_parts.is_empty()).then(|| region_parts.join(", "));
    let footer_region = google_footer_location(html)
        .as_deref()
        .and_then(region_from_place_name);
    let region_from_footer = footer_region.is_some();
    let region = footer_region.or(card_region);
    let country_code = country
        .map(|(_, code)| code.to_string())
        .or_else(|| country_code_from_gl(html));
    let country_name = country.map(|(name, _)| name.to_string()).or_else(|| {
        country_code
            .as_deref()
            .and_then(country_names::country_name)
            .map(str::to_string)
    });

    if country_code.is_none() && country_name.is_none() && region.is_none() {
        None
    } else {
        Some(GoogleLocation {
            country_code,
            country: country_name,
            region,
            region_from_footer,
        })
    }
}

/// Reduces a place name such as `"Shenzhen, Guangdong, China"` to the
/// city/region part, using the same country table as the answer card.
fn region_from_place_name(text: &str) -> Option<String> {
    let remainder = country_names::strip_country_name(text);
    let remainder = remainder
        .trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, ',' | '.' | '-' | '|' | '/' | '·')
        })
        .trim();
    (!remainder.is_empty()).then(|| remainder.to_string())
}

/// Reads the place Google prints in the footer of its search pages, such as
/// `Shenzhen, Guangdong, China · Based on your past activity`.
///
/// The footer is server-rendered and comes from the same GeoIP lookup behind
/// the answer card, so it is the only place Google reveals the city to a plain
/// HTTP client. Google's boilerplate strings are used as anchors instead of
/// its rotating class names.
fn google_footer_location(html: &str) -> Option<String> {
    const MARKERS: &[&str] = &[
        "Based on your past activity",
        "Based on your device",
        "Use precise location",
    ];
    // The place name sits immediately before the boilerplate. Keeping the
    // scan close to it stops earlier page text from being read as a city.
    const WINDOW: usize = 4096;

    for marker in MARKERS {
        let Some(marker_start) = find_visible_phrase(html, marker) else {
            continue;
        };
        let window_start = floor_char_boundary(html, marker_start.saturating_sub(WINDOW));
        // Start after a tag: a window cut through `<div class="…` would
        // otherwise leak the partial tag into the extracted text.
        let window_start = html[window_start..marker_start]
            .find('>')
            .map_or(window_start, |offset| window_start + offset + 1);
        if let Some(location) = last_place_text(&html[window_start..marker_start]) {
            return Some(location);
        }
    }
    None
}

/// Returns the last place-like text in the fragment before a footer marker.
fn last_place_text(fragment: &str) -> Option<String> {
    // The location can be a bare text node directly before the marker.
    if let Some(tag_end) = fragment.rfind('>') {
        if let Some(text) = clean_place_text(&html_text(&fragment[tag_end + 1..])) {
            return Some(text);
        }
    }
    // Separator elements (` · `) often sit between the location and the
    // marker, so the location is usually an earlier text node.
    if let Some(text) = text_nodes(fragment)
        .into_iter()
        .rev()
        .find_map(|text| clean_place_text(&text))
    {
        return Some(text);
    }
    // The location itself may be split across markup (`<b>St. John's</b>,
    // Antigua and Barbuda`), so flatten the last fully closed elements too.
    element_texts(fragment, |_| true)
        .into_iter()
        .rev()
        .find_map(|text| clean_place_text(&text))
}

fn clean_place_text(text: &str) -> Option<String> {
    let text = text
        .trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, ',' | '.' | '-' | '|' | '/' | '·')
        })
        .trim();
    is_location_text(text).then(|| text.to_string())
}

/// Finds the last occurrence of `phrase` in visible page text, skipping
/// `<script>`/`<style>` bodies whose template strings repeat UI copy.
fn find_visible_phrase(html: &str, phrase: &str) -> Option<usize> {
    let haystack = html.to_ascii_lowercase();
    let needle = phrase.to_ascii_lowercase();
    let mut cursor = 0;
    let mut found = None;
    while cursor < haystack.len() {
        let text_end = haystack[cursor..]
            .find('<')
            .map_or(haystack.len(), |offset| cursor + offset);
        if let Some(offset) = haystack[cursor..text_end].rfind(&needle) {
            found = Some(cursor + offset);
        }
        if text_end == haystack.len() {
            break;
        }
        let Some(next) = skip_tag(&haystack, text_end) else {
            break;
        };
        cursor = next;
    }
    found
}

/// Collects the text of visible text nodes in document order.
fn text_nodes(html: &str) -> Vec<String> {
    let haystack = html.to_ascii_lowercase();
    let mut texts = Vec::new();
    let mut cursor = 0;
    while cursor < haystack.len() {
        let text_end = haystack[cursor..]
            .find('<')
            .map_or(haystack.len(), |offset| cursor + offset);
        let text = html_text(&html[cursor..text_end]);
        if !text.is_empty() {
            texts.push(text);
        }
        if text_end == haystack.len() {
            break;
        }
        let Some(next) = skip_tag(&haystack, text_end) else {
            break;
        };
        cursor = next;
    }
    texts
}

/// Moves past the tag at `tag_start`, skipping `<script>`/`<style>` bodies.
fn skip_tag(haystack: &str, tag_start: usize) -> Option<usize> {
    let tag_end = find_tag_end(haystack, tag_start)?;
    if haystack[tag_start..].starts_with("</") || haystack[tag_start..].starts_with("<!") {
        return Some(tag_end + 1);
    }
    let tag_name = haystack[tag_start + 1..tag_end]
        .split(|character: char| character.is_whitespace() || character == '/' || character == '>')
        .next()
        .unwrap_or("");
    Some(if tag_name == "script" || tag_name == "style" {
        matching_close(haystack, tag_end + 1, tag_name)
            .map(|close| close + 1)
            .unwrap_or(tag_end + 1)
    } else {
        tag_end + 1
    })
}

/// Clamps `index` to the closest character boundary at or before it.
fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// Reads the two-letter `gl=` region Google puts in its own links.
fn country_code_from_gl(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut cursor = 0;
    while let Some(relative) = lower[cursor..].find("gl=") {
        let index = cursor + relative;
        cursor = index + 3;
        if index > 0 && bytes[index - 1].is_ascii_alphanumeric() {
            continue;
        }
        let (Some(first), Some(second)) = (bytes.get(index + 3), bytes.get(index + 4)) else {
            break;
        };
        if !first.is_ascii_alphabetic() || !second.is_ascii_alphabetic() {
            continue;
        }
        if bytes
            .get(index + 5)
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        {
            continue;
        }
        let code = lower[index + 3..index + 5].to_ascii_uppercase();
        if country_names::is_country_code(&code) {
            *counts.entry(code).or_default() += 1;
        }
    }
    let mut entries = counts.into_iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    entries.first().map(|(code, _)| code.clone())
}

/// Reads the country Google assigned to the visitor from YouTube's
/// `INNERTUBE_CONTEXT`.
///
/// The same GeoIP lookup feeds the search answer card; YouTube is used because
/// it renders that context into the HTML instead of behind JavaScript.
async fn youtube_google_country(client: &Client) -> Result<String, String> {
    let response = client
        .get(GOOGLE_GEO_URL)
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .header(reqwest::header::COOKIE, GOOGLE_CONSENT_COOKIE)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("YouTube returned HTTP {status}"));
    }
    let body = response.text().await.map_err(|error| error.to_string())?;
    parse_youtube_google_country(&body)
        .ok_or_else(|| "YouTube did not report a country".to_string())
}

/// Picks Google's country out of the YouTube page, preferring `countryCode`
/// and falling back to `gl`.
fn parse_youtube_google_country(html: &str) -> Option<String> {
    for key in ["countryCode", "gl"] {
        let Some(value) = json_string_field(html, key) else {
            continue;
        };
        if value.eq_ignore_ascii_case("uk") {
            return Some("GB".to_string());
        }
        if country_names::is_country_code(&value) {
            return Some(value.to_ascii_uppercase());
        }
    }
    None
}

/// Returns the string value of `"key"` in a JSON document.
fn json_string_field(document: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":\"");
    let start = document.find(&pattern)? + pattern.len();
    let value = &document[start..];
    let end = value.find('"')?;
    Some(value[..end].to_string())
}

/// Rejects answer-card lines that are dates, times or help text rather than a
/// place name.
fn is_location_text(text: &str) -> bool {
    let length = text.chars().count();
    if !(2..=64).contains(&length) {
        return false;
    }
    if !text.chars().any(|character| character.is_alphabetic()) {
        return false;
    }
    if text.chars().any(|character| character.is_ascii_digit()) {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    ![
        "based on",
        "precise",
        "your location",
        "learn more",
        // Time-zone suffixes mark the local time card, not a place name.
        "gmt+",
        "gmt-",
        "utc+",
        "utc-",
    ]
    .iter()
    .any(|phrase| lower.contains(phrase))
}

/// Collects the text of every element whose tag matches `is_target`.
///
/// Script and style bodies are skipped: Google's JavaScript bundles contain
/// template strings with the same class names as the answer card, and parsing
/// those would invent a location.
fn element_texts<F>(html: &str, is_target: F) -> Vec<String>
where
    F: Fn(&str) -> bool,
{
    let lower = html.to_ascii_lowercase();
    let mut texts = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = lower[cursor..].find('<') {
        let start = cursor + relative;
        if lower[start..].starts_with("</") || lower[start..].starts_with("<!") {
            cursor = start + 2;
            continue;
        }
        let Some(tag_end) = find_tag_end(&lower, start) else {
            break;
        };
        let tag_content = &lower[start + 1..tag_end];
        let tag_name = tag_content
            .split(|character: char| {
                character.is_whitespace() || character == '/' || character == '>'
            })
            .next()
            .unwrap_or("");
        if tag_name == "script" || tag_name == "style" {
            cursor = matching_close(&lower, tag_end + 1, tag_name)
                .map(|close| close + 1)
                .unwrap_or(tag_end + 1);
            continue;
        }
        if !tag_name.is_empty() && is_target(tag_content) {
            if let Some(close) = matching_close(&lower, tag_end + 1, tag_name) {
                let text = html_text(&html[tag_end + 1..close]);
                if !text.is_empty() {
                    texts.push(text);
                }
                cursor = close + 1;
                continue;
            }
        }
        cursor = tag_end + 1;
    }
    texts
}

/// Returns the position of the closing tag that matches an element whose
/// content starts at `from`.
fn matching_close(haystack: &str, from: usize, tag_name: &str) -> Option<usize> {
    let open_prefix = format!("<{tag_name}");
    let close_prefix = format!("</{tag_name}");
    let mut depth = 1_usize;
    let mut cursor = from;
    loop {
        let next_open = find_tag_prefix(haystack, &open_prefix, cursor);
        let next_close = find_tag_prefix(haystack, &close_prefix, cursor);
        match (next_open, next_close) {
            (None, None) => return None,
            (None, Some(close)) => {
                depth -= 1;
                if depth == 0 {
                    return Some(close);
                }
                cursor = close + close_prefix.len();
            }
            (Some(open), Some(close)) if close < open => {
                depth -= 1;
                if depth == 0 {
                    return Some(close);
                }
                cursor = close + close_prefix.len();
            }
            (Some(open), _) => {
                depth += 1;
                cursor = find_tag_end(haystack, open).map(|end| end + 1)?;
            }
        }
    }
}

fn find_tag_prefix(haystack: &str, prefix: &str, from: usize) -> Option<usize> {
    let mut cursor = from;
    while let Some(relative) = haystack[cursor..].find(prefix) {
        let index = cursor + relative;
        let boundary = haystack[index + prefix.len()..]
            .chars()
            .next()
            .is_none_or(|character| {
                character.is_whitespace() || character == '>' || character == '/'
            });
        if boundary {
            return Some(index);
        }
        cursor = index + prefix.len();
    }
    None
}

/// Finds the `>` that ends a tag, ignoring `>` inside quoted attributes.
fn find_tag_end(html: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    for (offset, byte) in html.as_bytes()[start..].iter().enumerate() {
        match (quote, *byte) {
            (None, b'"' | b'\'') => quote = Some(*byte),
            (Some(quote_byte), byte) if byte == quote_byte => quote = None,
            (None, b'>') => return Some(start + offset),
            _ => {}
        }
    }
    None
}

fn tag_has_class(tag_content: &str, class: &str) -> bool {
    tag_attribute(tag_content, "class").is_some_and(|value| {
        value
            .split_ascii_whitespace()
            .any(|token| token.eq_ignore_ascii_case(class))
    })
}

/// Reads one attribute value from the text between `<` and `>`.
fn tag_attribute<'a>(tag_content: &'a str, name: &str) -> Option<&'a str> {
    let bytes = tag_content.as_bytes();
    let mut index = 0;
    while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    while index < bytes.len() {
        while index < bytes.len() && (bytes[index].is_ascii_whitespace() || bytes[index] == b'/') {
            index += 1;
        }
        let name_start = index;
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && bytes[index] != b'='
            && bytes[index] != b'>'
        {
            index += 1;
        }
        if name_start == index {
            index += 1;
            continue;
        }
        let attribute_name = &tag_content[name_start..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            continue;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            return None;
        }
        let quote = bytes[index];
        let (value_start, value_end) = if quote == b'"' || quote == b'\'' {
            index += 1;
            let value_start = index;
            while index < bytes.len() && bytes[index] != quote {
                index += 1;
            }
            let value_end = index;
            if index < bytes.len() {
                index += 1;
            }
            (value_start, value_end)
        } else {
            let value_start = index;
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() && bytes[index] != b'>'
            {
                index += 1;
            }
            (value_start, index)
        };
        if attribute_name.eq_ignore_ascii_case(name) {
            return Some(&tag_content[value_start..value_end]);
        }
    }
    None
}

/// Flattens a small HTML fragment into plain text.
fn html_text(fragment: &str) -> String {
    let mut text = String::new();
    let mut cursor = 0;
    while let Some(relative) = fragment[cursor..].find('<') {
        let start = cursor + relative;
        text.push_str(&fragment[cursor..start]);
        match find_tag_end(fragment, start) {
            Some(end) => {
                text.push(' ');
                cursor = end + 1;
            }
            None => {
                cursor = fragment.len();
                break;
            }
        }
    }
    text.push_str(&fragment[cursor..]);
    decode_html_entities(&text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_html_entities(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find('&') {
        result.push_str(&rest[..index]);
        let after = &rest[index..];
        let entity_end = after.find(';').filter(|end| *end <= 10);
        let Some(entity_end) = entity_end else {
            result.push('&');
            rest = &rest[index + 1..];
            continue;
        };
        if let Some(decoded) = decode_entity(&after[1..entity_end]) {
            result.push(decoded);
            rest = &after[entity_end + 1..];
        } else {
            result.push('&');
            rest = &rest[index + 1..];
        }
    }
    result.push_str(rest);
    result
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        "ndash" => Some('–'),
        "mdash" => Some('—'),
        "hellip" => Some('…'),
        "middot" => Some('·'),
        "rsquo" => Some('’'),
        "lsquo" => Some('‘'),
        "ldquo" => Some('“'),
        "rdquo" => Some('”'),
        "deg" => Some('°'),
        _ => numeric_entity(entity),
    }
}

fn numeric_entity(entity: &str) -> Option<char> {
    let value = if let Some(hex) = entity
        .strip_prefix("#x")
        .or_else(|| entity.strip_prefix("#X"))
    {
        u32::from_str_radix(hex, 16).ok()?
    } else if let Some(decimal) = entity.strip_prefix('#') {
        decimal.parse::<u32>().ok()?
    } else {
        return None;
    };
    char::from_u32(value)
}

async fn exit_from_ipwho(client: &Client) -> Result<GoogleExitResult, String> {
    let value = client
        .get(EXIT_INFO_URL)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json::<Value>()
        .await
        .map_err(|error| format!("The exit geo response could not be parsed: {error}"))?;

    if value.get("success").and_then(Value::as_bool) == Some(false) {
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(format!(
            "The exit geo service rejected the lookup: {message}"
        ));
    }

    let ip = text_field(&value, "ip")
        .ok_or_else(|| "The exit geo response did not include an IP.".to_string())?;
    Ok(GoogleExitResult {
        ip,
        country_code: text_field(&value, "country_code"),
        country: text_field(&value, "country"),
        region: place_from_ipwho(&value),
    })
}

/// Joins the city and region the way Google's answer card reads
/// (`Los Angeles, California`) instead of losing one of the two.
fn place_from_ipwho(value: &Value) -> Option<String> {
    match (text_field(value, "city"), text_field(value, "region")) {
        (Some(city), Some(region)) if !city.eq_ignore_ascii_case(&region) => {
            Some(format!("{city}, {region}"))
        }
        (Some(city), _) => Some(city),
        (None, region) => region,
    }
}

/// Fallback that needs no JSON parsing: Cloudflare reports `ip=` and a
/// two-letter `loc=` for the connecting client.
pub(crate) async fn exit_from_cloudflare(client: &Client) -> Result<GoogleExitResult, String> {
    let body = client
        .get(EXIT_TRACE_URL)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .text()
        .await
        .map_err(|error| error.to_string())?;

    let mut ip = None;
    let mut country_code = None;
    for line in body.lines() {
        if let Some(value) = line.strip_prefix("ip=") {
            ip = Some(value.trim().to_string()).filter(|value| !value.is_empty());
        } else if let Some(value) = line.strip_prefix("loc=") {
            let value = value.trim().to_ascii_uppercase();
            if !value.is_empty() && value != "XX" {
                country_code = Some(value);
            }
        }
    }

    Ok(GoogleExitResult {
        ip: ip.ok_or_else(|| "The exit trace did not include an IP.".to_string())?,
        country_code,
        country: None,
        region: None,
    })
}

pub(crate) fn text_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn proxied_client(socks_port: u16, timeout: Duration) -> Result<Client, String> {
    proxied_client_with(format!("socks5h://127.0.0.1:{socks_port}"), timeout)
}

fn proxied_client_with(proxy_url: String, timeout: Duration) -> Result<Client, String> {
    let proxy = reqwest::Proxy::all(proxy_url).map_err(|error| error.to_string())?;
    Client::builder()
        .user_agent(BROWSER_USER_AGENT)
        .timeout(timeout)
        .proxy(proxy)
        .build()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn latency_summary_averages_samples_and_counts_lost_probes() {
        assert_eq!(summarize_samples(&[100, 200, 300, 400], 4), Some((250, 0)));
        assert_eq!(summarize_samples(&[100, 200, 300], 4), Some((200, 25)));
        assert_eq!(summarize_samples(&[120], 4), Some((120, 75)));
        assert_eq!(summarize_samples(&[], 4), None);
        assert_eq!(summarize_samples(&[100], 0), None);
    }

    #[test]
    fn joins_city_and_region_without_duplicating_them() {
        let with_both = json!({ "city": "Los Angeles", "region": "California" });
        assert_eq!(
            place_from_ipwho(&with_both).as_deref(),
            Some("Los Angeles, California")
        );

        let same = json!({ "city": "Singapore", "region": "Singapore" });
        assert_eq!(place_from_ipwho(&same).as_deref(), Some("Singapore"));

        let city_only = json!({ "city": "Tokyo", "region": "" });
        assert_eq!(place_from_ipwho(&city_only).as_deref(), Some("Tokyo"));
    }

    #[test]
    fn blank_geo_fields_are_treated_as_missing() {
        let value = json!({ "ip": "203.0.113.7", "country": "  ", "region": "Tokyo" });

        assert_eq!(text_field(&value, "ip").as_deref(), Some("203.0.113.7"));
        assert_eq!(text_field(&value, "country"), None);
        assert_eq!(text_field(&value, "region").as_deref(), Some("Tokyo"));
    }

    #[test]
    fn reads_a_two_line_location_card() {
        let html = r#"
            <div class="wDYxhc" data-attrid="kc:/local:location">
                <div class="Z0LcW">Tokyo</div>
                <div class="iAIpCb">Japan</div>
            </div>
        "#;

        let location = parse_google_search_location(html).unwrap();
        assert_eq!(location.country_code.as_deref(), Some("JP"));
        assert_eq!(location.country.as_deref(), Some("Japan"));
        assert_eq!(location.region.as_deref(), Some("Tokyo"));
    }

    #[test]
    fn reads_a_single_line_location_card_with_markup_and_entities() {
        let html = r#"
            <div class="LrzXr">
                <b>St.&nbsp;John&#39;s</b>,&nbsp;Antigua and Barbuda
            </div>
        "#;

        let location = parse_google_search_location(html).unwrap();
        assert_eq!(location.country_code.as_deref(), Some("AG"));
        assert_eq!(location.region.as_deref(), Some("St. John's"));
    }

    #[test]
    fn reads_the_location_from_a_data_attribute_card() {
        let html = r#"
            <div data-attrid="address">
                <span>Frankfurt am Main</span>, Germany
            </div>
        "#;

        let location = parse_google_search_location(html).unwrap();
        assert_eq!(location.country_code.as_deref(), Some("DE"));
        assert_eq!(location.region.as_deref(), Some("Frankfurt am Main"));
    }

    #[test]
    fn reads_the_city_from_googles_search_footer() {
        let html = r#"
            <div class="card">
                <div class="Z0LcW">Hong Kong</div>
            </div>
            <div id="foot">
                <div class="KjOnKe">
                    <div class="x"><span>Shenzhen, Guangdong, China</span></div>
                    <div class="y">
                        <span>Based on your past activity</span>
                        <span> – </span>
                        <a href="/prefs">Use precise location</a>
                    </div>
                </div>
            </div>
        "#;

        let location = parse_google_search_location(html).unwrap();
        assert_eq!(location.country_code.as_deref(), Some("HK"));
        // The footer wins over the card and is kept even though the card put
        // the exit in Hong Kong. Country names are stripped from the region.
        assert_eq!(location.region.as_deref(), Some("Shenzhen, Guangdong"));
        assert!(location.region_from_footer);
    }

    #[test]
    fn reads_a_bare_footer_location_before_a_separator() {
        let html = r#"
            <div class="foot">
                Shenzhen<span> · </span>
                <span>Based on your device's location</span>
                <a href="/prefs">Use precise location</a>
            </div>
        "#;

        let location = parse_google_search_location(html).unwrap();
        assert_eq!(location.region.as_deref(), Some("Shenzhen"));
        assert!(location.region_from_footer);
    }

    #[test]
    fn ignores_footer_copy_inside_scripts() {
        let html = r#"
            <script>
                var footer = "Shenzhen · Based on your past activity · Use precise location";
            </script>
        "#;

        assert_eq!(parse_google_search_location(html), None);
    }

    #[test]
    fn keeps_the_footer_city_when_the_localized_country_disagrees() {
        let footer = GoogleLocation {
            region: Some("Shenzhen, Guangdong".to_string()),
            region_from_footer: true,
            ..Default::default()
        };

        assert_eq!(
            region_for_country(&footer, Some("HK")).as_deref(),
            Some("Shenzhen, Guangdong")
        );
    }

    #[test]
    fn drops_a_card_city_whose_country_disagrees() {
        let card = GoogleLocation {
            country_code: Some("CN".to_string()),
            region: Some("Shenzhen".to_string()),
            ..Default::default()
        };

        assert_eq!(region_for_country(&card, Some("HK")), None);
    }

    #[test]
    fn ignores_answer_classes_inside_scripts() {
        let html = r#"
            <script>var card = '<div class="Z0LcW">Fake, Atlantis</div>';</script>
        "#;

        assert_eq!(parse_google_search_location(html), None);
    }

    #[test]
    fn ignores_time_and_advice_lines() {
        let html = r#"
            <div class="Z0LcW">5:48 PM</div>
            <div class="iAIpCb">Based on your past activity</div>
        "#;

        assert_eq!(parse_google_search_location(html), None);
    }

    #[test]
    fn falls_back_to_googles_region_parameter() {
        let html = r#"
            <a href="/search?q=test&amp;gl=jp&amp;hl=en">next</a>
            <a href="/search?q=test&amp;gl=jp&amp;hl=en">2</a>
            <a href="/preferences?hl=en&amp;gl=us">settings</a>
        "#;

        let location = parse_google_search_location(html).unwrap();
        assert_eq!(location.country_code.as_deref(), Some("JP"));
        assert_eq!(location.country.as_deref(), Some("Japan"));
    }

    #[test]
    fn reads_googles_country_from_youtube_innertube_context() {
        let html = r#"
            <script>var ytcfg = {"INNERTUBE_CONTEXT":{"client":{
                "hl":"en","gl":"gb","remoteHost":"203.0.113.7",
                "countryCode":"GB","clientName":"WEB"}}};</script>
        "#;

        assert_eq!(parse_youtube_google_country(html).as_deref(), Some("GB"));
    }

    #[test]
    fn prefers_youtube_country_code_over_gl() {
        let html = r#"{"client":{"gl":"US","countryCode":"JP"}}"#;

        assert_eq!(parse_youtube_google_country(html).as_deref(), Some("JP"));
    }

    #[test]
    fn ignores_youtube_gl_values_that_are_not_countries() {
        let html = r#"{"client":{"gl":"en"}}"#;

        assert_eq!(parse_youtube_google_country(html), None);
    }

    #[test]
    fn reads_the_country_from_googles_localized_host() {
        assert_eq!(
            country_from_google_host("www.google.com.hk").as_deref(),
            Some("HK")
        );
        assert_eq!(
            country_from_google_host("google.com.hk").as_deref(),
            Some("HK")
        );
        assert_eq!(
            country_from_google_host("www.google.co.uk").as_deref(),
            Some("GB")
        );
        assert_eq!(
            country_from_google_host("www.google.co.jp").as_deref(),
            Some("JP")
        );
        assert_eq!(
            country_from_google_host("www.google.de").as_deref(),
            Some("DE")
        );
    }

    #[test]
    fn ignores_unlocalized_and_non_google_hosts() {
        assert_eq!(country_from_google_host("www.google.com"), None);
        assert_eq!(country_from_google_host("consent.google.com"), None);
        assert_eq!(country_from_google_host("www.google.example"), None);
        assert_eq!(country_from_google_host(""), None);
    }
}
