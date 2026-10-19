#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod diagnostics;
mod navigation;
mod shortcuts;
mod updates;
#[cfg(windows)]
mod windows;

use navigation::{account_url, classify, is_recovery, Destination, CASESIM_URL};
use std::sync::Mutex;
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{Manager, WebviewUrl, WebviewWindow};

struct Connection(Mutex<tauri::Url>);

// This command accepts no URL, credentials or filesystem paths. Tauri ACLs and
// the caller URL both restrict it to the bundled recovery document.
#[tauri::command]
async fn connect(window: WebviewWindow, state: tauri::State<'_, Connection>) -> Result<(), String> {
    if window.label() != "main" || !window.url().map(|url| is_recovery(&url)).unwrap_or(false) {
        return Err("Recovery page required".into());
    }
    if window
        .app_handle()
        .state::<updates::Updates>()
        .session
        .lock()
        .map_err(|_| "Desktop state unavailable")?
        .installing()
    {
        return Err("An update is being installed".into());
    }
    let target = state
        .0
        .lock()
        .map_err(|_| "Connection unavailable")?
        .clone();
    window
        .navigate(target)
        .map_err(|_| "Connection unavailable".into())
}

fn navigate(window: &WebviewWindow, url: &tauri::Url) {
    match classify(url) {
        Destination::Application => {
            let _ = window.navigate(url.clone());
        }
        Destination::Browser => {
            // HTTP(S) URLs only; no shell interpolation or protocol handlers.
            let _ = open::that_detached(url.as_str());
        }
        Destination::Block => {}
    }
}

fn open_controls(app: &tauri::AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("desktop") {
        window.unminimize()?;
        window.show()?;
        window.set_focus()?;
        return Ok(());
    }
    let window =
        tauri::WebviewWindowBuilder::new(app, "desktop", WebviewUrl::App("settings.html".into()))
            .data_directory(app.path().app_local_data_dir()?.join("WebView2"))
            .title("CaseSim Desktop")
            .inner_size(520.0, 640.0)
            .min_inner_size(420.0, 540.0)
            .center()
            .resizable(true)
            .theme(Some(tauri::Theme::Dark))
            .devtools(false)
            .background_color(tauri::webview::Color(2, 2, 2, 255))
            .on_navigation(navigation::is_control)
            .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
            .build()?;
    window.hide_menu()?;
    #[cfg(windows)]
    windows::install_shortcuts(&window)?;
    Ok(())
}

pub(crate) fn dispatch_shortcut(app: &tauri::AppHandle, action: shortcuts::Action) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        use shortcuts::Action;
        if action == Action::Controls {
            app.state::<diagnostics::Diagnostics>()
                .record("controls_open_requested");
            if open_controls(&app).is_err() {
                app.state::<diagnostics::Diagnostics>()
                    .record("controls_open_failed");
            } else {
                app.state::<diagnostics::Diagnostics>()
                    .record("controls_open_ready");
            }
            return;
        }
        let Some(window) = app.get_webview_window("main") else {
            return;
        };
        if app
            .state::<updates::Updates>()
            .session
            .lock()
            .map_or(true, |s| s.installing())
        {
            return;
        }
        match action {
            Action::Home => {
                let _ = window.navigate(CASESIM_URL.parse().unwrap());
            }
            Action::Account => {
                let _ = window.navigate(account_url(window.url().ok().as_ref()));
            }
            Action::Fullscreen => {
                let _ = window.set_fullscreen(!window.is_fullscreen().unwrap_or(false));
            }
            Action::ResetZoom => {
                let _ = window.set_zoom(1.0);
            }
            Action::Controls => unreachable!(),
        }
    });
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum DesktopAction {
    Game,
    Account,
    LeaveGame,
    ZoomReset,
    Fullscreen,
    CloseControls,
}

#[tauri::command]
async fn desktop_action(window: WebviewWindow, action: DesktopAction) -> Result<(), String> {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    updates::require_control(&window)?;
    let app = window.app_handle();
    if app
        .state::<updates::Updates>()
        .session
        .lock()
        .map_err(|_| "Desktop state unavailable")?
        .installing()
    {
        return Err("An update is being installed".into());
    }
    let main = app
        .get_webview_window("main")
        .ok_or("Game window unavailable")?;
    match action {
        DesktopAction::LeaveGame => {
            if app.dialog().message("Finish all openings and battles and stop Auto Open first. Leave the game for desktop maintenance now? Nothing will be opened or replayed automatically.")
                .title("Leave CaseSim").kind(MessageDialogKind::Warning).buttons(MessageDialogButtons::OkCancel).blocking_show() {
                main.navigate("http://tauri.localhost/index.html?paused=1".parse().unwrap()).map_err(|_| "Could not leave the game")?;
            }
        }
        DesktopAction::Game => {
            main.navigate(CASESIM_URL.parse().unwrap()).map_err(|_| "Could not open the game")?;
            main.show().map_err(|_| "Could not show game window")?;
            main.set_focus().map_err(|_| "Could not focus game window")?;
        }
        DesktopAction::Account => { main.navigate(account_url(main.url().ok().as_ref())).map_err(|_| "Could not open account")?; }
        DesktopAction::ZoomReset => { main.set_zoom(1.0).map_err(|_| "Could not reset zoom")?; }
        DesktopAction::Fullscreen => { main.set_fullscreen(!main.is_fullscreen().map_err(|_| "Window state unavailable")?).map_err(|_| "Could not change fullscreen")?; }
        DesktopAction::CloseControls => { window.close().map_err(|_| "Could not close desktop controls")?; }
    }
    Ok(())
}

fn main() {
    let result = tauri::Builder::default()
        .manage(Connection(Mutex::new(CASESIM_URL.parse().unwrap())))
        .manage(updates::Updates::default())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .menu(|app| {
            let home = MenuItemBuilder::with_id("casesim-home", "Open CaseSim")
                .accelerator("Ctrl+Shift+H")
                .build(app)?;
            let account = MenuItemBuilder::with_id("casesim-account", "Account and sign out")
                .accelerator("Ctrl+Shift+A")
                .build(app)?;
            let controls = MenuItemBuilder::with_id("casesim-controls", "Desktop controls")
                .accelerator("Ctrl+Shift+D")
                .build(app)?;
            let fullscreen = MenuItemBuilder::with_id("casesim-fullscreen", "Fullscreen")
                .accelerator("F11")
                .build(app)?;
            let zoom = MenuItemBuilder::with_id("casesim-zoom", "Reset zoom")
                .accelerator("Ctrl+0")
                .build(app)?;
            let menu = SubmenuBuilder::new(app, "CaseSim")
                .items(&[&home, &account, &controls, &fullscreen, &zoom])
                .build()?;
            MenuBuilder::new(app).item(&menu).build()
        })
        .on_menu_event(|app, event| {
            use shortcuts::Action;
            let action = match event.id().as_ref() {
                "casesim-home" => Action::Home,
                "casesim-account" => Action::Account,
                "casesim-controls" => Action::Controls,
                "casesim-fullscreen" => Action::Fullscreen,
                "casesim-zoom" => Action::ResetZoom,
                _ => return,
            };
            dispatch_shortcut(app, action);
        })
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            connect,
            desktop_action,
            updates::desktop_status,
            updates::check_update,
            updates::download_update,
            updates::install_update
        ])
        .setup(|app| {
            app.manage(diagnostics::Diagnostics::new(app.path().app_log_dir()?)?);
            app.state::<diagnostics::Diagnostics>().record("startup");
            app.state::<diagnostics::Diagnostics>()
                .record(concat!("version_", env!("CARGO_PKG_VERSION")));
            let handle = app.handle().clone();
            let popup_handle = handle.clone();
            let profile = app.path().app_local_data_dir()?.join("WebView2");
            std::fs::create_dir_all(&profile)?;
            let window =
                tauri::WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("CaseSim")
                    .inner_size(1440.0, 900.0)
                    .min_inner_size(960.0, 640.0)
                    .prevent_overflow()
                    .center()
                    .resizable(true)
                    .theme(Some(tauri::Theme::Dark))
                    .background_color(tauri::webview::Color(2, 2, 2, 255))
                    .data_directory(profile)
                    .devtools(false)
                    .disable_drag_drop_handler()
                    .on_document_title_changed(|window, _| {
                        tauri::async_runtime::spawn(async move {
                            let _ = window.set_title("CaseSim");
                        });
                    })
                    .on_navigation(move |url| {
                        if is_recovery(url) {
                            return true;
                        }
                        if handle
                            .state::<updates::Updates>()
                            .session
                            .lock()
                            .map_or(true, |s| s.installing())
                        {
                            return false;
                        }
                        match classify(url) {
                            Destination::Application => {
                                if let Ok(mut target) = handle.state::<Connection>().0.lock() {
                                    *target = url.clone();
                                }
                                true
                            }
                            Destination::Browser => {
                                let _ = open::that_detached(url.as_str());
                                false
                            }
                            Destination::Block => false,
                        }
                    })
                    .on_new_window(move |url, _| {
                        if let Some(window) = popup_handle.get_webview_window("main") {
                            tauri::async_runtime::spawn(async move {
                                navigate(&window, &url);
                            });
                        }
                        tauri::webview::NewWindowResponse::Deny
                    })
                    .build()?;
            #[cfg(windows)]
            {
                windows::install_navigation_recovery(&window)?;
                windows::install_shortcuts(&window)?;
                // Native keys are handled by WebView2, independent of menu state.
                let _ = app.remove_menu()?;
            }
            #[cfg(not(windows))]
            window.hide_menu()?;
            // Page titles from the online service must not rename the taskbar app.
            window.set_title("CaseSim")?;
            Ok(())
        })
        .run(tauri::generate_context!());
    if result.is_err() {
        #[cfg(windows)]
        windows::show_startup_error();
        eprintln!("CaseSim desktop could not start");
        std::process::exit(1);
    }
}
