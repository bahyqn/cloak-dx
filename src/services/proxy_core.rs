use std::{
    fs,
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use super::{
    database::{data_directory, Database, Subscription},
    geo_assets::geo_asset_directory,
    system_proxy,
    xray_config::{prepare_for_test, prepare_with_settings},
    xray_settings::XraySettings,
};

pub const DEFAULT_SOCKS_PORT: u16 = 2080;
pub const DEFAULT_HTTP_PORT: u16 = 2081;

const START_TIMEOUT: Duration = Duration::from_millis(300);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProxyCoreStatus {
    pub running: bool,
    pub engine: Option<String>,
    pub pid: Option<u32>,
    pub socks_port: Option<u16>,
    pub http_port: Option<u16>,
    pub subscription_id: Option<i64>,
    pub node_name: Option<String>,
    pub warning: Option<String>,
}

struct RunningCore {
    child: Child,
    config_path: PathBuf,
    pid: u32,
    socks_port: Option<u16>,
    http_port: Option<u16>,
    subscription_id: i64,
    node_name: Option<String>,
}

/// A short-lived Xray process used to test a single node.
///
/// The process and its temporary config are cleaned up when this value is
/// dropped.
pub struct TemporaryCore {
    child: Child,
    config_path: PathBuf,
    log_path: PathBuf,
    socks_port: Option<u16>,
}

impl TemporaryCore {
    pub fn socks_port(&self) -> Option<u16> {
        self.socks_port
    }
}

impl Drop for TemporaryCore {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.config_path);
        let _ = fs::remove_file(&self.log_path);
    }
}

/// Owns the running proxy core process.
///
/// Only one core can run at a time. Starting a new subscription stops the
/// previous process first, and stopping removes the generated config file.
pub struct ProxyCoreService {
    running: Mutex<Option<RunningCore>>,
    system_proxy: AtomicBool,
    system_proxy_applied: AtomicBool,
}

impl Default for ProxyCoreService {
    fn default() -> Self {
        Self::new()
    }
}

impl ProxyCoreService {
    pub fn new() -> Self {
        Self {
            running: Mutex::new(None),
            // Starting a core should make the network work out of the box.
            system_proxy: AtomicBool::new(true),
            system_proxy_applied: AtomicBool::new(false),
        }
    }

    pub fn system_proxy_enabled(&self) -> bool {
        self.system_proxy.load(Ordering::SeqCst)
    }

    /// Turns the system proxy on or off. If a core is already running the
    /// system settings are updated immediately.
    pub fn set_system_proxy_enabled(&self, enabled: bool) -> Result<(), String> {
        if !enabled {
            self.system_proxy.store(false, Ordering::SeqCst);
            self.system_proxy_applied.store(false, Ordering::SeqCst);
            return system_proxy::disable();
        }

        self.system_proxy.store(true, Ordering::SeqCst);
        let status = self.status();
        if !status.running {
            return Ok(());
        }
        system_proxy::enable(
            status.http_port.unwrap_or(DEFAULT_HTTP_PORT),
            status.socks_port.unwrap_or(DEFAULT_SOCKS_PORT),
        )?;
        self.system_proxy_applied.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn status(&self) -> ProxyCoreStatus {
        let dead = {
            let mut running = self.lock();
            let Some(core) = running.as_mut() else {
                return ProxyCoreStatus::default();
            };

            match core.child.try_wait() {
                Ok(Some(_)) => {
                    let config_path = core.config_path.clone();
                    *running = None;
                    let _ = fs::remove_file(config_path);
                    true
                }
                Ok(None) | Err(_) => false,
            }
        };

        if dead {
            // The core went away on its own; don't leave the system pointing at
            // a port that no longer exists.
            if self.system_proxy_applied.swap(false, Ordering::SeqCst) {
                let _ = system_proxy::disable();
            }
            return ProxyCoreStatus::default();
        }

        let running = self.lock();
        running.as_ref().map(status_from).unwrap_or_default()
    }

    pub fn start_selected_subscription(
        &self,
        database: &Database,
    ) -> Result<ProxyCoreStatus, String> {
        let subscription = database
            .selected_subscription()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "No subscription is selected.".to_string())?;
        self.start_subscription(database, &subscription)
    }

    pub fn start_subscription(
        &self,
        database: &Database,
        subscription: &Subscription,
    ) -> Result<ProxyCoreStatus, String> {
        let executable = resolve_xray_executable(database)?;
        if subscription.raw_config.trim().is_empty() {
            return Err(
                "The subscription has no configuration yet. Refresh it before starting."
                    .to_string(),
            );
        }
        let settings = XraySettings::load(database);

        // Validate before disturbing a core that is already running.
        prepare_with_settings(
            &subscription.raw_config,
            subscription.selected_node_key.as_deref(),
            DEFAULT_SOCKS_PORT,
            DEFAULT_HTTP_PORT,
            &settings,
        )?;

        self.stop();

        let socks_port = available_port(DEFAULT_SOCKS_PORT, None)?;
        let http_port = available_port(DEFAULT_HTTP_PORT, Some(socks_port))?;
        let prepared = prepare_with_settings(
            &subscription.raw_config,
            subscription.selected_node_key.as_deref(),
            socks_port,
            http_port,
            &settings,
        )?;

        let runtime_directory = runtime_directory()?;
        let config_path = runtime_directory.join("xray-config.json");
        let log_path = runtime_directory.join("xray.log");
        let mut child = spawn_xray(&executable, &prepared.config, &config_path, &log_path)?;

        wait_until_running(&mut child, &log_path, &config_path)
            .map_err(|error| tun_error_hint(&executable, settings.tun_enabled, error))?;
        if let Some(port) = prepared.socks_port {
            wait_for_listening(&mut child, port, &log_path, &config_path)?;
        }

        let mut warning = None;
        // TUN already captures every connection, so pointing the system proxy
        // at the loopback inbounds on top of it only adds a redundant hop.
        if self.system_proxy_enabled() && !settings.tun_enabled {
            let http_port = prepared.http_port.unwrap_or(DEFAULT_HTTP_PORT);
            let socks_port = prepared.socks_port.unwrap_or(DEFAULT_SOCKS_PORT);
            match system_proxy::enable(http_port, socks_port) {
                Ok(()) => self.system_proxy_applied.store(true, Ordering::SeqCst),
                Err(error) => {
                    warning = Some(format!(
                        "The proxy started, but the system proxy could not be enabled: {error}"
                    ));
                }
            }
        }

        let status = ProxyCoreStatus {
            running: true,
            engine: Some("xray".to_string()),
            pid: Some(child.id()),
            socks_port: prepared.socks_port,
            http_port: prepared.http_port,
            subscription_id: Some(subscription.id),
            node_name: prepared.node_name.clone(),
            warning,
        };
        *self.lock() = Some(RunningCore {
            pid: child.id(),
            socks_port: prepared.socks_port,
            http_port: prepared.http_port,
            subscription_id: subscription.id,
            node_name: prepared.node_name,
            child,
            config_path,
        });
        Ok(status)
    }

    /// Applies new configuration settings to the core that is currently
    /// running.
    ///
    /// Xray reads its config only at startup, so the process is restarted with
    /// the same subscription and node. Returns `None` when no core is running.
    pub fn reload_running_subscription(
        &self,
        database: &Database,
    ) -> Result<Option<ProxyCoreStatus>, String> {
        let status = self.status();
        if !status.running {
            return Ok(None);
        }
        let subscription_id = status
            .subscription_id
            .ok_or_else(|| "The running core is not tied to a subscription.".to_string())?;
        let subscription = database
            .subscriptions()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|subscription| subscription.id == subscription_id)
            .ok_or_else(|| "The subscription used by the running core was removed.".to_string())?;

        self.start_subscription(database, &subscription).map(Some)
    }

    /// Saves new Xray settings and applies them to the running core.
    ///
    /// Returns a user-facing status message. If the restarted core cannot come
    /// up (for example TUN without `CAP_NET_ADMIN`), `previous` is saved again
    /// and a second restart is attempted, so the caller can keep showing the
    /// restored values.
    pub fn apply_settings(
        &self,
        database: &Database,
        previous: &XraySettings,
        next: &XraySettings,
    ) -> Result<String, String> {
        next.save(database)?;

        if !self.status().running {
            return Ok("Saved. The new settings apply the next time a core starts.".to_string());
        }

        match self.reload_running_subscription(database) {
            Ok(Some(_)) => Ok(
                "Saved and applied. The running core was restarted with the new settings."
                    .to_string(),
            ),
            Ok(None) => {
                Ok("Saved. No core is running; the settings apply at the next start.".to_string())
            }
            Err(error) => {
                let restored = previous
                    .save(database)
                    .and_then(|()| self.reload_running_subscription(database).map(|_| ()));
                Err(match restored {
                    Ok(()) => format!(
                        "Could not apply the new settings: {error}. The previous settings were restored."
                    ),
                    Err(restore_error) => format!(
                        "Could not apply the new settings: {error}. Restoring the previous settings also failed: {restore_error}"
                    ),
                })
            }
        }
    }

    /// Gives the Xray binary the capabilities its TUN inbound needs.
    ///
    /// `pkexec` lets the desktop's polkit agent show the administrator prompt,
    /// so the app itself never runs as root. The capability is stored on the
    /// executable and survives app restarts, but has to be granted again after
    /// the core is updated to a new file.
    pub async fn grant_tun_privileges(&self, database: &Database) -> Result<(), String> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = database;
            Err(
                "TUN privileges are not granted through file capabilities on this platform."
                    .to_string(),
            )
        }

        #[cfg(target_os = "linux")]
        {
            let executable = resolve_xray_executable(database)?;
            let setcap = ["/usr/sbin/setcap", "/sbin/setcap"]
                .into_iter()
                .map(Path::new)
                .find(|candidate| candidate.is_file())
                .ok_or_else(|| {
                    "setcap was not found. Install the libcap2-bin package first.".to_string()
                })?;

            let output = tokio::process::Command::new("pkexec")
                .arg(setcap)
                // SO_BINDTODEVICE (auto route) needs CAP_NET_RAW on top of the
                // CAP_NET_ADMIN required to create the interface and add routes.
                .arg("cap_net_admin,cap_net_raw+ep")
                .arg(&executable)
                .output()
                .await
                .map_err(|error| {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        format!(
                            "pkexec was not found. Run `sudo setcap cap_net_admin,cap_net_raw+ep {}` manually.",
                            executable.display()
                        )
                    } else {
                        format!("Could not run pkexec: {error}")
                    }
                })?;

            if output.status.success() {
                return Ok(());
            }

            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.trim();
            Err(if stderr.is_empty() {
                format!(
                    "Granting TUN capabilities failed: pkexec exited with {}.",
                    output.status
                )
            } else {
                format!("Granting TUN capabilities failed: {stderr}")
            })
        }
    }

    /// Starts a throwaway Xray instance for a single node so it can be tested
    /// without disturbing the main proxy core.
    pub fn start_temporary_core(
        &self,
        database: &Database,
        subscription: &Subscription,
        node_key: &str,
    ) -> Result<TemporaryCore, String> {
        let executable = resolve_xray_executable(database)?;
        if subscription.raw_config.trim().is_empty() {
            return Err(
                "The subscription has no configuration yet. Refresh it before testing.".to_string(),
            );
        }

        let socks_port = available_port(0, None)?;
        let http_port = available_port(0, Some(socks_port))?;
        let prepared = prepare_for_test(
            &subscription.raw_config,
            Some(node_key),
            socks_port,
            http_port,
        )?;

        let runtime_directory = runtime_directory()?;
        let unique = unique_suffix();
        let config_path = runtime_directory.join(format!("xray-test-{unique}.json"));
        let log_path = runtime_directory.join(format!("xray-test-{unique}.log"));
        let mut child = spawn_xray(&executable, &prepared.config, &config_path, &log_path)?;
        wait_until_running(&mut child, &log_path, &config_path)?;

        if let Some(port) = prepared.socks_port {
            wait_for_listening(&mut child, port, &log_path, &config_path)?;
        }

        Ok(TemporaryCore {
            child,
            config_path,
            log_path,
            socks_port: prepared.socks_port,
        })
    }

    pub fn stop(&self) {
        {
            let mut running = self.lock();
            if let Some(mut core) = running.take() {
                let _ = core.child.kill();
                let _ = core.child.wait();
                let _ = fs::remove_file(&core.config_path);
            }
        }
        if self.system_proxy_applied.swap(false, Ordering::SeqCst) {
            let _ = system_proxy::disable();
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<RunningCore>> {
        self.running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for ProxyCoreService {
    fn drop(&mut self) {
        self.stop();
    }
}

fn status_from(core: &RunningCore) -> ProxyCoreStatus {
    ProxyCoreStatus {
        running: true,
        engine: Some("xray".to_string()),
        pid: Some(core.pid),
        socks_port: core.socks_port,
        http_port: core.http_port,
        subscription_id: Some(core.subscription_id),
        node_name: core.node_name.clone(),
        warning: None,
    }
}

fn resolve_xray_executable(database: &Database) -> Result<PathBuf, String> {
    let installs = database
        .core_installs()
        .map_err(|error| error.to_string())?;
    let install = installs
        .iter()
        .find(|install| install.selected && install.engine == "xray")
        .or_else(|| installs.iter().find(|install| install.engine == "xray"))
        .ok_or_else(|| {
            "No Xray-core installation found. Install Xray-core from Settings first.".to_string()
        })?;

    let executable = PathBuf::from(&install.executable_path);
    if !executable.is_file() {
        return Err(format!(
            "The Xray-core executable is missing: {}",
            executable.display()
        ));
    }
    Ok(executable)
}

/// Xray reports a missing TUN capability as a generic "operation not
/// permitted" while starting the inbound, so translate it into a hint that
/// names the actual fix. Any other failure is passed through untouched.
fn tun_error_hint(executable: &Path, tun_enabled: bool, error: String) -> String {
    if cfg!(target_os = "linux") && tun_enabled && error.contains("operation not permitted") {
        format!(
            "TUN mode needs the CAP_NET_ADMIN and CAP_NET_RAW capabilities. Open \
             Settings → TUN and click \"Grant TUN permission\", or run `sudo setcap \
             cap_net_admin,cap_net_raw+ep {}`. ({error})",
            executable.display()
        )
    } else {
        error
    }
}

fn wait_until_running(
    child: &mut Child,
    log_path: &Path,
    config_path: &Path,
) -> Result<(), String> {
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let log_tail = read_log_tail(log_path);
                let _ = fs::remove_file(config_path);
                return Err(if log_tail.is_empty() {
                    format!("Xray-core exited immediately with {status}.")
                } else {
                    format!("Xray-core exited immediately with {status}: {log_tail}")
                });
            }
            Ok(None) if Instant::now() >= deadline => return Ok(()),
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(config_path);
                return Err(format!("Could not monitor the Xray-core process: {error}"));
            }
        }
    }
}

fn spawn_xray(
    executable: &Path,
    config: &serde_json::Value,
    config_path: &Path,
    log_path: &Path,
) -> Result<Child, String> {
    fs::write(
        config_path,
        serde_json::to_vec_pretty(config).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("Could not write the Xray config: {error}"))?;

    let log = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)
        .map_err(|error| format!("Could not open the Xray log: {error}"))?;
    let error_log = log
        .try_clone()
        .map_err(|error| format!("Could not open the Xray log: {error}"))?;

    Command::new(executable)
        .arg("run")
        .arg("-c")
        .arg(config_path)
        // GeoIP/GeoSite files managed in Settings live in one shared folder.
        .env("XRAY_LOCATION_ASSET", geo_asset_directory())
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(error_log))
        .spawn()
        .map_err(|error| {
            let _ = fs::remove_file(config_path);
            format!(
                "Could not start Xray-core at {}: {error}",
                executable.display()
            )
        })
}

fn wait_for_listening(
    child: &mut Child,
    port: u16,
    log_path: &Path,
    config_path: &Path,
) -> Result<(), String> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            let log_tail = read_log_tail(log_path);
            let _ = fs::remove_file(config_path);
            return Err(if log_tail.is_empty() {
                format!("Xray-core exited before opening its proxy port with {status}.")
            } else {
                format!("Xray-core exited before opening its proxy port: {log_tail}")
            });
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(config_path);
            return Err("Xray-core did not open its local proxy port in time.".to_string());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn unique_suffix() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}-{sequence}", std::process::id())
}

fn read_log_tail(path: &Path) -> String {
    let Ok(content) = fs::read_to_string(path) else {
        return String::new();
    };
    let mut tail = content.trim().chars().rev().take(2000).collect::<Vec<_>>();
    tail.reverse();
    tail.into_iter().collect()
}

fn available_port(preferred: u16, avoid: Option<u16>) -> Result<u16, String> {
    if preferred != 0 {
        if let Some(port) = bind_free_port(preferred, avoid)? {
            return Ok(port);
        }
    }
    for _ in 0..20 {
        if let Some(port) = bind_free_port(0, avoid)? {
            return Ok(port);
        }
    }
    Err("Could not reserve a local port for Xray-core.".to_string())
}

fn bind_free_port(port: u16, avoid: Option<u16>) -> Result<Option<u16>, String> {
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(_) => return Ok(None),
    };
    let bound = listener
        .local_addr()
        .map_err(|error| format!("Could not inspect the reserved port: {error}"))?
        .port();
    if Some(bound) == avoid {
        return Ok(None);
    }
    Ok(Some(bound))
}

fn runtime_directory() -> Result<PathBuf, String> {
    let directory = data_directory().join("runtime");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Could not create the runtime directory: {error}"))?;
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_service_is_idle() {
        let service = ProxyCoreService::new();

        assert_eq!(service.status(), ProxyCoreStatus::default());
        service.stop();
        assert_eq!(service.status(), ProxyCoreStatus::default());
    }

    #[test]
    fn resolves_a_free_port_that_differs_from_the_avoided_one() {
        let first = available_port(0, None).unwrap();
        let second = available_port(0, Some(first)).unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn system_proxy_is_enabled_by_default() {
        assert!(ProxyCoreService::new().system_proxy_enabled());
    }

    #[test]
    fn tun_permission_errors_get_an_actionable_hint() {
        let executable = PathBuf::from("/opt/xray/xray");
        let hint = tun_error_hint(
            &executable,
            true,
            "Xray-core exited immediately with exit status: 1: operation not permitted".to_string(),
        );

        assert!(hint.contains("CAP_NET_ADMIN"));
        assert!(hint.contains("CAP_NET_RAW"));
        assert!(hint.contains("Grant TUN permission"));
        assert!(hint.contains("setcap"));
        assert!(hint.contains("/opt/xray/xray"));

        let unrelated = "Xray-core exited immediately with exit status: 1".to_string();
        assert_eq!(
            tun_error_hint(&executable, true, unrelated.clone()),
            unrelated
        );
        assert_eq!(
            tun_error_hint(&executable, false, "operation not permitted".to_string()),
            "operation not permitted"
        );
    }
}
