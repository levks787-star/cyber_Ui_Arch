//! Original UI tones. Short glassy ticks and airy chimes, synthesized here.
//! They are not samples from any console.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const RATE: u32 = 44_100;

#[derive(Clone, Copy, Debug)]
pub enum Sfx {
    Hover,
    Focus,
    ToggleOn,
    ToggleOff,
    Slider,
    Theme,
    Reset,
    Start,
}

enum Cmd {
    Play(Sfx),
    Enabled(bool),
    Volume(f32),
}

pub struct Audio {
    tx: Option<Sender<Cmd>>,
}

impl Audio {
    pub fn start(enabled: bool, volume: f32) -> Self {
        let (tx, rx) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("acc-audio".into())
            .spawn(move || audio_thread(rx, enabled, volume.clamp(0.0, 1.0)))
            .is_ok();
        Self {
            tx: if spawned { Some(tx) } else { None },
        }
    }

    pub fn play(&self, sfx: Sfx) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Play(sfx));
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Enabled(enabled));
        }
    }

    pub fn set_volume(&self, volume: f32) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Volume(volume.clamp(0.0, 1.0)));
        }
    }
}

fn audio_thread(rx: Receiver<Cmd>, mut enabled: bool, mut volume: f32) {
    let opened = rodio::OutputStream::try_default();
    let Ok((_stream, handle)) = opened else {
        log::info!("no audio device; UI sounds stay silent");
        while rx.recv().is_ok() {}
        return;
    };
    let bank = Bank::new();
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Play(sfx) => {
                if enabled && volume > 0.001 {
                    let clip = bank.get(sfx);
                    let gained = scale(clip, volume);
                    let _ = handle.play_raw(Tone {
                        data: Arc::new(gained),
                        pos: 0,
                        rate: RATE,
                    });
                }
            }
            Cmd::Enabled(value) => enabled = value,
            Cmd::Volume(value) => volume = value.clamp(0.0, 1.0),
        }
    }
}

struct Tone {
    data: Arc<Vec<f32>>,
    pos: usize,
    rate: u32,
}

impl Iterator for Tone {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let sample = self.data.get(self.pos).copied()?;
        self.pos += 1;
        Some(sample)
    }
}

impl rodio::Source for Tone {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        1
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f32(self.data.len() as f32 / self.rate as f32))
    }
}

struct Bank {
    hover: Vec<f32>,
    focus: Vec<f32>,
    on: Vec<f32>,
    off: Vec<f32>,
    slider: Vec<f32>,
    theme: Vec<f32>,
    reset: Vec<f32>,
    start: Vec<f32>,
}

impl Bank {
    fn new() -> Self {
        Self {
            hover: tick(2140.0, 0.055, 0.012, 0.08),
            focus: focus_chime(),
            on: click(980.0, 1460.0, 0.07, true),
            off: click(720.0, 390.0, 0.08, false),
            slider: tick(2680.0, 0.028, 0.004, 0.02),
            theme: shimmer(),
            reset: whoosh(),
            start: startup(),
        }
    }

    fn get(&self, sfx: Sfx) -> &Vec<f32> {
        match sfx {
            Sfx::Hover => &self.hover,
            Sfx::Focus => &self.focus,
            Sfx::ToggleOn => &self.on,
            Sfx::ToggleOff => &self.off,
            Sfx::Slider => &self.slider,
            Sfx::Theme => &self.theme,
            Sfx::Reset => &self.reset,
            Sfx::Start => &self.start,
        }
    }
}

fn scale(src: &[f32], volume: f32) -> Vec<f32> {
    let g = volume * 0.85;
    src.iter().map(|s| s * g).collect()
}

fn env(t: f32, attack: f32, decay: f32) -> f32 {
    if t < attack {
        t / attack.max(0.0001)
    } else {
        (-(t - attack) / decay.max(0.0001)).exp()
    }
}

fn noise(state: &mut u32) -> f32 {
    *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
    (*state >> 9) as f32 / 8388608.0 - 1.0
}

fn render(dur: f32, mut body: impl FnMut(f32, &mut u32) -> f32) -> Vec<f32> {
    let n = (dur * RATE as f32) as usize;
    let mut rng = 0xA5C3_u32;
    let mut out = Vec::with_capacity(n);
    let mut prev = 0.0;
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let raw = body(t, &mut rng);
        let sample = (raw * 0.72 + prev * 0.28).tanh();
        prev = sample * 0.35;
        out.push(sample * 0.42);
    }
    // short fade so clips don't click at the end
    let fade = (0.004 * RATE as f32) as usize;
    for (i, s) in out.iter_mut().rev().take(fade).enumerate() {
        *s *= i as f32 / fade as f32;
    }
    out
}

fn tick(freq: f32, dur: f32, decay: f32, air: f32) -> Vec<f32> {
    render(dur, move |t, rng| {
        let e = env(t, 0.0015, decay);
        let glass = (std::f32::consts::TAU * freq * t).sin() * 0.55
            + (std::f32::consts::TAU * freq * 2.02 * t).sin() * 0.22
            + (std::f32::consts::TAU * freq * 3.97 * t).sin() * 0.08;
        let airy = noise(rng) * air * env(t, 0.0008, 0.018);
        (glass + airy) * e
    })
}

fn click(from: f32, to: f32, dur: f32, bright: bool) -> Vec<f32> {
    render(dur, move |t, rng| {
        let k = (t / dur).clamp(0.0, 1.0);
        let f = from + (to - from) * k;
        let e = env(t, 0.0012, if bright { 0.045 } else { 0.06 });
        let tone = (std::f32::consts::TAU * f * t).sin() * 0.62
            + (std::f32::consts::TAU * f * 2.0 * t).sin() * if bright { 0.28 } else { 0.1 };
        let grit = noise(rng) * 0.12 * env(t, 0.0005, 0.012);
        (tone + grit) * e
    })
}

fn focus_chime() -> Vec<f32> {
    render(0.32, move |t, rng| {
        let e = env(t, 0.012, 0.16);
        let a = (std::f32::consts::TAU * 523.25 * t).sin() * 0.34;
        let b = (std::f32::consts::TAU * 783.99 * t).sin() * 0.22;
        let c = (std::f32::consts::TAU * 1046.5 * t).sin() * 0.12 * env(t, 0.02, 0.12);
        let air = noise(rng) * 0.05 * env(t, 0.002, 0.05);
        let sweep = (std::f32::consts::TAU * (640.0 - t * 900.0) * t).sin() * 0.08 * env(t, 0.001, 0.07);
        (a + b + c + air + sweep) * e
    })
}

fn shimmer() -> Vec<f32> {
    render(0.48, move |t, rng| {
        let k = (t / 0.48).clamp(0.0, 1.0);
        let f = 420.0 + k * 980.0;
        let e = env(t, 0.02, 0.22);
        let a = (std::f32::consts::TAU * f * t).sin();
        let b = (std::f32::consts::TAU * (f * 1.005) * t).sin();
        let beat = (a + b) * 0.28;
        let high = (std::f32::consts::TAU * f * 2.01 * t).sin() * 0.12;
        let air = noise(rng) * 0.04 * env(t, 0.01, 0.2);
        (beat + high + air) * e
    })
}

fn whoosh() -> Vec<f32> {
    render(0.34, move |t, rng| {
        let k = (t / 0.34).clamp(0.0, 1.0);
        let f = 520.0 - k * 340.0;
        let e = env(t, 0.01, 0.14);
        let mut n = noise(rng);
        n = n * 0.35 + (std::f32::consts::TAU * f * t).sin() * 0.65;
        n * e * 0.7
    })
}

fn startup() -> Vec<f32> {
    render(0.72, move |t, rng| {
        let notes = [(0.0, 392.0), (0.11, 523.25), (0.22, 659.25)];
        let mut s = 0.0;
        for (at, freq) in notes {
            if t >= at {
                let u = t - at;
                let e = env(u, 0.01, 0.28);
                s += (std::f32::consts::TAU * freq * u).sin() * 0.28 * e;
                s += (std::f32::consts::TAU * freq * 2.003 * u).sin() * 0.08 * e;
            }
        }
        s + noise(rng) * 0.03 * env(t, 0.005, 0.12)
    })
}
