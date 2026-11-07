//! Arena Drift — a top-down demolition derby and racer.
//!
//! Derby: ram the other cars until you're the last one still running.
//! Front hits hurt the other car more than you; getting T-boned hurts most.
//! Race: a few laps against three rivals; first over the line wins.
//! Power-ups spawn around the arena or track: health packs, guns, rockets, mines and turbo.

use bevy::prelude::*;

mod arena;
mod cars;
mod icons;
mod minimap;
mod physics;
mod sound;
mod terrain;
mod track;

use std::collections::VecDeque;

use arena::{ARENAS, ArenaArt, ArenaDef, Explosive, Obstacle};
use bevy::light::NotShadowCaster;
use bevy::window::PrimaryWindow;
use minimap::{Icon, MinimapPlugin};

use cars::{CarArt, CarModel, MODELS, Paint};
use icons::IconArt;
use physics::{Obb, cross};
use sound::{Sfx, SoundPlugin, Sounds};
use terrain::Terrain;
use track::Track;

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
/// Pulls cars back down after a jump (px/s²).
const FALL_GRAVITY: f32 = 1100.0;
/// A car this far above the ground counts as airborne.
const AIRBORNE: f32 = 4.0;
/// Speed lost per second while driving through water.
const WATER_DRAG: f32 = 1.4;
/// Turbo from driving over a boost pad.
const BOOST_SECONDS: f32 = 1.2;
const BOOST_KICK: f32 = 160.0;

const RESTITUTION: f32 = 0.6;
/// Scales rotational inertia; higher = cars spin less from off-centre hits.
const INERTIA_SCALE: f32 = 2.0;
/// How quickly tyres kill collision-induced spin (per second).
const SPIN_DAMPING: f32 = 5.0;
const WRECK_SPIN_DAMPING: f32 = 2.0;
const DAMAGE_THRESHOLD: f32 = 70.0; // impact speed below this does no damage
const DAMAGE_SCALE: f32 = 0.16;
const WALL_DAMAGE_SCALE: f32 = 0.05;
/// Races are about speed, not survival: crashes do this much of their derby damage.
const RACE_CRASH_DAMAGE: f32 = 0.35;

const AI_CARS: usize = 5;
/// Cars in a race, the player included.
const RACE_CARS: usize = 4;
/// Seconds of "3, 2, 1" before a race starts.
const COUNTDOWN: f32 = 3.0;

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
    Menu,
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
    /// Vertical speed, for jumps.
    vz: f32,
    /// In the air: no grip, no steering, no engine.
    airborne: bool,
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
            vz: 0.0,
            airborne: false,
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
    /// Racing: where across the track it's aiming (offset from the centreline).
    lane: f32,
    /// Racing: how much throttle it uses on the straights (1 = flat out).
    pace: f32,
}

impl Ai {
    fn new(player_bias: f32, aggression: f32) -> Self {
        Self { stuck: 0.0, reversing: 0.0, player_bias, aggression, lane: 0.0, pace: 1.0 }
    }
}

/// A car's progress round a race track.
#[derive(Component)]
struct Racer {
    /// Distance driven along the track since the start line (negative on
    /// the grid; one lap is `track.length`).
    progress: f32,
    /// Nearest centreline sample, to speed up the next lookup.
    index: usize,
    /// Sideways offset from the centreline.
    lateral: f32,
    /// Race clock when the current lap started.
    lap_start: f32,
    best_lap: Option<f32>,
    /// Race clock when it crossed the line for the last time.
    finish: Option<f32>,
}

impl Racer {
    /// Laps completed so far.
    fn laps_done(&self, track: &Track) -> u32 {
        (self.progress / track.length).floor().max(0.0) as u32
    }
}

/// Race clock and start countdown. Only exists during races.
#[derive(Resource)]
struct Race {
    countdown: f32,
    clock: f32,
}

impl Race {
    fn started(&self) -> bool {
        self.countdown <= 0.0
    }
}

#[derive(Component)]
struct CountdownText;

/// Floating health bar for a car.
#[derive(Component)]
struct HealthBar {
    car: Entity,
    fill: Entity,
    fill_mat: Handle<StandardMaterial>,
    dot: Entity,
    dot_mat: Handle<StandardMaterial>,
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
    /// Races: where the player finished (1 = won), or `None` if they didn't.
    place: Option<usize>,
    race_time: Option<f32>,
    best_lap: Option<f32>,
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
    /// Cars in the current round.
    field: Vec<usize>,
}

impl Sim {
    /// Models for this round: all of them, or one tier repeated to fill the grid.
    fn lineup(&self, count: usize) -> Vec<usize> {
        let pool: Vec<usize> = match self.tier {
            Some(t) => cars::tier_models(t),
            None => (0..MODELS.len()).collect(),
        };
        // Offsetting by the round rotates both starting spots and which
        // models get doubled up when the pool is smaller than the grid.
        let r = self.round as usize;
        (0..count).map(|i| pool[(i + r) % pool.len()]).collect()
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
    dot_mesh: Handle<Mesh>,
    arrow_mesh: Handle<Mesh>,
    bar_back_mat: Handle<StandardMaterial>,
    white_mat: Handle<StandardMaterial>,
    spray_mat: Handle<StandardMaterial>,
    ripple_mat: Handle<StandardMaterial>,
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
        field: Vec::new(),
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
        .init_resource::<MenuChoice>()
        .init_resource::<LastInput>()
        // Dev shortcut: ARENA_DRIFT_AUTOSTART=1 skips the menu and garage.
        .insert_state(if autostart { GameState::Playing } else { GameState::Menu })
        .init_resource::<CarArt>()
        .init_resource::<IconArt>()
        .init_resource::<ArenaArt>()
        .add_systems(Startup, setup_world)
        .add_systems(Update, dev_screenshot)
        .add_systems(PreUpdate, gamepad_menu_keys.after(bevy::input::InputSystems))
        .add_systems(OnEnter(GameState::Menu), setup_menu)
        .add_systems(Update, menu_input.run_if(in_state(GameState::Menu)))
        .add_systems(OnEnter(GameState::Garage), setup_garage)
        .add_systems(Update, (garage_input, garage_animate).chain().run_if(in_state(GameState::Garage)))
        .add_systems(OnEnter(GameState::Playing), start_round)
        .add_systems(
            Update,
            (
                (
                    player_input,
                    ai_drive,
                    ai_race,
                    car_physics,
                    static_collisions,
                    car_collisions,
                    track_progress.run_if(resource_exists::<Track>),
                    follow_terrain,
                    race_hazards.run_if(resource_exists::<Track>),
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
                    update_countdown,
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
                river_ripples.run_if(resource_exists::<Track>),
                (attach_health_bars, update_health_bars).chain().after(follow_terrain),
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

/// `ARENA_DRIFT_LOOK=x,y,zoom`: where a player-less camera looks (dev aid).
fn dev_look() -> Option<(Vec2, f32)> {
    let v = std::env::var("ARENA_DRIFT_LOOK").ok()?;
    let n: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (n.len() == 3).then(|| (Vec2::new(n[0], n[1]), n[2]))
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
        dot_mesh: meshes.add(Circle::new(1.0)),
        // Points down, at the car below it.
        arrow_mesh: meshes.add(Triangle2d::new(Vec2::new(0.0, -5.0), Vec2::new(6.0, 4.0), Vec2::new(-6.0, 4.0))),
        bar_back_mat: materials.add(glow(Color::srgb(0.12, 0.03, 0.03))),
        white_mat: materials.add(glow(Color::WHITE)),
        spray_mat: materials.add(glow(Color::srgba(0.85, 0.93, 1.0, 0.75))),
        // Drawn after the water and the ford's gravel.
        ripple_mat: materials.add(StandardMaterial { depth_bias: 40.0, ..glow(Color::srgba(0.85, 0.95, 1.0, 0.3)) }),
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
    mut sim: Option<ResMut<Sim>>,
    old: Query<Entity, With<RoundEntity>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    skids.0.clear();
    *score = Score::default();
    let def = current.def();
    let (obstacles, ground, track) =
        arena::spawn_arena(&mut commands, &mut arena_art, &mut meshes, &mut images, &mut materials, def);
    *terrain = ground;
    minimap::spawn_minimap(&mut commands, def.size, RoundEntity);
    let field = if track.is_some() { RACE_CARS } else { AI_CARS + 1 };
    // The player's car first, then rivals from the same tier.
    let lineup = match &mut sim {
        Some(sim) => {
            sim.field = sim.lineup(field);
            sim.field.clone()
        }
        None => cars::tier_lineup(selected.0, field - 1),
    };
    let paints = cars::lineup_paints(&lineup);
    pickup_timer.0 = 1.0; // first power-up shows up quickly
    blasts.0.clear();

    let grid = match &track {
        // The player starts third, with one rival behind.
        Some(t) => {
            let slots = t.grid(lineup.len());
            let mut order: Vec<usize> = (0..lineup.len()).collect();
            if sim.is_none() {
                let player = order.remove(0);
                order.insert(2.min(order.len()), player);
            }
            let mut grid = vec![(Vec2::ZERO, 0.0); lineup.len()];
            for (slot, &car) in order.iter().enumerate() {
                grid[car] = slots[slot];
            }
            grid
        }
        // Fair starting spots: open road ahead of every car, no barrels next to anyone.
        None => arena::spawn_points(&obstacles, def.size, lineup.len()).0,
    };
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
        if let Some(t) = &track {
            let spot = t.locate(pos, None);
            commands.entity(root).insert(Racer {
                progress: t.delta(0.0, spot.s),
                index: spot.index,
                lateral: spot.lateral,
                lap_start: 0.0,
                best_lap: None,
                finish: None,
            });
        }

        let t = i as f32 / (field - 1) as f32;
        if is_player {
            commands.entity(root).insert(Player);
        } else if sim.is_some() {
            commands.entity(root).insert(Ai::new(1.0, 0.9));
        } else if track.is_some() {
            // Racers differ in how hard they push, but none drives flat out.
            commands.entity(root).insert(Ai { pace: 0.82 + 0.08 * t, ..Ai::new(1.0, 0.78 + 0.1 * t) });
        } else {
            commands.entity(root).insert(Ai::new(0.6 + 0.3 * t, 0.75 + 0.25 * (1.0 - t)));
        }
    }

    match track {
        Some(t) => {
            commands.insert_resource(t);
            commands.insert_resource(Race { countdown: COUNTDOWN, clock: 0.0 });
        }
        None => {
            commands.remove_resource::<Track>();
            commands.remove_resource::<Race>();
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
    if def.is_race() {
        commands.spawn((
            RoundEntity,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                top: percent(30),
                justify_content: JustifyContent::Center,
                ..default()
            },
            children![(
                CountdownText,
                Text::new(""),
                TextFont { font_size: FontSize::Px(110.0), ..default() },
                TextColor(Color::srgb(0.95, 0.8, 0.3)),
                TextShadow::default(),
            )],
        ));
    }
}

// ---------------------------------------------------------------------------
// Main menu
// ---------------------------------------------------------------------------

/// Which menu entry is highlighted: 0 = race, 1 = arena.
#[derive(Resource, Default)]
struct MenuChoice(usize);

#[derive(Component)]
struct MenuItem(usize);

const MENU_ITEMS: [(&str, &str); 2] = [
    ("NEW RACE", "Race three rivals. First over the line wins."),
    ("ARENA", "Demolition derby. Last car running wins."),
];

fn setup_menu(mut commands: Commands, mut terrain: ResMut<Terrain>, old: Query<Entity, With<RoundEntity>>) {
    for e in &old {
        commands.entity(e).despawn();
    }
    *terrain = Terrain::default();
    commands.remove_resource::<Track>();
    commands.remove_resource::<Race>();

    let gold = Color::srgb(0.95, 0.8, 0.3);
    commands
        .spawn((
            DespawnOnExit(GameState::Menu),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(18),
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("ARENA DRIFT"),
                TextFont { font_size: FontSize::Px(76.0), ..default() },
                TextColor(gold),
                Node { margin: UiRect::bottom(px(30)), ..default() },
            ));
            for (i, (title, blurb)) in MENU_ITEMS.iter().enumerate() {
                root.spawn((
                    MenuItem(i),
                    Node {
                        width: px(520),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::all(px(14)),
                        border: UiRect::all(px(3)),
                        ..default()
                    },
                    BorderColor::all(Color::NONE),
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.04)),
                    children![
                        (Text::new(*title), TextFont { font_size: FontSize::Px(40.0), ..default() }, TextColor(Color::WHITE)),
                        (
                            Text::new(*blurb),
                            TextFont { font_size: FontSize::Px(18.0), ..default() },
                            TextColor(Color::srgb(0.7, 0.7, 0.7)),
                            TextLayout::justify(Justify::Center),
                        ),
                    ],
                ));
            }
            root.spawn((
                Text::new("Up/Down choose  |  Enter select  (gamepad: D-pad, A)"),
                TextFont { font_size: FontSize::Px(20.0), ..default() },
                TextColor(Color::srgb(0.6, 0.6, 0.6)),
                Node { margin: UiRect::top(px(30)), ..default() },
            ));
        });
}

fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut choice: ResMut<MenuChoice>,
    mut current: ResMut<CurrentArena>,
    mut next: ResMut<NextState<GameState>>,
    mut items: Query<(&MenuItem, &mut BorderColor, &mut BackgroundColor)>,
) {
    let n = MENU_ITEMS.len();
    if keys.any_just_pressed([KeyCode::ArrowUp, KeyCode::KeyW]) {
        choice.0 = (choice.0 + n - 1) % n;
    }
    if keys.any_just_pressed([KeyCode::ArrowDown, KeyCode::KeyS, KeyCode::Tab]) {
        choice.0 = (choice.0 + 1) % n;
    }
    for (item, mut border, mut bg) in &mut items {
        let on = item.0 == choice.0;
        *border = BorderColor::all(if on { Color::srgb(0.95, 0.8, 0.3) } else { Color::NONE });
        bg.0 = Color::srgba(1.0, 1.0, 1.0, if on { 0.12 } else { 0.04 });
    }
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::Space]) {
        // Keep the last arena or track of that kind, if there was one.
        let race = choice.0 == 0;
        if current.def().is_race() != race {
            current.0 = arena::arenas_for(race)[0];
        }
        next.set(GameState::Garage);
    }
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
        // Next arena or track of the same kind.
        let pool = arena::arenas_for(current.def().is_race());
        let k = pool.iter().position(|&i| i == current.0).unwrap_or(0);
        current.0 = pool[(k + 1) % pool.len()];
    }
    if keys.just_pressed(KeyCode::Escape) {
        next.set(GameState::Menu);
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
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::Space]) {
        next.set(GameState::Playing);
    }

    let Ok(mut text) = info.single_mut() else { return };
    let m = &MODELS[selected.0];
    let handling = |m: &CarModel| m.turn_rate * 0.6 + m.grip * 0.4;
    let toughness = |m: &CarModel| m.max_hp * m.armor;
    let def = current.def();
    let venue = match def.track() {
        Some(t) => format!("Track: {} ({} laps)", def.name.to_uppercase(), t.laps),
        None => format!("Arena: {}", def.name.to_uppercase()),
    };
    text.0 = format!(
        "TIER {}   <  {}  >\n{}\nRivals: {}\n\n{}  -  Tab to change\n{}\n\n\
         Top speed  {}\n\
         Accel      {}\n\
         Handling   {}\n\
         Toughness  {}\n\
         Weight     {}\n\n\
         Up/Down tier  |  Left/Right car  |  Enter/Space start  |  Esc menu\n\
         Gamepad: D-pad choose  |  Y change {}  |  A start  |  B menu",
        m.tier,
        m.name.to_uppercase(),
        m.blurb,
        cars::tier_models(m.tier).iter().map(|&i| MODELS[i].name).collect::<Vec<_>>().join(", "),
        venue,
        def.blurb,
        stat_bar(m.top_speed, |m| m.top_speed),
        stat_bar(m.engine, |m| m.engine),
        stat_bar(handling(m), handling),
        stat_bar(toughness(m), toughness),
        stat_bar(m.mass, |m| m.mass),
        if def.is_race() { "track" } else { "arena" },
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
    pads: Query<&Gamepad>,
    mut last: ResMut<LastInput>,
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
    let mut handbrake = keys.pressed(KeyCode::Space);
    let mut fire = false;
    // Gamepads, all analog: the left stick steers (left/right) and drives
    // (up = throttle, down = brake/reverse); the triggers also work as
    // throttle and brake. A or RB fires, B or LB is the handbrake.
    let mut pad_used = false;
    for pad in &pads {
        let stick = pad.left_stick();
        let x = stick_axis(stick.x, STICK_DEADZONE);
        // Responsive right from the middle, flattening out towards full lock.
        let pad_steer = -x.signum() * x.abs().powf(STEER_CURVE);
        // A slightly bigger dead zone for throttle, so steering hard doesn't
        // also brake.
        let y = stick_axis(stick.y, 0.2);
        let pad_throttle = y.signum() * y.abs().powf(STEER_CURVE)
            + pad.get(GamepadButton::RightTrigger2).unwrap_or(0.0)
            - pad.get(GamepadButton::LeftTrigger2).unwrap_or(0.0);
        let pad_handbrake = pad.pressed(GamepadButton::East) || pad.pressed(GamepadButton::LeftTrigger);
        let pad_fire = pad.pressed(GamepadButton::South) || pad.pressed(GamepadButton::RightTrigger);
        steer += pad_steer;
        throttle += pad_throttle;
        handbrake |= pad_handbrake;
        fire |= pad_fire;
        pad_used |= pad_steer != 0.0 || pad_throttle.abs() > 0.05 || pad_handbrake || pad_fire;
    }
    if pad_used {
        last.0 = InputDevice::Gamepad;
    } else if keys.get_pressed().next().is_some() {
        last.0 = InputDevice::Keyboard;
    }
    car.throttle = throttle.clamp(-1.0, 1.0);
    car.steer = steer.clamp(-1.0, 1.0);
    car.handbrake = handbrake;
    // Ctrl suits arrow-key driving. In the browser, Ctrl+W (W held for throttle)
    // would close the tab; pages can't block that, so index.html asks before
    // leaving instead.
    armory.fire = fire
        || keys.any_pressed([
            KeyCode::KeyF,
            KeyCode::KeyC,
            KeyCode::KeyV,
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
        ]);
}

/// Stick movement smaller than this is ignored (worn sticks drift a little).
const STICK_DEADZONE: f32 = 0.1;
/// Stick response curve: below 1 reacts strongly to small movements and
/// changes less near the edge.
const STEER_CURVE: f32 = 0.85;

/// A stick axis with a dead zone, rescaled to still reach ±1.
fn stick_axis(v: f32, deadzone: f32) -> f32 {
    if v.abs() <= deadzone { 0.0 } else { v.signum() * ((v.abs() - deadzone) / (1.0 - deadzone)).min(1.0) }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum InputDevice {
    #[default]
    Keyboard,
    Gamepad,
}

/// What the player drove with last, for showing the right control hints.
#[derive(Resource, Default)]
struct LastInput(InputDevice);

/// Menus are driven by keys, so gamepad buttons press the matching keys:
/// D-pad (or the left stick outside races) for arrows, A / Start for Enter,
/// B for Esc and Y / Select for Tab.
fn gamepad_menu_keys(
    state: Res<State<GameState>>,
    pads: Query<&Gamepad>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut held: Local<Vec<KeyCode>>,
) {
    let mut want = Vec::new();
    for pad in &pads {
        let mut dir = pad.dpad();
        // While driving the stick steers by itself; arrows would mean full lock.
        if *state.get() != GameState::Playing {
            let stick = pad.left_stick();
            if stick.length() > 0.6 {
                dir += stick;
            }
        }
        for (on, key) in [
            (dir.y > 0.5, KeyCode::ArrowUp),
            (dir.y < -0.5, KeyCode::ArrowDown),
            (dir.x < -0.5, KeyCode::ArrowLeft),
            (dir.x > 0.5, KeyCode::ArrowRight),
            (pad.pressed(GamepadButton::South) || pad.pressed(GamepadButton::Start), KeyCode::Enter),
            (pad.pressed(GamepadButton::East), KeyCode::Escape),
            (pad.pressed(GamepadButton::North) || pad.pressed(GamepadButton::Select), KeyCode::Tab),
        ] {
            if on && !want.contains(&key) {
                want.push(key);
            }
        }
    }
    for &key in &want {
        if !held.contains(&key) {
            keys.press(key);
        }
    }
    for &key in held.iter() {
        if !want.contains(&key) {
            keys.release(key);
        }
    }
    *held = want;
}

/// Derby AI: hunt the other cars, grab power-ups on the way.
fn ai_drive(
    time: Res<Time>,
    track: Option<Res<Track>>,
    mut ais: Query<(Entity, &Transform, &mut Car, &mut Ai, &mut Armory)>,
    targets: Query<(Entity, &Transform, &Car, Has<Player>), Without<Ai>>,
    pickups: Query<(&Transform, &Pickup)>,
    obstacles: Query<&Obstacle>,
) {
    if track.is_some() {
        return; // racing: see `ai_race`
    }
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

/// Back up and turn when barely moving for a while. Returns true while backing up.
fn ai_unstick(ai: &mut Ai, car: &mut Car, dt: f32, turn_towards: f32) -> bool {
    if ai.reversing > 0.0 {
        ai.reversing -= dt;
        car.throttle = -1.0;
        // Steering flips in reverse: this swings the nose towards the goal.
        car.steer = -turn_towards.signum();
        car.handbrake = false;
        return true;
    }
    if car.vel.length() < 25.0 {
        ai.stuck += dt;
    } else {
        ai.stuck = 0.0;
    }
    if ai.stuck > 1.2 {
        ai.stuck = 0.0;
        ai.reversing = 0.7;
    }
    false
}

/// How fast a car can take a bend of curvature `bend` (1 / radius).
fn corner_speed(model: &CarModel, bend: f32, skill: f32) -> f32 {
    skill * 0.8 * model.turn_rate / bend.max(1e-4)
}

/// Race AI: follow the track as fast as the corners allow, overtake, pick up
/// power-ups near the racing line and shoot whoever is in front.
#[allow(clippy::type_complexity)]
fn ai_race(
    time: Res<Time>,
    track: Option<Res<Track>>,
    race: Option<Res<Race>>,
    mut ais: Query<(Entity, &Transform, &mut Car, &mut Ai, &mut Armory, &Racer)>,
    others: Query<(Entity, &Transform, &Car, &Racer), Without<Ai>>,
    pickups: Query<(&Transform, &Pickup)>,
) {
    let (Some(track), Some(race)) = (track, race) else { return };
    let dt = time.delta_secs();
    let half = track.width / 2.0;

    // Every car, wrecks included: (entity, position, progress, lateral, alive).
    let mut field: Vec<(Entity, Vec2, f32, f32, bool)> = others
        .iter()
        .map(|(e, t, c, r)| (e, t.translation.truncate(), r.progress, r.lateral, c.alive()))
        .collect();
    field.extend(
        ais.iter().map(|(e, t, c, _, _, r)| (e, t.translation.truncate(), r.progress, r.lateral, c.alive())),
    );
    let pickups: Vec<(Vec2, PickupKind)> = pickups.iter().map(|(t, p)| (t.translation.truncate(), p.0)).collect();
    // Boost pads (distance along the track, offset across) and ramp lips.
    let pads: Vec<(f32, f32)> = track
        .boosts()
        .iter()
        .map(|&(p, _)| {
            let spot = track.locate(p, None);
            (spot.s, spot.lateral)
        })
        .collect();
    let ramps = track.ramps();
    // The player, while still racing (`others` only holds non-AI cars).
    let player = others.iter().find(|(_, _, c, r)| c.alive() && r.finish.is_none()).map(|(.., r)| r.progress);

    for (me, tf, mut car, mut ai, mut armory, racer) in &mut ais {
        armory.fire = false;
        if !car.alive() || !race.started() || racer.finish.is_some() {
            // Finished cars cruise on, gently.
            car.throttle = if racer.finish.is_some() && car.alive() { 0.3 } else { 0.0 };
            car.handbrake = false;
            if racer.finish.is_none() {
                continue;
            }
        }
        let pos = tf.translation.truncate();
        let fwd = car.forward();
        let speed = car.vel.length();
        let s = racer.progress;

        // Lane: cut to the inside of the next bend...
        let look = 110.0 + speed * 0.4;
        let bend_ahead = (1..=4)
            .map(|k| {
                let d = look * 0.5 * k as f32;
                let i = ((s + d).rem_euclid(track.length) / track.step()) as usize;
                track.sample(i).2
            })
            .fold(0.0_f32, |a, b| if b.abs() > a.abs() { b } else { a });
        let mut lane = (bend_ahead * 160.0).clamp(-0.5, 0.5) * half;
        // ...detour for a power-up if it's near the line ahead and useful...
        let want = |k: PickupKind| match k {
            PickupKind::Health => car.health() < 0.6,
            PickupKind::Turbo => true,
            _ => armory.weapon == Weapon::None,
        };
        for &(p, kind) in &pickups {
            let rel = p - pos;
            if want(kind) && fwd.dot(rel) > 60.0 && rel.length() < 420.0 {
                let spot = track.locate(p, Some(racer.index));
                if (spot.lateral - lane).abs() < half * 0.7 {
                    lane = spot.lateral;
                    break;
                }
            }
        }
        // ...run over boost pads...
        let here = s.rem_euclid(track.length);
        for &(ps, pl) in &pads {
            let d = track.delta(here, ps);
            if d > 40.0 && d < 450.0 {
                lane = pl;
            }
        }
        // ...line up straight and flat out for a jump...
        let jumping = ramps.iter().any(|&r| (-20.0..650.0).contains(&track.delta(here, r)));
        if jumping {
            lane = 0.0;
        }
        // ...and go round a slower car (or a wreck) in the way. Progress
        // wraps per lap, so compare positions on the track, not laps.
        for &(e, _, other_s, other_lat, _) in &field {
            let gap = track.delta(s.rem_euclid(track.length), other_s.rem_euclid(track.length));
            if e != me && gap > 0.0 && gap < 150.0 && (other_lat - lane).abs() < 50.0 {
                let room_left = half - other_lat;
                let room_right = other_lat + half;
                lane = if room_left > room_right { other_lat + 60.0 } else { other_lat - 60.0 };
            }
        }
        let lane = lane.clamp(-half + 40.0, half - 40.0);
        ai.lane += (lane - ai.lane) * (1.0 - (-3.0 * dt).exp());

        let aim_s = s + look;
        let target = track.point_at(aim_s) + track.dir_at(aim_s).perp() * ai.lane;
        let diff = fwd.angle_to(target - pos);
        if ai_unstick(&mut ai, &mut car, dt, diff) {
            continue;
        }
        if racer.finish.is_some() {
            car.steer = (diff * 3.0).clamp(-1.0, 1.0);
            continue;
        }

        // Rubber band: ease off when well ahead of the player, push a little
        // harder when well behind, so races stay close.
        let pace = match player {
            _ if jumping => 1.0,
            Some(p) if s > p => ai.pace - 0.15 * ((s - p) / 1200.0).min(1.0),
            Some(p) => (ai.pace + 0.1 * ((p - s) / 1200.0).min(1.0)).min(1.0),
            None => ai.pace,
        };

        // Fastest speed that still makes every bend coming up, given room to brake.
        let brake = 650.0;
        let allowed = (0..16)
            .map(|k| {
                let d = 20.0 + 40.0 * k as f32;
                let v = corner_speed(car.model, track.curvature_at(s + d), ai.aggression);
                (v * v + 2.0 * brake * d).sqrt()
            })
            .fold(f32::MAX, f32::min)
            .min(car.model.top_speed * pace);
        car.steer = (diff * 3.0).clamp(-1.0, 1.0);
        car.throttle = if diff.abs() > 1.4 {
            0.6 // pointing the wrong way: turn round
        } else if speed > allowed + 25.0 {
            -1.0
        } else if speed > allowed {
            0.2
        } else {
            pace
        };
        car.handbrake = diff.abs() > 1.0 && speed > 220.0;

        // Shoot at whoever is in front, drop mines on whoever is behind.
        let in_sights = |max: f32, cone: f32| {
            field.iter().any(|&(e, p, _, _, alive)| {
                let rel = p - pos;
                alive && e != me && rel.length() < max && rel.length() > 30.0 && fwd.angle_to(rel).abs() < cone
            })
        };
        armory.fire = match armory.weapon {
            Weapon::None => false,
            Weapon::Guns => in_sights(500.0, 0.2),
            Weapon::Rockets => in_sights(800.0, 0.1),
            Weapon::Mines => field.iter().any(|&(e, p, _, _, alive)| {
                let rel = p - pos;
                alive && e != me && rel.length() < 250.0 && fwd.dot(rel.normalize_or_zero()) < -0.7
            }),
        };
    }
}

/// Track every car's progress round the circuit: laps, lap times, finish.
fn track_progress(
    time: Res<Time>,
    track: Res<Track>,
    mut race: ResMut<Race>,
    mut sounds: ResMut<Sounds>,
    mut cars: Query<(&Transform, &Car, &mut Racer, Has<Player>)>,
) {
    let dt = time.delta_secs();
    if !race.started() {
        let before = race.countdown.ceil();
        race.countdown -= dt;
        if race.countdown <= 0.0 {
            sounds.play(Sfx::Go, 0.8);
        } else if race.countdown.ceil() < before {
            sounds.play(Sfx::Beep, 0.7);
        }
        return;
    }
    race.clock += dt;
    let full = track.length * track.laps as f32;
    for (tf, car, mut racer, is_player) in &mut cars {
        let spot = track.locate(tf.translation.truncate(), Some(racer.index));
        let laps_before = racer.laps_done(&track);
        let s_before = racer.progress.rem_euclid(track.length);
        racer.progress += track.delta(s_before, spot.s);
        racer.index = spot.index;
        racer.lateral = spot.lateral;
        if racer.finish.is_some() || !car.alive() {
            continue;
        }
        if racer.laps_done(&track) > laps_before {
            let lap = race.clock - racer.lap_start;
            racer.best_lap = Some(racer.best_lap.map_or(lap, |b| b.min(lap)));
            racer.lap_start = race.clock;
            if is_player && racer.progress < full {
                sounds.play(Sfx::Beep, 0.5);
            }
        }
        if racer.progress >= full {
            racer.finish = Some(race.clock);
        }
    }
}

/// Boost pads, falling into a ravine or river, landing off the track, and
/// spray when driving through water.
#[allow(clippy::too_many_arguments)]
fn race_hazards(
    mut commands: Commands,
    art: Res<Art>,
    mut rng: ResMut<Rng>,
    track: Res<Track>,
    terrain: Res<Terrain>,
    mut sounds: ResMut<Sounds>,
    mut cars: Query<(&mut Transform, &mut Car, &mut Armory, &mut Racer, Has<Player>)>,
) {
    let boosts = track.boosts();
    let jumps = track.jumps();
    let half = track::BOOST_PAD / 2.0;
    for (mut tf, mut car, mut armory, mut racer, is_player) in &mut cars {
        if !car.alive() || car.airborne {
            continue;
        }
        let pos = tf.translation.truncate();
        let vol = if is_player { 1.0 } else { 0.35 };

        for &(at, dir) in &boosts {
            let d = pos - at;
            if d.dot(dir).abs() < half.x && d.dot(dir.perp()).abs() < half.y {
                // A kick on the way in, then turbo for a moment.
                if armory.turbo < BOOST_SECONDS - 0.3 {
                    sounds.play(Sfx::Turbo, vol);
                    let fwd = car.forward();
                    let top = car.model.top_speed * 1.4;
                    car.vel = (car.vel + fwd * BOOST_KICK).clamp_length_max(top);
                }
                armory.turbo = armory.turbo.max(BOOST_SECONDS);
            }
        }

        if terrain.water_depth(pos) > 0.0 && car.vel.length() > 60.0 && rng.next_f32() < 0.8 {
            let back = pos - car.forward() * car.model.size.x * 0.4;
            let side = car.forward().perp() * rng.range(-1.0, 1.0) * car.model.size.y * 0.6;
            let vel = -car.vel * 0.3 + car.forward().perp() * rng.range(-120.0, 120.0);
            commands.spawn((
                RoundEntity,
                Spark { vel, life: rng.range(0.2, 0.45) },
                Mesh3d(art.spark_mesh.clone()),
                MeshMaterial3d(art.spray_mat.clone()),
                Transform::from_translation((back + side).extend(tf.translation.z + 4.0)),
            ));
        }

        // Down in a ravine or the river, or landed beside the track: put the
        // car back on the track, a run-up before any jump it missed.
        let fell = terrain.cut_depth(pos) > 40.0 && !terrain.on_deck(pos);
        let off = racer.lateral.abs() > track.width / 2.0 + track::BARRIER + 40.0;
        if !(fell || off) {
            continue;
        }
        let here = racer.progress.rem_euclid(track.length);
        let back = jumps
            .iter()
            .map(|&j| track.delta(here, j))
            .find(|d| (-400.0..300.0).contains(d))
            .map_or(40.0, |d| d + 450.0);
        let s = racer.progress - back;
        let at = track.point_at(s);
        tf.translation = at.extend(terrain.height(at));
        car.heading = track.dir_at(s).to_angle();
        car.vel = Vec2::ZERO;
        car.spin = 0.0;
        car.vz = 0.0;
        racer.progress = s;
        racer.index = track.locate(at, None).index;
        racer.lateral = 0.0;
        sounds.play(Sfx::Crash, vol * 0.6);
    }
}

/// A streak of foam drifting down the river.
#[derive(Component)]
struct Ripple {
    vel: Vec2,
    life: f32,
    max_life: f32,
    length: f32,
}

/// Foam streaks drift downstream; more of them, and faster, where the river
/// runs shallow (over the ford).
fn river_ripples(
    mut commands: Commands,
    time: Res<Time>,
    art: Res<Art>,
    terrain: Res<Terrain>,
    current: Res<CurrentArena>,
    mut rng: ResMut<Rng>,
    mut ripples: Query<(Entity, &mut Transform, &mut Ripple)>,
) {
    let dt = time.delta_secs();
    for (e, mut tf, mut r) in &mut ripples {
        r.life -= dt;
        if r.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        tf.translation += (r.vel * dt).extend(0.0);
        // Grow in, then shrink away.
        let k = r.life / r.max_life;
        let fade = (k * (1.0 - k) * 4.0).min(1.0);
        tf.scale = Vec3::new(r.length * fade.max(0.05), 2.2 * fade.max(0.05), 1.0);
    }

    let Some((level, river)) = terrain.water else { return };
    let dir = (river.b - river.a).normalize();
    let half = current.def().size / 2.0 + Vec2::splat(300.0);
    // Try a handful of random spots on the visible river each frame.
    for _ in 0..((1200.0 * dt).ceil() as usize) {
        let along = rng.range(0.0, river.a.distance(river.b));
        let side = rng.range(-1.0, 1.0) * (river.half_width + river.bank);
        let p = river.a + dir * along + dir.perp() * side;
        if p.x.abs() > half.x || p.y.abs() > half.y {
            continue;
        }
        let depth = terrain.water_depth(p);
        if depth <= 1.0 {
            continue;
        }
        // Shallow water is fast and broken; deep water is calm.
        let shallow = depth < 15.0;
        if !shallow && rng.next_f32() > 0.5 {
            continue;
        }
        let speed = if shallow { rng.range(70.0, 110.0) } else { rng.range(25.0, 45.0) };
        let life = rng.range(1.2, 2.4);
        let length = if shallow { rng.range(10.0, 22.0) } else { rng.range(18.0, 34.0) };
        commands.spawn((
            RoundEntity,
            Ripple { vel: dir * speed, life, max_life: life, length },
            Mesh3d(art.square.clone()),
            MeshMaterial3d(art.ripple_mat.clone()),
            Transform::from_translation(p.extend(level + 0.4))
                .with_rotation(Quat::from_rotation_z(dir.to_angle()))
                .with_scale(Vec3::splat(0.01)),
            bevy::light::NotShadowCaster,
        ));
    }
}

/// Finishing order so far: finished cars by time, then running cars by
/// distance, then wrecks.
fn race_order<'a>(racers: impl Iterator<Item = (Entity, &'a Car, &'a Racer)>) -> Vec<Entity> {
    let mut v: Vec<(Entity, &Car, &Racer)> = racers.collect();
    let key = |c: &Car, r: &Racer| match r.finish {
        Some(t) => (0, t),
        None if c.alive() => (1, -r.progress),
        None => (2, -r.progress),
    };
    v.sort_by(|a, b| {
        let (ka, kb) = (key(a.1, a.2), key(b.1, b.2));
        ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
    });
    v.into_iter().map(|(e, ..)| e).collect()
}

fn update_countdown(race: Option<Res<Race>>, mut text: Query<&mut Text, With<CountdownText>>) {
    let (Some(race), Ok(mut text)) = (race, text.single_mut()) else { return };
    let want = if !race.started() {
        format!("{}", race.countdown.ceil() as i32)
    } else if race.clock < 0.8 {
        "GO!".to_string()
    } else {
        String::new()
    };
    if text.0 != want {
        text.0 = want;
    }
}

fn car_physics(
    time: Res<Time>,
    terrain: Res<Terrain>,
    race: Option<Res<Race>>,
    mut q: Query<(&mut Transform, &mut Car, &mut Armory)>,
) {
    let dt = time.delta_secs();
    // Everyone waits on the grid during the countdown.
    if race.is_some_and(|r| !r.started()) {
        for (_, mut car, mut armory) in &mut q {
            car.vel = Vec2::ZERO;
            car.spin = 0.0;
            armory.fire = false;
        }
        return;
    }
    for (mut tf, mut car, mut armory) in &mut q {
        let m = car.model;
        if car.airborne {
            // Flying: keep going the way it was going.
            armory.turbo = (armory.turbo - dt).max(0.0);
            car.heading += car.spin * dt;
            tf.translation += (car.vel * dt).extend(0.0);
            continue;
        }
        let pos = tf.translation.truncate();
        // Slopes pull everything downhill: valleys collect wrecks.
        let downhill = -terrain.gradient(pos) * terrain::GRAVITY;
        car.vel += downhill * dt;
        // Water slows everything down.
        if terrain.water_depth(pos) > 0.0 {
            car.vel *= (-WATER_DRAG * dt).exp();
        }
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

/// Keep every car on the ground under it, pitched and rolled with the slope;
/// when the ground falls away faster than gravity (a ramp's lip, a ravine),
/// it flies.
fn follow_terrain(time: Res<Time>, terrain: Res<Terrain>, mut q: Query<(&mut Transform, &mut Car)>) {
    let dt = time.delta_secs().max(1e-4);
    for (mut tf, mut car) in &mut q {
        let pos = tf.translation.truncate();
        let ground = terrain.height(pos);
        let before = tf.translation.z;
        car.vz -= FALL_GRAVITY * dt;
        let mut z = before + car.vz * dt;
        if z <= ground {
            // On the ground: ride along with it. A big step up (a wall, the
            // far side of a ravine) just puts the car on top.
            let climb = if ground - before > 30.0 { 0.0 } else { (ground - before) / dt };
            z = ground;
            car.vz = climb.min(600.0);
        }
        car.airborne = z > ground + AIRBORNE;
        tf.translation.z = z;
        tf.rotation = if car.airborne {
            // Nose follows the flight path, a little.
            let pitch = (car.vz / car.vel.length().max(100.0)).atan() * 0.6;
            Quat::from_rotation_z(car.heading) * Quat::from_rotation_y(-pitch)
        } else {
            terrain.resting(pos, car.model.size / 2.0, car.heading)
        };
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
    race: Option<Res<Race>>,
    mut cars: Query<(&mut Transform, &mut Car, Has<Player>)>,
    mut obstacles: Query<(&Obstacle, Option<&mut Explosive>)>,
) {
    let scale = if race.is_some() { RACE_CRASH_DAMAGE } else { 1.0 };
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
            let dmg = (impact - DAMAGE_THRESHOLD * 1.5) * WALL_DAMAGE_SCALE * factor * scale / car.model.armor;
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
        if !car.alive() || car.airborne || !(slide > 110.0 || (car.handbrake && speed > 80.0)) {
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
    race: Option<Res<Race>>,
    mut q: Query<(&mut Transform, &mut Car, Has<Player>)>,
) {
    let scale = if race.is_some() { RACE_CRASH_DAMAGE } else { 1.0 };
    let mut combos = q.iter_combinations_mut();
    while let Some([(mut ta, mut a, a_player), (mut tb, mut b, b_player)]) = combos.fetch_next() {
        let (pa, pb) = (ta.translation.truncate(), tb.translation.truncate());
        // One flying over the other.
        if (ta.translation.z - tb.translation.z).abs() > 25.0 {
            continue;
        }
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
        let dmg = |m: f32| (jolt(m) - DAMAGE_THRESHOLD).max(0.0) * DAMAGE_SCALE * scale;
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
    track: Option<Res<Track>>,
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
        pos = match &track {
            // Somewhere on the track, away from the start.
            Some(t) => {
                let s = rng.range(250.0, t.length - 350.0);
                let side = rng.range(-1.0, 1.0) * (t.width / 2.0 - 45.0);
                t.point_at(s) + t.dir_at(s).perp() * side
            }
            None => Vec2::new(rng.range(-margin.x, margin.x), rng.range(-margin.y, margin.y)),
        };
        let clear_of_cars = cars.iter().all(|t| t.translation.truncate().distance(pos) > 90.0);
        let solid = terrain.cut_depth(pos) < 10.0 || terrain.on_deck(pos);
        if clear_of_cars && solid && obstacles.iter().all(|o| o.distance(pos) > 40.0) {
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

const BAR_WIDTH: f32 = 44.0;

/// Give every new car a floating health bar (plus an arrow for the player).
fn attach_health_bars(
    mut commands: Commands,
    art: Res<Art>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cars: Query<(Entity, Has<Player>), Added<Car>>,
) {
    for (car, is_player) in &cars {
        let flat = |m: &Handle<Mesh>, mat: &Handle<StandardMaterial>, at: Vec3, scale: Vec3| {
            (
                Mesh3d(m.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(at).with_scale(scale),
                NotShadowCaster,
            )
        };
        let unlit = |c: Color| StandardMaterial { base_color: c, unlit: true, ..default() };
        let fill_mat = materials.add(unlit(Color::srgb(0.2, 0.9, 0.2)));
        let dot_mat = materials.add(unlit(Color::WHITE));
        let fill = commands.spawn(flat(&art.square, &fill_mat, Vec3::new(0.0, 0.0, 0.5), Vec3::new(BAR_WIDTH - 4.0, 3.5, 1.0))).id();
        let dot = commands.spawn(flat(&art.dot_mesh, &dot_mat, Vec3::new(BAR_WIDTH / 2.0 + 7.0, 0.0, 0.5), Vec3::splat(4.5))).id();
        let back = commands.spawn(flat(&art.square, &art.bar_back_mat, Vec3::ZERO, Vec3::new(BAR_WIDTH, 7.5, 1.0))).id();
        let mut parts = vec![back, fill, dot];
        if is_player {
            let arrow = flat(&art.arrow_mesh, &art.white_mat, Vec3::new(0.0, 13.0, 0.5), Vec3::ONE);
            parts.push(commands.spawn((arrow, PlayerArrow)).id());
        }
        commands
            .spawn((RoundEntity, HealthBar { car, fill, fill_mat, dot, dot_mat }, Transform::default(), Visibility::default()))
            .add_children(&parts);
    }
}

/// Bobbing arrow over the player's health bar.
#[derive(Component)]
struct PlayerArrow;

/// Keep each health bar over its car, showing health and the weapon carried.
#[allow(clippy::type_complexity)]
fn update_health_bars(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bars: Query<(Entity, &HealthBar, &mut Transform, &mut Visibility)>,
    cars: Query<(&Transform, &Car, &Armory), Without<HealthBar>>,
    mut parts: Query<(&mut Transform, &mut Visibility), (Without<HealthBar>, Without<Car>)>,
    arrows: Query<Entity, With<PlayerArrow>>,
) {
    for (entity, bar, mut tf, mut vis) in &mut bars {
        let Ok((car_tf, car, armory)) = cars.get(bar.car) else {
            commands.entity(entity).despawn();
            continue;
        };
        let show = car.alive();
        let want = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if !show {
            continue;
        }
        // Floats over the roof, just above the car on screen.
        tf.translation = car_tf.translation + Vec3::new(0.0, car.model.size.x * 0.5 + 12.0, cars::roof_height(car.model) + 14.0);

        let health = car.health();
        if let Ok((mut ftf, _)) = parts.get_mut(bar.fill) {
            let w = (BAR_WIDTH - 4.0) * health;
            ftf.scale.x = w.max(0.01);
            ftf.translation.x = -(BAR_WIDTH - 4.0) / 2.0 + w / 2.0;
        }
        let color = Color::srgb((1.0 - health) * 1.6, health * 1.2, 0.15);
        if let Some(mut m) = materials.get_mut(&bar.fill_mat)
            && m.base_color != color
        {
            m.base_color = color;
        }

        let weapon = match armory.weapon {
            Weapon::None => None,
            Weapon::Guns => Some(PickupKind::Guns.color()),
            Weapon::Rockets => Some(PickupKind::Rockets.color()),
            Weapon::Mines => Some(PickupKind::Mines.color()),
        };
        if let Ok((_, mut dvis)) = parts.get_mut(bar.dot) {
            let want = if weapon.is_some() { Visibility::Inherited } else { Visibility::Hidden };
            if *dvis != want {
                *dvis = want;
            }
        }
        if let Some(c) = weapon
            && let Some(mut m) = materials.get_mut(&bar.dot_mat)
            && m.base_color != c
        {
            m.base_color = c;
        }
    }
    let bob = (time.elapsed_secs() * 5.0).sin() * 2.0;
    for arrow in &arrows {
        if let Ok((mut atf, _)) = parts.get_mut(arrow) {
            atf.translation.y = 13.0 + bob;
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

    let showroom = matches!(state.get(), GameState::Garage | GameState::Menu);
    let (target, target_zoom, target_ground, snap) = if showroom {
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
            // Dev aid: ARENA_DRIFT_LOOK=x,y,zoom shows that spot instead.
            Err(_) if let Some((at, z)) = dev_look() => (at, z, terrain.height(at), true),
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

    if !showroom {
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

fn race_time(t: f32) -> String {
    format!("{}:{:04.1}", (t / 60.0) as u32, t % 60.0)
}

#[allow(clippy::type_complexity)]
fn update_hud(
    score: Res<Score>,
    track: Option<Res<Track>>,
    race: Option<Res<Race>>,
    cars: Query<(Entity, &Car, &Armory, Has<Player>, Option<&Racer>)>,
    last: Res<LastInput>,
    mut hud: Query<&mut Text, With<Hud>>,
) {
    let Ok(mut text) = hud.single_mut() else { return };
    let help = match last.0 {
        InputDevice::Keyboard => "W/S or arrows drive  |  Space handbrake  |  Ctrl or F/C/V fire",
        InputDevice::Gamepad => "Left stick or RT/LT drive  |  B/LB handbrake  |  A/RB fire",
    };
    let Some((player, car, armory, _, racer)) = cars.iter().find(|(.., p, _)| *p) else { return };

    let weapon_text = if armory.weapon == Weapon::None {
        armory.weapon.name().to_string()
    } else {
        format!("{} x{}", armory.weapon.name(), armory.ammo)
    };
    let turbo_text = if armory.turbo > 0.0 { format!("   TURBO {:.1}s", armory.turbo) } else { String::new() };
    let status = match (&track, &race, racer) {
        (Some(track), Some(race), Some(racer)) => {
            let order = race_order(cars.iter().filter_map(|(e, c, _, _, r)| r.map(|r| (e, c, r))));
            let place = order.iter().position(|&e| e == player).unwrap_or(0) + 1;
            let lap = (racer.laps_done(track) + 1).min(track.laps);
            let best = racer.best_lap.map_or("-".to_string(), race_time);
            format!(
                "POS {place}/{}   LAP {lap}/{}   TIME {}   BEST {best}",
                order.len(),
                track.laps,
                race_time(race.clock),
            )
        }
        _ => format!(
            "Rivals {}   Wrecked {}   Score {:.0}",
            cars.iter().filter(|(_, c, _, p, _)| !p && c.alive()).count(),
            score.wrecks,
            total_score(&score)
        ),
    };

    text.0 = format!(
        "{}  HP {:>3.0}/{:.0}   Weapon {}{}   {}\n{}",
        car.model.name,
        car.hp.max(0.0),
        car.model.max_hp,
        weapon_text,
        turbo_text,
        status,
        help,
    );
}

fn total_score(score: &Score) -> f32 {
    score.damage_dealt * 10.0 + score.wrecks as f32 * 500.0 + if score.won { 2000.0 } else { 0.0 }
}

// ---------------------------------------------------------------------------
// Round flow
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn check_round_end(
    time: Res<Time>,
    cars: Query<(Entity, &Car, Has<Player>)>,
    racers: Query<(Entity, &Car, &Racer)>,
    track: Option<Res<Track>>,
    race: Option<Res<Race>>,
    mut score: ResMut<Score>,
    mut next: ResMut<NextState<GameState>>,
    sim: Option<ResMut<Sim>>,
) {
    let racing = track.is_some() && race.is_some();
    if let Some(mut sim) = sim {
        sim.clock += time.delta_secs();
        let alive: Vec<&Car> = cars.iter().map(|(_, c, _)| c).filter(|c| c.alive()).collect();
        let over = if racing {
            racers.iter().all(|(_, c, r)| r.finish.is_some() || !c.alive())
        } else {
            alive.len() <= 1
        };
        if !over && sim.clock < SIM_ROUND_LIMIT {
            return;
        }
        for m in sim.field.clone() {
            sim.entries[m] += 1;
        }
        let winner = if racing {
            let order = race_order(racers.iter());
            racers.get(order[0]).ok().filter(|(_, _, r)| r.finish.is_some()).map(|(_, c, _)| c.model)
        } else {
            (alive.len() == 1).then(|| alive[0].model)
        };
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
        if racing {
            for e in race_order(racers.iter()) {
                let (_, c, r) = racers.get(e).unwrap();
                let result = match r.finish {
                    Some(t) => race_time(t),
                    None if c.alive() => format!("{:.1} laps", r.progress / track.as_ref().unwrap().length),
                    None => "wrecked".to_string(),
                };
                eprintln!("SIM     {:<11} {:>10}  best lap {}", c.model.name, result, r.best_lap.map_or("-".into(), race_time));
            }
        }
        sim.clock = 0.0;
        next.set(GameState::GameOver);
        return;
    }

    if racing {
        let Some((player, car, racer)) = racers.iter().find(|(e, ..)| cars.get(*e).is_ok_and(|c| c.2)) else { return };
        if racer.finish.is_some() || !car.alive() {
            let order = race_order(racers.iter());
            score.place = racer.finish.and(order.iter().position(|&e| e == player).map(|p| p + 1));
            score.race_time = racer.finish;
            score.best_lap = racer.best_lap;
            score.won = score.place == Some(1);
            next.set(GameState::GameOver);
        }
        return;
    }

    let player_alive = cars.iter().any(|(_, c, p)| p && c.alive());
    let rivals = cars.iter().filter(|(_, c, p)| !p && c.alive()).count();
    if !player_alive || rivals == 0 {
        score.won = player_alive;
        next.set(GameState::GameOver);
    }
}

fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// When the game-over screen appeared; input is ignored for a moment so a
/// fire button still held from the race doesn't start a rematch.
#[derive(Resource)]
struct GameOverAt(f32);

fn show_game_over(
    time: Res<Time<Real>>,
    mut commands: Commands,
    score: Res<Score>,
    mut sounds: ResMut<Sounds>,
    sim: Option<Res<Sim>>,
    race: Option<Res<Race>>,
) {
    commands.insert_resource(GameOverAt(time.elapsed_secs()));
    if sim.is_some() {
        return;
    }
    let (green, red) = (Color::srgb(0.4, 1.0, 0.5), Color::srgb(1.0, 0.35, 0.3));
    let (headline, color, details) = match (race.is_some(), score.place) {
        (true, Some(place)) => (
            if place == 1 { "YOU WIN!".to_string() } else { format!("FINISHED {}", ordinal(place)) },
            if place == 1 { green } else { Color::srgb(0.95, 0.8, 0.3) },
            format!(
                "Time {}    |    Best lap {}",
                score.race_time.map_or("-".into(), race_time),
                score.best_lap.map_or("-".into(), race_time)
            ),
        ),
        (true, None) => ("WRECKED".to_string(), red, "Did not finish".to_string()),
        (false, _) => (
            if score.won { "LAST CAR RUNNING!".to_string() } else { "WRECKED".to_string() },
            if score.won { green } else { red },
            format!("Score {:.0}    |    {} wrecked", total_score(&score), score.wrecks),
        ),
    };
    let good = score.won || score.place.is_some_and(|p| p <= 2);
    sounds.play(if good { Sfx::Win } else { Sfx::Lose }, 0.8);
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
                Text::new(details),
                TextFont { font_size: FontSize::Px(30.0), ..default() },
                TextColor(Color::WHITE),
            ),
            (
                Text::new("Enter: rematch    |    G: garage    |    M: menu"),
                TextFont { font_size: FontSize::Px(22.0), ..default() },
                TextColor(Color::srgb(0.7, 0.7, 0.7)),
            ),
        ],
    ));
}

fn game_over_input(
    time: Res<Time<Real>>,
    shown: Option<Res<GameOverAt>>,
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
        let fair = 100.0 / sim.field.len().max(1) as f32;
        for ((m, w), entries) in MODELS.iter().zip(sim.wins).zip(sim.entries) {
            if entries == 0 {
                continue;
            }
            // Chance that any one car of this model wins its round; fair is 1 in the field size.
            eprintln!(
                "SIM   T{} {:<11} {:>3} wins / {:>3} cars entered = {:>5.1}% per car (fair {:.1}%)",
                m.tier, m.name, w, entries, w as f32 / entries as f32 * 100.0, fair,
            );
        }
        eprintln!("SIM   draws          {:>3}", sim.draws);
        exit.write(AppExit::Success);
        return;
    }
    if shown.is_some_and(|t| time.elapsed_secs() - t.0 < 1.0) {
        return;
    }
    if keys.just_pressed(KeyCode::Enter) {
        next.set(GameState::Playing);
    } else if keys.just_pressed(KeyCode::KeyG) {
        next.set(GameState::Garage);
    } else if keys.any_just_pressed([KeyCode::KeyM, KeyCode::Escape]) {
        next.set(GameState::Menu);
    }
}
