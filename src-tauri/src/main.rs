// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    {
        std::env::remove_var("SESSION_MANAGER");
        // WebKitGTK's GPU rasteriser scrolls choppily on some Wayland and
        // hybrid-GPU setups; CPU rendering is smooth for a UI this light.
        // Set before the webview starts; a value the user chose wins.
        if std::env::var_os("WEBKIT_SKIA_ENABLE_CPU_RENDERING").is_none() {
            std::env::set_var("WEBKIT_SKIA_ENABLE_CPU_RENDERING", "1");
        }
    }

    parolassh_lib::run()
}
