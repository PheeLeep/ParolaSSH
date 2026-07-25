mod app_paths;
mod commands;
pub mod hosts;
pub mod remote;
// Public so the integration tests in `tests/` can drive the audit against
// fixture directories.
pub mod ssh;

use remote::registry::SessionRegistry;
use remote::secrets::SecretVault;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();

    // Only ever one ParolaSSH. A second copy would hold its own registry of
    // SSH sessions and its own copy of any remembered password, and would
    // race the first over `hosts.json` — two windows editing the same list
    // with last-write-wins is how saved connections quietly disappear.
    //
    // Registered before every other plugin, as the plugin requires: launching
    // again hands the arguments to the running instance and returns.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        // Treat a second launch as "show me the one I already have".
        if let Some(window) = <tauri::AppHandle as tauri::Manager<_>>::get_webview_window(app, "main")
        {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }));

    builder
        .plugin(tauri_plugin_opener::init())
        // Live SSH sessions and in-memory passwords. Both are process-scoped
        // on purpose: quitting the app drops every credential it holds.
        .manage(SessionRegistry::new())
        .manage(SecretVault::new())
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
            // Saved connections
            hosts::commands::list_hosts,
            hosts::commands::save_host,
            hosts::commands::delete_host,
            hosts::commands::list_host_groups,
            hosts::commands::list_host_tags,
            // Reaching them
            remote::commands::probe_host,
            remote::commands::connect_host,
            remote::commands::disconnect_host,
            remote::commands::connected_hosts,
            remote::commands::has_remembered_password,
            remote::commands::forget_password,
            remote::commands::privilege_report,
            remote::commands::preview_power,
            remote::commands::power_host,
            remote::commands::heartbeat,
            // Interactive terminal
            remote::commands::open_shell,
            remote::commands::write_shell,
            remote::commands::resize_shell,
            remote::commands::close_shell,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            // Close every SSH session on the way out rather than letting the
            // process exit drop the sockets — the remote sshd then logs a
            // clean disconnect instead of a broken pipe.
            if let tauri::RunEvent::Exit = event {
                let registry = <tauri::AppHandle as tauri::Manager<_>>::state::<SessionRegistry>(app);
                tauri::async_runtime::block_on(registry.disconnect_all());
            }
        });
}
