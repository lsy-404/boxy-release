use std::{
    path::Path,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use boxy::{ConnectorHost, LaunchOptions};
use url::Url;

#[derive(Default)]
struct ProbeHost {
    connected: AtomicBool,
    process_requests: AtomicUsize,
}

impl ConnectorHost for ProbeHost {
    fn open_editor(&self, _url: Url) -> Result<(), String> {
        self.connected.store(true, Ordering::Release);
        Ok(())
    }

    fn open_external(&self, _url: Url) -> Result<(), String> {
        Err("external links are unavailable in this probe".to_string())
    }

    fn approve_process(&self, _executable: &str, _args: &[String], _cwd: Option<&Path>) -> bool {
        self.process_requests.fetch_add(1, Ordering::Relaxed);
        false
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
    if !host.connected.load(Ordering::Acquire) || host.process_requests.load(Ordering::Relaxed) != 0
    {
        return Err("Handshake verification did not complete without local tasks".to_string());
    }
    println!("Handshake accepted; no browser or local task was executed.");
    Ok(())
}
