pub mod animation;
pub mod commands;
pub mod db;
pub mod model;
pub mod scheduler;
pub mod system_monitor;
pub mod tray;
pub mod watchdog;

pub fn run() {
    use std::time::Duration;

    use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

    let arguments = std::env::args().collect::<Vec<_>>();
    let started_from_autostart = arguments.iter().any(|argument| argument == "--autostart");
    let started_from_watchdog_restart = arguments
        .iter()
        .any(|argument| argument == "--watchdog-restart");
    let manual_start = !started_from_autostart && !started_from_watchdog_restart;
    let state = commands::build_state().expect("failed to initialize reminder database");
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, arguments, _| {
            let manual_start = !arguments
                .iter()
                .any(|argument| argument == "--autostart" || argument == "--watchdog-restart");
            if manual_start {
                let _ = commands::show_window(app, "reminders");
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::list_reminders,
            commands::create_reminder,
            commands::update_reminder,
            commands::delete_reminder,
            commands::stop_loop,
            commands::resume_loop,
            commands::show_add_window,
            commands::show_edit_window,
            commands::hide_add_window,
            commands::complete_animation,
            commands::record_animation_probe
        ])
        .setup(move |app| {
            tray::create(app.handle())?;
            let autolaunch = app.autolaunch();
            let _ = autolaunch.enable();
            if manual_start {
                let _ = commands::show_window(app.handle(), "reminders");
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                let _ = commands::process_scheduler(&handle);
                std::thread::sleep(Duration::from_secs(1));
            });
            watchdog::start(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to run rocket reminder")
        .run(|app, event| tray::handle_run_event(app, &event));
}
