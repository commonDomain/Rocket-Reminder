use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use chrono::Local;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

use crate::animation;
use crate::db::ReminderRepository;
use crate::model::{
    initial_loop_occurrence, schedule_with_repeat, validate_content, validate_loop_config,
    LoopConfig, Reminder, RepeatType,
};
use crate::scheduler::SchedulerService;

pub struct AppState {
    pub scheduler: Arc<SchedulerService>,
    pub locked: AtomicBool,
    pub animation_probe: Mutex<Option<AnimationProbeResult>>,
}

#[derive(Debug, Clone)]
pub struct AnimationProbeResult {
    pub id: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateReminderInput {
    pub content: String,
    pub time: String,
    pub repeat_type: RepeatType,
    pub loop_enabled: bool,
    pub loop_hours: Option<i64>,
    pub loop_minutes: Option<i64>,
    pub loop_start_time: Option<String>,
    pub loop_end_time: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReminderEditorPayload {
    mode: &'static str,
    reminder: Option<Reminder>,
}

pub fn database_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let install_dir = exe
        .parent()
        .ok_or_else(|| "无法确定程序安装目录".to_string())?;
    Ok(install_dir.join("reminders.db"))
}

pub fn build_state() -> Result<AppState, String> {
    let repo = ReminderRepository::open(&database_path()?).map_err(|error| error.to_string())?;
    Ok(AppState {
        scheduler: Arc::new(SchedulerService::new(Arc::new(repo))),
        locked: AtomicBool::new(false),
        animation_probe: Mutex::new(None),
    })
}

#[tauri::command]
pub fn list_reminders(
    state: State<'_, AppState>,
    include_history: bool,
) -> Result<Vec<Reminder>, String> {
    state
        .scheduler
        .repository()
        .list(include_history)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn create_reminder(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: CreateReminderInput,
) -> Result<Reminder, String> {
    validate_content(&input.content).map_err(|error| error.to_string())?;
    let now = Local::now();
    let loop_config = input.loop_config().map_err(|error| error.to_string())?;
    let scheduled_at = input
        .scheduled_at(loop_config.as_ref(), now)
        .map_err(|error| error.to_string())?;
    let reminder = state
        .scheduler
        .repository()
        .insert_pending(
            input.content.trim(),
            scheduled_at,
            input.repeat_type,
            loop_config,
        )
        .map_err(|error| error.to_string())?;
    process_scheduler(&app)?;
    emit_changed(&app);
    Ok(reminder)
}

#[tauri::command]
pub fn update_reminder(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: i64,
    input: CreateReminderInput,
) -> Result<Reminder, String> {
    validate_content(&input.content).map_err(|error| error.to_string())?;
    let now = Local::now();
    let loop_config = input.loop_config().map_err(|error| error.to_string())?;
    let scheduled_at = input
        .scheduled_at(loop_config.as_ref(), now)
        .map_err(|error| error.to_string())?;
    let was_active = state
        .scheduler
        .remove_claim(id)
        .map_err(|error| error.to_string())?;
    let reminder = state
        .scheduler
        .repository()
        .update_pending(
            id,
            input.content.trim(),
            scheduled_at,
            input.repeat_type,
            loop_config,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "只能编辑未完成的提醒事项".to_string())?;
    if was_active {
        animation::hide(&app);
    }
    emit_changed(&app);
    process_scheduler(&app)?;
    Ok(reminder)
}

#[tauri::command]
pub fn delete_reminder(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let was_active = state
        .scheduler
        .remove_claim(id)
        .map_err(|error| error.to_string())?;
    state
        .scheduler
        .repository()
        .delete(id)
        .map_err(|error| error.to_string())?;
    if was_active {
        animation::hide(&app);
        animation::play_next(&app, &state.scheduler)?;
    }
    emit_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn stop_loop(app: tauri::AppHandle, state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let (changed, was_active) = state
        .scheduler
        .stop_loop(id, Local::now())
        .map_err(|error| error.to_string())?;
    if was_active {
        animation::hide(&app);
        animation::play_next(&app, &state.scheduler)?;
    }
    if changed {
        emit_changed(&app);
    }
    Ok(())
}

#[tauri::command]
pub fn resume_loop(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    if state
        .scheduler
        .resume_loop(id, Local::now())
        .map_err(|error| error.to_string())?
    {
        emit_changed(&app);
        process_scheduler(&app)?;
    }
    Ok(())
}

#[tauri::command]
pub fn show_add_window(app: tauri::AppHandle) -> Result<(), String> {
    show_window(&app, "add-reminder")?;
    emit_editor_payload(&app, "create", None);
    Ok(())
}

#[tauri::command]
pub fn show_edit_window(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let reminder = state
        .scheduler
        .repository()
        .get(id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "找不到需要编辑的提醒事项".to_string())?;
    if !matches!(
        reminder.status,
        crate::model::ReminderStatus::Pending
            | crate::model::ReminderStatus::Running
            | crate::model::ReminderStatus::Stopped
    ) {
        return Err("只能编辑未完成的提醒事项".to_string());
    }
    show_window(&app, "add-reminder")?;
    emit_editor_payload(&app, "edit", Some(reminder));
    Ok(())
}

#[tauri::command]
pub fn hide_add_window(app: tauri::AppHandle) -> Result<(), String> {
    app.get_webview_window("add-reminder")
        .ok_or_else(|| "找不到新增提醒窗口".to_string())?
        .hide()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn complete_animation(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let completed = state
        .scheduler
        .complete_active(id, Local::now())
        .map_err(|error| error.to_string())?;
    if completed {
        emit_changed(&app);
        animation::hide(&app);
        animation::play_next(&app, &state.scheduler)?;
    }
    Ok(())
}

#[tauri::command]
pub fn record_animation_probe(
    state: State<'_, AppState>,
    id: String,
    ok: bool,
    message: String,
) -> Result<(), String> {
    let mut probe = state
        .animation_probe
        .lock()
        .map_err(|_| "无法记录动画自检结果，内部状态被占用".to_string())?;
    *probe = Some(AnimationProbeResult { id, ok, message });
    Ok(())
}

pub fn clear_animation_probe(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut probe = state
        .animation_probe
        .lock()
        .map_err(|_| "无法清空动画自检结果，内部状态被占用".to_string())?;
    *probe = None;
    Ok(())
}

pub fn latest_animation_probe(
    app: &tauri::AppHandle,
    id: &str,
) -> Result<Option<AnimationProbeResult>, String> {
    let state = app.state::<AppState>();
    let probe = state
        .animation_probe
        .lock()
        .map_err(|_| "无法读取动画自检结果，内部状态被占用".to_string())?;
    Ok(probe.as_ref().filter(|result| result.id == id).cloned())
}

pub fn process_scheduler(app: &tauri::AppHandle) -> Result<(), String> {
    use std::sync::atomic::Ordering;

    let state = app.state::<AppState>();
    let locked = crate::system_monitor::is_workstation_locked();
    let was_locked = state.locked.swap(locked, Ordering::Relaxed);
    let now = Local::now();
    let mut changed = false;
    if locked {
        animation::hide(app);
        changed |= state
            .scheduler
            .cancel_claimed_as_missed(now)
            .map_err(|error| error.to_string())?;
    }
    changed |= state
        .scheduler
        .tick(now, locked)
        .map_err(|error| error.to_string())?;
    if changed || was_locked != locked {
        emit_changed(app);
    }
    if !locked {
        animation::play_next(app, &state.scheduler)?;
    }
    Ok(())
}

pub fn show_window(app: &tauri::AppHandle, label: &str) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("找不到窗口：{label}"))?;
    let _ = window.unminimize();
    let _ = window.center();
    if label == "reminders" {
        let _ = window.set_skip_taskbar(false);
    }
    window.show().map_err(|error| error.to_string())?;
    bring_to_front(&window);
    let _ = window.set_focus();
    Ok(())
}

#[cfg(target_os = "windows")]
fn bring_to_front(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, SetForegroundWindow, SetWindowPos, ShowWindowAsync, HWND_NOTOPMOST,
        HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE,
    };

    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            ShowWindowAsync(hwnd.0, SW_RESTORE);
            SetWindowPos(
                hwnd.0,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            );
            SetWindowPos(
                hwnd.0,
                HWND_NOTOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            );
            BringWindowToTop(hwnd.0);
            SetForegroundWindow(hwnd.0);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn bring_to_front(_window: &tauri::WebviewWindow) {}

pub fn emit_changed(app: &tauri::AppHandle) {
    let _ = app.emit("reminders-changed", ());
}

impl CreateReminderInput {
    fn scheduled_at(
        &self,
        loop_config: Option<&LoopConfig>,
        now: chrono::DateTime<Local>,
    ) -> Result<chrono::DateTime<Local>, crate::model::ValidationError> {
        if let Some(config) = loop_config {
            initial_loop_occurrence(config, self.repeat_type, now)
        } else {
            schedule_with_repeat(&self.time, self.repeat_type, now)
        }
    }

    fn loop_config(&self) -> Result<Option<LoopConfig>, crate::model::ValidationError> {
        if !self.loop_enabled {
            return Ok(None);
        }
        let config = LoopConfig {
            interval_minutes: self.loop_hours.unwrap_or(0) * 60 + self.loop_minutes.unwrap_or(0),
            start_time: self.loop_start_time.clone().unwrap_or_default(),
            end_time: self.loop_end_time.clone().unwrap_or_default(),
        };
        validate_loop_config(&config)?;
        Ok(Some(config))
    }
}

fn emit_editor_payload(app: &tauri::AppHandle, mode: &'static str, reminder: Option<Reminder>) {
    if let Some(window) = app.get_webview_window("add-reminder") {
        let _ = window.emit("reminder-editor", ReminderEditorPayload { mode, reminder });
    }
}
