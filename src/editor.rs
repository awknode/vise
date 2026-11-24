//! VISE editor, v0.2 — direction A ("HUD"): 2advanced-style black glass. Chamfered panels,
//! hairlines, amber + cyan light. The hero is a 200° LED arc meter (GR lights from the right,
//! a light-blade pointer, peak-hold tick, glowing readout); beside it a two-lane history.
//! Knobs: a brushed-metal cap whose sheen turns with the value, inside a recessed LED ring.
//! The knob labels follow the model.

use crate::awk_ui::*;
use crate::presets::PresetBar;
use crate::{comp, get, Shared, ViseParams};
use nice_plug::prelude::*;
use nice_plug_egui::create_egui_editor;
use egui::{pos2, vec2, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Mesh, Painter, Pos2, Rect, Response, Sense, Shape, Stroke};
use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub const WIDTH: u32 = 1000;
pub const HEIGHT: u32 = 820;
/// everything below the preset strip is laid out on a 1000 × 640 board starting here
const TOP: f32 = 44.0;

const BG: Color32 = rgb(0x07, 0x09, 0x0c);
const PANEL: Color32 = rgb(0x0d, 0x11, 0x16);
const PANEL_HI: Color32 = rgb(0x14, 0x1a, 0x22);
const EDGE: Color32 = rgb(0x22, 0x2d, 0x38);
const HAIR: Color32 = rgb(0x2e, 0x3b, 0x48);
const TXT: Color32 = rgb(0xe8, 0xee, 0xf4);
const DIMT: Color32 = rgb(0x6f, 0x7f, 0x8e);
const FAINT: Color32 = rgb(0x3a, 0x47, 0x54);
const AMBER: Color32 = rgb(0xff, 0x8a, 0x1f);
const CYAN: Color32 = rgb(0x46, 0xc8, 0xff);
const SEG_OFF: Color32 = rgb(0x1a, 0x22, 0x2b);
const HIST: usize = 240;

const MODEL_LINE: [&str; 8] = ["VARI-MU // TUBES // SLOW + THICK", "FET // FAST // DIST 2-3 // NUKE", "BUS // WARMTH + TRANNY", "CLASS A // PAST INFINITY", "OPTO + VCA // IRON // MASTER", "OPTO + VCA // CLASS A GLOW", "FET // FAST // ALL BUTTONS", "OPTO // T4 CELL // SMOOTH"];
const SHORT: [&str; 8] = ["VARI-MU", "FET", "BUS", "CLASS A", "OPTO+VCA", "OPTO+VCA A", "FET 76", "OPTO T4"];

pub(crate) struct St {
    presets: PresetBar,
    skin: Skin,
    /// (input dBFS, output dBFS, GR dB) per frame
    hist: VecDeque<(f32, f32, f32)>,
    /// meter position 0..1 (eased), peak-hold GR and when it was set
    needle: f32,
    pk: f32,
    pk_t: f64,
}

fn new_state(name: &'static str) -> St {
    let skin = Skin::new(Look::Hud, AMBER, "VISE", env!("CARGO_PKG_VERSION"), "");
    St { presets: PresetBar::new(name), skin, hist: VecDeque::with_capacity(HIST + 1), needle: 0.0, pk: 0.0, pk_t: 0.0 }
}

pub(crate) fn create(params: Arc<ViseParams>, shared: Arc<Shared>) -> Option<crate::nice_app::AwkEditor> {
    let state = params.editor_state.clone();
    let st = new_state("Vise");
    crate::presets::apply_scale(&state, &params.ui_scale);
    create_egui_editor(state, nice_plug_egui::RepaintNotifier::new(), nice_plug_egui::EguiNiceSettings::default(), crate::nice_app::AwkApp::new(fonts, {
        let mut __st = st;
        Box::new(move |ctx: &mut egui::Ui, setter: &ParamSetter| {
            let st = &mut __st;
            frame(ctx, setter, st, params.as_ref(), shared.as_ref())
        })
    }))
}

fn fonts(ctx: &egui::Context) {
    install_fonts(ctx);
    let mut f = FontDefinitions::default();
    let faces: [(&str, &'static [u8]); 5] = [
        ("awk-d", include_bytes!("../assets/Michroma-Regular.ttf")),
        ("awk-l", include_bytes!("../assets/BarlowCondensed-SemiBold.ttf")),
        ("awk-b", include_bytes!("../assets/BarlowCondensed-Bold.ttf")),
        ("awk-m", include_bytes!("../assets/ShareTechMono-Regular.ttf")),
        ("pat-m", include_bytes!("../assets/IBMPlexMono-Medium.ttf")),
    ];
    let fallbacks: Vec<String> = ["Ubuntu-Light", "NotoEmoji-Regular", "emoji-icon-font"].iter().filter(|n| f.font_data.contains_key(**n)).map(|s| s.to_string()).collect();
    for (name, bytes) in faces {
        f.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
        let mut fam = vec![name.to_string()];
        fam.extend(fallbacks.iter().cloned());
        f.families.insert(FontFamily::Name(name.into()), fam);
    }
    for old in ["block", "marker", "pat-h"] {
        let mut fam = vec!["awk-b".to_string()];
        fam.extend(fallbacks.iter().cloned());
        f.families.insert(FontFamily::Name(old.into()), fam);
    }
    if let Some(p) = f.families.get_mut(&FontFamily::Proportional) {
        p.insert(0, "awk-l".into());
    }
    if let Some(m) = f.families.get_mut(&FontFamily::Monospace) {
        m.insert(0, "awk-m".into());
    }
    ctx.set_fonts(f);
}
fn tech(s: f32) -> FontId {
    FontId::new(s, FontFamily::Name("awk-d".into()))
}
fn set_int(setter: &ParamSetter, p: &IntParam, v: i32) {
    setter.begin_set_parameter(p);
    setter.set_parameter(p, v);
    setter.end_set_parameter(p);
}
fn set_bool(setter: &ParamSetter, p: &BoolParam, v: bool) {
    setter.begin_set_parameter(p);
    setter.set_parameter(p, v);
    setter.end_set_parameter(p);
}
fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.clamp(0.0, 1.0) * 255.0) as u8)
}
fn lerp_rgb(a: [u8; 3], b: [u8; 3], t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0)).round() as u8;
    rgb(l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2]))
}
fn on_circle(c: Pos2, a: f32, r: f32) -> Pos2 {
    c + vec2(a.cos(), a.sin()) * r
}
/// A stroke with a soft glow under it (egui has no blur: two wider translucent passes).
fn glow_line(pa: &Painter, pts: Vec<Pos2>, w: f32, col: Color32, amt: f32) {
    pa.add(Shape::line(pts.clone(), Stroke::new(w + 9.0, alpha(col, 0.06 * amt))));
    pa.add(Shape::line(pts.clone(), Stroke::new(w + 4.0, alpha(col, 0.16 * amt))));
    pa.add(Shape::line(pts, Stroke::new(w, col)));
}
fn glow_dot(pa: &Painter, c: Pos2, r: f32, col: Color32) {
    pa.circle_filled(c, r * 3.2, alpha(col, 0.08));
    pa.circle_filled(c, r * 2.0, alpha(col, 0.18));
    pa.circle_filled(c, r, col);
}
fn glow_txt(pa: &Painter, pos: Pos2, a: Align2, s: &str, font: FontId, col: Color32, sp: f32) -> Rect {
    for (dx, dy) in [(-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0), (-1.4, -1.4), (1.4, -1.4), (-1.4, 1.4), (1.4, 1.4)] {
        txt(pa, pos + vec2(dx, dy), a, s, font.clone(), alpha(col, 0.09), sp);
    }
    txt(pa, pos, a, s, font, col, sp)
}

/// Panel with the top-right and bottom-left corners cut.
fn chamfer(r: Rect, c: f32) -> Vec<Pos2> {
    vec![r.left_top(), pos2(r.right() - c, r.top()), pos2(r.right(), r.top() + c), r.right_bottom(), pos2(r.left() + c, r.bottom()), pos2(r.left(), r.bottom() - c)]
}
fn panel(pa: &Painter, r: Rect, tag: &str, title: &str) {
    let pts = chamfer(r, 14.0);
    pa.add(Shape::convex_polygon(pts.clone(), PANEL, Stroke::NONE));
    pa.add(Shape::closed_line(pts, Stroke::new(1.0, EDGE)));
    let t = Rect::from_min_size(r.min, vec2(30.0, 16.0));
    pa.rect_filled(t, 0.0, AMBER);
    txt(pa, t.center(), Align2::CENTER_CENTER, tag, mono(10.0), BG, 0.0);
    txt(pa, t.right_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, title, mono(10.0), DIMT, 1.5);
}
fn chip(ui: &mut egui::Ui, r: Rect, s: &str, on: bool, col: Color32, id: impl std::hash::Hash + std::fmt::Debug) -> Response {
    let resp = ui.interact(r, ui.scope_id().with(("chip", id)), Sense::click());
    let pa = ui.painter();
    let pts = chamfer(r, 5.0);
    pa.add(Shape::convex_polygon(pts, if on { alpha(col, 0.18) } else if resp.hovered() { PANEL_HI } else { PANEL }, Stroke::new(1.0, if on { col } else { EDGE })));
    txt(pa, r.center(), Align2::CENTER_CENTER, s, mono(10.0), if on { col } else { DIMT }, 1.0);
    resp
}

/// Brushed-metal sheen: an annulus whose colour follows the angle (a conic gradient),
/// rotated by `rot` so the light moves as the knob turns.
fn brushed(pa: &Painter, c: Pos2, r0: f32, r1: f32, rot: f32) {
    const STOPS: [[u8; 3]; 8] = [[0x2b, 0x33, 0x3d], [0x7d, 0x88, 0x94], [0x2b, 0x33, 0x3d], [0x15, 0x1a, 0x20], [0x5a, 0x64, 0x70], [0x2b, 0x33, 0x3d], [0x15, 0x1a, 0x20], [0x2b, 0x33, 0x3d]];
    let n = 96u32;
    let mut m = Mesh::default();
    for i in 0..=n {
        let a = i as f32 / n as f32 * TAU;
        let u = ((a + rot) / TAU).rem_euclid(1.0) * (STOPS.len() - 1) as f32;
        let k = (u as usize).min(STOPS.len() - 2);
        let col = lerp_rgb(STOPS[k], STOPS[k + 1], u - k as f32);
        m.colored_vertex(on_circle(c, a, r0), col);
        m.colored_vertex(on_circle(c, a, r1), col);
    }
    for i in 0..n {
        let b = i * 2;
        m.add_triangle(b, b + 1, b + 2);
        m.add_triangle(b + 1, b + 3, b + 2);
    }
    pa.add(Shape::mesh(m));
}

/// Knob: recessed well, 27-segment LED ring (brightening toward the tip), brushed cap, glowing pointer.
#[allow(clippy::too_many_arguments)]
fn knob(ui: &mut egui::Ui, setter: &ParamSetter, c: Pos2, r: f32, param: &FloatParam, label: &str, value: &str, col: Color32, id: impl std::hash::Hash + std::fmt::Debug) -> Response {
    let rect = Rect::from_center_size(c, vec2(r * 2.0 + 28.0, r * 2.0 + 28.0));
    let uid = ui.scope_id().with(("knob", id));
    let resp = ui.interact(rect, uid, Sense::click_and_drag());
    knob_input(ui, setter, &resp, uid.with("d"), param);
    let pa = ui.painter();
    let v = param.unmodulated_normalized_value();
    let hot = resp.hovered() || resp.dragged();
    pa.circle_filled(c, r + 13.0, rgb(0x05, 0x07, 0x0a));
    pa.circle_stroke(c, r + 13.0, Stroke::new(1.0, rgb(0x1b, 0x23, 0x2c)));
    let n = 27usize;
    let lit = (v * n as f32).round() as usize;
    for i in 0..n {
        let (u0, u1) = (i as f32 / n as f32 + 0.006, (i + 1) as f32 / n as f32 - 0.006);
        let pts: Vec<Pos2> = (0..=4).map(|j| on_circle(c, ang(u0 + (u1 - u0) * j as f32 / 4.0), r + 8.0)).collect();
        if i < lit {
            let tip = i + 1 == lit;
            let k = if tip { 1.0 } else { 0.55 + 0.45 * i as f32 / n as f32 };
            glow_line(pa, pts, 4.0, alpha(col, k), if tip { 1.6 } else { 0.7 });
        } else {
            pa.add(Shape::line(pts, Stroke::new(4.0, SEG_OFF)));
        }
    }
    disc_shadow(pa, c, r, vec2(0.0, 5.0), 0.9);
    let t = ui.input(|i| i.time) as f32;
    brushed(pa, c, r * 0.6, r, t * 0.25 + v * 2.0);
    // bevel: light upper rim
    let rim: Vec<Pos2> = (0..=24).map(|j| on_circle(c, PI + PI * j as f32 / 24.0, r - 0.5)).collect();
    pa.add(Shape::line(rim, Stroke::new(1.0, Color32::from_white_alpha(56))));
    disc_radial(pa, c, r * 0.66, rgb(0x1d, 0x24, 0x2c), rgb(0x09, 0x0c, 0x10), vec2(-r * 0.13, -r * 0.2));
    pa.circle_stroke(c, r * 0.66, Stroke::new(1.0, Color32::from_white_alpha(10)));
    let a = ang(v);
    glow_line(pa, vec![on_circle(c, a, r * 0.7), on_circle(c, a, r * 0.95)], 3.0, if hot { TXT } else { col }, 1.0);
    let big = r > 26.0;
    txt(pa, c + vec2(0.0, r + if big { 30.0 } else { 26.0 }), Align2::CENTER_CENTER, label, labb(if big { 13.0 } else { 11.5 }), if hot { TXT } else { DIMT }, 2.2);
    txt(pa, c + vec2(0.0, r + if big { 46.0 } else { 40.0 }), Align2::CENTER_CENTER, value, mono(if big { 12.0 } else { 10.5 }), col, 0.0);
    resp
}

fn frame(root: &mut egui::Ui, setter: &ParamSetter, st: &mut St, p: &ViseParams, sh: &Shared) {
    let ctx_owned = root.ctx().clone();
    let ctx = &ctx_owned;
    ctx.request_repaint_after(std::time::Duration::from_millis(16));
    st.skin.begin(ctx);
    let gr_now = get(&sh.gr);
    let now = ctx.input(|i| i.time);
    if !p.bypass.value() || st.hist.is_empty() {
        st.hist.push_back((get(&sh.vin), get(&sh.vout), gr_now));
        while st.hist.len() > HIST {
            st.hist.pop_front();
        }
    }
    if gr_now > st.pk || now - st.pk_t > 1.5 {
        st.pk = gr_now;
        st.pk_t = now;
    }
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BG)).show(root, |ui| {
        let whole = ui.max_rect();
        let o = whole.min.to_vec2() + vec2(0.0, TOP);
        let at = |x: f32, y: f32| pos2(x, y) + o;
        let rect = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(at(x, y), vec2(w, h));
        let pa = ui.painter().clone();
        let model = p.model.value().clamp(0, 7) as usize;
        // header
        txt(&pa, at(28.0, 40.0), Align2::LEFT_CENTER, "VISE", tech(30.0), TXT, 6.0);
        pa.rect_filled(rect(28.0, 60.0, 162.0, 2.0), 0.0, AMBER);
        txt(&pa, at(206.0, 32.0), Align2::LEFT_CENTER, "DYNAMICS // 8-MODEL COMP + EQ", mono(10.0), CYAN, 1.5);
        txt(&pa, at(206.0, 48.0), Align2::LEFT_CENTER, format!("SYS.0{} // {}", model + 1, MODEL_LINE[model]), mono(10.0), DIMT, 1.0);
        routing(ui, setter, p, sh, o);
        models(ui, setter, p, model, rect(28.0, 76.0, 944.0, 56.0));
        arc_meter(ui, setter, st, p, sh, rect(28.0, 144.0, 500.0, 268.0));
        history(&pa, st, rect(540.0, 144.0, 432.0, 268.0));
        // controls
        panel(&pa, rect(28.0, 424.0, 944.0, 174.0), "03", "CONTROL");
        let ks = p.knobs();
        for i in 0..4 {
            knob(ui, setter, at(100.0 + i as f32 * 128.0, 496.0), 32.0, ks[i], comp::knob_name(model, i), &comp::knob_text(model, i, ks[i].value()), AMBER, ("k", i));
        }
        pa.rect_filled(rect(552.0, 446.0, 1.0, 138.0), 0.0, HAIR);
        pa.rect_filled(rect(812.0, 446.0, 1.0, 138.0), 0.0, HAIR);
        for i in 4..7 {
            knob(ui, setter, at(604.0 + (i - 4) as f32 * 84.0, 490.0), 22.0, ks[i], comp::knob_name(model, i), &comp::knob_text(model, i, ks[i].value()), CYAN, ("k", i));
        }
        knob(ui, setter, at(862.0, 490.0), 22.0, &p.mix, "MIX", &format!("{:.0}%", p.mix.value() * 100.0), TXT, "mix");
        knob(ui, setter, at(936.0, 490.0), 22.0, &p.out, "OUTPUT", &format!("{:+.1} dB", p.out.value()), TXT, "out");
        push_pull(ui, setter, p, rect(28.0, 606.0, 944.0, 130.0));
        footer(ui, setter, p, sh, Rect::from_min_max(at(0.0, 742.0), whole.right_bottom()));
        let _ = st.presets.strip(ui, setter, p, whole, &st.skin, &p.mix, &p.bypass, &p.ui_scale);
    });
}

/// 04 PUSH-PULL EQ (EQP-1A style, after the compressor): ON, LOW FREQ / BOOST / ATTEN,
/// HIGH FREQ / BOOST / BANDWIDTH / ATTEN / ATTEN SEL.
fn push_pull(ui: &mut egui::Ui, setter: &ParamSetter, p: &ViseParams, r: Rect) {
    let pa = ui.painter().clone();
    panel(&pa, r, "04", "PUSH-PULL EQ");
    let on = p.eq_on.value();
    let cr = Rect::from_min_size(r.min + vec2(16.0, 40.0), vec2(84.0, 22.0));
    if chip(ui, cr, if on { "EQ ON" } else { "EQ OFF" }, on, AMBER, "eq_on").on_hover_text("Boost and cut the same low FREQ together for the push-pull trick: a bump at the bottom with a dip just above it").clicked() {
        set_bool(setter, &p.eq_on, !on);
    }
    txt(&pa, r.min + vec2(16.0, 80.0), Align2::LEFT_CENTER, "BOOST + ATTEN", mono(9.0), FAINT, 1.0);
    txt(&pa, r.min + vec2(16.0, 94.0), Align2::LEFT_CENTER, "= PUSH-PULL", mono(9.0), FAINT, 1.0);
    let col = if on { AMBER } else { DIMT };
    let y = r.top() + 56.0;
    let amt = |v: f32| format!("{v:.1}");
    let ks: [(&str, &FloatParam, String, f32); 8] = [
        ("LOW FREQ", &p.eq_lf, crate::pultec::hz_text(crate::pultec::pick(&crate::pultec::LOW_HZ, p.eq_lf.value())), 186.0),
        ("BOOST", &p.eq_lb, amt(p.eq_lb.value()), 276.0),
        ("ATTEN", &p.eq_la, amt(p.eq_la.value()), 366.0),
        ("HIGH FREQ", &p.eq_hf, crate::pultec::hz_text(crate::pultec::pick(&crate::pultec::HIGH_HZ, p.eq_hf.value())), 486.0),
        ("BOOST", &p.eq_hb, amt(p.eq_hb.value()), 576.0),
        ("BANDWIDTH", &p.eq_bw, if p.eq_bw.value() < 3.0 { "SHARP".into() } else if p.eq_bw.value() > 7.0 { "BROAD".into() } else { amt(p.eq_bw.value()) }, 666.0),
        ("ATTEN", &p.eq_ha, amt(p.eq_ha.value()), 776.0),
        ("ATTEN SEL", &p.eq_hs, crate::pultec::hz_text(crate::pultec::pick(&crate::pultec::ATT_HZ, p.eq_hs.value())), 866.0),
    ];
    txt(&pa, pos2(r.left() + 276.0, r.top() + 12.0), Align2::CENTER_CENTER, "LOW", mono(9.0), DIMT, 2.0);
    txt(&pa, pos2(r.left() + 576.0, r.top() + 12.0), Align2::CENTER_CENTER, "HIGH", mono(9.0), DIMT, 2.0);
    pa.rect_filled(Rect::from_min_size(pos2(r.left() + 426.0, r.top() + 22.0), vec2(1.0, 96.0)), 0.0, HAIR);
    pa.rect_filled(Rect::from_min_size(pos2(r.left() + 721.0, r.top() + 22.0), vec2(1.0, 96.0)), 0.0, HAIR);
    for (i, (label, param, value, x)) in ks.iter().enumerate() {
        knob(ui, setter, pos2(r.left() + x, y), 20.0, param, label, value, col, ("eq", i));
    }
}

/// Horizontal 0..100% fader (LINK, M/S MID / SIDE). Double-click = 100%.
fn hfader(ui: &mut egui::Ui, setter: &ParamSetter, bar: Rect, param: &FloatParam, key: &str, label: &str, tip: &str) {
    let pa = ui.painter().clone();
    let id = ui.scope_id().with(key);
    let resp = ui.interact(bar, id, Sense::click_and_drag()).on_hover_text(tip);
    if resp.double_clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, 1.0);
        setter.end_set_parameter(param);
    } else {
        let at_pointer = |q: Pos2| ((q.x - bar.left() - 4.0) / (bar.width() - 8.0)).clamp(0.0, 1.0);
        if resp.clicked() {
            if let Some(q) = resp.interact_pointer_pos() {
                setter.begin_set_parameter(param);
                setter.set_parameter(param, at_pointer(q));
                setter.end_set_parameter(param);
            }
        }
        if resp.drag_started() {
            setter.begin_set_parameter(param);
        }
        if resp.dragged() {
            if let Some(q) = resp.interact_pointer_pos() {
                setter.set_parameter(param, at_pointer(q));
            }
        }
        if resp.drag_stopped() {
            setter.end_set_parameter(param);
        }
    }
    let v = param.value();
    let pts = chamfer(bar, 5.0);
    pa.add(Shape::convex_polygon(pts.clone(), if resp.hovered() || resp.dragged() { PANEL_HI } else { PANEL }, Stroke::NONE));
    let fill = Rect::from_min_max(bar.min + vec2(3.0, 3.0), pos2(bar.left() + 3.0 + (bar.width() - 6.0) * v, bar.bottom() - 3.0));
    if fill.width() > 0.5 {
        pa.rect_filled(fill, 0.0, alpha(CYAN, 0.28));
        glow_line(&pa, vec![pos2(fill.right(), fill.top()), pos2(fill.right(), fill.bottom())], 2.0, CYAN, 0.8);
    }
    pa.add(Shape::closed_line(pts, Stroke::new(1.0, EDGE)));
    let t = if label.is_empty() { format!("{:.0}%", v * 100.0) } else { format!("{label} {:.0}%", v * 100.0) };
    txt(&pa, bar.center(), Align2::CENTER_CENTER, t, mono(10.0), TXT, 1.0);
}

/// Header, top right: CHANNELS + LINK, SIDECHAIN (INT / EXT + key level) + LISTEN.
fn routing(ui: &mut egui::Ui, setter: &ParamSetter, p: &ViseParams, sh: &Shared, o: egui::Vec2) {
    let at = |x: f32, y: f32| pos2(x, y) + o;
    let pa = ui.painter().clone();
    let (y1, y2) = (30.0, 56.0);
    let chip_at = |x: f32, y: f32, w: f32| Rect::from_min_size(at(x, y - 9.0), vec2(w, 18.0));
    txt(&pa, at(470.0, y1), Align2::LEFT_CENTER, "CHANNELS", mono(9.0), DIMT, 1.5);
    let chan = p.chan.value().clamp(0, 3) as usize;
    let mut x = 548.0;
    for (i, (s, w)) in [("STEREO", 58.0), ("M / S", 46.0), ("MID", 42.0), ("SIDE", 46.0)].iter().enumerate() {
        let tip = ["Left and right, linked by LINK", "Mid and side, each compressed on its own: MID / SIDE set how much each gets", "Compress the mid only; the side passes", "Compress the side only; the mid passes"][i];
        if chip(ui, chip_at(x, y1, *w), s, chan == i, CYAN, ("chan", i)).on_hover_text(tip).clicked() {
            set_int(setter, &p.chan, i as i32);
        }
        x += w + 4.0;
    }
    if chan == 1 {
        // M / S: two faders instead of LINK
        hfader(ui, setter, Rect::from_min_size(at(772.0, y1 - 9.0), vec2(96.0, 18.0)), &p.ms_mid, "ms_mid", "MID", "Mid: how much of the compression the mid gets. Double-click: 100%");
        hfader(ui, setter, Rect::from_min_size(at(876.0, y1 - 9.0), vec2(96.0, 18.0)), &p.ms_side, "ms_side", "SIDE", "Side: how much of the compression the side gets. Double-click: 100%");
    } else {
        txt(&pa, at(772.0, y1), Align2::LEFT_CENTER, "LINK", mono(9.0), DIMT, 1.5);
        hfader(ui, setter, Rect::from_min_size(at(806.0, y1 - 9.0), vec2(166.0, 18.0)), &p.link, "link", "", "Stereo link: 100% = one gain for both channels, 0% = each on its own. Double-click: 100%");
    }
    // SIDECHAIN
    txt(&pa, at(470.0, y2), Align2::LEFT_CENTER, "SIDECHAIN", mono(9.0), DIMT, 1.5);
    let ext = p.ext.value();
    if chip(ui, chip_at(548.0, y2, 44.0), "INT", !ext, AMBER, "int").on_hover_text("The compressor listens to this track").clicked() {
        set_bool(setter, &p.ext, false);
    }
    if chip(ui, chip_at(596.0, y2, 44.0), "EXT", ext, AMBER, "ext").on_hover_text("The compressor listens to the sidechain input (route a kick, vocal... into VISE's sidechain in FL)").clicked() {
        set_bool(setter, &p.ext, true);
    }
    if ext {
        let k = get(&sh.key);
        if k > -80.0 {
            glow_dot(&pa, at(652.0, y2), 2.5, CYAN);
            txt(&pa, at(660.0, y2), Align2::LEFT_CENTER, format!("KEY {:.0} dB", k), mono(9.0), CYAN, 1.0);
        } else {
            pa.circle_filled(at(652.0, y2), 2.5, SEG_OFF);
            txt(&pa, at(660.0, y2), Align2::LEFT_CENTER, "NO KEY", mono(9.0), FAINT, 1.0);
        }
    }
    // LISTEN
    txt(&pa, at(772.0, y2), Align2::LEFT_CENTER, "LISTEN", mono(9.0), DIMT, 1.5);
    let lis = p.listen.value().clamp(0, 2) as usize;
    for (i, (s, x, w, tip)) in [(1usize, ("SC", 822.0, 44.0, "Hear what the detector hears (after SC HPF)")), (2, ("Δ DELTA", 870.0, 102.0, "Hear only what the compression takes out"))] {
        if chip(ui, chip_at(x, y2, w), s, lis == i, AMBER, ("listen", i)).on_hover_text(tip).clicked() {
            set_int(setter, &p.listen, if lis == i { 0 } else { i as i32 });
        }
    }
}

fn models(ui: &mut egui::Ui, setter: &ParamSetter, p: &ViseParams, model: usize, r: Rect) {
    let n = comp::N_MODELS;
    let tw = (r.width() - 8.0 * (n - 1) as f32) / n as f32;
    for i in 0..n {
        let tr = Rect::from_min_size(pos2(r.left() + i as f32 * (tw + 8.0), r.top()), vec2(tw, r.height()));
        let resp = ui.interact(tr, ui.scope_id().with(("model", i)), Sense::click());
        if resp.clicked() {
            set_int(setter, &p.model, i as i32);
        }
        let on = i == model;
        let hot = resp.hovered();
        let pa = ui.painter();
        let pts = chamfer(tr, 10.0);
        pa.add(Shape::convex_polygon(pts.clone(), if on { rgb(0x2a, 0x17, 0x07) } else if hot { PANEL_HI } else { PANEL }, Stroke::NONE));
        if on {
            for k in 0..10 {
                pa.rect_filled(Rect::from_min_size(pos2(tr.left(), tr.top() + k as f32 * 3.0), vec2(tw - 10.0, 3.0)), 0.0, alpha(AMBER, 0.25 * (1.0 - k as f32 / 10.0)));
            }
            pa.rect_filled(Rect::from_min_size(tr.min, vec2(tw - 10.0, 3.0)), 0.0, AMBER);
        }
        pa.add(Shape::closed_line(pts, Stroke::new(1.0, if on { AMBER } else if hot { HAIR } else { EDGE })));
        txt(pa, tr.min + vec2(10.0, 14.0), Align2::LEFT_CENTER, format!("M.0{}", i + 1), mono(9.0), if on { AMBER } else { FAINT }, 1.0);
        txt(pa, tr.min + vec2(10.0, 32.0), Align2::LEFT_CENTER, comp::MODELS[i], tech(15.0), if on || hot { TXT } else { DIMT }, 1.5);
        txt(pa, tr.min + vec2(10.0, 47.0), Align2::LEFT_CENTER, SHORT[i], mono(9.0), if on { AMBER } else { FAINT }, 1.0);
        let lc = pos2(tr.right() - 20.0, tr.top() + 15.0);
        if on {
            glow_dot(pa, lc, 3.0, AMBER);
        } else {
            pa.circle_filled(lc, 3.0, SEG_OFF);
        }
    }
}

/// GR: 0..20 dB lit from the right (square-root scale); IN / OUT: -48..+6 dBFS from the left.
fn meter_frac(mode: usize, v: f32) -> f32 {
    if mode == 0 {
        (v / 20.0).clamp(0.0, 1.0).sqrt()
    } else {
        ((v + 48.0) / 54.0).clamp(0.0, 1.0)
    }
}

fn arc_meter(ui: &mut egui::Ui, setter: &ParamSetter, st: &mut St, p: &ViseParams, sh: &Shared, r: Rect) {
    let pa = ui.painter().clone();
    panel(&pa, r, "01", "METER");
    let mode = p.vu.value().clamp(0, 2) as usize;
    let gr_mode = mode == 0;
    let (gr, vin, vout) = (get(&sh.gr), get(&sh.vin), get(&sh.vout));
    let val = [gr, vin, vout][mode];
    let dt = ui.input(|i| i.stable_dt).clamp(0.001, 0.1);
    st.needle += (meter_frac(mode, val) - st.needle) * (1.0 - (-dt / 0.05).exp());
    let c = pos2(r.center().x, r.bottom() - 26.0);
    let big_r = 168.0;
    let (a0, a1) = (PI + 0.14, TAU - 0.14);
    let n = 56;
    for i in 0..n {
        let u = (i as f32 + 0.5) / n as f32;
        let a = a0 + (a1 - a0) * u;
        let on = if gr_mode { u >= 1.0 - st.needle } else { u <= st.needle };
        let pts = vec![on_circle(c, a, big_r - 30.0), on_circle(c, a, big_r - 8.0)];
        if on {
            let col = if gr_mode {
                lerp_rgb([255, 170, 60], [255, 50, 40], 1.0 - u)
            } else if u > 48.0 / 54.0 {
                AMBER
            } else {
                lerp_rgb([70, 200, 255], [232, 238, 244], u)
            };
            glow_line(&pa, pts, 4.2, col, 1.0);
        } else {
            pa.add(Shape::line(pts, Stroke::new(4.2, SEG_OFF)));
        }
    }
    let inner: Vec<Pos2> = (0..=60).map(|j| on_circle(c, a0 + (a1 - a0) * j as f32 / 60.0, big_r - 36.0)).collect();
    pa.add(Shape::line(inner, Stroke::new(1.0, rgb(0x26, 0x31, 0x3c))));
    let ticks: Vec<(f32, &str)> = if gr_mode {
        vec![(0.0, "0"), (1.0, "1"), (2.0, "2"), (3.0, "3"), (5.0, "5"), (7.0, "7"), (10.0, "10"), (15.0, "15"), (20.0, "20")]
    } else {
        vec![(-48.0, "-48"), (-36.0, "-36"), (-24.0, "-24"), (-18.0, "-18"), (-12.0, "-12"), (-6.0, "-6"), (0.0, "0"), (6.0, "+6")]
    };
    for (d, s) in ticks {
        let u = if gr_mode { 1.0 - (d / 20.0).sqrt() } else { (d + 48.0) / 54.0 };
        let a = a0 + (a1 - a0) * u;
        pa.line_segment([on_circle(c, a, big_r - 2.0), on_circle(c, a, big_r + 3.0)], Stroke::new(1.0, DIMT));
        txt(&pa, on_circle(c, a, big_r + 13.0), Align2::CENTER_CENTER, s, mono(10.0), DIMT, 0.0);
    }
    if gr_mode {
        let u = 1.0 - (st.pk / 20.0).clamp(0.0, 1.0).sqrt();
        let a = a0 + (a1 - a0) * u;
        let tip = on_circle(c, a, big_r - 9.0);
        let d = vec2(a.cos(), a.sin());
        let nrm = vec2(-d.y, d.x);
        pa.add(Shape::convex_polygon(vec![tip, tip + d * 9.0 + nrm * 5.0, tip + d * 9.0 - nrm * 5.0], rgb(0xff, 0x40, 0x30), Stroke::NONE));
    }
    // light blade
    let ua = if gr_mode { 1.0 - st.needle } else { st.needle };
    let an = a0 + (a1 - a0) * ua;
    let blade = vec![on_circle(c, an, big_r - 44.0), on_circle(c, an, big_r - 2.0)];
    let bc = if gr_mode { AMBER } else { CYAN };
    pa.add(Shape::line(blade.clone(), Stroke::new(12.0, alpha(bc, 0.07))));
    pa.add(Shape::line(blade.clone(), Stroke::new(6.0, alpha(bc, 0.2))));
    pa.add(Shape::line(blade, Stroke::new(2.0, Color32::from_white_alpha(245))));
    // hub
    let hub: Vec<Pos2> = (0..=24).map(|j| on_circle(c, PI + PI * j as f32 / 24.0, 30.0)).collect();
    pa.add(Shape::convex_polygon(hub.clone(), rgb(0x16, 0x1c, 0x23), Stroke::new(1.0, EDGE)));
    disc_radial(&pa, c, 18.0, rgb(0x2a, 0x33, 0x3d), rgb(0x16, 0x1c, 0x23), vec2(0.0, -4.0));
    pa.rect_filled(Rect::from_min_max(pos2(c.x - 31.0, c.y), pos2(c.x + 31.0, c.y + 20.0)), 0.0, PANEL);
    // readout
    let big = if gr_mode { format!("-{:.1}", gr) } else { format!("{:.1}", val.max(-99.0)) };
    glow_txt(&pa, pos2(c.x, c.y - 74.0), Align2::CENTER_CENTER, &big, tech(40.0), bc, 1.0);
    txt(&pa, pos2(c.x, c.y - 40.0), Align2::CENTER_CENTER, ["dB GAIN REDUCTION", "dBFS INPUT", "dBFS OUTPUT"][mode], mono(10.0), DIMT, 1.5);
    txt(&pa, pos2(r.left() + 16.0, r.bottom() - 12.0), Align2::LEFT_CENTER, format!("IN {:.1}", vin.max(-99.0)), mono(10.0), CYAN, 0.0);
    txt(&pa, pos2(r.right() - 16.0, r.bottom() - 12.0), Align2::RIGHT_CENTER, format!("OUT {:.1}", vout.max(-99.0)), mono(10.0), TXT, 0.0);
    for (i, s) in ["GR", "IN", "OUT"].iter().enumerate() {
        let cr = Rect::from_min_size(pos2(r.right() - 150.0 + i as f32 * 46.0, r.top() + 22.0), vec2(42.0, 18.0));
        if chip(ui, cr, s, mode == i, AMBER, ("vu", i)).clicked() {
            set_int(setter, &p.vu, i as i32);
        }
    }
}

fn history(pa: &Painter, st: &St, r: Rect) {
    panel(pa, r, "02", "HISTORY");
    let gx = r.left() + 14.0;
    let gw = r.width() - 56.0;
    let lane1 = Rect::from_min_size(pos2(gx, r.top() + 30.0), vec2(gw, 70.0));
    let lane2 = Rect::from_min_max(pos2(gx, r.top() + 112.0), pos2(gx + gw, r.bottom() - 14.0));
    pa.rect_filled(lane1, 0.0, rgb(0x0a, 0x0d, 0x11));
    pa.rect_filled(lane2, 0.0, rgb(0x0a, 0x0d, 0x11));
    txt(pa, pos2(lane1.left() + 6.0, lane1.bottom() - 8.0), Align2::LEFT_CENTER, "GR dB", mono(9.0), AMBER, 1.0);
    txt(pa, pos2(lane2.left() + 6.0, lane2.top() + 9.0), Align2::LEFT_CENTER, "IN / OUT dBFS", mono(9.0), DIMT, 1.0);
    for d in [6.0, 12.0, 18.0] {
        let y = lane1.top() + d / 24.0 * lane1.height();
        pa.rect_filled(Rect::from_min_size(pos2(gx, y), vec2(gw, 1.0)), 0.0, alpha(AMBER, 0.10));
        txt(pa, pos2(lane1.right() + 8.0, y), Align2::LEFT_CENTER, format!("{}", d as i32), mono(9.0), FAINT, 0.0);
    }
    for d in [0.0, -24.0, -48.0] {
        let y = lane2.top() + (-d / 48.0) * lane2.height();
        pa.rect_filled(Rect::from_min_size(pos2(gx, y), vec2(gw, 1.0)), 0.0, Color32::from_white_alpha(12));
        txt(pa, pos2(lane2.right() + 8.0, y), Align2::LEFT_CENTER, format!("{}", d as i32), mono(9.0), FAINT, 0.0);
    }
    let n = st.hist.len();
    if n < 2 {
        return;
    }
    let xs = |i: usize| gx + gw - (n - 1 - i) as f32 * gw / (HIST - 1) as f32;
    let ygr = |g: f32| lane1.top() + (g / 24.0).clamp(0.0, 1.0) * lane1.height();
    let ylv = |d: f32| lane2.top() + (-d / 48.0).clamp(0.0, 1.0) * lane2.height();
    strip_fill(pa, (0..n).map(|i| (pos2(xs(i), lane1.top()), pos2(xs(i), ygr(st.hist[i].2)))), alpha(AMBER, 0.28));
    strip_fill(pa, (0..n).map(|i| (pos2(xs(i), ylv(st.hist[i].0)), pos2(xs(i), lane2.bottom()))), alpha(CYAN, 0.10));
    glow_line(pa, (0..n).map(|i| pos2(xs(i), ygr(st.hist[i].2))).collect(), 1.6, AMBER, 0.7);
    pa.add(Shape::line((0..n).map(|i| pos2(xs(i), ylv(st.hist[i].0))).collect(), Stroke::new(1.0, alpha(CYAN, 0.6))));
    pa.add(Shape::line((0..n).map(|i| pos2(xs(i), ylv(st.hist[i].1))).collect(), Stroke::new(1.4, TXT)));
}

/// One seamless fill between pairs of points (a triangle strip), so translucent fills don't show seams.
fn strip_fill(pa: &Painter, pairs: impl Iterator<Item = (Pos2, Pos2)>, col: Color32) {
    let mut m = Mesh::default();
    for (a, b) in pairs {
        m.colored_vertex(a, col);
        m.colored_vertex(b, col);
    }
    let n = m.vertices.len() as u32 / 2;
    for i in 1..n {
        let b = (i - 1) * 2;
        m.add_triangle(b, b + 1, b + 2);
        m.add_triangle(b + 1, b + 3, b + 2);
    }
    pa.add(Shape::mesh(m));
}

fn footer(ui: &mut egui::Ui, setter: &ParamSetter, p: &ViseParams, sh: &Shared, r: Rect) {
    let pa = ui.painter().clone();
    pa.rect_filled(r, 0.0, rgb(0x05, 0x07, 0x0a));
    pa.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, EDGE));
    let cy = r.center().y;
    txt(&pa, pos2(r.left() + 28.0, cy), Align2::LEFT_CENTER, "OVERSAMPLE", mono(10.0), DIMT, 1.5);
    for (i, s) in OS_NAMES.iter().enumerate() {
        let cr = Rect::from_min_size(pos2(r.left() + 116.0 + i as f32 * 48.0, cy - 9.0), vec2(44.0, 18.0));
        if chip(ui, cr, s, p.os.value() == i as i32, CYAN, ("os", i)).on_hover_text("Oversampling for the colour stage").clicked() {
            set_int(setter, &p.os, i as i32);
        }
    }
    let lim = -sh.meters.db(4);
    let safe = p.safe.value();
    let lbl = if safe { if lim > 0.1 { format!("SAFE -{:.1}", lim) } else { "SAFE 0 dB".to_string() } } else { "SAFE OFF".to_string() };
    if chip(ui, Rect::from_min_size(pos2(r.left() + 330.0, cy - 9.0), vec2(100.0, 18.0)), &lbl, safe, AMBER, "safe").on_hover_text("Never over 0 dB").clicked() {
        set_bool(setter, &p.safe, !safe);
    }
    let lat = sh.lat.load(Ordering::Relaxed);
    txt(&pa, pos2(r.left() + 456.0, cy), Align2::LEFT_CENTER, format!("LATENCY {lat} SMP"), mono(10.0), DIMT, 1.0);
    txt(&pa, pos2(r.right() - 28.0, cy), Align2::RIGHT_CENTER, format!("AWK STUDIOS // VISE v{}", env!("CARGO_PKG_VERSION")), mono(10.0), DIMT, 1.5);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, State};
    use nice_plug::context::gui::GuiContextInner;
    use nice_plug::plugin::PluginState;
    use egui::{epaint, Event, ImageData, Modifiers, PointerButton, RawInput, TextureId, TextureWrapMode, Vec2};
    use std::collections::HashMap;

    struct Mock;
    impl GuiContextInner for Mock {
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Vst3
        }
        fn request_restart(&self) {}
        unsafe fn raw_begin_set_parameter(&self, _p: ParamPtr) {}
        unsafe fn raw_set_parameter_normalized(&self, _p: ParamPtr, _n: f32) {}
        unsafe fn raw_end_set_parameter(&self, _p: ParamPtr) {}
        fn get_state(&self) -> PluginState {
            PluginState { version: String::new(), params: Default::default(), fields: Default::default() }
        }
        fn set_state(&self, _s: PluginState) {}
    }
    struct Tex {
        w: usize,
        h: usize,
        px: Vec<[f32; 4]>,
        repeat: bool,
    }
    fn apply_delta(texs: &mut HashMap<TextureId, Tex>, out: &egui::FullOutput) {
        for (id, d) in out.textures_delta.set.iter().flat_map(|(id, ds)| ds.iter().map(move |d| (id, d))) {
            let (w, h, px): (usize, usize, Vec<[f32; 4]>) = match &d.image {
                ImageData::Color(c) => (c.size[0], c.size[1], c.pixels.iter().map(|p| [p.r() as f32 / 255.0, p.g() as f32 / 255.0, p.b() as f32 / 255.0, p.a() as f32 / 255.0]).collect()),
            };
            let repeat = d.options.wrap_mode == TextureWrapMode::Repeat;
            if let Some(pos) = d.pos {
                let t = texs.get_mut(id).unwrap();
                for y in 0..h {
                    for x in 0..w {
                        t.px[(pos[1] + y) * t.w + pos[0] + x] = px[y * w + x];
                    }
                }
            } else {
                texs.insert(*id, Tex { w, h, px, repeat });
            }
        }
    }
    fn render(ctx: &egui::Context, out: egui::FullOutput, texs: &mut HashMap<TextureId, Tex>, path: &str) {
        let img = raster(ctx, out, texs);
        let bytes: Vec<u8> = img.iter().flat_map(|p| p.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8).collect::<Vec<_>>()).collect();
        std::fs::write(path, bytes).unwrap();
    }
    fn raster(ctx: &egui::Context, out: egui::FullOutput, texs: &mut HashMap<TextureId, Tex>) -> Vec<[f32; 3]> {
        apply_delta(texs, &out);
        let (w, h) = (WIDTH as usize, HEIGHT as usize);
        let mut img = vec![[0.0f32; 3]; w * h];
        let prims = ctx.tessellate(out.shapes, 1.0);
        for cp in prims {
            let epaint::Primitive::Mesh(mesh) = cp.primitive else { continue };
            let clip = cp.clip_rect;
            let tex = texs.get(&mesh.texture_id);
            for tri in mesh.indices.chunks(3) {
                let v = [mesh.vertices[tri[0] as usize], mesh.vertices[tri[1] as usize], mesh.vertices[tri[2] as usize]];
                let (x0, x1) = (v.iter().map(|q| q.pos.x).fold(f32::MAX, f32::min).max(clip.left()).max(0.0), v.iter().map(|q| q.pos.x).fold(f32::MIN, f32::max).min(clip.right()).min(w as f32));
                let (y0, y1) = (v.iter().map(|q| q.pos.y).fold(f32::MAX, f32::min).max(clip.top()).max(0.0), v.iter().map(|q| q.pos.y).fold(f32::MIN, f32::max).min(clip.bottom()).min(h as f32));
                let area = (v[1].pos.x - v[0].pos.x) * (v[2].pos.y - v[0].pos.y) - (v[2].pos.x - v[0].pos.x) * (v[1].pos.y - v[0].pos.y);
                if area.abs() < 1e-6 {
                    continue;
                }
                for py in (y0.floor() as usize)..(y1.ceil() as usize).min(h) {
                    for px in (x0.floor() as usize)..(x1.ceil() as usize).min(w) {
                        let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
                        let e = |a: &epaint::Vertex, b: &epaint::Vertex| (b.pos.x - a.pos.x) * (fy - a.pos.y) - (fx - a.pos.x) * (b.pos.y - a.pos.y);
                        let (w0, w1, w2) = (e(&v[1], &v[2]) / area, e(&v[2], &v[0]) / area, e(&v[0], &v[1]) / area);
                        if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                            continue;
                        }
                        let mut c = [0.0f32; 4];
                        for i in 0..4 {
                            c[i] = (w0 * v[0].color.to_array()[i] as f32 + w1 * v[1].color.to_array()[i] as f32 + w2 * v[2].color.to_array()[i] as f32) / 255.0;
                        }
                        if let Some(t) = tex {
                            let mut u = w0 * v[0].uv.x + w1 * v[1].uv.x + w2 * v[2].uv.x;
                            let mut vv = w0 * v[0].uv.y + w1 * v[1].uv.y + w2 * v[2].uv.y;
                            if t.repeat {
                                u = u.rem_euclid(1.0);
                                vv = vv.rem_euclid(1.0);
                            }
                            let tx = ((u * t.w as f32) as usize).min(t.w - 1);
                            let ty = ((vv * t.h as f32) as usize).min(t.h - 1);
                            let s = t.px[ty * t.w + tx];
                            for i in 0..4 {
                                c[i] *= s[i];
                            }
                        }
                        let d = &mut img[py * w + px];
                        for i in 0..3 {
                            d[i] = c[i] + d[i] * (1.0 - c[3]);
                        }
                    }
                }
            }
        }
        img
    }
    fn raw(i: usize, ev: Vec<Event>) -> RawInput {
        RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(WIDTH as f32, HEIGHT as f32))), time: Some(i as f64 * 0.04), events: ev, ..Default::default() }
    }

    #[test]
    fn draws_and_clicks() {
        let ctx = egui::Context::default();
        fonts(&ctx);
        let p = ViseParams::default();
        let sh = Shared::default();
        let mock = Mock;
        let setter = ParamSetter::new(&mock);
        let mut st = new_state("ViseTest");
        let mut i = 0;
        for y in (50..680).step_by(19) {
            for x in (10..990).step_by(37) {
                let q = Pos2::new(x as f32, y as f32);
                let _ = ctx.run_ui(raw(i, vec![Event::PointerMoved(q), Event::PointerButton { pos: q, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]), |c| frame(c, &setter, &mut st, &p, &sh));
                let _ = ctx.run_ui(raw(i + 1, vec![Event::PointerButton { pos: q, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]), |c| frame(c, &setter, &mut st, &p, &sh));
                i += 2;
            }
        }
    }

    /// Chip text must be dead centre (rendered pixels). Guards awk_ui::txt.
    #[test]
    fn chip_text_is_centred() {
        let labels = ["GR", "IN", "OUT", "OFF", "2x", "4x", "8x", "SAFE 0 dB", "STEREO", "M / S", "MID", "SIDE", "INT", "EXT", "SC", "Δ DELTA"];
        for on in [false, true] {
            let ctx = egui::Context::default();
            fonts(&ctx);
            let mut texs = HashMap::new();
            let rects: Vec<Rect> = (0..labels.len()).map(|i| Rect::from_min_size(pos2(40.0 + (i % 4) as f32 * 200.0, 80.0 + (i / 4) as f32 * 60.0), vec2(if labels[i].len() > 3 { 100.0 } else { 44.0 }, 18.0))).collect();
            let mut last = None;
            for f in 0..3 {
                let out = ctx.run_ui(raw(f, vec![]), |c| {
                    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BG)).show(c, |ui| {
                        for (i, r) in rects.iter().enumerate() {
                            let _ = chip(ui, *r, labels[i], on, AMBER, ("t", i));
                        }
                    });
                });
                if f < 2 {
                    apply_delta(&mut texs, &out);
                }
                last = Some(out);
            }
            let img = raster(&ctx, last.unwrap(), &mut texs);
            let w = WIDTH as usize;
            for (i, r) in rects.iter().enumerate() {
                let bg = img[(r.center().y as usize) * w + (r.left() + 6.0) as usize];
                let inner = r.shrink2(vec2(6.0, 3.0));
                let (mut x0, mut x1, mut y0, mut y1) = (usize::MAX, 0, usize::MAX, 0);
                for y in inner.top() as usize..inner.bottom() as usize {
                    for x in inner.left() as usize..inner.right() as usize {
                        let q = img[y * w + x];
                        if (q[0] - bg[0]).abs() + (q[1] - bg[1]).abs() + (q[2] - bg[2]).abs() > 0.45 {
                            x0 = x0.min(x);
                            x1 = x1.max(x);
                            y0 = y0.min(y);
                            y1 = y1.max(y);
                        }
                    }
                }
                assert!(x0 < x1, "{}: no text", labels[i]);
                let (dx, dy) = ((x0 + x1 + 1) as f32 * 0.5 - r.center().x, (y0 + y1 + 1) as f32 * 0.5 - r.center().y);
                assert!(dx.abs() <= 1.0 && dy.abs() <= 1.0, "{} off centre ({dx:+.1}, {dy:+.1})", labels[i]);
            }
        }
    }

    /// SHOT_DIR: the real engine on a drum-ish loop. 0 = ALPHA on GR, 1 = 670 on IN, 2 = DS NUKE.
    #[test]
    fn screenshots() {
        let Ok(dir) = std::env::var("SHOT_DIR") else { return };
        let ctx = egui::Context::default();
        fonts(&ctx);
        let mock = Mock;
        let setter = ParamSetter::new(&mock);
        let mut texs = HashMap::new();
        let k = |n: &str, v: f32| FloatParam::new(n, v, FloatRange::Linear { min: 0.0, max: 1.0 });
        for (n, (model, vu)) in [(3i32, 0i32), (0, 1), (1, 0), (6, 0)].iter().enumerate() {
            let mut p = ViseParams::default();
            p.model = IntParam::new("m", *model, IntRange::Linear { min: 0, max: 7 });
            p.vu = IntParam::new("v", *vu, IntRange::Linear { min: 0, max: 2 });
            p.k0 = k("k0", 0.62);
            let mut stt = State::default();
            stt.model = *model as usize;
            stt.k[0] = 0.62;
            if *model == 0 {
                p.chan = IntParam::new("c", 1, IntRange::Linear { min: 0, max: 3 });
                p.link = FloatParam::new("l", 0.6, FloatRange::Linear { min: 0.0, max: 1.0 });
                stt.chan = 1;
                stt.link = 0.6;
            }
            if *model == 6 {
                p.eq_on = BoolParam::new("eq", true);
                p.eq_lb = FloatParam::new("lb", 6.0, FloatRange::Linear { min: 0.0, max: 10.0 });
                p.eq_la = FloatParam::new("la", 3.0, FloatRange::Linear { min: 0.0, max: 10.0 });
                p.eq_hb = FloatParam::new("hb", 4.0, FloatRange::Linear { min: 0.0, max: 10.0 });
            }
            if *model == 1 {
                p.ext = BoolParam::new("e", true);
                stt.ext = true;
                p.k1 = k("r", 0.95);
                p.k4 = k("d", 0.7);
                p.mix = FloatParam::new("mix", 0.6, FloatRange::Linear { min: 0.0, max: 1.0 });
                stt.k[1] = 0.95;
                stt.k[4] = 0.7;
                stt.mix = 0.6;
            }
            let sh = Arc::new(Shared::default());
            let mut e = Engine::new(48000.0, 1).with_shared(sh.clone());
            let mut st = new_state("ViseShot");
            let mut seed = 5u32;
            let mut t = 0usize;
            let mut stop = 0;
            for f in 0..HIST {
                let mut l: Vec<f32> = (0..1600)
                    .map(|_| {
                        t += 1;
                        let beat = (t % 12000) as f32 / 48000.0;
                        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                        let nz = (seed >> 8) as f32 / 8388608.0 - 1.0;
                        let amp = if (t / 48000) % 4 == 3 { 0.25 } else { 0.7 };
                        amp * ((-beat * 9.0).exp() * (0.7 * (2.0 * std::f32::consts::PI * 55.0 * t as f32 / 48000.0).sin() + 0.4 * nz) + 0.08 * nz)
                    })
                    .collect();
                let mut r = l.clone();
                let key = l.clone();
                e.process_key(&mut l, &mut r, Some((&key, &key)), &stt);
                st.hist.push_back((get(&sh.vin), get(&sh.vout), get(&sh.gr)));
                st.pk = st.pk.max(get(&sh.gr));
                // freeze on a frame with real gain reduction showing
                if f > HIST - 40 && get(&sh.gr) > 3.0 {
                    stop = f;
                    break;
                }
            }
            let _ = stop;
            let v = [get(&sh.gr), get(&sh.vin), get(&sh.vout)][*vu as usize];
            st.needle = meter_frac(*vu as usize, v);
            let mut last = None;
            for f in 0..3 {
                last = Some(ctx.run_ui(raw(n * 10 + f, vec![]), |c| frame(c, &setter, &mut st, &p, &sh)));
                if f < 2 {
                    apply_delta(&mut texs, last.as_ref().unwrap());
                }
            }
            render(&ctx, last.unwrap(), &mut texs, &format!("{}/vise{}.rgb", dir, n));
        }
    }
}
