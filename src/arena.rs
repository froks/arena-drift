//! Arenas: size, theme, obstacles and decorations, all data-driven.
//!
//! An arena is an [`ArenaDef`]: a size, colours, and a `build` function that
//! places obstacles and decorations into a [`Layout`]. The boundary walls are
//! ordinary box obstacles, so nothing assumes a rectangular play area: a race
//! track is a [`TrackDef`] whose edges become rows of barrier boxes.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;

use crate::RoundEntity;
use crate::physics::{Contact, Obb};
use crate::terrain::{Canvas, Cut, Deck, DeckKind, Terrain, berm, bump, smoothstep};
use crate::track::{self, BARRIER, BOOST_PAD, Track, TrackDef};

/// What an arena is used for: a walled derby arena, or a race track.
#[derive(Clone, Copy)]
pub enum Mode {
    Derby,
    Race(&'static TrackDef),
}

pub struct Theme {
    pub floor: Color,
    /// Colours for the random ground patches that break up the floor.
    pub patches: [Color; 2],
    /// Ground outside the walls.
    pub outfield: Color,
    pub wall: Color,
    /// Alternating stripe colour along the walls (hazard paint, fence posts...).
    pub wall_stripe: Option<Color>,
}

pub struct ArenaDef {
    pub name: &'static str,
    pub blurb: &'static str,
    pub mode: Mode,
    pub size: Vec2,
    pub theme: Theme,
    pub seed: u32,
    pub build: fn(&mut Layout),
    /// Ground height at a point, given the arena size. See [`crate::terrain`].
    pub relief: fn(Vec2, Vec2) -> f32,
}

impl ArenaDef {
    pub fn track(&self) -> Option<&'static TrackDef> {
        match self.mode {
            Mode::Race(t) => Some(t),
            Mode::Derby => None,
        }
    }
    pub fn is_race(&self) -> bool {
        self.track().is_some()
    }
}

/// Indices into [`ARENAS`] of the derby arenas (`race == false`) or the tracks.
pub fn arenas_for(race: bool) -> Vec<usize> {
    (0..ARENAS.len()).filter(|&i| ARENAS[i].is_race() == race).collect()
}

pub static ARENAS: [ArenaDef; 6] = [
    ArenaDef {
        name: "Dust Bowl",
        blurb: "Open dirt bowl. Tyre chicanes, concrete and a crowd that wants carnage.",
        mode: Mode::Derby,
        size: Vec2::new(2400.0, 1500.0),
        theme: Theme {
            floor: Color::srgb(0.42, 0.33, 0.22),
            patches: [Color::srgb(0.38, 0.29, 0.19), Color::srgb(0.47, 0.37, 0.25)],
            outfield: Color::srgb(0.2, 0.19, 0.17),
            wall: Color::srgb(0.75, 0.6, 0.15),
            wall_stripe: Some(Color::srgb(0.12, 0.12, 0.12)),
        },
        seed: 11,
        build: dust_bowl,
        relief: dust_bowl_relief,
    },
    ArenaDef {
        name: "Junkyard",
        blurb: "Lanes of shipping containers and rusting hulks. Lots of cover, lots of corners.",
        mode: Mode::Derby,
        size: Vec2::new(2200.0, 1800.0),
        theme: Theme {
            floor: Color::srgb(0.27, 0.25, 0.22),
            patches: [Color::srgb(0.23, 0.21, 0.19), Color::srgb(0.31, 0.28, 0.24)],
            outfield: Color::srgb(0.16, 0.15, 0.14),
            wall: Color::srgb(0.45, 0.47, 0.5),
            wall_stripe: Some(Color::srgb(0.38, 0.4, 0.43)),
        },
        seed: 23,
        build: junkyard,
        relief: junkyard_relief,
    },
    ArenaDef {
        name: "Timberland",
        blurb: "A forest clearing with a log fence. Trees, rocks and hay bales everywhere.",
        mode: Mode::Derby,
        size: Vec2::new(2600.0, 1700.0),
        theme: Theme {
            floor: Color::srgb(0.3, 0.42, 0.22),
            patches: [Color::srgb(0.42, 0.36, 0.24), Color::srgb(0.26, 0.37, 0.19)],
            outfield: Color::srgb(0.17, 0.27, 0.13),
            wall: Color::srgb(0.45, 0.3, 0.16),
            wall_stripe: Some(Color::srgb(0.33, 0.21, 0.1)),
        },
        seed: 37,
        build: timberland,
        relief: timberland_relief,
    },
    ArenaDef {
        name: "Dustline Speedway",
        blurb: "Fast sweepers round a dusty oval, a kink on the back straight and a ramp to fly off.",
        mode: Mode::Race(&track::DUSTLINE),
        size: Vec2::new(3900.0, 2300.0),
        theme: Theme {
            floor: Color::srgb(0.45, 0.36, 0.24),
            patches: [Color::srgb(0.4, 0.31, 0.2), Color::srgb(0.5, 0.4, 0.27)],
            outfield: Color::srgb(0.36, 0.28, 0.19),
            wall: Color::srgb(0.85, 0.85, 0.82),
            wall_stripe: Some(Color::srgb(0.8, 0.15, 0.12)),
        },
        seed: 51,
        build: dustline,
        relief: dustline_relief,
    },
    ArenaDef {
        name: "Red Canyon",
        blurb: "Long and technical: a hairpin into the canyon, and a ravine to jump. Hit the boost.",
        mode: Mode::Race(&track::RED_CANYON),
        size: Vec2::new(4000.0, 2600.0),
        theme: Theme {
            floor: Color::srgb(0.55, 0.32, 0.2),
            patches: [Color::srgb(0.5, 0.28, 0.17), Color::srgb(0.62, 0.38, 0.24)],
            outfield: Color::srgb(0.48, 0.27, 0.17),
            wall: Color::srgb(0.2, 0.2, 0.22),
            wall_stripe: Some(Color::srgb(0.9, 0.8, 0.2)),
        },
        seed: 67,
        build: red_canyon,
        relief: red_canyon_relief,
    },
    ArenaDef {
        name: "Pine Ridge",
        blurb: "Through the woods and over a river twice: once on a bridge, once through a ford.",
        mode: Mode::Race(&track::PINE_RIDGE),
        size: Vec2::new(3800.0, 2400.0),
        theme: Theme {
            floor: Color::srgb(0.28, 0.42, 0.2),
            patches: [Color::srgb(0.24, 0.37, 0.18), Color::srgb(0.33, 0.46, 0.22)],
            outfield: Color::srgb(0.2, 0.32, 0.15),
            wall: Color::srgb(0.85, 0.85, 0.82),
            wall_stripe: Some(Color::srgb(0.15, 0.4, 0.75)),
        },
        seed: 83,
        build: pine_ridge,
        relief: pine_ridge_relief,
    },
];

pub const WALL_THICKNESS: f32 = 24.0;

// ---------------------------------------------------------------------------
// Obstacles
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObstacleKind {
    Wall,
    /// Track-side barrier: forgiving, so scraping along it doesn't wreck you.
    Barrier,
    TireStack,
    Concrete,
    Barrel,
    Container,
    Tree,
    Rock,
    Log,
    HayBale,
    Hulk,
}

impl ObstacleKind {
    /// Bounciness when a car hits it.
    pub fn restitution(self) -> f32 {
        match self {
            ObstacleKind::TireStack => 0.85,
            ObstacleKind::HayBale | ObstacleKind::Barrier => 0.2,
            _ => 0.35,
        }
    }
    /// Multiplier on crash damage: tyres cushion, concrete and rock hurt.
    pub fn damage(self) -> f32 {
        match self {
            ObstacleKind::TireStack | ObstacleKind::HayBale => 0.25,
            ObstacleKind::Barrier => 0.2,
            ObstacleKind::Concrete | ObstacleKind::Rock | ObstacleKind::Tree => 1.6,
            _ => 1.0,
        }
    }
    pub fn minimap_color(self) -> Color {
        match self {
            ObstacleKind::Barrel => Color::srgb(0.85, 0.2, 0.1),
            ObstacleKind::Tree => Color::srgb(0.15, 0.35, 0.12),
            ObstacleKind::Barrier => Color::srgb(0.75, 0.75, 0.75),
            _ => Color::srgb(0.55, 0.55, 0.55),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Shape {
    Circle(f32),
    /// Full size (length along the rotation, width across).
    Box(Vec2),
}

#[derive(Component, Clone, Debug)]
pub struct Obstacle {
    pub kind: ObstacleKind,
    pub shape: Shape,
    pub pos: Vec2,
    pub rot: f32,
    /// Paint for kinds that come in colours (containers, hulks, ...).
    pub tint: Color,
}

impl Obstacle {
    fn obb(&self, size: Vec2) -> Obb {
        Obb::new(self.pos, self.rot, size)
    }

    pub fn bounding_radius(&self) -> f32 {
        match self.shape {
            Shape::Circle(r) => r,
            Shape::Box(s) => s.length() / 2.0,
        }
    }

    /// Axis-aligned footprint for the minimap.
    pub fn map_size(&self) -> Vec2 {
        match self.shape {
            Shape::Circle(r) => Vec2::splat(r * 2.0),
            Shape::Box(s) => {
                let (sin, cos) = self.rot.sin_cos();
                Vec2::new(s.x * cos.abs() + s.y * sin.abs(), s.x * sin.abs() + s.y * cos.abs()) * 0.8
            }
        }
    }

    /// Distance from a point to the obstacle's surface (0 inside).
    pub fn distance(&self, p: Vec2) -> f32 {
        match self.shape {
            Shape::Circle(r) => (p.distance(self.pos) - r).max(0.0),
            Shape::Box(s) => self.obb(s).distance(p),
        }
    }

    /// Car box vs obstacle. The normal points from the car into the obstacle.
    pub fn collide_car(&self, car: &Obb) -> Option<Contact> {
        match self.shape {
            Shape::Circle(r) => car.collide_circle(self.pos, r),
            Shape::Box(s) => car.collide(&self.obb(s)),
        }
    }
}

/// Barrels: blow up when their hp runs out.
#[derive(Component)]
pub struct Explosive {
    pub hp: f32,
    /// Whether the player did the last damage (for scoring the blast).
    pub by_player: bool,
}

pub const BARREL_HP: f32 = 10.0;

// ---------------------------------------------------------------------------
// Decorations (no collision)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecorKind {
    Patch,
    OilStain,
    PaintedRing,
    PaintedLine,
    Bush,
    Tree,
    Stand,
    Floodlight,
    ScrapPile,
    GrassTuft,
}

#[derive(Clone, Debug)]
pub struct Decor {
    pub kind: DecorKind,
    pub pos: Vec2,
    pub rot: f32,
    pub size: Vec2,
    pub tint: Color,
}

// ---------------------------------------------------------------------------
// Layout building
// ---------------------------------------------------------------------------

pub struct Layout {
    pub size: Vec2,
    pub obstacles: Vec<Obstacle>,
    pub decor: Vec<Decor>,
    pub track: Option<Track>,
    /// Ravines and river channels: nothing gets placed in them.
    pub cuts: Vec<Cut>,
    rng: u32,
}

impl Layout {
    pub fn new(def: &ArenaDef) -> Self {
        let track = def.track().map(Track::new);
        let cuts = track.as_ref().map_or(Vec::new(), Track::cuts);
        let mut layout =
            Self { size: def.size, obstacles: Vec::new(), decor: Vec::new(), track, cuts, rng: def.seed.max(1) };
        match &layout.track {
            Some(t) => {
                let theme = &def.theme;
                for (i, (pos, rot, size)) in t.barriers().into_iter().enumerate() {
                    let tint = match theme.wall_stripe {
                        Some(stripe) if i % 2 == 1 => stripe,
                        _ => theme.wall,
                    };
                    layout.obstacles.push(Obstacle { kind: ObstacleKind::Barrier, shape: Shape::Box(size), pos, rot, tint });
                }
            }
            None => layout.walls(),
        }
        layout.ground_patches(def.theme.patches);
        (def.build)(&mut layout);
        layout
    }

    pub fn half(&self) -> Vec2 {
        self.size / 2.0
    }

    pub fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng as f32 / u32::MAX as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.rand()
    }

    fn walls(&mut self) {
        let h = self.half() + Vec2::splat(WALL_THICKNESS / 2.0);
        let (w, t) = (self.size.x + WALL_THICKNESS * 2.0, WALL_THICKNESS);
        for (pos, size) in [
            (Vec2::new(0.0, h.y), Vec2::new(w, t)),
            (Vec2::new(0.0, -h.y), Vec2::new(w, t)),
            (Vec2::new(h.x, 0.0), Vec2::new(t, self.size.y)),
            (Vec2::new(-h.x, 0.0), Vec2::new(t, self.size.y)),
        ] {
            self.obstacles.push(Obstacle { kind: ObstacleKind::Wall, shape: Shape::Box(size), pos, rot: 0.0, tint: Color::WHITE });
        }
    }

    fn ground_patches(&mut self, colors: [Color; 2]) {
        let half = self.half();
        let count = (self.size.x * self.size.y / 90_000.0) as usize;
        for i in 0..count {
            let pos = Vec2::new(self.range(-half.x, half.x), self.range(-half.y, half.y));
            let size = Vec2::new(self.range(80.0, 320.0), self.range(50.0, 180.0));
            let rot = self.range(0.0, TAU);
            self.decor(DecorKind::Patch, pos, rot, size, colors[i % 2]);
        }
    }

    pub fn obstacle(&mut self, kind: ObstacleKind, pos: Vec2, rot: f32, shape: Shape, tint: Color) {
        self.obstacles.push(Obstacle { kind, shape, pos, rot, tint });
    }

    pub fn decor(&mut self, kind: DecorKind, pos: Vec2, rot: f32, size: Vec2, tint: Color) {
        self.decor.push(Decor { kind, pos, rot, size, tint });
    }

    /// Is there room for something of radius `r` at `pos`? On a track that
    /// means off the track, clear of the barriers.
    pub fn is_free(&self, pos: Vec2, r: f32, gap: f32) -> bool {
        let half = self.half() - Vec2::splat(r + 100.0);
        pos.x.abs() < half.x
            && pos.y.abs() < half.y
            && self.track.as_ref().is_none_or(|t| t.distance(pos) > t.width / 2.0 + BARRIER + r + gap)
            && self.cuts.iter().all(|c| c.distance(pos) > c.half_width + c.bank + r)
            && self
                .decor
                .iter()
                .filter(|d| d.kind == DecorKind::Stand)
                .all(|d| d.pos.distance(pos) > d.size.x / 2.0 + r + gap)
            && self
                .obstacles
                .iter()
                .filter(|o| !matches!(o.kind, ObstacleKind::Wall | ObstacleKind::Barrier))
                .all(|o| o.pos.distance(pos) > o.bounding_radius() + r + gap)
    }

    /// A random free spot inside the arena, if one turns up.
    pub fn free_spot(&mut self, r: f32, gap: f32) -> Option<Vec2> {
        let half = self.half();
        (0..60).find_map(|_| {
            let p = Vec2::new(self.range(-half.x, half.x), self.range(-half.y, half.y));
            self.is_free(p, r, gap).then_some(p)
        })
    }

    // Placement helpers ------------------------------------------------------

    pub fn tire_stack(&mut self, pos: Vec2) {
        self.obstacle(ObstacleKind::TireStack, pos, 0.0, Shape::Circle(20.0), Color::WHITE);
    }
    pub fn barrel(&mut self, pos: Vec2) {
        self.obstacle(ObstacleKind::Barrel, pos, 0.0, Shape::Circle(15.0), Color::WHITE);
    }
    pub fn concrete(&mut self, pos: Vec2, rot: f32) {
        self.obstacle(ObstacleKind::Concrete, pos, rot, Shape::Box(Vec2::new(96.0, 34.0)), Color::WHITE);
    }
    pub fn container(&mut self, pos: Vec2, rot: f32, tint: Color) {
        self.obstacle(ObstacleKind::Container, pos, rot, Shape::Box(Vec2::new(170.0, 64.0)), tint);
    }
    pub fn tree(&mut self, pos: Vec2, scale: f32) {
        // Only the trunk collides; the canopy hangs over the cars.
        self.obstacle(ObstacleKind::Tree, pos, 0.0, Shape::Circle(13.0 * scale), Color::WHITE);
        let rot = self.range(0.0, TAU);
        self.decor(DecorKind::Tree, pos, rot, Vec2::splat(52.0 * scale), Color::WHITE);
    }
    pub fn rock(&mut self, pos: Vec2, r: f32) {
        let rot = self.range(0.0, TAU);
        self.obstacle(ObstacleKind::Rock, pos, rot, Shape::Circle(r), Color::WHITE);
    }
    pub fn log(&mut self, pos: Vec2, rot: f32, len: f32) {
        self.obstacle(ObstacleKind::Log, pos, rot, Shape::Box(Vec2::new(len, 24.0)), Color::WHITE);
    }
    pub fn hay(&mut self, pos: Vec2, rot: f32) {
        self.obstacle(ObstacleKind::HayBale, pos, rot, Shape::Box(Vec2::new(54.0, 36.0)), Color::WHITE);
    }
    pub fn hulk(&mut self, pos: Vec2, rot: f32, tint: Color) {
        self.obstacle(ObstacleKind::Hulk, pos, rot, Shape::Box(Vec2::new(66.0, 34.0)), tint);
    }

    /// A small arc of tyre stacks.
    pub fn tire_arc(&mut self, center: Vec2, radius: f32, from: f32, to: f32, count: usize) {
        for i in 0..count {
            let a = from + (to - from) * i as f32 / (count - 1).max(1) as f32;
            self.tire_stack(center + Vec2::from_angle(a) * radius);
        }
    }

    /// Scatter trees, bushes etc. in the band outside the walls.
    pub fn outfield(&mut self, kind: DecorKind, count: usize, band: (f32, f32), size: (f32, f32), tint: Color) {
        let half = self.half();
        for _ in 0..count {
            // Pick a side, then a spot in the band beyond that wall.
            let side = (self.rand() * 4.0) as usize;
            let depth = self.range(band.0, band.1);
            let along = self.range(-1.0, 1.0);
            let pos = match side {
                0 => Vec2::new(along * (half.x + band.1), half.y + depth),
                1 => Vec2::new(along * (half.x + band.1), -half.y - depth),
                2 => Vec2::new(half.x + depth, along * (half.y + band.1)),
                _ => Vec2::new(-half.x - depth, along * (half.y + band.1)),
            };
            let s = self.range(size.0, size.1);
            let rot = self.range(0.0, TAU);
            self.decor(kind, pos, rot, Vec2::splat(s), tint);
        }
    }
}

// ---------------------------------------------------------------------------
// The arenas
// ---------------------------------------------------------------------------

/// A real bowl: a flat floor in the middle, rising to a rim at the walls.
fn dust_bowl_relief(p: Vec2, size: Vec2) -> f32 {
    let r = (p / (size / 2.0)).length();
    -30.0 + 100.0 * smoothstep(0.3, 1.0, r) + berm(p, size, 60.0)
}

/// Mostly level, with a drainage ditch snaking across and a few junk mounds.
fn junkyard_relief(p: Vec2, size: Vec2) -> f32 {
    let ditch = p.y - 160.0 * (p.x / 420.0).sin();
    let mounds = [
        (Vec2::new(-470.0, -430.0), 180.0, 55.0),
        (Vec2::new(470.0, 430.0), 180.0, 55.0),
        (Vec2::new(-900.0, 600.0), 220.0, 45.0),
        (Vec2::new(900.0, -600.0), 220.0, 45.0),
    ];
    -55.0 * (-(ditch * ditch) / (110.0 * 110.0)).exp()
        + mounds.iter().map(|&(at, r, h)| bump(p, at, r, h)).sum::<f32>()
        + berm(p, size, 50.0)
}

/// Rolling forest floor cut by a winding creek valley.
fn timberland_relief(p: Vec2, size: Vec2) -> f32 {
    let creek = p.y - (230.0 * (p.x / 520.0).sin() - 120.0);
    28.0 * (p.x / 330.0 + 0.7).sin() * (p.y / 280.0 - 0.4).cos()
        + 20.0 * ((p.x - 0.6 * p.y) / 450.0).sin()
        - 80.0 * (-(creek * creek) / (170.0 * 170.0)).exp()
        + berm(p, size, 70.0)
}

fn dust_bowl(l: &mut Layout) {
    let h = l.half();
    let white = Color::srgba(1.0, 1.0, 1.0, 0.22);
    l.decor(DecorKind::PaintedRing, Vec2::ZERO, 0.0, Vec2::splat(230.0), white);
    l.decor(DecorKind::PaintedLine, Vec2::ZERO, FRAC_PI_2, Vec2::new(h.y * 2.0, 8.0), white);

    // Concrete cross around the centre.
    for i in 0..4 {
        let a = i as f32 * FRAC_PI_2;
        l.concrete(Vec2::from_angle(a) * 330.0, a + FRAC_PI_2);
    }
    // Tyre chicanes in each quarter and arcs in the corners.
    for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        let c = Vec2::new(sx * 640.0, sy * 330.0);
        for k in -1..=1 {
            l.tire_stack(c + Vec2::new(k as f32 * 44.0, 0.0));
        }
        let corner = Vec2::new(sx * (h.x - 40.0), sy * (h.y - 40.0));
        let start = Vec2::new(-sx, -sy).to_angle();
        l.tire_arc(corner, 230.0, start - 0.6, start + 0.6, 6);
    }
    // Barrel clusters.
    for c in [Vec2::new(-1000.0, 0.0), Vec2::new(1000.0, 0.0), Vec2::new(0.0, 560.0), Vec2::new(0.0, -560.0)] {
        for k in 0..3 {
            l.barrel(c + Vec2::from_angle(k as f32 * TAU / 3.0) * 22.0);
        }
    }
    for _ in 0..14 {
        if let Some(p) = l.free_spot(30.0, 40.0) {
            let (rot, s) = (l.range(0.0, TAU), l.range(50.0, 110.0));
            l.decor(DecorKind::OilStain, p, rot, Vec2::new(s, s * 0.6), Color::WHITE);
        }
    }

    // Grandstands top and bottom, floodlights in the corners, trees beyond.
    for x in [-620.0, 620.0] {
        l.decor(DecorKind::Stand, Vec2::new(x, h.y + 140.0), 0.0, Vec2::new(760.0, 150.0), Color::WHITE);
        l.decor(DecorKind::Stand, Vec2::new(x, -h.y - 140.0), std::f32::consts::PI, Vec2::new(760.0, 150.0), Color::WHITE);
    }
    for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        l.decor(DecorKind::Floodlight, Vec2::new(sx * (h.x + 90.0), sy * (h.y + 90.0)), 0.0, Vec2::splat(1.0), Color::WHITE);
    }
    l.outfield(DecorKind::Tree, 40, (260.0, 650.0), (40.0, 70.0), Color::WHITE);
    l.outfield(DecorKind::Bush, 50, (60.0, 600.0), (12.0, 26.0), Color::WHITE);
}

fn junkyard(l: &mut Layout) {
    let h = l.half();
    let colors = [
        Color::srgb(0.65, 0.2, 0.15),
        Color::srgb(0.15, 0.4, 0.6),
        Color::srgb(0.25, 0.5, 0.3),
        Color::srgb(0.75, 0.55, 0.15),
    ];
    // Container lanes.
    let lanes = [
        (Vec2::new(-560.0, 420.0), 0.0),
        (Vec2::new(-380.0, 420.0), 0.0),
        (Vec2::new(560.0, -420.0), 0.0),
        (Vec2::new(380.0, -420.0), 0.0),
        (Vec2::new(-820.0, -250.0), FRAC_PI_2),
        (Vec2::new(820.0, 250.0), FRAC_PI_2),
        (Vec2::new(0.0, 700.0), 0.0),
        (Vec2::new(0.0, -700.0), 0.0),
        (Vec2::new(-60.0, 60.0), 0.35),
        (Vec2::new(90.0, -80.0), 0.35),
    ];
    for (i, (pos, rot)) in lanes.into_iter().enumerate() {
        l.container(pos, rot, colors[i % colors.len()]);
    }
    // Rusting hulks.
    let rust = [Color::srgb(0.45, 0.25, 0.15), Color::srgb(0.35, 0.33, 0.3), Color::srgb(0.3, 0.2, 0.25)];
    for i in 0..12 {
        if let Some(p) = l.free_spot(40.0, 60.0) {
            let rot = l.range(0.0, TAU);
            l.hulk(p, rot, rust[i % rust.len()]);
        }
    }
    // Tyre piles and barrels.
    for _ in 0..6 {
        if let Some(c) = l.free_spot(60.0, 50.0) {
            for k in 0..4 {
                l.tire_stack(c + Vec2::from_angle(k as f32 * TAU / 4.0 + 0.3) * 26.0);
            }
        }
    }
    for _ in 0..6 {
        if let Some(c) = l.free_spot(40.0, 50.0) {
            l.barrel(c);
            l.barrel(c + Vec2::new(30.0, 6.0));
        }
    }
    for _ in 0..20 {
        if let Some(p) = l.free_spot(30.0, 30.0) {
            let (rot, s) = (l.range(0.0, TAU), l.range(50.0, 120.0));
            l.decor(DecorKind::OilStain, p, rot, Vec2::new(s, s * 0.6), Color::WHITE);
        }
    }
    l.outfield(DecorKind::ScrapPile, 45, (80.0, 600.0), (60.0, 140.0), Color::WHITE);
    l.outfield(DecorKind::Tree, 14, (300.0, 650.0), (40.0, 60.0), Color::WHITE);
    let _ = h;
}

fn timberland(l: &mut Layout) {
    // Clusters of trees.
    for _ in 0..7 {
        if let Some(c) = l.free_spot(120.0, 80.0) {
            let n = 3 + (l.rand() * 3.0) as usize;
            for k in 0..n {
                let off = Vec2::from_angle(k as f32 * TAU / n as f32 + l.range(0.0, 0.8)) * l.range(40.0, 90.0);
                let s = l.range(0.9, 1.3);
                l.tree(c + off, s);
            }
        }
    }
    for _ in 0..10 {
        if let Some(p) = l.free_spot(40.0, 60.0) {
            let r = l.range(22.0, 42.0);
            l.rock(p, r);
        }
    }
    for _ in 0..8 {
        if let Some(p) = l.free_spot(70.0, 60.0) {
            let (rot, len) = (l.range(0.0, TAU), l.range(110.0, 170.0));
            l.log(p, rot, len);
        }
    }
    for _ in 0..6 {
        if let Some(c) = l.free_spot(50.0, 60.0) {
            let rot = l.range(0.0, TAU);
            l.hay(c, rot);
            l.hay(c + Vec2::from_angle(rot) * 58.0, rot);
        }
    }
    for _ in 0..4 {
        if let Some(c) = l.free_spot(30.0, 60.0) {
            l.barrel(c);
        }
    }
    for _ in 0..60 {
        if let Some(p) = l.free_spot(10.0, 10.0) {
            l.decor(DecorKind::GrassTuft, p, 0.0, Vec2::splat(1.0), Color::WHITE);
        }
    }
    l.outfield(DecorKind::Tree, 90, (90.0, 650.0), (40.0, 75.0), Color::WHITE);
    l.outfield(DecorKind::Bush, 40, (40.0, 400.0), (14.0, 28.0), Color::WHITE);
}

// ---------------------------------------------------------------------------
// The race tracks
// ---------------------------------------------------------------------------

fn dustline_relief(p: Vec2, size: Vec2) -> f32 {
    track::rolling(p, size, 18.0, 0.0)
}

/// Hills, and a canyon the hairpin drops down into.
fn red_canyon_relief(p: Vec2, size: Vec2) -> f32 {
    track::rolling(p, size, 30.0, 2.1) + bump(p, Vec2::new(0.0, 250.0), 520.0, -85.0)
}

fn pine_ridge_relief(p: Vec2, size: Vec2) -> f32 {
    track::rolling(p, size, 40.0, 1.3)
}

impl Layout {
    fn track(&self) -> &Track {
        self.track.as_ref().expect("race layouts have a track")
    }

    /// Grandstands along the start straight, on the outside of the track.
    fn grandstands(&mut self, from: f32, to: f32) {
        let t = self.track().clone();
        let mut s = from;
        while s < to {
            let (p, dir) = (t.point_at(s + 150.0), t.dir_at(s + 150.0));
            let out = if (p + dir.perp() * 10.0).length() > (p - dir.perp() * 10.0).length() { dir.perp() } else { -dir.perp() };
            let depth = 140.0;
            let at = p + out * (t.width / 2.0 + BARRIER + 40.0 + depth / 2.0);
            self.decor(DecorKind::Stand, at, out.to_angle() - FRAC_PI_2, Vec2::new(280.0, depth), Color::WHITE);
            s += 300.0;
        }
    }

    /// Something (tyre stacks, hay bales) along the outside of every sharp
    /// corner, just beyond the barrier.
    fn corner_guards(&mut self, mut place: impl FnMut(&mut Self, Vec2, f32)) {
        let t = self.track().clone();
        for i in (0..t.len()).step_by(2) {
            let (p, dir, bend) = t.sample(i);
            if bend.abs() < 1.0 / 420.0 {
                continue;
            }
            let out = -dir.perp() * bend.signum();
            place(self, p + out * (t.width / 2.0 + BARRIER + 40.0), dir.to_angle());
        }
    }

    /// Scatter decorations of radius `r` anywhere off the track.
    fn scatter(&mut self, count: usize, r: f32, gap: f32, mut place: impl FnMut(&mut Self, Vec2)) {
        for _ in 0..count {
            if let Some(p) = self.free_spot(r, gap) {
                place(self, p);
            }
        }
    }
}

fn dustline(l: &mut Layout) {
    l.grandstands(-450.0, 600.0);
    l.corner_guards(|l, p, _| l.tire_stack(p));
    l.scatter(30, 30.0, 20.0, |l, p| {
        let s = l.range(14.0, 26.0);
        l.decor(DecorKind::Bush, p, 0.0, Vec2::splat(s), Color::WHITE);
    });
    l.scatter(12, 50.0, 40.0, |l, p| {
        let s = l.range(40.0, 60.0);
        l.decor(DecorKind::Tree, p, 0.0, Vec2::splat(s), Color::WHITE);
    });
    let h = l.half();
    for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        l.decor(DecorKind::Floodlight, Vec2::new(sx * (h.x - 60.0), sy * (h.y - 60.0)), 0.0, Vec2::splat(1.0), Color::WHITE);
    }
}

fn red_canyon(l: &mut Layout) {
    l.grandstands(-300.0, 300.0);
    l.corner_guards(|l, p, _| l.tire_stack(p));
    l.scatter(45, 45.0, 30.0, |l, p| {
        let r = l.range(25.0, 60.0);
        l.rock(p, r);
    });
    l.scatter(25, 30.0, 20.0, |l, p| {
        let s = l.range(12.0, 22.0);
        l.decor(DecorKind::Bush, p, 0.0, Vec2::splat(s), Color::WHITE);
    });
    l.scatter(12, 80.0, 30.0, |l, p| {
        let s = l.range(70.0, 130.0);
        l.decor(DecorKind::ScrapPile, p, 0.0, Vec2::splat(s), Color::WHITE);
    });
}

fn pine_ridge(l: &mut Layout) {
    l.grandstands(-300.0, 300.0);
    l.corner_guards(|l, p, rot| l.hay(p, rot));
    l.scatter(140, 55.0, 15.0, |l, p| {
        let s = l.range(40.0, 70.0);
        l.decor(DecorKind::Tree, p, 0.0, Vec2::splat(s), Color::WHITE);
    });
    l.scatter(14, 40.0, 30.0, |l, p| {
        let r = l.range(20.0, 40.0);
        l.rock(p, r);
    });
    l.scatter(60, 20.0, 10.0, |l, p| {
        let s = l.range(14.0, 26.0);
        l.decor(DecorKind::Bush, p, 0.0, Vec2::splat(s), Color::WHITE);
    });
    l.scatter(80, 10.0, 10.0, |l, p| l.decor(DecorKind::GrassTuft, p, 0.0, Vec2::splat(1.0), Color::WHITE));
}

// ---------------------------------------------------------------------------
// Starting grid
// ---------------------------------------------------------------------------

/// How much open road a car gets in front of it at the start.
pub const SPAWN_RUNWAY: f32 = 300.0;

/// Is this a fair place to start: not on top of anything, not next to a
/// barrel, and with open road towards the centre?
fn fair_spawn(obstacles: &[Obstacle], pos: Vec2, heading: f32) -> bool {
    let dir = Vec2::from_angle(heading);
    obstacles.iter().all(|o| {
        let keep_away = if o.kind == ObstacleKind::Barrel { 140.0 } else { 70.0 };
        o.distance(pos) > keep_away
            && (1..=(SPAWN_RUNWAY / 20.0) as i32).all(|k| o.distance(pos + dir * (k as f32 * 20.0)) > 30.0)
    })
}

/// Starting positions (and headings, facing the centre) for `count` cars,
/// spread around a ring. Each spot slides around the ring and inwards until
/// it's fair; the bool says whether every car got a fair spot.
pub fn spawn_points(obstacles: &[Obstacle], size: Vec2, count: usize) -> (Vec<(Vec2, f32)>, bool) {
    let mut points: Vec<(Vec2, f32)> = Vec::new();
    let mut all_fair = true;
    for i in 0..count {
        let base = i as f32 / count as f32 * TAU + FRAC_PI_2;
        let candidates = [1.0_f32, 0.9, 0.8, 0.7].into_iter().flat_map(|radius| {
            (0..11).map(move |k| {
                // 0, +0.1, -0.1, +0.2, -0.2, ... radians around the ring.
                let off = ((k + 1) / 2) as f32 * 0.1 * if k % 2 == 1 { 1.0 } else { -1.0 };
                let a = base + off;
                Vec2::new(a.cos() * size.x * 0.38, a.sin() * size.y * 0.36) * radius
            })
        });
        let spaced = |p: Vec2, points: &[(Vec2, f32)]| points.iter().all(|(q, _)| q.distance(p) > 160.0);
        let fair = candidates
            .clone()
            .find(|&p| spaced(p, &points) && fair_spawn(obstacles, p, (-p).to_angle()));
        let pos = fair.unwrap_or_else(|| {
            all_fair = false;
            candidates.clone().next().unwrap()
        });
        points.push((pos, (-pos).to_angle()));
    }
    (points, all_fair)
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// How far the boundary walls stand above the ground.
const WALL_HEIGHT: f32 = 40.0;

#[derive(Clone, Copy)]
enum M {
    Cube,
    /// Cylinder standing on its end (axis along Z).
    Cyl,
    Ball,
    /// Faceted lump.
    Rock,
    /// Ring lying flat (axis along Z).
    Torus,
}

/// Shared unit meshes and a material cache keyed by colour.
#[derive(Resource)]
pub struct ArenaArt {
    cube: Handle<Mesh>,
    cyl: Handle<Mesh>,
    ball: Handle<Mesh>,
    rock: Handle<Mesh>,
    torus: Handle<Mesh>,
    mats: HashMap<(u32, bool), Handle<StandardMaterial>>,
}

impl FromWorld for ArenaArt {
    fn from_world(world: &mut World) -> Self {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let upright = Quat::from_rotation_x(FRAC_PI_2);
        let mut rock: Mesh = Sphere::new(1.0).mesh().ico(1).expect("valid icosphere");
        rock.duplicate_vertices();
        rock.compute_flat_normals();
        Self {
            cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
            cyl: meshes.add(Mesh::from(Cylinder::new(1.0, 1.0)).rotated_by(upright)),
            ball: meshes.add(Sphere::new(1.0).mesh().ico(3).expect("valid icosphere")),
            rock: meshes.add(rock),
            torus: meshes.add(Mesh::from(Torus::new(0.3, 1.0)).rotated_by(upright)),
            mats: HashMap::new(),
        }
    }
}

/// A plain matte material. `unlit` ones glow at their own colour.
pub fn surface(color: Color, unlit: bool) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.9,
        reflectance: 0.2,
        unlit,
        alpha_mode: if color.alpha() < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
        ..default()
    }
}

/// Tree tops inside the arena fade out while the player drives underneath.
#[derive(Component)]
pub struct Canopy {
    radius: f32,
    alpha: f32,
    materials: Vec<Handle<StandardMaterial>>,
}

struct Painter<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    art: &'a mut ArenaArt,
    materials: &'a mut Assets<StandardMaterial>,
    terrain: Terrain,
    /// Whether trees inside the arena fade out under the player.
    fade_trees: bool,
}

impl Painter<'_, '_, '_> {
    fn material(&mut self, c: Color, unlit: bool) -> Handle<StandardMaterial> {
        let key = (u32::from_le_bytes(c.to_srgba().to_u8_array()), unlit);
        let materials = &mut *self.materials;
        self.art.mats.entry(key).or_insert_with(|| materials.add(surface(c, unlit))).clone()
    }

    /// Something standing on the ground at `pos`, sunk to the lowest point
    /// under a footprint of radius `footprint` so it never floats on a slope.
    fn root(&mut self, pos: Vec2, rot: f32, footprint: f32) -> Entity {
        let z = self.terrain.lowest(pos, footprint);
        self.root_at(pos.extend(z), Quat::from_rotation_z(rot))
    }

    fn root_at(&mut self, at: Vec3, rotation: Quat) -> Entity {
        self.commands
            .spawn((RoundEntity, Transform::from_translation(at).with_rotation(rotation), Visibility::default()))
            .id()
    }

    fn add(&mut self, root: Entity, m: M, material: Handle<StandardMaterial>, at: Vec3, rot: Quat, scale: Vec3) {
        let mesh = match m {
            M::Cube => self.art.cube.clone(),
            M::Cyl => self.art.cyl.clone(),
            M::Ball => self.art.ball.clone(),
            M::Rock => self.art.rock.clone(),
            M::Torus => self.art.torus.clone(),
        };
        let child = self
            .commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(at).with_rotation(rot).with_scale(scale),
            ))
            .id();
        self.commands.entity(root).add_child(child);
    }

    fn part(&mut self, root: Entity, m: M, color: Color, at: Vec3, scale: Vec3) {
        let material = self.material(color, false);
        self.add(root, m, material, at, Quat::IDENTITY, scale);
    }

    /// A box whose bottom sits at `base`.
    fn block(&mut self, root: Entity, color: Color, at: Vec2, base: f32, size: Vec3) {
        self.part(root, M::Cube, color, at.extend(base + size.z / 2.0), size);
    }

    /// An upright cylinder of radius `r` whose bottom sits at `base`.
    fn post(&mut self, root: Entity, color: Color, at: Vec2, base: f32, r: f32, height: f32) {
        self.part(root, M::Cyl, color, at.extend(base + height / 2.0), Vec3::new(r, r, height));
    }
}

/// Spawn everything for an arena. Returns the obstacles (so callers can
/// pick spawn points before the entities exist) and the ground relief.
pub fn spawn_arena(
    commands: &mut Commands,
    art: &mut ArenaArt,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    def: &ArenaDef,
) -> (Vec<Obstacle>, Terrain, Option<Track>) {
    let layout = Layout::new(def);
    let mut terrain = Terrain::new(def.relief, def.size).with_cuts(layout.cuts.clone());
    if let Some(t) = &layout.track {
        terrain = t.shape(terrain);
    }
    let mut canvas = Canvas::new(def.size, def.theme.outfield);
    canvas.rect(Vec2::ZERO, 0.0, def.size, def.theme.floor);
    let fade_trees = !def.is_race();
    let mut p = Painter { commands, art, materials, terrain: terrain.clone(), fade_trees };

    for d in &layout.decor {
        draw_decor(&mut p, &mut canvas, d);
    }
    if let (Some(t), Some(td)) = (&layout.track, def.track()) {
        if let Some((_, river)) = terrain.water {
            let (mid, along) = ((river.a + river.b) / 2.0, river.b - river.a);
            let wide = 2.0 * (river.half_width + river.bank);
            canvas.rect(mid, along.to_angle(), Vec2::new(along.length(), wide), Color::srgb(0.3, 0.27, 0.2));
        }
        paint_track(&mut canvas, t, td);
        for cut in layout.cuts.iter().filter(|c| c.depth > 150.0) {
            // Ravine: dark rock all the way down.
            let along = cut.b - cut.a;
            let wide = 2.0 * (cut.half_width + cut.bank) + 10.0;
            canvas.rect((cut.a + cut.b) / 2.0, along.to_angle(), Vec2::new(along.length(), wide), Color::srgb(0.22, 0.16, 0.13));
        }
        for (pos, dir) in t.boosts() {
            draw_boost(&mut p, &mut canvas, pos, dir);
        }
        for deck in terrain.decks().iter().filter(|d| d.kind == DeckKind::Ramp) {
            draw_ramp(&mut p, deck);
        }
        for (kind, s0, s1) in t.crossings() {
            draw_crossing(&mut p, meshes, images, t, td, kind, s0, s1);
        }
        if let Some((level, river)) = terrain.water {
            let water = p.materials.add(StandardMaterial {
                base_color: Color::srgb(0.16, 0.38, 0.5),
                alpha_mode: AlphaMode::Blend,
                // Drawn after the ford's fading gravel, so the gravel shows through.
                depth_bias: 20.0,
                perceptual_roughness: 0.12,
                reflectance: 0.6,
                ..default()
            });
            p.commands.spawn((
                RoundEntity,
                Mesh3d(meshes.add(water_mesh(&terrain, level, &river, def.size))),
                MeshMaterial3d(water),
                Transform::from_translation(((river.a + river.b) / 2.0).extend(level)),
                bevy::light::NotShadowCaster,
            ));
        }
        let walls = p.materials.add(StandardMaterial {
            perceptual_roughness: 0.9,
            reflectance: 0.2,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        p.commands.spawn((
            RoundEntity,
            Mesh3d(meshes.add(barrier_mesh(t, &terrain, &def.theme))),
            MeshMaterial3d(walls),
            Transform::default(),
        ));
        draw_gantry(&mut p, t);
    }
    for o in &layout.obstacles {
        let root = draw_obstacle(&mut p, o, &def.theme);
        p.commands.entity(root).insert(o.clone());
        if o.kind != ObstacleKind::Wall {
            p.commands.entity(root).insert(crate::minimap::Icon { color: o.kind.minimap_color(), size: o.map_size(), layer: 1 });
        }
        if o.kind == ObstacleKind::Barrel {
            p.commands.entity(root).insert(Explosive { hp: BARREL_HP, by_player: false });
        }
    }

    let (mesh, material) = crate::terrain::build_ground(&terrain, canvas, meshes, images, p.materials);
    p.commands.spawn((RoundEntity, mesh, material, Transform::default()));
    (layout.obstacles, terrain, layout.track)
}

/// The river's surface: a grid over the channel whose transparency follows
/// the depth, so the shallows (and the ford) show the bottom through it.
fn water_mesh(terrain: &Terrain, level: f32, river: &Cut, size: Vec2) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let dir = (river.b - river.a).normalize();
    let left = dir.perp();
    // Only the stretch that's on the map.
    let reach = size / 2.0 + Vec2::splat(crate::terrain::MARGIN);
    let inside = |p: Vec2| p.x.abs() < reach.x && p.y.abs() < reach.y;
    let total = river.a.distance(river.b);
    let along: Vec<f32> = (0..=(total / 20.0) as usize).map(|i| i as f32 * 20.0).filter(|&d| inside(river.a + dir * d)).collect();
    let half = river.half_width + river.bank;
    let across = 16;
    let origin = (river.a + river.b) / 2.0;
    let (mut pos, mut colors, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for &d in &along {
        for j in 0..=across {
            let p = river.a + dir * d + left * (j as f32 / across as f32 * 2.0 - 1.0) * half;
            let depth = level - terrain.height(p);
            // Clear in the shallows, deep blue in the channel.
            let alpha = if depth <= 0.0 { 0.0 } else { (0.25 + depth / 50.0).min(0.88) };
            pos.push([p.x - origin.x, p.y - origin.y, 0.0]);
            colors.push([1.0, 1.0, 1.0, alpha]);
        }
    }
    let row = across as u32 + 1;
    for i in 0..along.len().saturating_sub(1) as u32 {
        for j in 0..across as u32 {
            let a = i * row + j;
            idx.extend([a, a + row, a + row + 1, a, a + row + 1, a + 1]);
        }
    }
    let n = pos.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(idx))
}

/// Asphalt, kerbs, the chequered start line and the grid boxes.
fn paint_track(canvas: &mut Canvas, t: &Track, def: &TrackDef) {
    let half = t.width / 2.0;
    for i in 0..t.len() {
        let (p, _, _) = t.sample(i);
        canvas.ellipse(p, 0.0, Vec2::splat(half), def.asphalt);
    }
    for i in 0..t.len() {
        let (p, dir, _) = t.sample(i);
        let color = def.kerbs[(i / 2) % 2];
        for side in [-1.0, 1.0] {
            canvas.rect(p + dir.perp() * side * (half - 7.0), dir.to_angle(), Vec2::new(t.step() + 1.0, 14.0), color);
        }
    }
    // Chequered line across the track at s = 0.
    let (p, dir) = (t.point_at(0.0), t.dir_at(0.0));
    let square = 16.0;
    let n = (t.width / square).floor() as i32;
    for row in 0..2 {
        for k in 0..n {
            let across = (k as f32 + 0.5 - n as f32 / 2.0) * square;
            let along = (row as f32 - 0.5) * square;
            let color = if (k + row) % 2 == 0 { Color::srgb(0.95, 0.95, 0.95) } else { Color::srgb(0.08, 0.08, 0.08) };
            canvas.rect(p + dir * along + dir.perp() * across, dir.to_angle(), Vec2::splat(square), color);
        }
    }
    // Grid boxes: a bracket in front of each starting slot.
    let white = Color::srgba(1.0, 1.0, 1.0, 0.8);
    for (at, heading) in t.grid(crate::RACE_CARS) {
        let dir = Vec2::from_angle(heading);
        canvas.rect(at + dir * 32.0, heading, Vec2::new(4.0, 48.0), white);
        for side in [-1.0, 1.0] {
            canvas.rect(at + dir * 14.0 + dir.perp() * side * 24.0, heading, Vec2::new(36.0, 4.0), white);
        }
    }
}

/// A boost pad: a dark plate with glowing chevrons pointing the way.
fn draw_boost(p: &mut Painter, canvas: &mut Canvas, pos: Vec2, dir: Vec2) {
    canvas.rect(pos, dir.to_angle(), BOOST_PAD + Vec2::splat(8.0), Color::srgb(0.95, 0.75, 0.1));
    canvas.rect(pos, dir.to_angle(), BOOST_PAD, Color::srgb(0.1, 0.12, 0.16));
    let root = p.root_at(pos.extend(p.terrain.height(pos)), Quat::from_rotation_z(dir.to_angle()));
    let glow = p.material(Color::srgb(0.25, 0.85, 1.0), true);
    // Three ">" chevrons, centred on the pad: each arm is 24 long at 45°,
    // so it reaches 8.5 back and 8.5 out from the chevron's tip.
    for k in 0..3 {
        let tip = -9.0 + 18.0 * k as f32;
        for side in [-1.0, 1.0] {
            let at = Vec3::new(tip - 8.5, side * 8.5, 1.2);
            let rot = Quat::from_rotation_z(-side * std::f32::consts::FRAC_PI_4);
            p.add(root, M::Cube, glow.clone(), at, rot, Vec3::new(24.0, 6.0, 1.5));
        }
    }
}

/// A ramp: a sloping slab on a solid base, with hazard stripes on the lip.
fn draw_ramp(p: &mut Painter, deck: &Deck) {
    let root = p.root_at(deck.center.extend(0.0), Quat::from_rotation_z(deck.dir.to_angle()));
    let (top, side) = (Color::srgb(0.55, 0.42, 0.28), Color::srgb(0.4, 0.33, 0.25));
    let top = p.material(top, false);
    let n = ((deck.half.x * 2.0 / 20.0).ceil() as usize).max(1);
    let seg = deck.half.x * 2.0 / n as f32;
    let thick = 4.0;
    for i in 0..n {
        let (z0, z1) = (deck.height_at(i as f32 / n as f32), deck.height_at((i + 1) as f32 / n as f32));
        let x = -deck.half.x + (i as f32 + 0.5) * seg;
        let pitch = ((z1 - z0) / seg).atan();
        let mid = (z0 + z1) / 2.0;
        let size = Vec3::new(seg / pitch.cos() + 2.0, deck.half.y * 2.0, thick);
        p.add(root, M::Cube, top.clone(), Vec3::new(x, 0.0, mid - thick / 2.0), Quat::from_rotation_y(-pitch), size);
        let ground = p.terrain.ground(deck.center + deck.dir * x) - 10.0;
        if mid - thick > ground {
            p.block(root, side, Vec2::new(x, 0.0), ground, Vec3::new(seg + 0.5, deck.half.y * 2.0, mid - thick - ground));
        }
    }
    let lip = deck.height_at(1.0);
    let n = (deck.half.y * 2.0 / 20.0) as i32;
    for k in 0..n {
        let color = if k % 2 == 0 { Color::srgb(0.95, 0.8, 0.1) } else { Color::srgb(0.1, 0.1, 0.1) };
        let y = -deck.half.y + (k as f32 + 0.5) * deck.half.y * 2.0 / n as f32;
        p.block(root, color, Vec2::new(deck.half.x - 5.0, y), lip - 0.5, Vec3::new(10.0, deck.half.y * 2.0 / n as f32, 1.2));
    }
}

/// One cross-section of a ribbon: centre, the "left" direction, how far it
/// reaches each way, and its top and bottom heights.
struct Section {
    center: Vec2,
    left: Vec2,
    half: f32,
    top: f32,
    bottom: f32,
}

/// A continuous strip along a path: two side walls, and a top surface if
/// `top` is set.
fn ribbon(sections: &[Section], top: bool) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let (mut pos, mut normals, mut idx): (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>) = (vec![], vec![], vec![]);
    let mut strip = |a: &dyn Fn(&Section) -> Vec3, b: &dyn Fn(&Section) -> Vec3, normal: &dyn Fn(&Section) -> Vec3| {
        let base = pos.len() as u32;
        for s in sections {
            pos.push(a(s).to_array());
            pos.push(b(s).to_array());
            normals.push(normal(s).to_array());
            normals.push(normal(s).to_array());
        }
        for i in 0..sections.len() as u32 - 1 {
            let (a0, b0, a1, b1) = (base + 2 * i, base + 2 * i + 1, base + 2 * i + 2, base + 2 * i + 3);
            idx.extend([a0, a1, b1, a0, b1, b0]);
        }
    };
    let edge = |s: &Section, side: f32, z: f32| (s.center + s.left * side * s.half).extend(z);
    if top {
        strip(&|s| edge(s, -1.0, s.top), &|s| edge(s, 1.0, s.top), &|_| Vec3::Z);
    }
    for side in [-1.0, 1.0] {
        strip(&move |s| edge(s, side, s.top), &move |s| edge(s, side, s.bottom), &move |s| (s.left * side).extend(0.0));
    }
    // Textures are laid flat over the world, one tile every 128 units.
    let uvs: Vec<[f32; 2]> = pos.iter().map(|p| [p[0] / 128.0, p[1] / 128.0]).collect();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(idx))
}

/// Every track barrier in one mesh: walls that meet end to end (no
/// overlapping faces to flicker), coloured in alternating sections, with
/// their tops following the ground.
fn barrier_mesh(t: &Track, terrain: &Terrain, theme: &Theme) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let (mut pos, mut normals, mut colors, mut idx) = (vec![], vec![], vec![], vec![]);
    let mut quad = |mut corners: [Vec3; 4], normal: Vec3, color: [f32; 4]| {
        // Wind the triangles to face along `normal`, so lighting is right on
        // both sides of the track (mirrored corners would face backwards).
        if (corners[1] - corners[0]).cross(corners[2] - corners[0]).dot(normal) < 0.0 {
            corners.reverse();
        }
        let base = pos.len() as u32;
        for c in corners {
            pos.push(c.to_array());
            normals.push(normal.to_array());
            colors.push(color);
        }
        idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    for ([ia, ib, oa, ob], striped) in t.barrier_walls() {
        let color = match theme.wall_stripe {
            Some(stripe) if striped => stripe,
            _ => theme.wall,
        };
        let c = color.to_linear();
        let color = [c.red, c.green, c.blue, 1.0];
        let top = |p: Vec2| terrain.height(p) + 26.0;
        let bottom = |p: Vec2| terrain.height(p) - 20.0;
        let up = |p: Vec2| p.extend(top(p));
        let down = |p: Vec2| p.extend(bottom(p));
        let inward = ((ia + ib) / 2.0 - (oa + ob) / 2.0).normalize_or_zero().extend(0.0);
        quad([up(ia), up(ib), up(ob), up(oa)], Vec3::Z, color);
        quad([up(ia), up(ib), down(ib), down(ia)], inward, color);
        quad([up(oa), up(ob), down(ob), down(oa)], -inward, color);
    }
    // The material uses vertex colours for the paint.
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(idx))
}

/// The top of a ribbon as a fine grid whose opacity is `alpha` at each
/// point, with vertices relative to `origin`.
fn faded_top(sections: &[Section], origin: Vec3, alpha: &dyn Fn(Vec2) -> f32) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    const ACROSS: usize = 24;
    let (mut pos, mut uvs, mut colors, mut idx) = (vec![], vec![], vec![], vec![]);
    for s in sections {
        for j in 0..=ACROSS {
            let c = s.center + s.left * (j as f32 / ACROSS as f32 * 2.0 - 1.0) * s.half;
            pos.push((c.extend(s.top) - origin).to_array());
            uvs.push([c.x / 128.0, c.y / 128.0]);
            colors.push([1.0, 1.0, 1.0, alpha(c).clamp(0.0, 1.0)]);
        }
    }
    let row = ACROSS as u32 + 1;
    for i in 0..sections.len() as u32 - 1 {
        for j in 0..ACROSS as u32 {
            let a = i * row + j;
            idx.extend([a, a + row, a + row + 1, a, a + row + 1, a + 1]);
        }
    }
    let n = pos.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(idx))
}

/// A tiling gravel texture: pebbles of mixed stone colours, each with a light
/// top-left and a shadow to the bottom right, packed on darker grit.
fn gravel_texture() -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    const N: usize = 256;
    let mut rng = 0x06A1_17E1_u32;
    let mut rand = || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng as f32 / u32::MAX as f32
    };
    // Grit: dark, slightly speckled.
    let mut px: Vec<Vec3> = (0..N * N).map(|_| Vec3::splat(0.26 + rand() * 0.08) * Vec3::new(1.0, 0.96, 0.88)).collect();
    let stones = [
        Vec3::new(0.62, 0.58, 0.5),
        Vec3::new(0.5, 0.47, 0.42),
        Vec3::new(0.7, 0.66, 0.58),
        Vec3::new(0.55, 0.45, 0.36),
        Vec3::new(0.44, 0.44, 0.45),
    ];
    for _ in 0..1040 {
        let (cx, cy) = (rand() * N as f32, rand() * N as f32);
        let (rx, ry) = (2.0 + rand() * 4.5, 1.8 + rand() * 3.5);
        let angle = rand() * std::f32::consts::TAU;
        let color = stones[(rand() * stones.len() as f32) as usize % stones.len()] * (0.9 + rand() * 0.2);
        let (sin, cos) = angle.sin_cos();
        let r = rx.max(ry).ceil() as i32 + 2;
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (dx as f32, dy as f32);
                let (u, v) = ((x * cos + y * sin) / rx, (-x * sin + y * cos) / ry);
                let d = u * u + v * v;
                // Wraps round the edges so the texture tiles.
                let i = ((cy as i32 + dy).rem_euclid(N as i32) as usize) * N + (cx as i32 + dx).rem_euclid(N as i32) as usize;
                if d <= 1.0 {
                    // Lit from the top left.
                    let light = 1.0 + 0.25 * (-(x / rx + y / ry) * 0.5).clamp(-1.0, 1.0) - 0.15 * d;
                    px[i] = color * light;
                } else if d <= 1.6 && dx + dy > 0 {
                    px[i] *= 0.75; // shadow
                }
            }
        }
    }
    let data = px
        .iter()
        .flat_map(|c| {
            let [r, g, b] = c.to_array().map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8);
            [r, g, b, 255]
        })
        .collect();
    let mut image = Image::new(
        Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// A bridge or ford: one continuous surface following the track, with kerbs.
/// A bridge stands on its banks and on pillars in the river; a ford is a
/// solid causeway.
#[allow(clippy::too_many_arguments)]
fn draw_crossing(
    p: &mut Painter,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    t: &Track,
    def: &TrackDef,
    kind: DeckKind,
    s0: f32,
    s1: f32,
) {
    let n = ((s1 - s0) / 10.0).ceil() as usize;
    let at = |i: usize| s0 + (s1 - s0) * i as f32 / n as f32;
    let road = t.width / 2.0;
    let (body, body_half, thick) = match kind {
        DeckKind::Bridge => (Color::srgb(0.58, 0.57, 0.54), road + BARRIER + 17.0, 16.0),
        _ => (Color::srgb(0.48, 0.45, 0.38), road + BARRIER + 10.0, 6.0),
    };
    let terrain = p.terrain.clone();
    let sections = |half: f32, lift: f32, depth: &dyn Fn(Vec2, f32) -> f32| -> Vec<Section> {
        (0..=n)
            .map(|i| {
                let s = at(i);
                let center = t.point_at(s);
                let top = terrain.height(center) + lift;
                Section { center, left: t.dir_at(s).perp(), half, top, bottom: depth(center, top) }
            })
            .collect()
    };
    // A ford is loose gravel; everything else is plain.
    let gravel = (kind == DeckKind::Ford).then(|| images.add(gravel_texture()));
    let solid = |material: Color| StandardMaterial {
        base_color: if gravel.is_some() { Color::WHITE } else { material },
        base_color_texture: gravel.clone(),
        perceptual_roughness: 0.9,
        double_sided: true,
        cull_mode: None,
        ..default()
    };
    let mut spawn = |p: &mut Painter, mesh: Mesh, color: Color| {
        let material = p.materials.add(solid(color));
        p.commands.spawn((RoundEntity, Mesh3d(meshes.add(mesh)), MeshMaterial3d(material), Transform::default()));
    };
    // The body: a girder over the water, solid where the bank comes up close.
    let body_sections = sections(body_half, -0.6, &|c, top| {
        let ground = terrain.ground(c) - 10.0;
        if kind == DeckKind::Ford || ground > top - 45.0 { ground.min(top - thick) } else { top - thick }
    });
    spawn(p, ribbon(&body_sections, kind != DeckKind::Ford), body);
    if kind == DeckKind::Bridge {
        spawn(p, ribbon(&sections(road, 0.0, &|_, top| top - 1.0), true), def.asphalt);
        // Kerbs: short blocks in alternating colours along both edges.
        for i in 0..n {
            let s = (at(i) + at(i + 1)) / 2.0;
            let (c, dir) = (t.point_at(s), t.dir_at(s));
            let color = def.kerbs[(i / 6) % 2];
            for side in [-1.0, 1.0] {
                let e = c + dir.perp() * side * (road - 7.0);
                let root = p.root_at(e.extend(p.terrain.height(e)), Quat::from_rotation_z(dir.to_angle()));
                // Every other block a hair higher, so overlapping ends don't flicker.
                let lift = (i % 2) as f32 * 0.25;
                p.block(root, color, Vec2::ZERO, -0.5, Vec3::new((s1 - s0) / n as f32 + 1.0, 14.0 + lift, 0.9 + lift));
            }
        }
        // Pillars where the river runs underneath.
        for i in (0..=n).step_by(9) {
            let c = t.point_at(at(i));
            let top = p.terrain.height(c) - thick;
            let ground = p.terrain.ground(c) - 10.0;
            if top - ground > 45.0 {
                let root = p.root_at(c.extend(0.0), Quat::from_rotation_z(t.dir_at(at(i)).to_angle()));
                for y in [-0.6, 0.6] {
                    p.post(root, body, Vec2::new(0.0, y * body_half), ground, 10.0, top - ground);
                }
            }
        }
    }
    if kind == DeckKind::Ford
        && let Some((level, river)) = terrain.water
    {
        // The gravel fades out with distance from the river, so the ford
        // blends into the road along lines that follow the banks.
        let fade = |c: Vec2| 1.0 - smoothstep(river.half_width + 30.0, river.half_width + river.bank + 10.0, river.distance(c));
        let origin = body_sections[n / 2].center.extend(level);
        let material = p.materials.add(StandardMaterial {
            base_color_texture: gravel.clone(),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.9,
            ..default()
        });
        p.commands.spawn((
            RoundEntity,
            Mesh3d(meshes.add(faded_top(&body_sections, origin, &fade))),
            MeshMaterial3d(material),
            Transform::from_translation(origin),
            bevy::light::NotShadowCaster,
        ));
    }
}

/// A gantry over the start/finish line.
fn draw_gantry(p: &mut Painter, t: &Track) {
    let (at, dir) = (t.point_at(0.0), t.dir_at(0.0));
    let ground = p.terrain.height(at);
    let root = p.root_at(at.extend(ground), Quat::from_rotation_z(dir.to_angle()));
    let reach = t.width / 2.0 + BARRIER + 16.0;
    let steel = Color::srgb(0.25, 0.25, 0.28);
    let mut top = 0.0_f32;
    for side in [-1.0, 1.0] {
        let foot = Vec2::new(0.0, side * reach);
        let base = p.terrain.height(at + dir.perp() * side * reach) - ground;
        top = top.max(base);
        p.block(root, steel, foot, base - 10.0, Vec3::new(20.0, 20.0, 14.0));
        p.post(root, steel, foot, base - 10.0, 6.0, 130.0);
    }
    let beam = top + 100.0;
    p.block(root, steel, Vec2::ZERO, beam, Vec3::new(14.0, reach * 2.0 + 14.0, 14.0));
    let n = ((reach * 2.0) / 16.0) as i32;
    for k in 0..n {
        for row in 0..2 {
            let y = (k as f32 + 0.5 - n as f32 / 2.0) * 16.0;
            let color = if (k + row) % 2 == 0 { Color::srgb(0.95, 0.95, 0.95) } else { Color::srgb(0.08, 0.08, 0.08) };
            p.block(root, color, Vec2::new((row as f32 - 0.5) * 7.0, y), beam + 14.0, Vec3::new(7.0, 16.0, 1.0));
        }
    }
}

fn draw_obstacle(p: &mut Painter, o: &Obstacle, theme: &Theme) -> Entity {
    let footprint = o.bounding_radius() * 0.7;
    match (o.kind, o.shape) {
        (ObstacleKind::Wall, Shape::Box(s)) => {
            // Built from short segments so the top follows the ground.
            let root = p.root_at(o.pos.extend(0.0), Quat::IDENTITY);
            let (len, across, along) = if s.x > s.y { (s.x, s.y, Vec2::X) } else { (s.y, s.x, Vec2::Y) };
            let n = (len / 60.0).round().max(1.0) as i32;
            let seg = len / n as f32;
            for i in 0..n {
                let t = -len / 2.0 + (i as f32 + 0.5) * seg;
                let at = along * t;
                let world = o.pos + at;
                let heights = [-0.5, 0.0, 0.5].map(|k| p.terrain.height(world + along * seg * k));
                let lo = heights.iter().copied().fold(f32::MAX, f32::min) - 20.0;
                let hi = heights.iter().copied().fold(f32::MIN, f32::max) + WALL_HEIGHT;
                let size = |l: f32, a: f32, h: f32| {
                    if along == Vec2::X { Vec3::new(l, a, h) } else { Vec3::new(a, l, h) }
                };
                p.block(root, theme.wall, at, lo, size(seg + 0.5, across, hi - lo));
                if let Some(stripe) = theme.wall_stripe
                    && i % 2 == 0
                {
                    p.block(root, stripe, at, lo, size(seg * 0.5, across + 0.6, hi - lo + 0.6));
                }
            }
            root
        }
        // Track barriers are drawn as one continuous mesh (see `barrier_mesh`).
        (ObstacleKind::Barrier, _) => p.root_at(o.pos.extend(0.0), Quat::IDENTITY),
        (ObstacleKind::TireStack, Shape::Circle(r)) => {
            let root = p.root(o.pos, 0.0, footprint);
            for k in 0..3 {
                let shade = if k == 1 { 0.13 } else { 0.09 };
                let z = r * 0.35 * (1 + 2 * k) as f32;
                p.part(root, M::Torus, Color::srgb(shade, shade, shade + 0.01), Vec3::new(0.0, 0.0, z), Vec3::splat(r));
            }
            p.post(root, Color::srgb(0.04, 0.04, 0.04), Vec2::ZERO, 0.0, r * 0.4, r * 2.0);
            root
        }
        (ObstacleKind::Barrel, Shape::Circle(r)) => {
            let root = p.root(o.pos, 0.0, footprint);
            let height = r * 2.2;
            p.post(root, Color::srgb(0.75, 0.13, 0.08), Vec2::ZERO, 0.0, r, height);
            for z in [0.25, 0.65] {
                p.post(root, Color::srgb(0.5, 0.08, 0.05), Vec2::ZERO, height * z, r * 1.05, 1.8);
            }
            p.post(root, Color::srgb(0.5, 0.08, 0.05), Vec2::ZERO, height, r * 0.8, 0.8);
            p.post(root, Color::srgb(0.95, 0.8, 0.15), Vec2::ZERO, height, r * 0.32, 1.2);
            root
        }
        (ObstacleKind::Concrete, Shape::Box(s)) => {
            // Jersey barrier: wide foot, narrower top.
            let root = p.root(o.pos, o.rot, footprint);
            p.block(root, Color::srgb(0.56, 0.56, 0.54), Vec2::ZERO, -6.0, Vec3::new(s.x, s.y, 16.0));
            p.block(root, Color::srgb(0.62, 0.62, 0.6), Vec2::ZERO, 10.0, Vec3::new(s.x, s.y * 0.62, 16.0));
            p.block(root, Color::srgb(0.7, 0.7, 0.68), Vec2::ZERO, 26.0, Vec3::new(s.x, s.y * 0.4, 8.0));
            for x in [-0.25, 0.25] {
                p.block(root, Color::srgb(0.42, 0.42, 0.41), Vec2::new(x * s.x, 0.0), -6.0, Vec3::new(2.0, s.y + 0.4, 40.2));
            }
            root
        }
        (ObstacleKind::Container, Shape::Box(s)) => {
            let root = p.root(o.pos, o.rot, footprint);
            let height = 60.0;
            p.block(root, o.tint, Vec2::ZERO, -8.0, Vec3::new(s.x, s.y, height + 8.0));
            let ridge = o.tint.mix(&Color::BLACK, 0.25);
            let n = 12;
            for i in 0..n {
                let x = -s.x / 2.0 + (i as f32 + 0.5) * s.x / n as f32;
                p.block(root, ridge, Vec2::new(x, 0.0), height, Vec3::new(3.0, s.y - 6.0, 1.2));
                p.block(root, ridge, Vec2::new(x, 0.0), -8.0, Vec3::new(3.0, s.y + 1.2, height + 4.0));
            }
            p.block(root, o.tint.mix(&Color::BLACK, 0.45), Vec2::new(s.x / 2.0 - 3.0, 0.0), -8.0, Vec3::new(6.0, s.y + 0.6, height + 8.6));
            root
        }
        // Trunk and canopy are drawn by the matching tree decoration.
        (ObstacleKind::Tree, _) => p.root(o.pos, 0.0, 0.0),
        (ObstacleKind::Rock, Shape::Circle(r)) => {
            let root = p.root(o.pos, o.rot, footprint);
            p.part(root, M::Rock, Color::srgb(0.42, 0.4, 0.37), Vec3::new(0.0, 0.0, r * 0.25), Vec3::new(r, r * 0.9, r * 0.75));
            let top = p.material(Color::srgb(0.52, 0.5, 0.46), false);
            p.add(root, M::Rock, top, Vec3::new(-r * 0.15, r * 0.15, r * 0.55), Quat::from_rotation_z(0.6), Vec3::new(r * 0.6, r * 0.55, r * 0.5));
            root
        }
        (ObstacleKind::Log, Shape::Box(s)) => {
            let root = p.root(o.pos, o.rot, footprint);
            let r = s.y / 2.0;
            let lying = Quat::from_rotation_y(FRAC_PI_2);
            let bark = p.material(Color::srgb(0.4, 0.26, 0.14), false);
            p.add(root, M::Cyl, bark, Vec3::new(0.0, 0.0, r * 0.9), lying, Vec3::new(r, r, s.x));
            let wood = p.material(Color::srgb(0.7, 0.55, 0.35), false);
            for x in [-0.5, 0.5] {
                p.add(root, M::Cyl, wood.clone(), Vec3::new(x * (s.x + 0.6), 0.0, r * 0.9), lying, Vec3::new(r * 0.85, r * 0.85, 0.6));
            }
            for y in [-0.2, 0.2] {
                let z = r * 0.9 + (r * r - (y * s.y) * (y * s.y)).sqrt();
                p.block(root, Color::srgb(0.32, 0.2, 0.1), Vec2::new(0.0, y * s.y), z - 1.0, Vec3::new(s.x - 8.0, 2.0, 1.5));
            }
            root
        }
        (ObstacleKind::HayBale, Shape::Box(s)) => {
            let root = p.root(o.pos, o.rot, footprint);
            p.block(root, Color::srgb(0.85, 0.72, 0.35), Vec2::ZERO, -4.0, Vec3::new(s.x, s.y, 32.0));
            for x in [-0.22, 0.22] {
                p.block(root, Color::srgb(0.55, 0.4, 0.2), Vec2::new(x * s.x, 0.0), -4.0, Vec3::new(2.5, s.y + 0.6, 32.6));
            }
            root
        }
        (ObstacleKind::Hulk, Shape::Box(s)) => {
            // A dead car, sagging to one side.
            let root = p.root(o.pos, o.rot, footprint);
            let sag = p.root_at(Vec3::ZERO, Quat::from_rotation_x(0.07 * (o.rot * 3.0).sin()));
            p.commands.entity(root).add_child(sag);
            p.block(sag, o.tint, Vec2::ZERO, 0.0, Vec3::new(s.x, s.y, 13.0));
            p.block(sag, o.tint.mix(&Color::BLACK, 0.35), Vec2::new(-s.x * 0.08, 0.0), 13.0, Vec3::new(s.x * 0.4, s.y * 0.8, 9.0));
            p.block(sag, Color::srgb(0.08, 0.08, 0.09), Vec2::new(s.x * 0.2, 0.0), 12.0, Vec3::new(s.x * 0.1, s.y * 0.7, 6.0));
            p.post(sag, Color::srgb(0.5, 0.28, 0.12), Vec2::new(-s.x * 0.3, s.y * 0.2), 13.0, 6.0, 0.6);
            root
        }
        _ => p.root(o.pos, o.rot, 0.0),
    }
}

fn draw_decor(p: &mut Painter, canvas: &mut Canvas, d: &Decor) {
    match d.kind {
        // Flat markings are painted into the ground texture.
        DecorKind::Patch => canvas.ellipse(d.pos, d.rot, d.size / 2.0, d.tint),
        DecorKind::OilStain => {
            let oil = Color::srgba(0.05, 0.05, 0.06, 0.45);
            canvas.ellipse(d.pos, d.rot, d.size / 2.0, oil);
            let off = Vec2::from_angle(d.rot).rotate(d.size * Vec2::new(0.35, 0.2));
            canvas.ellipse(d.pos + off, d.rot, d.size / 4.0, oil);
        }
        DecorKind::PaintedRing => canvas.ring(d.pos, d.size.x * 0.93, d.size.x, d.tint),
        DecorKind::PaintedLine => canvas.rect(d.pos, d.rot, d.size, d.tint),
        DecorKind::Bush => {
            let r = d.size.x;
            let root = p.root(d.pos, d.rot, r * 0.5);
            for (off, shade) in [(Vec2::new(-0.4, -0.2), 0.0), (Vec2::new(0.4, -0.1), 0.06), (Vec2::new(0.0, 0.35), 0.12)] {
                let color = Color::srgb(0.16 + shade, 0.36 + shade, 0.14);
                p.part(root, M::Ball, color, (off * r).extend(r * (0.35 + shade * 2.0)), Vec3::new(r, r, r * 0.8) * 0.75);
            }
        }
        DecorKind::Tree => {
            let r = d.size.x;
            let root = p.root(d.pos, d.rot, r * 0.25);
            let trunk = r * 1.5;
            p.post(root, Color::srgb(0.3, 0.2, 0.12), Vec2::ZERO, -6.0, r * 0.25, trunk + 6.0);
            let blobs = [
                (Vec2::ZERO, 0.0, 1.0, Color::srgb(0.12, 0.3, 0.12)),
                (Vec2::new(-0.2, 0.15), 0.35, 0.65, Color::srgb(0.17, 0.4, 0.16)),
                (Vec2::new(-0.3, 0.25), 0.6, 0.32, Color::srgb(0.24, 0.5, 0.2)),
            ];
            // Inside the arena, cars can drive under trees: give each its own
            // materials so it can fade out over the player.
            let inside =
                p.fade_trees && d.pos.x.abs() < p.terrain_half().x && d.pos.y.abs() < p.terrain_half().y;
            let mut faders = Vec::new();
            for (off, up, size, color) in blobs {
                let material = if inside {
                    let m = p.materials.add(surface(color, false));
                    faders.push(m.clone());
                    m
                } else {
                    p.material(color, false)
                };
                let at = (off * r).extend(trunk + up * r);
                p.add(root, M::Ball, material, at, Quat::IDENTITY, Vec3::new(r, r, r * 0.8) * size);
            }
            if inside {
                p.commands.entity(root).insert(Canopy { radius: r, alpha: 1.0, materials: faders });
            }
        }
        DecorKind::Stand => {
            // Stepped seating rising away from the arena (local -y faces it).
            let s = d.size;
            let root = p.root(d.pos, d.rot, s.y * 0.3);
            let rows = 4;
            let depth = s.y / rows as f32;
            let crowd = [
                Color::srgb(0.85, 0.3, 0.25),
                Color::srgb(0.25, 0.45, 0.85),
                Color::srgb(0.9, 0.8, 0.3),
                Color::srgb(0.9, 0.9, 0.9),
                Color::srgb(0.3, 0.7, 0.35),
                Color::srgb(0.75, 0.55, 0.4),
            ];
            let mut k = 0usize;
            for row in 0..rows {
                let front = -s.y / 2.0 + row as f32 * depth;
                let top = 14.0 * (row + 1) as f32;
                let shade = 0.3 + 0.04 * (row % 2) as f32;
                let y = (front + s.y / 2.0) / 2.0;
                p.block(root, Color::srgb(shade, shade, shade + 0.02), Vec2::new(0.0, y), -20.0, Vec3::new(s.x, s.y / 2.0 - front, top + 20.0));
                let n = (s.x / 16.0) as i32;
                for i in 0..n {
                    k = k.wrapping_mul(1_103_515_245).wrapping_add(12_345 + i as usize);
                    if k % 7 == 0 {
                        continue; // empty seat
                    }
                    let x = -s.x / 2.0 + (i as f32 + 0.5) * s.x / n as f32;
                    let fan = crowd[(k >> 8) % crowd.len()];
                    p.part(root, M::Ball, fan, Vec3::new(x, front + depth * 0.55, top + 5.0), Vec3::new(5.0, 5.0, 6.0));
                }
            }
            p.block(root, Color::srgb(0.22, 0.22, 0.24), Vec2::new(0.0, s.y / 2.0 - 4.0), -20.0, Vec3::new(s.x, 8.0, 14.0 * rows as f32 + 60.0));
        }
        DecorKind::Floodlight => {
            canvas.ellipse(d.pos, 0.0, Vec2::splat(140.0), Color::srgba(1.0, 0.95, 0.7, 0.08));
            let facing = (-d.pos).to_angle();
            let root = p.root(d.pos, facing, 10.0);
            let steel = Color::srgb(0.2, 0.2, 0.22);
            p.block(root, steel, Vec2::ZERO, -4.0, Vec3::new(24.0, 24.0, 14.0));
            p.post(root, steel, Vec2::ZERO, 0.0, 5.0, 230.0);
            p.block(root, steel, Vec2::new(4.0, 0.0), 225.0, Vec3::new(16.0, 44.0, 22.0));
            let lamp = p.material(Color::srgb(1.0, 0.95, 0.75), true);
            p.add(root, M::Cube, lamp.clone(), Vec3::new(12.5, 0.0, 236.0), Quat::IDENTITY, Vec3::new(1.5, 38.0, 16.0));
            p.add(root, M::Cube, lamp, Vec3::new(4.0, 0.0, 247.5), Quat::IDENTITY, Vec3::new(12.0, 38.0, 1.5));
        }
        DecorKind::ScrapPile => {
            let s = d.size.x;
            let root = p.root(d.pos, d.rot, s * 0.3);
            let greys = [
                Color::srgb(0.35, 0.33, 0.3),
                Color::srgb(0.45, 0.28, 0.18),
                Color::srgb(0.28, 0.28, 0.3),
                Color::srgb(0.5, 0.48, 0.45),
            ];
            for i in 0..7 {
                let a = i as f32 * 2.4;
                let off = Vec2::from_angle(a) * s * 0.25 * ((i % 3) as f32 * 0.5 + 0.3);
                // Pieces nearer the middle sit higher up the heap.
                let z = s * 0.2 * (1.0 - off.length() / (s * 0.4)).max(0.0);
                let tilt = Quat::from_rotation_z(a) * Quat::from_rotation_x(0.25 * ((i % 3) as f32 - 1.0));
                let material = p.material(greys[i % greys.len()], false);
                p.add(root, M::Cube, material, off.extend(z), tilt, Vec3::new(s * 0.45, s * 0.22, s * 0.12));
            }
        }
        DecorKind::GrassTuft => {
            let root = p.root(d.pos, 0.0, 4.0);
            for (i, off) in [Vec2::new(-4.0, 0.0), Vec2::new(0.0, 3.0), Vec2::new(4.0, -1.0)].into_iter().enumerate() {
                let lean = i as f32 - 1.0;
                let blade = p.material(Color::srgb(0.2, 0.33 + i as f32 * 0.04, 0.15), false);
                let rot = Quat::from_rotation_z(0.3 * lean) * Quat::from_rotation_y(0.3 * lean);
                p.add(root, M::Cube, blade, off.extend(4.0), rot, Vec3::new(2.0, 2.0, 10.0));
            }
        }
    }
}

impl Painter<'_, '_, '_> {
    fn terrain_half(&self) -> Vec2 {
        self.terrain.size() / 2.0
    }
}

/// Fade tree tops the player is driving under, so they don't hide the car.
pub fn fade_canopies(
    time: Res<Time>,
    player: Query<&Transform, With<crate::Player>>,
    mut canopies: Query<(&Transform, &mut Canopy)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let player = player.single().ok().map(|t| t.translation.truncate());
    let k = 1.0 - (-8.0 * time.delta_secs()).exp();
    for (tf, mut canopy) in &mut canopies {
        let under = player.is_some_and(|p| p.distance(tf.translation.truncate()) < canopy.radius + 20.0);
        let target = if under { 0.3 } else { 1.0 };
        if (canopy.alpha - target).abs() < 0.01 {
            continue;
        }
        canopy.alpha += (target - canopy.alpha) * k;
        if (canopy.alpha - target).abs() < 0.01 {
            canopy.alpha = target;
        }
        for handle in &canopy.materials {
            if let Some(mut m) = materials.get_mut(handle) {
                m.base_color.set_alpha(canopy.alpha);
                // Opaque when solid, so it still casts a shadow.
                m.alpha_mode = if canopy.alpha < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_keep_obstacles_inside_and_apart() {
        for def in ARENAS.iter().filter(|d| !d.is_race()) {
            let l = Layout::new(def);
            let half = def.size / 2.0;
            let inner: Vec<&Obstacle> = l.obstacles.iter().filter(|o| o.kind != ObstacleKind::Wall).collect();
            assert!(inner.len() > 15, "{}: only {} obstacles", def.name, inner.len());
            for o in &inner {
                assert!(o.pos.x.abs() < half.x && o.pos.y.abs() < half.y, "{}: {:?} outside", def.name, o.kind);
            }
            // There must be room to drive across the middle third.
            let blocked = (-10..=10)
                .filter(|i| {
                    let p = Vec2::new(*i as f32 * half.x / 12.0, 0.0);
                    inner.iter().any(|o| o.distance(p) < 1.0)
                })
                .count();
            assert!(blocked < 5, "{}: centre line mostly blocked", def.name);
        }
    }

    #[test]
    fn every_car_starts_with_open_road() {
        for def in ARENAS.iter().filter(|d| !d.is_race()) {
            let layout = Layout::new(def);
            let (points, all_fair) = spawn_points(&layout.obstacles, def.size, 6);
            assert!(all_fair, "{}: some car has no fair start: {points:?}", def.name);
        }
    }

    #[test]
    fn old_dust_bowl_player_start_is_rejected() {
        // Where the player used to start: nose-first into a concrete block,
        // barrels right behind.
        let layout = Layout::new(&ARENAS[0]);
        let old = Vec2::new(0.0, 437.0);
        assert!(!fair_spawn(&layout.obstacles, old, (-old).to_angle()));
        let (points, _) = spawn_points(&layout.obstacles, ARENAS[0].size, 6);
        assert!(points[0].0.distance(old) > 50.0, "player still starts at {:?}", points[0].0);
    }

    #[test]
    fn track_scenery_stays_off_the_track() {
        for def in ARENAS.iter().filter(|d| d.is_race()) {
            let l = Layout::new(def);
            let t = l.track.as_ref().unwrap();
            let clear = t.width / 2.0 + BARRIER;
            for o in l.obstacles.iter().filter(|o| o.kind != ObstacleKind::Barrier) {
                let d = t.distance(o.pos) - o.bounding_radius();
                assert!(d > clear, "{}: {:?} at {} is on the track", def.name, o.kind, o.pos);
            }
            for d in l.decor.iter().filter(|d| matches!(d.kind, DecorKind::Tree | DecorKind::Stand)) {
                assert!(t.distance(d.pos) > clear, "{}: {:?} at {} is on the track", def.name, d.kind, d.pos);
            }
        }
    }

    #[test]
    fn layouts_are_deterministic() {
        for def in &ARENAS {
            let (a, b) = (Layout::new(def), Layout::new(def));
            assert_eq!(a.obstacles.len(), b.obstacles.len());
            assert!(a.obstacles.iter().zip(&b.obstacles).all(|(x, y)| x.pos == y.pos));
        }
    }
}
