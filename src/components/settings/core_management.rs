use std::sync::Arc;

use dioxus::prelude::*;

use crate::{
    i18n::I18n,
    services::{
        core_manager::{
            available_releases, core_storage_directory, install_release, remove_install, Channel,
            Engine,
        },
        database::{database_path, CoreInstall, Database},
        geo_assets::{
            engine_label, geo_asset_directory, geo_asset_status, update_geo_assets, GeoAssetStatus,
        },
        proxy_core::ProxyCoreService,
    },
};

#[component]
pub(super) fn CoreManagement() -> Element {
    let database = use_context::<Arc<Database>>();
    let mut data_revision = use_context::<Signal<u64>>();
    let i18n = use_context::<I18n>();
    let _revision = data_revision();
    let mut engine = use_signal(|| "xray".to_string());
    let mut channel = use_signal(|| "main".to_string());
    let mut selected_version = use_signal(String::new);
    let initial_releases = database
        .cached_core_releases("xray", "main")
        .unwrap_or_default();
    let mut releases = use_signal(|| initial_releases);
    let mut release_busy = use_signal(|| false);
    let mut status = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut geo_busy = use_signal(|| Option::<Engine>::None);
    let mut geo_status = use_signal(String::new);
    let installed = database.core_installs().unwrap_or_default();
    let xray_geo_status = geo_asset_status(Engine::Xray);
    let sing_box_geo_status = geo_asset_status(Engine::SingBox);

    let release_list = releases();
    let selected_release = release_list
        .iter()
        .find(|release| release.version == selected_version())
        .or_else(|| release_list.first())
        .cloned();
    let has_selected_release = selected_release.is_some();
    let release_for_download = selected_release.clone();
    let download_database = Arc::clone(&database);
    let refresh_database = Arc::clone(&database);
    let engine_cache_database = Arc::clone(&database);
    let channel_cache_database = Arc::clone(&database);
    let on_download = move |_| {
        if busy() {
            return;
        }
        let Some(release) = release_for_download.clone() else {
            status.set(
                i18n.t("Select an available release version first.")
                    .to_string(),
            );
            return;
        };
        let selected_engine = if engine() == "xray" {
            Engine::Xray
        } else {
            Engine::SingBox
        };
        let selected_channel = if channel() == "preview" {
            Channel::Preview
        } else {
            Channel::Main
        };
        busy.set(true);
        status.set(format!("{} {}...", i18n.t("Downloading"), release.version));
        let database = Arc::clone(&download_database);
        spawn(async move {
            match install_release(&database, selected_engine, selected_channel, &release).await {
                Ok(install) => {
                    status.set(format!(
                        "{} {} {} ({})",
                        i18n.t("Installed"),
                        install.engine,
                        install.channel,
                        install.version
                    ));
                    data_revision.with_mut(|revision| *revision += 1);
                }
                Err(error) => status.set(format!("Download failed: {error}")),
            }
            busy.set(false);
        });
    };
    let on_refresh_releases = move |_| {
        if release_busy() {
            return;
        }
        release_busy.set(true);
        status.set(i18n.t("Refreshing available versions...").to_string());
        let database = Arc::clone(&refresh_database);
        let engine_id = engine();
        let channel_id = channel();
        let selected_engine = if engine_id == "xray" {
            Engine::Xray
        } else {
            Engine::SingBox
        };
        let selected_channel = if channel_id == "preview" {
            Channel::Preview
        } else {
            Channel::Main
        };
        spawn(async move {
            match available_releases(selected_engine, selected_channel).await {
                Ok(found) => {
                    match database.replace_core_release_cache(&engine_id, &channel_id, &found) {
                        Ok(()) => {
                            selected_version.set(String::new());
                            releases.set(found.clone());
                            status.set(format!(
                                "{} {} {}",
                                i18n.t("Loaded"),
                                found.len(),
                                i18n.t("available versions.")
                            ));
                        }
                        Err(error) => {
                            status.set(format!("Could not save release versions: {error}"))
                        }
                    }
                }
                Err(error) => status.set(format!("Could not load releases: {error}")),
            }
            release_busy.set(false);
        });
    };
    let update_geo = EventHandler::new(move |target: Engine| {
        if geo_busy().is_some() {
            return;
        }
        geo_busy.set(Some(target));
        geo_status.set(format!(
            "{} {} {}",
            i18n.t("Updating"),
            engine_label(target),
            i18n.t("geodata...")
        ));
        spawn(async move {
            match update_geo_assets(target).await {
                Ok(()) => {
                    geo_status.set(format!(
                        "{} {}",
                        engine_label(target),
                        i18n.t("geodata updated.")
                    ));
                    data_revision.with_mut(|revision| *revision += 1);
                }
                Err(error) => geo_status.set(format!("Geodata update failed: {error}")),
            }
            geo_busy.set(None);
        });
    });

    let use_core_hint = i18n.t(
        "\"Use this core\" switches the active engine; the next started subscription runs on it. Deleting an installation removes its binary from disk but keeps your subscriptions and settings.",
    );

    rsx! {
        section { class: "card settings-card core-management-card",
            h3 {
                i { class: "fa-solid fa-microchip" }
                "{i18n.t(\"Core Management\")}"
            }
            p { class: "settings-hint",
                i { class: "fa-solid fa-circle-info" }
                "{i18n.t(\"Cloak DX runs on an external proxy engine. Download Xray-core or sing-box here, choose which installed build to use, and keep the geodata its routing rules depend on up to date. The selected core is what the sidebar shows and what every subscription starts.\")}"
            }
            div { class: "setting-divider", "{i18n.t(\"Download Core\")}" }
            p { class: "settings-hint",
                i { class: "fa-solid fa-circle-info" }
                "{i18n.t(\"Pick a core, release channel, and version, then download it. Versions are listed from the official release pages for this system's architecture; refresh the list after switching core or channel.\")}"
            }
            div { class: "setting-item storage-path-item",
                div { class: "setting-info",
                    label { "{i18n.t(\"Proxy core storage\")}" }
                    span { "{i18n.t(\"Downloaded core binaries\")}" }
                }
                code { class: "storage-path", "{core_storage_directory().display()}" }
            }
            div { class: "setting-item storage-path-item",
                div { class: "setting-info",
                    label { "{i18n.t(\"SQLite database\")}" }
                    span { "{i18n.t(\"Subscriptions and application data\")}" }
                }
                code { class: "storage-path", "{database_path().display()}" }
            }
            div { class: "setting-item",
                div { class: "setting-info",
                    label { "{i18n.t(\"Core\")}" }
                    span { "{i18n.t(\"Select which core release to install\")}" }
                }
                select {
                    class: "setting-select",
                    value: "{engine}",
                    onchange: move |event| {
                        let value = event.value();
                        engine.set(value.clone());
                        selected_version.set(String::new());
                        let engine_id = if value == "xray" { "xray" } else { "sing-box" };
                        let channel_id = if channel() == "preview" { "preview" } else { "main" };
                        releases
                            .set(
                                engine_cache_database
                                    .cached_core_releases(engine_id, channel_id)
                                    .unwrap_or_default(),
                            );
                    },
                    option { value: "xray", "Xray-core" }
                    option { value: "sing-box", "sing-box" }
                }
            }
            div { class: "setting-item",
                div { class: "setting-info",
                    label { "{i18n.t(\"Release channel\")}" }
                    span { "{i18n.t(\"Main is stable; preview tracks prereleases\")}" }
                }
                select {
                    class: "setting-select",
                    value: "{channel}",
                    onchange: move |event| {
                        let value = event.value();
                        channel.set(value.clone());
                        selected_version.set(String::new());
                        let engine_id = if engine() == "xray" { "xray" } else { "sing-box" };
                        let channel_id = if value == "preview" { "preview" } else { "main" };
                        releases
                            .set(
                                channel_cache_database
                                    .cached_core_releases(engine_id, channel_id)
                                    .unwrap_or_default(),
                            );
                    },
                    option { value: "main", "{i18n.t(\"Main (stable)\")}" }
                    option { value: "preview", "Preview" }
                }
            }
            div { class: "setting-item",
                div { class: "setting-info",
                    label { "{i18n.t(\"Available version\")}" }
                    span { "{i18n.t(\"Releases matching this core, channel, and system architecture\")}" }
                }
                button {
                    class: "btn-icon",
                    title: i18n.t("Refresh available versions"),
                    "aria-label": i18n.t("Refresh available versions"),
                    disabled: release_busy(),
                    onclick: on_refresh_releases,
                    if release_busy() {
                        i { class: "fa-solid fa-rotate fa-spin" }
                    } else {
                        i { class: "fa-solid fa-rotate" }
                    }
                }
                if !release_list.is_empty() {
                    select {
                        class: "setting-select",
                        value: if selected_version().is_empty() { release_list[0].version.clone() } else { selected_version() },
                        onchange: move |event| selected_version.set(event.value()),
                        for release in &release_list {
                            option { value: "{release.version}",
                                "{release.version} · {release.published_at}"
                            }
                        }
                    }
                } else if release_busy() {
                    span { class: "core-status-message", "{i18n.t(\"Refreshing versions...\")}" }
                } else {
                    span { class: "core-status-message",
                        "{i18n.t(\"No cached versions. Click refresh to load releases.\")}"
                    }
                }
            }
            button {
                class: "btn-primary core-download-button",
                disabled: busy() || !has_selected_release,
                onclick: on_download,
                if busy() {
                    i { class: "fa-solid fa-spinner fa-spin" }
                    "{i18n.t(\"Downloading...\")}"
                } else {
                    i { class: "fa-solid fa-download" }
                    "{i18n.t(\"Download and Install\")}"
                }
            }
            if !status().is_empty() {
                p { class: "core-status-message", role: "status", "{status}" }
            }

            div { class: "setting-divider", "{i18n.t(\"Installed Cores\")}" }
            p { class: "settings-hint",
                i { class: "fa-solid fa-circle-info" }
                "{use_core_hint}"
            }
            if installed.is_empty() {
                div { class: "empty-state", "{i18n.t(\"No cores installed yet.\")}" }
            } else {
                div { class: "core-install-list",
                    for install in installed {
                        CoreInstallRow {
                            key: "{install.engine}-{install.channel}-{install.version}",
                            install,
                        }
                    }
                }
            }

            div { class: "setting-divider", "{i18n.t(\"Geo Assets\")}" }
            p { class: "settings-hint",
                i { class: "fa-solid fa-circle-info" }
                "{i18n.t(\"Bypass mainland China, ad blocking, and local DNS rules match traffic through these GeoIP/GeoSite files. Download or update them once per engine; a running core keeps the data it loaded at startup, so restart it to pick up new files.\")}"
            }
            div { class: "setting-item storage-path-item",
                div { class: "setting-info",
                    label { "{i18n.t(\"Asset directory\")}" }
                    span { "{i18n.t(\"All GeoIP and GeoSite data files share this folder\")}" }
                }
                code { class: "storage-path", "{geo_asset_directory().display()}" }
            }
            div { class: "core-install-list",
                GeoAssetRow {
                    engine: Engine::Xray,
                    title: i18n.t("Xray-core geodata").to_string(),
                    summary: geo_asset_summary(&xray_geo_status, i18n),
                    installed: xray_geo_status.iter().all(|status| status.size.is_some()),
                    busy: geo_busy() == Some(Engine::Xray),
                    on_update: update_geo,
                }
                GeoAssetRow {
                    engine: Engine::SingBox,
                    title: i18n.t("sing-box geodata").to_string(),
                    summary: geo_asset_summary(&sing_box_geo_status, i18n),
                    installed: sing_box_geo_status.iter().all(|status| status.size.is_some()),
                    busy: geo_busy() == Some(Engine::SingBox),
                    on_update: update_geo,
                }
            }
            if !geo_status().is_empty() {
                p { class: "core-status-message", role: "status", "{geo_status}" }
            }
        }
    }
}

#[component]
fn GeoAssetRow(
    engine: Engine,
    title: String,
    summary: String,
    installed: bool,
    busy: bool,
    on_update: EventHandler<Engine>,
) -> Element {
    let i18n = use_context::<I18n>();

    rsx! {
        div { class: "core-install-row",
            div { class: "core-install-info",
                strong { "{title}" }
                span { "{summary}" }
            }
            div { class: "core-install-actions",
                button {
                    class: "btn-secondary small",
                    title: i18n.t("Download the latest official geodata files"),
                    disabled: busy,
                    onclick: move |_| on_update.call(engine),
                    if busy {
                        i { class: "fa-solid fa-spinner fa-spin" }
                        if installed { "{i18n.t(\"Updating...\")}" } else { "{i18n.t(\"Downloading...\")}" }
                    } else {
                        i { class: "fa-solid fa-rotate" }
                        if installed { "{i18n.t(\"Update\")}" } else { "{i18n.t(\"Download\")}" }
                    }
                }
            }
        }
    }
}

fn geo_asset_summary(statuses: &[GeoAssetStatus], i18n: I18n) -> String {
    statuses
        .iter()
        .map(|status| match status.size {
            Some(size) => format!("{} ({})", status.file_name, format_bytes(size)),
            None => format!("{} ({})", status.file_name, i18n.t("not downloaded")),
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut amount = bytes as f64;
    let mut unit = 0;
    while amount >= 1024.0 && unit < UNITS.len() - 1 {
        amount /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{amount:.1} {}", UNITS[unit])
    }
}

#[component]
fn CoreInstallRow(install: CoreInstall) -> Element {
    let database = use_context::<Arc<Database>>();
    let proxy_core = use_context::<Arc<ProxyCoreService>>();
    let mut data_revision = use_context::<Signal<u64>>();
    let i18n = use_context::<I18n>();
    let database_for_click = Arc::clone(&database);
    let proxy_core_for_click = Arc::clone(&proxy_core);
    let database_for_delete = Arc::clone(&database);
    let install_for_delete = install.clone();
    let engine = install.engine.clone();
    let channel = install.channel.clone();
    let version = install.version.clone();
    let mut delete_busy = use_signal(|| false);
    let mut delete_error = use_signal(String::new);

    rsx! {
        div { class: "core-install-row",
            div { class: "core-install-info",
                strong { "{install.engine} · {install.version}" }
                span { "{install.channel} · {install.executable_path}" }
            }
            div { class: "core-install-actions",
                if install.selected {
                    span { class: "status-badge connected", "{i18n.t(\"Selected\")}" }
                } else {
                    button {
                        class: "btn-secondary small",
                        onclick: move |_| {
                            match proxy_core_for_click.select_core(
                                &database_for_click,
                                &engine,
                                &channel,
                                &version,
                            ) {
                                Ok(()) => {
                                    delete_error.set(String::new());
                                    data_revision.with_mut(|revision| *revision += 1);
                                }
                                Err(error) => delete_error.set(error),
                            }
                        },
                        "{i18n.t(\"Use this core\")}"
                    }
                }
                button {
                    class: "btn-icon",
                    title: i18n.t("Delete core"),
                    "aria-label": i18n.t("Delete core"),
                    disabled: delete_busy(),
                    onclick: move |_| {
                        if delete_busy() {
                            return;
                        }
                        delete_busy.set(true);
                        delete_error.set(String::new());
                        match remove_install(&database_for_delete, &install_for_delete) {
                            Ok(()) => data_revision.with_mut(|revision| *revision += 1),
                            Err(error) => delete_error.set(error),
                        }
                        delete_busy.set(false);
                    },
                    if delete_busy() {
                        i { class: "fa-solid fa-spinner fa-spin" }
                    } else {
                        i { class: "fa-solid fa-trash" }
                    }
                }
            }
            if !delete_error().is_empty() {
                p { class: "core-status-message error", role: "alert", "{delete_error}" }
            }
        }
    }
}
