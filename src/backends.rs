use crate::model::{Battery, GpuStat, Level, Mount, Radio, Temp, Toggle};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use zbus::zvariant::OwnedObjectPath;
use zbus::blocking::Connection;

pub fn command_output(cmd: &mut Command, timeout: Duration) -> Option<std::process::Output> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(12));
            }
            Err(_) => return None,
        }
    }
}

fn system_bus() -> Result<Connection, String> {
    if !Path::new("/run/dbus/system_bus_socket").exists() {
        return Err("system bus unavailable".into());
    }
    Connection::system().map_err(|e| format!("dbus: {e}"))
}

fn proxy<'a>(
    conn: &'a Connection,
    dest: &'a str,
    path: &'a str,
    iface: &'a str,
) -> Result<zbus::blocking::Proxy<'a>, String> {
    zbus::blocking::Proxy::new(conn, dest, path, iface).map_err(|e| e.to_string())
}

pub fn read_wifi() -> Result<Radio, String> {
    let conn = system_bus()?;
    let nm = proxy(
        &conn,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager",
        "org.freedesktop.NetworkManager",
    )?;
    let enabled: bool = nm
        .get_property("WirelessEnabled")
        .map_err(|e| format!("WirelessEnabled: {e}"))?;
    let mut radio = Radio {
        available: true,
        enabled,
        headline: "NO LINK".into(),
        detail: "NetworkManager".into(),
        devices: Vec::new(),
        signal: 0.0,
    };
    let paths: Vec<OwnedObjectPath> = nm.call("GetDevices", &()).unwrap_or_default();
    for path in paths {
        let dev = match proxy(
            &conn,
            "org.freedesktop.NetworkManager",
            path.as_str(),
            "org.freedesktop.NetworkManager.Device",
        ) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let kind: u32 = dev.get_property("DeviceType").unwrap_or(0);
        if kind != 2 {
            continue;
        }
        let iface: String = dev.get_property("Interface").unwrap_or_else(|_| "wlan".into());
        radio.devices.push(iface.clone());
        let wifi = match proxy(
            &conn,
            "org.freedesktop.NetworkManager",
            path.as_str(),
            "org.freedesktop.NetworkManager.Device.Wireless",
        ) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let ap: OwnedObjectPath = match wifi.get_property("ActiveAccessPoint") {
            Ok(p) => p,
            Err(_) => continue,
        };
        if ap.as_str() == "/" || ap.as_str().is_empty() {
            radio.headline = format!("{iface} idle");
            continue;
        }
        let ap_p = match proxy(
            &conn,
            "org.freedesktop.NetworkManager",
            ap.as_str(),
            "org.freedesktop.NetworkManager.AccessPoint",
        ) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let ssid: Vec<u8> = ap_p.get_property("Ssid").unwrap_or_default();
        let strength: u8 = ap_p.get_property("Strength").unwrap_or(0);
        let name = String::from_utf8_lossy(&ssid).trim().to_string();
        radio.headline = if name.is_empty() { iface } else { name };
        radio.signal = strength as f32 / 100.0;
        radio.detail = format!("signal {strength}%");
        break;
    }
    Ok(radio)
}

pub fn set_wifi(on: bool) -> Result<(), String> {
    let conn = system_bus()?;
    let nm = proxy(
        &conn,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager",
        "org.freedesktop.NetworkManager",
    )?;
    nm.set_property("WirelessEnabled", on)
        .map_err(|e| format!("wifi toggle failed: {e}"))
}

pub fn read_bluetooth() -> Result<Radio, String> {
    let conn = system_bus()?;
    let om = proxy(
        &conn,
        "org.bluez",
        "/",
        "org.freedesktop.DBus.ObjectManager",
    )?;
    let objects: std::collections::HashMap<
        OwnedObjectPath,
        std::collections::HashMap<String, std::collections::HashMap<String, zbus::zvariant::OwnedValue>>,
    > = om
        .call("GetManagedObjects", &())
        .map_err(|e| format!("BlueZ: {e}"))?;
    let mut radio = Radio {
        available: false,
        enabled: false,
        headline: "NO ADAPTER".into(),
        detail: "BlueZ".into(),
        devices: Vec::new(),
        signal: 0.0,
    };
    for (path, ifaces) in &objects {
        if let Some(props) = ifaces.get("org.bluez.Adapter1") {
            radio.available = true;
            radio.enabled = owned_bool(props.get("Powered")).unwrap_or(false);
            radio.headline = owned_string(props.get("Name")).unwrap_or_else(|| "hci".into());
            radio.detail = path.to_string();
        }
        if let Some(props) = ifaces.get("org.bluez.Device1") {
            let name = owned_string(props.get("Name"))
                .or_else(|| owned_string(props.get("Address")))
                .unwrap_or_else(|| "device".into());
            let connected = owned_bool(props.get("Connected")).unwrap_or(false);
            let label = if connected {
                format!("{name} *")
            } else {
                name
            };
            radio.devices.push(label);
        }
    }
    radio.devices.truncate(5);
    if radio.available {
        Ok(radio)
    } else {
        Err("BlueZ adapter unavailable".into())
    }
}

fn owned_bool(v: Option<&zbus::zvariant::OwnedValue>) -> Option<bool> {
    let v = v?;
    bool::try_from(v).ok()
}

fn owned_string(v: Option<&zbus::zvariant::OwnedValue>) -> Option<String> {
    let v = v?;
    <&str>::try_from(v).ok().map(|s| s.to_string())
}

pub fn set_bluetooth(on: bool) -> Result<(), String> {
    let radio = read_bluetooth()?;
    let path = radio.detail;
    if !path.starts_with('/') {
        return Err("BlueZ adapter path missing".into());
    }
    let conn = system_bus()?;
    let props = proxy(
        &conn,
        "org.bluez",
        &path,
        "org.freedesktop.DBus.Properties",
    )?;
    let body = (
        "org.bluez.Adapter1",
        "Powered",
        zbus::zvariant::Value::from(on),
    );
    props
        .call::<_, _, ()>("Set", &body)
        .map_err(|e| format!("bluetooth toggle failed: {e}"))?;
    Ok(())
}

pub fn read_power_profile() -> Result<String, String> {
    let conn = system_bus()?;
    let proxy = proxy(
        &conn,
        "org.freedesktop.UPower.PowerProfiles",
        "/org/freedesktop/UPower/PowerProfiles",
        "org.freedesktop.UPower.PowerProfiles",
    )?;
    proxy
        .get_property("ActiveProfile")
        .map_err(|e| format!("power profile: {e}"))
}

pub fn set_power_profile(profile: &str) -> Result<(), String> {
    let conn = system_bus()?;
    let proxy = proxy(
        &conn,
        "org.freedesktop.UPower.PowerProfiles",
        "/org/freedesktop/UPower/PowerProfiles",
        "org.freedesktop.UPower.PowerProfiles",
    )?;
    proxy
        .set_property("ActiveProfile", profile)
        .map_err(|e| format!("profile set failed: {e}"))
}

pub fn read_volume() -> Result<Level, String> {
    if let Some(level) = volume_wpctl() {
        return Ok(level);
    }
    volume_pactl().ok_or_else(|| "wpctl/pactl unavailable".into())
}

fn volume_wpctl() -> Option<Level> {
    let out = command_output(
        Command::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]),
        Duration::from_millis(700),
    )?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parts = text.split_whitespace();
    let _ = parts.next();
    let value: f32 = parts.next()?.parse().ok()?;
    Some(Level {
        available: true,
        value: value.clamp(0.0, 1.5),
        muted: text.contains("MUTED"),
        detail: "wpctl".into(),
    })
}

fn volume_pactl() -> Option<Level> {
    let vol = command_output(
        Command::new("pactl").args(["get-sink-volume", "@DEFAULT_SINK@"]),
        Duration::from_millis(700),
    )?;
    if !vol.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&vol.stdout);
    let pct = text.split('%').next()?;
    let num = pct.split_whitespace().last()?.parse::<f32>().ok()?;
    let mute = command_output(
        Command::new("pactl").args(["get-sink-mute", "@DEFAULT_SINK@"]),
        Duration::from_millis(700),
    )
    .map(|o| String::from_utf8_lossy(&o.stdout).contains("yes"))
    .unwrap_or(false);
    Some(Level {
        available: true,
        value: (num / 100.0).clamp(0.0, 1.5),
        muted: mute,
        detail: "pactl".into(),
    })
}

pub fn set_volume(value: f32) -> Result<(), String> {
    let v = value.clamp(0.0, 1.0);
    if command_output(
        Command::new("wpctl").args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{v:.3}")]),
        Duration::from_millis(700),
    )
    .map(|o| o.status.success())
    .unwrap_or(false)
    {
        return Ok(());
    }
    let pct = format!("{}%", (v * 100.0).round() as u32);
    if command_output(
        Command::new("pactl").args(["set-sink-volume", "@DEFAULT_SINK@", &pct]),
        Duration::from_millis(700),
    )
    .map(|o| o.status.success())
    .unwrap_or(false)
    {
        return Ok(());
    }
    Err("wpctl/pactl unavailable".into())
}

pub fn set_mute(muted: bool) -> Result<(), String> {
    let flag = if muted { "1" } else { "0" };
    if command_output(
        Command::new("wpctl").args(["set-mute", "@DEFAULT_AUDIO_SINK@", flag]),
        Duration::from_millis(700),
    )
    .map(|o| o.status.success())
    .unwrap_or(false)
    {
        return Ok(());
    }
    let flag = if muted { "1" } else { "0" };
    if command_output(
        Command::new("pactl").args(["set-sink-mute", "@DEFAULT_SINK@", flag]),
        Duration::from_millis(700),
    )
    .map(|o| o.status.success())
    .unwrap_or(false)
    {
        return Ok(());
    }
    Err("mute control unavailable".into())
}

pub fn read_brightness() -> Result<Level, String> {
    if let Some((path, cur, max)) = backlight() {
        return Ok(Level {
            available: true,
            value: cur as f32 / max as f32,
            muted: false,
            detail: path.file_name().unwrap_or_default().to_string_lossy().into(),
        });
    }
    if let Some(out) = command_output(Command::new("brightnessctl").arg("-m"), Duration::from_millis(600))
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            let parts: Vec<&str> = text.trim().split(',').collect();
            if parts.len() >= 4 {
                let pct = parts[3].trim_end_matches('%').parse::<f32>().unwrap_or(0.0);
                return Ok(Level {
                    available: true,
                    value: (pct / 100.0).clamp(0.0, 1.0),
                    muted: false,
                    detail: "brightnessctl".into(),
                });
            }
        }
    }
    Err("no backlight".into())
}

fn backlight() -> Option<(PathBuf, u32, u32)> {
    let mut best: Option<(PathBuf, u32, u32)> = None;
    for entry in fs::read_dir("/sys/class/backlight").ok()?.flatten() {
        let Ok(cur_txt) = fs::read_to_string(entry.path().join("brightness")) else {
            continue;
        };
        let Ok(max_txt) = fs::read_to_string(entry.path().join("max_brightness")) else {
            continue;
        };
        let Ok(cur) = cur_txt.trim().parse::<u32>() else {
            continue;
        };
        let Ok(max) = max_txt.trim().parse::<u32>() else {
            continue;
        };
        if max > 0 && best.as_ref().map(|(_, _, m)| max > *m).unwrap_or(true) {
            best = Some((entry.path(), cur, max));
        }
    }
    best
}

pub fn set_brightness(value: f32) -> Result<(), String> {
    let value = value.clamp(0.02, 1.0);
    if let Some((path, _, max)) = backlight() {
        let v = (value * max as f32).round() as u32;
        if fs::write(path.join("brightness"), v.to_string()).is_ok() {
            return Ok(());
        }
    }
    let pct = format!("{}%", (value * 100.0).round() as u32);
    if command_output(
        Command::new("brightnessctl").args(["set", &pct]),
        Duration::from_millis(700),
    )
    .map(|o| o.status.success())
    .unwrap_or(false)
    {
        return Ok(());
    }
    Err("brightness control unavailable".into())
}

pub fn read_dnd() -> Result<Toggle, String> {
    if let Some(out) = command_output(Command::new("dunstctl").arg("is-paused"), Duration::from_millis(500))
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            return Ok(Toggle {
                available: true,
                enabled: text.contains("true"),
                detail: "dunst".into(),
            });
        }
    }
    Err("dunst/mako unavailable".into())
}

pub fn set_dnd(on: bool) -> Result<(), String> {
    let flag = if on { "true" } else { "false" };
    if command_output(
        Command::new("dunstctl").args(["set-paused", flag]),
        Duration::from_millis(600),
    )
    .map(|o| o.status.success())
    .unwrap_or(false)
    {
        return Ok(());
    }
    let args: &[&str] = if on {
        &["mode", "-a", "do-not-disturb"]
    } else {
        &["mode", "-r", "do-not-disturb"]
    };
    if command_output(Command::new("makoctl").args(args), Duration::from_millis(600))
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Ok(());
    }
    Err("dunst/mako unavailable".into())
}

pub fn read_night() -> Result<Toggle, String> {
    let running = process_running("gammastep") || process_running("redshift") || process_running("wlsunset");
    if which("gammastep") || which("redshift") || which("wlsunset") {
        return Ok(Toggle {
            available: true,
            enabled: running,
            detail: "gammastep/redshift".into(),
        });
    }
    Err("gammastep/redshift unavailable".into())
}

pub fn set_night(on: bool) -> Result<(), String> {
    if on {
        if command_output(
            Command::new("gammastep").args(["-O", "4000", "-P"]),
            Duration::from_millis(800),
        )
        .map(|o| o.status.success())
        .unwrap_or(false)
        {
            return Ok(());
        }
        if command_output(
            Command::new("redshift").args(["-O", "4000", "-P"]),
            Duration::from_millis(800),
        )
        .map(|o| o.status.success())
        .unwrap_or(false)
        {
            return Ok(());
        }
    } else if command_output(Command::new("gammastep").arg("-x"), Duration::from_millis(800))
        .map(|o| o.status.success())
        .unwrap_or(false)
        || command_output(Command::new("redshift").arg("-x"), Duration::from_millis(800))
            .map(|o| o.status.success())
            .unwrap_or(false)
    {
        return Ok(());
    }
    Err("gammastep/redshift unavailable".into())
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
        .unwrap_or(false)
}

fn process_running(name: &str) -> bool {
    let Ok(entries) = fs::read_dir("/proc") else {
        return false;
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let comm = fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
        if comm.trim() == name {
            return true;
        }
    }
    false
}

pub fn read_battery() -> Battery {
    let Ok(dir) = fs::read_dir("/sys/class/power_supply") else {
        return Battery {
            present: false,
            percent: 0.0,
            status: "AC".into(),
        };
    };
    for entry in dir.flatten() {
        let kind = fs::read_to_string(entry.path().join("type")).unwrap_or_default();
        if !kind.to_ascii_lowercase().contains("battery") {
            continue;
        }
        let percent = fs::read_to_string(entry.path().join("capacity"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0.0);
        let status = fs::read_to_string(entry.path().join("status"))
            .unwrap_or_else(|_| "Unknown".into());
        return Battery {
            present: true,
            percent,
            status: status.trim().to_string(),
        };
    }
    Battery {
        present: false,
        percent: 0.0,
        status: "AC".into(),
    }
}

pub fn read_gpu() -> GpuStat {
    let Ok(dir) = fs::read_dir("/sys/class/drm") else {
        return GpuStat {
            present: false,
            name: "no drm".into(),
            util: 0.0,
            temp_c: None,
        };
    };
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let busy = entry.path().join("device/gpu_busy_percent");
        if let Ok(text) = fs::read_to_string(&busy) {
            if let Ok(v) = text.trim().parse::<f32>() {
                let temp = fs::read_dir(entry.path().join("device/hwmon"))
                    .ok()
                    .and_then(|rd| {
                        rd.flatten().find_map(|hw| {
                            fs::read_to_string(hw.path().join("temp1_input"))
                                .ok()
                                .and_then(|s| s.trim().parse::<f32>().ok())
                                .map(|c| c / 1000.0)
                        })
                    });
                return GpuStat {
                    present: true,
                    name,
                    util: (v / 100.0).clamp(0.0, 1.0),
                    temp_c: temp,
                };
            }
        }
    }
    GpuStat {
        present: false,
        name: "unavailable".into(),
        util: 0.0,
        temp_c: None,
    }
}

pub fn read_journal() -> Vec<String> {
    if let Some(out) = command_output(
        Command::new("journalctl").args(["-n", "7", "--no-pager", "-o", "short", "--quiet"]),
        Duration::from_millis(900),
    ) {
        if out.status.success() {
            let lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            if !lines.is_empty() {
                return lines;
            }
        }
    }
    vec!["journal unavailable".into()]
}

pub fn offline_radio(title: &str, err: String) -> Radio {
    Radio {
        available: false,
        enabled: false,
        headline: title.into(),
        detail: err,
        devices: Vec::new(),
        signal: 0.0,
    }
}

pub fn offline_level(err: String) -> Level {
    Level {
        available: false,
        value: 0.0,
        muted: false,
        detail: err,
    }
}

pub fn offline_toggle(err: String) -> Toggle {
    Toggle {
        available: false,
        enabled: false,
        detail: err,
    }
}

pub fn read_mounts() -> Vec<Mount> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut mounts = Vec::new();
    for disk in disks.list() {
        let total = disk.total_space();
        if total < 64 * 1024 * 1024 {
            continue;
        }
        let avail = disk.available_space();
        let used = total.saturating_sub(avail);
        let name = disk.mount_point().display().to_string();
        if mounts.iter().any(|m: &Mount| m.name == name) {
            continue;
        }
        mounts.push(Mount { name, total, used });
    }
    mounts.sort_by(|a, b| b.total.cmp(&a.total));
    mounts.truncate(4);
    mounts
}

pub fn read_temps() -> Vec<Temp> {
    let comps = sysinfo::Components::new_with_refreshed_list();
    let mut out = Vec::new();
    for comp in &comps {
        let temp = comp.temperature();
        if let Some(c) = temp.filter(|c| *c > 1.0 && *c < 140.0) {
            out.push(Temp {
                label: comp.label().to_string(),
                celsius: c,
            });
        }
    }
    out.truncate(6);
    out
}
