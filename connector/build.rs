fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64")
    {
        let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let source = root.join("native/session_bridge.c");
        let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        let dll = out.join("session_bridge.dll");
        let compiler = cc::Build::new().get_compiler();
        let mut command = compiler.to_command();
        command.current_dir(&out);
        if compiler.is_like_msvc() {
            command
                .arg("/nologo")
                .arg("/O2")
                .arg("/LD")
                .arg(&source)
                .arg(format!("/Fe:{}", dll.display()))
                .arg("/link")
                .arg("user32.lib");
        } else {
            command
                .arg("-shared")
                .arg("-O2")
                .arg("-o")
                .arg(&dll)
                .arg(&source)
                .arg("-luser32");
        }
        assert!(
            command
                .status()
                .expect("cannot run native compiler")
                .success(),
            "cannot build native session helper"
        );
        println!("cargo:rerun-if-changed={}", source.display());
    }

    #[cfg(feature = "desktop")]
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "select_service",
            "configure_connect",
            "install_connect_panel",
        ]),
    ))
    .expect("cannot build the desktop application");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set"));
        let resource = output.join("resource.lib");
        if resource.is_file() {
            println!("cargo:rustc-link-arg-examples={}", resource.display());
        }
    }
}
