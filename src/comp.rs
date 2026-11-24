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
//! 76     1176 style FET: INPUT into a fixed threshold, 4 / 8 / 12 / 20 / ALL
//!        (all-buttons: faster release, more bite, more grit).
//! 2A     LA-2A style opto: PEAK RED. into a fixed threshold, COMP / LIMIT,
//!        EMPH (sidechain high-frequency emphasis), MEMORY (how long the T4 cell holds).
//!
//! v0.4: fitted against UnFairchild (670), Shadow Hills Mastering / Class A (SH-A / SH-B),
//! elysia alpha (ALPHA), UAD 1176 (76) and Waves CLA-2A (2A) with awk-bench.
//! COLOR now leaves small signals at unity (it used to add level as it was turned up).
//! DS and FAT keep their v0.3 colour exactly.
//!
//! Gain is worked out at the base rate (linked stereo); the colour stage runs
//! oversampled. Latency = the oversampler's (`latency()`).

use crate::dsp::*;
use crate::oversample::{self, Oversampler, MAX_FACTOR};
use crate::params::MAX_K;

pub const MODELS: [&str; 8] = ["670", "DS", "FAT", "ALPHA", "SH-A", "SH-B", "76", "2A"];
pub const N_MODELS: usize = 8;
pub const R76: [f32; 5] = [4.0, 8.0, 12.0, 20.0, 0.0];
pub const R76_NAMES: [&str; 5] = ["4:1", "8:1", "12:1", "20:1", "ALL"];
pub const TUBES: [&str; 7] = ["6386", "6386 NOS", "6BC8", "6BA6", "6BZ6", "5670", "6CG7"];
/// (even, odd, knee x) per tube set
const TUBE_CHAR: [(f32, f32, f32); 7] = [(0.10, 0.55, 1.0), (0.06, 0.45, 1.1), (0.22, 0.45, 0.9), (0.14, 0.7, 0.8), (0.08, 0.35, 1.3), (0.3, 0.6, 0.7), (0.18, 0.8, 1.0)];
pub const DS_RATIOS: [f32; 8] = [1.0, 2.0, 3.0, 4.0, 6.0, 10.0, 20.0, 100.0];
pub const DS_RATIO_NAMES: [&str; 8] = ["1:1", "2:1", "3:1", "4:1", "6:1", "10:1", "20:1", "NUKE"];
pub const FAT_MODES: [&str; 4] = ["GEN", "BUSS", "SPANK", "TRACK"];
pub const SH_RATIOS: [f32; 6] = [1.2, 1.5, 2.0, 3.0, 4.0, 6.0];
/// 670 time constants: (attack ms, release s, slow release s; 0 = not program dependent)
pub const TIMES: [(f32, f32, f32); 6] = [(0.2, 0.068, 0.0), (0.2, 0.114, 0.0), (0.4, 0.18, 0.0), (0.4, 1.4, 0.0), (0.4, 0.14, 10.0), (0.2, 0.03, 25.0)];

pub fn model_uses_input(model: usize) -> bool {
    matches!(model, 0 | 1 | 2 | 6 | 7)
}
/// INPUT drive into a fixed threshold. 0 = a hot track (peaks near 0 dBFS) is left alone.
pub const INPUT_MAX_DB: f32 = 36.0;
pub fn input_db(v: f32) -> f32 {
    v * INPUT_MAX_DB
}
/// 76 INPUT (0.4.1): same +36 dB top as before, but a curved law so the shared knob default (40%)
/// is 0 dB: a normalised 808 gets ~9 dB of peak reduction instead of slamming ~22 dB (+14.4 dB used
/// to sit at 40%). 0% = -12 dB (clean), 70% = +16 dB, 100% = +36 dB.
pub fn input76_db(v: f32) -> f32 {
    -12.0 + 48.0 * v.clamp(0.0, 1.0).powf(1.513)
}
pub fn model_input_db(model: usize, v: f32) -> f32 {
    if model == 6 { input76_db(v) } else { input_db(v) }
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
    0.1 * 1200f32.powf(v)
}
pub fn sh_release_ms(v: f32) -> f32 {
    50.0 * 24f32.powf(v)
}
/// 76: 20 us .. 0.8 ms
pub fn f76_attack_ms(v: f32) -> f32 {
    0.02 * 40f32.powf(v)
}
/// 76: fast .. slow (the hardware knob runs the other way; here bigger = longer, like every other model)
pub fn f76_release_ms(v: f32) -> f32 {
    130.0 * 13f32.powf(v.max(0.0).sqrt())
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
        (7, 0) => "PEAK RED",
        (_, 0) if model_uses_input(model) => "INPUT",
        (_, 0) => "THRESH",
        (0, 1) => "BIAS",
        (0, 2) => "TIME",
        (0, 3) => "TUBES",
        (1, 1) | (3, 1) | (4, 1) | (5, 1) => "RATIO",
        (2, 1) | (7, 1) => "MODE",
        (6, 1) => "RATIO",
        (7, 2) => "EMPH",
        (7, 3) => "MEMORY",
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
        (6, 0) => format!("{:+.1} dB", input76_db(v)),
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
        (6, 1) => R76_NAMES[step(5, v)].to_string(),
        (6, 2) => ms(f76_attack_ms(v)),
        (6, 3) => ms(f76_release_ms(v)),
        (7, 1) => (if v < 0.5 { "COMP" } else { "LIMIT" }).into(),
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
    /// v0.3 colour (DS, FAT): kept bit-exact
    legacy: bool,
    /// colour hears the source level (INPUT taken back out), like the hardware's output stage
    col_src: bool,
    /// vari-mu ratio growth per dB over
    prog: f32,
    /// slow cell: charge speed multiplier and how much of it holds the gain down
    rel2_chg: f32,
    rel2_w: f32,
    /// opto memory weight (SH)
    o_mem: f32,
    /// 2A sidechain high-frequency emphasis (0..1)
    emph: f32,
    /// extra smoothing on the gain (one-pole coefficient, 0 = off): rounds the corner of very fast attacks (FAT)
    gslew: f32,
    /// FAT WARMTH envelope attack coefficient
    w_att: f32,
    /// detector looks ahead by the comp's fixed delay (FAT): gain is already down when a note starts
    look: bool,
}

/// small fixed delay line (up to LEN-1 samples)
#[derive(Clone)]
pub struct Delay {
    buf: [f32; Delay::LEN],
    w: usize,
}
impl Default for Delay {
    fn default() -> Self {
        Self { buf: [0.0; Delay::LEN], w: 0 }
    }
}
impl Delay {
    pub const LEN: usize = 512;
    fn clear(&mut self) {
        self.buf = [0.0; Self::LEN];
    }
    #[inline]
    fn tick(&mut self, x: f32, n: usize) -> f32 {
        self.buf[self.w] = x;
        let r = (self.w + Self::LEN - n) % Self::LEN;
        self.w = (self.w + 1) % Self::LEN;
        self.buf[r]
    }
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
    emp: [Lp1; 2],
    /// slow loudness in / out (AUTO GAIN)
    p_in: f32,
    p_out: f32,
    /// AUTO GAIN makeup right now (linear)
    pub mk: f32,
    /// AUTO GAIN part 1: undoes INPUT exactly (follows the knob, no learning)
    pk: f32,
    /// AUTO GAIN part 2: learned make-up for the gain reduction (1 after reset)
    gk: f32,
    fresh: bool,
    gk_seed: f32,
    /// gain slew state (FAT)
    gs: [f32; 2],
    /// fixed 2 ms delay before the gain (lookahead for FAT; the others detect on the delayed signal)
    la: [Delay; 4],
    la_n: usize,
    /// learned make-up per model, so switching models lands at the right level (0 = not learned yet)
    gk_mem: [f32; N_MODELS],
    model: usize,
    /// seconds of fast relearning left after a model switch
    relearn: f32,
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
            c: Cfg { pre: 1.0, thr: -18.0, inv: 0.5, knee: 6.0, att: 0.1, rel: 0.001, rel2: 0.0, feedback: false, progressive: false, even: 0.0, odd: 0.0, drive: 0.0, gr_drive: 0.0, opto: false, o_att: 0.0, o_rel: 0.0, o_rel2: 0.0, warmth: 0.0, tranny: 0.0, auto: 1.0, hp_a: 0.0, legacy: false, col_src: false, prog: 0.18, rel2_chg: 6.0, rel2_w: 1.0, o_mem: 0.8, emph: 0.0, gslew: 0.0, w_att: 0.3, look: false },
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
            emp: [Lp1::default(); 2],
            p_in: 0.0,
            p_out: 0.0,
            mk: 1.0,
            pk: 1.0,
            gk: 1.0,
            fresh: true,
            gk_seed: 0.0,
            gs: [0.0; 2],
            la: [Delay::default(), Delay::default(), Delay::default(), Delay::default()],
            la_n: ((sr * 0.002).round() as usize).clamp(1, Delay::LEN - 1),
            gk_mem: [0.0; N_MODELS],
            model: 0,
            relearn: 0.0,
            sc: [0.0; 2],
            gr_db: 0.0,
        };
        c.control(&[0.4, 0.5, 0.3, 0.4, 0.3, 1.0, 0.2], 0);
        c
    }
    pub fn latency(&self) -> usize {
        oversample::latency(self.os[0].factor()) + self.la_n
    }
    /// learned AUTO GAIN make-up (saved with the project, so reopening sounds the same)
    pub fn learned(&self) -> f32 {
        self.gk
    }
    /// seed the make-up from a saved project (0 = unknown, estimate from the knobs)
    pub fn seed_learned(&mut self, v: f32) {
        if v.is_finite() && v > 0.0 {
            self.gk_seed = v.clamp(0.25, 8.0);
            self.gk = self.gk_seed;
        }
    }
    pub fn reset(&mut self) {
        self.os.iter_mut().for_each(|o| o.clear());
        self.gr = [0.0; 2];
        self.gr2 = [0.0; 2];
        self.og = [0.0; 2];
        self.og2 = [0.0; 2];
        self.gs = [0.0; 2];
        self.la.iter_mut().for_each(|d| d.clear());
        self.fb_lvl = [1.0; 2];
        self.sc = [0.0; 2];
        self.hp = [Lp1::default(); 2];
        self.dc = [Lp1::default(); 2];
        self.dc2 = [Lp1::default(); 2];
        self.hf = [Lp1::default(); 2];
        self.lf = [Lp1::default(); 2];
        self.emp = [Lp1::default(); 2];
        self.hf_env = 0.0;
        // AUTO GAIN state (p_in, p_out, pk, gk, mk) is kept on purpose: hosts reset on
        // play/stop, and relearning would make the first hit after play sound different.
        self.gr_db = 0.0;
    }

    pub fn control(&mut self, k: &[f32; MAX_K], model: usize) {
        let sr = self.sr;
        let t = |ms: f32| tc(ms * 0.001, sr);
        let mut c = self.c;
        c.pre = if model_uses_input(model) { db(model_input_db(model, k[0])) } else { 1.0 };
        c.auto = k[5];
        let fresh = self.fresh;
        c.hp_a = op_a(hpf_hz(k[6]), sr);
        c.opto = false;
        c.progressive = false;
        c.feedback = false;
        c.rel2 = 0.0;
        c.warmth = 0.0;
        c.tranny = 0.0;
        c.gr_drive = 0.0;
        c.legacy = false;
        c.col_src = false;
        c.prog = 0.18;
        c.rel2_chg = 6.0;
        c.rel2_w = 1.0;
        c.o_mem = 0.8;
        c.emph = 0.0;
        c.gslew = 0.0;
        c.w_att = 0.3;
        c.look = false;
        match model {
            0 => {
                // vari-mu: fixed threshold, BIAS moves it and the knee
                let (att, rel, slow) = TIMES[step(6, k[2])];
                let (ev, od, kx) = TUBE_CHAR[step(7, k[3])];
                // v0.4: more BIAS (DC) = less compression, as on the Fairchild
                let bias = (k[1] - 0.5) * 2.0;
                c.thr = -4.0 + bias * 6.0;
                c.knee = (14.0 + bias * 4.0) * kx;
                c.inv = 0.6;
                c.progressive = true;
                c.prog = 0.07;
                c.feedback = true;
                c.att = t(att);
                c.rel = tc(rel, sr);
                c.rel2 = if slow > 0.0 { tc(slow, sr) } else { 0.0 };
                c.rel2_chg = 1.0;
                c.rel2_w = 0.6;
                c.even = ev * 0.3;
                c.odd = od;
                c.drive = 0.3 + 2.2 * k[4];
                c.gr_drive = 0.05;
                c.col_src = true;
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
                c.legacy = true;
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
                c.legacy = true;
                // 0.4.1: round the gain's corner a little (0.4 ms) and give WARMTH a 1 ms attack.
                c.gslew = t(0.4);
                c.w_att = t(1.0);
                // ...and the make-up hit the first ~1 ms before the gain came down (+7 dB overshoot
                // per note). FAT now looks 2 ms ahead; every model has that delay so latency is constant.
                c.look = true;
            }
            3 => {
                c.thr = thresh_db(k[0]);
                c.inv = alpha_inv(k[1]);
                c.knee = 4.0 + 4.0 * k[4];
                c.att = t(al_attack_ms(k[2]) * (1.0 + 0.6 * k[4]));
                c.rel = t(al_release_ms(k[3]));
                // elysia alpha is very clean: COLOR 0 = clean, 100 = WARM
                c.even = 0.06;
                c.odd = 0.25;
                c.drive = 0.02 + 1.2 * k[4];
            }
            6 => {
                // 76: INPUT into a fixed threshold
                let i = step(5, k[1]);
                let all = i == 4;
                c.thr = -12.0;
                c.inv = if all { -0.03 } else { 1.0 / R76[i] };
                c.knee = [6.0, 4.0, 3.0, 2.0, 2.0][i];
                c.att = t(f76_attack_ms(k[2]));
                c.rel = t(f76_release_ms(k[3]) * if all { 0.07 } else { 1.0 });
                // the 1176's grit comes from INPUT driving its amp: colour hears the driven level
                // (UAD 1176 at -12 dBFS with heavy GR: 2nd -44, 3rd -52; ALL: -34 / -35)
                c.even = if all { 0.18 } else { 0.12 };
                c.odd = if all { 0.45 } else { 0.3 };
                c.drive = 0.4 + 1.4 * k[4] + if all { 0.6 } else { 0.0 };
            }
            7 => {
                // 2A: T4 opto. PEAK RED. into a fixed threshold, 10 ms attack,
                // fast first release then a slow, program dependent tail (MEMORY)
                // fitted to Waves CLA-2A: ~3.7:1 (LIMIT ~20:1), 8-20 ms attack,
                // ~200 ms first release then a ~1 s tail (MEMORY 50%)
                let limit = k[1] >= 0.5;
                c.thr = -8.0;
                c.inv = if limit { 0.05 } else { 0.27 };
                c.knee = if limit { 6.0 } else { 8.0 };
                c.att = t(12.0);
                c.rel = t(60.0);
                c.rel2 = tc(0.15 + 1.0 * k[3], sr);
                c.rel2_chg = 2.0;
                c.rel2_w = 0.5;
                c.emph = k[2].max(0.002);
                c.even = 0.15;
                c.odd = 0.2;
                c.drive = 0.3 + 1.4 * k[4];
                c.col_src = true;
            }
            _ => {
                let class_a = model == 5;
                c.thr = thresh_db(k[0]);
                // the Class A's discrete stage squeezes about half as hard
                let r = SH_RATIOS[step(6, k[1])];
                c.inv = 1.0 / if class_a { 1.0 + (r - 1.0) * 0.45 } else { r };
                c.knee = if class_a { 9.0 } else { 6.0 };
                c.att = t(sh_attack_ms(k[2]));
                c.rel = t(sh_release_ms(k[3]));
                c.opto = true;
                // the opto lets go in 50..300 ms like the plug-ins (v0.3 held ~2 s)
                let os = if class_a { 2.0 } else { 1.0 };
                c.o_att = t(2.0 * os);
                c.o_rel = t(30.0 * os);
                c.o_rel2 = tc(0.3 * os, sr);
                c.o_mem = 0.1;
                // NICKEL -> IRON -> STEEL: subtle, as on the plug-ins
                let x = k[4];
                c.even = 0.02 + 0.08 * x + if class_a { 0.03 } else { 0.0 };
                c.odd = 0.3 + 0.4 * x;
                c.drive = 0.3 + 1.2 * x;
            }
        }
        if !fresh && model != self.model {
            // model switch: the learned make-up belonged to the old model (76 needs ~+15 dB, SH-B ~+5),
            // so carrying it over made the first notes far too loud or quiet. Remember it per model,
            // start the new one from its memory (or an estimate from the real input level), relearn fast.
            self.gk_mem[self.model] = self.gk;
            let old_pre = self.c.pre.max(1e-6);
            let in_rms_db = if self.p_in > 1e-10 { 10.0 * (self.p_in / (old_pre * old_pre)).log10() } else { -18.0 };
            let g = if self.gk_mem[model] > 0.0 { self.gk_mem[model] } else { Self::estimate_gk(&c, in_rms_db) };
            self.gk = g;
            self.pk = (1.0 / c.pre.max(1e-6)).powf(c.auto);
            // keep the followers consistent with the new make-up until real data arrives
            self.p_in = self.p_in / (old_pre * old_pre) * c.pre * c.pre;
            self.p_out = self.p_in / (g * g).max(1e-6);
            self.mk = self.pk * self.gk;
            self.relearn = 1.0;
        }
        self.model = model;
        if fresh {
            // first load: start already compensating INPUT, and guess the squeeze make-up
            // from the static curve at a typical level (-14 dBFS RMS, kick-ish crest +10 dB)
            self.pk = (1.0 / c.pre.max(1e-6)).powf(c.auto);
            let lvl = -4.0 + 20.0 * c.pre.max(1e-6).log10();
            let gr = Self::curve(&c, lvl - c.thr).clamp(0.0, 24.0) * 0.6;
            if self.gk_seed > 0.0 {
                self.gk = self.gk_seed;
            } else {
                self.gk = db(gr).powf(c.auto);
            }
            self.mk = self.pk * self.gk;
            self.fresh = false;
        }
        self.c = c;
    }

    /// make-up guess for a model from the static curve: peak of a bass note ~ RMS + 4 dB, average
    /// reduction ~55% of the peak's. Only used the first time a model is picked in an instance.
    fn estimate_gk(c: &Cfg, in_rms_db: f32) -> f32 {
        let pk = in_rms_db + 4.0 + 20.0 * c.pre.max(1e-6).log10();
        let gr = Self::curve(c, pk - c.thr).clamp(0.0, 24.0) * 0.55;
        db(gr).clamp(0.25, 16.0).powf(c.auto)
    }

    /// static curve: gain reduction (dB, >= 0 for compression) for `over` dB above threshold
    #[inline]
    fn curve(c: &Cfg, over: f32) -> f32 {
        let w = c.knee.max(0.1);
        let inv = if c.progressive {
            // vari-mu: ratio climbs with level (2:1 near threshold, ~20:1 well over)
            (c.inv / (1.0 + over.max(0.0) * c.prog)).max(0.04)
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
        if !c.legacy {
            // unity for small signals; the even part grows with drive
            let d = drive;
            let e = c.even * d.min(2.0) * 0.5;
            let y = (d * (x + e * x * x)).tanh() / d;
            return x + (y - x) * (c.odd + c.even).min(1.0).max(0.25);
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
            og = self.og[ch].max(self.og2[ch] * c.o_mem);
            thr += og * 0.5;
        }
        let tgt = Self::curve(&c, ldb - thr);
        self.gr[ch] += if tgt > self.gr[ch] { c.att } else { c.rel } * (tgt - self.gr[ch]);
        let mut gr = self.gr[ch];
        if c.rel2 > 0.0 {
            // program dependent: a slow cell charges on sustained material
            self.gr2[ch] += if self.gr[ch] > self.gr2[ch] { c.rel2 * c.rel2_chg } else { c.rel2 } * (self.gr[ch] - self.gr2[ch]);
            gr = gr.max(self.gr2[ch] * c.rel2_w);
        }
        let g = (gr + og).clamp(-12.0, 48.0);
        if c.gslew > 0.0 {
            self.gs[ch] += c.gslew * (g - self.gs[ch]);
            return self.gs[ch];
        }
        g
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
            self.hf_env += if e > self.hf_env { c.w_att } else { 0.0015 } * (e - self.hf_env);
            let g = 1.0 / (1.0 + c.warmth * 10.0 * self.hf_env);
            (l - h0 * (1.0 - g), r - h1 * (1.0 - g))
        } else {
            (l, r)
        };
        // fixed delay: the audio (and, for non-lookahead models, everything the detector hears)
        let n = self.la_n;
        let (dl, dr) = (self.la[0].tick(l, n), self.la[1].tick(r, n));
        let key_d = key.map(|(ka, kb)| (self.la[2].tick(ka, n), self.la[3].tick(kb, n)));
        let (sl, sr_) = if c.look { (l, r) } else { (dl, dr) };
        let key = if c.look { key } else { key_d };
        let (l, r) = (dl, dr);
        // detector: sidechain HPF. Internal: feedback models listen to the output.
        // External key: feed-forward, scaled by INPUT like the main signal would be.
        let d = match key {
            Some((ka, kb)) => [ka * c.pre, kb * c.pre],
            None if c.feedback => [sl * self.fb_lvl[0], sr_ * self.fb_lvl[1]],
            None => [sl, sr_],
        };
        let mut h = [d[0] - self.hp[0].tick(d[0], c.hp_a), d[1] - self.hp[1].tick(d[1], c.hp_a)];
        if c.emph > 0.001 {
            // 2A EMPH (like the CLA-2A's HI FREQ): sidechain sensitivity -7..+7 dB around 50%,
            // plus the top end heard a little louder as it goes up
            let a = op_a(1500.0, self.sr);
            let g = db((c.emph - 0.5) * 14.0);
            for ch in 0..2 {
                let lo = self.emp[ch].tick(h[ch], a);
                h[ch] = (h[ch] + (h[ch] - lo) * c.emph) * g;
            }
        }
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
            self.gs[1] = self.gs[0];
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
        let (cs_in, cs_out) = if c.col_src { (1.0 / c.pre.max(1e-6), c.pre) } else { (1.0, 1.0) };
        let (a, b) = (a * cs_in, b * cs_in);
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
            let y = out[ch] * cs_out;
            out[ch] = y - self.dc[ch].tick(y, 0.0007);
        }
        let _ = &self.dc2;
        // AUTO GAIN, two parts:
        //  pk = 1/INPUT, exact and immediate (so a hot INPUT never reaches the output
        //       before the follower has learned anything, e.g. the first hit after reset)
        //  gk = learned make-up for the squeeze only, measured against the INPUT-scaled
        //       signal, frozen while the input is silent (no drift in gaps)
        let pk_t = (1.0 / c.pre.max(1e-6)).powf(c.auto);
        self.pk += tc(0.02, self.sr) * (pk_t - self.pk);
        let fast = self.relearn > 0.0;
        if p_raw > 1e-8 {
            if fast {
                self.relearn -= 1.0 / self.sr;
            }
            let a = tc(if fast { 0.15 } else { 1.0 }, self.sr);
            self.p_in += a * (p_raw * c.pre * c.pre - self.p_in);
            self.p_out += a * (0.5 * (out[0] * out[0] + out[1] * out[1]) - self.p_out);
        }
        let gk_t = if self.p_in > 1e-9 && self.p_out > 1e-9 {
            (self.p_in / self.p_out).sqrt().clamp(0.25, 16.0).powf(c.auto)
        } else {
            self.gk
        };
        self.gk += tc(if fast { 0.03 } else { 0.08 }, self.sr) * (gk_t - self.gk);
        self.mk = self.pk * self.gk;
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
        for m in 0..N_MODELS {
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
        for m in 0..N_MODELS {
            let (_, gr) = run(m, [0.2, 0.5, 0.3, 0.4, 0.0, 0.0, 0.1], 0.01);
            assert!(gr < 0.5, "model {} gr {}", MODELS[m], gr);
        }
    }

    /// FIT=1 cargo test --release fit_report -- --nocapture
    /// Same measurements as awk-bench comp_bench.py: GR at -42..0 dBFS (200 Hz staircase,
    /// relative to the -42 dB gain) and a -36 -> -6 -> -36 dBFS step at 2 kHz.
    pub fn fit(model: usize, k: [f32; MAX_K]) -> (Vec<f32>, f32, i32, i32, Option<i32>, Option<i32>, f32, f32) {
        let sr = 48000.0;
        let seg = 24000;
        let lv: Vec<f32> = (0..15).map(|i| -42.0 + 3.0 * i as f32).collect();
        let mut c = Comp::new(sr, 2);
        c.control(&k, model);
        let mut gains = vec![];
        let mut n = 0usize;
        let mut h2 = (0.0f32, 0.0f32, 0.0f32);
        for (li, &l) in lv.iter().enumerate() {
            let a = 10f32.powf(l / 20.0) * std::f32::consts::SQRT_2;
            let (mut si, mut so) = (0.0f64, 0.0f64);
            for i in 0..seg {
                let x = a * (std::f32::consts::TAU * 200.0 * n as f32 / sr).sin();
                n += 1;
                let (y, _) = c.tick(x, x);
                if i >= seg / 2 {
                    si += (x * x) as f64;
                    so += (y * y) as f64;
                    if li == 10 {
                        // -12 dBFS: 2nd / 3rd harmonic by correlation
                        let ph = std::f32::consts::TAU * 200.0 * (n - 1) as f32 / sr;
                        h2.0 += y * ph.sin();
                        h2.1 += y * (2.0 * ph).cos() + 0.0;
                        h2.2 += y * (3.0 * ph).sin();
                    }
                }
            }
            gains.push(10.0 * (so / si).log10() as f32);
        }
        let g0 = gains[0];
        let gr: Vec<f32> = gains.iter().map(|g| g - g0).collect();
        let _ = h2;
        // step
        let mut c = Comp::new(sr, 2);
        c.control(&k, model);
        let total = 48000 + 72000 + 144000;
        let mut env = vec![];
        let (mut si, mut so) = (0.0f32, 0.0f32);
        let lat = c.latency();
        let xs: Vec<f32> = (0..total + lat)
            .map(|i| {
                let l = if (48000..120000).contains(&i) { -6.0 } else { -36.0 };
                let a = 10f32.powf(l / 20.0) * std::f32::consts::SQRT_2;
                a * (std::f32::consts::TAU * 2000.0 * i as f32 / sr).sin()
            })
            .collect();
        // feed the input `lat` samples early so each output lines up with xs[i]
        for i in 0..total {
            let (y, _) = c.tick(xs[i + lat], xs[i + lat]);
            let (x, y) = if i >= lat { (xs[i], y) } else { (0.0, 0.0) };
            si += x * x;
            so += y * y;
            if i % 96 == 95 {
                env.push(10.0 * (so / si).log10());
                si = 0.0;
                so = 0.0;
            }
        }
        let mut pre: Vec<f32> = env[200..490].to_vec();
        pre.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let base = pre[pre.len() / 2];
        let env: Vec<f32> = env.iter().map(|e| e - base).collect();
        let (on, off) = (500usize, 1250usize);
        let mut fin_v: Vec<f32> = env[off - 60..off - 5].to_vec();
        fin_v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let fin = fin_v[fin_v.len() / 2];
        let first = |s: &[f32], f: &dyn Fn(f32) -> bool| s.iter().position(|v| f(*v)).map(|p| p as i32 * 2);
        let a63 = first(&env[on..off], &|v| v <= 0.63 * fin).unwrap_or(-1);
        let a90 = first(&env[on..off], &|v| v <= 0.9 * fin).unwrap_or(-1);
        let r63 = first(&env[off..], &|v| v >= 0.37 * fin);
        let r90 = first(&env[off..], &|v| v >= 0.1 * fin);
        (gr, fin, a63, a90, r63, r90, g0, 0.0)
    }

    #[test]
    fn fit_report() {
        if std::env::var("FIT").is_err() {
            return;
        }
        let show = |name: &str, model: usize, k: [f32; MAX_K]| {
            let (gr, fin, a63, a90, r63, r90, g0, _) = fit(model, k);
            let g: Vec<String> = (0..15).step_by(2).map(|i| format!("{:.1}", gr[i])).collect();
            println!("{name:18} mk {g0:5.1} GR [{}] st {fin:6.1} a63 {a63} a90 {a90} r63 {r63:?} r90 {r90:?}", g.join(", "));
        };
        let spec = std::env::var("FIT").unwrap();
        for (mi, mn) in MODELS.iter().enumerate() {
            if !spec.split(',').any(|s| s == *mn || s == "all") {
                continue;
            }
            match mi {
                0 => {
                    for i in [0.0, 0.25, 0.5, 0.75, 1.0] { show(&format!("670 in{i}"), 0, [i, 0.5, 0.2, 0.0, 0.3, 0.0, 0.0]); }
                    for tcn in 0..6 { show(&format!("670 tc{}", tcn + 1), 0, [0.6, 0.5, tcn as f32 * 0.2, 0.0, 0.3, 0.0, 0.0]); }
                    for b in [0.0, 0.5, 1.0] { show(&format!("670 bias{b}"), 0, [0.5, b, 0.2, 0.0, 0.3, 0.0, 0.0]); }
                }
                3 => {
                    for th in [0.0, 0.25, 0.5, 0.75, 1.0] { show(&format!("ALPHA thr{th}"), 3, [th, 0.4, 0.3, 0.4, 0.3, 0.0, 0.0]); }
                }
                4 | 5 => {
                    for th in [0.0, 0.25, 0.5, 0.75, 1.0] { show(&format!("{mn} thr{th}"), mi, [th, 0.6, 0.4, 0.4, 0.3, 0.0, 0.0]); }
                    for a in [0.0, 0.33, 0.66, 1.0] { show(&format!("{mn} att{a}"), mi, [0.5, 0.6, a, 0.4, 0.3, 0.0, 0.0]); }
                    for rl in [0.0, 0.33, 0.66, 1.0] { show(&format!("{mn} rel{rl}"), mi, [0.5, 0.6, 0.4, rl, 0.3, 0.0, 0.0]); }
                }
                6 => {
                    for i in [0.0, 0.25, 0.5, 0.75, 1.0] { show(&format!("76 in{i}"), 6, [i, 0.0, 0.5, 0.5, 0.3, 0.0, 0.0]); }
                    for r in 0..5 { show(&format!("76 ratio{}", R76_NAMES[r]), 6, [0.55, r as f32 * 0.25, 0.5, 0.5, 0.3, 0.0, 0.0]); }
                    for rl in [0.0, 0.5, 1.0] { show(&format!("76 rel{rl}"), 6, [0.55, 0.0, 0.5, rl, 0.3, 0.0, 0.0]); }
                }
                7 => {
                    for i in [0.0, 0.25, 0.5, 0.75, 1.0] { show(&format!("2A pr{i}"), 7, [i, 0.0, 0.5, 0.5, 0.3, 0.0, 0.0]); }
                    show("2A limit", 7, [0.5, 1.0, 0.5, 0.5, 0.3, 0.0, 0.0]);
                    for e in [0.0, 1.0] { show(&format!("2A emph{e}"), 7, [0.5, 0.0, e, 0.5, 0.3, 0.0, 0.0]); }
                }
                _ => {}
            }
        }
    }
    thread_local!(static DEC808: std::cell::Cell<f32> = std::cell::Cell::new(0.45));
    /// synthetic 808: 120 -> 48 Hz pitch drop, -1 dBFS peak, 0.45 s decay, one hit per 0.5 s
    fn hit808(i: usize, sr: f32) -> f32 {
        let per = (sr * 0.5) as usize;
        let n = (i % per) as f32 / sr;
        let f = 48.0 + 72.0 * (-n / 0.012).exp();
        let ph = std::f32::consts::TAU * (48.0 * n + 72.0 * 0.012 * (1.0 - (-n / 0.012).exp()));
        let _ = f;
        0.89 * (-n / DEC808.with(|d| d.get())).exp() * ph.sin()
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt().max(1e-9)
    }

    /// V808=1 cargo test --release --lib v808_report -- --nocapture
    /// default knobs on an 808: GR, onset HF (click), and the level of the first hit after a model switch.
    #[test]
    fn v808_report() {
        if std::env::var("V808").is_err() {
            return;
        }
        let sr = 48000.0;
        let per = (sr * 0.5) as usize;
        let k = [0.4, 0.5, 0.3, 0.4, 0.3, 1.0, 0.2];
        for m in 0..N_MODELS {
            let mut c = Comp::new(sr, 2);
            c.control(&k, m);
            let (mut gmax, mut ys, mut xs) = (0.0f32, vec![], vec![]);
            for i in 0..per * 8 {
                let x = hit808(i, sr);
                let (y, _) = c.tick(x, x);
                if i >= per * 7 {
                    gmax = gmax.max(c.gr_db);
                    ys.push(y);
                    xs.push(x);
                }
            }
            // onset HF: 1st-order HP at 2 kHz, first 15 ms, vs the dry
            let hf = |v: &[f32]| {
                let a = (-std::f32::consts::TAU * 2000.0 / sr).exp();
                let (mut lp, mut e) = (0.0f32, 0.0f32);
                for &x in &v[..(sr * 0.015) as usize] {
                    lp = a * lp + (1.0 - a) * x;
                    e += (x - lp) * (x - lp);
                }
                10.0 * e.max(1e-12).log10()
            };
            let lat = c.latency();
            // short hits with gaps: the compressor fully lets go, like separate 808 notes
            let hf3 = |v: &[f32]| {
                let a = (-std::f32::consts::TAU * 3000.0 / sr).exp();
                let (mut l1, mut l2, mut e) = (0.0f32, 0.0f32, 0.0f32);
                for &x in &v[..(sr * 0.003) as usize] {
                    l1 = a * l1 + (1.0 - a) * x;
                    let h1 = x - l1;
                    l2 = a * l2 + (1.0 - a) * h1;
                    e += (h1 - l2) * (h1 - l2);
                }
                10.0 * e.max(1e-14).log10()
            };
            DEC808.with(|d| d.set(0.12));
            let mut c2 = Comp::new(sr, 2);
            c2.control(&k, m);
            let (mut y2, mut x2) = (vec![], vec![]);
            for i in 0..per * 8 {
                let x = hit808(i, sr);
                let (y, _) = c2.tick(x, x);
                if i >= per * 7 {
                    y2.push(y);
                    x2.push(x);
                }
            }
            DEC808.with(|d| d.set(0.45));
            let in_db = 10.0 * (c.p_in / (c.c.pre * c.c.pre)).log10();
            print!("gk {:5.1} est {:5.1} dB ", 20.0 * c.gk.log10(), 20.0 * Comp::estimate_gk(&c.c, in_db).log10());
            print!("  click(3ms>3k) {:6.1} vs dry {:6.1}  ", hf3(&y2[lat..]), hf3(&x2));
            let w = (sr * 0.015) as usize;
            let e = |v: &[f32]| 20.0 * rms(&v[..w]).log10();
            let rel_y = hf(&ys[lat..]) - 10.0 * (w as f32).log10() - e(&ys[lat..]);
            let rel_x = hf(&xs) - 10.0 * (w as f32).log10() - e(&xs);
            println!("{:5} GRmax {:5.1} dB  out-in {:+5.1} dB  onset HF/level {:6.1} dB (dry {:6.1})", MODELS[m], gmax, 20.0 * (rms(&ys) / rms(&xs)).log10(), rel_y, rel_x);
        }
        // switch matrix: 4 s on A, then B. first hit level vs settled (hit 8) level, dB
        print!("switch A->B first-hit jump (dB)\n      ");
        for b in 0..N_MODELS {
            print!("{:>6}", MODELS[b]);
        }
        println!();
        for a in 0..N_MODELS {
            print!("{:5} ", MODELS[a]);
            for b in 0..N_MODELS {
                let mut c = Comp::new(sr, 2);
                c.control(&k, a);
                for i in 0..per * 8 {
                    let x = hit808(i, sr);
                    c.tick(x, x);
                }
                c.control(&k, b);
                let mut hits = vec![];
                for h in 0..10 {
                    let mut y = vec![];
                    for i in 0..per {
                        let x = hit808(i, sr);
                        y.push(c.tick(x, x).0);
                    }
                    hits.push(20.0 * rms(&y).log10());
                    let _ = h;
                }
                print!("{:+6.1}", hits[0] - hits[9]);
            }
            println!();
        }
    }

    #[test]
    fn fat_click_diag() {
        if std::env::var("V808").is_err() {
            return;
        }
        let sr = 48000.0;
        let per = (sr * 0.5) as usize;
        DEC808.with(|d| d.set(0.12));
        let hf3 = |v: &[f32]| {
            let a = (-std::f32::consts::TAU * 3000.0 / sr).exp();
            let (mut l1, mut l2, mut e) = (0.0f32, 0.0f32, 0.0f32);
            for &x in &v[..(sr * 0.003) as usize] {
                l1 = a * l1 + (1.0 - a) * x;
                let h1 = x - l1;
                l2 = a * l2 + (1.0 - a) * h1;
                e += (h1 - l2) * (h1 - l2);
            }
            10.0 * e.max(1e-14).log10()
        };
        for (name, k) in [
            ("default", [0.4, 0.5, 0.3, 0.4, 0.3, 1.0, 0.2]),
            ("color 0", [0.4, 0.5, 0.3, 0.4, 0.0, 1.0, 0.2]),
            ("warmth 0", [0.4, 0.5, 0.0, 0.4, 0.3, 1.0, 0.2]),
            ("tranny 0", [0.4, 0.5, 0.3, 0.0, 0.3, 1.0, 0.2]),
            ("all 0", [0.4, 0.5, 0.0, 0.0, 0.0, 1.0, 0.2]),
            ("GEN", [0.4, 0.0, 0.3, 0.4, 0.3, 1.0, 0.2]),
            ("BUSS", [0.4, 0.34, 0.3, 0.4, 0.3, 1.0, 0.2]),
            ("TRACK", [0.4, 1.0, 0.3, 0.4, 0.3, 1.0, 0.2]),
        ] {
            let mut c = Comp::new(sr, 2);
            c.control(&k, 2);
            let mut y = vec![];
            for i in 0..per * 8 {
                let (o, _) = c.tick(hit808(i, sr), 0.0);
                if i >= per * 7 {
                    y.push(o);
                }
            }
            let lat = c.latency();
            // first 3 ms vs a later 3 ms window (the body of the note)
            if name == "default" || name == "BUSS" {
                let mut f = String::new();
                for v in &y[lat..lat + 2400] {
                    f.push_str(&format!("{v}\n"));
                }
                std::fs::write(format!("/src/target/fat_{name}.txt"), f).unwrap();
                let mut f = String::new();
                for i in 0..2400 {
                    f.push_str(&format!("{}\n", hit808(i, sr)));
                }
                std::fs::write("/src/target/dry808.txt", f).unwrap();
            }
            println!("FAT {name:9} onset {:6.1}  body(20ms) {:6.1}", hf3(&y[lat..]), hf3(&y[lat + 960..]));
        }
        DEC808.with(|d| d.set(0.45));
    }

}
