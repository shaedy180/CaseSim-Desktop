use serde::{Deserialize, Serialize};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{Manager, WebviewWindow};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Channel {
    endpoint: Option<String>,
    public_key: Option<String>,
    download_origins: Vec<String>,
}
impl Channel {
    fn configured(&self) -> bool {
        self.endpoint.as_ref().is_some_and(|v| secure_url(v))
            && self.public_key.as_ref().is_some_and(|k| k.len() >= 40)
            && !self.download_origins.is_empty()
            && self.download_origins.iter().all(|v| {
                secure_url(v)
                    && tauri::Url::parse(v).is_ok_and(|u| {
                        u.path() == "/" && u.query().is_none() && u.fragment().is_none()
                    })
            })
    }
    fn permits(&self, url: &tauri::Url) -> bool {
        secure_url(url.as_str())
            && self
                .download_origins
                .iter()
                .any(|origin| origin.trim_end_matches('/') == url.origin().ascii_serialization())
    }
}
fn secure_url(value: &str) -> bool {
    tauri::Url::parse(value).is_ok_and(|u| {
        u.scheme() == "https"
            && u.username().is_empty()
            && u.password().is_none()
            && u.port_or_known_default() == Some(443)
            && u.host_str().is_some_and(|h| {
                h.contains('.')
                    && !h.ends_with(".localhost")
                    && h != "tauri.localhost"
                    && h != "ipc.localhost"
                    && h.parse::<std::net::IpAddr>().is_err()
            })
    })
}

#[derive(Default)]
pub struct Session {
    phase: &'static str,
    update: Option<Update>,
    bytes: Vec<u8>,
    downloaded: u64,
    total: Option<u64>,
    error: Option<&'static str>,
    authorization_expires: Option<Instant>,
}
impl Session {
    pub fn busy(&self) -> bool {
        matches!(
            self.phase,
            "authorizing" | "checking" | "downloading" | "installing"
        )
    }
    pub fn installing(&self) -> bool {
        self.phase == "installing"
    }
}
pub struct Updates {
    pub channel: Channel,
    pub session: Mutex<Session>,
}
impl Default for Updates {
    fn default() -> Self {
        Self {
            channel: serde_json::from_str(include_str!("../update-channel.json"))
                .expect("valid embedded update channel"),
            session: Mutex::new(Session::default()),
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    version: &'static str,
    configured: bool,
    phase: &'static str,
    available_version: Option<String>,
    downloaded: u64,
    total: Option<u64>,
    error: Option<&'static str>,
    game_running: bool,
}
pub fn require_control(window: &WebviewWindow) -> Result<(), String> {
    if window.label() != "desktop"
        || !window
            .url()
            .is_ok_and(|u| crate::navigation::is_control(&u))
    {
        return Err("Local desktop controls required".into());
    }
    Ok(())
}
pub fn game_running(app: &tauri::AppHandle) -> bool {
    // Fail closed if the main window cannot be inspected. No remote activity IPC.
    app.get_webview_window("main")
        .is_none_or(|w| !w.url().is_ok_and(|u| crate::navigation::is_recovery(&u)))
}
fn failed(state: &Updates, error: &'static str) -> Result<(), String> {
    if let Ok(mut s) = state.session.lock() {
        s.phase = "error";
        s.error = Some(error);
        s.bytes.clear();
        s.update = None;
    }
    Err(error.into())
}
#[tauri::command]
pub async fn desktop_status(
    window: WebviewWindow,
    state: tauri::State<'_, Updates>,
) -> Result<Status, String> {
    require_control(&window)?;
    let game_running = game_running(window.app_handle());
    let s = state
        .session
        .lock()
        .map_err(|_| "Desktop state unavailable")?;
    Ok(Status {
        version: env!("CARGO_PKG_VERSION"),
        configured: state.channel.configured(),
        phase: s.phase,
        available_version: s.update.as_ref().map(|u| u.version.clone()),
        downloaded: s.downloaded,
        total: s.total,
        error: s.error,
        game_running,
    })
}
#[tauri::command]
pub async fn check_update(
    window: WebviewWindow,
    state: tauri::State<'_, Updates>,
) -> Result<(), String> {
    require_control(&window)?;
    if !state.channel.configured() {
        return Err("Updates are not configured in this preview".into());
    }
    {
        let mut s = state
            .session
            .lock()
            .map_err(|_| "Desktop state unavailable")?;
        if s.busy() {
            return Err("An update operation is already running".into());
        }
        *s = Session {
            phase: "authorizing",
            ..Default::default()
        };
    }
    let result = async {
        let authorization = crate::release_auth::authorize().await?;
        {
            let mut s = state
                .session
                .lock()
                .map_err(|_| "Desktop state unavailable")?;
            s.phase = "checking";
            s.authorization_expires = Some(authorization.expires_at);
        }
        window
            .app_handle()
            .updater_builder()
            .endpoints(vec![state
                .channel
                .endpoint
                .as_ref()
                .unwrap()
                .parse()
                .map_err(|_| "Invalid update channel")?])
            .map_err(|_| "Invalid update channel")?
            .pubkey(state.channel.public_key.as_ref().unwrap())
            .header("Authorization", authorization.header)
            .map_err(|_| "Update authorization unavailable")?
            .timeout(Duration::from_secs(30))
            .configure_client(|client| {
                client
                    .https_only(true)
                    .redirect(reqwest::redirect::Policy::none())
            })
            .restart_after_install(true)
            .build()
            .map_err(|_| "Update service unavailable")?
            .check()
            .await
            .map_err(|_| "Update check failed. Check your connection or sign in again.")
    }
    .await;
    match result {
        Ok(mut update) => {
            if update
                .as_ref()
                .is_some_and(|u| !state.channel.permits(&u.download_url))
            {
                return failed(&state, "The update download source is not trusted");
            }
            if let Some(update) = &mut update {
                update.timeout = Some(Duration::from_secs(180));
            }
            let mut s = state
                .session
                .lock()
                .map_err(|_| "Desktop state unavailable")?;
            s.phase = if update.is_some() {
                "available"
            } else {
                "current"
            };
            s.update = update;
            Ok(())
        }
        Err(error) => failed(&state, error),
    }
}
#[tauri::command]
pub async fn download_update(
    window: WebviewWindow,
    state: tauri::State<'_, Updates>,
) -> Result<(), String> {
    require_control(&window)?;
    let update = {
        let mut s = state
            .session
            .lock()
            .map_err(|_| "Desktop state unavailable")?;
        if s.phase != "available" {
            return Err("Check for an update first".into());
        }
        if s.authorization_expires
            .is_none_or(|expiry| Instant::now() >= expiry)
        {
            s.phase = "error";
            s.error = Some("Update permission expired. Check again to sign in.");
            s.update = None;
            return Err("Update permission expired. Check again to sign in.".into());
        }
        let u = s.update.clone().ok_or("No update available")?;
        s.phase = "downloading";
        s.downloaded = 0;
        s.total = None;
        u
    };
    let bytes = update
        .download(
            |size, total| {
                if let Ok(mut s) = state.session.lock() {
                    s.downloaded = s.downloaded.saturating_add(size as u64);
                    s.total = total;
                }
            },
            || {},
        )
        .await;
    match bytes {
        Ok(bytes) => {
            let mut s = state
                .session
                .lock()
                .map_err(|_| "Desktop state unavailable")?;
            s.bytes = bytes;
            s.phase = "ready";
            Ok(())
        }
        Err(_) => failed(
            &state,
            "Download permission, connection or signature validation failed. Check again to sign in. Nothing was installed.",
        ),
    }
}
#[tauri::command]
pub async fn install_update(
    window: WebviewWindow,
    state: tauri::State<'_, Updates>,
) -> Result<(), String> {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    require_control(&window)?;
    if game_running(window.app_handle()) {
        return Err("Finish your game and leave it from Desktop controls before installing".into());
    }
    let (update, bytes) = {
        let mut s = state
            .session
            .lock()
            .map_err(|_| "Desktop state unavailable")?;
        if s.phase != "ready" {
            return Err("Download and verify an update first".into());
        }
        let u = s.update.clone().ok_or("No verified update")?;
        s.phase = "installing";
        (u, std::mem::take(&mut s.bytes))
    };
    let dialog = window.app_handle().dialog().message("Install the verified desktop update and restart CaseSim now? Your game must be finished and Auto Open stopped.")
        .title("Update CaseSim Desktop").kind(MessageDialogKind::Warning).buttons(MessageDialogButtons::OkCancel);
    // Async commands execute off the UI thread. No automatic restart or remote grant.
    if !dialog.blocking_show() {
        let mut s = state
            .session
            .lock()
            .map_err(|_| "Desktop state unavailable")?;
        s.phase = "ready";
        s.bytes = bytes;
        return Ok(());
    }
    if game_running(window.app_handle()) {
        return failed(&state, "The game is still open. Update cancelled.");
    }
    window
        .app_handle()
        .state::<crate::diagnostics::Diagnostics>()
        .record("update_install_confirmed");
    use tauri_plugin_window_state::AppHandleExt;
    let _ = window.app_handle().save_window_state(
        tauri_plugin_window_state::StateFlags::SIZE
            | tauri_plugin_window_state::StateFlags::POSITION
            | tauri_plugin_window_state::StateFlags::MAXIMIZED,
    );
    update.install(bytes).map_err(|_| {
        let _ = failed(
            &state,
            "Update installation failed. Restart manually and try again.",
        );
        "Update installation failed. Nothing was restarted.".into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn updates_require_https_a_key_and_explicit_download_origins() {
        let mut c = Channel {
            endpoint: Some(
                "https://github.com/example/desktop/releases/latest/download/latest.json".into(),
            ),
            public_key: Some("a".repeat(80)),
            download_origins: vec!["https://github.com".into()],
        };
        assert!(c.configured());
        for url in [
            "http://github.com/test",
            "https://user:secret@github.com/test",
            "https://127.0.0.1/test",
            "https://tauri.localhost/",
            "https://github.com:444/test",
        ] {
            assert!(!secure_url(url));
        }
        assert!(c.permits(&"https://github.com/example/release.exe".parse().unwrap()));
        assert!(!c.permits(&"https://github.com.evil.test/release.exe".parse().unwrap()));
        c.public_key = None;
        assert!(!c.configured());
        let disabled: Channel =
            serde_json::from_str(r#"{"endpoint":null,"publicKey":null,"downloadOrigins":[]}"#)
                .unwrap();
        assert!(!disabled.configured());
    }
    #[test]
    fn operations_are_serialized_and_only_installation_blocks_game_navigation() {
        for phase in ["authorizing", "checking", "downloading", "installing"] {
            assert!(Session {
                phase,
                ..Default::default()
            }
            .busy());
        }
        for phase in ["ready", "available", "error", "current"] {
            assert!(!Session {
                phase,
                ..Default::default()
            }
            .busy());
        }
        assert!(Session {
            phase: "installing",
            ..Default::default()
        }
        .installing());
    }

    #[test]
    fn official_updater_rejects_tampering_and_manifest_version_substitution() {
        // Normal check() initializes the provider. This offline test restores
        // signed metadata directly and must initialize its own TLS test runtime.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/update-signature.json")).unwrap();
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({"pubkey": fixture["publicKey"], "requireSignedVersion": true}),
        );
        let app = tauri::test::mock_builder()
            .plugin(
                tauri_plugin_updater::Builder::new()
                    .pubkey(fixture["publicKey"].as_str().unwrap())
                    .build(),
            )
            .build(context)
            .unwrap();
        let updater = app
            .updater_builder()
            .target("windows-x86_64")
            .endpoints(vec!["https://example.test/latest.json".parse().unwrap()])
            .unwrap()
            .executable_path(std::env::temp_dir().join("CaseSim-fixture.exe"))
            .build()
            .unwrap();
        let manifest = serde_json::json!({"version":"22.7.50","platforms":{"windows-x86_64":{"url":"https://example.test/fixture.exe","signature":fixture["signature"]}}});
        let update = updater.restore_update(manifest.clone()).unwrap().unwrap();
        let bytes = fixture["payload"].as_str().unwrap().as_bytes();
        update.verify(bytes).unwrap();
        assert!(update.verify(b"tampered package").is_err());
        let mut forged = manifest.clone();
        forged["version"] = "22.7.51".into();
        assert!(updater
            .restore_update(forged)
            .unwrap()
            .unwrap()
            .verify(bytes)
            .is_err());
        let mut old = manifest;
        old["version"] = "0.0.0".into();
        assert!(updater.restore_update(old).unwrap().is_none());

        // Transport fixture exists only in this Rust test. Production channels
        // reject loopback/plain HTTP and configure reqwest with HTTPS-only.
        for payload in [bytes.to_vec(), b"tampered download".to_vec()] {
            use std::io::{Read, Write};
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let expected = payload == bytes;
            let server = std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                while std::time::Instant::now() < deadline {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            stream
                                .set_read_timeout(Some(Duration::from_secs(2)))
                                .unwrap();
                            let _ = stream.read(&mut [0; 4096]);
                            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", payload.len()).unwrap();
                            stream.write_all(&payload).unwrap();
                            return;
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        Err(e) => panic!("fixture server failed: {e}"),
                    }
                }
                panic!("fixture download did not arrive");
            });
            let local = updater.restore_update(serde_json::json!({"version":"22.7.50","platforms":{"windows-x86_64":{"url":format!("http://{address}/fixture.exe"),"signature":fixture["signature"]}}})).unwrap().unwrap();
            let mut local = local;
            local.no_proxy = true;
            local.timeout = Some(Duration::from_secs(3));
            let mut downloaded = 0;
            let result =
                tauri::async_runtime::block_on(local.download(|size, _| downloaded += size, || {}));
            assert_eq!(result.is_ok(), expected);
            assert!(downloaded > 0);
            server.join().unwrap();
        }
    }
}
