use crate::backends;
use crate::model::{
    parse_diskstats, parse_netdev, Battery, Command, GpuStat, Level, Radio, Snapshot, Toggle,
};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{ProcessesToUpdate, System};

pub struct SystemHandle {
    snap: Arc<Mutex<Snapshot>>,
    tx: Sender<Command>,
}

impl SystemHandle {
    pub fn start(demo: bool) -> Self {
        let snap = Arc::new(Mutex::new(Snapshot::placeholder(demo)));
        let (tx, rx) = mpsc::channel();
        let shared = Arc::clone(&snap);
        thread::Builder::new()
            .name("acc-sampler".into())
            .spawn(move || {
                if demo {
                    demo_loop(rx, shared);
                } else {
                    live_loop(rx, shared);
                }
            })
            .expect("sampler thread");
        Self { snap, tx }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snap.lock().expect("snapshot").clone()
    }

    /// Copy the latest sample only when the background thread published a new one.
    pub fn copy_if_newer(&self, dest: &mut Snapshot) -> bool {
        let guard = self.snap.lock().expect("snapshot");
        if guard.generation == dest.generation && dest.generation != 0 {
            return false;
        }
        *dest = guard.clone();
        true
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }
}

fn publish(slot: &Mutex<Snapshot>, mut snap: Snapshot, generation: &mut u64) {
    *generation += 1;
    snap.generation = *generation;
    *slot.lock().expect("snapshot") = snap;
}

fn demo_loop(rx: Receiver<Command>, slot: Arc<Mutex<Snapshot>>) {
    let mut wifi = true;
    let mut bt = true;
    let mut volume = 0.67_f32;
    let mut muted = false;
    let mut bright = 0.46_f32;
    let mut dnd = false;
    let mut night = false;
    let mut profile = 1usize;
    let profiles = ["power-saver", "balanced", "performance"];
    let mut t = 0.0_f32;
    let mut generation = 0_u64;
    let started = Instant::now();
    loop {
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Command::Wifi(v) => wifi = v,
                Command::Bluetooth(v) => bt = v,
                Command::Volume(v) => {
                    volume = v.clamp(0.0, 1.0);
                    muted = false;
                }
                Command::Mute(v) => muted = v,
                Command::Brightness(v) => bright = v.clamp(0.02, 1.0),
                Command::Dnd(v) => dnd = v,
                Command::Night(v) => night = v,
                Command::CycleProfile => profile = (profile + 1) % profiles.len(),
            }
        }
        t += 0.22;
        let mut snap = Snapshot::placeholder(true);
        snap.hostname = "zenith".into();
        snap.kernel = "6.8.9-arch1-1".into();
        snap.os_name = "Arch Linux".into();
        snap.arch = "x86_64".into();
        snap.uptime_secs = 3 * 3600 + 17 * 60 + started.elapsed().as_secs();
        snap.load = [
            0.42 + 0.2 * (t * 0.3).sin().abs(),
            0.38,
            0.31,
        ];
        snap.cpu = (0..8)
            .map(|i| {
                let w = (t * (0.7 + i as f32 * 0.13) + i as f32).sin() * 0.5 + 0.5;
                (0.12 + 0.72 * w).clamp(0.04, 0.98)
            })
            .collect();
        snap.cpu_avg = snap.cpu.iter().sum::<f32>() / snap.cpu.len() as f32;
        snap.cpu_freq_mhz = 3200 + (400.0 * (t * 0.4).sin()) as u64;
        snap.mem_total = 32 * 1024 * 1024 * 1024;
        snap.mem_used = (14.2 + 1.4 * (t * 0.15).sin()) as u64 * 1024 * 1024 * 1024 / 1;
        // The line above is integer division mess. Set explicitly:
        let used_gib = 13.6 + 1.1 * (t * 0.17).sin();
        snap.mem_used = (used_gib * 1024.0 * 1024.0 * 1024.0) as u64;
        snap.mem_avail = snap.mem_total.saturating_sub(snap.mem_used);
        snap.mounts = vec![
            crate::model::Mount {
                name: "/".into(),
                total: 512 * 1024 * 1024 * 1024,
                used: (330.0 * 1024.0 * 1024.0 * 1024.0) as u64,
            },
            crate::model::Mount {
                name: "/home".into(),
                total: 1800 * 1024 * 1024 * 1024,
                used: (640.0 * 1024.0 * 1024.0 * 1024.0) as u64,
            },
            crate::model::Mount {
                name: "/var".into(),
                total: 80 * 1024 * 1024 * 1024,
                used: (22.0 * 1024.0 * 1024.0 * 1024.0) as u64,
            },
        ];
        snap.disk_r_bps = ((18.0 + 22.0 * (t * 0.8).sin().abs()) * 1024.0 * 1024.0) as f64;
        snap.disk_w_bps = ((6.0 + 14.0 * (t * 1.1).cos().abs()) * 1024.0 * 1024.0) as f64;
        snap.net_rx_bps = ((28.0 + 18.0 * (t * 0.6).sin().abs()) * 1_000_000.0 / 8.0) as f64;
        snap.net_tx_bps = ((8.0 + 7.0 * (t * 0.9).cos().abs()) * 1_000_000.0 / 8.0) as f64;
        snap.processes = vec![
            ("firefox", 18.4 + 6.0 * (t).sin().abs(), 1_100_u64),
            ("pipewire", 2.1, 86),
            ("systemd", 1.4, 42),
            ("kworker", 4.2 * (t * 1.7).sin().abs(), 12),
            ("journald", 0.8, 64),
            ("arch-cc", 3.1, 180),
        ]
        .into_iter()
        .map(|(n, c, m)| crate::model::Proc {
            name: n.into(),
            cpu: c,
            mem: m * 1024 * 1024,
        })
        .collect();
        snap.temps = vec![
            crate::model::Temp {
                label: "k10temp".into(),
                celsius: 58.0 + 8.0 * (t * 0.25).sin().abs(),
            },
            crate::model::Temp {
                label: "nvme".into(),
                celsius: 41.0 + 3.0 * (t * 0.2).cos().abs(),
            },
            crate::model::Temp {
                label: "wifi".into(),
                celsius: 39.0,
            },
        ];
        snap.battery = Battery {
            present: true,
            percent: 76.0 + 4.0 * (t * 0.05).sin(),
            status: "Discharging".into(),
        };
        snap.gpu = GpuStat {
            present: true,
            name: "card0".into(),
            util: (0.22 + 0.35 * (t * 0.7).sin().abs()).clamp(0.0, 1.0),
            temp_c: Some(54.0 + 6.0 * (t * 0.3).sin().abs()),
        };
        snap.wifi = Radio {
            available: true,
            enabled: wifi,
            headline: if wifi { "Zenith-Net_5G".into() } else { "RADIO OFF".into() },
            detail: if wifi { "signal demo".into() } else { "disabled".into() },
            devices: vec!["wlan0".into()],
            signal: if wifi { 0.55 + 0.3 * (t * 0.4).sin().abs() } else { 0.0 },
        };
        snap.bluetooth = Radio {
            available: true,
            enabled: bt,
            headline: "hci0".into(),
            detail: "BlueZ demo".into(),
            devices: if bt {
                vec!["Keychron K8 *".into(), "WH-1000XM4".into()]
            } else {
                Vec::new()
            },
            signal: if bt { 0.8 } else { 0.0 },
        };
        snap.volume = Level {
            available: true,
            value: volume,
            muted,
            detail: "demo".into(),
        };
        snap.brightness = Level {
            available: true,
            value: bright,
            muted: false,
            detail: "demo".into(),
        };
        snap.dnd = Toggle {
            available: true,
            enabled: dnd,
            detail: "demo".into(),
        };
        snap.night = Toggle {
            available: true,
            enabled: night,
            detail: "demo".into(),
        };
        snap.power_profile = profiles[profile].into();
        snap.power_available = true;
        let logs = [
            "systemd[1]: Started Session of user arch",
            "kernel: wlan0: associated Zenith-Net_5G",
            "NetworkManager: DHCPACK on wlan0",
            "pipewire: graph rate 48000",
            "kernel: nvme nvme0: temp ok",
            "bluetoothd: endpoint connected",
            "power-profiles: balanced",
            "arch-control-center: panel online",
        ];
        let shift = (t as usize / 3) % logs.len();
        snap.log_lines = (0..6).map(|i| logs[(shift + i) % logs.len()].into()).collect();
        publish(&slot, snap, &mut generation);
        thread::sleep(Duration::from_millis(60));
    }
}

struct Live {
    sys: System,
    wifi: Radio,
    bluetooth: Radio,
    volume: Level,
    brightness: Level,
    dnd: Toggle,
    night: Toggle,
    profile: String,
    profile_ok: bool,
    mounts: Vec<crate::model::Mount>,
    temps: Vec<crate::model::Temp>,
    battery: Battery,
    gpu: GpuStat,
    logs: Vec<String>,
    prev_rx: u64,
    prev_tx: u64,
    prev_dr: u64,
    prev_dw: u64,
    have_io: bool,
    last_slow: Instant,
    last_log: Instant,
}

fn live_loop(rx: Receiver<Command>, slot: Arc<Mutex<Snapshot>>) {
    let mut live = Live {
        sys: System::new_all(),
        wifi: backends::offline_radio("WI-FI", "probing".into()),
        bluetooth: backends::offline_radio("BT", "probing".into()),
        volume: backends::offline_level("probing".into()),
        brightness: backends::offline_level("probing".into()),
        dnd: backends::offline_toggle("probing".into()),
        night: backends::offline_toggle("probing".into()),
        profile: "unknown".into(),
        profile_ok: false,
        mounts: Vec::new(),
        temps: Vec::new(),
        battery: Battery {
            present: false,
            percent: 0.0,
            status: "AC".into(),
        },
        gpu: GpuStat {
            present: false,
            name: "—".into(),
            util: 0.0,
            temp_c: None,
        },
        logs: vec!["journal…".into()],
        prev_rx: 0,
        prev_tx: 0,
        prev_dr: 0,
        prev_dw: 0,
        have_io: false,
        last_slow: Instant::now() - Duration::from_secs(10),
        last_log: Instant::now() - Duration::from_secs(10),
    };
    thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    live.sys.refresh_cpu_all();
    let mut generation = 0_u64;
    loop {
        let tick = Instant::now();
        while let Ok(cmd) = rx.try_recv() {
            apply_live(&mut live, cmd);
        }
        if live.last_slow.elapsed() > Duration::from_millis(1200) {
            refresh_slow(&mut live);
            live.last_slow = Instant::now();
        }
        if live.last_log.elapsed() > Duration::from_secs(3) {
            live.logs = backends::read_journal();
            live.last_log = Instant::now();
        }
        let snap = sample_fast(&mut live);
        publish(&slot, snap, &mut generation);
        let spent = tick.elapsed();
        if spent < Duration::from_millis(70) {
            thread::sleep(Duration::from_millis(70) - spent);
        }
    }
}

fn refresh_slow(live: &mut Live) {
    live.wifi = backends::read_wifi().unwrap_or_else(|e| backends::offline_radio("WI-FI", e));
    live.bluetooth = backends::read_bluetooth().unwrap_or_else(|e| backends::offline_radio("BT", e));
    live.volume = backends::read_volume().unwrap_or_else(backends::offline_level);
    live.brightness = backends::read_brightness().unwrap_or_else(backends::offline_level);
    live.dnd = backends::read_dnd().unwrap_or_else(backends::offline_toggle);
    live.night = backends::read_night().unwrap_or_else(backends::offline_toggle);
    match backends::read_power_profile() {
        Ok(p) => {
            live.profile = p;
            live.profile_ok = true;
        }
        Err(_) => {
            live.profile_ok = false;
            if live.profile == "unknown" {
                live.profile = "unavailable".into();
            }
        }
    }
    live.mounts = backends::read_mounts();
    live.temps = backends::read_temps();
    live.battery = backends::read_battery();
    live.gpu = backends::read_gpu();
}

fn apply_live(live: &mut Live, cmd: Command) {
    match cmd {
        Command::Wifi(on) => match backends::set_wifi(on) {
            Ok(()) => {
                live.wifi.available = true;
                live.wifi.enabled = on;
                if !on {
                    live.wifi.headline = "RADIO OFF".into();
                }
            }
            Err(e) => live.wifi.detail = e,
        },
        Command::Bluetooth(on) => match backends::set_bluetooth(on) {
            Ok(()) => {
                live.bluetooth.available = true;
                live.bluetooth.enabled = on;
            }
            Err(e) => live.bluetooth.detail = e,
        },
        Command::Volume(v) => match backends::set_volume(v) {
            Ok(()) => {
                live.volume.available = true;
                live.volume.value = v;
                live.volume.muted = false;
            }
            Err(e) => live.volume.detail = e,
        },
        Command::Mute(m) => match backends::set_mute(m) {
            Ok(()) => {
                live.volume.available = true;
                live.volume.muted = m;
            }
            Err(e) => live.volume.detail = e,
        },
        Command::Brightness(v) => match backends::set_brightness(v) {
            Ok(()) => {
                live.brightness.available = true;
                live.brightness.value = v;
            }
            Err(e) => live.brightness.detail = e,
        },
        Command::Dnd(on) => match backends::set_dnd(on) {
            Ok(()) => {
                live.dnd.available = true;
                live.dnd.enabled = on;
            }
            Err(e) => live.dnd.detail = e,
        },
        Command::Night(on) => match backends::set_night(on) {
            Ok(()) => {
                live.night.available = true;
                live.night.enabled = on;
            }
            Err(e) => live.night.detail = e,
        },
        Command::CycleProfile => {
            let order = ["power-saver", "balanced", "performance"];
            let idx = order.iter().position(|p| *p == live.profile).unwrap_or(1);
            let next = order[(idx + 1) % order.len()];
            match backends::set_power_profile(next) {
                Ok(()) => {
                    live.profile = next.into();
                    live.profile_ok = true;
                }
                Err(e) => {
                    live.profile_ok = false;
                    live.profile = e;
                }
            }
        }
    }
}

fn sample_fast(live: &mut Live) -> Snapshot {
    live.sys.refresh_cpu_all();
    live.sys.refresh_memory();
    live.sys
        .refresh_processes(ProcessesToUpdate::All, true);
    let dt = 0.28_f64;
    let (rx, tx) = std::fs::read_to_string("/proc/net/dev")
        .map(|t| parse_netdev(&t))
        .unwrap_or((0, 0));
    let (dr, dw) = std::fs::read_to_string("/proc/diskstats")
        .map(|t| parse_diskstats(&t))
        .unwrap_or((0, 0));
    let (rx_bps, tx_bps, dr_bps, dw_bps) = if live.have_io {
        (
            rate(live.prev_rx, rx, dt),
            rate(live.prev_tx, tx, dt),
            rate(live.prev_dr, dr, dt),
            rate(live.prev_dw, dw, dt),
        )
    } else {
        live.have_io = true;
        (0.0, 0.0, 0.0, 0.0)
    };
    live.prev_rx = rx;
    live.prev_tx = tx;
    live.prev_dr = dr;
    live.prev_dw = dw;

    let mut procs: Vec<_> = live
        .sys
        .processes()
        .values()
        .map(|p| crate::model::Proc {
            name: p.name().to_string_lossy().to_string(),
            cpu: p.cpu_usage(),
            mem: p.memory(),
        })
        .collect();
    procs.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal));
    procs.truncate(6);

    let cpu: Vec<f32> = live
        .sys
        .cpus()
        .iter()
        .map(|c| (c.cpu_usage() / 100.0).clamp(0.0, 1.0))
        .collect();
    let cpu_avg = if cpu.is_empty() {
        0.0
    } else {
        cpu.iter().sum::<f32>() / cpu.len() as f32
    };
    let freq = live.sys.cpus().first().map(|c| c.frequency()).unwrap_or(0);
    let load = read_load();

    let mut snap = Snapshot::placeholder(false);
    snap.hostname = System::host_name().unwrap_or_else(|| "host".into());
    snap.kernel = System::kernel_version().unwrap_or_else(|| "unknown".into());
    snap.os_name = System::long_os_version()
        .or_else(System::name)
        .unwrap_or_else(|| "Linux".into());
    snap.arch = System::cpu_arch();
    snap.uptime_secs = System::uptime();
    snap.load = load;
    snap.cpu = if cpu.is_empty() { vec![0.0] } else { cpu };
    snap.cpu_avg = cpu_avg;
    snap.cpu_freq_mhz = freq;
    snap.mem_total = live.sys.total_memory();
    snap.mem_used = live.sys.used_memory();
    snap.mem_avail = live.sys.available_memory();
    snap.mounts = live.mounts.clone();
    snap.disk_r_bps = dr_bps;
    snap.disk_w_bps = dw_bps;
    snap.net_rx_bps = rx_bps;
    snap.net_tx_bps = tx_bps;
    snap.processes = procs;
    snap.temps = live.temps.clone();
    snap.battery = live.battery.clone();
    snap.gpu = live.gpu.clone();
    snap.wifi = live.wifi.clone();
    snap.bluetooth = live.bluetooth.clone();
    snap.volume = live.volume.clone();
    snap.brightness = live.brightness.clone();
    snap.dnd = live.dnd.clone();
    snap.night = live.night.clone();
    snap.power_profile = live.profile.clone();
    snap.power_available = live.profile_ok;
    snap.log_lines = live.logs.clone();
    snap
}

fn rate(prev: u64, now: u64, dt: f64) -> f64 {
    if now >= prev && dt > 0.0 {
        (now - prev) as f64 / dt
    } else {
        0.0
    }
}

fn read_load() -> [f32; 3] {
    let text = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let mut it = text.split_whitespace();
    let a = it.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let b = it.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let c = it.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    [a, b, c]
}
