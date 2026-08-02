//! A full-revolution quadric band whose rim was split by a boolean.
//!
//! `tessellate_solid` shares one polyline per edge so neighbouring faces meet on
//! identical vertices. For a CLOSED circle edge it sampled that polyline from the
//! curve's own parameter origin, `t = 0`, which is wherever the underlying
//! `Circle3D` happens to start — not where the edge's own vertex is. On a rim
//! that closes on a seam vertex the two differ by an arbitrary angle, and the
//! CDT boundary walk that consumes the polyline then jumps by that angle when it
//! crosses from the seam line onto the rim. On a periodic surface the jump
//! unwraps into an extra turn: the walk reported a `u` span of 2.5 turns for a
//! band that is one turn around, the CDT tiled the sheared domain, and the
//! triangles folded back over the cylinder.
//!
//! The body is the one OpenZCAD reports on: a 40 x 24 x 10 plate fused with an
//! r6 h20 boss seated so that part of it overhangs the plate's `x = 0` wall. The
//! fuse splits the boss wall into a tab (below the plate top, over the arc that
//! is outside the wall) and a full ring above it — and the ring's lower rim is
//! three arcs meeting the seam, so it takes the CDT path rather than the
//! structured two-rim band.
//!
//! The mesh stayed closed and 2-manifold throughout, which is why nothing
//! caught it: the folded band is a watertight surface that simply encloses
//! 34 mm3 it should not. `mass_properties` (exact per-face integrals, no mesh)
//! read the body correctly to 6e-15 the whole time, so the two routes disagreed
//! by 0.30 % and only the meshed one was wrong.
//!
//! Everything asserted here is written out from the construction's own
//! dimensions. Nothing is a recorded measurement, and nothing compares one
//! kernel route against another.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{PI, TAU};

use brepkit_math::mat::Mat4;
use brepkit_operations::boolean::{BooleanOp, boolean};
use brepkit_operations::measure::solid_volume;
use brepkit_operations::primitives::{make_box, make_cylinder};
use brepkit_operations::tessellate::{TriangleMesh, tessellate_solid};
use brepkit_operations::transform::transform_solid;
use brepkit_operations::validate;
use brepkit_topology::Topology;
use brepkit_topology::solid::SolidId;

/// Plate and boss in units of the model's own scale factor.
const PLATE_X: f64 = 40.0;
const PLATE_Y: f64 = 24.0;
const PLATE_Z: f64 = 10.0;
const R: f64 = 6.0;
const H: f64 = 20.0;

/// Scale factors, coarsest FIRST so a fix that only holds at unit scale cannot
/// hide behind being swept first. Every length below — the model, the
/// deflection — carries this factor, and every tolerance asserted is relative,
/// so each row is the same problem in different units.
const SCALES: [f64; 3] = [1000.0, 1.0, 0.001];

/// Axis offsets `d` as a FRACTION of the radius: the boss axis stands at
/// `x = d`, so `d < R` leaves `R - d` of it hanging past the `x = 0` wall.
/// Swept rather than pinned at one seating, because the split that triggers the
/// defect is the same at every overhang and a fix has to hold across the range.
///
/// Capped at `0.4 R`. Past about half the radius the overhang thins to a sliver
/// and the solid tessellation stops closing at all — 3198 free edges at `0.6 R`,
/// on every scale, before this fix and after it. That is the sliver band
/// `regress_boss_crossing_a_wall` already documents on the boolean side, not
/// something this fix reaches, so the sweep stays clear of it.
const DEPTH_FRACTIONS: [f64; 4] = [0.4, 0.25, 0.1, 0.05];

/// The seating OpenZCAD reports on: the axis at `x = 3`, half the radius, so
/// 3 mm of the boss hangs past the wall.
const REPORTED_FRACTION: f64 = 0.5;

/// Build the fused body: plate `[0,PLATE_X] x [0,PLATE_Y] x [0,PLATE_Z]` and an
/// `R` x `H` boss on a vertical axis at `(d, PLATE_Y/2)`, both scaled by `s`.
fn build(topo: &mut Topology, s: f64, d: f64) -> SolidId {
    let plate = make_box(topo, PLATE_X * s, PLATE_Y * s, PLATE_Z * s).unwrap();
    let boss = make_cylinder(topo, R * s, H * s).unwrap();
    transform_solid(
        topo,
        boss,
        &Mat4::translation(d * s, PLATE_Y * s / 2.0, 0.0),
    )
    .unwrap();
    boolean(topo, BooleanOp::Fuse, plate, boss).unwrap()
}

/// Area of the boss footprint outside the wall: the circular segment of the
/// disc (centre `x = d`, radius `R`) beyond `x = 0`.
fn segment_outside(d: f64, s: f64) -> f64 {
    let (r, d) = (R * s, d * s);
    r * r * (d / r).acos() - d * (r * r - d * d).sqrt()
}

/// `plate + whole boss - the part of the boss buried in the plate`.
fn closed_form_volume(d: f64, s: f64) -> f64 {
    let (r, h) = (R * s, H * s);
    let (px, py, pz) = (PLATE_X * s, PLATE_Y * s, PLATE_Z * s);
    px * py * pz + PI * r * r * h - (PI * r * r - segment_outside(d, s)) * pz
}

/// Surface area of the same body, face by face: the two plate faces the boss
/// interrupts, the four plate walls (one of them notched by the boss passing
/// through it), the boss's top disc, the full ring of boss wall above the plate,
/// and the tab of boss wall below it over the arc that is outside the wall.
fn closed_form_area(d: f64, s: f64) -> f64 {
    let (r, h, ds) = (R * s, H * s, d * s);
    let (px, py, pz) = (PLATE_X * s, PLATE_Y * s, PLATE_Z * s);
    let disc = PI * r * r;
    let segment = segment_outside(d, s);
    // Half-chord where the boss crosses the x = 0 plane.
    let half_chord = (r * r - ds * ds).sqrt();
    // Arc of the boss wall standing outside the wall plane.
    let exposed_arc = 2.0 * (ds / r).acos();

    let bottom = px * py + segment;
    let top = px * py - (disc - segment);
    let walls_y = 2.0 * px * pz;
    let wall_x_far = py * pz;
    let wall_x_near = py * pz - 2.0 * half_chord * pz;
    let boss_top = disc;
    let boss_ring = TAU * r * (h - pz);
    let boss_tab = exposed_arc * r * pz;

    bottom + top + walls_y + wall_x_far + wall_x_near + boss_top + boss_ring + boss_tab
}

fn mesh_area(mesh: &TriangleMesh) -> f64 {
    mesh.indices
        .chunks_exact(3)
        .map(|t| {
            let (a, b, c) = (
                mesh.positions[t[0] as usize],
                mesh.positions[t[1] as usize],
                mesh.positions[t[2] as usize],
            );
            (b - a).cross(c - a).length() * 0.5
        })
        .sum()
}

/// `(free edges, non-manifold edges)` of a triangle mesh: edges incident to one
/// triangle, and edges incident to three or more.
fn mesh_edge_defects(mesh: &TriangleMesh) -> (usize, usize) {
    use std::collections::HashMap;
    let mut counts: HashMap<(u32, u32), usize> = HashMap::new();
    for tri in mesh.indices.chunks_exact(3) {
        for &(i, j) in &[(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            let key = if i < j { (i, j) } else { (j, i) };
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    (
        counts.values().filter(|&&c| c == 1).count(),
        counts.values().filter(|&&c| c > 2).count(),
    )
}

fn assert_closed_solid(topo: &Topology, solid: SolidId, what: &str) {
    let report = validate::validate_solid(topo, solid).expect("validate");
    assert!(
        report.is_valid(),
        "{what}: not a closed 2-manifold solid: {}",
        report
            .issues
            .iter()
            .filter(|i| i.severity == validate::Severity::Error)
            .map(|i| i.description.clone())
            .collect::<Vec<_>>()
            .join("; ")
    );
}

/// The mesh of the fused body must have the body's own surface area.
///
/// This is the assertion that sees the fold. An inscribed triangle mesh can only
/// come in UNDER the exact area of a boundary that curves away from it, so any
/// excess at all is surface counted twice; the folded band ran 3.8 % over. The
/// bound above is one-sided for exactly that reason and holds at every scale.
///
/// The bound below is the chord deficit of a mesh sampled at 1e-4 of the model's
/// own extent, and it is tight only from unit scale up. Below unit scale a
/// separate, pre-existing defect on the TAB face costs another 0.75 - 1.2 % —
/// deflection-independent, the same at 0.1, 0.01 and 0.001, and present before
/// this fix too — so there the lower bound is only wide enough to catch a mesh
/// that lost a face.
#[test]
fn the_meshed_body_has_the_body_s_own_surface_area() {
    for s in SCALES {
        for f in DEPTH_FRACTIONS {
            let d = R * f;
            let mut topo = Topology::new();
            let solid = build(&mut topo, s, d);
            let what = format!("scale {s}, axis at {f} R");

            assert_closed_solid(&topo, solid, &what);

            let mesh = tessellate_solid(&topo, solid, 1e-4 * s).unwrap();
            let (free, non_manifold) = mesh_edge_defects(&mesh);
            assert_eq!(
                (free, non_manifold),
                (0, 0),
                "{what}: mesh has {free} free and {non_manifold} non-manifold edge(s)"
            );

            let want = closed_form_area(d, s);
            let got = mesh_area(&mesh);
            let rel = (got - want) / want;
            assert!(
                rel <= 1e-9,
                "{what}: mesh area {got:.6} EXCEEDS the closed form {want:.6} \
                 ({:+.4} %) — an inscribed mesh cannot, so this is doubled surface",
                rel * 100.0
            );
            let floor = if s >= 1.0 { -1e-3 } else { -2e-2 };
            assert!(
                rel >= floor,
                "{what}: mesh area {got:.6} against the closed form {want:.6} ({:+.4} %)",
                rel * 100.0
            );
        }
    }
}

/// And the volume that mesh carries is the body's volume.
///
/// The fold enclosed real extra space, so it read HIGH — the same one-sided
/// statement applies, and it is the one OpenZCAD sees: `kernel.volume` reported
/// +0.30 % on this body, converging to +0.3125 % as the deflection tightened,
/// while the exact per-face integral had it right all along.
#[test]
fn the_meshed_body_does_not_enclose_more_than_the_body() {
    for s in SCALES {
        for f in DEPTH_FRACTIONS {
            let d = R * f;
            let mut topo = Topology::new();
            let solid = build(&mut topo, s, d);
            let what = format!("scale {s}, axis at {f} R");

            let want = closed_form_volume(d, s);
            let got = solid_volume(&topo, solid, 1e-4 * s).unwrap();
            let rel = (got - want) / want;
            assert!(
                rel <= 1e-6,
                "{what}: volume {got:.6} EXCEEDS the closed form {want:.6} ({:+.5} %)",
                rel * 100.0
            );
        }
    }
}

/// At the scales where the mesh route is the one that answers, it answers
/// exactly. Held to 1e-4 relative — four orders tighter than the +0.3125 % the
/// fold converged to, and loose enough for the chord deficit at 1e-4 of extent.
///
/// Restricted to unit scale and up on purpose: below it a SEPARATE, pre-existing
/// defect takes over on the tab face, worth a steady -0.44 % at 0.1, 0.01 and
/// 0.001 alike and unmoved by deflection. It is not this fix's (the same -0.44 %
/// sits under the +0.31 % before the fix), and the two assertions above cover
/// every scale.
#[test]
fn the_meshed_volume_is_the_closed_form_at_and_above_unit_scale() {
    for s in [1000.0, 1.0] {
        for f in DEPTH_FRACTIONS {
            let d = R * f;
            let mut topo = Topology::new();
            let solid = build(&mut topo, s, d);
            let want = closed_form_volume(d, s);
            let got = solid_volume(&topo, solid, 1e-4 * s).unwrap();
            let rel = (got - want).abs() / want;
            assert!(
                rel < 1e-4,
                "scale {s}, axis at {f} R: volume {got:.6} against the closed form \
                 {want:.6} ({:.5} %)",
                rel * 100.0
            );
        }
    }
}

/// The body as the product builds it, at its own scale: a 40 x 24 x 10 plate and
/// an r6 h20 boss at `x = 3`. The app read 10984.864189375206 against the hand
/// closed form 10952.079901041901, +0.30 %.
#[test]
fn the_reported_body_measures_its_closed_form() {
    let d = R * REPORTED_FRACTION;
    let mut topo = Topology::new();
    let solid = build(&mut topo, 1.0, d);
    assert_closed_solid(&topo, solid, "the reported body");

    let mesh = tessellate_solid(&topo, solid, 1e-4).unwrap();
    assert_eq!(
        mesh_edge_defects(&mesh),
        (0, 0),
        "the reported body: mesh is not a closed 2-manifold"
    );

    let want_area = closed_form_area(d, 1.0);
    let area = mesh_area(&mesh);
    let area_rel = (area - want_area) / want_area;
    assert!(
        (-1e-3..=1e-9).contains(&area_rel),
        "the reported body: mesh area {area:.6} against the closed form \
         {want_area:.6} ({:+.4} %)",
        area_rel * 100.0
    );

    let want = closed_form_volume(d, 1.0);
    let got = solid_volume(&topo, solid, 1e-4).unwrap();
    let rel = (got - want).abs() / want;
    assert!(
        rel < 1e-4,
        "the reported body: volume {got:.12} against the closed form {want:.12} ({:.5} %)",
        rel * 100.0
    );
}
