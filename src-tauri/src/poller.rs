use crate::api::models::*;
use crate::api::{YunxiaoClient, extract_array, extract_object};
use crate::commands::fill_latest_runs;
use crate::config::AppConfig;
use crate::state::AppState;
use serde_json::Value;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

/// 前端监听的事件名。
pub const EVT_SNAPSHOT: &str = "yunxiao://snapshot";
pub const EVT_STATUS_CHANGED: &str = "yunxiao://status-changed";
pub const EVT_POLL_ERROR: &str = "yunxiao://poll-error";
pub const EVT_POLL_STATE: &str = "yunxiao://poll-state";

/// 启动后台轮询循环。
///
/// 整个应用生命周期只启动一次；「开始/暂停」通过 `state.polling` 原子开关控制，
/// 「立即刷新」通过 `state.notify_flag` 唤醒，避免反复 spawn/abort 任务带来的竞态。
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let sleep_secs = match tick(&app).await {
                Ok(secs) => secs,
                Err(e) => {
                    let _ = app.emit(EVT_POLL_ERROR, e.to_string());
                    // 出错时退避，避免令牌失效后高频重试
                    15
                }
            };

            // 先把 Notify 句柄克隆出来，避免在 select! 分支里持有临时 State 引用
            let notify_flag = {
                let st = app.state::<AppState>();
                st.notify_flag.clone()
            };
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(sleep_secs.max(1))) => {}
                _ = notify_flag.notified() => {}
            }
        }
    });
}

/// 轮询开关的「上一次已广播值」，避免暂停态下每秒重复推送同一事件。
static LAST_POLL_STATE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn emit_poll_state(app: &AppHandle, running: bool) {
    if LAST_POLL_STATE.swap(running, Ordering::SeqCst) == running {
        return;
    }
    let _ = app.emit(EVT_POLL_STATE, running);
}

/// 执行一轮轮询，返回下一轮应等待的秒数。
async fn tick(app: &AppHandle) -> Result<u64, String> {
    let state = app.state::<AppState>();

    let polling = state.polling.load(Ordering::SeqCst);
    if !polling {
        emit_poll_state(app, false);
        // 暂停时不需要长时间等待，靠 notify_flag 立刻唤醒
        return Ok(1);
    }

    let (client, cfg) = {
        let cfg = state.cfg.read().await;
        match YunxiaoClient::from_config(state.http.clone(), &cfg) {
            Some(c) => (c, cfg.clone()),
            None => {
                emit_poll_state(app, false);
                return Err("尚未完成配置，轮询已跳过".into());
            }
        }
    };

    // 1) 确定要盯的流水线集合
    let mut items: Vec<PipelineSummary> = Vec::new();
    if !cfg.watched_pipeline_ids.is_empty() {
        for id in cfg.watched_pipeline_ids.iter().take(60) {
            // 单条流水线取详情失败（例如已被删除）不应中断整轮轮询，降级为占位条目
            let obj = match client.get_pipeline_raw(*id).await {
                Ok(raw) => extract_object(&raw),
                Err(_) => Value::Null,
            };
            if let Some(mut p) = PipelineSummary::from_value(&obj, true) {
                p.pipeline_id = *id;
                items.push(p);
            } else {
                items.push(PipelineSummary {
                    pipeline_id: *id,
                    name: format!("流水线 {id}"),
                    watched: true,
                    ..Default::default()
                });
            }
        }
    } else {
        let raw = client
            .list_pipelines_raw(1, cfg.auto_watch_limit.min(30) as u32, None, None)
            .await
            .map_err(|e| e.to_string())?;
        items = extract_array(&raw)
            .iter()
            .filter_map(|v| PipelineSummary::from_value(v, false))
            .collect();
    }

    // 2) 补齐最近一次运行
    fill_latest_runs(&client, &mut items).await;

    // 3) 统计 + 状态跃迁检测
    let mut running = 0usize;
    let mut failed = 0usize;
    let mut success = 0usize;
    let mut changes: Vec<StatusChange> = Vec::new();
    let mut last_map = state.last_status.lock().await;

    for p in items.iter() {
        let Some(run) = p.latest_run.as_ref() else {
            continue;
        };
        match run.status.to_ascii_uppercase().as_str() {
            "SUCCESS" => success += 1,
            "FAIL" | "FAILED" | "ERROR" => failed += 1,
            _ => {}
        }
        if run.is_active() {
            running += 1;
        }

        let key = p.pipeline_id;
        let prev = last_map.get(&key).cloned();
        let changed = match &prev {
            // 换了运行实例，或同一实例状态变了，都算跃迁
            None => false, // 首轮只记录，不通知，避免启动时刷屏
            Some(old) => old.run_id != run.pipeline_run_id || old.status != run.status,
        };

        if changed {
            if let Some(old) = prev {
                changes.push(StatusChange {
                    pipeline_id: p.pipeline_id,
                    pipeline_name: p.name.clone(),
                    pipeline_run_id: run.pipeline_run_id,
                    from_status: old.status.clone(),
                    to_status: run.status.clone(),
                    at: now_millis(),
                });
            }
        }

        last_map.insert(
            key,
            crate::state::StatusRecord {
                run_id: run.pipeline_run_id,
                status: run.status.clone(),
                pipeline_name: p.name.clone(),
            },
        );
    }
    drop(last_map);

    // 4) 推送快照
    let interval = if running > 0 {
        cfg.fast_poll_interval_secs
    } else {
        cfg.poll_interval_secs
    };
    let snapshot = PollSnapshot {
        pipelines: items.clone(),
        fetched_at: now_millis(),
        running_count: running,
        failed_count: failed,
        success_count: success,
        interval_secs: interval,
    };
    *state.snapshot.write().await = items;
    app.emit(EVT_SNAPSHOT, &snapshot).map_err(|e| e.to_string())?;
    emit_poll_state(app, true);
    update_tray_tooltip(app, &snapshot);

    // 5) 状态跃迁 → 事件 + 桌面通知
    for change in changes {
        app.emit(EVT_STATUS_CHANGED, &change)
            .map_err(|e| e.to_string())?;
        maybe_notify(app, &cfg, &change);
    }

    Ok(interval)
}

fn maybe_notify(app: &AppHandle, cfg: &AppConfig, change: &StatusChange) {
    let status = change.to_status.to_ascii_uppercase();
    let (emoji, should) = match status.as_str() {
        "SUCCESS" => ("✅", cfg.notify_on_success),
        "FAIL" | "FAILED" | "ERROR" => ("❌", cfg.notify_on_fail),
        "CANCELED" | "CANCELLED" => ("⛔", cfg.notify_on_canceled),
        _ => ("ℹ️", false),
    };
    if !should {
        return;
    }
    let title = format!("{emoji} 流水线{} - {}", status_label(&status), change.pipeline_name);
    let body = format!(
        "运行 #{}：{} → {}",
        change.pipeline_run_id,
        status_label(&change.from_status.to_ascii_uppercase()),
        status_label(&status)
    );
    let _ = app.notification().builder().title(title).body(body).show();
}

pub fn status_label(status: &str) -> String {
    match status.to_ascii_uppercase().as_str() {
        "SUCCESS" => "运行成功".into(),
        "FAIL" | "FAILED" | "ERROR" => "运行失败".into(),
        "RUNNING" => "运行中".into(),
        "WAITING" => "等待中".into(),
        "CANCELED" | "CANCELLED" => "已取消".into(),
        "INIT" => "初始化".into(),
        "QUEUED" => "排队中".into(),
        "" | "UNKNOWN" => "未知".into(),
        other => other.to_string(),
    }
}

/// 托盘提示实时反映运行态，鼠标悬停就能看到有没有在跑、有没有失败。
fn update_tray_tooltip(app: &AppHandle, snapshot: &PollSnapshot) {
    let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) else {
        return;
    };
    let tooltip = format!(
        "云效流水线监控｜运行中 {} · 成功 {} · 失败 {}",
        snapshot.running_count, snapshot.success_count, snapshot.failed_count
    );
    let _ = tray.set_tooltip(Some(&tooltip));
}
