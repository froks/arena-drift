//! Oriented-box geometry for car collisions.

use bevy::math::Vec2;

/// An oriented bounding box: a car's footprint.
#[derive(Clone, Copy, Debug)]
pub struct Obb {
    pub center: Vec2,
    /// Unit axes: [forward, left].
    pub axes: [Vec2; 2],
    /// Half length along forward, half width along left.
    pub half: Vec2,
}

/// Result of two boxes overlapping.
#[derive(Clone, Copy, Debug)]
pub struct Contact {
    /// Unit normal pointing from box A towards box B.
    pub normal: Vec2,
    pub depth: f32,
    /// Approximate world-space contact point.
    pub point: Vec2,
}

/// 2D cross product (z component).
pub fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

impl Obb {
    pub fn new(center: Vec2, heading: f32, size: Vec2) -> Self {
        let fwd = Vec2::from_angle(heading);
        Self { center, axes: [fwd, fwd.perp()], half: size / 2.0 }
    }

    pub fn corners(&self) -> [Vec2; 4] {
        let [f, l] = self.axes;
        let (x, y) = (f * self.half.x, l * self.half.y);
        let c = self.center;
        [c + x + y, c - x + y, c - x - y, c + x - y]
    }

    /// Half the box's extent when projected onto `axis`.
    fn projected_radius(&self, axis: Vec2) -> f32 {
        self.half.x * axis.dot(self.axes[0]).abs() + self.half.y * axis.dot(self.axes[1]).abs()
    }

    pub fn closest_point(&self, p: Vec2) -> Vec2 {
        let d = p - self.center;
        let x = d.dot(self.axes[0]).clamp(-self.half.x, self.half.x);
        let y = d.dot(self.axes[1]).clamp(-self.half.y, self.half.y);
        self.center + self.axes[0] * x + self.axes[1] * y
    }

    /// Distance from a point to the box (0 inside).
    pub fn distance(&self, p: Vec2) -> f32 {
        self.closest_point(p).distance(p)
    }

    /// Radius of a circle that always contains the box.
    pub fn bounding_radius(&self) -> f32 {
        self.half.length()
    }

    /// Separating-axis test. Returns how to push B out of A, plus a contact point.
    pub fn collide(&self, other: &Obb) -> Option<Contact> {
        let delta = other.center - self.center;
        if delta.length() > self.bounding_radius() + other.bounding_radius() {
            return None;
        }
        let mut best: Option<(f32, Vec2, bool)> = None; // (depth, normal, axis belongs to self)
        for (i, axis) in self.axes.iter().chain(other.axes.iter()).enumerate() {
            let dist = delta.dot(*axis);
            let overlap = self.projected_radius(*axis) + other.projected_radius(*axis) - dist.abs();
            if overlap <= 0.0 {
                return None;
            }
            // Slightly prefer the earlier axis on ties so results are stable.
            if best.is_none_or(|(d, _, _)| overlap < d - 1e-4) {
                let normal = if dist >= 0.0 { *axis } else { -*axis };
                best = Some((overlap, normal, i < 2));
            }
        }
        let (depth, normal, from_self) = best?;

        // Contact: the corner(s) of the "incident" box that dig deepest.
        let point = if from_self {
            deepest(&other.corners(), -normal)
        } else {
            deepest(&self.corners(), normal)
        };
        Some(Contact { normal, depth, point })
    }
}

/// Average of the corners furthest along `dir` (two when an edge is flush).
fn deepest(corners: &[Vec2; 4], dir: Vec2) -> Vec2 {
    let max = corners.iter().map(|c| c.dot(dir)).fold(f32::MIN, f32::max);
    let (sum, n) = corners
        .iter()
        .filter(|c| c.dot(dir) > max - 1.0)
        .fold((Vec2::ZERO, 0.0), |(s, n), c| (s + *c, n + 1.0));
    sum / n
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    fn car(x: f32, y: f32, heading: f32) -> Obb {
        Obb::new(Vec2::new(x, y), heading, Vec2::new(50.0, 30.0))
    }

    #[test]
    fn separated_boxes_dont_collide() {
        assert!(car(0.0, 0.0, 0.0).collide(&car(60.0, 0.0, 0.0)).is_none());
        // Long cars side by side: circles of radius ~29 would overlap at 32 apart, boxes don't.
        assert!(car(0.0, 0.0, 0.0).collide(&car(0.0, 32.0, 0.0)).is_none());
    }

    #[test]
    fn head_on_overlap_pushes_along_forward_axis() {
        let c = car(0.0, 0.0, 0.0).collide(&car(45.0, 0.0, 0.0)).unwrap();
        assert!((c.normal - Vec2::X).length() < 1e-4, "{c:?}");
        assert!((c.depth - 5.0).abs() < 1e-3, "{c:?}");
        assert!((c.point.x - 22.5).abs() < 2.6, "{c:?}"); // on the touching faces
    }

    #[test]
    fn normal_points_from_a_to_b() {
        let c = car(0.0, 0.0, 0.0).collide(&car(-45.0, 0.0, 0.0)).unwrap();
        assert!((c.normal + Vec2::X).length() < 1e-4, "{c:?}");
    }

    #[test]
    fn t_bone_hits_the_side() {
        // B points straight down into A's left flank.
        let c = car(0.0, 0.0, 0.0).collide(&car(5.0, 38.0, -FRAC_PI_2)).unwrap();
        assert!((c.normal - Vec2::Y).length() < 1e-4, "{c:?}");
        assert!((c.depth - 2.0).abs() < 1e-3, "{c:?}");
        assert!((c.point.y - 15.0).abs() < 2.1 && c.point.x.abs() < 20.0, "{c:?}");
    }

    #[test]
    fn closest_point_and_distance() {
        let b = car(0.0, 0.0, 0.0);
        assert_eq!(b.distance(Vec2::new(10.0, 5.0)), 0.0);
        assert!((b.distance(Vec2::new(35.0, 0.0)) - 10.0).abs() < 1e-4);
        let rotated = car(0.0, 0.0, FRAC_PI_2);
        assert!((rotated.distance(Vec2::new(0.0, 35.0)) - 10.0).abs() < 1e-4);
    }
}
