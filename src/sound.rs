//! Procedurally generated sound effects.
//!
//! Every sound is synthesised at startup and handed to Bevy as an in-memory WAV,
//! so the game ships without any audio files. Gameplay code queues sounds with
//! [`Sounds::play`]; [`play_sounds`] turns the queue into audio entities.

use std::f32::consts::{PI, TAU};
use std::sync::Arc;

use bevy::audio::Volume;
use bevy::prelude::*;

use crate::{Armory, Car, GameState, Player, Rng};

const RATE: u32 = 22_050;
const MASTER_VOLUME: f32 = 0.6;
const ENGINE_VOLUME: f32 = 0.3;

/// Firing frequencies (Hz) the engine loops are recorded at. At runtime the
/// loops nearest the current rpm are crossfaded and only pitched a little,
/// so the exhaust resonances don't slide around like a tape on fast-forward.
const ENGINE_LAYERS: [f32; 3] = [36.0, 60.0, 100.0];
const IDLE_RPM: f32 = 32.0;
const MAX_RPM: f32 = 112.0;
/// Upper speed of each gear; the last one is open-ended.
const GEAR_TOPS: [f32; 4] = [150.0, 290.0, 430.0, 600.0];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sfx {
    Crash,
    Gun,
    BulletHit,
    Rocket,
    MineDrop,
    Explosion,
    Health,
    WeaponPickup,
    Turbo,
    Wreck,
    Win,
    Lose,
    /// Countdown before a race: "3, 2, 1"...
    Beep,
    /// ...and "go".
    Go,
}

impl Sfx {
    const COUNT: usize = 14;

    /// Small random pitch changes keep repeated sounds from getting tiring.
    fn pitch_jitter(self) -> f32 {
        match self {
            Sfx::Crash | Sfx::Gun | Sfx::BulletHit | Sfx::Explosion | Sfx::Wreck => 0.12,
            Sfx::Rocket | Sfx::MineDrop => 0.06,
            _ => 0.0,
        }
    }
}

/// Sounds requested this frame: (sound, volume 0..1).
#[derive(Resource, Default)]
pub struct Sounds(Vec<(Sfx, f32)>);

impl Sounds {
    pub fn play(&mut self, sfx: Sfx, volume: f32) {
        self.0.push((sfx, volume.clamp(0.0, 1.0)));
    }
}

#[derive(Resource)]
struct SfxBank([Handle<AudioSource>; Sfx::COUNT]);

/// One layer of the player's engine sound, recorded at `base_rpm`.
#[derive(Component)]
struct EngineLayer {
    base_rpm: f32,
}

/// Smoothed engine state, so revs rise and fall instead of jumping.
#[derive(Default)]
struct EngineState {
    rpm: f32,
    volume: f32,
}

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Sounds>()
            .add_systems(Startup, build_sounds)
            .add_systems(Update, (play_sounds, engine_sound));
    }
}

fn build_sounds(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut add = |samples: Vec<f32>| {
        sources.add(AudioSource { bytes: Arc::from(to_wav(&normalize(samples))) })
    };
    // Order must match the `Sfx` enum.
    let bank = [
        add(crash(0.45)),
        add(gun()),
        add(bullet_hit()),
        add(rocket()),
        add(mine_drop()),
        add(explosion(1.4)),
        add(health()),
        add(weapon_pickup()),
        add(turbo()),
        add(wreck()),
        add(win()),
        add(lose()),
        add(beep(440.0, 0.25)),
        add(beep(880.0, 0.6)),
    ];
    for base_rpm in ENGINE_LAYERS {
        let handle = add(engine_loop(base_rpm));
        commands.spawn((
            EngineLayer { base_rpm },
            AudioPlayer(handle),
            PlaybackSettings { volume: Volume::Linear(0.0), ..PlaybackSettings::LOOP },
        ));
    }
    commands.insert_resource(SfxBank(bank));
}

fn play_sounds(
    mut commands: Commands,
    mut queue: ResMut<Sounds>,
    bank: Option<Res<SfxBank>>,
    mut rng: ResMut<Rng>,
    sim: Option<Res<crate::Sim>>,
) {
    let Some(bank) = bank else { return };
    if sim.is_some() {
        // Balance sims run silent.
        queue.0.clear();
        return;
    }
    if queue.0.is_empty() {
        return;
    }
    // One instance per sound per frame (at the loudest requested volume), so
    // e.g. six cars hitting the same wall don't stack into a wall of noise.
    let mut loudest = [0.0_f32; Sfx::COUNT];
    let mut order = Vec::new();
    for (sfx, vol) in queue.0.drain(..) {
        let i = sfx as usize;
        if loudest[i] == 0.0 {
            order.push(sfx);
        }
        loudest[i] = loudest[i].max(vol);
    }
    for sfx in order {
        let jitter = sfx.pitch_jitter();
        commands.spawn((
            AudioPlayer(bank.0[sfx as usize].clone()),
            PlaybackSettings {
                volume: Volume::Linear(loudest[sfx as usize] * MASTER_VOLUME),
                speed: 1.0 + rng.range(-jitter, jitter),
                ..PlaybackSettings::DESPAWN
            },
        ));
    }
}

/// Where the revs "want" to be: climbs through each gear, drops on upshift.
fn target_rpm(speed: f32, throttle: f32, turbo: bool) -> f32 {
    let mut lo = 0.0;
    let mut gear_rpm = MAX_RPM;
    for (gear, &hi) in GEAR_TOPS.iter().enumerate() {
        if speed < hi || gear == GEAR_TOPS.len() - 1 {
            let frac = ((speed - lo) / (hi - lo)).clamp(0.0, 1.0);
            let start = if gear == 0 { IDLE_RPM } else { 58.0 };
            gear_rpm = start + (100.0 - start) * frac;
            break;
        }
        lo = hi;
    }
    // Flooring it while slow (or pushing against a car) still revs the engine.
    let free_rev = IDLE_RPM + 45.0 * throttle.abs();
    let rpm = gear_rpm.max(free_rev) + if turbo { 10.0 } else { 0.0 };
    rpm.min(MAX_RPM)
}

/// Drive the player's engine layers from speed and throttle; fade out when not driving.
fn engine_sound(
    time: Res<Time>,
    game: Res<State<GameState>>,
    mut engine: Local<EngineState>,
    player: Query<(&Car, &Armory), With<Player>>,
    mut layers: Query<(&EngineLayer, &mut AudioSink)>,
) {
    let dt = time.delta_secs();
    let driving = player
        .single()
        .ok()
        .filter(|(car, _)| car.alive() && *game.get() == GameState::Playing);

    let (target, target_volume) = match driving {
        Some((car, armory)) => {
            let load = car.throttle.abs();
            (
                // Gears are tuned for a 600 px/s car; scale to this model.
                target_rpm(car.vel.length() * 600.0 / car.model.top_speed, car.throttle, armory.turbo > 0.0),
                0.5 + 0.5 * load,
            )
        }
        None => (IDLE_RPM, 0.0),
    };
    if engine.rpm == 0.0 {
        engine.rpm = IDLE_RPM;
    }
    // Revs climb a bit slower than they fall, which sells the gear changes.
    let rate = if target > engine.rpm { 6.0 } else { 9.0 };
    engine.rpm += (target - engine.rpm) * (1.0 - (-rate * dt).exp());
    engine.volume += (target_volume - engine.volume) * (1.0 - (-10.0 * dt).exp());

    // Equal-power crossfade between the two layers either side of the rpm,
    // interpolated in log space since pitch is perceived logarithmically.
    let rpm = engine.rpm.clamp(ENGINE_LAYERS[0], ENGINE_LAYERS[ENGINE_LAYERS.len() - 1]);
    let weight = |base: f32| -> f32 {
        for pair in ENGINE_LAYERS.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if rpm >= a && rpm <= b {
                let t = (rpm / a).ln() / (b / a).ln();
                if base == a {
                    return (t * PI / 2.0).cos();
                }
                if base == b {
                    return (t * PI / 2.0).sin();
                }
            }
        }
        0.0
    };

    let pitch = player.single().map(|(car, _)| car.model.engine_pitch).unwrap_or(1.0);
    for (layer, mut sink) in &mut layers {
        let w = weight(layer.base_rpm);
        sink.set_speed(engine.rpm / layer.base_rpm * pitch);
        sink.set_volume(Volume::Linear(ENGINE_VOLUME * MASTER_VOLUME * engine.volume * w));
    }
}

// ---------------------------------------------------------------------------
// Synthesis
// ---------------------------------------------------------------------------

/// Render `secs` of audio by calling `f(t)` for every sample.
fn render(secs: f32, mut f: impl FnMut(f32) -> f32) -> Vec<f32> {
    let n = (secs * RATE as f32) as usize;
    (0..n).map(|i| f(i as f32 / RATE as f32)).collect()
}

struct Noise(u32);

impl Noise {
    fn white(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// One-pole low-pass filter; `a` in 0..1, smaller = darker.
struct LowPass(f32);

impl LowPass {
    fn step(&mut self, x: f32, a: f32) -> f32 {
        self.0 += a * (x - self.0);
        self.0
    }
}

/// Oscillator that integrates frequency, so sweeps don't glitch.
struct Osc(f32);

impl Osc {
    fn step(&mut self, freq: f32) -> f32 {
        self.0 = (self.0 + freq / RATE as f32).fract();
        self.0
    }
}

fn sine(phase: f32) -> f32 {
    (phase * TAU).sin()
}
fn saw(phase: f32) -> f32 {
    phase * 2.0 - 1.0
}
fn square(phase: f32) -> f32 {
    if phase < 0.5 { 1.0 } else { -1.0 }
}
/// Fast attack, exponential decay.
fn env(t: f32, attack: f32, decay: f32) -> f32 {
    (t / attack).min(1.0) * (-t * decay).exp()
}

fn crash(secs: f32) -> Vec<f32> {
    let mut n = Noise(0x1234_5678);
    let mut lp = LowPass(0.0);
    render(secs, |t| {
        let body = lp.step(n.white(), 0.3) * env(t, 0.002, 11.0);
        // Inharmonic partials sound like bent metal.
        let ring = (sine(t * 310.0 % 1.0) + sine(t * 527.0 % 1.0) * 0.8 + sine(t * 811.0 % 1.0) * 0.6)
            * env(t, 0.002, 13.0);
        let thump = sine(t * 62.0 % 1.0) * env(t, 0.003, 16.0);
        body * 1.2 + ring * 0.25 + thump * 0.9
    })
}

fn gun() -> Vec<f32> {
    let mut n = Noise(0xBEEF_1234);
    let mut lp = LowPass(0.0);
    render(0.1, |t| {
        let crack = lp.step(n.white(), 0.6) * env(t, 0.0005, 55.0);
        let pop = square(t * 140.0 % 1.0) * env(t, 0.0005, 45.0);
        crack + pop * 0.35
    })
}

fn bullet_hit() -> Vec<f32> {
    let mut n = Noise(0xFACE_0001);
    render(0.06, |t| {
        sine(t * 1900.0 % 1.0) * env(t, 0.0005, 80.0) + n.white() * env(t, 0.0005, 120.0) * 0.6
    })
}

fn rocket() -> Vec<f32> {
    let mut n = Noise(0x0BAD_F00D);
    let mut lp = LowPass(0.0);
    let mut osc = Osc(0.0);
    let dur = 0.75;
    render(dur, |t| {
        let k = t / dur;
        let whoosh = lp.step(n.white(), 0.04 + 0.35 * k) * env(t, 0.04, 2.5);
        let motor = saw(osc.step(85.0 + 90.0 * k)) * env(t, 0.02, 3.5);
        whoosh * 1.4 + motor * 0.25
    })
}

fn mine_drop() -> Vec<f32> {
    let mut n = Noise(0xD00D_FEED);
    render(0.22, |t| {
        let clack = n.white() * env(t, 0.0005, 70.0) + sine(t * 420.0 % 1.0) * env(t, 0.001, 40.0);
        let beep = if (0.09..0.17).contains(&t) {
            square(t * 1050.0 % 1.0) * 0.25 * (1.0 - (t - 0.09) / 0.08)
        } else {
            0.0
        };
        clack * 0.8 + beep
    })
}

fn explosion(secs: f32) -> Vec<f32> {
    let mut n = Noise(0xB00B_1E55);
    let mut lp1 = LowPass(0.0);
    let mut lp2 = LowPass(0.0);
    let mut osc = Osc(0.0);
    render(secs, |t| {
        let white = n.white();
        // Double-filtered noise gives a deep rumble.
        let rumble = lp2.step(lp1.step(white, 0.08), 0.12) * 4.0 * env(t, 0.004, 3.0);
        let crack = white * env(t, 0.0005, 35.0) * 0.5;
        let thump = sine(osc.step(75.0 - 40.0 * (t * 3.0).min(1.0))) * env(t, 0.003, 5.0);
        ((rumble + crack + thump) * 1.6).tanh()
    })
}

fn health() -> Vec<f32> {
    let mut osc = Osc(0.0);
    let dur = 0.35;
    render(dur, |t| {
        let f = 440.0 + 600.0 * (t / dur);
        let p = osc.step(f);
        (sine(p) + 0.3 * sine((p * 2.0) % 1.0)) * (PI * t / dur).sin()
    })
}

/// A short rising arpeggio.
fn arpeggio(notes: &[f32], step: f32, tail: f32, wave: fn(f32) -> f32) -> Vec<f32> {
    let dur = step * (notes.len() - 1) as f32 + tail;
    render(dur, |t| {
        let i = ((t / step) as usize).min(notes.len() - 1);
        let local = t - i as f32 * step;
        let decay = if i == notes.len() - 1 { 4.0 } else { 14.0 };
        let p = (t * notes[i]) % 1.0;
        (sine(p) * 0.7 + wave(p) * 0.3) * env(local, 0.004, decay)
    })
}

fn weapon_pickup() -> Vec<f32> {
    arpeggio(&[523.0, 659.0, 784.0], 0.06, 0.18, square)
}

fn turbo() -> Vec<f32> {
    let mut n = Noise(0x7E57_7E57);
    let mut lp = LowPass(0.0);
    let mut osc = Osc(0.0);
    let dur = 0.8;
    render(dur, |t| {
        let k = t / dur;
        let air = lp.step(n.white(), 0.05 + 0.4 * k) * 1.2;
        let rev = saw(osc.step(110.0 + 330.0 * k * k)) * 0.35;
        (air + rev) * (PI * k).sin()
    })
}

fn wreck() -> Vec<f32> {
    let boom = explosion(1.2);
    let bang = crash(0.6);
    boom.iter()
        .enumerate()
        .map(|(i, b)| b * 0.75 + bang.get(i).copied().unwrap_or(0.0) * 0.6)
        .collect()
}

fn win() -> Vec<f32> {
    arpeggio(&[523.0, 659.0, 784.0, 1047.0], 0.12, 0.6, square)
}

/// A plain start-light beep.
fn beep(freq: f32, dur: f32) -> Vec<f32> {
    render(dur, |t| {
        let p = (t * freq) % 1.0;
        (sine(p) * 0.7 + square(p) * 0.15) * env(t, 0.005, 3.0 / dur)
    })
}

fn lose() -> Vec<f32> {
    let notes = [392.0, 330.0, 262.0, 196.0];
    let step = 0.18;
    let dur = step * 3.0 + 0.8;
    render(dur, |t| {
        let i = ((t / step) as usize).min(3);
        let local = t - i as f32 * step;
        let vibrato = if i == 3 { 1.0 + 0.02 * (local * 30.0).sin() } else { 1.0 };
        let p = (t * notes[i] * vibrato) % 1.0;
        (saw(p) * 0.4 + sine(p) * 0.6) * env(local, 0.005, if i == 3 { 2.5 } else { 8.0 })
    })
}

/// Deterministic hash → 0..1, so per-cylinder variation repeats exactly each loop.
fn hash01(mut x: u32) -> f32 {
    x = (x ^ 61) ^ (x >> 16);
    x = x.wrapping_mul(9);
    x ^= x >> 4;
    x = x.wrapping_mul(0x27d4_eb2d);
    x ^= x >> 15;
    x as f32 / u32::MAX as f32
}

/// Two-pole resonator: rings at `freq` with bandwidth `bw` when excited.
struct Resonator {
    a1: f32,
    a2: f32,
    gain: f32,
    y1: f32,
    y2: f32,
}

impl Resonator {
    fn new(freq: f32, bw: f32, gain: f32) -> Self {
        let r = (-PI * bw / RATE as f32).exp();
        let w = TAU * freq / RATE as f32;
        Self { a1: 2.0 * r * w.cos(), a2: -r * r, gain: gain * (1.0 - r), y1: 0.0, y2: 0.0 }
    }
    fn step(&mut self, x: f32) -> f32 {
        let y = self.gain * x + self.a1 * self.y1 + self.a2 * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// A seamless one-second-ish engine loop at a given firing frequency.
///
/// Each cylinder firing is a short kick of impulse + noise that rings through
/// a few fixed "exhaust pipe" resonances. Firings vary slightly in strength
/// and timing (with one stronger cylinder) to give an uneven, mechanical
/// rumble. The excitation is exactly periodic over the loop, and we render
/// three loops and keep the middle one, so the resonators' tails wrap
/// around seamlessly.
fn engine_loop(firing_hz: f32) -> Vec<f32> {
    let period = (RATE as f32 / firing_hz).round() as usize;
    let pulses = firing_hz.round() as usize; // ≈ one second
    let len = period * pulses;

    // Excitation signal for one loop.
    let mut excite = vec![0.0_f32; len];
    for k in 0..pulses {
        let strength = 0.8 + 0.35 * hash01(k as u32) + if k % 4 == 0 { 0.25 } else { 0.0 };
        let jitter = ((hash01(k as u32 + 1000) - 0.5) * 0.06 * period as f32) as isize;
        let start = (k * period) as isize + jitter;
        let burst = (RATE as f32 * 0.004) as isize; // 4 ms of noise
        for i in 0..burst {
            let n = (start + i).rem_euclid(len as isize) as usize;
            let decay = (-(i as f32) / (burst as f32 * 0.3)).exp();
            let noise = hash01(n as u32 + 77_777) * 2.0 - 1.0;
            let click = if i == 0 { 1.5 } else { 0.0 };
            excite[n] += strength * (click + noise * 0.6 * decay);
        }
    }

    let mut formants = [
        Resonator::new(95.0, 50.0, 1.0),
        Resonator::new(240.0, 90.0, 0.8),
        Resonator::new(560.0, 180.0, 0.45),
        Resonator::new(1350.0, 450.0, 0.15),
    ];
    let mut breath = LowPass(0.0);
    let mut tone = LowPass(0.0);
    let (mut dc_x, mut dc_y) = (0.0_f32, 0.0_f32);

    let mut out = Vec::with_capacity(len);
    for n in 0..len * 3 {
        let i = n % len;
        let x = excite[i];
        let mut s: f32 = formants.iter_mut().map(|r| r.step(x)).sum();

        // Exhaust breath: dark noise that swells right after each firing.
        let since_fire = (i % period) as f32 / period as f32;
        let noise = breath.step(hash01(i as u32 + 5_000_000) * 2.0 - 1.0, 0.12);
        s += noise * 0.5 * (-since_fire * 4.0).exp();

        // Gentle saturation, smoothing, and DC removal.
        let s = tone.step((s * 2.5).tanh(), 0.45);
        let y = s - dc_x + 0.995 * dc_y;
        dc_x = s;
        dc_y = y;

        if n >= len && n < len * 2 {
            out.push(y);
        }
    }
    out
}

fn normalize(mut samples: Vec<f32>) -> Vec<f32> {
    let peak = samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
    if peak > 0.0 {
        for s in &mut samples {
            *s *= 0.9 / peak;
        }
    }
    samples
}

/// Encode mono f32 samples as a 16-bit PCM WAV file.
fn to_wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The jump across the loop seam should look like any other sample step,
    /// otherwise the loop clicks every time it wraps around.
    #[test]
    fn engine_loops_are_seamless() {
        for hz in ENGINE_LAYERS {
            let s = normalize(engine_loop(hz));
            assert!(s.iter().all(|x| x.is_finite()), "{hz} Hz: non-finite samples");
            let max_step = s.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
            let seam = (s[0] - s[s.len() - 1]).abs();
            assert!(seam <= max_step, "{hz} Hz: seam jump {seam} > largest step {max_step}");
        }
    }

    #[test]
    fn engine_loops_have_body() {
        for hz in ENGINE_LAYERS {
            let s = normalize(engine_loop(hz));
            let rms = (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
            let mean = s.iter().sum::<f32>() / s.len() as f32;
            assert!(rms > 0.15, "{hz} Hz: too quiet / spiky (rms {rms})");
            assert!(mean.abs() < 0.02, "{hz} Hz: DC offset {mean}");
        }
    }

    #[test]
    fn revs_climb_and_drop_on_upshift() {
        assert!((target_rpm(0.0, 0.0, false) - IDLE_RPM).abs() < 0.01);
        let before = target_rpm(GEAR_TOPS[0] - 1.0, 1.0, false);
        let after = target_rpm(GEAR_TOPS[0] + 1.0, 1.0, false);
        assert!(after < before - 20.0, "no drop on upshift: {before} -> {after}");
    }

    /// `cargo test dump_sounds -- --ignored` writes every sound to $SOUND_DUMP_DIR.
    #[test]
    #[ignore]
    fn dump_sounds() {
        let dir = std::env::var("SOUND_DUMP_DIR").expect("set SOUND_DUMP_DIR");
        for hz in ENGINE_LAYERS {
            let looped: Vec<f32> = normalize(engine_loop(hz)).repeat(3);
            std::fs::write(format!("{dir}/engine_{hz}hz_x3.wav"), to_wav(&looped)).unwrap();
        }
    }
}
