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

#[test]
fn repro_offset2() {
    let _ = env_logger::builder().is_test(false).try_init();
    use brepkit_operations::offset_v2::offset_solid_v2;
    for d in [2.0, 0.001] {
        let mut topo = Topology::default();
        let sph = make_sphere(&mut topo, 10.0, 32).unwrap();
        match offset_solid_v2(&mut topo, sph, d) {
            Ok(sid) => {
                let v = solid_volume(&topo, sid, DEFL).unwrap();
                let m = brepkit_operations::tessellate::tessellate_solid(&topo, sid, 0.01).unwrap();
                println!(
                    "offset +{d}: vol={v:.4} exact={:.4} tris={} {}",
                    4.0 / 3.0 * PI * (10.0 + d).powi(3),
                    m.indices.len() / 3,
                    describe(&topo, sid)
                );
                for f in solid_faces(&topo, sid).unwrap() {
                    let face = topo.face(f).unwrap();
                    let w = topo.wire(face.outer_wire()).unwrap();
                    println!(
                        "   face {f:?} {} rev={} outer_edges={} inners={}",
                        face.surface().type_tag(),
                        face.is_reversed(),
                        w.edges().len(),
                        face.inner_wires().len()
                    );
                }
            }
            Err(e) => println!("offset +{d}: ERR {e}"),
        }
    }
}

#[test]
fn repro_offset_faces() {
    let _ = env_logger::builder().is_test(false).try_init();
    use brepkit_operations::offset_v2::offset_solid_v2;
    use brepkit_operations::tessellate::tessellate;
    let mut topo = Topology::default();
    let sph = make_sphere(&mut topo, 10.0, 32).unwrap();
    for f in solid_faces(&topo, sph).unwrap() {
        let m = tessellate(&topo, f, 0.05).unwrap();
        let face = topo.face(f).unwrap();
        let w = topo.wire(face.outer_wire()).unwrap();
        println!(
            "ORIG face {f:?} rev={} edges={} fwd={:?} tris={}",
            face.is_reversed(),
            w.edges().len(),
            w.edges().iter().map(|o| o.is_forward()).collect::<Vec<_>>(),
            m.indices.len() / 3
        );
    }
    let off = offset_solid_v2(&mut topo, sph, 2.0).unwrap();
    for f in solid_faces(&topo, off).unwrap() {
        let m = tessellate(&topo, f, 0.05).unwrap();
        let face = topo.face(f).unwrap();
        let w = topo.wire(face.outer_wire()).unwrap();
        println!(
            "OFFSET face {f:?} rev={} edges={} fwd={:?} tris={}",
            face.is_reversed(),
            w.edges().len(),
            w.edges().iter().map(|o| o.is_forward()).collect::<Vec<_>>(),
            m.indices.len() / 3
        );
        for oe in w.edges().iter().take(3) {
            let e = topo.edge(oe.edge()).unwrap();
            let a = topo.vertex(e.start()).unwrap().point();
            let b = topo.vertex(e.end()).unwrap().point();
            println!(
                "    edge {:?} {} ({:.3},{:.3},{:.3})->({:.3},{:.3},{:.3})",
                oe.edge(), e.curve().type_tag(),
                a.x(), a.y(), a.z(), b.x(), b.y(), b.z()
            );
        }
    }
}

fn zrange(m: &brepkit_operations::tessellate::TriangleMesh) -> (f64, f64, f64) {
    let mut lo = f64::MAX;
    let mut hi = f64::MIN;
    for p in &m.positions {
        lo = lo.min(p.z());
        hi = hi.max(p.z());
    }
    // signed volume of this patch alone (cone to origin) — sign shows winding
    let mut v = 0.0;
    for t in m.indices.chunks_exact(3) {
        let a = m.positions[t[0] as usize];
        let b = m.positions[t[1] as usize];
        let c = m.positions[t[2] as usize];
        v += (a.x() * (b.y() * c.z() - c.y() * b.z()) - b.x() * (a.y() * c.z() - c.y() * a.z())
            + c.x() * (a.y() * b.z() - b.y() * a.z()))
            / 6.0;
    }
    (lo, hi, v)
}

#[test]
fn repro_offset_sides() {
    let _ = env_logger::builder().is_test(false).try_init();
    use brepkit_operations::offset_v2::offset_solid_v2;
    use brepkit_operations::tessellate::tessellate;
    let mut topo = Topology::default();
    let sph = make_sphere(&mut topo, 10.0, 32).unwrap();
    for f in solid_faces(&topo, sph).unwrap() {
        let m = tessellate(&topo, f, 0.05).unwrap();
        let (lo, hi, v) = zrange(&m);
        println!("ORIG {f:?} rev={} z=[{lo:.3},{hi:.3}] signedvol={v:.3}",
                 topo.face(f).unwrap().is_reversed());
    }
    let off = offset_solid_v2(&mut topo, sph, 2.0).unwrap();
    for f in solid_faces(&topo, off).unwrap() {
        let m = tessellate(&topo, f, 0.05).unwrap();
        let (lo, hi, v) = zrange(&m);
        println!("OFFSET {f:?} rev={} z=[{lo:.3},{hi:.3}] signedvol={v:.3}",
                 topo.face(f).unwrap().is_reversed());
    }
}

#[test]
fn repro_offset_open_cases() {
    let _ = env_logger::builder().is_test(false).try_init();
    use brepkit_operations::offset_v2::offset_solid_v2;
    // (a) a body that merely CONTAINS a sphere face: half sphere
    let mut topo = Topology::default();
    let sph = make_sphere(&mut topo, 10.0, 32).unwrap();
    let bx = make_box(&mut topo, 60.0, 60.0, 60.0).unwrap();
    let tool = copy_and_transform_solid(&mut topo, bx, &Mat4::translation(-30.0, -30.0, 0.0))
        .unwrap();
    match boolean(&mut topo, BooleanOp::Cut, sph, tool) {
        Ok(half) => {
            println!("half sphere: {}", describe(&topo, half));
            match offset_solid_v2(&mut topo, half, 2.0) {
                Ok(o) => println!(
                    "  half sphere offset +2: vol={:.4} exact={:.4} {}",
                    solid_volume(&topo, o, DEFL).unwrap(),
                    2.0 / 3.0 * PI * 12.0f64.powi(3),
                    describe(&topo, o)
                ),
                Err(e) => println!("  half sphere offset +2 ERR {e}"),
            }
        }
        Err(e) => println!("half sphere cut ERR {e}"),
    }
}

#[test]
fn repro_cyl_offset_scales() {
    use brepkit_operations::offset_v2::offset_solid_v2;
    use brepkit_operations::primitives::make_cylinder;
    for scale in [0.001f64, 1.0] {
        let s = 10.0 * scale;
        let mut topo = Topology::default();
        let solid = make_cylinder(&mut topo, s / 2.0, s).unwrap();
        match offset_solid_v2(&mut topo, solid, s * 0.2) {
            Ok(o) => println!("cyl scale={scale}: ok vol={:.6} exact={:.6}",
                solid_volume(&topo, o, s*0.001).unwrap(),
                PI*(s/2.0+s*0.2).powi(2)*(s+2.0*s*0.2)),
            Err(e) => println!("cyl scale={scale}: ERR {e}"),
        }
    }
}

/// Non-asserting scale sweep, used to confirm the defect at EVERY scale
/// rather than only at whichever one asserts first.
#[test]
fn repro_scale_sweep() {
    use brepkit_operations::offset_v2::offset_solid_v2;
    for scale in [0.001f64, 1.0] {
        let r = 10.0 * scale;
        let exact = 4.0 / 3.0 * PI * r * r * r;
        for seg in [16usize, 32, 64] {
            let mut topo = Topology::default();
            let sph = make_sphere(&mut topo, r, seg).unwrap();
            let side = 2.0 * r;
            let bx = make_box(&mut topo, side, side, side).unwrap();
            let far = copy_and_transform_solid(&mut topo, bx, &Mat4::translation(100.0 * r, 0.0, 0.0)).unwrap();
            match boolean(&mut topo, BooleanOp::Cut, sph, far) {
                Ok(sid) => {
                    let v = solid_volume(&topo, sid, r * 0.005).unwrap();
                    let tags: Vec<_> = solid_faces(&topo, sid).unwrap().iter()
                        .map(|&f| topo.face(f).unwrap().surface().type_tag()).collect();
                    let mut c = std::collections::BTreeMap::new();
                    for t in &tags { *c.entry(*t).or_insert(0usize) += 1; }
                    println!("CUT scale={scale} seg={seg}: relerr={:.3e} faces={:?}", (v - exact).abs() / exact, c);
                }
                Err(e) => println!("CUT scale={scale} seg={seg}: ERR {e}"),
            }
        }
        for ratio in [0.2f64, 1e-4] {
            let d = r * ratio;
            let outer = r + d;
            let ex = 4.0 / 3.0 * PI * outer.powi(3);
            let mut topo = Topology::default();
            let sph = make_sphere(&mut topo, r, 32).unwrap();
            match offset_solid_v2(&mut topo, sph, d) {
                Ok(o) => {
                    let v = solid_volume(&topo, o, outer * 0.005).unwrap();
                    let m = brepkit_operations::tessellate::tessellate_solid(&topo, o, outer * 0.001).unwrap();
                    let mut lo = f64::INFINITY; let mut hi = f64::NEG_INFINITY;
                    for p in &m.positions { lo = lo.min(p.z()); hi = hi.max(p.z()); }
                    println!("OFFSET scale={scale} d={ratio}r: volrelerr={:.3e} tris={} zspan=[{lo:.4},{hi:.4}] meshvol={:.6} exact={ex:.6}",
                        (v - ex).abs() / ex, m.indices.len() / 3, mesh_volume(&topo, o));
                }
                Err(e) => println!("OFFSET scale={scale} d={ratio}r: ERR {e}"),
            }
        }
    }
}

#[test]
fn repro_timing_1000x() {
    for seg in [16usize, 64] {
        let t0 = std::time::Instant::now();
        let r = 10000.0;
        let mut topo = Topology::default();
        let sph = make_sphere(&mut topo, r, seg).unwrap();
        let bx = make_box(&mut topo, 2.0 * r, 2.0 * r, 2.0 * r).unwrap();
        let far = copy_and_transform_solid(&mut topo, bx, &Mat4::translation(100.0 * r, 0.0, 0.0)).unwrap();
        let t1 = std::time::Instant::now();
        let sid = boolean(&mut topo, BooleanOp::Cut, sph, far).unwrap();
        let t2 = std::time::Instant::now();
        let v = solid_volume(&topo, sid, r * 0.005).unwrap();
        let t3 = std::time::Instant::now();
        println!("seg={seg} build={:?} boolean={:?} volume={:?} v={v:.1}",
                 t1 - t0, t2 - t1, t3 - t2);
    }
}

#[test]
fn repro_cap_cut() {
    let _ = env_logger::builder().is_test(false).try_init();
    let r = 10.0;
    let side = 12.0 * r;
    for top in [r / 2.0, -r / 2.0] {
        let mut topo = Topology::default();
        let sph = make_sphere(&mut topo, r, 32).unwrap();
        let bx = make_box(&mut topo, side, side, side).unwrap();
        let tool = copy_and_transform_solid(&mut topo, bx,
            &Mat4::translation(-side / 2.0, -side / 2.0, top - side)).unwrap();
        match boolean(&mut topo, BooleanOp::Cut, sph, tool) {
            Ok(sid) => println!("top={top}: vol={:.4} {}", solid_volume(&topo, sid, 0.05).unwrap(), describe(&topo, sid)),
            Err(e) => println!("top={top}: ERR {e}"),
        }
    }
}

#[test]
fn repro_cyl_tess_scale() {
    use brepkit_operations::offset_v2::offset_solid_v2;
    use brepkit_operations::primitives::make_cylinder;
    for scale in [0.001f64, 1.0] {
        let s = 10.0 * scale;
        let d = s * 0.2;
        let mut topo = Topology::default();
        let solid = make_cylinder(&mut topo, s / 2.0, s).unwrap();
        let out = offset_solid_v2(&mut topo, solid, d).unwrap();
        let r = s / 2.0 + d;
        let exact = PI * r * r * (s + 2.0 * d);
        for k in [1e-3f64, 1e-4, 1e-5] {
            let m = brepkit_operations::tessellate::tessellate_solid(&topo, out, s * k).unwrap();
            let mut v = 0.0;
            for t in m.indices.chunks_exact(3) {
                let a = m.positions[t[0] as usize];
                let b = m.positions[t[1] as usize];
                let c = m.positions[t[2] as usize];
                v += (a.x() * (b.y() * c.z() - c.y() * b.z()) - b.x() * (a.y() * c.z() - c.y() * a.z())
                    + c.x() * (a.y() * b.z() - b.y() * a.z())) / 6.0;
            }
            println!("cyl scale={scale} defl={:.3e} tris={} relerr={:.4}", s * k, m.indices.len()/3, (v - exact).abs()/exact);
        }
    }
}
