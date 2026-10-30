mod api;
mod commands;
mod config;
mod error;
mod poller;
mod state;
mod tray;

use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let handle = app.handle().clone();

            // 配置目录：Windows → %APPDATA%/<identifier>，Linux → ~/.config/<identifier>
            let dir = app
                .path()
                .app_config_dir()
                .map_err(|e| format!("无法定位应用配置目录：{e}"))?;
            let store = config::ConfigStore::new(dir);
            let cfg = store.load().unwrap_or_default();

            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(10))
                .user_agent(concat!(
                    "yunxiao-flow-monitor/",
                    env!("CARGO_PKG_VERSION")
                ))
                .build()
                .map_err(|e| format!("初始化 HTTP 客户端失败：{e}"))?;

            // 只有配置齐备且用户开启了自动轮询，启动时才直接进入轮询
            let auto_start = cfg.auto_start_polling && cfg.is_ready();
            let app_state = state::AppState::new(http, store, cfg);
            app_state.polling.store(auto_start, Ordering::SeqCst);
            app.manage(app_state);

            tray::setup(&handle)?;
            tray::sync_toggle_label(&handle);
            poller::spawn(handle);

            Ok(())
        })
        .on_window_event(|window, event| {
            // 关闭窗口默认最小化到托盘，保持后台监控不中断；真正退出走托盘菜单
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() != "main" {
                    return;
                }
                let minimize = window
                    .app_handle()
                    .try_state::<state::AppState>()
                    .map(|s| s.minimize_on_close.load(Ordering::SeqCst))
                    .unwrap_or(true);
                if minimize {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            // 配置
            commands::get_config,
            commands::save_config,
            commands::test_connection,
            commands::get_autostart,
            commands::set_autostart,
            // 只读查询
            commands::list_pipelines,
            commands::get_pipeline,
            commands::list_runs,
            commands::get_run,
            commands::get_latest_run,
            commands::get_job_steps,
            commands::get_job_step_log,
            commands::get_job_step_log_url,
            // 运行控制
            commands::start_run,
            commands::stop_run,
            commands::start_job,
            commands::stop_job,
            commands::skip_job,
            // 轮询与关注
            commands::set_polling,
            commands::get_polling,
            commands::refresh_now,
            commands::get_snapshot,
            commands::toggle_watch,
        ])
        .run(tauri::generate_context!())
        .expect("启动云效流水线监控失败");
}
