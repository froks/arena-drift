//! Ground relief. Every arena has a height function; gameplay stays on the
//! XY plane and Z is "up". The ground is a lit mesh built from the height
//! function, with flat markings (patches, oil, paint) baked into its texture.
//!
//! Race tracks add to that: [`Cut`]s carve ravines and river channels into
//! the ground, [`Deck`]s are surfaces cars drive on above (or below) it —
//! ramps, bridges, fords — and there can be water.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use std::sync::Arc;

use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// How strongly slopes pull cars downhill (px/s² per unit of slope).
pub const GRAVITY: f32 = 700.0;
/// How far the ground mesh reaches beyond the arena walls.
pub const MARGIN: f32 = 900.0;
/// Ground mesh resolution in world units.
const CELL: f32 = 20.0;
/// Ground texture resolution.
const TEXELS_PER_UNIT: f32 = 0.4;
const MAX_TEXTURE: f32 = 2048.0;
/// How much steeper than real the ground looks in its lighting.
const HILLSHADE: f32 = 3.0;

/// A trench carved into the ground along a line segment.
#[derive(Clone, Copy, Debug)]
pub struct Cut {
    pub a: Vec2,
    pub b: Vec2,
    /// Half width of the flat bottom.
    pub half_width: f32,
    /// How far the sides spread beyond that (steep for a ravine, gentle for a river).
    pub bank: f32,
    pub depth: f32,
}

impl Cut {
    pub fn distance(&self, p: Vec2) -> f32 {
        let ab = self.b - self.a;
        let t = ((p - self.a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        p.distance(self.a + ab * t)
    }
    /// How much lower the ground is at `p` because of this cut.
    pub fn amount(&self, p: Vec2) -> f32 {
        self.depth * (1.0 - smoothstep(self.half_width, self.half_width + self.bank, self.distance(p)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeckKind {
    Ramp,
    Bridge,
    Ford,
}

/// A driving surface that isn't the ground: a rectangle along `dir`, whose
/// height runs from `h0` at the back to `h1` at the front, dipping by `sag`
/// in the middle (negative arches it up).
#[derive(Clone, Copy, Debug)]
pub struct Deck {
    pub kind: DeckKind,
    pub center: Vec2,
    pub dir: Vec2,
    pub half: Vec2,
    pub h0: f32,
    pub h1: f32,
    pub sag: f32,
}

impl Deck {
    /// Position along the deck (0 at the back, 1 at the front), if `p` is on it.
    fn along(&self, p: Vec2) -> Option<f32> {
        let d = p - self.center;
        let (x, y) = (d.dot(self.dir), d.dot(self.dir.perp()));
        (x.abs() <= self.half.x && y.abs() <= self.half.y).then(|| (x / self.half.x + 1.0) / 2.0)
    }
    pub fn height_at(&self, t: f32) -> f32 {
        self.h0 + (self.h1 - self.h0) * t - self.sag * (std::f32::consts::PI * t).sin()
    }
}

/// The current ground: a height for every point on the XY plane.
#[derive(Resource, Clone)]
pub struct Terrain {
    relief: fn(Vec2, Vec2) -> f32,
    size: Vec2,
    cuts: Arc<[Cut]>,
    decks: Arc<[Deck]>,
    /// Water: its surface height and the channel it fills.
    pub water: Option<(f32, Cut)>,
}

fn flat(_: Vec2, _: Vec2) -> f32 {
    0.0
}

impl Default for Terrain {
    fn default() -> Self {
        Self::new(flat, Vec2::ONE)
    }
}

impl Terrain {
    pub fn new(relief: fn(Vec2, Vec2) -> f32, size: Vec2) -> Self {
        Self { relief, size, cuts: Arc::new([]), decks: Arc::new([]), water: None }
    }

    pub fn with_cuts(mut self, cuts: Vec<Cut>) -> Self {
        self.cuts = cuts.into();
        self
    }

    pub fn with_decks(mut self, decks: Vec<Deck>) -> Self {
        self.decks = decks.into();
        self
    }

    pub fn decks(&self) -> &[Deck] {
        &self.decks
    }

    /// Height of the ground itself (what the ground mesh shows). It stays a
    /// little under any deck, so it never pokes up through a bridge.
    pub fn ground(&self, p: Vec2) -> f32 {
        let ground = (self.relief)(p, self.size) - self.cuts.iter().map(|c| c.amount(p)).sum::<f32>();
        self.decks.iter().filter_map(|d| d.along(p).map(|t| d.height_at(t) - 3.0)).fold(ground, f32::min)
    }

    /// How deep the cuts are at `p` (0 outside them).
    pub fn cut_depth(&self, p: Vec2) -> f32 {
        self.cuts.iter().map(|c| c.amount(p)).sum()
    }

    /// Whether `p` is on a deck (ramp, bridge, ford) rather than the ground.
    pub fn on_deck(&self, p: Vec2) -> bool {
        self.decks.iter().any(|d| d.along(p).is_some())
    }

    /// How deep the water is at `p` (0 when dry).
    pub fn water_depth(&self, p: Vec2) -> f32 {
        match self.water {
            Some((level, channel)) if channel.distance(p) < channel.half_width + channel.bank => {
                (level - self.height(p)).max(0.0)
            }
            _ => 0.0,
        }
    }

    /// Size of the arena inside the walls.
    pub fn size(&self) -> Vec2 {
        self.size
    }

    /// Height of whatever cars drive on at `p`: a deck if there is one,
    /// otherwise the ground.
    pub fn height(&self, p: Vec2) -> f32 {
        for d in self.decks.iter() {
            if let Some(t) = d.along(p) {
                return d.height_at(t);
            }
        }
        self.ground(p)
    }

    /// Uphill direction, scaled by steepness (rise per unit).
    pub fn gradient(&self, p: Vec2) -> Vec2 {
        const E: f32 = 4.0;
        Vec2::new(
            self.height(p + Vec2::X * E) - self.height(p - Vec2::X * E),
            self.height(p + Vec2::Y * E) - self.height(p - Vec2::Y * E),
        ) / (2.0 * E)
    }

    pub fn normal(&self, p: Vec2) -> Vec3 {
        let g = self.gradient(p);
        Vec3::new(-g.x, -g.y, 1.0).normalize()
    }

    /// The lowest ground under a round footprint, so things don't float on slopes.
    pub fn lowest(&self, p: Vec2, r: f32) -> f32 {
        (0..8)
            .map(|i| self.height(p + Vec2::from_angle(i as f32 * std::f32::consts::FRAC_PI_4) * r))
            .fold(self.height(p), f32::min)
    }

    /// Orientation for a box of half size `half` resting on the ground,
    /// facing `heading`: it pitches and rolls with the slope under it.
    pub fn resting(&self, p: Vec2, half: Vec2, heading: f32) -> Quat {
        let fwd = Vec2::from_angle(heading);
        let left = fwd.perp();
        let pitch = (self.height(p + fwd * half.x) - self.height(p - fwd * half.x)) / (2.0 * half.x);
        let roll = (self.height(p + left * half.y) - self.height(p - left * half.y)) / (2.0 * half.y);
        let up = Vec3::new(-pitch, -roll, 1.0).normalize();
        Quat::from_rotation_z(heading) * Quat::from_rotation_arc(Vec3::Z, up)
    }
}

// ---------------------------------------------------------------------------
// Relief helpers for the arena height functions
// ---------------------------------------------------------------------------

pub fn smoothstep(lo: f32, hi: f32, x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Distance from `p` to a centred rectangle of half size `half` (0 inside).
pub fn outside(p: Vec2, half: Vec2) -> f32 {
    (p.abs() - half).max(Vec2::ZERO).length()
}

/// A round hill (positive) or hollow (negative).
pub fn bump(p: Vec2, at: Vec2, radius: f32, height: f32) -> f32 {
    height * (-(p.distance_squared(at)) / (radius * radius)).exp()
}

/// The ground rising beyond the walls, so the arena sits in a dip.
pub fn berm(p: Vec2, size: Vec2, height: f32) -> f32 {
    height * smoothstep(60.0, 500.0, outside(p, size / 2.0))
}

// ---------------------------------------------------------------------------
// Ground texture
// ---------------------------------------------------------------------------

/// A CPU-side picture of the ground that markings get painted into.
pub struct Canvas {
    width: usize,
    height: usize,
    /// World position of the top-left corner.
    origin: Vec2,
    scale: f32,
    pixels: Vec<Vec3>,
}

impl Canvas {
    pub fn new(size: Vec2, background: Color) -> Self {
        let extent = size + Vec2::splat(MARGIN * 2.0);
        let scale = TEXELS_PER_UNIT.min(MAX_TEXTURE / extent.max_element());
        let (width, height) = ((extent.x * scale) as usize, (extent.y * scale) as usize);
        let bg = background.to_srgba();
        Self {
            width,
            height,
            origin: Vec2::new(-extent.x / 2.0, extent.y / 2.0),
            scale,
            pixels: vec![Vec3::new(bg.red, bg.green, bg.blue); width * height],
        }
    }

    /// Paint a shape given as a signed distance function (negative inside),
    /// over the world-space box `[min, max]`, with soft edges.
    fn fill(&mut self, min: Vec2, max: Vec2, color: Color, sd: impl Fn(Vec2) -> f32) {
        let c = color.to_srgba();
        let rgb = Vec3::new(c.red, c.green, c.blue);
        let x0 = (((min.x - self.origin.x) * self.scale).floor().max(0.0)) as usize;
        let x1 = (((max.x - self.origin.x) * self.scale).ceil().max(0.0) as usize).min(self.width);
        let y0 = (((self.origin.y - max.y) * self.scale).floor().max(0.0)) as usize;
        let y1 = (((self.origin.y - min.y) * self.scale).ceil().max(0.0) as usize).min(self.height);
        for y in y0..y1 {
            for x in x0..x1 {
                let p = Vec2::new(
                    self.origin.x + (x as f32 + 0.5) / self.scale,
                    self.origin.y - (y as f32 + 0.5) / self.scale,
                );
                let cover = (0.5 - sd(p) * self.scale).clamp(0.0, 1.0) * c.alpha;
                if cover > 0.0 {
                    let px = &mut self.pixels[y * self.width + x];
                    *px = px.lerp(rgb, cover);
                }
            }
        }
    }

    /// A rotated ellipse with radii `radii`.
    pub fn ellipse(&mut self, center: Vec2, rot: f32, radii: Vec2, color: Color) {
        let r = radii.max_element();
        let inv = Vec2::from_angle(-rot);
        self.fill(center - Vec2::splat(r + 4.0), center + Vec2::splat(r + 4.0), color, |p| {
            let local = inv.rotate(p - center);
            ((local / radii).length() - 1.0) * radii.min_element()
        });
    }

    pub fn ring(&mut self, center: Vec2, inner: f32, outer: f32, color: Color) {
        let r = Vec2::splat(outer + 4.0);
        self.fill(center - r, center + r, color, |p| {
            let d = p.distance(center);
            (d - outer).max(inner - d)
        });
    }

    /// A rotated rectangle of full size `size`.
    pub fn rect(&mut self, center: Vec2, rot: f32, size: Vec2, color: Color) {
        let r = Vec2::splat(size.length() / 2.0 + 4.0);
        let inv = Vec2::from_angle(-rot);
        self.fill(center - r, center + r, color, |p| {
            let q = inv.rotate(p - center).abs() - size / 2.0;
            q.max(Vec2::ZERO).length() + q.max_element().min(0.0)
        });
    }

    fn into_image(self) -> Image {
        let data = self
            .pixels
            .iter()
            .flat_map(|c| {
                let [r, g, b] = c.to_array().map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
                [r, g, b, 255]
            })
            .collect();
        Image::new(
            Extent3d { width: self.width as u32, height: self.height as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        )
    }
}

// ---------------------------------------------------------------------------
// Ground mesh
// ---------------------------------------------------------------------------

/// Build the ground mesh for `terrain` and its material from `canvas`.
pub fn build_ground(
    terrain: &Terrain,
    canvas: Canvas,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> (Mesh3d, MeshMaterial3d<StandardMaterial>) {
    let extent = terrain.size + Vec2::splat(MARGIN * 2.0);
    let (nx, ny) = ((extent.x / CELL).ceil() as usize, (extent.y / CELL).ceil() as usize);
    let min = -extent / 2.0;

    let mut positions = Vec::with_capacity((nx + 1) * (ny + 1));
    let mut normals = Vec::with_capacity(positions.capacity());
    let mut uvs = Vec::with_capacity(positions.capacity());
    let mut colors = Vec::with_capacity(positions.capacity());
    for j in 0..=ny {
        for i in 0..=nx {
            let p = min + Vec2::new(i as f32 / nx as f32, j as f32 / ny as f32) * extent;
            let h = terrain.ground(p);
            positions.push([p.x, p.y, h]);
            // Shade as if slopes were steeper than they are, so the relief
            // reads from straight above (a hillshade, like on a map).
            const E: f32 = 4.0;
            let slope = Vec2::new(
                terrain.ground(p + Vec2::X * E) - terrain.ground(p - Vec2::X * E),
                terrain.ground(p + Vec2::Y * E) - terrain.ground(p - Vec2::Y * E),
            ) / (2.0 * E);
            let g = slope * HILLSHADE;
            normals.push(Vec3::new(-g.x, -g.y, 1.0).normalize().to_array());
            uvs.push([i as f32 / nx as f32, 1.0 - j as f32 / ny as f32]);
            // Hollows a touch darker, high ground a touch lighter.
            let shade = (1.0 + h * 0.004).clamp(0.6, 1.2);
            colors.push([shade, shade, shade, 1.0]);
        }
    }
    let mut indices = Vec::with_capacity(nx * ny * 6);
    let row = (nx + 1) as u32;
    for j in 0..ny as u32 {
        for i in 0..nx as u32 {
            let a = j * row + i;
            indices.extend([a, a + 1, a + row + 1, a, a + row + 1, a + row]);
        }
    }

    let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices));
    let material = StandardMaterial {
        base_color_texture: Some(images.add(canvas.into_image())),
        perceptual_roughness: 1.0,
        reflectance: 0.05,
        ..default()
    };
    (Mesh3d(meshes.add(mesh)), MeshMaterial3d(materials.add(material)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slope(p: Vec2, _: Vec2) -> f32 {
        p.x * 0.5
    }

    #[test]
    fn gradient_and_normal_follow_the_slope() {
        let t = Terrain::new(slope, Vec2::splat(100.0));
        assert!((t.gradient(Vec2::new(10.0, 3.0)) - Vec2::new(0.5, 0.0)).length() < 1e-4);
        let n = t.normal(Vec2::ZERO);
        assert!(n.x < 0.0 && n.z > 0.0 && n.y.abs() < 1e-5);
    }

    #[test]
    fn resting_box_tilts_nose_up_when_facing_uphill() {
        let t = Terrain::new(slope, Vec2::splat(100.0));
        let q = t.resting(Vec2::ZERO, Vec2::new(25.0, 15.0), 0.0);
        let nose = q * Vec3::X;
        assert!(nose.z > 0.3, "nose should point uphill: {nose:?}");
    }
}
