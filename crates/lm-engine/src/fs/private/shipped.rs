//! 出厂的私密目录（施工 5-13 起，5-17 上照各家对照补齐，2026-10-21 项目主人定）：钥匙、云和集群的凭据、明文令牌、密码库和
//! 钥匙环、浏览器、别的 agent 的登录、旧版 Miyu 的数据。写成 `~/…`；有的是文件（`~/.netrc` 这种）。不管在不在。
//!
//! 头五个三个平台都一样、先后不变：`~/.ssh`、`~/.gnupg`、`~/.claude`、`~/.miyu`、`~/.config/gh`。

/// Linux、BSD：照对照报告的推荐（`私密目录-各家对照.md`「推荐的 Miyu 出厂清单」）。BSD 上的桌面程序也照 XDG 放，同一份。
#[cfg(not(target_os = "macos"))]
pub const SHIPPED_PRIVATE: &[&str] = &[
    "~/.ssh",
    "~/.gnupg",
    "~/.claude",
    "~/.miyu",
    "~/.config/gh",
    // 别的 agent 的登录。
    "~/.codex",
    "~/.gemini",
    // 云和集群的凭据。
    "~/.aws",
    "~/.azure",
    "~/.config/gcloud",
    "~/.oci",
    "~/.kube",
    "~/.docker",
    // 明文令牌。
    "~/.git-credentials",
    "~/.config/git/credentials",
    "~/.netrc",
    "~/.pypirc",
    "~/.cargo/credentials.toml",
    "~/.gem/credentials",
    "~/.vault-token",
    "~/.terraform.d",
    "~/.pgpass",
    // 密码库和桌面钥匙环。
    "~/.password-store",
    "~/.local/share/keyrings",
    "~/.local/share/kwalletd",
    // 浏览器、邮件：各渠道版，Flatpak、Snap 装的。
    "~/.mozilla",
    "~/.thunderbird",
    "~/.config/google-chrome",
    "~/.config/google-chrome-beta",
    "~/.config/google-chrome-unstable",
    "~/.config/chromium",
    "~/.config/BraveSoftware",
    "~/.config/microsoft-edge",
    "~/.config/vivaldi",
    "~/.config/opera",
    "~/.var/app/org.mozilla.firefox",
    "~/.var/app/org.mozilla.Thunderbird",
    "~/.var/app/com.google.Chrome",
    "~/.var/app/org.chromium.Chromium",
    "~/.var/app/com.brave.Browser",
    "~/.var/app/com.microsoft.Edge",
    "~/.var/app/com.vivaldi.Vivaldi",
    "~/.var/app/com.opera.Opera",
    "~/snap/firefox",
    "~/snap/chromium",
];

/// macOS（LM-5，照 Miyu 去掉私密目录以前 `miyu-fs/src/private/shipped.rs` 的那一份）：钥匙串、浏览器、密码库在 `~/Library` 下。
#[cfg(target_os = "macos")]
pub const SHIPPED_PRIVATE: &[&str] = &[
    "~/.ssh",
    "~/.gnupg",
    "~/.claude",
    "~/.miyu",
    "~/.config/gh",
    "~/.codex",
    "~/.gemini",
    "~/.aws",
    "~/.azure",
    "~/.config/gcloud",
    "~/.kube",
    "~/.docker",
    "~/.git-credentials",
    "~/.netrc",
    "~/.pypirc",
    "~/.cargo/credentials.toml",
    "~/.vault-token",
    "~/Library/Keychains",
    "~/Library/Cookies",
    "~/Library/Safari",
    "~/Library/Application Support/Google/Chrome",
    "~/Library/Application Support/Chromium",
    "~/Library/Application Support/BraveSoftware",
    "~/Library/Application Support/Microsoft Edge",
    "~/Library/Application Support/Vivaldi",
    "~/Library/Application Support/Firefox",
    "~/Library/Application Support/1Password",
    "~/Library/Application Support/Bitwarden",
];
