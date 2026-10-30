use crate::poller;
use crate::state::AppState;
use std::sync::atomic::Ordering;
use tauri::{
    AppHandle, Emitter, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

pub const TRAY_ID: &str = "yunxiao-main-tray";

/// 需要在运行期动态改文案的托盘菜单项。
pub struct TrayItems {
    pub toggle: MenuItem<tauri::Wry>,
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主界面", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "立即刷新", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "暂停轮询", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let polling_now = app
        .try_state::<AppState>()
        .map(|s| s.polling.load(Ordering::SeqCst))
        .unwrap_or(false);
    toggle.set_text(if polling_now { "暂停轮询" } else { "开始轮询" })?;

    let menu = Menu::new(app)?;
    menu.append_items(&[&show, &refresh, &toggle, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("云效流水线监控")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "refresh" => {
                if let Some(s) = app.try_state::<AppState>() {
                    s.polling.store(true, Ordering::SeqCst);
                    sync_toggle_label(app);
                    s.notify_flag.notify_one();
                }
            }
            "toggle" => {
                if let Some(s) = app.try_state::<AppState>() {
                    let next = !s.polling.load(Ordering::SeqCst);
                    s.polling.store(next, Ordering::SeqCst);
                    if next {
                        s.notify_flag.notify_one();
                    }
                    sync_toggle_label(app);
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 左键单击托盘图标 → 唤出主窗口（macOS 习惯用左键弹菜单，这里统一为唤窗）
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    app.manage(TrayItems { toggle });
    Ok(())
}

/// 让托盘菜单里的「开始/暂停轮询」文案与实际状态保持一致。
pub fn sync_toggle_label(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let polling = state.polling.load(Ordering::SeqCst);
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items
            .toggle
            .set_text(if polling { "暂停轮询" } else { "开始轮询" });
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(if polling {
            "云效流水线监控｜轮询中"
        } else {
            "云效流水线监控｜已暂停"
        }));
    }
    let _ = app.emit(poller::EVT_POLL_STATE, polling);
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}
