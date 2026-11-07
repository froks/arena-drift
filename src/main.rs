//! Arena Drift — a top-down demolition derby.
//!
//! Ram the other cars until you're the last one still running.
//! Front hits hurt the other car more than you; getting T-boned hurts most.
//! Power-ups spawn around the arena: health packs, guns, rockets, mines and turbo.

use bevy::prelude::*;

mod arena;
mod cars;
mod icons;
mod minimap;
mod physics;
mod sound;
mod terrain;

use std::collections::VecDeque;

use arena::{ARENAS, ArenaArt, ArenaDef, Explosive, Obstacle};
use bevy::window::PrimaryWindow;
use minimap::{Icon, MinimapPlugin};

use cars::{CarArt, CarModel, MODELS, Paint};
use icons::IconArt;
use physics::{Obb, cross};
use sound::{Sfx, SoundPlugin, Sounds};
use terrain::Terrain;

// ---------------------------------------------------------------------------
// Tuning
// ---------------------------------------------------------------------------

const BARREL_BLAST: f32 = 110.0;
const BARREL_DAMAGE: f32 = 38.0;
/// Most skid-mark segments kept at once; the oldest fade out first.
const MAX_SKIDS: usize = 1600;

// Shared by all models; per-model handling lives in `cars::MODELS`.
const BRAKE_FORCE: f32 = 900.0;
const REVERSE_FORCE: f32 = 300.0;
const ENGINE_BRAKE: f32 = 140.0;
const HANDBRAKE_GRIP: f32 = 1.2;
const WRECK_DRAG: f32 = 2.5;
const WRECK_MASS: f32 = 2.5; // wrecks are heavier, so they act like obstacles
const TURBO_MULTIPLIER: f32 = 1.9;

const RESTITUTION: f32 = 0.6;
/// Scales rotational inertia; higher = cars spin less from off-centre hits.
const INERTIA_SCALE: f32 = 2.0;
/// How quickly tyres kill collision-induced spin (per second).
const SPIN_DAMPING: f32 = 5.0;
const WRECK_SPIN_DAMPING: f32 = 2.0;
const DAMAGE_THRESHOLD: f32 = 70.0; // impact speed below this does no damage
const DAMAGE_SCALE: f32 = 0.16;
const WALL_DAMAGE_SCALE: f32 = 0.05;

const AI_CARS: usize = 5;

// Power-ups
const PICKUP_INTERVAL: f32 = 3.0;
const MAX_PICKUPS: usize = 5;
const PICKUP_RADIUS: f32 = 16.0;
const HEALTH_PACK: f32 = 40.0;
const TURBO_SECONDS: f32 = 4.0;

// Weapons
const GUN_AMMO: u32 = 40;
const GUN_COOLDOWN: f32 = 0.08;
const GUN_SPEED: f32 = 1100.0;
const GUN_DAMAGE: f32 = 2.5;

const ROCKET_AMMO: u32 = 3;
const ROCKET_COOLDOWN: f32 = 0.6;
const ROCKET_START_SPEED: f32 = 250.0;
const ROCKET_ACCEL: f32 = 1600.0;
const ROCKET_MAX_SPEED: f32 = 1100.0;
const ROCKET_DAMAGE: f32 = 40.0;
const ROCKET_BLAST: f32 = 90.0;

const MINE_AMMO: u32 = 3;
const MINE_COOLDOWN: f32 = 0.5;
const MINE_ARM_TIME: f32 = 0.8;
const MINE_DAMAGE: f32 = 45.0;
const MINE_BLAST: f32 = 100.0;

const BLAST_KNOCKBACK: f32 = 600.0;
/// How high above the ground bullets and rockets fly.
const SHOT_HEIGHT: f32 = 10.0;
/// How high above the ground power-ups hover.
const PICKUP_HOVER: f32 = 14.0;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum GameState {
    #[default]
    Garage,
    Playing,
    GameOver,
}

#[derive(Component)]
struct Car {
    model: &'static CarModel,
    heading: f32,
    vel: Vec2,
    /// Angular velocity from collisions (rad/s), on top of steering.
    spin: f32,
    hp: f32,
    throttle: f32,
    steer: f32,
    handbrake: bool,
    paint: Paint,
    /// Set once the wreck has been handled (sound, paint).
    wrecked: bool,
}

impl Car {
    fn new(model: &'static CarModel, heading: f32, paint: Paint) -> Self {
        Self {
            model,
            heading,
            vel: Vec2::ZERO,
            spin: 0.0,
            hp: model.max_hp,
            throttle: 0.0,
            steer: 0.0,
            handbrake: false,
            paint,
            wrecked: false,
        }
    }
    fn alive(&self) -> bool {
        self.hp > 0.0
    }
    fn forward(&self) -> Vec2 {
        Vec2::from_angle(self.heading)
    }
    fn obb(&self, pos: Vec2) -> Obb {
        Obb::new(pos, self.heading, self.model.size)
    }
    fn mass(&self) -> f32 {
        if self.alive() { self.model.mass } else { self.model.mass * WRECK_MASS }
    }
    /// Rotational inertia of a solid box.
    fn inertia(&self) -> f32 {
        let s = self.model.size;
        self.mass() * (s.x * s.x + s.y * s.y) / 12.0 * INERTIA_SCALE
    }
    /// Apply an impulse at `r` (offset from the car's centre).
    fn apply_impulse(&mut self, impulse: Vec2, r: Vec2) {
        self.vel += impulse / self.mass();
        self.spin += cross(r, impulse) / self.inertia();
    }
    fn point_velocity(&self, r: Vec2) -> Vec2 {
        self.vel + self.spin * r.perp()
    }
    fn health(&self) -> f32 {
        (self.hp / self.model.max_hp).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Weapon {
    None,
    Guns,
    Rockets,
    Mines,
}

impl Weapon {
    fn name(self) -> &'static str {
        match self {
            Weapon::None => "none",
            Weapon::Guns => "Machine gun",
            Weapon::Rockets => "Rockets",
            Weapon::Mines => "Mines",
        }
    }
}

/// What a car is carrying. Every car has one.
#[derive(Component)]
struct Armory {
    weapon: Weapon,
    ammo: u32,
    cooldown: f32,
    /// Set by the driver (player or AI) when they want to fire this frame.
    fire: bool,
    /// Seconds of turbo left.
    turbo: f32,
}

impl Default for Armory {
    fn default() -> Self {
        Self { weapon: Weapon::None, ammo: 0, cooldown: 0.0, fire: false, turbo: 0.0 }
    }
}

#[derive(Component)]
struct Player;

#[derive(Component)]
struct Ai {
    /// Seconds spent barely moving.
    stuck: f32,
    /// While > 0 the AI backs up to get unstuck.
    reversing: f32,
    /// Per-car personality: how much it prefers going after the player.
    player_bias: f32,
    aggression: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PickupKind {
    Health,
    Guns,
    Rockets,
    Mines,
    Turbo,
}

impl PickupKind {
    const ALL: [PickupKind; 5] = [
        PickupKind::Health,
        PickupKind::Guns,
        PickupKind::Rockets,
        PickupKind::Mines,
        PickupKind::Turbo,
    ];
    /// Relative spawn chance.
    fn weight(self) -> f32 {
        match self {
            PickupKind::Health => 3.0,
            PickupKind::Guns => 3.0,
            PickupKind::Rockets => 2.0,
            PickupKind::Mines => 1.5,
            PickupKind::Turbo => 1.5,
        }
    }
    fn color(self) -> Color {
        match self {
            PickupKind::Health => Color::srgb(0.2, 0.85, 0.3),
            PickupKind::Guns => Color::srgb(0.95, 0.85, 0.2),
            PickupKind::Rockets => Color::srgb(1.0, 0.5, 0.15),
            PickupKind::Mines => Color::srgb(0.9, 0.2, 0.25),
            PickupKind::Turbo => Color::srgb(0.2, 0.8, 1.0),
        }
    }
}

#[derive(Component)]
struct Pickup(PickupKind);

/// Height a power-up bobs around.
#[derive(Component)]
struct Hover(f32);

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProjectileKind {
    Bullet,
    Rocket,
}

#[derive(Component)]
struct Projectile {
    kind: ProjectileKind,
    owner: Entity,
    by_player: bool,
    vel: Vec2,
    life: f32,
}

#[derive(Component)]
struct Mine {
    by_player: bool,
    arm: f32,
}

#[derive(Component)]
struct Spark {
    vel: Vec2,
    life: f32,
}

#[derive(Component)]
struct Smoke {
    vel: Vec2,
    life: f32,
    max_life: f32,
}

#[derive(Component)]
struct Explosion {
    radius: f32,
    life: f32,
}

/// Everything that belongs to one round and gets wiped on restart.
#[derive(Component)]
struct RoundEntity;

#[derive(Component)]
struct Hud;

#[derive(Resource, Default)]
struct Score {
    damage_dealt: f32,
    wrecks: u32,
    won: bool,
}

#[derive(Resource, Default)]
struct PickupTimer(f32);

/// Index into `arena::ARENAS` of the arena to fight in.
#[derive(Resource, Default)]
struct CurrentArena(usize);

impl CurrentArena {
    fn def(&self) -> &'static ArenaDef {
        &ARENAS[self.0]
    }
}

#[derive(Resource, Default)]
struct SkidMarks(VecDeque<Entity>);

/// Index into `cars::MODELS` of the player's car.
#[derive(Resource, Default)]
struct SelectedCar(usize);

struct Blast {
    pos: Vec2,
    radius: f32,
    damage: f32,
    by_player: bool,
}

/// Balance testing: `ARENA_DRIFT_SIM=<rounds>` runs AI-only rounds at
/// high speed with every model in the field and prints who wins.
#[derive(Resource)]
struct Sim {
    /// `ARENA_DRIFT_SIM_TIER=<n>`: only that tier's models take part.
    tier: Option<u8>,
    rounds: u32,
    round: u32,
    wins: [u32; MODELS.len()],
    /// How many cars of each model have taken part, over all rounds.
    entries: [u32; MODELS.len()],
    draws: u32,
    clock: f32,
    total_time: f32,
}

impl Sim {
    /// Models for this round: all of them, or one tier repeated to fill the grid.
    fn lineup(&self) -> Vec<usize> {
        let pool: Vec<usize> = match self.tier {
            Some(t) => cars::tier_models(t),
            None => (0..MODELS.len()).collect(),
        };
        // Offsetting by the round rotates both starting spots and which
        // models get doubled up when the pool is smaller than the grid.
        let r = self.round as usize;
        (0..=AI_CARS).map(|i| pool[(i + r) % pool.len()]).collect()
    }
}

const SIM_SPEED: f32 = 3.0;
const SIM_ROUND_LIMIT: f32 = 120.0;

/// Explosions queued up this frame by rockets and mines.
#[derive(Resource, Default)]
struct PendingBlasts(Vec<Blast>);

/// Shared meshes and materials for effects.
#[derive(Resource)]
struct Art {
    spark_mesh: Handle<Mesh>,
    spark_mat: Handle<StandardMaterial>,
    exhaust_mat: Handle<StandardMaterial>,
    bullet_mesh: Handle<Mesh>,
    bullet_mat: Handle<StandardMaterial>,
    blast_mesh: Handle<Mesh>,
    blast_mat: Handle<StandardMaterial>,
    smoke_mesh: Handle<Mesh>,
    smoke_mat: Handle<StandardMaterial>,
    square: Handle<Mesh>,
    skid_mat: Handle<StandardMaterial>,
}

/// Tiny xorshift RNG so we don't need an extra crate.
#[derive(Resource)]
struct Rng(u32);

impl Rng {
    fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32) / (u32::MAX as f32)
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

fn main() {
    let sim_tier = std::env::var("ARENA_DRIFT_SIM_TIER").ok().and_then(|v| v.parse::<u8>().ok());
    let sim = std::env::var("ARENA_DRIFT_SIM").ok().and_then(|v| v.parse::<u32>().ok()).map(|rounds| Sim {
        tier: sim_tier,
        rounds: rounds.max(1),
        round: 0,
        wins: [0; MODELS.len()],
        entries: [0; MODELS.len()],
        draws: 0,
        clock: 0.0,
        total_time: 0.0,
    });
    let arena = std::env::var("ARENA_DRIFT_ARENA").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    let autostart = sim.is_some() || std::env::var_os("ARENA_DRIFT_AUTOSTART").is_some();

    let mut app = App::new();
    if let Some(sim) = sim {
        app.insert_resource(sim).add_systems(
            Startup,
            |mut time: ResMut<Time<Virtual>>| time.set_relative_speed(SIM_SPEED),
        );
    }
    app
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Arena Drift".into(),
                resolution: (1280, 800).into(),
                // In the browser: draw into <canvas id="bevy"> and fill the page.
                canvas: Some("#bevy".into()),
                fit_canvas_to_parent: true,
                // Keep keys like Tab and Space from scrolling or moving focus.
                prevent_default_event_handling: true,
                ..default()
            }),
            ..default()
        }))
        .add_plugins((SoundPlugin, MinimapPlugin))
        .insert_resource(CurrentArena(arena.min(ARENAS.len() - 1)))
        .init_resource::<SkidMarks>()
        .insert_resource(ClearColor(Color::srgb(0.08, 0.08, 0.1)))
        .insert_resource(Rng(0x9E37_79B9))
        .init_resource::<Score>()
        .init_resource::<PickupTimer>()
        .init_resource::<PendingBlasts>()
        .init_resource::<SelectedCar>()
        .init_resource::<Terrain>()
        .insert_resource(bevy::light::DirectionalLightShadowMap {
            size: if cfg!(target_arch = "wasm32") { 2048 } else { 4096 },
        })
        // Dev shortcut: ARENA_DRIFT_AUTOSTART=1 skips the garage.
        .insert_state(if autostart { GameState::Playing } else { GameState::Garage })
        .init_resource::<CarArt>()
        .init_resource::<IconArt>()
        .init_resource::<ArenaArt>()
        .add_systems(Startup, setup_world)
        .add_systems(Update, dev_screenshot)
        .add_systems(OnEnter(GameState::Garage), setup_garage)
        .add_systems(Update, (garage_input, garage_animate).chain().run_if(in_state(GameState::Garage)))
        .add_systems(OnEnter(GameState::Playing), start_round)
        .add_systems(
            Update,
            (
                (
                    player_input,
                    ai_drive,
                    car_physics,
                    static_collisions,
                    car_collisions,
                    follow_terrain,
                    skid_marks,
                )
                    .chain(),
                (
                    fire_weapons,
                    move_projectiles,
                    update_mines,
                    detonate_barrels,
                    resolve_blasts,
                    collect_pickups,
                    spawn_pickups,
                    update_car_visuals,
                    update_hud,
                    check_round_end,
                )
                    .chain(),
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        )
        .add_systems(
            Update,
            (
                update_sparks,
                update_smoke,
                update_explosions,
                spin_pickups,
                icons::pulse_pickup_glow,
                cars::steer_front_wheels,
                arena::fade_canopies,
                draw_health_bars,
                camera_follow.after(follow_terrain),
            ),
        )
        .add_systems(OnEnter(GameState::GameOver), show_game_over)
        .add_systems(Update, game_over_input.run_if(in_state(GameState::GameOver)))
        .run();
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

/// Dev aid: `ARENA_DRIFT_SCREENSHOT=out.png:6` saves the game window to
/// `out.png` after 6 seconds (wall-clock), once.
fn dev_screenshot(mut commands: Commands, time: Res<Time<Real>>, mut done: Local<bool>) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    if *done {
        return;
    }
    let Some((path, secs)) = std::env::var("ARENA_DRIFT_SCREENSHOT")
        .ok()
        .and_then(|v| v.rsplit_once(':').map(|(p, s)| (p.to_string(), s.parse::<f32>().unwrap_or(5.0))))
    else {
        *done = true;
        return;
    };
    if time.elapsed_secs() >= secs {
        *done = true;
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
}

/// Vertical field of view. Wider means more parallax on tall things.
const CAMERA_FOV: f32 = 0.7;

/// Camera height that shows `visible` world units from top to bottom of the
/// screen at ground level.
fn camera_height(visible: f32) -> f32 {
    visible / 2.0 / (CAMERA_FOV / 2.0).tan()
}

fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Straight down at the ground (Z is up, +Y is up the screen). The
    // perspective makes tall things lean out from the middle of the screen.
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { fov: CAMERA_FOV, near: 20.0, far: 20_000.0, ..default() }),
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        bevy::core_pipeline::tonemapping::DebandDither::Disabled,
        Transform::from_xyz(0.0, 24.0, camera_height(800.0)).looking_to(Vec3::NEG_Z, Vec3::Y),
        AmbientLight { color: Color::srgb(0.85, 0.88, 1.0), brightness: 380.0, ..default() },
    ));
    // Low afternoon sun from the upper left: shadows fall down and to the right.
    commands.spawn((
        DirectionalLight { illuminance: 3400.0, shadow_maps_enabled: true, ..default() },
        Transform::default().looking_to(Vec3::new(0.6, -0.7, -1.0), Vec3::Z),
        shadow_cascades(camera_height(800.0)),
    ));

    let glow = |c: Color| StandardMaterial {
        base_color: c,
        unlit: true,
        alpha_mode: if c.alpha() < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
        ..default()
    };
    commands.insert_resource(Art {
        spark_mesh: meshes.add(Cuboid::new(4.0, 4.0, 4.0)),
        spark_mat: materials.add(glow(Color::srgb(1.0, 0.8, 0.3))),
        exhaust_mat: materials.add(glow(Color::srgb(0.4, 0.8, 1.0))),
        bullet_mesh: meshes.add(Cuboid::new(9.0, 3.0, 3.0)),
        bullet_mat: materials.add(glow(Color::srgb(1.0, 0.95, 0.5))),
        blast_mesh: meshes.add(Sphere::new(1.0).mesh().ico(3).expect("valid icosphere")),
        blast_mat: materials.add(glow(Color::srgba(1.0, 0.55, 0.15, 0.7))),
        smoke_mesh: meshes.add(Sphere::new(1.0).mesh().ico(2).expect("valid icosphere")),
        smoke_mat: materials.add(StandardMaterial {
            base_color: Color::srgba(0.35, 0.35, 0.37, 0.45),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 1.0,
            ..default()
        }),
        square: meshes.add(Rectangle::new(1.0, 1.0)),
        skid_mat: materials.add(StandardMaterial {
            depth_bias: 50.0,
            ..glow(Color::srgba(0.05, 0.05, 0.05, 0.35))
        }),
    });
}

/// Shadows only need to reach the ground, which is about `height` below the camera.
fn shadow_cascades(height: f32) -> bevy::light::CascadeShadowConfig {
    bevy::light::CascadeShadowConfigBuilder {
        num_cascades: 1,
        minimum_distance: 1.0,
        maximum_distance: height * 1.5 + 400.0,
        first_cascade_far_bound: height * 1.5 + 400.0,
        overlap_proportion: 0.2,
    }
    .build()
}

#[allow(clippy::too_many_arguments)]
fn start_round(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut terrain: ResMut<Terrain>,
    car_art: Res<CarArt>,
    selected: Res<SelectedCar>,
    mut score: ResMut<Score>,
    mut pickup_timer: ResMut<PickupTimer>,
    mut blasts: ResMut<PendingBlasts>,
    mut arena_art: ResMut<ArenaArt>,
    mut skids: ResMut<SkidMarks>,
    current: Res<CurrentArena>,
    sim: Option<Res<Sim>>,
    old: Query<Entity, With<RoundEntity>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    skids.0.clear();
    *score = Score::default();
    let def = current.def();
    let (obstacles, ground) =
        arena::spawn_arena(&mut commands, &mut arena_art, &mut meshes, &mut images, &mut materials, def);
    *terrain = ground;
    minimap::spawn_minimap(&mut commands, def.size, RoundEntity);
    // The player's car first, then rivals from the same tier.
    let lineup = match &sim {
        Some(sim) => sim.lineup(),
        None => cars::tier_lineup(selected.0, AI_CARS),
    };
    let paints = cars::lineup_paints(&lineup);
    pickup_timer.0 = 1.0; // first power-up shows up quickly
    blasts.0.clear();

    // Fair starting grid: open road ahead of every car, no barrels next to anyone.
    let total = lineup.len();
    let (grid, _) = arena::spawn_points(&obstacles, def.size, total);
    for (i, ((&model_index, &color), &(pos, heading))) in lineup.iter().zip(&paints).zip(&grid).enumerate() {
        let model = &MODELS[model_index];
        let paint = cars::new_paint(&mut materials, model, color);

        let root = commands
            .spawn((
                RoundEntity,
                Armory::default(),
                Transform::from_translation(pos.extend(terrain.height(pos)))
                    .with_rotation(terrain.resting(pos, model.size / 2.0, heading)),
            ))
            .id();
        cars::attach_car_visual(&mut commands, &car_art, model_index, &paint, root);
        commands.entity(root).insert(Car::new(model, heading, paint));
        let is_player = i == 0 && sim.is_none();
        commands.entity(root).insert(Icon {
            color: if is_player { Color::WHITE } else { color },
            size: Vec2::splat(if is_player { 110.0 } else { 80.0 }),
            layer: 3,
        });

        if i == 0 && sim.is_none() {
            commands.entity(root).insert(Player);
        } else if sim.is_some() {
            commands.entity(root).insert(Ai { stuck: 0.0, reversing: 0.0, player_bias: 1.0, aggression: 0.9 });
        } else {
            let t = i as f32 / AI_CARS as f32;
            commands.entity(root).insert(Ai {
                stuck: 0.0,
                reversing: 0.0,
                player_bias: 0.6 + 0.3 * t,
                aggression: 0.75 + 0.25 * (1.0 - t),
            });
        }
    }

    if sim.is_some() {
        return; // no player, no HUD
    }
    commands.spawn((
        RoundEntity,
        Hud,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(22.0),
            ..default()
        },
        TextColor(Color::WHITE),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            left: px(12),
            padding: UiRect::axes(px(8), px(4)),
            ..default()
        },
    ));
}

// ---------------------------------------------------------------------------
// Garage (car selection)
// ---------------------------------------------------------------------------

#[derive(Component)]
struct GarageCar(usize);

#[derive(Component)]
struct GarageInfo;

const GARAGE_SPACING: f32 = 260.0;
const GARAGE_Y: f32 = 170.0;

/// Where a model sits in the garage row of its tier.
fn garage_slot(model: usize) -> f32 {
    let row = cars::tier_models(MODELS[model].tier);
    let k = row.iter().position(|&m| m == model).unwrap_or(0) as f32;
    (k - (row.len() as f32 - 1.0) / 2.0) * GARAGE_SPACING
}

fn setup_garage(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut terrain: ResMut<Terrain>,
    car_art: Res<CarArt>,
    old: Query<Entity, With<RoundEntity>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    *terrain = Terrain::default();

    // Showroom floor: a gold-trimmed plinth.
    for (size, color, top) in [
        (Vec3::new(1232.0, 712.0, 20.0), Color::srgb(0.75, 0.6, 0.15), -1.0),
        (Vec3::new(1200.0, 680.0, 20.0), Color::srgb(0.22, 0.19, 0.16), 0.0),
    ] {
        commands.spawn((
            DespawnOnExit(GameState::Garage),
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(materials.add(arena::surface(color, false))),
            Transform::from_xyz(0.0, 0.0, top - size.z / 2.0),
        ));
    }

    for (i, model) in MODELS.iter().enumerate() {
        let x = garage_slot(i);
        let root = commands
            .spawn((
                GarageCar(i),
                DespawnOnExit(GameState::Garage),
                Transform::from_xyz(x, GARAGE_Y, 0.0)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
                    .with_scale(Vec3::splat(2.0)),
            ))
            .id();
        let paint = cars::new_paint(&mut materials, model, model.look.paint);
        cars::attach_car_visual(&mut commands, &car_art, i, &paint, root);
    }

    commands.spawn((
        DespawnOnExit(GameState::Garage),
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::vertical(px(4)),
            ..default()
        },
        children![
            (
                Text::new("CHOOSE YOUR RIDE"),
                TextFont { font_size: FontSize::Px(40.0), ..default() },
                TextColor(Color::srgb(0.95, 0.8, 0.3)),
            ),
            (
                GarageInfo,
                Text::new(""),
                TextFont { font_size: FontSize::Px(22.0), ..default() },
                TextColor(Color::WHITE),
                Node { margin: UiRect::bottom(px(30)), ..default() },
            ),
        ],
    ));
}

/// A text bar like `######----` scaled between the weakest and strongest model.
fn stat_bar(value: f32, stat: impl Fn(&CarModel) -> f32) -> String {
    let (lo, hi) = MODELS
        .iter()
        .map(&stat)
        .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
    let filled = 2 + ((value - lo) / (hi - lo).max(1e-6) * 8.0).round() as usize;
    format!("{}{}", "#".repeat(filled), "-".repeat(10 - filled))
}

fn garage_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut selected: ResMut<SelectedCar>,
    mut current: ResMut<CurrentArena>,
    mut next: ResMut<NextState<GameState>>,
    mut info: Query<&mut Text, With<GarageInfo>>,
) {
    if keys.just_pressed(KeyCode::Tab) {
        current.0 = (current.0 + 1) % ARENAS.len();
    }
    // Left/Right: car within the tier. Up/Down: tier (keeping the column).
    let tiers = cars::tiers();
    let row = cars::tier_models(MODELS[selected.0].tier);
    let col = row.iter().position(|&m| m == selected.0).unwrap_or(0);
    let tier_pos = tiers.iter().position(|&t| t == MODELS[selected.0].tier).unwrap_or(0);
    if keys.any_just_pressed([KeyCode::ArrowLeft, KeyCode::KeyA]) {
        selected.0 = row[(col + row.len() - 1) % row.len()];
    }
    if keys.any_just_pressed([KeyCode::ArrowRight, KeyCode::KeyD]) {
        selected.0 = row[(col + 1) % row.len()];
    }
    let new_tier = if keys.any_just_pressed([KeyCode::ArrowUp, KeyCode::KeyW]) {
        Some((tier_pos + 1).min(tiers.len() - 1))
    } else if keys.any_just_pressed([KeyCode::ArrowDown, KeyCode::KeyS]) {
        Some(tier_pos.saturating_sub(1))
    } else {
        None
    };
    if let Some(t) = new_tier {
        let row = cars::tier_models(tiers[t]);
        selected.0 = row[col.min(row.len() - 1)];
    }
    if keys.just_pressed(KeyCode::Enter) {
        next.set(GameState::Playing);
    }

    let Ok(mut text) = info.single_mut() else { return };
    let m = &MODELS[selected.0];
    let handling = |m: &CarModel| m.turn_rate * 0.6 + m.grip * 0.4;
    let toughness = |m: &CarModel| m.max_hp * m.armor;
    text.0 = format!(
        "TIER {}   <  {}  >\n{}\nRivals: {}\n\nArena: {} ({:?})  -  Tab to change\n{}\n\n\
         Top speed  {}\n\
         Accel      {}\n\
         Handling   {}\n\
         Toughness  {}\n\
         Weight     {}\n\n\
         Up/Down tier  |  Left/Right car  |  Enter race",
        m.tier,
        m.name.to_uppercase(),
        m.blurb,
        cars::tier_models(m.tier).iter().map(|&i| MODELS[i].name).collect::<Vec<_>>().join(", "),
        current.def().name.to_uppercase(),
        current.def().mode,
        current.def().blurb,
        stat_bar(m.top_speed, |m| m.top_speed),
        stat_bar(m.engine, |m| m.engine),
        stat_bar(handling(m), handling),
        stat_bar(toughness(m), toughness),
        stat_bar(m.mass, |m| m.mass),
    );
}

fn garage_animate(
    time: Res<Time>,
    selected: Res<SelectedCar>,
    mut gizmos: Gizmos,
    mut q: Query<(&GarageCar, &mut Transform, &mut Visibility)>,
) {
    let t = time.elapsed_secs();
    let tier = MODELS[selected.0].tier;
    for (car, mut tf, mut vis) in &mut q {
        // Only the selected tier's row is on show.
        *vis = if MODELS[car.0].tier == tier { Visibility::Inherited } else { Visibility::Hidden };
        let up = std::f32::consts::FRAC_PI_2;
        if car.0 == selected.0 {
            tf.scale = Vec3::splat(2.5);
            tf.rotation = Quat::from_rotation_z(up + (t * 1.5).sin() * 0.25);
            gizmos.rect(
                Isometry3d::from_translation(tf.translation.truncate().extend(1.0)),
                Vec2::new(GARAGE_SPACING - 40.0, 210.0),
                Color::srgb(0.95, 0.8, 0.3),
            );
        } else {
            tf.scale = Vec3::splat(2.0);
            tf.rotation = Quat::from_rotation_z(up);
        }
    }
}

// ---------------------------------------------------------------------------
// Driving
// ---------------------------------------------------------------------------

fn player_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut q: Query<(&mut Car, &mut Armory), With<Player>>,
) {
    let Ok((mut car, mut armory)) = q.single_mut() else { return };
    if !car.alive() {
        armory.fire = false;
        return;
    }
    let pressed = |a: KeyCode, b: KeyCode| keys.pressed(a) || keys.pressed(b);

    let mut throttle = 0.0;
    if pressed(KeyCode::KeyW, KeyCode::ArrowUp) {
        throttle += 1.0;
    }
    if pressed(KeyCode::KeyS, KeyCode::ArrowDown) {
        throttle -= 1.0;
    }
    let mut steer = 0.0;
    if pressed(KeyCode::KeyA, KeyCode::ArrowLeft) {
        steer += 1.0;
    }
    if pressed(KeyCode::KeyD, KeyCode::ArrowRight) {
        steer -= 1.0;
    }
    car.throttle = throttle;
    car.steer = steer;
    car.handbrake = keys.pressed(KeyCode::Space);
    // Left Ctrl fires only in native builds: in the browser, Ctrl+W (with W held for
    // throttle) closes the tab, and pages are not allowed to prevent that shortcut.
    armory.fire = keys.any_pressed([KeyCode::KeyF, KeyCode::KeyC, KeyCode::KeyV])
        || (cfg!(not(target_arch = "wasm32")) && keys.pressed(KeyCode::ControlLeft));
}

fn ai_drive(
    time: Res<Time>,
    mut ais: Query<(Entity, &Transform, &mut Car, &mut Ai, &mut Armory)>,
    targets: Query<(Entity, &Transform, &Car, Has<Player>), Without<Ai>>,
    pickups: Query<(&Transform, &Pickup)>,
    obstacles: Query<&Obstacle>,
) {
    let dt = time.delta_secs();

    // Snapshot every live car so AIs can target each other too.
    let mut live: Vec<(Entity, Vec2, Vec2, bool)> = targets
        .iter()
        .filter(|(_, _, c, _)| c.alive())
        .map(|(e, t, c, p)| (e, t.translation.truncate(), c.vel, p))
        .collect();
    live.extend(
        ais.iter()
            .filter(|(_, _, c, _, _)| c.alive())
            .map(|(e, t, c, _, _)| (e, t.translation.truncate(), c.vel, false)),
    );
    let pickups: Vec<(Vec2, PickupKind)> = pickups
        .iter()
        .map(|(t, p)| (t.translation.truncate(), p.0))
        .collect();

    for (me, tf, mut car, mut ai, mut armory) in &mut ais {
        armory.fire = false;
        if !car.alive() {
            car.throttle = 0.0;
            car.steer = 0.0;
            continue;
        }
        let pos = tf.translation.truncate();
        let fwd = car.forward();

        // Pick the "cheapest" target; the player looks closer than they are.
        let target = live
            .iter()
            .filter(|(e, ..)| *e != me)
            .min_by(|a, b| {
                let cost = |t: &(Entity, Vec2, Vec2, bool)| {
                    let d = t.1.distance(pos);
                    if t.3 { d * ai.player_bias } else { d }
                };
                cost(a).total_cmp(&cost(b))
            });
        let Some(&(_, tpos, tvel, _)) = target else {
            car.throttle = 0.0;
            continue;
        };

        // Get unstuck: if barely moving for a while, back up and turn.
        let speed = car.vel.length();
        if ai.reversing > 0.0 {
            ai.reversing -= dt;
            car.throttle = -1.0;
            car.steer = 1.0;
            car.handbrake = false;
            continue;
        }
        if speed < 25.0 {
            ai.stuck += dt;
        } else {
            ai.stuck = 0.0;
        }
        if ai.stuck > 1.2 {
            ai.stuck = 0.0;
            ai.reversing = 0.7;
        }

        // Go for a power-up if it's worth the detour, otherwise attack.
        let nearest_pickup = |want: &dyn Fn(PickupKind) -> bool, max: f32| {
            pickups
                .iter()
                .filter(|(_, k)| want(*k))
                .map(|(p, _)| *p)
                .filter(|p| p.distance(pos) < max)
                .min_by(|a, b| a.distance(pos).total_cmp(&b.distance(pos)))
        };
        let goal = if car.health() < 0.45 {
            nearest_pickup(&|k| k == PickupKind::Health, f32::INFINITY)
        } else if armory.weapon == Weapon::None {
            nearest_pickup(&|k| k != PickupKind::Health, 380.0)
        } else {
            None
        };
        let aim = goal.unwrap_or_else(|| {
            // Aim a bit ahead of where the target is going.
            let lead = (tpos.distance(pos) / 400.0).min(0.6);
            tpos + tvel * lead
        });
        let diff = fwd.angle_to(aim - pos);

        car.steer = (diff * 2.5).clamp(-1.0, 1.0);
        car.throttle = if diff.abs() < 1.2 { ai.aggression } else { 0.45 };
        car.handbrake = diff.abs() > 1.8 && speed > 150.0;

        // Feelers: if something is in the way (and it isn't the target),
        // steer towards whichever side is clear.
        let look = 70.0 + speed * 0.35;
        if goal.is_some() || tpos.distance(pos) > look {
            let blocked = |dir: Vec2| {
                obstacles
                    .iter()
                    .any(|o| o.distance(pos + dir * look) < 16.0 || o.distance(pos + dir * look * 0.5) < 16.0)
            };
            if blocked(fwd) {
                let left = fwd.rotate(Vec2::from_angle(0.6));
                let right = fwd.rotate(Vec2::from_angle(-0.6));
                car.steer = match (blocked(left), blocked(right)) {
                    (false, true) => 1.0,
                    (true, false) => -1.0,
                    (false, false) => if diff >= 0.0 { 1.0 } else { -1.0 },
                    (true, true) => car.steer,
                };
                car.throttle = car.throttle.min(0.6);
            }
        }

        // Shoot when lined up.
        let to_target = tpos - pos;
        let dist = to_target.length();
        let off = fwd.angle_to(to_target).abs();
        armory.fire = match armory.weapon {
            Weapon::None => false,
            Weapon::Guns => dist < 550.0 && off < 0.25,
            Weapon::Rockets => dist < 800.0 && dist > 80.0 && off < 0.12,
            Weapon::Mines => live.iter().any(|(e, p, _, _)| {
                let rel = *p - pos;
                *e != me && rel.length() < 220.0 && fwd.dot(rel.normalize_or_zero()) < -0.6
            }),
        };
    }
}

fn car_physics(time: Res<Time>, terrain: Res<Terrain>, mut q: Query<(&mut Transform, &mut Car, &mut Armory)>) {
    let dt = time.delta_secs();
    for (mut tf, mut car, mut armory) in &mut q {
        let m = car.model;
        // Slopes pull everything downhill: valleys collect wrecks.
        let downhill = -terrain.gradient(tf.translation.truncate()) * terrain::GRAVITY;
        car.vel += downhill * dt;
        let fwd = car.forward();
        let right = fwd.perp();
        let mut v_fwd = car.vel.dot(fwd);
        let mut v_side = car.vel.dot(right);
        let turbo = armory.turbo > 0.0;
        armory.turbo = (armory.turbo - dt).max(0.0);

        if car.alive() {
            // Engine / brake / reverse.
            let engine = if turbo { m.engine * TURBO_MULTIPLIER } else { m.engine };
            let accel = match car.throttle {
                t if t > 0.0 => t * engine,
                t if t < 0.0 && v_fwd > 20.0 => t * BRAKE_FORCE,
                t => t * REVERSE_FORCE,
            };
            v_fwd += accel * dt;
            // Off the throttle the engine holds the car back, so it doesn't
            // roll away down gentle slopes.
            if car.throttle == 0.0 {
                v_fwd -= v_fwd.signum() * (ENGINE_BRAKE * dt).min(v_fwd.abs());
            }

            // Steering only works while rolling; flips when reversing.
            let roll = (v_fwd / 180.0).clamp(-1.0, 1.0);
            car.heading += car.steer * m.turn_rate * roll * dt;
            car.spin *= (-SPIN_DAMPING * dt).exp();

            v_fwd *= (-m.drag() * dt).exp();
            let grip = if car.handbrake { HANDBRAKE_GRIP } else { m.grip };
            v_side *= (-grip * dt).exp();
            car.vel = fwd * v_fwd + right * v_side;
        } else {
            car.vel *= (-WRECK_DRAG * dt).exp();
            car.spin *= (-WRECK_SPIN_DAMPING * dt).exp();
        }
        car.heading += car.spin * dt;

        tf.translation += (car.vel * dt).extend(0.0);
    }
}

/// Sit every car on the ground under it, pitched and rolled with the slope.
fn follow_terrain(terrain: Res<Terrain>, mut q: Query<(&mut Transform, &Car)>) {
    for (mut tf, car) in &mut q {
        let pos = tf.translation.truncate();
        tf.translation.z = terrain.height(pos);
        tf.rotation = terrain.resting(pos, car.model.size / 2.0, car.heading);
    }
}

// ---------------------------------------------------------------------------
// Collisions & damage
// ---------------------------------------------------------------------------

/// Apply damage (reduced by armour) and, if the player caused it, credit them.
fn hurt(car: &mut Car, amount: f32, credit_player: bool, score: &mut Score) {
    if !car.alive() {
        return;
    }
    let amount = amount / car.model.armor;
    car.hp -= amount;
    if credit_player {
        score.damage_dealt += amount;
        if !car.alive() {
            score.wrecks += 1;
        }
    }
}

/// Cars against walls and obstacles (which never move).
fn static_collisions(
    mut commands: Commands,
    art: Res<Art>,
    mut rng: ResMut<Rng>,
    mut sounds: ResMut<Sounds>,
    mut cars: Query<(&mut Transform, &mut Car, Has<Player>)>,
    mut obstacles: Query<(&Obstacle, Option<&mut Explosive>)>,
) {
    for (mut tf, mut car, is_player) in &mut cars {
        let reach = car.model.size.length() / 2.0;
        // (impact speed, damage factor, contact point, outward normal)
        let mut worst: Option<(f32, f32, Vec2, Vec2)> = None;
        for (o, explosive) in &mut obstacles {
            let pos = tf.translation.truncate();
            if o.pos.distance(pos) > o.bounding_radius() + reach {
                continue;
            }
            let Some(c) = o.collide_car(&car.obb(pos)) else { continue };
            let n = -c.normal; // obstacle -> car
            tf.translation += (n * c.depth).extend(0.0);
            let r = c.point - pos;
            let vn = car.point_velocity(r).dot(n);
            if vn >= 0.0 {
                continue;
            }
            // Impulse at the contact: clipping something with a corner spins you.
            let rn = cross(r, n);
            let j = -(1.0 + o.kind.restitution()) * vn / (1.0 / car.mass() + rn * rn / car.inertia());
            car.apply_impulse(n * j, r);

            if let Some(mut barrel) = explosive {
                barrel.hp -= (-vn - 60.0).max(0.0) * 0.08;
                barrel.by_player |= is_player;
            }
            if worst.is_none_or(|(i, ..)| -vn > i) {
                worst = Some((-vn, o.kind.damage(), c.point, n));
            }
        }

        let Some((impact, factor, point, normal)) = worst else { continue };
        if impact > 60.0 {
            let soft = 0.4 + 0.6 * factor.min(1.0);
            sounds.play(Sfx::Crash, (impact / 600.0) * soft * if is_player { 1.0 } else { 0.5 });
        }
        if impact > DAMAGE_THRESHOLD * 1.5 && car.alive() {
            let dmg = (impact - DAMAGE_THRESHOLD * 1.5) * WALL_DAMAGE_SCALE * factor / car.model.armor;
            car.hp -= dmg;
            spawn_sparks(&mut commands, &art, &mut rng, point.extend(tf.translation.z + 8.0), normal.perp(), impact * factor.min(1.0));
        }
    }
}

/// Dark streaks behind the rear wheels when a car slides.
fn skid_marks(
    mut commands: Commands,
    time: Res<Time>,
    art: Res<Art>,
    terrain: Res<Terrain>,
    mut skids: ResMut<SkidMarks>,
    cars: Query<(&Transform, &Car)>,
) {
    let dt = time.delta_secs();
    for (tf, car) in &cars {
        let speed = car.vel.length();
        let fwd = car.forward();
        let slide = car.vel.dot(fwd.perp()).abs();
        if !car.alive() || !(slide > 110.0 || (car.handbrake && speed > 80.0)) {
            continue;
        }
        let m = car.model;
        let hl = m.size / 2.0;
        let rear = m.look.axles[m.look.axles.len() - 1] * hl.x;
        let pos = tf.translation.truncate();
        for side in [-1.0, 1.0] {
            let at = pos + fwd * rear + fwd.perp() * side * (hl.y - m.look.wheel.y * 0.2);
            let ground = Quat::from_rotation_arc(Vec3::Z, terrain.normal(at));
            let mark = commands
                .spawn((
                    RoundEntity,
                    Mesh3d(art.square.clone()),
                    MeshMaterial3d(art.skid_mat.clone()),
                    Transform::from_translation(at.extend(terrain.height(at) + 0.4))
                        .with_rotation(ground * Quat::from_rotation_z(car.vel.to_angle()))
                        .with_scale(Vec3::new(speed * dt + 2.0, m.look.wheel.y * 0.8, 1.0)),
                ))
                .id();
            skids.0.push_back(mark);
        }
    }
    while skids.0.len() > MAX_SKIDS {
        if let Some(old) = skids.0.pop_front()
            && let Ok(mut e) = commands.get_entity(old)
        {
            e.despawn();
        }
    }
}

fn car_collisions(
    mut commands: Commands,
    art: Res<Art>,
    mut rng: ResMut<Rng>,
    mut score: ResMut<Score>,
    mut sounds: ResMut<Sounds>,
    mut q: Query<(&mut Transform, &mut Car, Has<Player>)>,
) {
    let mut combos = q.iter_combinations_mut();
    while let Some([(mut ta, mut a, a_player), (mut tb, mut b, b_player)]) = combos.fetch_next() {
        let (pa, pb) = (ta.translation.truncate(), tb.translation.truncate());
        let Some(contact) = a.obb(pa).collide(&b.obb(pb)) else { continue };
        let n = contact.normal;

        // Separate the boxes; the lighter one gets moved more.
        let (ma, mb) = (a.mass(), b.mass());
        ta.translation -= (n * contact.depth * mb / (ma + mb)).extend(0.0);
        tb.translation += (n * contact.depth * ma / (ma + mb)).extend(0.0);

        // Relative velocity at the contact point, including spin.
        let (ra, rb) = (contact.point - pa, contact.point - pb);
        let closing = (a.point_velocity(ra) - b.point_velocity(rb)).dot(n);
        if closing <= 0.0 {
            continue;
        }

        // Rigid-body impulse: off-centre hits turn into spin instead of push.
        let (rna, rnb) = (cross(ra, n), cross(rb, n));
        let k = 1.0 / ma + 1.0 / mb + rna * rna / a.inertia() + rnb * rnb / b.inertia();
        let j = (1.0 + RESTITUTION) * closing / k;
        a.apply_impulse(-n * j, ra);
        b.apply_impulse(n * j, rb);

        if closing > 40.0 {
            let near = if a_player || b_player { 1.0 } else { 0.55 };
            sounds.play(Sfx::Crash, (0.15 + closing / 500.0) * near);
        }

        // Damage follows how hard each car got jolted, so heavy cars shrug
        // off hits from light ones. Normalised so a centred hit between equal
        // masses behaves as "closing speed".
        let jolt = |m: f32| (j / m) / ((1.0 + RESTITUTION) / 2.0);
        let dmg = |m: f32| (jolt(m) - DAMAGE_THRESHOLD).max(0.0) * DAMAGE_SCALE;
        // Hitting with your nose is good, taking it in the side is bad.
        let a_front = a.forward().dot(n).max(0.0);
        let b_front = b.forward().dot(-n).max(0.0);
        let (dmg_a, dmg_b) = (dmg(ma) * (1.25 - 0.9 * a_front), dmg(mb) * (1.25 - 0.9 * b_front));
        if dmg_a + dmg_b <= 0.0 {
            continue;
        }
        hurt(&mut a, dmg_a, b_player, &mut score);
        hurt(&mut b, dmg_b, a_player, &mut score);

        let z = (ta.translation.z + tb.translation.z) / 2.0 + 8.0;
        spawn_sparks(&mut commands, &art, &mut rng, contact.point.extend(z), n.perp(), closing);
    }
}

// ---------------------------------------------------------------------------
// Weapons
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn fire_weapons(
    mut commands: Commands,
    time: Res<Time>,
    art: Res<Art>,
    terrain: Res<Terrain>,
    icon_art: Res<IconArt>,
    mut rng: ResMut<Rng>,
    mut sounds: ResMut<Sounds>,
    mut q: Query<(Entity, &Transform, &Car, &mut Armory, Has<Player>)>,
) {
    let dt = time.delta_secs();
    for (e, tf, car, mut armory, is_player) in &mut q {
        armory.cooldown -= dt;
        if !car.alive() {
            continue;
        }
        let pos = tf.translation.truncate();
        let fwd = car.forward();
        let half_len = car.model.size.x * 0.5;

        // Turbo flames.
        if armory.turbo > 0.0 && car.throttle > 0.0 {
            let back = pos - fwd * (half_len + 2.0);
            let vel = -fwd.rotate(Vec2::from_angle(rng.range(-0.3, 0.3))) * rng.range(80.0, 200.0);
            commands.spawn((
                RoundEntity,
                Spark { vel: vel + car.vel * 0.5, life: rng.range(0.1, 0.25) },
                Mesh3d(art.spark_mesh.clone()),
                MeshMaterial3d(art.exhaust_mat.clone()),
                Transform::from_translation(back.extend(tf.translation.z + 6.0)),
            ));
        }

        if !armory.fire || armory.cooldown > 0.0 || armory.ammo == 0 {
            continue;
        }

        let nose = pos + fwd * (half_len + 8.0);
        let fwd_speed = car.vel.dot(fwd).max(0.0);
        let vol = if is_player { 1.0 } else { 0.45 };
        match armory.weapon {
            Weapon::None => continue,
            Weapon::Guns => {
                sounds.play(Sfx::Gun, vol * 0.6);
                armory.cooldown = GUN_COOLDOWN;
                let dir = fwd.rotate(Vec2::from_angle(rng.range(-0.04, 0.04)));
                let vel = dir * (GUN_SPEED + fwd_speed);
                commands.spawn((
                    RoundEntity,
                    Projectile { kind: ProjectileKind::Bullet, owner: e, by_player: is_player, vel, life: 0.7 },
                    Mesh3d(art.bullet_mesh.clone()),
                    MeshMaterial3d(art.bullet_mat.clone()),
                    Transform::from_translation(nose.extend(terrain.height(nose) + SHOT_HEIGHT))
                        .with_rotation(Quat::from_rotation_z(vel.to_angle())),
                ));
            }
            Weapon::Rockets => {
                sounds.play(Sfx::Rocket, vol);
                armory.cooldown = ROCKET_COOLDOWN;
                let vel = fwd * (ROCKET_START_SPEED + fwd_speed);
                let rocket = commands
                    .spawn((
                        RoundEntity,
                        Projectile { kind: ProjectileKind::Rocket, owner: e, by_player: is_player, vel, life: 2.0 },
                        Transform::from_translation(nose.extend(terrain.height(nose) + SHOT_HEIGHT))
                            .with_rotation(Quat::from_rotation_z(vel.to_angle())),
                    ))
                    .id();
                icons::attach_rocket(&mut commands, &icon_art, rocket);
            }
            Weapon::Mines => {
                sounds.play(Sfx::MineDrop, vol);
                armory.cooldown = MINE_COOLDOWN;
                let tail = pos - fwd * (half_len + 14.0);
                let mine = commands
                    .spawn((
                        RoundEntity,
                        Mine { by_player: is_player, arm: MINE_ARM_TIME },
                        Transform::from_translation(tail.extend(terrain.height(tail))),
                    ))
                    .id();
                icons::attach_mine(&mut commands, &icon_art, mine);
            }
        }
        armory.ammo -= 1;
        if armory.ammo == 0 {
            armory.weapon = Weapon::None;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn move_projectiles(
    mut commands: Commands,
    time: Res<Time>,
    art: Res<Art>,
    mut rng: ResMut<Rng>,
    mut score: ResMut<Score>,
    mut blasts: ResMut<PendingBlasts>,
    mut sounds: ResMut<Sounds>,
    mut shots: Query<(Entity, &mut Transform, &mut Projectile), Without<Car>>,
    mut cars: Query<(Entity, &Transform, &mut Car, Has<Player>)>,
    mut obstacles: Query<(&Obstacle, Option<&mut Explosive>)>,
    current: Res<CurrentArena>,
    terrain: Res<Terrain>,
) {
    let dt = time.delta_secs();
    let half = current.def().size / 2.0;

    for (e, mut tf, mut shot) in &mut shots {
        shot.life -= dt;
        if shot.kind == ProjectileKind::Rocket {
            let dir = shot.vel.normalize_or_zero();
            let speed = (shot.vel.length() + ROCKET_ACCEL * dt).min(ROCKET_MAX_SPEED);
            shot.vel = dir * speed;
            // Smoke trail.
            let back = tf.translation.truncate() - dir * 11.0;
            commands.spawn((
                RoundEntity,
                Spark { vel: -dir * 60.0, life: 0.15 },
                Mesh3d(art.spark_mesh.clone()),
                MeshMaterial3d(art.spark_mat.clone()),
                Transform::from_translation(back.extend(tf.translation.z)),
            ));
        }
        tf.translation += (shot.vel * dt).extend(0.0);
        let pos = tf.translation.truncate();
        // Shots skim over the ground, following hills and dips.
        tf.translation.z = terrain.height(pos) + SHOT_HEIGHT;

        // What did it hit?
        let out_of_bounds = pos.x.abs() > half.x + 200.0 || pos.y.abs() > half.y + 200.0;
        let mut hit_wall = None;
        for (o, explosive) in &mut obstacles {
            if o.pos.distance(pos) <= o.bounding_radius() + 2.0 && o.distance(pos) <= 0.5 {
                hit_wall = Some(explosive);
                break;
            }
        }
        let mut hit_car = None;
        for (ce, ctf, car, is_player) in &mut cars {
            if ce != shot.owner && car.obb(ctf.translation.truncate()).distance(pos) < 2.0 {
                hit_car = Some((car, is_player));
                break;
            }
        }

        if hit_car.is_none() && hit_wall.is_none() && !out_of_bounds && shot.life > 0.0 {
            continue;
        }
        commands.entity(e).despawn();

        match shot.kind {
            ProjectileKind::Bullet => {
                if let Some((mut car, is_player)) = hit_car {
                    hurt(&mut car, GUN_DAMAGE, shot.by_player && !is_player, &mut score);
                    sounds.play(Sfx::BulletHit, if shot.by_player || is_player { 0.5 } else { 0.25 });
                    let push = 25.0 / car.mass();
                    car.vel += shot.vel.normalize_or_zero() * push;
                    spawn_sparks(&mut commands, &art, &mut rng, tf.translation, -shot.vel.normalize_or_zero(), 30.0);
                } else if let Some(explosive) = hit_wall {
                    if let Some(mut barrel) = explosive {
                        barrel.hp -= 4.0;
                        barrel.by_player = shot.by_player;
                    }
                    spawn_sparks(&mut commands, &art, &mut rng, tf.translation, -shot.vel.normalize_or_zero(), 20.0);
                }
            }
            ProjectileKind::Rocket => blasts.0.push(Blast {
                pos,
                radius: ROCKET_BLAST,
                damage: ROCKET_DAMAGE,
                by_player: shot.by_player,
            }),
        }
    }
}

fn update_mines(
    mut commands: Commands,
    time: Res<Time>,
    mut blasts: ResMut<PendingBlasts>,
    mut mines: Query<(Entity, &mut Transform, &mut Mine), Without<Car>>,
    cars: Query<(&Transform, &Car)>,
) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for (e, mut tf, mut mine) in &mut mines {
        mine.arm -= dt;
        let pos = tf.translation.truncate();
        if mine.arm > 0.0 {
            tf.scale = Vec3::splat(0.7);
            continue;
        }
        // Armed mines pulse.
        tf.scale = Vec3::splat(1.0 + 0.12 * (t * 8.0).sin());
        let triggered = cars.iter().any(|(ctf, car)| {
            car.alive() && car.obb(ctf.translation.truncate()).distance(pos) < 9.0
        });
        if triggered {
            commands.entity(e).despawn();
            blasts.0.push(Blast {
                pos,
                radius: MINE_BLAST,
                damage: MINE_DAMAGE,
                by_player: mine.by_player,
            });
        }
    }
}

fn detonate_barrels(
    mut commands: Commands,
    mut blasts: ResMut<PendingBlasts>,
    barrels: Query<(Entity, &Obstacle, &Explosive)>,
) {
    for (e, o, barrel) in &barrels {
        if barrel.hp <= 0.0 {
            commands.entity(e).despawn();
            blasts.0.push(Blast { pos: o.pos, radius: BARREL_BLAST, damage: BARREL_DAMAGE, by_player: barrel.by_player });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_blasts(
    mut commands: Commands,
    art: Res<Art>,
    terrain: Res<Terrain>,
    mut rng: ResMut<Rng>,
    mut score: ResMut<Score>,
    mut blasts: ResMut<PendingBlasts>,
    mut sounds: ResMut<Sounds>,
    mut cars: Query<(&Transform, &mut Car, Has<Player>)>,
    mut barrels: Query<(&Obstacle, &mut Explosive)>,
) {
    for blast in blasts.0.drain(..) {
        sounds.play(Sfx::Explosion, 0.9);
        // Barrels caught in a blast go off next frame: chain reactions.
        for (o, mut barrel) in &mut barrels {
            let d = o.distance(blast.pos);
            if d < blast.radius {
                barrel.hp -= blast.damage * (1.0 - d / blast.radius);
                barrel.by_player = blast.by_player;
            }
        }
        for (tf, mut car, is_player) in &mut cars {
            let center = tf.translation.truncate();
            let d = car.obb(center).distance(blast.pos);
            if d > blast.radius {
                continue;
            }
            let falloff = 1.0 - d / blast.radius;
            let delta = center - blast.pos;
            let d = delta.length();
            hurt(&mut car, blast.damage * (0.35 + 0.65 * falloff), blast.by_player && !is_player, &mut score);
            let push = if d > 0.01 { delta / d } else { Vec2::X };
            let mass = car.mass();
            car.vel += push * BLAST_KNOCKBACK * (0.3 + 0.7 * falloff) / mass;
        }

        let center = blast.pos.extend(terrain.height(blast.pos) + 10.0);
        commands.spawn((
            RoundEntity,
            Explosion { radius: blast.radius, life: 0.0 },
            Mesh3d(art.blast_mesh.clone()),
            MeshMaterial3d(art.blast_mat.clone()),
            Transform::from_translation(center),
        ));
        for i in 0..4 {
            let dir = Vec2::from_angle(i as f32 * std::f32::consts::FRAC_PI_2);
            spawn_sparks(&mut commands, &art, &mut rng, center, dir, 220.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Power-ups
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn spawn_pickups(
    mut commands: Commands,
    time: Res<Time>,
    icon_art: Res<IconArt>,
    mut rng: ResMut<Rng>,
    mut timer: ResMut<PickupTimer>,
    existing: Query<(), With<Pickup>>,
    cars: Query<&Transform, With<Car>>,
    obstacles: Query<&Obstacle>,
    current: Res<CurrentArena>,
    terrain: Res<Terrain>,
) {
    timer.0 -= time.delta_secs();
    if timer.0 > 0.0 {
        return;
    }
    timer.0 = PICKUP_INTERVAL;
    if existing.iter().count() >= MAX_PICKUPS {
        return;
    }

    // Weighted random kind.
    let total: f32 = PickupKind::ALL.iter().map(|k| k.weight()).sum();
    let mut roll = rng.range(0.0, total);
    let mut index = 0;
    for (i, k) in PickupKind::ALL.iter().enumerate() {
        roll -= k.weight();
        if roll <= 0.0 {
            index = i;
            break;
        }
    }
    let kind = PickupKind::ALL[index];

    // Find a spot not right on top of a car.
    let margin = current.def().size / 2.0 - Vec2::splat(80.0);
    let mut pos = Vec2::ZERO;
    for _ in 0..30 {
        pos = Vec2::new(rng.range(-margin.x, margin.x), rng.range(-margin.y, margin.y));
        let clear_of_cars = cars.iter().all(|t| t.translation.truncate().distance(pos) > 90.0);
        if clear_of_cars && obstacles.iter().all(|o| o.distance(pos) > 40.0) {
            break;
        }
    }

    let pickup = commands
        .spawn((
            RoundEntity,
            Pickup(kind),
            Hover(terrain.height(pos) + PICKUP_HOVER),
            Icon { color: kind.color(), size: Vec2::splat(60.0), layer: 2 },
            Transform::from_translation(pos.extend(terrain.height(pos) + PICKUP_HOVER)),
        ))
        .id();
    icons::attach_pickup_icon(&mut commands, &icon_art, index, pickup);
}

fn collect_pickups(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    pickups: Query<(Entity, &Transform, &Pickup)>,
    mut cars: Query<(&Transform, &mut Car, &mut Armory, Has<Player>)>,
) {
    for (pe, ptf, pickup) in &pickups {
        let ppos = ptf.translation.truncate();
        for (ctf, mut car, mut armory, is_player) in &mut cars {
            if !car.alive() || car.obb(ctf.translation.truncate()).distance(ppos) > PICKUP_RADIUS {
                continue;
            }
            let sfx = match pickup.0 {
                PickupKind::Health => Sfx::Health,
                PickupKind::Turbo => Sfx::Turbo,
                _ => Sfx::WeaponPickup,
            };
            sounds.play(sfx, if is_player { 0.8 } else { 0.3 });
            let (weapon, ammo) = match pickup.0 {
                PickupKind::Health => {
                    car.hp = (car.hp + HEALTH_PACK).min(car.model.max_hp);
                    (None, 0)
                }
                PickupKind::Turbo => {
                    armory.turbo = TURBO_SECONDS;
                    (None, 0)
                }
                PickupKind::Guns => (Some(Weapon::Guns), GUN_AMMO),
                PickupKind::Rockets => (Some(Weapon::Rockets), ROCKET_AMMO),
                PickupKind::Mines => (Some(Weapon::Mines), MINE_AMMO),
            };
            if let Some(weapon) = weapon {
                // Same weapon stacks ammo, a different one replaces it.
                if armory.weapon == weapon {
                    armory.ammo += ammo;
                } else {
                    armory.weapon = weapon;
                    armory.ammo = ammo;
                }
            }
            commands.entity(pe).despawn();
            break;
        }
    }
}

/// Pickups wobble, bob and breathe so they catch the eye.
fn spin_pickups(time: Res<Time>, mut q: Query<(&mut Transform, &Hover), With<Pickup>>) {
    let t = time.elapsed_secs();
    for (mut tf, hover) in &mut q {
        tf.rotation = Quat::from_rotation_z((t * 2.0).sin() * 0.25);
        tf.scale = Vec3::splat(1.0 + 0.06 * (t * 4.0).sin());
        tf.translation.z = hover.0 + 3.0 * (t * 3.0).sin();
    }
}

// ---------------------------------------------------------------------------
// Visuals
// ---------------------------------------------------------------------------

fn spawn_sparks(
    commands: &mut Commands,
    art: &Art,
    rng: &mut Rng,
    at: Vec3,
    dir: Vec2,
    strength: f32,
) {
    let count = ((strength / 25.0) as usize).clamp(3, 18);
    for _ in 0..count {
        let side = if rng.next_f32() < 0.5 { 1.0 } else { -1.0 };
        let angle = rng.range(-0.8, 0.8);
        let speed = rng.range(80.0, 160.0 + strength);
        let vel = Vec2::from_angle(angle).rotate(dir * side) * speed;
        commands.spawn((
            RoundEntity,
            Spark { vel, life: rng.range(0.2, 0.5) },
            Mesh3d(art.spark_mesh.clone()),
            MeshMaterial3d(art.spark_mat.clone()),
            Transform::from_translation(at),
        ));
    }
}

/// Darken paint with damage, char wrecks, and make badly hurt cars smoke.
#[allow(clippy::too_many_arguments)]
fn update_car_visuals(
    mut commands: Commands,
    time: Res<Time>,
    art: Res<Art>,
    mut rng: ResMut<Rng>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sounds: ResMut<Sounds>,
    mut q: Query<(&Transform, &mut Car, Option<&mut Icon>)>,
) {
    let charred = Color::srgb(0.2, 0.17, 0.15);
    for (tf, mut car, icon) in &mut q {
        let health = car.health();
        let mix = if car.alive() { 0.35 + 0.65 * health } else { 0.12 };
        for (handle, base) in car.paint.handles.iter().zip(car.paint.colors) {
            let color = charred.mix(&base, mix);
            if let Some(mut mat) = materials.get_mut(handle)
                && mat.base_color != color
            {
                mat.base_color = color;
            }
        }
        if !car.alive() && !car.wrecked {
            car.wrecked = true;
            sounds.play(Sfx::Wreck, 1.0);
            if let Some(mut icon) = icon {
                icon.color = Color::srgb(0.3, 0.3, 0.3);
                icon.layer = 2;
            }
        }

        // Smoke: thicker the worse the damage; wrecks smoulder.
        let smoke_rate = if !car.alive() { 6.0 } else if health < 0.4 { (0.4 - health) * 40.0 } else { 0.0 };
        if rng.next_f32() < smoke_rate * time.delta_secs() {
            let pos = tf.translation.truncate() + car.forward() * car.model.size.x * 0.25;
            let drift = Vec2::new(rng.range(-15.0, 15.0), rng.range(20.0, 45.0));
            let life = rng.range(0.8, 1.4);
            commands.spawn((
                RoundEntity,
                Smoke { vel: drift - car.vel * 0.15, life, max_life: life },
                Mesh3d(art.smoke_mesh.clone()),
                MeshMaterial3d(art.smoke_mat.clone()),
                Transform::from_translation(pos.extend(tf.translation.z + cars::roof_height(car.model)))
                    .with_scale(Vec3::splat(4.0)),
            ));
        }
    }
}

fn draw_health_bars(mut gizmos: Gizmos, time: Res<Time>, q: Query<(&Transform, &Car, &Armory, Has<Player>)>) {
    for (tf, car, armory, is_player) in &q {
        if !car.alive() {
            continue;
        }
        let health = car.health();
        // Floats just above the roof, behind the car on screen.
        let left = tf.translation
            + Vec3::new(-20.0, car.model.size.x * 0.5 + 10.0, cars::roof_height(car.model) + 8.0);
        let line = |gizmos: &mut Gizmos, a: Vec2, b: Vec2, c: Color| {
            gizmos.line(left + a.extend(0.0), left + b.extend(0.0), c);
        };
        line(&mut gizmos, Vec2::ZERO, Vec2::X * 40.0, Color::srgb(0.3, 0.05, 0.05));
        let bar = Color::srgb(1.0 - health, health, 0.15);
        line(&mut gizmos, Vec2::ZERO, Vec2::X * 40.0 * health, bar);

        // A small coloured dot shows what weapon a car is carrying.
        let weapon_color = match armory.weapon {
            Weapon::None => None,
            Weapon::Guns => Some(PickupKind::Guns.color()),
            Weapon::Rockets => Some(PickupKind::Rockets.color()),
            Weapon::Mines => Some(PickupKind::Mines.color()),
        };
        if let Some(c) = weapon_color {
            gizmos.circle(Isometry3d::from_translation(left + Vec3::new(48.0, 0.0, 0.0)), 4.0, c);
        }

        // Bobbing arrow above the player's car.
        if is_player {
            let bob = (time.elapsed_secs() * 5.0).sin() * 2.0;
            let tip = Vec2::new(20.0, 8.0 + bob);
            let white = Color::srgb(1.0, 1.0, 1.0);
            line(&mut gizmos, tip, tip + Vec2::new(-6.0, 7.0), white);
            line(&mut gizmos, tip, tip + Vec2::new(6.0, 7.0), white);
            line(&mut gizmos, tip + Vec2::new(-6.0, 7.0), tip + Vec2::new(6.0, 7.0), white);
        }
    }
}

fn update_sparks(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut Spark)>,
) {
    let dt = time.delta_secs();
    for (e, mut tf, mut spark) in &mut q {
        spark.life -= dt;
        if spark.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        spark.vel *= (-4.0 * dt).exp();
        tf.translation += (spark.vel * dt).extend(0.0);
        tf.scale = Vec3::splat((spark.life * 3.0).min(1.0));
    }
}

fn update_smoke(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut Smoke)>,
) {
    let dt = time.delta_secs();
    for (e, mut tf, mut smoke) in &mut q {
        smoke.life -= dt;
        if smoke.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let age = 1.0 - smoke.life / smoke.max_life;
        tf.translation += (smoke.vel * dt).extend(30.0 * dt);
        // Puffs grow, then shrink away as they fade.
        tf.scale = Vec3::splat(4.0 + 10.0 * age.sqrt() * (1.0 - age * age));
    }
}

fn update_explosions(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut Explosion)>,
) {
    const DURATION: f32 = 0.35;
    for (e, mut tf, mut ex) in &mut q {
        ex.life += time.delta_secs();
        if ex.life >= DURATION {
            commands.entity(e).despawn();
            continue;
        }
        let t = ex.life / DURATION;
        // Expand fast, then shrink away.
        let size = (ex.radius * t.sqrt() * (1.0 - t * t)).max(1.0);
        tf.scale = Vec3::new(size, size, size * 0.6);
    }
}

/// Follow the player with a little look-ahead, zoom out with speed, and stay
/// inside the arena. Without a player (balance sims) show the whole arena.
///
/// The camera always looks straight down; zoom is how many world units per
/// screen pixel there are at ground level, which sets the camera's height.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn camera_follow(
    time: Res<Time>,
    state: Res<State<GameState>>,
    current: Res<CurrentArena>,
    terrain: Res<Terrain>,
    window: Single<&Window, With<PrimaryWindow>>,
    player: Query<(&Transform, &Car), (With<Player>, Without<Camera3d>)>,
    camera: Single<&mut Transform, (With<Camera3d>, Without<Car>)>,
    light: Single<&mut bevy::light::CascadeShadowConfig>,
    mut zoom: Local<Option<f32>>,
    mut ground: Local<f32>,
) {
    let mut tf = camera.into_inner();
    let view = Vec2::new(window.width(), window.height()).max(Vec2::ONE);

    let (target, target_zoom, target_ground, snap) = if *state.get() == GameState::Garage {
        // The showroom is laid out for 1280x800; fit it to whatever the window is.
        let fit = (Vec2::new(1280.0, 800.0) / view).max_element().max(1.0);
        (Vec2::new(0.0, 24.0), fit, 0.0, true)
    } else {
        let arena = current.def().size;
        match player.single() {
            Ok((ptf, car)) => {
                let speed = car.vel.length();
                let pos = ptf.translation.truncate();
                (pos + car.vel * 0.3, 1.0 + (speed / 800.0).min(1.0) * 0.2, ptf.translation.z, false)
            }
            Err(_) => {
                let fit = ((arena + Vec2::splat(200.0)) / view).max_element();
                (Vec2::new(0.0, 30.0 * fit), fit, terrain.height(Vec2::ZERO), false)
            }
        }
    };

    let k = if snap { 1.0 } else { 1.0 - (-4.0 * time.delta_secs()).exp() };
    let z = zoom.get_or_insert(target_zoom);
    *z += (target_zoom - *z) * k;
    *ground += (target_ground - *ground) * k;
    let mut pos = tf.translation.truncate();
    pos += (target - pos) * k;

    if *state.get() != GameState::Garage {
        // Don't show more than a strip of the outfield.
        let arena = current.def().size;
        let room = arena / 2.0 + Vec2::splat(160.0) - view / 2.0 * *z;
        pos.x = if room.x > 0.0 { pos.x.clamp(-room.x, room.x) } else { 0.0 };
        pos.y = if room.y > 0.0 { pos.y.clamp(-room.y, room.y) } else { 0.0 };
    }
    let height = camera_height(view.y * *z);
    tf.translation = pos.extend(*ground + height);

    // Keep shadows reaching the ground as the camera climbs and dives.
    let mut cascades = light.into_inner();
    let reach = cascades.bounds.last().copied().unwrap_or(0.0);
    let want = shadow_cascades(height);
    let want_reach = want.bounds.last().copied().unwrap_or(0.0);
    if (reach - want_reach).abs() > want_reach * 0.1 {
        *cascades = want;
    }
}

fn update_hud(
    score: Res<Score>,
    cars: Query<(&Car, &Armory, Has<Player>)>,
    mut hud: Query<&mut Text, With<Hud>>,
) {
    let Ok(mut text) = hud.single_mut() else { return };
    let Some((car, armory, _)) = cars.iter().find(|(_, _, p)| *p) else { return };
    let rivals = cars.iter().filter(|(c, _, p)| !p && c.alive()).count();

    let weapon_text = if armory.weapon == Weapon::None {
        armory.weapon.name().to_string()
    } else {
        format!("{} x{}", armory.weapon.name(), armory.ammo)
    };
    let turbo_text = if armory.turbo > 0.0 { format!("   TURBO {:.1}s", armory.turbo) } else { String::new() };

    text.0 = format!(
        "{}  HP {:>3.0}/{:.0}   Weapon {}{}   Rivals {}   Wrecked {}   Score {:.0}\n\
         W/S throttle  |  A/D steer  |  Space handbrake  |  F/C/V fire",
        car.model.name,
        car.hp.max(0.0),
        car.model.max_hp,
        weapon_text,
        turbo_text,
        rivals,
        score.wrecks,
        total_score(&score),
    );
}

fn total_score(score: &Score) -> f32 {
    score.damage_dealt * 10.0 + score.wrecks as f32 * 500.0 + if score.won { 2000.0 } else { 0.0 }
}

// ---------------------------------------------------------------------------
// Round flow
// ---------------------------------------------------------------------------

fn check_round_end(
    time: Res<Time>,
    cars: Query<(&Car, Has<Player>)>,
    mut score: ResMut<Score>,
    mut next: ResMut<NextState<GameState>>,
    sim: Option<ResMut<Sim>>,
) {
    if let Some(mut sim) = sim {
        sim.clock += time.delta_secs();
        let alive: Vec<&Car> = cars.iter().map(|(c, _)| c).filter(|c| c.alive()).collect();
        if alive.len() > 1 && sim.clock < SIM_ROUND_LIMIT {
            return;
        }
        for m in sim.lineup() {
            sim.entries[m] += 1;
        }
        let winner = (alive.len() == 1).then(|| alive[0].model);
        match winner.and_then(|w| MODELS.iter().position(|m| std::ptr::eq(m, w))) {
            Some(i) => sim.wins[i] += 1,
            None => sim.draws += 1,
        }
        sim.round += 1;
        sim.total_time += sim.clock;
        eprintln!(
            "SIM round {:>3}/{}: {:<10} ({:.0}s)",
            sim.round,
            sim.rounds,
            winner.map_or("draw", |m| m.name),
            sim.clock
        );
        sim.clock = 0.0;
        next.set(GameState::GameOver);
        return;
    }

    let player_alive = cars.iter().any(|(c, p)| p && c.alive());
    let rivals = cars.iter().filter(|(c, p)| !p && c.alive()).count();
    if !player_alive || rivals == 0 {
        score.won = player_alive;
        next.set(GameState::GameOver);
    }
}

fn show_game_over(mut commands: Commands, score: Res<Score>, mut sounds: ResMut<Sounds>, sim: Option<Res<Sim>>) {
    if sim.is_some() {
        return;
    }
    sounds.play(if score.won { Sfx::Win } else { Sfx::Lose }, 0.8);
    let (headline, color) = if score.won {
        ("LAST CAR RUNNING!", Color::srgb(0.4, 1.0, 0.5))
    } else {
        ("WRECKED", Color::srgb(1.0, 0.35, 0.3))
    };
    commands.spawn((
        DespawnOnExit(GameState::GameOver),
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: px(12),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        children![
            (
                Text::new(headline),
                TextFont { font_size: FontSize::Px(64.0), ..default() },
                TextColor(color),
            ),
            (
                Text::new(format!(
                    "Score {:.0}    |    {} wrecked",
                    total_score(&score),
                    score.wrecks
                )),
                TextFont { font_size: FontSize::Px(30.0), ..default() },
                TextColor(Color::WHITE),
            ),
            (
                Text::new("Enter: rematch    |    G: garage"),
                TextFont { font_size: FontSize::Px(22.0), ..default() },
                TextColor(Color::srgb(0.7, 0.7, 0.7)),
            ),
        ],
    ));
}

fn game_over_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<GameState>>,
    sim: Option<Res<Sim>>,
    mut exit: MessageWriter<AppExit>,
) {
    if let Some(sim) = sim {
        if sim.round < sim.rounds {
            next.set(GameState::Playing);
            return;
        }
        let n = sim.rounds as f32;
        eprintln!("SIM results over {} rounds (avg {:.0}s):", sim.rounds, sim.total_time / n);
        let fair = 100.0 / (AI_CARS + 1) as f32;
        for ((m, w), entries) in MODELS.iter().zip(sim.wins).zip(sim.entries) {
            if entries == 0 {
                continue;
            }
            // Chance that any one car of this model wins its round; fair is 1 in 6.
            eprintln!(
                "SIM   T{} {:<11} {:>3} wins / {:>3} cars entered = {:>5.1}% per car (fair {:.1}%)",
                m.tier, m.name, w, entries, w as f32 / entries as f32 * 100.0, fair,
            );
        }
        eprintln!("SIM   draws          {:>3}", sim.draws);
        exit.write(AppExit::Success);
        return;
    }
    if keys.just_pressed(KeyCode::Enter) {
        next.set(GameState::Playing);
    } else if keys.just_pressed(KeyCode::KeyG) {
        next.set(GameState::Garage);
    }
}
