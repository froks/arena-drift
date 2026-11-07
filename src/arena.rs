//! Arenas: size, theme, obstacles and decorations, all data-driven.
//!
//! An arena is an [`ArenaDef`]: a size, colours, and a `build` function that
//! places obstacles and decorations into a [`Layout`]. The boundary walls are
//! ordinary box obstacles, so nothing assumes a rectangular play area — a race
//! track can later be built from walls laid along a path plus a new [`Mode`].

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;

use crate::RoundEntity;
use crate::physics::{Contact, Obb};
use crate::terrain::{Canvas, Terrain, berm, bump, smoothstep};

/// What an arena is used for. Only derbies exist so far; a race track would
/// add `Race { checkpoints, laps }` and reuse walls/obstacles as borders.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Derby,
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

pub static ARENAS: [ArenaDef; 3] = [
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
];

pub const WALL_THICKNESS: f32 = 24.0;

// ---------------------------------------------------------------------------
// Obstacles
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObstacleKind {
    Wall,
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
            ObstacleKind::HayBale => 0.2,
            _ => 0.35,
        }
    }
    /// Multiplier on crash damage: tyres cushion, concrete and rock hurt.
    pub fn damage(self) -> f32 {
        match self {
            ObstacleKind::TireStack | ObstacleKind::HayBale => 0.25,
            ObstacleKind::Concrete | ObstacleKind::Rock | ObstacleKind::Tree => 1.6,
            _ => 1.0,
        }
    }
    pub fn minimap_color(self) -> Color {
        match self {
            ObstacleKind::Barrel => Color::srgb(0.85, 0.2, 0.1),
            ObstacleKind::Tree => Color::srgb(0.15, 0.35, 0.12),
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
    rng: u32,
}

impl Layout {
    pub fn new(def: &ArenaDef) -> Self {
        let mut layout = Self { size: def.size, obstacles: Vec::new(), decor: Vec::new(), rng: def.seed.max(1) };
        layout.walls();
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

    /// Is there room for something of radius `r` at `pos`?
    pub fn is_free(&self, pos: Vec2, r: f32, gap: f32) -> bool {
        let half = self.half() - Vec2::splat(r + 100.0);
        pos.x.abs() < half.x
            && pos.y.abs() < half.y
            && self
                .obstacles
                .iter()
                .filter(|o| o.kind != ObstacleKind::Wall)
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
) -> (Vec<Obstacle>, Terrain) {
    let layout = Layout::new(def);
    let terrain = Terrain::new(def.relief, def.size);
    let mut canvas = Canvas::new(def.size, def.theme.outfield);
    canvas.rect(Vec2::ZERO, 0.0, def.size, def.theme.floor);
    let mut p = Painter { commands, art, materials, terrain };

    for d in &layout.decor {
        draw_decor(&mut p, &mut canvas, d);
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
    (layout.obstacles, terrain)
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
            let inside = d.pos.x.abs() < p.terrain_half().x && d.pos.y.abs() < p.terrain_half().y;
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
        for def in &ARENAS {
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
        for def in &ARENAS {
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
    fn layouts_are_deterministic() {
        for def in &ARENAS {
            let (a, b) = (Layout::new(def), Layout::new(def));
            assert_eq!(a.obstacles.len(), b.obstacles.len());
            assert!(a.obstacles.iter().zip(&b.obstacles).all(|(x, y)| x.pos == y.pos));
        }
    }
}
