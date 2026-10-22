use std::sync::Arc;

use dioxus::prelude::*;

use crate::services::{
    core_manager::{available_releases, core_storage_directory, install_release, Channel, Engine},
    database::{database_path, CoreInstall, Database},
};

#[component]
pub(super) fn CoreManagement() -> Element {
    let database = use_context::<Arc<Database>>();
    let mut data_revision = use_context::<Signal<u64>>();
    let _revision = data_revision();
    let mut engine = use_signal(|| "sing-box".to_string());
    let mut channel = use_signal(|| "main".to_string());
    let mut selected_version = use_signal(String::new);
    let initial_releases = database
        .cached_core_releases("sing-box", "main")
        .unwrap_or_default();
    let mut releases = use_signal(|| initial_releases);
    let mut release_busy = use_signal(|| false);
    let mut status = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let installed = database.core_installs().unwrap_or_default();

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
            status.set("Select an available release version first.".to_string());
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
        status.set(format!("Downloading {}...", release.version));
        let database = Arc::clone(&download_database);
        spawn(async move {
            match install_release(&database, selected_engine, selected_channel, &release).await {
                Ok(install) => {
                    status.set(format!(
                        "Installed {} {} ({})",
                        install.engine, install.channel, install.version
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
        status.set("Refreshing available versions...".to_string());
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
                            status.set(format!("Loaded {} available versions.", found.len()));
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

    rsx! {
        section { class: "card settings-card core-management-card",
            h3 {
                i { class: "fa-solid fa-microchip" }
                "Core Management"
            }
            div { class: "setting-divider", "Download Core" }
            div { class: "setting-item storage-path-item",
                div { class: "setting-info",
                    label { "Proxy core storage" }
                    span { "Downloaded core binaries" }
                }
                code { class: "storage-path", "{core_storage_directory().display()}" }
            }
            div { class: "setting-item storage-path-item",
                div { class: "setting-info",
                    label { "SQLite database" }
                    span { "Subscriptions and application data" }
                }
                code { class: "storage-path", "{database_path().display()}" }
            }
            div { class: "setting-item",
                div { class: "setting-info",
                    label { "Core" }
                    span { "Select which core release to install" }
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
                    option { value: "sing-box", "sing-box" }
                    option { value: "xray", "Xray-core" }
                }
            }
            div { class: "setting-item",
                div { class: "setting-info",
                    label { "Release channel" }
                    span { "Main is stable; preview tracks prereleases" }
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
                    option { value: "main", "Main (stable)" }
                    option { value: "preview", "Preview" }
                }
            }
            div { class: "setting-item",
                div { class: "setting-info",
                    label { "Available version" }
                    span { "Releases matching this core, channel, and system architecture" }
                }
                button {
                    class: "btn-icon",
                    title: "Refresh available versions",
                    "aria-label": "Refresh available versions",
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
                    span { class: "core-status-message", "Refreshing versions..." }
                } else {
                    span { class: "core-status-message",
                        "No cached versions. Click refresh to load releases."
                    }
                }
            }
            button {
                class: "btn-primary core-download-button",
                disabled: busy() || !has_selected_release,
                onclick: on_download,
                if busy() {
                    i { class: "fa-solid fa-spinner fa-spin" }
                    "Downloading..."
                } else {
                    i { class: "fa-solid fa-download" }
                    "Download and Install"
                }
            }
            if !status().is_empty() {
                p { class: "core-status-message", role: "status", "{status}" }
            }

            div { class: "setting-divider", "Installed Cores" }
            if installed.is_empty() {
                div { class: "empty-state", "No cores installed yet." }
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
        }
    }
}

#[component]
fn CoreInstallRow(install: CoreInstall) -> Element {
    let database = use_context::<Arc<Database>>();
    let mut data_revision = use_context::<Signal<u64>>();
    let database_for_click = Arc::clone(&database);
    let engine = install.engine.clone();
    let channel = install.channel.clone();
    let version = install.version.clone();

    rsx! {
        div { class: "core-install-row",
            div { class: "core-install-info",
                strong { "{install.engine} · {install.version}" }
                span { "{install.channel} · {install.executable_path}" }
            }
            if install.selected {
                span { class: "status-badge connected", "Selected" }
            } else {
                button {
                    class: "btn-secondary small",
                    onclick: move |_| {
                        if database_for_click.select_core(&engine, &channel, &version).is_ok() {
                            data_revision.with_mut(|revision| *revision += 1);
                        }
                    },
                    "Use this core"
                }
            }
        }
    }
}
