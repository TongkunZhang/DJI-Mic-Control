// Hide the console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use std::sync::Arc;
use std::time::Duration;

use device::DeviceManager;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

/// Pick the device tray actions apply to: the sole device, else the first
/// connected one.
fn pick_device(mgr: &DeviceManager) -> Option<String> {
    if let Some(id) = mgr.only_device() {
        return Some(id);
    }
    mgr.list().into_iter().find(|d| d.connected).map(|d| d.id)
}

/// Flip a two-value setting: if it currently reads `a`, set `b`, otherwise `a`.
fn toggle_between(mgr: &DeviceManager, setting: &str, a: &str, b: &str) {
    let Some(id) = pick_device(mgr) else {
        return;
    };
    let current = mgr
        .status(&id)
        .ok()
        .and_then(|s| s.settings.get(setting).cloned());
    let next = if current.as_deref() == Some(a) { b } else { a };
    let _ = mgr.set(&id, setting, next);
}

/// Terminate the process, first SIGKILLing our child processes (the WebKitGTK
/// web/network helpers) so they die instantly instead of running their graceful
/// GL teardown, which segfaults in Mesa's EGL finalizer on some drivers.
#[cfg(unix)]
fn quit_now() -> ! {
    let me = std::process::id();
    if let Ok(dir) = std::fs::read_dir("/proc") {
        for entry in dir.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else {
                continue;
            };
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
                continue;
            };
            // `stat` is: pid (comm) state ppid ...; comm may contain spaces or
            // ')' so parse the fields after the final ')'.
            if let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r) {
                let mut fields = rest.split_whitespace();
                let _state = fields.next();
                if fields.next().and_then(|s| s.parse::<u32>().ok()) == Some(me) {
                    unsafe { libc::kill(pid, libc::SIGKILL) };
                }
            }
        }
    }
    // Exit immediately without running our own finalizers (which would also try
    // to tear down GL) and before WebKit can respawn a helper.
    unsafe { libc::_exit(0) }
}

/// Show or hide the macOS Dock icon (no-op elsewhere). The app hides it while
/// running in the background with no window, like a menu-bar app.
fn set_dock_visible(app: &AppHandle, visible: bool) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_dock_visibility(visible);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, visible);
    }
}

/// Reveal and focus the main window (and show the Dock icon).
fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        set_dock_visible(app, true);
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// What the tray menu reflects: whether to show the per-device toggles (exactly
/// one mic connected), the current NC mode and LED state, and whether "Start at
/// login" is enabled. Compared each tick so the menu is only rebuilt on change.
type MenuState = (bool, Option<String>, Option<String>, bool);

/// Read the current menu-relevant state.
fn menu_state(app: &AppHandle) -> MenuState {
    let autostart = app.autolaunch().is_enabled().unwrap_or(false);
    let mgr = app.state::<Arc<DeviceManager>>().inner().clone();
    if mgr.list().len() != 1 {
        return (false, None, None, autostart);
    }
    match pick_device(&mgr).and_then(|id| mgr.status(&id).ok()) {
        Some(st) => (
            true,
            st.settings.get("noise-cancel").cloned(),
            st.settings.get("mic-leds").cloned(),
            autostart,
        ),
        None => (true, None, None, autostart),
    }
}

/// Build the tray menu. The per-device toggles appear only when exactly one mic
/// is connected, and their labels show the current mode and what tapping does.
fn build_menu(app: &AppHandle, state: &MenuState) -> tauri::Result<Menu<Wry>> {
    let (show, nc, led, autostart) = state;
    let open = MenuItem::with_id(app, "open", "Open DJI Mic Control", true, None::<&str>)?;
    let startup = CheckMenuItem::with_id(
        app,
        "toggle-autostart",
        "Start at login (hidden)",
        true,
        *autostart,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep_a = PredefinedMenuItem::separator(app)?;
    let sep_b = PredefinedMenuItem::separator(app)?;

    if *show {
        let led_label = match led.as_deref() {
            Some("on") => "Mic LEDs: On  →  Off",
            Some("off") => "Mic LEDs: Off  →  On",
            _ => "Toggle Mic LEDs",
        };
        let nc_label = match nc.as_deref() {
            Some("strong") => "Noise Cancel: Strong  →  Basic",
            Some("basic") => "Noise Cancel: Basic  →  Strong",
            _ => "Toggle NC Mode (Basic / Strong)",
        };
        let leds = MenuItem::with_id(app, "toggle-leds", led_label, true, None::<&str>)?;
        let nc = MenuItem::with_id(app, "toggle-nc", nc_label, true, None::<&str>)?;
        let sep_c = PredefinedMenuItem::separator(app)?;
        Menu::with_items(
            app,
            &[&open, &sep_a, &leds, &nc, &sep_b, &startup, &sep_c, &quit],
        )
    } else {
        Menu::with_items(app, &[&open, &sep_a, &startup, &sep_b, &quit])
    }
}

/// Swap the tray's menu to match current state (main thread only).
fn set_tray_menu(app: &AppHandle, state: &MenuState) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("main"), build_menu(app, state)) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn main() {
    // On SIGTERM/SIGINT (logout, shutdown, Ctrl-C) exit via quit_now() so the
    // WebKit helpers are killed rather than crashing in their GL teardown. This
    // keeps the accelerated DMABUF renderer (full frame rate) without the noisy
    // exit-time segfaults.
    #[cfg(unix)]
    {
        use signal_hook::consts::{SIGINT, SIGTERM};
        if let Ok(mut signals) = signal_hook::iterator::Signals::new([SIGTERM, SIGINT]) {
            std::thread::spawn(move || {
                if signals.forever().next().is_some() {
                    quit_now();
                }
            });
        }
    }

    let manager = Arc::new(DeviceManager::new());

    // Continuously rescan the bus so hotplugged devices appear/disappear.
    let scanner = manager.clone();
    std::thread::spawn(move || loop {
        scanner.refresh();
        std::thread::sleep(Duration::from_millis(1000));
    });

    // When launched at login/startup with --hidden, stay in the tray and don't
    // show the window.
    let start_hidden = std::env::args().skip(1).any(|a| a == "--hidden" || a == "--minimized");

    tauri::Builder::default()
        // Re-launching focuses the running window instead of starting a copy.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        // "Start at login" registers the app to autostart with --hidden.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .manage(manager)
        .setup(move |app| {
            // Set the window icon explicitly so the titlebar and taskbar show it
            // even when the desktop-file association is unavailable (e.g. X11).
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(icon) = Image::from_bytes(include_bytes!("../icons/128x128.png")) {
                    let _ = window.set_icon(icon);
                }
                // The window is created hidden; reveal it unless we started at login.
                if start_hidden {
                    set_dock_visible(app.handle(), false);
                } else {
                    let _ = window.show();
                }
            }

            let initial = menu_state(app.handle());
            let menu = build_menu(app.handle(), &initial)?;
            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("DJI Mic Control")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => show_main(app),
                    "toggle-leds" => {
                        let mgr = app.state::<Arc<DeviceManager>>().inner().clone();
                        std::thread::spawn(move || toggle_between(&mgr, "mic-leds", "on", "off"));
                    }
                    "toggle-nc" => {
                        let mgr = app.state::<Arc<DeviceManager>>().inner().clone();
                        std::thread::spawn(move || {
                            toggle_between(&mgr, "noise-cancel", "strong", "basic")
                        });
                    }
                    "toggle-autostart" => {
                        let auto = app.autolaunch();
                        let _ = if auto.is_enabled().unwrap_or(false) {
                            auto.disable()
                        } else {
                            auto.enable()
                        };
                        // Reflect the new state in the menu's checkbox immediately.
                        set_tray_menu(app, &menu_state(app));
                    }
                    "quit" => {
                        #[cfg(unix)]
                        quit_now();
                        #[cfg(not(unix))]
                        app.exit(0);
                    }
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon().cloned() {
                tray = tray.icon(icon);
            }
            tray.build(app)?;

            // Keep the tray menu in sync with device count, NC/LED state, and the
            // autostart setting.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut last = initial;
                loop {
                    let state = menu_state(&handle);
                    if state != last {
                        last = state.clone();
                        let h = handle.clone();
                        let _ = handle.run_on_main_thread(move || set_tray_menu(&h, &state));
                    }
                    std::thread::sleep(Duration::from_millis(1000));
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            // Close to tray: hide the window (and the Dock icon) instead of
            // quitting the app.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                set_dock_visible(window.app_handle(), false);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::snapshot,
            commands::set_setting,
            commands::udev_help,
        ])
        .run(tauri::generate_context!())
        .expect("error while running DJI Mic Control");
}
