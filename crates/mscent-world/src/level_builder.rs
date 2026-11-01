//! Small helper for authoring brush levels in code: boxes with materials,
//! numbered automatically. Surface re-texturing slabs (floors, ceilings and
//! light panels) are applied after every other brush, so they win ties on
//! the plane they share with the room.

use crate::brush::{Brush, Op};
use crate::materials;
use mscent_math::{DPlane, Vec3d};

enum Shape {
    Cuboid([f64; 3], [f64; 3]),
    Planes(Vec<DPlane>),
}

struct Spec {
    shape: Shape,
    op: Op,
    material: u16,
}

/// Collects brush specs and builds them in a stable order.
#[derive(Default)]
pub struct LevelBuilder {
    main: Vec<Spec>,
    slabs: Vec<Spec>,
}

fn v(p: [f64; 3]) -> Vec3d {
    Vec3d::new(p[0], p[1], p[2])
}

impl LevelBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Carve an empty box; its walls, floor and ceiling use `material`.
    pub fn carve(&mut self, min: [f64; 3], max: [f64; 3], material: u16) -> &mut Self {
        self.main.push(Spec {
            shape: Shape::Cuboid(min, max),
            op: Op::Subtract,
            material,
        });
        self
    }

    /// Fill a box with solid material (a pillar, a step).
    pub fn solid(&mut self, min: [f64; 3], max: [f64; 3], material: u16) -> &mut Self {
        self.main.push(Spec {
            shape: Shape::Cuboid(min, max),
            op: Op::Add,
            material,
        });
        self
    }

    /// Fill an arbitrary convex shape given by outward planes (a ramp).
    pub fn solid_planes(&mut self, planes: &[DPlane], material: u16) -> &mut Self {
        self.main.push(Spec {
            shape: Shape::Planes(planes.to_vec()),
            op: Op::Add,
            material,
        });
        self
    }

    /// Re-texture the floor of the footprint at height `z` by adding a
    /// solid slab just below it. It changes no geometry.
    pub fn floor(&mut self, lo: [f64; 2], hi: [f64; 2], z: f64, material: u16) -> &mut Self {
        self.slab([lo[0], lo[1], z - 1.0], [hi[0], hi[1], z], material)
    }

    /// Re-texture a ceiling patch at height `z` the same way.
    pub fn ceiling(&mut self, lo: [f64; 2], hi: [f64; 2], z: f64, material: u16) -> &mut Self {
        self.slab([lo[0], lo[1], z], [hi[0], hi[1], z + 1.0], material)
    }

    fn slab(&mut self, min: [f64; 3], max: [f64; 3], material: u16) -> &mut Self {
        self.slabs.push(Spec {
            shape: Shape::Cuboid(min, max),
            op: Op::Add,
            material,
        });
        self
    }

    /// A room with `(walls, floor, ceiling)` materials.
    pub fn room(
        &mut self,
        lo: [f64; 3],
        hi: [f64; 3],
        (walls, floor, ceiling): (u16, u16, u16),
    ) -> &mut Self {
        self.carve(lo, hi, walls);
        self.floor([lo[0], lo[1]], [hi[0], hi[1]], lo[2], floor);
        self.ceiling([lo[0], lo[1]], [hi[0], hi[1]], hi[2], ceiling);
        self
    }

    /// A lit panel inset in the ceiling above `centre`, `half` units wide.
    pub fn panel(&mut self, centre: [f64; 2], half: f64, z: f64) -> &mut Self {
        self.ceiling(
            [centre[0] - half, centre[1] - half],
            [centre[0] + half, centre[1] + half],
            z,
            materials::LIGHTPANEL,
        )
    }

    /// Build the brushes. A spec the brush validator rejects is skipped, so
    /// an authoring mistake shows up as a missing room in tests, not a panic.
    pub fn finish(self) -> Vec<Brush> {
        let mut out = Vec::new();
        for (order, s) in (0u32..).zip(self.main.iter().chain(self.slabs.iter())) {
            let built = match &s.shape {
                Shape::Cuboid(lo, hi) => Brush::cuboid(v(*lo), v(*hi), s.op, order),
                Shape::Planes(p) => Brush::new(p, s.op, order),
            };
            if let Ok(b) = built {
                out.push(b.with_material(s.material));
            }
        }
        out
    }
}
