#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    rocket_reminder_lib::run();
}
