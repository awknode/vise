//! Factory presets: validated against the real params, played (bounded), written with PRESET_OUT.
use nice_plug::prelude::*;
use serde_json::{json, Map, Value};
use vise::*;

type Preset = (&'static str, &'static [(&'static str, f64)]);
// model: 0 670, 1 DS, 2 FAT, 3 ALPHA, 4 SH-A, 5 SH-B, 6 76, 7 2A (0.4.0)
// 76: k1 ratio 0 / .25 / .5 / .75 / 1 = 4 / 8 / 12 / 20 / ALL. 2A: k1 < .5 COMP, >= .5 LIMIT; k2 EMPH; k3 MEMORY.
// EQ: eq_lf 0 / 1/3 / 2/3 / 1 = 20 / 30 / 60 / 100 Hz; eq_hf 0..1 over 3/4/5/8/10/12/16 kHz; amounts 0..10.
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
    // ---- 0.4.0: 76, 2A and the PUSH-PULL EQ
    ("76 - Vocal Upfront 4-1", &[("model", 6.0), ("k0", 0.45), ("k1", 0.0), ("k2", 0.6), ("k3", 0.35), ("k4", 0.3)]),
    ("76 - All Buttons Drum Smash", &[("model", 6.0), ("k0", 0.6), ("k1", 1.0), ("k2", 0.5), ("k3", 0.3), ("k4", 0.4), ("gmix", 0.5)]),
    ("76 - Bass Even-Out 8-1", &[("model", 6.0), ("k0", 0.5), ("k1", 0.25), ("k2", 0.3), ("k3", 0.5), ("k4", 0.35), ("k6", 0.0)]),
    ("76 - Room Nuke 20-1", &[("model", 6.0), ("k0", 0.7), ("k1", 0.75), ("k2", 0.8), ("k3", 0.15), ("k4", 0.5), ("gmix", 0.45)]),
    ("2A - Vocal Leveler", &[("model", 7.0), ("k0", 0.45), ("k1", 0.0), ("k2", 0.5), ("k3", 0.5), ("k4", 0.3)]),
    ("2A - Bass Smooth Limit", &[("model", 7.0), ("k0", 0.5), ("k1", 1.0), ("k2", 0.3), ("k3", 0.6), ("k4", 0.35), ("k6", 0.0)]),
    ("2A - Bus Gentle Glue", &[("model", 7.0), ("k0", 0.35), ("k1", 0.0), ("k2", 0.4), ("k3", 0.7), ("k4", 0.25)]),
    ("2A - De-Harsh (Emphasis)", &[("model", 7.0), ("k0", 0.4), ("k1", 0.0), ("k2", 0.9), ("k3", 0.4), ("k4", 0.2)]),
    ("EQ - Kick Push-Pull 60", &[("model", 3.0), ("k0", 0.3), ("eq_on", 1.0), ("eq_lf", 0.6667), ("eq_lb", 6.0), ("eq_la", 3.0)]),
    ("EQ - 808 Push-Pull 30", &[("model", 7.0), ("k0", 0.4), ("eq_on", 1.0), ("eq_lf", 0.3333), ("eq_lb", 5.0), ("eq_la", 2.5)]),
    ("EQ - Vocal Air 12k", &[("model", 7.0), ("k0", 0.45), ("eq_on", 1.0), ("eq_hf", 0.8333), ("eq_hb", 4.0), ("eq_bw", 7.0)]),
    ("EQ - Mix Bus Push-Pull Trick", &[("model", 4.0), ("k0", 0.3), ("k1", 0.3), ("eq_on", 1.0), ("eq_lf", 0.6667), ("eq_lb", 4.0), ("eq_la", 2.0), ("eq_hf", 0.6667), ("eq_hb", 2.5), ("eq_bw", 8.0), ("eq_ha", 1.5), ("eq_hs", 1.0)]),
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
        let _: nice_plug::plugin::PluginState = serde_json::from_value(v.clone()).unwrap();
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
                "ms_mid" => st.ms_mid = *x as f32,
                "ms_side" => st.ms_side = *x as f32,
                "eq_on" => st.eq.on = *x != 0.0,
                "eq_lf" => st.eq.lf = *x as f32,
                "eq_lb" => st.eq.lb = *x as f32,
                "eq_la" => st.eq.la = *x as f32,
                "eq_hf" => st.eq.hf = *x as f32,
                "eq_hb" => st.eq.hb = *x as f32,
                "eq_bw" => st.eq.bw = *x as f32,
                "eq_ha" => st.eq.ha = *x as f32,
                "eq_hs" => st.eq.hs = *x as f32,
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
