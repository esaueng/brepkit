//! Property oracles for the structured targets.
//!
//! A crash fuzzer would have found none of the fourteen kernel defects that
//! motivated this harness. Every one of them produced confident, well-formed,
//! *wrong* output: a bore filled in, a shell left open, a face that measured
//! zero, an operation that quietly did four of the five things it was asked
//! to do. So the value here is entirely in the oracles, not in reaching
//! `unreachable!()`.
//!
//! Each function below states a property the engine must hold, and panics
//! with the numbers when it does not — libFuzzer turns that into a
//! reproducible artifact.
//!
//! **A typed refusal is a pass.** `Unsupported`, `RadiusTooLarge`,
//! `EmptyResult` and friends are the engine correctly declining to return a
//! wrong answer. Callers stop the case; nothing here is invoked.

#![allow(dead_code)]

use std::collections::BTreeMap;

use brepkit_math::aabb::Aabb3;
use brepkit_operations::measure::{mass_properties, solid_bounding_box, solid_volume};
use brepkit_operations::tessellate::{
    boundary_edge_count, non_manifold_edge_count, tessellate_solid,
};
use brepkit_topology::Topology;
use brepkit_topology::explorer;
use brepkit_topology::solid::SolidId;

/// Relative slack for volume comparisons.
///
/// Deliberately loose. These are *gross-disagreement* detectors, not precision
/// checks: the defects they exist to catch were order-one — a bore counted as
/// material, a wall integrating to exactly zero, 1735 mm³ invented from
/// nothing. A tight bound here would only manufacture false positives out of
/// ordinary inscribed-mesh undercount.
pub const VOL_SLACK: f64 = 1e-2;

/// Absolute floor, so near-zero volumes do not divide the relative test.
pub const VOL_FLOOR: f64 = 1e-6;

/// Topological census of a solid, cheap enough to run at every tree node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Census {
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    /// Total inner wires across all faces — the hole mouths.
    pub inner_wires: usize,
    /// Edges referenced by exactly one face use: the shell is open there.
    pub free_edges: usize,
    /// Edges referenced by three or more face uses.
    pub non_manifold_edges: usize,
    /// Edges referenced by no face at all.
    pub orphan_edges: usize,
    /// Per surface-type face counts, for the analytic-vs-mesh tell.
    pub surfaces: BTreeMap<&'static str, usize>,
}

impl Census {
    /// `V - E + F`, unadjusted.
    #[must_use]
    #[allow(clippy::cast_possible_wrap)]
    pub fn euler(&self) -> i64 {
        self.vertices as i64 - self.edges as i64 + self.faces as i64
    }

    /// `2 - (V - E + F - L)`, which must be a non-negative even number:
    /// twice the genus of a closed orientable surface.
    #[must_use]
    #[allow(clippy::cast_possible_wrap)]
    pub fn twice_genus(&self) -> i64 {
        2 - (self.euler() - self.inner_wires as i64)
    }
}

/// Take a census.
///
/// # Errors
///
/// Propagates topology lookup failures.
pub fn census(topo: &Topology, solid: SolidId) -> Result<Census, brepkit_topology::TopologyError> {
    let (faces, edges, vertices) = explorer::solid_entity_counts(topo, solid)?;

    let mut inner_wires = 0;
    let mut surfaces: BTreeMap<&'static str, usize> = BTreeMap::new();
    for fid in explorer::solid_faces(topo, solid)? {
        let face = topo.face(fid)?;
        inner_wires += face.inner_wires().len();
        *surfaces.entry(face.surface().type_tag()).or_default() += 1;
    }

    // `edge_to_face_map` counts *face uses*, so a seam edge that appears twice
    // in one face's wire counts twice — which is what manifoldness means here.
    let map = explorer::edge_to_face_map(topo, solid)?;
    let mut free_edges = 0;
    let mut non_manifold_edges = 0;
    let mut orphan_edges = 0;
    for uses in map.values() {
        match uses.len() {
            0 => orphan_edges += 1,
            1 => free_edges += 1,
            2 => {}
            _ => non_manifold_edges += 1,
        }
    }

    Ok(Census {
        faces,
        edges,
        vertices,
        inner_wires,
        free_edges,
        non_manifold_edges,
        orphan_edges,
        surfaces,
    })
}

// ── I1/I2: the result is actually a solid ──────────────────────────────

/// **Closed 2-manifold shell, and a consistent Euler characteristic.**
///
/// Every edge is used by exactly two faces, and `V - E + F - L = 2 - 2g` for
/// a non-negative integer genus `g`.
///
/// Catches the defect class where an operation returns something that is not
/// a solid while every check it was given passed — `shell` returning
/// `V-E+F = 7` where 3 was required, with 72 open mesh edges (#48); the open
/// shells left by `draft` (#41) and `chamfer` (#43).
///
/// # Panics
///
/// Panics with the census when the shell is open, non-manifold, or has an
/// impossible Euler characteristic.
pub fn assert_closed_manifold(what: &str, c: &Census) {
    assert!(
        c.free_edges == 0 && c.non_manifold_edges == 0 && c.orphan_edges == 0,
        "{what}: result is not a closed 2-manifold — {} free edge(s), {} non-manifold edge(s), \
         {} orphan edge(s); census {c:?}",
        c.free_edges,
        c.non_manifold_edges,
        c.orphan_edges,
    );

    let tg = c.twice_genus();
    assert!(
        tg >= 0 && tg % 2 == 0,
        "{what}: Euler characteristic V-E+F = {} with L = {} inner loop(s) implies genus {}, \
         which is not a non-negative integer; census {c:?}",
        c.euler(),
        c.inner_wires,
        f64::from(i32::try_from(tg).unwrap_or(i32::MAX)) / 2.0,
    );
}

/// **Mesh-level watertightness.**
///
/// The B-Rep check above and this one are not the same statement: the
/// tessellator welds shared boundary vertices and can paper over a small
/// B-Rep gap, while a B-Rep that is closed can still tessellate to a leaky
/// mesh through a collapsed seam. Defect #48 was visible here as 72 open mesh
/// edges, so both rungs are checked.
///
/// # Panics
///
/// Panics when the mesh has boundary or non-manifold edges.
pub fn assert_watertight_mesh(what: &str, topo: &Topology, solid: SolidId, deflection: f64) {
    let Ok(mesh) = tessellate_solid(topo, solid, deflection) else {
        // A tessellation refusal is the engine declining, not a wrong answer.
        return;
    };
    let b = boundary_edge_count(&mesh);
    let n = non_manifold_edge_count(&mesh);
    assert!(
        b == 0 && n == 0,
        "{what}: tessellation at deflection {deflection} is not watertight — \
         {b} boundary edge(s), {n} non-manifold edge(s)",
    );
}

// ── I3: hole preservation ──────────────────────────────────────────────

/// **A modifier must not silently reduce the hole count.**
///
/// This one invariant covers the single largest defect class in the batch
/// that motivated the harness: `defeature` (#39), `draft` (#41),
/// `chamfer` (#43), `split` (#45) and `shell` (#48) each returned a solid
/// with the bore filled in, and each passed every check it was given.
///
/// Applied only to operations whose contract says the holes survive. An
/// operation *asked* to remove a hole is exempt, and a refusal is exempt.
///
/// # Panics
///
/// Panics when inner wires were lost.
pub fn assert_holes_preserved(what: &str, before: &Census, after: &Census) {
    assert!(
        after.inner_wires >= before.inner_wires,
        "{what}: dropped inner wires without saying so — {} before, {} after. \
         An operation that cannot keep a hole must refuse with a typed error, \
         not return a filled body. before {before:?}; after {after:?}",
        before.inner_wires,
        after.inner_wires,
    );
}

// ── I4: volume bounds ──────────────────────────────────────────────────

/// A volume reading, plus the box it came from.
pub struct Measured {
    pub volume: f64,
    pub aabb: Aabb3,
}

/// Read a solid's volume and bounding box.
///
/// Returns `None` when either measurement declines — a refusal, not a finding.
#[must_use]
pub fn measure(topo: &Topology, solid: SolidId) -> Option<Measured> {
    let aabb = solid_bounding_box(topo, solid).ok()?;
    let diag = (aabb.max - aabb.min).length();
    let volume = solid_volume(topo, solid, volume_deflection(diag)).ok()?;
    if !volume.is_finite() {
        return None;
    }
    Some(Measured { volume, aabb })
}

/// A deflection fine enough to beat `solid_volume`'s internal clamp
/// (`min(requested, bbox_diag * 5e-5)`), so two requests actually differ,
/// but coarse enough that a fuzz iteration stays sub-second.
#[must_use]
pub fn volume_deflection(diag: f64) -> f64 {
    if diag.is_finite() && diag > 0.0 {
        (diag * 4e-5).max(1e-7)
    } else {
        1e-3
    }
}

/// **A boolean may not invent material.**
///
/// * `cut(a, b)` ⊆ `a`
/// * `fuse(a, b)` ≤ `vol(a) + vol(b)` and ≥ `max(vol(a), vol(b))`
/// * `intersect(a, b)` ≤ `min(vol(a), vol(b))`
///
/// and in every case the result's bounding box lies inside the operands'.
///
/// Catches `split` inventing 1735 mm³ (#45) and any boolean that returns a
/// superset of what it was given.
///
/// # Panics
///
/// Panics when the result exceeds its operands.
pub fn assert_volume_bounds(what: &str, op: &str, a: &Measured, b: &Measured, r: &Measured) {
    let slack = |v: f64| v.abs().mul_add(VOL_SLACK, VOL_FLOOR);

    match op {
        "cut" => assert!(
            r.volume <= a.volume + slack(a.volume),
            "{what}: cut produced {:.6} from a target of {:.6} — a cut cannot add material",
            r.volume,
            a.volume,
        ),
        "fuse" => {
            let sum = a.volume + b.volume;
            assert!(
                r.volume <= sum + slack(sum),
                "{what}: fuse produced {:.6} from operands summing to {:.6} — \
                 a union cannot exceed the sum of its parts",
                r.volume,
                sum,
            );
            let biggest = a.volume.max(b.volume);
            assert!(
                r.volume >= biggest - slack(biggest),
                "{what}: fuse produced {:.6}, less than its larger operand {:.6} — \
                 a union contains each operand",
                r.volume,
                biggest,
            );
        }
        "intersect" => {
            let smallest = a.volume.min(b.volume);
            assert!(
                r.volume <= smallest + slack(smallest),
                "{what}: intersect produced {:.6}, more than its smaller operand {:.6}",
                r.volume,
                smallest,
            );
        }
        _ => {}
    }

    // Containment in the operands' combined box catches invented material that
    // happens to balance out in the volume total.
    let hull = a.aabb.union(b.aabb);
    let margin = ((hull.max - hull.min).length() * 1e-6).max(1e-6);
    assert!(
        hull.expanded(margin).contains_point(r.aabb.min)
            && hull.expanded(margin).contains_point(r.aabb.max),
        "{what}: result box [{:?} .. {:?}] escapes the operands' box [{:?} .. {:?}] — \
         the result occupies space neither operand did",
        r.aabb.min,
        r.aabb.max,
        hull.min,
        hull.max,
    );
}

// ── I5: measurement agreement ──────────────────────────────────────────

/// **Two independent volume paths must agree.**
///
/// `mass_properties` integrates the exact face geometry with Gauss quadrature
/// and never tessellates; `solid_volume` runs a tessellation/analytic ladder.
/// They share no code below the face list, so a disagreement means one of
/// them is reading the geometry wrong.
///
/// Catches #49 directly: a curved face counted its holes as material, and a
/// bore wall integrated to exactly zero. Both moved `mass_properties` by an
/// order-one amount while `solid_volume` was unchanged.
///
/// # Panics
///
/// Panics when the two disagree beyond [`VOL_SLACK`].
pub fn assert_measurements_agree(what: &str, topo: &Topology, solid: SolidId, tessellated: f64) {
    let Ok(props) = mass_properties(topo, solid) else {
        return; // a refusal, not a finding
    };
    if !props.mass.is_finite() {
        return;
    }
    let scale = props.mass.abs().max(tessellated.abs()).max(VOL_FLOOR);
    let rel = (props.mass - tessellated).abs() / scale;
    assert!(
        rel <= VOL_SLACK,
        "{what}: mass_properties says {:.9} but solid_volume says {:.9} \
         (relative difference {rel:.3e}). These integrate the same faces by \
         independent routes; disagreement means one is misreading the geometry.",
        props.mass,
        tessellated,
    );
}

/// **Refining the tessellation must not inflate the volume.**
///
/// A well-formed inscribed mesh converges to the truth *from below*, so a
/// finer deflection may only move the reading up by a hair. A volume that
/// climbs under refinement is the recorded signature of a self-intersection,
/// a collapsed seam or a doubled face.
///
/// # Panics
///
/// Panics when the two readings differ beyond [`VOL_SLACK`].
pub fn assert_deflection_stable(what: &str, topo: &Topology, solid: SolidId, coarse: f64) {
    let Ok(aabb) = solid_bounding_box(topo, solid) else {
        return;
    };
    let diag = (aabb.max - aabb.min).length();
    let fine = volume_deflection(diag) * 0.4;
    let Ok(v_fine) = solid_volume(topo, solid, fine) else {
        return;
    };
    if !v_fine.is_finite() {
        return;
    }
    let scale = v_fine.abs().max(coarse.abs()).max(VOL_FLOOR);
    let rel = (v_fine - coarse).abs() / scale;
    assert!(
        rel <= VOL_SLACK,
        "{what}: volume moved from {coarse:.9} to {v_fine:.9} when the deflection was \
         refined (relative {rel:.3e}). A sound solid's inscribed mesh converges from \
         below; movement this large signals broken geometry.",
    );
}

// ── I6: determinism ────────────────────────────────────────────────────

/// A canonical, order-independent fingerprint of a solid's topology and
/// coarse geometry.
///
/// Entity *ids* are deliberately excluded: arena indices depend on allocation
/// order, which is an implementation detail. What must be reproducible is the
/// shape — counts, surface census, and the multiset of face normals and
/// centroids, quantized so that last-bit rounding does not register as
/// non-determinism.
#[must_use]
pub fn fingerprint(topo: &Topology, solid: SolidId) -> Option<Vec<u8>> {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let c = census(topo, solid).ok()?;
    let mut rows: Vec<(i64, i64, i64, i64, i64, i64, &'static str)> = Vec::new();
    for fid in explorer::solid_faces(topo, solid).ok()? {
        let face = topo.face(fid).ok()?;
        let verts = brepkit_operations::boolean::face_polygon(topo, fid).ok()?;
        let n = verts.len().max(1) as f64;
        let cx = verts.iter().map(|p| p.x()).sum::<f64>() / n;
        let cy = verts.iter().map(|p| p.y()).sum::<f64>() / n;
        let cz = verts.iter().map(|p| p.z()).sum::<f64>() / n;
        let q = |v: f64| (v * 1e6).round() as i64;
        rows.push((
            q(cx),
            q(cy),
            q(cz),
            verts.len() as i64,
            face.inner_wires().len() as i64,
            i64::from(face.is_reversed()),
            face.surface().type_tag(),
        ));
    }
    rows.sort_unstable();

    let mut h = DefaultHasher::new();
    (c.faces, c.edges, c.vertices, c.inner_wires).hash(&mut h);
    c.surfaces.hash(&mut h);
    rows.hash(&mut h);
    Some(h.finish().to_le_bytes().to_vec())
}

/// **The same input must produce the same output.**
///
/// # Panics
///
/// Panics when two evaluations of the identical tree disagree.
pub fn assert_deterministic(what: &str, first: &[u8], second: &[u8]) {
    assert!(
        first == second,
        "{what}: two evaluations of the identical input produced different topology \
         fingerprints ({first:02x?} vs {second:02x?}). The engine is reading uninitialised \
         state, iterating a hash map, or depending on address order.",
    );
}

// ── I7: idempotence ────────────────────────────────────────────────────

/// **`fuse(a, a)` is `a`, and cutting twice is cutting once.**
///
/// # Panics
///
/// Panics when the repeat differs from the original.
pub fn assert_idempotent(what: &str, once: &Census, twice: &Census, v_once: f64, v_twice: f64) {
    let scale = v_once.abs().max(v_twice.abs()).max(VOL_FLOOR);
    assert!(
        (v_once - v_twice).abs() / scale <= VOL_SLACK,
        "{what}: repeating the operation changed the volume from {v_once:.9} to {v_twice:.9}",
    );
    assert!(
        once.inner_wires == twice.inner_wires,
        "{what}: repeating the operation changed the hole count from {} to {}",
        once.inner_wires,
        twice.inner_wires,
    );
}

// ── I8: completeness ───────────────────────────────────────────────────

/// **An operation given N items processes all N, or fails saying which it did not.**
///
/// Catches #44, where the binding returned a silent subset of the requested
/// blend — indistinguishable from success at the call site.
///
/// # Panics
///
/// Panics on a silent partial success.
pub fn assert_complete(what: &str, requested: usize, succeeded: usize, is_partial: bool) {
    assert!(
        !is_partial && succeeded >= requested,
        "{what}: asked for {requested} item(s), reported {succeeded} succeeded \
         (is_partial = {is_partial}) and still returned Ok. A partial result must be \
         a typed error naming what it skipped, never a success.",
    );
}
