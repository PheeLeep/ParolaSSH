// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {

    #[cfg(target_os = "linux")]
    std::env::remove_var("SESSION_MANAGER");

    parolassh_lib::run()
}
