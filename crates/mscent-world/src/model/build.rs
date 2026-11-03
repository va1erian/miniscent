//! Pieces to meshes. Solids become faces directly, since overlaps are left to the
//! z-buffer: only solids a cut touches go through CSG, which takes thousands of
//! times as long and gives over twice the triangles. Voxels take the usual path.

use mscent_math::{Aabb, Vec3, Vec3d, epsilon};

use super::cost::{VOLUME_ORDER, estimated_ns, part_box};
use super::{Blueprint, Hard, MAX_MODEL_COORD, MAX_MODEL_NS, ModelError, Motion, Part, Piece};
use crate::brush::{Brush, Op};
use crate::compile;
use crate::faces::{Face, brush_faces, extract_faces};
use crate::voxel::{ChunkMesh, Context, DabOp, Lod, Mesher, apply_dab};

/// Levels of detail of a part: full, half and quarter.
pub const LEVELS: usize = 3;

/// The material the surrounding brush of a part's CSG gives its faces, which no
/// role can name, so they can be told apart and dropped.
const SURROUND: u16 = u16::MAX;
/// Room around what a part's CSG holds, so the surrounding brush touches nothing.
const SURROUND_MARGIN: f64 = 4.0;
/// A lower level of a chunk may have one folded triangle in this many.
const MIN_UNFOLDED_SHARE: usize = 50;

/// One vertex of a part, in model units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartVertex {
    pub pos: Vec3,
    /// Unit length, pointing out of the surface.
    pub normal: Vec3,
}

/// One level of detail of a part. Triangles wind counter-clockwise from outside.
/// Voxel surfaces share vertices and shade smoothly; each solid face has its own,
/// which is what keeps a hard edge hard.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartMesh {
    pub vertices: Vec<PartVertex>,
    pub triangles: Vec<[u16; 3]>,
    /// Index into [`Blueprint::skins`] of each triangle.
    pub roles: Vec<u8>,
    /// Empty when the mesh has no vertices.
    pub bounds: Option<Aabb>,
}

/// A rigid part, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelPart {
    pub lods: [PartMesh; LEVELS],
    pub pivot: Vec3,
    pub axis: Vec3,
    pub motion: Motion,
}

/// A built model. Pure in the blueprint: the same one gives the same bits.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub parts: Vec<ModelPart>,
}

/// Builds a model, or refuses before any work if it would take too long.
pub fn build(blueprint: &Blueprint) -> Result<Model, ModelError> {
    let ns = estimated_ns(blueprint);
    if ns > MAX_MODEL_NS {
        return Err(ModelError::TooCostly { ns });
    }
    let parts = blueprint
        .parts
        .iter()
        .map(|p| build_part(p, blueprint.skins.len()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Model { parts })
}

fn build_part(part: &Part, roles: usize) -> Result<ModelPart, ModelError> {
    let cuts = cut_brushes(part)?;
    let mut lods: [PartMesh; LEVELS] = Default::default();
    add_solids(part, &cuts, &mut lods)?;
    add_voxels(part, &cuts, roles, &mut lods)?;
    Ok(ModelPart {
        lods,
        pivot: part.pivot.to_f32(),
        axis: part.axis.to_f32(),
        motion: part.motion,
    })
}

/// A part's cuts as subtract brushes, each after the part's volume in order.
fn cut_brushes(part: &Part) -> Result<Vec<Brush>, ModelError> {
    let mut order = VOLUME_ORDER;
    let mut out = Vec::new();
    for piece in &part.pieces {
        if let Piece::Cut { shape, role } = piece {
            order += 1;
            let brush = shape
                .brush(Op::Subtract, order)
                .map_err(ModelError::Brush)?;
            out.push(brush.with_material(u16::from(*role)));
        }
    }
    Ok(out)
}

fn overlaps(a: &Aabb, b: &Aabb) -> bool {
    a.min.x <= b.max.x
        && b.min.x <= a.max.x
        && a.min.y <= b.max.y
        && b.min.y <= a.max.y
        && a.min.z <= b.max.z
        && b.min.z <= a.max.z
}

fn add_solids(
    part: &Part,
    cuts: &[Brush],
    lods: &mut [PartMesh; LEVELS],
) -> Result<(), ModelError> {
    let mut touched = Vec::new();
    for piece in &part.pieces {
        let Piece::Solid {
            shape,
            role,
            detail,
        } = piece
        else {
            continue;
        };
        let brush = solid_brush(shape, *role)?;
        let bounds = brush.bounds();
        if cuts.iter().any(|c| overlaps(&c.bounds(), &bounds)) {
            touched.push(brush);
            continue;
        }
        let levels = if *detail { 1 } else { LEVELS };
        for face in brush_faces(&brush) {
            for mesh in &mut lods[..levels] {
                push_face(mesh, &face)?;
            }
        }
    }
    if !touched.is_empty() {
        for face in cut_faces(&touched, cuts)? {
            for mesh in lods.iter_mut() {
                push_face(mesh, &face)?;
            }
        }
    }
    Ok(())
}

fn solid_brush(shape: &Hard, role: u8) -> Result<Brush, ModelError> {
    let brush = shape.brush(Op::Add, 0).map_err(ModelError::Brush)?;
    Ok(brush.with_material(u16::from(role)))
}

/// The surface of `solids` less `cuts`. The world starts solid, so a brush around
/// them gives them an outside, and its faces are dropped. Only the cuts that touch
/// a solid go in.
fn cut_faces(solids: &[Brush], cuts: &[Brush]) -> Result<Vec<Face>, ModelError> {
    let mut lo = Vec3d::new(MAX_MODEL_COORD, MAX_MODEL_COORD, MAX_MODEL_COORD);
    let mut hi = lo * -1.0;
    let mut brushes = Vec::with_capacity(solids.len() + cuts.len() + 1);
    for solid in solids {
        let b = solid.bounds();
        lo = Vec3d::new(
            lo.x.min(f64::from(b.min.x)),
            lo.y.min(f64::from(b.min.y)),
            lo.z.min(f64::from(b.min.z)),
        );
        hi = Vec3d::new(
            hi.x.max(f64::from(b.max.x)),
            hi.y.max(f64::from(b.max.y)),
            hi.z.max(f64::from(b.max.z)),
        );
    }
    let margin = Vec3d::new(SURROUND_MARGIN, SURROUND_MARGIN, SURROUND_MARGIN);
    let outer =
        Brush::cuboid(lo - margin, hi + margin, Op::Subtract, 0).map_err(ModelError::Brush)?;
    brushes.push(outer.with_material(SURROUND));
    let mut order = 1;
    for solid in solids {
        brushes.push(ordered(solid, Op::Add, order).map_err(ModelError::Brush)?);
        order += 1;
    }
    for cut in cuts {
        if solids.iter().any(|s| overlaps(&s.bounds(), &cut.bounds())) {
            brushes.push(ordered(cut, Op::Subtract, order).map_err(ModelError::Brush)?);
            order += 1;
        }
    }
    let bsp = compile(&brushes).map_err(ModelError::Csg)?;
    let mut faces = extract_faces(&bsp, &brushes);
    faces.retain(|f| f.material != SURROUND);
    Ok(faces)
}

/// A copy of `brush` at another place in the CSG's order.
fn ordered(brush: &Brush, op: Op, order: u32) -> Result<Brush, crate::brush::BrushError> {
    Brush::new_textured(brush.planes(), brush.texinfos(), op, order)
}

/// Adds a convex face with vertices of its own. Repairs at T-junctions leave extra
/// points along its edges, so a plain fan would make triangles of no area, and the
/// triangles beside them would not share those points: pinholes along the edge.
fn push_face(mesh: &mut PartMesh, face: &Face) -> Result<(), ModelError> {
    let normal = face.plane.to_f32().normal;
    let points: Vec<Vec3> = face.polygon.vertices().iter().map(|p| p.to_f32()).collect();
    let role = u8::try_from(face.material).unwrap_or(0);
    let base = mesh.vertices.len();
    if base + points.len() > usize::from(u16::MAX) + 1 {
        return Err(ModelError::TooManyVertices);
    }
    mesh.vertices
        .extend(points.iter().map(|&pos| PartVertex { pos, normal }));
    for corners in ears(&points) {
        mesh.triangles.push(corners.map(|i| (base + i) as u16));
        mesh.roles.push(role);
    }
    extend_bounds(mesh, &points);
    Ok(())
}

fn twice_area(a: Vec3, b: Vec3, c: Vec3) -> f32 {
    (b - a).cross(c - a).length()
}

/// Whether the polygon of `left` has any area, by its fan from the first point.
/// Convex, so the fan's triangles cannot cancel.
fn has_area(points: &[Vec3], left: &[usize]) -> bool {
    let Some((&first, rest)) = left.split_first() else {
        return false;
    };
    rest.windows(2).any(|w| {
        twice_area(points[first], points[w[0]], points[w[1]]) > epsilon::MODEL_MIN_TWICE_AREA
    })
}

/// Triangles of a convex polygon using every point, none without area. It clips a
/// corner at a time, and never one that leaves only a line. A polygon with no area
/// gives nothing.
fn ears(points: &[Vec3]) -> Vec<[usize; 3]> {
    let mut left: Vec<usize> = (0..points.len()).collect();
    let mut out = Vec::with_capacity(points.len().saturating_sub(2));
    while left.len() > 3 {
        let n = left.len();
        let clip = (0..n).find(|&i| {
            let (a, b, c) = (left[(i + n - 1) % n], left[i], left[(i + 1) % n]);
            let mut rest = left.clone();
            rest.remove(i);
            twice_area(points[a], points[b], points[c]) > epsilon::MODEL_MIN_TWICE_AREA
                && has_area(points, &rest)
        });
        let Some(i) = clip else {
            return out;
        };
        out.push([left[(i + n - 1) % n], left[i], left[(i + 1) % n]]);
        left.remove(i);
    }
    if let &[a, b, c] = left.as_slice()
        && twice_area(points[a], points[b], points[c]) > epsilon::MODEL_MIN_TWICE_AREA
    {
        out.push([a, b, c]);
    }
    out
}

fn extend_bounds(mesh: &mut PartMesh, points: &[Vec3]) {
    for &p in points {
        mesh.bounds = Some(match mesh.bounds {
            Some(b) => Aabb::new(
                Vec3::new(b.min.x.min(p.x), b.min.y.min(p.y), b.min.z.min(p.z)),
                Vec3::new(b.max.x.max(p.x), b.max.y.max(p.y), b.max.z.max(p.z)),
            ),
            None => Aabb::new(p, p),
        });
    }
}

/// A part's voxels, edited in file order and meshed with its cuts applied.
fn add_voxels(
    part: &Part,
    cuts: &[Brush],
    roles: usize,
    lods: &mut [PartMesh; LEVELS],
) -> Result<(), ModelError> {
    let Some(place) = part_box(part) else {
        return Ok(());
    };
    let mut volume = place.volume().map_err(ModelError::Volume)?;
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, cuts);
    for piece in &part.pieces {
        let (soft, op) = match *piece {
            Piece::Add { ref shape, role } => (shape, DabOp::Add { material: role }),
            Piece::Paint { ref shape, role } => (shape, DabOp::Paint { material: role }),
            Piece::Carve { ref shape } => (shape, DabOp::Carve),
            Piece::Smooth { ref shape } => (shape, DabOp::Smooth),
            Piece::Solid { .. } | Piece::Cut { .. } => continue,
        };
        if let Some(prim) = soft.prim() {
            apply_dab(&mut volume, &mut ctx, &prim, op);
        }
    }
    let mut mesher = Mesher::new();
    for coord in volume.all_chunks() {
        let Some(chunk) = mesher.mesh(&volume, &mut ctx, coord) else {
            continue;
        };
        if chunk.triangles.is_empty() {
            continue;
        }
        let half = usable_level(chunk.clustered(Lod::Half), &chunk);
        let quarter = usable_level(chunk.clustered(Lod::Quarter), &half);
        let [full_mesh, half_mesh, quarter_mesh] = lods;
        push_chunk(full_mesh, &chunk, roles)?;
        push_chunk(half_mesh, &half, roles)?;
        push_chunk(quarter_mesh, &quarter, roles)?;
    }
    Ok(())
}

/// `lower`, unless merging cells has folded it over itself, as it does on a limb
/// a few cells thick: then `finer`. A folded triangle faces inward and would be
/// culled from outside, leaving a hole. Borders match at every level, so chunks at
/// different levels still meet.
fn usable_level(lower: ChunkMesh, finer: &ChunkMesh) -> ChunkMesh {
    if folded(&lower).saturating_mul(MIN_UNFOLDED_SHARE) > lower.triangles.len() {
        finer.clone()
    } else {
        lower
    }
}

/// How many triangles face against the normals of their own vertices.
fn folded(mesh: &ChunkMesh) -> usize {
    let is_folded = |t: &&[u16; 3]| {
        let [a, b, c] = t.map(|i| mesh.vertices[usize::from(i)]);
        let n = (b.pos - a.pos).cross(c.pos - a.pos);
        n.length() > epsilon::MODEL_MIN_TWICE_AREA && n.dot(a.normal + b.normal + c.normal) <= 0.0
    };
    mesh.triangles.iter().filter(is_folded).count()
}

/// Adds a chunk's surface. Neighbours keep their own copies of the vertices on the
/// border between them, which the surface makes identical, so no crack can open.
fn push_chunk(mesh: &mut PartMesh, chunk: &ChunkMesh, roles: usize) -> Result<(), ModelError> {
    let base = mesh.vertices.len();
    if base + chunk.vertices.len() > usize::from(u16::MAX) + 1 {
        return Err(ModelError::TooManyVertices);
    }
    mesh.vertices
        .extend(chunk.vertices.iter().map(|v| PartVertex {
            pos: v.pos,
            normal: v.normal,
        }));
    for (t, &material) in chunk.triangles.iter().zip(&chunk.materials) {
        mesh.triangles.push(t.map(|i| i + base as u16));
        // Anything a sample cannot name is the first role, not an index past the
        // end for a renderer to trip on.
        let role = if usize::from(material) < roles {
            material
        } else {
            0
        };
        mesh.roles.push(role);
    }
    let points: Vec<Vec3> = chunk.vertices.iter().map(|v| v.pos).collect();
    extend_bounds(mesh, &points);
    Ok(())
}
