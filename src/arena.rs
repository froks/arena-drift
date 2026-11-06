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

#[derive(Clone, Copy)]
enum M {
    Square,
    Circle,
    Ring,
    Rock,
}

/// Shared unit meshes and a material cache keyed by colour.
#[derive(Resource)]
pub struct ArenaArt {
    square: Handle<Mesh>,
    circle: Handle<Mesh>,
    ring: Handle<Mesh>,
    rock: Handle<Mesh>,
    mats: HashMap<u32, Handle<ColorMaterial>>,
}

impl FromWorld for ArenaArt {
    fn from_world(world: &mut World) -> Self {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        Self {
            square: meshes.add(Rectangle::new(1.0, 1.0)),
            circle: meshes.add(Circle::new(1.0)),
            ring: meshes.add(Annulus::new(0.93, 1.0)),
            rock: meshes.add(RegularPolygon::new(1.0, 7)),
            mats: HashMap::new(),
        }
    }
}

struct Painter<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    art: &'a mut ArenaArt,
    materials: &'a mut Assets<ColorMaterial>,
}

impl Painter<'_, '_, '_> {
    fn mat(&mut self, c: Color) -> Handle<ColorMaterial> {
        let key = u32::from_le_bytes(c.to_srgba().to_u8_array());
        let materials = &mut *self.materials;
        self.art.mats.entry(key).or_insert_with(|| materials.add(c)).clone()
    }

    fn root(&mut self, pos: Vec2, z: f32, rot: f32) -> Entity {
        self.commands
            .spawn((
                RoundEntity,
                Transform::from_translation(pos.extend(z)).with_rotation(Quat::from_rotation_z(rot)),
                Visibility::default(),
            ))
            .id()
    }

    #[allow(clippy::too_many_arguments)]
    fn add(&mut self, root: Entity, m: M, color: Color, at: Vec2, z: f32, rot: f32, scale: Vec2) {
        let mesh = match m {
            M::Square => self.art.square.clone(),
            M::Circle => self.art.circle.clone(),
            M::Ring => self.art.ring.clone(),
            M::Rock => self.art.rock.clone(),
        };
        let material = self.mat(color);
        let child = self
            .commands
            .spawn((
                Mesh2d(mesh),
                MeshMaterial2d(material),
                Transform::from_translation(at.extend(z))
                    .with_rotation(Quat::from_rotation_z(rot))
                    .with_scale(scale.extend(1.0)),
            ))
            .id();
        self.commands.entity(root).add_child(child);
    }

    fn shadow(&mut self, root: Entity, m: M, scale: Vec2) {
        self.add(root, m, Color::srgba(0.0, 0.0, 0.0, 0.3), Vec2::new(3.0, -4.0), -0.05, 0.0, scale * 1.08);
    }
}

// Draw layers.
const Z_OUTFIELD: f32 = -12.0;
const Z_FLOOR: f32 = -10.0;
const Z_PATCH: f32 = -9.6;
const Z_PAINT: f32 = -9.4;
const Z_OUTSIDE: f32 = -2.0;
const Z_OBSTACLE: f32 = 0.5;
const Z_CANOPY: f32 = 8.0;

/// Spawn everything for an arena. Returns the obstacles so callers can
/// pick spawn points before the entities exist.
pub fn spawn_arena(
    commands: &mut Commands,
    art: &mut ArenaArt,
    materials: &mut Assets<ColorMaterial>,
    def: &ArenaDef,
) -> Vec<Obstacle> {
    let layout = Layout::new(def);
    let mut p = Painter { commands, art, materials };

    let ground = p.root(Vec2::ZERO, Z_OUTFIELD, 0.0);
    p.add(ground, M::Square, def.theme.outfield, Vec2::ZERO, 0.0, 0.0, def.size + Vec2::splat(2400.0));
    let floor = p.root(Vec2::ZERO, Z_FLOOR, 0.0);
    p.add(floor, M::Square, def.theme.floor, Vec2::ZERO, 0.0, 0.0, def.size);

    for d in &layout.decor {
        draw_decor(&mut p, d);
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
    layout.obstacles
}

fn draw_obstacle(p: &mut Painter, o: &Obstacle, theme: &Theme) -> Entity {
    let root = p.root(o.pos, Z_OBSTACLE, o.rot);
    match (o.kind, o.shape) {
        (ObstacleKind::Wall, Shape::Box(s)) => {
            p.add(root, M::Square, theme.wall, Vec2::ZERO, 0.0, 0.0, s);
            if let Some(stripe) = theme.wall_stripe {
                // Stripes along the long side.
                let (len, across, horizontal) = if s.x > s.y { (s.x, s.y, true) } else { (s.y, s.x, false) };
                let step = 60.0;
                let n = (len / step) as i32;
                for i in (0..n).step_by(2) {
                    let t = -len / 2.0 + (i as f32 + 0.5) * step;
                    let (at, scale) = if horizontal {
                        (Vec2::new(t, 0.0), Vec2::new(step * 0.5, across))
                    } else {
                        (Vec2::new(0.0, t), Vec2::new(across, step * 0.5))
                    };
                    p.add(root, M::Square, stripe, at, 0.01, 0.0, scale);
                }
            }
        }
        (ObstacleKind::TireStack, Shape::Circle(r)) => {
            p.shadow(root, M::Circle, Vec2::splat(r));
            p.add(root, M::Circle, Color::srgb(0.09, 0.09, 0.1), Vec2::ZERO, 0.0, 0.0, Vec2::splat(r));
            p.add(root, M::Ring, Color::srgb(0.22, 0.22, 0.24), Vec2::ZERO, 0.01, 0.0, Vec2::splat(r * 0.8));
            p.add(root, M::Circle, Color::srgb(0.16, 0.16, 0.17), Vec2::ZERO, 0.02, 0.0, Vec2::splat(r * 0.62));
            p.add(root, M::Circle, Color::srgb(0.05, 0.05, 0.05), Vec2::ZERO, 0.03, 0.0, Vec2::splat(r * 0.38));
        }
        (ObstacleKind::Barrel, Shape::Circle(r)) => {
            p.shadow(root, M::Circle, Vec2::splat(r));
            p.add(root, M::Circle, Color::srgb(0.75, 0.13, 0.08), Vec2::ZERO, 0.0, 0.0, Vec2::splat(r));
            p.add(root, M::Ring, Color::srgb(0.5, 0.08, 0.05), Vec2::ZERO, 0.01, 0.0, Vec2::splat(r * 0.75));
            p.add(root, M::Circle, Color::srgb(0.95, 0.8, 0.15), Vec2::ZERO, 0.02, 0.0, Vec2::splat(r * 0.3));
        }
        (ObstacleKind::Concrete, Shape::Box(s)) => {
            p.shadow(root, M::Square, s);
            p.add(root, M::Square, Color::srgb(0.58, 0.58, 0.56), Vec2::ZERO, 0.0, 0.0, s);
            p.add(root, M::Square, Color::srgb(0.68, 0.68, 0.66), Vec2::ZERO, 0.01, 0.0, s - Vec2::splat(10.0));
            for x in [-0.25, 0.25] {
                p.add(root, M::Square, Color::srgb(0.45, 0.45, 0.44), Vec2::new(x * s.x, 0.0), 0.02, 0.0, Vec2::new(2.0, s.y - 10.0));
            }
        }
        (ObstacleKind::Container, Shape::Box(s)) => {
            p.shadow(root, M::Square, s);
            p.add(root, M::Square, o.tint, Vec2::ZERO, 0.0, 0.0, s);
            let ridge = o.tint.mix(&Color::BLACK, 0.25);
            let n = 12;
            for i in 0..n {
                let x = -s.x / 2.0 + (i as f32 + 0.5) * s.x / n as f32;
                p.add(root, M::Square, ridge, Vec2::new(x, 0.0), 0.01, 0.0, Vec2::new(3.0, s.y - 6.0));
            }
            p.add(root, M::Square, o.tint.mix(&Color::BLACK, 0.45), Vec2::new(s.x / 2.0 - 3.0, 0.0), 0.02, 0.0, Vec2::new(6.0, s.y));
        }
        (ObstacleKind::Tree, Shape::Circle(r)) => {
            // Trunk only; the canopy is a decoration drawn above the cars.
            p.add(root, M::Circle, Color::srgb(0.3, 0.2, 0.12), Vec2::ZERO, 0.0, 0.0, Vec2::splat(r));
        }
        (ObstacleKind::Rock, Shape::Circle(r)) => {
            p.shadow(root, M::Rock, Vec2::splat(r));
            p.add(root, M::Rock, Color::srgb(0.42, 0.4, 0.37), Vec2::ZERO, 0.0, 0.0, Vec2::splat(r));
            p.add(root, M::Rock, Color::srgb(0.52, 0.5, 0.46), Vec2::new(-r * 0.15, r * 0.15), 0.01, 0.6, Vec2::splat(r * 0.6));
        }
        (ObstacleKind::Log, Shape::Box(s)) => {
            p.shadow(root, M::Square, s);
            p.add(root, M::Square, Color::srgb(0.4, 0.26, 0.14), Vec2::ZERO, 0.0, 0.0, s);
            for y in [-0.2, 0.2] {
                p.add(root, M::Square, Color::srgb(0.32, 0.2, 0.1), Vec2::new(0.0, y * s.y), 0.01, 0.0, Vec2::new(s.x - 8.0, 2.0));
            }
            for x in [-0.5, 0.5] {
                p.add(root, M::Circle, Color::srgb(0.7, 0.55, 0.35), Vec2::new(x * s.x, 0.0), 0.02, 0.0, Vec2::new(5.0, s.y / 2.0));
            }
        }
        (ObstacleKind::HayBale, Shape::Box(s)) => {
            p.shadow(root, M::Square, s);
            p.add(root, M::Square, Color::srgb(0.85, 0.72, 0.35), Vec2::ZERO, 0.0, 0.0, s);
            for x in [-0.22, 0.22] {
                p.add(root, M::Square, Color::srgb(0.55, 0.4, 0.2), Vec2::new(x * s.x, 0.0), 0.01, 0.0, Vec2::new(2.5, s.y));
            }
        }
        (ObstacleKind::Hulk, Shape::Box(s)) => {
            p.shadow(root, M::Square, s);
            p.add(root, M::Square, o.tint, Vec2::ZERO, 0.0, 0.0, s);
            p.add(root, M::Square, o.tint.mix(&Color::BLACK, 0.35), Vec2::new(-s.x * 0.08, 0.0), 0.01, 0.0, Vec2::new(s.x * 0.4, s.y * 0.8));
            p.add(root, M::Square, Color::srgb(0.08, 0.08, 0.09), Vec2::new(s.x * 0.2, 0.0), 0.02, 0.0, Vec2::new(s.x * 0.1, s.y * 0.7));
            p.add(root, M::Circle, Color::srgb(0.5, 0.28, 0.12), Vec2::new(-s.x * 0.3, s.y * 0.2), 0.03, 0.0, Vec2::new(7.0, 5.0));
        }
        _ => {}
    }
    root
}

fn draw_decor(p: &mut Painter, d: &Decor) {
    match d.kind {
        DecorKind::Patch => {
            let root = p.root(d.pos, Z_PATCH, d.rot);
            p.add(root, M::Circle, d.tint, Vec2::ZERO, 0.0, 0.0, d.size / 2.0);
        }
        DecorKind::OilStain => {
            let root = p.root(d.pos, Z_PAINT + 0.1, d.rot);
            let oil = Color::srgba(0.05, 0.05, 0.06, 0.45);
            p.add(root, M::Circle, oil, Vec2::ZERO, 0.0, 0.0, d.size / 2.0);
            p.add(root, M::Circle, oil, d.size * Vec2::new(0.35, 0.2), 0.01, 0.0, d.size / 4.0);
        }
        DecorKind::PaintedRing => {
            let root = p.root(d.pos, Z_PAINT, 0.0);
            p.add(root, M::Ring, d.tint, Vec2::ZERO, 0.0, 0.0, d.size);
        }
        DecorKind::PaintedLine => {
            let root = p.root(d.pos, Z_PAINT, d.rot);
            p.add(root, M::Square, d.tint, Vec2::ZERO, 0.0, 0.0, d.size);
        }
        DecorKind::Bush => {
            let root = p.root(d.pos, Z_OUTSIDE, d.rot);
            let r = d.size.x;
            for (off, shade) in [(Vec2::new(-0.4, -0.2), 0.0), (Vec2::new(0.4, -0.1), 0.06), (Vec2::new(0.0, 0.35), 0.12)] {
                p.add(root, M::Circle, Color::srgb(0.16 + shade, 0.36 + shade, 0.14), off * r, shade, 0.0, Vec2::splat(r * 0.75));
            }
        }
        DecorKind::Tree => {
            let root = p.root(d.pos, Z_CANOPY, d.rot);
            let r = d.size.x;
            p.add(root, M::Circle, Color::srgba(0.0, 0.0, 0.0, 0.25), Vec2::new(10.0, -14.0), -9.0, 0.0, Vec2::splat(r));
            p.add(root, M::Circle, Color::srgba(0.12, 0.3, 0.12, 0.93), Vec2::ZERO, 0.0, 0.0, Vec2::splat(r));
            p.add(root, M::Circle, Color::srgba(0.17, 0.4, 0.16, 0.95), Vec2::new(-r * 0.2, r * 0.15), 0.01, 0.0, Vec2::splat(r * 0.65));
            p.add(root, M::Circle, Color::srgba(0.24, 0.5, 0.2, 0.95), Vec2::new(-r * 0.3, r * 0.25), 0.02, 0.0, Vec2::splat(r * 0.32));
        }
        DecorKind::Stand => {
            let root = p.root(d.pos, Z_OUTSIDE, d.rot);
            let s = d.size;
            p.add(root, M::Square, Color::srgb(0.3, 0.3, 0.32), Vec2::ZERO, 0.0, 0.0, s);
            let rows = 4;
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
                let y = -s.y / 2.0 + (row as f32 + 0.6) * s.y / (rows as f32 + 0.3);
                p.add(root, M::Square, Color::srgb(0.38, 0.38, 0.4), Vec2::new(0.0, y - 10.0), 0.01, 0.0, Vec2::new(s.x - 10.0, 4.0));
                let n = (s.x / 16.0) as i32;
                for i in 0..n {
                    k = k.wrapping_mul(1_103_515_245).wrapping_add(12_345 + i as usize);
                    if k % 7 == 0 {
                        continue; // empty seat
                    }
                    let x = -s.x / 2.0 + (i as f32 + 0.5) * s.x / n as f32;
                    p.add(root, M::Circle, crowd[(k >> 8) % crowd.len()], Vec2::new(x, y), 0.02, 0.0, Vec2::splat(5.0));
                }
            }
        }
        DecorKind::Floodlight => {
            let root = p.root(d.pos, Z_OUTSIDE, 0.0);
            p.add(root, M::Circle, Color::srgba(1.0, 0.95, 0.7, 0.08), Vec2::ZERO, 0.0, 0.0, Vec2::splat(140.0));
            p.add(root, M::Square, Color::srgb(0.2, 0.2, 0.22), Vec2::ZERO, 0.01, 0.0, Vec2::splat(18.0));
            p.add(root, M::Square, Color::srgb(1.0, 0.95, 0.75), Vec2::ZERO, 0.02, 0.0, Vec2::new(14.0, 6.0));
        }
        DecorKind::ScrapPile => {
            let root = p.root(d.pos, Z_OUTSIDE, d.rot);
            let s = d.size.x;
            let greys = [
                Color::srgb(0.35, 0.33, 0.3),
                Color::srgb(0.45, 0.28, 0.18),
                Color::srgb(0.28, 0.28, 0.3),
                Color::srgb(0.5, 0.48, 0.45),
            ];
            for i in 0..7 {
                let a = i as f32 * 2.4;
                let off = Vec2::from_angle(a) * s * 0.25 * ((i % 3) as f32 * 0.5 + 0.3);
                p.add(root, M::Square, greys[i % greys.len()], off, i as f32 * 0.01, a, Vec2::new(s * 0.45, s * 0.22));
            }
        }
        DecorKind::GrassTuft => {
            let root = p.root(d.pos, Z_PAINT, 0.0);
            for (i, off) in [Vec2::new(-4.0, 0.0), Vec2::new(0.0, 3.0), Vec2::new(4.0, -1.0)].into_iter().enumerate() {
                p.add(root, M::Square, Color::srgb(0.2, 0.33 + i as f32 * 0.04, 0.15), off, 0.0, 0.3 * (i as f32 - 1.0), Vec2::new(2.0, 9.0));
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
