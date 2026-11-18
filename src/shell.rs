//! Hyprland shell: left dock, app launcher, and settings.
//! The 3D control center is a separate window launched from the dock.

use crate::audio::{Audio, Sfx};
use crate::config::{Config, UserTheme};
use crate::desktop::{self, AppEntry};
use crate::gpu::{FlatGpu, GpuBackend, GpuOptions, RenderQuality};
use crate::hypr::{self, HyprSnap};
use crate::paint::{Canvas, UiRect};
use crate::palette::{self, Palette};
use crate::theme::{colors, Rgba, ThemeId};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId, WindowLevel};

pub struct ShellArgs {
    pub launcher: bool,
    pub settings: bool,
    pub shots: Option<PathBuf>,
    pub backend: GpuBackend,
    pub software: bool,
    pub quality: RenderQuality,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Welcome,
    General,
    Screen,
    Appearance,
    Wallpaper,
    Apps,
    Panel,
    Dock,
    Launcher,
    Osd,
    Notes,
    Shot,
    Wellbeing,
}

#[derive(Clone)]
struct Icon {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

struct Shell {
    args: ShellArgs,
    cfg: Config,
    audio: Audio,
    apps: Vec<AppEntry>,
    fonts: Vec<(String, Option<PathBuf>)>,
    icons: HashMap<String, Icon>,
    hypr: Arc<Mutex<HyprSnap>>,
    cmd: Arc<AtomicU8>,
    dock: Option<View>,
    launcher: Option<View>,
    settings: Option<View>,
    show_launcher: bool,
    show_settings: bool,
    page: Page,
    scroll: f32,
    query: String,
    field: Field,
    cursor: [f32; 2],
    pointer: Option<WindowId>,
    dock_hits: Vec<Hit>,
    launcher_hits: Vec<Hit>,
    settings_hits: Vec<Hit>,
    font_applied: String,
    drag: Drag,
    hover_id: u64,
    hover_t: f32,
    frames: u32,
    last: Instant,
    shots_done: u8,
    scale: f32,
}

struct View {
    window: Arc<Window>,
    gpu: Option<FlatGpu>,
    canvas: Option<Canvas>,
    failed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    None,
    Search,
    Wallpaper,
    Gtk,
    ShotDir,
}

#[derive(Clone)]
enum Drag {
    None,
    Opacity,
}

#[derive(Clone)]
struct Hit {
    rect: UiRect,
    id: u64,
    act: Act,
}

#[derive(Clone)]
enum Act {
    Overview,
    Workspace(i32),
    Launch(usize),
    Focus(String),
    OpenSettings,
    OpenCenter,
    Page(Page),
    Radius(i32),
    Opacity,
    BlurToggle,
    Glass(bool),
    Font(i32),
    Search,
    Palette(String),
    AddTheme,
    DelTheme(usize),
    Matugen,
    Field(Field),
    Break(i32),
    Flag(&'static str),
    Pin(usize),
}

pub fn run(args: ShellArgs) {
    let open = if args.launcher { 1 } else { 0 } | if args.settings { 2 } else { 0 };
    let cmd = match acquire(open) {
        Some(cmd) => cmd,
        None => return,
    };
    let cfg = Config::load();
    let audio = Audio::start(cfg.sound, cfg.sound_volume);
    audio.play(Sfx::Start);
    let event_loop = EventLoop::new().expect("event loop");
    let mut shell = Shell {
        show_launcher: args.launcher || args.shots.is_some(),
        show_settings: args.settings || args.shots.is_some(),
        args,
        cfg,
        audio,
        apps: desktop::scan_apps(),
        fonts: desktop::discover_fonts(),
        icons: HashMap::new(),
        hypr: hypr::start(),
        cmd,
        dock: None,
        launcher: None,
        settings: None,
        page: Page::Appearance,
        scroll: 0.0,
        query: String::new(),
        field: Field::None,
        cursor: [0.0; 2],
        pointer: None,
        dock_hits: Vec::new(),
        launcher_hits: Vec::new(),
        settings_hits: Vec::new(),
        font_applied: String::new(),
        drag: Drag::None,
        hover_id: 0,
        hover_t: 0.0,
        frames: 0,
        last: Instant::now(),
        shots_done: 0,
        scale: 1.0,
    };
    if shell.cfg.shell.pinned.is_empty() {
        shell.cfg.shell.pinned = default_pins(&shell.apps);
    }
    if let Err(err) = event_loop.run_app(&mut shell) {
        eprintln!("shell: {err}");
    }
}

fn default_pins(apps: &[AppEntry]) -> Vec<String> {
    let want = ["firefox", "chromium", "google-chrome", "kitty", "alacritty", "org.gnome.Nautilus", "thunar", "nautilus"];
    let mut pins = Vec::new();
    for id in want {
        if apps.iter().any(|a| a.id == id) {
            pins.push(id.to_string());
        }
    }
    if pins.is_empty() {
        pins.extend(apps.iter().take(4).map(|a| a.id.clone()));
    }
    pins
}

impl winit::application::ApplicationHandler for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.dock.is_some() {
            return;
        }
        let monitor = event_loop.primary_monitor().or_else(|| event_loop.available_monitors().next());
        let (mw, mh, scale) = monitor
            .map(|m| (m.size().width, m.size().height, m.scale_factor() as f32))
            .unwrap_or((1440, 900, 1.0));
        self.scale = (scale * self.cfg.shell.ui_scale).clamp(0.85, 2.0);
        let dock_w = (78.0 * self.scale).round() as u32;
        self.dock = Some(self.open(event_loop, "arch-shell", "ARCH SHELL", dock_w, mh, 0, 0, true));
        let lw = (900.0 * self.scale).round() as u32;
        let lh = (620.0 * self.scale).round() as u32;
        self.launcher = Some(self.open(event_loop, "arch-launcher", "Запуск", lw, lh, ((mw - lw) / 2) as i32, ((mh - lh) / 5) as i32, false));
        let sw = (1140.0 * self.scale).round().min(mw as f32 - 40.0) as u32;
        let sh = (740.0 * self.scale).round().min(mh as f32 - 40.0) as u32;
        self.settings = Some(self.open(event_loop, "arch-settings", "Настройки", sw, sh, ((mw - sw) / 2) as i32, ((mh - sh) / 6) as i32, false));
        event_loop.set_control_flow(ControlFlow::Poll);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                if self.settings.as_ref().is_some_and(|v| v.window.id() == id) {
                    self.show_settings = false;
                    if let Some(v) = &self.settings {
                        v.window.set_visible(false);
                    }
                } else if self.launcher.as_ref().is_some_and(|v| v.window.id() == id) {
                    self.show_launcher = false;
                    if let Some(v) = &self.launcher {
                        v.window.set_visible(false);
                    }
                } else {
                    event_loop.exit();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = [position.x as f32, position.y as f32];
                self.pointer = Some(id);
                if matches!(self.drag, Drag::Opacity) {
                    self.set_opacity();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => self.mouse(id, button, state),
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 40.0) as f32,
                };
                if self.show_settings {
                    self.scroll = (self.scroll - lines * 36.0).max(0.0);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if event.logical_key == Key::Named(NamedKey::Escape) {
                    if self.show_launcher {
                        self.show_launcher = false;
                    } else if self.show_settings {
                        self.show_settings = false;
                    } else {
                        event_loop.exit();
                    }
                    return;
                }
                self.type_key(&event.logical_key);
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        for view in [&self.dock, &self.launcher, &self.settings].into_iter().flatten() {
            view.window.request_redraw();
        }
    }
}

impl Shell {
    fn open(&self, el: &ActiveEventLoop, class: &str, title: &str, w: u32, h: u32, x: i32, y: i32, dock: bool) -> View {
        let mut attrs = Window::default_attributes()
            .with_title(title)
            .with_inner_size(PhysicalSize::new(w.max(1), h.max(1)))
            .with_position(PhysicalPosition::new(x, y))
            .with_resizable(!dock)
            .with_transparent(true);
        if dock {
            attrs = attrs.with_decorations(false).with_window_level(WindowLevel::AlwaysOnTop);
        }
        attrs = winit::platform::wayland::WindowAttributesExtWayland::with_name(attrs, class, class);
        attrs = winit::platform::x11::WindowAttributesExtX11::with_name(attrs, class, class);
        let window = Arc::new(el.create_window(attrs).expect("window"));
        let opts = GpuOptions {
            backend: self.args.backend,
            software: self.args.software,
            want_transparency: true,
            quality: self.args.quality,
        };
        let gpu = match FlatGpu::new(window.clone(), opts) {
            Ok(gpu) => Some(gpu),
            Err(err) => {
                log::error!("flat gpu {class}: {err}");
                None
            }
        };
        let failed = gpu.is_none();
        let theme = colors(ThemeId::Neon, 300.0);
        View { window, gpu, canvas: Some(Canvas::new(w, h, theme)), failed }
    }

    fn frame(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().clamp(0.0, 0.05);
        self.last = now;
        self.frames += 1;
        let msg = self.cmd.swap(0, Ordering::SeqCst);
        if msg & 1 != 0 {
            self.show_launcher = !self.show_launcher;
        }
        if msg & 2 != 0 {
            self.show_settings = !self.show_settings;
        }
        if msg & 4 != 0 {
            el.exit();
            return;
        }
        self.hover_t += (1.0 - self.hover_t) * (1.0 - (-10.0 * dt).exp());
        if let Some(id) = self.pointer {
            let hits = self.hits_for(id);
            self.hover_id = hits.iter().find(|h| h.rect.contains(self.cursor[0], self.cursor[1])).map(|h| h.id).unwrap_or(0);
        }
        self.warm_icons();
        self.apply_font();
        let pal = self.palette();
        self.paint(ViewKind::Dock, &pal);
        self.paint(ViewKind::Launcher, &pal);
        self.paint(ViewKind::Settings, &pal);
        self.capture();
        if self.args.shots.is_some() && self.shots_done >= 3 {
            el.exit();
        }
    }

    fn capture(&mut self) {
        let Some(dir) = self.args.shots.clone() else { return };
        let _ = std::fs::create_dir_all(&dir);
        let save = |canvas: &Option<Canvas>, name: &str| {
            let Some(canvas) = canvas else { return };
            let path = dir.join(name);
            let _ = save_png(&path, canvas.w, canvas.h, canvas.pixels());
        };
        if self.frames == 3 {
            if let Some(dock) = &self.dock {
                save(&dock.canvas, "04-dock.png");
            }
            self.shots_done = 1;
            self.show_launcher = true;
        }
        if self.frames == 8 {
            if let Some(launcher) = &self.launcher {
                save(&launcher.canvas, "05-launcher.png");
            }
            self.shots_done = 2;
            self.show_launcher = false;
            self.show_settings = true;
            self.page = Page::Appearance;
        }
        if self.frames == 12 {
            if let Some(settings) = &self.settings {
                save(&settings.canvas, "06-settings.png");
            }
            self.shots_done = 3;
        }
    }

    fn palette(&self) -> Palette {
        if let Some(p) = palette::by_id(&self.cfg.shell.palette) {
            return *p;
        }
        if let Some(custom) = self.cfg.shell.custom_themes.iter().find(|t| t.name == self.cfg.shell.palette) {
            if let Some(cols) = custom_palette(custom) {
                return cols;
            }
        }
        palette::by_id("cyberpunk").copied().unwrap_or(PRESET_FALLBACK)
    }

    fn apply_font(&mut self) {
        let name = self.cfg.shell.font_name.clone();
        if self.font_applied.eq_ignore_ascii_case(&name) {
            return;
        }
        self.font_applied = name.clone();
        if name.eq_ignore_ascii_case("JetBrains Mono") {
            return;
        }
        if let Some((_, Some(path))) = self.fonts.iter().find(|(n, _)| n.eq_ignore_ascii_case(&name)) {
            if let Ok(bytes) = std::fs::read(path) {
                for view in [&mut self.dock, &mut self.launcher, &mut self.settings].into_iter().flatten() {
                    if let Some(canvas) = view.canvas.as_mut() {
                        canvas.load_font_bytes(&bytes);
                    }
                }
            }
        }
    }

    fn warm_icons(&mut self) {
        let mut names = Vec::new();
        for id in &self.cfg.shell.pinned {
            if let Some(app) = self.apps.iter().find(|a| &a.id == id) {
                names.push(app.icon.clone());
            }
        }
        for app in self.apps.iter().take(36) {
            names.push(app.icon.clone());
        }
        for name in names {
            let _ = self.icon(&name);
        }
    }

    fn hits_for(&self, id: WindowId) -> &[Hit] {
        if self.dock.as_ref().is_some_and(|v| v.window.id() == id) {
            &self.dock_hits
        } else if self.launcher.as_ref().is_some_and(|v| v.window.id() == id) {
            &self.launcher_hits
        } else if self.settings.as_ref().is_some_and(|v| v.window.id() == id) {
            &self.settings_hits
        } else {
            &[]
        }
    }

    fn mouse(&mut self, id: WindowId, button: MouseButton, state: ElementState) {
        if state == ElementState::Released {
            self.drag = Drag::None;
            return;
        }
        if button != MouseButton::Left && button != MouseButton::Right {
            return;
        }
        let cursor = self.cursor;
        let hit = self.hits_for(id).iter().find(|h| h.rect.contains(cursor[0], cursor[1])).map(|h| h.act.clone());
        let Some(act) = hit else {
            self.field = Field::None;
            return;
        };
        self.audio.play(Sfx::ToggleOn);
        match act {
            Act::Overview => self.show_launcher = !self.show_launcher,
            Act::OpenSettings => self.show_settings = !self.show_settings,
            Act::OpenCenter => open_center(),
            Act::Workspace(id) => hypr::focus_workspace(id),
            Act::Launch(i) => {
                if let Some(app) = self.apps.get(i) {
                    desktop::launch(&app.exec);
                    self.show_launcher = false;
                }
            }
            Act::Pin(i) => {
                if let Some(app) = self.apps.get(i) {
                    let id = app.id.clone();
                    if let Some(pos) = self.cfg.shell.pinned.iter().position(|p| p == &id) {
                        self.cfg.shell.pinned.remove(pos);
                    } else {
                        self.cfg.shell.pinned.push(id);
                    }
                    self.cfg.save();
                }
            }
            Act::Focus(addr) => hypr::focus_client(&addr),
            Act::Page(page) => self.page = page,
            Act::Radius(delta) => {
                self.cfg.shell.corner_radius = (self.cfg.shell.corner_radius + delta as f32).clamp(0.0, 32.0);
                self.cfg.save();
                self.audio.play(Sfx::Slider);
            }
            Act::Opacity => {
                self.drag = Drag::Opacity;
                self.set_opacity();
            }
            Act::BlurToggle => {
                self.cfg.shell.wallpaper_blur = !self.cfg.shell.wallpaper_blur;
                self.cfg.save();
            }
            Act::Glass(on) => {
                self.cfg.shell.liquid_glass = on;
                self.cfg.shell.wallpaper_blur = true;
                self.cfg.save();
            }
            Act::Font(delta) => self.cycle_font(delta),
            Act::Search => self.field = Field::Search,
            Act::Palette(id) => self.select_palette(id),
            Act::AddTheme => self.add_theme(),
            Act::DelTheme(i) => {
                if i < self.cfg.shell.custom_themes.len() {
                    let name = self.cfg.shell.custom_themes[i].name.clone();
                    self.cfg.shell.custom_themes.remove(i);
                    if self.cfg.shell.palette == name {
                        self.cfg.shell.palette = "cyberpunk".into();
                    }
                    self.cfg.save();
                }
            }
            Act::Matugen => self.matugen(),
            Act::Field(field) => self.field = field,
            Act::Break(delta) => {
                self.cfg.shell.wellbeing_break_min = self.cfg.shell.wellbeing_break_min.saturating_add_signed(delta).clamp(5, 180);
                self.cfg.save();
            }
            Act::Flag(which) => {
                match which {
                    "clock" => self.cfg.shell.panel_clock = !self.cfg.shell.panel_clock,
                    "hide" => self.cfg.shell.dock_autohide = !self.cfg.shell.dock_autohide,
                    "osd" => self.cfg.shell.osd = !self.cfg.shell.osd,
                    "notes" => self.cfg.shell.notifications = !self.cfg.shell.notifications,
                    "sound" => {
                        self.cfg.sound = !self.cfg.sound;
                        self.audio.set_enabled(self.cfg.sound);
                    }
                    _ => {}
                }
                self.cfg.save();
            }
        }
        let _ = button;
    }

    fn set_opacity(&mut self) {
        let hits = self.settings_hits.clone();
        if let Some(hit) = hits.iter().find(|h| matches!(h.act, Act::Opacity)) {
            self.cfg.shell.opacity = hit.rect.value_x(self.cursor[0]).clamp(0.35, 1.0);
            self.cfg.save();
        }
    }

    fn cycle_font(&mut self, delta: i32) {
        if self.fonts.is_empty() {
            return;
        }
        let idx = self.fonts.iter().position(|(n, _)| n.eq_ignore_ascii_case(&self.cfg.shell.font_name)).unwrap_or(0);
        let next = (idx as i32 + delta).rem_euclid(self.fonts.len() as i32) as usize;
        self.cfg.shell.font_name = self.fonts[next].0.clone();
        self.font_applied.clear();
        self.cfg.save();
        self.audio.play(Sfx::Slider);
    }

    fn select_palette(&mut self, id: String) {
        self.cfg.shell.palette = id;
        if let Some(p) = palette::by_id(&self.cfg.shell.palette) {
            self.cfg.custom_hue = palette::accent_hue(p.accents[0]);
            self.cfg.theme = match p.id {
                "cyberpunk" => ThemeId::Neon,
                "horizon" => ThemeId::Blood,
                "solarized" => ThemeId::Acid,
                "night-owl" | "tokyo" | "ocean" => ThemeId::Ice,
                "obsidian" | "vesper" | "github" | "carbonfox" | "darker" => ThemeId::Mono,
                _ => ThemeId::Custom,
            };
        }
        self.cfg.save();
        self.audio.play(Sfx::Theme);
    }

    fn add_theme(&mut self) {
        let pal = self.palette();
        let n = self.cfg.shell.custom_themes.len() + 1;
        let theme = UserTheme {
            name: format!("моя тема {n}"),
            swatches: pal.accents.iter().copied().map(palette::to_hex).collect(),
        };
        self.cfg.shell.palette = theme.name.clone();
        self.cfg.shell.custom_themes.push(theme);
        self.cfg.save();
        self.audio.play(Sfx::Theme);
    }

    fn matugen(&mut self) {
        let seed = sample_wallpaper(&self.cfg.shell.wallpaper).unwrap_or(self.palette().accents[0]);
        let tones = palette::from_seed(seed);
        let theme = UserTheme {
            name: "обои".into(),
            swatches: tones.iter().copied().map(palette::to_hex).collect(),
        };
        self.cfg.shell.custom_themes.retain(|t| t.name != "обои");
        self.cfg.shell.palette = theme.name.clone();
        self.cfg.shell.custom_themes.insert(0, theme);
        self.cfg.theme = ThemeId::Custom;
        self.cfg.custom_hue = palette::accent_hue(seed);
        self.cfg.save();
        self.audio.play(Sfx::Theme);
    }

    fn type_key(&mut self, key: &Key) {
        let field = self.field;
        let buf = match field {
            Field::Search => &mut self.query,
            Field::Wallpaper => &mut self.cfg.shell.wallpaper,
            Field::Gtk => &mut self.cfg.shell.gtk_theme,
            Field::ShotDir => &mut self.cfg.shell.screenshot_dir,
            Field::None => return,
        };
        match key {
            Key::Named(NamedKey::Backspace) => {
                buf.pop();
            }
            Key::Character(s) => {
                if s.chars().count() == 1 && !s.chars().any(|c| c.is_control()) {
                    buf.push_str(s);
                }
            }
            _ => return,
        }
        if !matches!(field, Field::Search) {
            self.cfg.save();
        }
    }

    fn icon(&mut self, name: &str) -> Option<&Icon> {
        if name.is_empty() {
            return None;
        }
        if !self.icons.contains_key(name) {
            let icon = desktop::find_icon(name).and_then(|path| load_png(&path));
            if let Some(icon) = icon {
                self.icons.insert(name.to_string(), icon);
            } else {
                return None;
            }
        }
        self.icons.get(name)
    }
}

#[derive(Clone, Copy)]
enum ViewKind {
    Dock,
    Launcher,
    Settings,
}

impl Shell {
    fn paint(&mut self, kind: ViewKind, pal: &Palette) {
        let visible = match kind {
            ViewKind::Dock => true,
            ViewKind::Launcher => self.show_launcher,
            ViewKind::Settings => self.show_settings,
        };
        let prepared = {
            let view = match kind {
                ViewKind::Dock => self.dock.as_mut(),
                ViewKind::Launcher => self.launcher.as_mut(),
                ViewKind::Settings => self.settings.as_mut(),
            };
            let Some(view) = view else { return };
            view.window.set_visible(visible);
            if !visible {
                return;
            }
            Some((view.window.inner_size(), view.canvas.take()))
        };
        let Some((size, Some(mut canvas))) = prepared else { return };
        canvas.resize(size.width, size.height);
        let hits = match kind {
            ViewKind::Dock => draw_dock(&mut canvas, self, pal),
            ViewKind::Launcher => draw_launcher(&mut canvas, self, pal),
            ViewKind::Settings => draw_settings(&mut canvas, self, pal),
        };
        match kind {
            ViewKind::Dock => self.dock_hits = hits,
            ViewKind::Launcher => self.launcher_hits = hits,
            ViewKind::Settings => self.settings_hits = hits,
        }
        let view = match kind {
            ViewKind::Dock => self.dock.as_mut(),
            ViewKind::Launcher => self.launcher.as_mut(),
            ViewKind::Settings => self.settings.as_mut(),
        };
        if let Some(view) = view {
            view.canvas = Some(canvas);
            present(view);
        }
    }
}

fn present(view: &mut View) {
    if view.failed {
        return;
    }
    let Some(canvas) = view.canvas.as_ref() else { return };
    let pixels = canvas.pixels().to_vec();
    let (w, h) = (canvas.w, canvas.h);
    let Some(gpu) = view.gpu.as_mut() else { return };
    if let Err(err) = gpu.present(&pixels, w, h) {
        log::error!("present: {err}");
        view.failed = true;
    }
}

fn draw_dock(c: &mut Canvas, shell: &Shell, pal: &Palette) -> Vec<Hit> {
    let w = c.w as f32;
    let h = c.h as f32;
    let s = shell.scale;
    c.clear(Rgba::new(0, 0, 0, 0));
    let radius = shell.cfg.shell.corner_radius * s;
    let alpha = (shell.cfg.shell.opacity * 255.0) as u8;
    let bg = if shell.cfg.shell.liquid_glass {
        pal.surface.mix(Rgba::new(255, 255, 255, 255), 0.08).alpha(alpha.saturating_sub(20))
    } else {
        pal.bg.alpha(alpha)
    };
    c.fill_round(6.0 * s, 8.0 * s, w - 12.0 * s, h - 16.0 * s, radius, bg);
    c.stroke_round(6.0 * s, 8.0 * s, w - 12.0 * s, h - 16.0 * s, radius, pal.accents[0].alpha(140), 1.0);
    if shell.cfg.shell.liquid_glass {
        c.line(16.0 * s, 18.0 * s, w - 16.0 * s, 18.0 * s, Rgba::new(255, 255, 255, 70), 1.0);
    }
    let mut hits = Vec::new();
    let mut y = 22.0 * s;
    let btn = 48.0 * s;
    let x = (w - btn) * 0.5;
    icon_button(c, &mut hits, x, y, btn, pal, "A", Act::Overview, 1, shell);
    y += btn + 14.0 * s;
    let snap = shell.hypr.lock().ok().map(|g| g.clone()).unwrap_or_default();
    for id in &snap.workspaces {
        let rect = UiRect { x, y, w: btn, h: 22.0 * s };
        let on = *id == snap.active_workspace;
        c.fill_round(x, y, btn, 22.0 * s, 6.0 * s, if on { pal.accents[0].alpha(180) } else { pal.surface.alpha(160) });
        c.text(x + 18.0 * s, y + 3.0 * s, 12.0 * s, pal.text, &id.to_string());
        hits.push(Hit { rect, id: 100 + *id as u64, act: Act::Workspace(*id) });
        y += 28.0 * s;
    }
    y += 8.0 * s;
    for id in &shell.cfg.shell.pinned {
        let Some(idx) = shell.apps.iter().position(|a| &a.id == id) else { continue };
        let app = &shell.apps[idx];
        let running = snap.clients.iter().find(|cl| class_match(&cl.class, app));
        let mark = if running.is_some() { pal.accents[1] } else { pal.muted.alpha(0) };
        y = dock_app(c, &mut hits, x, y, btn, s, pal, app, Act::Launch(idx), mark, shell);
        if let Some(client) = running {
            if let Some(hit) = hits.last_mut() {
                hit.act = Act::Focus(client.address.clone());
            }
        }
    }
    for client in &snap.clients {
        if shell.apps.iter().any(|a| shell.cfg.shell.pinned.iter().any(|p| p == &a.id) && class_match(&client.class, a)) {
            continue;
        }
        let letter = client.class.chars().next().unwrap_or('?').to_string();
        c.fill_round(x, y, btn, btn, 12.0 * s, pal.surface);
        c.stroke_round(x, y, btn, btn, 12.0 * s, pal.accents[0].alpha(120), 1.0);
        c.text(x + 16.0 * s, y + 14.0 * s, 16.0 * s, pal.text, &letter);
        c.fill_round(x + btn * 0.5 - 3.0 * s, y + btn + 3.0 * s, 6.0 * s, 6.0 * s, 3.0 * s, pal.accents[1]);
        hits.push(Hit {
            rect: UiRect { x, y, w: btn, h: btn + 10.0 * s },
            id: 500 + hits.len() as u64,
            act: Act::Focus(client.address.clone()),
        });
        y += btn + 16.0 * s;
        if y > h - 160.0 * s {
            break;
        }
    }
    let bottom = h - 22.0 * s - btn;
    icon_button(c, &mut hits, x, bottom - btn - 12.0 * s, btn, pal, "3D", Act::OpenCenter, 2, shell);
    icon_button(c, &mut hits, x, bottom, btn, pal, "S", Act::OpenSettings, 3, shell);
    if shell.cfg.shell.panel_clock {
        let clock = chrono::Local::now().format("%H:%M").to_string();
        c.text(x - 2.0 * s, bottom - btn - 36.0 * s, 11.0 * s, pal.muted, &clock);
    }
    hits
}

fn dock_app(c: &mut Canvas, hits: &mut Vec<Hit>, x: f32, y: f32, btn: f32, s: f32, pal: &Palette, app: &AppEntry, act: Act, dot: Rgba, shell: &Shell) -> f32 {
    c.fill_round(x, y, btn, btn, 12.0 * s, pal.surface);
    c.stroke_round(x, y, btn, btn, 12.0 * s, pal.accents[0].alpha(110), 1.0);
    let letter: String = app.name.chars().take(1).collect();
    if let Some(icon) = load_cached(shell, &app.icon) {
        c.blit_rgba((x + 8.0 * s) as i32, (y + 8.0 * s) as i32, (btn - 16.0 * s) as u32, (btn - 16.0 * s) as u32, &icon.rgba, icon.w, icon.h);
    } else {
        c.text(x + 16.0 * s, y + 14.0 * s, 16.0 * s, pal.text, &letter);
    }
    if dot.a > 0 {
        c.fill_round(x + btn * 0.5 - 3.0 * s, y + btn + 3.0 * s, 6.0 * s, 6.0 * s, 3.0 * s, dot);
    }
    hits.push(Hit { rect: UiRect { x, y, w: btn, h: btn + 8.0 * s }, id: 200 + hits.len() as u64, act });
    y + btn + 16.0 * s
}

fn load_cached(shell: &Shell, name: &str) -> Option<Icon> {
    shell.icons.get(name).cloned()
}

fn icon_button(c: &mut Canvas, hits: &mut Vec<Hit>, x: f32, y: f32, btn: f32, pal: &Palette, glyph: &str, act: Act, id: u64, shell: &Shell) {
    let hot = shell.hover_id == id;
    let grow = if hot { 2.0 * shell.hover_t * shell.scale } else { 0.0 };
    c.fill_round(x - grow, y - grow, btn + grow * 2.0, btn + grow * 2.0, 14.0 * shell.scale, pal.surface);
    c.stroke_round(x - grow, y - grow, btn + grow * 2.0, btn + grow * 2.0, 14.0 * shell.scale, pal.accents[0].alpha(if hot { 220 } else { 120 }), 1.0);
    c.text(x + 14.0 * shell.scale, y + 14.0 * shell.scale, 16.0 * shell.scale, pal.accents[0], glyph);
    hits.push(Hit { rect: UiRect { x, y, w: btn, h: btn }, id, act });
}

fn draw_launcher(c: &mut Canvas, shell: &Shell, pal: &Palette) -> Vec<Hit> {
    let w = c.w as f32;
    let h = c.h as f32;
    let s = shell.scale;
    c.clear(pal.bg.alpha(235));
    let r = shell.cfg.shell.corner_radius * s;
    c.stroke_round(8.0 * s, 8.0 * s, w - 16.0 * s, h - 16.0 * s, r, pal.accents[0].alpha(120), 1.0);
    c.text(28.0 * s, 22.0 * s, 18.0 * s, pal.text, "Приложения");
    c.text(220.0 * s, 26.0 * s, 12.0 * s, pal.muted, "поиск, запуск, закрепление на доке");
    let mut hits = Vec::new();
    let box_r = UiRect { x: 28.0 * s, y: 58.0 * s, w: w - 56.0 * s, h: 36.0 * s };
    c.fill_round(box_r.x, box_r.y, box_r.w, box_r.h, 8.0 * s, pal.surface);
    c.stroke_round(box_r.x, box_r.y, box_r.w, box_r.h, 8.0 * s, pal.accents[0].alpha(100), 1.0);
    let shown = if shell.query.is_empty() { "Поиск приложений…".into() } else { shell.query.clone() };
    c.text(box_r.x + 12.0 * s, box_r.y + 8.0 * s, 14.0 * s, if shell.query.is_empty() { pal.muted } else { pal.text }, &shown);
    hits.push(Hit { rect: box_r, id: 9, act: Act::Search });
    let snap = shell.hypr.lock().ok().map(|g| g.clone()).unwrap_or_default();
    if !snap.workspaces.is_empty() {
        let mut x = 28.0 * s;
        for id in snap.workspaces.iter().take(8) {
            let rect = UiRect { x, y: 104.0 * s, w: 36.0 * s, h: 22.0 * s };
            let on = *id == snap.active_workspace;
            c.fill_round(x, rect.y, rect.w, rect.h, 6.0 * s, if on { pal.accents[0] } else { pal.surface });
            c.text(x + 12.0 * s, rect.y + 2.0 * s, 12.0 * s, pal.text, &id.to_string());
            hits.push(Hit { rect, id: 300 + *id as u64, act: Act::Workspace(*id) });
            x += 44.0 * s;
        }
    }
    let q = shell.query.to_lowercase();
    let matched: Vec<usize> = shell
        .apps
        .iter()
        .enumerate()
        .filter(|(_, a)| q.is_empty() || a.name.to_lowercase().contains(&q) || a.id.to_lowercase().contains(&q))
        .map(|(i, _)| i)
        .take(24)
        .collect();
    let cell_w = 132.0 * s;
    let cell_h = 108.0 * s;
    let cols = ((w - 40.0 * s) / cell_w).floor().max(1.0) as usize;
    for (n, idx) in matched.iter().copied().enumerate() {
        let col = n % cols;
        let row = n / cols;
        let x = 24.0 * s + col as f32 * cell_w;
        let y = 140.0 * s + row as f32 * cell_h;
        if y > h - 20.0 * s {
            break;
        }
        let app = &shell.apps[idx];
        c.fill_round(x, y, cell_w - 10.0 * s, cell_h - 12.0 * s, 12.0 * s, pal.surface.alpha(230));
        c.stroke_round(x, y, cell_w - 10.0 * s, cell_h - 12.0 * s, 12.0 * s, pal.accents[0].alpha(80), 1.0);
        if let Some(icon) = load_cached(shell, &app.icon) {
            c.blit_rgba((x + 40.0 * s) as i32, (y + 12.0 * s) as i32, (36.0 * s) as u32, (36.0 * s) as u32, &icon.rgba, icon.w, icon.h);
        } else {
            let letter: String = app.name.chars().take(1).collect();
            c.text(x + 48.0 * s, y + 16.0 * s, 20.0 * s, pal.accents[0], &letter);
        }
        c.text_fit(x + 8.0 * s, y + 56.0 * s, 12.0 * s, pal.text, &app.name, cell_w - 24.0 * s);
        let pinned = shell.cfg.shell.pinned.iter().any(|p| p == &app.id);
        c.text(x + 8.0 * s, y + 74.0 * s, 10.0 * s, if pinned { pal.accents[1] } else { pal.muted }, if pinned { "на доке" } else { "закрепить" });
        hits.push(Hit {
            rect: UiRect { x, y, w: cell_w - 10.0 * s, h: 70.0 * s },
            id: 1000 + idx as u64,
            act: Act::Launch(idx),
        });
        hits.push(Hit {
            rect: UiRect { x, y: y + 70.0 * s, w: cell_w - 10.0 * s, h: 24.0 * s },
            id: 2000 + idx as u64,
            act: Act::Pin(idx),
        });
    }
    if matched.is_empty() {
        c.text(28.0 * s, 160.0 * s, 14.0 * s, pal.muted, "Ничего не найдено в каталогах .desktop");
    }
    hits
}

fn draw_settings(c: &mut Canvas, shell: &Shell, pal: &Palette) -> Vec<Hit> {
    let w = c.w as f32;
    let h = c.h as f32;
    let s = shell.scale;
    c.clear(pal.bg);
    let nav_w = 230.0 * s;
    c.fill_rect(0.0, 0.0, nav_w, h, pal.surface.alpha(255));
    c.text(18.0 * s, 16.0 * s, 16.0 * s, pal.text, "Настройки");
    let mut hits = Vec::new();
    let items = [
        (Page::Welcome, "Добро пожаловать", false),
        (Page::General, "Общие", false),
        (Page::Screen, "Экран", false),
        (Page::Appearance, "Тема", false),
        (Page::Appearance, "Оформление", true),
        (Page::Wallpaper, "Обои", true),
        (Page::Apps, "Темы приложений", true),
        (Page::Panel, "Панель", false),
        (Page::Dock, "Док", false),
        (Page::Launcher, "Лаунчер", false),
        (Page::Osd, "OSD", false),
        (Page::Notes, "Уведомления", false),
        (Page::Shot, "Скриншот", false),
        (Page::Wellbeing, "Благополучие", false),
    ];
    let mut y = 52.0 * s;
    for (page, label, child) in items {
        let x = if child { 28.0 * s } else { 12.0 * s };
        let header = label == "Тема";
        let on = shell.page == page && !header;
        if on {
            c.fill_round(8.0 * s, y - 2.0 * s, nav_w - 16.0 * s, 26.0 * s, 8.0 * s, pal.accents[0].alpha(50));
        }
        c.text(x, y, 13.0 * s, if on { pal.text } else { pal.muted }, label);
        hits.push(Hit {
            rect: UiRect { x: 8.0 * s, y: y - 2.0 * s, w: nav_w - 16.0 * s, h: 26.0 * s },
            id: 4000 + hits.len() as u64,
            act: Act::Page(page),
        });
        y += 30.0 * s;
    }
    let cx = nav_w + 18.0 * s;
    let cw = w - cx - 18.0 * s;
    match shell.page {
        Page::Appearance => appearance(c, shell, pal, &mut hits, cx, cw, s),
        Page::Welcome => welcome(c, pal, &mut hits, cx, s),
        Page::General => general(c, shell, pal, &mut hits, cx, cw, s),
        Page::Screen => screen_page(c, shell, pal, &mut hits, cx, cw, s),
        Page::Wallpaper => wallpaper_page(c, shell, pal, &mut hits, cx, cw, s),
        Page::Apps => apps_page(c, shell, pal, &mut hits, cx, cw, s),
        Page::Panel => toggle_page(c, pal, &mut hits, cx, s, "Панель", "Часы на доке", shell.cfg.shell.panel_clock, "clock"),
        Page::Dock => toggle_page(c, pal, &mut hits, cx, s, "Док", "Прятать, пока курсор не рядом", shell.cfg.shell.dock_autohide, "hide"),
        Page::Launcher => {
            c.text(cx, 24.0 * s, 20.0 * s, pal.text, "Лаунчер");
            c.text(cx, 58.0 * s, 13.0 * s, pal.muted, "Сетка .desktop-файлов из XDG. Клик запускает, «закрепить» добавляет на док.");
            c.text(cx, 90.0 * s, 13.0 * s, pal.text, &format!("Найдено приложений: {}", shell.apps.len()));
        }
        Page::Osd => toggle_page(c, pal, &mut hits, cx, s, "OSD", "Всплывающие подсказки громкости и яркости", shell.cfg.shell.osd, "osd"),
        Page::Notes => toggle_page(c, pal, &mut hits, cx, s, "Уведомления", "Показывать уведомления оболочки", shell.cfg.shell.notifications, "notes"),
        Page::Shot => {
            c.text(cx, 24.0 * s, 20.0 * s, pal.text, "Скриншот");
            c.text(cx, 58.0 * s, 13.0 * s, pal.muted, "Каталог для снимков экрана");
            field(c, pal, &mut hits, cx, 90.0 * s, cw, s, &shell.cfg.shell.screenshot_dir, Field::ShotDir);
        }
        Page::Wellbeing => {
            c.text(cx, 24.0 * s, 20.0 * s, pal.text, "Благополучие");
            c.text(cx, 58.0 * s, 13.0 * s, pal.muted, "Напоминание встать из-за стола");
            stepper(c, pal, &mut hits, cx, 100.0 * s, s, &format!("{} мин", shell.cfg.shell.wellbeing_break_min), Act::Break(-5), Act::Break(5));
        }
    }
    hits
}

fn welcome(c: &mut Canvas, pal: &Palette, hits: &mut Vec<Hit>, x: f32, s: f32) {
    c.text(x, 24.0 * s, 22.0 * s, pal.text, "Arch Control Center");
    c.text(x, 64.0 * s, 14.0 * s, pal.muted, "Оболочка поверх Hyprland: док, лаунчер и этот экран настроек.");
    c.text(x, 92.0 * s, 14.0 * s, pal.muted, "Трёхмерный центр управления — одна из частей оболочки, кнопка ◉ на доке.");
    c.text(x, 120.0 * s, 14.0 * s, pal.muted, "Без Hyprland док всё равно запускает приложения из .desktop.");
    let rect = UiRect { x, y: 170.0 * s, w: 280.0 * s, h: 36.0 * s };
    c.fill_round(rect.x, rect.y, rect.w, rect.h, 8.0 * s, pal.accents[0].alpha(180));
    c.text(rect.x + 18.0 * s, rect.y + 8.0 * s, 14.0 * s, pal.text, "Открыть центр управления");
    hits.push(Hit { rect, id: 7, act: Act::OpenCenter });
}

fn general(c: &mut Canvas, shell: &Shell, pal: &Palette, hits: &mut Vec<Hit>, x: f32, w: f32, s: f32) {
    c.text(x, 24.0 * s, 20.0 * s, pal.text, "Общие");
    row(c, pal, x, 70.0 * s, w, s, "Звук интерфейса", "Тики и перезвоны оболочки");
    toggle(c, pal, hits, x + w - 54.0 * s, 78.0 * s, s, shell.cfg.sound, Act::Flag("sound"));
    c.text(x, 150.0 * s, 13.0 * s, pal.muted, "Hyprland: exec-once запускает --shell. Повторный --shell открывает лаунчер или настройки.");
}

fn screen_page(c: &mut Canvas, shell: &Shell, pal: &Palette, hits: &mut Vec<Hit>, x: f32, w: f32, s: f32) {
    c.text(x, 24.0 * s, 20.0 * s, pal.text, "Экран");
    row(c, pal, x, 70.0 * s, w, s, "Непрозрачность элементов", "Панели дока и окон оболочки");
    slider(c, pal, hits, x + w - 220.0 * s, 84.0 * s, 200.0 * s, s, shell.cfg.shell.opacity, Act::Opacity);
    c.text(x, 140.0 * s, 13.0 * s, pal.muted, "Масштаб окна берётся из scale_factor монитора и множителя ui_scale в config.toml.");
}

fn appearance(c: &mut Canvas, shell: &Shell, pal: &Palette, hits: &mut Vec<Hit>, x: f32, w: f32, s: f32) {
    c.text(x, 18.0 * s, 20.0 * s, pal.text, "Оформление");
    let mut y = 58.0 * s - shell.scroll;
    row(c, pal, x, y, w, s, "Шрифт", "Шрифт подписей оболочки");
    stepper(c, pal, hits, x + w - 260.0 * s, y + 8.0 * s, s, &shell.cfg.shell.font_name, Act::Font(-1), Act::Font(1));
    y += 62.0 * s;
    row(c, pal, x, y, w, s, "Радиус скругления", "Углы дока, карточек и кнопок");
    stepper(c, pal, hits, x + w - 180.0 * s, y + 8.0 * s, s, &format!("{}px", shell.cfg.shell.corner_radius.round() as i32), Act::Radius(-1), Act::Radius(1));
    y += 62.0 * s;
    row(c, pal, x, y, w, s, "Непрозрачность элементов", "Док, лаунчер и карточки");
    slider(c, pal, hits, x + w - 220.0 * s, y + 16.0 * s, 200.0 * s, s, shell.cfg.shell.opacity, Act::Opacity);
    y += 62.0 * s;
    row(c, pal, x, y, w, s, "Размытый фон из обоев", "Матовый тон или более светлое стекло");
    let seg_x = x + w - 250.0 * s;
    seg(c, pal, hits, seg_x, y + 10.0 * s, s, "Размытие", !shell.cfg.shell.liquid_glass, Act::Glass(false));
    seg(c, pal, hits, seg_x + 108.0 * s, y + 10.0 * s, s, "Жидкое стекло", shell.cfg.shell.liquid_glass, Act::Glass(true));
    toggle(c, pal, hits, x + w - 46.0 * s, y + 12.0 * s, s, shell.cfg.shell.wallpaper_blur, Act::BlurToggle);
    y += 78.0 * s;
    c.text(x, y, 16.0 * s, pal.text, "Цветовые темы");
    c.text(x, y + 24.0 * s, 12.0 * s, pal.muted, "Готовый пресет или своя палитра. Центр управления получает тот же акцент.");
    let search = UiRect { x: x + w - 220.0 * s, y: y, w: 210.0 * s, h: 28.0 * s };
    c.fill_round(search.x, search.y, search.w, search.h, 8.0 * s, pal.surface);
    c.stroke_round(search.x, search.y, search.w, search.h, 8.0 * s, pal.accents[0].alpha(90), 1.0);
    let label = if shell.query.is_empty() { "Поиск тем…".into() } else { shell.query.clone() };
    c.text(search.x + 10.0 * s, search.y + 5.0 * s, 12.0 * s, if shell.query.is_empty() { pal.muted } else { pal.text }, &label);
    hits.push(Hit { rect: search, id: 11, act: Act::Search });
    y += 58.0 * s;
    let plus = UiRect { x, y, w: 36.0 * s, h: 28.0 * s };
    c.fill_round(plus.x, plus.y, plus.w, plus.h, 8.0 * s, pal.surface);
    c.text(plus.x + 12.0 * s, plus.y + 4.0 * s, 16.0 * s, pal.text, "+");
    hits.push(Hit { rect: plus, id: 12, act: Act::AddTheme });
    c.text(x + 46.0 * s, y + 4.0 * s, 13.0 * s, pal.muted, "Новая тема из текущих цветов");
    y += 40.0 * s;
    for (i, theme) in shell.cfg.shell.custom_themes.iter().enumerate() {
        if !shell.query.is_empty() && !theme.name.to_lowercase().contains(&shell.query.to_lowercase()) {
            continue;
        }
        theme_card(c, pal, hits, x, y, w, s, &theme.name, &theme.swatches, shell.cfg.shell.palette == theme.name, Some(i));
        y += 58.0 * s;
    }
    let mat = UiRect { x, y, w, h: 48.0 * s };
    c.fill_round(mat.x, mat.y, mat.w, mat.h, 10.0 * s, pal.surface);
    c.stroke_round(mat.x, mat.y, mat.w, mat.h, 10.0 * s, pal.accents[1].alpha(140), 1.0);
    c.text(x + 14.0 * s, y + 6.0 * s, 14.0 * s, pal.text, "Собрать палитру из обоев");
    c.text(x + 14.0 * s, y + 26.0 * s, 11.0 * s, pal.muted, "Как Matugen: один цвет-семя и тёмная шкала. Без чужого кода.");
    hits.push(Hit { rect: mat, id: 13, act: Act::Matugen });
    y += 62.0 * s;
    let q = shell.query.to_lowercase();
    let cols = ((w / (280.0 * s)).floor() as usize).clamp(1, 3);
    let card_w = (w - (cols as f32 - 1.0) * 12.0 * s) / cols as f32;
    let mut n = 0;
    for preset in palette::presets() {
        if !q.is_empty() && !preset.name.to_lowercase().contains(&q) && !preset.id.contains(&q) {
            continue;
        }
        let col = n % cols;
        let row = n / cols;
        let cx = x + col as f32 * (card_w + 12.0 * s);
        let cy = y + row as f32 * 58.0 * s;
        let swatches: Vec<String> = preset.accents.iter().copied().map(palette::to_hex).collect();
        theme_card(c, pal, hits, cx, cy, card_w, s, preset.name, &swatches, shell.cfg.shell.palette == preset.id, None);
        if let Some(hit) = hits.last_mut() {
            if matches!(hit.act, Act::Palette(_)) {
                hit.act = Act::Palette(preset.id.to_string());
            }
        }
        n += 1;
    }
}

fn wallpaper_page(c: &mut Canvas, shell: &Shell, pal: &Palette, hits: &mut Vec<Hit>, x: f32, w: f32, s: f32) {
    c.text(x, 24.0 * s, 20.0 * s, pal.text, "Обои");
    c.text(x, 58.0 * s, 13.0 * s, pal.muted, "Путь к PNG. Палитра «из обоев» берёт средний насыщенный цвет.");
    field(c, pal, hits, x, 96.0 * s, w, s, if shell.cfg.shell.wallpaper.is_empty() { "~/.config/arch-control-center/wallpaper.png" } else { &shell.cfg.shell.wallpaper }, Field::Wallpaper);
    c.text(x, 160.0 * s, 13.0 * s, pal.muted, "Размытие самого рабочего стола делает Hyprland (decoration:blur). Здесь — режим стекла оболочки.");
}

fn apps_page(c: &mut Canvas, shell: &Shell, pal: &Palette, hits: &mut Vec<Hit>, x: f32, w: f32, s: f32) {
    c.text(x, 24.0 * s, 20.0 * s, pal.text, "Темы приложений");
    c.text(x, 58.0 * s, 13.0 * s, pal.muted, "Имя GTK-темы записывается в конфиг. Оболочка его не навязывает чужим программам.");
    field(c, pal, hits, x, 96.0 * s, w, s, if shell.cfg.shell.gtk_theme.is_empty() { "Adwaita-dark" } else { &shell.cfg.shell.gtk_theme }, Field::Gtk);
}

fn toggle_page(c: &mut Canvas, pal: &Palette, hits: &mut Vec<Hit>, x: f32, s: f32, title: &str, subtitle: &str, on: bool, flag: &'static str) {
    c.text(x, 24.0 * s, 20.0 * s, pal.text, title);
    c.text(x, 64.0 * s, 13.0 * s, pal.muted, subtitle);
    toggle(c, pal, hits, x, 100.0 * s, s, on, Act::Flag(flag));
}

fn row(c: &mut Canvas, pal: &Palette, x: f32, y: f32, w: f32, s: f32, title: &str, sub: &str) {
    c.text(x, y, 14.0 * s, pal.text, title);
    c.text(x, y + 20.0 * s, 11.0 * s, pal.muted, sub);
    c.line(x, y + 46.0 * s, x + w, y + 46.0 * s, pal.muted.alpha(50), 1.0);
}

fn stepper(c: &mut Canvas, pal: &Palette, hits: &mut Vec<Hit>, x: f32, y: f32, s: f32, label: &str, dec: Act, inc: Act) {
    let h = 28.0 * s;
    let bw = 28.0 * s;
    let mw = 140.0 * s;
    c.fill_round(x, y, bw, h, 6.0 * s, pal.surface);
    c.text(x + 8.0 * s, y + 4.0 * s, 14.0 * s, pal.text, "<");
    hits.push(Hit { rect: UiRect { x, y, w: bw, h }, id: hits.len() as u64 + 20, act: dec });
    c.fill_round(x + bw + 4.0 * s, y, mw, h, 6.0 * s, pal.surface);
    c.text_fit(x + bw + 10.0 * s, y + 5.0 * s, 12.0 * s, pal.text, label, mw - 16.0 * s);
    c.fill_round(x + bw + mw + 8.0 * s, y, bw, h, 6.0 * s, pal.surface);
    c.text(x + bw + mw + 16.0 * s, y + 4.0 * s, 14.0 * s, pal.text, ">");
    hits.push(Hit { rect: UiRect { x: x + bw + mw + 8.0 * s, y, w: bw, h }, id: hits.len() as u64 + 21, act: inc });
}

fn slider(c: &mut Canvas, pal: &Palette, hits: &mut Vec<Hit>, x: f32, y: f32, w: f32, s: f32, value: f32, act: Act) {
    c.fill_round(x, y, w, 8.0 * s, 4.0 * s, pal.surface);
    c.fill_round(x, y, w * value.clamp(0.0, 1.0), 8.0 * s, 4.0 * s, pal.accents[0]);
    let kx = x + w * value.clamp(0.0, 1.0) - 6.0 * s;
    c.fill_round(kx, y - 4.0 * s, 12.0 * s, 16.0 * s, 6.0 * s, pal.text);
    hits.push(Hit { rect: UiRect { x, y: y - 8.0 * s, w, h: 24.0 * s }, id: 30, act });
}

fn seg(c: &mut Canvas, pal: &Palette, hits: &mut Vec<Hit>, x: f32, y: f32, s: f32, label: &str, on: bool, act: Act) {
    let w = 100.0 * s;
    let h = 26.0 * s;
    c.fill_round(x, y, w, h, 8.0 * s, if on { pal.accents[0].alpha(200) } else { pal.surface });
    c.text_fit(x + 8.0 * s, y + 4.0 * s, 11.0 * s, pal.text, label, w - 12.0 * s);
    hits.push(Hit { rect: UiRect { x, y, w, h }, id: hits.len() as u64 + 40, act });
}

fn toggle(c: &mut Canvas, pal: &Palette, hits: &mut Vec<Hit>, x: f32, y: f32, s: f32, on: bool, act: Act) {
    let w = 42.0 * s;
    let h = 22.0 * s;
    c.fill_round(x, y, w, h, h * 0.5, if on { pal.accents[0] } else { pal.surface });
    let kx = if on { x + w - h + 2.0 * s } else { x + 2.0 * s };
    c.fill_round(kx, y + 2.0 * s, h - 4.0 * s, h - 4.0 * s, h * 0.4, pal.text);
    hits.push(Hit { rect: UiRect { x, y, w, h }, id: hits.len() as u64 + 50, act });
}

fn field(c: &mut Canvas, pal: &Palette, hits: &mut Vec<Hit>, x: f32, y: f32, w: f32, s: f32, text: &str, field: Field) {
    c.fill_round(x, y, w, 34.0 * s, 8.0 * s, pal.surface);
    c.stroke_round(x, y, w, 34.0 * s, 8.0 * s, pal.accents[0].alpha(90), 1.0);
    c.text_fit(x + 10.0 * s, y + 8.0 * s, 13.0 * s, pal.text, text, w - 20.0 * s);
    hits.push(Hit { rect: UiRect { x, y, w, h: 34.0 * s }, id: hits.len() as u64 + 60, act: Act::Field(field) });
}

fn theme_card(c: &mut Canvas, pal: &Palette, hits: &mut Vec<Hit>, x: f32, y: f32, w: f32, s: f32, name: &str, swatches: &[String], on: bool, delete: Option<usize>) {
    c.fill_round(x, y, w, 48.0 * s, 10.0 * s, pal.surface);
    c.stroke_round(x, y, w, 48.0 * s, 10.0 * s, if on { pal.accents[0] } else { pal.muted.alpha(70) }, 1.0);
    c.text_fit(x + 12.0 * s, y + 14.0 * s, 13.0 * s, pal.text, name, w * 0.45);
    let mut dx = x + w - 16.0 * s - swatches.len() as f32 * 16.0 * s;
    if delete.is_some() {
        dx -= 28.0 * s;
    }
    for (i, hex) in swatches.iter().take(5).enumerate() {
        let col = palette::parse_hex(hex).unwrap_or(pal.accents[i % 5]);
        c.fill_round(dx, y + 16.0 * s, 12.0 * s, 12.0 * s, 6.0 * s, col);
        dx += 16.0 * s;
    }
    hits.push(Hit {
        rect: UiRect { x, y, w: w - if delete.is_some() { 36.0 * s } else { 0.0 }, h: 48.0 * s },
        id: hits.len() as u64 + 70,
        act: Act::Palette(name.to_string()),
    });
    if let Some(i) = delete {
        let rect = UiRect { x: x + w - 32.0 * s, y: y + 12.0 * s, w: 22.0 * s, h: 22.0 * s };
        c.text(rect.x + 4.0 * s, rect.y + 2.0 * s, 12.0 * s, pal.muted, "x");
        hits.push(Hit { rect, id: 8000 + i as u64, act: Act::DelTheme(i) });
    }
}

fn class_match(class: &str, app: &AppEntry) -> bool {
    let class = class.to_lowercase();
    class == app.wm_class.to_lowercase() || class == app.id.to_lowercase() || class == app.name.to_lowercase()
}

fn open_center() {
    let Ok(exe) = std::env::current_exe() else { return };
    let _ = Command::new(exe).arg("--center").stdin(std::process::Stdio::null()).spawn();
}

fn custom_palette(theme: &UserTheme) -> Option<Palette> {
    let mut accents = [Rgba::new(255, 255, 255, 255); 5];
    for (i, hex) in theme.swatches.iter().take(5).enumerate() {
        accents[i] = palette::parse_hex(hex)?;
    }
    let bg = accents[0].mix(Rgba::new(0, 0, 0, 255), 0.82);
    Some(Palette {
        id: "custom",
        name: "custom",
        bg,
        surface: bg.mix(Rgba::new(255, 255, 255, 255), 0.06),
        text: Rgba::new(236, 236, 242, 255),
        muted: Rgba::new(140, 140, 150, 255),
        accents,
    })
}

const PRESET_FALLBACK: Palette = Palette {
    id: "cyberpunk",
    name: "Cyberpunk",
    bg: Rgba::new(13, 2, 33, 255),
    surface: Rgba::new(26, 5, 51, 255),
    text: Rgba::new(209, 247, 255, 255),
    muted: Rgba::new(107, 76, 122, 255),
    accents: [
        Rgba::new(255, 42, 109, 255),
        Rgba::new(5, 217, 232, 255),
        Rgba::new(247, 6, 207, 255),
        Rgba::new(209, 247, 255, 255),
        Rgba::new(119, 0, 166, 255),
    ],
};

fn sample_wallpaper(path: &str) -> Option<Rgba> {
    if path.is_empty() {
        return None;
    }
    let path = if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home).join(rest)
    } else {
        PathBuf::from(path)
    };
    let file = std::fs::File::open(path).ok()?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let bytes = &buf[..info.buffer_size()];
    let step = (info.width as usize * info.height as usize / 400).max(1);
    let mut acc = [0u64; 3];
    let mut n = 0u64;
    let channels = info.color_type.samples();
    let mut i = 0;
    while i + 2 < bytes.len() {
        acc[0] += bytes[i] as u64;
        acc[1] += bytes[i + 1] as u64;
        acc[2] += bytes[i + 2] as u64;
        n += 1;
        i += channels.max(3) * step;
    }
    if n == 0 {
        return None;
    }
    Some(Rgba::new((acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, 255))
}

fn load_png(path: &Path) -> Option<Icon> {
    if path.extension().and_then(|e| e.to_str()) != Some("png") {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let src = &buf[..info.buffer_size()];
    let mut rgba = vec![0u8; (info.width * info.height * 4) as usize];
    let channels = info.color_type.samples();
    for i in 0..(info.width * info.height) as usize {
        let s = i * channels;
        if s >= src.len() {
            break;
        }
        rgba[i * 4] = src[s];
        rgba[i * 4 + 1] = if channels > 1 { src[s + 1] } else { src[s] };
        rgba[i * 4 + 2] = if channels > 2 { src[s + 2] } else { src[s] };
        rgba[i * 4 + 3] = if channels > 3 { src[s + 3] } else { 255 };
    }
    Some(Icon { w: info.width, h: info.height, rgba })
}

fn save_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    Ok(())
}

fn acquire(open: u8) -> Option<Arc<AtomicU8>> {
    let path = shell_socket();
    if let Ok(mut stream) = UnixStream::connect(&path) {
        let msg = if open == 0 { b"ping".to_vec() } else { vec![open] };
        let _ = stream.write_all(&msg);
        log::info!("shell already running");
        std::process::exit(0);
    }
    let _ = std::fs::remove_file(&path);
    let cmd = Arc::new(AtomicU8::new(open));
    match UnixListener::bind(&path) {
        Ok(listener) => {
            let flag = Arc::clone(&cmd);
            std::thread::Builder::new()
                .name("acc-shell".into())
                .spawn(move || {
                    for mut stream in listener.incoming().flatten() {
                        let mut buf = [0u8; 8];
                        let n = stream.read(&mut buf).unwrap_or(0);
                        if n == 0 || &buf[..n] == b"ping" {
                            flag.fetch_or(1, Ordering::SeqCst);
                        } else {
                            flag.fetch_or(buf[0], Ordering::SeqCst);
                        }
                    }
                })
                .ok();
        }
        Err(err) => log::warn!("shell socket: {err}"),
    }
    Some(cmd)
}

fn shell_socket() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("arch-shell.sock");
        }
    }
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    PathBuf::from(format!("/tmp/arch-shell-{user}.sock"))
}
