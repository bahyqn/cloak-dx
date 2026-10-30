//! Simplified Chinese strings for the sing-box settings panel.
//!
//! Kept separate from the Xray catalog so each engine's translations can grow
//! with its own options without touching the other engine.

pub(crate) const MESSAGES: &[(&str, &str)] = &[
    // Panel headings and hints
    ("sing-box · General", "sing-box · 通用"),
    ("sing-box · Routing", "sing-box · 路由"),
    ("sing-box · DNS", "sing-box · DNS"),
    ("sing-box · TUN", "sing-box · TUN"),
    ("sing-box · Advanced", "sing-box · 高级"),
    (
        "These options are saved immediately and take effect the next time sing-box starts.",
        "这些选项会立即保存，并在下次启动 sing-box 时生效。",
    ),
    (
        "Saved. sing-box will use these settings when it starts.",
        "已保存。sing-box 启动后将使用这些设置。",
    ),
    (
        "Saved and applied. The running core was restarted with the new settings.",
        "已保存并应用。正在运行的核心已使用新设置重启。",
    ),
    (
        "Saved. No core is running; the settings apply at the next start.",
        "已保存。当前没有运行中的核心，设置将在下次启动时生效。",
    ),
    // General
    ("Logging", "日志"),
    ("Log level", "日志级别"),
    (
        "Verbosity written to the core log",
        "写入核心日志的详细程度",
    ),
    ("Log timestamps", "日志时间戳"),
    ("Prefix every log line with the time", "在每条日志前添加时间"),
    ("Enable IPv6", "启用 IPv6"),
    (
        "Allow AAAA lookups and IPv6 addresses in the generated config",
        "允许 AAAA 查询并在生成的配置中使用 IPv6 地址",
    ),
    ("Local Ports", "本地端口"),
    ("Mixed port", "混合端口"),
    (
        "Single loopback port serving both SOCKS and HTTP for apps and the system proxy",
        "同时为应用和系统代理提供 SOCKS 与 HTTP 的单一回环端口",
    ),
    ("Local Network Access", "局域网访问"),
    ("Allow LAN connections", "允许局域网连接"),
    (
        "Let other devices on this network use this machine as a proxy",
        "允许此网络中的其他设备将本机作为代理",
    ),
    ("LAN port", "局域网端口"),
    (
        "Port other devices connect to for mixed SOCKS/HTTP traffic",
        "其他设备连接混合 SOCKS/HTTP 流量所使用的端口",
    ),
    ("LAN address", "局域网地址"),
    (
        "Point other devices at this address with the port above",
        "让其他设备使用上方端口连接到此地址",
    ),
    (
        "Could not detect this machine's LAN address. Use its network IP with the port above.",
        "无法检测到此机器的局域网地址。请使用其网络 IP 和上述端口。",
    ),
    ("Username", "用户名"),
    (
        "Leave both this and the password empty to accept connections without authentication",
        "将此项和密码都留空可接受无需认证的连接",
    ),
    ("Password", "密码"),
    (
        "Must be set together with the username",
        "必须与用户名一起设置",
    ),
    (
        "Only one of the two fields is filled in, so authentication stays off. Set both a username and a password to require them.",
        "两个字段只填写了一个，因此不会启用认证。同时填写用户名和密码才会要求认证。",
    ),
    // Routing
    ("Routing mode", "路由模式"),
    (
        "Rule follows the routing rules below, Global proxies every connection, Direct disables the proxy",
        "Rule 按下方路由规则分流，Global 代理所有连接，Direct 禁用代理",
    ),
    ("Node Selection", "节点选择"),
    ("Auto failover", "自动故障转移"),
    (
        "Probe every node and keep traffic on the fastest reachable one; manual node selection is ignored",
        "探测所有节点并将流量保持在最快且可达的节点上；手动选择的节点会被忽略",
    ),
    ("Bypass", "绕过"),
    ("Bypass mainland China", "绕过中国大陆"),
    ("Send Chinese domains and IPs direct", "让中国域名和 IP 直连"),
    ("Bypass LAN and private networks", "绕过局域网和私有网络"),
    ("Keep local addresses off the proxy", "让本地地址不经过代理"),
    ("Block ads", "拦截广告"),
    ("Reject geosite:category-ads-all", "拒绝 geosite:category-ads-all"),
    ("Block QUIC (UDP 443)", "拦截 QUIC（UDP 443）"),
    (
        "Make QUIC-capable apps fall back to TCP so the routing rules see their traffic",
        "让支持 QUIC 的应用回退到 TCP，以便路由规则能够识别其流量",
    ),
    ("Custom routing rules", "自定义路由规则"),
    (
        "Send a domain or IP to the proxy, direct, or the blackhole",
        "将域名或 IP 发送到代理、直连或黑洞",
    ),
    (
        "One rule per line: <domain-or-ip> <direct|proxy|block>. Rules run before the automatic bypass rules, so they can override them.",
        "每行一条规则：<域名或 IP> <direct|proxy|block>。规则先于自动绕过规则执行，因此可以覆盖它们。",
    ),
    // DNS
    ("Resolution", "解析"),
    ("DNS strategy", "DNS 策略"),
    (
        "Which address family sing-box prefers when a domain has both A and AAAA records",
        "当域名同时具有 A 和 AAAA 记录时 sing-box 优先使用的地址族",
    ),
    ("Resolvers", "解析服务器"),
    ("Local DNS server", "本地 DNS 服务器"),
    (
        "Resolves bypassed domains (China, private and custom direct rules); empty uses the system resolver",
        "解析被绕过的域名（中国大陆、私有网络及自定义直连规则）；留空则使用系统解析器",
    ),
    ("Remote DNS server", "远程 DNS 服务器"),
    (
        "Resolves proxied domains; the query is routed through the proxy, so it answers from the exit's region",
        "解析被代理的域名；查询会经过代理，因此由出口所在地区返回答案",
    ),
    ("Fake DNS", "Fake DNS"),
    ("Enable Fake DNS", "启用 Fake DNS"),
    (
        "Answer with synthetic IPs and recover the domain through sniffing",
        "使用合成 IP 应答，并通过嗅探恢复域名",
    ),
    ("IPv4 range", "IPv4 网段"),
    (
        "CIDR used for synthetic IPv4 answers, e.g. 198.18.0.0/15",
        "用于合成 IPv4 应答的 CIDR，例如 198.18.0.0/15",
    ),
    ("IPv6 range", "IPv6 网段"),
    (
        "CIDR used for synthetic IPv6 answers, e.g. fc00::/18",
        "用于合成 IPv6 应答的 CIDR，例如 fc00::/18",
    ),
    (
        "Fake DNS needs sniffing, and is most useful with TUN mode enabled in the TUN tab.",
        "Fake DNS 需要嗅探，并且在 TUN 标签页启用 TUN 模式时最为有用。",
    ),
    // TUN
    ("TUN Interface", "TUN 接口"),
    ("Enable TUN mode", "启用 TUN 模式"),
    (
        "Route all system traffic through sing-box without app-level proxy settings",
        "无需应用级代理设置即可让所有系统流量经过 sing-box",
    ),
    (
        "TUN needs administrator capabilities on the sing-box binary (CAP_NET_ADMIN, CAP_NET_RAW). Cloak DX asks for them automatically when sing-box starts with TUN enabled; you can also grant them now.",
        "TUN 需要 sing-box 二进制具备管理员能力（CAP_NET_ADMIN、CAP_NET_RAW）。启用 TUN 启动 sing-box 时 Cloak DX 会自动请求权限，也可以现在手动授予。",
    ),
    (
        "The permission is stored on the selected sing-box binary; grant it again after the core is updated.",
        "权限存储在选中的 sing-box 二进制上；核心更新后需要重新授予。",
    ),
    ("Interface name", "接口名称"),
    (
        "Empty lets sing-box pick an available name",
        "留空则由 sing-box 选择可用名称",
    ),
    ("Maximum packet size on the interface", "接口上的最大数据包大小"),
    ("IPv4 address", "IPv4 地址"),
    ("Address prefix assigned to the TUN interface", "分配给 TUN 接口的地址前缀"),
    ("Stack", "网络栈"),
    ("Network stack implementation", "网络栈实现"),
    ("Auto route", "自动路由"),
    (
        "Add default routes so traffic enters the TUN interface",
        "添加默认路由，使流量进入 TUN 接口",
    ),
    ("Strict route", "严格路由"),
    (
        "Use split default routes so existing routes cannot bypass the TUN",
        "使用拆分默认路由，避免现有路由绕过 TUN",
    ),
    ("Auto redirect", "自动重定向"),
    (
        "Use nftables to redirect traffic without adding routes (Linux only)",
        "使用 nftables 重定向流量而不添加路由（仅 Linux）",
    ),
    // Advanced
    ("Sniffing", "嗅探"),
    ("Enable sniffing", "启用嗅探"),
    (
        "Recover the hostname from HTTP, TLS and QUIC handshakes so domain rules also match apps that connect by IP",
        "从 HTTP、TLS 和 QUIC 握手中恢复主机名，使域名规则也能匹配通过 IP 连接的应用",
    ),
    ("Override destination", "覆盖目标地址"),
    (
        "Connect to the sniffed domain instead of the original IP",
        "连接到嗅探到的域名，而非原始 IP",
    ),
    ("Multiplexing", "多路复用"),
    (
        "Multiplex connections on the proxy outbound",
        "在代理出站上多路复用连接",
    ),
    (
        "Reduce handshake overhead when the server supports it",
        "在服务器支持时减少握手开销",
    ),
    ("Protocol", "协议"),
    ("Multiplex protocol used with the server", "与服务器配合使用的多路复用协议"),
    ("Max connections", "最大连接数"),
    (
        "Maximum number of multiplexed connections kept open",
        "保持打开的多路复用连接数上限",
    ),
    ("Padding", "填充"),
    ("Pad multiplexed traffic to hide its size", "填充多路复用流量以隐藏其大小"),
    ("Connection", "连接"),
    ("TCP Fast Open", "TCP Fast Open"),
    (
        "Allow TCP Fast Open on outbound connections",
        "允许出站连接使用 TCP Fast Open",
    ),
    ("Cache file", "缓存文件"),
    (
        "Store DNS and selector state across restarts",
        "在重启后保留 DNS 与选择器状态",
    ),
    // Option labels
    ("Global", "全局"),
    ("Direct", "直连"),
    ("Rule", "规则"),
    ("Prefer IPv4", "优先 IPv4"),
    ("Prefer IPv6", "优先 IPv6"),
    ("IPv4 only", "仅 IPv4"),
    ("IPv6 only", "仅 IPv6"),
    ("System", "系统"),
    ("Mixed", "混合"),
    ("No password", "无密码"),
    ("Enter a port between 1 and 65535.", "请输入 1 到 65535 之间的端口。"),
];
