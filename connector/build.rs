use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=../assets/boxy-pen.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let icon_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/boxy-pen.ico")
            .canonicalize()
            .expect("Boxy icon asset must exist");
        let icon_path = icon_path.to_string_lossy();

        winresource::WindowsResource::new()
            .set_icon(icon_path.as_ref())
            .compile()
            .expect("Windows Boxy icon resource must compile");
    }
}
