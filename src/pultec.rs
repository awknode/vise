//! PUSH-PULL EQ (v0.4): an EQP-1A style program EQ after the compressor.
//!
//! LOW: FREQ 20 / 30 / 60 / 100 Hz, BOOST (up to +13.5 dB) and ATTEN (up to -17.5 dB).
//! The two are separate curves at slightly different places, so turning both up gives the
//! famous push-pull: a fat bump at the bottom with a dip just above it.
//! HIGH: FREQ 3..16 kHz bell BOOST (up to +16 dB) with BANDWIDTH, and a high shelf ATTEN at
//! 5 / 10 / 20 kHz (up to -16 dB). A touch of tube (2nd harmonic) when it's on.
//!
//! No headless Pultec could be measured (the UAD one only runs inside the UA engine), so
//! the curves follow the published EQP-1A behaviour: boost = shelf at 1.8 x FREQ (S 0.7),
//! atten = gentler shelf at 2.5 x FREQ (S 0.5).

use crate::dsp::Biq;

pub const LOW_HZ: [f32; 4] = [20.0, 30.0, 60.0, 100.0];
pub const HIGH_HZ: [f32; 7] = [3000.0, 4000.0, 5000.0, 8000.0, 10000.0, 12000.0, 16000.0];
pub const ATT_HZ: [f32; 3] = [5000.0, 10000.0, 20000.0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqSet {
    pub on: bool,
    /// selectors 0..1 (stepped), amounts 0..10
    pub lf: f32,
    pub lb: f32,
    pub la: f32,
    pub hf: f32,
    pub hb: f32,
    pub bw: f32,
    pub ha: f32,
    pub hs: f32,
}
impl Default for EqSet {
    fn default() -> Self {
        Self { on: false, lf: 2.0 / 3.0, lb: 0.0, la: 0.0, hf: 0.5, hb: 0.0, bw: 5.0, ha: 0.0, hs: 0.5 }
    }
}

pub fn pick<const N: usize>(list: &[f32; N], v: f32) -> f32 {
    list[((v.clamp(0.0, 1.0) * (N - 1) as f32).round() as usize).min(N - 1)]
}
pub fn hz_text(f: f32) -> String {
    if f >= 1000.0 { format!("{:.0} kHz", f / 1000.0) } else { format!("{f:.0} Hz") }
}

pub struct PushPull {
    sr: f32,
    set: Option<EqSet>,
    f: [[Biq; 4]; 2],
    /// on / off crossfade (no clicks)
    w: f32,
}

impl PushPull {
    pub fn new(sr: f32) -> Self {
        Self { sr, set: None, f: [[Biq::default(); 4]; 2], w: 0.0 }
    }
    pub fn clear(&mut self) {
        self.f.iter_mut().flatten().for_each(|b| b.clear());
    }
    fn design(&mut self, s: &EqSet) {
        let sr = self.sr;
        let lf = pick(&LOW_HZ, s.lf);
        let hf = pick(&HIGH_HZ, s.hf);
        let af = pick(&ATT_HZ, s.hs).min(sr * 0.45);
        // BANDWIDTH 0 = sharp (Q 2.5) .. 10 = broad (Q 0.4)
        let q = 2.5 * (0.4f32 / 2.5).powf(s.bw.clamp(0.0, 10.0) / 10.0);
        for ch in 0..2 {
            let [b, a, h, ha] = &mut self.f[ch];
            b.shelf(lf * 1.8, 1.35 * s.lb.clamp(0.0, 10.0), 0.7, false, sr);
            a.shelf(lf * 2.5, -1.75 * s.la.clamp(0.0, 10.0), 0.5, false, sr);
            h.peak(hf.min(sr * 0.45), q, 1.6 * s.hb.clamp(0.0, 10.0), sr);
            ha.shelf(af, -1.6 * s.ha.clamp(0.0, 10.0), 0.6, true, sr);
        }
    }
    #[inline]
    pub fn tick(&mut self, l: f32, r: f32, s: &EqSet) -> (f32, f32) {
        if self.set.as_ref() != Some(s) {
            self.design(s);
            self.set = Some(*s);
        }
        let tgt = if s.on { 1.0 } else { 0.0 };
        self.w += (tgt - self.w) * 0.002;
        if self.w < 1e-4 && !s.on {
            self.w = 0.0;
            return (l, r);
        }
        let mut out = [l, r];
        for (ch, x) in [l, r].into_iter().enumerate() {
            let mut y = x;
            for b in self.f[ch].iter_mut() {
                y = b.tick(y);
            }
            // a touch of tube: 2nd harmonic around -60 dB at -12 dBFS, unity for small signals
            let y = y + 0.02 * y * y.abs();
            out[ch] = x + (y - x) * self.w;
        }
        (out[0], out[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn gain(s: &EqSet, f: f32) -> f32 {
        let sr = 48000.0;
        let mut e = PushPull::new(sr);
        let (mut si, mut so) = (0.0f32, 0.0f32);
        for i in 0..96000 {
            let x = 0.05 * (std::f32::consts::TAU * f * i as f32 / sr).sin();
            let (y, _) = e.tick(x, x, s);
            if i > 48000 {
                si += x * x;
                so += y * y;
            }
        }
        10.0 * (so / si).log10()
    }
    #[test]
    fn push_pull_bump_and_dip() {
        let s = EqSet { on: true, lb: 10.0, la: 5.0, ..EqSet::default() }; // 60 Hz
        let (lo, f60, mid, top) = (gain(&s, 25.0), gain(&s, 60.0), gain(&s, 250.0), gain(&s, 2000.0));
        println!("push-pull 60 Hz, boost 10 atten 5: 25 Hz {lo:.1}  60 Hz {f60:.1}  250 Hz {mid:.1}  2 kHz {top:.1}");
        assert!(lo > 3.0 && f60 > 2.0, "bump");
        assert!(mid < -0.3, "dip above");
        assert!(top.abs() < 0.3, "mids/highs untouched");
        let flat = EqSet { on: true, ..EqSet::default() };
        for f in [30.0, 300.0, 3000.0] {
            assert!(gain(&flat, f).abs() < 0.2, "flat when all at 0");
        }
        let hi = EqSet { on: true, hb: 10.0, ..EqSet::default() }; // HIGH FREQ default = 8 kHz
        assert!(gain(&hi, 8000.0) > 12.0);
    }
}
