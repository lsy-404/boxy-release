fn main() {
    println!("cargo:rerun-if-env-changed=BOXY_RCLONE_PATH");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    let generated = output.join("rclone_binary.rs");
    let manifest = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"),
    );
    let license = manifest.join("resources/rclone-COPYING.txt");
    println!("cargo:rerun-if-changed={}", license.display());
    let staged_license = output.join("rclone-COPYING.txt");
    std::fs::copy(&license, &staged_license).expect("cannot stage the rclone license");
    if let Some(source) = std::env::var_os("BOXY_RCLONE_PATH") {
        let source = std::path::PathBuf::from(source);
        println!("cargo:rerun-if-changed={}", source.display());
        let target_name = if std::env::var("TARGET")
            .unwrap_or_default()
            .contains("windows")
        {
            "rclone.exe"
        } else {
            "rclone"
        };
        let target = output.join(target_name);
        std::fs::copy(&source, &target).expect("cannot stage the pinned rclone helper");
        std::fs::write(
            generated,
            format!(
                "pub const EMBEDDED_RCLONE: Option<&[u8]> = Some(include_bytes!({:?}));\npub const RCLONE_LICENSE: &[u8] = include_bytes!({:?});",
                target, staged_license
            ),
        )
        .expect("cannot generate rclone resource reference");
    } else {
        std::fs::write(
            generated,
            format!("pub const EMBEDDED_RCLONE: Option<&[u8]> = None;\npub const RCLONE_LICENSE: &[u8] = include_bytes!({:?});", staged_license),
        )
        .expect("cannot generate rclone resource reference");
    }
    #[cfg(feature = "desktop")]
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["select_service"])),
    )
    .expect("cannot build the desktop application");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!(
            "cargo:rustc-link-arg-examples={}",
            output.join("resource.lib").display()
        );
    }
}
