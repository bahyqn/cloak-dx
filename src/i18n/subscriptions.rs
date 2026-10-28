//! Simplified Chinese strings for the Subscriptions screen.

pub(crate) const MESSAGES: &[(&str, &str)] = &[
    // Page and card
    ("Subscriptions", "订阅"),
    ("Update All", "全部更新"),
    ("Import", "导入"),
    ("No subscriptions added.", "尚未添加订阅。"),
    ("Never", "从未"),
    ("Nodes", "节点"),
    ("Usable", "可用"),
    (
        "Refresh this subscription to fetch its configuration and count nodes.",
        "刷新此订阅以获取配置并统计节点。",
    ),
    ("Upload", "上传"),
    ("Download", "下载"),
    ("Total Traffic", "总流量"),
    ("Expire Time:", "到期时间："),
    ("Reset Time:", "重置时间："),
    ("N/A", "暂无"),
    ("Refresh failed", "刷新失败"),
    // Card actions
    ("Edit subscription", "编辑订阅"),
    ("Update Now", "立即更新"),
    ("Update subscription now", "立即更新订阅"),
    ("Delete subscription", "删除订阅"),
    // Add / edit dialog
    ("Edit Subscription", "编辑订阅"),
    ("Add Subscription", "添加订阅"),
    ("Close dialog", "关闭对话框"),
    ("Name", "名称"),
    ("e.g. My Premium Proxy", "例如：我的高级代理"),
    ("Auto Update", "自动更新"),
    ("Update Interval", "更新间隔"),
    ("hr", "小时"),
    (
        "Leave empty for default (Sing-Box)",
        "留空则使用默认值（Sing-Box）",
    ),
    ("Cancel", "取消"),
    ("Save Changes", "保存更改"),
    ("Save", "保存"),
    // Validation and save errors
    ("Name and URL are required.", "名称和 URL 为必填项。"),
    (
        "URL must start with http:// or https://.",
        "URL 必须以 http:// 或 https:// 开头。",
    ),
    (
        "Update interval must be a positive number of hours.",
        "更新间隔必须为正整数小时数。",
    ),
    ("Could not save subscription", "无法保存订阅"),
];
