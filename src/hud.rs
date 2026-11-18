use crate::paint::{Canvas, UiRect};
use crate::theme::{self, ThemeId};

#[derive(Clone, Copy, Debug)]
pub enum HudAction {
    Theme(ThemeId),
    Hue,
    Reset,
}

#[derive(Clone, Copy, Debug)]
pub struct HudHit {
    pub rect: UiRect,
    pub action: HudAction,
}

pub struct Hud {
    pub hits: Vec<HudHit>,
}

impl Hud {
    pub fn draw(
        canvas: &mut Canvas,
        theme_id: ThemeId,
        hue: f32,
        demo: bool,
        hovering: bool,
    ) -> Self {
        canvas.clear(theme::Rgba::new(0, 0, 0, 0));
        let w = canvas.w as f32;
        canvas.text(22.0, 16.0, 15.0, canvas.theme.primary, "ARCH LINUX");
        canvas.text(148.0, 18.0, 12.0, canvas.theme.dim, "CONTROL CENTER");
        canvas.text(300.0, 18.0, 11.0, canvas.theme.secondary, "v0.2 BETA");
        if demo {
            canvas.text(390.0, 18.0, 11.0, canvas.theme.warn, "DEMO");
        } else {
            canvas.text(390.0, 18.0, 11.0, canvas.theme.ok, "LIVE");
        }
        if hovering {
            canvas.text(460.0, 18.0, 11.0, canvas.theme.primary, "FOCUS");
        }

        let mut hits = Vec::new();
        let sw = 54.0;
        let gap = 8.0;
        let presets = ThemeId::all();
        let slider_w = 130.0;
        let total = presets.len() as f32 * (sw + gap) + slider_w + 16.0;
        let mut x = w - total - 18.0;
        let y = 14.0;
        for id in presets {
            let color = theme::swatch(id);
            canvas.fill_rect(x, y, sw, 16.0, color);
            let border = if theme_id == id {
                canvas.theme.text
            } else {
                canvas.theme.line
            };
            canvas.stroke_rect(x, y, sw, 16.0, border, if theme_id == id { 1.8 } else { 1.0 });
            canvas.text(x, y + 18.0, 10.0, canvas.theme.dim, id.label());
            hits.push(HudHit {
                rect: UiRect { x, y, w: sw, h: 34.0 },
                action: HudAction::Theme(id),
            });
            x += sw + gap;
        }
        for i in 0..slider_w as i32 {
            let h = i as f32 / slider_w * 360.0;
            canvas.fill_rect(x + i as f32, y, 1.0, 16.0, theme::hsv(h, 0.85, 1.0));
        }
        canvas.stroke_rect(x, y, slider_w, 16.0, canvas.theme.line, 1.0);
        let marker = x + (hue.rem_euclid(360.0) / 360.0) * slider_w;
        canvas.fill_rect(marker - 1.0, y - 3.0, 2.0, 22.0, canvas.theme.text);
        canvas.text(x, y + 18.0, 10.0, if theme_id == ThemeId::Custom { canvas.theme.primary } else { canvas.theme.dim }, "HUE");
        hits.push(HudHit {
            rect: UiRect { x, y: y - 4.0, w: slider_w, h: 40.0 },
            action: HudAction::Hue,
        });

        let bw = 92.0;
        let bh = 22.0;
        let bx = w * 0.5 - bw * 0.5;
        let by = canvas.h as f32 - 36.0;
        canvas.fill_rect(bx, by, bw, bh, canvas.theme.panel);
        canvas.stroke_rect(bx, by, bw, bh, canvas.theme.primary, 1.2);
        canvas.text(bx + 22.0, by + 4.0, 12.0, canvas.theme.text, "RESET");
        hits.push(HudHit {
            rect: UiRect { x: bx, y: by, w: bw, h: bh },
            action: HudAction::Reset,
        });
        canvas.text(22.0, canvas.h as f32 - 32.0, 11.0, canvas.theme.dim, "DRAG ORBIT    WHEEL ZOOM    RMB PAN    R RESET    1-5 THEME");
        Self { hits }
    }
}
