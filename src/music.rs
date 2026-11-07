//! Background music: fast synthpop, composed and synthesised in code like the
//! sound effects, so the game still ships without audio files.
//!
//! Every arena and track has its own song, and the menus have a calmer one.
//! A song is an arrangement (intro, verse, chorus, breakdown, chorus) over a
//! chord progression, played by a small synth band:
//!
//! - four-on-the-floor kick, claps on 2 and 4, off-beat open hats,
//! - an octave-bouncing bass,
//! - detuned "supersaw" chords that pump with the kick (sidechain ducking),
//! - a plucked arpeggio and a lead hook built from the chord tones,
//! - risers and cymbal crashes into the choruses.
//!
//! Songs are rendered the first time they're needed (notes are cached, so it
//! takes a fraction of a second) and crossfade when the screen changes. `N`
//! switches the music off and on.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::audio::Volume;
use bevy::prelude::*;

use crate::sound::{LowPass, Noise, Osc, RATE, env, normalize, saw, sine, square, to_wav};
use crate::{CurrentArena, GameState};

const MUSIC_VOLUME: f32 = 0.24;
/// Seconds to crossfade from one song to the next.
const FADE: f32 = 1.2;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Song {
    Menu,
    DustBowl,
    Junkyard,
    Timberland,
    Dustline,
    RedCanyon,
    PineRidge,
}

/// Chord progressions, as scale degrees (0 = the key's first chord).
const POP: &[i32] = &[0, 4, 5, 3]; // I  V  vi IV
const POP_ALT: &[i32] = &[5, 3, 0, 4]; // vi IV I  V
/// Desert: home chord, the half-step-up chord, home, the one below.
const SAND: &[i32] = &[0, 1, 0, 6];
const SAND_ALT: &[i32] = &[0, 6, 1, 0];
/// Folk: home, the major chord a fourth up, home, the one a step down.
const FOLK: &[i32] = &[0, 3, 0, 6];
const FOLK_ALT: &[i32] = &[3, 0, 6, 0];
const IRON: &[i32] = &[0, 6, 5, 6];
const IRON_ALT: &[i32] = &[0, 1, 5, 6];

/// Lead hook shapes: (16th step, length in steps, chord tone) over one bar.
/// Chord tone 0/1/2 is the triad, 3 the octave above the root, -1 below.
const HOOKS: [&[(usize, usize, i32)]; 4] = [
    &[(0, 3, 2), (3, 3, 1), (6, 2, 2), (8, 2, 3), (10, 4, 2), (14, 2, 1)],
    &[(0, 2, 0), (2, 2, 1), (4, 2, 2), (6, 4, 3), (12, 2, 2), (14, 2, 1)],
    &[(0, 4, 3), (4, 2, 2), (6, 2, 1), (8, 6, 2), (14, 2, 0)],
    &[(0, 2, 2), (2, 1, 2), (3, 3, 3), (6, 2, 2), (8, 3, 1), (11, 3, 0), (14, 2, -1)],
];

/// Desert melodies: two-bar phrases of (16th step, length in steps, scale
/// step from the tonic). They move mostly by step over the drone, lean on the
/// half step above the tonic, and come home to it, like a maqam line rather
/// than a pop hook. Played as call (first) and answer (second).
/// (16th step, length in steps, pitch) notes of a phrase.
type Phrase = &'static [(usize, usize, i32)];

const DESERT_PHRASES: [[Phrase; 2]; 2] = [
    [
        &[(0, 2, 4), (2, 2, 3), (4, 2, 2), (6, 2, 1), (8, 4, 2), (12, 2, 1), (14, 2, 0), (16, 3, 1), (19, 1, 2), (20, 4, 1), (24, 8, 0)],
        &[(0, 2, 4), (2, 2, 5), (4, 4, 4), (8, 2, 3), (10, 2, 4), (12, 4, 3), (16, 2, 2), (18, 2, 3), (20, 2, 2), (22, 2, 1), (24, 8, 0)],
    ],
    [
        &[(0, 4, 0), (4, 2, 1), (6, 2, 2), (8, 6, 3), (14, 2, 2), (16, 2, 1), (18, 2, 2), (20, 4, 1), (24, 8, 0)],
        &[(0, 2, 2), (2, 2, 3), (4, 2, 4), (6, 2, 5), (8, 6, 4), (14, 2, 3), (16, 2, 2), (18, 2, 1), (20, 4, 2), (24, 8, 1)],
    ],
];

const MAJOR: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];
#[cfg_attr(not(test), allow(dead_code))]
const MINOR: [i32; 7] = [0, 2, 3, 5, 7, 8, 10];
/// Middle-Eastern scales: the half step up from the root and the wide step
/// in the middle are what make them sound "oriental".
const HIJAZ: [i32; 7] = [0, 1, 4, 5, 7, 8, 10];
const DOUBLE_HARMONIC: [i32; 7] = [0, 1, 4, 5, 7, 8, 11];
const PHRYGIAN: [i32; 7] = [0, 1, 3, 5, 7, 8, 10];
/// Folk modes.
const DORIAN: [i32; 7] = [0, 2, 3, 5, 7, 9, 10];
const MIXOLYDIAN: [i32; 7] = [0, 2, 4, 5, 7, 9, 10];

/// The flavour on top of the synthpop band.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Theme {
    /// Plain synthpop.
    Synth,
    /// Sand and canyons: hand drums, a drone, an oud-like pluck, a ney-like
    /// lead with grace notes, Middle-Eastern scales.
    Desert,
    /// Woods: woodblock and shaker, a kalimba, a breathy flute, birdsong and
    /// wind chimes, folk modes.
    Forest,
    /// Scrap: metal clanks and a crunchy bit-crushed lead.
    Industrial,
}

/// Everything that makes one song different from another.
struct SongDef {
    bpm: f32,
    /// MIDI note of the key's root.
    root: i32,
    scale: [i32; 7],
    theme: Theme,
    verse: &'static [i32],
    chorus: &'static [i32],
    hook: usize,
    /// How bright the chords are (filter opening, 0..1).
    brightness: f32,
    /// How gritty the bass is.
    grit: f32,
    /// Synth lead voice: 0 = saw, 1 = square, in between mixes them.
    lead_square: f32,
    /// The calm menu song has no claps or big build-ups.
    calm: bool,
}

impl Song {
    #[cfg_attr(not(test), allow(dead_code))]
    const ALL: [Song; 7] =
        [Song::Menu, Song::DustBowl, Song::Junkyard, Song::Timberland, Song::Dustline, Song::RedCanyon, Song::PineRidge];

    fn def(self) -> SongDef {
        let base = SongDef {
            bpm: 126.0,
            root: 57,
            scale: MAJOR,
            theme: Theme::Synth,
            verse: POP,
            chorus: POP,
            hook: 0,
            brightness: 0.6,
            grit: 1.0,
            lead_square: 0.3,
            calm: false,
        };
        match self {
            Song::Menu => SongDef {
                bpm: 110.0,
                root: 53,
                verse: POP_ALT,
                chorus: POP_ALT,
                hook: 2,
                brightness: 0.35,
                lead_square: 0.8,
                calm: true,
                ..base
            },
            Song::DustBowl => SongDef {
                root: 57,
                scale: HIJAZ,
                theme: Theme::Desert,
                verse: SAND,
                chorus: SAND_ALT,
                hook: 1,
                brightness: 0.5,
                grit: 2.0,
                ..base
            },
            Song::Dustline => SongDef {
                bpm: 128.0,
                root: 55,
                scale: DOUBLE_HARMONIC,
                theme: Theme::Desert,
                verse: SAND_ALT,
                chorus: SAND,
                hook: 0,
                brightness: 0.7,
                ..base
            },
            Song::RedCanyon => SongDef {
                bpm: 132.0,
                root: 50,
                scale: PHRYGIAN,
                theme: Theme::Desert,
                verse: &[0, 1, 5, 1],
                chorus: &[0, 6, 5, 1],
                hook: 3,
                brightness: 0.6,
                grit: 2.2,
                ..base
            },
            Song::Timberland => SongDef {
                bpm: 120.0,
                root: 55,
                scale: DORIAN,
                theme: Theme::Forest,
                verse: FOLK,
                chorus: FOLK_ALT,
                hook: 2,
                brightness: 0.3,
                ..base
            },
            Song::PineRidge => SongDef {
                bpm: 124.0,
                root: 52,
                scale: MIXOLYDIAN,
                theme: Theme::Forest,
                verse: FOLK_ALT,
                chorus: &[0, 6, 3, 0],
                hook: 1,
                brightness: 0.35,
                ..base
            },
            Song::Junkyard => SongDef {
                bpm: 122.0,
                root: 52,
                scale: PHRYGIAN,
                theme: Theme::Industrial,
                verse: IRON,
                chorus: IRON_ALT,
                hook: 3,
                brightness: 0.4,
                grit: 3.0,
                lead_square: 0.9,
                ..base
            },
        }
    }
}

/// MIDI note of scale degree `degree` (any integer) above `root`.
fn degree_note(root: i32, scale: &[i32; 7], degree: i32) -> i32 {
    root + scale[degree.rem_euclid(7) as usize] + 12 * degree.div_euclid(7)
}

/// The next note of the scale above `note`.
fn next_in_scale(root: i32, scale: &[i32; 7], note: i32) -> i32 {
    (-14..=28).map(|d| degree_note(root, scale, d)).find(|&n| n > note).unwrap_or(note + 2)
}

fn freq(midi: i32) -> f32 {
    440.0 * 2f32.powf((midi - 69) as f32 / 12.0)
}

/// A section of the arrangement.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Intro,
    Verse,
    Chorus,
    Break,
}

/// Rendered notes, keyed by instrument, pitch and length, so repeated notes
/// are only synthesised once.
#[derive(Default)]
struct NoteCache(HashMap<(u8, i32, usize), Arc<Vec<f32>>>);

impl NoteCache {
    fn get(&mut self, instrument: u8, midi: i32, len: usize, make: impl FnOnce() -> Vec<f32>) -> Arc<Vec<f32>> {
        self.0.entry((instrument, midi, len)).or_insert_with(|| Arc::new(make())).clone()
    }
}

/// A song as a seamless loop.
pub fn render_song(song: Song) -> Vec<f32> {
    let def = song.def();
    let theme = def.theme;
    let step_len = 60.0 / def.bpm / 4.0 * RATE as f32; // a 16th, in samples
    let arrangement: &[(Part, usize)] = if def.calm {
        &[(Part::Intro, 4), (Part::Verse, 8), (Part::Chorus, 8), (Part::Break, 4)]
    } else {
        &[(Part::Intro, 4), (Part::Verse, 8), (Part::Chorus, 8), (Part::Break, 4), (Part::Chorus, 8)]
    };
    let bars: usize = arrangement.iter().map(|(_, n)| n).sum();
    let len = (bars as f32 * 16.0 * step_len) as usize;
    // Three busses: drums, a "pumped" bus ducked by the kick, and the rest.
    let (mut drums, mut pumped, mut rest) = (vec![0.0_f32; len], vec![0.0_f32; len], vec![0.0_f32; len]);
    let at = |bar: usize, s: f32| ((bar as f32 * 16.0 + s) * step_len) as usize;
    let add = |bus: &mut Vec<f32>, start: usize, samples: &[f32], gain: f32| {
        for (i, v) in samples.iter().enumerate() {
            bus[(start + i) % len] += v * gain;
        }
    };
    let samples_of = |steps: f32| (steps * step_len) as usize;
    let mut cache = NoteCache::default();
    let mut seed = 0x5EED_0001_u32 ^ (def.root as u32 * 7919);
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    let (kick, crash) = (kick(), crash());
    let riser = riser(samples_of(64.0));
    // The backbeat (2 and 4) and the off-beat "hats", per theme.
    let (backbeat, offbeat, ticks) = match theme {
        Theme::Synth => (clap(), hat(0.18), hat(0.04)),
        Theme::Desert => (darbuka_tek(), tambourine(), tambourine()),
        Theme::Forest => (woodblock(), shaker(), shaker()),
        Theme::Industrial => (clank(), metal_tick(), metal_tick()),
    };
    let doum = darbuka_doum();

    let mut bar = 0;
    for &(part, count) in arrangement {
        for k in 0..count {
            let chords = if part == Part::Chorus { def.chorus } else { def.verse };
            let degree = chords[k % chords.len()];
            let chord: Vec<i32> = (0..3).map(|i| degree_note(def.root, &def.scale, degree + 2 * i)).collect();
            let full = part == Part::Verse || part == Part::Chorus;
            let last = k == count - 1;

            // Drums.
            for s in 0..16 {
                let beat = s % 4 == 0;
                let kick_on = match part {
                    Part::Intro | Part::Break => s == 0,
                    _ => beat,
                };
                if kick_on && !(def.calm && part == Part::Break) {
                    add(&mut drums, at(bar, s as f32), &kick, 0.9);
                }
                if full && !def.calm && (s == 4 || s == 12) {
                    add(&mut drums, at(bar, s as f32), &backbeat, 0.55);
                }
                if full && s % 4 == 2 {
                    let level = match (def.calm, part) {
                        (true, _) => 0.12,
                        (false, Part::Chorus) => 0.22,
                        _ => 0.14,
                    };
                    add(&mut drums, at(bar, s as f32), &offbeat, level);
                }
                let tick_on = match theme {
                    // A shaker keeps going all through the woods.
                    Theme::Forest => full && s % 2 == 1,
                    _ => part == Part::Chorus && s % 2 == 1,
                };
                if tick_on {
                    add(&mut drums, at(bar, s as f32), &ticks, 0.1);
                }
                // A roll building into the chorus.
                if !def.calm && last && part == Part::Verse && s >= 8 {
                    add(&mut drums, at(bar, s as f32), &backbeat, 0.15 + 0.03 * (s - 8) as f32);
                }
            }
            match theme {
                // The darbuka's "doum - tek - tek - doum - tek" (maqsum) rhythm
                // over the beat, sparser in the intro and breakdown.
                Theme::Desert => {
                    let pattern: &[(usize, bool)] = &[(0, true), (2, false), (6, false), (8, true), (12, false), (14, false)];
                    for &(s, low) in pattern {
                        if !full && !low {
                            continue;
                        }
                        let (hit, gain) = if low { (&doum, 0.5) } else { (&backbeat, 0.3) };
                        add(&mut drums, at(bar, s as f32), hit, gain);
                    }
                }
                // Metal hits on the off-beats in the scrapyard.
                Theme::Industrial if full => {
                    add(&mut drums, at(bar, 7.0), &backbeat, 0.2);
                    add(&mut drums, at(bar, 14.0), &backbeat, 0.25);
                }
                _ => {}
            }
            if !def.calm && part == Part::Chorus && k == 0 {
                add(&mut rest, at(bar, 0.0), &crash, 0.3);
            }
            // A riser over the whole breakdown, into the last chorus.
            if !def.calm && part == Part::Break && k == 0 {
                add(&mut rest, at(bar, 0.0), &riser, 0.2);
            }

            // Bass: root on the beat, octave up on the off-beat (synthpop's
            // bouncing eighths). Sparser in the breakdown, none in the intro.
            if part != Part::Intro {
                // In the desert the bass stays home on the tonic, under the drone.
                let low = if theme == Theme::Desert { def.root - 24 } else { chord[0] - 24 };
                for e in 0..8 {
                    if part == Part::Break && e % 2 == 1 {
                        continue;
                    }
                    let note = if e % 2 == 0 { low } else { low + 12 };
                    let n = samples_of(1.8);
                    let s = cache.get(1, note, n, || bass(freq(note), n, def.grit));
                    add(&mut pumped, at(bar, e as f32 * 2.0), &s, 0.42);
                }
            }

            // Chords, the whole bar, pumping with the kick. The desert has an
            // open drone (root and fifth) instead of full chords.
            let n = samples_of(16.0);
            let held = match part {
                Part::Chorus => 0.15,
                Part::Verse => 0.05,
                _ => 0.08,
            };
            match theme {
                // A drone stays on the tonic whatever the bass does.
                Theme::Desert => {
                    let tonic = def.root;
                    for note in [tonic, tonic + 7, tonic + 12] {
                        let s = cache.get(5, note, n, || drone(freq(note), n));
                        add(&mut pumped, at(bar, 0.0), &s, held * 1.1);
                    }
                }
                _ => {
                    for (i, &note) in chord.iter().enumerate() {
                        let note = note + if i == 0 { 12 } else { 0 };
                        let s = cache.get(2, note, n, || supersaw(freq(note), n, def.brightness));
                        add(&mut pumped, at(bar, 0.0), &s, held);
                    }
                }
            }

            // The arpeggio. Synth and scrapyard: up and down the chord.
            // Desert: an oud running up and down the scale. Woods: a kalimba.
            if part != Part::Break || def.calm || theme == Theme::Forest {
                let pattern = [0, 1, 2, 3, 2, 1, 0, 2];
                let scale_run = [0, 1, 2, 1, 0, -1, 0, 2];
                for s in 0..16 {
                    if theme == Theme::Industrial && s % 2 == 1 {
                        continue;
                    }
                    let note = match theme {
                        Theme::Desert => degree_note(def.root, &def.scale, degree + scale_run[s % 8]) + 12,
                        _ => {
                            (match pattern[s % 8] {
                                3 => chord[0] + 12,
                                i => chord[i],
                            }) + 12
                        }
                    };
                    let n = samples_of(2.0);
                    let gain = if part == Part::Chorus { 0.12 } else { 0.09 };
                    let tone = match theme {
                        Theme::Desert => cache.get(6, note, n, || oud(freq(note), n)),
                        Theme::Forest => cache.get(7, note + 12, n, || kalimba(freq(note + 12), n)),
                        _ => cache.get(3, note, n, || pluck(freq(note), n, def.brightness)),
                    };
                    add(&mut rest, at(bar, s as f32), &tone, gain * if theme == Theme::Forest { 1.3 } else { 1.0 });
                }
            }

            // Desert lead: stepwise phrases around the tonic, call and answer,
            // with one quick ornament as each phrase comes home.
            if part == Part::Chorus && theme == Theme::Desert && k % 2 == 0 {
                let phrases = &DESERT_PHRASES[def.hook % DESERT_PHRASES.len()];
                let phrase = phrases[(k / 2) % 2];
                // Keep the line in a comfortable register.
                let mut home = def.root + 12;
                while home < 64 {
                    home += 12;
                }
                let tonic_shift = home - def.root;
                for (i, &(s, length, step)) in phrase.iter().enumerate() {
                    let note = degree_note(def.root, &def.scale, step) + tonic_shift;
                    let n = samples_of(length as f32 * 0.94);
                    let voice = cache.get(8, note, n, || ney(freq(note), n));
                    add(&mut rest, at(bar, s as f32), &voice, 0.26);
                    if i == phrase.len() - 1 {
                        let upper = next_in_scale(def.root, &def.scale, note);
                        let g = samples_of(0.5);
                        let gv = cache.get(8, upper, g, || ney(freq(upper), g));
                        add(&mut rest, at(bar, s as f32 - 0.5), &gv, 0.12);
                    }
                }
            }

            // The lead hook in the choruses, dipping lower every other bar
            // like an answer to the first.
            if part == Part::Chorus && theme != Theme::Desert {
                for &(s, length, tone) in HOOKS[def.hook] {
                    let base = match tone {
                        -1 => chord[2] - 12,
                        3 => chord[0] + 12,
                        t => chord[t as usize],
                    };
                    let note = base + if k % 2 == 1 && tone >= 2 { 0 } else { 12 };
                    let n = samples_of(length as f32 * 0.92);
                    let (voice, gain) = match theme {
                        Theme::Forest => (cache.get(9, note, n, || flute(freq(note), n)), 0.3),
                        Theme::Industrial => (cache.get(10, note, n, || crushed(freq(note), n)), 0.2),
                        _ => (cache.get(4, note, n, || lead(freq(note), n, def.lead_square)), 0.24),
                    };
                    add(&mut rest, at(bar, s as f32), &voice, if def.calm { gain * 0.7 } else { gain });
                }
            }

            // The woods: birdsong in the quieter parts, wind chimes now and then.
            if theme == Theme::Forest {
                if part != Part::Chorus && rand() < 0.7 {
                    let s = (rand() * 12.0) as usize;
                    let call = bird_call(rand());
                    add(&mut rest, at(bar, s as f32), &call, 0.12);
                }
                if (part == Part::Intro || part == Part::Break || k % 4 == 3) && rand() < 0.6 {
                    for (c, &tone) in chord.iter().enumerate() {
                        let note = tone + 36;
                        let n = samples_of(12.0);
                        let bell = cache.get(11, note, n, || chime(freq(note), n));
                        add(&mut rest, at(bar, 4.0 * c as f32 + rand() * 2.0), &bell, 0.06);
                    }
                }
            }
            bar += 1;
        }
    }

    // Sidechain: duck the pumped bus right after every beat.
    let beat_len = 4.0 * step_len;
    for (i, v) in pumped.iter_mut().enumerate() {
        let since = (i as f32 % beat_len) / RATE as f32;
        *v *= 1.0 - 0.65 * (-since * 14.0).exp();
    }
    // Echo on the melodic parts, then mix and glue with gentle saturation.
    let delay = (3.0 * step_len) as usize;
    let dry = rest.clone();
    for i in 0..len {
        rest[(i + delay) % len] += dry[i] * 0.22;
    }
    normalize((0..len).map(|i| ((drums[i] + pumped[i] + rest[i]) * 0.9).tanh()).collect())
}

fn kick() -> Vec<f32> {
    let mut osc = Osc(0.0);
    crate::sound::render(0.3, |t| {
        let f = 48.0 + 120.0 * (-t * 35.0).exp();
        (sine(osc.step(f)) * env(t, 0.001, 8.0) * 1.2 + if t < 0.002 { 0.5 } else { 0.0 }).tanh()
    })
}

/// A clap: a few quick noise bursts, then a short tail.
fn clap() -> Vec<f32> {
    let mut n = Noise(0xC1A9_0001);
    let mut lp = LowPass(0.0);
    let mut hp = LowPass(0.0);
    crate::sound::render(0.25, |t| {
        let white = n.white();
        let band = lp.step(white, 0.5) - hp.step(white, 0.08);
        let bursts: f32 = [0.0, 0.011, 0.022].iter().map(|&b| if t >= b { env(t - b, 0.0005, 60.0) } else { 0.0 }).sum();
        band * (bursts * 0.6 + env(t, 0.03, 14.0) * 0.7)
    })
}

fn hat(decay: f32) -> Vec<f32> {
    let mut n = Noise(0x4A70_0001);
    let mut lp = LowPass(0.0);
    crate::sound::render(decay * 3.0, |t| {
        let white = n.white();
        (white - lp.step(white, 0.55)) * env(t, 0.0005, 1.0 / decay)
    })
}

fn crash() -> Vec<f32> {
    let mut n = Noise(0xC2A5_0001);
    let mut lp = LowPass(0.0);
    crate::sound::render(2.0, |t| {
        let white = n.white();
        (white - lp.step(white, 0.35)) * env(t, 0.002, 1.8)
    })
}

/// White noise sweeping up over `len` samples.
fn riser(len: usize) -> Vec<f32> {
    let mut n = Noise(0x2153_0001);
    let mut lp = LowPass(0.0);
    let secs = len as f32 / RATE as f32;
    crate::sound::render(secs, |t| {
        let k = t / secs;
        lp.step(n.white(), 0.02 + 0.6 * k * k) * k * k
    })
}

fn bass(f: f32, len: usize, grit: f32) -> Vec<f32> {
    let mut osc = Osc(0.0);
    let mut sub = Osc(0.0);
    let mut lp1 = LowPass(0.0);
    let mut lp2 = LowPass(0.0);
    let dur = len as f32 / RATE as f32;
    crate::sound::render(dur, |t| {
        let gate = ((dur - t) / 0.01).clamp(0.0, 1.0);
        // The filter snaps open on each note: a plucky, punchy bass.
        let cutoff = 0.05 + 0.25 * (-t * 18.0).exp();
        let s = lp2.step(lp1.step(saw(osc.step(f)), cutoff), cutoff) + 0.5 * sine(sub.step(f));
        (s * grit * 1.5).tanh() * gate * (t / 0.003).min(1.0)
    })
}

/// Five detuned saws: the big synthpop chord sound.
fn supersaw(f: f32, len: usize, brightness: f32) -> Vec<f32> {
    let detune = [-0.012, -0.006, 0.0, 0.006, 0.012];
    let mut oscs: Vec<Osc> = (0..5).map(|i| Osc(i as f32 * 0.21)).collect();
    let mut lp1 = LowPass(0.0);
    let mut lp2 = LowPass(0.0);
    let dur = len as f32 / RATE as f32;
    let cutoff = 0.06 + 0.25 * brightness;
    crate::sound::render(dur, |t| {
        let s: f32 = oscs.iter_mut().zip(detune).map(|(o, d)| saw(o.step(f * (1.0 + d)))).sum::<f32>() / 2.5;
        let fade = (t / 0.01).min(1.0) * ((dur - t) / 0.05).clamp(0.0, 1.0);
        lp2.step(lp1.step(s, cutoff), cutoff) * fade
    })
}

fn pluck(f: f32, len: usize, brightness: f32) -> Vec<f32> {
    let mut osc = Osc(0.0);
    let mut lp = LowPass(0.0);
    let dur = len as f32 / RATE as f32;
    crate::sound::render(dur, |t| {
        let p = osc.step(f);
        let cutoff = 0.1 + 0.5 * brightness * (-t * 20.0).exp();
        lp.step(saw(p) * 0.6 + square(p) * 0.4, cutoff) * env(t, 0.001, 10.0)
    })
}

fn lead(f: f32, len: usize, squareness: f32) -> Vec<f32> {
    let mut a = Osc(0.0);
    let mut b = Osc(0.5);
    let mut lp = LowPass(0.0);
    let dur = len as f32 / RATE as f32;
    crate::sound::render(dur + 0.06, |t| {
        let gate = if t < dur { 1.0 } else { (1.0 - (t - dur) / 0.06).max(0.0) };
        // Vibrato creeps in on held notes.
        let vib = 1.0 + 0.007 * (t * 5.5 * std::f32::consts::TAU).sin() * ((t - 0.15) * 3.0).clamp(0.0, 1.0);
        let pa = a.step(f * vib);
        let pb = b.step(f * vib * 1.004);
        let s = (saw(pa) * (1.0 - squareness) + square(pa) * squareness) * 0.6 + saw(pb) * 0.4;
        lp.step(s, 0.35) * gate * (t / 0.006).min(1.0)
    })
}

// --- Desert -----------------------------------------------------------------

/// Darbuka "doum": a deep, round hand-drum hit with a little pitch drop.
fn darbuka_doum() -> Vec<f32> {
    let mut osc = Osc(0.0);
    crate::sound::render(0.35, |t| sine(osc.step(95.0 + 60.0 * (-t * 25.0).exp())) * env(t, 0.001, 9.0))
}

/// Darbuka "tek": a sharp, ringing slap on the rim.
fn darbuka_tek() -> Vec<f32> {
    let mut n = Noise(0x7E4B_0001);
    let mut lp = LowPass(0.0);
    crate::sound::render(0.15, |t| {
        let white = n.white();
        let ring = sine((t * 760.0) % 1.0) * 0.6 + sine((t * 1240.0) % 1.0) * 0.3;
        (ring * env(t, 0.0005, 35.0) + (white - lp.step(white, 0.3)) * env(t, 0.0005, 60.0) * 0.6) * 0.9
    })
}

/// Tambourine: bright jingles, a little longer than a hi-hat.
fn tambourine() -> Vec<f32> {
    let mut n = Noise(0x7A3B_0001);
    let mut lp = LowPass(0.0);
    crate::sound::render(0.2, |t| {
        let white = n.white();
        let jingle = (white - lp.step(white, 0.7)) * (1.0 + 0.6 * sine((t * 5300.0) % 1.0));
        jingle * env(t, 0.002, 18.0) * 0.8
    })
}

/// A harmonium-like drone: reedy, with a slow swell and tremolo.
fn drone(f: f32, len: usize) -> Vec<f32> {
    let mut a = Osc(0.0);
    let mut b = Osc(0.3);
    let mut lp = LowPass(0.0);
    let dur = len as f32 / RATE as f32;
    crate::sound::render(dur, |t| {
        let reed = square(a.step(f)) * 0.5 + saw(b.step(f * 1.003)) * 0.5;
        let tremolo = 1.0 + 0.12 * (t * 4.5 * std::f32::consts::TAU).sin();
        let fade = (t / 0.08).min(1.0) * ((dur - t) / 0.08).clamp(0.0, 1.0);
        lp.step(reed, 0.12) * tremolo * fade
    })
}

/// An oud-like pluck: bright, nasal, quick to fade, sliding up into pitch.
fn oud(f: f32, len: usize) -> Vec<f32> {
    let mut osc = Osc(0.0);
    let mut lp = LowPass(0.0);
    let dur = len as f32 / RATE as f32;
    crate::sound::render(dur, |t| {
        let slide = 1.0 - 0.012 * (-t * 80.0).exp();
        let p = osc.step(f * slide);
        let body = saw(p) * 0.7 + sine((p * 2.0) % 1.0) * 0.3;
        lp.step(body, 0.25 + 0.4 * (-t * 25.0).exp()) * env(t, 0.001, 14.0)
    })
}

/// A ney-like lead: breathy and reedy, scooping up into each note, with a
/// wide, slow vibrato.
fn ney(f: f32, len: usize) -> Vec<f32> {
    let mut osc = Osc(0.0);
    let mut n = Noise(0x2E70_0001 ^ (f as u32));
    let mut breath_lp = LowPass(0.0);
    let mut lp = LowPass(0.0);
    let dur = len as f32 / RATE as f32;
    crate::sound::render(dur + 0.08, |t| {
        let gate = if t < dur { 1.0 } else { (1.0 - (t - dur) / 0.08).max(0.0) };
        // A slight scoop into the note and a gentle, late vibrato: enough to
        // sound breathed, not so much it sounds out of tune.
        let scoop = 1.0 - 0.02 * (-t * 35.0).exp();
        let vib = 1.0 + 0.005 * (t * 5.0 * std::f32::consts::TAU).sin() * ((t - 0.2) * 3.0).clamp(0.0, 1.0);
        let p = osc.step(f * scoop * vib);
        let tone = sine(p) * 0.7 + sine((p * 2.0) % 1.0) * 0.25 + saw(p) * 0.15;
        // Faintly airy throughout, with a puff as the note starts.
        let breath = breath_lp.step(n.white(), 0.3) * (0.06 + 0.12 * (-t * 25.0).exp());
        lp.step(tone + breath, 0.4) * gate * (t / 0.03).min(1.0)
    })
}

// --- Forest -----------------------------------------------------------------

/// A hollow woodblock knock.
fn woodblock() -> Vec<f32> {
    crate::sound::render(0.12, |t| (sine((t * 820.0) % 1.0) * 0.8 + sine((t * 2150.0) % 1.0) * 0.3) * env(t, 0.0005, 40.0))
}

/// A soft shaker: a short swell of high noise.
fn shaker() -> Vec<f32> {
    let mut n = Noise(0x54A4_0001);
    let mut lp = LowPass(0.0);
    crate::sound::render(0.1, |t| {
        let white = n.white();
        (white - lp.step(white, 0.5)) * (t / 0.02).min(1.0) * (-t * 40.0).exp() * 0.7
    })
}

/// A kalimba (thumb piano): a pure, bell-like plucked tone.
fn kalimba(f: f32, len: usize) -> Vec<f32> {
    let dur = len as f32 / RATE as f32;
    crate::sound::render(dur, |t| {
        (sine((t * f) % 1.0) + 0.3 * sine((t * f * 5.4) % 1.0) * (-t * 30.0).exp()) * env(t, 0.001, 7.0)
    })
}

/// A soft flute with gentle vibrato: a pure tone with just a short puff of
/// air as each note starts (no hiss under held notes).
fn flute(f: f32, len: usize) -> Vec<f32> {
    let mut osc = Osc(0.0);
    let mut n = Noise(0xF107_0001 ^ (f as u32));
    let mut breath_lp = LowPass(0.0);
    let dur = len as f32 / RATE as f32;
    crate::sound::render(dur + 0.1, |t| {
        let gate = if t < dur { 1.0 } else { (1.0 - (t - dur) / 0.1).max(0.0) };
        let vib = 1.0 + 0.008 * (t * 5.2 * std::f32::consts::TAU).sin() * ((t - 0.12) * 4.0).clamp(0.0, 1.0);
        let p = osc.step(f * vib);
        let tone = sine(p) + 0.12 * sine((p * 2.0) % 1.0) + 0.05 * sine((p * 3.0) % 1.0);
        let chiff = breath_lp.step(n.white(), 0.3) * 0.08 * (-t * 30.0).exp();
        (tone * 0.8 + chiff) * gate * (t / 0.04).min(1.0)
    })
}

/// A little bird: two to four quick chirps sweeping up or down. `pick`
/// (0..1) chooses which bird.
fn bird_call(pick: f32) -> Vec<f32> {
    let chirps = 2 + (pick * 3.0) as usize;
    let base = 2600.0 + 1800.0 * pick;
    let down = pick > 0.5;
    let mut osc = Osc(0.0);
    let each = 0.07;
    let gap = 0.05;
    crate::sound::render(chirps as f32 * (each + gap), |t| {
        let i = (t / (each + gap)) as usize;
        let local = t - i as f32 * (each + gap);
        if local > each {
            return 0.0;
        }
        let k = local / each;
        let sweep = if down { 1.25 - 0.45 * k } else { 0.8 + 0.5 * k };
        let f = base * sweep * (1.0 + 0.04 * i as f32);
        sine(osc.step(f)) * (std::f32::consts::PI * k).sin()
    })
}

/// A wind chime: a high bell tone with a long shimmer.
fn chime(f: f32, len: usize) -> Vec<f32> {
    let dur = len as f32 / RATE as f32;
    crate::sound::render(dur, |t| {
        (sine((t * f) % 1.0) + 0.4 * sine((t * f * 2.76) % 1.0) * (-t * 4.0).exp()) * env(t, 0.002, 1.5)
    })
}

// --- Scrapyard -----------------------------------------------------------------

/// A metal clank: inharmonic ringing partials and a hit of noise.
fn clank() -> Vec<f32> {
    let mut n = Noise(0xC1A4_0002);
    crate::sound::render(0.3, |t| {
        let ring: f32 = [(523.0, 1.0), (811.0, 0.8), (1270.0, 0.6), (1967.0, 0.4)]
            .iter()
            .map(|&(f, a)| sine((t * f) % 1.0) * a)
            .sum();
        (ring * 0.4 * env(t, 0.0005, 14.0) + n.white() * env(t, 0.0005, 70.0) * 0.6).tanh()
    })
}

/// A dry metallic tick.
fn metal_tick() -> Vec<f32> {
    crate::sound::render(0.06, |t| (sine((t * 3170.0) % 1.0) + sine((t * 4630.0) % 1.0)) * 0.5 * env(t, 0.0005, 70.0))
}

/// A crunchy lead: a square wave with its resolution and sample rate
/// crushed, like an old sampler.
fn crushed(f: f32, len: usize) -> Vec<f32> {
    let mut osc = Osc(0.0);
    let dur = len as f32 / RATE as f32;
    let mut held = 0.0;
    let mut i = 0;
    crate::sound::render(dur + 0.04, |t| {
        let gate = if t < dur { 1.0 } else { (1.0 - (t - dur) / 0.04).max(0.0) };
        let p = osc.step(f);
        let s = square(p) * 0.6 + saw(p) * 0.4;
        if i % 3 == 0 {
            held = (s * 4.0).round() / 4.0;
        }
        i += 1;
        held * gate * (t / 0.005).min(1.0) * 0.8
    })
}

// ---------------------------------------------------------------------------
// Playing
// ---------------------------------------------------------------------------

/// Rendered songs, and whether music is on.
#[derive(Resource)]
struct Music {
    songs: HashMap<Song, Handle<AudioSource>>,
    on: bool,
    playing: Option<(Song, Entity)>,
}

/// A song being played, fading in (or out and then removed, if `leaving`).
#[derive(Component)]
struct MusicPlayer {
    volume: f32,
    leaving: bool,
}

pub struct MusicPlugin;

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Music { songs: HashMap::new(), on: true, playing: None })
            .add_systems(Update, (toggle_music, choose_song, fade_music).chain());
    }
}

fn toggle_music(keys: Res<ButtonInput<KeyCode>>, mut music: ResMut<Music>) {
    if keys.just_pressed(KeyCode::KeyN) {
        music.on = !music.on;
    }
}

/// Menu music in the menu and garage; the venue's own song once you're
/// choosing (or on) a track or arena.
fn wanted_song(state: GameState, venue: Song) -> Song {
    match state {
        GameState::Menu | GameState::Garage => Song::Menu,
        GameState::TrackSelect | GameState::Playing | GameState::GameOver => venue,
    }
}

fn choose_song(
    mut commands: Commands,
    mut music: ResMut<Music>,
    mut sources: ResMut<Assets<AudioSource>>,
    state: Res<State<GameState>>,
    current: Res<CurrentArena>,
    sim: Option<Res<crate::Sim>>,
    mut players: Query<&mut MusicPlayer>,
) {
    if sim.is_some() {
        return; // balance sims run silent
    }
    let want = wanted_song(*state.get(), current.def().music);
    if music.playing.is_some_and(|(song, _)| song == want) {
        return;
    }
    // Fade out whatever is playing, start the new song from silence.
    if let Some((_, old)) = music.playing.take()
        && let Ok(mut p) = players.get_mut(old)
    {
        p.leaving = true;
    }
    let handle = music
        .songs
        .entry(want)
        .or_insert_with(|| sources.add(AudioSource { bytes: Arc::from(to_wav(&render_song(want))) }))
        .clone();
    let entity = commands
        .spawn((
            MusicPlayer { volume: 0.0, leaving: false },
            AudioPlayer(handle),
            PlaybackSettings { volume: Volume::Linear(0.0), ..PlaybackSettings::LOOP },
        ))
        .id();
    music.playing = Some((want, entity));
}

fn fade_music(
    mut commands: Commands,
    time: Res<Time<Real>>,
    music: Res<Music>,
    mut players: Query<(Entity, &mut MusicPlayer, Option<&mut AudioSink>)>,
) {
    let step = time.delta_secs() / FADE;
    for (entity, mut p, sink) in &mut players {
        let target = if p.leaving || !music.on { 0.0 } else { 1.0 };
        p.volume = if p.volume < target { (p.volume + step).min(target) } else { (p.volume - step).max(target) };
        if p.leaving && p.volume <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        if let Some(mut sink) = sink {
            sink.set_volume(Volume::Linear(p.volume * MUSIC_VOLUME));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn songs_are_seamless_loops_with_some_life_in_them() {
        for song in Song::ALL {
            let s = render_song(song);
            assert!(s.iter().all(|x| x.is_finite()), "{song:?}: non-finite samples");
            let rms = (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
            assert!(rms > 0.08, "{song:?}: too quiet (rms {rms})");
            // The loop point (a kick on the downbeat) should be no bigger a
            // jump than the music makes anywhere else.
            let largest = s.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
            let seam = (s[0] - s[s.len() - 1]).abs();
            assert!(seam <= largest, "{song:?}: seam jump {seam} > largest step {largest}");
        }
    }

    #[test]
    fn every_venue_has_its_own_song() {
        let mut seen: Vec<Song> = crate::arena::ARENAS.iter().map(|a| a.music).collect();
        seen.sort_by_key(|s| *s as u8);
        seen.dedup();
        assert_eq!(seen.len(), crate::arena::ARENAS.len());
        assert!(!seen.contains(&Song::Menu));
    }

    #[test]
    fn scale_degrees_wrap_into_octaves() {
        assert_eq!(degree_note(57, &MINOR, 0), 57);
        assert_eq!(degree_note(57, &MINOR, 7), 69);
        assert_eq!(degree_note(57, &MINOR, -1), 55);
        assert_eq!(degree_note(57, &MINOR, 2), 60);
        assert_eq!(degree_note(57, &MAJOR, 2), 61);
        // Hijaz: a half step, then the wide step that makes it sound eastern.
        assert_eq!(degree_note(57, &HIJAZ, 1), 58);
        assert_eq!(degree_note(57, &HIJAZ, 2), 61);
        assert_eq!(next_in_scale(57, &HIJAZ, 57), 58);
        assert_eq!(next_in_scale(57, &HIJAZ, 58), 61);
    }

    /// `cargo test dump_music -- --ignored` writes every song to $SOUND_DUMP_DIR.
    #[test]
    #[ignore]
    fn dump_music() {
        let dir = std::env::var("SOUND_DUMP_DIR").expect("set SOUND_DUMP_DIR");
        for song in Song::ALL {
            std::fs::write(format!("{dir}/music_{song:?}.wav"), to_wav(&render_song(song))).unwrap();
        }
    }
}
