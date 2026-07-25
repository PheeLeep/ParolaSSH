mod commands;
// Public so the integration tests in `tests/` can drive the audit against
// fixture directories.
pub mod ssh;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::ssh_location,
            commands::list_ssh_keys,
            commands::audit_ssh_dir,
            commands::set_finding_suppressed,
            commands::restrict_permissions,
            commands::generate_ssh_key,
            commands::delete_ssh_key,
            commands::verify_key_passphrase,
            commands::read_public_key,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
