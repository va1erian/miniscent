//! The jobs a stroke thread runs besides dabbing: a generator built at the
//! place the pointer chose, and a field poured into a whole volume.

use mscent_math::Vec3d;
use mscent_world::Op;
use mscent_world::voxel::{CellSize, MountainRange, fill, mountains};

use super::document::{BrushDef, Id, Shape};
use super::generators::{FieldKind, Kind, Site, field, generate};
use super::sculptor::Sculptor;

/// Width of a range per unit of tool radius, and its limits: one-unit cells make
/// a chunk 16 across, and a volume at most 16 chunks.
const RANGE_PER_RADIUS: f64 = 32.0;
const MIN_RANGE: f64 = 64.0;
const MAX_RANGE: f64 = 256.0;
/// Tallest peak as a share of the width.
const PEAK_SHARE: f64 = 0.25;

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
        if !kind.planned() {
            self.raise_mountains(origin, seed);
            return;
        }
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

    /// Builds a range on the surface at `origin` as a volume of its own: wide in
    /// proportion to the tool's radius, in one-unit cells, with its ground a unit
    /// under the surface so it rises out of a floor without a seam.
    fn raise_mountains(&mut self, origin: Vec3d, seed: u32) {
        let side = (self.tool.radius * RANGE_PER_RADIUS)
            .clamp(MIN_RANGE, MAX_RANGE)
            .round();
        let low = Vec3d::new(origin.x - side * 0.5, origin.y - side * 0.5, origin.z - 3.0);
        let peak = side * PEAK_SHARE;
        let high = Vec3d::new(low.x + side, low.y + side, low.z + 2.0 + peak + 1.0);
        let material = self.tool.material;
        let order = self.doc.brushes().len() as u32;
        let range = MountainRange {
            min: low,
            max: high,
            cell: CellSize::One,
            peak,
            seed,
            material,
            order,
        };
        let volume = match mountains(&range) {
            Ok(v) => v,
            Err(e) => {
                self.note = Some(format!("Cannot make mountains: {e}"));
                return;
            }
        };
        if self.others.iter().any(|(_, o)| o.overlaps(&volume)) {
            self.note = Some("Another volume is in the way".into());
            return;
        }
        let def = BrushDef {
            id: self.new_id,
            shape: Shape::Volume(volume.clone()),
            op: Op::Add,
            material: u16::from(material),
            texinfos: Vec::new(),
        };
        let [nx, ny, nz] = volume.chunks();
        let all = (0..nz).flat_map(|z| (0..ny).flat_map(move |y| (0..nx).map(move |x| [x, y, z])));
        if self.adopt(def, volume, None, origin)
            && let Some(target) = self.target.as_mut()
        {
            target.dirty.extend(all);
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
