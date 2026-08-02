//! Regression: a Cut whose tool is disjoint from the target must be an
//! identity, including when the target is a sphere.
//!
//! `make_sphere` builds the ball as TWO spherical patches that share one
//! equatorial loop and differ only in the direction they walk it. Three
//! separate stages of the boolean pipeline keyed faces on the direction-
//! agnostic edge set and so read the two hemispheres as coincident
//! duplicates of each other:
//!
//! 1. `same_domain::build_sd_grouping` grouped them and dropped one as
//!    within-rank residue.
//! 2. `builder_solid::remove_doubled_faces` dropped both as a doubled pair.
//! 3. `MIN_SOLID_FACES` rejected the surviving 2-face solid as too small.
//!
//! Any one of the three sent the operation to the mesh fallback, which
//! replaces the exact spherical surfaces with an inscribed polyhedron and
//! loses ~0.29% of the volume. Every assertion below is against a closed form
//! written out by hand, never against another integrator.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::f64::consts::PI;

use brepkit_math::mat::Mat4;
use brepkit_operations::boolean::{BooleanOp, boolean};
use brepkit_operations::copy::copy_and_transform_solid;
use brepkit_operations::measure::solid_volume;
use brepkit_operations::primitives::{make_box, make_cylinder, make_sphere};
use brepkit_topology::Topology;
use brepkit_topology::explorer::solid_faces;
use brepkit_topology::solid::SolidId;

/// Model scales, listed so the ORDER is a rotation of the natural
/// small-to-large one. A result that only holds for whichever scale runs
/// first is a passing accident; rotating the list makes that visible instead
/// of hiding it behind a lucky first entry.
const SCALES: [f64; 3] = [1000.0, 0.001, 1.0];

/// Segment counts. The sphere's surface is exact regardless, so an identity
/// Cut must return the same volume at every one of these; a result that moves
/// with the segment count is a tessellated stand-in, not the exact surface.
const SEGMENTS: [usize; 3] = [16, 32, 64];

fn sphere_volume(r: f64) -> f64 {
    4.0 / 3.0 * PI * r * r * r
}

/// Edge-use census over every face of the solid (outer shell plus cavities).
fn edge_use_counts(topo: &Topology, solid: SolidId) -> HashMap<usize, usize> {
    let mut uses: HashMap<usize, usize> = HashMap::new();
    for fid in solid_faces(topo, solid).unwrap() {
        let face = topo.face(fid).unwrap();
        for wid in std::iter::once(face.outer_wire()).chain(face.inner_wires().iter().copied()) {
            for oe in topo.wire(wid).unwrap().edges() {
                *uses.entry(oe.edge().index()).or_insert(0) += 1;
            }
        }
    }
    uses
}

/// Closed 2-manifold: every edge used exactly twice — no free (boundary)
/// edges, no over-shared (non-manifold) ones.
fn assert_closed_two_manifold(topo: &Topology, solid: SolidId, what: &str) {
    let uses = edge_use_counts(topo, solid);
    assert!(!uses.is_empty(), "{what}: solid has no edges");
    let free = uses.values().filter(|&&c| c == 1).count();
    let non_manifold = uses.values().filter(|&&c| c > 2).count();
    assert_eq!(free, 0, "{what}: expected 0 free edges, got {free}");
    assert_eq!(
        non_manifold, 0,
        "{what}: expected 0 non-manifold edges, got {non_manifold}"
    );
}

fn surface_tags(topo: &Topology, solid: SolidId) -> Vec<&'static str> {
    solid_faces(topo, solid)
        .unwrap()
        .iter()
        .map(|&f| topo.face(f).unwrap().surface().type_tag())
        .collect()
}

/// A tool that touches nothing: a cube of the target's own size, pushed a
/// hundred radii away. Both extents scale with the model, so the gap is the
/// same multiple of the model at every scale.
fn far_tool(topo: &mut Topology, radius: f64) -> SolidId {
    let side = 2.0 * radius;
    let bx = make_box(topo, side, side, side).unwrap();
    copy_and_transform_solid(topo, bx, &Mat4::translation(100.0 * radius, 0.0, 0.0)).unwrap()
}

#[test]
fn cut_with_a_disjoint_tool_is_an_identity_on_a_sphere() {
    for scale in SCALES {
        let radius = 10.0 * scale;
        let exact = sphere_volume(radius);
        for segments in SEGMENTS {
            let mut topo = Topology::default();
            let sphere = make_sphere(&mut topo, radius, segments).unwrap();
            let tool = far_tool(&mut topo, radius);

            let result = boolean(&mut topo, BooleanOp::Cut, sphere, tool)
                .unwrap_or_else(|e| panic!("scale {scale} seg {segments}: cut failed: {e}"));

            let what = format!("scale {scale} seg {segments}");
            let volume = solid_volume(&topo, result, radius * 0.005).unwrap();
            let rel = (volume - exact).abs() / exact;
            assert!(
                rel < 1e-9,
                "{what}: cut by a disjoint tool changed the volume: got {volume}, \
                 closed form 4/3*pi*r^3 = {exact} (relative error {rel:.3e})"
            );

            // The exact surface must survive. The mesh fallback replaces both
            // spherical patches with thousands of planes, so this is the
            // assertion that distinguishes "right number" from "right body".
            let tags = surface_tags(&topo, result);
            assert_eq!(
                tags,
                vec!["sphere", "sphere"],
                "{what}: expected the two spherical patches to survive, got {tags:?}"
            );

            assert_closed_two_manifold(&topo, result, &what);
        }
    }
}

/// Segment-independence, stated directly: the sphere's surface is analytic,
/// so an identity Cut cannot depend on the equatorial polygon's resolution.
/// Before the fix all three counts agreed too — on the WRONG value — because
/// the mesh fallback tessellates from the deflection, not from `segments`.
/// Pairing this with the closed-form assertion above is what makes the pair
/// meaningful.
#[test]
fn disjoint_cut_volume_is_independent_of_sphere_segments() {
    for scale in SCALES {
        let radius = 10.0 * scale;
        let mut volumes = Vec::new();
        for segments in SEGMENTS {
            let mut topo = Topology::default();
            let sphere = make_sphere(&mut topo, radius, segments).unwrap();
            let tool = far_tool(&mut topo, radius);
            let result = boolean(&mut topo, BooleanOp::Cut, sphere, tool).unwrap();
            volumes.push(solid_volume(&topo, result, radius * 0.005).unwrap());
        }
        let exact = sphere_volume(radius);
        for (segments, volume) in SEGMENTS.iter().zip(&volumes) {
            let rel = (volume - exact).abs() / exact;
            assert!(
                rel < 1e-9,
                "scale {scale} seg {segments}: got {volume}, closed form {exact} \
                 (relative error {rel:.3e})"
            );
        }
    }
}

/// Controls in the same shape as the sphere case. These passed before the fix
/// and must keep passing: the defect was specific to a body whose faces share
/// their whole boundary, not to disjoint cuts in general.
#[test]
fn disjoint_cut_controls_box_and_cylinder_are_still_exact() {
    for scale in SCALES {
        let radius = 10.0 * scale;

        // Box control: 2r cube, closed form (2r)^3.
        let mut topo = Topology::default();
        let side = 2.0 * radius;
        let target = make_box(&mut topo, side, side, side).unwrap();
        let tool = far_tool(&mut topo, radius);
        let result = boolean(&mut topo, BooleanOp::Cut, target, tool).unwrap();
        let volume = solid_volume(&topo, result, radius * 0.005).unwrap();
        let exact = side * side * side;
        assert!(
            (volume - exact).abs() / exact < 1e-9,
            "scale {scale}: box control got {volume}, closed form {exact}"
        );
        assert_closed_two_manifold(&topo, result, &format!("scale {scale} box control"));

        // Cylinder control: r, height 2r, closed form pi*r^2*2r.
        let mut topo = Topology::default();
        let target = make_cylinder(&mut topo, radius, 2.0 * radius).unwrap();
        let tool = far_tool(&mut topo, radius);
        let result = boolean(&mut topo, BooleanOp::Cut, target, tool).unwrap();
        let volume = solid_volume(&topo, result, radius * 0.005).unwrap();
        let exact = PI * radius * radius * 2.0 * radius;
        assert!(
            (volume - exact).abs() / exact < 1e-9,
            "scale {scale}: cylinder control got {volume}, closed form {exact}"
        );
        assert_closed_two_manifold(&topo, result, &format!("scale {scale} cylinder control"));
    }
}

/// A Cut that DOES bite must still respond to where the tool sits. With the
/// sphere faceted by an earlier identity cut this stopped being true — mirror
/// placements returned the same number. The two closed forms here differ by
/// 5.4x, so a single shared answer cannot pass by accident.
///
/// The tool is a cube 6x the sphere's diameter, so only the plane at its top
/// face meets the ball; the remaining tool faces are far outside. Placing that
/// plane at +r/2 leaves the cap above it, at -r/2 leaves everything above it.
#[test]
fn mirror_placed_cuts_return_their_own_closed_forms() {
    for scale in SCALES {
        let radius = 10.0 * scale;
        let side = 12.0 * radius;
        // Spherical cap of height h on a sphere of radius r: pi*h^2*(r - h/3).
        let h = radius / 2.0;
        let cap = PI * h * h * (radius - h / 3.0);
        let rest = sphere_volume(radius) - cap;

        for (top, exact, label) in [
            (radius / 2.0, cap, "tool top at +r/2 keeps the cap"),
            (-radius / 2.0, rest, "tool top at -r/2 keeps the rest"),
        ] {
            let mut topo = Topology::default();
            let sphere = make_sphere(&mut topo, radius, 32).unwrap();
            let bx = make_box(&mut topo, side, side, side).unwrap();
            let tool = copy_and_transform_solid(
                &mut topo,
                bx,
                &Mat4::translation(-side / 2.0, -side / 2.0, top - side),
            )
            .unwrap();
            let result = boolean(&mut topo, BooleanOp::Cut, sphere, tool)
                .unwrap_or_else(|e| panic!("scale {scale} {label}: cut failed: {e}"));
            let volume = solid_volume(&topo, result, radius * 0.005).unwrap();
            let rel = (volume - exact).abs() / exact;
            // The GFA path is exact where it applies; where it does not the
            // mesh fallback still lands within a few tenths of a percent. The
            // point of this test is that the two placements produce DIFFERENT
            // answers, each near its own closed form — not that both are exact.
            assert!(
                rel < 0.01,
                "scale {scale} {label}: got {volume}, closed form {exact} \
                 (relative error {rel:.3e})"
            );
            assert_closed_two_manifold(&topo, result, &format!("scale {scale} {label}"));
        }
    }
}
