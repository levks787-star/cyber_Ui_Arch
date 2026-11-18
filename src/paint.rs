use crate::theme::Rgba;
use fontdue::{Font, FontSettings, Metrics};
use std::collections::HashMap;
use tiny_skia::{FillRule, LineCap, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

#[derive(Clone, Copy, Debug)]
pub struct UiRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl UiRect {
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn value_x(self, x: f32) -> f32 {
        if self.w <= 1.0 {
            return 0.0;
        }
        ((x - self.x) / self.w).clamp(0.0, 1.0)
    }
}

struct Glyph {
    metrics: Metrics,
    bitmap: Vec<u8>,
}

pub struct Canvas {
    pm: Pixmap,
    font: Font,
    cache: HashMap<(char, u16), Glyph>,
    pub theme: crate::theme::ThemeColors,
    pub w: u32,
    pub h: u32,
}

impl Canvas {
    pub fn new(w: u32, h: u32, theme: crate::theme::ThemeColors) -> Self {
        let pm = Pixmap::new(w.max(1), h.max(1)).expect("pixmap");
        let font = Font::from_bytes(FONT_BYTES, FontSettings::default()).expect("jetbrains mono");
        Self {
            pm,
            font,
            cache: HashMap::new(),
            theme,
            w,
            h,
        }
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == self.w && h == self.h && w > 0 && h > 0 {
            return;
        }
        self.w = w.max(1);
        self.h = h.max(1);
        self.pm = Pixmap::new(self.w, self.h).expect("pixmap");
    }

    pub fn clear(&mut self, color: Rgba) {
        self.pm.fill(color_of(color));
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgba) {
        let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w.max(0.0), h.max(0.0)) else {
            return;
        };
        self.pm.fill_rect(rect, &paint(color), Transform::identity(), None);
    }

    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgba, width: f32) {
        let mut pb = PathBuilder::new();
        pb.move_to(x, y);
        pb.line_to(x + w, y);
        pb.line_to(x + w, y + h);
        pb.line_to(x, y + h);
        pb.close();
        if let Some(path) = pb.finish() {
            self.stroke_path(&path, color, width, LineCap::Butt);
        }
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: Rgba, width: f32) {
        let mut pb = PathBuilder::new();
        pb.move_to(x0, y0);
        pb.line_to(x1, y1);
        if let Some(path) = pb.finish() {
            self.stroke_path(&path, color, width, LineCap::Butt);
        }
    }

    pub fn poly_stroke(&mut self, pts: &[(f32, f32)], color: Rgba, width: f32) {
        if pts.len() < 2 {
            return;
        }
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].0, pts[0].1);
        for p in &pts[1..] {
            pb.line_to(p.0, p.1);
        }
        if let Some(path) = pb.finish() {
            self.stroke_path(&path, color, width, LineCap::Round);
        }
    }

    fn stroke_path(&mut self, path: &Path, color: Rgba, width: f32, cap: LineCap) {
        let mut stroke = Stroke::default();
        stroke.width = width.max(0.5);
        stroke.line_cap = cap;
        self.pm
            .stroke_path(path, &paint(color), &stroke, Transform::identity(), None);
    }

    fn fill_path(&mut self, path: &Path, color: Rgba) {
        self.pm
            .fill_path(path, &paint(color), FillRule::Winding, Transform::identity(), None);
    }

    pub fn dots(&mut self, x: f32, y: f32, w: f32, color: Rgba) {
        let mut t = 0.0;
        while t < w {
            self.fill_rect(x + t, y, 1.4, 1.2, color);
            t += 4.5;
        }
    }

    pub fn brackets(&mut self, inset: f32, len: f32, color: Rgba) {
        let w = self.w as f32;
        let h = self.h as f32;
        self.line(inset, inset, inset + len, inset, color, 1.1);
        self.line(inset, inset, inset, inset + len, color, 1.1);
        self.line(w - inset, inset, w - inset - len, inset, color, 1.1);
        self.line(w - inset, inset, w - inset, inset + len, color, 1.1);
        self.line(inset, h - inset, inset + len, h - inset, color, 1.1);
        self.line(inset, h - inset, inset, h - inset - len, color, 1.1);
        self.line(w - inset, h - inset, w - inset - len, h - inset, color, 1.1);
        self.line(w - inset, h - inset, w - inset, h - inset - len, color, 1.1);
    }

    pub fn text(&mut self, x: f32, y: f32, size: f32, color: Rgba, text: &str) {
        let baseline = y + size * 0.80;
        let mut pen = x;
        for ch in text.chars() {
            let key = (ch, (size * 10.0) as u16);
            if !self.cache.contains_key(&key) {
                let (metrics, bitmap) = self.font.rasterize(ch, size);
                self.cache.insert(key, Glyph { metrics, bitmap });
            }
            let glyph = self.cache.get(&key).unwrap();
            let gx = pen as i32 + glyph.metrics.xmin;
            let gy = baseline as i32 - glyph.metrics.ymin - glyph.metrics.height as i32;
            blit(
                self.pm.data_mut(),
                self.w,
                self.h,
                &glyph.bitmap,
                glyph.metrics.width,
                glyph.metrics.height,
                gx,
                gy,
                color,
            );
            pen += glyph.metrics.advance_width;
        }
    }

    pub fn measure(&mut self, text: &str, size: f32) -> f32 {
        let mut w = 0.0;
        for ch in text.chars() {
            let key = (ch, (size * 10.0) as u16);
            if !self.cache.contains_key(&key) {
                let (metrics, bitmap) = self.font.rasterize(ch, size);
                self.cache.insert(key, Glyph { metrics, bitmap });
            }
            w += self.cache.get(&key).unwrap().metrics.advance_width;
        }
        w
    }

    pub fn text_fit(&mut self, x: f32, y: f32, size: f32, color: Rgba, text: &str, max_w: f32) {
        let mut s: String = text.chars().take(80).collect();
        while self.measure(&s, size) > max_w && s.len() > 1 {
            s.pop();
        }
        if s.len() < text.chars().count() && s.len() > 2 {
            s.pop();
            s.push('.');
            s.push('.');
        }
        self.text(x, y, size, color, &s);
    }

    pub fn text_right(&mut self, right: f32, y: f32, size: f32, color: Rgba, text: &str) {
        let w = self.measure(text, size);
        self.text(right - w, y, size, color, text);
    }

    pub fn spark(&mut self, x: f32, y: f32, w: f32, h: f32, data: &[f32], color: Rgba) {
        self.spark_slice(x, y, w, h, data, color);
    }

    pub fn spark_owned(&mut self, x: f32, y: f32, w: f32, h: f32, data: Vec<f32>, color: Rgba) {
        self.spark_slice(x, y, w, h, &data, color);
    }

    fn spark_slice(&mut self, x: f32, y: f32, w: f32, h: f32, data: &[f32], color: Rgba) {
        if data.len() < 2 || w < 2.0 || h < 2.0 {
            return;
        }
        let max = data.iter().copied().fold(0.02_f32, f32::max);
        let mut line = PathBuilder::new();
        let mut area = PathBuilder::new();
        let n = data.len();
        for (i, v) in data.iter().enumerate() {
            let px = x + i as f32 / (n - 1) as f32 * w;
            let py = y + h - (v / max).clamp(0.0, 1.0) * h;
            if i == 0 {
                line.move_to(px, py);
                area.move_to(px, py);
            } else {
                line.line_to(px, py);
                area.line_to(px, py);
            }
        }
        area.line_to(x + w, y + h);
        area.line_to(x, y + h);
        area.close();
        if let Some(path) = area.finish() {
            self.fill_path(&path, color.alpha(48));
        }
        if let Some(path) = line.finish() {
            self.stroke_path(&path, color, 1.25, LineCap::Round);
        }
    }

    pub fn vbar(&mut self, x: f32, y: f32, w: f32, h: f32, frac: f32, color: Rgba) {
        self.fill_rect(x, y, w, h, self.theme.panel2);
        let f = frac.clamp(0.0, 1.0);
        let bh = (h * f).max(1.0);
        self.fill_rect(x, y + h - bh, w, bh, color);
    }

    pub fn hbar(&mut self, x: f32, y: f32, w: f32, h: f32, frac: f32, color: Rgba) {
        self.fill_rect(x, y, w, h, self.theme.panel2);
        self.fill_rect(x, y, w * frac.clamp(0.0, 1.0), h, color);
    }

    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, thick: f32, frac: f32, fg: Rgba) {
        self.arc(cx, cy, r, -std::f32::consts::FRAC_PI_2, std::f32::consts::TAU, thick, self.theme.line.alpha(140));
        let f = frac.clamp(0.0, 1.0);
        if f > 0.004 {
            let a0 = -std::f32::consts::FRAC_PI_2;
            self.arc(cx, cy, r, a0, a0 + f * std::f32::consts::TAU, thick, fg);
        }
    }

    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, width: f32, color: Rgba) {
        let span = (a1 - a0).abs();
        let steps = ((span / std::f32::consts::TAU) * 72.0).clamp(6.0, 80.0) as i32;
        let mut pb = PathBuilder::new();
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let a = a0 + (a1 - a0) * t;
            let px = cx + a.cos() * r;
            let py = cy + a.sin() * r;
            if i == 0 {
                pb.move_to(px, py);
            } else {
                pb.line_to(px, py);
            }
        }
        if let Some(path) = pb.finish() {
            self.stroke_path(&path, color, width, LineCap::Round);
        }
    }

    pub fn seven(&mut self, x: f32, y: f32, dw: f32, dh: f32, text: &str, color: Rgba, colon_on: bool) {
        let gap = dw * 0.22;
        let mut cx = x;
        for ch in text.chars() {
            if ch == ':' {
                if colon_on {
                    let s = (dh * 0.07).max(2.0);
                    self.fill_rect(cx, y + dh * 0.28, s, s, color);
                    self.fill_rect(cx, y + dh * 0.64, s, s, color);
                }
                cx += gap + 4.0;
                continue;
            }
            if let Some(d) = ch.to_digit(10) {
                self.digit(cx, y, dw, dh, d as u8, color);
                cx += dw + gap;
            }
        }
    }

    fn digit(&mut self, x: f32, y: f32, w: f32, h: f32, d: u8, color: Rgba) {
        const SEGS: [u8; 10] = [
            0b0111_111, 0b0000_110, 0b1011_011, 0b1001_111, 0b1100_110, 0b1101_101, 0b1111_101,
            0b0000_111, 0b1111_111, 0b1101_111,
        ];
        let mask = SEGS[d as usize];
        let t = (w * 0.16).max(2.0);
        let hh = h * 0.5;
        if mask & 1 != 0 {
            self.fill_rect(x + t, y, w - 2.0 * t, t, color);
        }
        if mask & 2 != 0 {
            self.fill_rect(x + w - t, y + t * 0.6, t, hh - t, color);
        }
        if mask & 4 != 0 {
            self.fill_rect(x + w - t, y + hh + t * 0.2, t, hh - t * 1.2, color);
        }
        if mask & 8 != 0 {
            self.fill_rect(x + t, y + h - t, w - 2.0 * t, t, color);
        }
        if mask & 16 != 0 {
            self.fill_rect(x, y + hh + t * 0.2, t, hh - t * 1.2, color);
        }
        if mask & 32 != 0 {
            self.fill_rect(x, y + t * 0.6, t, hh - t, color);
        }
        if mask & 64 != 0 {
            self.fill_rect(x + t, y + hh - t * 0.5, w - 2.0 * t, t, color);
        }
    }

    pub fn toggle(&mut self, x: f32, y: f32, on: bool, available: bool) -> UiRect {
        let w = 48.0;
        let h = 18.0;
        let border = if !available {
            self.theme.dim.alpha(120)
        } else if on {
            self.theme.primary
        } else {
            self.theme.line
        };
        self.fill_rect(x, y, w, h, self.theme.panel2);
        self.stroke_rect(x, y, w, h, border, 1.2);
        let kx = if on { x + w - h + 2.0 } else { x + 2.0 };
        let knob = if !available {
            self.theme.dim
        } else if on {
            self.theme.primary
        } else {
            self.theme.dim
        };
        self.fill_rect(kx, y + 2.0, h - 4.0, h - 4.0, knob);
        UiRect { x, y, w, h }
    }

    pub fn slider(&mut self, x: f32, y: f32, w: f32, value: f32, color: Rgba) -> UiRect {
        let h = 8.0;
        self.fill_rect(x, y, w, h, self.theme.panel2);
        self.stroke_rect(x, y, w, h, self.theme.line.alpha(180), 1.0);
        self.fill_rect(x, y, (w * value.clamp(0.0, 1.0)).max(1.0), h, color);
        for i in 0..11 {
            let tx = x + i as f32 * w / 10.0;
            self.fill_rect(tx, y - 4.0, 1.0, 3.0, self.theme.dim.alpha(160));
        }
        UiRect {
            x,
            y: y - 8.0,
            w,
            h: h + 16.0,
        }
    }

    pub fn copy_rgba(&self, out: &mut Vec<u8>) {
        let data = self.pm.data();
        out.resize(data.len(), 0);
        out.copy_from_slice(data);
        unpremultiply(out);
    }
}

fn paint(color: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color_of(color));
    p.anti_alias = true;
    p
}

fn color_of(c: Rgba) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn blit(
    data: &mut [u8],
    pw: u32,
    ph: u32,
    bitmap: &[u8],
    gw: usize,
    gh: usize,
    gx: i32,
    gy: i32,
    color: Rgba,
) {
    let pw = pw as i32;
    let ph = ph as i32;
    for row in 0..gh {
        let dy = gy + row as i32;
        if dy < 0 || dy >= ph {
            continue;
        }
        for col in 0..gw {
            let dx = gx + col as i32;
            if dx < 0 || dx >= pw {
                continue;
            }
            let cov = bitmap[row * gw + col] as u32;
            if cov == 0 {
                continue;
            }
            let a = (color.a as u32 * cov) / 255;
            let i = ((dy as u32 * pw as u32 + dx as u32) * 4) as usize;
            let inv = 255 - a;
            let sr = (color.r as u32 * a) / 255;
            let sg = (color.g as u32 * a) / 255;
            let sb = (color.b as u32 * a) / 255;
            data[i] = (sr + data[i] as u32 * inv / 255) as u8;
            data[i + 1] = (sg + data[i + 1] as u32 * inv / 255) as u8;
            data[i + 2] = (sb + data[i + 2] as u32 * inv / 255) as u8;
            data[i + 3] = (a + data[i + 3] as u32 * inv / 255) as u8;
        }
    }
}

pub fn unpremultiply(buf: &mut [u8]) {
    for px in buf.chunks_exact_mut(4) {
        let a = px[3];
        if a == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        } else if a < 255 {
            px[0] = ((px[0] as u16 * 255) / a as u16).min(255) as u8;
            px[1] = ((px[1] as u16 * 255) / a as u16).min(255) as u8;
            px[2] = ((px[2] as u16 * 255) / a as u16).min(255) as u8;
        }
    }
}
