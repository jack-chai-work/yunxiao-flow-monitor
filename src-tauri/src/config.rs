use crate::error::{AppResult, other};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 云效中心版（公共云）默认服务接入点。
pub const DEFAULT_BASE_URL: &str = "https://openapi-rdc.aliyuncs.com";

/// 应用配置。
///
/// 说明：`token` 不会随普通配置一起明文落盘，而是走 [`ConfigStore`] 的凭据通道
/// （默认混淆存储，开启 `secure-store` feature 后走系统钥匙串）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    /// 服务接入点，中心版固定为 openapi-rdc.aliyuncs.com
    pub base_url: String,
    /// 组织 ID（中心版必填），组织管理后台 → 基本信息 页面可获取
    pub organization_id: String,
    /// 常规轮询间隔（秒）
    pub poll_interval_secs: u64,
    /// 存在运行中流水线时的加速轮询间隔（秒）
    pub fast_poll_interval_secs: u64,
    /// 关注的流水线 ID；为空时自动关注列表首页
    pub watched_pipeline_ids: Vec<i64>,
    /// 自动关注列表首页时最多关注多少条
    pub auto_watch_limit: u64,
    /// 启动即开始轮询
    pub auto_start_polling: bool,
    /// 运行成功时发桌面通知
    pub notify_on_success: bool,
    /// 运行失败时发桌面通知
    pub notify_on_fail: bool,
    /// 运行被取消时发桌面通知
    pub notify_on_canceled: bool,
    /// 关闭窗口时最小化到托盘而不是退出
    pub minimize_to_tray_on_close: bool,
    /// 令牌（仅在内存中流转，序列化给前端时由 ConfigDto 做脱敏）
    #[serde(skip)]
    pub token: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            organization_id: String::new(),
            poll_interval_secs: 10,
            fast_poll_interval_secs: 3,
            watched_pipeline_ids: Vec::new(),
            auto_watch_limit: 30,
            auto_start_polling: true,
            notify_on_success: true,
            notify_on_fail: true,
            notify_on_canceled: false,
            minimize_to_tray_on_close: true,
            token: String::new(),
        }
    }
}

impl AppConfig {
    pub fn normalize(&mut self) {
        if self.base_url.trim().is_empty() {
            self.base_url = DEFAULT_BASE_URL.to_string();
        }
        self.base_url = self.base_url.trim().trim_end_matches('/').to_string();
        // 允许用户直接粘贴带 https:// 前缀或漏掉协议头的域名
        if !self.base_url.starts_with("http://") && !self.base_url.starts_with("https://") {
            self.base_url = format!("https://{}", self.base_url);
        }
        self.organization_id = self.organization_id.trim().to_string();
        self.token = self.token.trim().to_string();
        self.poll_interval_secs = self.poll_interval_secs.clamp(3, 600);
        self.fast_poll_interval_secs = self.fast_poll_interval_secs.clamp(1, 60);
        self.auto_watch_limit = self.auto_watch_limit.clamp(1, 100);
        // 快速间隔不应慢于常规间隔
        if self.fast_poll_interval_secs > self.poll_interval_secs {
            self.fast_poll_interval_secs = self.poll_interval_secs;
        }
    }

    /// 令牌与组织 ID 都齐备才认为可以发起请求。
    pub fn is_ready(&self) -> bool {
        !self.token.is_empty() && !self.organization_id.is_empty()
    }
}

/// 前端可见的配置视图：令牌只回传「是否已保存 + 掩码」，绝不回传原文。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDto {
    pub base_url: String,
    pub organization_id: String,
    pub poll_interval_secs: u64,
    pub fast_poll_interval_secs: u64,
    pub watched_pipeline_ids: Vec<i64>,
    pub auto_watch_limit: u64,
    pub auto_start_polling: bool,
    pub notify_on_success: bool,
    pub notify_on_fail: bool,
    pub notify_on_canceled: bool,
    pub minimize_to_tray_on_close: bool,
    pub has_token: bool,
    pub token_masked: String,
    pub secure_store_enabled: bool,
}

impl From<&AppConfig> for ConfigDto {
    fn from(c: &AppConfig) -> Self {
        Self {
            base_url: c.base_url.clone(),
            organization_id: c.organization_id.clone(),
            poll_interval_secs: c.poll_interval_secs,
            fast_poll_interval_secs: c.fast_poll_interval_secs,
            watched_pipeline_ids: c.watched_pipeline_ids.clone(),
            auto_watch_limit: c.auto_watch_limit,
            auto_start_polling: c.auto_start_polling,
            notify_on_success: c.notify_on_success,
            notify_on_fail: c.notify_on_fail,
            notify_on_canceled: c.notify_on_canceled,
            minimize_to_tray_on_close: c.minimize_to_tray_on_close,
            has_token: !c.token.is_empty(),
            token_masked: mask_token(&c.token),
            secure_store_enabled: cfg!(feature = "secure-store"),
        }
    }
}

/// 前端提交的配置。`token` 为空字符串表示「保持原令牌不变」。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConfigInput {
    pub base_url: String,
    pub organization_id: String,
    pub poll_interval_secs: u64,
    pub fast_poll_interval_secs: u64,
    pub watched_pipeline_ids: Vec<i64>,
    pub auto_watch_limit: u64,
    pub auto_start_polling: bool,
    pub notify_on_success: bool,
    pub notify_on_fail: bool,
    pub notify_on_canceled: bool,
    pub minimize_to_tray_on_close: bool,
    pub token: String,
}

impl Default for ConfigInput {
    fn default() -> Self {
        let d = AppConfig::default();
        Self {
            base_url: d.base_url,
            organization_id: d.organization_id,
            poll_interval_secs: d.poll_interval_secs,
            fast_poll_interval_secs: d.fast_poll_interval_secs,
            watched_pipeline_ids: d.watched_pipeline_ids,
            auto_watch_limit: d.auto_watch_limit,
            auto_start_polling: d.auto_start_polling,
            notify_on_success: d.notify_on_success,
            notify_on_fail: d.notify_on_fail,
            notify_on_canceled: d.notify_on_canceled,
            minimize_to_tray_on_close: d.minimize_to_tray_on_close,
            token: String::new(),
        }
    }
}

pub fn mask_token(token: &str) -> String {
    if token.is_empty() {
        return String::new();
    }
    let n = token.chars().count();
    if n <= 8 {
        return "*".repeat(n);
    }
    let head: String = token.chars().take(4).collect();
    let tail: String = token.chars().skip(n - 4).collect();
    format!("{head}{}{tail}", "*".repeat(6))
}

/// 配置持久化。
///
/// 目录布局（Tauri app_config_dir）：
/// - `config.json`      普通配置，明文可读，便于排查
/// - `credentials.bin`  令牌，默认做混淆存储；开启 `secure-store` 后改走系统钥匙串
pub struct ConfigStore {
    dir: PathBuf,
}

impl ConfigStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn config_path(&self) -> PathBuf {
        self.dir.join("config.json")
    }

    fn credential_path(&self) -> PathBuf {
        self.dir.join("credentials.bin")
    }

    pub fn load(&self) -> AppResult<AppConfig> {
        let mut cfg = if self.config_path().exists() {
            let raw = std::fs::read_to_string(self.config_path()).map_err(other)?;
            serde_json::from_str::<AppConfig>(&raw).unwrap_or_default()
        } else {
            AppConfig::default()
        };

        cfg.token = self.load_token().unwrap_or_default();
        cfg.normalize();
        Ok(cfg)
    }

    pub fn save(&self, cfg: &AppConfig) -> AppResult<()> {
        std::fs::create_dir_all(&self.dir).map_err(other)?;

        // 1) 普通配置（token 字段被 #[serde(skip)] 排除，不会落盘）
        let raw = serde_json::to_string_pretty(cfg).map_err(other)?;
        std::fs::write(self.config_path(), raw).map_err(other)?;
        restrict_permissions(&self.config_path());

        // 2) 令牌
        if cfg.token.is_empty() {
            let _ = self.clear_token();
        } else {
            self.save_token(&cfg.token)?;
        }
        Ok(())
    }

    fn save_token(&self, token: &str) -> AppResult<()> {
        #[cfg(feature = "secure-store")]
        {
            if let Ok(entry) = keyring_entry() {
                if entry.set_password(token).is_ok() {
                    let _ = std::fs::remove_file(self.credential_path());
                    return Ok(());
                }
            }
            // 钥匙串不可用时自动回落到混淆文件，保证功能可用
        }
        std::fs::create_dir_all(&self.dir).map_err(other)?;
        let encoded = obfuscate(token);
        std::fs::write(self.credential_path(), encoded).map_err(other)?;
        restrict_permissions(&self.credential_path());
        Ok(())
    }

    fn load_token(&self) -> AppResult<String> {
        #[cfg(feature = "secure-store")]
        {
            if let Ok(entry) = keyring_entry() {
                if let Ok(token) = entry.get_password() {
                    if !token.is_empty() {
                        return Ok(token);
                    }
                }
            }
        }
        if !self.credential_path().exists() {
            return Ok(String::new());
        }
        let raw = std::fs::read_to_string(self.credential_path()).map_err(other)?;
        Ok(deobfuscate(&raw))
    }

    fn clear_token(&self) -> AppResult<()> {
        #[cfg(feature = "secure-store")]
        {
            if let Ok(entry) = keyring_entry() {
                let _ = entry.delete_credential();
            }
        }
        if self.credential_path().exists() {
            std::fs::remove_file(self.credential_path()).map_err(other)?;
        }
        Ok(())
    }
}

#[cfg(feature = "secure-store")]
fn keyring_entry() -> Result<keyring::Entry, AppError> {
    keyring::Entry::new("yunxiao-flow-monitor", "personal-access-token").map_err(other)
}

/// 混淆密钥。
///
/// 注意：这只是「避免令牌在磁盘上以明文出现」的轻量混淆，不是加密。
/// 需要真正的安全存储请用 `--features secure-store` 编译，走系统钥匙串。
const OBFUSCATION_KEY: &[u8] = b"yunxiao-flow-monitor/obfuscation-key-v1";

fn obfuscate(plain: &str) -> String {
    use base64::Engine;
    let bytes = plain.as_bytes();
    let xored: Vec<u8> = bytes
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ OBFUSCATION_KEY[i % OBFUSCATION_KEY.len()])
        .collect();
    base64::engine::general_purpose::STANDARD.encode(xored)
}

fn deobfuscate(encoded: &str) -> String {
    use base64::Engine;
    let Ok(xored) = base64::engine::general_purpose::STANDARD.decode(encoded.trim()) else {
        return String::new();
    };
    let plain: Vec<u8> = xored
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ OBFUSCATION_KEY[i % OBFUSCATION_KEY.len()])
        .collect();
    String::from_utf8_lossy(&plain).to_string()
}

/// Unix 下把敏感文件收紧到 0600，Windows 上无对应语义，直接跳过。
#[cfg(unix)]
fn restrict_permissions(path: &PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perm = meta.permissions();
        perm.set_mode(0o600);
        let _ = std::fs::set_permissions(path, perm);
    }
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &PathBuf) {}
