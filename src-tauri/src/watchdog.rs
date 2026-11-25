use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration as StdDuration, Instant};

use chrono::Local;
use tauri::Manager;

use crate::commands::{self, AnimationProbeResult, AppState};
use crate::{animation, system_monitor};

const FIRST_CHECK_DELAY: StdDuration = StdDuration::from_secs(60);
const CHECK_INTERVAL: StdDuration = StdDuration::from_secs(10 * 60);
const PROBE_TIMEOUT: StdDuration = StdDuration::from_secs(5);
const RESTART_AFTER_SEVERE_FAILURES: u32 = 2;
const MAX_LOG_BYTES: u64 = 1_000_000;

struct HealthOutcome {
    severe_failure: bool,
}

pub fn start(app: tauri::AppHandle) {
    thread::spawn(move || {
        log_line("守护线程已启动：启动后 1 分钟做第一次检查，之后每 10 分钟检查一次。");
        log_line(
            "保护规则：守护检查不会修改提醒内容、提醒时间、循环设置，也不会删除任何提醒事件。",
        );
        thread::sleep(FIRST_CHECK_DELAY);

        let mut check_number = 1_u64;
        let mut consecutive_severe_failures = 0_u32;

        loop {
            let outcome = run_health_check(&app, check_number);
            if outcome.severe_failure {
                consecutive_severe_failures += 1;
                log_line(&format!(
                    "本轮仍有严重问题，连续严重失败次数：{}。",
                    consecutive_severe_failures
                ));
            } else {
                if consecutive_severe_failures > 0 {
                    log_line("严重问题已恢复，连续失败计数清零。");
                }
                consecutive_severe_failures = 0;
            }

            if consecutive_severe_failures >= RESTART_AFTER_SEVERE_FAILURES {
                log_line("连续两轮健康检查失败，准备执行程序自我重启。");
                match restart_app(&app) {
                    Ok(()) => break,
                    Err(error) => {
                        log_line(&format!("自我重启失败：{}。守护线程会继续检查。", error));
                        consecutive_severe_failures = 0;
                    }
                }
            }

            check_number += 1;
            thread::sleep(CHECK_INTERVAL);
        }
    });
}

fn run_health_check(app: &tauri::AppHandle, check_number: u64) -> HealthOutcome {
    let mut severe_failure = false;
    let now = Local::now();

    log_line(&format!(
        "========== 第 {} 次健康检查开始 ==========",
        check_number
    ));
    log_line("主程序：正常，守护线程还能运行，说明主进程没有卡死。");

    let state = app.state::<AppState>();

    match state.scheduler.repository().health_check() {
        Ok(health) if health.is_ok() => {
            log_line(&format!(
                "数据库：正常。自检结果：{}；提醒总数 {}，待提醒 {}，循环中 {}，已完成 {}，已错过 {}，已停止 {}。",
                health.quick_check_message,
                health.total,
                health.pending,
                health.running,
                health.completed,
                health.missed,
                health.stopped
            ));
        }
        Ok(health) => {
            log_line(&format!(
                "数据库：发现异常。自检结果：{}；外键问题 {} 个。开始尝试安全修复。",
                health.quick_check_message, health.foreign_key_issues
            ));
            match state.scheduler.repository().repair_lightly() {
                Ok(()) => match state.scheduler.repository().health_check() {
                    Ok(rechecked) if rechecked.is_ok() => {
                        log_line("数据库：安全修复成功，复查已经正常。");
                    }
                    Ok(rechecked) => {
                        log_line(&format!(
                            "数据库：安全修复后仍有问题。自检结果：{}；外键问题 {} 个。需要保留现场，暂不删除任何提醒。",
                            rechecked.quick_check_message, rechecked.foreign_key_issues
                        ));
                        severe_failure = true;
                    }
                    Err(error) => {
                        log_line(&format!("数据库：修复后复查失败：{}。", error));
                        severe_failure = true;
                    }
                },
                Err(error) => {
                    log_line(&format!("数据库：安全修复失败：{}。", error));
                    severe_failure = true;
                }
            }
        }
        Err(error) => {
            log_line(&format!("数据库：无法完成体检：{}。", error));
            severe_failure = true;
        }
    }

    match state.scheduler.health_snapshot(now) {
        Ok(health) => log_line(&format!(
            "调度队列：排队 {} 个，已认领 {} 个，{}。",
            health.queued,
            health.claimed,
            match (health.active_id, health.active_for_seconds) {
                (Some(id), Some(seconds)) => format!("正在播放提醒 #{id}，已经持续 {seconds} 秒"),
                _ => "当前没有正在播放的提醒".to_string(),
            }
        )),
        Err(error) => {
            log_line(&format!("调度队列：读取状态失败：{}。", error));
            severe_failure = true;
        }
    }

    match commands::process_scheduler(app) {
        Ok(()) => log_line("调度器：已主动扫描一次提醒，并尝试恢复卡住的动画状态。"),
        Err(error) => {
            log_line(&format!("调度器：扫描失败：{}。", error));
            severe_failure = true;
        }
    }

    if !check_window_exists(app, "reminders", "提醒列表窗口") {
        severe_failure = true;
    }
    if !check_window_exists(app, "add-reminder", "新增/编辑窗口") {
        severe_failure = true;
    }
    if !check_window_exists(app, "rocket-banner", "小火箭透明动画窗口") {
        severe_failure = true;
    }

    match state.scheduler.health_snapshot(Local::now()) {
        Ok(health) if health.active_id.is_some() || health.queued > 0 => {
            log_line(
                "隐藏动画页面测试：当前有真实提醒正在播放或排队，跳过本次测试，避免打断用户提醒。",
            );
        }
        Ok(_) if system_monitor::is_workstation_locked() => {
            log_line("隐藏动画页面测试：电脑当前处于锁屏状态，跳过本次测试。");
        }
        Ok(_) => match run_animation_probe(app) {
            Ok(result) if result.ok => {
                log_line(&format!("隐藏动画页面测试：正常。{}", result.message));
            }
            Ok(result) => {
                log_line(&format!("隐藏动画页面测试：失败。{}", result.message));
                severe_failure = true;
            }
            Err(error) => {
                log_line(&format!("隐藏动画页面测试：失败。{}", error));
                severe_failure = true;
            }
        },
        Err(error) => {
            log_line(&format!(
                "隐藏动画页面测试：无法读取调度状态，已跳过：{}。",
                error
            ));
            severe_failure = true;
        }
    }

    if severe_failure {
        log_line("本轮结论：发现严重问题，已尽力修复；如果连续两轮仍失败，会自动重启程序。");
    } else {
        log_line("本轮结论：正常，不需要重启。");
    }
    log_line("========== 健康检查结束 ==========");

    HealthOutcome { severe_failure }
}

fn check_window_exists(app: &tauri::AppHandle, label: &str, name: &str) -> bool {
    if app.get_webview_window(label).is_some() {
        log_line(&format!("窗口：{} 正常存在。", name));
        true
    } else {
        log_line(&format!("窗口：{} 不存在，准备依靠重启恢复。", name));
        false
    }
}

fn run_animation_probe(app: &tauri::AppHandle) -> Result<AnimationProbeResult, String> {
    let id = format!("probe-{}", Local::now().timestamp_millis());
    commands::clear_animation_probe(app)?;
    animation::probe(app, &id)?;

    let started = Instant::now();
    loop {
        if let Some(result) = commands::latest_animation_probe(app, &id)? {
            return Ok(result);
        }
        if started.elapsed() >= PROBE_TIMEOUT {
            return Err("隐藏动画页面 5 秒内没有回应".to_string());
        }
        thread::sleep(StdDuration::from_millis(100));
    }
}

fn restart_app(app: &tauri::AppHandle) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    spawn_delayed_restart(&exe)?;
    log_line("已发出重启命令：3 秒后重新启动程序，当前进程即将退出。");
    app.exit(0);
    Ok(())
}

#[cfg(target_os = "windows")]
fn spawn_delayed_restart(exe: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let exe = exe.to_string_lossy().replace('\'', "''");
    let script = format!(
        "Start-Sleep -Seconds 3; Start-Process -FilePath '{}' -ArgumentList '--autostart','--watchdog-restart'",
        exe
    );
    Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn spawn_delayed_restart(exe: &Path) -> Result<(), String> {
    Command::new(exe)
        .args(["--autostart", "--watchdog-restart"])
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn log_path() -> PathBuf {
    install_dir().join("rocket-reminder-watchdog.log")
}

fn install_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn log_line(message: &str) {
    let _ = write_log_line(message);
}

fn write_log_line(message: &str) -> std::io::Result<()> {
    let path = log_path();
    rotate_log_if_needed(&path)?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(
        file,
        "[{}] {}",
        Local::now().format("%Y-%m-%d %H:%M:%S"),
        message
    )
}

fn rotate_log_if_needed(path: &Path) -> std::io::Result<()> {
    if fs::metadata(path)
        .map(|metadata| metadata.len() <= MAX_LOG_BYTES)
        .unwrap_or(true)
    {
        return Ok(());
    }
    let old_path = path.with_extension("log.old");
    let _ = fs::remove_file(&old_path);
    fs::rename(path, old_path)
}
