use std::sync::atomic::{AtomicBool, Ordering};

use boxy::{BrowserCookie, ConnectorHost, LaunchOptions};
use url::Url;

#[derive(Default)]
struct ProbeHost {
    connected: AtomicBool,
}

impl ConnectorHost for ProbeHost {
    fn open_editor(&self, _url: Url, _cookies: Vec<BrowserCookie>) -> Result<(), String> {
        self.connected.store(true, Ordering::Release);
        Ok(())
    }

    fn open_external(&self, _url: Url) -> Result<(), String> {
        Err("external links are unavailable in this probe".to_string())
    }

    fn should_stop(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }
}

fn main() -> Result<(), String> {
    let server = std::env::args()
        .nth(1)
        .ok_or("Pass the service origin to check")?;
    let config = boxy::resolve_config(&LaunchOptions::default(), &server)?;
    let host = ProbeHost::default();
    boxy::run_connected(&config, &host)?;
    if !host.connected.load(Ordering::Acquire) {
        return Err("Handshake verification did not complete without local tasks".to_string());
    }
    println!("Handshake accepted; no browser or local task was executed.");
    Ok(())
}
