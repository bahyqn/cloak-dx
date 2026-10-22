use std::{fs, io::Cursor, path::PathBuf};

use flate2::read::GzDecoder;
use reqwest::header::{ACCEPT, USER_AGENT};
use serde_json::Value;
use tar::Archive;
use zip::ZipArchive;

pub use super::database::CoreRelease;
use super::database::{data_directory, CoreInstall, Database};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Engine {
    Xray,
    SingBox,
}

impl Engine {
    pub fn id(self) -> &'static str {
        match self {
            Self::Xray => "xray",
            Self::SingBox => "sing-box",
        }
    }

    pub fn repository(self) -> &'static str {
        match self {
            Self::Xray => "XTLS/Xray-core",
            Self::SingBox => "SagerNet/sing-box",
        }
    }

    fn binary_name(self) -> &'static str {
        match self {
            Self::Xray => "xray",
            Self::SingBox => "sing-box",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Channel {
    Main,
    Preview,
}

impl Channel {
    pub fn id(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Preview => "preview",
        }
    }
}

pub async fn available_releases(
    engine: Engine,
    channel: Channel,
) -> Result<Vec<CoreRelease>, String> {
    let client = reqwest::Client::builder()
        .user_agent("cloak-dx")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| error.to_string())?;
    let endpoint = format!(
        "https://api.github.com/repos/{}/releases?per_page=10",
        engine.repository()
    );
    let response = client
        .get(endpoint)
        .header(USER_AGENT, "cloak-dx")
        .header(ACCEPT, "application/vnd.github+json")
        .send()
        .await;
    let Ok(response) = response else {
        return available_releases_from_atom(&client, engine, channel).await;
    };
    if !response.status().is_success() {
        return available_releases_from_atom(&client, engine, channel).await;
    }
    let payload: Value = match response.json().await {
        Ok(payload) => payload,
        Err(_) => return available_releases_from_atom(&client, engine, channel).await,
    };
    let Some(releases) = payload.as_array() else {
        return Err("GitHub returned an invalid release list".to_string());
    };
    let mut available = Vec::new();
    for release in releases {
        let is_preview = release["prerelease"].as_bool().unwrap_or(false);
        if (channel == Channel::Preview) != is_preview || release["draft"].as_bool() == Some(true) {
            continue;
        }
        let Ok(asset) = find_asset(release, engine) else {
            continue;
        };
        let (Some(version), Some(asset_name), Some(download_url)) = (
            release["tag_name"].as_str(),
            asset["name"].as_str(),
            asset["browser_download_url"].as_str(),
        ) else {
            continue;
        };
        available.push(CoreRelease {
            version: version.to_string(),
            asset_name: asset_name.to_string(),
            download_url: download_url.to_string(),
            published_at: release["published_at"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        });
    }
    Ok(available)
}

async fn available_releases_from_atom(
    client: &reqwest::Client,
    engine: Engine,
    channel: Channel,
) -> Result<Vec<CoreRelease>, String> {
    let url = format!("https://github.com/{}/releases.atom", engine.repository());
    let xml = client
        .get(url)
        .header(USER_AGENT, "cloak-dx")
        .send()
        .await
        .map_err(|error| format!("Release API unavailable and Atom fallback failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Release API unavailable and Atom fallback failed: {error}"))?
        .text()
        .await
        .map_err(|error| error.to_string())?;
    parse_atom_releases(&xml, engine, channel)
}

fn parse_atom_releases(
    xml: &str,
    engine: Engine,
    channel: Channel,
) -> Result<Vec<CoreRelease>, String> {
    let document = roxmltree::Document::parse(xml).map_err(|error| error.to_string())?;
    let mut releases = Vec::new();
    for entry in document
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
    {
        let Some(title) = entry
            .children()
            .find(|node| node.has_tag_name("title"))
            .and_then(|node| node.text())
        else {
            continue;
        };
        let title = title.trim();
        if is_preview_tag(title) != (channel == Channel::Preview) {
            continue;
        }
        let published_at = entry
            .children()
            .find(|node| node.has_tag_name("updated"))
            .and_then(|node| node.text())
            .unwrap_or_default()
            .to_string();
        let (asset_name, download_url) = release_asset(engine, title);
        releases.push(CoreRelease {
            version: title.to_string(),
            asset_name,
            download_url,
            published_at,
        });
    }
    Ok(releases)
}

fn is_preview_tag(tag: &str) -> bool {
    let lower = tag.to_ascii_lowercase();
    ["alpha", "beta", "rc", "preview", "dev"]
        .iter()
        .any(|marker| lower.contains(marker))
}

fn release_asset(engine: Engine, version: &str) -> (String, String) {
    let tag = version.trim_start_matches('v');
    let release_tag = match engine {
        Engine::SingBox if !version.starts_with('v') => format!("v{version}"),
        _ => version.to_string(),
    };
    let name = match (engine, cfg!(target_arch = "aarch64")) {
        (Engine::Xray, true) => "Xray-linux-arm64-v8a.zip".to_string(),
        (Engine::Xray, false) => "Xray-linux-64.zip".to_string(),
        (Engine::SingBox, true) => format!("sing-box-{tag}-linux-arm64.tar.gz"),
        (Engine::SingBox, false) => format!("sing-box-{tag}-linux-amd64.tar.gz"),
    };
    let url = format!(
        "https://github.com/{}/releases/download/{release_tag}/{name}",
        engine.repository()
    );
    (name, url)
}

pub async fn install_release(
    database: &Database,
    engine: Engine,
    channel: Channel,
    release: &CoreRelease,
) -> Result<CoreInstall, String> {
    let client = reqwest::Client::builder()
        .user_agent("cloak-dx")
        .build()
        .map_err(|error| error.to_string())?;
    let archive_bytes = client
        .get(&release.download_url)
        .header(USER_AGENT, "cloak-dx")
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .bytes()
        .await
        .map_err(|error| error.to_string())?;

    let destination = core_directory(engine, channel, &release.version);
    fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    let executable_path = destination.join(engine.binary_name());
    extract_executable(
        engine,
        &release.asset_name,
        archive_bytes.as_ref(),
        &executable_path,
    )?;
    set_executable(&executable_path)?;

    let install = CoreInstall {
        engine: engine.id().to_owned(),
        channel: channel.id().to_owned(),
        version: release.version.clone(),
        executable_path: executable_path.to_string_lossy().into_owned(),
        selected: false,
    };
    database
        .record_core_install(&install)
        .map_err(|error| error.to_string())?;
    Ok(install)
}

fn find_asset(release: &Value, engine: Engine) -> Result<Value, String> {
    let architecture_matches = |name: &str| match (engine, cfg!(target_arch = "aarch64")) {
        (Engine::Xray, true) => name.contains("linux-arm64-v8a"),
        (Engine::Xray, false) => name.contains("linux-64"),
        (Engine::SingBox, true) => name.contains("linux-arm64"),
        (Engine::SingBox, false) => name.contains("linux-amd64"),
    };
    release["assets"]
        .as_array()
        .and_then(|assets| {
            assets.iter().find(|asset| {
                let name = asset["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                architecture_matches(&name)
                    && match engine {
                        Engine::Xray => name.starts_with("xray-linux-") && name.ends_with(".zip"),
                        Engine::SingBox => {
                            name.starts_with("sing-box-")
                                && name.ends_with(".tar.gz")
                                && !name.contains("-musl")
                                && !name.contains("-glibc")
                        }
                    }
            })
        })
        .cloned()
        .ok_or_else(|| {
            let arch = if cfg!(target_arch = "aarch64") {
                "arm64"
            } else {
                "x86_64"
            };
            format!("No Linux {arch} release archive found for {}", engine.id())
        })
}

fn extract_executable(
    engine: Engine,
    archive_name: &str,
    bytes: &[u8],
    output: &PathBuf,
) -> Result<(), String> {
    let target = engine.binary_name();
    if archive_name.ends_with(".zip") {
        let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|error| error.to_string())?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
            if PathBuf::from(entry.name())
                .file_name()
                .and_then(|name| name.to_str())
                == Some(target)
            {
                let mut file = fs::File::create(output).map_err(|error| error.to_string())?;
                std::io::copy(&mut entry, &mut file).map_err(|error| error.to_string())?;
                return Ok(());
            }
        }
    } else if archive_name.ends_with(".tar.gz") || archive_name.ends_with(".tgz") {
        let decoder = GzDecoder::new(Cursor::new(bytes));
        let mut archive = Archive::new(decoder);
        let entries = archive.entries().map_err(|error| error.to_string())?;
        for entry in entries {
            let mut entry = entry.map_err(|error| error.to_string())?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            if entry
                .path()
                .map_err(|error| error.to_string())?
                .file_name()
                .and_then(|name| name.to_str())
                == Some(target)
            {
                let mut file = fs::File::create(output).map_err(|error| error.to_string())?;
                std::io::copy(&mut entry, &mut file).map_err(|error| error.to_string())?;
                return Ok(());
            }
        }
    }
    Err(format!(
        "Executable {target} was not found in release archive"
    ))
}

fn set_executable(path: &PathBuf) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn core_directory(engine: Engine, channel: Channel, version: &str) -> PathBuf {
    data_directory()
        .join("cores")
        .join(engine.id())
        .join(channel.id())
        .join(version.replace(['/', '\\'], "_"))
}

pub fn core_storage_directory() -> PathBuf {
    data_directory().join("cores")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_current_linux_release_asset_names() {
        let xray_release = serde_json::json!({
            "assets": [
                {"name": "Xray-linux-64.zip", "browser_download_url": "https://example.com/xray.zip"},
                {"name": "Xray-linux-64.zip.dgst", "browser_download_url": "https://example.com/xray.dgst"}
            ]
        });
        assert_eq!(
            find_asset(&xray_release, Engine::Xray).unwrap()["name"],
            "Xray-linux-64.zip"
        );

        let sing_box_release = serde_json::json!({
            "assets": [
                {"name": "sing-box-1.14.3-linux-amd64-musl.tar.gz", "browser_download_url": "https://example.com/musl.tar.gz"},
                {"name": "sing-box-1.14.3-linux-amd64.tar.gz", "browser_download_url": "https://example.com/sing-box.tar.gz"}
            ]
        });
        assert_eq!(
            find_asset(&sing_box_release, Engine::SingBox).unwrap()["name"],
            "sing-box-1.14.3-linux-amd64.tar.gz"
        );
    }

    #[test]
    fn parses_atom_tags_into_stable_and_preview_releases() {
        let feed = r#"<feed xmlns="http://www.w3.org/2005/Atom">
            <entry><title>v1.14.3</title><updated>2026-01-01T00:00:00Z</updated></entry>
            <entry><title>v1.15.0-alpha.12</title><updated>2026-02-01T00:00:00Z</updated></entry>
        </feed>"#;
        let stable = parse_atom_releases(feed, Engine::SingBox, Channel::Main).unwrap();
        let preview = parse_atom_releases(feed, Engine::SingBox, Channel::Preview).unwrap();
        assert_eq!(stable[0].version, "v1.14.3");
        assert_eq!(stable[0].asset_name, "sing-box-1.14.3-linux-amd64.tar.gz");
        assert_eq!(
            stable[0].download_url,
            "https://github.com/SagerNet/sing-box/releases/download/v1.14.3/sing-box-1.14.3-linux-amd64.tar.gz"
        );
        assert_eq!(preview[0].version, "v1.15.0-alpha.12");
    }
}
