//! Oversampling for awk studios plugins (same file in each project that uses it;
//! master copy in ~/Projects/awk-ui/oversample.rs).
//!
//! Cascaded 2x stages of linear-phase halfband FIR filters (Kaiser windowed sinc).
//! Stage lengths 49 / 17 / 9 are chosen so the total delay is a whole number of
//! base-rate samples, which lets the rest of a plugin line up with a plain delay:
//!     factor 1: 0    factor 2: 24    factor 4: 28    factor 8: 29   samples.
//! Per channel: `up(x)` gives `factor` samples at the high rate, process them,
//! then `down(&buf)` gives one base-rate sample back.

#![allow(dead_code)]

pub const MAX_FACTOR: usize = 8;
const LENS: [usize; 3] = [49, 17, 9];

/// Factor for the OVERSAMPLING param value (0 = OFF, 1 = 2x, 2 = 4x, 3 = 8x).
pub fn factor_for(os: i32) -> usize {
    1 << os.clamp(0, 3)
}

/// Delay in base-rate samples added by up + down at `factor`.
pub fn latency(factor: usize) -> usize {
    let stages = factor.trailing_zeros() as usize;
    let mut d = 0.0f64;
    for (s, len) in LENS.iter().enumerate().take(stages) {
        // up + down at stage s: (len - 1) samples at 2^(s+1) x the base rate.
        d += (len - 1) as f64 / (1u64 << (s + 1)) as f64;
    }
    d.round() as usize
}

fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let y = x * x / 4.0;
    for k in 1..50 {
        term *= y / (k as f64 * k as f64);
        sum += term;
        if term < 1e-12 * sum {
            break;
        }
    }
    sum
}

/// Halfband lowpass (cutoff at a quarter of the high rate), unity DC gain.
fn design(len: usize) -> Vec<f32> {
    let m = (len - 1) as f64 / 2.0;
    let beta = 8.0;
    let mut h: Vec<f64> = (0..len)
        .map(|n| {
            let x = n as f64 - m;
            let sinc = if x == 0.0 { 0.5 } else { (std::f64::consts::PI * 0.5 * x).sin() / (std::f64::consts::PI * x) };
            let r = x / m;
            let w = bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / bessel_i0(beta);
            sinc * w
        })
        .collect();
    let s: f64 = h.iter().sum();
    h.iter_mut().for_each(|v| *v /= s);
    h.into_iter().map(|v| v as f32).collect()
}

#[derive(Clone)]
struct Up {
    even: Vec<f32>,
    odd: Vec<f32>,
    hist: Vec<f32>,
    pos: usize,
}
impl Up {
    fn new(len: usize) -> Self {
        let h = design(len);
        let even: Vec<f32> = h.iter().step_by(2).map(|v| v * 2.0).collect();
        let odd: Vec<f32> = h.iter().skip(1).step_by(2).map(|v| v * 2.0).collect();
        let n = even.len().max(odd.len());
        Self { even, odd, hist: vec![0.0; n * 2], pos: 0 }
    }
    fn clear(&mut self) {
        self.hist.iter_mut().for_each(|s| *s = 0.0);
    }
    #[inline]
    fn push(&mut self, x: f32) -> (f32, f32) {
        // Double-written ring so the newest-first window is contiguous.
        let n = self.hist.len() / 2;
        self.pos = (self.pos + n - 1) % n;
        self.hist[self.pos] = x;
        self.hist[self.pos + n] = x;
        let w = &self.hist[self.pos..self.pos + n];
        let a: f32 = self.even.iter().zip(w).map(|(c, s)| c * s).sum();
        let b: f32 = self.odd.iter().zip(w).map(|(c, s)| c * s).sum();
        (a, b)
    }
}

#[derive(Clone)]
struct Down {
    h: Vec<f32>,
    hist: Vec<f32>,
    pos: usize,
}
impl Down {
    fn new(len: usize) -> Self {
        Self { h: design(len), hist: vec![0.0; len * 2], pos: 0 }
    }
    fn clear(&mut self) {
        self.hist.iter_mut().for_each(|s| *s = 0.0);
    }
    #[inline]
    fn write(&mut self, x: f32) {
        let n = self.h.len();
        self.pos = (self.pos + n - 1) % n;
        self.hist[self.pos] = x;
        self.hist[self.pos + n] = x;
    }
    /// Takes two high-rate samples, returns one. The output is computed on the
    /// first of the pair, which keeps the overall delay a whole sample.
    #[inline]
    fn push(&mut self, a: f32, b: f32) -> f32 {
        self.write(a);
        let n = self.h.len();
        let y = self.h.iter().zip(&self.hist[self.pos..self.pos + n]).map(|(c, s)| c * s).sum();
        self.write(b);
        y
    }
}

/// One channel of up/down sampling at a fixed factor (1, 2, 4 or 8).
#[derive(Clone)]
pub struct Oversampler {
    factor: usize,
    up: Vec<Up>,
    down: Vec<Down>,
}

impl Oversampler {
    pub fn new(factor: usize) -> Self {
        let factor = factor.clamp(1, MAX_FACTOR).next_power_of_two();
        let stages = factor.trailing_zeros() as usize;
        Self { factor, up: LENS.iter().take(stages).map(|&l| Up::new(l)).collect(), down: LENS.iter().take(stages).map(|&l| Down::new(l)).collect() }
    }
    pub fn factor(&self) -> usize {
        self.factor
    }
    pub fn clear(&mut self) {
        self.up.iter_mut().for_each(Up::clear);
        self.down.iter_mut().for_each(Down::clear);
    }
    /// Fills `out[..factor]` with the upsampled version of `x`.
    #[inline]
    pub fn up(&mut self, x: f32, out: &mut [f32; MAX_FACTOR]) {
        out[0] = x;
        let mut n = 1;
        for st in self.up.iter_mut() {
            // Inputs go through the filter oldest first.
            let src = *out;
            for i in 0..n {
                let (a, b) = st.push(src[i]);
                out[2 * i] = a;
                out[2 * i + 1] = b;
            }
            n *= 2;
        }
    }
    /// Returns one base-rate sample from `inp[..factor]`.
    #[inline]
    pub fn down(&mut self, inp: &mut [f32; MAX_FACTOR]) -> f32 {
        let mut n = self.factor;
        for st in self.down.iter_mut().rev() {
            for i in 0..n / 2 {
                inp[i] = st.push(inp[2 * i], inp[2 * i + 1]);
            }
            n /= 2;
        }
        inp[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latencies_are_whole_samples() {
        assert_eq!([latency(1), latency(2), latency(4), latency(8)], [0, 24, 28, 29]);
    }

    /// Upsampled impulse response has the right total gain.
    #[test]
    fn up_order_is_correct() {
        for f in [2usize, 4, 8] {
            let mut os = Oversampler::new(f);
            let mut buf = [0.0f32; MAX_FACTOR];
            let mut out = Vec::new();
            for n in 0..400 {
                let x = if n == 0 { 1.0 } else { 0.0 };
                os.up(x, &mut buf);
                out.extend_from_slice(&buf[..f]);
            }
            // Impulse response sums to the factor (unity DC gain at the high rate after zero-stuffing).
            let s: f32 = out.iter().sum();
            assert!((s - f as f32).abs() < 1e-3, "factor {f}: sum {s}");
        }
    }

    /// up -> down passes a low sine with unity gain, delayed by exactly `latency`.
    #[test]
    fn round_trip_is_a_pure_delay() {
        let sr = 48000.0f32;
        for f in [1usize, 2, 4, 8] {
            let mut os = Oversampler::new(f);
            let d = latency(f);
            let mut buf = [0.0f32; MAX_FACTOR];
            let x: Vec<f32> = (0..4800).map(|n| (2.0 * std::f32::consts::PI * 1000.0 * n as f32 / sr).sin()).collect();
            let y: Vec<f32> = x
                .iter()
                .map(|&s| {
                    os.up(s, &mut buf);
                    os.down(&mut buf)
                })
                .collect();
            let err = (1000..4800).map(|n| (y[n] - x[n - d]).abs()).fold(0.0f32, f32::max);
            assert!(err < 2e-3, "factor {f}: max error {err}");
        }
    }

    /// Content near the base Nyquist made at the high rate is removed on the way down.
    #[test]
    fn rejects_images() {
        let mut os = Oversampler::new(2);
        let mut buf = [0.0f32; MAX_FACTOR];
        // A tone generated at the high rate, above the base Nyquist.
        let mut peak = 0.0f32;
        for n in 0..4000 {
            for k in 0..2 {
                let t = (2 * n + k) as f32;
                buf[k] = (std::f32::consts::PI * 0.75 * t).sin(); // 0.75 x high-rate Nyquist = 1.5 x base Nyquist
            }
            let y = os.down(&mut buf);
            if n > 200 {
                peak = peak.max(y.abs());
            }
        }
        assert!(peak < 0.01, "image leak {peak}");
    }
}
