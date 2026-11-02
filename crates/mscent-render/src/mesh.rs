//! Low-poly vertex-lit meshes for enemies and the goal.
//!
//! Lighting is computed once per vertex in world space and interpolated
//! across each triangle. The rasterizer already interpolates texture
//! coordinates perspective-correctly, so the light rides in the `u`
//! coordinate of a 1x1 texture: no extra interpolation path is needed and
//! clipping against the near plane keeps the light correct.

use mscent_math::Vec3;

use crate::camera::Camera;
use crate::framebuffer::Framebuffer;
use crate::raster::{DrawStats, PolyList, Vertex};
use crate::scene::eye_of;
use crate::shade::{Fog, SpanEnd, SpanShader, lerp_rgb, mul_rgb};
use crate::texture::Texture;

/// Largest vertex index a mesh can address.
pub const MAX_VERTICES: usize = 1 << 16;
/// Brightest light factor, where 1.0 leaves the colour unchanged.
const MAX_LIGHT: f32 = 2.0;

/// One mesh vertex in model space. `normal` should be unit length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshVertex {
    pub pos: Vec3,
    pub normal: Vec3,
}

/// A triangle, counter-clockwise seen from outside, with a flat colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Triangle {
    pub indices: [u16; 3],
    pub color: u32,
}

/// An indexed triangle mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    verts: Vec<MeshVertex>,
    tris: Vec<Triangle>,
}

/// Where a mesh sits in the world: a yaw about +Z, uniform scale, position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub pos: Vec3,
    pub yaw: f32,
    pub scale: f32,
}

/// A point light with linear-squared falloff to zero at `radius`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    pub pos: Vec3,
    pub radius: f32,
    pub intensity: f32,
}

/// Lights acting on meshes. `sun_dir` points from the surface toward the sun.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshLighting {
    pub ambient: f32,
    pub sun_dir: Vec3,
    pub sun: f32,
    pub points: Vec<PointLight>,
}

impl Default for MeshLighting {
    fn default() -> Self {
        Self {
            ambient: 0.35,
            sun_dir: Vec3::new(0.3, -0.4, 0.85)
                .normalized()
                .unwrap_or(Vec3::ZERO),
            sun: 0.75,
            points: Vec::new(),
        }
    }
}

impl Mesh {
    /// `None` if there are too many vertices, an index is out of range, or
    /// any coordinate is not finite.
    pub fn new(verts: Vec<MeshVertex>, tris: Vec<Triangle>) -> Option<Self> {
        let finite = verts
            .iter()
            .all(|v| v.pos.is_finite() && v.normal.is_finite());
        let in_range = tris
            .iter()
            .all(|t| t.indices.iter().all(|&i| usize::from(i) < verts.len()));
        (finite && in_range && verts.len() <= MAX_VERTICES).then_some(Self { verts, tris })
    }

    pub fn vertices(&self) -> &[MeshVertex] {
        &self.verts
    }

    pub fn triangles(&self) -> &[Triangle] {
        &self.tris
    }

    /// A double pyramid: a regular `sides`-gon of `radius` in the XY plane
    /// with apexes `top` above and `bottom` below it. Normals are smooth, so
    /// the shape reads as rounded despite its few triangles. `None` unless
    /// `3 <= sides <= 32` and the dimensions are positive and finite.
    pub fn bipyramid(sides: u16, radius: f32, top: f32, bottom: f32, color: u32) -> Option<Self> {
        let dims_ok = [radius, top, bottom]
            .iter()
            .all(|d| d.is_finite() && *d > 0.0);
        if !(3..=32).contains(&sides) || !dims_ok {
            return None;
        }
        let mut verts = Vec::with_capacity(usize::from(sides) + 2);
        for i in 0..sides {
            let a = f32::from(i) * core::f32::consts::TAU / f32::from(sides);
            let (s, c) = a.sin_cos();
            verts.push(MeshVertex {
                pos: Vec3::new(c * radius, s * radius, 0.0),
                normal: Vec3::new(c, s, 0.0),
            });
        }
        let (apex_top, apex_bottom) = (sides, sides + 1);
        verts.push(MeshVertex {
            pos: Vec3::new(0.0, 0.0, top),
            normal: Vec3::new(0.0, 0.0, 1.0),
        });
        verts.push(MeshVertex {
            pos: Vec3::new(0.0, 0.0, -bottom),
            normal: Vec3::new(0.0, 0.0, -1.0),
        });
        let mut tris = Vec::with_capacity(usize::from(sides) * 2);
        for i in 0..sides {
            let j = (i + 1) % sides;
            tris.push(Triangle {
                indices: [i, j, apex_top],
                color,
            });
            tris.push(Triangle {
                indices: [j, i, apex_bottom],
                color,
            });
        }
        Self::new(verts, tris)
    }

    /// A smooth UV sphere of `radius` with `rings` latitude bands and
    /// `sectors` longitude slices. `None` unless `2 <= rings <= 32`,
    /// `3 <= sectors <= 64` and `radius` is positive and finite.
    pub fn sphere(rings: u16, sectors: u16, radius: f32, color: u32) -> Option<Self> {
        let ok = (2..=32).contains(&rings) && (3..=64).contains(&sectors);
        if !ok || !radius.is_finite() || radius <= 0.0 {
            return None;
        }
        let vert = |n: Vec3| MeshVertex {
            pos: n * radius,
            normal: n,
        };
        let mut verts = vec![
            vert(Vec3::new(0.0, 0.0, 1.0)),
            vert(Vec3::new(0.0, 0.0, -1.0)),
        ];
        for r in 1..rings {
            let phi = f32::from(r) * core::f32::consts::PI / f32::from(rings);
            let (sp, cp) = phi.sin_cos();
            for k in 0..sectors {
                let a = f32::from(k) * core::f32::consts::TAU / f32::from(sectors);
                verts.push(vert(Vec3::new(a.cos() * sp, a.sin() * sp, cp)));
            }
        }
        // Vertex of ring `r` (1-based) and slice `k`.
        let at = |r: u16, k: u16| 2 + (r - 1) * sectors + k % sectors;
        let mut tris = Vec::new();
        let mut tri = |a, b, c| {
            tris.push(Triangle {
                indices: [a, b, c],
                color,
            });
        };
        for k in 0..sectors {
            tri(at(1, k), at(1, k + 1), 0);
            tri(at(rings - 1, k + 1), at(rings - 1, k), 1);
        }
        for r in 1..rings - 1 {
            for k in 0..sectors {
                tri(at(r, k), at(r + 1, k), at(r, k + 1));
                tri(at(r, k + 1), at(r + 1, k), at(r + 1, k + 1));
            }
        }
        Self::new(verts, tris)
    }

    /// Several meshes in one, each moved by an offset. `None` if the result
    /// would have too many vertices.
    pub fn combine(parts: &[(Mesh, Vec3)]) -> Option<Self> {
        let mut verts = Vec::new();
        let mut tris = Vec::new();
        for (mesh, offset) in parts {
            let base = u16::try_from(verts.len()).ok()?;
            verts.extend(mesh.verts.iter().map(|v| MeshVertex {
                pos: v.pos + *offset,
                normal: v.normal,
            }));
            for t in &mesh.tris {
                let [a, b, c] = t.indices.map(|i| base.checked_add(i));
                tris.push(Triangle {
                    indices: [a?, b?, c?],
                    color: t.color,
                });
            }
        }
        Self::new(verts, tris)
    }

    /// The enemy: a round red body with two bone horns and a glowing yellow
    /// eye on its +X side, about 1.5 units tall.
    pub fn enemy() -> Self {
        let horn = || Self::bipyramid(5, 0.12, 0.6, 0.05, 0xffe8_e0c0);
        let parts: Option<Vec<(Mesh, Vec3)>> = [
            (
                Self::sphere(8, 14, 0.7, 0xffc0_2828),
                Vec3::new(0.0, 0.0, 0.1),
            ),
            (horn(), Vec3::new(0.0, 0.4, 0.6)),
            (horn(), Vec3::new(0.0, -0.4, 0.6)),
            (
                Self::sphere(4, 8, 0.2, 0xfff8_e840),
                Vec3::new(0.58, 0.0, 0.25),
            ),
        ]
        .into_iter()
        .map(|(m, o)| Some((m?, o)))
        .collect();
        parts
            .and_then(|p| Self::combine(&p))
            .unwrap_or_else(Self::empty)
    }

    /// The goal: a tall gold octahedron wearing a ring of six cyan gems,
    /// about 2.2 units tall.
    pub fn goal() -> Self {
        let mut parts = vec![(Self::bipyramid(4, 0.5, 1.1, 1.1, 0xfff0_c030), Vec3::ZERO)];
        for i in 0u8..6 {
            let a = f32::from(i) * core::f32::consts::TAU / 6.0;
            let gem = Self::bipyramid(4, 0.14, 0.3, 0.3, 0xff50_e0f0);
            parts.push((gem, Vec3::new(a.cos() * 0.95, a.sin() * 0.95, 0.0)));
        }
        let parts: Option<Vec<(Mesh, Vec3)>> =
            parts.into_iter().map(|(m, o)| Some((m?, o))).collect();
        parts
            .and_then(|p| Self::combine(&p))
            .unwrap_or_else(Self::empty)
    }

    /// An enemy on the player's side: the enemy's shape, in green.
    pub fn friendly() -> Self {
        let parts = [
            (
                Self::sphere(8, 14, 0.7, 0xff28_b048),
                Vec3::new(0.0, 0.0, 0.1),
            ),
            (
                Self::sphere(4, 8, 0.2, 0xfff8_e840),
                Vec3::new(0.58, 0.0, 0.25),
            ),
        ];
        Self::assembled(parts)
    }

    /// Something to shoot: an amber gem on a dark ball.
    pub fn switch() -> Self {
        Self::assembled([
            (Self::sphere(6, 10, 0.4, 0xff30_3038), Vec3::ZERO),
            (
                Self::bipyramid(4, 0.22, 0.65, 0.65, 0xffff_a020),
                Vec3::ZERO,
            ),
        ])
    }

    /// Something to pick up: a flat red diamond.
    pub fn keycard() -> Self {
        Self::assembled([(Self::bipyramid(4, 0.3, 0.3, 0.3, 0xffe0_3030), Vec3::ZERO)])
    }

    /// Something to use: a tall dark post with a cyan screen near the top.
    pub fn terminal() -> Self {
        Self::assembled([
            (Self::bipyramid(4, 0.3, 0.6, 0.6, 0xff40_4858), Vec3::ZERO),
            (
                Self::sphere(4, 8, 0.16, 0xff40_e0f0),
                Vec3::new(0.0, 0.0, 0.3),
            ),
        ])
    }

    /// A plain four-sided block.
    pub fn crate_box() -> Self {
        Self::assembled([(Self::bipyramid(4, 0.5, 0.5, 0.5, 0xff90_7040), Vec3::ZERO)])
    }

    fn assembled<const N: usize>(parts: [(Option<Self>, Vec3); N]) -> Self {
        let parts: Option<Vec<(Mesh, Vec3)>> =
            parts.into_iter().map(|(m, o)| Some((m?, o))).collect();
        parts
            .and_then(|p| Self::combine(&p))
            .unwrap_or_else(Self::empty)
    }

    fn empty() -> Self {
        Self {
            verts: Vec::new(),
            tris: Vec::new(),
        }
    }
}

impl Pose {
    fn point(&self, p: Vec3) -> Vec3 {
        let (s, c) = self.yaw.sin_cos();
        let q = p * self.scale;
        Vec3::new(q.x * c - q.y * s, q.x * s + q.y * c, q.z) + self.pos
    }

    fn direction(&self, d: Vec3) -> Vec3 {
        let (s, c) = self.yaw.sin_cos();
        Vec3::new(d.x * c - d.y * s, d.x * s + d.y * c, d.z)
    }
}

impl MeshLighting {
    /// Light factor at a world point with unit normal `n`, in `0..=2`.
    pub fn at(&self, p: Vec3, n: Vec3) -> f32 {
        let mut light = self.ambient + self.sun * n.dot(self.sun_dir).max(0.0);
        for pl in &self.points {
            let to = pl.pos - p;
            let d = to.length();
            if d < pl.radius && d > 1e-4 {
                let fall = 1.0 - d / pl.radius;
                light += pl.intensity * (n.dot(to) / d).max(0.0) * fall * fall;
            }
        }
        light.clamp(0.0, MAX_LIGHT)
    }
}

/// Flat colour scaled by a light factor that arrives in the `u` coordinate.
struct GouraudShader {
    color: u32,
    fog: Option<Fog>,
    light0: f32,
    light_step: f32,
    fog0: f32,
    fog_step: f32,
}

impl SpanShader for GouraudShader {
    fn begin_span(&mut self, start: &SpanEnd, end: &SpanEnd, len: u32) {
        let n = len.max(1) as f32;
        self.light0 = start.u * 256.0;
        self.light_step = (end.u - start.u) * 256.0 / n;
        if let Some(fog) = self.fog {
            self.fog0 = fog.amount(start.depth);
            self.fog_step = (fog.amount(end.depth) - self.fog0) / n;
        }
    }

    fn shade(&self, i: u32, _texel: u32, _fu: i32, _fv: i32) -> u32 {
        let i = i as f32;
        let k = (self.light0 + self.light_step * i).clamp(0.0, MAX_LIGHT * 256.0);
        let lit = mul_rgb(self.color, k as u32);
        match self.fog {
            Some(fog) => {
                let amount = (self.fog0 + self.fog_step * i).clamp(0.0, 256.0);
                lerp_rgb(lit, fog.color, amount as u32)
            }
            None => lit,
        }
    }
}

/// Draw `mesh` at `pose`, lit per vertex, with back faces culled. Draw it
/// after the level so the shared z-buffer hides it behind walls.
pub fn draw_mesh(
    fb: &mut Framebuffer,
    cam: &Camera,
    mesh: &Mesh,
    pose: &Pose,
    lighting: &MeshLighting,
    fog: Option<Fog>,
) -> DrawStats {
    // The texture only supplies the carrier for the light; its texel is unused.
    let carrier = Texture::solid(0xffff_ffff);
    let mut list = PolyList::default();
    list_mesh(cam, mesh, pose, lighting, &carrier, &mut list);
    fill_meshes(fb, &list, &carrier, fog)
}

/// Adds the camera-facing triangles of `mesh` at `pose` to `list`, lit per
/// vertex, for [`fill_meshes`]. `carrier` must be the texture filling will use.
pub(crate) fn list_mesh(
    cam: &Camera,
    mesh: &Mesh,
    pose: &Pose,
    lighting: &MeshLighting,
    carrier: &Texture,
    list: &mut PolyList,
) {
    if !pose.pos.is_finite() || !pose.yaw.is_finite() || !pose.scale.is_finite() {
        return;
    }
    let eye = eye_of(cam);
    let world: Vec<(Vec3, f32)> = mesh
        .verts
        .iter()
        .map(|v| {
            let p = pose.point(v.pos);
            (p, lighting.at(p, pose.direction(v.normal)))
        })
        .collect();
    for tri in &mesh.tris {
        let [a, b, c] = tri.indices.map(|i| world.get(usize::from(i)).copied());
        let (Some(a), Some(b), Some(c)) = (a, b, c) else {
            continue;
        };
        let facing = (b.0 - a.0).cross(c.0 - a.0).dot(eye - a.0);
        if facing <= 0.0 {
            continue;
        }
        let verts = [a, b, c].map(|(pos, light)| Vertex::new(pos, light, 0.0));
        list.push(cam, &verts, carrier, tri.color);
    }
}

/// Fills the triangles of `list` that touch the rows `fb` holds. Their tag is
/// their flat colour. Draw after the level, for the shared z-buffer.
pub(crate) fn fill_meshes(
    fb: &mut Framebuffer,
    list: &PolyList,
    carrier: &Texture,
    fog: Option<Fog>,
) -> DrawStats {
    let mut total = DrawStats::default();
    for poly in list.in_rows(fb.row_range()) {
        let mut shader = GouraudShader {
            color: poly.tag,
            fog,
            light0: 0.0,
            light_step: 0.0,
            fog0: 0.0,
            fog_step: 0.0,
        };
        let st = list.fill(fb, poly, carrier, &mut shader);
        total.pixels += st.pixels;
        total.spans += st.spans;
    }
    total
}

#[cfg(test)]
#[path = "mesh_tests.rs"]
mod mesh_tests;
