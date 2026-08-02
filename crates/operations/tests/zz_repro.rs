//! Scratch reproduction harness (temporary).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::f64::consts::PI;

use brepkit_math::mat::Mat4;
use brepkit_operations::boolean::{BooleanOp, boolean};
use brepkit_operations::copy::copy_and_transform_solid;
use brepkit_operations::measure::solid_volume;
use brepkit_operations::primitives::{make_box, make_sphere};
use brepkit_topology::Topology;
use brepkit_topology::explorer::solid_faces;
use brepkit_topology::solid::SolidId;

const DEFL: f64 = 0.05;

fn describe(topo: &Topology, s: SolidId) -> String {
    let faces = solid_faces(topo, s).unwrap();
    let mut counts = std::collections::BTreeMap::new();
    for &f in &faces {
        *counts
            .entry(topo.face(f).unwrap().surface().type_tag())
            .or_insert(0usize) += 1;
    }
    let mut uses: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for &f in &faces {
        let face = topo.face(f).unwrap();
        for wid in std::iter::once(face.outer_wire()).chain(face.inner_wires().iter().copied()) {
            for oe in topo.wire(wid).unwrap().edges() {
                *uses.entry(oe.edge().index()).or_insert(0) += 1;
            }
        }
    }
    let free = uses.values().filter(|&&c| c == 1).count();
    let nm = uses.values().filter(|&&c| c > 2).count();
    let mv = mesh_volume(topo, s);
    format!(
        "faces={} {counts:?} free={free} nonmanifold={nm} meshvol={mv:.4}",
        faces.len()
    )
}

/// Volume from the tessellated mesh via the divergence theorem, written by
/// hand — independent of `integrate_face`.
fn mesh_volume(topo: &Topology, s: SolidId) -> f64 {
    let m = brepkit_operations::tessellate::tessellate_solid(topo, s, 0.01).unwrap();
    let mut v = 0.0;
    for t in m.indices.chunks_exact(3) {
        let a = m.positions[t[0] as usize];
        let b = m.positions[t[1] as usize];
        let c = m.positions[t[2] as usize];
        v += (a.x() * (b.y() * c.z() - c.y() * b.z()) - b.x() * (a.y() * c.z() - c.y() * a.z())
            + c.x() * (a.y() * b.z() - b.y() * a.z()))
            / 6.0;
    }
    v
}

#[test]
fn repro_cut_disjoint() {
    let _ = env_logger::builder().is_test(false).try_init();
    for seg in [16usize, 32, 64] {
        let mut topo = Topology::default();
        let sph = make_sphere(&mut topo, 10.0, seg).unwrap();
        let bx = make_box(&mut topo, 20.0, 20.0, 20.0).unwrap();
        let far =
            copy_and_transform_solid(&mut topo, bx, &Mat4::translation(1000.0, 0.0, 0.0)).unwrap();
        match boolean(&mut topo, BooleanOp::Cut, sph, far) {
            Ok(sid) => {
                let v = solid_volume(&topo, sid, DEFL).unwrap();
                println!(
                    "seg={seg} after={v:.4} exact={:.4} {}",
                    4.0 / 3.0 * PI * 1000.0,
                    describe(&topo, sid)
                );
            }
            Err(e) => println!("seg={seg} ERR {e}"),
        }
    }
}

#[test]
fn repro_cut_overlap() {
    let _ = env_logger::builder().is_test(false).try_init();
    for seg in [16usize, 32, 64] {
        let mut topo = Topology::default();
        let sph = make_sphere(&mut topo, 10.0, seg).unwrap();
        let bx = make_box(&mut topo, 20.0, 20.0, 20.0).unwrap();
        match boolean(&mut topo, BooleanOp::Cut, sph, bx) {
            Ok(sid) => {
                let v = solid_volume(&topo, sid, DEFL).unwrap();
                println!(
                    "seg={seg} overlap cut vol={v:.4} exact={:.4} {}",
                    7.0 / 8.0 * 4.0 / 3.0 * PI * 1000.0,
                    describe(&topo, sid)
                );
            }
            Err(e) => println!("seg={seg} overlap cut ERR {e}"),
        }
    }
}

/// The brief's "mirror-image cuts return byte-identical volumes" case: one
/// 60-cube whose TOP face sits at z = +5 and at z = -5. Closed forms differ by
/// 5.4x, so an identical pair of answers is proof the tool position was ignored.
#[test]
fn repro_mirror_cuts() {
    let _ = env_logger::builder().is_test(false).try_init();
    let cap = PI * 5.0 * 5.0 * (10.0 - 5.0 / 3.0); // 654.4985
    let big = 4.0 / 3.0 * PI * 1000.0 - cap; // 3534.2917
    for (top, exact) in [(5.0, cap), (-5.0, big)] {
        let mut topo = Topology::default();
        let sph = make_sphere(&mut topo, 10.0, 32).unwrap();
        let bx = make_box(&mut topo, 60.0, 60.0, 60.0).unwrap();
        let tool =
            copy_and_transform_solid(&mut topo, bx, &Mat4::translation(-30.0, -30.0, top - 60.0))
                .unwrap();
        match boolean(&mut topo, BooleanOp::Cut, sph, tool) {
            Ok(sid) => {
                let v = solid_volume(&topo, sid, DEFL).unwrap();
                println!(
                    "tool top z={top}: vol={v:.4} exact={exact:.4} err={:+.4}% {}",
                    (v - exact) / exact * 100.0,
                    describe(&topo, sid)
                );
            }
            Err(e) => println!("tool top z={top}: ERR {e}"),
        }
    }
}

/// Half-space cuts on every axis, both signs, both "keep the big piece" and
/// "keep the cap". Tool is a 60-cube so no tool plane but the cut plane can
/// touch the ball.
#[test]
fn repro_cut_halfspace() {
    let _ = env_logger::builder().is_test(false).try_init();
    let cap = PI * 5.0 * 5.0 * (10.0 - 5.0 / 3.0); // 654.4985
    let big = 4.0 / 3.0 * PI * 1000.0 - cap; // 3534.2917
    // (label, translation of a 60-cube whose min corner sits at t, expected)
    let cases: [(&str, [f64; 3], f64); 6] = [
        ("remove x>+5", [5.0, -30.0, -30.0], big),
        ("remove x<-5", [-65.0, -30.0, -30.0], big),
        ("remove y>+5", [-30.0, 5.0, -30.0], big),
        ("remove y<-5", [-30.0, -65.0, -30.0], big),
        ("remove z>+5", [-30.0, -30.0, 5.0], big),
        ("remove z<-5", [-30.0, -30.0, -65.0], big),
    ];
    for (label, t, exact) in cases {
        let mut topo = Topology::default();
        let sph = make_sphere(&mut topo, 10.0, 32).unwrap();
        let bx = make_box(&mut topo, 60.0, 60.0, 60.0).unwrap();
        let tool = copy_and_transform_solid(&mut topo, bx, &Mat4::translation(t[0], t[1], t[2]))
            .unwrap();
        match boolean(&mut topo, BooleanOp::Cut, sph, tool) {
            Ok(sid) => {
                let v = solid_volume(&topo, sid, DEFL).unwrap();
                println!(
                    "{label}: vol={v:.4} exact={exact:.4} err={:+.4}% {}",
                    (v - exact) / exact * 100.0,
                    describe(&topo, sid)
                );
            }
            Err(e) => println!("{label}: ERR {e}"),
        }
    }
}
