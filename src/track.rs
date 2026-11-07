//! Race tracks: a closed centreline (a smooth curve through a few control
//! points) with a fixed width. Barriers run along both edges, and every car's
//! progress is measured along the centreline, so laps, positions and the AI's
//! racing line all come from the same data.

use bevy::prelude::*;

use crate::terrain::{Cut, Deck, DeckKind, Terrain};

/// Something along a track. Positions are fractions of a lap from the line.
#[derive(Clone, Copy, Debug)]
pub enum Feature {
    /// A pad that gives a burst of turbo; `lateral` is its offset across the track.
    Boost { at: f32, lateral: f32 },
    /// A ramp across the track that throws cars into the air.
    Ramp { at: f32 },
    /// A ramp in front of a ravine cut across the track: jump it or fall in.
    Jump { at: f32 },
    /// A river crossing the track twice: once on a bridge, once through a ford.
    River { bridge: f32, ford: f32 },
}

/// How tall a ramp's lip is, and how long its slope.
pub const RAMP_HEIGHT: f32 = 26.0;
pub const RAMP_LENGTH: f32 = 110.0;
/// Width of the gap a jump has to clear.
pub const RAVINE_GAP: f32 = 130.0;
/// The river channel: flat bottom, banks and depth.
const RIVER_HALF: f32 = 70.0;
const RIVER_BANK: f32 = 90.0;
const RIVER_DEPTH: f32 = 95.0;
/// A boost pad's size (along, across).
pub const BOOST_PAD: Vec2 = Vec2::new(70.0, 56.0);

/// A track as authored: control points of a closed loop, in driving order.
/// The start/finish line sits on the first point, with the grid behind it.
pub struct TrackDef {
    pub points: &'static [Vec2],
    pub width: f32,
    pub laps: u32,
    pub asphalt: Color,
    /// Alternating kerb colours on the edges.
    pub kerbs: [Color; 2],
    pub features: &'static [Feature],
}

/// How far apart centreline samples are.
const SPACING: f32 = 30.0;
/// Barrier thickness.
pub const BARRIER: f32 = 30.0;

/// The track as used at runtime: evenly spaced centreline samples.
#[derive(Resource, Clone)]
pub struct Track {
    points: Vec<Vec2>,
    /// Unit direction of travel at each sample.
    dirs: Vec<Vec2>,
    /// How sharply the track bends at each sample (1 / radius; positive
    /// bends left).
    curvature: Vec<f32>,
    pub length: f32,
    pub width: f32,
    pub laps: u32,
    pub features: &'static [Feature],
}

/// Where something is relative to the track.
#[derive(Clone, Copy, Debug)]
pub struct Spot {
    /// Index of the sample at the start of the segment it's on.
    pub index: usize,
    /// Distance along the centreline from the start line.
    pub s: f32,
    /// Sideways offset from the centreline (positive = left of travel).
    pub lateral: f32,
}

fn catmull_rom(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let (t2, t3) = (t * t, t * t * t);
    0.5 * ((2.0 * p1) + (-p0 + p2) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2 + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

impl Track {
    pub fn new(def: &TrackDef) -> Self {
        // A dense polyline through the control points...
        let n = def.points.len();
        let mut dense = Vec::new();
        for i in 0..n {
            let p = |k: usize| def.points[(i + k + n - 1) % n];
            for k in 0..64 {
                dense.push(catmull_rom(p(0), p(1), p(2), p(3), k as f32 / 64.0));
            }
        }
        let mut cum = vec![0.0];
        for i in 0..dense.len() {
            let d = dense[i].distance(dense[(i + 1) % dense.len()]);
            cum.push(cum[i] + d);
        }
        let total = *cum.last().unwrap();
        // ...resampled at even spacing.
        let count = (total / SPACING).round() as usize;
        let step = total / count as f32;
        let mut points = Vec::with_capacity(count);
        let mut j = 0;
        for i in 0..count {
            let s = i as f32 * step;
            while cum[j + 1] < s {
                j += 1;
            }
            let t = (s - cum[j]) / (cum[j + 1] - cum[j]).max(1e-6);
            points.push(dense[j].lerp(dense[(j + 1) % dense.len()], t));
        }
        let dirs: Vec<Vec2> = (0..count)
            .map(|i| (points[(i + 1) % count] - points[(i + count - 1) % count]).normalize())
            .collect();
        let curvature = (0..count)
            .map(|i| {
                let a = dirs[(i + count - 2) % count];
                let b = dirs[(i + 2) % count];
                a.angle_to(b) / (4.0 * step)
            })
            .collect();
        Self {
            points,
            dirs,
            curvature,
            length: count as f32 * step,
            width: def.width,
            laps: def.laps,
            features: def.features,
        }
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn step(&self) -> f32 {
        self.length / self.points.len() as f32
    }

    fn index_at(&self, s: f32) -> (usize, f32) {
        let f = s.rem_euclid(self.length) / self.step();
        let i = (f.floor() as usize) % self.len();
        (i, f.fract())
    }

    /// Centreline point at distance `s` (wraps around).
    pub fn point_at(&self, s: f32) -> Vec2 {
        let (i, t) = self.index_at(s);
        self.points[i].lerp(self.points[(i + 1) % self.len()], t)
    }

    /// Direction of travel at distance `s`.
    pub fn dir_at(&self, s: f32) -> Vec2 {
        let (i, t) = self.index_at(s);
        self.dirs[i].lerp(self.dirs[(i + 1) % self.len()], t).normalize()
    }

    /// How sharply the track bends at `s` (1 / radius, either way).
    pub fn curvature_at(&self, s: f32) -> f32 {
        self.curvature[self.index_at(s).0].abs()
    }

    /// Sample `i`: centreline point, direction and signed curvature.
    pub fn sample(&self, i: usize) -> (Vec2, Vec2, f32) {
        let i = i % self.len();
        (self.points[i], self.dirs[i], self.curvature[i])
    }

    /// Where `p` is on the track, searching near sample `hint` first.
    pub fn locate(&self, p: Vec2, hint: Option<usize>) -> Spot {
        let n = self.len();
        let best = |range: &mut dyn Iterator<Item = usize>| {
            range
                .map(|i| {
                    let (a, b) = (self.points[i], self.points[(i + 1) % n]);
                    let ab = b - a;
                    let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
                    (i, t, p.distance_squared(a + ab * t))
                })
                .min_by(|x, y| x.2.total_cmp(&y.2))
                .unwrap()
        };
        let mut found = match hint {
            Some(h) => best(&mut (0..25).map(|k| (h + n + k - 12) % n)),
            None => best(&mut (0..n)),
        };
        // Lost (shoved a long way): look everywhere.
        if hint.is_some() && found.2 > (self.width * 1.5).powi(2) {
            found = best(&mut (0..n));
        }
        let (i, t, _) = found;
        let (a, b) = (self.points[i], self.points[(i + 1) % n]);
        let on = a.lerp(b, t);
        let lateral = (b - a).normalize().perp().dot(p - on);
        Spot { index: i, s: (i as f32 + t) * self.step(), lateral }
    }

    /// Signed distance travelled going from `from` to `to` along the track,
    /// taking the short way round.
    pub fn delta(&self, from: f32, to: f32) -> f32 {
        let d = (to - from).rem_euclid(self.length);
        if d > self.length / 2.0 { d - self.length } else { d }
    }

    /// Distance from `p` to the centreline (brute force; for layout only).
    pub fn distance(&self, p: Vec2) -> f32 {
        self.locate(p, None).lateral.abs()
    }

    /// Where along the track (in distance) a lap fraction is.
    pub fn at(&self, fraction: f32) -> f32 {
        fraction * self.length
    }

    /// Distance along the track of every jump's ravine.
    pub fn jumps(&self) -> Vec<f32> {
        self.features
            .iter()
            .filter_map(|f| match *f {
                Feature::Jump { at } => Some(self.at(at)),
                _ => None,
            })
            .collect()
    }

    /// Ramp lips: distance along the track where cars take off.
    pub fn ramps(&self) -> Vec<f32> {
        self.features
            .iter()
            .filter_map(|f| match *f {
                Feature::Jump { at } => Some(self.at(at) - RAVINE_GAP / 2.0),
                Feature::Ramp { at } => Some(self.at(at)),
                _ => None,
            })
            .collect()
    }

    /// Boost pads: (centre, direction).
    pub fn boosts(&self) -> Vec<(Vec2, Vec2)> {
        self.features
            .iter()
            .filter_map(|f| match *f {
                Feature::Boost { at, lateral } => {
                    let s = self.at(at);
                    Some((self.point_at(s) + self.dir_at(s).perp() * lateral, self.dir_at(s)))
                }
                _ => None,
            })
            .collect()
    }

    /// The river's course: a long line through both crossings.
    fn river(&self) -> Option<(Vec2, Vec2, f32, f32)> {
        self.features.iter().find_map(|f| match *f {
            Feature::River { bridge, ford } => {
                let (a, b) = (self.point_at(self.at(bridge)), self.point_at(self.at(ford)));
                let dir = (b - a).normalize();
                Some((a - dir * 3000.0, b + dir * 3000.0, self.at(bridge), self.at(ford)))
            }
            _ => None,
        })
    }

    /// The bridge and the ford: (kind, start, end) along the track.
    /// Each runs until the whole width of the road is clear of the river
    /// channel, however slanted the crossing is.
    pub fn crossings(&self) -> Vec<(DeckKind, f32, f32)> {
        let Some((a, b, bridge, ford)) = self.river() else { return Vec::new() };
        let channel = river_cut(a, b);
        let reach = self.width / 2.0 + BARRIER + 10.0;
        let clear = |s: f32| {
            let (p, left) = (self.point_at(s), self.dir_at(s).perp());
            [-1.0, -0.5, 0.0, 0.5, 1.0]
                .iter()
                .all(|&k| channel.distance(p + left * k * reach) > RIVER_HALF + RIVER_BANK + 15.0)
        };
        let end = |s: f32, step: f32| {
            let mut e = s;
            while !clear(e) && (e - s).abs() < 1500.0 {
                e += step;
            }
            e + step * 2.0
        };
        [(DeckKind::Bridge, bridge), (DeckKind::Ford, ford)]
            .into_iter()
            .map(|(kind, s)| (kind, end(s, -5.0), end(s, 5.0)))
            .collect()
    }

    /// Ravines and river channels carved into the ground.
    pub fn cuts(&self) -> Vec<Cut> {
        let mut cuts = Vec::new();
        for s in self.jumps() {
            let (p, across) = (self.point_at(s), self.dir_at(s).perp());
            let reach = self.width / 2.0 + 260.0;
            cuts.push(Cut {
                a: p - across * reach,
                b: p + across * reach,
                half_width: RAVINE_GAP / 2.0 - 25.0,
                bank: 25.0,
                depth: 170.0,
            });
        }
        if let Some((a, b, ..)) = self.river() {
            cuts.push(river_cut(a, b));
        }
        cuts
    }

    /// Ramps, the bridge and the ford, given the ground they sit on. Also
    /// sets the water level when there's a river.
    pub fn shape(&self, terrain: Terrain) -> Terrain {
        let mut decks = Vec::new();
        let mut water = None;
        // A deck running along the track from `s0` to `s1`.
        let along = |kind: DeckKind, s0: f32, s1: f32, half_width: f32, h0: f32, h1: f32, sag: f32| {
            let (a, b) = (self.point_at(s0), self.point_at(s1));
            let half = Vec2::new(a.distance(b) / 2.0, half_width);
            Deck { kind, center: (a + b) / 2.0, dir: (b - a).normalize(), half, h0, h1, sag }
        };
        for s in self.ramps() {
            let start = s - RAMP_LENGTH;
            let (h0, h1) = (terrain.ground(self.point_at(start)), terrain.ground(self.point_at(s)));
            decks.push(along(DeckKind::Ramp, start, s, self.width / 2.0, h0, h1 + RAMP_HEIGHT, 0.0));
        }
        if let Some((a, b, ..)) = self.river() {
            let half_width = self.width / 2.0 + BARRIER + 10.0;
            let ground = |s: f32| terrain.ground(self.point_at(s));
            let crossings = self.crossings();
            let (_, b0, b1) = crossings[0];
            let (_, f0, f1) = crossings[1];
            // Water sits a little below the lower river bank.
            let level = [b0, b1, f0, f1].map(ground).into_iter().fold(f32::MAX, f32::min) - 22.0;
            water = Some((level, river_cut(a, b)));
            // Bridge and ford follow the track's curve in short sections,
            // with one height profile over the whole crossing.
            let mut crossing = |kind: DeckKind, s0: f32, s1: f32, sag: f32| {
                let profile = Deck {
                    kind,
                    center: Vec2::ZERO,
                    dir: Vec2::X,
                    half: Vec2::ONE,
                    h0: ground(s0),
                    h1: ground(s1),
                    sag,
                };
                let n = ((s1 - s0) / (self.step() * 2.0)).ceil() as usize;
                for i in 0..n {
                    let (t0, t1) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
                    let (a, b) = (s0 + (s1 - s0) * t0, s0 + (s1 - s0) * t1);
                    let mut d = along(kind, a, b, half_width, profile.height_at(t0), profile.height_at(t1), 0.0);
                    d.half.x += 10.0; // overlap, so the outside of a bend has no holes
                    decks.push(d);
                }
            };
            crossing(DeckKind::Bridge, b0, b1, 0.0);
            // The ford dips just under the water in the middle.
            let (h0, h1) = (ground(f0), ground(f1));
            crossing(DeckKind::Ford, f0, f1, (h0 + h1) / 2.0 - (level - 7.0));
        }
        let mut terrain = terrain.with_decks(decks);
        terrain.water = water;
        terrain
    }

    /// Barrier boxes along both edges: (centre, rotation, size). There are
    /// none over a ravine: the gap has to be jumped.
    pub fn barriers(&self) -> Vec<(Vec2, f32, Vec2)> {
        let jumps = self.jumps();
        let n = self.len();
        let half = self.width / 2.0 + BARRIER / 2.0;
        let mut out = Vec::new();
        for side in [1.0, -1.0] {
            let edge = |i: usize| self.points[i % n] + self.dirs[i % n].perp() * side * half;
            for i in (0..n).step_by(2) {
                let (a, b) = (edge(i), edge(i + 2));
                let len = a.distance(b);
                if len < 1.0 {
                    continue;
                }
                let s = i as f32 * self.step();
                if jumps.iter().any(|&j| self.delta(j, s).abs() < RAVINE_GAP / 2.0 + 40.0) {
                    continue;
                }
                // A little longer than the gap so the corners overlap.
                out.push(((a + b) / 2.0, (b - a).to_angle(), Vec2::new(len + BARRIER * 0.6, BARRIER)));
            }
        }
        out
    }

    /// Starting grid: two columns, staggered, behind the start line.
    pub fn grid(&self, count: usize) -> Vec<(Vec2, f32)> {
        (0..count)
            .map(|i| {
                let s = -70.0 - i as f32 * 75.0;
                let side = if i % 2 == 0 { 1.0 } else { -1.0 };
                let dir = self.dir_at(s);
                (self.point_at(s) + dir.perp() * side * self.width * 0.22, dir.to_angle())
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// The tracks
// ---------------------------------------------------------------------------

pub static DUSTLINE: TrackDef = TrackDef {
    points: &[
        Vec2::new(0.0, -700.0),
        Vec2::new(900.0, -700.0),
        Vec2::new(1400.0, -450.0),
        Vec2::new(1520.0, 0.0),
        Vec2::new(1400.0, 450.0),
        Vec2::new(900.0, 700.0),
        Vec2::new(420.0, 660.0),
        Vec2::new(0.0, 480.0),
        Vec2::new(-420.0, 660.0),
        Vec2::new(-900.0, 700.0),
        Vec2::new(-1400.0, 450.0),
        Vec2::new(-1520.0, 0.0),
        Vec2::new(-1400.0, -450.0),
        Vec2::new(-900.0, -700.0),
    ],
    width: 280.0,
    laps: 4,
    asphalt: Color::srgb(0.24, 0.23, 0.23),
    kerbs: [Color::srgb(0.85, 0.15, 0.12), Color::srgb(0.92, 0.92, 0.9)],
    features: &[
        Feature::Boost { at: 0.4, lateral: 50.0 },
        Feature::Ramp { at: 0.6 },
        Feature::Boost { at: 0.925, lateral: -50.0 },
    ],
};

pub static RED_CANYON: TrackDef = TrackDef {
    points: &[
        Vec2::new(-300.0, -900.0),
        Vec2::new(800.0, -900.0),
        Vec2::new(1450.0, -650.0),
        Vec2::new(1620.0, -50.0),
        Vec2::new(1500.0, 600.0),
        Vec2::new(1000.0, 930.0),
        Vec2::new(500.0, 780.0),
        Vec2::new(360.0, 320.0),
        Vec2::new(240.0, -50.0),
        Vec2::new(0.0, -170.0),
        Vec2::new(-240.0, -50.0),
        Vec2::new(-360.0, 320.0),
        Vec2::new(-450.0, 780.0),
        Vec2::new(-950.0, 930.0),
        Vec2::new(-1500.0, 650.0),
        Vec2::new(-1620.0, 0.0),
        Vec2::new(-1400.0, -620.0),
        Vec2::new(-900.0, -880.0),
    ],
    width: 260.0,
    laps: 3,
    asphalt: Color::srgb(0.3, 0.26, 0.24),
    kerbs: [Color::srgb(0.9, 0.85, 0.2), Color::srgb(0.12, 0.12, 0.12)],
    features: &[
        Feature::Boost { at: 0.05, lateral: 0.0 },
        Feature::Boost { at: 0.25, lateral: -50.0 },
        Feature::Boost { at: 0.797, lateral: 0.0 },
        Feature::Jump { at: 0.825 },
    ],
};

pub static PINE_RIDGE: TrackDef = TrackDef {
    points: &[
        Vec2::new(-200.0, -750.0),
        Vec2::new(700.0, -800.0),
        Vec2::new(1300.0, -550.0),
        Vec2::new(1480.0, -20.0),
        Vec2::new(1180.0, 360.0),
        Vec2::new(700.0, 280.0),
        Vec2::new(330.0, 520.0),
        Vec2::new(-50.0, 800.0),
        Vec2::new(-650.0, 820.0),
        Vec2::new(-1250.0, 600.0),
        Vec2::new(-1480.0, 80.0),
        Vec2::new(-1250.0, -420.0),
        Vec2::new(-750.0, -700.0),
    ],
    width: 260.0,
    laps: 4,
    asphalt: Color::srgb(0.22, 0.23, 0.22),
    kerbs: [Color::srgb(0.85, 0.15, 0.12), Color::srgb(0.92, 0.92, 0.9)],
    features: &[
        Feature::River { bridge: 0.6, ford: 0.087 },
        Feature::Boost { at: 0.64, lateral: 40.0 },
        Feature::Boost { at: 0.95, lateral: -40.0 },
    ],
};

fn river_cut(a: Vec2, b: Vec2) -> Cut {
    Cut { a, b, half_width: RIVER_HALF, bank: RIVER_BANK, depth: RIVER_DEPTH }
}

/// Gentle hills everywhere, with the ground rising beyond the track's box.
pub fn rolling(p: Vec2, size: Vec2, amp: f32, phase: f32) -> f32 {
    amp * (p.x / 520.0 + phase).sin() * (p.y / 430.0 - phase * 0.7).cos()
        + amp * 0.5 * ((p.x + 0.8 * p.y) / 610.0 + phase * 2.0).sin()
        + crate::terrain::berm(p, size, 70.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracks() -> [(&'static str, &'static TrackDef); 3] {
        [("Dustline", &DUSTLINE), ("Red Canyon", &RED_CANYON), ("Pine Ridge", &PINE_RIDGE)]
    }

    #[test]
    fn tracks_never_come_close_to_themselves() {
        for (name, def) in tracks() {
            let t = Track::new(def);
            let n = t.len();
            // Parts of the track that are far apart along it must be far
            // apart on the ground too, or the barriers would overlap.
            let along = (def.width * 3.0 / t.step()) as usize;
            let need = def.width + BARRIER * 2.0 + 60.0;
            for i in 0..n {
                for j in i + along..n {
                    if n - j + i < along {
                        continue;
                    }
                    let d = t.points[i].distance(t.points[j]);
                    assert!(d > need, "{name}: samples {i} and {j} only {d:.0} apart (need {need:.0})");
                }
            }
        }
    }

    #[test]
    fn corners_are_wider_than_the_track() {
        for (name, def) in tracks() {
            let t = Track::new(def);
            let tightest = t.curvature.iter().map(|c| c.abs()).fold(0.0, f32::max);
            let radius = 1.0 / tightest;
            assert!(radius > def.width / 2.0 + 60.0, "{name}: a corner has radius {radius:.0}");
        }
    }

    #[test]
    fn locate_and_progress_round_trip() {
        let t = Track::new(&DUSTLINE);
        for s in [0.0, 123.0, t.length * 0.5, t.length - 40.0] {
            let p = t.point_at(s) + t.dir_at(s).perp() * 50.0;
            let spot = t.locate(p, None);
            assert!(t.delta(s, spot.s).abs() < 2.0, "s {s} -> {}", spot.s);
            assert!((spot.lateral - 50.0).abs() < 3.0, "lateral {}", spot.lateral);
        }
        assert!((t.delta(t.length - 10.0, 10.0) - 20.0).abs() < 1e-3);
        assert!((t.delta(10.0, t.length - 10.0) + 20.0).abs() < 1e-3);
    }

    #[test]
    fn tracks_fit_their_arenas() {
        for def in crate::arena::ARENAS.iter().filter(|a| a.is_race()) {
            let t = Track::new(def.track().unwrap());
            let room = def.size / 2.0 - Vec2::splat(t.width / 2.0 + BARRIER + 150.0);
            for i in 0..t.len() {
                let p = t.points[i];
                assert!(p.x.abs() < room.x && p.y.abs() < room.y, "{}: track leaves the arena at {p}", def.name);
            }
        }
    }

    /// The ground as built for a race: relief, cuts and decks.
    fn race_terrain(def: &crate::arena::ArenaDef) -> (Track, Terrain) {
        let t = Track::new(def.track().unwrap());
        let terrain = t.shape(Terrain::new(def.relief, def.size).with_cuts(t.cuts()));
        (t, terrain)
    }

    #[test]
    fn features_sit_on_the_track() {
        for def in crate::arena::ARENAS.iter().filter(|a| a.is_race()) {
            let (t, _) = race_terrain(def);
            for (p, _) in t.boosts() {
                assert!(t.distance(p) + BOOST_PAD.y / 2.0 < t.width / 2.0, "{}: boost pad off the track", def.name);
            }
            // Jumps need a straight run-up: no tight bend over the ramp.
            for s in t.ramps() {
                let bend = (0..8).map(|k| t.curvature_at(s - k as f32 * 20.0)).fold(0.0, f32::max);
                assert!(bend < 1.0 / 600.0, "{}: ramp on a bend (radius {:.0})", def.name, 1.0 / bend);
            }
        }
    }

    #[test]
    fn ravines_are_deep_and_ramps_lift() {
        let def = &crate::arena::ARENAS[4];
        let (t, terrain) = race_terrain(def);
        let s = t.jumps()[0];
        let gap = terrain.height(t.point_at(s));
        let lip = terrain.height(t.point_at(s - RAVINE_GAP / 2.0 - 1.0));
        let run_up = terrain.height(t.point_at(s - RAVINE_GAP / 2.0 - RAMP_LENGTH - 5.0));
        assert!(lip - gap > 120.0, "ravine only {:.0} deep", lip - gap);
        assert!(lip - run_up > RAMP_HEIGHT * 0.7, "ramp only {:.0} high", lip - run_up);
    }

    #[test]
    fn bridge_stays_dry_and_ford_is_wet() {
        let def = &crate::arena::ARENAS[5];
        let (t, terrain) = race_terrain(def);
        let crossings = t.crossings();
        let mid = |k: usize| t.point_at((crossings[k].1 + crossings[k].2) / 2.0);
        assert_eq!(terrain.water_depth(mid(0)), 0.0, "bridge is under water");
        let ford = terrain.water_depth(mid(1));
        assert!(ford > 2.0 && ford < 15.0, "ford water is {ford:.1} deep");
        // Away from the river nothing is flooded.
        for k in 0..t.len() {
            let p = t.point_at(k as f32 * t.step());
            let near = crossings.iter().any(|&(_, a, b)| t.delta(a, k as f32 * t.step()) > -20.0 && t.delta(b, k as f32 * t.step()) < 20.0);
            if !near {
                assert_eq!(terrain.water_depth(p), 0.0, "track flooded at {p}");
            }
        }
    }

    #[test]
    fn grid_is_behind_the_line_and_on_the_track() {
        for (_, def) in tracks() {
            let t = Track::new(def);
            for (p, _) in t.grid(4) {
                let spot = t.locate(p, None);
                assert!(t.delta(0.0, spot.s) < -40.0);
                assert!(spot.lateral.abs() < def.width / 2.0 - 30.0);
            }
        }
    }
}
