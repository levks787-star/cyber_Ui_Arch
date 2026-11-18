use crate::theme::Rgba;

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub id: &'static str,
    pub name: &'static str,
    pub bg: Rgba,
    pub surface: Rgba,
    pub text: Rgba,
    pub muted: Rgba,
    pub accents: [Rgba; 5],
}

pub fn presets() -> &'static [Palette] {
    &PRESETS
}

pub fn by_id(id: &str) -> Option<&'static Palette> {
    PRESETS.iter().find(|p| p.id == id)
}

const PRESETS: [Palette; 16] = [
    pal("obsidian", "Obsidian", 0x1e1e1e, 0x2a2a2a, 0xe7e5e4, 0x44403c, [0x7c3aed, 0xa78bfa, 0xc4b5fd, 0xf5f5f4, 0x44403c]),
    pal("cyberpunk", "Cyberpunk", 0x0d0221, 0x1a0533, 0xd1f7ff, 0x6b4c7a, [0xff2a6d, 0x05d9e8, 0xf706cf, 0xd1f7ff, 0x7700a6]),
    pal("night-owl", "Night Owl", 0x011627, 0x0b2942, 0xd6deeb, 0x5f7e97, [0x82aaff, 0xc792ea, 0x7fdbca, 0xecc48d, 0xd6deeb]),
    pal("tokyo", "Tokyo", 0x1a1b26, 0x24283b, 0xc0caf5, 0x565f89, [0x7aa2f7, 0xbb9af7, 0x7dcfff, 0x9ece6a, 0xc0caf5]),
    pal("kanagawa", "Kanagawa", 0x1f1f28, 0x2a2a37, 0xdcd7ba, 0x727169, [0x957fb8, 0x7e9cd8, 0xd27e99, 0xffa066, 0xdcd7ba]),
    pal("espresso", "Espresso", 0x2d2016, 0x3c2a1e, 0xf0e6d2, 0x8a6a4f, [0xc4a484, 0xe6c7a0, 0xa67c52, 0xf0e6d2, 0x6b4f3a]),
    pal("solarized", "Solarized", 0x002b36, 0x073642, 0xeee8d5, 0x586e75, [0x268bd2, 0x2aa198, 0x859900, 0xb58900, 0xeee8d5]),
    pal("github", "GitHub Dark", 0x0d1117, 0x161b22, 0xe6edf3, 0x6e7681, [0x58a6ff, 0xbc8cff, 0x3fb950, 0xf0883e, 0xe6edf3]),
    pal("material", "Material", 0x121212, 0x1e1e1e, 0xffffff, 0x9e9e9e, [0xbb86fc, 0x03dac6, 0xcf6679, 0xffffff, 0x3700b3]),
    pal("vesper", "Vesper", 0x101010, 0x1c1c1c, 0xffffff, 0xa0a0a0, [0xffc799, 0x99ffe4, 0xff8080, 0xa0a0a0, 0xffffff]),
    pal("ayu", "Ayu Dark", 0x0b0e14, 0x11151c, 0xbfbdb6, 0x565b66, [0xe6b450, 0x39bae6, 0xc2d94c, 0xff8f40, 0xbfbdb6]),
    pal("carbonfox", "Carbonfox", 0x161616, 0x262626, 0xf2f4f8, 0x6f6f6f, [0x78a9ff, 0x3ddbd9, 0xbe95ff, 0xee5396, 0xf2f4f8]),
    pal("flexoki", "Flexoki Dark", 0x100f0f, 0x1c1b1a, 0xcecdc3, 0x6f6e69, [0xd14d41, 0xda702c, 0xd0a215, 0x879a39, 0xcecdc3]),
    pal("ocean", "Material Ocean", 0x0f111a, 0x1a1c25, 0xeeffff, 0x676e95, [0x84ffff, 0xc792ea, 0x82aaff, 0xf78c6c, 0xc3e88d]),
    pal("horizon", "Horizon", 0x1c1e26, 0x232530, 0xe0e0e0, 0x6c6f93, [0xe95678, 0xfab795, 0xfac29a, 0x09f7a0, 0xe0e0e0]),
    pal("darker", "Material Darker", 0x212121, 0x292929, 0xeeffff, 0x4a4a4a, [0x82aaff, 0xc792ea, 0xc3e88d, 0xffcb6b, 0xeeffff]),
];

const fn pal(id: &'static str, name: &'static str, bg: u32, surface: u32, text: u32, muted: u32, accents: [u32; 5]) -> Palette {
    Palette {
        id,
        name,
        bg: rgb(bg),
        surface: rgb(surface),
        text: rgb(text),
        muted: rgb(muted),
        accents: [rgb(accents[0]), rgb(accents[1]), rgb(accents[2]), rgb(accents[3]), rgb(accents[4])],
    }
}

const fn rgb(hex: u32) -> Rgba {
    Rgba::new(((hex >> 16) & 0xff) as u8, ((hex >> 8) & 0xff) as u8, (hex & 0xff) as u8, 255)
}

pub fn parse_hex(text: &str) -> Option<Rgba> {
    let text = text.trim().trim_start_matches('#');
    if text.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(text, 16).ok()?;
    Some(rgb(n))
}

pub fn to_hex(c: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

/// Build a dark palette from one seed color. Used when a wallpaper is sampled.
pub fn from_seed(seed: Rgba) -> [Rgba; 5] {
    let bg = Rgba::new(seed.r / 8, seed.g / 8, seed.b / 8, 255).mix(Rgba::new(8, 8, 12, 255), 0.45);
    let accent = seed;
    let lift = seed.mix(Rgba::new(255, 255, 255, 255), 0.35);
    let second = Rgba::new(seed.b, seed.r, seed.g, 255);
    let text = Rgba::new(236, 236, 242, 255);
    [bg, accent, lift, second, text]
}

pub fn accent_hue(c: Rgba) -> f32 {
    let r = c.r as f32 / 255.0;
    let g = c.g as f32 / 255.0;
    let b = c.b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if d < 1e-4 {
        return 186.0;
    }
    let h = if (max - r).abs() < 1e-4 {
        ((g - b) / d).rem_euclid(6.0)
    } else if (max - g).abs() < 1e-4 {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h * 60.0
}
