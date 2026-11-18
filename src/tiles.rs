use crate::model::{fmt_gib, fmt_mbps, fmt_mib, fmt_mibs, fmt_uptime, Hist, Snapshot};
use crate::paint::{Canvas, UiRect};
use crate::theme::Rgba;
use chrono::{DateTime, Local};

pub const TEX_W: u32 = 420;
pub const TEX_H: u32 = 270;
pub const WORLD_W: f32 = 1.78;
pub const WORLD_H: f32 = 1.16;
pub const GAP_X: f32 = 0.22;
pub const GAP_Y: f32 = 0.18;
pub const COLS: usize = 5;
pub const ROWS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Host,
    Clock,
    Cpu,
    Memory,
    Disk,
    Wifi,
    Network,
    Bluetooth,
    Audio,
    Brightness,
    DiskIo,
    Sensors,
    Processes,
    Battery,
    Gpu,
    Log,
    Power,
    Heatmap,
    NetChart,
    Shortcuts,
}

#[derive(Clone, Copy, Debug)]
pub struct TileSpec {
    pub index: &'static str,
    pub title: &'static str,
    pub kind: Kind,
}

pub fn catalog() -> &'static [TileSpec] {
    &CATALOG
}

const CATALOG: [TileSpec; 20] = [
    TileSpec { index: "00", title: "HOST", kind: Kind::Host },
    TileSpec { index: "01", title: "CLOCK", kind: Kind::Clock },
    TileSpec { index: "02", title: "CPU CORES", kind: Kind::Cpu },
    TileSpec { index: "03", title: "MEMORY", kind: Kind::Memory },
    TileSpec { index: "04", title: "DISK", kind: Kind::Disk },
    TileSpec { index: "05", title: "WI-FI", kind: Kind::Wifi },
    TileSpec { index: "06", title: "NETWORK", kind: Kind::Network },
    TileSpec { index: "07", title: "BLUETOOTH", kind: Kind::Bluetooth },
    TileSpec { index: "08", title: "AUDIO", kind: Kind::Audio },
    TileSpec { index: "09", title: "BRIGHTNESS", kind: Kind::Brightness },
    TileSpec { index: "10", title: "DISK I/O", kind: Kind::DiskIo },
    TileSpec { index: "11", title: "SENSORS", kind: Kind::Sensors },
    TileSpec { index: "12", title: "PROCESSES", kind: Kind::Processes },
    TileSpec { index: "13", title: "BATTERY", kind: Kind::Battery },
    TileSpec { index: "14", title: "GPU", kind: Kind::Gpu },
    TileSpec { index: "15", title: "SYSTEM LOG", kind: Kind::Log },
    TileSpec { index: "16", title: "POWER", kind: Kind::Power },
    TileSpec { index: "17", title: "HEATMAP", kind: Kind::Heatmap },
    TileSpec { index: "18", title: "NET HISTORY", kind: Kind::NetChart },
    TileSpec { index: "19", title: "SHORTCUTS", kind: Kind::Shortcuts },
];

pub fn network_index() -> usize {
    CATALOG.iter().position(|t| t.kind == Kind::Network).unwrap_or(6)
}

pub fn grid_pos(i: usize) -> glam::Vec3 {
    let col = (i % COLS) as f32;
    let row = (i / COLS) as f32;
    let x = (col - (COLS as f32 - 1.0) / 2.0) * (WORLD_W + GAP_X);
    let y = ((ROWS as f32 - 1.0) / 2.0 - row) * (WORLD_H + GAP_Y);
    let z = (((col as i32 + row as i32) % 3) as f32 - 1.0) * 0.045;
    glam::Vec3::new(x, y, z)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Wifi,
    Bluetooth,
    Mute,
    Dnd,
    Night,
    Profile,
    Volume,
    Brightness,
}

#[derive(Clone, Copy, Debug)]
pub struct Hotspot {
    pub rect: UiRect,
    pub action: Action,
}

pub struct DrawCtx<'a> {
    pub snap: &'a Snapshot,
    pub hist: &'a Hist,
    pub cpu_vis: &'a [f32],
    pub focus: f32,
    pub phase: f32,
    pub now: DateTime<Local>,
}

pub fn draw(canvas: &mut Canvas, spec: &TileSpec, ctx: &DrawCtx) -> Vec<Hotspot> {
    chrome(canvas, spec.index, spec.title, ctx.focus);
    match spec.kind {
        Kind::Host => host(canvas, ctx),
        Kind::Clock => clock(canvas, ctx),
        Kind::Cpu => cpu(canvas, ctx),
        Kind::Memory => memory(canvas, ctx),
        Kind::Disk => disk(canvas, ctx),
        Kind::Wifi => wifi(canvas, ctx),
        Kind::Network => network(canvas, ctx),
        Kind::Bluetooth => bluetooth(canvas, ctx),
        Kind::Audio => audio(canvas, ctx),
        Kind::Brightness => brightness(canvas, ctx),
        Kind::DiskIo => disk_io(canvas, ctx),
        Kind::Sensors => sensors(canvas, ctx),
        Kind::Processes => processes(canvas, ctx),
        Kind::Battery => battery(canvas, ctx),
        Kind::Gpu => gpu(canvas, ctx),
        Kind::Log => log(canvas, ctx),
        Kind::Power => power(canvas, ctx),
        Kind::Heatmap => heatmap(canvas, ctx),
        Kind::NetChart => net_chart(canvas, ctx),
        Kind::Shortcuts => shortcuts(canvas),
    }
}

fn chrome(c: &mut Canvas, index: &str, title: &str, focus: f32) {
    c.clear(c.theme.panel);
    let primary = c.theme.primary;
    let secondary = c.theme.secondary;
    c.fill_rect(0.0, 0.0, TEX_W as f32, 2.0, primary.alpha((50.0 + focus * 180.0) as u8));
    let border = c.theme.line.mix(primary, focus);
    c.stroke_rect(0.5, 0.5, TEX_W as f32 - 1.0, TEX_H as f32 - 1.0, border, 1.0);
    if focus > 0.08 {
        c.stroke_rect(
            3.0,
            3.0,
            TEX_W as f32 - 6.0,
            TEX_H as f32 - 6.0,
            secondary.alpha((focus * 220.0) as u8),
            1.1,
        );
    }
    c.brackets(7.0, 12.0, primary.alpha(210));
    c.text(16.0, 10.0, 11.0, secondary, index);
    c.text(48.0, 9.0, 13.0, c.theme.text, title);
    c.dots(16.0, 30.0, TEX_W as f32 - 32.0, c.theme.dim.alpha(170));
}

fn badge(c: &mut Canvas, text: &str) {
    c.text_right(TEX_W as f32 - 14.0, TEX_H as f32 - 22.0, 10.0, c.theme.warn, text);
}

fn host(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    let s = ctx.snap;
    arch_mark(c, 58.0, 118.0, 78.0, c.theme.primary);
    c.text(120.0, 48.0, 22.0, c.theme.text, &s.hostname);
    c.text(120.0, 80.0, 12.0, c.theme.primary, &s.os_name);
    c.text_fit(120.0, 100.0, 11.0, c.theme.dim, &format!("KERN {}", s.kernel), 270.0);
    c.text(120.0, 120.0, 11.0, c.theme.dim, &s.arch);
    c.text(120.0, 150.0, 11.0, c.theme.dim, "UPTIME");
    c.text(120.0, 168.0, 16.0, c.theme.secondary, &fmt_uptime(s.uptime_secs));
    c.text(16.0, 214.0, 10.0, c.theme.dim, "LOAD  1   5   15");
    for (i, v) in s.load.iter().enumerate() {
        let x = 16.0 + i as f32 * 120.0;
        c.vbar(x, 232.0, 70.0, 22.0, (v / 4.0).clamp(0.0, 1.0), c.theme.primary);
        c.text(x + 74.0, 234.0, 11.0, c.theme.text, &format!("{v:.2}"));
    }
    Vec::new()
}

fn arch_mark(c: &mut Canvas, cx: f32, cy: f32, s: f32, color: Rgba) {
    let h = s;
    let w = s * 0.92;
    c.poly_stroke(
        &[
            (cx, cy - h * 0.48),
            (cx - w * 0.5, cy + h * 0.46),
            (cx + w * 0.5, cy + h * 0.46),
            (cx, cy - h * 0.48),
        ],
        color,
        1.6,
    );
    c.poly_stroke(
        &[
            (cx, cy - h * 0.22),
            (cx - w * 0.28, cy + h * 0.30),
            (cx + w * 0.28, cy + h * 0.30),
            (cx, cy - h * 0.22),
        ],
        color.alpha(180),
        1.1,
    );
}

fn clock(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    let now = ctx.now;
    let text = now.format("%H:%M:%S").to_string();
    c.seven(28.0, 58.0, 34.0, 78.0, &text, c.theme.primary, now.timestamp() % 2 == 0);
    c.text(28.0, 160.0, 16.0, c.theme.text, &now.format("%A").to_string().to_uppercase());
    c.text(28.0, 184.0, 13.0, c.theme.dim, &now.format("%d %b %Y").to_string().to_uppercase());
    let off = now.offset().local_minus_utc();
    let sign = if off >= 0 { '+' } else { '-' };
    let abs = off.abs();
    c.text(
        28.0,
        210.0,
        12.0,
        c.theme.secondary,
        &format!("UTC{sign}{:02}:{:02}", abs / 3600, (abs % 3600) / 60),
    );
    c.ring(340.0, 150.0, 42.0, 4.0, (now.timestamp() % 60) as f32 / 60.0, c.theme.secondary);
    c.text(318.0, 142.0, 11.0, c.theme.dim, "SEC");
    Vec::new()
}

fn cpu(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    let cores = if ctx.cpu_vis.is_empty() { &ctx.snap.cpu } else { ctx.cpu_vis };
    let n = cores.len().clamp(1, 16);
    let cols = if n > 8 { 8 } else { n };
    let rows = (n + cols - 1) / cols;
    let area_w = 250.0;
    let area_h = 150.0;
    let gap = 6.0;
    let bw = (area_w - gap * (cols as f32 - 1.0)) / cols as f32;
    let bh = (area_h - gap * (rows as f32 - 1.0)) / rows as f32;
    for i in 0..n {
        let col = (i % cols) as f32;
        let row = (i / cols) as f32;
        let x = 18.0 + col * (bw + gap);
        let y = 48.0 + row * (bh + gap);
        let v = cores.get(i).copied().unwrap_or(0.0);
        c.vbar(x, y, bw, bh, v, if v > 0.85 { c.theme.warn } else { c.theme.primary });
    }
    let pct = ctx.snap.cpu_avg * 100.0;
    c.text_right(TEX_W as f32 - 16.0, 48.0, 28.0, c.theme.text, &format!("{pct:.0}"));
    c.text_right(TEX_W as f32 - 16.0, 82.0, 11.0, c.theme.dim, "% LOAD");
    c.text_right(
        TEX_W as f32 - 16.0,
        104.0,
        12.0,
        c.theme.secondary,
        &format!("{} MHz", ctx.snap.cpu_freq_mhz),
    );
    c.text(280.0, 140.0, 10.0, c.theme.dim, &format!("{} CORES", ctx.snap.cpu.len()));
    c.spark_owned(280.0, 168.0, 120.0, 70.0, hist_slice(&ctx.hist.cpu), c.theme.secondary);
    Vec::new()
}

fn memory(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    let s = ctx.snap;
    let frac = if s.mem_total == 0 {
        0.0
    } else {
        s.mem_used as f32 / s.mem_total as f32
    };
    c.text(18.0, 48.0, 26.0, c.theme.text, &format!("{} / {}", fmt_gib(s.mem_used), fmt_gib(s.mem_total)));
    c.text(18.0, 82.0, 12.0, c.theme.dim, "GiB USED / TOTAL");
    c.hbar(18.0, 112.0, TEX_W as f32 - 36.0, 16.0, frac, c.theme.primary);
    c.text(18.0, 136.0, 12.0, c.theme.secondary, &format!("AVAIL {} GiB", fmt_gib(s.mem_avail)));
    c.text(18.0, 168.0, 10.0, c.theme.dim, "USAGE");
    c.spark_owned(18.0, 186.0, TEX_W as f32 - 36.0, 60.0, hist_slice(&ctx.hist.mem), c.theme.primary);
    Vec::new()
}

fn disk(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    if ctx.snap.mounts.is_empty() {
        c.text(18.0, 70.0, 14.0, c.theme.dim, "NO MOUNTS");
        badge(c, "UNAVAILABLE");
        return Vec::new();
    }
    let root = ctx.snap.mounts.iter().find(|m| m.name == "/").unwrap_or(&ctx.snap.mounts[0]);
    let frac = if root.total == 0 { 0.0 } else { root.used as f32 / root.total as f32 };
    c.ring(78.0, 130.0, 48.0, 7.0, frac, c.theme.primary);
    c.text(48.0, 118.0, 16.0, c.theme.text, &format!("{:.0}%", frac * 100.0));
    let mut y = 46.0;
    for m in ctx.snap.mounts.iter().take(4) {
        let f = if m.total == 0 { 0.0 } else { m.used as f32 / m.total as f32 };
        c.text_fit(150.0, y, 12.0, c.theme.text, &m.name, 140.0);
        c.text_right(
            TEX_W as f32 - 16.0,
            y,
            11.0,
            c.theme.dim,
            &format!("{} / {}", fmt_gib(m.used), fmt_gib(m.total)),
        );
        c.hbar(150.0, y + 18.0, TEX_W as f32 - 166.0, 8.0, f, c.theme.secondary);
        y += 48.0;
    }
    Vec::new()
}

fn wifi(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    let w = &ctx.snap.wifi;
    let hit = c.toggle(18.0, 46.0, w.enabled, w.available);
    c.text(76.0, 46.0, 12.0, c.theme.dim, if w.enabled { "ON" } else { "OFF" });
    c.text_fit(18.0, 80.0, 18.0, c.theme.text, &w.headline, 380.0);
    c.text_fit(18.0, 108.0, 12.0, c.theme.dim, &w.detail, 380.0);
    let bars = (w.signal * 5.0).round() as i32;
    for i in 0..5 {
        let h = 10.0 + i as f32 * 8.0;
        let col = if i < bars { c.theme.primary } else { c.theme.line };
        c.fill_rect(18.0 + i as f32 * 16.0, 200.0 - h, 10.0, h, col);
    }
    c.text(110.0, 168.0, 12.0, c.theme.secondary, &format!("SIG {:.0}%", w.signal * 100.0));
    c.spark_owned(110.0, 190.0, 280.0, 50.0, hist_slice(&ctx.hist.signal), c.theme.primary);
    if !w.available {
        badge(c, "UNAVAILABLE");
    }
    vec![Hotspot { rect: hit, action: Action::Wifi }]
}

fn network(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    let nodes = [(90.0, 110.0), (180.0, 78.0), (250.0, 140.0), (150.0, 170.0), (300.0, 100.0)];
    let links = [(0, 1), (1, 2), (0, 3), (3, 2), (2, 4), (1, 4)];
    for (a, b) in links {
        c.line(nodes[a].0, nodes[a].1, nodes[b].0, nodes[b].1, c.theme.line, 1.0);
    }
    for (i, (x, y)) in nodes.iter().enumerate() {
        let pulse = if i == 2 { 0.5 + 0.5 * (ctx.phase * 3.0).sin() } else { 0.3 };
        let col = if i == 2 { c.theme.secondary } else { c.theme.primary.alpha((140.0 + pulse * 100.0) as u8) };
        c.fill_rect(x - 5.0, y - 5.0, 10.0, 10.0, col);
        c.stroke_rect(x - 8.0, y - 8.0, 16.0, 16.0, col, 1.0);
    }
    c.text(18.0, 200.0, 10.0, c.theme.dim, "RX");
    c.text(18.0, 216.0, 18.0, c.theme.primary, &fmt_mbps(ctx.snap.net_rx_bps));
    c.text(70.0, 224.0, 10.0, c.theme.dim, "Mbps");
    c.text(180.0, 200.0, 10.0, c.theme.dim, "TX");
    c.text(180.0, 216.0, 18.0, c.theme.secondary, &fmt_mbps(ctx.snap.net_tx_bps));
    c.text(232.0, 224.0, 10.0, c.theme.dim, "Mbps");
    c.spark_owned(280.0, 200.0, 120.0, 48.0, hist_slice(&ctx.hist.rx), c.theme.primary);
    Vec::new()
}

fn bluetooth(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    let b = &ctx.snap.bluetooth;
    let hit = c.toggle(18.0, 46.0, b.enabled, b.available);
    c.text(76.0, 46.0, 12.0, c.theme.dim, if b.enabled { "ON" } else { "OFF" });
    c.text_fit(18.0, 78.0, 16.0, c.theme.text, &b.headline, 380.0);
    c.text_fit(18.0, 102.0, 11.0, c.theme.dim, &b.detail, 380.0);
    if b.devices.is_empty() {
        c.text(18.0, 140.0, 12.0, c.theme.dim, "NO DEVICES");
    } else {
        for (i, dev) in b.devices.iter().take(5).enumerate() {
            let y = 132.0 + i as f32 * 22.0;
            c.fill_rect(18.0, y + 4.0, 6.0, 6.0, if dev.contains('*') { c.theme.primary } else { c.theme.dim });
            c.text_fit(32.0, y, 12.0, c.theme.text, dev.trim_end_matches(" *"), 360.0);
        }
    }
    if !b.available {
        badge(c, "UNAVAILABLE");
    }
    vec![Hotspot { rect: hit, action: Action::Bluetooth }]
}

fn audio(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    let v = &ctx.snap.volume;
    let shown = if v.muted { 0.0 } else { v.value.min(1.0) };
    let mut wave = Vec::with_capacity(64);
    for i in 0..64 {
        let p = ctx.phase * 6.0 + i as f32 * 0.35;
        let amp = shown * (0.35 + 0.65 * (p * 0.5).sin().abs());
        wave.push((p.sin() * 0.5 + 0.5) * amp);
    }
    c.spark(16.0, 44.0, TEX_W as f32 - 32.0, 90.0, &wave, c.theme.secondary);
    c.text(18.0, 150.0, 28.0, c.theme.text, &format!("{:.0}", shown * 100.0));
    c.text(90.0, 164.0, 12.0, c.theme.dim, if v.muted { "MUTED" } else { "PERCENT" });
    let slider = c.slider(18.0, 210.0, 280.0, shown, c.theme.primary);
    let mute = button(c, 314.0, 200.0, 86.0, 26.0, if v.muted { "UNMUTE" } else { "MUTE" }, v.muted);
    if !v.available {
        badge(c, "UNAVAILABLE");
        c.text_fit(18.0, 236.0, 11.0, c.theme.warn, &v.detail, 280.0);
    } else {
        c.text(18.0, 236.0, 11.0, c.theme.dim, &v.detail);
    }
    vec![
        Hotspot { rect: slider, action: Action::Volume },
        Hotspot { rect: mute, action: Action::Mute },
    ]
}

fn brightness(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    let b = &ctx.snap.brightness;
    c.text(18.0, 50.0, 36.0, c.theme.text, &format!("{:.0}", b.value * 100.0));
    c.text(120.0, 70.0, 14.0, c.theme.dim, "%");
    c.ring(330.0, 100.0, 46.0, 6.0, b.value, c.theme.primary);
    c.text(300.0, 92.0, 11.0, c.theme.dim, "NITS");
    let slider = c.slider(18.0, 160.0, TEX_W as f32 - 36.0, b.value, c.theme.secondary);
    c.text_fit(18.0, 190.0, 12.0, c.theme.dim, &b.detail, 380.0);
    for i in 0..12 {
        let x = 18.0 + i as f32 * 16.0;
        c.line(x, 220.0, x + 8.0, 232.0, c.theme.primary.alpha(90 + i as u8 * 10), 1.0);
    }
    if !b.available {
        badge(c, "UNAVAILABLE");
    }
    vec![Hotspot { rect: slider, action: Action::Brightness }]
}

fn disk_io(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    c.text(18.0, 46.0, 11.0, c.theme.dim, "READ MiB/s");
    c.text(18.0, 64.0, 22.0, c.theme.primary, &fmt_mibs(ctx.snap.disk_r_bps));
    c.spark_owned(18.0, 100.0, TEX_W as f32 - 36.0, 50.0, hist_slice(&ctx.hist.dr), c.theme.primary);
    c.text(18.0, 160.0, 11.0, c.theme.dim, "WRITE MiB/s");
    c.text(18.0, 178.0, 22.0, c.theme.secondary, &fmt_mibs(ctx.snap.disk_w_bps));
    c.spark_owned(18.0, 210.0, TEX_W as f32 - 36.0, 42.0, hist_slice(&ctx.hist.dw), c.theme.secondary);
    Vec::new()
}

fn sensors(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    if ctx.snap.temps.is_empty() {
        c.text(18.0, 70.0, 14.0, c.theme.dim, "NO THERMAL ZONES");
        c.text(18.0, 96.0, 12.0, c.theme.dim, "lm_sensors / sysfs empty");
        badge(c, "UNAVAILABLE");
        return Vec::new();
    }
    for (i, t) in ctx.snap.temps.iter().take(6).enumerate() {
        let y = 44.0 + i as f32 * 34.0;
        c.text_fit(18.0, y, 12.0, c.theme.text, &t.label, 230.0);
        c.text_right(TEX_W as f32 - 16.0, y, 14.0, c.theme.primary, &format!("{:.0} C", t.celsius));
        c.hbar(18.0, y + 18.0, TEX_W as f32 - 36.0, 6.0, (t.celsius / 100.0).clamp(0.0, 1.0), c.theme.secondary);
    }
    Vec::new()
}

fn processes(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    c.text(18.0, 40.0, 10.0, c.theme.dim, "NAME");
    c.text_right(330.0, 40.0, 10.0, c.theme.dim, "CPU");
    c.text_right(TEX_W as f32 - 14.0, 40.0, 10.0, c.theme.dim, "RSS");
    if ctx.snap.processes.is_empty() {
        c.text(18.0, 70.0, 12.0, c.theme.dim, "NO SAMPLES YET");
        return Vec::new();
    }
    for (i, p) in ctx.snap.processes.iter().take(6).enumerate() {
        let y = 58.0 + i as f32 * 30.0;
        c.text_fit(18.0, y, 13.0, c.theme.text, &p.name, 200.0);
        c.text_right(330.0, y, 12.0, c.theme.primary, &format!("{:.1}", p.cpu));
        c.text_right(TEX_W as f32 - 14.0, y, 12.0, c.theme.dim, &fmt_mib(p.mem));
        c.dots(18.0, y + 18.0, TEX_W as f32 - 36.0, c.theme.line.alpha(120));
    }
    Vec::new()
}

fn battery(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    let b = &ctx.snap.battery;
    if !b.present {
        c.text(18.0, 80.0, 22.0, c.theme.text, "AC POWER");
        c.text(18.0, 116.0, 13.0, c.theme.dim, "NO BATTERY DETECTED");
        c.ring(300.0, 150.0, 50.0, 6.0, 1.0, c.theme.primary.alpha(160));
        return Vec::new();
    }
    let frac = (b.percent / 100.0).clamp(0.0, 1.0);
    c.ring(110.0, 140.0, 58.0, 8.0, frac, if frac < 0.15 { c.theme.warn } else { c.theme.primary });
    c.text(62.0, 126.0, 22.0, c.theme.text, &format!("{:.0}", b.percent));
    c.text(200.0, 90.0, 18.0, c.theme.secondary, &b.status.to_uppercase());
    c.hbar(200.0, 130.0, 190.0, 14.0, frac, c.theme.primary);
    c.text(200.0, 160.0, 12.0, c.theme.dim, "CHARGE");
    Vec::new()
}

fn gpu(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    let g = &ctx.snap.gpu;
    if !g.present {
        c.text(18.0, 70.0, 16.0, c.theme.dim, "GPU UNAVAILABLE");
        c.text_fit(18.0, 98.0, 12.0, c.theme.dim, &g.name, 380.0);
        c.spark(18.0, 140.0, TEX_W as f32 - 36.0, 70.0, &[0.0, 0.0, 0.0, 0.0], c.theme.line);
        badge(c, "UNAVAILABLE");
        return Vec::new();
    }
    c.text_fit(18.0, 46.0, 16.0, c.theme.text, &g.name, 380.0);
    c.text(18.0, 80.0, 32.0, c.theme.primary, &format!("{:.0}%", g.util * 100.0));
    c.hbar(18.0, 130.0, TEX_W as f32 - 36.0, 16.0, g.util, c.theme.primary);
    if let Some(t) = g.temp_c {
        c.text(18.0, 164.0, 14.0, c.theme.secondary, &format!("TEMP {t:.0} C"));
    }
    c.vbar(18.0, 200.0, 24.0, 48.0, g.util, c.theme.secondary);
    Vec::new()
}

fn log(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    for (i, line) in ctx.snap.log_lines.iter().take(7).enumerate() {
        let y = 42.0 + i as f32 * 28.0;
        let col = if line.to_ascii_lowercase().contains("fail") || line.to_ascii_lowercase().contains("err") {
            c.theme.warn
        } else if i % 2 == 0 {
            c.theme.primary
        } else {
            c.theme.text
        };
        c.text(16.0, y, 10.0, c.theme.dim, &format!("{:02}", i + 1));
        c.text_fit(40.0, y, 11.0, col, line, 360.0);
    }
    Vec::new()
}

fn power(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    let label = ctx.snap.power_profile.to_uppercase();
    let profile = button(c, 18.0, 48.0, TEX_W as f32 - 36.0, 36.0, &label, true);
    c.text(18.0, 96.0, 11.0, c.theme.dim, if ctx.snap.power_available { "PROFILE  CLICK TO CYCLE" } else { "POWER PROFILES DAEMON" });
    let dnd = c.toggle(18.0, 130.0, ctx.snap.dnd.enabled, ctx.snap.dnd.available);
    c.text(78.0, 130.0, 14.0, c.theme.text, "DO NOT DISTURB");
    c.text_fit(18.0, 154.0, 11.0, c.theme.dim, &ctx.snap.dnd.detail, 380.0);
    let night = c.toggle(18.0, 186.0, ctx.snap.night.enabled, ctx.snap.night.available);
    c.text(78.0, 186.0, 14.0, c.theme.text, "NIGHT LIGHT");
    c.text_fit(18.0, 210.0, 11.0, c.theme.dim, &ctx.snap.night.detail, 380.0);
    if !ctx.snap.power_available {
        badge(c, "PARTIAL");
    }
    vec![
        Hotspot { rect: profile, action: Action::Profile },
        Hotspot { rect: dnd, action: Action::Dnd },
        Hotspot { rect: night, action: Action::Night },
    ]
}

fn heatmap(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    let rows: Vec<&[f32]> = ctx.hist.cores.iter().map(|v| v.as_slice()).collect();
    if rows.is_empty() {
        c.text(18.0, 80.0, 13.0, c.theme.dim, "COLLECTING CORES");
        return Vec::new();
    }
    let cols = rows.first().map(|r| r.len()).unwrap_or(1).clamp(1, 16);
    let n = rows.len();
    let cell_w = (TEX_W as f32 - 36.0) / n as f32;
    let cell_h = 160.0 / cols as f32;
    for (x, row) in rows.iter().enumerate() {
        for core in 0..cols {
            let v = row.get(core).copied().unwrap_or(0.0).clamp(0.0, 1.0);
            let col = c.theme.panel2.mix(c.theme.primary, v).mix(c.theme.secondary, (v - 0.65).max(0.0));
            c.fill_rect(18.0 + x as f32 * cell_w, 48.0 + core as f32 * cell_h, cell_w - 1.0, cell_h - 1.0, col);
        }
    }
    c.text(18.0, 220.0, 11.0, c.theme.dim, "TIME →    CORES ↓");
    Vec::new()
}

fn net_chart(c: &mut Canvas, ctx: &DrawCtx) -> Vec<Hotspot> {
    c.text(18.0, 42.0, 11.0, c.theme.primary, "DOWN");
    c.text(80.0, 42.0, 11.0, c.theme.secondary, "UP");
    c.spark_owned(18.0, 64.0, TEX_W as f32 - 36.0, 80.0, hist_slice(&ctx.hist.rx), c.theme.primary);
    c.spark_owned(18.0, 156.0, TEX_W as f32 - 36.0, 80.0, hist_slice(&ctx.hist.tx), c.theme.secondary);
    Vec::new()
}

fn shortcuts(c: &mut Canvas) -> Vec<Hotspot> {
    let rows = [
        ("LMB DRAG", "ORBIT ALL AXES"),
        ("WHEEL", "ZOOM"),
        ("RMB DRAG", "PAN"),
        ("R", "RESET VIEW"),
        ("1-5", "THEME PRESET"),
        ("6", "CUSTOM HUE"),
        ("HOVER", "FOCUS + BLUR"),
        ("ESC", "QUIT"),
    ];
    for (i, (k, v)) in rows.iter().enumerate() {
        let y = 42.0 + i as f32 * 26.0;
        c.text(18.0, y, 12.0, c.theme.primary, k);
        c.text(150.0, y, 12.0, c.theme.text, v);
    }
    Vec::new()
}

fn button(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, label: &str, active: bool) -> UiRect {
    c.fill_rect(x, y, w, h, c.theme.panel2);
    c.stroke_rect(x, y, w, h, if active { c.theme.primary } else { c.theme.line }, 1.2);
    c.text_fit(x + 10.0, y + (h - 14.0) * 0.5, 13.0, c.theme.text, label, w - 20.0);
    UiRect { x, y, w, h }
}

fn hist_slice(q: &std::collections::VecDeque<f32>) -> Vec<f32> {
    if q.is_empty() {
        vec![0.0, 0.0]
    } else {
        q.iter().copied().collect()
    }
}
