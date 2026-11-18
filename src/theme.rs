use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeId {
    Neon,
    Acid,
    Ice,
    Blood,
    Mono,
    Custom,
}

impl ThemeId {
    pub fn from_key(k: &str) -> Option<Self> {
        match k {
            "1" => Some(Self::Neon),
            "2" => Some(Self::Acid),
            "3" => Some(Self::Ice),
            "4" => Some(Self::Blood),
            "5" => Some(Self::Mono),
            "6" => Some(Self::Custom),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Neon => "NEON",
            Self::Acid => "ACID",
            Self::Ice => "ICE",
            Self::Blood => "BLOOD",
            Self::Mono => "MONO",
            Self::Custom => "HUE",
        }
    }

    pub fn all() -> [Self; 5] {
        [Self::Neon, Self::Acid, Self::Ice, Self::Blood, Self::Mono]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    pub fn mix(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Self {
            r: m(self.r, other.r),
            g: m(self.g, other.g),
            b: m(self.b, other.b),
            a: m(self.a, other.a),
        }
    }

    pub fn to_f32(self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ThemeColors {
    pub bg: Rgba,
    pub panel: Rgba,
    pub panel2: Rgba,
    pub line: Rgba,
    pub text: Rgba,
    pub dim: Rgba,
    pub primary: Rgba,
    pub secondary: Rgba,
    pub ok: Rgba,
    pub warn: Rgba,
}

impl ThemeColors {
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            bg: self.bg.mix(other.bg, t),
            panel: self.panel.mix(other.panel, t),
            panel2: self.panel2.mix(other.panel2, t),
            line: self.line.mix(other.line, t),
            text: self.text.mix(other.text, t),
            dim: self.dim.mix(other.dim, t),
            primary: self.primary.mix(other.primary, t),
            secondary: self.secondary.mix(other.secondary, t),
            ok: self.ok.mix(other.ok, t),
            warn: self.warn.mix(other.warn, t),
        }
    }
}

pub fn hsv(h: f32, s: f32, v: f32) -> Rgba {
    let h = h.rem_euclid(360.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Rgba::new(
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
        255,
    )
}

fn base(panel_tint: Rgba, primary: Rgba, secondary: Rgba) -> ThemeColors {
    ThemeColors {
        bg: Rgba::new(4, 6, 10, 255),
        panel: Rgba::new(
            10 + panel_tint.r / 18,
            12 + panel_tint.g / 18,
            18 + panel_tint.b / 18,
            255,
        ),
        panel2: Rgba::new(6, 8, 12, 255),
        line: Rgba::new(48, 64, 78, 255),
        text: Rgba::new(226, 244, 248, 255),
        dim: Rgba::new(96, 122, 136, 255),
        primary,
        secondary,
        ok: Rgba::new(64, 230, 170, 255),
        warn: Rgba::new(255, 176, 64, 255),
    }
}

pub fn colors(id: ThemeId, hue: f32) -> ThemeColors {
    match id {
        ThemeId::Neon => base(
            hsv(186.0, 0.8, 1.0),
            Rgba::new(0, 240, 255, 255),
            Rgba::new(255, 43, 214, 255),
        ),
        ThemeId::Acid => base(
            hsv(68.0, 0.8, 1.0),
            Rgba::new(230, 255, 61, 255),
            Rgba::new(61, 126, 255, 255),
        ),
        ThemeId::Ice => base(
            hsv(196.0, 0.35, 1.0),
            Rgba::new(186, 244, 255, 255),
            Rgba::new(232, 248, 255, 255),
        ),
        ThemeId::Blood => base(
            hsv(356.0, 0.8, 1.0),
            Rgba::new(255, 42, 58, 255),
            Rgba::new(255, 106, 61, 255),
        ),
        ThemeId::Mono => base(
            Rgba::new(180, 180, 180, 255),
            Rgba::new(232, 232, 232, 255),
            Rgba::new(154, 154, 154, 255),
        ),
        ThemeId::Custom => {
            let primary = hsv(hue, 0.82, 1.0);
            let secondary = hsv(hue + 148.0, 0.7, 1.0);
            base(primary, primary, secondary)
        }
    }
}

pub fn swatch(id: ThemeId) -> Rgba {
    colors(id, 186.0).primary
}
