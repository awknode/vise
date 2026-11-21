# VISE manual

## Signal flow

```
in ─► [M/S encode] ─► COMPRESSOR (+ oversampled COLOR) ─► [decode] ─► MIX with dry ─► OUTPUT ─► SAFE
         detector hears: the track (INT) or the sidechain input (EXT)
```

## Models and their knobs

The first knob is **INPUT** on models that drive into a fixed threshold (670, DS, FAT) and **THRESH** on the others. At INPUT 0 there's no gain reduction.

| Model | Character | Knobs |
|---|---|---|
| **670** | Vari-mu tubes, slow and thick | INPUT, BIAS, TIME (TC 1-6; 5 and 6 are program dependent), TUBES (7 tube sets), COLOR, AUTO GAIN, SC HPF |
| **DS** | FET, fast; ratios 1:1 to 20:1 and NUKE | INPUT, RATIO, ATTACK, RELEASE, DIST, AUTO GAIN, SC HPF |
| **FAT** | Bus compressor with warmth and transformer | INPUT, MODE (GEN / BUSS / SPANK / TRACK), WARMTH, TRANNY, COLOR, AUTO GAIN, SC HPF |
| **ALPHA** | Class A, goes past infinity | THRESH, RATIO, ATTACK, RELEASE, COLOR, AUTO GAIN, SC HPF |
| **SH-A** | Opto + VCA with iron, for the master | THRESH, RATIO (1.2:1 to 6:1), ATTACK, RELEASE, IRON, AUTO GAIN, SC HPF |
| **SH-B** | Opto + VCA with class-A glow | THRESH, RATIO, ATTACK, RELEASE, IRON, AUTO GAIN, SC HPF |

## Routing (header)

- **CHANNELS:** STEREO, M / S (mid and side each compressed on their own), MID only, or SIDE only. The part you don't compress passes through untouched.
- **LINK:** 100 % gives one gain for both channels; 0 % gives each channel its own gain.
- **SIDECHAIN INT / EXT:** EXT makes the compressor listen to the plugin's sidechain input. In FL Studio, route the key track to VISE's mixer track as a sidechain. The KEY readout shows the incoming key level.
- **LISTEN:** SC plays the detector signal (after SC HPF); Δ DELTA plays only what the compression removes.

## Meter

The METER switch picks GR (gain reduction, lit from the right), IN or OUT. The red tick holds the peak. Below the meter, a two-lane history shows gain reduction on top and input/output levels underneath.

## Footer

OVERSAMPLING (OFF / 2x / 4x / 8x) for the colour stage, and SAFE.
