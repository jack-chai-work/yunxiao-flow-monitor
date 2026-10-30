use crate::api::{YunxiaoClient, extract_array, extract_object, models::*};
use crate::config::{AppConfig, ConfigDto, ConfigInput};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use futures::stream::{self, StreamExt};
use serde_json::Value;
use tauri::State;
use tauri_plugin_autostart::ManagerExt;

/// 由当前配置构造客户端；配置不完整时给出明确指引。
async fn client(state: &State<'_, AppState>) -> AppResult<YunxiaoClient> {
    let cfg = state.cfg.read().await;
    YunxiaoClient::from_config(state.http.clone(), &cfg).ok_or(AppError::NotConfigured)
}

// ---------------------------------------------------------------------------
// 配置
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> AppResult<ConfigDto> {
    let cfg = state.cfg.read().await;
    Ok(ConfigDto::from(&*cfg))
}

/// 保存配置。`token` 传空字符串表示「保留原令牌」；传 `__CLEAR__` 表示清除。
#[tauri::command]
pub async fn save_config(
    state: State<'_, AppState>,
    input: ConfigInput,
) -> AppResult<ConfigDto> {
    let mut cfg = {
        let current = state.cfg.read().await;
        current.clone()
    };

    let keep_token = cfg.token.clone();
    cfg.base_url = input.base_url;
    cfg.organization_id = input.organization_id;
    cfg.poll_interval_secs = input.poll_interval_secs;
    cfg.fast_poll_interval_secs = input.fast_poll_interval_secs;
    cfg.watched_pipeline_ids = input.watched_pipeline_ids;
    cfg.auto_watch_limit = input.auto_watch_limit;
    cfg.auto_start_polling = input.auto_start_polling;
    cfg.notify_on_success = input.notify_on_success;
    cfg.notify_on_fail = input.notify_on_fail;
    cfg.notify_on_canceled = input.notify_on_canceled;
    cfg.minimize_to_tray_on_close = input.minimize_to_tray_on_close;

    let trimmed = input.token.trim().to_string();
    cfg.token = if trimmed.is_empty() {
        keep_token
    } else if trimmed == "__CLEAR__" {
        String::new()
    } else {
        trimmed
    };
    cfg.normalize();

    let store = state.store.read().await;
    store.save(&cfg)?;
    drop(store);

    *state.cfg.write().await = cfg.clone();
    state.minimize_on_close.store(
        cfg.minimize_to_tray_on_close,
        std::sync::atomic::Ordering::SeqCst,
    );
    // 配置变了，清掉历史状态，避免用旧记录做状态跃迁判断
    state.last_status.lock().await.clear();
    // 立刻唤醒轮询循环，让新配置马上生效
    state.notify_flag.notify_one();

    Ok(ConfigDto::from(&cfg))
}

/// 连通性测试：用最小分页调一次 ListPipelines，成功即代表接入点/组织/令牌都可用。
#[tauri::command]
pub async fn test_connection(state: State<'_, AppState>) -> AppResult<Value> {
    let c = client(&state).await?;
    let raw = c.list_pipelines_raw(1, 1, None, None).await?;
    let arr = extract_array(&raw);
    Ok(serde_json::json!({
        "ok": true,
        "organizationId": c.organization_id(),
        "samplePipelineName": arr.first().and_then(|v| v.get("name")).and_then(|n| n.as_str()),
        "message": "连接成功，令牌与组织 ID 均有效",
    }))
}

/// 读取是否已开启开机自启动（操作系统登录启动项的状态）。
#[tauri::command]
pub async fn get_autostart(app: tauri::AppHandle) -> AppResult<bool> {
    app.autolaunch()
        .is_enabled()
        .map_err(|e| AppError::Other(format!("读取开机启动状态失败：{e}")))
}

/// 开启 / 关闭开机自启动。Windows 下写入当前用户的 Run 注册表项，无需管理员权限。
#[tauri::command]
pub async fn set_autostart(app: tauri::AppHandle, enabled: bool) -> AppResult<bool> {
    let manager = app.autolaunch();
    let res = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    res.map_err(|e| AppError::Other(format!("设置开机启动失败：{e}")))?;
    manager
        .is_enabled()
        .map_err(|e| AppError::Other(format!("读取开机启动状态失败：{e}")))
}

// ---------------------------------------------------------------------------
// 只读查询
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelinePage {
    pub items: Vec<PipelineSummary>,
    pub total: Option<i64>,
    pub page: u32,
    pub per_page: u32,
}

#[tauri::command]
pub async fn list_pipelines(
    state: State<'_, AppState>,
    page: Option<u32>,
    per_page: Option<u32>,
    name: Option<String>,
    status_list: Option<String>,
    with_latest: Option<bool>,
) -> AppResult<PipelinePage> {
    let c = client(&state).await?;
    let page = page.unwrap_or(1).max(1);
    let per_page = per_page.unwrap_or(30).clamp(1, 30);
    let raw = c
        .list_pipelines_raw(page, per_page, name.as_deref(), status_list.as_deref())
        .await?;

    let total = raw
        .get("totalCount")
        .and_then(|v| v.as_i64())
        .or_else(|| raw.get("total").and_then(|v| v.as_i64()));

    let watched: Vec<i64> = state.cfg.read().await.watched_pipeline_ids.clone();
    let mut items: Vec<PipelineSummary> = extract_array(&raw)
        .iter()
        .filter_map(|v| PipelineSummary::from_value(v, false))
        .collect();
    for it in items.iter_mut() {
        it.watched = watched.contains(&it.pipeline_id);
    }

    if with_latest.unwrap_or(false) {
        fill_latest_runs(&c, &mut items).await;
    }

    Ok(PipelinePage {
        items,
        total,
        page,
        per_page,
    })
}

/// 取单条流水线最近一次运行；失败时返回 None，由调用方保留旧值。
async fn fetch_latest_run(c: &YunxiaoClient, pid: i64) -> Option<RunSummary> {
    let v = c.get_latest_run_raw(pid).await.ok()?;
    RunSummary::from_value(&extract_object(&v))
}

/// 并发补齐最近一次运行信息，限制并发避免打爆接口。
pub async fn fill_latest_runs(c: &YunxiaoClient, items: &mut [PipelineSummary]) {
    // 先把 (下标, 流水线 ID) 取成拥有所有权的值，避免把引用带进 async 块：
    // 否则 rustc 会因高阶生命周期推断（FnOnce is not general enough）而拒绝编译。
    let targets: Vec<(usize, i64)> = items
        .iter()
        .enumerate()
        .map(|(idx, p)| (idx, p.pipeline_id))
        .collect();

    let fetched: Vec<(usize, Option<RunSummary>)> = stream::iter(targets)
        .map(|(idx, pid)| {
            let c = c.clone();
            async move { (idx, fetch_latest_run(&c, pid).await) }
        })
        .buffer_unordered(6)
        .collect()
        .await;

    for (idx, run) in fetched {
        if let Some(items_slot) = items.get_mut(idx) {
            if run.is_some() {
                items_slot.latest_run = run;
            }
        }
    }
}

#[tauri::command]
pub async fn get_pipeline(state: State<'_, AppState>, pipeline_id: i64) -> AppResult<Value> {
    let c = client(&state).await?;
    Ok(extract_object(&c.get_pipeline_raw(pipeline_id).await?))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPage {
    pub items: Vec<RunSummary>,
    pub total: Option<i64>,
    pub page: u32,
    pub per_page: u32,
}

#[tauri::command]
pub async fn list_runs(
    state: State<'_, AppState>,
    pipeline_id: i64,
    page: Option<u32>,
    per_page: Option<u32>,
    status: Option<String>,
    trigger_mode: Option<i64>,
) -> AppResult<RunPage> {
    let c = client(&state).await?;
    let page = page.unwrap_or(1).max(1);
    let per_page = per_page.unwrap_or(20).clamp(1, 30);
    let raw = c
        .list_runs_raw(pipeline_id, page, per_page, status.as_deref(), trigger_mode)
        .await?;

    let total = raw
        .get("totalCount")
        .and_then(|v| v.as_i64())
        .or_else(|| raw.get("total").and_then(|v| v.as_i64()));

    let items = extract_array(&raw)
        .iter()
        .filter_map(|v| RunSummary::from_value(v))
        .map(|mut r| {
            if r.pipeline_id == 0 {
                r.pipeline_id = pipeline_id;
            }
            r
        })
        .collect();

    Ok(RunPage {
        items,
        total,
        page,
        per_page,
    })
}

#[tauri::command]
pub async fn get_run(
    state: State<'_, AppState>,
    pipeline_id: i64,
    pipeline_run_id: i64,
) -> AppResult<RunSummary> {
    let c = client(&state).await?;
    let raw = c.get_run_raw(pipeline_id, pipeline_run_id).await?;
    let obj = extract_object(&raw);
    let mut run = RunSummary::from_value(&obj).ok_or_else(|| {
        AppError::Parse(format!(
            "无法解析运行实例响应：{}",
            obj.to_string().chars().take(300).collect::<String>()
        ))
    })?;
    if run.pipeline_id == 0 {
        run.pipeline_id = pipeline_id;
    }
    if run.pipeline_run_id == 0 {
        run.pipeline_run_id = pipeline_run_id;
    }
    Ok(run)
}

#[tauri::command]
pub async fn get_latest_run(state: State<'_, AppState>, pipeline_id: i64) -> AppResult<RunSummary> {
    let c = client(&state).await?;
    let raw = c.get_latest_run_raw(pipeline_id).await?;
    let obj = extract_object(&raw);
    RunSummary::from_value(&obj).ok_or_else(|| AppError::Other("该流水线暂无运行记录".into()))
}

#[tauri::command]
pub async fn get_job_steps(
    state: State<'_, AppState>,
    pipeline_id: i64,
    pipeline_run_id: i64,
    job_id: i64,
) -> AppResult<JobSteps> {
    let c = client(&state).await?;
    let raw = c
        .get_job_steps_raw(pipeline_id, pipeline_run_id, job_id)
        .await?;
    Ok(JobSteps::from_value(&extract_object(&raw), job_id))
}

#[tauri::command]
pub async fn get_job_step_log(
    state: State<'_, AppState>,
    pipeline_id: i64,
    pipeline_run_id: i64,
    job_id: i64,
    step_index: i64,
    build_id: i64,
    offset: Option<i64>,
    limit: Option<i64>,
) -> AppResult<LogChunk> {
    let c = client(&state).await?;
    let raw = c
        .get_job_step_log_raw(
            pipeline_id,
            pipeline_run_id,
            job_id,
            step_index,
            build_id,
            offset.unwrap_or(0),
            limit.unwrap_or(50_000),
        )
        .await?;
    Ok(LogChunk::from_value(&extract_object(&raw)))
}

#[tauri::command]
pub async fn get_job_step_log_url(
    state: State<'_, AppState>,
    pipeline_id: i64,
    pipeline_run_id: i64,
    job_id: i64,
    step_index: i64,
    build_id: i64,
) -> AppResult<Value> {
    let c = client(&state).await?;
    c.get_job_step_log_url_raw(pipeline_id, pipeline_run_id, job_id, step_index, build_id)
        .await
}

// ---------------------------------------------------------------------------
// 写操作（运行控制）
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn start_run(
    state: State<'_, AppState>,
    pipeline_id: i64,
    params: Option<Value>,
) -> AppResult<i64> {
    let c = client(&state).await?;
    let run_id = c.start_run(pipeline_id, params).await?;
    state.notify_flag.notify_one();
    Ok(run_id)
}

#[tauri::command]
pub async fn stop_run(
    state: State<'_, AppState>,
    pipeline_id: i64,
    pipeline_run_id: i64,
) -> AppResult<bool> {
    let c = client(&state).await?;
    let ok = c.stop_run(pipeline_id, pipeline_run_id).await?;
    state.notify_flag.notify_one();
    Ok(ok)
}

#[tauri::command]
pub async fn start_job(
    state: State<'_, AppState>,
    pipeline_id: i64,
    pipeline_run_id: i64,
    job_id: i64,
) -> AppResult<bool> {
    let c = client(&state).await?;
    let ok = c.start_job(pipeline_id, pipeline_run_id, job_id).await?;
    state.notify_flag.notify_one();
    Ok(ok)
}

#[tauri::command]
pub async fn stop_job(
    state: State<'_, AppState>,
    pipeline_id: i64,
    pipeline_run_id: i64,
    job_id: i64,
) -> AppResult<bool> {
    let c = client(&state).await?;
    let ok = c.stop_job(pipeline_id, pipeline_run_id, job_id).await?;
    state.notify_flag.notify_one();
    Ok(ok)
}

#[tauri::command]
pub async fn skip_job(
    state: State<'_, AppState>,
    pipeline_id: i64,
    pipeline_run_id: i64,
    job_id: i64,
) -> AppResult<bool> {
    let c = client(&state).await?;
    let ok = c.skip_job(pipeline_id, pipeline_run_id, job_id).await?;
    state.notify_flag.notify_one();
    Ok(ok)
}

// ---------------------------------------------------------------------------
// 轮询控制 / 关注列表
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn set_polling(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> AppResult<bool> {
    state
        .polling
        .store(enabled, std::sync::atomic::Ordering::SeqCst);
    if enabled {
        state.notify_flag.notify_one();
    }
    crate::tray::sync_toggle_label(&app);
    Ok(enabled)
}

#[tauri::command]
pub async fn get_polling(state: State<'_, AppState>) -> AppResult<bool> {
    Ok(state.polling.load(std::sync::atomic::Ordering::SeqCst))
}

/// 手动触发一次立即轮询，不阻塞等待结果。
#[tauri::command]
pub async fn refresh_now(state: State<'_, AppState>) -> AppResult<()> {
    state.notify_flag.notify_one();
    Ok(())
}

/// 取最近一次轮询快照，前端启动时先用它渲染，避免白屏等待。
#[tauri::command]
pub async fn get_snapshot(state: State<'_, AppState>) -> AppResult<Vec<PipelineSummary>> {
    Ok(state.snapshot.read().await.clone())
}

#[tauri::command]
pub async fn toggle_watch(state: State<'_, AppState>, pipeline_id: i64) -> AppResult<Vec<i64>> {
    let mut cfg: AppConfig = { state.cfg.read().await.clone() };
    if let Some(pos) = cfg
        .watched_pipeline_ids
        .iter()
        .position(|id| *id == pipeline_id)
    {
        cfg.watched_pipeline_ids.remove(pos);
    } else {
        cfg.watched_pipeline_ids.push(pipeline_id);
    }
    let ids = cfg.watched_pipeline_ids.clone();

    let store = state.store.read().await;
    store.save(&cfg)?;
    drop(store);

    *state.cfg.write().await = cfg;
    state.notify_flag.notify_one();
    Ok(ids)
}
