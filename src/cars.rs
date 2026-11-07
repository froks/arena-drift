//! Car models: handling stats and looks, all data-driven.
//!
//! To add a car, add an entry to [`MODELS`]. Its `look` describes the body
//! outline and details; the visual is built from simple shapes at startup.

use bevy::prelude::*;

use crate::Car;

/// Body stripe layout.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stripes {
    None,
    /// One wide stripe down the middle.
    Center,
    /// Two narrow racing stripes.
    Twin,
    /// One stripe along each flank.
    Side,
}

/// How a model looks. Everything is in fractions of half length / half width,
/// so a look scales with the model's size.
pub struct Look {
    /// Top half of the body outline (y > 0), from the nose back to the tail.
    /// It's mirrored for the bottom half and must form a convex shape.
    pub outline: &'static [(f32, f32)],
    /// Roof: (centre x, half length, half width).
    pub cabin: (f32, f32, f32),
    pub stripes: Stripes,
    pub spoiler: bool,
    pub ram_plate: bool,
    pub cargo_bed: bool,
    /// Axle positions along the car, front axle first.
    pub axles: &'static [f32],
    /// Tyre size in pixels (length, width).
    pub wheel: Vec2,
    pub paint: Color,
    pub accent: Color,
}

pub struct CarModel {
    pub name: &'static str,
    pub tier: u8,
    pub blurb: &'static str,
    /// Length × width in pixels.
    pub size: Vec2,
    pub max_hp: f32,
    /// Affects how much a car is shoved around in collisions and blasts,
    /// and how much collision damage it takes.
    pub mass: f32,
    /// Acceleration (px/s²).
    pub engine: f32,
    pub top_speed: f32,
    /// Sideways grip: higher = less drift.
    pub grip: f32,
    /// Steering rate at full lock (rad/s).
    pub turn_rate: f32,
    /// Incoming damage is divided by this.
    pub armor: f32,
    /// Engine sound pitch multiplier: big engines rumble lower.
    pub engine_pitch: f32,
    pub look: Look,
}

impl CarModel {
    /// Forward drag that makes `engine` top out at `top_speed`.
    pub fn drag(&self) -> f32 {
        self.engine / self.top_speed
    }
}

pub static MODELS: [CarModel; 12] = [
    CarModel {
        name: "Scrapper",
        tier: 1,
        blurb: "A beaten-up hatchback. Cheap, cheerful, forgiving.",
        size: Vec2::new(50.0, 30.0),
        max_hp: 90.0,
        mass: 0.9,
        engine: 520.0,
        top_speed: 540.0,
        grip: 6.5,
        turn_rate: 3.6,
        armor: 1.0,
        engine_pitch: 1.15,
        look: Look {
            outline: &[(1.0, 0.75), (0.92, 0.92), (-0.9, 0.95), (-1.0, 0.82)],
            cabin: (-0.25, 0.5, 0.78),
            stripes: Stripes::None,
            spoiler: false,
            ram_plate: false,
            cargo_bed: false,
            axles: &[0.6, -0.6],
            wheel: Vec2::new(11.2, 6.2),
            paint: Color::srgb(0.85, 0.5, 0.2),
            accent: Color::srgb(0.95, 0.92, 0.85),
        },
    },
    CarModel {
        name: "Hornet",
        tier: 1,
        blurb: "Tiny and twitchy. Quick off the line, folds like paper.",
        size: Vec2::new(47.5, 27.5),
        max_hp: 85.0,
        mass: 0.8,
        engine: 600.0,
        top_speed: 600.0,
        grip: 7.0,
        turn_rate: 4.0,
        armor: 0.9,
        engine_pitch: 1.3,
        look: Look {
            outline: &[(1.0, 0.5), (0.85, 0.85), (0.5, 0.97), (-0.7, 0.97), (-0.95, 0.8), (-1.0, 0.55)],
            cabin: (-0.15, 0.42, 0.75),
            stripes: Stripes::Twin,
            spoiler: false,
            ram_plate: false,
            cargo_bed: false,
            axles: &[0.58, -0.6],
            wheel: Vec2::new(10.0, 6.2),
            paint: Color::srgb(0.95, 0.8, 0.15),
            accent: Color::srgb(0.1, 0.1, 0.1),
        },
    },
    CarModel {
        name: "Rustbucket",
        tier: 1,
        blurb: "Wood-panelled station wagon. Slow, but built like a shed.",
        size: Vec2::new(54.0, 29.0),
        max_hp: 92.0,
        mass: 1.0,
        engine: 470.0,
        top_speed: 500.0,
        grip: 6.0,
        turn_rate: 3.2,
        armor: 1.0,
        engine_pitch: 0.95,
        look: Look {
            outline: &[(1.0, 0.8), (0.94, 0.93), (-0.95, 0.95), (-1.0, 0.88)],
            cabin: (-0.3, 0.6, 0.8),
            stripes: Stripes::Side,
            spoiler: false,
            ram_plate: false,
            cargo_bed: false,
            axles: &[0.62, -0.62],
            wheel: Vec2::new(11.2, 6.2),
            paint: Color::srgb(0.55, 0.7, 0.8),
            accent: Color::srgb(0.5, 0.33, 0.18),
        },
    },
    CarModel {
        name: "Sandhopper",
        tier: 1,
        blurb: "Dune buggy on fat tyres. Featherweight, loose and lively.",
        size: Vec2::new(44.0, 30.0),
        max_hp: 88.0,
        mass: 0.78,
        engine: 640.0,
        top_speed: 580.0,
        grip: 5.0,
        turn_rate: 4.3,
        armor: 0.95,
        engine_pitch: 1.4,
        look: Look {
            outline: &[(1.0, 0.45), (0.75, 0.62), (-0.8, 0.62), (-1.0, 0.5)],
            cabin: (-0.2, 0.3, 0.5),
            stripes: Stripes::None,
            spoiler: false,
            ram_plate: false,
            cargo_bed: false,
            axles: &[0.62, -0.6],
            wheel: Vec2::new(12.5, 8.0),
            paint: Color::srgb(0.25, 0.75, 0.35),
            accent: Color::srgb(0.1, 0.1, 0.1),
        },
    },
    CarModel {
        name: "Bruiser",
        tier: 2,
        blurb: "Big-block muscle car. Loves to slide, hits hard.",
        size: Vec2::new(60.0, 32.5),
        max_hp: 115.0,
        mass: 1.2,
        engine: 560.0,
        top_speed: 640.0,
        grip: 5.2,
        turn_rate: 3.2,
        armor: 1.1,
        engine_pitch: 0.85,
        look: Look {
            outline: &[(1.0, 0.82), (0.96, 0.92), (-0.92, 0.92), (-1.0, 0.85)],
            cabin: (-0.3, 0.32, 0.72),
            stripes: Stripes::Twin,
            spoiler: true,
            ram_plate: false,
            cargo_bed: false,
            axles: &[0.62, -0.62],
            wheel: Vec2::new(12.5, 7.5),
            paint: Color::srgb(0.8, 0.15, 0.15),
            accent: Color::srgb(0.95, 0.95, 0.95),
        },
    },
    CarModel {
        name: "Razorback",
        tier: 2,
        blurb: "Balanced sports coupe. Grippy and precise.",
        size: Vec2::new(57.5, 30.0),
        max_hp: 100.0,
        mass: 1.0,
        engine: 600.0,
        top_speed: 690.0,
        grip: 6.8,
        turn_rate: 3.7,
        armor: 1.0,
        engine_pitch: 1.05,
        look: Look {
            outline: &[(1.0, 0.35), (0.8, 0.75), (0.45, 0.95), (-0.75, 0.95), (-1.0, 0.75)],
            cabin: (-0.2, 0.35, 0.7),
            stripes: Stripes::Center,
            spoiler: true,
            ram_plate: false,
            cargo_bed: false,
            axles: &[0.55, -0.62],
            wheel: Vec2::new(11.2, 6.2),
            paint: Color::srgb(0.15, 0.45, 0.9),
            accent: Color::srgb(0.95, 0.95, 0.95),
        },
    },
    CarModel {
        name: "Interceptor",
        tier: 2,
        blurb: "Long, fast sedan. Built for chasing things down.",
        size: Vec2::new(60.0, 30.0),
        max_hp: 118.0,
        mass: 1.15,
        engine: 580.0,
        top_speed: 680.0,
        grip: 6.2,
        turn_rate: 3.4,
        armor: 1.05,
        engine_pitch: 0.95,
        look: Look {
            outline: &[(1.0, 0.72), (0.93, 0.9), (-0.92, 0.9), (-1.0, 0.78)],
            cabin: (-0.1, 0.3, 0.74),
            stripes: Stripes::Side,
            spoiler: false,
            ram_plate: false,
            cargo_bed: false,
            axles: &[0.6, -0.6],
            wheel: Vec2::new(11.2, 6.2),
            paint: Color::srgb(0.1, 0.35, 0.2),
            accent: Color::srgb(0.85, 0.7, 0.2),
        },
    },
    CarModel {
        name: "Brawler",
        tier: 2,
        blurb: "Pickup truck. Heavy enough to push most cars around.",
        size: Vec2::new(62.0, 32.0),
        max_hp: 125.0,
        mass: 1.3,
        engine: 500.0,
        top_speed: 590.0,
        grip: 5.8,
        turn_rate: 3.0,
        armor: 1.05,
        engine_pitch: 0.8,
        look: Look {
            outline: &[(1.0, 0.85), (0.95, 0.95), (-0.97, 0.95), (-1.0, 0.88)],
            cabin: (0.35, 0.22, 0.84),
            stripes: Stripes::None,
            spoiler: false,
            ram_plate: false,
            cargo_bed: true,
            axles: &[0.6, -0.6],
            wheel: Vec2::new(12.5, 7.5),
            paint: Color::srgb(0.8, 0.7, 0.5),
            accent: Color::srgb(0.4, 0.25, 0.12),
        },
    },
    CarModel {
        name: "Juggernaut",
        tier: 3,
        blurb: "Armoured truck with a ram plate. Slow to turn, hard to stop.",
        size: Vec2::new(70.0, 37.5),
        // Was 170 HP / 1.35 armour / 1.8 mass: won 85% of balance-sim rounds.
        max_hp: 130.0,
        mass: 1.5,
        engine: 440.0,
        top_speed: 520.0,
        grip: 6.0,
        turn_rate: 2.4,
        armor: 1.1,
        engine_pitch: 0.7,
        look: Look {
            outline: &[(1.0, 0.92), (0.97, 0.97), (-0.97, 0.97), (-1.0, 0.92)],
            cabin: (0.48, 0.3, 0.86),
            stripes: Stripes::None,
            spoiler: false,
            ram_plate: true,
            cargo_bed: true,
            axles: &[0.62, -0.35, -0.75],
            wheel: Vec2::new(12.5, 7.5),
            paint: Color::srgb(0.35, 0.45, 0.25),
            accent: Color::srgb(0.85, 0.75, 0.2),
        },
    },
    CarModel {
        name: "Phantom",
        tier: 3,
        blurb: "Wedge-shaped supercar. Blistering speed, sharp handling.",
        size: Vec2::new(62.5, 32.5),
        // Was 120: lost 2:1 to the Juggernaut in tier-only balance sims.
        max_hp: 135.0,
        mass: 1.1,
        engine: 700.0,
        top_speed: 780.0,
        grip: 7.2,
        turn_rate: 3.9,
        armor: 1.1,
        engine_pitch: 1.2,
        look: Look {
            outline: &[(1.0, 0.3), (0.4, 0.85), (-0.6, 1.0), (-1.0, 0.9)],
            cabin: (-0.1, 0.3, 0.6),
            stripes: Stripes::Side,
            spoiler: true,
            ram_plate: false,
            cargo_bed: false,
            axles: &[0.55, -0.6],
            wheel: Vec2::new(12.5, 7.5),
            paint: Color::srgb(0.5, 0.2, 0.75),
            accent: Color::srgb(0.3, 0.9, 0.9),
        },
    },
    CarModel {
        name: "Warhound",
        tier: 3,
        blurb: "Armoured muscle car with a ram plate. Made for T-bones.",
        size: Vec2::new(64.0, 34.0),
        max_hp: 125.0,
        mass: 1.3,
        engine: 600.0,
        top_speed: 680.0,
        grip: 6.0,
        turn_rate: 3.2,
        armor: 1.1,
        engine_pitch: 0.8,
        look: Look {
            outline: &[(1.0, 0.85), (0.95, 0.95), (-0.92, 0.95), (-1.0, 0.85)],
            cabin: (-0.3, 0.3, 0.7),
            stripes: Stripes::Twin,
            spoiler: true,
            ram_plate: true,
            cargo_bed: false,
            axles: &[0.62, -0.62],
            wheel: Vec2::new(12.5, 7.5),
            paint: Color::srgb(0.3, 0.32, 0.35),
            accent: Color::srgb(0.95, 0.5, 0.1),
        },
    },
    CarModel {
        name: "Vortex",
        tier: 3,
        blurb: "Prototype racer. The fastest thing here, if you can keep it in one piece.",
        size: Vec2::new(66.0, 34.0),
        max_hp: 132.0,
        mass: 1.1,
        engine: 720.0,
        top_speed: 800.0,
        grip: 7.5,
        turn_rate: 4.0,
        armor: 1.05,
        engine_pitch: 1.35,
        look: Look {
            outline: &[(1.0, 0.3), (0.3, 0.9), (-0.85, 1.0), (-1.0, 0.9)],
            cabin: (-0.2, 0.25, 0.5),
            stripes: Stripes::Center,
            spoiler: true,
            ram_plate: false,
            cargo_bed: false,
            axles: &[0.55, -0.62],
            wheel: Vec2::new(12.5, 7.5),
            paint: Color::srgb(0.95, 0.45, 0.1),
            accent: Color::srgb(0.1, 0.15, 0.4),
        },
    },
];

// ---------------------------------------------------------------------------
// Line-ups
// ---------------------------------------------------------------------------

/// Spare paint jobs for when a model appears more than once in a round.
pub const SPARE_PAINTS: [Color; 8] = [
    Color::srgb(0.1, 0.65, 0.6),   // teal
    Color::srgb(0.85, 0.85, 0.88), // white
    Color::srgb(0.9, 0.35, 0.6),   // pink
    Color::srgb(0.55, 0.8, 0.2),   // lime
    Color::srgb(0.2, 0.22, 0.55),  // navy
    Color::srgb(0.5, 0.1, 0.15),   // maroon
    Color::srgb(0.6, 0.62, 0.65),  // silver
    Color::srgb(0.18, 0.18, 0.2),  // black
];

/// Every tier that has at least one model, lowest first.
pub fn tiers() -> Vec<u8> {
    let mut tiers: Vec<u8> = MODELS.iter().map(|m| m.tier).collect();
    tiers.sort_unstable();
    tiers.dedup();
    tiers
}

/// Indices of every model in a tier.
pub fn tier_models(tier: u8) -> Vec<usize> {
    (0..MODELS.len()).filter(|&i| MODELS[i].tier == tier).collect()
}

/// Models for a round: the player's car first, then `rivals` cars from the
/// same tier. Rivals start with the tier's other models, then repeat.
pub fn tier_lineup(player: usize, rivals: usize) -> Vec<usize> {
    let mut pool = tier_models(MODELS[player].tier);
    pool.sort_by_key(|&i| i == player); // the player's own model goes last
    std::iter::once(player).chain((0..rivals).map(|k| pool[k % pool.len()])).collect()
}

/// Paint for each car in a line-up: the first of each model wears its own
/// colours, repeats get spare paint so every car is distinguishable.
pub fn lineup_paints(lineup: &[usize]) -> Vec<Color> {
    let mut seen = Vec::new();
    let mut spare = 0;
    lineup
        .iter()
        .map(|&m| {
            if seen.contains(&m) {
                spare += 1;
                SPARE_PAINTS[(spare - 1) % SPARE_PAINTS.len()]
            } else {
                seen.push(m);
                MODELS[m].look.paint
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Visuals
// ---------------------------------------------------------------------------

/// Which material a part uses: shared, or one of the car's own paints
/// (those get darkened as the car takes damage).
#[derive(Clone)]
enum Slot {
    Fixed(Handle<StandardMaterial>),
    Paint,
    PaintDark,
    Accent,
}

#[derive(Clone)]
struct Part {
    mesh: Handle<Mesh>,
    slot: Slot,
    transform: Transform,
    front_wheel: bool,
}

/// Prebuilt parts for every model.
#[derive(Resource)]
pub struct CarArt {
    parts: Vec<Vec<Part>>,
}

/// A car's own paint materials and their original colours.
pub struct Paint {
    pub handles: [Handle<StandardMaterial>; 3],
    pub colors: [Color; 3],
}

#[derive(Component)]
pub struct FrontWheel;

/// Height of a model's roof above the ground (for health bars and the like).
pub fn roof_height(model: &CarModel) -> f32 {
    let p = Proportions::of(model);
    p.body_top + p.cabin_h
}

/// Vertical layout of a model, in world units.
struct Proportions {
    wheel_r: f32,
    clearance: f32,
    body_h: f32,
    body_top: f32,
    cabin_h: f32,
}

impl Proportions {
    fn of(model: &CarModel) -> Self {
        let look = &model.look;
        let w = model.size.y;
        let wheel_r = look.wheel.x / 2.0;
        let clearance = wheel_r * 0.5;
        let body_h = w * if look.cargo_bed { 0.38 } else { 0.32 };
        Self { wheel_r, clearance, body_h, body_top: clearance + body_h, cabin_h: w * 0.27 }
    }
}

// Built when the app is created (not in a Startup system), because the first
// state's OnEnter systems run before Startup and already need it.
impl FromWorld for CarArt {
    fn from_world(world: &mut World) -> Self {
        world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
            build_car_art(&mut meshes, &mut materials)
        })
    }
}

fn build_car_art(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> CarArt {
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    // Cylinders run along Y, which is the axle direction for a car.
    let wheel = meshes.add(Cylinder::new(1.0, 1.0));
    let mut fixed = |c: Color, roughness: f32, unlit: bool| {
        Slot::Fixed(materials.add(StandardMaterial {
            base_color: c,
            perceptual_roughness: roughness,
            unlit,
            ..default()
        }))
    };
    let tyre = fixed(Color::srgb(0.07, 0.07, 0.08), 0.95, false);
    let hub = fixed(Color::srgb(0.5, 0.5, 0.52), 0.5, false);
    let glass = fixed(Color::srgb(0.45, 0.62, 0.78), 0.15, false);
    let glass_dark = fixed(Color::srgb(0.16, 0.22, 0.3), 0.15, false);
    let headlight = fixed(Color::srgb(1.0, 0.95, 0.7), 1.0, true);
    let taillight = fixed(Color::srgb(0.9, 0.1, 0.1), 1.0, true);
    let steel = fixed(Color::srgb(0.55, 0.57, 0.6), 0.4, false);
    let trim = fixed(Color::srgb(0.12, 0.12, 0.13), 0.7, false);

    let parts = MODELS
        .iter()
        .map(|model| {
            let look = &model.look;
            let hl = model.size / 2.0;
            let w = model.size.y;
            let pr = Proportions::of(model);
            let top = pr.body_top;
            let body = meshes.add(Extrusion::new(body_polygon(model), pr.body_h));
            let part = |mesh: &Handle<Mesh>, slot: &Slot, transform: Transform| Part {
                mesh: mesh.clone(),
                slot: slot.clone(),
                transform,
                front_wheel: false,
            };
            // A box whose bottom sits at `base`.
            let block = |slot: &Slot, x: f32, y: f32, base: f32, size: Vec3| {
                part(&cube, slot, Transform::from_xyz(x, y, base + size.z / 2.0).with_scale(size))
            };
            // A thin pane from `from` to `to` (in the car's XZ plane), `width` across.
            let pane = |slot: &Slot, from: Vec2, to: Vec2, width: f32| {
                let (a, b) = (Vec3::new(from.x, 0.0, from.y), Vec3::new(to.x, 0.0, to.y));
                let dir = (b - a).normalize();
                part(
                    &cube,
                    slot,
                    Transform::from_translation((a + b) / 2.0)
                        .with_rotation(Quat::from_rotation_arc(Vec3::X, dir))
                        .with_scale(Vec3::new(a.distance(b), width, 1.2)),
                )
            };
            let mut p = Vec::new();

            // Wheels poke out from under the body.
            for (i, &ax) in look.axles.iter().enumerate() {
                for side in [-1.0, 1.0] {
                    let at = Vec3::new(ax * hl.x, side * (hl.y - look.wheel.y * 0.2), pr.wheel_r);
                    let mut t = part(&wheel, &tyre, Transform::from_translation(at).with_scale(Vec3::new(pr.wheel_r, look.wheel.y, pr.wheel_r)));
                    t.front_wheel = i == 0;
                    p.push(t);
                    let cap = at + Vec3::Y * side * look.wheel.y * 0.5;
                    p.push(part(&wheel, &hub, Transform::from_translation(cap).with_scale(Vec3::new(pr.wheel_r * 0.5, 0.6, pr.wheel_r * 0.5))));
                }
            }

            p.push(part(&body, &Slot::Paint, Transform::from_xyz(0.0, 0.0, pr.clearance + pr.body_h / 2.0)));

            if look.cargo_bed {
                // An open bed: dark floor with ribs and a rim round the edge.
                let (back, front) = (-0.93 * hl.x, 0.12 * hl.x);
                let len = front - back;
                let cx = (back + front) / 2.0;
                p.push(block(&Slot::PaintDark, cx, 0.0, top, Vec3::new(len, w * 0.8, 0.4)));
                for k in 0..4 {
                    let x = back + len * (k as f32 + 0.5) / 4.0;
                    p.push(block(&trim, x, 0.0, top, Vec3::new(1.5, w * 0.74, 0.8)));
                }
                for side in [-1.0, 1.0] {
                    p.push(block(&Slot::Paint, cx, side * w * 0.42, top, Vec3::new(len, w * 0.06, 3.0)));
                }
                p.push(block(&Slot::Paint, back + 1.0, 0.0, top, Vec3::new(2.0, w * 0.84, 3.0)));
            }

            // Stripes run over the bonnet, roof and boot.
            let roof_top = top + pr.cabin_h;
            let stripes: Vec<(f32, f32, f32, f32)> = match look.stripes {
                Stripes::None => vec![],
                Stripes::Center => vec![(0.0, 0.0, model.size.x * 0.95, w * 0.2)],
                Stripes::Twin => [-1.0, 1.0].map(|s| (0.0, s * w * 0.12, model.size.x * 0.95, w * 0.08)).to_vec(),
                Stripes::Side => [-1.0, 1.0].map(|s| (-0.1 * hl.x, s * w * 0.36, model.size.x * 0.75, w * 0.07)).to_vec(),
            };
            for &(x, y, len, width) in &stripes {
                p.push(block(&Slot::Accent, x, y, top, Vec3::new(len, width, 0.5)));
            }

            // Cabin: tinted glass all round under a painted roof, with a
            // sloping windscreen in front and rear window behind.
            let (cx, chl, chw) = look.cabin;
            let (roof_x, roof_len, roof_w) = (cx * hl.x, 2.0 * chl * hl.x, 2.0 * chw * hl.y);
            p.push(block(&glass_dark, roof_x, 0.0, top, Vec3::new(roof_len, roof_w, pr.cabin_h - 1.0)));
            p.push(block(&Slot::PaintDark, roof_x, 0.0, roof_top - 1.6, Vec3::new(roof_len * 0.92, roof_w * 0.94, 1.6)));
            if look.stripes == Stripes::Center || look.stripes == Stripes::Twin {
                for &(_, y, _, width) in &stripes {
                    p.push(block(&Slot::Accent, roof_x, y, roof_top - 0.1, Vec3::new(roof_len * 0.92, width, 0.3)));
                }
            }
            let front = roof_x + roof_len / 2.0;
            let ws = 0.2 * hl.x + pr.cabin_h * 0.4;
            p.push(pane(&glass, Vec2::new(front, roof_top - 0.6), Vec2::new(front + ws, top + 0.2), roof_w * 0.9));
            if !look.cargo_bed {
                let back = roof_x - roof_len / 2.0;
                let rw = 0.12 * hl.x + pr.cabin_h * 0.3;
                p.push(pane(&glass_dark, Vec2::new(back, roof_top - 0.6), Vec2::new(back - rw, top + 0.2), roof_w * 0.8));
            }

            // Lights sit in the nose and tail.
            let nose_half = look.outline[0].1 * hl.y;
            let tail_half = look.outline[look.outline.len() - 1].1 * hl.y;
            let light_w = (nose_half * 0.45).min(6.0);
            for side in [-1.0, 1.0] {
                p.push(block(&headlight, hl.x - 1.4, side * nose_half * 0.55, top - 4.0, Vec3::new(3.0, light_w, 4.3)));
                p.push(block(&taillight, -hl.x + 1.4, side * tail_half * 0.65, top - 4.0, Vec3::new(3.0, tail_half * 0.4, 4.3)));
            }

            if look.spoiler {
                for side in [-1.0, 1.0] {
                    p.push(block(&trim, -hl.x + 3.0, side * w * 0.32, top, Vec3::new(2.0, 1.5, 5.0)));
                }
                p.push(block(&trim, -hl.x + 3.0, 0.0, top + 5.0, Vec3::new(5.0, w * 0.98, 1.2)));
            }
            if look.ram_plate {
                p.push(block(&steel, hl.x + 2.0, 0.0, pr.clearance * 0.5, Vec3::new(5.0, w * 1.08, pr.body_h * 0.95)));
                for side in [-1.0, 0.0, 1.0] {
                    p.push(block(&trim, hl.x + 4.6, side * w * 0.35, pr.clearance + pr.body_h * 0.3, Vec3::new(1.0, 2.0, 2.0)));
                }
            }
            p
        })
        .collect();

    CarArt { parts }
}

fn body_polygon(model: &CarModel) -> ConvexPolygon {
    let hl = model.size / 2.0;
    let top = model.look.outline.iter().map(|&(x, y)| Vec2::new(x * hl.x, y * hl.y));
    let bottom = model.look.outline.iter().rev().map(|&(x, y)| Vec2::new(x * hl.x, -y * hl.y));
    ConvexPolygon::new(top.chain(bottom).collect::<Vec<_>>())
        .unwrap_or_else(|e| panic!("{}: body outline is not convex: {e:?}", model.name))
}

fn paint_material(color: Color) -> StandardMaterial {
    StandardMaterial { base_color: color, perceptual_roughness: 0.4, reflectance: 0.45, ..default() }
}

/// Make a fresh set of paint materials for one car.
pub fn new_paint(materials: &mut Assets<StandardMaterial>, model: &CarModel, paint: Color) -> Paint {
    let dark = paint.mix(&Color::BLACK, 0.35);
    let accent = model.look.accent;
    Paint {
        handles: [paint, dark, accent].map(|c| materials.add(paint_material(c))),
        colors: [paint, dark, accent],
    }
}

/// Spawn a car's visual as children of `root`.
pub fn attach_car_visual(commands: &mut Commands, art: &CarArt, model_index: usize, paint: &Paint, root: Entity) {
    commands.entity(root).insert(Visibility::default()).with_children(|parent| {
        for part in &art.parts[model_index] {
            let material = match &part.slot {
                Slot::Fixed(h) => h.clone(),
                Slot::Paint => paint.handles[0].clone(),
                Slot::PaintDark => paint.handles[1].clone(),
                Slot::Accent => paint.handles[2].clone(),
            };
            let mut child = parent.spawn((Mesh3d(part.mesh.clone()), MeshMaterial3d(material), part.transform));
            if part.front_wheel {
                child.insert(FrontWheel);
            }
        }
    });
}

/// Like [`attach_car_visual`], but every material is the car's own copy, so
/// the car can be faded on its own (the garage does that). Returns them all.
pub fn attach_car_visual_owned(
    commands: &mut Commands,
    art: &CarArt,
    model_index: usize,
    paint: &Paint,
    root: Entity,
    materials: &mut Assets<StandardMaterial>,
) -> Vec<Handle<StandardMaterial>> {
    let mut own: Vec<(Handle<StandardMaterial>, Handle<StandardMaterial>)> = Vec::new();
    let mut all: Vec<Handle<StandardMaterial>> = paint.handles.to_vec();
    commands.entity(root).insert(Visibility::default()).with_children(|parent| {
        for part in &art.parts[model_index] {
            let material = match &part.slot {
                Slot::Fixed(h) => match own.iter().find(|(shared, _)| shared == h) {
                    Some((_, mine)) => mine.clone(),
                    None => {
                        let copy = materials.get(h).cloned().unwrap_or_default();
                        let mine = materials.add(copy);
                        own.push((h.clone(), mine.clone()));
                        all.push(mine.clone());
                        mine
                    }
                },
                Slot::Paint => paint.handles[0].clone(),
                Slot::PaintDark => paint.handles[1].clone(),
                Slot::Accent => paint.handles[2].clone(),
            };
            parent.spawn((Mesh3d(part.mesh.clone()), MeshMaterial3d(material), part.transform));
        }
    });
    all
}

/// Front wheels turn with the steering input.
pub fn steer_front_wheels(cars: Query<&Car>, mut wheels: Query<(&ChildOf, &mut Transform), With<FrontWheel>>) {
    for (child_of, mut tf) in &mut wheels {
        if let Ok(car) = cars.get(child_of.parent()) {
            let angle = if car.alive() { car.steer * 0.45 } else { 0.0 };
            tf.rotation = Quat::from_rotation_z(angle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rivals_come_from_the_players_tier() {
        for player in 0..MODELS.len() {
            let lineup = tier_lineup(player, 5);
            assert_eq!(lineup.len(), 6);
            assert_eq!(lineup[0], player);
            assert!(lineup.iter().all(|&m| MODELS[m].tier == MODELS[player].tier));
            // The tier's other model shows up first.
            assert_ne!(lineup[1], player, "{}", MODELS[player].name);
        }
    }

    #[test]
    fn every_car_in_a_lineup_looks_different() {
        for player in 0..MODELS.len() {
            let lineup = tier_lineup(player, 5);
            let looks: Vec<(usize, [u8; 4])> = lineup
                .iter()
                .zip(lineup_paints(&lineup))
                .map(|(&m, c)| (m, c.to_srgba().to_u8_array()))
                .collect();
            for (i, a) in looks.iter().enumerate() {
                assert!(!looks[i + 1..].contains(a), "duplicate look in {lineup:?}");
            }
        }
    }

    #[test]
    fn every_body_outline_is_convex() {
        for model in &MODELS {
            body_polygon(model);
        }
    }

    #[test]
    fn outlines_run_nose_to_tail_above_centre() {
        for model in &MODELS {
            let o = model.look.outline;
            assert!(o.iter().all(|&(_, y)| y > 0.0), "{}: outline must be y > 0", model.name);
            assert!(o[0].0 > o[o.len() - 1].0, "{}: outline must start at the nose", model.name);
        }
    }
}
