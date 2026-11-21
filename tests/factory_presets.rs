//! Factory presets: validated against the real params, played (bounded), written with PRESET_OUT.
use nih_plug::prelude::*;
use serde_json::{json, Map, Value};
use vise::*;

type Preset = (&'static str, &'static [(&'static str, f64)]);
// model: 0 670, 1 DS, 2 FAT, 3 ALPHA, 4 SH-A, 5 SH-B
// (0.3.0: INPUT is 0..+36 dB into a threshold 12 dB higher; k0 for 670/DS/FAT remapped so they sound the same)
const PRESETS: &[Preset] = &[
    ("670 - Vocal Glue", &[("model", 0.0), ("k0", 0.633), ("k1", 0.55), ("k2", 0.2), ("k3", 0.0), ("k4", 0.35)]),
    ("670 - Slow Bus (TC5)", &[("model", 0.0), ("k0", 0.6), ("k2", 0.8), ("k3", 0.5), ("k4", 0.25)]),
    ("670 - Thick Bass", &[("model", 0.0), ("k0", 0.7), ("k2", 0.4), ("k3", 0.85), ("k4", 0.5), ("k6", 0.0)]),
    ("670 - Drum Room Squash", &[("model", 0.0), ("k0", 0.833), ("k2", 0.0), ("k3", 0.3), ("k4", 0.55), ("gmix", 0.5)]),
    ("DS - Drum Smash Parallel", &[("model", 1.0), ("k0", 0.8), ("k1", 0.86), ("k2", 0.3), ("k3", 0.35), ("k4", 0.3), ("gmix", 0.55)]),
    ("DS - Nuke + Dist 3", &[("model", 1.0), ("k0", 0.733), ("k1", 1.0), ("k2", 0.2), ("k3", 0.3), ("k4", 0.8), ("gmix", 0.5)]),
    ("DS - Vocal Grab 4:1", &[("model", 1.0), ("k0", 0.667), ("k1", 0.43), ("k2", 0.4), ("k3", 0.45), ("k4", 0.1)]),
    ("DS - Room Mic Explode", &[("model", 1.0), ("k0", 0.9), ("k1", 0.86), ("k2", 0.1), ("k3", 0.2), ("k4", 0.55), ("k6", 0.5)]),
    ("FAT - Bus Warmth", &[("model", 2.0), ("k0", 0.6), ("k1", 0.33), ("k2", 0.35), ("k3", 0.4), ("k4", 0.35)]),
    ("FAT - Spank Snare", &[("model", 2.0), ("k0", 0.7), ("k1", 0.67), ("k2", 0.2), ("k3", 0.2), ("k4", 0.5), ("gmix", 0.7)]),
    ("FAT - Tape-ish Track", &[("model", 2.0), ("k0", 0.633), ("k1", 1.0), ("k2", 0.6), ("k3", 0.6), ("k4", 0.4)]),
    ("ALPHA - Negative Pump", &[("model", 3.0), ("k0", 0.55), ("k1", 0.95), ("k2", 0.4), ("k3", 0.3), ("k4", 0.3)]),
    ("ALPHA - Clean Vocal", &[("model", 3.0), ("k0", 0.45), ("k1", 0.45), ("k2", 0.45), ("k3", 0.45), ("k4", 0.15)]),
    ("ALPHA - Mix Bus Gold", &[("model", 3.0), ("k0", 0.3), ("k1", 0.35), ("k2", 0.6), ("k3", 0.5), ("k4", 0.35), ("k6", 0.35)]),
    ("ALPHA - 808 Hold", &[("model", 3.0), ("k0", 0.5), ("k1", 0.6), ("k2", 0.55), ("k3", 0.35), ("k4", 0.4), ("k6", 0.0)]),
    ("SH-A - Master Polish", &[("model", 4.0), ("k0", 0.35), ("k1", 0.3), ("k2", 0.6), ("k3", 0.5), ("k4", 0.5)]),
    ("SH-A - Steel Bite", &[("model", 4.0), ("k0", 0.5), ("k1", 0.5), ("k2", 0.4), ("k3", 0.4), ("k4", 0.9)]),
    ("SH-B - Class A Glow", &[("model", 5.0), ("k0", 0.4), ("k1", 0.4), ("k2", 0.8), ("k3", 0.45), ("k4", 0.5)]),
    ("SH-B - Nickel Air Master", &[("model", 5.0), ("k0", 0.3), ("k1", 0.25), ("k2", 0.7), ("k3", 0.55), ("k4", 0.1), ("k6", 0.4)]),
    ("SH-B - Glue + 2x Parallel", &[("model", 5.0), ("k0", 0.6), ("k1", 0.67), ("k2", 0.5), ("k3", 0.4), ("k4", 0.6), ("gmix", 0.6), ("os", 2.0)]),
    // sidechain: route a kick (or vocal) into VISE's sidechain input
    ("SC - Kick Ducks Bass", &[("model", 3.0), ("ext", 1.0), ("k0", 0.55), ("k1", 0.6), ("k2", 0.2), ("k3", 0.35), ("k4", 0.0), ("k5", 0.0), ("k6", 0.0)]),
    ("SC - Vocal Makes Room", &[("model", 4.0), ("ext", 1.0), ("k0", 0.45), ("k1", 0.3), ("k2", 0.5), ("k3", 0.45), ("k4", 0.2), ("k5", 0.0), ("k6", 0.35)]),
    ("SC - Hard Pump (DS)", &[("model", 1.0), ("ext", 1.0), ("k0", 0.733), ("k1", 0.86), ("k2", 0.15), ("k3", 0.3), ("k4", 0.0), ("k5", 0.0)]),
    // stereo / mid-side
    ("MS - Master Glue", &[("model", 4.0), ("chan", 1.0), ("link", 0.5), ("k0", 0.35), ("k1", 0.3), ("k2", 0.6), ("k3", 0.5), ("k4", 0.4)]),
    ("MS - Side Tamer", &[("model", 3.0), ("chan", 3.0), ("k0", 0.5), ("k1", 0.45), ("k2", 0.4), ("k3", 0.4), ("k4", 0.1)]),
    ("Unlinked - Overheads", &[("model", 1.0), ("link", 0.0), ("k0", 0.7), ("k1", 0.43), ("k2", 0.35), ("k3", 0.4), ("k4", 0.2)]),
];

fn build(params: &ViseParams, over: &[(&str, f64)]) -> Value {
    let map = params.param_map();
    let mut out = Map::new();
    for (id, v) in over {
        let ptr = map.iter().find(|(i, _, _)| i == id).unwrap_or_else(|| panic!("no param {id}")).1;
        let val = unsafe {
            match ptr {
                ParamPtr::FloatParam(p) => {
                    let p = &*p;
                    assert!((p.preview_plain(p.preview_normalized(*v as f32)) - *v as f32).abs() < 1e-3, "{id} out of range");
                    json!({ "f32": *v as f32 })
                }
                ParamPtr::IntParam(p) => {
                    let p = &*p;
                    assert_eq!(p.preview_plain(p.preview_normalized(*v as i32)), *v as i32, "{id} out of range");
                    json!({ "i32": *v as i32 })
                }
                ParamPtr::BoolParam(_) => json!({ "bool": *v != 0.0 }),
                _ => panic!("{id}"),
            }
        };
        out.insert(id.to_string(), val);
    }
    json!({ "version": env!("CARGO_PKG_VERSION"), "params": out, "fields": {} })
}

#[test]
fn factory_presets_are_valid() {
    let params = ViseParams::default();
    let dir = std::env::var("PRESET_OUT").ok();
    for p in PRESETS {
        assert!(!p.0.contains('/'));
        let v = build(&params, p.1);
        let _: nih_plug::wrapper::state::PluginState = serde_json::from_value(v.clone()).unwrap();
        let mut st = State::default();
        let mut os = 1;
        for (id, x) in p.1 {
            match *id {
                "model" => st.model = *x as usize,
                "gmix" => st.mix = *x as f32,
                "os" => os = *x as i32,
                "ext" => st.ext = *x != 0.0,
                "chan" => st.chan = *x as usize,
                "link" => st.link = *x as f32,
                k => st.k[k[1..].parse::<usize>().unwrap()] = *x as f32,
            }
        }
        let mut e = Engine::new(48000.0, os);
        let mut s = 9u32;
        let mut l: Vec<f32> = (0..48000).map(|i| { s = s.wrapping_mul(1664525).wrapping_add(1013904223); 0.6 * ((s >> 8) as f32 / 8388608.0 - 1.0) * if (i / 5000) % 2 == 0 { 1.0 } else { 0.1 } }).collect();
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) { e.process(a, b, &st); }
        let pk = l.iter().chain(r.iter()).fold(0.0f32, |a, v| a.max(v.abs()));
        assert!(pk.is_finite() && pk <= 1.0, "{}: {pk}", p.0);
        if let Some(d) = &dir {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(format!("{d}/{}.awkp", p.0), serde_json::to_string_pretty(&v).unwrap()).unwrap();
        }
    }
}
