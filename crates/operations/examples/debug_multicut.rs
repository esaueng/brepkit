//! Repro: Cut with a multi-component subtrahend on the analytic flange blank.
//!
//! Sweeps N = 1..6 bolt cylinders fused into ONE subtrahend body and cuts
//! that from the unified rim+hub blank in a single boolean.
//!
//! Run: `cargo run --release --example debug_multicut -p brepkit-operations`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout,
    clippy::print_stderr
)]

use std::collections::BTreeMap;

use brepkit_math::mat::Mat4;
use brepkit_math::vec::{Point3, Vec3};
use brepkit_operations::boolean::{BooleanOp, boolean};
use brepkit_operations::heal::unify_faces;
use brepkit_operations::primitives;
use brepkit_operations::revolve::revolve;
use brepkit_operations::transform::transform_solid;
use brepkit_topology::Topology;
use brepkit_topology::builder::{make_planar_face_from_wire, make_polygon_wire};
use brepkit_topology::explorer::solid_faces;
use brepkit_topology::solid::SolidId;

const TOL: f64 = 1e-7;

fn census(topo: &Topology, solid: SolidId) -> BTreeMap<&'static str, usize> {
    let mut m = BTreeMap::new();
    for fid in solid_faces(topo, solid).unwrap() {
        *m.entry(topo.face(fid).unwrap().surface().type_tag())
            .or_insert(0) += 1;
    }
    m
}

fn free_and_nonmanifold(topo: &Topology, solid: SolidId) -> (usize, usize) {
    let mut usage: BTreeMap<usize, usize> = BTreeMap::new();
    for fid in solid_faces(topo, solid).unwrap() {
        let face = topo.face(fid).unwrap();
        for wid in std::iter::once(face.outer_wire()).chain(face.inner_wires().iter().copied()) {
            for oe in topo.wire(wid).unwrap().edges() {
                *usage.entry(oe.edge().index()).or_insert(0) += 1;
            }
        }
    }
    (
        usage.values().filter(|&&c| c == 1).count(),
        usage.values().filter(|&&c| c >= 3).count(),
    )
}

fn revolved_annulus(
    topo: &mut Topology,
    r_inner: f64,
    r_outer: f64,
    z_lo: f64,
    z_hi: f64,
) -> SolidId {
    let pts = [
        Point3::new(r_inner, 0.0, z_lo),
        Point3::new(r_outer, 0.0, z_lo),
        Point3::new(r_outer, 0.0, z_hi),
        Point3::new(r_inner, 0.0, z_hi),
    ];
    let wire = make_polygon_wire(topo, &pts, TOL).unwrap();
    let face = make_planar_face_from_wire(topo, wire).unwrap();
    revolve(
        topo,
        face,
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        std::f64::consts::TAU,
    )
    .unwrap()
}

fn blank(topo: &mut Topology) -> SolidId {
    let rim = revolved_annulus(topo, 24.0, 45.0, 0.0, 10.0);
    let hub = revolved_annulus(topo, 12.0, 24.0, 0.0, 26.0);
    let b = boolean(topo, BooleanOp::Fuse, rim, hub).expect("blank fuse");
    unify_faces(topo, b).unwrap();
    b
}

fn bolt(topo: &mut Topology, i: usize, n: usize) -> SolidId {
    #[allow(clippy::cast_precision_loss)]
    let angle = std::f64::consts::TAU * (i as f64) / (n.max(6) as f64);
    let c = primitives::make_cylinder(topo, 3.0, 16.0).unwrap();
    transform_solid(
        topo,
        c,
        &Mat4::translation(34.0 * angle.cos(), 34.0 * angle.sin(), -3.0),
    )
    .unwrap();
    c
}

fn main() {
    env_logger::init();

    let mut topo = Topology::new();
    let b0 = blank(&mut topo);
    println!(
        "blank: {:?} free/nm={:?}",
        census(&topo, b0),
        free_and_nonmanifold(&topo, b0)
    );

    for n in 1..=6usize {
        let mut topo = Topology::new();
        let body = blank(&mut topo);

        // Fuse N bolt cylinders into ONE subtrahend body.
        let mut pattern = bolt(&mut topo, 0, n);
        for i in 1..n {
            let next = bolt(&mut topo, i, n);
            pattern = boolean(&mut topo, BooleanOp::Fuse, pattern, next)
                .unwrap_or_else(|e| panic!("N={n}: pattern fuse {i} failed: {e:?}"));
        }
        let (pf, pnm) = free_and_nonmanifold(&topo, pattern);
        println!(
            "N={n} pattern: {:?} free={pf} nm={pnm}",
            census(&topo, pattern)
        );

        match boolean(&mut topo, BooleanOp::Cut, body, pattern) {
            Ok(r) => {
                let (f, nm) = free_and_nonmanifold(&topo, r);
                println!("N={n} OK  {:?} free={f} nm={nm}", census(&topo, r));
            }
            Err(e) => println!("N={n} FAILED {e:?}"),
        }
    }
}
