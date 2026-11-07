//! Power-up badges plus in-world mine and rocket shapes, built from simple
//! extruded meshes.

use std::f32::consts::{FRAC_PI_4, TAU};

use bevy::prelude::*;

use crate::PickupKind;

#[derive(Clone)]
struct IconPart {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
}

#[derive(Resource)]
pub struct IconArt {
    pickups: Vec<Vec<IconPart>>,
    glow_mesh: Handle<Mesh>,
    glow: Vec<Handle<StandardMaterial>>,
    mine: Vec<IconPart>,
    rocket: Vec<IconPart>,
}

/// Pulsing halo around a pickup.
#[derive(Component)]
pub struct PickupGlow;

/// Unit shapes, extruded 1 unit thick and centred on z = 0.
struct Kit<'a> {
    square: Handle<Mesh>,
    circle: Handle<Mesh>,
    triangle: Handle<Mesh>,
    materials: &'a mut Assets<StandardMaterial>,
}

impl Kit<'_> {
    /// A slab of `mesh` whose bottom sits at `base`, `thick` units tall.
    #[allow(clippy::too_many_arguments)]
    fn part(&mut self, mesh: &Handle<Mesh>, color: Color, x: f32, y: f32, base: f32, thick: f32, sx: f32, sy: f32, angle: f32) -> IconPart {
        IconPart {
            mesh: mesh.clone(),
            material: self.materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 0.5,
                ..default()
            }),
            transform: Transform::from_xyz(x, y, base + thick / 2.0)
                .with_rotation(Quat::from_rotation_z(angle))
                .with_scale(Vec3::new(sx, sy, thick)),
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn rect(&mut self, color: Color, x: f32, y: f32, base: f32, thick: f32, len: f32, width: f32, angle: f32) -> IconPart {
        let m = self.square.clone();
        self.part(&m, color, x, y, base, thick, len, width, angle)
    }
    fn dot(&mut self, color: Color, x: f32, y: f32, base: f32, thick: f32, r: f32) -> IconPart {
        let m = self.circle.clone();
        self.part(&m, color, x, y, base, thick, r, r, 0.0)
    }
    #[allow(clippy::too_many_arguments)]
    fn tri(&mut self, color: Color, x: f32, y: f32, base: f32, thick: f32, size: f32, angle: f32) -> IconPart {
        let m = self.triangle.clone();
        self.part(&m, color, x, y, base, thick, size, size, angle)
    }
}

/// Rotate a whole group of parts around the Z axis.
fn rotated(parts: Vec<IconPart>, angle: f32) -> Vec<IconPart> {
    let q = Quat::from_rotation_z(angle);
    parts
        .into_iter()
        .map(|mut p| {
            p.transform.translation = q * p.transform.translation;
            p.transform.rotation = q * p.transform.rotation;
            p
        })
        .collect()
}

const WHITE: Color = Color::srgb(0.96, 0.96, 0.96);
const FLAME: Color = Color::srgb(1.0, 0.85, 0.3);

/// A rocket lying along +X, `base` at its underside.
fn rocket_shape(k: &mut Kit, body: Color, trim: Color, base: f32) -> Vec<IconPart> {
    vec![
        k.tri(FLAME, -9.0, 0.0, base + 1.0, 3.0, 5.0, std::f32::consts::PI),
        k.rect(trim, -5.5, 3.2, base + 1.5, 1.5, 4.0, 2.5, 0.0),
        k.rect(trim, -5.5, -3.2, base + 1.5, 1.5, 4.0, 2.5, 0.0),
        k.rect(body, -1.0, 0.0, base, 5.0, 11.0, 5.0, 0.0),
        k.tri(trim, 6.5, 0.0, base + 0.5, 4.0, 5.0, 0.0),
    ]
}

fn mine_shape(k: &mut Kit, color: Color, base: f32) -> Vec<IconPart> {
    let mut parts: Vec<IconPart> = (0..8)
        .map(|i| {
            let a = i as f32 * TAU / 8.0;
            k.rect(color, a.cos() * 6.5, a.sin() * 6.5, base, 2.5, 4.0, 2.5, a)
        })
        .collect();
    parts.push(k.dot(Color::srgb(0.25, 0.08, 0.08), 0.0, 0.0, base, 3.5, 6.0));
    parts.push(k.dot(color, 0.0, 0.0, base, 4.5, 2.5));
    parts
}

impl FromWorld for IconArt {
    fn from_world(world: &mut World) -> Self {
        world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
            build_icon_art(&mut meshes, &mut materials)
        })
    }
}

fn build_icon_art(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> IconArt {
    let badge_mesh = meshes.add(Extrusion::new(Circle::new(15.0), 1.0));
    let ring_mesh = meshes.add(Extrusion::new(Annulus::new(13.0, 15.5), 1.0));
    let glow_mesh = meshes.add(Extrusion::new(Annulus::new(16.0, 19.0), 1.0));
    let mut k = Kit {
        square: meshes.add(Extrusion::new(Rectangle::new(1.0, 1.0), 1.0)),
        circle: meshes.add(Extrusion::new(Circle::new(1.0), 1.0)),
        triangle: meshes.add(Extrusion::new(
            Triangle2d::new(Vec2::new(0.5, 0.0), Vec2::new(-0.5, 0.5), Vec2::new(-0.5, -0.5)),
            1.0,
        )),
        materials,
    };

    let pickups = PickupKind::ALL
        .iter()
        .map(|&kind| {
            let c = kind.color();
            let mut parts = vec![
                k.part(&badge_mesh, Color::srgb(0.09, 0.09, 0.11), 0.0, 0.0, 0.0, 3.0, 1.0, 1.0, 0.0),
                k.part(&ring_mesh, c, 0.0, 0.0, 0.0, 4.0, 1.0, 1.0, 0.0),
            ];
            // Symbols stand proud of the badge.
            let (z, t) = (2.0, 3.0);
            parts.extend(match kind {
                PickupKind::Health => vec![
                    k.rect(c, 0.0, 0.0, z, t, 15.0, 5.5, 0.0),
                    k.rect(c, 0.0, 0.0, z, t, 5.5, 15.0, 0.0),
                ],
                PickupKind::Guns => vec![
                    k.rect(c, -5.0, 0.0, z, t, 7.0, 11.0, 0.0),
                    k.rect(c, 2.5, 3.5, z, t, 12.0, 2.5, 0.0),
                    k.rect(c, 2.5, -3.5, z, t, 12.0, 2.5, 0.0),
                    k.rect(WHITE, 9.0, 3.5, z, t + 0.3, 2.0, 3.5, 0.0),
                    k.rect(WHITE, 9.0, -3.5, z, t + 0.3, 2.0, 3.5, 0.0),
                ],
                PickupKind::Rockets => rotated(rocket_shape(&mut k, WHITE, c, z), FRAC_PI_4),
                PickupKind::Mines => mine_shape(&mut k, c, z),
                PickupKind::Turbo => {
                    let mut chevrons = Vec::new();
                    for x in [-3.0, 4.0] {
                        chevrons.push(k.rect(c, x - 2.2, 2.6, z, t, 8.0, 3.0, -FRAC_PI_4));
                        chevrons.push(k.rect(c, x - 2.2, -2.6, z, t, 8.0, 3.0, FRAC_PI_4));
                    }
                    chevrons
                }
            });
            parts
        })
        .collect();

    let glow = PickupKind::ALL
        .iter()
        .map(|kind| {
            k.materials.add(StandardMaterial {
                base_color: kind.color().with_alpha(0.4),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                ..default()
            })
        })
        .collect();
    let mine = mine_shape(&mut k, Color::srgb(0.95, 0.2, 0.2), 0.0);
    let rocket = rocket_shape(&mut k, WHITE, Color::srgb(0.95, 0.35, 0.2), -2.5);

    IconArt { pickups, glow_mesh, glow, mine, rocket }
}

fn attach(commands: &mut Commands, parts: &[IconPart], root: Entity) {
    commands.entity(root).insert(Visibility::default()).with_children(|parent| {
        for p in parts {
            parent.spawn((Mesh3d(p.mesh.clone()), MeshMaterial3d(p.material.clone()), p.transform));
        }
    });
}

pub fn attach_pickup_icon(commands: &mut Commands, art: &IconArt, kind_index: usize, root: Entity) {
    attach(commands, &art.pickups[kind_index], root);
    let glow = commands
        .spawn((
            PickupGlow,
            Mesh3d(art.glow_mesh.clone()),
            MeshMaterial3d(art.glow[kind_index].clone()),
            Transform::from_xyz(0.0, 0.0, 1.0),
        ))
        .id();
    commands.entity(root).add_child(glow);
}

pub fn attach_mine(commands: &mut Commands, art: &IconArt, root: Entity) {
    attach(commands, &art.mine, root);
}

pub fn attach_rocket(commands: &mut Commands, art: &IconArt, root: Entity) {
    attach(commands, &art.rocket, root);
}

/// Pulse the halo around each pickup.
pub fn pulse_pickup_glow(time: Res<Time>, mut q: Query<&mut Transform, With<PickupGlow>>) {
    let t = time.elapsed_secs();
    for mut tf in &mut q {
        let s = 1.0 + 0.12 * (t * 5.0).sin();
        tf.scale = Vec3::new(s, s, 1.0);
    }
}
