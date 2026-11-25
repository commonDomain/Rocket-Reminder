use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, RunEvent};

use crate::commands::show_window;

pub fn create(app: &tauri::AppHandle) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text("reminders", "提醒事项")
        .text("exit", "退出")
        .build()?;
    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("提醒事项")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "reminders" => {
                let _ = show_window(app, "reminders");
            }
            "exit" if confirm_exit() => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                let _ = show_window(tray.app_handle(), "reminders");
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub fn handle_run_event(app: &tauri::AppHandle, event: &RunEvent) {
    if let RunEvent::WindowEvent { label, event, .. } = event {
        if label == "reminders" || label == "add-reminder" {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Some(window) = app.get_webview_window(label) {
                    let _ = window.hide();
                    if label == "reminders" {
                        let _ = window.set_skip_taskbar(true);
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn confirm_exit() -> bool {
    use std::ptr::null_mut;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDOK, MB_ICONQUESTION, MB_OKCANCEL,
    };

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
    unsafe {
        MessageBoxW(
            null_mut(),
            wide("确定要退出提醒程序吗？开机自动启动设置不会改变。").as_ptr(),
            wide("退出提醒事项").as_ptr(),
            MB_OKCANCEL | MB_ICONQUESTION,
        ) == IDOK
    }
}

#[cfg(not(target_os = "windows"))]
fn confirm_exit() -> bool {
    true
}
