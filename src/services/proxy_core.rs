use std::{
    fs,
    io::{Read, Write},
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

use serde_json::Value;

use super::{
    database::{data_directory, Database, Subscription},
    geo_assets::geo_asset_directory,
    sing_box_config::{
        self, HotSwitch as SingBoxHotSwitch, PreparedConfig as SingBoxPreparedConfig,
    },
    sing_box_settings::SingBoxSettings,
    subscription_manager::{
        filter_nodes_for_engine, parse_subscription_nodes, subscription_node_key,
    },
    system_proxy,
    xray_config::{
        prepare_for_test, prepare_with_settings, HotSwitch, PreparedConfig as XrayPreparedConfig,
    },
    xray_settings::{XraySettings, DEFAULT_LAN_HTTP_PORT, DEFAULT_LAN_SOCKS_PORT},
};

pub const DEFAULT_SOCKS_PORT: u16 = 2080;
pub const DEFAULT_HTTP_PORT: u16 = 2081;

const START_TIMEOUT: Duration = Duration::from_millis(300);
/// How long a core may take to open a port it is expected to listen on.
///
/// Both engines open their inbounds in a fraction of a second on an idle
/// machine, but a batch test can start dozens of cores at once, so this is
/// generous enough to survive that load without hiding a core that is
/// genuinely stuck.
const PORT_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a core gets to clean up after SIGTERM before it is killed.
///
/// With TUN enabled the core owns the interface, the routes and the nftables
/// redirect rules it created. Only a normal shutdown removes them, and closing
/// a core that is serving traffic can take a moment, so the wait is long
/// enough to let that finish; a core that is truly stuck still gets killed.
const STOP_GRACE: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProxyCoreStatus {
    pub running: bool,
    pub engine: Option<String>,
    pub pid: Option<u32>,
    pub socks_port: Option<u16>,
    pub http_port: Option<u16>,
    pub lan_socks_port: Option<u16>,
    pub lan_http_port: Option<u16>,
    pub subscription_id: Option<i64>,
    pub node_name: Option<String>,
    /// Seconds since the core started, recomputed on every status poll.
    pub uptime_seconds: Option<u64>,
    pub warning: Option<String>,
}

impl ProxyCoreStatus {
    /// Engine name used in application logs, such as `xray`.
    pub fn engine_name(&self) -> &str {
        self.engine.as_deref().unwrap_or("core")
    }
}

/// Upload and download rates measured between two statistics polls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrafficRates {
    pub uplink_bps: u64,
    pub downlink_bps: u64,
}

struct RunningCore {
    child: Child,
    config_path: PathBuf,
    executable: PathBuf,
    engine: String,
    pid: u32,
    socks_port: Option<u16>,
    http_port: Option<u16>,
    lan_socks_port: Option<u16>,
    lan_http_port: Option<u16>,
    api_port: Option<u16>,
    started_at: Instant,
    subscription_id: i64,
    node_key: Option<String>,
    node_name: Option<String>,
    /// Selector/balancer metadata used for live node switching.
    hot_switch: Option<LiveSwitch>,
}

/// Engine-neutral view of the mechanism that moves a running core to another
/// node. Xray uses a balancer over its API; sing-box uses a selector exposed
/// through the Clash API.
struct LiveSwitch {
    api_port: u16,
    group: String,
    selected_tag: String,
    nodes: Vec<LiveNode>,
}

struct LiveNode {
    key: String,
    name: Option<String>,
    tag: String,
}

/// Ports and config shared by both engines before starting the process.
struct PreparedRuntime {
    config: Value,
    socks_port: Option<u16>,
    http_port: Option<u16>,
    lan_socks_port: Option<u16>,
    lan_http_port: Option<u16>,
    node_name: Option<String>,
    api_port: Option<u16>,
    hot_switch: Option<LiveSwitch>,
}

/// Cumulative Xray counters and the moment they were read, used to turn the
/// next sample into a rate.
struct TrafficSample {
    uplink: u64,
    downlink: u64,
    at: Instant,
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
        // Test configs never enable TUN or routing, so a hard kill cannot
        // leave routes or firewall rules behind; keeping it exit-fast also
        // keeps batch tests from waiting on a graceful shutdown per node.
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
    /// Previous statistics sample, so the dashboard can show rates instead of
    /// the cumulative counters Xray exposes.
    last_traffic: Mutex<Option<TrafficSample>>,
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
            last_traffic: Mutex::new(None),
        }
    }

    pub fn system_proxy_enabled(&self) -> bool {
        self.system_proxy.load(Ordering::SeqCst)
    }

    /// Engine whose test data describes the current setup: the running core
    /// when one is up, otherwise the core picked in Settings.
    ///
    /// Test results are stored per engine so switching cores cannot show one
    /// engine's measurements under another. When nothing has been installed
    /// yet, Xray is the only engine a test can start.
    pub fn active_engine(&self, database: &Database) -> String {
        self.status()
            .engine
            .or_else(|| database.selected_engine().ok().flatten())
            .unwrap_or_else(|| "xray".to_string())
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

    /// Reads Xray's cumulative inbound counters and returns the traffic rate
    /// since the previous call.
    ///
    /// The first call only records a baseline and reports zero, so the counters
    /// accumulated before the dashboard opened are not shown as one huge
    /// burst. `None` means no core with an API inbound is running (or the API
    /// call failed), which the caller renders as an idle connection.
    pub fn traffic_rates(&self) -> Option<TrafficRates> {
        let (engine, executable, api_port) = {
            let running = self.lock();
            let core = running.as_ref()?;
            (core.engine.clone(), core.executable.clone(), core.api_port?)
        };

        let (uplink, downlink) = if engine == "sing-box" {
            clash_connection_totals(api_port)?
        } else {
            let server = format!("127.0.0.1:{api_port}");
            let output = Command::new(&executable)
                .args(["api", "statsquery", "-s", &server])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            parse_traffic_counters(&output.stdout)?
        };

        let now = Instant::now();
        let mut last = self
            .last_traffic
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let rates = match last.as_ref() {
            Some(previous) => {
                let elapsed = now.duration_since(previous.at).as_secs_f64().max(0.001);
                TrafficRates {
                    uplink_bps: counter_rate(uplink, previous.uplink, elapsed),
                    downlink_bps: counter_rate(downlink, previous.downlink, elapsed),
                }
            }
            None => TrafficRates::default(),
        };
        *last = Some(TrafficSample {
            uplink,
            downlink,
            at: now,
        });
        Some(rates)
    }

    /// Tail of the core's own log, shown on the Logs page.
    ///
    /// The file is truncated on every start, so this is always the current
    /// run. Only the last 500 lines (and at most 256 KiB) are returned to keep
    /// a long-running core from bloating the UI.
    pub fn core_log(&self) -> Option<String> {
        let running_engine = self.lock().as_ref().map(|core| core.engine.clone());
        let directory = runtime_directory().ok()?;
        let path = match running_engine {
            Some(engine) => directory.join(format!("{}.log", engine_file_stem(&engine))),
            None => newest_core_log(&directory)?,
        };
        let bytes = fs::read(path).ok()?;
        let start = bytes.len().saturating_sub(256 * 1024);
        let text = String::from_utf8_lossy(&bytes[start..]);
        let mut lines: Vec<&str> = text.lines().collect();
        // The first line is likely cut in half when the file was too big.
        if start > 0 && !lines.is_empty() {
            lines.remove(0);
        }
        let first = lines.len().saturating_sub(500);
        Some(lines[first..].join("\n"))
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

    /// Makes a core installation the active engine.
    ///
    /// When the engine actually changes while a core is running, the node that
    /// core is serving is first stored under its own engine and the process is
    /// stopped. The next engine can then start from its own last selection, and
    /// switching back restores the node this engine was using.
    pub fn select_core(
        &self,
        database: &Database,
        engine: &str,
        channel: &str,
        version: &str,
    ) -> Result<(), String> {
        let previous_engine = database
            .selected_engine()
            .map_err(|error| error.to_string())?;
        if previous_engine.as_deref() == Some(engine) {
            return database
                .select_core(engine, channel, version)
                .map_err(|error| error.to_string());
        }

        let status = self.status();
        if status.running && status.engine.as_deref() != Some(engine) {
            if let (Some(subscription_id), Some(node_key), Some(running_engine)) = (
                status.subscription_id,
                self.running_node_key(),
                status.engine.clone(),
            ) {
                database
                    .select_subscription_node_for_engine(
                        subscription_id,
                        &running_engine,
                        &node_key,
                    )
                    .map_err(|error| error.to_string())?;
            }
            self.stop();
        }

        database
            .select_core(engine, channel, version)
            .map_err(|error| error.to_string())
    }

    fn running_node_key(&self) -> Option<String> {
        self.lock().as_ref().and_then(|core| core.node_key.clone())
    }

    pub fn start_subscription(
        &self,
        database: &Database,
        subscription: &Subscription,
    ) -> Result<ProxyCoreStatus, String> {
        let engine = database
            .selected_engine()
            .map_err(|error| error.to_string())?
            .unwrap_or_else(|| "xray".to_string());
        self.start_subscription_with_engine(database, subscription, &engine)
    }

    fn start_subscription_with_engine(
        &self,
        database: &Database,
        subscription: &Subscription,
        engine: &str,
    ) -> Result<ProxyCoreStatus, String> {
        // The persisted selection is shared between engines, so it can name a
        // node this engine cannot dial. Start the first node the engine does
        // support, exactly like the Proxies page displays it.
        let node_key = effective_node_key(subscription, engine);
        let (prepared, executable, tun_enabled) =
            prepare_runtime(database, subscription, engine, node_key.as_deref())?;

        // TUN is unusable without file capabilities, and a freshly installed
        // or upgraded binary never carries them. Ask for them before touching
        // the running core, so a cancelled prompt leaves the old proxy alone.
        if tun_enabled {
            ensure_tun_capabilities(&executable)?;
        }

        self.stop();

        let runtime_directory = runtime_directory()?;
        let stem = engine_file_stem(engine);
        let config_path = runtime_directory.join(format!("{stem}-config.json"));
        let log_path = runtime_directory.join(format!("{stem}.log"));
        let mut child = spawn_core(
            engine,
            &executable,
            &prepared.config,
            &config_path,
            &log_path,
        )?;

        wait_until_running(engine, &mut child, &log_path, &config_path)
            .map_err(|error| tun_error_hint(engine, &executable, tun_enabled, error))?;

        let mut listening_ports = [
            prepared.socks_port,
            prepared.http_port,
            prepared.lan_socks_port,
            prepared.lan_http_port,
            prepared.api_port,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        listening_ports.sort_unstable();
        listening_ports.dedup();
        for port in listening_ports {
            wait_for_listening(engine, &mut child, port, &log_path, &config_path)?;
        }

        // Xray's balancer has to be pointed at the selected node before any
        // system traffic is routed through the core. sing-box already starts
        // its selector with that node as the default.
        if engine == "xray" {
            if let Some(hot) = prepared.hot_switch.as_ref() {
                if let Err(error) =
                    override_balancer(&executable, hot.api_port, &hot.group, &hot.selected_tag)
                {
                    terminate_core(&mut child);
                    let _ = fs::remove_file(&config_path);
                    return Err(format!(
                        "The proxy started, but the selected node could not be activated: {error}"
                    ));
                }
            }
        }

        let mut warning = None;
        // TUN already captures every connection, so pointing the system proxy
        // at the loopback inbounds on top of it only adds a redundant hop.
        if self.system_proxy_enabled() && !tun_enabled {
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

        // Persist the node the core is actually running under this engine, so
        // Proxies highlights it and a later engine switch can restore it.
        if let Some(key) = node_key.as_deref() {
            let _ = database.select_subscription_node_for_engine(subscription.id, engine, key);
        }
        let node_key = node_key.or_else(|| {
            prepared.hot_switch.as_ref().and_then(|hot| {
                hot.nodes
                    .iter()
                    .find(|node| node.tag == hot.selected_tag)
                    .map(|node| node.key.clone())
            })
        });
        let status = ProxyCoreStatus {
            running: true,
            engine: Some(engine.to_string()),
            pid: Some(child.id()),
            socks_port: prepared.socks_port,
            http_port: prepared.http_port,
            lan_socks_port: prepared.lan_socks_port,
            lan_http_port: prepared.lan_http_port,
            subscription_id: Some(subscription.id),
            node_name: prepared.node_name.clone(),
            uptime_seconds: Some(0),
            warning,
        };
        *self
            .last_traffic
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        *self.lock() = Some(RunningCore {
            pid: child.id(),
            executable,
            engine: engine.to_string(),
            socks_port: prepared.socks_port,
            http_port: prepared.http_port,
            lan_socks_port: prepared.lan_socks_port,
            lan_http_port: prepared.lan_http_port,
            api_port: prepared.api_port,
            started_at: Instant::now(),
            subscription_id: subscription.id,
            node_key,
            node_name: prepared.node_name,
            hot_switch: prepared.hot_switch,
            child,
            config_path,
        });
        Ok(status)
    }

    /// Switches the running core to another node without restarting it.
    ///
    /// Returns `Ok(None)` when the running config cannot switch live (a full
    /// Xray config, or a node that is not in the generated outbound list), so
    /// the caller can fall back to a restart.
    pub fn switch_running_node(&self, node_key: &str) -> Result<Option<ProxyCoreStatus>, String> {
        let snapshot = {
            let running = self.lock();
            let Some(core) = running.as_ref() else {
                return Ok(None);
            };
            let Some(hot) = core.hot_switch.as_ref() else {
                return Ok(None);
            };
            let Some(node) = hot.nodes.iter().find(|node| node.key == node_key) else {
                return Ok(None);
            };
            if hot.selected_tag == node.tag {
                return Ok(Some(status_from(core)));
            }
            (
                core.pid,
                core.engine.clone(),
                core.executable.clone(),
                hot.api_port,
                hot.group.clone(),
                node.tag.clone(),
                node.name.clone(),
            )
        };
        let (pid, engine, executable, api_port, group, tag, name) = snapshot;

        // The command talks to the loopback API inbound; release the lock while
        // it runs so status polling keeps working.
        override_group(&engine, &executable, api_port, &group, &tag)?;

        let mut running = self.lock();
        if let Some(core) = running.as_mut() {
            if core.pid == pid {
                if let Some(hot) = core.hot_switch.as_mut() {
                    hot.selected_tag = tag;
                }
                core.node_key = Some(node_key.to_string());
                core.node_name = name;
                return Ok(Some(status_from(core)));
            }
        }
        Ok(None)
    }

    /// Applies a node switch after the selection was saved to the database.
    ///
    /// The boolean is `true` when the core had to be restarted because its
    /// config cannot switch nodes live.
    pub fn switch_to_node(
        &self,
        database: &Database,
        subscription_id: i64,
        node_key: &str,
    ) -> Result<(ProxyCoreStatus, bool), String> {
        let status = self.status();
        if !status.running {
            return Err("The proxy core is not running.".to_string());
        }
        if status.subscription_id != Some(subscription_id) {
            return Err("The proxy core is running a different subscription.".to_string());
        }
        if let Some(status) = self.switch_running_node(node_key)? {
            return Ok((status, false));
        }
        let status = self
            .reload_running_subscription(database)?
            .ok_or_else(|| "The proxy core stopped while switching nodes.".to_string())?;
        Ok((status, true))
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
        let engine = status
            .engine
            .ok_or_else(|| "The running core does not name its engine.".to_string())?;

        self.start_subscription_with_engine(database, &subscription, &engine)
            .map(Some)
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

        // Options such as the node-test concurrency do not appear in the
        // generated config, so saving them must not interrupt the connection.
        if !previous.requires_core_restart(next) {
            return Ok("Saved. The change applies the next time nodes are tested.".to_string());
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
            let engine = database
                .selected_engine()
                .map_err(|error| error.to_string())?
                .unwrap_or_else(|| "xray".to_string());
            let executable = resolve_core_executable(database, &engine)?;
            if tun_capabilities_granted(&executable) {
                return Ok(());
            }
            let setcap = find_capability_tool("setcap").ok_or_else(|| {
                "setcap was not found. Install the libcap2-bin package first.".to_string()
            })?;

            let output = tokio::process::Command::new(
                find_capability_tool("pkexec").unwrap_or_else(|| PathBuf::from("pkexec")),
            )
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

    /// Starts a throwaway engine instance for a single node so it can be
    /// tested without disturbing the main proxy core.
    pub fn start_temporary_core(
        &self,
        database: &Database,
        subscription: &Subscription,
        node_key: &str,
    ) -> Result<TemporaryCore, String> {
        let engine = database
            .selected_engine()
            .map_err(|error| error.to_string())?
            .unwrap_or_else(|| "xray".to_string());
        let executable = resolve_core_executable(database, &engine)?;
        if subscription.raw_config.trim().is_empty() {
            return Err(
                "The subscription has no configuration yet. Refresh it before testing.".to_string(),
            );
        }

        let mixed_port = available_port(0, &[])?;
        let config = match engine.as_str() {
            "xray" => {
                let http_port = available_port(0, &[mixed_port])?;
                // The test must exercise the same transport options as the real
                // core, otherwise a node can look healthy while the running core
                // cannot use it (for example when mux is enabled on a server
                // that rejects it).
                let settings = XraySettings::load(database);
                prepare_for_test(
                    &subscription.raw_config,
                    Some(node_key),
                    mixed_port,
                    http_port,
                    &settings,
                )?
                .config
            }
            "sing-box" => {
                let settings = SingBoxSettings::load(database);
                settings.validate()?;
                sing_box_config::prepare_for_test(
                    &subscription.raw_config,
                    Some(node_key),
                    mixed_port,
                    &settings,
                )?
                .config
            }
            other => return Err(format!("Unknown proxy core engine: {other}")),
        };

        let runtime_directory = runtime_directory()?;
        let unique = unique_suffix();
        let config_path =
            runtime_directory.join(format!("{}-test-{unique}.json", engine_file_stem(&engine)));
        let log_path =
            runtime_directory.join(format!("{}-test-{unique}.log", engine_file_stem(&engine)));
        let mut child = spawn_core(&engine, &executable, &config, &config_path, &log_path)?;
        wait_until_running(&engine, &mut child, &log_path, &config_path)?;
        wait_for_listening(&engine, &mut child, mixed_port, &log_path, &config_path)?;

        Ok(TemporaryCore {
            child,
            config_path,
            log_path,
            socks_port: Some(mixed_port),
        })
    }

    pub fn stop(&self) {
        {
            let mut running = self.lock();
            if let Some(mut core) = running.take() {
                // Graceful first: a killed sing-box (or Xray) leaves its TUN
                // routes and nftables redirect rules behind, which breaks the
                // whole system's networking.
                terminate_core(&mut core.child);
                let _ = fs::remove_file(&core.config_path);
            }
        }
        *self
            .last_traffic
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
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

/// Stops a core process without skipping its cleanup.
///
/// Both engines install network state while they run (a TUN interface, routes
/// and, with `auto_redirect`, nftables rules) and remove it in their own
/// shutdown path. On Unix `Child::kill` sends SIGKILL, which skips that path
/// and leaves the machine with rules pointing at a proxy that no longer
/// exists, so the core is asked to quit first. Only a core that ignores
/// SIGTERM for [`STOP_GRACE`] is killed.
fn terminate_core(child: &mut Child) {
    #[cfg(unix)]
    {
        // Safety: the child has not been reaped yet, so its pid still belongs
        // to this process.
        let signalled = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) } == 0;
        if signalled {
            let deadline = Instant::now() + STOP_GRACE;
            while Instant::now() < deadline {
                match child.try_wait() {
                    // Reaped by `try_wait`, so the process is gone and its
                    // TUN routes and firewall rules have been removed.
                    Ok(Some(_)) => return,
                    Ok(None) => thread::sleep(Duration::from_millis(25)),
                    Err(_) => break,
                }
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn status_from(core: &RunningCore) -> ProxyCoreStatus {
    ProxyCoreStatus {
        running: true,
        engine: Some(core.engine.clone()),
        pid: Some(core.pid),
        socks_port: core.socks_port,
        http_port: core.http_port,
        lan_socks_port: core.lan_socks_port,
        lan_http_port: core.lan_http_port,
        subscription_id: Some(core.subscription_id),
        node_name: core.node_name.clone(),
        uptime_seconds: Some(core.started_at.elapsed().as_secs()),
        warning: None,
    }
}

/// Resolves the node an engine will actually dial.
///
/// The selection is stored once per subscription and shared between engines;
/// after a switch it can name a node the new engine cannot run (for example a
/// ShadowsocksR node when sing-box is active). The first node the engine
/// supports is used then, matching what the Proxies page shows as selected.
fn effective_node_key(subscription: &Subscription, engine: &str) -> Option<String> {
    let nodes = filter_nodes_for_engine(
        Some(engine),
        parse_subscription_nodes(&subscription.raw_config),
    );
    subscription
        .selected_node_key
        .clone()
        .filter(|key| nodes.iter().any(|node| &subscription_node_key(node) == key))
        .or_else(|| nodes.first().map(subscription_node_key))
}

/// Prepares ports and config for the engine the user picked.
///
/// Nothing is stopped here: a config that cannot be built must leave an
/// already-running core untouched.
fn prepare_runtime(
    database: &Database,
    subscription: &Subscription,
    engine: &str,
    selected_node_key: Option<&str>,
) -> Result<(PreparedRuntime, PathBuf, bool), String> {
    let executable = resolve_core_executable(database, engine)?;
    if subscription.raw_config.trim().is_empty() {
        return Err(
            "The subscription has no configuration yet. Refresh it before starting.".to_string(),
        );
    }

    match engine {
        "xray" => {
            let settings = XraySettings::load(database);
            settings.validate()?;

            // Validate before disturbing a core that is already running.
            prepare_with_settings(
                &subscription.raw_config,
                selected_node_key,
                settings.local_socks_port,
                settings.local_http_port,
                lan_defaults(&settings),
                None,
                &settings,
            )?;

            let socks_port = available_port(settings.local_socks_port, &[])?;
            let http_port = available_port(settings.local_http_port, &[socks_port])?;
            let lan_ports = if settings.lan_enabled {
                let lan_socks_port =
                    available_lan_port(DEFAULT_LAN_SOCKS_PORT, &[socks_port, http_port])?;
                let lan_http_port = available_lan_port(
                    DEFAULT_LAN_HTTP_PORT,
                    &[socks_port, http_port, lan_socks_port],
                )?;
                Some((lan_socks_port, lan_http_port))
            } else {
                None
            };
            let mut reserved = vec![socks_port, http_port];
            if let Some((lan_socks, lan_http)) = lan_ports {
                reserved.push(lan_socks);
                reserved.push(lan_http);
            }
            let api_port = available_port(0, &reserved)?;
            let prepared = prepare_with_settings(
                &subscription.raw_config,
                selected_node_key,
                socks_port,
                http_port,
                lan_ports,
                Some(api_port),
                &settings,
            )?;
            let tun_enabled = settings.tun_enabled;
            Ok((runtime_from_xray(prepared), executable, tun_enabled))
        }
        "sing-box" => {
            let settings = SingBoxSettings::load(database);
            settings.validate()?;
            let mixed_port = available_port(settings.mixed_port, &[])?;
            let lan_port = if settings.lan_enabled {
                Some(available_lan_port(settings.lan_port, &[mixed_port])?)
            } else {
                None
            };
            let mut reserved = vec![mixed_port];
            if let Some(lan_port) = lan_port {
                reserved.push(lan_port);
            }
            let api_port = available_port(0, &reserved)?;
            let prepared = sing_box_config::prepare_with_settings(
                &subscription.raw_config,
                selected_node_key,
                mixed_port,
                lan_port,
                Some(api_port),
                &settings,
            )?;
            let tun_enabled = settings.tun_enabled;
            Ok((runtime_from_sing_box(prepared), executable, tun_enabled))
        }
        other => Err(format!("Unknown proxy core engine: {other}")),
    }
}

fn runtime_from_xray(prepared: XrayPreparedConfig) -> PreparedRuntime {
    PreparedRuntime {
        config: prepared.config,
        socks_port: prepared.socks_port,
        http_port: prepared.http_port,
        lan_socks_port: prepared.lan_socks_port,
        lan_http_port: prepared.lan_http_port,
        node_name: prepared.node_name,
        api_port: prepared.api_port,
        hot_switch: prepared.hot_switch.map(live_switch_from_xray),
    }
}

fn runtime_from_sing_box(prepared: SingBoxPreparedConfig) -> PreparedRuntime {
    PreparedRuntime {
        config: prepared.config,
        socks_port: Some(prepared.mixed_port),
        http_port: Some(prepared.mixed_port),
        lan_socks_port: prepared.lan_port,
        lan_http_port: prepared.lan_port,
        node_name: prepared.node_name,
        api_port: prepared.api_port,
        hot_switch: prepared.hot_switch.map(live_switch_from_sing_box),
    }
}

fn live_switch_from_xray(hot: HotSwitch) -> LiveSwitch {
    LiveSwitch {
        api_port: hot.api_port,
        group: hot.balancer,
        selected_tag: hot.selected_tag,
        nodes: hot
            .nodes
            .into_iter()
            .map(|node| LiveNode {
                key: node.key,
                name: node.name,
                tag: node.tag,
            })
            .collect(),
    }
}

fn live_switch_from_sing_box(hot: SingBoxHotSwitch) -> LiveSwitch {
    LiveSwitch {
        api_port: hot.api_port,
        group: hot.selector,
        selected_tag: hot.selected_tag,
        nodes: hot
            .nodes
            .into_iter()
            .map(|node| LiveNode {
                key: node.key,
                name: node.name,
                tag: node.tag,
            })
            .collect(),
    }
}

fn resolve_core_executable(database: &Database, engine: &str) -> Result<PathBuf, String> {
    let installs = database
        .core_installs()
        .map_err(|error| error.to_string())?;
    let install = installs
        .iter()
        .find(|install| install.selected && install.engine == engine)
        .or_else(|| installs.iter().find(|install| install.engine == engine))
        .ok_or_else(|| {
            format!(
                "No {} installation found. Install it from Settings first.",
                engine_label(engine)
            )
        })?;

    let executable = PathBuf::from(&install.executable_path);
    if !executable.is_file() {
        return Err(format!(
            "The {} executable is missing: {}",
            engine_label(engine),
            executable.display()
        ));
    }
    Ok(executable)
}

fn engine_label(engine: &str) -> &'static str {
    match engine {
        "sing-box" => "sing-box",
        _ => "Xray-core",
    }
}

fn engine_file_stem(engine: &str) -> &'static str {
    match engine {
        "sing-box" => "sing-box",
        _ => "xray",
    }
}

/// Xray reports a missing TUN capability as a generic "operation not
/// permitted" while starting the inbound, so translate it into a hint that
/// names the actual fix. Any other failure is passed through untouched.
fn tun_error_hint(engine: &str, executable: &Path, tun_enabled: bool, error: String) -> String {
    if cfg!(target_os = "linux") && tun_enabled && error.contains("operation not permitted") {
        format!(
            "TUN mode needs the CAP_NET_ADMIN and CAP_NET_RAW capabilities. Open \
             Settings → {} and click \"Grant TUN permission\", or run `sudo setcap \
             cap_net_admin,cap_net_raw+ep {}`. ({error})",
            engine_label(engine),
            executable.display()
        )
    } else {
        error
    }
}

/// Locates `setcap` / `getcap` even when `/usr/sbin` is not on `PATH`.
#[cfg(target_os = "linux")]
fn find_capability_tool(name: &str) -> Option<PathBuf> {
    ["/usr/sbin", "/sbin", "/usr/bin", "/bin"]
        .into_iter()
        .map(|directory| Path::new(directory).join(name))
        .find(|candidate| candidate.is_file())
}

/// Whether the binary already carries both capabilities with the effective
/// bit. When `getcap` is unavailable the state cannot be inspected, so this
/// reports "granted" and lets startup surface its normal TUN error instead of
/// prompting on every start.
#[cfg(target_os = "linux")]
fn tun_capabilities_granted(executable: &Path) -> bool {
    let Some(getcap) = find_capability_tool("getcap") else {
        return true;
    };
    Command::new(getcap)
        .arg(executable)
        .output()
        .map(|output| capabilities_are_granted(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or(true)
}

/// `getcap` prints either `cap_net_admin,cap_net_raw+ep` or the `=ep` form
/// depending on the libcap version.
#[cfg(target_os = "linux")]
fn capabilities_are_granted(getcap_output: &str) -> bool {
    let effective = getcap_output.contains("+ep") || getcap_output.contains("=ep");
    getcap_output.contains("cap_net_admin") && getcap_output.contains("cap_net_raw") && effective
}

/// Grants TUN capabilities on demand.
///
/// Called before starting a core with TUN enabled. `pkexec` raises the
/// desktop's polkit prompt, so Debian asks for the administrator password the
/// first time a binary is used and again after every core upgrade, while
/// already-granted binaries start without any prompt.
#[cfg(target_os = "linux")]
fn ensure_tun_capabilities(executable: &Path) -> Result<(), String> {
    if tun_capabilities_granted(executable) {
        return Ok(());
    }
    let setcap = find_capability_tool("setcap").ok_or_else(|| {
        "setcap was not found. Install the libcap2-bin package first.".to_string()
    })?;
    let output =
        Command::new(find_capability_tool("pkexec").unwrap_or_else(|| PathBuf::from("pkexec")))
            .arg(setcap)
            .arg("cap_net_admin,cap_net_raw+ep")
            .arg(executable)
            .output()
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
    let detail = if stderr.is_empty() {
        format!("pkexec exited with {}", output.status)
    } else {
        stderr.to_string()
    };
    Err(format!(
        "TUN mode needs the CAP_NET_ADMIN and CAP_NET_RAW capabilities, but granting them failed: \
         {detail}. Run `sudo setcap cap_net_admin,cap_net_raw+ep {}` manually.",
        executable.display()
    ))
}

#[cfg(not(target_os = "linux"))]
fn ensure_tun_capabilities(_executable: &Path) -> Result<(), String> {
    Ok(())
}

fn wait_until_running(
    engine: &str,
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
                    format!("{} exited immediately with {status}.", engine_label(engine))
                } else {
                    format!(
                        "{} exited immediately with {status}: {log_tail}",
                        engine_label(engine)
                    )
                });
            }
            Ok(None) if Instant::now() >= deadline => return Ok(()),
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                terminate_core(child);
                let _ = fs::remove_file(config_path);
                return Err(format!(
                    "Could not monitor the {} process: {error}",
                    engine_label(engine)
                ));
            }
        }
    }
}

fn spawn_core(
    engine: &str,
    executable: &Path,
    config: &serde_json::Value,
    config_path: &Path,
    log_path: &Path,
) -> Result<Child, String> {
    fs::write(
        config_path,
        serde_json::to_vec_pretty(config).map_err(|error| error.to_string())?,
    )
    .map_err(|error| {
        format!(
            "Could not write the {} config: {error}",
            engine_label(engine)
        )
    })?;

    let log = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)
        .map_err(|error| format!("Could not open the {} log: {error}", engine_label(engine)))?;
    let error_log = log
        .try_clone()
        .map_err(|error| format!("Could not open the {} log: {error}", engine_label(engine)))?;

    let mut command = Command::new(executable);
    command
        .arg("run")
        .arg("-c")
        .arg(config_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(error_log));
    if engine == "xray" {
        // GeoIP/GeoSite files managed in Settings live in one shared folder.
        command.env("XRAY_LOCATION_ASSET", geo_asset_directory());
    }
    command.spawn().map_err(|error| {
        let _ = fs::remove_file(config_path);
        format!(
            "Could not start {} at {}: {error}",
            engine_label(engine),
            executable.display()
        )
    })
}

/// Points the running core's group at one node without restarting it.
fn override_group(
    engine: &str,
    executable: &Path,
    api_port: u16,
    group: &str,
    tag: &str,
) -> Result<(), String> {
    if engine == "sing-box" {
        let body = serde_json::to_vec(&serde_json::json!({"name": tag}))
            .map_err(|error| error.to_string())?;
        let path = format!("/proxies/{group}");
        clash_api_request(api_port, "PUT", &path, Some(&body)).map(|_| ())
    } else {
        override_balancer(executable, api_port, group, tag)
    }
}

/// Points the Xray balancer at one node using `xray api bo`, so no connection
/// is interrupted and the process is not restarted.
fn override_balancer(
    executable: &Path,
    api_port: u16,
    balancer: &str,
    tag: &str,
) -> Result<(), String> {
    let output = Command::new(executable)
        .args(["api", "bo", "-s", &format!("127.0.0.1:{api_port}")])
        .arg("-b")
        .arg(balancer)
        .arg(tag)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Could not run the Xray API: {error}"))?;

    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    Err(if stderr.is_empty() {
        format!("xray api bo exited with {}.", output.status)
    } else {
        stderr.to_string()
    })
}

/// Minimal HTTP/1.1 client for sing-box's loopback Clash API.
///
/// The responses are tiny JSON documents (or an empty 204), and keeping this
/// local avoids adding an HTTP server dependency to the sync core service.
fn clash_api_request(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|error| format!("Could not reach the sing-box API: {error}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n");
    if let Some(body) = body {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("Could not write to the sing-box API: {error}"))?;
    if let Some(body) = body {
        stream
            .write_all(body)
            .map_err(|error| format!("Could not write to the sing-box API: {error}"))?;
    }

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("Could not read from the sing-box API: {error}"))?;
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "The sing-box API returned an incomplete response.".to_string())?;
    let headers = String::from_utf8_lossy(&response[..header_end]);
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| "The sing-box API returned an invalid status line.".to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!(
            "The sing-box API returned HTTP {status} for {method} {path}."
        ));
    }
    let body = &response[header_end + 4..];
    // Go's HTTP server streams JSON responses with chunked encoding, which the
    // minimal client has to unwrap before serde can read it.
    let chunked = headers.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("transfer-encoding")
                && value.to_ascii_lowercase().contains("chunked")
        })
    });
    if chunked {
        decode_chunked(body)
    } else {
        Ok(body.to_vec())
    }
}

/// Removes HTTP chunked framing from a response body.
fn decode_chunked(mut input: &[u8]) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    loop {
        let line_end = input
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| "The sing-box API returned a truncated chunk.".to_string())?;
        let size = std::str::from_utf8(&input[..line_end])
            .ok()
            .and_then(|line| line.split(';').next())
            .and_then(|size| usize::from_str_radix(size.trim(), 16).ok())
            .ok_or_else(|| "The sing-box API returned an invalid chunk size.".to_string())?;
        input = &input[line_end + 2..];
        if size == 0 {
            return Ok(output);
        }
        if input.len() < size {
            return Err("The sing-box API returned a truncated chunk.".to_string());
        }
        output.extend_from_slice(&input[..size]);
        input = &input[size..];
        if let Some(rest) = input.strip_prefix(b"\r\n".as_slice()) {
            input = rest;
        }
    }
}

fn clash_connection_totals(api_port: u16) -> Option<(u64, u64)> {
    let body = clash_api_request(api_port, "GET", "/connections", None).ok()?;
    let value: Value = serde_json::from_slice(&body).ok()?;
    let uplink = value.get("uploadTotal")?.as_u64()?;
    let downlink = value.get("downloadTotal")?.as_u64()?;
    Some((uplink, downlink))
}

fn wait_for_listening(
    engine: &str,
    child: &mut Child,
    port: u16,
    log_path: &Path,
    config_path: &Path,
) -> Result<(), String> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + PORT_WAIT_TIMEOUT;
    loop {
        if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            let log_tail = read_log_tail(log_path);
            let _ = fs::remove_file(config_path);
            return Err(if log_tail.is_empty() {
                format!(
                    "{} exited before opening its proxy port with {status}.",
                    engine_label(engine)
                )
            } else {
                format!(
                    "{} exited before opening its proxy port: {log_tail}",
                    engine_label(engine)
                )
            });
        }
        if Instant::now() >= deadline {
            // The log tail says whether the core was still initializing (for
            // example stuck on TUN/nftables state) or had already failed.
            let log_tail = read_log_tail(log_path);
            terminate_core(child);
            let _ = fs::remove_file(config_path);
            return Err(if log_tail.is_empty() {
                format!(
                    "{} did not open its local proxy port in time.",
                    engine_label(engine)
                )
            } else {
                format!(
                    "{} did not open its local proxy port in time: {log_tail}",
                    engine_label(engine)
                )
            });
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

/// Sums the inbound traffic counters from `xray api statsquery` output.
///
/// Xray omits counters that are still zero, encodes 64-bit integers as JSON
/// strings, and also counts its own API inbound, which would otherwise show up
/// as dashboard traffic.
fn parse_traffic_counters(output: &[u8]) -> Option<(u64, u64)> {
    let value: Value = serde_json::from_slice(output).ok()?;
    let stats = match value.get("stat") {
        Some(Value::Array(stats)) => stats.as_slice(),
        Some(_) => return None,
        // Xray omits the field entirely when no counter has been recorded.
        None => return Some((0, 0)),
    };

    let mut uplink = 0;
    let mut downlink = 0;
    for stat in stats {
        let Some(name) = stat.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(traffic) = name.strip_prefix("inbound>>>") else {
            continue;
        };
        if traffic.starts_with("api>>>") {
            continue;
        }
        let Some(counter) = stat.get("value").and_then(counter_value) else {
            continue;
        };
        if traffic.ends_with(">>>traffic>>>uplink") {
            uplink += counter;
        } else if traffic.ends_with(">>>traffic>>>downlink") {
            downlink += counter;
        }
    }
    Some((uplink, downlink))
}

fn counter_value(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

/// Converts two cumulative counter readings into bytes per second. A counter
/// that went backwards means the core was restarted, so report no traffic
/// rather than a wrapped-around spike.
fn counter_rate(current: u64, previous: u64, elapsed_seconds: f64) -> u64 {
    let delta = current.saturating_sub(previous);
    if delta == 0 || elapsed_seconds <= 0.0 {
        return 0;
    }
    (delta as f64 / elapsed_seconds).round() as u64
}

/// Best-effort LAN address other devices can use to reach this machine.
///
/// Connecting a UDP socket does not send a packet; it only asks the OS which
/// source address it would use, which is the address LAN clients should dial.
pub fn local_lan_ipv4() -> Option<std::net::Ipv4Addr> {
    let socket = std::net::UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    socket.connect(("8.8.8.8", 80)).ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(address) if !address.is_loopback() && !address.is_unspecified() => {
            Some(address)
        }
        _ => None,
    }
}

fn available_port(preferred: u16, avoid: &[u16]) -> Result<u16, String> {
    available_port_on("127.0.0.1", preferred, avoid)
}

/// LAN inbounds bind every interface, so a port that is only free on loopback
/// is not enough.
fn available_lan_port(preferred: u16, avoid: &[u16]) -> Result<u16, String> {
    available_port_on("0.0.0.0", preferred, avoid)
}

fn available_port_on(address: &str, preferred: u16, avoid: &[u16]) -> Result<u16, String> {
    if preferred != 0 {
        if let Some(port) = bind_free_port(address, preferred, avoid)? {
            return Ok(port);
        }
    }
    for _ in 0..20 {
        if let Some(port) = bind_free_port(address, 0, avoid)? {
            return Ok(port);
        }
    }
    Err("Could not reserve a local port for the proxy core.".to_string())
}

fn bind_free_port(address: &str, port: u16, avoid: &[u16]) -> Result<Option<u16>, String> {
    let listener = match TcpListener::bind((address, port)) {
        Ok(listener) => listener,
        Err(_) => return Ok(None),
    };
    let bound = listener
        .local_addr()
        .map_err(|error| format!("Could not inspect the reserved port: {error}"))?
        .port();
    if avoid.contains(&bound) {
        return Ok(None);
    }
    Ok(Some(bound))
}

/// The LAN ports used for config validation before the real ports are picked.
fn lan_defaults(settings: &XraySettings) -> Option<(u16, u16)> {
    settings
        .lan_enabled
        .then_some((DEFAULT_LAN_SOCKS_PORT, DEFAULT_LAN_HTTP_PORT))
}

fn newest_core_log(directory: &Path) -> Option<PathBuf> {
    ["xray.log", "sing-box.log"]
        .into_iter()
        .filter_map(|file| {
            let path = directory.join(file);
            let modified = fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok()?;
            Some((modified, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

pub(crate) fn runtime_directory() -> Result<PathBuf, String> {
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

    /// A core has to see SIGTERM, otherwise sing-box keeps its TUN routes and
    /// nftables redirect rules after the app stops it and the machine loses
    /// its network.
    #[cfg(unix)]
    #[test]
    fn stopping_a_core_asks_it_to_shut_down_before_killing_it() {
        let suffix = unique_suffix();
        let ready = std::env::temp_dir().join(format!("cloak-dx-sigterm-{suffix}.ready"));
        let marker = std::env::temp_dir().join(format!("cloak-dx-sigterm-{suffix}.marker"));
        let _ = fs::remove_file(&ready);
        let _ = fs::remove_file(&marker);
        // The trap only exists once the shell has run the command, so the
        // test waits for `ready` before signalling. Without that wait the
        // shell can still die from the default SIGTERM action.
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "trap 'touch {}; exit 0' TERM; touch {}; while :; do sleep 0.02; done",
                marker.display(),
                ready.display()
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.is_file() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(ready.is_file(), "the test shell never became ready");

        terminate_core(&mut child);

        assert!(
            marker.is_file(),
            "the core was killed without a chance to clean up"
        );
        let _ = fs::remove_file(ready);
        let _ = fs::remove_file(marker);
    }

    #[test]
    fn tun_capabilities_need_both_names_and_the_effective_bit() {
        assert!(capabilities_are_granted(
            "/path/sing-box cap_net_admin,cap_net_raw+ep\n"
        ));
        assert!(capabilities_are_granted(
            "/path/sing-box = cap_net_admin,cap_net_raw+ep\n"
        ));
        assert!(capabilities_are_granted(
            "/path/sing-box cap_net_admin=ep cap_net_raw=ep\n"
        ));
        assert!(!capabilities_are_granted("/path/sing-box\n"));
        assert!(!capabilities_are_granted(
            "/path/sing-box cap_net_admin+ep\n"
        ));
        assert!(!capabilities_are_granted(
            "/path/sing-box cap_net_admin,cap_net_raw+p\n"
        ));
    }

    #[test]
    fn statsquery_counters_are_summed_across_inbounds() {
        let output = br#"{
            "stat": [
                {"name": "inbound>>>socks>>>traffic>>>uplink", "value": "1000"},
                {"name": "inbound>>>http>>>traffic>>>uplink", "value": 24},
                {"name": "inbound>>>socks>>>traffic>>>downlink", "value": "4096"},
                {"name": "inbound>>>api>>>traffic>>>uplink", "value": "7"},
                {"name": "inbound>>>api>>>traffic>>>downlink", "value": "9"},
                {"name": "outbound>>>direct>>>traffic>>>uplink", "value": "555"},
                {"name": "inbound>>>http>>>traffic>>>downlink"}
            ]
        }"#;

        assert_eq!(parse_traffic_counters(output), Some((1024, 4096)));
    }

    #[test]
    fn empty_statsquery_output_counts_as_no_traffic() {
        assert_eq!(parse_traffic_counters(br#"{}"#), Some((0, 0)));
        assert_eq!(parse_traffic_counters(br#"{"stat": []}"#), Some((0, 0)));
        assert_eq!(parse_traffic_counters(b"not json"), None);
    }

    #[test]
    fn chunked_responses_are_unwrapped_before_json_parsing() {
        let body = b"1a\r\n{\"uploadTotal\": 1, \"downlo\r\n1f\r\nadTotal\": 2, \"connections\": []}\r\n0\r\n\r\n";
        assert_eq!(
            decode_chunked(body).as_deref(),
            Ok(&br#"{"uploadTotal": 1, "downloadTotal": 2, "connections": []}"#[..])
        );
        assert!(decode_chunked(b"5\r\nhello").is_err());
    }

    #[test]
    fn counter_rates_handle_elapsed_time_and_resets() {
        assert_eq!(counter_rate(2000, 1000, 2.0), 500);
        assert_eq!(counter_rate(1000, 1000, 2.0), 0);
        assert_eq!(counter_rate(10, 5000, 2.0), 0);
        assert_eq!(counter_rate(2000, 1000, 0.0), 0);
    }

    #[test]
    fn resolves_a_free_port_that_differs_from_the_avoided_one() {
        let first = available_port(0, &[]).unwrap();
        let second = available_port(0, &[first]).unwrap();

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
            "xray",
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
            tun_error_hint("xray", &executable, true, unrelated.clone()),
            unrelated
        );
        assert_eq!(
            tun_error_hint(
                "xray",
                &executable,
                false,
                "operation not permitted".to_string()
            ),
            "operation not permitted"
        );
    }
}
