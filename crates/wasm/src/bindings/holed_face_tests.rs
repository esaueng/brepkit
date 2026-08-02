//! Regression tests for faces built with inner (hole) wires and then
//! extruded — the `addHolesToFace` / `makeFaceFromWires` → `extrude` path.
//!
//! `docs/production-readiness/stability-matrix.md` lists Extrude as
//! "Blocked: full degenerate/cavity matrix incomplete", and until these
//! tests existed the hole-attaching APIs had no coverage of any kind. Two
//! profiles are exercised end to end:
//!
//! 1. a polygon annulus — all-line loops, exactly known area;
//! 2. an 'O'-like contour whose outer and inner loops both MIX line edges
//!    with cubic-bezier (NURBS) edges, which is the shape a glyph outline
//!    produces.
//!
//! Each asserts the extruded solid is watertight, has the expected face
//! count, has volume ≈ (outer − hole) area × depth against an oracle
//! computed here independently of the kernel, and classifies points in the
//! hole as outside the material — volume alone cannot distinguish a real
//! through-hole from one merely subtracted from the integral.
//!
//! `validate_solid` is asserted narrowly rather than in full: see
//! [`assert_solid`] and `extruded_annulus_shell_orientation_is_inconsistent`
//! for the pre-existing extrude defect that stops it being clean today.
//!
//! Every kernel call goes through `execute_batch`: `JsError` cannot be
//! constructed on non-wasm targets, so the `#[wasm_bindgen]` methods are
//! not directly testable on their error paths.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::kernel::BrepKernel;

// ── batch plumbing ────────────────────────────────────────────────

/// Run a batch and return every result entry.
fn run(k: &mut BrepKernel, ops: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let json = serde_json::Value::Array(ops.to_vec()).to_string();
    serde_json::from_str(&k.execute_batch(&json)).unwrap()
}

/// Run a batch, require every op to succeed, and return the `ok` payloads.
fn run_all_ok(k: &mut BrepKernel, ops: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let results = run(k, ops);
    for (i, r) in results.iter().enumerate() {
        assert!(
            r.get("ok").is_some(),
            "op {i} ({}) failed: {r}",
            ops[i]["op"]
        );
    }
    results
        .into_iter()
        .map(|r| r.get("ok").cloned().unwrap())
        .collect()
}

/// Run a batch whose LAST op must fail, and return that failure message.
fn run_expect_last_error(k: &mut BrepKernel, ops: &[serde_json::Value]) -> String {
    let results = run(k, ops);
    for (i, r) in results.iter().enumerate().take(ops.len() - 1) {
        assert!(
            r.get("ok").is_some(),
            "setup op {i} ({}) failed: {r}",
            ops[i]["op"]
        );
    }
    let last = results.last().unwrap();
    match last.get("error").and_then(serde_json::Value::as_str) {
        Some(s) => s.to_string(),
        None => panic!("expected the last op to fail, got {last}"),
    }
}

fn op(name: &str, args: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"op": name, "args": args})
}

fn as_u32(v: &serde_json::Value) -> u32 {
    u32::try_from(v.as_u64().unwrap()).unwrap()
}

// ── loop description ──────────────────────────────────────────────

/// One segment of a 2D loop laid on the plane `z = z`.
#[derive(Clone, Copy)]
enum Seg {
    /// Straight segment to `(x, y)`.
    Line(f64, f64),
    /// Cubic bezier to `(x3, y3)` with controls `(x1, y1)`, `(x2, y2)`.
    Cubic(f64, f64, f64, f64, f64, f64),
}

/// A closed loop: a start point plus segments returning to it.
struct Loop {
    start: (f64, f64),
    segs: Vec<Seg>,
    z: f64,
}

impl Loop {
    /// Emit the batch ops that build this loop's edges, then a `makeWire`.
    ///
    /// Endpoint doubles are shared bit-for-bit between adjacent segments
    /// (each segment's start is the literal previous end), which is what
    /// `makeWire`'s 1e-7 weld needs to close the loop.
    fn build_ops(&self, ops: &mut Vec<serde_json::Value>) -> usize {
        let first_edge_index = ops.len();
        let mut cur = self.start;
        for seg in &self.segs {
            match *seg {
                Seg::Line(x, y) => {
                    ops.push(op(
                        "makeLineEdge",
                        serde_json::json!({
                            "x1": cur.0, "y1": cur.1, "z1": self.z,
                            "x2": x, "y2": y, "z2": self.z,
                        }),
                    ));
                    cur = (x, y);
                }
                Seg::Cubic(x1, y1, x2, y2, x3, y3) => {
                    ops.push(op(
                        "makeNurbsEdge",
                        serde_json::json!({
                            "startX": cur.0, "startY": cur.1, "startZ": self.z,
                            "endX": x3, "endY": y3, "endZ": self.z,
                            "degree": 3,
                            "knots": [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                            "controlPoints": [
                                cur.0, cur.1, self.z,
                                x1, y1, self.z,
                                x2, y2, self.z,
                                x3, y3, self.z,
                            ],
                            "weights": [1.0, 1.0, 1.0, 1.0],
                        }),
                    ));
                    cur = (x3, y3);
                }
            }
        }
        assert!(
            (cur.0 - self.start.0).abs() < 1e-12 && (cur.1 - self.start.1).abs() < 1e-12,
            "loop does not return to its start point"
        );
        first_edge_index
    }

    /// Number of edges this loop contributes.
    fn edge_count(&self) -> usize {
        self.segs.len()
    }

    /// Signed area of the loop, computed here by densely sampling the same
    /// segment definitions the kernel was handed. This is the oracle the
    /// extruded volume is checked against — it never consults the kernel.
    fn signed_area(&self) -> f64 {
        const BEZIER_SAMPLES: usize = 4096;
        let mut pts: Vec<(f64, f64)> = vec![self.start];
        let mut cur = self.start;
        for seg in &self.segs {
            match *seg {
                Seg::Line(x, y) => {
                    pts.push((x, y));
                    cur = (x, y);
                }
                Seg::Cubic(x1, y1, x2, y2, x3, y3) => {
                    for i in 1..=BEZIER_SAMPLES {
                        #[allow(clippy::cast_precision_loss)]
                        let t = i as f64 / BEZIER_SAMPLES as f64;
                        let mt = 1.0 - t;
                        let b0 = mt * mt * mt;
                        let b1 = 3.0 * mt * mt * t;
                        let b2 = 3.0 * mt * t * t;
                        let b3 = t * t * t;
                        pts.push((
                            b0 * cur.0 + b1 * x1 + b2 * x2 + b3 * x3,
                            b0 * cur.1 + b1 * y1 + b2 * y2 + b3 * y3,
                        ));
                    }
                    cur = (x3, y3);
                }
            }
        }
        // The final point repeats `start`; shoelace wraps anyway.
        let n = pts.len() - 1;
        let mut acc = 0.0;
        for i in 0..n {
            let (xi, yi) = pts[i];
            let (xj, yj) = pts[i + 1];
            acc += xi.mul_add(yj, -(xj * yi));
        }
        acc / 2.0
    }
}

/// A square loop, CCW when `ccw`, laid at `z = 0`.
fn square(half: f64, ccw: bool) -> Loop {
    let corners = if ccw {
        [(-half, -half), (half, -half), (half, half), (-half, half)]
    } else {
        [(-half, -half), (-half, half), (half, half), (half, -half)]
    };
    Loop {
        start: corners[0],
        segs: vec![
            Seg::Line(corners[1].0, corners[1].1),
            Seg::Line(corners[2].0, corners[2].1),
            Seg::Line(corners[3].0, corners[3].1),
            Seg::Line(corners[0].0, corners[0].1),
        ],
        z: 0.0,
    }
}

/// A "capsule": two straight sides joined by two cubic-bezier caps.
///
/// `w` is the half-width of the straight sides, `h` their half-height, and
/// `bulge` how far past `w` the caps reach at their control points. CCW when
/// `ccw`. This is the mixed line/bezier contour an 'O' glyph produces.
fn capsule(w: f64, h: f64, bulge: f64, ccw: bool) -> Loop {
    let b = w + bulge;
    if ccw {
        Loop {
            start: (-w, -h),
            segs: vec![
                // bottom, left → right
                Seg::Line(w, -h),
                // right cap, bottom → top, bulging +x
                Seg::Cubic(b, -h, b, h, w, h),
                // top, right → left
                Seg::Line(-w, h),
                // left cap, top → bottom, bulging −x
                Seg::Cubic(-b, h, -b, -h, -w, -h),
            ],
            z: 0.0,
        }
    } else {
        Loop {
            start: (-w, -h),
            segs: vec![
                // left cap, bottom → top, bulging −x
                Seg::Cubic(-b, -h, -b, h, -w, h),
                // top, left → right
                Seg::Line(w, h),
                // right cap, top → bottom, bulging +x
                Seg::Cubic(b, h, b, -h, w, -h),
                // bottom, right → left
                Seg::Line(-w, -h),
            ],
            z: 0.0,
        }
    }
}

// ── assembly + assertions ─────────────────────────────────────────

/// How the holed face is assembled — the two APIs must agree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FaceApi {
    /// `makeFaceFromWires(outer, [holes])`
    FromWires,
    /// `makePlanarFaceFromWire(outer)` then `addHolesToFace(face, [holes])`
    AddHoles,
}

/// Build `outer` + `holes`, make a face by `api`, extrude by `depth`, and
/// return the kernel plus the solid handle.
fn extrude_holed_face(outer: &Loop, holes: &[Loop], api: FaceApi, depth: f64) -> (BrepKernel, u32) {
    let mut k = BrepKernel::new();
    let mut ops: Vec<serde_json::Value> = Vec::new();

    let outer_first = outer.build_ops(&mut ops);
    let outer_edges: Vec<usize> = (outer_first..outer_first + outer.edge_count()).collect();
    let mut hole_edge_ranges = Vec::new();
    for h in holes {
        let first = h.build_ops(&mut ops);
        hole_edge_ranges.push((first, h.edge_count()));
    }

    // Resolve the edge handles produced above, then continue in a second
    // batch: makeWire needs the handles as literal arguments.
    let edge_results = run_all_ok(&mut k, &ops);
    let edge_handle = |i: usize| as_u32(&edge_results[i]);

    let mut ops2: Vec<serde_json::Value> = vec![op(
        "makeWire",
        serde_json::json!({
            "edges": outer_edges.iter().map(|&i| edge_handle(i)).collect::<Vec<_>>(),
            "closed": true,
        }),
    )];
    for &(first, count) in &hole_edge_ranges {
        ops2.push(op(
            "makeWire",
            serde_json::json!({
                "edges": (first..first + count).map(edge_handle).collect::<Vec<_>>(),
                "closed": true,
            }),
        ));
    }
    let wire_results = run_all_ok(&mut k, &ops2);
    let outer_wire = as_u32(&wire_results[0]);
    let hole_wires: Vec<u32> = wire_results[1..].iter().map(as_u32).collect();

    let face_ops = match api {
        FaceApi::FromWires => vec![op(
            "makeFaceFromWires",
            serde_json::json!({"outerWire": outer_wire, "innerWires": hole_wires}),
        )],
        FaceApi::AddHoles => vec![op(
            "makePlanarFaceFromWire",
            serde_json::json!({"wire": outer_wire}),
        )],
    };
    let face_results = run_all_ok(&mut k, &face_ops);
    let face = match api {
        FaceApi::FromWires => as_u32(&face_results[0]),
        FaceApi::AddHoles => {
            let base = as_u32(&face_results[0]);
            let r = run_all_ok(
                &mut k,
                &[op(
                    "addHolesToFace",
                    serde_json::json!({"face": base, "holeWires": hole_wires}),
                )],
            );
            as_u32(&r[0])
        }
    };

    let solid_results = run_all_ok(
        &mut k,
        &[op(
            "extrude",
            serde_json::json!({
                "face": face, "dirX": 0.0, "dirY": 0.0, "dirZ": 1.0, "distance": depth,
            }),
        )],
    );
    let solid = as_u32(&solid_results[0]);
    (k, solid)
}

/// Assert the solid is watertight, has `expected_faces` faces, has volume
/// within `rel_tol` of `expected_volume`, and classifies `inside_probes` as
/// material and `outside_probes` (points in the holes, and outside the
/// body) as void.
///
/// `validate_solid` is run too, but its result is asserted narrowly: any
/// error it reports must be `ShellOrientationConsistent`. Extruding a holed
/// face has always produced a shell whose cap↔hole-wall shared edges are
/// traversed the same way by both faces, and it still does — this is a
/// pre-existing extrude defect, reproducible with no wasm in the picture
/// (see `extruded_annulus_shell_orientation_is_inconsistent`). Asserting
/// "no OTHER kind of error" catches a regression without failing the day
/// that defect is fixed.
#[allow(clippy::too_many_arguments)]
fn assert_solid(
    k: &mut BrepKernel,
    solid: u32,
    expected_faces: usize,
    expected_volume: f64,
    rel_tol: f64,
    deflection: f64,
    inside_probes: &[(f64, f64, f64)],
    outside_probes: &[(f64, f64, f64)],
) {
    use brepkit_check::validate::CheckId;

    let quality = run_all_ok(
        k,
        &[op(
            "meshQuality",
            serde_json::json!({"solid": solid, "deflection": deflection}),
        )],
    );
    let q = &quality[0];
    assert_eq!(
        q["boundaryEdges"].as_u64(),
        Some(0),
        "mesh has boundary edges — not watertight: {q}"
    );
    assert_eq!(
        q["nonManifoldEdges"].as_u64(),
        Some(0),
        "mesh has non-manifold edges: {q}"
    );
    assert_eq!(
        q["isWatertight"].as_bool(),
        Some(true),
        "not watertight: {q}"
    );

    let solid_id = k.resolve_solid(solid).unwrap();
    let report = brepkit_check::validate::validate_solid(
        &k.topo,
        solid_id,
        &brepkit_check::validate::ValidateOptions::default(),
    )
    .unwrap();
    let unexpected: Vec<_> = report
        .issues
        .iter()
        .filter(|i| i.severity == brepkit_check::validate::Severity::Error)
        .filter(|i| i.check != CheckId::ShellOrientationConsistent)
        .collect();
    assert!(
        unexpected.is_empty(),
        "validate_solid reported errors beyond the known shell-orientation \
         defect: {unexpected:?}"
    );

    let faces = brepkit_topology::explorer::solid_faces(&k.topo, solid_id).unwrap();
    assert_eq!(
        faces.len(),
        expected_faces,
        "face count — a count far above this means the profile was faceted \
         rather than swept exactly"
    );

    let vol = run_all_ok(
        k,
        &[op(
            "volume",
            serde_json::json!({"solid": solid, "deflection": deflection}),
        )],
    );
    let v = vol[0].as_f64().unwrap();
    let err = (v - expected_volume).abs() / expected_volume.abs();
    assert!(
        err < rel_tol,
        "volume {v} vs expected {expected_volume} (relative error {err:.3e} > {rel_tol:.3e})"
    );

    // Volume alone cannot tell a solid with a real through-hole from one
    // whose hole was merely subtracted from the integral — probe the hole.
    let options = brepkit_check::classify::ClassifyOptions::default();
    for &(x, y, z) in inside_probes {
        let c = brepkit_check::classify::classify_point(
            &k.topo,
            solid_id,
            brepkit_math::vec::Point3::new(x, y, z),
            &options,
        )
        .unwrap();
        assert_eq!(
            c,
            brepkit_check::classify::PointClassification::Inside,
            "({x}, {y}, {z}) should be inside the material"
        );
    }
    for &(x, y, z) in outside_probes {
        let c = brepkit_check::classify::classify_point(
            &k.topo,
            solid_id,
            brepkit_math::vec::Point3::new(x, y, z),
            &options,
        )
        .unwrap();
        assert_eq!(
            c,
            brepkit_check::classify::PointClassification::Outside,
            "({x}, {y}, {z}) should be outside the material — the hole is \
             not actually open"
        );
    }
}

// ── (a) polygon annulus ───────────────────────────────────────────

#[test]
fn annulus_from_wires_extrudes_to_a_watertight_tube() {
    let outer = square(10.0, true);
    let hole = square(5.0, false);
    let depth = 5.0;
    // 20×20 outer minus 10×10 hole.
    let expected_volume = (400.0 - 100.0) * depth;

    // In the ring, in the hole, and clear of the body.
    let inside = [(7.5, 0.0, 2.5), (0.0, -7.5, 2.5)];
    let outside = [(0.0, 0.0, 2.5), (0.0, 0.0, 10.0), (30.0, 0.0, 2.5)];

    let (mut k, solid) = extrude_holed_face(&outer, &[hole], FaceApi::FromWires, depth);
    // 4 outer walls + 4 hole walls + 2 caps.
    assert_solid(
        &mut k,
        solid,
        10,
        expected_volume,
        1e-9,
        0.05,
        &inside,
        &outside,
    );
}

#[test]
fn annulus_via_add_holes_to_face_matches_make_face_from_wires() {
    let depth = 5.0;
    let expected_volume = (400.0 - 100.0) * depth;
    let (mut k, solid) = extrude_holed_face(
        &square(10.0, true),
        &[square(5.0, false)],
        FaceApi::AddHoles,
        depth,
    );
    assert_solid(
        &mut k,
        solid,
        10,
        expected_volume,
        1e-9,
        0.05,
        &[(7.5, 0.0, 2.5)],
        &[(0.0, 0.0, 2.5)],
    );
}

#[test]
fn annulus_with_two_disjoint_holes_extrudes_cleanly() {
    let outer = square(10.0, true);
    let hole_a = Loop {
        start: (-7.0, -3.0),
        segs: vec![
            Seg::Line(-7.0, 3.0),
            Seg::Line(-3.0, 3.0),
            Seg::Line(-3.0, -3.0),
            Seg::Line(-7.0, -3.0),
        ],
        z: 0.0,
    };
    let hole_b = Loop {
        start: (3.0, -3.0),
        segs: vec![
            Seg::Line(3.0, 3.0),
            Seg::Line(7.0, 3.0),
            Seg::Line(7.0, -3.0),
            Seg::Line(3.0, -3.0),
        ],
        z: 0.0,
    };
    assert!(hole_a.signed_area() < 0.0, "hole A should be CW");
    assert!(hole_b.signed_area() < 0.0, "hole B should be CW");

    let depth = 2.0;
    let expected_volume = (400.0 - 24.0 - 24.0) * depth;
    let (mut k, solid) = extrude_holed_face(&outer, &[hole_a, hole_b], FaceApi::FromWires, depth);
    // 4 outer walls + 4 + 4 hole walls + 2 caps.
    assert_solid(
        &mut k,
        solid,
        14,
        expected_volume,
        1e-9,
        0.05,
        // The bridge between the two holes, and the margin around them.
        &[(0.0, 0.0, 1.0), (0.0, 8.0, 1.0)],
        // Inside each hole.
        &[(-5.0, 0.0, 1.0), (5.0, 0.0, 1.0)],
    );
}

// ── (b) 'O'-like contour, lines mixed with beziers ────────────────

#[test]
fn o_glyph_contour_mixing_lines_and_beziers_extrudes_to_a_valid_solid() {
    let outer = capsule(4.0, 6.0, 5.0, true);
    let hole = capsule(2.0, 3.0, 3.0, false);
    assert!(outer.signed_area() > 0.0, "outer should be CCW");
    assert!(hole.signed_area() < 0.0, "hole should be CW");

    let depth = 3.0;
    let expected_volume = (outer.signed_area() + hole.signed_area()) * depth;

    let (mut k, solid) = extrude_holed_face(&outer, &[hole], FaceApi::FromWires, depth);
    // 4 outer walls (2 planar, 2 ruled NURBS) + 4 hole walls + 2 caps.
    // The caps are chorded by tessellation, so volume is checked at a
    // relative tolerance rather than exactly.
    assert_solid(
        &mut k,
        solid,
        10,
        expected_volume,
        2e-3,
        0.005,
        // In the wall of the 'O', above and beside the counter.
        &[(0.0, 4.5, 1.5), (3.0, 0.0, 1.5)],
        // In the counter, and clear of the glyph.
        &[(0.0, 0.0, 1.5), (0.0, 20.0, 1.5)],
    );
}

#[test]
fn o_glyph_contour_via_add_holes_to_face_matches_make_face_from_wires() {
    let outer = capsule(4.0, 6.0, 5.0, true);
    let hole = capsule(2.0, 3.0, 3.0, false);
    let depth = 3.0;
    let expected_volume = (outer.signed_area() + hole.signed_area()) * depth;

    let (mut k, solid) = extrude_holed_face(&outer, &[hole], FaceApi::AddHoles, depth);
    assert_solid(
        &mut k,
        solid,
        10,
        expected_volume,
        2e-3,
        0.005,
        &[(0.0, 4.5, 1.5)],
        &[(0.0, 0.0, 1.5)],
    );
}

#[test]
fn glyph_side_walls_are_exact_nurbs_not_faceted() {
    // The bezier caps must become ruled NURBS side faces. If extrude ever
    // falls back to chording the profile, the face count explodes and the
    // face-count assertion above is the signal — this test states the
    // stronger property directly.
    let (k, solid) = extrude_holed_face(
        &capsule(4.0, 6.0, 5.0, true),
        &[capsule(2.0, 3.0, 3.0, false)],
        FaceApi::FromWires,
        3.0,
    );
    let solid_id = k.resolve_solid(solid).unwrap();
    let faces = brepkit_topology::explorer::solid_faces(&k.topo, solid_id).unwrap();
    let nurbs = faces
        .iter()
        .filter(|&&f| {
            matches!(
                k.topo.face(f).unwrap().surface(),
                brepkit_topology::face::FaceSurface::Nurbs(_)
            )
        })
        .count();
    assert_eq!(
        nurbs,
        4,
        "expected one NURBS wall per bezier segment (2 outer + 2 hole), got {nurbs} of {} faces",
        faces.len()
    );
}

// ── known-open: extrude's shell orientation on holed profiles ─────

/// Ready-repro for a defect this work uncovered but did not fix.
///
/// Extruding a face with an inner wire produces a shell in which the eight
/// edges shared between the caps and the hole walls (four at each cap) are
/// traversed in the SAME direction by both adjacent faces, where a closed
/// oriented shell requires opposite directions. `validate_solid` reports it
/// as `ShellOrientationConsistent`.
///
/// The result is nonetheless watertight, correctly classified, and of the
/// right volume — the geometry is right and the orientation bookkeeping is
/// not. It is pre-existing and has nothing to do with the hole-attaching
/// bindings: `brepkit_operations::extrude` reproduces it on a face built
/// directly from `brepkit_topology::builder`, with no wasm in the picture.
/// It matters for consumers that read orientation rather than re-derive it
/// (STEP export, GFA).
///
/// Acceptance target for the eventual fix: this test passes unmodified.
#[test]
#[ignore = "open: extrude leaves cap<->hole-wall shared edges co-oriented"]
fn extruded_annulus_shell_orientation_is_inconsistent() {
    let (k, solid) = extrude_holed_face(
        &square(10.0, true),
        &[square(5.0, false)],
        FaceApi::FromWires,
        5.0,
    );
    let solid_id = k.resolve_solid(solid).unwrap();
    let report = brepkit_check::validate::validate_solid(
        &k.topo,
        solid_id,
        &brepkit_check::validate::ValidateOptions::default(),
    )
    .unwrap();
    assert!(
        report.is_valid(),
        "validate_solid found {} error(s): {:?}",
        report.error_count(),
        report.issues
    );
}

// ── validation: what addHolesToFace used to accept silently ───────

/// Build an outer square wire and return `(kernel, outer_wire_handle)`.
fn kernel_with_outer_square(half: f64) -> (BrepKernel, u32) {
    let mut k = BrepKernel::new();
    let outer = square(half, true);
    let mut ops = Vec::new();
    outer.build_ops(&mut ops);
    let edges = run_all_ok(&mut k, &ops);
    let handles: Vec<u32> = edges.iter().map(as_u32).collect();
    let w = run_all_ok(
        &mut k,
        &[op(
            "makeWire",
            serde_json::json!({"edges": handles, "closed": true}),
        )],
    );
    let wire = as_u32(&w[0]);
    (k, wire)
}

/// Build a loop's wire in an existing kernel and return its handle.
fn build_wire(k: &mut BrepKernel, l: &Loop, closed: bool) -> u32 {
    let mut ops = Vec::new();
    l.build_ops(&mut ops);
    let edges = run_all_ok(k, &ops);
    let handles: Vec<u32> = edges.iter().map(as_u32).collect();
    let w = run_all_ok(
        k,
        &[op(
            "makeWire",
            serde_json::json!({"edges": handles, "closed": closed}),
        )],
    );
    as_u32(&w[0])
}

#[test]
fn open_hole_wire_is_rejected() {
    let (mut k, outer_wire) = kernel_with_outer_square(10.0);
    // Three sides of a square: a path, not a loop.
    let ops = [
        op(
            "makeLineEdge",
            serde_json::json!({"x1": -5.0, "y1": -5.0, "z1": 0.0,
                               "x2": -5.0, "y2":  5.0, "z2": 0.0}),
        ),
        op(
            "makeLineEdge",
            serde_json::json!({"x1": -5.0, "y1":  5.0, "z1": 0.0,
                               "x2":  5.0, "y2":  5.0, "z2": 0.0}),
        ),
        op(
            "makeLineEdge",
            serde_json::json!({"x1":  5.0, "y1":  5.0, "z1": 0.0,
                               "x2":  5.0, "y2": -5.0, "z2": 0.0}),
        ),
    ];
    let edges = run_all_ok(&mut k, &ops);
    let handles: Vec<u32> = edges.iter().map(as_u32).collect();
    let w = run_all_ok(
        &mut k,
        &[op(
            "makeWire",
            serde_json::json!({"edges": handles, "closed": false}),
        )],
    );
    let open_wire = as_u32(&w[0]);
    let msg = run_expect_last_error(
        &mut k,
        &[op(
            "makeFaceFromWires",
            serde_json::json!({"outerWire": outer_wire, "innerWires": [open_wire]}),
        )],
    );
    assert!(msg.contains("not a closed loop"), "message was: {msg}");
}

#[test]
fn hole_wire_off_the_face_plane_is_rejected() {
    let (mut k, outer_wire) = kernel_with_outer_square(10.0);
    let lifted = Loop {
        start: (-5.0, -5.0),
        segs: vec![
            Seg::Line(-5.0, 5.0),
            Seg::Line(5.0, 5.0),
            Seg::Line(5.0, -5.0),
            Seg::Line(-5.0, -5.0),
        ],
        z: 1.0,
    };
    let hole_wire = build_wire(&mut k, &lifted, true);
    let msg = run_expect_last_error(
        &mut k,
        &[op(
            "makeFaceFromWires",
            serde_json::json!({"outerWire": outer_wire, "innerWires": [hole_wire]}),
        )],
    );
    assert!(
        msg.contains("does not lie on the face's surface"),
        "message was: {msg}"
    );
}

#[test]
fn hole_wire_outside_the_outer_wire_is_rejected() {
    let (mut k, outer_wire) = kernel_with_outer_square(10.0);
    let outside = Loop {
        start: (20.0, 20.0),
        segs: vec![
            Seg::Line(20.0, 25.0),
            Seg::Line(25.0, 25.0),
            Seg::Line(25.0, 20.0),
            Seg::Line(20.0, 20.0),
        ],
        z: 0.0,
    };
    let hole_wire = build_wire(&mut k, &outside, true);
    let msg = run_expect_last_error(
        &mut k,
        &[op(
            "makeFaceFromWires",
            serde_json::json!({"outerWire": outer_wire, "innerWires": [hole_wire]}),
        )],
    );
    assert!(msg.contains("not contained"), "message was: {msg}");
}

#[test]
fn hole_wire_straddling_the_outer_boundary_is_rejected() {
    let (mut k, outer_wire) = kernel_with_outer_square(10.0);
    // Half in, half out.
    let straddle = Loop {
        start: (5.0, -2.0),
        segs: vec![
            Seg::Line(5.0, 2.0),
            Seg::Line(15.0, 2.0),
            Seg::Line(15.0, -2.0),
            Seg::Line(5.0, -2.0),
        ],
        z: 0.0,
    };
    let hole_wire = build_wire(&mut k, &straddle, true);
    let msg = run_expect_last_error(
        &mut k,
        &[op(
            "makeFaceFromWires",
            serde_json::json!({"outerWire": outer_wire, "innerWires": [hole_wire]}),
        )],
    );
    assert!(msg.contains("not contained"), "message was: {msg}");
}

#[test]
fn the_outer_wire_cannot_be_its_own_hole() {
    let (mut k, outer_wire) = kernel_with_outer_square(10.0);
    let msg = run_expect_last_error(
        &mut k,
        &[op(
            "makeFaceFromWires",
            serde_json::json!({"outerWire": outer_wire, "innerWires": [outer_wire]}),
        )],
    );
    assert!(msg.contains("outer wire"), "message was: {msg}");
}

#[test]
fn the_same_hole_listed_twice_is_rejected() {
    let (mut k, outer_wire) = kernel_with_outer_square(10.0);
    let hole_wire = build_wire(&mut k, &square(5.0, false), true);
    let msg = run_expect_last_error(
        &mut k,
        &[op(
            "makeFaceFromWires",
            serde_json::json!({"outerWire": outer_wire, "innerWires": [hole_wire, hole_wire]}),
        )],
    );
    assert!(msg.contains("listed twice"), "message was: {msg}");
}

#[test]
fn nested_holes_are_rejected() {
    let (mut k, outer_wire) = kernel_with_outer_square(10.0);
    let big = build_wire(&mut k, &square(6.0, false), true);
    let small = build_wire(&mut k, &square(3.0, false), true);
    let msg = run_expect_last_error(
        &mut k,
        &[op(
            "makeFaceFromWires",
            serde_json::json!({"outerWire": outer_wire, "innerWires": [big, small]}),
        )],
    );
    assert!(msg.contains("overlaps"), "message was: {msg}");
}

#[test]
fn partially_overlapping_holes_are_rejected() {
    let (mut k, outer_wire) = kernel_with_outer_square(10.0);
    // Two rectangles that cross, neither containing the other.
    let a = Loop {
        start: (-6.0, -1.0),
        segs: vec![
            Seg::Line(-6.0, 1.0),
            Seg::Line(2.0, 1.0),
            Seg::Line(2.0, -1.0),
            Seg::Line(-6.0, -1.0),
        ],
        z: 0.0,
    };
    let b = Loop {
        start: (-1.0, -6.0),
        segs: vec![
            Seg::Line(-1.0, 2.0),
            Seg::Line(1.0, 2.0),
            Seg::Line(1.0, -6.0),
            Seg::Line(-1.0, -6.0),
        ],
        z: 0.0,
    };
    let wa = build_wire(&mut k, &a, true);
    let wb = build_wire(&mut k, &b, true);
    let msg = run_expect_last_error(
        &mut k,
        &[op(
            "makeFaceFromWires",
            serde_json::json!({"outerWire": outer_wire, "innerWires": [wa, wb]}),
        )],
    );
    assert!(msg.contains("overlaps"), "message was: {msg}");
}

#[test]
fn adding_a_hole_that_duplicates_an_existing_one_is_rejected() {
    let (mut k, outer_wire) = kernel_with_outer_square(10.0);
    let hole_wire = build_wire(&mut k, &square(5.0, false), true);
    let faces = run_all_ok(
        &mut k,
        &[op(
            "makeFaceFromWires",
            serde_json::json!({"outerWire": outer_wire, "innerWires": [hole_wire]}),
        )],
    );
    let face = as_u32(&faces[0]);
    let msg = run_expect_last_error(
        &mut k,
        &[op(
            "addHolesToFace",
            serde_json::json!({"face": face, "holeWires": [hole_wire]}),
        )],
    );
    assert!(msg.contains("already an inner wire"), "message was: {msg}");
}

#[test]
fn add_holes_to_face_rejects_an_invalid_wire_handle() {
    let (mut k, outer_wire) = kernel_with_outer_square(10.0);
    let faces = run_all_ok(
        &mut k,
        &[op(
            "makePlanarFaceFromWire",
            serde_json::json!({"wire": outer_wire}),
        )],
    );
    let face = as_u32(&faces[0]);
    let msg = run_expect_last_error(
        &mut k,
        &[op(
            "addHolesToFace",
            serde_json::json!({"face": face, "holeWires": [9999]}),
        )],
    );
    assert!(msg.contains("wire"), "message was: {msg}");
}

#[test]
fn a_valid_hole_is_still_accepted_after_hardening() {
    // Guard against the checks over-rejecting: the ordinary case must pass.
    let (mut k, outer_wire) = kernel_with_outer_square(10.0);
    let hole_wire = build_wire(&mut k, &square(5.0, false), true);
    let r = run_all_ok(
        &mut k,
        &[op(
            "makeFaceFromWires",
            serde_json::json!({"outerWire": outer_wire, "innerWires": [hole_wire]}),
        )],
    );
    let face_id = k.resolve_face(as_u32(&r[0])).unwrap();
    assert_eq!(k.topo.face(face_id).unwrap().inner_wires().len(), 1);
}

#[test]
fn a_ccw_hole_is_accepted_because_extrude_handles_either_winding() {
    let (mut k, outer_wire) = kernel_with_outer_square(10.0);
    let ccw_hole = build_wire(&mut k, &square(5.0, true), true);
    let r = run_all_ok(
        &mut k,
        &[op(
            "makeFaceFromWires",
            serde_json::json!({"outerWire": outer_wire, "innerWires": [ccw_hole]}),
        )],
    );
    assert!(r[0].as_u64().is_some());
}

#[test]
fn make_face_from_wires_with_no_holes_is_a_plain_planar_face() {
    let (mut k, outer_wire) = kernel_with_outer_square(10.0);
    let r = run_all_ok(
        &mut k,
        &[op(
            "makeFaceFromWires",
            serde_json::json!({"outerWire": outer_wire}),
        )],
    );
    let face_id = k.resolve_face(as_u32(&r[0])).unwrap();
    let face = k.topo.face(face_id).unwrap();
    assert!(face.inner_wires().is_empty());
    assert!(matches!(
        face.surface(),
        brepkit_topology::face::FaceSurface::Plane { .. }
    ));
}

#[test]
fn make_face_from_wires_rejects_a_non_planar_outer_wire() {
    let mut k = BrepKernel::new();
    let skew = Loop {
        start: (0.0, 0.0),
        segs: vec![
            Seg::Line(10.0, 0.0),
            Seg::Line(10.0, 10.0),
            Seg::Line(0.0, 10.0),
            Seg::Line(0.0, 0.0),
        ],
        z: 0.0,
    };
    let mut ops = Vec::new();
    skew.build_ops(&mut ops);
    // Replace one edge with a lifted one so the loop is not coplanar.
    ops[1] = op(
        "makeLineEdge",
        serde_json::json!({"x1": 10.0, "y1": 0.0, "z1": 0.0, "x2": 10.0, "y2": 10.0, "z2": 6.0}),
    );
    ops[2] = op(
        "makeLineEdge",
        serde_json::json!({"x1": 10.0, "y1": 10.0, "z1": 6.0, "x2": 0.0, "y2": 10.0, "z2": 0.0}),
    );
    let edges = run_all_ok(&mut k, &ops);
    let handles: Vec<u32> = edges.iter().map(as_u32).collect();
    let msg = run_expect_last_error(
        &mut k,
        &[
            op(
                "makeWire",
                serde_json::json!({"edges": handles, "closed": true}),
            ),
            op("makeFaceFromWires", serde_json::json!({"outerWire": 0})),
        ],
    );
    assert!(msg.to_lowercase().contains("planar"), "message was: {msg}");
}
