//! A hive: a lens of wax stuck to the surface, with hexagonal cells carved into
//! it in a packed grid. The walls between cells are what the carving leaves.

use mscent_math::rng::Rng;

use super::Plan;
use super::site::{Frame, Site};

/// Cosine and sine of 30 degrees: the grid's second axis.
const COS30: f64 = 0.866_025_403_784_438_6;
/// Cells carved this far below the lens' top surface, in cells, at least.
const FLOOR_CELLS: f64 = 2.0;
/// Walls are at least this many cells thick, so they survive meshing.
const WALL_CELLS: f64 = 2.0;

pub(super) fn grow(plan: &mut Plan, f: &Frame, site: &Site, rng: &mut Rng) {
    let s = site.size;
    let reach = 3.4 * s;
    let lift = 1.5 * s;
    // The lens bulges out of the surface, thickest in the middle: a curved comb.
    plan.ellipsoid(f.origin, f.n, (lift, reach), false, site.material);
    let apothem = 0.42 * s;
    let wall = (0.14 * s).max(WALL_CELLS * site.cell);
    let pitch = 2.0 * apothem + wall;
    let span = (reach / pitch).ceil() as i32;
    for i in -span..=span {
        for j in -span..=span {
            // Neighbouring cells sit across flats: at 90 and 30 degrees.
            let (x, y) = (
                pitch * COS30 * f64::from(i),
                pitch * (f64::from(j) + 0.5 * f64::from(i)),
            );
            let rho = x.hypot(y);
            if rho + apothem > reach * 0.92 {
                continue;
            }
            let top = lift * (1.0 - (rho / reach).powi(2)).max(0.0).sqrt();
            let floor = (FLOOR_CELLS * site.cell).max(0.12 * s) + rng.range_f64(0.0, 0.25 * s);
            if top < floor + site.cell {
                continue;
            }
            let (bottom, ceiling) = (floor, top + 0.5 * s);
            let centre = f.at(x, y, 0.5 * (bottom + ceiling));
            plan.hex(centre, f.n, f.u, (apothem, 0.5 * (ceiling - bottom)));
        }
    }
}
