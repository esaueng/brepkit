//! Validation and construction for faces carrying inner (hole) wires.
//!
//! `addHolesToFace` and `makeFaceFromWires` are the only two entry points in
//! the kernel that attach an inner wire to a face, and whatever they build
//! is fed straight into `extrude`. An inner wire that is open, that does not
//! lie on the face's surface, or that escapes the outer boundary produces a
//! face that no downstream code can interpret — extrude walks it anyway and
//! emits a solid that looks plausible and is not watertight. The checks here
//! are what turn that class of failure into a typed error at the boundary.
//!
//! # What is and is not checked
//!
//! Always, for every surface type:
//! - each hole wire is topologically closed
//!   ([`validate_wire_closed`](brepkit_topology::validation::validate_wire_closed));
//! - each hole wire is distinct from the outer wire and from every other
//!   inner wire on the face;
//! - every sampled point of each hole wire lies on the face's surface within
//!   tolerance (the generalization of "coplanar" to non-planar surfaces).
//!
//! Only for planar faces:
//! - each hole lies inside the outer wire;
//! - holes do not overlap or nest inside each other.
//!
//! Containment on a curved surface would need a UV-space point-in-polygon
//! test with periodic-seam unwrapping, which `brepkit-check` keeps private.
//! Rather than approximate it — a wrong containment answer on a cylinder
//! would reject valid input — the two positional checks are skipped there
//! and this limitation is stated rather than hidden. Every hole that reaches
//! `extrude` on a *planar* face, which is the path glyph outlines and every
//! sketch-region profile take, is fully checked.
//!
//! Hole winding is deliberately NOT constrained: `extrude` detects inner-wire
//! winding per wire (`brepkit_operations::winding::inner_wire_is_cw`) and
//! builds correct side faces for either, so requiring CW here would reject
//! input the kernel already handles.

use brepkit_check::util::{point_in_polygon_3d, wire_polygon_curve_sampled};
use brepkit_geometry::extrema::{
    point_to_cone, point_to_cylinder, point_to_sphere, point_to_surface, point_to_torus,
};
use brepkit_math::tolerance::Tolerance;
use brepkit_math::vec::{Point2, Point3, Vec3};
use brepkit_topology::Topology;
use brepkit_topology::face::{Face, FaceId, FaceSurface};
use brepkit_topology::wire::WireId;

use crate::error::WasmError;
use crate::helpers::polygons_overlap_2d;

/// Samples contributed by a closed curved edge when outlining a wire.
const CLOSED_CURVE_SAMPLES: usize = 32;

/// Samples contributed by an open curved edge (an arc, a bezier segment)
/// when outlining a wire. A glyph counter is a handful of short beziers, so
/// one chord per segment would both miss the bow of the curve and leave the
/// containment test with a polygon too coarse to trust.
const OPEN_CURVE_SAMPLES: usize = 8;

/// Relative on-surface tolerance for closed-form surfaces (plane, cylinder,
/// cone, sphere, torus). Their point-to-surface distance is exact, so this
/// only has to absorb the caller's own coordinate round-off.
const EXACT_SURFACE_REL_TOL: f64 = 1e-7;

/// Relative on-surface tolerance for NURBS surfaces. Their closest-point
/// query is iterative rather than closed-form, so a residual well above the
/// linear tolerance is expected for a point that genuinely lies on the
/// surface; holding NURBS to `EXACT_SURFACE_REL_TOL` would reject valid input.
const NURBS_SURFACE_REL_TOL: f64 = 1e-5;

/// Distance from `p` to the (untrimmed) surface `surface`.
///
/// Plane and the four analytic surfaces have closed-form answers. NURBS goes
/// through the grid-seeded Newton projection in `brepkit-geometry` rather
/// than [`FaceSurface::project_point`]: the latter falls back to the domain
/// midpoint when its Newton iteration fails, which would read as an enormous
/// deviation and reject a hole that is in fact on the surface.
fn surface_deviation(surface: &FaceSurface, p: Point3) -> f64 {
    match surface {
        FaceSurface::Plane { normal, d } => {
            let n = *normal;
            n.x()
                .mul_add(p.x(), n.y().mul_add(p.y(), n.z() * p.z()) - *d)
                .abs()
        }
        FaceSurface::Cylinder(c) => point_to_cylinder(p, c).distance,
        FaceSurface::Cone(c) => point_to_cone(p, c).distance,
        FaceSurface::Sphere(s) => point_to_sphere(p, s).distance,
        FaceSurface::Torus(t) => point_to_torus(p, t).distance,
        FaceSurface::Nurbs(n) => point_to_surface(p, n, n.domain_u(), n.domain_v()).distance,
    }
}

/// Relative-to-absolute on-surface tolerance for `surface` at scale `scale`.
fn on_surface_tolerance(surface: &FaceSurface, scale: f64) -> f64 {
    let rel = if matches!(surface, FaceSurface::Nurbs(_)) {
        NURBS_SURFACE_REL_TOL
    } else {
        EXACT_SURFACE_REL_TOL
    };
    // Never tighter than the workspace linear tolerance, so unit-scale
    // geometry is not held to a tolerance smaller than the kernel's own.
    (rel * scale).max(Tolerance::new().linear)
}

/// Largest absolute coordinate over `points`, floored at 1.0.
///
/// Used to turn the relative tolerances above into absolute ones. Flooring
/// at 1.0 keeps sub-millimetre geometry from being held to an absurdly tight
/// bound.
fn coordinate_scale(points: &[Point3]) -> f64 {
    points.iter().fold(1.0, |acc, p| {
        acc.max(p.x().abs()).max(p.y().abs()).max(p.z().abs())
    })
}

/// Outline a wire as a 3D polygon, with curved edges chorded finely enough
/// for a containment test.
fn wire_outline(topo: &Topology, wire: WireId) -> Result<Vec<Point3>, WasmError> {
    let pts = wire_polygon_curve_sampled(topo, wire, CLOSED_CURVE_SAMPLES, OPEN_CURVE_SAMPLES)?;
    if pts.len() < 3 {
        return Err(WasmError::InvalidInput {
            reason: format!(
                "wire {} outlines only {} point(s); a face boundary needs at least 3",
                wire.index(),
                pts.len()
            ),
        });
    }
    Ok(pts)
}

/// The normal used for planar containment tests, or `None` when the face is
/// not planar (containment is not checked there — see the module docs).
fn planar_normal(surface: &FaceSurface) -> Option<Vec3> {
    match surface {
        FaceSurface::Plane { normal, .. } => Some(*normal),
        FaceSurface::Nurbs(_)
        | FaceSurface::Cylinder(_)
        | FaceSurface::Cone(_)
        | FaceSurface::Sphere(_)
        | FaceSurface::Torus(_) => None,
    }
}

/// True when every point of `inner` lies inside the polygon `outer`.
///
/// Returns the index of the first escaping point instead of a bare bool so
/// the error can name it.
fn first_point_outside(inner: &[Point3], outer: &[Point3], normal: Vec3) -> Option<usize> {
    inner
        .iter()
        .position(|p| !point_in_polygon_3d(p, outer, &normal))
}

/// Drop a planar 3D polygon into 2D along the axis the plane normal is
/// most aligned with — the same projection [`point_in_polygon_3d`] uses,
/// so the two agree about which polygon a point is in.
fn project_to_2d(points: &[Point3], normal: Vec3) -> Vec<Point2> {
    let (ax, ay, az) = (normal.x().abs(), normal.y().abs(), normal.z().abs());
    if az >= ax && az >= ay {
        points.iter().map(|p| Point2::new(p.x(), p.y())).collect()
    } else if ay >= ax {
        points.iter().map(|p| Point2::new(p.x(), p.z())).collect()
    } else {
        points.iter().map(|p| Point2::new(p.y(), p.z())).collect()
    }
}

/// True when two planar loops share any area.
///
/// Vertex containment alone is not enough: two rectangles crossing in a
/// plus sign have no vertex of either inside the other, so the edge-crossing
/// half of [`polygons_overlap_2d`] is what catches them.
fn loops_overlap(a: &[Point3], b: &[Point3], normal: Vec3) -> bool {
    polygons_overlap_2d(&project_to_2d(a, normal), &project_to_2d(b, normal))
}

/// Validate a set of hole wires against a face's outer wire and surface.
///
/// `existing_inner` are the inner wires the face already carries (empty when
/// building a face from scratch); `new_holes` are the wires being added.
/// Both are checked for mutual overlap, so adding a second copy of an
/// existing hole is rejected.
///
/// # Errors
///
/// Returns [`WasmError::InvalidInput`] naming the offending wire when a hole
/// duplicates another wire, is not closed, leaves the face's surface, escapes
/// the outer wire, or overlaps another hole. Returns
/// [`WasmError::Check`] / [`WasmError::Topology`] if the topology cannot be
/// walked at all.
pub fn validate_hole_wires(
    topo: &Topology,
    surface: &FaceSurface,
    outer_wire: WireId,
    existing_inner: &[WireId],
    new_holes: &[WireId],
) -> Result<(), WasmError> {
    // ── Identity: a hole may not be the outer wire or an existing hole ──
    for (i, &hole) in new_holes.iter().enumerate() {
        if hole == outer_wire {
            return Err(WasmError::InvalidInput {
                reason: format!(
                    "hole wire {i} (wire {}) is the face's own outer wire",
                    hole.index()
                ),
            });
        }
        if existing_inner.contains(&hole) {
            return Err(WasmError::InvalidInput {
                reason: format!(
                    "hole wire {i} (wire {}) is already an inner wire of this face",
                    hole.index()
                ),
            });
        }
        if new_holes[..i].contains(&hole) {
            return Err(WasmError::InvalidInput {
                reason: format!("hole wire {} (wire {}) is listed twice", i, hole.index()),
            });
        }
    }

    // ── Closedness ────────────────────────────────────────────────
    for (i, &hole) in new_holes.iter().enumerate() {
        let wire = topo.wire(hole)?;
        brepkit_topology::validation::validate_wire_closed(wire, topo).map_err(|e| {
            WasmError::InvalidInput {
                reason: format!(
                    "hole wire {i} (wire {}) is not a closed loop: {e}",
                    hole.index()
                ),
            }
        })?;
    }

    // ── Outlines ──────────────────────────────────────────────────
    let outer_poly = wire_outline(topo, outer_wire)?;
    let new_polys = new_holes
        .iter()
        .map(|&h| wire_outline(topo, h))
        .collect::<Result<Vec<_>, _>>()?;

    let mut scale = coordinate_scale(&outer_poly);
    for poly in &new_polys {
        scale = scale.max(coordinate_scale(poly));
    }
    let surf_tol = on_surface_tolerance(surface, scale);

    // ── On the face's surface ─────────────────────────────────────
    for (i, poly) in new_polys.iter().enumerate() {
        for p in poly {
            let dev = surface_deviation(surface, *p);
            // `>` rather than `!(<= )` keeps clippy happy, but NaN must not
            // slip through as "on the surface" — reject it explicitly.
            if dev.is_nan() || dev > surf_tol {
                return Err(WasmError::InvalidInput {
                    reason: format!(
                        "hole wire {} (wire {}) does not lie on the face's surface: \
                         point ({:.6}, {:.6}, {:.6}) is {dev:.3e} away, tolerance {surf_tol:.3e}",
                        i,
                        new_holes[i].index(),
                        p.x(),
                        p.y(),
                        p.z(),
                    ),
                });
            }
        }
    }

    // ── Containment and hole-vs-hole overlap (planar faces only) ──
    let Some(normal) = planar_normal(surface) else {
        return Ok(());
    };

    for (i, poly) in new_polys.iter().enumerate() {
        if let Some(k) = first_point_outside(poly, &outer_poly, normal) {
            let p = poly[k];
            return Err(WasmError::InvalidInput {
                reason: format!(
                    "hole wire {} (wire {}) is not contained in the face's outer wire: \
                     point ({:.6}, {:.6}, {:.6}) lies outside it",
                    i,
                    new_holes[i].index(),
                    p.x(),
                    p.y(),
                    p.z(),
                ),
            });
        }
    }

    // Two holes of one face must be disjoint. A hole nested in another, or
    // crossing it, describes a region that is already void — the face it
    // produces has no consistent interior. Both cases show up as a point of
    // one loop landing inside the other, so one symmetric test covers them.
    let existing_polys = existing_inner
        .iter()
        .map(|&w| wire_outline(topo, w))
        .collect::<Result<Vec<_>, _>>()?;

    for (i, poly) in new_polys.iter().enumerate() {
        let others = existing_polys
            .iter()
            .zip(existing_inner.iter())
            .map(|(p, w)| (p, *w, true))
            .chain(
                new_polys
                    .iter()
                    .zip(new_holes.iter())
                    .enumerate()
                    .filter(|&(j, _)| j != i)
                    .map(|(_, (p, w))| (p, *w, false)),
            );
        for (other_poly, other_wire, other_is_existing) in others {
            if loops_overlap(poly, other_poly, normal) {
                let which = if other_is_existing {
                    "an existing inner wire"
                } else {
                    "another new hole wire"
                };
                return Err(WasmError::InvalidInput {
                    reason: format!(
                        "hole wire {} (wire {}) overlaps {which} (wire {}); \
                         holes of one face must be disjoint",
                        i,
                        new_holes[i].index(),
                        other_wire.index()
                    ),
                });
            }
        }
    }

    Ok(())
}

/// Build a new face from `outer_wire` plus validated `hole_wires`.
///
/// The surface is taken from `surface`; the caller supplies it because
/// `addHolesToFace` reuses the source face's surface while
/// `makeFaceFromWires` derives one from the outer wire.
///
/// # Errors
///
/// Propagates every error from [`validate_hole_wires`].
pub fn build_holed_face(
    topo: &mut Topology,
    surface: FaceSurface,
    outer_wire: WireId,
    existing_inner: &[WireId],
    new_holes: &[WireId],
) -> Result<FaceId, WasmError> {
    validate_hole_wires(topo, &surface, outer_wire, existing_inner, new_holes)?;

    let mut inner = existing_inner.to_vec();
    inner.extend_from_slice(new_holes);
    Ok(topo.add_face(Face::new(outer_wire, inner, surface)))
}
