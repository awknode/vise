use vise::*;
const SR: f32 = 48000.0;
fn noise(n: usize, a: f32) -> Vec<f32> {
    let mut s = 4242u32;
    (0..n).map(|_| { s = s.wrapping_mul(1664525).wrapping_add(1013904223); a * ((s >> 8) as f32 / 8388608.0 - 1.0) }).collect()
}
fn rms(x: &[f32]) -> f32 { (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt() }
fn run(e: &mut Engine, st: &State, x: &[f32]) -> Vec<f32> {
    let mut l = x.to_vec(); let mut r = x.to_vec();
    for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) { e.process(a, b, st); }
    l
}

#[test]
fn every_model_squeezes_and_auto_gain_holds_level() {
    for m in 0..6 {
        let mut st = State::default();
        st.model = m;
        st.safe = false;
        st.k[0] = 0.7;
        let mut e = Engine::new(SR, 1);
        let x: Vec<f32> = noise(96000, 0.5).iter().enumerate().map(|(i, v)| v * if (i / 6000) % 2 == 0 { 1.0 } else { 0.2 }).collect();
        let y = run(&mut e, &st, &x);
        let gr = get(&e.shared.gr);
        let d = 20.0 * (rms(&y[48000..]) / rms(&x[48000..])).log10();
        println!("{}: gr {gr:.1} dB, level {d:+.1} dB", comp::MODELS[m]);
        assert!(y.iter().all(|v| v.is_finite()));
        assert!(gr > 0.5, "{} not compressing", comp::MODELS[m]);
        assert!(d.abs() < 6.0, "{} level moved {d:.1} dB", comp::MODELS[m]);
    }
}

#[test]
fn mix_zero_and_bypass_are_the_dry_delayed() {
    for os in 0..4 {
        let mut e = Engine::new(SR, os);
        let lat = e.lat;
        let x = noise(24000, 0.3);
        let mut st = State::default();
        st.mix = 0.0;
        st.safe = false;
        let y = run(&mut e, &st, &x);
        for i in 4000..24000 {
            assert!((y[i] - x[i - lat]).abs() < 1e-5, "os {os} mix 0 sample {i}");
        }
        let mut e = Engine::new(SR, os);
        let mut st = State::default();
        st.bypass = true;
        st.safe = false;
        let y = run(&mut e, &st, &x);
        for i in 8000..24000 {
            assert!((y[i] - x[i - lat]).abs() < 1e-5, "os {os} bypass sample {i}");
        }
    }
}

#[test]
fn safe_never_over_zero() {
    let mut st = State::default();
    st.out_db = 18.0;
    st.k[5] = 0.0;
    let mut e = Engine::new(SR, 1);
    let y = run(&mut e, &st, &noise(48000, 0.9));
    let pk = y.iter().fold(0.0f32, |a, v| a.max(v.abs()));
    assert!(pk <= 1.0, "peak {pk}");
}

#[test]
fn cpu() {
    let st = State::default();
    let mut e = Engine::new(SR, 3);
    let x = noise(48000 * 5, 0.3);
    let t = std::time::Instant::now();
    run(&mut e, &st, &x);
    let pct = t.elapsed().as_secs_f32() / 5.0 * 100.0;
    println!("vise 8x: {pct:.2}% of one core");
    assert!(pct < 10.0);
}

fn run_st(e: &mut Engine, st: &State, l: &[f32], r: &[f32], key: Option<(&[f32], &[f32])>) -> (Vec<f32>, Vec<f32>) {
    let (mut a, mut b) = (l.to_vec(), r.to_vec());
    let mut i = 0;
    while i < a.len() {
        let j = (i + 256).min(a.len());
        let k = key.map(|(x, y)| (&x[i..j], &y[i..j]));
        e.process_key(&mut a[i..j], &mut b[i..j], k, st);
        i = j;
    }
    (a, b)
}
fn db_ratio(y: &[f32], x: &[f32]) -> f32 {
    20.0 * (rms(y) / rms(x)).log10()
}
fn clean_alpha() -> State {
    let mut st = State::default();
    st.model = 3;
    st.k[4] = 0.0; // no colour
    st.k[5] = 0.0; // no auto gain: reduction shows as level
    st.safe = false;
    st
}

/// SC EXT: a loud key squeezes a quiet track; without EXT (or with no key) it's ignored.
#[test]
fn external_key_drives_compression() {
    let x = noise(96000, 0.04);
    let key: Vec<f32> = noise(96000, 0.9).iter().rev().cloned().collect();
    let mut st = clean_alpha();
    st.k[0] = 0.6;
    let (y_int, _) = run_st(&mut Engine::new(SR, 1), &st, &x, &x, Some((&key, &key)));
    st.ext = true;
    let mut e = Engine::new(SR, 1);
    let (y_ext, _) = run_st(&mut e, &st, &x, &x, Some((&key, &key)));
    let gr = get(&e.shared.gr);
    let (y_none, _) = run_st(&mut Engine::new(SR, 1), &st, &x, &x, None);
    let (a, b, c) = (db_ratio(&y_int[48000..], &x[48000..]), db_ratio(&y_ext[48000..], &x[48000..]), db_ratio(&y_none[48000..], &x[48000..]));
    println!("int {a:+.1} dB, ext {b:+.1} dB (gr {gr:.1}), ext without key {c:+.1} dB, key level {:.1}", get(&e.shared.key));
    assert!(a.abs() < 0.5 && c.abs() < 0.5);
    assert!(b < -6.0 && gr > 6.0);
    assert!(get(&e.shared.key) > -10.0);
}

/// LINK 0: a loud left doesn't pull the quiet right down. LINK 100: it does.
#[test]
fn link_zero_keeps_channels_apart() {
    let l = noise(96000, 0.8);
    let r: Vec<f32> = noise(96000, 0.03).iter().rev().cloned().collect();
    let mut st = clean_alpha();
    st.k[0] = 0.6;
    let (_, r1) = run_st(&mut Engine::new(SR, 1), &st, &l, &r, None);
    st.link = 0.0;
    let (l0, r0) = run_st(&mut Engine::new(SR, 1), &st, &l, &r, None);
    let (d1, d0, dl0) = (db_ratio(&r1[48000..], &r[48000..]), db_ratio(&r0[48000..], &r[48000..]), db_ratio(&l0[48000..], &l[48000..]));
    println!("right: linked {d1:+.1} dB, unlinked {d0:+.1} dB; left unlinked {dl0:+.1} dB");
    assert!(d1 < -6.0, "linked right should duck");
    assert!(d0.abs() < 0.5, "unlinked right should be left alone");
    assert!(dl0 < -6.0, "unlinked left still compresses");
}

/// MID only: the side passes exactly (delayed); SIDE only: the mid does.
#[test]
fn mid_only_and_side_only_leave_the_other_half() {
    for os in [0, 2] {
        let m = noise(48000, 0.7);
        let s: Vec<f32> = noise(48000, 0.1).iter().rev().cloned().collect();
        let l: Vec<f32> = m.iter().zip(&s).map(|(a, b)| a + b).collect();
        let r: Vec<f32> = m.iter().zip(&s).map(|(a, b)| a - b).collect();
        for (chan, keep) in [(2usize, &s), (3, &m)] {
            let mut st = clean_alpha();
            st.k[0] = 0.7;
            st.chan = chan;
            let mut e = Engine::new(SR, os);
            let lat = e.lat;
            let (yl, yr) = run_st(&mut e, &st, &l, &r, None);
            for i in 4000..48000 {
                let got = if chan == 2 { (yl[i] - yr[i]) * 0.5 } else { (yl[i] + yr[i]) * 0.5 };
                assert!((got - keep[i - lat]).abs() < 1e-4, "chan {chan} os {os} sample {i}: {got} vs {}", keep[i - lat]);
            }
            assert!(get(&e.shared.gr) > 1.0, "chan {chan} still compresses its half");
        }
    }
}

/// DELTA is near silent when nothing is compressed and loud when it is.
#[test]
fn delta_is_what_gets_taken_out() {
    // in-band material (the oversampler's filters only differ above ~20 kHz)
    let tones = |amp: f32| -> Vec<f32> { (0..96000).map(|i| { let t = i as f32 / SR; amp * (0.5 * (std::f32::consts::TAU * 110.0 * t).sin() + 0.3 * (std::f32::consts::TAU * 1250.0 * t).sin() + 0.2 * (std::f32::consts::TAU * 5100.0 * t).sin()) }).collect() };
    let quiet = tones(0.03);
    let loud = tones(0.9);
    let mut st = clean_alpha();
    st.k[0] = 0.5;
    st.listen = 2;
    let (dq, _) = run_st(&mut Engine::new(SR, 1), &st, &quiet, &quiet, None);
    let (dl, _) = run_st(&mut Engine::new(SR, 1), &st, &loud, &loud, None);
    let (q, l) = (db_ratio(&dq[48000..], &quiet[48000..]), db_ratio(&dl[48000..], &loud[48000..]));
    println!("delta: uncompressed {q:.1} dB, compressed {l:.1} dB re input");
    // what remains is the colour stage DC blocker (~5 Hz HPF): 2.6° of phase at 110 Hz ≈ -27 dB
    assert!(q < -24.0, "delta should be near silent without compression");
    assert!(l > -12.0, "delta should carry what was removed");
}

/// SC listen plays the key (after the SC HPF).
#[test]
fn sc_listen_plays_the_key() {
    let x = noise(48000, 0.3);
    let key: Vec<f32> = (0..48000).map(|i| 0.3 * (std::f32::consts::TAU * 1000.0 * i as f32 / SR).sin()).collect();
    let mut st = clean_alpha();
    st.k[6] = 0.0;
    st.ext = true;
    st.listen = 1;
    let (y, _) = run_st(&mut Engine::new(SR, 1), &st, &x, &x, Some((&key, &key)));
    let d = db_ratio(&y[24000..], &key[24000..]);
    println!("sc listen vs key: {d:+.2} dB");
    assert!(d.abs() < 1.0);
}

/// INPUT / THRESH at 0 = no compression, on every model, for a hot drum loop (peaks -5 dBFS).
#[test]
fn knob_at_zero_does_not_compress() {
    let mut z = 77u32;
    let x: Vec<f32> = (0..96000).map(|i| {
        z = z.wrapping_mul(1664525).wrapping_add(1013904223);
        let nz = (z >> 8) as f32 / 8388608.0 - 1.0;
        let t = (i % 24000) as f32 / SR;
        let t2 = ((i + 12000) % 24000) as f32 / SR;
        let kick = (-t * 12.0).exp() * (std::f32::consts::TAU * (50.0 + 80.0 * (-t * 30.0).exp()) * t).sin();
        let snare = if (i / 24000) % 2 == 1 { (-t2 * 18.0).exp() * nz * 0.6 } else { 0.0 };
        0.5 * (kick + snare) + 0.02 * nz
    }).collect();
    for m in 0..6 {
        let mut st = State::default();
        st.model = m;
        st.k[0] = 0.0;
        let mut e = Engine::new(SR, 1);
        let (mut l, mut r) = (x.clone(), x.clone());
        let mut mx = 0f32;
        for (i, (a, b)) in l.chunks_mut(256).zip(r.chunks_mut(256)).enumerate() {
            e.process(a, b, &st);
            if i > 40 { mx = mx.max(get(&e.shared.gr)); }
        }
        println!("{}: max GR at knob 0 = {mx:.2} dB", comp::MODELS[m]);
        assert!(mx < 0.5, "{} compresses {mx:.1} dB with the knob at 0", comp::MODELS[m]);
    }
}
