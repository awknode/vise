//! COMPRESS: six compressor models behind the same seven knobs.
//!
//!   k0 INPUT / THRESH   k1 model-specific (BIAS, RATIO, MODE, OPTO...)
//!   k2 / k3 time (or WARMTH / TRANNY on FAT, TIME + TUBES on 670)
//!   k4 COLOR            k5 AUTO GAIN        k6 SC HPF (detector only)
//!
//! 670    vari-mu (Herchild / Fairchild style): feedback detection, ratio that
//!        grows with level, six TIME constants (5 and 6 program dependent), seven
//!        tube sets. Push-pull tubes: distortion rises with gain reduction.
//! DS     Distressor style: feed-forward, 1:1 .. 20:1 + NUKE, DIST 2 (even) /
//!        DIST 3 (odd) colour.
//! FAT    Fatso style: four modes (GEN, BUSS, SPANK, TRACK), WARMTH (fast high-end
//!        squash), TRANNY (transformer low end), tape colour.
//! ALPHA  elysia style class-A VCA: clean, ratio past infinity (negative), WARM.
//! SH-A   Shadow Hills mastering style: opto stage then discrete VCA stage,
//!        COLOR morphs the output transformer NICKEL -> IRON -> STEEL.
//! SH-B   the Class A version: slower opto, softer VCA, warmer (more 2nd).
//!
//! Gain is worked out at the base rate (linked stereo); the colour stage runs
//! oversampled. Latency = the oversampler's (`latency()`).

use crate::dsp::*;
use crate::oversample::{self, Oversampler, MAX_FACTOR};
use crate::params::MAX_K;

pub const MODELS: [&str; 6] = ["670", "DS", "FAT", "ALPHA", "SH-A", "SH-B"];
pub const TUBES: [&str; 7] = ["6386", "6386 NOS", "6BC8", "6BA6", "6BZ6", "5670", "6CG7"];
/// (even, odd, knee x) per tube set
const TUBE_CHAR: [(f32, f32, f32); 7] = [(0.10, 0.55, 1.0), (0.06, 0.45, 1.1), (0.22, 0.45, 0.9), (0.14, 0.7, 0.8), (0.08, 0.35, 1.3), (0.3, 0.6, 0.7), (0.18, 0.8, 1.0)];
pub const DS_RATIOS: [f32; 8] = [1.0, 2.0, 3.0, 4.0, 6.0, 10.0, 20.0, 100.0];
pub const DS_RATIO_NAMES: [&str; 8] = ["1:1", "2:1", "3:1", "4:1", "6:1", "10:1", "20:1", "NUKE"];
pub const FAT_MODES: [&str; 4] = ["GEN", "BUSS", "SPANK", "TRACK"];
pub const SH_RATIOS: [f32; 6] = [1.2, 1.5, 2.0, 3.0, 4.0, 6.0];
/// 670 time constants: (attack ms, release s, slow release s; 0 = not program dependent)
pub const TIMES: [(f32, f32, f32); 6] = [(0.2, 0.3, 0.0), (0.2, 0.8, 0.0), (0.4, 2.0, 0.0), (0.4, 5.0, 0.0), (0.4, 2.0, 10.0), (0.2, 0.3, 25.0)];

pub fn model_uses_input(model: usize) -> bool {
    model <= 2
}
/// INPUT drive into a fixed threshold. 0 = a hot track (peaks near 0 dBFS) is left alone.
pub const INPUT_MAX_DB: f32 = 36.0;
pub fn input_db(v: f32) -> f32 {
    v * INPUT_MAX_DB
}
pub fn thresh_db(v: f32) -> f32 {
    -v * 40.0
}
pub fn ds_attack_ms(v: f32) -> f32 {
    0.05 * 600f32.powf(v)
}
pub fn ds_release_ms(v: f32) -> f32 {
    50.0 * 70f32.powf(v)
}
pub fn al_attack_ms(v: f32) -> f32 {
    0.1 * 1200f32.powf(v)
}
pub fn al_release_ms(v: f32) -> f32 {
    5.0 * 400f32.powf(v)
}
pub fn sh_attack_ms(v: f32) -> f32 {
    0.1 * 300f32.powf(v)
}
pub fn sh_release_ms(v: f32) -> f32 {
    100.0 * 12f32.powf(v)
}
pub fn hpf_hz(v: f32) -> f32 {
    20.0 * 15f32.powf(v)
}
/// ALPHA ratio: 1/R from 1 (1:1) through 0 (infinity) to -0.25 (negative)
pub fn alpha_inv(v: f32) -> f32 {
    1.0 - v * 1.25
}
pub fn step(list_len: usize, v: f32) -> usize {
    ((v.clamp(0.0, 1.0) * (list_len - 1) as f32).round() as usize).min(list_len - 1)
}

/// Knob names per model.
pub fn knob_name(model: usize, k: usize) -> &'static str {
    match (model, k) {
        (_, 0) if model_uses_input(model) => "INPUT",
        (_, 0) => "THRESH",
        (0, 1) => "BIAS",
        (0, 2) => "TIME",
        (0, 3) => "TUBES",
        (1, 1) | (3, 1) | (4, 1) | (5, 1) => "RATIO",
        (2, 1) => "MODE",
        (2, 2) => "WARMTH",
        (2, 3) => "TRANNY",
        (_, 2) => "ATTACK",
        (_, 3) => "RELEASE",
        (1, 4) => "DIST",
        (4, 4) | (5, 4) => "IRON",
        (_, 4) => "COLOR",
        (_, 5) => "AUTO GAIN",
        _ => "SC HPF",
    }
}

pub fn knob_text(model: usize, k: usize, v: f32) -> String {
    let ms = |x: f32| if x >= 1000.0 { format!("{:.1} s", x / 1000.0) } else if x < 1.0 { format!("{:.2} ms", x) } else { format!("{:.0} ms", x) };
    match (model, k) {
        (_, 0) if model_uses_input(model) => format!("+{:.1} dB", input_db(v)),
        (_, 0) => format!("{:.1} dB", thresh_db(v)),
        (0, 1) => format!("{:+.0}", (v - 0.5) * 20.0),
        (0, 2) => format!("TC {}", step(6, v) + 1),
        (0, 3) => TUBES[step(7, v)].to_string(),
        (1, 1) => DS_RATIO_NAMES[step(8, v)].to_string(),
        (3, 1) => {
            let i = alpha_inv(v);
            if i.abs() < 0.02 { "INF".into() } else if i < 0.0 { format!("NEG {:.1}", -1.0 / i) } else { format!("{:.1}:1", 1.0 / i) }
        }
        (4, 1) | (5, 1) => format!("{:.1}:1", SH_RATIOS[step(6, v)]),
        (2, 1) => FAT_MODES[step(4, v)].to_string(),
        (1, 2) => ms(ds_attack_ms(v)),
        (1, 3) => ms(ds_release_ms(v)),
        (3, 2) => ms(al_attack_ms(v)),
        (3, 3) => ms(al_release_ms(v)),
        (4, 2) | (5, 2) => ms(sh_attack_ms(v)),
        (4, 3) | (5, 3) => ms(sh_release_ms(v)),
        (1, 4) => {
            if v < 0.02 { "CLEAN".into() } else if v < 0.5 { format!("D2 {:.0}%", v * 200.0) } else { format!("D3 {:.0}%", (v - 0.5) * 200.0) }
        }
        (4, 4) | (5, 4) => (if v < 0.33 { "NICKEL" } else if v < 0.67 { "IRON" } else { "STEEL" }).into(),
        (_, 5) => if v < 0.01 { "OFF".into() } else { format!("{:.0}%", v * 100.0) },
        (_, 6) => format!("{:.0} Hz", hpf_hz(v)),
        _ => format!("{:.0}%", v * 100.0),
    }
}

#[derive(Clone, Copy)]
struct Cfg {
    pre: f32,
    thr: f32,
    /// 1/ratio (670: starting 1/ratio, gets smaller with level)
    inv: f32,
    knee: f32,
    att: f32,
    rel: f32,
    /// program-dependent slow release coefficient (0 = off)
    rel2: f32,
    feedback: bool,
    progressive: bool,
    // colour
    even: f32,
    odd: f32,
    drive: f32,
    /// colour rises with gain reduction (vari-mu)
    gr_drive: f32,
    // SH opto stage
    opto: bool,
    o_att: f32,
    o_rel: f32,
    o_rel2: f32,
    // FAT
    warmth: f32,
    tranny: f32,
    auto: f32,
    hp_a: f32,
}

pub struct Comp {
    sr: f32,
    os: [Oversampler; 2],
    c: Cfg,
    /// per-channel gain computer state (equal when fully linked)
    gr: [f32; 2],
    gr2: [f32; 2],
    og: [f32; 2],
    og2: [f32; 2],
    fb_lvl: [f32; 2],
    hp: [Lp1; 2],
    dc: [Lp1; 2],
    dc2: [Lp1; 2],
    hf: [Lp1; 2],
    hf_env: f32,
    lf: [Lp1; 2],
    /// slow loudness in / out (AUTO GAIN)
    p_in: f32,
    p_out: f32,
    /// AUTO GAIN makeup right now (linear)
    pub mk: f32,
    /// what the detector heard last sample (after the SC HPF, before INPUT drive)
    pub sc: [f32; 2],
    /// gain reduction right now (dB, positive) for the meter
    pub gr_db: f32,
}

impl Comp {
    pub fn os_factor(&self) -> usize {
        self.os[0].factor()
    }
    pub fn new(sr: f32, factor: usize) -> Self {
        let mut c = Self {
            sr,
            os: [Oversampler::new(factor), Oversampler::new(factor)],
            c: Cfg { pre: 1.0, thr: -18.0, inv: 0.5, knee: 6.0, att: 0.1, rel: 0.001, rel2: 0.0, feedback: false, progressive: false, even: 0.0, odd: 0.0, drive: 0.0, gr_drive: 0.0, opto: false, o_att: 0.0, o_rel: 0.0, o_rel2: 0.0, warmth: 0.0, tranny: 0.0, auto: 1.0, hp_a: 0.0 },
            gr: [0.0; 2],
            gr2: [0.0; 2],
            og: [0.0; 2],
            og2: [0.0; 2],
            fb_lvl: [1.0; 2],
            hp: [Lp1::default(); 2],
            dc: [Lp1::default(); 2],
            dc2: [Lp1::default(); 2],
            hf: [Lp1::default(); 2],
            hf_env: 0.0,
            lf: [Lp1::default(); 2],
            p_in: 0.0,
            p_out: 0.0,
            mk: 1.0,
            sc: [0.0; 2],
            gr_db: 0.0,
        };
        c.control(&[0.4, 0.5, 0.3, 0.4, 0.3, 1.0, 0.2], 0);
        c
    }
    pub fn latency(&self) -> usize {
        oversample::latency(self.os[0].factor())
    }
    pub fn reset(&mut self) {
        self.os.iter_mut().for_each(|o| o.clear());
        self.gr = [0.0; 2];
        self.gr2 = [0.0; 2];
        self.og = [0.0; 2];
        self.og2 = [0.0; 2];
        self.fb_lvl = [1.0; 2];
        self.sc = [0.0; 2];
        self.hp = [Lp1::default(); 2];
        self.dc = [Lp1::default(); 2];
        self.dc2 = [Lp1::default(); 2];
        self.hf = [Lp1::default(); 2];
        self.lf = [Lp1::default(); 2];
        self.hf_env = 0.0;
        self.p_in = 0.0;
        self.p_out = 0.0;
        self.mk = 1.0;
        self.gr_db = 0.0;
    }

    pub fn control(&mut self, k: &[f32; MAX_K], model: usize) {
        let sr = self.sr;
        let t = |ms: f32| tc(ms * 0.001, sr);
        let mut c = self.c;
        c.pre = if model_uses_input(model) { db(input_db(k[0])) } else { 1.0 };
        c.auto = k[5];
        c.hp_a = op_a(hpf_hz(k[6]), sr);
        c.opto = false;
        c.progressive = false;
        c.feedback = false;
        c.rel2 = 0.0;
        c.warmth = 0.0;
        c.tranny = 0.0;
        c.gr_drive = 0.0;
        match model {
            0 => {
                // vari-mu: fixed threshold, BIAS moves it and the knee
                let (att, rel, slow) = TIMES[step(6, k[2])];
                let (ev, od, kx) = TUBE_CHAR[step(7, k[3])];
                let bias = (k[1] - 0.5) * 2.0;
                c.thr = -4.0 - bias * 6.0;
                c.knee = (14.0 - bias * 4.0) * kx;
                c.inv = 0.6;
                c.progressive = true;
                c.feedback = true;
                c.att = t(att);
                c.rel = tc(rel, sr);
                c.rel2 = if slow > 0.0 { tc(slow, sr) } else { 0.0 };
                c.even = ev;
                c.odd = od;
                c.drive = 0.3 + 2.2 * k[4];
                c.gr_drive = 0.05;
            }
            1 => {
                let r = DS_RATIOS[step(8, k[1])];
                let nuke = r > 50.0;
                c.thr = 0.0;
                c.inv = 1.0 / r;
                c.knee = if nuke { 1.0 } else { 6.0 };
                c.att = t(ds_attack_ms(k[2]) * if nuke { 0.3 } else { 1.0 });
                c.rel = t(ds_release_ms(k[3]));
                let d2 = (k[4] * 2.0).min(1.0);
                let d3 = ((k[4] - 0.5) * 2.0).max(0.0);
                c.even = 0.35 * d2;
                c.odd = 0.9 * d3;
                c.drive = 0.15 + 2.5 * d2 + 2.0 * d3;
            }
            2 => {
                let mode = step(4, k[1]);
                let (r, att, rel, knee) = [(2.5, 5.0, 150.0, 8.0), (2.0, 30.0, 600.0, 12.0), (15.0, 0.5, 60.0, 3.0), (4.0, 2.0, 100.0, 6.0)][mode];
                c.thr = -2.0;
                c.inv = 1.0 / r;
                c.knee = knee;
                c.att = t(att);
                c.rel = t(rel);
                c.rel2 = if mode == 1 { tc(2.0, sr) } else { 0.0 };
                c.feedback = mode == 1;
                c.warmth = k[2];
                c.tranny = k[3];
                c.even = 0.12;
                c.odd = 0.5;
                c.drive = 0.2 + 2.6 * k[4] + if mode == 2 { 0.6 } else { 0.0 };
            }
            3 => {
                c.thr = thresh_db(k[0]);
                c.inv = alpha_inv(k[1]);
                c.knee = 4.0 + 4.0 * k[4];
                c.att = t(al_attack_ms(k[2]) * (1.0 + 0.6 * k[4]));
                c.rel = t(al_release_ms(k[3]));
                c.even = 0.18;
                c.odd = 0.25;
                c.drive = 0.05 + 1.8 * k[4];
            }
            _ => {
                let class_a = model == 5;
                c.thr = thresh_db(k[0]);
                c.inv = 1.0 / SH_RATIOS[step(6, k[1])];
                c.knee = if class_a { 9.0 } else { 6.0 };
                c.att = t(sh_attack_ms(k[2]));
                c.rel = t(sh_release_ms(k[3]));
                c.opto = true;
                let os = if class_a { 1.4 } else { 1.0 };
                c.o_att = t(10.0 * os);
                c.o_rel = t(60.0 * os);
                c.o_rel2 = tc(1.5 * os, sr);
                // NICKEL -> IRON -> STEEL
                let x = k[4];
                c.even = 0.05 + 0.25 * x + if class_a { 0.12 } else { 0.0 };
                c.odd = 0.3 + 0.5 * x;
                c.drive = 0.1 + 1.6 * x * x + 0.3 * x;
            }
        }
        self.c = c;
    }

    /// static curve: gain reduction (dB, >= 0 for compression) for `over` dB above threshold
    #[inline]
    fn curve(c: &Cfg, over: f32) -> f32 {
        let w = c.knee.max(0.1);
        let inv = if c.progressive {
            // vari-mu: ratio climbs with level (2:1 near threshold, ~20:1 well over)
            (c.inv / (1.0 + over.max(0.0) * 0.18)).max(0.04)
        } else {
            c.inv
        };
        let s = 1.0 - inv;
        if 2.0 * over < -w {
            0.0
        } else if 2.0 * over <= w {
            s * (over + w * 0.5).powi(2) / (2.0 * w)
        } else {
            s * over
        }
    }

    #[inline]
    fn color(c: &Cfg, x: f32, drive: f32) -> f32 {
        if drive < 0.01 {
            return x;
        }
        // asymmetric: even from the squared term, odd from tanh
        let d = drive;
        // normalise so small signals pass at unity (a 0.2 floor here used to cost low-drive settings up to 3 dB)
        let y = (d * (x + c.even * x * x)).tanh() / d.tanh().max(1e-3);
        x + (y - x) * (c.odd + c.even).min(1.0).max(0.25)
    }

    /// One channel's gain computer: detector level (dB) in, gain reduction (dB) out.
    #[inline]
    fn gain_db(&mut self, ch: usize, ldb: f32) -> f32 {
        let c = self.c;
        // opto stage (SH): slow, smooth, with memory
        let mut thr = c.thr;
        let mut og = 0.0;
        if c.opto {
            let t = Self::curve(&Cfg { inv: 0.4, knee: 10.0, progressive: false, ..c }, ldb - (c.thr + 4.0)).min(8.0);
            self.og[ch] += if t > self.og[ch] { c.o_att } else { c.o_rel } * (t - self.og[ch]);
            // the cell remembers: long stretches of reduction recover slowly
            self.og2[ch] += if self.og[ch] > self.og2[ch] { c.o_rel2 * 4.0 } else { c.o_rel2 } * (self.og[ch] - self.og2[ch]);
            og = self.og[ch].max(self.og2[ch] * 0.8);
            thr += og * 0.5;
        }
        let tgt = Self::curve(&c, ldb - thr);
        self.gr[ch] += if tgt > self.gr[ch] { c.att } else { c.rel } * (tgt - self.gr[ch]);
        let mut gr = self.gr[ch];
        if c.rel2 > 0.0 {
            // program dependent: a slow cell charges on sustained material
            self.gr2[ch] += if self.gr[ch] > self.gr2[ch] { c.rel2 * 6.0 } else { c.rel2 } * (self.gr[ch] - self.gr2[ch]);
            gr = gr.max(self.gr2[ch]);
        }
        (gr + og).clamp(-12.0, 48.0)
    }

    pub fn tick(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.tick_key(l, r, None, 1.0)
    }

    /// `key`: external sidechain sample (replaces the internal detector signal).
    /// `link`: 1 = both channels get the same gain, 0 = each channel on its own.
    pub fn tick_key(&mut self, l: f32, r: f32, key: Option<(f32, f32)>, link: f32) -> (f32, f32) {
        let c = self.c;
        let p_raw = 0.5 * (l * l + r * r);
        let (l, r) = (l * c.pre, r * c.pre);
        // FAT WARMTH: fast squash of the top end only
        let (l, r) = if c.warmth > 0.001 {
            let a = op_a(4500.0, self.sr);
            let h0 = l - self.hf[0].tick(l, a);
            let h1 = r - self.hf[1].tick(r, a);
            let e = h0.abs().max(h1.abs());
            self.hf_env += if e > self.hf_env { 0.3 } else { 0.0015 } * (e - self.hf_env);
            let g = 1.0 / (1.0 + c.warmth * 10.0 * self.hf_env);
            (l - h0 * (1.0 - g), r - h1 * (1.0 - g))
        } else {
            (l, r)
        };
        // detector: sidechain HPF. Internal: feedback models listen to the output.
        // External key: feed-forward, scaled by INPUT like the main signal would be.
        let d = match key {
            Some((ka, kb)) => [ka * c.pre, kb * c.pre],
            None if c.feedback => [l * self.fb_lvl[0], r * self.fb_lvl[1]],
            None => [l, r],
        };
        let h = [d[0] - self.hp[0].tick(d[0], c.hp_a), d[1] - self.hp[1].tick(d[1], c.hp_a)];
        let inv_pre = 1.0 / c.pre.max(1e-6);
        self.sc = [h[0] * inv_pre, h[1] * inv_pre];
        let own = [20.0 * h[0].abs().max(1e-6).log10(), 20.0 * h[1].abs().max(1e-6).log10()];
        let linked = own[0].max(own[1]);
        let link = link.clamp(0.0, 1.0);
        let mut g = [1.0f32; 2];
        let mut gr_max = -100.0f32;
        let chans = if link > 0.999 { 1 } else { 2 };
        for ch in 0..chans {
            let ldb = linked * link + own[ch] * (1.0 - link);
            let gr = self.gain_db(ch, ldb);
            gr_max = gr_max.max(gr);
            g[ch] = db(-gr);
        }
        if chans == 1 {
            // fully linked: one computer drives both (keep channel 1's state in step)
            g[1] = g[0];
            self.gr[1] = self.gr[0];
            self.gr2[1] = self.gr2[0];
            self.og[1] = self.og[0];
            self.og2[1] = self.og2[0];
        }
        self.gr_db = gr_max;
        let feedback = c.feedback && key.is_none();
        self.fb_lvl = if feedback { g } else { [1.0; 2] };
        let gr = gr_max;
        // FAT TRANNY: transformer low end
        let (mut a, mut b) = (l * g[0], r * g[1]);
        if c.tranny > 0.001 {
            let la = op_a(160.0, self.sr);
            let lo0 = self.lf[0].tick(a, la);
            let lo1 = self.lf[1].tick(b, la);
            a += (soft(lo0 * 2.0, 2.0) * 0.5 - lo0 * 0.5) * c.tranny * 2.0 + lo0 * c.tranny * 0.25;
            b += (soft(lo1 * 2.0, 2.0) * 0.5 - lo1 * 0.5) * c.tranny * 2.0 + lo1 * c.tranny * 0.25;
        }
        // colour, oversampled
        let drive = c.drive * (1.0 + c.gr_drive * gr.max(0.0));
        let f = self.os[0].factor();
        let mut out = [0.0f32; 2];
        for (ch, x) in [a, b].into_iter().enumerate() {
            if f == 1 {
                out[ch] = Self::color(&c, x, drive);
            } else {
                let mut buf = [0.0f32; MAX_FACTOR];
                self.os[ch].up(x, &mut buf);
                for v in buf.iter_mut().take(f) {
                    *v = Self::color(&c, *v, drive);
                }
                out[ch] = self.os[ch].down(&mut buf);
            }
            // DC from the even harmonics
            let y = out[ch];
            out[ch] = y - self.dc[ch].tick(y, 0.0007);
        }
        let _ = &self.dc2;
        // AUTO GAIN: comes out as loud as it went in (slow, so the squeeze still shows)
        let a = tc(1.0, self.sr);
        self.p_in += a * (p_raw - self.p_in);
        self.p_out += a * (0.5 * (out[0] * out[0] + out[1] * out[1]) - self.p_out);
        let mk_t = if self.p_in > 1e-9 && self.p_out > 1e-9 {
            let full = (self.p_in / self.p_out).sqrt().clamp(0.06, 16.0);
            full.powf(c.auto)
        } else {
            self.mk
        };
        self.mk += tc(0.08, self.sr) * (mk_t - self.mk);
        (out[0] * self.mk, out[1] * self.mk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(model: usize, k: [f32; MAX_K], amp: f32) -> (f32, f32) {
        let sr = 48000.0;
        let mut c = Comp::new(sr, 2);
        c.control(&k, model);
        let mut pk = 0.0f32;
        let mut gr = 0.0;
        for i in 0..96000 {
            let x = amp * (std::f32::consts::TAU * 220.0 * i as f32 / sr).sin();
            let (a, b) = c.tick(x, x);
            assert!(a.is_finite() && b.is_finite());
            if i > 72000 {
                pk = pk.max(a.abs());
                gr = c.gr_db;
            }
        }
        (pk, gr)
    }

    /// Every model compresses a loud sine, stays bounded and, with AUTO GAIN,
    /// comes out within a few dB of what went in.
    #[test]
    fn every_model_compresses_and_auto_gain_holds_level() {
        for m in 0..6 {
            let k = [0.6, 0.5, 0.3, 0.4, 0.3, 1.0, 0.1];
            let (pk, gr) = run(m, k, 0.7);
            assert!(gr > 1.0, "model {} gr {}", MODELS[m], gr);
            let out_db = 20.0 * pk.log10();
            let in_db = 20.0 * 0.7f32.log10();
            assert!((out_db - in_db).abs() < 6.0, "model {} in {in_db:.1} out {out_db:.1} gr {gr:.1}", MODELS[m]);
        }
    }

    /// Quiet signals pass (almost) untouched.
    #[test]
    fn quiet_passes() {
        for m in 0..6 {
            let (_, gr) = run(m, [0.2, 0.5, 0.3, 0.4, 0.0, 0.0, 0.1], 0.01);
            assert!(gr < 0.5, "model {} gr {}", MODELS[m], gr);
        }
    }
}
