use schemars::JsonSchema;
use serde::Serialize;

/// The power source and the battery's charge, as last read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    /// The charge in percent, when the computer has a battery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    /// Whether the computer runs on its battery (not on mains power).
    pub on_battery: bool,
    /// Whether the battery is charging.
    pub charging: bool,
}

pub trait Power: Send + Sync {
    fn read(&self) -> Option<Battery>;
}

pub fn parse_pmset(text: &str) -> Option<Battery> {
    let source = text.lines().find_map(|line| line.split_once("drawing from '").map(|(_, rest)| rest))?;
    let on_battery = !source.starts_with("AC Power");
    let battery = text.lines().find(|line| line.contains("InternalBattery") || line.contains('%'));
    let percent = battery.and_then(|line| {
        let (before, _) = line.split_once('%')?;
        let digits: String =
            before.chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
        digits.parse::<u8>().ok().map(|percent| percent.min(100))
    });
    let state = battery.and_then(|line| line.split(';').nth(1)).map(str::trim).unwrap_or_default();
    let charging = state == "charging" || state == "finishing charge";
    Some(Battery { percent, on_battery, charging })
}

pub fn parse_power_supplies(supplies: &[(String, String, String, String)]) -> Option<Battery> {
    let mains = supplies.iter().any(|(kind, _, _, online)| kind.trim() == "Mains" && online.trim() == "1");
    let battery = supplies.iter().find(|(kind, ..)| kind.trim() == "Battery");
    if battery.is_none() && supplies.is_empty() {
        return None;
    }
    let status = battery.map(|(_, status, ..)| status.trim()).unwrap_or_default();
    let percent = battery.and_then(|(_, _, capacity, _)| capacity.trim().parse::<u8>().ok()).map(|p| p.min(100));
    let on_battery = battery.is_some() && !mains && status == "Discharging";
    Some(Battery { percent, on_battery, charging: status == "Charging" })
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemPower;

impl Power for SystemPower {
    fn read(&self) -> Option<Battery> {
        if cfg!(target_os = "macos") {
            let output = std::process::Command::new("/usr/bin/pmset")
                .args(["-g", "batt"])
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .ok()?;
            parse_pmset(&String::from_utf8_lossy(&output.stdout))
        } else if cfg!(target_os = "linux") {
            let entries = std::fs::read_dir("/sys/class/power_supply").ok()?;
            let read =
                |path: &std::path::Path, name: &str| std::fs::read_to_string(path.join(name)).unwrap_or_default();
            let supplies: Vec<_> = entries
                .flatten()
                .map(|entry| entry.path())
                .map(|path| {
                    (read(&path, "type"), read(&path, "status"), read(&path, "capacity"), read(&path, "online"))
                })
                .collect();
            parse_power_supplies(&supplies)
        } else {
            None
        }
    }
}

#[cfg(any(test, feature = "fake"))]
#[derive(Default)]
pub struct FakePower {
    pub pmset: std::sync::Mutex<String>,
    pub reads: std::sync::atomic::AtomicUsize,
}

#[cfg(any(test, feature = "fake"))]
impl FakePower {
    pub fn new(pmset: &str) -> Self {
        Self { pmset: std::sync::Mutex::new(pmset.to_owned()), reads: std::sync::atomic::AtomicUsize::new(0) }
    }

    pub fn set(&self, pmset: &str) {
        if let Ok(mut text) = self.pmset.lock() {
            pmset.clone_into(&mut text);
        }
    }
}

#[cfg(any(test, feature = "fake"))]
impl Power for FakePower {
    fn read(&self) -> Option<Battery> {
        self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.pmset.lock().ok().and_then(|text| parse_pmset(&text))
    }
}
