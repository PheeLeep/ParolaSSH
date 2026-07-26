mod app_paths;
mod commands;
pub mod hosts;
mod private_file;
pub mod remote;
// Public so the integration tests in `tests/` can drive the audit against
// fixture directories.
pub mod ssh;
pub mod vpn;

use std::sync::Arc;

use remote::registry::SessionRegistry;
use remote::secrets::SecretVault;
use remote::transfers::TransferManager;

/// The tray keeps the app reachable while its window is hidden, so closing it
/// with live SSH sessions can minimize instead of tearing connections down.
#[cfg(desktop)]
mod tray {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::{AppHandle, Manager};

    fn show_main(app: &AppHandle) {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }

    pub fn init(app: &tauri::App) -> tauri::Result<()> {
        let show = MenuItem::with_id(app, "show", "Show ParolaSSH", true, None::<&str>)?;
        let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
        let menu = Menu::with_items(app, &[&show, &quit])?;

        TrayIconBuilder::with_id("main")
            .icon(
                app.default_window_icon()
                    .expect("bundle config always ships window icons")
                    .clone(),
            )
            .tooltip("ParolaSSH")
            .menu(&menu)
            // Left click restores the window, right click opens the menu. Some
            // Linux status-notifier hosts only show the menu, hence "Show".
            .show_menu_on_left_click(false)
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    show_main(tray.app_handle());
                }
            })
            .on_menu_event(|app, event| match event.id.as_ref() {
                "show" => show_main(app),
                // Fires RunEvent::Exit below, which disconnects every session.
                "quit" => app.exit(0),
                _ => {}
            })
            .build(app)?;

        Ok(())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());

    // Only ever one ParolaSSH: a second copy would hold its own sessions and
    // remembered passwords, and would race the first over `hosts.json`.
    // Registered before every other plugin, as the plugin requires.
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
        .setup(|app| {
            #[cfg(desktop)]
            tray::init(app)?;
            #[cfg(not(desktop))]
            let _ = app;
            Ok(())
        })
        // Both process-scoped on purpose: quitting drops every credential.
        .manage(SessionRegistry::new())
        .manage(SecretVault::new())
        // One queue for every host: what transfers ration is the local uplink,
        // not any single server. `Arc` because running tasks outlive the
        // command that spawned them and need their own handle.
        .manage(Arc::new(TransferManager::new()))
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
            vpn::commands::vpn_overview,
            vpn::commands::tailscale_peers,
            remote::commands::connect_host,
            remote::commands::disconnect_host,
            remote::commands::connected_hosts,
            remote::commands::has_remembered_password,
            remote::commands::forget_password,
            remote::commands::host_key_passphrase_need,
            remote::commands::privilege_report,
            remote::commands::preview_power,
            remote::commands::power_host,
            remote::commands::heartbeat,
            // Interactive terminal
            remote::commands::open_shell,
            remote::commands::list_shells,
            remote::commands::write_shell,
            remote::commands::resize_shell,
            remote::commands::close_shell,
            // Long-running streams (followed logs)
            remote::commands::close_stream,
            // Services
            remote::commands::list_services,
            remote::commands::preview_service_action,
            remote::commands::service_action,
            remote::commands::service_log,
            remote::commands::follow_service_log,
            // Performance
            remote::commands::sample_metrics,
            // Updates
            remote::commands::check_updates,
            // Remote audit
            remote::commands::remote_audit,
            remote::commands::set_remote_finding_suppressed,
            // Files (SFTP)
            remote::commands::list_remote_dir,
            remote::commands::remote_home_dir,
            remote::commands::create_remote_dir,
            remote::commands::delete_remote_entry,
            remote::commands::rename_remote_entry,
            remote::commands::copy_remote_entry,
            remote::commands::list_remote_tree,
            remote::commands::local_conflicts,
            remote::commands::remote_conflicts,
            // Transfers — one queue across every host
            remote::commands::enqueue_download,
            remote::commands::enqueue_upload,
            remote::commands::list_transfers,
            remote::commands::transfer_summary,
            remote::commands::cancel_transfer,
            remote::commands::set_transfer_priority,
            remote::commands::set_max_concurrent_transfers,
            remote::commands::clear_finished_transfers,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            // Close every session on the way out, so the remote sshd logs a
            // clean disconnect rather than a broken pipe.
            if let tauri::RunEvent::Exit = event {
                let registry = <tauri::AppHandle as tauri::Manager<_>>::state::<SessionRegistry>(app);
                tauri::async_runtime::block_on(registry.disconnect_all());
            }
        });
}
