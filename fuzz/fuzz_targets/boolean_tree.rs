//! Structured fuzzing of the boolean engine.
//!
//! Builds a bounded expression tree of primitives, rigid placements and
//! booleans, then asserts the properties a boolean must have. The point is not
//! to reach a panic in the engine — it is to catch a *plausible* answer that
//! is wrong: material invented, a bore filled, a shell left open, two volume
//! integrators disagreeing, or the same input producing two different results.
//!
//! Structural invariants run at every internal node; the expensive
//! measurement, determinism and idempotence batteries run once at the root.

#![no_main]

use libfuzzer_sys::fuzz_target;

mod invariants;
mod shapegen;

use brepkit_topology::Topology;
use invariants as inv;
use shapegen::{Node, Refusal};

/// Cap on result complexity for the expensive root battery. A mesh fallback
/// can produce thousands of planar faces; measuring those four ways turns a
/// fuzz iteration into a timeout report, which teaches nothing.
const HEAVY_FACE_LIMIT: usize = 120;

fuzz_target!(|node: Node| {
    let mut topo = Topology::new();

    // Per-node structural checks, plus operand-relative volume bounds.
    let mut violations_checked = 0usize;
    let root = shapegen::eval(&mut topo, &node, &mut |t, c| {
        violations_checked += 1;
        let what = format!("{} (node {violations_checked})", c.kind.name());

        // I1/I2 — the result is actually a solid.
        if let Ok(census) = inv::census(t, c.result) {
            inv::assert_closed_manifold(&what, &census);

            // I4 — the result may not exceed its operands. Both operands are
            // still live in `topo`, so this is checked where the engine made
            // the claim rather than only at the root.
            if census.faces <= HEAVY_FACE_LIMIT
                && let (Some(a), Some(b), Some(r)) = (
                    inv::measure(t, c.lhs),
                    inv::measure(t, c.rhs),
                    inv::measure(t, c.result),
                )
            {
                inv::assert_volume_bounds(&what, c.kind.name(), &a, &b, &r);
            }
        }
    });

    // Every `Err` here is the engine refusing: an empty algebraic result, an
    // unsupported configuration, a degenerate primitive. Refusing is a correct
    // outcome and is not a finding.
    let root = match root {
        Ok(root) => root,
        Err(Refusal::Engine(_) | Refusal::Degenerate) => return,
    };

    let Ok(census) = inv::census(&topo, root) else {
        return;
    };
    inv::assert_closed_manifold("root", &census);

    if census.faces > HEAVY_FACE_LIMIT {
        return;
    }

    // I1 (mesh rung) — a closed B-Rep that tessellates leaky is still broken.
    if let Ok(aabb) = brepkit_operations::measure::solid_bounding_box(&topo, root) {
        let diag = (aabb.max - aabb.min).length();
        inv::assert_watertight_mesh("root", &topo, root, inv::volume_deflection(diag) * 4.0);
    }

    // I5 — two independent volume routes must agree, and refining the
    // tessellation must not inflate the answer.
    if let Some(m) = inv::measure(&topo, root) {
        inv::assert_measurements_agree("root", &topo, root, m.volume);
        inv::assert_deflection_stable("root", &topo, root, m.volume);
    }

    // I6 — determinism. A second evaluation of the identical tree, in a fresh
    // arena, must produce the identical fingerprint.
    let Some(fp1) = inv::fingerprint(&topo, root) else {
        return;
    };
    let mut topo2 = Topology::new();
    if let Ok(root2) = shapegen::eval_quiet(&mut topo2, &node)
        && let Some(fp2) = inv::fingerprint(&topo2, root2)
    {
        inv::assert_deterministic("root", &fp1, &fp2);
    }

    // I7 — idempotence. `fuse(r, r)` must be `r`. Built on a copy of the root
    // so the self-fuse operates on two distinct handles.
    check_self_fuse(&topo, root, &census);
});

/// `fuse(a, a)` must be `a`: same hole count, same volume.
fn check_self_fuse(topo: &Topology, root: brepkit_topology::solid::SolidId, before: &inv::Census) {
    use brepkit_operations::boolean::{BooleanOp, boolean};
    use brepkit_operations::copy::copy_solid;

    let mut t = topo.clone();
    let Ok(twin) = copy_solid(&mut t, root) else {
        return;
    };
    let Ok(fused) = boolean(&mut t, BooleanOp::Fuse, root, twin) else {
        return; // a refusal is a pass
    };
    let Ok(after) = inv::census(&t, fused) else {
        return;
    };
    inv::assert_closed_manifold("fuse(a, a)", &after);
    inv::assert_holes_preserved("fuse(a, a)", before, &after);

    let (Some(v0), Some(v1)) = (inv::measure(topo, root), inv::measure(&t, fused)) else {
        return;
    };
    inv::assert_idempotent("fuse(a, a)", before, &after, v0.volume, v1.volume);
}
