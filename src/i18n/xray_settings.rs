//! Simplified Chinese strings for the Xray settings panel (General, Routing,
//! DNS, TUN and Advanced sections).

pub(crate) const MESSAGES: &[(&str, &str)] = &[
    // Section headings
    ("Xray-core · General", "Xray-core · 通用"),
    ("Xray-core · Routing", "Xray-core · 路由"),
    ("Xray-core · DNS", "Xray-core · DNS"),
    ("Xray-core · TUN", "Xray-core · TUN"),
    ("Xray-core · Advanced", "Xray-core · 高级"),
    // Panel header and save status
    (
        "Changes apply immediately and restart the running core.",
        "更改会立即生效，并重启正在运行的核心。",
    ),
    (
        "Changes are saved now and apply the next time a core starts.",
        "更改会立即保存，并在下次启动核心时生效。",
    ),
    (
        "Applying changes to the running core...",
        "正在将更改应用到运行中的核心...",
    ),
    ("Saving...", "正在保存..."),
    // General · Logging
    ("Logging", "日志"),
    ("Log level", "日志级别"),
    (
        "Verbosity written to the core log",
        "写入核心日志的详细程度",
    ),
    // General · Multiplexing
    ("Multiplexing", "多路复用"),
    (
        "Multiplex connections on the proxy outbound",
        "在代理出栈上多路复用连接",
    ),
    ("Mux concurrency", "Mux 并发数"),
    (
        "Maximum simultaneous multiplexed streams",
        "同时多路复用的最大流数量",
    ),
    ("XUDP concurrency", "XUDP 并发数"),
    (
        "UDP sessions multiplexed over one connection. 0 disables XUDP.",
        "通过单个连接多路复用的 UDP 会话数。0 表示禁用 XUDP。",
    ),
    ("QUIC (UDP 443) via mux", "通过 Mux 的 QUIC（UDP 443）"),
    (
        "Reject asks apps to fall back to TCP, allow tunnels QUIC, skip bypasses mux for it",
        "拒绝会让应用回退到 TCP，允许会通过代理转发 QUIC，跳过则对该流量不使用 Mux",
    ),
    ("Reject", "拒绝"),
    ("Allow", "允许"),
    ("Skip mux", "跳过 Mux"),
    // General · Node Selection
    ("Node Selection", "节点选择"),
    ("Auto failover", "自动故障转移"),
    (
        "Probe every node and keep traffic on the fastest reachable one; manual node selection is ignored",
        "探测所有节点，并始终使用最快且可用的节点；此时手动选择节点不会生效",
    ),
    // General · Local Ports
    ("Local Ports", "本地端口"),
    ("SOCKS port", "SOCKS 端口"),
    (
        "Loopback port used by apps and the system proxy",
        "应用和系统代理使用的回环端口",
    ),
    ("HTTP port", "HTTP 端口"),
    ("Must differ from the SOCKS port", "必须与 SOCKS 端口不同"),
    // General · Local Network Access
    ("Local Network Access", "局域网访问"),
    ("Allow LAN connections", "允许局域网连接"),
    (
        "Let other devices on this network use this machine as a proxy",
        "允许本网络中的其他设备将此机器用作代理",
    ),
    ("Username", "用户名"),
    (
        "Leave both this and the password empty to accept connections without authentication",
        "用户名和密码均留空时，将接受无需认证的连接",
    ),
    ("Password", "密码"),
    (
        "Must be set together with the username",
        "必须与用户名同时设置",
    ),
    ("No password", "无密码"),
    (
        "Only one of the two fields is filled in, so authentication stays off. Set both a username and a password to require them.",
        "两个字段中只有一个已填写，因此认证仍处于关闭状态。同时设置用户名和密码后才会要求认证。",
    ),
    (
        "LAN clients must send this username and password (SOCKS5 user/pass or HTTP basic auth).",
        "局域网客户端必须发送此用户名和密码（SOCKS5 用户名/密码或 HTTP 基本认证）。",
    ),
    (
        "Without a username and password, anyone who can reach this machine on the network can use the proxy. Only enable this on a trusted network.",
        "未设置用户名和密码时，网络中的任何人都能访问此机器并使用代理。请仅在可信网络上启用。",
    ),
    ("LAN proxy ports", "局域网代理端口"),
    (
        "Point other devices at this machine's LAN address with these ports",
        "让其他设备使用这些端口连接此机器的局域网地址",
    ),
    (
        "The next core start prefers these ports, and picks a free one if they are busy",
        "下次启动核心时会优先使用这些端口；若端口被占用，则自动选择空闲端口",
    ),
    ("LAN IP", "局域网 IP"),
    (
        "Could not detect this machine's LAN address. Use its network IP with the ports above.",
        "无法检测到此机器的局域网地址。请使用其网络 IP 和上述端口。",
    ),
    (
        "Local applications keep using the loopback ports without a password, so the system proxy and built-in tests are unaffected.",
        "本地应用仍会使用回环端口且无需密码，因此系统代理和内置测试不受影响。",
    ),
    // Routing · Bypass
    ("Bypass", "绕过"),
    ("Bypass mainland China", "绕过中国大陆"),
    (
        "Send Chinese domains and IPs direct",
        "中国大陆的域名和 IP 直连",
    ),
    (
        "Bypass LAN and private networks",
        "绕过局域网和私有网络",
    ),
    ("Keep local addresses off the proxy", "本地地址不经过代理"),
    ("Block ads", "屏蔽广告"),
    (
        "Reject geosite:category-ads-all",
        "拒绝 geosite:category-ads-all",
    ),
    ("Block QUIC (UDP 443)", "屏蔽 QUIC (UDP 443)"),
    (
        "Make QUIC-capable apps fall back to TCP so the routing rules see their traffic",
        "让支持 QUIC 的应用回退到 TCP，以便路由规则能够识别其流量",
    ),
    // Routing · Custom bypass
    ("Custom bypass", "自定义绕过"),
    ("Bypass list", "绕过列表"),
    (
        "Domains and IPs that connect directly, one entry per line",
        "直连的域名和 IP，每行一条",
    ),
    (
        "Accepts domains (example.com, *.example.com), IPs and CIDRs (10.0.0.0/8, 2001:db8::/32), and Xray prefixes such as domain:, full:, keyword:, regexp:, geosite: and geoip:. Lines starting with # are ignored.",
        "支持域名（example.com、*.example.com）、IP 和 CIDR（10.0.0.0/8、2001:db8::/32），以及 Xray 前缀，例如 domain:、full:、keyword:、regexp:、geosite: 和 geoip:。以 # 开头的行会被忽略。",
    ),
    // Routing · Outbound rules
    ("Outbound rules", "出栈规则"),
    (
        "Send a domain or IP to the proxy, direct, or the blackhole",
        "将域名或 IP 发送到代理、直连或黑洞",
    ),
    (
        "One rule per line: <domain-or-ip> <direct|proxy|block>. Rules run before the bypass rules above, so they can override them. Domain and IP syntax matches the bypass list.",
        "每行一条规则：<domain-or-ip> <direct|proxy|block>。这些规则会在上述绕过规则之前执行，因此可以覆盖它们。域名和 IP 语法与绕过列表相同。",
    ),
    // Routing · Resolution
    ("Resolution", "解析"),
    ("Domain strategy", "域名策略"),
    (
        "When Xray resolves a destination while routing",
        "Xray 在路由时解析目标地址的时机",
    ),
    (
        "Geo rules use the geodata downloaded in Core Management. Download Xray geodata there before enabling them.",
        "地理规则使用在核心管理中下载的地理数据。启用这些规则前，请先在核心管理中下载 Xray 地理数据。",
    ),
    // DNS · Resolvers
    ("Resolvers", "解析器"),
    ("Local DNS server", "本地 DNS 服务器"),
    (
        "Resolves bypassed domains (China, private and the custom bypass list); empty uses the system resolver",
        "解析被绕过的域名（中国大陆、私有网络及自定义绕过列表）；留空则使用系统解析器",
    ),
    ("Remote DNS server", "远程 DNS 服务器"),
    (
        "Resolves proxied domains; the query is routed through the proxy, so it answers from the exit's region",
        "解析走代理的域名；查询会经代理转发，因此由出口所在地区返回结果",
    ),
    (
        "1.1.1.1 or https://1.1.1.1/dns-query",
        "1.1.1.1 或 https://1.1.1.1/dns-query",
    ),
    (
        "Both resolvers can be enabled together: the local server answers bypassed domains and the remote server answers everything else. With only one set, it answers every query. While Fake DNS is on, proxied domains are answered by Fake DNS, so the remote server is not used.",
        "两个解析器可以同时启用：本地服务器解析被绕过的域名，远程服务器解析其余所有域名。若只设置一个，则由它响应所有查询。开启 FakeDNS 后，走代理的域名由 FakeDNS 应答，因此不会使用远程服务器。",
    ),
    // DNS · Fake DNS
    ("Fake DNS", "FakeDNS"),
    ("Enable Fake DNS", "启用 FakeDNS"),
    (
        "Answer with synthetic IPs and recover the domain through sniffing",
        "使用合成 IP 应答，并通过嗅探还原域名",
    ),
    (
        "Fake DNS is only used while TUN mode is enabled in the TUN tab.",
        "仅在 TUN 标签页启用 TUN 模式时才会使用 FakeDNS。",
    ),
    ("Fake IP pool", "Fake IP 池"),
    (
        "IPv4 CIDR used for synthetic answers",
        "用于合成应答的 IPv4 CIDR",
    ),
    ("Pool size", "池大小"),
    (
        "Maximum number of cached fake mappings",
        "缓存假映射的最大数量",
    ),
    (
        "When IPv6 is enabled a 2001:2::/48 pool is added automatically.",
        "启用 IPv6 时会自动添加 2001:2::/48 地址池。",
    ),
    // TUN · TUN Interface
    ("TUN Interface", "TUN 接口"),
    ("Enable TUN mode", "启用 TUN 模式"),
    (
        "Route all system traffic through Xray without app-level proxy settings",
        "无需应用级代理设置，将所有系统流量路由到 Xray",
    ),
    (
        "TUN needs network administrator privileges (CAP_NET_ADMIN). Without them the core cannot start; Cloak DX then restores the previous settings automatically.",
        "TUN 需要网络管理员权限（CAP_NET_ADMIN）。缺少该权限时核心无法启动，Cloak DX 随后会自动恢复之前的设置。",
    ),
    (
        "Waiting for administrator authorization...",
        "正在等待管理员授权...",
    ),
    (
        "TUN permissions granted. Enable TUN mode if needed, then start the core.",
        "已授予 TUN 权限。如需使用请启用 TUN 模式，然后启动核心。",
    ),
    ("Grant TUN permission", "授予 TUN 权限"),
    (
        "The permission is stored on the selected Xray binary; grant it again after the core is updated.",
        "该权限存储在所选 Xray 二进制文件上；更新核心后需要重新授予。",
    ),
    // TUN · IPv6
    ("Enable IPv6", "启用 IPv6"),
    (
        "Allow AAAA lookups and add IPv6 TUN addresses and routes",
        "允许 AAAA 查询，并添加 IPv6 TUN 地址和路由",
    ),
    // TUN · Interface
    ("Interface", "接口"),
    ("Interface name", "接口名称"),
    ("Empty lets Xray pick an available name", "留空则由 Xray 自动选择可用名称"),
    (
        "Maximum packet size on the interface",
        "接口上的最大数据包大小",
    ),
    ("Stack", "网络栈"),
    ("Network stack implementation", "网络栈实现"),
    (
        "Xray's TUN always uses its built-in stack; this preference applies to cores that support choosing one.",
        "Xray 的 TUN 始终使用其内置网络栈；此偏好仅适用于支持选择网络栈的核心。",
    ),
    ("IPv4 gateway", "IPv4 网关"),
    (
        "Address prefix assigned to the TUN interface",
        "分配给 TUN 接口的地址前缀",
    ),
    ("Auto route", "自动路由"),
    (
        "Add default routes and bind Xray outbounds to the physical interface",
        "添加默认路由，并将 Xray 出栈绑定到物理接口",
    ),
    ("Strict route", "严格路由"),
    (
        "Use split default routes so existing routes cannot bypass the TUN",
        "使用拆分默认路由，避免现有路由绕过 TUN",
    ),
    (
        "Strict route only takes effect while Auto route is enabled.",
        "仅在启用自动路由时，严格路由才会生效。",
    ),
    (
        "IPv6 gateway fc00::1/64 and its routes are added automatically while IPv6 is enabled above.",
        "在上方启用 IPv6 时，会自动添加 IPv6 网关 fc00::1/64 及其路由。",
    ),
    // Advanced · Sniffing
    ("Sniffing", "嗅探"),
    ("Enable sniffing", "启用嗅探"),
    (
        "Recover the hostname from HTTP, TLS and QUIC handshakes so domain rules also match apps that connect by IP",
        "从 HTTP、TLS 和 QUIC 握手中还原主机名，使域名规则也能匹配通过 IP 连接的应用",
    ),
    ("Detect plain HTTP requests", "检测明文 HTTP 请求"),
    (
        "Read the server name from the TLS handshake",
        "从 TLS 握手读取服务器名称",
    ),
    (
        "Read the server name from QUIC Initial packets",
        "从 QUIC Initial 数据包读取服务器名称",
    ),
    ("Route only", "仅用于路由"),
    (
        "Use the sniffed hostname for routing only, keeping the original address for the connection",
        "嗅探到的主机名仅用于路由，连接仍使用原始地址",
    ),
    (
        "TUN keeps sniffing on while FakeDNS is enabled, otherwise the fake addresses cannot be mapped back to their domains.",
        "启用 FakeDNS 时，TUN 会保持嗅探开启，否则无法将假地址映射回对应域名。",
    ),
    // Advanced · Fragment
    ("Fragment", "分片"),
    ("Enable fragment", "启用分片"),
    (
        "Split the TLS ClientHello sent to the node into small pieces to evade DPI",
        "将发送到节点的 TLS ClientHello 拆分为小片段，以规避 DPI",
    ),
    ("Packets", "数据包"),
    (
        "Which packets to split: tlshello or a packet range such as 1-3",
        "要拆分的数据包：tlshello 或类似 1-3 的数据包范围",
    ),
    ("Length", "长度"),
    (
        "Bytes per fragment, as a count or a range such as 100-200",
        "每个分片的字节数，可为单个数值或类似 100-200 的范围",
    ),
    ("Interval", "间隔"),
    (
        "Milliseconds between fragments, as a count or a range such as 10-20",
        "分片之间的毫秒间隔，可为单个数值或类似 10-20 的范围",
    ),
    (
        "Some networks or nodes reject fragmented handshakes; turn fragment off if a node stops connecting. Fragment applies to node links; full Xray JSON subscriptions keep their own transport settings.",
        "某些网络或节点会拒绝分片握手；如果某个节点无法连接，请关闭分片。分片仅适用于节点链接；完整的 Xray JSON 订阅会保留其自身的传输设置。",
    ),
];
