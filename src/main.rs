mod audio;
mod backends;
mod camera;
mod config;
mod desktop;
mod detach;
mod gpu;
mod hud;
mod hypr;
mod instance;
mod model;
mod paint;
mod palette;
mod shell;
mod system;
mod theme;
mod tiles;

use audio::{Audio, Sfx};
use camera::{damp, Camera};
use clap::Parser;
use config::Config;
use glam::{Mat4, Vec3};
use gpu::{Gpu, RenderQuality, TileDraw};
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
use gpu::{GpuBackend, GpuOptions};
use winit::window::{CursorIcon, Fullscreen, Window, WindowId, WindowLevel};

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum BackendFlag {
    Vulkan,
    Gl,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum QualityFlag {
    Low,
    Medium,
    High,
}

impl QualityFlag {
    fn render(self) -> RenderQuality {
        RenderQuality::parse(match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        })
    }
}

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
    /// Borderless fullscreen on top. A second launch closes the open overlay.
    #[arg(long)]
    overlay: bool,
    /// Force a graphics backend. Also ACC_BACKEND or WGPU_BACKEND (vulkan, gl).
    #[arg(long, value_enum)]
    backend: Option<BackendFlag>,
    /// Use lavapipe (Vulkan software) when its ICD is installed. Does not override VK_ICD_FILENAMES.
    #[arg(long)]
    software: bool,
    /// Blur resolution, bloom and MSAA. High is the default.
    #[arg(long, value_enum, default_value_t = QualityFlag::High)]
    quality: QualityFlag,
    /// Keep the process attached to the terminal. By default it detaches and logs to the cache file.
    #[arg(long, short = 'f')]
    foreground: bool,
    /// Left dock, launcher, and settings. The 3D center stays available as --center.
    #[arg(long)]
    shell: bool,
    /// Open the app grid. With a running shell this toggles it.
    #[arg(long)]
    launcher: bool,
    /// Open settings. With a running shell this toggles it.
    #[arg(long)]
    settings: bool,
    /// Save dock, launcher, and settings PNGs and exit.
    #[arg(long)]
    shell_shots: Option<PathBuf>,
    /// Open the 3D control center instead of the shell.
    #[arg(long)]
    center: bool,
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
    camera: Camera,
    cursor: [f32; 2],
    cursor_prev: [f32; 2],
    cursor_valid: bool,
    drag: Drag,
    drag_button: Option<MouseButton>,
    alt: bool,
    ctrl: bool,
    shift: bool,
    keys: FlyKeys,
    hover: Vec<f32>,
    hover_idx: Option<usize>,
    hotspots: Vec<Vec<Hotspot>>,
    hud_hits: Vec<hud::HudHit>,
    force_hover: Option<usize>,
    hist: Hist,
    base_snap: Snapshot,
    snap: Snapshot,
    tile_key: Vec<u64>,
    tile_at: Vec<Instant>,
    hud_key: u64,
    hud_at: Instant,
    show_fps: bool,
    fps_value: f32,
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
    audio: Audio,
    hover_tile: Option<usize>,
    focus_sent: Option<usize>,
    last_hover_sfx: Instant,
    last_slider_sfx: Instant,
    overlay: bool,
    intro: f32,
    closing: bool,
    overlay_lock: Option<instance::OverlayLock>,
}

#[derive(Clone, Copy, Default)]
struct FlyKeys {
    w: bool,
    a: bool,
    s: bool,
    d: bool,
    q: bool,
    e: bool,
}

#[derive(Clone, Copy)]
enum Drag {
    None,
    Orbit,
    Roll,
    Pan,
    Dolly,
    Fly,
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
        let audio = Audio::start(config.sound, config.sound_volume);
        audio.play(Sfx::Start);
        let overlay = (args.overlay || config.overlay) && args.shots.is_none();
        let overlay_lock = if overlay {
            Some(instance::OverlayLock::acquire_or_exit())
        } else {
            None
        };
        let n = catalog().len();
        let hist = if args.demo || showcase { Hist::seed_demo() } else { Hist::new() };
        let placeholder = Snapshot::placeholder(args.demo || showcase);
        let now = Instant::now();
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
            camera: Camera::new(),
            cursor: [0.0, 0.0],
            cursor_prev: [0.0, 0.0],
            cursor_valid: false,
            drag: Drag::None,
            drag_button: None,
            alt: false,
            ctrl: false,
            shift: false,
            keys: FlyKeys::default(),
            hover: vec![0.0; n],
            hover_idx: None,
            hotspots: vec![Vec::new(); n],
            hud_hits: Vec::new(),
            force_hover: None,
            hist,
            base_snap: placeholder.clone(),
            snap: placeholder,
            tile_key: vec![u64::MAX; n],
            tile_at: vec![now; n],
            hud_key: u64::MAX,
            hud_at: now,
            show_fps: false,
            fps_value: 0.0,
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
            audio,
            hover_tile: None,
            focus_sent: None,
            last_hover_sfx: Instant::now(),
            last_slider_sfx: Instant::now(),
            overlay,
            intro: if overlay { 0.0 } else { 1.0 },
            closing: false,
            overlay_lock,
        }
    }

    fn frame(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().clamp(0.0, 0.25);
        self.last = now;
        self.time += dt;
        self.phase += dt;
        self.frames += 1;
        self.fps_value = damp(self.fps_value, 1.0 / dt.max(1.0 / 480.0), 6.0, dt);
        if self.fps_at.elapsed() > Duration::from_secs(2) {
            let fps = self.frames as f32 / self.fps_at.elapsed().as_secs_f32();
            log::info!("fps {fps:.1}");
            self.frames = 0;
            self.fps_at = Instant::now();
        }

        if self.sys.copy_if_newer(&mut self.base_snap) || self.overrides_live() {
            let mut snap = self.base_snap.clone();
            self.apply_overrides(&mut snap);
            self.snap = snap;
        }
        let snap = self.snap.clone();
        self.push_hist(&snap);
        if self.cpu_vis.len() != snap.cpu.len() {
            self.cpu_vis = snap.cpu.clone();
        }
        for (vis, target) in self.cpu_vis.iter_mut().zip(snap.cpu.iter()) {
            *vis = damp(*vis, *target, 8.0, dt);
        }

        if matches!(self.drag, Drag::Fly) {
            let mut wish = Vec3::ZERO;
            if self.keys.w { wish.z += 1.0; }
            if self.keys.s { wish.z -= 1.0; }
            if self.keys.d { wish.x += 1.0; }
            if self.keys.a { wish.x -= 1.0; }
            if self.keys.e { wish.y += 1.0; }
            if self.keys.q { wish.y -= 1.0; }
            self.camera.fly_move(wish, dt, self.shift);
        }
        if self.showcase && !self.user_control {
            self.camera.showcase_spin(dt);
            self.force_hover = if (1.4..7.4).contains(&self.time) {
                Some(network_index())
            } else {
                None
            };
            if self.time > 4.6 && !self.switched_acid {
                self.set_theme(ThemeId::Acid, false);
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
        self.ui_sounds();
        if self.overlay {
            if self.overlay_lock.as_ref().is_some_and(|lock| lock.close_requested()) {
                self.closing = true;
            }
            let target = if self.closing { 0.0 } else { 1.0 };
            self.intro = damp(self.intro, target, 8.0, dt);
            if self.closing && self.intro < 0.025 {
                el.exit();
                return;
            }
        }

        self.tile_canvas.theme = self.theme;
        let drawn = chrono::Local::now();
        let second = drawn.timestamp();
        let anim = (self.time * 15.0) as u32;
        let theme_key = theme_fingerprint(&self.theme);
        let mut upload = vec![false; catalog().len()];
        for (i, spec) in catalog().iter().enumerate() {
            let key = tile_fingerprint(spec.kind, &snap, theme_key, second, anim);
            let hot = matches!(self.drag, Drag::Slider { tile, .. } if tile == i);
            let due = self.tile_key[i] != key
                && (hot || self.tile_key[i] == u64::MAX || self.tile_at[i].elapsed() >= Duration::from_millis(50));
            if !due {
                continue;
            }
            let ctx = DrawCtx {
                snap: &snap,
                hist: &self.hist,
                cpu_vis: &self.cpu_vis,
                focus: 0.0,
                phase: self.phase,
                now: drawn,
            };
            self.hotspots[i] = tiles::draw(&mut self.tile_canvas, spec, &ctx);
            self.tile_canvas.copy_rgba(&mut self.pixels[i]);
            self.tile_key[i] = key;
            self.tile_at[i] = Instant::now();
            upload[i] = true;
        }

        let mut hud_upload = false;
        if let Some(window) = &self.window {
            let size = window.inner_size();
            self.size = (size.width.max(1), size.height.max(1));
            let hud_key = hud_fingerprint(theme_key, self.theme_id, self.hue, self.config.sound, max_h > 0.45, self.size, self.show_fps, self.fps_value);
            let hud_due = self.hud_key != hud_key
                && (self.hud_key == u64::MAX || self.size.0 != self.hud_canvas.w || self.hud_at.elapsed() >= Duration::from_millis(50));
            if hud_due {
                self.hud_canvas.resize(self.size.0, self.size.1);
                self.hud_canvas.theme = self.theme;
                let hud = Hud::draw(
                    &mut self.hud_canvas,
                    self.theme_id,
                    self.hue,
                    snap.demo,
                    max_h > 0.45,
                    self.config.sound,
                    self.show_fps.then_some(self.fps_value),
                );
                self.hud_hits = hud.hits;
                self.hud_canvas.copy_rgba(&mut self.hud_pixels);
                self.hud_key = hud_key;
                self.hud_at = Instant::now();
                hud_upload = true;
            }
            let icon = if self.pointer() { CursorIcon::Pointer } else { CursorIcon::Default };
            window.set_cursor(icon);
        }

        let capture = self.shot_due();
        let focus = self.hovered().filter(|_| max_h > 0.08);
        if let Some(gpu) = &mut self.gpu {
            gpu.resize(self.size.0, self.size.1);
            let aspect = self.size.0 as f32 / self.size.1 as f32;
            let vp = self.camera.view_proj(aspect);
            let mut draws = Vec::with_capacity(catalog().len());
            for i in 0..catalog().len() {
                let base = grid_pos(i);
                let hov = self.hover[i];
                let scale = (1.0 + hov * 0.46) * (0.92 + 0.08 * self.intro);
                let pos = Vec3::new(base.x, base.y, base.z + hov * 1.65);
                let model = Mat4::from_scale_rotation_translation(
                    Vec3::new(WORLD_W * scale, WORLD_H * scale, 1.0),
                    glam::Quat::IDENTITY,
                    pos,
                );
                let dim = max_h * 0.22;
                let bright = (1.0 - dim + hov * dim).clamp(0.7, 1.15);
                let frost = (max_h * (1.0 - hov)).clamp(0.0, 1.0);
                let mut accent = self.theme.primary.to_f32();
                accent[3] = 0.46;
                let secondary = self.theme.secondary.to_f32();
                draws.push(TileDraw {
                    model: model.to_cols_array_2d(),
                    params: [0.0, bright, hov, frost],
                    accent,
                    secondary,
                    pixels: &self.pixels[i],
                    upload: upload[i],
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
                    self.intro,
                    self.overlay,
                    max_h,
                    focus,
                    hud_upload,
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
        let _ = (COLS, Kind::Host);
    }

    fn hovered(&self) -> Option<usize> {
        if let Some(i) = self.force_hover {
            return Some(i);
        }
        if self.camera_drag() {
            return None;
        }
        self.hover_idx
    }

    fn camera_drag(&self) -> bool {
        matches!(self.drag, Drag::Orbit | Drag::Roll | Drag::Pan | Drag::Dolly | Drag::Fly)
    }

    fn update_hover(&mut self) {
        if self.shots.is_some() {
            self.hover_idx = None;
            return;
        }
        if self.hud_hot() {
            self.hover_idx = None;
            return;
        }
        if self.camera_drag() {
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
            if self.drag_button == Some(button) {
                if let Drag::Hue = self.drag {
                    self.config.theme = ThemeId::Custom;
                    self.config.custom_hue = self.hue;
                    self.config.save();
                }
                if let Drag::Slider { action, .. } = self.drag {
                    self.send_slider(action, self.slider);
                }
                if matches!(self.drag, Drag::Fly) {
                    self.camera.end_fly();
                }
                self.drag = Drag::None;
                self.drag_button = None;
            }
            return;
        }
        if self.drag_button.is_some() {
            return;
        }
        if button != MouseButton::Left && button != MouseButton::Right && button != MouseButton::Middle {
            return;
        }
        self.user_control = true;
        self.cursor_prev = self.cursor;
        let (nx, ny) = self.ndc();
        if button == MouseButton::Middle {
            self.drag = Drag::Pan;
            self.drag_button = Some(button);
            return;
        }
        if button == MouseButton::Right {
            if self.alt {
                self.drag = Drag::Dolly;
            } else {
                self.camera.begin_fly();
                self.drag = Drag::Fly;
            }
            self.drag_button = Some(button);
            return;
        }
        if self.ctrl {
            self.drag = Drag::Roll;
            self.drag_button = Some(button);
            return;
        }
        if self.alt {
            self.drag = Drag::Orbit;
            self.drag_button = Some(button);
            return;
        }
        if let Some(hit) = self.hud_hits.iter().copied().find(|h| h.rect.contains(self.cursor[0], self.cursor[1])) {
            match hit.action {
                HudAction::Theme(id) => self.set_theme(id, true),
                HudAction::Reset => {
                    self.camera.request_reset();
                    self.audio.play(Sfx::Reset);
                }
                HudAction::Sound => self.toggle_sound(),
                HudAction::Hue => {
                    self.hue_from_cursor(hit.rect);
                    self.drag = Drag::Hue;
                    self.drag_button = Some(button);
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
                        self.drag_button = Some(button);
                        self.send_slider(hot.action, self.slider);
                    }
                    other => self.fire(other),
                }
                return;
            }
        }
        self.drag = Drag::Orbit;
        self.drag_button = Some(button);
        let _ = (nx, ny);
    }

    fn on_cursor(&mut self) {
        let dx = self.cursor[0] - self.cursor_prev[0];
        let dy = self.cursor[1] - self.cursor_prev[1];
        self.cursor_prev = self.cursor;
        match self.drag {
            Drag::Orbit => self.camera.orbit_pixels(dx, dy),
            Drag::Roll => self.camera.roll_pixels(dx),
            Drag::Pan => self.camera.pan_pixels(dx, dy),
            Drag::Dolly => self.camera.dolly_pixels(dy),
            Drag::Fly => self.camera.fly_look(dx, dy),
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
            Key::Named(NamedKey::F3) => self.show_fps = !self.show_fps,
            Key::Character(s) => {
                let s = s.as_str();
                if s.eq_ignore_ascii_case("r") {
                    self.camera.request_reset();
                    self.audio.play(Sfx::Reset);
                    self.user_control = true;
                } else if s.eq_ignore_ascii_case("f") {
                    self.frame_view();
                    self.user_control = true;
                } else if s.eq_ignore_ascii_case("m") {
                    self.toggle_sound();
                }
                if let Some(id) = ThemeId::from_key(s) {
                    self.set_theme(id, true);
                }
            }
            _ => {}
        }
    }

    fn frame_view(&mut self) {
        if let Some(i) = self.hovered().or(self.hover_idx) {
            self.camera.frame_at(grid_pos(i), 1.15);
        } else {
            self.camera.frame_at(Vec3::new(0.0, 0.05, 0.0), 5.1);
        }
    }

    fn note_key(&mut self, key: &Key, down: bool) {
        let Key::Character(s) = key else {
            return;
        };
        match s.as_str() {
            "w" | "W" => self.keys.w = down,
            "a" | "A" => self.keys.a = down,
            "s" | "S" => self.keys.s = down,
            "d" | "D" => self.keys.d = down,
            "q" | "Q" => self.keys.q = down,
            "e" | "E" => self.keys.e = down,
            _ => {}
        }
    }

    fn overrides_live(&self) -> bool {
        self.overrides.wifi.is_some()
            || self.overrides.bt.is_some()
            || self.overrides.volume.is_some()
            || self.overrides.mute.is_some()
            || self.overrides.bright.is_some()
            || self.overrides.dnd.is_some()
            || self.overrides.night.is_some()
    }

    fn set_theme(&mut self, id: ThemeId, save: bool) {
        if self.theme_id != id {
            self.audio.play(Sfx::Theme);
        }
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
                self.toggle_sfx(next);
            }
            Action::Bluetooth => {
                let cur = self.overrides.bt.map(|(v, _)| v).unwrap_or(snap.bluetooth.enabled);
                let next = !cur;
                self.overrides.bt = Some((next, now));
                self.sys.send(Command::Bluetooth(next));
                self.toggle_sfx(next);
            }
            Action::Mute => {
                let cur = self.overrides.mute.map(|(v, _)| v).unwrap_or(snap.volume.muted);
                let next = !cur;
                self.overrides.mute = Some((next, now));
                self.sys.send(Command::Mute(next));
                self.toggle_sfx(!next);
            }
            Action::Dnd => {
                let cur = self.overrides.dnd.map(|(v, _)| v).unwrap_or(snap.dnd.enabled);
                let next = !cur;
                self.overrides.dnd = Some((next, now));
                self.sys.send(Command::Dnd(next));
                self.toggle_sfx(next);
            }
            Action::Night => {
                let cur = self.overrides.night.map(|(v, _)| v).unwrap_or(snap.night.enabled);
                let next = !cur;
                self.overrides.night = Some((next, now));
                self.sys.send(Command::Night(next));
                self.toggle_sfx(next);
            }
            Action::Profile => {
                self.sys.send(Command::CycleProfile);
                self.audio.play(Sfx::ToggleOn);
            }
            Action::Volume | Action::Brightness => {}
        }
    }

    fn send_slider(&mut self, action: Action, value: f32) {
        let now = Instant::now();
        if self.last_slider_sfx.elapsed() > Duration::from_millis(55) {
            self.audio.play(Sfx::Slider);
            self.last_slider_sfx = now;
        }
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

    fn toggle_sfx(&self, on: bool) {
        self.audio.play(if on { Sfx::ToggleOn } else { Sfx::ToggleOff });
    }

    fn toggle_sound(&mut self) {
        self.config.sound = !self.config.sound;
        self.audio.set_enabled(self.config.sound);
        self.audio.set_volume(self.config.sound_volume);
        self.config.save();
        if self.config.sound {
            self.audio.play(Sfx::ToggleOn);
        }
    }

    fn ui_sounds(&mut self) {
        let current = self.hovered();
        if current != self.hover_tile {
            if current.is_some() && self.last_hover_sfx.elapsed() > Duration::from_millis(75) {
                self.audio.play(Sfx::Hover);
                self.last_hover_sfx = Instant::now();
            }
            self.hover_tile = current;
            self.focus_sent = None;
        }
        if let Some(i) = current {
            if self.hover[i] > 0.62 && self.focus_sent != Some(i) {
                self.audio.play(Sfx::Focus);
                self.focus_sent = Some(i);
            }
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
        const PLAN: [f32; 3] = [0.7, 3.4, 6.4];
        PLAN.iter().enumerate().any(|(i, t)| !self.shot_done[i] && self.time >= *t)
    }

    fn save_shot(&mut self, rgba: &[u8]) {
        let Some(dir) = self.shots.clone() else { return };
        const PLAN: [(f32, &str); 3] = [
            (0.7, "01-orbit.png"),
            (3.4, "02-hover-neon.png"),
            (6.4, "03-hover-acid.png"),
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

fn theme_fingerprint(theme: &theme::ThemeColors) -> u64 {
    fn pack(c: [f32; 4]) -> u64 {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 32.0) as u64;
        q(c[0]) | (q(c[1]) << 6) | (q(c[2]) << 12)
    }
    pack(theme.primary.to_f32())
        ^ pack(theme.secondary.to_f32()).rotate_left(8)
        ^ pack(theme.text.to_f32()).rotate_left(16)
        ^ pack(theme.panel.to_f32()).rotate_left(24)
        ^ pack(theme.bg.to_f32()).rotate_left(4)
}

fn tile_fingerprint(kind: Kind, snap: &Snapshot, theme_key: u64, second: i64, anim: u32) -> u64 {
    let mut key = theme_key ^ snap.generation.wrapping_mul(0x9E37_79B1_85EB_CA87);
    key ^= (snap.wifi.enabled as u64) << 1;
    key ^= (snap.bluetooth.enabled as u64) << 2;
    key ^= (snap.volume.muted as u64) << 3;
    key ^= (snap.dnd.enabled as u64) << 4;
    key ^= (snap.night.enabled as u64) << 5;
    key ^= ((snap.volume.value * 500.0) as u64) << 8;
    key ^= ((snap.brightness.value * 500.0) as u64) << 20;
    match kind {
        Kind::Clock => key ^ (second as u64).wrapping_shl(40),
        Kind::Audio | Kind::Sensors | Kind::Cpu => key ^ (anim as u64).wrapping_shl(40),
        Kind::Shortcuts => theme_key,
        _ => key,
    }
}

fn hud_fingerprint(
    theme_key: u64,
    theme_id: ThemeId,
    hue: f32,
    sound: bool,
    focus: bool,
    size: (u32, u32),
    show_fps: bool,
    fps: f32,
) -> u64 {
    let id = match theme_id {
        ThemeId::Neon => 1,
        ThemeId::Acid => 2,
        ThemeId::Ice => 3,
        ThemeId::Blood => 4,
        ThemeId::Mono => 5,
        ThemeId::Custom => 6,
    };
    let fps_key = if show_fps { fps.round() as u64 } else { 0 };
    theme_key
        ^ (id << 33)
        ^ ((hue * 2.0) as u64).wrapping_shl(8)
        ^ (sound as u64)
        ^ ((focus as u64) << 2)
        ^ ((size.0 as u64) << 40)
        ^ ((size.1 as u64) << 20)
        ^ fps_key.wrapping_shl(48)
        ^ ((show_fps as u64) << 63)
}

fn pick(hover: &[f32], eye: Vec3, dir: Vec3) -> Option<(usize, f32, f32)> {
    let mut best: Option<(usize, f32, f32, f32)> = None;
    for i in 0..catalog().len() {
        let base = grid_pos(i);
        let hov = hover.get(i).copied().unwrap_or(0.0);
        let scale = 1.0 + hov * 0.46;
        let pos = Vec3::new(base.x, base.y, base.z + hov * 1.65);
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

fn resolve_backend(flag: Option<BackendFlag>) -> GpuBackend {
    if let Some(flag) = flag {
        return match flag {
            BackendFlag::Vulkan => GpuBackend::Vulkan,
            BackendFlag::Gl => GpuBackend::Gl,
        };
    }
    let from_env = std::env::var("ACC_BACKEND")
        .or_else(|_| std::env::var("WGPU_BACKEND"))
        .ok();
    match from_env.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("vulkan") | Some("vk") => GpuBackend::Vulkan,
        Some("gl") | Some("gles") | Some("opengl") => GpuBackend::Gl,
        _ => GpuBackend::Auto,
    }
}

fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("yes")
    )
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
        let mut attrs = Window::default_attributes().with_title("ARCH LINUX // CONTROL CENTER");
        attrs = winit::platform::wayland::WindowAttributesExtWayland::with_name(
            attrs,
            "arch-control-center",
            "arch-control-center",
        );
        attrs = winit::platform::x11::WindowAttributesExtX11::with_name(
            attrs,
            "arch-control-center",
            "arch-control-center",
        );
        if self.overlay {
            if let Some(monitor) = event_loop
                .primary_monitor()
                .or_else(|| event_loop.available_monitors().next())
            {
                let size = monitor.size();
                attrs = attrs.with_inner_size(winit::dpi::PhysicalSize::new(size.width, size.height));
            }
            attrs = attrs
                .with_decorations(false)
                .with_transparent(true)
                .with_resizable(false)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_fullscreen(Some(Fullscreen::Borderless(None)));
        } else {
            attrs = attrs
                .with_inner_size(LogicalSize::new(self.args.width, self.args.height))
                .with_position(LogicalPosition::new(40.0, 30.0))
                .with_min_inner_size(LogicalSize::new(960.0, 600.0));
        }
        let window = match event_loop.create_window(attrs) {
            Ok(w) => std::sync::Arc::new(w),
            Err(err) => {
                eprintln!("window: {err}");
                event_loop.exit();
                return;
            }
        };
        window.focus_window();
        let opts = GpuOptions {
            backend: resolve_backend(self.args.backend),
            software: self.args.software || env_flag("ACC_SOFTWARE"),
            want_transparency: self.overlay,
            quality: self.args.quality.render(),
        };
            match Gpu::new(window.clone(), catalog().len(), opts) {
            Ok(gpu) => {
                log::info!("{}", gpu.adapter_line());
                let size = window.inner_size();
                self.size = (size.width.max(1), size.height.max(1));
                self.gpu = Some(gpu);
                self.window = Some(window);
                event_loop.set_control_flow(ControlFlow::Poll);
            }
            Err(err) => {
                eprintln!("GPU init failed: {err}");
                eprintln!("NVIDIA: sudo pacman -S nvidia-utils vulkan-icd-loader && vulkaninfo --summary");
                eprintln!("AMD/Intel: vulkan-radeon или vulkan-intel. Без GPU: vulkan-swrast и --software");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                if self.overlay {
                    self.closing = true;
                } else {
                    event_loop.exit();
                }
            }
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
                self.user_control = true;
                if self.alt {
                    self.camera.dolly_lines(lines);
                } else {
                    let (nx, ny) = self.ndc();
                    let aspect = self.size.0 as f32 / self.size.1.max(1) as f32;
                    self.camera.zoom_toward_cursor(lines, nx, ny, aspect);
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                let state = mods.state();
                self.alt = state.alt_key();
                self.ctrl = state.control_key();
                self.shift = state.shift_key();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let down = event.state == ElementState::Pressed;
                self.note_key(&event.logical_key, down);
                if down && !event.repeat {
                    if event.logical_key == Key::Named(NamedKey::Escape) {
                        if self.overlay {
                            self.closing = true;
                        } else {
                            event_loop.exit();
                        }
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
    let args = Args::parse();
    if !args.foreground {
        detach::detach();
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let shell_mode = (args.shell || args.launcher || args.settings || args.shell_shots.is_some())
        && !args.center
        && !args.demo
        && !args.showcase
        && args.shots.is_none();
    if shell_mode {
        shell::run(shell::ShellArgs {
            launcher: args.launcher || args.shell_shots.is_some(),
            settings: args.settings || args.shell_shots.is_some(),
            shots: args.shell_shots.clone(),
            backend: resolve_backend(args.backend),
            software: args.software || env_flag("ACC_SOFTWARE"),
            quality: args.quality.render(),
        });
        return;
    }
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App::new(args);
    if let Err(err) = event_loop.run_app(&mut app) {
        eprintln!("event loop ended: {err}");
        std::process::exit(1);
    }
}
