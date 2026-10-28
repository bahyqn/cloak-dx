//! Simplified Chinese strings for the Network screen.

pub(crate) const MESSAGES: &[(&str, &str)] = &[
    // Page header and banners
    ("Network", "网络"),
    ("No active node", "无活动节点"),
    ("Proxy core is offline", "代理核心未运行"),
    ("Testing...", "测试中..."),
    ("Test", "测试"),
    ("Re-test", "重新测试"),
    ("Start the proxy core first", "请先启动代理核心"),
    (
        "These checks run through the active node. Start the core from the Dashboard, then return here to inspect the connection.",
        "这些检测通过活动节点运行。请先在仪表盘启动核心，然后返回此处检查连接。",
    ),
    // Google exit card
    ("Google Exit", "Google 出口"),
    ("Test Google exit", "测试 Google 出口"),
    ("Probing Google...", "正在探测 Google..."),
    ("Not tested yet", "尚未检测"),
    (
        "Shows the country and city Google infers from the active exit IP.",
        "显示 Google 根据当前出口 IP 推断出的国家和城市。",
    ),
    ("Google exit probe failed", "Google 出口探测失败"),
    ("Exit IP", "出口 IP"),
    ("Google country", "Google 国家"),
    ("City / region", "城市 / 地区"),
    // IPPure card
    ("Test with IPPure", "使用 IPPure 测试"),
    ("Asking IPPure...", "正在查询 IPPure..."),
    (
        "IPPure scores the exit IP for risk, ASN type, native/broadcast and automation.",
        "IPPure 会评估出口 IP 的风险、ASN 类型、原生/广播属性及自动化程度。",
    ),
    ("IPPure lookup failed", "IPPure 查询失败"),
    ("IPPure risk score", "IPPure 风险评分"),
    ("Location", "位置"),
    ("Human / bot", "真人 / 机器人"),
    ("human", "真人"),
    ("bot", "机器人"),
    // Service access card
    ("Service Access", "服务访问"),
    ("Test the selected services", "测试所选服务"),
    ("Service slot", "服务槽位"),
    ("Pick up to three services to check.", "最多选择三个服务进行检测。"),
    ("Google ping", "Google 延迟"),
    ("OK", "正常"),
    ("Failed", "失败"),
    // DNS leak card
    ("DNS Leak", "DNS 泄漏"),
    ("DNS leak check provider", "DNS 泄漏检测提供商"),
    ("Run the DNS leak check", "运行 DNS 泄漏检测"),
    ("Probing public resolvers...", "正在探测公共解析器..."),
    (
        "Random hostnames are resolved through the proxy, so the resolvers this connection actually uses show up here.",
        "随机主机名通过代理解析，因此这里显示的是该连接实际使用的解析器。",
    ),
    ("DNS leak check failed", "DNS 泄漏检测失败"),
    ("No DNS leak", "无 DNS 泄漏"),
    ("DNS status unknown", "DNS 状态未知"),
    ("DNS leak detected", "检测到 DNS 泄漏"),
    // DNS verdict summaries
    ("Resolvers are consistent with", "解析器与以下出口国家一致："),
    ("seen", "个已检测"),
    (
        "The exit country is unknown, so resolver locations cannot be compared. Run the Google exit or IPPure test first.",
        "出口国家未知，无法比较解析器位置。请先运行 Google 出口或 IPPure 检测。",
    ),
    (
        "The resolvers have no location data, so they cannot be compared with the exit country.",
        "解析器没有位置数据，无法与出口国家进行比较。",
    ),
    ("of", "/"),
    ("resolvers sit outside", "个解析器位于出口国家之外："),
    ("the exit country", "出口国家"),
    (
        "ipleak.net did not see this connection's DNS lookups (the resolver answered the probe names from cache), so the resolvers below were measured with",
        "ipleak.net 未看到该连接的 DNS 查询（解析器从缓存回答了探测域名），因此下面的解析器使用了",
    ),
    ("probes.", "探测。"),
    ("Unknown", "未知"),
    ("Unknown location", "未知位置"),
];
