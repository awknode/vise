//! Small DSP parts shared by the Patina modules.

use std::f32::consts::PI;
pub const TAU: f32 = 2.0 * PI;

#[derive(Clone)]
pub struct Rng(pub u32);
impl Rng {
    /// 0..1
    #[inline]
    pub fn u(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / 16_777_216.0
    }
    /// -1..1
    #[inline]
    pub fn b(&mut self) -> f32 {
        self.u() * 2.0 - 1.0
    }
}

/// FLUX: a smooth random wander (-1..1). More AMOUNT = it moves more often.
#[derive(Clone)]
pub struct Flux {
    rng: Rng,
    from: f32,
    to: f32,
    t: f32,
    dur: f32,
    pub v: f32,
}
impl Flux {
    pub fn new(seed: u32) -> Self {
        let mut f = Self { rng: Rng(seed | 1), from: 0.0, to: 0.0, t: 0.0, dur: 1.0, v: 0.0 };
        f.to = f.rng.b();
        f
    }
    /// advance `dt` seconds
    #[inline]
    pub fn step(&mut self, dt: f32, amount: f32) -> f32 {
        self.t += dt / self.dur;
        if self.t >= 1.0 {
            self.t = 0.0;
            self.from = self.to;
            self.to = self.rng.b();
            self.dur = (1.8 - 1.4 * amount) * (0.4 + 0.9 * self.rng.u());
        }
        let g = 0.5 - 0.5 * (std::f32::consts::PI * self.t).cos();
        self.v = self.from + (self.to - self.from) * g;
        self.v
    }
}

#[inline]
pub fn db(x: f32) -> f32 {
    10f32.powf(x / 20.0)
}
/// one-pole coefficient for a corner frequency
#[inline]
pub fn op_a(f: f32, sr: f32) -> f32 {
    1.0 - (-TAU * f.min(sr * 0.49) / sr).exp()
}
/// one-pole coefficient for a time constant
#[inline]
pub fn tc(sec: f32, sr: f32) -> f32 {
    1.0 - (-1.0 / (sec.max(1e-5) * sr)).exp()
}

#[derive(Clone, Copy, Default)]
pub struct Lp1 {
    pub z: f32,
}
impl Lp1 {
    #[inline]
    pub fn tick(&mut self, x: f32, a: f32) -> f32 {
        self.z += a * (x - self.z);
        self.z
    }
}

/// Attack/release envelope follower.
#[derive(Clone, Copy, Default)]
pub struct Env {
    pub e: f32,
    att: f32,
    rel: f32,
}
impl Env {
    pub fn new(att_s: f32, rel_s: f32, sr: f32) -> Self {
        Self { e: 0.0, att: tc(att_s, sr), rel: tc(rel_s, sr) }
    }
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let x = x.abs();
        self.e += if x > self.e { self.att } else { self.rel } * (x - self.e);
        self.e
    }
}

/// Simper / TPT state-variable filter.
#[derive(Clone, Copy, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    pub k: f32,
}
impl Svf {
    pub fn set(&mut self, f: f32, q: f32, sr: f32) {
        let g = (PI * f.clamp(10.0, sr * 0.45) / sr).tan();
        self.k = 1.0 / q.max(0.3);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }
    /// (low, band, high)
    #[inline]
    pub fn tick(&mut self, x: f32) -> (f32, f32, f32) {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1, x - self.k * v1 - v2)
    }
    pub fn clear(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

/// RBJ biquad (TDF2).
#[derive(Clone, Copy)]
pub struct Biq {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}
impl Default for Biq {
    fn default() -> Self {
        Self { b: [1.0, 0.0, 0.0], a: [0.0, 0.0], z: [0.0; 2] }
    }
}
impl Biq {
    fn wq(f: f32, q: f32, sr: f32) -> (f32, f32) {
        let w = TAU * (f / sr).clamp(1e-5, 0.49);
        let (s, c) = w.sin_cos();
        (s / (2.0 * q), c)
    }
    pub fn peak(&mut self, f: f32, q: f32, gain_db: f32, sr: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let (al, c) = Self::wq(f, q, sr);
        let a0 = 1.0 + al / a;
        self.b = [(1.0 + al * a) / a0, -2.0 * c / a0, (1.0 - al * a) / a0];
        self.a = [-2.0 * c / a0, (1.0 - al / a) / a0];
    }
    pub fn lowpass(&mut self, f: f32, q: f32, sr: f32) {
        let (al, c) = Self::wq(f, q, sr);
        let a0 = 1.0 + al;
        self.b = [(1.0 - c) / 2.0 / a0, (1.0 - c) / a0, (1.0 - c) / 2.0 / a0];
        self.a = [-2.0 * c / a0, (1.0 - al) / a0];
    }
    pub fn highpass(&mut self, f: f32, q: f32, sr: f32) {
        let (al, c) = Self::wq(f, q, sr);
        let a0 = 1.0 + al;
        self.b = [(1.0 + c) / 2.0 / a0, -(1.0 + c) / a0, (1.0 + c) / 2.0 / a0];
        self.a = [-2.0 * c / a0, (1.0 - al) / a0];
    }
    pub fn thru(&mut self) {
        self.b = [1.0, 0.0, 0.0];
        self.a = [0.0, 0.0];
    }
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
    pub fn clear(&mut self) {
        self.z = [0.0; 2];
    }
}

/// Delay line. `at(0)` is the newest sample pushed.
#[derive(Clone)]
pub struct Line {
    buf: Vec<f32>,
    mask: usize,
    w: usize,
}
impl Line {
    pub fn new(n: usize) -> Self {
        let size = (n + 8).next_power_of_two();
        Self { buf: vec![0.0; size], mask: size - 1, w: 0 }
    }
    /// longest usable delay
    pub fn cap(&self) -> usize {
        self.buf.len() - 4
    }
    #[inline]
    pub fn push(&mut self, x: f32) {
        self.w = (self.w + 1) & self.mask;
        self.buf[self.w] = x;
    }
    #[inline]
    pub fn at(&self, d: usize) -> f32 {
        self.buf[self.w.wrapping_sub(d) & self.mask]
    }
    #[inline]
    pub fn frac(&self, d: f32) -> f32 {
        let d = d.max(0.0);
        let i = d as usize;
        let f = d - i as f32;
        let a = self.at(i);
        a + (self.at(i + 1) - a) * f
    }
    /// 4-point Hermite (for pitch-moving reads)
    #[inline]
    pub fn cubic(&self, d: f32) -> f32 {
        let d = d.max(1.0);
        let i = d as usize;
        let f = d - i as f32;
        let (xm1, x0, x1, x2) = (self.at(i - 1), self.at(i), self.at(i + 1), self.at(i + 2));
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * f + c2) * f + c1) * f + x0
    }
    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|x| *x = 0.0);
    }
}

/// Schroeder allpass with a fixed length.
#[derive(Clone)]
pub struct Allpass {
    line: Line,
    pub n: usize,
}
impl Allpass {
    pub fn new(max: usize) -> Self {
        Self { line: Line::new(max), n: 1 }
    }
    #[inline]
    pub fn tick(&mut self, x: f32, g: f32) -> f32 {
        let v = self.line.at(self.n.max(1) - 1);
        let w = x + g * v;
        self.line.push(w);
        v - g * w
    }
    pub fn clear(&mut self) {
        self.line.clear();
    }
}

/// Soft clip with unity small-signal gain.
#[inline]
pub fn soft(x: f32, g: f32) -> f32 {
    if g <= 1.001 {
        x
    } else {
        (x * g).tanh() / g
    }
}

/// In-place 8-point Hadamard (orthonormal).
#[inline]
pub fn hadamard8(x: &mut [f32; 8]) {
    let mut h = 1;
    while h < 8 {
        let mut i = 0;
        while i < 8 {
            for j in i..i + h {
                let (a, b) = (x[j], x[j + h]);
                x[j] = a + b;
                x[j + h] = a - b;
            }
            i += h * 2;
        }
        h *= 2;
    }
    let s = 1.0 / 8f32.sqrt();
    x.iter_mut().for_each(|v| *v *= s);
}
