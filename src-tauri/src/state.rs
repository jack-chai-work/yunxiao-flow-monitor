use crate::api::models::PipelineSummary;
use crate::config::{AppConfig, ConfigStore};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::{Mutex, Notify, RwLock};

/// 某条流水线上一次观测到的运行状态，用于判断「状态跃迁」。
#[derive(Debug, Clone)]
pub struct StatusRecord {
    pub run_id: i64,
    pub status: String,
    pub pipeline_name: String,
}

/// 全局共享状态。
///
/// - `http`：复用的 reqwest 连接池，`YunxiaoClient` 只是它的轻量包装
/// - `cfg`：内存中的当前配置（磁盘副本由 `store` 管理）
/// - `polling`：轮询总开关，托盘菜单与前端都能翻转它
/// - `minimize_on_close`：`cfg.minimize_to_tray_on_close` 的原子镜像，
///   因为窗口事件回调是同步上下文，不能在这里 await 异步锁
/// - `notify_flag`：唤醒轮询循环立即执行一次（保存配置 / 手动刷新 / 写操作后）
/// - `last_status`：pipelineId → 上次状态，跃迁检测的唯一依据
/// - `snapshot`：最近一次轮询结果，供前端冷启动直接渲染
pub struct AppState {
    pub http: reqwest::Client,
    pub store: Arc<RwLock<ConfigStore>>,
    pub cfg: Arc<RwLock<AppConfig>>,
    pub polling: Arc<AtomicBool>,
    pub minimize_on_close: Arc<AtomicBool>,
    pub notify_flag: Arc<Notify>,
    pub last_status: Arc<Mutex<HashMap<i64, StatusRecord>>>,
    pub snapshot: Arc<RwLock<Vec<PipelineSummary>>>,
}

impl AppState {
    pub fn new(http: reqwest::Client, store: ConfigStore, cfg: AppConfig) -> Self {
        let minimize_on_close = Arc::new(AtomicBool::new(cfg.minimize_to_tray_on_close));
        Self {
            http,
            store: Arc::new(RwLock::new(store)),
            cfg: Arc::new(RwLock::new(cfg)),
            polling: Arc::new(AtomicBool::new(false)),
            minimize_on_close,
            notify_flag: Arc::new(Notify::new()),
            last_status: Arc::new(Mutex::new(HashMap::new())),
            snapshot: Arc::new(RwLock::new(Vec::new())),
        }
    }
}
