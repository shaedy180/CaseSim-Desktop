use tauri::{Manager, WebviewWindow};
use webview2_com::{
    AcceleratorKeyPressedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2NavigationCompletedEventArgs2, COREWEBVIEW2_PROCESS_FAILED_KIND,
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
        COREWEBVIEW2_WEB_ERROR_STATUS, COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED,
    },
    NavigationCompletedEventHandler, ProcessFailedEventHandler,
};
use windows::core::{Interface, BOOL};

pub fn install_shortcuts(window: &WebviewWindow) -> tauri::Result<()> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
        COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN, COREWEBVIEW2_PHYSICAL_KEY_STATUS,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    let app = window.app_handle().clone();
    window.with_webview(move |webview| unsafe {
        let controller = webview.controller();
        let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
            args.KeyEventKind(&mut kind)?;
            if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
            {
                return Ok(());
            }
            let mut key = 0;
            args.VirtualKey(&mut key)?;
            let Some(action) = crate::shortcuts::resolve(
                key,
                GetKeyState(VK_CONTROL.0.into()) < 0,
                GetKeyState(VK_SHIFT.0.into()) < 0,
                GetKeyState(VK_MENU.0.into()) < 0,
                GetKeyState(VK_LWIN.0.into()) < 0 || GetKeyState(VK_RWIN.0.into()) < 0,
            ) else {
                return Ok(());
            };
            args.SetHandled(true)?;
            let mut physical = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
            args.PhysicalKeyStatus(&mut physical)?;
            // Holding F11 must toggle only once, while repeated native key input
            // remains consumed instead of reaching the hosted document.
            if !physical.WasKeyDown.as_bool() {
                crate::dispatch_shortcut(&app, action);
            }
            Ok(())
        }));
        let mut token = 0;
        if controller
            .add_AcceleratorKeyPressed(&handler, &mut token)
            .is_err()
        {
            eprintln!("CaseSim: native keyboard shortcuts could not be initialized");
        }
    })
}

pub fn install_navigation_recovery(window: &WebviewWindow) -> tauri::Result<()> {
    let recovery = window.clone();
    let process_window = window.clone();
    window.with_webview(move |webview| unsafe {
        // These COM objects stay on WebView2's UI thread. The completion handler
        // covers top-level documents only, never failed images, APIs or iframes.
        let core = match webview.controller().CoreWebView2() {
            Ok(core) => core,
            Err(_) => return,
        };
        if let Ok(settings) = core.Settings() {
            let _ = settings.SetIsStatusBarEnabled(false);
        }
        let handler = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut success = BOOL::default();
            let mut error = COREWEBVIEW2_WEB_ERROR_STATUS::default();
            args.IsSuccess(&mut success)?;
            args.WebErrorStatus(&mut error)?;
            let mut status = 0;
            if let Ok(http) = args.cast::<ICoreWebView2NavigationCompletedEventArgs2>() {
                http.HttpStatusCode(&mut status)?;
            }
            // Cancelled external links/redirects are not outages. Keep auth's
            // 401/403 responses intact instead of replacing them with "offline".
            if error != COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED
                && !matches!(status, 401 | 403)
                && (!success.as_bool() || status >= 500)
            {
                let window = recovery.clone();
                // run_on_main_thread can execute inline inside a COM callback.
                // Use the worker boundary so synchronous URL queries cannot
                // block the WebView2 UI thread waiting for its own event loop.
                tauri::async_runtime::spawn(async move {
                    if let Ok(url) = window.url() {
                        if !crate::navigation::is_recovery(&url) {
                            let _ = window.navigate(
                                "http://tauri.localhost/index.html?failed=1"
                                    .parse()
                                    .unwrap(),
                            );
                        }
                    }
                });
            }
            Ok(())
        }));
        // WebView2 retains the handler until the webview is destroyed.
        let mut token = 0;
        if core.add_NavigationCompleted(&handler, &mut token).is_err() {
            // A missing recovery handler must not silently ship as working.
            eprintln!("CaseSim: navigation recovery could not be initialized");
        }
        let process_handler = ProcessFailedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            args.ProcessFailedKind(&mut kind)?;
            let window = process_window.clone();
            tauri::async_runtime::spawn(async move {
                window.app_handle().state::<crate::diagnostics::Diagnostics>().record("webview_process_failure");
                if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED {
                    // WebView2 owns GPU-process recovery. Keep hardware acceleration
                    // and the live game; never disable WebGL or replay an opening.
                    return;
                }
                if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
                    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
                    if window.app_handle().dialog().message("The desktop browser stopped. Restart CaseSim? Completed actions remain on the server and will not be replayed.")
                        .title("CaseSim recovery").kind(MessageDialogKind::Warning).buttons(MessageDialogButtons::OkCancel).blocking_show() {
                        window.app_handle().restart();
                    }
                } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED {
                    // Renderer failures get a local Retry screen, not an automatic
                    // page reload that could interrupt or repeat a user action.
                    let _ = window.navigate("http://tauri.localhost/index.html?failed=1".parse().unwrap());
                } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE {
                    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
                    if window.app_handle().dialog().message("The game view is not responding. Open the recovery screen? Choose Cancel to keep waiting. Completed actions will not be replayed.")
                        .title("CaseSim recovery").kind(MessageDialogKind::Warning).buttons(MessageDialogButtons::OkCancel).blocking_show() {
                        let _ = window.navigate("http://tauri.localhost/index.html?failed=1".parse().unwrap());
                    }
                }
            });
            Ok(())
        }));
        if core.add_ProcessFailed(&process_handler, &mut token).is_err() {
            eprintln!("CaseSim: process recovery could not be initialized");
        }
    })
}

pub fn show_startup_error() {
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    // Fixed text only; never expose a raw error containing paths or service data.
    unsafe {
        MessageBoxW(None, w!("CaseSim Desktop could not start. Check that Microsoft Edge WebView2 is installed and try again. If the problem continues, reinstall the verified CaseSim installer."), w!("CaseSim Desktop"), MB_OK | MB_ICONERROR);
    }
}
