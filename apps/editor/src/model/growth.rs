//! The jobs a stroke thread runs besides dabbing: a generator built at the
//! place the pointer chose, and a field poured into a whole volume.

use mscent_math::Vec3d;
use mscent_world::voxel::fill;

use super::document::Id;
use super::generators::{FieldKind, Kind, Site, field, generate};
use super::sculptor::Sculptor;

/// A whole-job request, run when the stroke ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Request {
    /// Grow `kind` from `seed` where the pointer last met a surface.
    Grow { kind: Kind, seed: u32 },
    /// Replace the samples of volume `id` with a field.
    Fill {
        id: Id,
        field: FieldKind,
        seed: u32,
        wavelength: f64,
    },
}

impl Sculptor<'_> {
    pub fn request(&mut self, request: Request) {
        match request {
            Request::Grow { kind, seed } => self.grow(kind, seed),
            Request::Fill {
                id,
                field: kind,
                seed,
                wavelength,
            } => self.pour(id, field(kind, seed, wavelength)),
        }
    }

    /// Builds `kind` at the chosen place, in the volume that holds it or a new one.
    fn grow(&mut self, kind: Kind, seed: u32) {
        let Some((origin, normal)) = self.site else {
            self.note = Some("Aim at a surface to grow on".into());
            return;
        };
        let held = self.doc.volumes().find(|(_, v)| v.holds(origin));
        let cell = held.map_or(self.tool.cell, |(_, v)| v.cell()).units();
        let site = Site {
            origin,
            normal,
            size: self.tool.radius,
            cell,
            material: self.tool.material,
        };
        let plan = generate(kind, &site, seed);
        let Some((lo, hi)) = plan.bounds() else {
            self.note = Some("That cannot grow here".into());
            return;
        };
        if !self.choose_target(origin, false, (lo, hi)) {
            return;
        }
        for piece in plan.pieces() {
            self.apply_op(&piece.prim, piece.op);
        }
    }

    /// Fills every sample of volume `id` from `field`, in the tool's material.
    fn pour(&mut self, id: Id, field: impl Fn(Vec3d) -> f64) {
        if !self.choose_volume(id) {
            return;
        }
        let material = self.tool.material;
        self.edit(|volume, ctx| fill(volume, ctx, &field, material));
    }
}
