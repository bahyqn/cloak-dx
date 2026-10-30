use std::{fs, path::PathBuf, time::Duration};

use reqwest::header::USER_AGENT;

use super::{core_manager::Engine, database::data_directory};

/// One downloadable geodata file used by a proxy core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeoAsset {
    pub file_name: &'static str,
    pub url: &'static str,
}

/// Xray-core reads the V2Fly geodata files. The domain list release asset is
/// named `dlc.dat` upstream but every client expects it as `geosite.dat`.
pub const XRAY_GEO_ASSETS: &[GeoAsset] = &[
    GeoAsset {
        file_name: "geosite.dat",
        url: "https://github.com/v2fly/domain-list-community/releases/latest/download/dlc.dat",
    },
    GeoAsset {
        file_name: "geoip.dat",
        url: "https://github.com/v2fly/geoip/releases/latest/download/geoip.dat",
    },
    GeoAsset {
        file_name: "geoip-only-cn-private.dat",
        url: "https://github.com/v2fly/geoip/releases/latest/download/geoip-only-cn-private.dat",
    },
];

/// sing-box consumes rule-set files, not the legacy `.db` geodata bundles.
/// The `rule-set` branch publishes one small binary file per category, which
/// is what `route.rule_set` can load locally.
pub const SING_BOX_GEO_ASSETS: &[GeoAsset] = &[
    GeoAsset {
        file_name: "geosite-cn.srs",
        url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs",
    },
    GeoAsset {
        file_name: "geoip-cn.srs",
        url: "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-cn.srs",
    },
    GeoAsset {
        file_name: "geosite-category-ads-all.srs",
        url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-ads-all.srs",
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeoAssetStatus {
    pub file_name: &'static str,
    /// `None` when the file has not been downloaded yet.
    pub size: Option<u64>,
}

pub fn geo_assets_for(engine: Engine) -> &'static [GeoAsset] {
    match engine {
        Engine::Xray => XRAY_GEO_ASSETS,
        Engine::SingBox => SING_BOX_GEO_ASSETS,
    }
}

pub fn engine_label(engine: Engine) -> &'static str {
    match engine {
        Engine::Xray => "Xray-core",
        Engine::SingBox => "sing-box",
    }
}

/// All geodata files live in one shared folder so the cores can be pointed at
/// a single asset directory.
pub fn geo_asset_directory() -> PathBuf {
    data_directory().join("geo")
}

pub fn geo_asset_status(engine: Engine) -> Vec<GeoAssetStatus> {
    let directory = geo_asset_directory();
    geo_assets_for(engine)
        .iter()
        .map(|asset| GeoAssetStatus {
            file_name: asset.file_name,
            size: fs::metadata(directory.join(asset.file_name))
                .ok()
                .map(|metadata| metadata.len()),
        })
        .collect()
}

/// Downloads the newest official geodata files for an engine, replacing the
/// previous copies atomically so a failed update never leaves a partial file.
pub async fn update_geo_assets(engine: Engine) -> Result<(), String> {
    let directory = geo_asset_directory();
    fs::create_dir_all(&directory).map_err(|error| {
        format!(
            "Could not create the geo asset directory {}: {error}",
            directory.display()
        )
    })?;

    let client = reqwest::Client::builder()
        .user_agent("cloak-dx")
        .connect_timeout(Duration::from_secs(30))
        // geoip.dat is around 23 MB and GitHub's asset host can be very slow;
        // only fail when the transfer stalls instead of after a fixed total.
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| error.to_string())?;

    for asset in geo_assets_for(engine) {
        let bytes = client
            .get(asset.url)
            .header(USER_AGENT, "cloak-dx")
            .send()
            .await
            .map_err(|error| format!("Could not download {}: {error}", asset.file_name))?
            .error_for_status()
            .map_err(|error| format!("Could not download {}: {error}", asset.file_name))?
            .bytes()
            .await
            .map_err(|error| format!("Could not download {}: {error}", asset.file_name))?;

        if bytes.len() < 1024 {
            return Err(format!(
                "The downloaded {} is unexpectedly small ({} bytes).",
                asset.file_name,
                bytes.len()
            ));
        }

        let destination = directory.join(asset.file_name);
        let temporary = directory.join(format!(".{}.part", asset.file_name));
        fs::write(&temporary, &bytes)
            .map_err(|error| format!("Could not write {}: {error}", temporary.display()))?;
        fs::rename(&temporary, &destination)
            .map_err(|error| format!("Could not replace {}: {error}", destination.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_engine_exposes_its_official_data_files() {
        assert_eq!(
            geo_assets_for(Engine::Xray)
                .iter()
                .map(|asset| asset.file_name)
                .collect::<Vec<_>>(),
            ["geosite.dat", "geoip.dat", "geoip-only-cn-private.dat"]
        );
        assert_eq!(
            geo_assets_for(Engine::SingBox)
                .iter()
                .map(|asset| asset.file_name)
                .collect::<Vec<_>>(),
            [
                "geosite-cn.srs",
                "geoip-cn.srs",
                "geosite-category-ads-all.srs"
            ]
        );
    }

    #[test]
    fn asset_names_do_not_collide_between_engines() {
        let mut names = XRAY_GEO_ASSETS
            .iter()
            .chain(SING_BOX_GEO_ASSETS)
            .map(|asset| asset.file_name)
            .collect::<Vec<_>>();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
    }

    #[test]
    fn every_asset_uses_an_official_https_download() {
        for asset in XRAY_GEO_ASSETS.iter().chain(SING_BOX_GEO_ASSETS) {
            assert!(
                asset.url.starts_with("https://"),
                "{} does not use HTTPS",
                asset.file_name
            );
        }
        for asset in XRAY_GEO_ASSETS {
            assert!(
                asset.url.contains("/releases/latest/download/"),
                "{} does not point at a latest release",
                asset.file_name
            );
        }
        for asset in SING_BOX_GEO_ASSETS {
            assert!(
                asset.url.contains("/SagerNet/sing-geo") && asset.url.contains("/rule-set/"),
                "{} does not point at an official rule set",
                asset.file_name
            );
        }
    }

    #[test]
    fn asset_directory_is_absolute_and_shared() {
        let directory = geo_asset_directory();
        assert!(directory.is_absolute());
        assert!(directory.ends_with("cloak-dx/geo"));
    }
}
