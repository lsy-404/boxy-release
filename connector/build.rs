fn main() {
    #[cfg(feature = "desktop")]
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["select_service"])),
    )
    .expect("cannot build the desktop application");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set"));
        println!(
            "cargo:rustc-link-arg-examples={}",
            output.join("resource.lib").display()
        );
    }
}
