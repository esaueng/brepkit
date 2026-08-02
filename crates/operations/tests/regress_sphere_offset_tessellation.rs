//! Regression: offsetting a sphere must produce a body you can actually see.
//!
//! `offset_solid` measured the offset ball perfectly (`4/3*pi*(r+d)^3`, to the
//! last digit) and kept both faces spherical, yet the body tessellated to ZERO
//! triangles and raised no warning — "no boundary edges" is vacuously true of
//! an empty mesh, so every watertightness check passed.
//!
//! Cause: `loops::try_direct_chain` walks the reconstructed boundary starting
//! from an arbitrary edge, which fixes the loop's traversal sense arbitrarily.
//! On a closed surface the sense IS the region — the same equatorial loop
//! bounds the northern hemisphere walked one way and the southern hemisphere
//! walked the other — so BOTH offset faces came out covering the northern
//! half, one of them inside out. `dedupe_coincident_triangles` then cancelled
//! the two opposite-winding copies against each other and the mesh emptied.
//!
//! The volume stayed right throughout because the face integrator reads the
//! surface and the face's own orientation, not the region the wire selects.
//! That is exactly why the assertions below are against the tessellated
//! geometry and hand-written closed forms rather than against another
//! integrator.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::f64::consts::PI;

use brepkit_operations::measure::solid_volume;
use brepkit_operations::offset_v2::offset_solid_v2;
use brepkit_operations::primitives::{make_box, make_cylinder, make_sphere, make_torus};
use brepkit_operations::tessellate::{TriangleMesh, tessellate_solid};
use brepkit_topology::Topology;
use brepkit_topology::explorer::solid_faces;
use brepkit_topology::solid::SolidId;

/// Model scales, ordered as a rotation of the natural small-to-large sweep so
/// a result that only holds at whichever scale runs first cannot pass.
const SCALES: [f64; 3] = [1000.0, 0.001, 1.0];

fn sphere_volume(r: f64) -> f64 {
    4.0 / 3.0 * PI * r * r * r
}

/// Volume enclosed by a triangle mesh, by the divergence theorem, written out
/// here on purpose: it shares nothing with the face integrator that
/// `solid_volume` and `mass_properties` both call, so agreement between the
/// two is real evidence rather than a tautology.
fn mesh_enclosed_volume(mesh: &TriangleMesh) -> f64 {
    let mut total = 0.0;
    for tri in mesh.indices.chunks_exact(3) {
        let a = mesh.positions[tri[0] as usize];
        let b = mesh.positions[tri[1] as usize];
        let c = mesh.positions[tri[2] as usize];
        total += (a.x() * (b.y() * c.z() - c.y() * b.z())
            - b.x() * (a.y() * c.z() - c.y() * a.z())
            + c.x() * (a.y() * b.z() - b.y() * a.z()))
            / 6.0;
    }
    total
}

/// Extent of the mesh along z. An offset ball must span the full diameter; a
/// single hemisphere covers only half of it.
fn mesh_z_span(mesh: &TriangleMesh) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for p in &mesh.positions {
        lo = lo.min(p.z());
        hi = hi.max(p.z());
    }
    (lo, hi)
}

fn assert_closed_two_manifold(topo: &Topology, solid: SolidId, what: &str) {
    let mut uses: HashMap<usize, usize> = HashMap::new();
    for fid in solid_faces(topo, solid).unwrap() {
        let face = topo.face(fid).unwrap();
        for wid in std::iter::once(face.outer_wire()).chain(face.inner_wires().iter().copied()) {
            for oe in topo.wire(wid).unwrap().edges() {
                *uses.entry(oe.edge().index()).or_insert(0) += 1;
            }
        }
    }
    assert!(!uses.is_empty(), "{what}: solid has no edges");
    let free = uses.values().filter(|&&c| c == 1).count();
    let non_manifold = uses.values().filter(|&&c| c > 2).count();
    assert_eq!(free, 0, "{what}: expected 0 free edges, got {free}");
    assert_eq!(
        non_manifold, 0,
        "{what}: expected 0 non-manifold edges, got {non_manifold}"
    );
}

#[test]
fn offsetting_a_sphere_produces_a_visible_body() {
    for scale in SCALES {
        let radius = 10.0 * scale;
        // Offsets stated as fractions of the radius so the same geometry is
        // tested at every scale. The tiny one reproduces the original report's
        // "+0.001 on r=10" case as a ratio rather than an absolute length.
        for ratio in [0.2, 1e-4] {
            let distance = radius * ratio;
            let outer = radius + distance;
            let exact = sphere_volume(outer);
            let what = format!("scale {scale} offset +{ratio}r");

            let mut topo = Topology::default();
            let sphere = make_sphere(&mut topo, radius, 32).unwrap();
            let result = offset_solid_v2(&mut topo, sphere, distance)
                .unwrap_or_else(|e| panic!("{what}: offset failed: {e}"));

            // Both spherical patches survive, and the shell is closed.
            let tags: Vec<_> = solid_faces(&topo, result)
                .unwrap()
                .iter()
                .map(|&f| topo.face(f).unwrap().surface().type_tag())
                .collect();
            assert_eq!(
                tags,
                vec!["sphere", "sphere"],
                "{what}: expected two spherical patches, got {tags:?}"
            );
            assert_closed_two_manifold(&topo, result, &what);

            // Measured volume against the closed form.
            let volume = solid_volume(&topo, result, outer * 0.005).unwrap();
            let rel = (volume - exact).abs() / exact;
            assert!(
                rel < 1e-9,
                "{what}: got {volume}, closed form 4/3*pi*(r+d)^3 = {exact} \
                 (relative error {rel:.3e})"
            );

            // The assertion the defect actually violated: the body must have
            // geometry. Zero triangles measured perfectly and warned about
            // nothing.
            let mesh = tessellate_solid(&topo, result, outer * 0.001).unwrap();
            let triangles = mesh.indices.len() / 3;
            assert!(
                triangles > 0,
                "{what}: offset body tessellated to ZERO triangles \
                 (volume measured {volume}, closed form {exact})"
            );

            // Both hemispheres must be present, not one of them twice. The
            // defect produced two copies of the NORTHERN half with opposite
            // winding, so the span was [0, outer] before they cancelled.
            let (lo, hi) = mesh_z_span(&mesh);
            let span_rel_error =
                ((hi - lo) - 2.0 * outer).abs() / (2.0 * outer);
            assert!(
                span_rel_error < 0.01,
                "{what}: mesh spans z in [{lo}, {hi}] — expected the full \
                 diameter {} (both hemispheres), relative error {span_rel_error:.3e}",
                2.0 * outer
            );

            // Independent volume, from the mesh, by hand. Positive (outward
            // winding) and within tessellation error of the same closed form.
            let from_mesh = mesh_enclosed_volume(&mesh);
            let mesh_rel = (from_mesh - exact).abs() / exact;
            assert!(
                mesh_rel < 0.01,
                "{what}: mesh encloses {from_mesh}, closed form {exact} \
                 (relative error {mesh_rel:.3e})"
            );
        }
    }
}

/// Controls: the same offset on bodies whose faces do NOT share their whole
/// boundary. These passed before the fix and must keep passing — the defect
/// was not about curvature, periodicity or seams. The torus is the sharp
/// control: closed, seamed, doubly periodic, and fine throughout.
#[test]
fn offset_controls_box_cylinder_and_torus_still_tessellate() {
    for scale in SCALES {
        let s = 10.0 * scale;
        let d = s * 0.2;

        // Box: side s offset by d gives (s + 2d)^3.
        let mut topo = Topology::default();
        let solid = make_box(&mut topo, s, s, s).unwrap();
        let out = offset_solid_v2(&mut topo, solid, d).unwrap();
        let exact = (s + 2.0 * d).powi(3);
        let mesh = tessellate_solid(&topo, out, s * 0.001).unwrap();
        assert!(
            !mesh.indices.is_empty(),
            "scale {scale}: box offset tessellated to zero triangles"
        );
        assert!(
            (mesh_enclosed_volume(&mesh) - exact).abs() / exact < 0.01,
            "scale {scale}: box offset mesh volume {} vs closed form {exact}",
            mesh_enclosed_volume(&mesh)
        );

        // Cylinder: r = s/2, h = s, offset d gives pi*(r+d)^2*(h+2d).
        let mut topo = Topology::default();
        let solid = make_cylinder(&mut topo, s / 2.0, s).unwrap();
        let out = offset_solid_v2(&mut topo, solid, d).unwrap();
        let r = s / 2.0 + d;
        let exact = PI * r * r * (s + 2.0 * d);
        let mesh = tessellate_solid(&topo, out, s * 0.001).unwrap();
        assert!(
            !mesh.indices.is_empty(),
            "scale {scale}: cylinder offset tessellated to zero triangles"
        );
        assert!(
            (mesh_enclosed_volume(&mesh) - exact).abs() / exact < 0.02,
            "scale {scale}: cylinder offset mesh volume {} vs closed form {exact}",
            mesh_enclosed_volume(&mesh)
        );

        // Torus: R = s, r = 0.3s, offset d gives 2*pi^2*R*(r+d)^2.
        let mut topo = Topology::default();
        let solid = make_torus(&mut topo, s, 0.3 * s, 32).unwrap();
        let out = offset_solid_v2(&mut topo, solid, d).unwrap();
        let minor = 0.3 * s + d;
        let exact = 2.0 * PI * PI * s * minor * minor;
        let mesh = tessellate_solid(&topo, out, s * 0.001).unwrap();
        assert!(
            !mesh.indices.is_empty(),
            "scale {scale}: torus offset tessellated to zero triangles"
        );
        assert!(
            (mesh_enclosed_volume(&mesh) - exact).abs() / exact < 0.02,
            "scale {scale}: torus offset mesh volume {} vs closed form {exact}",
            mesh_enclosed_volume(&mesh)
        );
    }
}
