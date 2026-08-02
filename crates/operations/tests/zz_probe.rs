//! Scratch probe. Not for commit.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::f64::consts::FRAC_PI_2;

use brepkit_math::mat::Mat4;
use brepkit_math::vec::Point3;
use brepkit_operations::boolean::{BooleanOp, boolean};
use brepkit_operations::measure::solid_volume;
use brepkit_operations::primitives::{make_box, make_cylinder, make_sphere};
use brepkit_operations::tessellate::{
    TriangleMesh, tessellate_solid, tessellate_solid_grouped_with_tolerance, welded_mesh_quality,
};
use brepkit_operations::transform::transform_solid;
use brepkit_topology::Topology;
use brepkit_topology::face::FaceSurface;
use brepkit_topology::solid::SolidId;

const R: f64 = 3.0;
const H: f64 = 30.0;

fn cross_drilled_shaft(bore: f64, s: f64) -> (Topology, SolidId) {
    let mut topo = Topology::new();
    let shaft = make_cylinder(&mut topo, R * s, H * s).unwrap();
    let len = (H + 4.0 * R) * s;
    let tool = make_cylinder(&mut topo, bore * s, len).unwrap();
    transform_solid(&mut topo, tool, &Mat4::rotation_y(FRAC_PI_2)).unwrap();
    transform_solid(
        &mut topo,
        tool,
        &Mat4::translation(-len / 2.0, 0.0, H * s / 2.0),
    )
    .unwrap();
    let res = boolean(&mut topo, BooleanOp::Cut, shaft, tool).unwrap();
    (topo, res)
}

fn mesh_volume(m: &TriangleMesh) -> f64 {
    let mut v: f64 = 0.0;
    for t in m.indices.chunks_exact(3) {
        let a = m.positions[t[0] as usize];
        let b = m.positions[t[1] as usize];
        let c = m.positions[t[2] as usize];
        let det = a.x() * (b.y() * c.z() - b.z() * c.y()) - a.y() * (b.x() * c.z() - b.z() * c.x())
            + a.z() * (b.x() * c.y() - b.y() * c.x());
        v += det / 6.0;
    }
    v.abs()
}

fn area_of(m: &TriangleMesh, range: std::ops::Range<usize>) -> f64 {
    m.indices[range]
        .chunks_exact(3)
        .map(|t| {
            let a = m.positions[t[0] as usize];
            let b = m.positions[t[1] as usize];
            let c = m.positions[t[2] as usize];
            (b - a).cross(c - a).length() / 2.0
        })
        .sum()
}

fn open_edges(m: &TriangleMesh) -> Vec<(Point3, Point3)> {
    let s = 1e6;
    let qk = |p: Point3| -> (i64, i64, i64) {
        (
            (p.x() * s).round() as i64,
            (p.y() * s).round() as i64,
            (p.z() * s).round() as i64,
        )
    };
    let mut edges: HashMap<((i64, i64, i64), (i64, i64, i64)), (u32, Point3, Point3)> =
        HashMap::new();
    for t in m.indices.chunks_exact(3) {
        let (pa, pb, pc) = (
            m.positions[t[0] as usize],
            m.positions[t[1] as usize],
            m.positions[t[2] as usize],
        );
        let (a, b, c) = (qk(pa), qk(pb), qk(pc));
        if a == b || b == c || a == c {
            continue;
        }
        for (p, r, pp, rr) in [(a, b, pa, pb), (b, c, pb, pc), (c, a, pc, pa)] {
            let key = if p <= r { (p, r) } else { (r, p) };
            let e = edges.entry(key).or_insert((0, pp, rr));
            e.0 += 1;
        }
    }
    edges
        .values()
        .filter(|v| v.0 == 1)
        .map(|v| (v.1, v.2))
        .collect()
}

fn kind(f: &brepkit_topology::face::Face) -> &'static str {
    match f.surface() {
        FaceSurface::Plane { .. } => "plane",
        FaceSurface::Cylinder(_) => "cyl",
        FaceSurface::Cone(_) => "cone",
        FaceSurface::Sphere(_) => "sphere",
        FaceSurface::Torus(_) => "torus",
        FaceSurface::Nurbs(_) => "nurbs",
    }
}

fn report(topo: &Topology, sol: SolidId, defl: f64, label: &str) {
    let (mesh, offsets) =
        tessellate_solid_grouped_with_tolerance(topo, sol, defl, brepkit_math::chord::DEFAULT_ANGULAR_TOL)
            .unwrap();
    let q = welded_mesh_quality(&mesh);
    println!(
        "{label} defl={defl}: mesh_vol={:.4} tris={} open={} nonman={}",
        mesh_volume(&mesh),
        mesh.indices.len() / 3,
        q.boundary_edges,
        q.non_manifold_edges
    );
    let faces = brepkit_topology::explorer::solid_faces(topo, sol).unwrap();
    for (i, &fid) in faces.iter().enumerate() {
        let f = topo.face(fid).unwrap();
        let r = offsets[i] as usize..offsets[i + 1] as usize;
        let n = (r.end - r.start) / 3;
        let ow = topo.wire(f.outer_wire()).unwrap();
        println!(
            "    face {i} {fid:?} {} oedges={} inner={} tris={n} area={:.4}",
            kind(f),
            ow.edges().len(),
            f.inner_wires().len(),
            area_of(&mesh, r)
        );
    }
    let oe = open_edges(&mesh);
    let mut zs: Vec<f64> = oe.iter().flat_map(|e| [e.0.z(), e.1.z()]).collect();
    zs.sort_by(f64::total_cmp);
    if !zs.is_empty() {
        println!(
            "    open-edge z: min={:.4} max={:.4}  (n={})",
            zs[0],
            zs[zs.len() - 1],
            oe.len()
        );
    }
}

#[test]
fn probe_shaft() {
    let mut topo = Topology::new();
    let shaft = make_cylinder(&mut topo, 3.0, 30.0).unwrap();
    println!("undrilled brep vol {:.4}", solid_volume(&topo, shaft, 0.01).unwrap());
    for &bore in &[3.0_f64, 2.0, 1.0] {
        let (topo, sol) = cross_drilled_shaft(bore, 1.0);
        println!("=== bore {bore} brep vol {:.4}", solid_volume(&topo, sol, 0.01).unwrap());
        for &defl in &[0.5_f64, 0.1, 0.05, 0.01, 0.005] {
            report(&topo, sol, defl, &format!("bore{bore}"));
        }
    }
}

#[test]
fn probe_shaft_default() {
    // What tessellate_solid (the display default) gives.
    for &bore in &[3.0_f64, 2.0, 1.0] {
        let (topo, sol) = cross_drilled_shaft(bore, 1.0);
        for &defl in &[0.5_f64, 0.2, 0.1, 0.05, 0.02, 0.01, 0.005, 0.001] {
            let mesh = tessellate_solid(&topo, sol, defl).unwrap();
            let q = welded_mesh_quality(&mesh);
            println!(
                "bore={bore} defl={defl}: vol={:.4} tris={} open={}",
                mesh_volume(&mesh),
                mesh.indices.len() / 3,
                q.boundary_edges
            );
        }
    }
}

fn cut_ball() -> (Topology, SolidId) {
    let mut topo = Topology::new();
    let ball = make_sphere(&mut topo, 10.0, 32).unwrap();
    let cutter = make_box(&mut topo, 40.0, 40.0, 40.0).unwrap();
    transform_solid(&mut topo, cutter, &Mat4::translation(-20.0, -20.0, 5.0)).unwrap();
    let res = boolean(&mut topo, BooleanOp::Cut, ball, cutter).unwrap();
    (topo, res)
}

#[test]
fn probe_ball() {
    let (topo, sol) = cut_ball();
    let r: f64 = 10.0;
    let h: f64 = 5.0;
    let closed = 4.0 / 3.0 * std::f64::consts::PI * r.powi(3)
        - std::f64::consts::PI * h * h * (3.0 * r - h) / 3.0;
    println!(
        "brep vol {:.13} closed {closed:.13}",
        solid_volume(&topo, sol, 0.01).unwrap()
    );
    let faces = brepkit_topology::explorer::solid_faces(&topo, sol).unwrap();
    for (i, &fid) in faces.iter().enumerate() {
        let f = topo.face(fid).unwrap();
        let ow = topo.wire(f.outer_wire()).unwrap();
        let zs: Vec<String> = ow
            .edges()
            .iter()
            .take(4)
            .map(|oe| {
                let e = topo.edge(oe.edge()).unwrap();
                let sp = topo.vertex(e.start()).unwrap().point();
                format!("({:.2},{:.2},{:.2})fwd={}", sp.x(), sp.y(), sp.z(), oe.is_forward())
            })
            .collect();
        let zmin = ow
            .edges()
            .iter()
            .map(|oe| topo.vertex(topo.edge(oe.edge()).unwrap().start()).unwrap().point().z())
            .fold(f64::INFINITY, f64::min);
        let zmax = ow
            .edges()
            .iter()
            .map(|oe| topo.vertex(topo.edge(oe.edge()).unwrap().start()).unwrap().point().z())
            .fold(f64::NEG_INFINITY, f64::max);
        println!(
            "  face {i} {fid:?} {} rev={} oedges={} inner={} z[{zmin:.3},{zmax:.3}] {zs:?}",
            kind(f),
            f.is_reversed(),
            ow.edges().len(),
            f.inner_wires().len()
        );
    }
    for &defl in &[0.1_f64, 0.05, 0.01, 0.005] {
        report(&topo, sol, defl, "ball");
    }
}

#[test]
fn probe_ball_equator() {
    let (topo, sol) = cut_ball();
    for &defl in &[0.1_f64, 0.01] {
        let (mesh, offsets) = tessellate_solid_grouped_with_tolerance(
            &topo,
            sol,
            defl,
            brepkit_math::chord::DEFAULT_ANGULAR_TOL,
        )
        .unwrap();
        let s = 1e6;
        let qk = |p: Point3| -> (i64, i64, i64) {
            (
                (p.x() * s).round() as i64,
                (p.y() * s).round() as i64,
                (p.z() * s).round() as i64,
            )
        };
        println!("--- defl {defl}");
        // per-face: vertices on z=0 ring
        let faces = brepkit_topology::explorer::solid_faces(&topo, sol).unwrap();
        let mut per_face_ring: Vec<Vec<f64>> = vec![];
        let mut per_face_edges: Vec<HashMap<((i64,i64,i64),(i64,i64,i64)), u32>> = vec![];
        for i in 0..faces.len() {
            let r = offsets[i] as usize..offsets[i + 1] as usize;
            let mut ring: Vec<f64> = vec![];
            let mut em: HashMap<((i64,i64,i64),(i64,i64,i64)), u32> = HashMap::new();
            for t in mesh.indices[r].chunks_exact(3) {
                let ps = [
                    mesh.positions[t[0] as usize],
                    mesh.positions[t[1] as usize],
                    mesh.positions[t[2] as usize],
                ];
                for p in ps {
                    if p.z().abs() < 1e-9 {
                        ring.push(p.y().atan2(p.x()));
                    }
                }
                let (a, b, c) = (qk(ps[0]), qk(ps[1]), qk(ps[2]));
                if a == b || b == c || a == c { continue; }
                for (p, q) in [(a, b), (b, c), (c, a)] {
                    let key = if p <= q { (p, q) } else { (q, p) };
                    *em.entry(key).or_default() += 1;
                }
            }
            ring.sort_by(f64::total_cmp);
            ring.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            println!(
                "  face {i}: z=0 ring verts {} radii {:?}",
                ring.len(),
                {
                    let mut rr: Vec<f64> = mesh.indices[offsets[i] as usize..offsets[i+1] as usize]
                        .iter()
                        .map(|&ix| mesh.positions[ix as usize])
                        .filter(|p| p.z().abs() < 1e-9)
                        .map(|p| p.x().hypot(p.y()))
                        .collect();
                    rr.sort_by(f64::total_cmp);
                    rr.dedup_by(|a,b| (*a-*b).abs()<1e-9);
                    (rr.first().copied().unwrap_or(0.0), rr.last().copied().unwrap_or(0.0))
                }
            );
            per_face_ring.push(ring);
            per_face_edges.push(em);
        }
        // whole-mesh open edges, attributed
        let oe = open_edges(&mesh);
        let mut by_face = vec![0usize; faces.len()];
        for (a, b) in &oe {
            let key = { let (x, y) = (qk(*a), qk(*b)); if x <= y { (x, y) } else { (y, x) } };
            for (i, em) in per_face_edges.iter().enumerate() {
                if em.contains_key(&key) {
                    by_face[i] += 1;
                }
            }
        }
        println!("  open edges {} attributed {:?}", oe.len(), by_face);
    }
}

#[test]
fn probe_plain_sphere() {
    for &seg in &[8usize, 16, 32, 64] {
        for &defl in &[0.5_f64, 0.1, 0.01] {
            for &s in &[1.0_f64, 1000.0, 0.001] {
                let mut topo = Topology::new();
                let ball = make_sphere(&mut topo, 10.0 * s, seg).unwrap();
                let mesh =
                    tessellate_solid(&topo, ball, defl * s).unwrap();
                let q = welded_mesh_quality(&mesh);
                let exact = 4.0 / 3.0 * std::f64::consts::PI * (10.0 * s).powi(3);
                println!(
                    "seg={seg} defl={defl} scale={s}: tris={} open={} vol={:.6} exact={exact:.6} ratio={:.6}",
                    mesh.indices.len() / 3,
                    q.boundary_edges,
                    mesh_volume(&mesh),
                    mesh_volume(&mesh) / exact
                );
            }
        }
    }
}

#[test]
fn probe_bore_face_direct() {
    for &bore in &[3.0_f64, 1.0] {
        let (topo, sol) = cross_drilled_shaft(bore, 1.0);
        let faces = brepkit_topology::explorer::solid_faces(&topo, sol).unwrap();
        for (i, &fid) in faces.iter().enumerate() {
            let f = topo.face(fid).unwrap();
            if !matches!(f.surface(), FaceSurface::Cylinder(_)) || !f.inner_wires().is_empty() {
                continue;
            }
            let ow = topo.wire(f.outer_wire()).unwrap();
            for oe in ow.edges() {
                let e = topo.edge(oe.edge()).unwrap();
                println!(
                    "bore{bore} face{i} edge curve = {}",
                    match e.curve() {
                        brepkit_topology::edge::EdgeCurve::Line => "Line",
                        brepkit_topology::edge::EdgeCurve::NurbsCurve(_) => "Nurbs",
                        brepkit_topology::edge::EdgeCurve::Circle(_) => "Circle",
                        brepkit_topology::edge::EdgeCurve::Ellipse(_) => "Ellipse",
                        _ => "other",
                    }
                );
            }
            for &defl in &[0.5_f64, 0.1, 0.01] {
                let m = brepkit_operations::tessellate::tessellate(&topo, fid, defl).unwrap();
                println!(
                    "  bore{bore} face{i} defl={defl} single-face tris={} area={:.4}",
                    m.indices.len() / 3,
                    area_of(&m, 0..m.indices.len())
                );
            }
        }
    }
}

#[test]
fn probe_bore_uv() {
    let (topo, sol) = cross_drilled_shaft(3.0, 1.0);
    let faces = brepkit_topology::explorer::solid_faces(&topo, sol).unwrap();
    for &fid in &faces {
        let f = topo.face(fid).unwrap();
        if !matches!(f.surface(), FaceSurface::Cylinder(_)) || !f.inner_wires().is_empty() {
            continue;
        }
        let m = brepkit_operations::tessellate::tessellate_with_uvs(&topo, fid, 0.01).unwrap();
        let uv_area: f64 = m
            .mesh
            .indices
            .chunks_exact(3)
            .map(|t| {
                let a = m.uvs[t[0] as usize];
                let b = m.uvs[t[1] as usize];
                let c = m.uvs[t[2] as usize];
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
            })
            .sum();
        let mut umin = f64::INFINITY;
        let mut umax = f64::NEG_INFINITY;
        let mut vmin = f64::INFINITY;
        let mut vmax = f64::NEG_INFINITY;
        for uv in &m.uvs {
            umin = umin.min(uv[0]);
            umax = umax.max(uv[0]);
            vmin = vmin.min(uv[1]);
            vmax = vmax.max(uv[1]);
        }
        println!(
            "face {fid:?}: tris={} uv_area={uv_area:.4} (exact 12) u[{umin:.4},{umax:.4}] v[{vmin:.4},{vmax:.4}] 3d_area={:.4}",
            m.mesh.indices.len() / 3,
            area_of(&m.mesh, 0..m.mesh.indices.len())
        );
        let ob = brepkit_operations::tessellate::boundary_edge_count(&m.mesh);
        println!("   boundary edges {ob}, verts {}", m.mesh.positions.len());
    }
}
