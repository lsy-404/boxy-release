use std::{collections::BTreeSet, io::Read, process::Command, time::Duration};

use serde::Serialize;
use sysinfo::{Disks, Networks, System};

const MAX_COMMAND_BYTES: usize = 256 * 1024;
const MAX_LIST_ITEMS: usize = 512;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    language: String,
    languages: Vec<String>,
    region: String,
    local_time: String,
    utc_offset_seconds: i64,
    timezone: String,
    hardware: HardwareInfo,
    disks: Vec<DiskInfo>,
    network: NetworkInfo,
    tray_applications: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HardwareInfo {
    cpu_model: String,
    cpu_vendor: String,
    physical_cores: usize,
    logical_cores: usize,
    cpu_frequency_mhz: u64,
    total_memory_bytes: u64,
    motherboard: String,
    product_name: String,
    serial_available: bool,
    gpus: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiskInfo {
    name: String,
    mount_point: String,
    file_system: String,
    total_bytes: u64,
    available_bytes: u64,
    used_percent: f32,
    removable: bool,
    read_only: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkInfo {
    interfaces: Vec<NetworkInterface>,
    detected_public_ip: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkInterface {
    name: String,
    addresses: Vec<String>,
    mac_address: String,
    is_loopback: bool,
}

pub fn collect(public_ip_url: &str) -> DeviceInfo {
    let (language, languages, region) = locale();
    let (local_time, utc_offset_seconds, timezone) = local_time();
    DeviceInfo {
        language,
        languages,
        region,
        local_time,
        utc_offset_seconds,
        timezone,
        hardware: hardware(),
        disks: disks(),
        network: NetworkInfo {
            interfaces: network_interfaces(),
            detected_public_ip: detected_public_ip(public_ip_url),
        },
        tray_applications: tray_applications(),
    }
}

fn locale() -> (String, Vec<String>, String) {
    #[cfg(target_os = "macos")]
    {
        let languages = defaults_array(&["read", "-g", "AppleLanguages"]);
        let region = defaults_string(&["read", "-g", "AppleLocale"]).unwrap_or_default();
        let language = languages.first().cloned().unwrap_or_default();
        (language, languages, region)
    }
    #[cfg(windows)]
    {
        let language = command_output(
            "powershell",
            &["-NoProfile", "-Command", "(Get-Culture).Name"],
        )
        .unwrap_or_default();
        let region = command_output(
            "powershell",
            &["-NoProfile", "-Command", "(Get-Culture).DisplayName"],
        )
        .unwrap_or_default();
        (language.clone(), vec![language], region)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let language = env::var("LANG").unwrap_or_default();
        let region = env::var("LC_ALL")
            .or_else(|_| env::var("LANG"))
            .unwrap_or_default();
        (language.clone(), vec![language], region)
    }
}

fn local_time() -> (String, i64, String) {
    let timezone = iana_time_zone::get_timezone().unwrap_or_default();
    match time::OffsetDateTime::now_local() {
        Ok(now) => (
            now.format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            now.offset().whole_seconds() as i64,
            timezone,
        ),
        Err(_) => (String::new(), 0, timezone),
    }
}

fn hardware() -> HardwareInfo {
    let mut system = System::new();
    system.refresh_memory();
    system.refresh_cpu_all();
    let cpus = system.cpus();
    let first = cpus.first();
    HardwareInfo {
        cpu_model: first
            .map(|cpu| cpu.brand().trim().to_string())
            .unwrap_or_default(),
        cpu_vendor: first
            .map(|cpu| cpu.vendor_id().trim().to_string())
            .unwrap_or_default(),
        physical_cores: System::physical_core_count().unwrap_or(0),
        logical_cores: cpus.len(),
        cpu_frequency_mhz: first.map_or(0, |cpu| cpu.frequency()),
        total_memory_bytes: system.total_memory(),
        motherboard: sysinfo::Motherboard::new()
            .and_then(|board| board.name())
            .unwrap_or_default(),
        product_name: sysinfo::Product::name().unwrap_or_default(),
        serial_available: sysinfo::Product::serial_number().is_some(),
        gpus: gpus(),
    }
}

fn disks() -> Vec<DiskInfo> {
    Disks::new_with_refreshed_list()
        .list()
        .iter()
        .take(MAX_LIST_ITEMS)
        .map(|disk| {
            let total = disk.total_space();
            let available = disk.available_space();
            let used = total.saturating_sub(available);
            DiskInfo {
                name: disk.name().to_string_lossy().to_string(),
                mount_point: disk.mount_point().to_string_lossy().to_string(),
                file_system: disk.file_system().to_string_lossy().to_string(),
                total_bytes: total,
                available_bytes: available,
                used_percent: if total == 0 {
                    0.0
                } else {
                    ((used as f64 * 100.0) / total as f64) as f32
                },
                removable: disk.is_removable(),
                read_only: disk.is_read_only(),
            }
        })
        .collect()
}

fn network_interfaces() -> Vec<NetworkInterface> {
    Networks::new_with_refreshed_list()
        .list()
        .iter()
        .take(MAX_LIST_ITEMS)
        .map(|(name, data)| NetworkInterface {
            name: name.clone(),
            addresses: data
                .ip_networks()
                .iter()
                .take(MAX_LIST_ITEMS)
                .map(|network| network.addr.to_string())
                .collect(),
            mac_address: data.mac_address().to_string(),
            is_loopback: data
                .ip_networks()
                .iter()
                .any(|network| network.addr.is_loopback()),
        })
        .collect()
}

fn detected_public_ip(url: &str) -> Option<String> {
    let response = ureq::get(url).timeout(Duration::from_secs(6)).call().ok()?;
    let mut body = String::new();
    response
        .into_reader()
        .take(4096)
        .read_to_string(&mut body)
        .ok()?;
    let candidate = body.trim();
    if candidate.is_empty()
        || candidate.len() > 64
        || !candidate
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.' || byte == b':')
    {
        return None;
    }
    Some(candidate.to_string())
}

fn gpus() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        let text = command_output(
            "/usr/sbin/system_profiler",
            &["-json", "SPDisplaysDataType"],
        )
        .unwrap_or_default();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(items) = value
                .get("SPDisplaysDataType")
                .and_then(|items| items.as_array())
            {
                return items
                    .iter()
                    .filter_map(|item| item.get("sppci_model").and_then(|model| model.as_str()))
                    .take(MAX_LIST_ITEMS)
                    .map(str::to_string)
                    .collect();
            }
        }
        Vec::new()
    }
    #[cfg(windows)]
    {
        let text = command_output(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name",
            ],
        )
        .unwrap_or_default();
        return text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(MAX_LIST_ITEMS)
            .map(str::to_string)
            .collect();
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    Vec::new()
}

fn tray_applications() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        let text = command_output("/usr/bin/lsappinfo", &["list"]).unwrap_or_default();
        let mut names = BTreeSet::new();
        let mut block = String::new();
        for line in text.lines().chain(std::iter::once("")) {
            if is_record_header(line.trim()) {
                collect_tray_block(&block, &mut names);
                block.clear();
            }
            block.push_str(line);
            block.push('\n');
            if names.len() >= MAX_LIST_ITEMS {
                break;
            }
        }
        names.into_iter().collect()
    }
    #[cfg(windows)]
    {
        Vec::new()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
fn collect_tray_block(block: &str, names: &mut BTreeSet<String>) {
    if !block.contains("type=\"UIElement\"") && !block.contains("type=\"BackgroundOnly\"") {
        return;
    }
    let Some(bundle) = property(block, "bundleID") else {
        return;
    };
    if !bundle.is_empty() && !bundle.starts_with("com.apple.") {
        names.insert(bundle.to_string());
    }
}

#[cfg(target_os = "macos")]
fn is_record_header(line: &str) -> bool {
    let Some((number, rest)) = line.split_once(") ") else {
        return false;
    };
    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()) && rest.starts_with('"')
}

#[cfg(target_os = "macos")]
fn property<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}=\"");
    block.lines().find_map(|line| {
        line.trim()
            .strip_prefix(&prefix)
            .and_then(|value| value.split('"').next())
    })
}

#[cfg(target_os = "macos")]
fn defaults_string(arguments: &[&str]) -> Option<String> {
    command_output("/usr/bin/defaults", arguments)
}

#[cfg(target_os = "macos")]
fn defaults_array(arguments: &[&str]) -> Vec<String> {
    let text = command_output("/usr/bin/defaults", arguments).unwrap_or_default();
    text.lines()
        .map(|line| {
            line.trim().trim_matches(|character| {
                character == '"' || character == ',' || character == ')' || character == '('
            })
        })
        .filter(|line| !line.is_empty())
        .take(MAX_LIST_ITEMS)
        .map(str::to_string)
        .collect()
}

fn command_output(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program).args(arguments).output().ok()?;
    if !output.status.success() || output.stdout.len() > MAX_COMMAND_BYTES {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::collect;

    #[test]
    fn collects_device_info_without_network_access() {
        let info = collect("http://127.0.0.1:9/");
        let value = serde_json::to_value(&info).unwrap();
        assert!(value.get("hardware").is_some());
        assert!(value.get("disks").is_some());
        assert!(value.get("network").is_some());
        assert!(value.get("trayApplications").is_some());
        assert!(value["hardware"]["logicalCores"].as_u64().unwrap() >= 1);
        assert!(value["hardware"]["totalMemoryBytes"].as_u64().unwrap() > 0);
        assert!(value["disks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|disk| disk["totalBytes"].as_u64().unwrap() > 0));
        assert!(value["network"]["detectedPublicIp"].is_null());
        assert!(value["languages"].is_array());
        assert!(value["localTime"].is_string());
        assert!(value["trayApplications"].is_array());
    }
}
