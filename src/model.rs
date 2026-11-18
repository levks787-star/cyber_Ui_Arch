use std::collections::VecDeque;

pub const HIST: usize = 56;

#[derive(Clone, Debug)]
pub struct Mount {
    pub name: String,
    pub total: u64,
    pub used: u64,
}

#[derive(Clone, Debug)]
pub struct Proc {
    pub name: String,
    pub cpu: f32,
    pub mem: u64,
}

#[derive(Clone, Debug)]
pub struct Temp {
    pub label: String,
    pub celsius: f32,
}

#[derive(Clone, Debug)]
pub struct Battery {
    pub present: bool,
    pub percent: f32,
    pub status: String,
}

#[derive(Clone, Debug)]
pub struct GpuStat {
    pub present: bool,
    pub name: String,
    pub util: f32,
    pub temp_c: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct Radio {
    pub available: bool,
    pub enabled: bool,
    pub headline: String,
    pub detail: String,
    pub devices: Vec<String>,
    pub signal: f32,
}

#[derive(Clone, Debug)]
pub struct Level {
    pub available: bool,
    pub value: f32,
    pub muted: bool,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct Toggle {
    pub available: bool,
    pub enabled: bool,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub generation: u64,
    pub demo: bool,
    pub hostname: String,
    pub kernel: String,
    pub os_name: String,
    pub arch: String,
    pub uptime_secs: u64,
    pub load: [f32; 3],
    pub cpu: Vec<f32>,
    pub cpu_avg: f32,
    pub cpu_freq_mhz: u64,
    pub mem_total: u64,
    pub mem_used: u64,
    pub mem_avail: u64,
    pub mounts: Vec<Mount>,
    pub disk_r_bps: f64,
    pub disk_w_bps: f64,
    pub net_rx_bps: f64,
    pub net_tx_bps: f64,
    pub processes: Vec<Proc>,
    pub temps: Vec<Temp>,
    pub battery: Battery,
    pub gpu: GpuStat,
    pub wifi: Radio,
    pub bluetooth: Radio,
    pub volume: Level,
    pub brightness: Level,
    pub dnd: Toggle,
    pub night: Toggle,
    pub power_profile: String,
    pub power_available: bool,
    pub log_lines: Vec<String>,
}

impl Snapshot {
    pub fn placeholder(demo: bool) -> Self {
        Self {
            generation: 0,
            demo,
            hostname: "arch".into(),
            kernel: "—".into(),
            os_name: if demo { "Arch Linux".into() } else { "Linux".into() },
            arch: std::env::consts::ARCH.into(),
            uptime_secs: 0,
            load: [0.0; 3],
            cpu: vec![0.0; 4],
            cpu_avg: 0.0,
            cpu_freq_mhz: 0,
            mem_total: 0,
            mem_used: 0,
            mem_avail: 0,
            mounts: Vec::new(),
            disk_r_bps: 0.0,
            disk_w_bps: 0.0,
            net_rx_bps: 0.0,
            net_tx_bps: 0.0,
            processes: Vec::new(),
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
            wifi: Radio {
                available: false,
                enabled: false,
                headline: "WI-FI".into(),
                detail: "waiting".into(),
                devices: Vec::new(),
                signal: 0.0,
            },
            bluetooth: Radio {
                available: false,
                enabled: false,
                headline: "ADAPTER".into(),
                detail: "waiting".into(),
                devices: Vec::new(),
                signal: 0.0,
            },
            volume: Level {
                available: false,
                value: 0.0,
                muted: false,
                detail: "waiting".into(),
            },
            brightness: Level {
                available: false,
                value: 0.4,
                muted: false,
                detail: "waiting".into(),
            },
            dnd: Toggle {
                available: false,
                enabled: false,
                detail: "waiting".into(),
            },
            night: Toggle {
                available: false,
                enabled: false,
                detail: "waiting".into(),
            },
            power_profile: "unknown".into(),
            power_available: false,
            log_lines: vec!["sync…".into()],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Command {
    Wifi(bool),
    Bluetooth(bool),
    Volume(f32),
    Mute(bool),
    Brightness(f32),
    Dnd(bool),
    Night(bool),
    CycleProfile,
}

#[derive(Clone, Debug)]
pub struct Hist {
    pub cpu: VecDeque<f32>,
    pub mem: VecDeque<f32>,
    pub rx: VecDeque<f32>,
    pub tx: VecDeque<f32>,
    pub dr: VecDeque<f32>,
    pub dw: VecDeque<f32>,
    pub signal: VecDeque<f32>,
    pub cores: VecDeque<Vec<f32>>,
    pub seen: u64,
}

impl Hist {
    pub fn new() -> Self {
        Self {
            cpu: VecDeque::new(),
            mem: VecDeque::new(),
            rx: VecDeque::new(),
            tx: VecDeque::new(),
            dr: VecDeque::new(),
            dw: VecDeque::new(),
            signal: VecDeque::new(),
            cores: VecDeque::new(),
            seen: 0,
        }
    }

    pub fn seed_demo() -> Self {
        let mut h = Self::new();
        for i in 0..HIST {
            let t = i as f32 * 0.18;
            h.push_sample(
                0.32 + 0.18 * (t * 1.2).sin().abs(),
                0.46 + 0.04 * (t * 0.3).sin(),
                0.35 + 0.25 * (t * 0.8).sin().abs(),
                0.2 + 0.15 * (t * 1.1 + 1.0).cos().abs(),
                0.15 + 0.1 * (t * 0.6).sin().abs(),
                0.28 + 0.2 * (t * 0.9).cos().abs(),
                0.62 + 0.2 * (t * 0.5).sin(),
                &(0..8)
                    .map(|c| 0.25 + 0.35 * ((t * 1.4 + c as f32).sin() * 0.5 + 0.5))
                    .collect::<Vec<_>>(),
            );
        }
        h.seen = 0;
        h
    }

    pub fn push_sample(
        &mut self,
        cpu: f32,
        mem: f32,
        rx: f32,
        tx: f32,
        dr: f32,
        dw: f32,
        signal: f32,
        cores: &[f32],
    ) {
        push(&mut self.cpu, cpu);
        push(&mut self.mem, mem);
        push(&mut self.rx, rx);
        push(&mut self.tx, tx);
        push(&mut self.dr, dr);
        push(&mut self.dw, dw);
        push(&mut self.signal, signal);
        self.cores.push_back(cores.to_vec());
        while self.cores.len() > HIST {
            self.cores.pop_front();
        }
    }
}

fn push(q: &mut VecDeque<f32>, v: f32) {
    q.push_back(v);
    while q.len() > HIST {
        q.pop_front();
    }
}

pub fn fmt_uptime(secs: u64) -> String {
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3_600;
    let m = (secs % 3_600) / 60;
    let s = secs % 60;
    if d > 0 {
        format!("{d}d {h:02}:{m:02}:{s:02}")
    } else {
        format!("{h:02}:{m:02}:{s:02}")
    }
}

pub fn fmt_gib(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

pub fn fmt_mib(bytes: u64) -> String {
    let mib = bytes as f64 / (1024.0 * 1024.0);
    if mib >= 1024.0 {
        format!("{:.1}G", mib / 1024.0)
    } else if mib >= 10.0 {
        format!("{mib:.0}M")
    } else {
        format!("{mib:.1}M")
    }
}

pub fn fmt_mbps(bytes_per_sec: f64) -> String {
    let mbps = bytes_per_sec * 8.0 / 1_000_000.0;
    if mbps >= 100.0 {
        format!("{mbps:.0}")
    } else if mbps >= 10.0 {
        format!("{mbps:.1}")
    } else {
        format!("{mbps:.2}")
    }
}

pub fn fmt_mibs(bytes_per_sec: f64) -> String {
    let v = bytes_per_sec / (1024.0 * 1024.0);
    if v >= 100.0 {
        format!("{v:.0}")
    } else if v >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

pub fn parse_netdev(text: &str) -> (u64, u64) {
    let mut rx = 0u64;
    let mut tx = 0u64;
    for line in text.lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        if name.trim() == "lo" {
            continue;
        }
        let nums: Vec<u64> = rest.split_whitespace().filter_map(|s| s.parse().ok()).collect();
        if nums.len() >= 9 {
            rx += nums[0];
            tx += nums[8];
        }
    }
    (rx, tx)
}

fn whole_disk(name: &str) -> bool {
    if let Some(rest) = name.strip_prefix("nvme") {
        return rest.contains('n') && !name.contains('p');
    }
    if name.starts_with("mmcblk") {
        return !name.contains('p');
    }
    if name.starts_with("sd") || name.starts_with("vd") || name.starts_with("xvd") {
        return name.chars().last().is_some_and(|c| c.is_ascii_alphabetic());
    }
    false
}

pub fn parse_diskstats(text: &str) -> (u64, u64) {
    let mut read = 0u64;
    let mut write = 0u64;
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 14 || !whole_disk(parts[2]) {
            continue;
        }
        let sectors_read: u64 = parts[5].parse().unwrap_or(0);
        let sectors_write: u64 = parts[9].parse().unwrap_or(0);
        read += sectors_read;
        write += sectors_write;
    }
    (read.saturating_mul(512), write.saturating_mul(512))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netdev_skips_loopback() {
        let text = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets
    lo: 100 1 0 0 0 0 0 0 50 1 0 0 0 0 0 0
  eth0: 1000 1 0 0 0 0 0 0 250 1 0 0 0 0 0 0
";
        assert_eq!(parse_netdev(text), (1000, 250));
    }

    #[test]
    fn diskstats_whole_disks_only() {
        let text = "\
   8 0 sda 1 0 100 0 1 0 50 0 0 0 0 0 0 0 0
   8 1 sda1 1 0 80 0 1 0 40 0 0 0 0 0 0 0 0
 259 0 nvme0n1 1 0 10 0 1 0 4 0 0 0 0 0 0 0 0
";
        let (r, w) = parse_diskstats(text);
        assert_eq!(r, (100 + 10) * 512);
        assert_eq!(w, (50 + 4) * 512);
    }
}
