use std::sync::Arc;

use chrono::Local;
use serde::Serialize;
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize};

use crate::scheduler::SchedulerService;

#[derive(Clone, Serialize)]
pub struct AnimationPayload {
    pub id: i64,
    pub content: String,
}

#[derive(Clone, Serialize)]
struct ProbePayload {
    id: String,
}

pub fn play_next(app: &tauri::AppHandle, scheduler: &Arc<SchedulerService>) -> Result<(), String> {
    let Some(reminder) = scheduler
        .next_animation(Local::now())
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let id = reminder.id;
    let result = (|| {
        let window = app
            .get_webview_window("rocket-banner")
            .ok_or_else(|| "找不到火箭提醒窗口".to_string())?;
        let monitor = window
            .primary_monitor()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "找不到主显示器".to_string())?;
        let size = monitor.size();
        let position = monitor.position();
        window
            .set_size(PhysicalSize::new(size.width, size.height))
            .map_err(|error| error.to_string())?;
        window
            .set_position(PhysicalPosition::new(position.x, position.y))
            .map_err(|error| error.to_string())?;
        window
            .set_ignore_cursor_events(true)
            .map_err(|error| error.to_string())?;
        window.show().map_err(|error| error.to_string())?;
        window
            .emit(
                "play-reminder",
                AnimationPayload {
                    id: reminder.id,
                    content: reminder.content,
                },
            )
            .map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = scheduler.abandon_active(id);
    }
    result
}

pub fn probe(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let window = app
        .get_webview_window("rocket-banner")
        .ok_or_else(|| "找不到火箭提醒窗口".to_string())?;
    let monitor = window
        .primary_monitor()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "找不到主显示器".to_string())?;
    let size = monitor.size();
    let position = monitor.position();
    window
        .set_size(PhysicalSize::new(size.width, size.height))
        .map_err(|error| error.to_string())?;
    window
        .set_position(PhysicalPosition::new(position.x, position.y))
        .map_err(|error| error.to_string())?;
    window
        .set_ignore_cursor_events(true)
        .map_err(|error| error.to_string())?;
    window
        .emit("watchdog-probe", ProbePayload { id: id.to_string() })
        .map_err(|error| error.to_string())
}

pub fn hide(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("rocket-banner") {
        let _ = window.emit("cancel-reminder", ());
        let _ = window.hide();
    }
}
