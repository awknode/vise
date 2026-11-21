//! VISE — awk studios. Six compressor models in one box:
//! 670 (vari-mu), DS, FAT, ALPHA, SH-A, SH-B. Each model has its own knob set
//! (the labels change with the model), COLOR, AUTO GAIN and a sidechain high-pass.
//!
//! in ─► [M/S] ─► COMP (oversampled colour stage) ─► [L/R] ─► MIX against the dry (delayed
//! to match) ─► OUTPUT ─► SAFE. The detector hears the input, or the external sidechain
//! key (SC EXT). LINK sets how much the two channels share one gain. LISTEN: SC (what the
//! detector hears) or Δ DELTA (only what the compression takes out).

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

mod awk_ui;
pub mod comp;
pub mod dsp;
mod editor;
mod oversample;
mod presets;

pub mod params {
    pub const MAX_K: usize = 7;
}
use params::MAX_K;

pub struct Shared {
    pub meters: awk_ui::Meters,
    /// gain reduction (dB, positive), VU-style input / output levels (dBFS RMS)
    pub gr: AtomicU32,
    pub vin: AtomicU32,
    pub vout: AtomicU32,
    /// external key level (dBFS RMS), -90 when there is none
    pub key: AtomicU32,
    pub lat: AtomicU32,
}
impl Default for Shared {
    fn default() -> Self {
        Self { meters: awk_ui::Meters::default(), gr: AtomicU32::new(0), vin: AtomicU32::new((-90f32).to_bits()), vout: AtomicU32::new((-90f32).to_bits()), key: AtomicU32::new((-90f32).to_bits()), lat: AtomicU32::new(0) }
    }
}
pub fn get(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}
fn put(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

pub const METER_MODES: [&str; 3] = ["GR", "IN", "OUT"];
pub const CHAN_NAMES: [&str; 4] = ["STEREO", "M / S", "MID", "SIDE"];
pub const LISTEN_NAMES: [&str; 3] = ["OFF", "SC", "DELTA"];

#[derive(Params)]
pub struct ViseParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,
    #[id = "model"]
    pub model: IntParam,
    #[id = "k0"]
    pub k0: FloatParam,
    #[id = "k1"]
    pub k1: FloatParam,
    #[id = "k2"]
    pub k2: FloatParam,
    #[id = "k3"]
    pub k3: FloatParam,
    #[id = "k4"]
    pub k4: FloatParam,
    #[id = "k5"]
    pub k5: FloatParam,
    #[id = "k6"]
    pub k6: FloatParam,
    #[id = "gmix"]
    pub mix: FloatParam,
    #[id = "out"]
    pub out: FloatParam,
    #[id = "os"]
    pub os: IntParam,
    #[id = "safe"]
    pub safe: BoolParam,
    #[id = "bypass"]
    pub bypass: BoolParam,
    #[id = "vu"]
    pub vu: IntParam,
    #[id = "link"]
    pub link: FloatParam,
    #[id = "chan"]
    pub chan: IntParam,
    #[id = "ext"]
    pub ext: BoolParam,
    #[id = "listen"]
    pub listen: IntParam,
}

fn k(name: &str, def: f32) -> FloatParam {
    FloatParam::new(name, def, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_smoother(SmoothingStyle::Linear(20.0))
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
        .with_unit("%")
}

impl Default for ViseParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(editor::WIDTH, editor::HEIGHT),
            model: IntParam::new("Model", 3, IntRange::Linear { min: 0, max: 5 }).with_value_to_string(Arc::new(|v| comp::MODELS[v.clamp(0, 5) as usize].to_string())),
            k0: k("Input / Thresh", 0.4),
            k1: k("Ratio / Mode", 0.5),
            k2: k("Attack / Time", 0.3),
            k3: k("Release / Tubes", 0.4),
            k4: k("Color", 0.3),
            k5: k("Auto Gain", 1.0),
            k6: k("SC HPF", 0.2),
            mix: awk_ui::mix_param(),
            out: FloatParam::new("Output", 0.0, FloatRange::Linear { min: -18.0, max: 18.0 }).with_smoother(SmoothingStyle::Linear(20.0)).with_unit(" dB").with_value_to_string(formatters::v2s_f32_rounded(1)),
            os: IntParam::new("Oversampling", 1, IntRange::Linear { min: 0, max: 3 }).with_value_to_string(Arc::new(|v| awk_ui::OS_NAMES[v.clamp(0, 3) as usize].to_string())),
            safe: awk_ui::safe_param(),
            bypass: awk_ui::bypass_param(),
            vu: IntParam::new("Meter", 0, IntRange::Linear { min: 0, max: 2 }).with_value_to_string(Arc::new(|v| METER_MODES[v.clamp(0, 2) as usize].to_string())).non_automatable(),
            link: FloatParam::new("Stereo Link", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage())
                .with_unit("%"),
            chan: IntParam::new("Channels", 0, IntRange::Linear { min: 0, max: 3 }).with_value_to_string(Arc::new(|v| CHAN_NAMES[v.clamp(0, 3) as usize].to_string())),
            ext: BoolParam::new("External Sidechain", false).with_value_to_string(Arc::new(|v| if v { "EXT".into() } else { "INT".into() })),
            listen: IntParam::new("Listen", 0, IntRange::Linear { min: 0, max: 2 }).with_value_to_string(Arc::new(|v| LISTEN_NAMES[v.clamp(0, 2) as usize].to_string())),
        }
    }
}
impl ViseParams {
    pub fn knobs(&self) -> [&FloatParam; MAX_K] {
        [&self.k0, &self.k1, &self.k2, &self.k3, &self.k4, &self.k5, &self.k6]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct State {
    pub model: usize,
    pub k: [f32; MAX_K],
    pub mix: f32,
    pub out_db: f32,
    pub safe: bool,
    pub bypass: bool,
    /// 0..1 stereo link
    pub link: f32,
    /// 0 STEREO, 1 M/S, 2 MID only, 3 SIDE only
    pub chan: usize,
    /// detector hears the external key (when one is connected)
    pub ext: bool,
    /// 0 off, 1 SC, 2 DELTA
    pub listen: usize,
}
impl Default for State {
    fn default() -> Self {
        Self { model: 3, k: [0.4, 0.5, 0.3, 0.4, 0.3, 1.0, 0.2], mix: 1.0, out_db: 0.0, safe: true, bypass: false, link: 1.0, chan: 0, ext: false, listen: 0 }
    }
}

pub struct Engine {
    pub sr: f32,
    comp: comp::Comp,
    dry: [awk_ui::DryDelay; 2],
    pub lat: usize,
    mix: f32,
    out_g: f32,
    ms_in: f32,
    ms_out: f32,
    ms_key: f32,
    byp: awk_ui::BypassRamp,
    ceil: awk_ui::Ceiling,
    meter: awk_ui::PeakFollower,
    pub shared: Arc<Shared>,
}

impl Engine {
    pub fn new(sr: f32, os: i32) -> Self {
        let comp = comp::Comp::new(sr, oversample::factor_for(os));
        let lat = comp.latency();
        let mut e = Self {
            sr,
            comp,
            dry: [awk_ui::DryDelay::new(lat + 8), awk_ui::DryDelay::new(lat + 8)],
            lat,
            mix: -1.0,
            out_g: -1.0,
            ms_in: 0.0,
            ms_out: 0.0,
            ms_key: 0.0,
            byp: awk_ui::BypassRamp::default(),
            ceil: awk_ui::Ceiling::default(),
            meter: awk_ui::PeakFollower::default(),
            shared: Arc::new(Shared::default()),
        };
        e.byp.set_sr(sr);
        e.ceil.set_sr(sr);
        e.meter.set_sr(sr);
        e
    }
    pub fn with_shared(mut self, s: Arc<Shared>) -> Self {
        s.lat.store(self.lat as u32, Ordering::Relaxed);
        self.shared = s;
        self
    }
    pub fn os_factor(&self) -> usize {
        self.comp.os_factor()
    }
    pub fn reset(&mut self) {
        self.comp.reset();
        self.dry.iter_mut().for_each(|d| d.clear());
    }
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], st: &State) {
        self.process_key(l, r, None, st);
    }
    pub fn process_key(&mut self, l: &mut [f32], r: &mut [f32], key: Option<(&[f32], &[f32])>, st: &State) {
        self.comp.control(&st.k, st.model);
        let sr = self.sr;
        let a = 1.0 - (-1.0 / (0.02 * sr)).exp();
        let vu = 1.0 - (-1.0 / (0.3 * sr)).exp();
        let out_t = dsp::db(st.out_db);
        // first block: start where the knobs are (no glide from a made-up value)
        if self.mix < 0.0 {
            self.mix = st.mix;
            self.out_g = out_t;
        }
        let ms = st.chan > 0;
        let key = if st.ext { key.filter(|(a, b)| a.len() >= l.len() && b.len() >= l.len()) } else { None };
        for i in 0..l.len() {
            let (il, ir) = (l[i], r[i]);
            let (xl, xr) = if ms { ((il + ir) * 0.5, (il - ir) * 0.5) } else { (il, ir) };
            let k = key.map(|(ka, kb)| {
                let (a, b) = (ka[i], kb[i]);
                self.ms_key += vu * (0.5 * (a * a + b * b) - self.ms_key);
                if ms { ((a + b) * 0.5, (a - b) * 0.5) } else { (a, b) }
            });
            let (cl, cr) = self.comp.tick_key(xl, xr, k, st.link);
            let (dl, dr) = (self.dry[0].tick(il, self.lat), self.dry[1].tick(ir, self.lat));
            // back to L/R; MID / SIDE only: the other half is the (delayed) dry
            let (cl, cr) = if ms {
                let (dm, ds) = ((dl + dr) * 0.5, (dl - dr) * 0.5);
                let (m, s) = match st.chan {
                    2 => (cl, ds),
                    3 => (dm, cr),
                    _ => (cl, cr),
                };
                (m + s, m - s)
            } else {
                (cl, cr)
            };
            self.mix += a * (st.mix - self.mix);
            self.out_g += a * (out_t - self.out_g);
            let (ol, or) = match st.listen {
                // SC: what the detector hears (not delayed; it's for setting the HPF / key by ear)
                1 => {
                    let [s0, s1] = self.comp.sc;
                    let (sl, sr) = if ms { (s0 + s1, s0 - s1) } else { (s0, s1) };
                    (sl * self.out_g, sr * self.out_g)
                }
                // DELTA: only what the compression takes away (makeup gain taken out)
                2 => {
                    let mk = self.comp.mk.max(1e-3);
                    ((dl - cl / mk) * self.mix * self.out_g, (dr - cr / mk) * self.mix * self.out_g)
                }
                _ => ((dl + (cl - dl) * self.mix) * self.out_g, (dr + (cr - dr) * self.mix) * self.out_g),
            };
            // bypass crossfades to the delayed dry, so the latency stays constant
            let w = self.byp.next(st.bypass);
            let (ol, or) = (dl + (ol - dl) * w, dr + (or - dr) * w);
            let (ol, or) = self.ceil.tick(ol, or, st.safe);
            let (ol, or) = if ol.is_finite() && or.is_finite() { (ol, or) } else { (0.0, 0.0) };
            self.ms_in += vu * (0.5 * (il * il + ir * ir) - self.ms_in);
            self.ms_out += vu * (0.5 * (ol * ol + or * or) - self.ms_out);
            self.meter.feed(il, ir, ol, or);
            l[i] = ol;
            r[i] = or;
        }
        put(&self.shared.gr, self.comp.gr_db.max(0.0));
        put(&self.shared.vin, 10.0 * self.ms_in.max(1e-10).log10());
        put(&self.shared.vout, 10.0 * self.ms_out.max(1e-10).log10());
        if key.is_none() {
            self.ms_key = 0.0;
        }
        put(&self.shared.key, 10.0 * self.ms_key.max(1e-10).log10());
        self.meter.store(&self.shared.meters);
        self.ceil.store(&self.shared.meters);
    }
}

pub struct Vise {
    params: Arc<ViseParams>,
    shared: Arc<Shared>,
    eng: Engine,
}

impl Default for Vise {
    fn default() -> Self {
        let shared = Arc::new(Shared::default());
        Self { params: Arc::new(ViseParams::default()), shared: shared.clone(), eng: Engine::new(48000.0, 1).with_shared(shared) }
    }
}

impl Plugin for Vise {
    const NAME: &'static str = "Vise";
    const VENDOR: &'static str = "awk studios";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout { main_input_channels: NonZeroU32::new(2), main_output_channels: NonZeroU32::new(2),
        aux_input_ports: &[new_nonzero_u32(2)],
        names: PortNames { layout: Some("Stereo"), main_input: Some("Input"), main_output: Some("Output"), aux_inputs: &["Sidechain"], aux_outputs: &[] },
        ..AudioIOLayout::const_default()
    }];
    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }
    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.shared.clone())
    }
    fn initialize(&mut self, _layout: &AudioIOLayout, cfg: &BufferConfig, ctx: &mut impl InitContext<Self>) -> bool {
        self.eng = Engine::new(cfg.sample_rate, self.params.os.value()).with_shared(self.shared.clone());
        ctx.set_latency_samples(self.eng.lat as u32);
        true
    }
    fn reset(&mut self) {
        self.eng.reset();
    }
    fn process(&mut self, buffer: &mut Buffer, aux: &mut AuxiliaryBuffers, ctx: &mut impl ProcessContext<Self>) -> ProcessStatus {
        let p = &self.params;
        let os = p.os.value();
        if oversample::factor_for(os) != self.eng.os_factor() {
            let sr = self.eng.sr;
            self.eng = Engine::new(sr, os).with_shared(self.shared.clone());
            ctx.set_latency_samples(self.eng.lat as u32);
        }
        let ks = p.knobs();
        let st = State {
            model: p.model.value().clamp(0, 5) as usize,
            k: std::array::from_fn(|i| ks[i].smoothed.next()),
            mix: p.mix.value(),
            out_db: p.out.smoothed.next(),
            safe: p.safe.value(),
            bypass: p.bypass.value(),
            link: p.link.smoothed.next(),
            chan: p.chan.value().clamp(0, 3) as usize,
            ext: p.ext.value(),
            listen: p.listen.value().clamp(0, 2) as usize,
        };
        let sl = buffer.as_slice();
        if sl.len() < 2 {
            return ProcessStatus::Normal;
        }
        let n = sl[0].len().saturating_sub(1) as u32;
        let (a, b) = sl.split_at_mut(1);
        let key = aux.inputs.first_mut().map(|k| k.as_slice_immutable()).filter(|k| !k.is_empty()).map(|k| (&*k[0], if k.len() > 1 { &*k[1] } else { &*k[0] }));
        self.eng.process_key(&mut *a[0], &mut *b[0], key, &st);
        if n > 0 {
            for k in ks {
                k.smoothed.next_step(n);
            }
            p.out.smoothed.next_step(n);
            p.link.smoothed.next_step(n);
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for Vise {
    const CLAP_ID: &'static str = "com.awknode.vise";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Six-model compressor");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Compressor, ClapFeature::Stereo];
}

impl Vst3Plugin for Vise {
    const VST3_CLASS_ID: [u8; 16] = *b"AwknodeViseComp1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

nih_export_clap!(Vise);
nih_export_vst3!(Vise);
