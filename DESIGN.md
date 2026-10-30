# Cloak DX Design Document

> Version: based on the current working tree (2026-10-30)
>
> This document describes only the actual behavior of the code as implemented, including known limitations. When referring to code locations, repository-relative paths are used for direct navigation.

---

## 0. Overview

### 0.1 Tech stack and process model

- UI: Rust + Dioxus 0.7 desktop app (`#[component]`, `use_signal`, `use_context`, `use_resource`, `spawn_forever`, etc.).
- Proxy core: it does not embed either engine; instead, it downloads the official **Xray-core** or **sing-box** binary from GitHub Releases and runs it as a child process. The engine chosen in Settings is the one that starts, serves the Proxies page, and runs node tests.
- Data storage: SQLite (`cloak-dx.sqlite3`), which stores subscriptions, core installation records, node test results, per-engine node selection, and all app settings.
- Only one main core is allowed to run at a time. The main core is an `xray run -c <config>` or `sing-box run -c <config>` child process owned by `ProxyCoreService`.
- Latency/speed tests start an additional **temporary core** (one per tested node) of the same engine, kill it immediately after the test, and delete temp files so the main core is unaffected.

### 0.2 Data directory

This is determined uniformly by `data_directory()`. On Linux, it is:

```text
$XDG_DATA_HOME/cloak-dx        # usually ~/.local/share/cloak-dx
$XDG_DATA_HOME/cloak-dx/cloak-dx.sqlite3
$XDG_DATA_HOME/cloak-dx/cores/<engine>/<channel>/<version>/xray
$XDG_DATA_HOME/cloak-dx/cores/sing-box/<channel>/<version>/sing-box
$XDG_DATA_HOME/cloak-dx/geo/geosite.dat
$XDG_DATA_HOME/cloak-dx/geo/geoip.dat
$XDG_DATA_HOME/cloak-dx/geo/geoip-only-cn-private.dat
$XDG_DATA_HOME/cloak-dx/geo/geosite-cn.srs
$XDG_DATA_HOME/cloak-dx/geo/geoip-cn.srs
$XDG_DATA_HOME/cloak-dx/geo/geosite-category-ads-all.srs
$XDG_DATA_HOME/cloak-dx/runtime/xray-config.json
$XDG_DATA_HOME/cloak-dx/runtime/xray.log
$XDG_DATA_HOME/cloak-dx/runtime/xray-test-<pid>-<nanos>-<seq>.json
$XDG_DATA_HOME/cloak-dx/runtime/xray-test-<pid>-<nanos>-<seq>.log
$XDG_DATA_HOME/cloak-dx/runtime/sing-box-config.json
$XDG_DATA_HOME/cloak-dx/runtime/sing-box.log
$XDG_DATA_HOME/cloak-dx/runtime/sing-box-test-<pid>-<nanos>-<seq>.json
$XDG_DATA_HOME/cloak-dx/runtime/sing-box-test-<pid>-<nanos>-<seq>.log
$XDG_DATA_HOME/cloak-dx/runtime/sing-box-cache.db
$XDG_DATA_HOME/cloak-dx/runtime/system-proxy-backup.json
```

- `cores/`: binaries downloaded by the core management page, organized by engine / channel / version.
- `geo/`: shared geodata for all cores. Xray reads the `.dat` files through the environment variable `XRAY_LOCATION_ASSET`; sing-box reads the SagerNet `.srs` rule-sets by absolute path from the generated config. Each engine ignores the other engine's files.
- `runtime/`: runtime files, including the current config, logs, temp test configs, and system proxy backup. `sing-box-cache.db` is sing-box's own cache file (DNS and selector state), created only when "Cache file" is enabled.
- The main core truncates its log on startup; the log page reads only the last 500 lines (and at most 256 KiB).

### 0.3 Page structure

| Page        | Purpose                                                                                                             |
| ----------- | ------------------------------------------------------------------------------------------------------------------- |
| Dashboard   | Start/stop core, subscription and node information, TUN/system-proxy toggles, connectivity tests, real-time traffic |
| Proxy Nodes | Node cards, latency/exit/download testing, sorting, failure hiding, node selection                                  |
| Subscribes  | Subscription management, refresh, node count, traffic information                                                   |
| Network     | Exit, IPPure, Service Access, and DNS Leak checks for the currently running node                                    |
| Logs        | App logs + tail of the core log                                                                                     |
| Settings    | Appearance; engine-specific General and tabs; core management                                                       |

---

## 1. How to enable the proxy core

The engine is chosen in **Settings → Core Management** (`xray` or `sing-box`), or from the core selector in the Proxies page header, and stored in `app_settings.selected_engine`. That single choice drives four things:

- which binary `ProxyCoreService` resolves and launches;
- which protocols the Proxies page lists (sing-box can dial more than Xray);
- which engine runs the temporary cores for latency/speed tests;
- which per-engine node selection is restored on startup.

The Proxies header offers one entry per installed engine. Choosing another engine goes through the same `select_core` path as Core Management: the node the running core serves is stored under that engine, the process is stopped, and the other engine becomes selected. If a core was running, the Proxies page starts it again on the new engine immediately; otherwise the choice applies the next time a subscription starts. Engines without an installed binary are not offered, because selecting one could not start a core.

Sections 1.2–1.8 describe the Xray path; 1.9 describes what differs when sing-box is active.

### 1.1 Prerequisites

1. Install at least one version of the selected engine in **Settings → Core Management**.
2. Add and refresh a subscription in **Subscribes**, then select one.
3. (Optional) Download geodata in Core Management: `.dat` files for Xray, `.srs` rule-sets for sing-box. When they are missing, the rules that depend on geodata are skipped automatically instead of causing the core to fail to start.

Subscription content supports two types:

- **Node link list**: `vless://`, `vmess://`, `trojan://`, `ss://`, `socks://`, `http://`, etc., and may be base64-wrapped. Each engine filters this list to the protocols it can dial (Xray excludes sing-box-only protocols such as Hysteria2 / TUIC / AnyTLS; sing-box excludes ShadowsocksR). The Proxies page and the generated config use the same filter, so the two never disagree.
- **Full Xray JSON config**: includes an `outbounds` array whose elements have a `protocol` field. This type is treated as a complete config; see 1.7.

### 1.2 Startup entry and full flow

Entry point: Dashboard Start button → `ProxyCoreService::start_selected_subscription` → `start_subscription`.

Flow (`src/services/proxy_core.rs`):

```text
Start
 └─ Read selected_engine (defaults to xray)
 └─ Load the currently selected subscription from the database (error if none)
 └─ effective_node_key: the node saved for this engine, otherwise the legacy
    last-selected node, otherwise the first node this engine can dial
 └─ resolve the executable: the selected installation of that engine,
    otherwise its first installation
 └─ Check that the subscription raw_config is not empty
 └─ Load + validate that engine's settings (XraySettings / SingBoxSettings)
 └─ prepare_runtime(...) preflight
      · Build a config using the configured loopback ports and default LAN ports
      · No process is launched here; only the config generation is validated
 └─ TUN enabled → ensure the binary has CAP_NET_ADMIN/CAP_NET_RAW
      · getcap says they are present → continue without prompting
      · missing → pkexec setcap (desktop asks for the password once per binary)
 └─ stop() stop old core (and restore system proxy)
 └─ Allocate actual ports
 └─ prepare_runtime(...) generate the final config
 └─ Write runtime/xray-config.json or runtime/sing-box-config.json
 └─ Start xray run -c <config> or sing-box run -c <config>
      · Xray gets XRAY_LOCATION_ASSET=<data>/geo
      · sing-box reads .srs rule-sets by absolute path
      · Redirect stdout/stderr to runtime/<engine>.log
 └─ wait_until_running: process must not exit within 300ms
 └─ wait_for_listening: wait up to 5s for each required listening port
      · SOCKS / mixed port
      · LAN port (if enabled)
      · API port (if enabled)
 └─ Point the engine at the selected node
      · Xray: xray api bo ... cloak-node-N
      · sing-box: PUT /proxies/proxy with the node tag
 └─ Persist effective_node_key under this engine (engine_node_selection)
 └─ If system proxy is enabled and TUN is off, write GNOME system proxy settings
 └─ Return ProxyCoreStatus
```

Key points:

- **Validate first, then stop the old core**. A preflight failure does not interrupt the current connection.
- Ports are allocated after preflight because preflight only needs to confirm the config structure is valid; the final ports may change if they are occupied.
- The config is not modified incrementally; instead, it is **regenerated from scratch on every start using the subscription source + all current settings**.
- When starting the main core, hot-switching is enabled as much as possible (see Section 2), so an extra loopback API port is allocated.
- The effective node is resolved with the same protocol filter the Proxies page uses, so switching to an engine that cannot dial the previously selected node falls back to the first node it can dial instead of failing.

### 1.3 Port allocation

Defaults (`src/services/xray_settings.rs`):

| Purpose     | Default   | Listen address |
| ----------- | --------- | -------------- |
| Local SOCKS | `2080`    | `127.0.0.1`    |
| Local HTTP  | `2081`    | `127.0.0.1`    |
| LAN SOCKS   | `2090`    | `0.0.0.0`      |
| LAN HTTP    | `2091`    | `0.0.0.0`      |
| Xray API    | temp port | `127.0.0.1`    |

sing-box uses a different port model (`src/services/sing_box_settings.rs`): one **mixed** inbound serves both SOCKS and HTTP on the same loopback port (default `2080`), and the LAN side has its own single mixed port (default `2090`) with optional username/password. Its Clash API port is allocated per start like the Xray API port. The actual ports are exposed in `ProxyCoreStatus` as `socks_port` / `http_port`, both pointing at the mixed port, so the system proxy and the Network checks work unchanged.

Allocation rules (`available_port` / `available_lan_port`):

1. Prefer the configured port first (for LAN, a fixed default of 2090/2091 is used rather than a user input, because there is no dedicated LAN port setting).
2. If it is occupied, bind port `0` on loopback or on `0.0.0.0`, retrying up to 20 times.
3. All assigned ports avoid each other (SOCKS, HTTP, LAN SOCKS, LAN HTTP, API).
4. LAN ports must be bindable on `0.0.0.0`; loopback-only availability does not count.
5. The actual used ports are written to `ProxyCoreStatus`, and both the system proxy and UI read the actual values.

Note: the SOCKS/HTTP port settings are only “preferred values.” If they are occupied at startup, the core will switch to another port; on the next startup it will retry the user-configured values again.

### 1.4 Proxy entry after the core starts

After a successful start, the same Xray process provides:

- Local SOCKS5 (including UDP) on `127.0.0.1:<socks_port>`;
- Local HTTP on `127.0.0.1:<http_port>`;
- Optional LAN SOCKS/HTTP on `0.0.0.0:<lan_port>`;
- Optional TUN interface;
- Loopback API (`dokodemo-door`, used only for `xray api` and statistics).

All of the app’s network checks (Network page, latency/speed tests, Service Access) connect through the local SOCKS entry using `socks5h://127.0.0.1:<port>`, so DNS is resolved by the node side and the observed behavior reflects what the site sees after going through the proxy.

### 1.5 System proxy (GNOME)

Implementation: `src/services/system_proxy.rs`.

- Enabled by default (`system_proxy = true` in `ProxyCoreService::new`), and it is **not persisted**; after app restart, it returns to enabled.
- When the core starts, if the system proxy toggle is on and **TUN is off**, the app modifies GNOME `gsettings`:
  - `org.gnome.system.proxy mode = manual`
  - `use-same-proxy = false`
  - HTTP / HTTPS / FTP point to `127.0.0.1:<http_port>`
  - SOCKS points to `127.0.0.1:<socks_port>`
- Before modifying them, the original values are saved to `runtime/system-proxy-backup.json`; if writing fails, the app immediately tries to restore them.
- When the core stops, the window closes, or the core process exits on its own, it tries to restore the backup and delete the backup file.
- TUN mode does not set the system proxy because TUN already takes over system traffic; stacking them would add an extra hop.
- Only Linux + GNOME is supported at the moment. Other platforms return an error.
- System proxy failures do not cause the core startup to fail; the error is shown as `ProxyCoreStatus.warning` on the Dashboard.

### 1.6 TUN mode

TUN is an Xray inbound, not an application-layer proxy:

- When enabled, the `base_config` adds a TUN inbound with tag `"cloak-tun-in"`.
- It adds a `dns-out` outbound (`protocol: "dns"`) and a routing rule: TUN inbound port 53 → `dns-out`.
- It may optionally add system DNS takeover, FakeDNS, IPv6 gateway, and routing.
- TUN mode does not write the system proxy.

See Section 6.4 for the actual field mapping.

### 1.7 Special handling for full Xray JSON subscriptions

When the subscription is a full Xray JSON config, the app does not parse individual nodes; it uses the entire config as the base:

- Force `log.loglevel` to the configured log level.
- If the config already has `inbounds`:
  - Keep the subscription’s own inbound and ports;
  - Read the SOCKS / HTTP ports from it for status display and system proxy;
  - If LAN is enabled, append a pair of `0.0.0.0` LAN SOCKS/HTTP inbounds at the end.
- If the config has no `inbounds`:
  - Insert the app-generated loopback SOCKS/HTTP inbounds (and optional LAN inbounds).
- If TUN is enabled: append the TUN inbound, `dns-out`, and DNS 53 rule; only append the app-generated DNS/FakeDNS when the subscription has no `dns` section of its own.
- If Mux is enabled: add mux to outbound entries whose protocol is a proxy protocol and that do not already contain `mux`.
- If fragmentation is enabled: add `dialerProxy` to proxy outbounds and append a `cloak-fragment` outbound at the end.
- If the config has no `api` section and does not already use the `api` tag: append the API inbound, API outbound, routing rules, `stats`, and stats policy.
- **No balancer is generated, and hot-switching is not enabled.**
- The subscription’s own routing, DNS, outbound config, and ports remain unchanged.

Therefore, for full JSON subscriptions:

- Traffic statistics are available if there is no API conflict.
- The “switch node” action in the node cards does not truly change the selected outbound within the subscription; clicking a node triggers a restart fallback, but the same JSON config is still used after restart.
- The temp core for latency/speed tests only rebinds inbound ports and does not change the subscription’s own routing/DNS/outbound.

Important: if the full JSON already contains `inbounds` but has no SOCKS or HTTP inbound, then `PreparedConfig.socks_port` / `http_port` becomes `None`. In that case:

- The Network page treats it as “no usable SOCKS port,” and all checks are unavailable;
- The system proxy still writes the default `2080` / `2081`, but the subscription may not actually be listening on those ports;
- The app’s own probes also cannot work through a SOCKS entry.

Such subscriptions should either provide a local SOCKS/HTTP inbound themselves or be converted to the node-link-list format.

### 1.8 Stop and error handling

- `stop()`:
  - asks the main core process to exit (`SIGTERM` on Unix) and waits up to `STOP_GRACE` (5 s) for it to shut down;
  - kills it (`SIGKILL`, or `Child::kill` on other platforms) only if it is still alive after that grace period;
  - deletes the runtime config of the active engine;
  - clears traffic sampling baselines;
  - restores the original settings if the system proxy was written by the app.
- Graceful termination is required, not cosmetic: with TUN enabled the core owns a TUN interface, routes and (with `auto_route` / `auto_redirect`) policy-routing and nftables entries that only `sing-box` / Xray themselves can remove. A `SIGKILL` skips that cleanup and leaves redirect rules pointing at a process that no longer exists, which takes the whole machine offline. `terminate_core` is therefore shared by `stop()`, the startup-failure paths and the port-wait timeout.
- Temporary test cores still use a hard kill: their generated configs never contain TUN, routing or DNS, so there is nothing to clean up, and batch tests should not spend the grace period per node.
- If the status poll detects the child process has exited:
  - cleans the runtime state and config file;
  - restores the system proxy if it was applied;
  - returns the Dashboard to the not-running state.
- On window close, `use_wry_event_handler` calls `stop()` before the event loop exits to prevent stale system proxy settings.
- On startup failure:
  - reads the tail of the engine log and attaches it to the error;
  - terminates the half-started core through the same graceful path, then deletes the temp config;
  - if it is a TUN permission error, replaces it with a user-actionable hint (see Section 5).
- After spawning, the app waits up to `PORT_WAIT_TIMEOUT` (10 s) for every port the config says the core will open. The wait is per port, retries a loopback TCP connect every 50 ms, and fails early if the child exits. It logs the core's tail when the deadline passes, so a core stuck before its inbounds start (for example on leftover TUN/nftables state) is distinguishable from one that started and then died. Temporary test cores share this wait, and with batch tests starting dozens at once the longer deadline matters.

### 1.9 sing-box runtime

When `selected_engine = "sing-box"`, `start_subscription_with_engine` uses `sing_box_config::prepare_with_settings` instead of the Xray generator. The differences that matter operationally:

- **One mixed inbound.** There is no separate SOCKS/HTTP pair; `mixed` listens on the configured loopback port, and the LAN inbound is a second mixed listener with optional credentials. Both report the same port through `status.socks_port` / `status.http_port`.
- **Every supported node becomes an outbound.** Nodes sing-box cannot dial are dropped up front (ShadowsocksR in particular), and a subscription with no usable node is rejected with a clear error rather than starting a config that cannot route.
- **Current-version compatibility.** `route.default_domain_resolver` is always emitted (required since 1.14), and Reality outbounds always carry uTLS (default fingerprint `chrome`) because sing-box refuses Reality without it. A local resolver addressed by hostname is bootstrapped through a system `local` server, since a DNS server cannot resolve its own hostname. Deprecated DNS options such as `independent_cache` are not generated.
- **Hot switching uses the Clash API.** With an API port, all nodes are wrapped in a `selector` group tagged `proxy` (or a `urltest` group when auto failover is on), and the Clash API listens on `127.0.0.1:<api_port>`. The selected node is applied with `PUT /proxies/proxy` and `{"name": "<node tag>"}`, and clicking a node while the core runs reuses that call instead of restarting.
- **No full-config passthrough.** JSON, Clash YAML, Xray JSON, and URI-list subscriptions are all translated into sing-box outbounds; the subscription's own routing, DNS, and inbounds are not copied. The routing/DNS/TUN options come entirely from the sing-box settings panel.
- **Traffic statistics come from `GET /connections`.** The `uploadTotal` / `downloadTotal` fields are sampled over time; the client also decodes chunked responses, since sing-box's Go HTTP server may not set `Content-Length`.
- **Geo data is `.srs`.** `geosite-cn.srs`, `geoip-cn.srs`, and `geosite-category-ads-all.srs` are referenced by absolute path and are skipped from the rules when missing.
- **Cache file.** When enabled, sing-box writes `runtime/sing-box-cache.db` so DNS and selector state survive restarts; it is left in place across stops, matching sing-box's own behavior.

The rest of the lifecycle (preflight, port allocation, log tail, system proxy, TUN permission handling, stop/error handling) is shared with Xray and behaves the same.

---

## 2. Hot update for node switching

The goal of hot update is: switch nodes without restarting the core process or disconnecting existing connections.

### 2.1 Node identifier

The node key is generated by the subscription parser (`src/services/subscription_manager.rs`):

```text
<protocol_length>:<protocol>:<name_length>:<name>:<address>
```

Example:

```text
5:vless:8:🇭🇰 HK 01:hk1.example.com
```

The length prefixes prevent separators in names or protocols from causing ambiguity. The database stores only this key; the node content is always re-parsed from the original subscription text.

Selection is stored per engine in `engine_node_selection(subscription_id, engine, node_key)` (primary key `(subscription_id, engine)`). The legacy `subscriptions.selected_node_key` column remains the "last node selected anywhere" fallback: reading a node for an engine prefers that engine's row, then the legacy value, then the first node the engine can dial. When the engine is switched while a core is running, `select_core` writes the running node under the old engine before stopping; `start_subscription_with_engine` then writes the effective node under the engine it just started. Changing a subscription URL deletes all per-engine rows and clears the legacy value, since the new payload may not contain those nodes.

### 2.2 Generated hot-switch config

Hot-switch config is generated only for subscriptions in the **node-link list** format and only when an API port is passed at startup (`src/services/xray_config.rs`):

- Each buildable node generates an outbound with tag:

  ```text
  cloak-node-0
  cloak-node-1
  ...
  ```

- All nodes share one balancer:

  ```json
  {
    "tag": "cloak-balancer",
    "selector": ["cloak-node-"]
  }
  ```

- A fallback route is added so that all unmatched TCP/UDP traffic is sent to the balancer:

  ```json
  {
    "type": "field",
    "network": "tcp,udp",
    "balancerTag": "cloak-balancer"
  }
  ```

- A loopback API inbound is added:

  ```json
  {
    "tag": "api",
    "listen": "127.0.0.1",
    "port": <api_port>,
    "protocol": "dokodemo-door",
    "settings": {"address": "127.0.0.1"}
  }
  ```

- The `api` outbound, `api` routing rules, `api.services`, `stats`, and `policy.system` statistics counters are added.

- Outbound filtering: `outbound_is_unbuildable` skips the following nodes to avoid one broken node preventing the entire config from starting:
  - `streamSettings.network` is `quic` or `http` (Xray 26 removed these transports);
  - `security = reality` but no `publicKey`;
  - `vless` / `trojan` has no TLS/Reality and the server address is a public IP.

### 2.3 Pointing the balancer to the selected node at startup

The startup flow executes this after the API port begins listening:

```bash
xray api bo -s 127.0.0.1:<api_port> -b cloak-balancer cloak-node-N
```

This corresponds to `override_balancer`. If this step fails, the entire startup fails and the child process is killed, because the user-selected node was not applied and continuing would route traffic to the wrong exit.

If `auto_failover` is enabled, `HotSwitch` metadata is not recorded and this command is not executed; the balancer is instead controlled by the observatory’s `leastPing` policy.

### 2.4 Switching when the user clicks a node

Flow (`src/components/proxies.rs` + `src/services/proxy_core.rs`):

1. Clicking a node card first calls `select_subscription_node(subscription_id, node_key)` to write to the database.
2. If the main core is running and the subscription currently being served is the same subscription as the card, it then calls `switch_to_node` in the background.
3. `switch_to_node` first tries `switch_running_node`:
   - Reads `HotSwitch` metadata from runtime state (API port, balancer name, node tag list);
   - Finds the target tag by key;
   - If the target tag is already the current tag, it returns immediately without executing a command;
   - Otherwise it runs:

     ```bash
     xray api bo -s 127.0.0.1:<api_port> -b cloak-balancer cloak-node-N
     ```

   - On success, it updates the in-memory `selected_tag` and `node_name` without restarting the process.
4. If hot-switching is not possible, it calls `reload_running_subscription`:
   - Rebuilds the config using the same subscription and the newly selected node;
   - Stops the old process and starts a new one;
   - Returns `restarted = true`.
5. The UI shows different toast messages based on `restarted`:
   - `Node switched.`: hot update succeeded;
   - `Node switched. This subscription format restarts the core.`: fallback restart path was used.

### 2.5 When hot-switching is not possible

The following conditions trigger the restart fallback or make the action unavailable:

- The subscription is a full Xray JSON config (no `HotSwitch`).
- The running config has no API port.
- The clicked node is not in the `hot.nodes` list (for example, it was filtered out by `outbound_is_unbuildable`, or the protocol cannot be converted to an outbound).
- `auto_failover` is enabled: the config intentionally does not record `HotSwitch`, so manual selection has no effect; clicking a node triggers the restart fallback, but after restart the balancer is still controlled by observatory.
- The core is not running.
- The core is running a different subscription (`switch_to_node` returns `The proxy core is running a different subscription.`).

### 2.6 Automatic failover

When `auto_failover = true` and the config has an API:

- The balancer adds:

  ```json
  "strategy": {"type": "leastPing"}
  ```

- An observatory is added:

  ```json
  {
    "subjectSelector": ["cloak-node-"],
    "probeUrl": "https://www.google.com/generate_204",
    "probeInterval": "60s",
    "enableConcurrency": true
  }
  ```

- Xray probes all nodes concurrently every 60 seconds, and the balancer automatically selects the node with the lowest reachable latency; if a node stops responding, traffic is removed from it automatically.
- Manual node selection is ignored; the UI still displays the selected node name from the subscription, but the actual exit is determined by observatory.

### 2.7 Traffic statistics

- The main core always requests an API at startup (for node-link subscriptions).
- Xray: the Dashboard traffic cards read cumulative counters via `xray api statsquery -s 127.0.0.1:<api_port>`, counting only `inbound>>>...>>>traffic>>>uplink/downlink` and excluding the `api` inbound itself.
- sing-box: the cards read `GET http://127.0.0.1:<api_port>/connections` and accumulate `uploadTotal` / `downloadTotal`, including when the response is chunked.
- The difference between two samples divided by time yields B/s; the first sample only establishes a baseline and is displayed as 0.
- If there is no API (for example, when a full JSON config conflicts), the traffic cards show idle.

### 2.8 Hot switching under sing-box

sing-box does not have an Xray-style balancer; it uses a **selector** outbound group:

- Every dialable node becomes an outbound tagged `cloak-node-N` and is listed in the `proxy` selector's `outbounds`.
- `default` is the selected node, so the first start needs no API call to route correctly.
- While the core is running, a click is applied with:

  ```http
  PUT /proxies/proxy
  {"name": "cloak-node-N"}
  ```

  This switches the selector without restarting the process. The Clash API is bound to `127.0.0.1` only and the request uses a short timeout; a failure falls back to `reload_running_subscription`.
- With **auto failover** enabled, the group is a `urltest` group instead, probing every node and keeping traffic on the fastest reachable one. As with Xray, manual selection cannot override it, so clicking a node falls back to a restart.
- Full-config subscriptions have no `HotSwitch` metadata: clicking a node restarts the core, same as Xray.

---

## 3. Network page features

All checks on the Network page target the **currently running core** and are sent through its SOCKS inbound, so the result describes the actual connection used for service traffic, rather than a separate test instance.

### 3.1 Page state and entry points

- The header shows the current node name and `SOCKS 127.0.0.1:<port>`.
- When the core is not running, it shows the banner “Start the proxy core first,” and all test buttons are disabled.
- The top **Test** button launches four checks at once:
  - Google Exit;
  - IPPure;
  - DNS Leak;
  - Service Access (including Google Ping).
- Each card has its own **Test / Re-test** button.
- While any check is running, the top button shows a spinning icon and is disabled.
- Results are stored in an app-level signal; switching to another page and back does not lose them, and checks that are still running are not canceled when leaving the page.
- When the selected subscription + selected node key changes, all Network results are cleared to avoid showing the previous node’s results under the new node.
- Late check results verify the `owner`; if the node changed in the meantime, the result is discarded and not written back.

### 3.2 Google Exit

Implementation: `network_test::test_google_exit` → `proxy_test::probe_google_exit`.

- Accesses the current core through the SOCKS client.
- Runs in parallel:
  - Google Search page location footer / “my location” answer card;
  - YouTube page country judgment from Google;
  - Redirected country domain when accessing `https://www.google.com/` (for example `google.com.hk`);
  - Cloudflare `cdn-cgi/trace` to obtain the exit IP;
  - `ipwho.is` as a fallback for exit IP and geolocation.
- Country priority: Google redirect country > YouTube country > Search answer card.
- City / region is used only when it matches the country determined by Google; the region returned by the Google footer is preserved as an exception because it reflects Google’s own GeoIP result.
- Card display:
  - Exit IP;
  - Google country;
  - City / region.
- This is a best-effort probe: if geolocation services fail, the latency test still returns normally.

### 3.3 IPPure

Implementation: `network_test::test_ippure`.

- First obtains the exit IP via Cloudflare trace.
- Requests IPPure in sequence:
  - `/api/info/ip-basic/<ip>`: ASN, organization, country, city, is-residential, etc.;
  - `/api/info/ip-risk/<ip>`: risk score (best effort);
  - `/api/info/asn/botclass/<asn>`: human / bot ratio (best effort).
- IPPure uses a signature challenge:
  - The first request returns response headers `x-k` (signature key) and `x-t` (timestamp);
  - The second request sends `x-k` and `x-t: <stamp>-<signature>`;
  - The signature algorithm is HMAC-SHA256(key, `GET-<url>--<stamp>`);
  - The two requests must use the same TCP connection, which is preserved via the reqwest connection pool.
- Risk score display (0–100):
  - 0–15: extremely clean
  - 16–25: clean
  - 26–40: neutral
  - 41–50: mild risk
  - 51–70: moderate risk
  - 71–100: extremely risky
- IP type:
  - `isp` or residential mark: residential IP
  - `hosting` / `business`: colocation/server IP
  - `enterprise`: business broadband
  - `education`: education IP
  - `government`: government IP
- Address nature: `is_broadcast = true` shows “broadcast IP,” otherwise “native IP.”
- The card also shows WARP flag, ASN, location, and human/bot ratio.

### 3.4 Service Access

Implementation: `src/services/service_access.rs`.

- Supports 8 services: Google, YouTube, Gemini, Netflix, GitHub, ChatGPT, Disney+, and Telegram.
- The page provides a **picker modal** opened from the card's **Choose** button, with a checkbox per service and the service name beside it. Up to 4 services can be selected; further checkboxes are disabled until one is cleared. The same service cannot be selected twice.
- Selection is persisted in the `app_settings` key `service_access`, as a comma-separated id string, for example:

  ```text
  google,youtube,netflix
  ```

- The Dashboard uses the same selection, so both sides show and test the same services.
- Each service is accessed through the current core’s SOCKS endpoint at a fixed URL with a 15-second timeout; any 2xx response counts as reachable and records the millisecond latency and HTTP status.
- Google Ping executes separately at `https://www.gstatic.com/generate_204`; even if Google is not among the four selected services, its result still appears.
- All selected services and Google Ping are tested concurrently.

### 3.5 DNS Leak

Implementation: `network_test::test_dns_leak`.

- Provides two detection sites:
  - **ipleak.net** (default)
  - **BrowserLeaks**
- All probe domains are resolved through the current core’s SOCKS (`socks5h`), so the detection sites see the actual resolver being used by that proxy connection.
- ipleak.net:
  - Generates a 40-character session;
  - Concurrently requests 3 endpoints: `<session>-<1..3>.ipleak.net/dnsdetection/`;
  - Merges the returned resolvers;
  - Uses `ipleak.net/json/<ip>` to supplement country, location, and organization.
- If ipleak.net does not return any resolvers (for example, the resolver returned a wildcard cache for `*.ipleak.net`), it falls back to BrowserLeaks and marks the result with `fallback = BrowserLeaks`.
- BrowserLeaks:
  - Two random IPv4 subdomains `*.dns4.browserleaks.net`;
  - One random IPv6 subdomain `*.dns6.browserleaks.net`;
  - Aggregates by resolver IP.
- Detection rules depend on the “exit country”:
  - Exit country is taken from Google Exit first;
  - next from IPPure;
  - finally inferred from the flag emoji in the current node name.
- Result logic:
  - Unknown exit country → `Mixed` (cannot determine);
  - All resolvers lack geolocation → `Mixed`;
  - All resolvers with geolocation match the exit country → `Protected`;
  - Any resolver with geolocation differs from the exit country → `Leaking`.
- The UI displays resolver IP, country, location, organization, and a result banner.
- The Dashboard DNS row reads the same result:
  - `Protected` → `Safe (No Leak)` / `Clean`
  - `Mixed` → `Unknown` / `Unknown`
  - `Leaking` → `Leaking` / `Polluted`

### 3.6 Result ownership and concurrency

- Each check has its own `running` signal; the top button combines them into a single “any running” state.
- The `run_*` functions use `spawn_forever`, and tests are not bound to the page scope.
- The `owner` (subscription id + node key) is captured at startup and compared again when finished; mismatches are discarded.
- Result reset logic lives in `src/app.rs` and is triggered by `data_revision`.

---

## 4. How local latency and download-speed tests work

Latency testing does not go through the main core; instead, for each node under test, it starts a **temporary instance of the selected engine**. This has several advantages:

- It does not interfere with the running proxy;
- It allows multiple nodes to be tested concurrently;
- Each node uses an independent real config, so the measured results are closer to actual usage;
- The tested protocol set always matches what the active engine can actually dial.

### 4.1 Creating the temporary core

`ProxyCoreService::start_temporary_core`:

1. Resolves the selected binary for the active engine.
2. Checks the subscription raw text is not empty.
3. Allocates a temporary loopback proxy port (Xray additionally gets a separate SOCKS and HTTP port; sing-box uses one mixed port).
4. Reads the active engine's settings and calls its `prepare_for_test`.
5. Generates a unique file name:

   ```text
   runtime/xray-test-<pid>-<unix_nanos>-<sequence>.json
   runtime/xray-test-<pid>-<unix_nanos>-<sequence>.log
   runtime/sing-box-test-<pid>-<unix_nanos>-<sequence>.json
   runtime/sing-box-test-<pid>-<unix_nanos>-<sequence>.log
   ```

6. Starts `xray run -c <temp-config>` (with `XRAY_LOCATION_ASSET`) or `sing-box run -c <temp-config>`.
7. Waits for the process to stabilize and for the proxy port to begin listening.
8. Returns `TemporaryCore`; its `Drop` kills the process, waits, and deletes the temp config and log.

### 4.2 Differences between the test config and main config

The shared principle is the same for both engines: keep what affects reachability (protocol, TLS, mux, sniffing), and drop what only adds overhead or privilege requirements (routing, DNS, FakeDNS, TUN, LAN, auto failover, API/balancer).

For Xray node-link subscriptions, `prepare_for_test` derives a `test_settings` configuration:

| Option                                  | Test core behavior                                |
| --------------------------------------- | ------------------------------------------------- |
| mux                                     | Kept (must fail if the node does not support mux) |
| fragment                                | Kept (DPI evasion is part of the real connection) |
| sniffing                                | Kept                                              |
| Local / remote DNS                      | Cleared                                           |
| FakeDNS                                 | Off                                               |
| Custom bypass list                      | Cleared                                           |
| Bypass mainland China / private network | Off                                               |
| TUN                                     | Off                                               |
| Auto failover                           | Off                                               |
| LAN                                     | Off                                               |
| API / balancer                          | Not generated                                     |
| Routing                                 | Fixed to empty rules + `domainStrategy: AsIs`     |

Why:

- The test only cares about latency and throughput, not georouting;
- Loading geosite/geoip for each test core would consume a large amount of memory when up to 30 temp cores are running simultaneously;
- TUN requires extra permissions, which tests should not depend on;
- Keeping mux / fragment / sniffing avoids false positives such as “the test is fast, but the main core cannot actually connect.”

For full Xray JSON subscriptions, `prepare_test_config` only rebinds ports:

- The first SOCKS inbound is changed to a temp SOCKS port;
- The first HTTP inbound is changed to a temp HTTP port;
- Other inbounds with ports are changed to random free ports;
- If there is no SOCKS/HTTP inbound, one is added;
- The subscription’s own routing, DNS, and outbound remain unchanged.

For sing-box, `sing_box_config::prepare_for_test` builds a minimal config instead of reusing the runtime one:

- only the tested node is emitted (tagged `proxy`) plus a `direct` outbound;
- one `mixed` inbound on the temp port, with `route.final = proxy`;
- DNS is reduced to a single `local` server, and TUN, LAN, routing rules, sniffing overrides, and the Clash API are not generated;
- the outbound options that change the real connection are still applied: TCP Fast Open, and mux on the protocols that support it.

This keeps a 30-way concurrent test from each loading geo rule-sets or requiring TUN privileges, while still failing on nodes whose real handshake or mux settings do not work.

### 4.3 Latency test

`proxy_test::test_latency`:

1. Starts a temporary core for that node.
2. Creates a reqwest client with `socks5h://127.0.0.1:<temp_socks>`, with a per-node timeout of 10 seconds.
3. Starts a Google Exit probe at the same time (to save one core startup while doing the latency request in parallel).
4. Sends **4** sequential `GET https://www.gstatic.com/generate_204` requests:
   - Each single request has a 4-second timeout;
   - Only successful responses with status code 2xx are counted in the sample;
   - The time of each request is recorded.
5. Calculates:
   - `milliseconds` = average of successful samples (integer milliseconds);
   - `packet_loss_percent` = `(4 - success_count) * 100 / 4`;
   - If all 4 requests fail, it returns the last error.
6. The Google Exit result is returned together as `LatencyTestResult.google_exit`; a failed exit probe does not affect the latency result.
7. The temporary core is dropped at the end of the function, and the process is cleaned up.

Why `gstatic` 204: it is much faster than the Google home page, which may exceed 10 seconds on some nodes.

### 4.4 Download-speed test

`proxy_test::test_speed`:

1. Starts a temporary core for that node.
2. Creates a SOCKS client with a total timeout of 30 seconds.
3. Requests:

   ```text
   https://speed.cloudflare.com/__down?bytes=26214400
   ```

4. Streams the response chunks and stops when any condition is met:
   - **8 seconds** of sampling time has elapsed;
   - **25 MiB** has been downloaded;
   - The server finishes the response;
   - A read error occurs (if data has already been read, the downloaded amount is preserved).
5. Calculates the download rate:

   ```text
   download_MiB_per_s = downloaded_bytes / 1_048_576 / elapsed_seconds
   ```

6. Upload test (best effort):
   - `POST https://speed.cloudflare.com/__up`;
   - Request body is 1 MiB of zero bytes;
   - Reads the full response;
   - Calculates MiB/s.
   - If upload fails, `upload_mbps = None` and the whole speed test does not fail.
7. The temporary core is cleaned up after `Drop`.

Design intent:

- It does not use “download the full 25 MiB” as the success condition, because slow nodes would time out before reporting anything;
- A fixed 8-second sampling window still yields meaningful throughput on slow nodes;
- The 25 MiB cap prevents extremely fast nodes from downloading too much data within 8 seconds.

### 4.5 Speed-test concurrency control

Setting: **Node Testing → Test concurrency**

- Default: 8;
- Range: 1–30;
- Read once at the start of each run;
- `worker_count = min(node_count, concurrency).max(1)`;
- Each worker is a separate OS thread, and internally creates `tokio::runtime::Builder::new_current_thread()`;
- All workers take nodes from a shared `VecDeque`;
- Results are sent back to the UI thread through an `std::sync::mpsc` channel;
- Each node under test still starts its own temporary core, so concurrency directly determines the number of core processes that exist simultaneously.

This setting does **not** enter the generated core config, so saving it does not restart the main core (see Section 6).

### 4.6 Result persistence and display

- Latency test:
  - Success: `node_test_results.latency_ms` is updated, `latency_failed = 0`, and `packet_loss` is updated;
  - Failure: `latency_ms = NULL`, `latency_failed = 1`;
  - If the same test succeeds for Google Exit, it separately updates `exit_ip` / `exit_country_code` / `exit_country` / `exit_region`.
- Manual “Test Google exit” updates only the exit fields and leaves latency unchanged.
- Rows are keyed by `(subscription_id, engine, node_key)`. `engine` is the running core, or the core selected in Settings when nothing is running; rows written by an older build predate the column and are attributed to `xray`. Switching engines therefore reads only that engine's history, drops the in-memory values measured with the previous core, and cancels running tests.
- Speed-test results exist **only in memory** and are not written to the database; restarting the app clears them.
- The Proxies page reads the database’s latency and exit results on startup, so after restarting the app the previous latency / packet loss / exit results are still visible.
- The dashboard’s Connectivity Test card reads the same live signals and the same `node_test_results` rows as the Proxies page, so a latency / packet-loss result produced by a test run on either page appears on both. Switching nodes no longer discards the live results of the node that becomes selected; switching engines still drops the in-memory values because they were measured with the previous core.
- The Proxies page and the dashboard only list nodes the active engine can dial. Xray hides protocols it has no outbound for (Hysteria/Hysteria2, TUIC, AnyTLS, ShadowsocksR, SSH); sing-box keeps Hysteria2/TUIC/AnyTLS/SSH and hides ShadowsocksR. A previously selected node that disappears falls back to the first visible node.
- Node cards show:
  - Latency + packet loss;
  - Download / upload;
  - Google exit country, region, and IP;
  - Each card can be tested individually: Google exit, latency + exit, speed.
- Batch buttons:
  - **Test Latency**: runs latency + Google exit for all nodes;
  - **Speed Test All**: runs download/upload for all nodes.
- Sorting:
  - **Latency**: ascending latency, failures / untested at the end;
  - **Download speed**: descending download, untested / failed at the end;
  - **Address & exit**: nodes whose Google exit country matches the node flag are prioritized, then sorted by country, address, and name.
- **Hide Timeout**:
  - Stored in `app_settings` as `hide_timeout`;
  - Enabled: removes cards whose most recent test failed;
  - Failures are read only from the active engine's rows, and switching engines clears the previous engine's in-memory results in the same event. A stale failure from the old core therefore cannot hide a node the new engine can dial;
  - If all cards are removed, a prompt is shown telling the user to turn the option off.

---

## 5. TUN mode permission files on Linux

### 5.1 What permissions are needed

A TUN inbound requires:

- `CAP_NET_ADMIN`: create TUN interfaces, add routes, and take over system DNS/routing;
- `CAP_NET_RAW`: needed by auto-routing (Xray binds outbound traffic to the physical interface) and by sing-box's auto-redirect mode.

Setting only `CAP_NET_ADMIN` is not enough; both must be granted together.

### 5.2 How to grant the permission

The app does not run as root as a whole; instead, it sets Linux file capabilities on the **selected engine's binary**:

```bash
setcap cap_net_admin,cap_net_raw+ep /path/to/xray
setcap cap_net_admin,cap_net_raw+ep /path/to/sing-box
```

- `+ep` means effective + permitted.
- The permission is stored in the file’s extended attribute `security.capability`, not in a standalone config file.
- Because of this, it follows the **specific file path**: after updating or reinstalling the core, the new file has no capability and must be re-granted.
- It remains valid after app restart because the binary itself has not changed.

Granting is automatic: right before every start with TUN enabled, `ensure_tun_capabilities` runs `getcap` on the selected binary. If both capabilities are present with the effective bit, startup continues silently. Otherwise it runs the same `pkexec setcap` command the button uses, so Debian shows the administrator prompt the first time that binary is used and again after a core upgrade (the new version is a different file). Cancelling the prompt or failing to grant aborts the start before the old core is stopped, and the error includes the manual `sudo setcap` command. The manual **Grant TUN permission** button remains available and skips the prompt when the capabilities are already set.

The same capabilities are what let the core remove its own network state on exit: the TUN interface, the routes, and with sing-box's `auto_redirect` the nftables redirect rules. That is why the app asks the core to stop with `SIGTERM` and waits before resorting to a kill (Section 1.8). Killing a TUN-enabled core skips that cleanup; the leftover redirect rules point at a closed port and the machine loses all network connectivity until the rules are removed.

### 5.3 UI flow

Settings → TUN → **Grant TUN permission**:

1. `resolve_core_executable` finds the currently selected binary for the active engine.
2. It looks for `setcap`, `getcap`, and `pkexec` under `/usr/sbin`, `/sbin`, `/usr/bin`, and `/bin`, so a desktop session with a minimal `PATH` still works.
   - If `setcap` is missing, it returns: `setcap was not found. Install the libcap2-bin package first.`
3. It runs:

   ```bash
   pkexec /usr/sbin/setcap cap_net_admin,cap_net_raw+ep <engine-binary>
   ```

4. `pkexec` triggers the desktop polkit agent’s administrator prompt; the app itself does not need root.
   - The project does not install a custom polkit `.policy` file; it relies on `pkexec`’s default `org.freedesktop.policykit.exec` action.
   - Therefore the system must have a usable polkit agent (GNOME, KDE, etc., typically provide one).
5. On success, the UI shows:
   - `TUN permissions granted. Enable TUN mode if needed, then start the core.`
6. On failure, it returns the `pkexec` / `setcap` stderr to the user.
7. If `pkexec` does not exist, it instructs the user to run manually:

   ```bash
   sudo setcap cap_net_admin,cap_net_raw+ep <engine-binary>
   ```

### 5.4 Verifying after granting

Users can check manually:

```bash
getcap /path/to/xray      # or /path/to/sing-box
```

The expected output contains:

```text
cap_net_admin,cap_net_raw+ep
```

Some libcap versions print the same set as `cap_net_admin,cap_net_raw=ep`; both are accepted by the automatic check.

### 5.5 Startup failure message

When the active engine lacks the required capabilities, it usually only reports `operation not permitted` and does not explicitly say the issue is TUN permissions. The app detects:

- the platform is Linux;
- `tun_enabled = true` in settings;
- the error text contains `operation not permitted`.

It then replaces it with a clearer message:

```text
TUN mode needs the CAP_NET_ADMIN and CAP_NET_RAW capabilities. Open Settings → TUN
and click "Grant TUN permission", or run `sudo setcap cap_net_admin,cap_net_raw+ep <path>`.
```

The message names the engine's settings tab and the binary that was launched.

### 5.6 Auto-rollback when settings save fails

Turning on TUN in the TUN page immediately saves the settings:

- If the main core is running, it restarts the core to apply TUN.
- If the restart fails because of insufficient permissions:
  - `apply_settings` writes the previous settings back to the database;
  - it restarts the old config (without TUN) to restore the original connection;
  - the UI shows an error and restores the old values.

This prevents a failed TUN attempt from leaving the user in a state where the setting appears enabled but the core cannot start.

### 5.7 DNS takeover on Linux

In the TUN config:

- If FakeDNS is enabled and `/run/systemd/resolve/stub-resolv.conf` exists, it sets:

  ```json
  "autoSystemDnsToGateway": true
  ```

- This prevents the system resolver from staying on the loopback address and bypassing TUN to query directly.
- If the system is not running systemd-resolved, this option is not requested to avoid breaking the entire TUN startup.
- This option is not requested on non-Linux platforms.

The `autoSystemDnsToGateway` field is Xray-only. In the sing-box config, DNS takeover is expressed differently: the TUN stack receives DNS queries because of the TUN route itself, and a `{"port": [53], "action": "hijack-dns"}` route rule sends them to the sing-box DNS servers configured in the DNS tab.

---

## 6. Settings and config generation

Settings are scoped to the selected core. With no core selected, only **General** (appearance) and **Core Management** are shown; choosing a core adds that engine's tabs. Xray-specific sections live in `settings/xray_settings.rs` and sing-box sections in `settings/sing_box_settings.rs`, so adding options to one engine cannot change the other engine's tabs or rendering. Sections 6.1–6.8 describe Xray; 6.9 describes the sing-box settings surface.

### 6.1 Save and apply flow

Each control in the settings panel builds a new `XraySettings` on `onchange` and then calls:

```text
XraySettingsPanel::persist
  └─ ProxyCoreService::apply_settings(database, previous, next)
       ├─ next.save(database)          // validate first, then write all 41 keys
       ├─ no running core → return “take effect on next start”
       ├─ only test_concurrency changes → return “take effect on next test,” no restart
       └─ other changes while the core is running
            ├─ reload_running_subscription
            │    └─ restart with the same subscription + same selected node
            └─ failure → previous.save + restart with the old config
```

Key points:

- `save` writes **all** setting keys, not just changed entries, to avoid leaving the database in a partially updated state.
- `validate` runs before any write; invalid values are not saved.
- `requires_core_restart` compares fields individually but normalizes `test_concurrency`, so changing it does not restart the main core.
- Restarting disconnects all established proxy connections, because Xray reads config only at startup.
- If starting the new config fails, the old settings are restored and the app retries starting the old configuration; the error message states whether the restoration succeeded.
- The system proxy toggle is not part of `XraySettings`; it is applied immediately through `set_system_proxy_enabled` and does not require a core restart.

The sing-box panel uses a simpler variant with the same guarantees:

```text
SingBoxSettingsPanel::persist
  └─ apply_sing_box_settings(database, previous, next)
       ├─ next.save(database)          // validate first, then write all 38 keys
       ├─ running engine is sing-box → reload_running_subscription
       │    └─ restart with the same subscription + same selected node
       ├─ running engine is xray or nothing → saved only, message says so
       └─ failure → previous.save + restore, combined error message
```

- The panel only restarts the core when the running engine is sing-box; otherwise it saves and reports that the values apply at the next start.
- Saving sing-box settings while Xray is running is safe; they take effect the next time sing-box starts.
- There is no `test_concurrency` equivalent, so every sing-box setting change restarts a running sing-box core.

### 6.2 Setting keys overview

Keys in `app_settings`:

```text
xray_log_level
xray_domain_strategy
xray_bypass_cn
xray_bypass_private
xray_block_ads
xray_block_quic
xray_bypass_list
xray_outbound_rules
xray_sniffing_enabled
xray_sniff_http
xray_sniff_tls
xray_sniff_quic
xray_sniff_route_only
xray_fragment_enabled
xray_fragment_packets
xray_fragment_length
xray_fragment_interval
xray_mux_enabled
xray_mux_concurrency
xray_mux_xudp_concurrency
xray_mux_proxy_udp443
xray_local_socks_port
xray_local_http_port
xray_auto_failover
xray_test_concurrency
xray_ipv6_enabled
xray_local_dns
xray_remote_dns
xray_fake_dns_enabled
xray_fake_dns_pool
xray_fake_dns_pool_size
xray_tun_enabled
xray_tun_name
xray_tun_mtu
xray_tun_gateway
xray_tun_auto_route
xray_tun_strict_route
xray_tun_stack
xray_lan_enabled
xray_lan_username
xray_lan_password
```

### 6.3 General

| Setting                 | Default   | Writes to Xray config                     | Notes                                                                       |
| ----------------------- | --------- | ----------------------------------------- | --------------------------------------------------------------------------- |
| Log level               | `warning` | `log.loglevel`                            | Both generated config and full JSON subscriptions are forced to override it |
| Mux                     | Off       | `mux` on proxy outbounds                  | See below                                                                   |
| Mux concurrency         | `8`       | `mux.concurrency`                         | 1–1024                                                                      |
| XUDP concurrency        | `16`      | `mux.xudpConcurrency`                     | 0 means do not write the XUDP field                                         |
| QUIC (UDP 443) via mux  | `reject`  | `mux.xudpProxyUDP443`                     | `reject` / `allow` / `skip`                                                 |
| Auto failover           | Off       | balancer + observatory                    | See 2.6                                                                     |
| Test concurrency        | `8`       | **Not written**                           | Only controls test process count; saving does not restart the core          |
| SOCKS port              | `2080`    | Local SOCKS inbound `port`                | Preferred value; another free port is used if occupied                      |
| HTTP port               | `2081`    | Local HTTP inbound `port`                 | Must differ from SOCKS                                                      |
| Allow LAN               | Off       | Appends `0.0.0.0` LAN SOCKS/HTTP inbounds | Default ports 2090/2091                                                     |
| LAN username / password | Empty     | LAN inbound `accounts`                    | Both must be non-empty to enable auth; setting only one disables auth       |

Mux generation format:

```json
"mux": {
  "enabled": true,
  "concurrency": 8,
  "xudpConcurrency": 16,
  "xudpProxyUDP443": "reject"
}
```

- Node-link subscriptions: applied to the selected node outbound and to every node outbound in the hot-switch config.
- Full JSON subscriptions: applied only to outbounds whose protocol is `vless` / `vmess` / `trojan` / `shadowsocks` / `socks` / `http` and that do not already contain `mux`.

### 6.4 Routing

| Setting               | Default        | Write location                                | Notes                                                  |
| --------------------- | -------------- | --------------------------------------------- | ------------------------------------------------------ |
| Bypass mainland China | On             | `routing.rules` `direct` rules                | Depends on `geosite.dat` / `geoip.dat`                 |
| Bypass LAN/private    | On             | Same                                          | Depends on geodata                                     |
| Block ads             | Off            | `geosite:category-ads-all` → `block`          | Depends on `geosite.dat`                               |
| Block QUIC (UDP 443)  | Off            | `network: udp`, `port: 443` → `block`         | Intended to run after ad rules and before direct rules |
| Bypass list           | Empty          | `routing.rules` `domain` / `ip` array         | Parsed line by line                                    |
| Outbound rules        | Empty          | `routing.rules`, before the auto-bypass rules | Each line is `<domain-or-ip> <direct\|proxy\|block>`   |
| Domain strategy       | `IPIfNonMatch` | `routing.domainStrategy`                      | Forced to `AsIs` when FakeDNS is active                |

Custom bypass list syntax:

- Regular domain → `domain:<domain>` (suffix match);
- `*.example.com` / `.example.com` → `domain:example.com`;
- IP or CIDR → inserted directly into `ip`;
- `geosite:` / `domain:` / `full:` / `keyword:` / `regexp:` / `geoip:` → passed through as-is;
- Lines starting with `#` or empty lines are ignored.

Outbound rules example:

```text
example.com block
10.0.0.0/8 direct
geosite:google proxy
```

- User rules are placed before auto-bypass rules, so they can override behavior like “bypass mainland China.”
- `proxy` target:
  - Non-hot-switch config → `outboundTag: "proxy"`;
  - Hot-switch config → `balancerTag: "cloak-balancer"`.

Actual routing rule order (when API is present):

```text
1. API inbound → api outbound
2. TUN inbound port 53 → dns-out (TUN only)
3. User outbound rules
4. Block ads
5. Block QUIC
6. Direct domain
7. Direct IP
8. Fallback: all TCP/UDP → cloak-balancer (hot-switch only)
```

The `bypass_cn` / `bypass_private` / `block_ads` rules are generated only when corresponding geodata files exist; otherwise they are skipped automatically to avoid Xray failing to start because `geosite.dat` / `geoip.dat` cannot be found.

### 6.5 DNS

| Setting      | Default         | Write location                                              | Notes                                            |
| ------------ | --------------- | ----------------------------------------------------------- | ------------------------------------------------ |
| Local DNS    | Empty           | scoped resolver in `dns.servers`                            | If empty, `localhost` (system resolver) is used  |
| Remote DNS   | Empty           | `dns.servers`                                               | Supports IP, IP:port, `tcp://`, `https://`, etc. |
| Fake DNS     | Off             | `fakedns` + `dns.servers`                                   | Only truly active when TUN is enabled            |
| Fake IP pool | `198.18.0.0/15` | `fakedns[].ipPool`                                          | IPv4 CIDR                                        |
| Pool size    | `65535`         | `fakedns[].poolSize`                                        | Cache mapping limit                              |
| IPv6         | On              | DNS `queryStrategy`, TUN gateway/routing, FakeDNS IPv6 pool | If off, `UseIPv4`; if on, `UseIP`                |

#### 6.5.1 When the `dns` section is generated

Rules for `dns_config`:

- If both local DNS and remote DNS are empty, FakeDNS is inactive, and IPv6 is enabled → **do not generate a `dns` section**; Xray uses the system default behavior (both address families).
- Otherwise, generate:

  ```json
  {
    "servers": [...],
    "queryStrategy": "UseIP" | "UseIPv4"
  }
  ```

#### 6.5.2 Order of `servers`

1. **Compute scoped local resolver**:
   - If `local_dns` is set, use it;
   - Else if FakeDNS is enabled or `remote_dns` is set, use `"localhost"`;
   - Else `None`.
2. **Add local resolver**:
   - If bypass domains exist (`bypass_domains` not empty), add the scoped resolver as a server with `domains` and `skipFallback: true`;
   - If there are no bypass domains, no remote DNS, and a scoped resolver exists, add it as a regular server.
3. **Add proxy-domain resolver**:
   - FakeDNS active → append `"fakedns"`;
   - Else if `remote_dns` is set → append it.
4. **Final fallback**:
   - If no remote DNS, or FakeDNS is active → append `"localhost"`;
   - If remote DNS is set and FakeDNS is inactive → do not append `localhost`, to avoid leaking proxy-domain queries to the ISP.

Typical combinations:

| local_dns | remote_dns | bypass domains      | `servers`                                                              |
| --------- | ---------- | ------------------- | ---------------------------------------------------------------------- |
| yes       | no         | no                  | `[local, "localhost"]`                                                 |
| yes       | no         | yes                 | `[{address: local, domains: [...], skipFallback: true}, "localhost"]`  |
| yes       | yes        | no                  | `[remote]`                                                             |
| yes       | yes        | yes                 | `[{address: local, domains: [...], skipFallback: true}, remote]`       |
| no        | yes        | yes                 | `[{address: "localhost", domains: [...], skipFallback: true}, remote]` |
| no        | no         | no, IPv6 off        | `["localhost"]`                                                        |
| any       | no         | any, FakeDNS active | `[fakedns, "localhost"]` (plus local scoped if bypass domains exist)   |

Note: when remote DNS is set and there are no bypass domains, `local_dns` does not appear in `servers` because it is only meant to answer bypassed domains.

Design goals:

- Bypassed domains (mainland China, private networks, custom direct connections) are answered by the local resolver to avoid returning an exit-region address;
- Domains sent through the proxy are answered by FakeDNS or the remote resolver to keep them consistent with the exit region;
- Proxy-domain queries do not fall back to the system DNS.

#### 6.5.3 FakeDNS

FakeDNS is activated only when:

```text
tun_enabled = true && fake_dns_enabled = true
```

Generated config:

```json
"fakedns": [
  {"ipPool": "198.18.0.0/15", "poolSize": 65535},
  {"ipPool": "2001:2::/48", "poolSize": 65535}
]
```

- If IPv6 is off, only the IPv4 pool is generated;
- If IPv6 is on, it adds `2001:2::/48`;
- TUN inbound sniffing forcibly includes `fakedns` even if global sniffing is disabled, or else fake addresses cannot be mapped back to domain names;
- `autoSystemDnsToGateway` is only enabled on Linux when `/run/systemd/resolve/stub-resolv.conf` exists.

### 6.6 TUN

| Setting        | Default       | Writes to `inbounds[].settings`                    | Notes                                                                                                |
| -------------- | ------------- | -------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| Enable TUN     | Off           | Controls the entire TUN inbound                    |                                                                                                      |
| Interface name | `cloak0`      | `name`                                             | If empty, Xray chooses it                                                                            |
| MTU            | `1500`        | `mtu`                                              | 576–9000                                                                                             |
| IPv4 gateway   | `10.0.0.1/16` | `gateway[0]`                                       | Must be an IPv4 CIDR                                                                                 |
| IPv6           | On            | `gateway` adds `fc00::1/64`; routing adds IPv6     |                                                                                                      |
| Auto route     | On            | `autoSystemRoutingTable`, `autoOutboundsInterface` |                                                                                                      |
| Strict route   | On            | Splits the default route                           | Only effective when Auto route is enabled                                                            |
| Stack          | `mixed`       | **Not written**                                    | Xray TUN currently has one built-in stack; the value is only reserved for future/other core versions |
| Local DNS      | Empty         | TUN `dns`                                          | Only written when it is an IP literal; mainly relevant on Windows                                    |

TUN inbound structure:

```json
{
  "tag": "cloak-tun-in",
  "protocol": "tun",
  "settings": {
    "name": "cloak0",
    "mtu": 1500,
    "gateway": ["10.0.0.1/16", "fc00::1/64"],
    "autoSystemRoutingTable": ["0.0.0.0/1", "128.0.0.0/1", "::/1", "8000::/1"],
    "autoOutboundsInterface": "auto"
  },
  "sniffing": {
    "enabled": true,
    "destOverride": ["http", "tls", "quic", "fakedns"],
    "metadataOnly": false
  }
}
```

Routing:

- Auto route off: do not write `autoSystemRoutingTable` or `autoOutboundsInterface`.
- Strict route on:
  - IPv4: `0.0.0.0/1` + `128.0.0.0/1`;
  - IPv6: `::/1` + `8000::/1`.
- Strict route off:
  - IPv4: `0.0.0.0/0`;
  - IPv6: `::/0`.
- Splitting the default route is more specific than the existing `0.0.0.0/0`, so it is not affected by the original route metric and it avoids traffic bypassing TUN.
- `autoOutboundsInterface: "auto"` prevents Xray’s own outbound traffic from being re-routed back into TUN and creating a loop.

TUN routing rule:

```json
{
  "type": "field",
  "inboundTag": ["cloak-tun-in"],
  "port": 53,
  "outboundTag": "dns-out"
}
```

### 6.7 Advanced

#### 6.7.1 Sniffing

| Setting         | Default | Writes                                      |
| --------------- | ------- | ------------------------------------------- |
| Enable sniffing | On      | `sniffing` on local SOCKS/HTTP/LAN inbounds |
| HTTP            | On      | `destOverride` includes `http`              |
| TLS             | On      | `destOverride` includes `tls`               |
| QUIC            | On      | `destOverride` includes `quic`              |
| Route only      | Off     | `sniffing.routeOnly = true`                 |

Generated form:

```json
"sniffing": {
  "enabled": true,
  "destOverride": ["http", "tls", "quic"],
  "metadataOnly": false
}
```

- `sniffing` is written only when sniffing is enabled.
- When TUN is active and FakeDNS is enabled, sniffing is forced on and `fakedns` is appended to `destOverride`.
- Full JSON subscriptions do not automatically get sniffing appended to their inbounds; only the app-generated TUN inbound does.

#### 6.7.2 Fragment

| Setting         | Default    | Writes                                                               |
| --------------- | ---------- | -------------------------------------------------------------------- |
| Enable fragment | Off        | `sockopt.dialerProxy` on proxy outbounds + trailing freedom outbound |
| Packets         | `tlshello` | `fragment.packets`                                                   |
| Length          | `100-200`  | `fragment.length`                                                    |
| Interval        | `10-20`    | `fragment.interval`                                                  |

Generation flow:

1. For all outbounds whose protocol is `vless` / `vmess` / `trojan` / `shadowsocks` / `socks` / `http`:
   - If `streamSettings.sockopt.dialerProxy` already exists, keep the subscription’s own value;
   - Otherwise write `"dialerProxy": "cloak-fragment"`.
2. Append at the end of the outbound array:

   ```json
   {
     "tag": "cloak-fragment",
     "protocol": "freedom",
     "settings": {
       "fragment": {
         "packets": "tlshello",
         "length": "100-200",
         "interval": "10-20"
       }
     }
   }
   ```

- It is placed at the end so it never becomes the default outbound; it can only be called via `dialerProxy`.
- Full JSON subscriptions also receive the same fragmentation logic.

### 6.8 Mapping matrix for full JSON subscriptions

| Setting / capability                           | Full JSON subscription                                                         |
| ---------------------------------------------- | ------------------------------------------------------------------------------ |
| Log level                                      | Forced application                                                             |
| Local ports                                    | Keep the subscription’s inbound ports; generate them only if no inbound exists |
| LAN                                            | Append a pair of LAN inbounds                                                  |
| TUN                                            | Append the TUN inbound, `dns-out`, and DNS 53 rule                             |
| DNS / FakeDNS                                  | Append only when the subscription has no `dns` section                         |
| Mux                                            | Append to proxy outbounds without `mux`                                        |
| Fragment                                       | Append                                                                         |
| Routing / bypass / block ads / domain strategy | Keep the subscription’s own routing without overriding                         |
| Auto failover                                  | Not applied                                                                    |
| Hot switch                                     | Not generated                                                                  |
| Stats API                                      | Appended if there is no `api` conflict                                         |
| Test core                                      | Only rebinds inbound ports                                                     |

### 6.9 sing-box settings

sing-box options live in `src/services/sing_box_settings.rs` and are stored under their own `sing_box_*` keys. They are loaded, validated, and saved independently of `XraySettings`, so an Xray default or key can never change a sing-box value and vice versa.

`ProxyCoreService` uses them whenever sing-box is the selected engine: `sing_box_config::prepare_with_settings` turns the subscription plus these values into `runtime/sing-box-config.json`. Saving a change while sing-box is running restarts it with the same subscription and node; saving while Xray is running (or nothing is running) stores the values without touching the current core, and they apply at the next sing-box start. The panel hint and the status message say which of those happened, and the header reads “sing-box settings apply immediately to the running core.”

How the tabs map to the generated config:

| Tab      | Generated config                                                                                                              |
| -------- | ----------------------------------------------------------------------------------------------------------------------------- |
| General  | `log`, the loopback `mixed` inbound, the LAN `mixed` inbound with optional auth; IPv6 affects DNS strategy and AAAA handling   |
| Routing  | `route.rules` + the final outbound: `rules` / `global` / `direct`, CN and ad rule-sets, reject actions for ads and QUIC, custom rules, and `selector` vs `urltest` for the node group. `route.default_domain_resolver` always points at the local resolver so outbound server names resolve without looping through the proxy |
| DNS      | `dns.servers`, `dns.final`, `dns.strategy`, the `fakeip` server and its ranges, plus a `local` bootstrap server when the local resolver is a hostname |
| TUN      | a `tun` inbound with `address`, `mtu`, `stack`, `auto_route`, `strict_route`, `auto_redirect`                                  |
| Advanced | inbound sniffing and the `sniff` / `override` route actions, outbound `multiplex` and `tcp_fast_open`, `experimental.cache_file` |

| Tab       | Options                                                                                                                                         |
| --------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| General   | Log level, log timestamps, IPv6, mixed (SOCKS+HTTP) port, LAN access, LAN port, LAN username/password                                           |
| Routing   | Routing mode (`rules` / `global` / `direct`), auto failover, bypass mainland China, bypass LAN/private, block ads, block QUIC, custom rules       |
| DNS       | DNS strategy, local resolver, remote resolver, FakeDNS with IPv4/IPv6 pools                                                                     |
| TUN       | Enable TUN, interface name, MTU, IPv4 address, stack, auto route, strict route, auto redirect                                                    |
| Advanced  | Sniffing and destination override, multiplex protocol / max connections / padding, TCP Fast Open, cache file                                    |

Defaults: `info` logs with timestamps; IPv6 on; mixed port `2080`; LAN off at `2090`; routing mode `rules`; auto failover off; bypass China and private networks on; ads and QUIC off; no custom rules; DNS strategy `prefer_ipv4` with empty resolvers; FakeDNS off (`198.18.0.0/15`, `fc00::/18`); TUN off (`cloak0`, MTU `1500`, `172.19.0.1/30`, stack `mixed`, auto route on, strict route and auto redirect off); sniffing on with override; mux off (`h2mux`, 4 connections, padding off); TCP Fast Open and cache file off.

Validation runs before any write:

- log level and routing mode must be one of the listed values;
- mixed and LAN ports must be non-zero and different;
- custom rules are `<domain-or-ip> <direct|proxy|block>`, one per line, with `#` comments allowed;
- DNS strategy must be listed, and a non-empty resolver must be an IP, `IP:port`, or a supported DNS URL;
- with FakeDNS enabled, both pools must be IPv4/IPv6 CIDR respectively;
- with TUN enabled, the name must have no spaces, MTU must be 576–9000, the address must be an IPv4 CIDR, and the stack must be listed;
- the multiplex protocol must be listed and the connection count must be 1–1024.

`save` validates first and then writes all 37 keys, so a partially saved state is impossible. `load` filters corrupt stored values back to their defaults instead of failing. The panel rebuilds `SingBoxSettings` on each `onchange`, saves it, and reports a status message: “Saved and applied” when a running sing-box core was restarted, “Saved” when the values will apply at the next start, and the restore outcome when a restart failed.

---

## 7. Save, restart, and effective matrix

| Setting category                                 | Save to DB | Requires core restart to take effect | Notes                                                        |
| ------------------------------------------------ | ---------- | ------------------------------------ | ------------------------------------------------------------ |
| Log level                                        | Yes        | Yes                                  | Both engines read config only at startup                     |
| Mux / XUDP / UDP443                              | Yes        | Yes                                  | Xray: mux / XUDP / UDP443; sing-box: multiplex / TFO         |
| Auto failover                                    | Yes        | Yes                                  |                                                              |
| Test concurrency                                 | Yes        | **No**                               | Xray only; not part of the Xray config                       |
| Local SOCKS/HTTP ports                           | Yes        | Yes                                  | Xray uses a SOCKS+HTTP pair; sing-box uses one mixed port. Preferred on next startup; a free port is chosen if occupied |
| LAN on/off / credentials                         | Yes        | Yes                                  |                                                              |
| Routing / bypass / ads / QUIC                    | Yes        | Yes                                  |                                                              |
| Domain strategy                                  | Yes        | Yes                                  |                                                              |
| Local / remote DNS                               | Yes        | Yes                                  |                                                              |
| FakeDNS                                          | Yes        | Yes                                  | Only effective when TUN is enabled                           |
| IPv6                                             | Yes        | Yes                                  |                                                              |
| TUN on/off and parameters                        | Yes        | Yes                                  | Auto-rollback on failure                                     |
| Sniffing                                         | Yes        | Yes                                  |                                                              |
| Fragment                                         | Yes        | Yes                                  |                                                              |
| TUN permission                                   | No         | No                                   | Directly modifies binary capabilities                        |
| System proxy toggle                              | No         | No                                   | Updates GNOME settings immediately                           |
| Theme / language / Service Access / Hide Timeout | Yes        | No                                   | UI preferences                                               |

When the core is not running: all settings are saved only; no core startup occurs.

sing-box follows the same rule with one difference: there is no test-concurrency exception, so every saved change restarts a running sing-box core. Saving sing-box settings while Xray is running stores them only, and selecting a different engine stops the running core, so the two panels never apply changes to each other's process. Switching the engine in Core Management stops the current core and saves its node under that engine, but does not start the other engine until the user presses Start; the equivalent selector in the Proxies header restarts it automatically when a core was running.

---

## 8. Known limitations and current implementation status

1. **Only one main core can run at a time.** Starting a new subscription stops the old core first.
2. **The Dashboard Rule / Global buttons are presentational only** and have no bound logic; the Global mode is not currently implemented.
3. **`tun_stack` is saved but not written to the Xray config.** Xray TUN currently uses the built-in network stack.
4. **Full Xray JSON subscriptions do not support hot-switching.** Clicking a node card restarts the core, but the config contents do not change.
5. **When auto failover is enabled, manual node selection has no effect.** Xray's balancer uses observatory `leastPing`; sing-box uses a `urltest` group. Both choose the exit themselves.
6. **Speed-test results are not persisted.** Latency / packet loss / Google exit are persisted; download / upload exist only in memory.
7. **System proxy support is Linux + GNOME only.**
8. **Geo rules depend on manually downloaded geodata.** When the relevant files are missing (`geosite.dat` / `geoip.dat` for Xray, `geosite-cn.srs` and friends for sing-box), those rules are skipped instead of erroring.
9. **TUN capability is tied to the specific binary file.** After updating the core, the permission must be re-granted.
10. **DNS Leak detection is heuristic**: it compares resolver country to exit country; when geolocation is unknown, only `Mixed` can be reported.
11. **Network checks depend on third-party services** (Google, Cloudflare, ipwho.is, IPPure, ipleak.net, BrowserLeaks). If those services are unreachable, the checks fail but the proxy itself is unaffected.
12. **The temp core for full JSON subscriptions only rebinds ports** (Xray only). If the subscription itself includes TUN or other special inbounds, the temp core may require extra conditions; it does not remove those inbounds automatically.
13. **Mux / fragment logic is still appended for full Xray JSON subscriptions**, but the subscription’s own `dialerProxy` takes precedence.
14. **Port settings are preferred values**; the actual startup port may change if blocked, and the UI and system proxy always use the actual port.
15. **Each engine keeps its own last node.** Switching engines saves the running node under the old engine and restores the new engine's saved node, falling back to the legacy selection and then to the first node that engine can dial. Changing a subscription URL clears both engine selections.

---

## 9. Key code index

| Feature                                                               | File                                                         |
| --------------------------------------------------------------------- | ------------------------------------------------------------ |
| Core lifecycle, engine selection, startup/stop/switching, TUN permission, traffic stats | `src/services/proxy_core.rs`                                 |
| Xray config generation, routing, DNS, FakeDNS, TUN, balancer          | `src/services/xray_config.rs`                                |
| Settings structure, validation, persistence, restart detection        | `src/services/xray_settings.rs`                              |
| sing-box config generation, selector/urltest, routing, DNS, TUN, `.srs` rules | `src/services/sing_box_config.rs`                     |
| sing-box settings structure, validation, persistence                  | `src/services/sing_box_settings.rs`                          |
| Subscription parsing, node key, base64, country flags                 | `src/services/subscription_manager.rs`                       |
| Core download, version list, geo data                                 | `src/services/core_manager.rs`, `src/services/geo_assets.rs` |
| GNOME system proxy                                                    | `src/services/system_proxy.rs`                               |
| Latency / speed / Google exit                                         | `src/services/proxy_test.rs`                                 |
| DNS Leak, IPPure                                                      | `src/services/network_test.rs`                               |
| Service Access                                                        | `src/services/service_access.rs`                             |
| Database schema, node test results, per-engine node selection       | `src/services/database.rs`                                   |
| Network page                                                          | `src/components/network.rs`                                  |
| Proxy Nodes page, test scheduling                                     | `src/components/proxies.rs`                                  |
| Settings UI                                                           | `src/components/settings/xray_settings.rs`                   |
| sing-box settings UI                                                  | `src/components/settings/sing_box_settings.rs`               |
| Dashboard connection cards / mode toggles / connectivity tests        | `src/components/dashboard/`                                  |
| App-level state and result ownership                                  | `src/app.rs`                                                 |
