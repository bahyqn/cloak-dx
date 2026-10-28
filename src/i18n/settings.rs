//! Simplified Chinese strings for the Settings shell, appearance picker and
//! core management screen.

pub(crate) const MESSAGES: &[(&str, &str)] = &[
    // Appearance picker
    ("Appearance", "外观"),
    (
        "Theme and language are saved and restored on the next launch.",
        "主题和语言会保存，并在下次启动时恢复。",
    ),
    ("Theme", "主题"),
    ("Language", "语言"),
    ("Dark", "深色"),
    (
        "The original slate-blue interface with glass panels",
        "原有的石板蓝玻璃质感界面",
    ),
    (
        "Light Adwaita surfaces, GNOME blue accent and typography",
        "浅色 Adwaita 表面、GNOME 蓝色强调色与字体",
    ),
    ("English interface", "英文界面"),
    ("Simplified Chinese interface", "简体中文界面"),
    // Settings shell
    ("Settings", "设置"),
    (
        "Xray settings apply immediately to the running core",
        "Xray 设置会立即应用到正在运行的核心",
    ),
    ("General", "通用"),
    ("Routing", "路由"),
    ("Advanced", "高级"),
    ("Core Management", "核心管理"),
    // Core management
    (
        "Cloak DX runs on an external proxy engine. Download Xray-core or sing-box here, choose which installed build to use, and keep the geodata its routing rules depend on up to date. The selected core is what the sidebar shows and what every subscription starts.",
        "Cloak DX 依赖外部代理核心运行。可在此下载 Xray-core 或 sing-box，选择要使用的已安装版本，并保持其路由规则所依赖的地理数据为最新。所选核心会显示在侧边栏，并作为每个订阅启动时使用的核心。",
    ),
    ("Download Core", "下载核心"),
    (
        "Pick a core, release channel, and version, then download it. Versions are listed from the official release pages for this system's architecture; refresh the list after switching core or channel.",
        "选择核心、发布通道和版本后即可下载。版本列表来自适用于本系统架构的官方发布页面；切换核心或通道后请刷新列表。",
    ),
    ("Proxy core storage", "代理核心存储位置"),
    ("Downloaded core binaries", "已下载的核心二进制文件"),
    ("SQLite database", "SQLite 数据库"),
    ("Subscriptions and application data", "订阅和应用数据"),
    ("Core", "核心"),
    ("Select which core release to install", "选择要安装的核心版本"),
    ("Release channel", "发布通道"),
    (
        "Main is stable; preview tracks prereleases",
        "Main 为稳定版；Preview 跟随预发布版本",
    ),
    ("Main (stable)", "Main（稳定版）"),
    ("Available version", "可用版本"),
    (
        "Releases matching this core, channel, and system architecture",
        "与此核心、通道及系统架构匹配的发布版本",
    ),
    ("Refresh available versions", "刷新可用版本"),
    ("Refreshing versions...", "正在刷新版本..."),
    ("Refreshing available versions...", "正在刷新可用版本..."),
    (
        "No cached versions. Click refresh to load releases.",
        "没有缓存的版本。点击刷新以加载发布版本。",
    ),
    ("Downloading", "正在下载"),
    ("Downloading...", "正在下载..."),
    ("Download and Install", "下载并安装"),
    ("Select an available release version first.", "请先选择一个可用的发布版本。"),
    ("Loaded", "已加载"),
    ("available versions.", "个可用版本。"),
    ("Installed", "已安装"),
    ("Installed Cores", "已安装的核心"),
    (
        "\"Use this core\" switches the active engine; the next started subscription runs on it. Deleting an installation removes its binary from disk but keeps your subscriptions and settings.",
        "“使用此核心”会切换当前使用的引擎；下次启动的订阅将在其上运行。删除安装会从磁盘移除其二进制文件，但会保留你的订阅和设置。",
    ),
    ("No cores installed yet.", "尚未安装任何核心。"),
    ("Selected", "已选择"),
    ("Use this core", "使用此核心"),
    ("Delete core", "删除核心"),
    ("Geo Assets", "地理数据资源"),
    (
        "Bypass mainland China, ad blocking, and local DNS rules match traffic through these GeoIP/GeoSite files. Download or update them once per engine; a running core keeps the data it loaded at startup, so restart it to pick up new files.",
        "分流中国大陆、广告拦截和本地 DNS 规则都通过这些 GeoIP/GeoSite 文件匹配流量。每个引擎只需下载或更新一次；正在运行的核心会沿用启动时加载的数据，因此请重启核心以使用新文件。",
    ),
    ("Asset directory", "资源目录"),
    (
        "All GeoIP and GeoSite data files share this folder",
        "所有 GeoIP 和 GeoSite 数据文件都存放在此文件夹中",
    ),
    ("Xray-core geodata", "Xray-core 地理数据"),
    ("sing-box geodata", "sing-box 地理数据"),
    (
        "Download the latest official geodata files",
        "下载最新的官方地理数据文件",
    ),
    ("Updating", "正在更新"),
    ("geodata...", "地理数据..."),
    ("geodata updated.", "地理数据已更新。"),
    ("Updating...", "正在更新..."),
    ("Update", "更新"),
    ("Download", "下载"),
    ("not downloaded", "未下载"),
];
