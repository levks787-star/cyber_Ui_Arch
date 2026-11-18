mod backends;
mod camera;
mod config;
mod gpu;
mod hud;
mod model;
mod paint;
mod system;
mod theme;
mod tiles;

use camera::{damp, Orbit};
use clap::Parser;
use config::Config;
use glam::{Mat4, Vec3};
use gpu::{Gpu, TileDraw};
use hud::{Hud, HudAction};
use model::{Command, Hist, Snapshot};
use paint::Canvas;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use system::SystemHandle;
use theme::{colors, ThemeColors, ThemeId};
use tiles::{
    catalog, grid_pos, network_index, Action, DrawCtx, Hotspot, Kind, COLS, TEX_H, TEX_W, WORLD_H,
    WORLD_W,
};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

#[derive(Parser, Debug)]
#[command(name = "arch-control-center", version, about = "3D control center for Arch Linux")]
struct Args {
    /// Simulated telemetry. Toggles stay inside the demo and do not touch the system.
    #[arg(long)]
    demo: bool,
    /// Slow orbit, focus the network tile, then switch themes.
    #[arg(long)]
    showcase: bool,
    /// Save showcase PNGs into this directory and exit.
    #[arg(long)]
    shots: Option<PathBuf>,
    #[arg(long, default_value_t = 1440u32)]
    width: u32,
    #[arg(long, default_value_t = 900u32)]
    height: u32,
}

struct App {
    args: Args,
    window: Option<std::sync::Arc<Window>>,
    gpu: Option<Gpu>,
    sys: SystemHandle,
    config: Config,
    theme_id: ThemeId,
    hue: f32,
    theme: ThemeColors,
    theme_target: ThemeColors,
    camera: Orbit,
    cursor: [f32; 2],
    cursor_valid: bool,
    drag: Drag,
    hover: Vec<f32>,
    hover_idx: Option<usize>,
    hotspots: Vec<Vec<Hotspot>>,
    hud_hits: Vec<hud::HudHit>,
    force_hover: Option<usize>,
    hist: Hist,
    cpu_vis: Vec<f32>,
    tile_canvas: Canvas,
    hud_canvas: Canvas,
    pixels: Vec<Vec<u8>>,
    hud_pixels: Vec<u8>,
    overrides: Overrides,
    slider: f32,
    phase: f32,
    time: f32,
    last: Instant,
    showcase: bool,
    shots: Option<PathBuf>,
    shot_done: [bool; 3],
    switched_acid: bool,
    user_control: bool,
    frames: u32,
    fps_at: Instant,
    size: (u32, u32),
}

#[derive(Clone, Copy)]
enum Drag {
    None,
    Orbit { last: (f32, f32) },
    Pan { last: (f32, f32) },
    Slider { action: Action, tile: usize },
    Hue,
}

struct Overrides {
    wifi: Option<(bool, Instant)>,
    bt: Option<(bool, Instant)>,
    volume: Option<(f32, Instant)>,
    mute: Option<(bool, Instant)>,
    bright: Option<(f32, Instant)>,
    dnd: Option<(bool, Instant)>,
    night: Option<(bool, Instant)>,
}

impl Default for Overrides {
    fn default() -> Self {
        Self {
            wifi: None,
            bt: None,
            volume: None,
            mute: None,
            bright: None,
            dnd: None,
            night: None,
        }
    }
}

impl App {
    fn new(args: Args) -> Self {
        let mut showcase = args.showcase;
        let shots = args.shots.clone();
        if shots.is_some() {
            showcase = true;
        }
        let config = Config::load();
        let theme_id = config.theme;
        let hue = config.custom_hue;
        let theme = colors(theme_id, hue);
        let n = catalog().len();
        let hist = if args.demo || showcase { Hist::seed_demo() } else { Hist::new() };
        Self {
            sys: SystemHandle::start(args.demo || showcase),
            args,
            window: None,
            gpu: None,
            config,
            theme_id,
            hue,
            theme,
            theme_target: theme,
            camera: Orbit::new(),
            cursor: [0.0, 0.0],
            cursor_valid: false,
            drag: Drag::None,
            hover: vec![0.0; n],
            hover_idx: None,
            hotspots: vec![Vec::new(); n],
            hud_hits: Vec::new(),
            force_hover: None,
            hist,
            cpu_vis: Vec::new(),
            tile_canvas: Canvas::new(TEX_W, TEX_H, theme),
            hud_canvas: Canvas::new(8, 8, theme),
            pixels: vec![vec![0; (TEX_W * TEX_H * 4) as usize]; n],
            hud_pixels: Vec::new(),
            overrides: Overrides::default(),
            slider: 0.0,
            phase: 0.0,
            time: 0.0,
            last: Instant::now(),
            showcase,
            shots,
            shot_done: [false; 3],
            switched_acid: false,
            user_control: false,
            frames: 0,
            fps_at: Instant::now(),
            size: (1, 1),
        }
    }

    fn frame(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().clamp(0.0, 0.25);
        self.last = now;
        self.time += dt;
        self.phase += dt;
        self.frames += 1;
        if self.fps_at.elapsed() > Duration::from_secs(2) {
            log::info!(
                "fps {:.1}",
                self.frames as f32 / self.fps_at.elapsed().as_secs_f32()
            );
            self.frames = 0;
            self.fps_at = Instant::now();
        }

        let mut snap = self.sys.snapshot();
        self.apply_overrides(&mut snap);
        self.push_hist(&snap);
        if self.cpu_vis.len() != snap.cpu.len() {
            self.cpu_vis = snap.cpu.clone();
        }
        for (vis, target) in self.cpu_vis.iter_mut().zip(snap.cpu.iter()) {
            *vis = damp(*vis, *target, 8.0, dt);
        }

        if self.showcase && !self.user_control {
            self.camera.showcase_spin(dt);
            self.force_hover = if (3.0..9.6).contains(&self.time) {
                Some(network_index())
            } else {
                None
            };
            if self.time > 6.5 && !self.switched_acid {
                self.set_theme(ThemeId::Acid, true);
                self.switched_acid = true;
            }
        }
        self.camera.tick(dt);
        self.theme = self.theme.lerp(self.theme_target, 1.0 - (-6.5 * dt).exp());

        self.update_hover();
        let max_h = self.hover.iter().copied().fold(0.0_f32, f32::max);
        for i in 0..self.hover.len() {
            let target = if self.hovered() == Some(i) { 1.0 } else { 0.0 };
            self.hover[i] = damp(self.hover[i], target, 10.0, dt);
        }

        self.tile_canvas.theme = self.theme;
        let drawn = chrono::Local::now();
        for (i, spec) in catalog().iter().enumerate() {
            let ctx = DrawCtx {
                snap: &snap,
                hist: &self.hist,
                cpu_vis: &self.cpu_vis,
                focus: self.hover[i],
                phase: self.phase,
                now: drawn,
            };
            self.hotspots[i] = tiles::draw(&mut self.tile_canvas, spec, &ctx);
            self.tile_canvas.copy_rgba(&mut self.pixels[i]);
        }

        if let Some(window) = &self.window {
            let size = window.inner_size();
            self.size = (size.width.max(1), size.height.max(1));
            self.hud_canvas.resize(self.size.0, self.size.1);
            self.hud_canvas.theme = self.theme;
            let hud = Hud::draw(
                &mut self.hud_canvas,
                self.theme_id,
                self.hue,
                snap.demo,
                max_h > 0.45,
            );
            self.hud_hits = hud.hits;
            self.hud_canvas.copy_rgba(&mut self.hud_pixels);
            let icon = if self.pointer() { CursorIcon::Pointer } else { CursorIcon::Default };
            window.set_cursor(icon);
        }

        let capture = self.shot_due();
        if let Some(gpu) = &mut self.gpu {
            gpu.resize(self.size.0, self.size.1);
            let aspect = self.size.0 as f32 / self.size.1 as f32;
            let vp = self.camera.view_proj(aspect);
            let mut draws = Vec::with_capacity(catalog().len());
            for i in 0..catalog().len() {
                let base = grid_pos(i);
                let hov = self.hover[i];
                let scale = 1.0 + hov * 0.34;
                let pos = Vec3::new(base.x, base.y, base.z + hov * 1.35);
                let model = Mat4::from_scale_rotation_translation(
                    Vec3::new(WORLD_W * scale, WORLD_H * scale, 1.0),
                    glam::Quat::IDENTITY,
                    pos,
                );
                let dim = max_h * 0.62;
                let bright = (1.0 - dim + hov * dim).clamp(0.38, 1.2);
                let blur = (max_h * 12.0 * (1.0 - hov)).clamp(0.0, 16.0);
                draws.push(TileDraw {
                    model: model.to_cols_array_2d(),
                    params: [blur, bright, hov, 0.0],
                    pixels: &self.pixels[i],
                });
            }
            let bg = self.theme.bg.to_f32();
            let mut grid = self.theme.primary.to_f32();
            grid[3] = 1.0;
            // The draw list borrows pixels, so render before the next mutate.
            let result = {
                let gpu = gpu;
                gpu.render(
                    vp.to_cols_array_2d(),
                    [bg[0], bg[1], bg[2]],
                    grid,
                    &draws,
                    &self.hud_pixels,
                    capture,
                )
            };
            match result {
                Ok(Some(px)) => self.save_shot(&px),
                Ok(None) => {}
                Err(err) => {
                    log::error!("render: {err}");
                    el.exit();
                }
            }
        }

        if self.shots.is_some() && self.shot_done.iter().all(|d| *d) {
            log::info!("shots written");
            el.exit();
        }

        let budget = Duration::from_millis(16);
        let spent = now.elapsed();
        if spent < budget {
            std::thread::sleep(budget - spent);
        }
        let _ = (COLS, Kind::Host);
    }

    fn hovered(&self) -> Option<usize> {
        if let Some(i) = self.force_hover {
            return Some(i);
        }
        if matches!(self.drag, Drag::Orbit { .. } | Drag::Pan { .. }) {
            return None;
        }
        self.hover_idx
    }

    fn update_hover(&mut self) {
        if self.hud_hot() {
            self.hover_idx = None;
            return;
        }
        if matches!(self.drag, Drag::Orbit { .. } | Drag::Pan { .. }) {
            self.hover_idx = None;
            return;
        }
        if !self.cursor_valid || self.size.0 == 0 {
            return;
        }
        let (nx, ny) = self.ndc();
        let aspect = self.size.0 as f32 / self.size.1 as f32;
        let (eye, dir) = self.camera.ray(nx, ny, aspect);
        self.hover_idx = pick(&self.hover, eye, dir).map(|(i, _, _)| i);
    }

    fn ndc(&self) -> (f32, f32) {
        let w = self.size.0.max(1) as f32;
        let h = self.size.1.max(1) as f32;
        ((self.cursor[0] / w) * 2.0 - 1.0, 1.0 - (self.cursor[1] / h) * 2.0)
    }

    fn hud_hot(&self) -> bool {
        self.hud_hits.iter().any(|h| h.rect.contains(self.cursor[0], self.cursor[1]))
    }

    fn pointer(&self) -> bool {
        self.hud_hot() || self.hover_idx.and_then(|i| self.hotspot_at(i)).is_some() || self.hovered().is_some()
    }

    fn hotspot_at(&self, tile: usize) -> Option<Hotspot> {
        let (nx, ny) = self.ndc();
        let aspect = self.size.0 as f32 / self.size.1.max(1) as f32;
        let (eye, dir) = self.camera.ray(nx, ny, aspect);
        let (_, u, v) = pick(&self.hover, eye, dir).filter(|(i, _, _)| *i == tile)?;
        let x = u * TEX_W as f32;
        let y = v * TEX_H as f32;
        self.hotspots.get(tile)?.iter().copied().find(|h| h.rect.contains(x, y))
    }

    fn on_mouse(&mut self, button: MouseButton, state: ElementState) {
        if state == ElementState::Released {
            if let Drag::Hue = self.drag {
                self.config.theme = ThemeId::Custom;
                self.config.custom_hue = self.hue;
                self.config.save();
            }
            if let Drag::Slider { action, .. } = self.drag {
                self.send_slider(action, self.slider);
            }
            self.drag = Drag::None;
            return;
        }
        if button != MouseButton::Left && button != MouseButton::Right && button != MouseButton::Middle {
            return;
        }
        self.user_control = true;
        let (nx, ny) = self.ndc();
        if button == MouseButton::Right || button == MouseButton::Middle {
            self.drag = Drag::Pan { last: (self.cursor[0], self.cursor[1]) };
            return;
        }
        if let Some(hit) = self.hud_hits.iter().copied().find(|h| h.rect.contains(self.cursor[0], self.cursor[1])) {
            match hit.action {
                HudAction::Theme(id) => self.set_theme(id, true),
                HudAction::Reset => self.camera.request_reset(),
                HudAction::Hue => {
                    self.hue_from_cursor(hit.rect);
                    self.drag = Drag::Hue;
                }
            }
            return;
        }
        if let Some((tile, _, _)) = {
            let aspect = self.size.0 as f32 / self.size.1.max(1) as f32;
            let (eye, dir) = self.camera.ray(nx, ny, aspect);
            pick(&self.hover, eye, dir)
        } {
            if let Some(hot) = self.hotspot_at(tile) {
                match hot.action {
                    Action::Volume | Action::Brightness => {
                        self.slider = hot.rect.value_x(self.tile_px(tile).0);
                        self.drag = Drag::Slider { action: hot.action, tile };
                        self.send_slider(hot.action, self.slider);
                    }
                    other => self.fire(other),
                }
                return;
            }
        }
        self.drag = Drag::Orbit { last: (nx, ny) };
    }

    fn on_cursor(&mut self) {
        match self.drag {
            Drag::Orbit { last } => {
                let (nx, ny) = self.ndc();
                let dt = 1.0 / 60.0;
                self.camera.apply_trackball(last, (nx, ny), dt);
                self.drag = Drag::Orbit { last: (nx, ny) };
            }
            Drag::Pan { last } => {
                let dx = self.cursor[0] - last.0;
                let dy = self.cursor[1] - last.1;
                self.camera.pan(dx, dy);
                self.drag = Drag::Pan { last: (self.cursor[0], self.cursor[1]) };
            }
            Drag::Slider { action, tile } => {
                if let Some(hot) = self.hotspots.get(tile).and_then(|list| list.iter().copied().find(|h| h.action == action)) {
                    let (x, _) = self.tile_px(tile);
                    self.slider = hot.rect.value_x(x);
                    self.send_slider(action, self.slider);
                }
            }
            Drag::Hue => {
                if let Some(hit) = self.hud_hits.iter().copied().find(|h| matches!(h.action, HudAction::Hue)) {
                    self.hue_from_cursor(hit.rect);
                }
            }
            Drag::None => {}
        }
    }

    fn tile_px(&self, tile: usize) -> (f32, f32) {
        let (nx, ny) = self.ndc();
        let aspect = self.size.0 as f32 / self.size.1.max(1) as f32;
        let (eye, dir) = self.camera.ray(nx, ny, aspect);
        if let Some((i, u, v)) = pick(&self.hover, eye, dir) {
            if i == tile {
                return (u * TEX_W as f32, v * TEX_H as f32);
            }
        }
        (0.0, 0.0)
    }

    fn hue_from_cursor(&mut self, rect: paint::UiRect) {
        self.hue = rect.value_x(self.cursor[0]) * 360.0;
        self.theme_id = ThemeId::Custom;
        self.theme_target = colors(ThemeId::Custom, self.hue);
        self.config.theme = ThemeId::Custom;
        self.config.custom_hue = self.hue;
    }

    fn on_key(&mut self, key: Key) {
        match key {
            Key::Named(NamedKey::Escape) => {}
            Key::Character(s) => {
                let s = s.as_str();
                if s.eq_ignore_ascii_case("r") {
                    self.camera.request_reset();
                }
                if let Some(id) = ThemeId::from_key(s) {
                    self.set_theme(id, true);
                }
            }
            _ => {}
        }
    }

    fn set_theme(&mut self, id: ThemeId, save: bool) {
        self.theme_id = id;
        self.theme_target = colors(id, self.hue);
        self.config.theme = id;
        self.config.custom_hue = self.hue;
        if save {
            self.config.save();
        }
    }

    fn fire(&mut self, action: Action) {
        let snap = self.sys.snapshot();
        let now = Instant::now();
        match action {
            Action::Wifi => {
                let cur = self.overrides.wifi.map(|(v, _)| v).unwrap_or(snap.wifi.enabled);
                let next = !cur;
                self.overrides.wifi = Some((next, now));
                self.sys.send(Command::Wifi(next));
            }
            Action::Bluetooth => {
                let cur = self.overrides.bt.map(|(v, _)| v).unwrap_or(snap.bluetooth.enabled);
                let next = !cur;
                self.overrides.bt = Some((next, now));
                self.sys.send(Command::Bluetooth(next));
            }
            Action::Mute => {
                let cur = self.overrides.mute.map(|(v, _)| v).unwrap_or(snap.volume.muted);
                let next = !cur;
                self.overrides.mute = Some((next, now));
                self.sys.send(Command::Mute(next));
            }
            Action::Dnd => {
                let cur = self.overrides.dnd.map(|(v, _)| v).unwrap_or(snap.dnd.enabled);
                let next = !cur;
                self.overrides.dnd = Some((next, now));
                self.sys.send(Command::Dnd(next));
            }
            Action::Night => {
                let cur = self.overrides.night.map(|(v, _)| v).unwrap_or(snap.night.enabled);
                let next = !cur;
                self.overrides.night = Some((next, now));
                self.sys.send(Command::Night(next));
            }
            Action::Profile => self.sys.send(Command::CycleProfile),
            Action::Volume | Action::Brightness => {}
        }
    }

    fn send_slider(&mut self, action: Action, value: f32) {
        let now = Instant::now();
        match action {
            Action::Volume => {
                self.overrides.volume = Some((value, now));
                self.overrides.mute = Some((false, now));
                self.sys.send(Command::Volume(value));
            }
            Action::Brightness => {
                self.overrides.bright = Some((value, now));
                self.sys.send(Command::Brightness(value));
            }
            _ => {}
        }
    }

    fn apply_overrides(&mut self, snap: &mut Snapshot) {
        let now = Instant::now();
        let fresh = |t: Instant| now.duration_since(t) < Duration::from_millis(1400);
        if let Some((v, t)) = self.overrides.wifi {
            if fresh(t) { snap.wifi.enabled = v; snap.wifi.available = true; } else { self.overrides.wifi = None; }
        }
        if let Some((v, t)) = self.overrides.bt {
            if fresh(t) { snap.bluetooth.enabled = v; snap.bluetooth.available = true; } else { self.overrides.bt = None; }
        }
        if let Some((v, t)) = self.overrides.volume {
            if fresh(t) { snap.volume.value = v; snap.volume.available = true; } else { self.overrides.volume = None; }
        }
        if let Some((v, t)) = self.overrides.mute {
            if fresh(t) { snap.volume.muted = v; } else { self.overrides.mute = None; }
        }
        if let Some((v, t)) = self.overrides.bright {
            if fresh(t) { snap.brightness.value = v; snap.brightness.available = true; } else { self.overrides.bright = None; }
        }
        if let Some((v, t)) = self.overrides.dnd {
            if fresh(t) { snap.dnd.enabled = v; } else { self.overrides.dnd = None; }
        }
        if let Some((v, t)) = self.overrides.night {
            if fresh(t) { snap.night.enabled = v; } else { self.overrides.night = None; }
        }
    }

    fn push_hist(&mut self, snap: &Snapshot) {
        if snap.generation == 0 || snap.generation == self.hist.seen {
            return;
        }
        let mem = if snap.mem_total == 0 { 0.0 } else { snap.mem_used as f32 / snap.mem_total as f32 };
        let rx = (snap.net_rx_bps / 5_000_000.0) as f32;
        let tx = (snap.net_tx_bps / 5_000_000.0) as f32;
        let dr = (snap.disk_r_bps / (8.0 * 1024.0 * 1024.0)) as f32;
        let dw = (snap.disk_w_bps / (8.0 * 1024.0 * 1024.0)) as f32;
        self.hist.push_sample(snap.cpu_avg, mem, rx, tx, dr, dw, snap.wifi.signal, &snap.cpu);
        self.hist.seen = snap.generation;
    }

    fn shot_due(&self) -> bool {
        if self.shots.is_none() {
            return false;
        }
        const PLAN: [f32; 3] = [2.5, 5.1, 8.3];
        PLAN.iter().enumerate().any(|(i, t)| !self.shot_done[i] && self.time >= *t)
    }

    fn save_shot(&mut self, rgba: &[u8]) {
        let Some(dir) = self.shots.clone() else { return };
        const PLAN: [(f32, &str); 3] = [
            (2.5, "01-orbit.png"),
            (5.1, "02-hover-neon.png"),
            (8.3, "03-hover-acid.png"),
        ];
        let _ = std::fs::create_dir_all(&dir);
        for (i, (t, name)) in PLAN.iter().enumerate() {
            if !self.shot_done[i] && self.time >= *t {
                let path = dir.join(name);
                match save_png(&path, self.size.0, self.size.1, rgba) {
                    Ok(()) => log::info!("wrote {}", path.display()),
                    Err(err) => log::error!("png: {err}"),
                }
                self.shot_done[i] = true;
                break;
            }
        }
    }
}

fn pick(hover: &[f32], eye: Vec3, dir: Vec3) -> Option<(usize, f32, f32)> {
    let mut best: Option<(usize, f32, f32, f32)> = None;
    for i in 0..catalog().len() {
        let base = grid_pos(i);
        let hov = hover.get(i).copied().unwrap_or(0.0);
        let scale = 1.0 + hov * 0.34;
        let pos = Vec3::new(base.x, base.y, base.z + hov * 1.35);
        let model = Mat4::from_scale_rotation_translation(
            Vec3::new(WORLD_W * scale, WORLD_H * scale, 1.0),
            glam::Quat::IDENTITY,
            pos,
        );
        if let Some((t, u, v)) = hit_quad(model, eye, dir) {
            if best.map(|b| t < b.1).unwrap_or(true) {
                best = Some((i, t, u, v));
            }
        }
    }
    best.map(|(i, _, u, v)| (i, u, v))
}

fn hit_quad(model: Mat4, eye: Vec3, dir: Vec3) -> Option<(f32, f32, f32)> {
    let inv = model.inverse();
    let e = inv.transform_point3(eye);
    let d = inv.transform_vector3(dir);
    if d.z.abs() < 1e-5 {
        return None;
    }
    let t_local = -e.z / d.z;
    if t_local < 0.0 {
        return None;
    }
    let p = e + d * t_local;
    if p.x.abs() > 0.5 || p.y.abs() > 0.5 {
        return None;
    }
    let hit = model.transform_point3(p);
    let world_t = (hit - eye).dot(dir);
    if world_t < 0.0 {
        return None;
    }
    Some((world_t, p.x + 0.5, 0.5 - p.y))
}

fn save_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    Ok(())
}

impl winit::application::ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("ARCH LINUX // CONTROL CENTER")
            .with_inner_size(LogicalSize::new(self.args.width, self.args.height))
            .with_position(LogicalPosition::new(40.0, 30.0))
            .with_min_inner_size(LogicalSize::new(960.0, 600.0));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => std::sync::Arc::new(w),
            Err(err) => {
                eprintln!("window: {err}");
                event_loop.exit();
                return;
            }
        };
        window.focus_window();
        match Gpu::new(window.clone(), catalog().len()) {
            Ok(gpu) => {
                let size = window.inner_size();
                self.size = (size.width.max(1), size.height.max(1));
                self.gpu = Some(gpu);
                self.window = Some(window);
                event_loop.set_control_flow(ControlFlow::Poll);
            }
            Err(err) => {
                eprintln!("GPU init failed: {err}");
                eprintln!("На Arch: sudo pacman -S mesa vulkan-swrast libxkbcommon");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.size = (size.width.max(1), size.height.max(1));
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = [position.x as f32, position.y as f32];
                self.cursor_valid = true;
                self.on_cursor();
            }
            WindowEvent::MouseInput { state, button, .. } => self.on_mouse(button, state),
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 80.0) as f32,
                };
                self.camera.zoom(lines);
                self.user_control = true;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed && !event.repeat {
                    if event.logical_key == Key::Named(NamedKey::Escape) {
                        event_loop.exit();
                    } else {
                        self.on_key(event.logical_key);
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App::new(args);
    if let Err(err) = event_loop.run_app(&mut app) {
        eprintln!("event loop ended: {err}");
        std::process::exit(1);
    }
}
