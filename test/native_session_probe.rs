#[cfg(all(windows, target_arch = "x86_64"))]
fn main() {
    let mut args = std::env::args().skip(1);
    let pid = args
        .next()
        .and_then(|p| p.parse::<u32>().ok())
        .expect("pid");
    let hash = args.next().expect("session hash");
    assert!(args.next().is_none(), "unexpected argument");
    match boxy::native_session_test_reload(pid, &hash) {
        Ok(result) => println!("{result}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() {
    eprintln!("unsupported platform");
    std::process::exit(1);
}
