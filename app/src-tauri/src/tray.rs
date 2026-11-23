use std::sync::atomic::{AtomicBool, Ordering};

use tauri::State;
#[cfg(target_os = "windows")]
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
#[cfg(desktop)]
use tauri::{AppHandle, Manager};

#[cfg(target_os = "windows")]
use crate::proc::{terminate_run, RunState};

/// Windows tray preferences pushed by the frontend at startup and on change.
/// 只剩「关闭到托盘」——最小化到托盘已退役。
#[derive(Default)]
pub(crate) struct TrayPrefs {
    close_to_tray: AtomicBool,
}

/// Close-to-tray only exists on Windows, the only platform with a tray icon.
pub(crate) fn close_to_tray_enabled(prefs: &TrayPrefs) -> bool {
    prefs.close_to_tray.load(Ordering::SeqCst)
}

fn tray_tooltip(running: bool) -> &'static str {
    if running {
        "Echoless — RUNNING"
    } else {
        "Echoless — STOPPED"
    }
}

pub(crate) fn update_tray_tooltip(app: &tauri::AppHandle, running: bool) {
    #[cfg(target_os = "windows")]
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(tray_tooltip(running)));
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (app, tray_tooltip(running));
}

#[cfg(target_os = "windows")]
const TRAY_ID: &str = "main-tray";
#[cfg(target_os = "windows")]
const TRAY_MENU_SHOW: &str = "show";
#[cfg(target_os = "windows")]
const TRAY_MENU_QUIT: &str = "quit";

#[cfg(desktop)]
pub(crate) fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn register_windows_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, TRAY_MENU_SHOW, "Show", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, TRAY_MENU_QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &separator, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip(tray_tooltip(false))
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_MENU_SHOW => show_main_window(app),
            TRAY_MENU_QUIT => {
                let state = app.state::<RunState>();
                let _ = terminate_run(&state);
                update_tray_tooltip(app, false);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    // The app keeps the built tray icon; it is looked up again by TRAY_ID.
    builder.build(app)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn set_tray_prefs(prefs: State<TrayPrefs>, close_to_tray: bool) {
    prefs.close_to_tray.store(
        cfg!(target_os = "windows") && close_to_tray,
        Ordering::SeqCst,
    );
}
