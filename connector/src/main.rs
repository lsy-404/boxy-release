#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod desktop;

fn main() {
    if let Some(code) = boxy::run_elevated_if_requested() {
        std::process::exit(code);
    }
    let result = boxy::prepare_config().and_then(desktop::run);
    let code = match result {
        Ok(code) => code,
        Err(error) => {
            rfd::MessageDialog::new()
                .set_title("Boxy Connector")
                .set_description(&error)
                .set_level(rfd::MessageLevel::Error)
                .show();
            1
        }
    };
    if code != 0 {
        std::process::exit(code);
    }
}
