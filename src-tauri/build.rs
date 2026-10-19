fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "connect",
            "desktop_action",
            "desktop_status",
            "check_update",
            "download_update",
            "install_update",
        ]),
    ))
    .expect("desktop build configuration must be valid");
}
