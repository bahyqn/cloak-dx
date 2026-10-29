# Cloak DX 设计文档

> 版本：基于当前工作树（2026-10-29）
>
> 本文只描述**代码实际实现的行为**，包括已知限制。涉及代码位置时使用仓库相对路径，便于直接定位。

---

## 0. 总览

### 0.1 技术栈与进程模型

- UI：Rust + Dioxus 0.7 桌面应用（`#[component]`、`use_signal`、`use_context`、`use_resource`、`spawn_forever` 等）。
- 代理核心：不内嵌 Xray，而是从 GitHub Releases 下载官方 **Xray-core** 二进制，作为子进程运行。
- 数据存储：SQLite（`cloak-dx.sqlite3`），保存订阅、核心安装记录、节点测试结果和全部应用设置。
- 同一时间只允许一个主核心运行。主核心是一个 `xray run -c <config>` 子进程，由 `ProxyCoreService` 持有。
- 测速/延迟测试会额外启动**临时核心**（每个被测节点一个），测试完成后立即杀掉并删除临时文件，不影响主核心。

### 0.2 数据目录

统一由 `data_directory()` 决定，Linux 上为：

```text
$XDG_DATA_HOME/cloak-dx        # 通常 ~/.local/share/cloak-dx
$XDG_DATA_HOME/cloak-dx/cloak-dx.sqlite3
$XDG_DATA_HOME/cloak-dx/cores/<engine>/<channel>/<version>/xray
$XDG_DATA_HOME/cloak-dx/geo/geosite.dat
$XDG_DATA_HOME/cloak-dx/geo/geoip.dat
$XDG_DATA_HOME/cloak-dx/geo/geoip-only-cn-private.dat
$XDG_DATA_HOME/cloak-dx/runtime/xray-config.json
$XDG_DATA_HOME/cloak-dx/runtime/xray.log
$XDG_DATA_HOME/cloak-dx/runtime/xray-test-<pid>-<nanos>-<seq>.json
$XDG_DATA_HOME/cloak-dx/runtime/xray-test-<pid>-<nanos>-<seq>.log
$XDG_DATA_HOME/cloak-dx/runtime/system-proxy-backup.json
```

- `cores/`：核心管理页下载的二进制，目录层级为引擎 / 渠道 / 版本。
- `geo/`：所有 Xray 实例共享的 GeoIP / GeoSite 数据，通过环境变量 `XRAY_LOCATION_ASSET` 指向它。
- `runtime/`：运行期文件，包括当前配置、日志、临时测试配置和系统代理备份。
- 主核心启动时会截断 `xray.log`；日志页只读取最后 500 行（且最多 256 KiB）。

### 0.3 页面结构

| 页面 | 作用 |
| --- | --- |
| Dashboard | 启动/停止核心、订阅与节点信息、TUN/系统代理开关、连接性测试、实时流量 |
| Proxy Nodes | 节点卡片、延迟/出口/下载测速、排序、失败隐藏、节点选择 |
| Subscribes | 订阅管理、刷新、节点数量与流量信息 |
| Network | 针对当前运行节点的出口、IPPure、Service Access、DNS Leak 检查 |
| Logs | 应用日志 + 核心日志尾部 |
| Settings | 外观；Xray General / Routing / DNS / TUN / Advanced；核心管理 |

---

## 1. 如何启用 Xray 代理

### 1.1 前置条件

1. 在 **Settings → Core Management** 安装至少一个 Xray-core 版本。
2. 在 **Subscribes** 添加并刷新订阅，并选中一个订阅。
3. （可选）在 Core Management 下载 Xray geodata。没有 geosite/geoip 文件时，依赖地理数据的规则会被自动跳过，而不是让核心启动失败。

订阅内容支持两类：

- **节点链接列表**：`vless://`、`vmess://`、`trojan://`、`ss://`、`socks://`、`http://` 等，可被 base64 包裹。
- **完整 Xray JSON 配置**：包含 `outbounds` 数组且其中元素带 `protocol` 字段。此类订阅按“整份配置”处理，详见 1.7。

### 1.2 启动入口与完整流程

入口：Dashboard 的 Start 按钮 → `ProxyCoreService::start_selected_subscription` → `start_subscription`。

流程如下（`src/services/proxy_core.rs`）：

```text
Start
 └─ 从数据库取当前选中的订阅（没有则报错）
 └─ resolve_xray_executable：优先使用 selected 的 Xray 安装，否则用第一个 Xray 安装
 └─ 检查订阅 raw_config 非空
 └─ XraySettings::load + validate()
 └─ prepare_with_settings(...) 预检
      · 使用设置里的回环端口和 LAN 默认端口生成一次配置
      · 此时不启动进程，只验证配置能被生成
 └─ stop() 停止旧核心（并恢复系统代理）
 └─ 分配实际端口
 └─ prepare_with_settings(...) 生成最终配置
 └─ 写入 runtime/xray-config.json
 └─ 启动 xray run -c <config>
      · 设置 XRAY_LOCATION_ASSET=<data>/geo
      · stdout/stderr 都重定向到 runtime/xray.log
 └─ wait_until_running：300ms 内进程不能退出
 └─ wait_for_listening：每个需要监听的端口最多等 5s
      · SOCKS 端口
      · LAN SOCKS 端口（如启用）
      · Xray API 端口（如启用）
 └─ 通过 API 把 balancer 指向所选节点
 └─ 若“系统代理”开启且 TUN 关闭，则写入 GNOME 系统代理
 └─ 返回 ProxyCoreStatus
```

关键点：

- **先验证，后停止旧核心**。预检失败不会打断当前连接。
- 端口在预检之后分配，因为预检只需要确认配置结构有效；最终端口可能因占用而变化。
- 配置不是增量修改，而是**每次启动都从订阅原文 + 当前全部设置重新生成**。
- 主核心启动时会尽量启用热切换（见第 2 节），因此会额外分配一个回环 API 端口。

### 1.3 端口分配

默认值（`src/services/xray_settings.rs`）：

| 用途 | 默认值 | 监听地址 |
| --- | --- | --- |
| 本地 SOCKS | `2080` | `127.0.0.1` |
| 本地 HTTP | `2081` | `127.0.0.1` |
| LAN SOCKS | `2090` | `0.0.0.0` |
| LAN HTTP | `2091` | `0.0.0.0` |
| Xray API | 临时端口 | `127.0.0.1` |

分配规则（`available_port` / `available_lan_port`）：

1. 优先尝试设置中的端口（LAN 使用固定默认 2090/2091，而不是用户输入，因为 LAN 端口没有单独的设置项）。
2. 如果被占用，就在回环或被 `0.0.0.0` 上绑定 `0` 端口，最多重试 20 次。
3. 所有已分配端口互相避让（SOCKS、HTTP、LAN SOCKS、LAN HTTP、API）。
4. LAN 端口必须在 `0.0.0.0` 上可绑定，仅在回环可用的端口不算。
5. 实际使用的端口写入 `ProxyCoreStatus`，系统代理和 UI 都读取实际值。

注意：设置里的 SOCKS/HTTP 端口是“首选值”。如果启动时被占用，核心会改用其他端口；下次启动仍会重新尝试用户设置的值。

### 1.4 核心启动后的代理入口

启动成功后，同一个 Xray 进程同时提供：

- 本地 SOCKS5（含 UDP）`127.0.0.1:<socks_port>`；
- 本地 HTTP `127.0.0.1:<http_port>`；
- 可选的 LAN SOCKS/HTTP `0.0.0.0:<lan_port>`；
- 可选的 TUN 接口；
- 回环 API（`dokodemo-door`，仅用于 `xray api` 和统计）。

应用自身的所有网络检查（Network 页、延迟/测速、Service Access）都通过本地 SOCKS 入口访问，使用 `socks5h://127.0.0.1:<port>`，因此 DNS 由节点侧解析，测到的是“经过代理后网站看到的行为”。

### 1.5 系统代理（GNOME）

实现：`src/services/system_proxy.rs`。

- 默认启用（`ProxyCoreService::new` 中 `system_proxy = true`），且**不持久化**；应用重启后恢复为开启。
- 启动核心时，如果系统代理开关开启且 **TUN 未启用**，则修改 GNOME 的 `gsettings`：
  - `org.gnome.system.proxy mode = manual`
  - `use-same-proxy = false`
  - HTTP / HTTPS / FTP 指向 `127.0.0.1:<http_port>`
  - SOCKS 指向 `127.0.0.1:<socks_port>`
- 修改前先把原始值保存到 `runtime/system-proxy-backup.json`；如果写入过程中失败，会立即尝试恢复。
- 停止核心、关闭窗口或核心进程自行退出时，都会尝试恢复备份并删除备份文件。
- TUN 模式下不会再设置系统代理，因为 TUN 已经接管系统流量，二者叠加只会多一跳。
- 目前只支持 Linux + GNOME。其他平台调用会返回错误。
- 系统代理失败不会导致核心启动失败；错误作为 `ProxyCoreStatus.warning` 显示在 Dashboard。

### 1.6 TUN 模式

TUN 是 Xray 的入站，不是应用层代理：

- 配置启用后，`base_config` 会增加 `tag: "cloak-tun-in"` 的 TUN 入站。
- 增加 `dns-out`（`protocol: "dns"`）出站，并增加一条路由规则：TUN 入站的 53 端口 → `dns-out`。
- 可选地增加系统 DNS 接管、FakeDNS、IPv6 网关与路由。
- TUN 模式下不写系统代理。

具体字段映射见 6.4。

### 1.7 完整 Xray JSON 订阅的特殊处理

当订阅是完整 Xray JSON 时，应用不解析单个节点，而是把整份配置作为基础：

- 强制覆盖 `log.loglevel` 为设置中的日志级别。
- 如果配置已有 `inbounds`：
  - 保留订阅自己的入站和端口；
  - 从中读取 SOCKS / HTTP 端口用于状态展示和系统代理；
  - 如果启用了 LAN，则在末尾追加一对 `0.0.0.0` 的 LAN SOCKS/HTTP 入站。
- 如果配置没有 `inbounds`：
  - 插入应用生成的回环 SOCKS/HTTP 入站（以及可选 LAN 入站）。
- 如果启用了 TUN：追加 TUN 入站、`dns-out`、DNS 53 规则；仅当订阅没有自己的 `dns` 段时，才追加应用生成的 DNS/FakeDNS。
- 如果启用了 Mux：给协议为代理协议且尚未包含 `mux` 的出站追加 mux。
- 如果启用了分片：给代理出站追加 `dialerProxy`，并在末尾追加 `cloak-fragment` 出站。
- 如果配置没有 `api` 段、也没有占用 `api` 标签：追加 API 入站、API 出站、路由规则、`stats` 和统计策略。
- **不会生成 balancer，也不会启用热切换。**
- 订阅自己的路由、DNS、出站和端口保持原样。

因此，完整 JSON 订阅下：

- 流量统计可用（如果没有 API 冲突）。
- 节点卡片上的“切换节点”不会真正改变订阅内部的出站选择；点击节点会走重启兜底，但重启后仍是同一份 JSON 配置。
- 延迟/测速的临时核心只重绑定入站端口，不改变订阅自身的路由/DNS/出站。

需要注意：如果完整 JSON 自带 `inbounds`，但没有 SOCKS 或 HTTP 入站，那么 `PreparedConfig` 里的 `socks_port` / `http_port` 会是 `None`。此时：

- Network 页会认为“没有可用的 SOCKS 端口”，所有检查不可用；
- 系统代理仍会按默认的 2080 / 2081 写入，但订阅可能根本没有监听这两个端口；
- 应用自身的探测也无法通过 SOCKS 入口工作。

这类订阅应自行提供一个本地 SOCKS/HTTP 入站，或改为节点链接列表格式。

### 1.8 停止与异常处理

- `stop()`：
  - kill 主核心进程并 wait；
  - 删除 `runtime/xray-config.json`；
  - 清空流量采样基线；
  - 如果系统代理是应用写入的，恢复原设置。
- 状态轮询发现子进程已退出：
  - 清理运行状态和配置文件；
  - 如果系统代理已应用，则恢复；
  - Dashboard 回到未运行状态。
- 关闭窗口时通过 `use_wry_event_handler` 在事件循环退出前调用 `stop()`，避免系统代理残留。
- 启动失败时：
  - 读取 `xray.log` 尾部并附在错误里；
  - 删除临时配置；
  - 如果是 TUN 权限错误，替换为可操作提示（见第 5 节）。

---

## 2. 节点切换的热更新

热更新的目标是：切换节点时不重启 Xray 进程，不断开已有连接。

### 2.1 节点标识

节点 key 由订阅解析器生成（`src/services/subscription_manager.rs`）：

```text
<protocol长度>:<protocol>:<name长度>:<name>:<address>
```

例如：

```text
5:vless:8:🇭🇰 HK 01:hk1.example.com
```

长度前缀用于避免名称或协议中的分隔符造成歧义。数据库只保存这个 key，节点内容始终从订阅原文重新解析。

### 2.2 生成的热切换配置

仅对**节点链接列表**的订阅，且启动时传入了 API 端口，才会生成热切换配置（`src/services/xray_config.rs`）：

- 每个可构建节点生成一个出站，tag 为：

  ```text
  cloak-node-0
  cloak-node-1
  ...
  ```

- 所有节点共用一个 balancer：

  ```json
  {
    "tag": "cloak-balancer",
    "selector": ["cloak-node-"]
  }
  ```

- 增加一条兜底路由，把所有未被前面规则匹配的 TCP/UDP 流量交给 balancer：

  ```json
  {
    "type": "field",
    "network": "tcp,udp",
    "balancerTag": "cloak-balancer"
  }
  ```

- 增加回环 API 入站：

  ```json
  {
    "tag": "api",
    "listen": "127.0.0.1",
    "port": <api_port>,
    "protocol": "dokodemo-door",
    "settings": {"address": "127.0.0.1"}
  }
  ```

- 增加 `api` 出站、`api` 路由规则、`api.services`、`stats` 和 `policy.system` 统计计数器。

- 出站过滤：`outbound_is_unbuildable` 会跳过以下节点，避免一个坏节点导致整份配置无法启动：
  - `streamSettings.network` 为 `quic` 或 `http`（Xray 26 已移除这些传输）；
  - `security = reality` 且没有 `publicKey`；
  - `vless` / `trojan` 没有 TLS/Reality，且服务器地址是公网 IP。

### 2.3 启动时把 balancer 对准所选节点

启动流程在 API 端口开始监听后执行：

```bash
xray api bo -s 127.0.0.1:<api_port> -b cloak-balancer cloak-node-N
```

对应 `override_balancer`。如果这一步失败，整个启动会失败并 kill 子进程，因为此时用户选择的节点没有生效，继续运行会走错出口。

如果 `auto_failover` 开启，则不会记录 `HotSwitch` 元数据，也不会执行这条命令；balancer 由 observatory 的 `leastPing` 策略接管。

### 2.4 用户点击节点时的切换

流程（`src/components/proxies.rs` + `src/services/proxy_core.rs`）：

1. 点击节点卡片，先调用 `select_subscription_node(subscription_id, node_key)` 写入数据库。
2. 如果主核心正在运行，且正在服务的订阅就是这张卡片所属订阅，则后台调用 `switch_to_node`。
3. `switch_to_node` 先尝试 `switch_running_node`：
   - 从运行态取出 `HotSwitch` 元数据（API 端口、balancer 名、节点 tag 列表）；
   - 按 key 找到目标 tag；
   - 如果目标 tag 已经是当前 tag，直接返回当前状态，不执行命令；
   - 否则执行：

     ```bash
     xray api bo -s 127.0.0.1:<api_port> -b cloak-balancer cloak-node-N
     ```

   - 成功后更新内存中的 `selected_tag` 和 `node_name`，不重启进程。
4. 如果无法热切换，则 `reload_running_subscription`：
   - 用同一个订阅和新的选中节点重新生成配置；
   - 停止旧进程并启动新进程；
   - 返回 `restarted = true`。
5. UI 根据 `restarted` 显示不同 toast：
   - `Node switched.`：热更新成功；
   - `Node switched. This subscription format restarts the core.`：走了重启兜底。

### 2.5 何时不能热切换

以下情况会走重启兜底，或直接不可用：

- 订阅是完整 Xray JSON（没有 `HotSwitch`）。
- 运行中的配置没有 API 端口。
- 点击的节点不在 `hot.nodes` 列表中（例如被 `outbound_is_unbuildable` 过滤，或该协议无法转换为出站）。
- `auto_failover` 开启：配置里故意不记录 `HotSwitch`，手动选择不生效；点击节点会触发重启兜底，但重启后 balancer 仍由 observatory 决定。
- 核心未运行。
- 核心正在运行另一个订阅（`switch_to_node` 会报 `The proxy core is running a different subscription.`）。

### 2.6 自动故障转移

当 `auto_failover = true` 且配置带 API 时：

- balancer 增加：

  ```json
  "strategy": {"type": "leastPing"}
  ```

- 增加 observatory：

  ```json
  {
    "subjectSelector": ["cloak-node-"],
    "probeUrl": "https://www.google.com/generate_204",
    "probeInterval": "60s",
    "enableConcurrency": true
  }
  ```

- Xray 每 60 秒并发探测所有节点，balancer 自动选择延迟最低且可达的节点；某个节点停止响应时自动移走流量。
- 手动节点选择被忽略；UI 仍显示订阅里选中的节点名，但实际出口由 observatory 决定。

### 2.7 流量统计

- 主核心启动时始终请求 API（节点链接订阅）。
- Dashboard 的流量卡通过 `xray api statsquery -s 127.0.0.1:<api_port>` 读取累计计数器。
- 只统计 `inbound>>>...>>>traffic>>>uplink/downlink`，并排除 `api` 入站自身。
- 两次采样之差除以时间得到 B/s；第一次采样只建立基线，显示为 0。
- 没有 API（例如完整 JSON 存在冲突）时，流量卡显示空闲。

---

## 3. Network 页面功能

Network 页的所有检查都针对**当前正在运行的核心**，通过它的 SOCKS 入站发出，因此结果描述的是实际服务流量的那条连接，而不是另起的测试实例。

### 3.1 页面状态与入口

- 顶部显示当前节点名和 `SOCKS 127.0.0.1:<port>`。
- 核心未运行时显示横幅 “Start the proxy core first”，所有测试按钮禁用。
- 顶部 **Test** 按钮同时启动四类检查：
  - Google Exit；
  - IPPure；
  - DNS Leak；
  - Service Access（含 Google Ping）。
- 每个卡片有自己的 **Test / Re-test** 按钮。
- 任意检查运行中，顶部按钮显示旋转图标并禁用。
- 结果放在应用级 signal 中，切换到其他页面再回来不会丢失；正在运行的检查也不会因离开页面而取消。
- 当“选中的订阅 + 选中的节点 key”变化时，所有 Network 结果会被清空，避免把上一个节点的结果显示在新节点下。
- 迟到的检查结果会检查 `owner`；如果期间节点已经变化，结果被丢弃，不会写回。

### 3.2 Google Exit

实现：`network_test::test_google_exit` → `proxy_test::probe_google_exit`。

- 通过当前核心的 SOCKS 客户端访问。
- 并行执行：
  - Google 搜索页的位置页脚 / “my location”答案卡；
  - YouTube 页面中的 Google 国家判断；
  - 访问 `https://www.google.com/` 时重定向到的国家域名（如 `google.com.hk`）；
  - Cloudflare `cdn-cgi/trace` 获取出口 IP；
  - `ipwho.is` 作为出口 IP 和地理信息的兜底。
- 国家优先级：Google 重定向国家 > YouTube 国家 > 搜索页答案卡。
- 城市 / 地区只在与 Google 判定的国家一致时使用；Google 页脚给出的地区例外保留，因为它反映 Google 自己的 GeoIP 结果。
- 卡片展示：
  - 出口 IP；
  - Google 国家；
  - 城市 / 地区。
- 这是“尽力而为”的探测：地理服务失败时，延迟测试仍可正常返回。

### 3.3 IPPure

实现：`network_test::test_ippure`。

- 先用 Cloudflare trace 获取出口 IP。
- 依次请求 IPPure：
  - `/api/info/ip-basic/<ip>`：ASN、组织、国家、城市、是否住宅等；
  - `/api/info/ip-risk/<ip>`：风险分（尽力而为）；
  - `/api/info/asn/botclass/<asn>`：人类 / 机器人比例（尽力而为）。
- IPPure 使用签名挑战：
  - 第一次请求返回响应头 `x-k`（签名 key）和 `x-t`（时间戳）；
  - 第二次请求带 `x-k` 和 `x-t: <stamp>-<signature>`；
  - 签名算法为 HMAC-SHA256(key, `GET-<url>--<stamp>`)；
  - 两次请求需要走同一条 TCP 连接，依靠 reqwest 连接池保持。
- 风险分展示（0–100）：
  - 0–15：极度纯净
  - 16–25：纯净
  - 26–40：中性
  - 41–50：轻度风险
  - 51–70：中度风险
  - 71–100：极度风险
- IP 类型：
  - `isp` 或住宅标记：住宅 IP
  - `hosting` / `business`：机房 IP
  - `enterprise`：商业宽带
  - `education`：教育 IP
  - `government`：政府 IP
- 地址性质：`is_broadcast = true` 显示“广播 IP”，否则“原生 IP”。
- 卡片还展示 WARP 标记、ASN、位置、人类/机器人比例。

### 3.4 Service Access

实现：`src/services/service_access.rs`。

- 支持 8 个服务：Google、YouTube、Gemini、Netflix、GitHub、ChatGPT、Disney+、Telegram。
- 页面提供 **3 个槽位**，每个槽位一个下拉框。不能重复选择同一个服务；把槽位改为空会从选择中移除。
- 选择持久化在 `app_settings` 的 `service_access` 键，格式为逗号分隔的 id，例如：

  ```text
  google,youtube,netflix
  ```

- Dashboard 也使用同一份选择，因此两侧显示和测试的服务一致。
- 每个服务通过当前核心的 SOCKS 访问固定 URL，15 秒超时；任何 2xx 视为可达，记录毫秒延迟和 HTTP 状态。
- Google Ping 独立执行 `https://www.gstatic.com/generate_204`，即使 Google 不在三个服务中也会显示结果。
- 所有选中的服务和 Google Ping 并发执行。

### 3.5 DNS Leak

实现：`network_test::test_dns_leak`。

- 提供两个检测站点：
  - **ipleak.net**（默认）
  - **BrowserLeaks**
- 所有探测域名都通过当前核心的 SOCKS 解析（`socks5h`），因此检测站点看到的是这条代理连接真正使用的解析器。
- ipleak.net：
  - 生成一个 40 字符 session；
  - 并发请求 3 个 `<session>-<1..3>.ipleak.net/dnsdetection/`；
  - 合并返回的解析器；
  - 通过 `ipleak.net/json/<ip>` 补充国家、地点、组织。
- 如果 ipleak.net 没有返回解析器（例如解析器对 `*.ipleak.net` 返回了通配缓存），自动回退到 BrowserLeaks，并在结果中标记 `fallback = BrowserLeaks`。
- BrowserLeaks：
  - 两个随机 IPv4 子域 `*.dns4.browserleaks.net`；
  - 一个随机 IPv6 子域 `*.dns6.browserleaks.net`；
  - 按解析器 IP 汇总。
- 判定规则依赖“出口国家”：
  - 出口国家优先取 Google Exit 的国家；
  - 其次取 IPPure 的国家；
  - 最后从当前节点名称中的国旗 emoji 推导。
- 判定：
  - 不知道出口国家 → `Mixed`（无法判断）；
  - 所有解析器都没有地理信息 → `Mixed`；
  - 所有有地理信息的解析器都与出口国家一致 → `Protected`；
  - 任意一个有地理信息的解析器与出口国家不一致 → `Leaking`。
- UI 展示解析器 IP、国家、地点、组织，以及结果横幅。
- Dashboard 的 DNS 行读取同一份结果：
  - `Protected` → `Safe (No Leak)` / `Clean`
  - `Mixed` → `Unknown` / `Unknown`
  - `Leaking` → `Leaking` / `Polluted`

### 3.6 结果归属与并发

- 每个检查都有独立的 `running` signal；顶部按钮把它们合并成一个“任意运行中”状态。
- `run_*` 函数使用 `spawn_forever`，测试不绑定页面作用域。
- 启动时捕获 `owner`（订阅 id + 节点 key），完成后再次比较；不一致就丢弃结果。
- 结果重置逻辑在 `src/app.rs`，由 `data_revision` 触发。

---

## 4. Xray 如何在本地测速、测试下载速度

测速不经过主核心，而是为每个被测节点启动一个**临时 Xray 实例**。这样：

- 不干扰正在运行的代理；
- 可以并发测试多个节点；
- 每个节点使用独立的实际配置，测到的结果更接近真实使用。

### 4.1 临时核心的创建

`ProxyCoreService::start_temporary_core`：

1. 解析所选 Xray 二进制。
2. 检查订阅原文非空。
3. 分配两个临时回环端口：SOCKS 和 HTTP。
4. 读取当前设置，调用 `prepare_for_test`。
5. 生成唯一文件名：

   ```text
   runtime/xray-test-<pid>-<unix_nanos>-<sequence>.json
   runtime/xray-test-<pid>-<unix_nanos>-<sequence>.log
   ```

6. 启动 `xray run -c <temp-config>`，设置 `XRAY_LOCATION_ASSET`。
7. 等待进程稳定并等待 SOCKS 端口监听。
8. 返回 `TemporaryCore`；其 `Drop` 会 kill 进程、wait，并删除临时配置和日志。

### 4.2 测试配置与主配置的差异

对于节点链接订阅，`prepare_for_test` 使用 `test_settings` 派生设置：

| 选项 | 测试核心行为 |
| --- | --- |
| mux | 保留（节点不支持 mux 时必须测出失败） |
| fragment | 保留（DPI 规避是真实连接的一部分） |
| sniffing | 保留 |
| 本地 / 远程 DNS | 清空 |
| FakeDNS | 关闭 |
| 自定义绕过列表 | 清空 |
| 绕过中国大陆 / 私有网络 | 关闭 |
| TUN | 关闭 |
| 自动故障转移 | 关闭 |
| LAN | 关闭 |
| API / balancer | 不生成 |
| 路由 | 固定为空规则 + `domainStrategy: AsIs` |

原因：

- 测试只关心延迟和吞吐，不需要 Geo 路由；
- 每个测试核心都加载 geosite/geoip 会让最多 30 个临时核心同时消耗大量内存；
- TUN 需要额外权限，测试不应依赖它；
- 保留 mux / fragment / sniffing，避免“测试很快但主核心实际连不上”的假阳性。

对于完整 Xray JSON 订阅，`prepare_test_config` 只做端口重绑定：

- 第一个 SOCKS 入站改用临时 SOCKS 端口；
- 第一个 HTTP 入站改用临时 HTTP 端口；
- 其他带端口的入站改成随机空闲端口；
- 如果没有 SOCKS/HTTP 入站，则补上；
- 订阅自身的路由、DNS、出站保持不变。

### 4.3 延迟测试

`proxy_test::test_latency`：

1. 为该节点启动一个临时核心。
2. 创建 `socks5h://127.0.0.1:<temp_socks>` 的 reqwest 客户端，节点级超时 10 秒。
3. 同时启动 Google Exit 探测（与延迟请求并行，节省一次核心启动）。
4. 顺序发送 **4 次** `GET https://www.gstatic.com/generate_204`：
   - 单次请求超时 4 秒；
   - 成功且状态码为 2xx 才计入样本；
   - 记录每次耗时。
5. 计算：
   - `milliseconds` = 成功样本的平均值（整数毫秒）；
   - `packet_loss_percent` = `(4 - 成功数) * 100 / 4`；
   - 如果 4 次全部失败，返回最后一次错误。
6. Google Exit 结果作为 `LatencyTestResult.google_exit` 一起返回；出口探测失败不影响延迟结果。
7. 临时核心在函数结束时 drop，进程被清理。

为什么是 gstatic 204：比 Google 首页快很多，首页在某些节点上可能超过 10 秒。

### 4.4 下载速度测试

`proxy_test::test_speed`：

1. 为该节点启动一个临时核心。
2. 创建 SOCKS 客户端，整体超时 30 秒。
3. 请求：

   ```text
   https://speed.cloudflare.com/__down?bytes=26214400
   ```

4. 流式读取响应块，满足任一条件即停止：
   - 已采样 **8 秒**；
   - 已下载 **25 MiB**；
   - 服务器结束响应；
   - 读取出错（如果已经读到数据，则保留已下载量）。
5. 计算下载速率：

   ```text
   download_MiB_per_s = downloaded_bytes / 1_048_576 / elapsed_seconds
   ```

6. 上传测试（尽力而为）：
   - `POST https://speed.cloudflare.com/__up`；
   - 请求体 1 MiB 零字节；
   - 读取完整响应；
   - 计算 MiB/s。
   - 上传失败时 `upload_mbps = None`，不会让整个测速失败。
7. 临时核心 drop 后清理。

设计意图：

- 不用“下载完整 25 MiB”作为成功条件，否则慢节点会在超时前什么也报不出来；
- 固定 8 秒采样窗口可以在慢节点上仍然得到有意义的吞吐；
- 25 MiB 上限防止极快节点在 8 秒内下载过多数据。

### 4.5 测速并发控制

设置项 **Node Testing → Test concurrency**：

- 默认 8；
- 范围 1–30；
- 每次测试开始时读取一次；
- `worker_count = min(节点数, 并发数).max(1)`；
- 每个 worker 是一个独立 OS 线程，内部创建 `tokio::runtime::Builder::new_current_thread()`；
- 所有 worker 从共享 `VecDeque` 中取节点；
- 结果通过 `std::sync::mpsc` 通道送回 UI 线程；
- 每个被测节点仍然启动自己的临时核心，所以并发数直接决定同时存在的 Xray 进程数。

该设置**不进入生成的 Xray 配置**，因此保存它不会重启主核心（见第 6 节）。

### 4.6 结果持久化与展示

- 延迟测试：
  - 成功：`node_test_results.latency_ms` 更新，`latency_failed = 0`，`packet_loss` 更新；
  - 失败：`latency_ms = NULL`，`latency_failed = 1`；
  - 同一次测试的 Google Exit 成功时，单独更新 `exit_ip` / `exit_country_code` / `exit_country` / `exit_region`。
- 手动“Test Google exit”只更新出口字段，不动延迟。
- 速度测试结果**只存在内存**，不写入数据库；重启应用后消失。
- Proxies 页启动时读取数据库中的延迟和出口结果，因此重启应用后仍能看到上次测试的延迟/丢包/出口。
- 节点卡片：
  - 延迟 + 丢包；
  - 下载 / 上传；
  - Google exit 国家、地区、IP；
  - 每个卡片可单独测试：Google exit、延迟+出口、速度。
- 批量按钮：
  - **Test Latency**：所有节点跑延迟 + Google exit；
  - **Speed Test All**：所有节点跑下载/上传。
- 排序：
  - **Latency**：延迟升序，失败/未测排最后；
  - **Download speed**：下载降序，未测/失败排最后；
  - **Address & exit**：Google exit 国家与节点国旗一致者优先，然后按国家、地址、名称排序。
- **Hide Timeout**：
  - 持久化在 `app_settings` 的 `hide_timeout`；
  - 开启后移除最近一次测试失败的卡片；
  - 如果全部被移除，显示提示让用户关闭该开关。

---

## 5. Linux 下 TUN 模式的授权文件

### 5.1 需要什么权限

Xray 的 TUN 入站需要：

- `CAP_NET_ADMIN`：创建 TUN 接口、添加路由、接管系统 DNS/路由；
- `CAP_NET_RAW`：自动路由模式下 Xray 需要把出站绑定到物理接口（例如 `SO_BINDTODEVICE`）。

仅设置 `CAP_NET_ADMIN` 不够；授权时必须两个一起给。

### 5.2 授权方式

应用不整体以 root 运行，而是给 **Xray 二进制文件**设置 Linux file capabilities：

```bash
setcap cap_net_admin,cap_net_raw+ep /path/to/xray
```

- `+ep` 表示 effective + permitted。
- 权限保存在文件的扩展属性 `security.capability` 上，不是单独的配置文件。
- 因此它跟随**具体文件路径**：更新/重新下载核心后，新文件没有 capability，需要重新授权。
- 应用重启后仍然有效，因为二进制文件没变。

### 5.3 UI 操作流程

Settings → TUN → **Grant TUN permission**：

1. `resolve_xray_executable` 找到当前选中的 Xray 二进制。
2. 在 `/usr/sbin/setcap` 和 `/sbin/setcap` 中查找 `setcap`。
   - 找不到时返回：`setcap was not found. Install the libcap2-bin package first.`
3. 执行：

   ```bash
   pkexec /usr/sbin/setcap cap_net_admin,cap_net_raw+ep <xray-path>
   ```

4. `pkexec` 触发桌面 polkit 代理的管理员授权框；应用自身不需要 root。
   - 项目没有安装自定义的 polkit `.policy` 文件，使用的是 `pkexec` 默认的 `org.freedesktop.policykit.exec` 动作；
   - 因此系统需要存在可用的 polkit 代理（GNOME、KDE 等桌面环境通常自带）。
5. 成功时 UI 显示：
   - `TUN permissions granted. Enable TUN mode if needed, then start the core.`
6. 失败时把 `pkexec` / `setcap` 的 stderr 返回给用户。
7. `pkexec` 不存在时提示手动执行：

   ```bash
   sudo setcap cap_net_admin,cap_net_raw+ep <xray-path>
   ```

### 5.4 授权后的验证

用户可以手动检查：

```bash
getcap /path/to/xray
```

期望包含：

```text
cap_net_admin,cap_net_raw+ep
```

### 5.5 启动失败时的提示

Xray 在缺少 capability 时通常只报 `operation not permitted`，不会说明是 TUN 权限。应用会检测：

- 平台是 Linux；
- 设置里 `tun_enabled = true`；
- 错误文本包含 `operation not permitted`。

然后替换为明确提示：

```text
TUN mode needs the CAP_NET_ADMIN and CAP_NET_RAW capabilities. Open Settings → TUN
and click "Grant TUN permission", or run `sudo setcap cap_net_admin,cap_net_raw+ep <path>`.
```

### 5.6 设置保存失败时的自动回滚

在 TUN 页打开 TUN 开关会立即保存设置：

- 如果主核心正在运行，会重启核心以应用 TUN。
- 如果重启因权限不足失败：
  - `apply_settings` 会把之前的设置重新写回数据库；
  - 再启动一次旧配置（不带 TUN），恢复原连接；
  - UI 显示错误并恢复旧值。

因此用户不会因为一次失败的 TUN 尝试而停留在“设置是开着的、核心却起不来”的状态。

### 5.7 Linux 上的 DNS 接管

TUN 配置中：

- 如果启用 FakeDNS，且 `/run/systemd/resolve/stub-resolv.conf` 存在，会设置：

  ```json
  "autoSystemDnsToGateway": true
  ```

- 这样系统解析器不再停留在回环地址，避免它绕过 TUN 直接查询。
- 如果系统没有运行 systemd-resolved，则不会请求该选项，避免整个 TUN 启动失败。
- 非 Linux 平台不会请求这个选项。

---

## 6. 设置参数如何更新到 Xray 配置

### 6.1 保存与应用路径

设置面板中的每个控件在 `onchange` 时构造一份新的 `XraySettings`，然后调用：

```text
XraySettingsPanel::persist
  └─ ProxyCoreService::apply_settings(database, previous, next)
       ├─ next.save(database)          // 先校验，再写全部 41 个键
       ├─ 没有运行中的核心 → 返回“下次启动生效”
       ├─ 只有 test_concurrency 变化 → 返回“下次测试生效”，不重启
       └─ 其他变化且核心运行中
            ├─ reload_running_subscription
            │    └─ 用同一订阅 + 同一选中节点重新 start_subscription
            └─ 失败 → previous.save + 用旧设置重启
```

要点：

- `save` 会写入**全部**设置键，而不是只写变化项，避免数据库处于部分更新状态。
- `validate` 在任何写入前执行；非法值不会保存。
- `requires_core_restart` 逐字段比较，但把 `test_concurrency` 归一化，因此它变化时不重启主核心。
- 重启会断开所有已建立的代理连接，这是 Xray 只能启动时读取配置导致的。
- 启动新配置失败时，旧设置会被恢复并再次尝试启动；错误信息会说明恢复是否成功。
- 系统代理开关不属于 `XraySettings`，由 `set_system_proxy_enabled` 立即应用，不需要重启核心。

### 6.2 设置键一览

`app_settings` 中的键：

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

| 设置 | 默认值 | 写入 Xray 配置的位置 | 说明 |
| --- | --- | --- | --- |
| Log level | `warning` | `log.loglevel` | 生成配置和完整 JSON 订阅都会被强制覆盖 |
| Mux | 关闭 | 代理出站的 `mux` | 见下 |
| Mux concurrency | `8` | `mux.concurrency` | 1–1024 |
| XUDP concurrency | `16` | `mux.xudpConcurrency` | 0 表示不写 XUDP 字段 |
| QUIC (UDP 443) via mux | `reject` | `mux.xudpProxyUDP443` | `reject` / `allow` / `skip` |
| Auto failover | 关闭 | balancer + observatory | 见 2.6 |
| Test concurrency | `8` | **不写入** | 仅控制测试进程数；保存不重启核心 |
| SOCKS port | `2080` | 本地 SOCKS 入站的 `port` | 首选值，占用时改用空闲端口 |
| HTTP port | `2081` | 本地 HTTP 入站的 `port` | 必须与 SOCKS 不同 |
| Allow LAN | 关闭 | 追加 `0.0.0.0` 的 LAN SOCKS/HTTP 入站 | 默认端口 2090/2091 |
| LAN username / password | 空 | LAN 入站的 `accounts` | 两者都非空才启用认证；只填一个视为不认证 |

Mux 生成形式：

```json
"mux": {
  "enabled": true,
  "concurrency": 8,
  "xudpConcurrency": 16,
  "xudpProxyUDP443": "reject"
}
```

- 节点链接订阅：应用到所选节点出站，以及热切换配置中的每一个节点出站。
- 完整 JSON 订阅：只应用到协议为 `vless` / `vmess` / `trojan` / `shadowsocks` / `socks` / `http` 且原本没有 `mux` 字段的出站。

### 6.4 Routing

| 设置 | 默认值 | 写入位置 | 说明 |
| --- | --- | --- | --- |
| Bypass mainland China | 开 | `routing.rules` 中 `direct` 规则 | 依赖 `geosite.dat` / `geoip.dat` |
| Bypass LAN/private | 开 | 同上 | 依赖 geodata |
| Block ads | 关 | `geosite:category-ads-all` → `block` | 依赖 `geosite.dat` |
| Block QUIC (UDP 443) | 关 | `network: udp`, `port: 443` → `block` | 目的在 ad 规则之后、直连规则之前 |
| Bypass list | 空 | `routing.rules` 的 `domain` / `ip` 数组 | 逐行解析 |
| Outbound rules | 空 | `routing.rules`，位于自动绕过规则之前 | 每行 `<domain-or-ip> <direct\|proxy\|block>` |
| Domain strategy | `IPIfNonMatch` | `routing.domainStrategy` | FakeDNS 激活时强制 `AsIs` |

自定义绕过列表语法：

- 普通域名 → `domain:<domain>`（后缀匹配）；
- `*.example.com` / `.example.com` → `domain:example.com`；
- IP 或 CIDR → 原样放入 `ip`；
- `geosite:` / `domain:` / `full:` / `keyword:` / `regexp:` / `geoip:` → 原样透传；
- `#` 开头或空行忽略。

出站规则：

```text
example.com block
10.0.0.0/8 direct
geosite:google proxy
```

- 用户规则排在自动绕过规则之前，因此可以覆盖“绕过中国大陆”等行为。
- `proxy` 目标：
  - 非热切换配置 → `outboundTag: "proxy"`；
  - 热切换配置 → `balancerTag: "cloak-balancer"`。

路由规则的实际顺序（有 API 时）：

```text
1. API 入站 → api 出站
2. TUN 入站 53 端口 → dns-out（仅 TUN）
3. 用户 outbound rules
4. 屏蔽广告
5. 屏蔽 QUIC
6. 直连域名
7. 直连 IP
8. 兜底：所有 TCP/UDP → cloak-balancer（仅热切换）
```

只有存在对应 geodata 文件时，`bypass_cn` / `bypass_private` / `block_ads` 才生成规则；否则自动跳过，避免 Xray 因找不到 `geosite.dat` / `geoip.dat` 而拒绝启动。

### 6.5 DNS

| 设置 | 默认值 | 写入位置 | 说明 |
| --- | --- | --- | --- |
| Local DNS | 空 | `dns.servers` 的 scoped resolver | 为空时用 `localhost`（系统解析器） |
| Remote DNS | 空 | `dns.servers` | 支持 IP、IP:port、`tcp://`、`https://` 等 |
| Fake DNS | 关 | `fakedns` + `dns.servers` | 只有 TUN 开启时才真正生效 |
| Fake IP pool | `198.18.0.0/15` | `fakedns[].ipPool` | IPv4 CIDR |
| Pool size | `65535` | `fakedns[].poolSize` | 缓存映射上限 |
| IPv6 | 开 | DNS `queryStrategy`、TUN 网关/路由、FakeDNS IPv6 池 | 关闭时 `UseIPv4`，开启时 `UseIP` |

#### 6.5.1 `dns` 段何时生成

`dns_config` 的规则：

- 如果本地 DNS、远程 DNS 都为空、FakeDNS 未激活、且 IPv6 开启 → **不生成 `dns` 段**，Xray 使用系统默认行为（两种地址族）。
- 其他情况生成：

  ```json
  {
    "servers": [...],
    "queryStrategy": "UseIP" | "UseIPv4"
  }
  ```

#### 6.5.2 `servers` 的顺序

1. **计算 scoped 本地解析器**：
   - 如果设置了 `local_dns`，使用它；
   - 否则，如果启用了 FakeDNS 或设置了 `remote_dns`，使用 `"localhost"`；
   - 否则为 `None`。
2. **加入本地解析器**：
   - 如果存在绕过域名（`bypass_domains` 非空），把 scoped 解析器作为带 `domains` 和 `skipFallback: true` 的服务器加入；
   - 如果没有绕过域名、没有远程 DNS，且 scoped 解析器存在，则把它作为普通服务器加入。
3. **代理域名解析器**：
   - FakeDNS 激活 → 追加 `"fakedns"`；
   - 否则如果设置了 `remote_dns` → 追加它。
4. **最终兜底**：
   - 如果没有远程 DNS，或 FakeDNS 激活 → 追加 `"localhost"`；
   - 如果设置了远程 DNS 且 FakeDNS 未激活 → 不追加 `localhost`，避免代理域名查询泄漏到本机 ISP。

由此产生的几种典型组合：

| local_dns | remote_dns | 绕过域名 | `servers` |
| --- | --- | --- | --- |
| 有 | 无 | 无 | `[local, "localhost"]` |
| 有 | 无 | 有 | `[{address: local, domains: [...], skipFallback: true}, "localhost"]` |
| 有 | 有 | 无 | `[remote]` |
| 有 | 有 | 有 | `[{address: local, domains: [...], skipFallback: true}, remote]` |
| 无 | 有 | 有 | `[{address: "localhost", domains: [...], skipFallback: true}, remote]` |
| 无 | 无 | 无，IPv6 关闭 | `["localhost"]` |
| 任意 | 无 | 任意，FakeDNS 激活 | `[fakedns, "localhost"]`（有绕过域名时还会有 local scoped） |

注意：在设置了远程 DNS 且没有绕过域名时，`local_dns` 不会出现在 `servers` 中，因为它的作用只是回答被绕过的域名。

设计目标：

- 被绕过的域名（中国大陆、私有网络、自定义直连）由本地解析器回答，避免拿到出口地区的地址；
- 走代理的域名由 FakeDNS 或远程解析器回答，保证与出口地区一致；
- 不让代理域名回落到系统 DNS。

#### 6.5.3 FakeDNS

只有满足以下条件才激活：

```text
tun_enabled = true && fake_dns_enabled = true
```

生成：

```json
"fakedns": [
  {"ipPool": "198.18.0.0/15", "poolSize": 65535},
  {"ipPool": "2001:2::/48", "poolSize": 65535}
]
```

- IPv6 关闭时只生成 IPv4 池；
- IPv6 开启时追加 `2001:2::/48`；
- TUN 入站的 sniffing 会强制包含 `fakedns`，即使全局 sniffing 关闭，否则假地址无法映射回域名；
- TUN 的 `autoSystemDnsToGateway` 只在 Linux + systemd-resolved stub 存在时启用。

### 6.6 TUN

| 设置 | 默认值 | 写入 `inbounds[].settings` | 说明 |
| --- | --- | --- | --- |
| Enable TUN | 关 | 控制整个 TUN 入站 | |
| Interface name | `cloak0` | `name` | 空字符串时不写，由 Xray 选择 |
| MTU | `1500` | `mtu` | 576–9000 |
| IPv4 gateway | `10.0.0.1/16` | `gateway[0]` | 必须是 IPv4 CIDR |
| IPv6 | 开 | `gateway` 追加 `fc00::1/64`；路由追加 IPv6 | |
| Auto route | 开 | `autoSystemRoutingTable`、`autoOutboundsInterface` | |
| Strict route | 开 | 拆分默认路由 | 仅在 Auto route 开启时生效 |
| Stack | `mixed` | **不写入** | Xray TUN 目前只有一个内置栈，值仅为未来/其他核心保留 |
| Local DNS | 空 | TUN `dns` | 只有值是 IP 字面量时才写入；该字段主要对 Windows 有效 |

TUN 入站结构：

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

路由：

- Auto route 关闭：不写 `autoSystemRoutingTable`，也不写 `autoOutboundsInterface`。
- Strict route 开启：
  - IPv4：`0.0.0.0/1` + `128.0.0.0/1`；
  - IPv6：`::/1` + `8000::/1`。
- Strict route 关闭：
  - IPv4：`0.0.0.0/0`；
  - IPv6：`::/0`。
- 拆分默认路由比现有的 `0.0.0.0/0` 更具体，因此不受原路由 metric 影响，能避免流量绕过 TUN。
- `autoOutboundsInterface: "auto"` 防止 Xray 自己的出站流量被重新路由进 TUN 形成环路。

TUN 路由规则：

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

| 设置 | 默认值 | 写入 |
| --- | --- | --- |
| Enable sniffing | 开 | 本地 SOCKS/HTTP/LAN 入站的 `sniffing` |
| HTTP | 开 | `destOverride` 包含 `http` |
| TLS | 开 | `destOverride` 包含 `tls` |
| QUIC | 开 | `destOverride` 包含 `quic` |
| Route only | 关 | `sniffing.routeOnly = true` |

生成形式：

```json
"sniffing": {
  "enabled": true,
  "destOverride": ["http", "tls", "quic"],
  "metadataOnly": false
}
```

- 只有启用 sniffing 时才写 `sniffing` 字段。
- TUN 在 FakeDNS 激活时会强制打开 sniffing，并追加 `fakedns` 到 `destOverride`。
- 完整 JSON 订阅的入站不会自动追加 sniffing；只有应用生成的 TUN 入站会。

#### 6.7.2 Fragment

| 设置 | 默认值 | 写入 |
| --- | --- | --- |
| Enable fragment | 关 | 代理出站的 `sockopt.dialerProxy` + 末尾 freedom 出站 |
| Packets | `tlshello` | `fragment.packets` |
| Length | `100-200` | `fragment.length` |
| Interval | `10-20` | `fragment.interval` |

生成方式：

1. 对所有协议属于 `vless` / `vmess` / `trojan` / `shadowsocks` / `socks` / `http` 的出站：
   - 如果 `streamSettings.sockopt.dialerProxy` 已存在，保留订阅自己的值；
   - 否则写入 `"dialerProxy": "cloak-fragment"`。
2. 在出站数组末尾追加：

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

- 放在末尾是为了让它永远不会成为默认出站；它只能通过 `dialerProxy` 被调用。
- 完整 JSON 订阅也会执行同样的分片附加逻辑。

### 6.8 完整 JSON 订阅下的映射矩阵

| 设置 / 能力 | 完整 JSON 订阅 |
| --- | --- |
| 日志级别 | 强制应用 |
| 本地端口 | 保留订阅入站端口；没有入站时才生成 |
| LAN | 追加一对 LAN 入站 |
| TUN | 追加 TUN 入站、`dns-out`、DNS 53 规则 |
| DNS / FakeDNS | 仅在订阅没有 `dns` 段时追加 |
| Mux | 追加到没有 `mux` 的代理出站 |
| Fragment | 追加 |
| Routing / bypass / block ads / domain strategy | 保留订阅自己的路由，不覆盖 |
| 自动故障转移 | 不应用 |
| 热切换 | 不生成 |
| 流量统计 API | 如果没有 `api` 冲突则追加 |
| 测试核心 | 只重绑定入站端口 |

---

## 7. 保存、重启与生效矩阵

| 设置类别 | 保存到 DB | 需要重启核心才能生效 | 备注 |
| --- | --- | --- | --- |
| 日志级别 | 是 | 是 | Xray 只在启动时读配置 |
| Mux / XUDP / UDP443 | 是 | 是 | |
| 自动故障转移 | 是 | 是 | |
| 测试并发数 | 是 | **否** | 不进入 Xray 配置 |
| 本地 SOCKS/HTTP 端口 | 是 | 是 | 下次启动优先使用；占用时自动换端口 |
| LAN 开关 / 凭据 | 是 | 是 | |
| 路由 / 绕过 / 广告 / QUIC | 是 | 是 | |
| 域名策略 | 是 | 是 | |
| 本地/远程 DNS | 是 | 是 | |
| FakeDNS | 是 | 是 | 只有 TUN 开启时生效 |
| IPv6 | 是 | 是 | |
| TUN 开关及参数 | 是 | 是 | 失败时自动回滚 |
| Sniffing | 是 | 是 | |
| Fragment | 是 | 是 | |
| TUN 授权 | 否 | 否 | 直接改二进制 capability |
| 系统代理开关 | 否 | 否 | 立即改 GNOME 设置 |
| 主题 / 语言 / Service Access / Hide Timeout | 是 | 否 | UI 偏好 |

核心未运行时：所有设置只保存，不启动核心。

---

## 8. 已知限制与实现现状

1. **同一时间只有一个主核心**。启动新订阅会先停止旧核心。
2. **Dashboard 的 Rule / Global 按钮是展示性的**，没有绑定处理逻辑；当前不实现 Global 模式。
3. **`tun_stack` 只保存，不写入 Xray 配置**。Xray 的 TUN 目前使用内置网络栈。
4. **完整 Xray JSON 订阅不支持热切换**。节点卡片点击会重启，但配置内容不会改变。
5. **自动故障转移开启时手动选择节点不生效**。balancer 由 observatory 的 `leastPing` 决定。
6. **测速结果不持久化**。延迟/丢包/Google exit 持久化，下载/上传只存在内存。
7. **系统代理仅支持 Linux + GNOME**。
8. **Geo 规则依赖手动下载的 geodata**。没有 `geosite.dat` / `geoip.dat` 时相关规则会被跳过，而不是报错。
9. **TUN capability 绑定具体二进制文件**。更新核心后需要重新授权。
10. **DNS Leak 判定是启发式的**：基于解析器国家与出口国家比较；未知地理信息时只能给出 `Mixed`。
11. **Network 检查依赖第三方服务**（Google、Cloudflare、ipwho.is、IPPure、ipleak.net、BrowserLeaks），这些服务不可达时会显示失败，不影响代理本身。
12. **完整 JSON 的测试核心只重绑定端口**。如果订阅自身包含 TUN 等特殊入站，测试核心也可能需要额外条件；测试核心不会主动移除这些入站。
13. **完整 JSON 的 mux / fragment 仍会被追加**，但订阅自己的 `dialerProxy` 优先。
14. **端口设置是首选值**，实际启动端口可能因占用而变化；UI 和系统代理始终使用实际端口。

---

## 9. 关键代码索引

| 功能 | 文件 |
| --- | --- |
| 核心生命周期、启动/停止/切换、TUN 授权、流量统计 | `src/services/proxy_core.rs` |
| Xray 配置生成、路由、DNS、FakeDNS、TUN、balancer | `src/services/xray_config.rs` |
| 设置结构、校验、持久化、重启判定 | `src/services/xray_settings.rs` |
| 订阅解析、节点 key、base64、国家旗标 | `src/services/subscription_manager.rs` |
| 核心下载、版本列表、geo 数据 | `src/services/core_manager.rs`、`src/services/geo_assets.rs` |
| GNOME 系统代理 | `src/services/system_proxy.rs` |
| 延迟/速度/Google exit | `src/services/proxy_test.rs` |
| DNS Leak、IPPure | `src/services/network_test.rs` |
| Service Access | `src/services/service_access.rs` |
| 数据库 schema、节点测试结果 | `src/services/database.rs` |
| Network 页 | `src/components/network.rs` |
| Proxy Nodes 页、测试调度 | `src/components/proxies.rs` |
| 设置 UI | `src/components/settings/xray_settings.rs` |
| Dashboard 连接卡片 / 模式开关 / 连接性测试 | `src/components/dashboard/` |
| 应用级状态与结果归属 | `src/app.rs` |
