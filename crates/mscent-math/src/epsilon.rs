//! Every numeric tolerance in the project lives here, so there is one place to
//! reason about robustness. Do not write a literal tolerance anywhere else.

/// Distance within which an `f64` point counts as lying on a plane. CSG
/// classification and polygon splitting use this. It is far below the snap
/// grid, so snapped geometry never lands in the ambiguous band by accident.
pub const PLANE_EPSILON_F64: f64 = 1.0e-5;

/// Distance within which an `f32` point counts as lying on a plane. Runtime
/// code (collision, rendering) works on `f32` data stored from the `f64`
/// build, so this is looser than the build tolerance.
pub const PLANE_EPSILON_F32: f32 = 1.0e-3;

/// Edge length of the grid that brush planes are snapped to before the CSG
/// build, in world units. A power of two so snapping is exact in binary
/// floating point.
pub const SNAP_GRID: f64 = 0.125;

/// Twice the area below which a triangle of a model solid face is a sliver left
/// by a repaired T-junction, in square model units.
pub const MODEL_MIN_TWICE_AREA: f32 = 1.0e-5;

/// The least a voxel vertex's smoothed normal must agree with its own cell's, as
/// a cosine, to be believed: less, and the reach spanned a thin sheet.
pub const VOXEL_NORMAL_MIN_AGREEMENT: f32 = 0.3;

/// The cosine past which the two count as one normal, and the cell's own stands.
pub const VOXEL_NORMAL_SAME: f32 = 0.9995;

/// How far, in cells, a voxel surface sinks behind the brush faces it meets at
/// the border of a volume, so the two never fight and never leave a gap.
pub const VOXEL_SEAM_BIAS_CELLS: f64 = 1.0 / 16.0;

/// How close to a BSP plane a voxel triangle's corner counts as on both sides
/// of it, when deciding whether air can see the triangle. Well below the seam
/// bias of the smallest cell, so a surface sunk behind a brush face stays buried.
pub const VOXEL_BURIED_MARGIN: f64 = 1.0e-3;

/// Fraction of a shadow ray, at each end, where a voxel triangle does not count
/// as a hit. A ray starts a fixed bias off the surface it lights and ends at a
/// lamp, so only a surface touching an end could reach into this band.
pub const OCCLUDER_RAY_MARGIN: f64 = 1.0e-6;

/// Slack on the edges of a voxel triangle in a shadow ray test, in barycentric
/// units. A ray through the shared edge of two triangles must hit one of them,
/// or light would leak through the seams of a mesh.
pub const OCCLUDER_EDGE_SLACK: f64 = 1.0e-9;

/// A ray whose determinant against a triangle is smaller than this runs in its
/// plane and misses it. The determinant is unnormalised: ray length times
/// twice the triangle's area.
pub const OCCLUDER_PARALLEL: f64 = 1.0e-12;

/// How far, in cells, a voxel cell's light is taken from the surface: its
/// shadow rays start this far along the normal, as a luxel's start the
/// normal bias off its wall.
pub const VOXEL_LIGHT_LIFT_CELLS: f32 = 1.5;

/// Vectors shorter than this are treated as zero when normalizing, so a
/// degenerate input yields `None` instead of NaN or infinity.
pub const MIN_LENGTH_F64: f64 = 1.0e-9;

/// `f32` counterpart of [`MIN_LENGTH_F64`].
pub const MIN_LENGTH_F32: f32 = 1.0e-6;

/// How far in front of the eye the portal flood clips a portal before projecting
/// it, in world units: only to keep the projection finite, not at the near
/// plane, as a ray can cross a portal nearer than that and see what is past it
/// further off. The flood treats an eye nearer a portal's plane than 0.05 as in
/// it, so a crossing it projects is at least 0.05 times the cosine of a screen
/// corner's angle from the view axis in front, well past this.
pub const PORTAL_CLIP_DEPTH: f32 = 1.0e-3;

/// Polygons whose area is below this (in squared world units) are discarded
/// as slivers after splitting.
pub const MIN_POLYGON_AREA: f64 = 1.0e-8;

/// How far off a wall to look for the leaf a surface lookup starts from. Past
/// the collision skin, so a shot's end point is on the empty side.
pub const SURFACE_PROBE: f32 = 0.05;

/// How far in front of a viewport face the points are that a lamp must see to
/// light the vista behind it, so that the face's own wall does not block them.
pub const WINDOW_PROBE_OFFSET: f64 = 0.05;

/// Distance from a face's plane within which a shot's end point counts as on
/// it: the probe distance again, since the end point may be off by that much.
pub const FACE_MATCH_DISTANCE: f32 = 0.05;

/// Least cosine between a shot's normal and a face's for them to be the same
/// surface. Normals come from the same plane, so this only absorbs rounding.
pub const FACE_MATCH_COS: f32 = 0.99;

/// Largest allowed deviation of a unit quaternion's squared norm from one
/// before tests consider it unnormalized.
pub const UNIT_NORM_TOLERANCE: f32 = 1.0e-4;

/// Distance from a particle's contact point at which the six axis probes look
/// for solid when estimating a surface normal. Larger than the bisection
/// error of a bounce, smaller than any wall the editor snaps to.
pub const PARTICLE_NORMAL_PROBE: f32 = 5.0e-2;

/// A bouncing particle meeting a wall slower than this, in units/s, stops
/// instead of bouncing. Well above a tick of gravity (0.15 at 9 units/s^2), so
/// one lying on a floor does not spend a surface query on every tick.
pub const PARTICLE_REST_SPEED: f32 = 0.5;

/// How much nearer than the scene a scorch decal may be and still show: the
/// decal's `1/z` times one plus this must reach the stored `1/z`. A decal lies
/// in the plane of the wall it marks, so the two differ only by rounding in
/// two interpolations; this is relative, so it holds from near to far. Far
/// below the gap between a wall and anything in front of it.
pub const DECAL_DEPTH_BIAS: f32 = 2.0e-3;

/// Above this cosine two rotations are close enough to blend linearly.
pub const SLERP_LINEAR_COS: f32 = 0.9995;

/// How near a bounce's contact point must be to a voxel surface for that
/// surface's own normal to be used, not the six axis probes: four probe
/// distances, so a contact the bisection left a hair off the rock still finds
/// it, while a brush wall that merely shares the volume's box does not.
pub const PARTICLE_VOXEL_NORMAL_REACH: f32 = 0.2;

/// Semi-axis, in tile units, below which an ellipse is treated as this thin: far
/// under a texel of the largest tile, and it keeps the division by it finite.
pub const TEXTURE_MIN_ELLIPSE_AXIS: f32 = 1.0e-6;

/// Width, in tile units, below which an anti-aliased edge is treated as this
/// sharp: the ramp is softness plus a texel, so only a tile of no texels gets
/// here, and it keeps the division by the ramp finite.
pub const TEXTURE_MIN_EDGE_RAMP: f32 = 1.0e-6;

/// Length under which a random vector in the unit square is too short to give a
/// scratch a direction it can be trusted to normalize to.
pub const SCRATCH_MIN_DIRECTION: f32 = 0.05;

/// The most the hardware renderer's frame may differ from the software one's,
/// as a mean over its pixels, in levels of 255 per colour channel.
pub const PARITY_MEAN_LEVELS: f32 = 1.5;

/// The same, over the worst 8x8 block: a shadow or highlight that moved shows
/// here, when a mean over the frame would average it away.
pub const PARITY_BLOCK_LEVELS: f32 = 3.0;

/// The same for `1/z`, as a mean relative difference.
pub const PARITY_DEPTH_RELATIVE: f32 = 1.0e-3;

/// How much `1/z` may jump between neighbouring pixels, relative to the nearer,
/// before they count as an edge that the two renderers may fairly place apart.
pub const PARITY_EDGE_DEPTH: f32 = 0.02;

/// The most a GPU's fine finish may change a frame from its reference finish, as
/// a mean over its pixels, in levels of 255 per colour channel: anti-aliasing and
/// filtering move edges and texels, not the shading. Measured: up to 1.9.
pub const FINE_MEAN_LEVELS: f64 = 3.0;

/// The same, in the worst 8x8 block the parity test reads, away from silhouettes:
/// a shading change on one surface shows here when the mean would average it away.
/// Measured: 1.1 to 6.7, the most where anisotropic filtering sharpens a far
/// floor; half as much specular again, in the fine finish only, gives 11.1.
pub const FINE_BLOCK_LEVELS: f32 = 8.0;

/// How much of the reference finish's distance from the truth (a frame drawn four
/// times larger and averaged down) a fine frame may keep. Anisotropic filtering
/// is most of the gain: without it a fine frame is further from the truth than
/// the reference, and with it 4% to 16% nearer.
pub const FINE_NEARER: f64 = 1.0;

/// The same over the pixels on a silhouette, where multisampling shows: with it a
/// fine frame is 21% to 36% nearer there, without it 1% to 35%.
pub const FINE_EDGES_NEARER: f64 = 0.85;

/// The least share of a fine frame's pixels whose depth agrees with the
/// reference's, to [`PARITY_DEPTH_RELATIVE`]: only silhouettes may differ, and
/// 98% to 99% agree.
pub const FINE_DEPTH_AGREEMENT: f64 = 0.95;
