//! Face provenance must not depend on the unit a body is modelled in.
//!
//! The same body at 1x, 1000x and 0.001x is the same body: the map from input
//! faces to output faces is identical up to the scale factor. A provenance
//! matcher with an absolute distance budget instead answers differently at
//! every scale — and a wrong answer moves a user's saved face selection onto a
//! different face rather than dropping it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use brepkit_math::vec::Point3;
use brepkit_operations::boolean::{self, BooleanOp, collect_face_signatures};
use brepkit_operations::evolution::{EvolutionMap, build_evolution_by_geometry};
use brepkit_operations::primitives::make_box;
use brepkit_operations::transform::transform_solid;
use brepkit_topology::Topology;

/// Two overlapping boxes fused, everything scaled by `s`.
///
/// A fresh arena each time, so face indices are identical across scales and
/// the maps can be compared entry for entry.
fn fused_pair_evolution(s: f64) -> EvolutionMap {
    let mut topo = Topology::new();
    let a = make_box(&mut topo, 10.0 * s, 10.0 * s, 10.0 * s).unwrap();
    let b = make_box(&mut topo, 10.0 * s, 10.0 * s, 10.0 * s).unwrap();
    let shift = brepkit_math::mat::Mat4::translation(6.0 * s, 0.0, 0.0);
    transform_solid(&mut topo, b, &shift).unwrap();

    let mut inputs = collect_face_signatures(&topo, a).unwrap();
    inputs.extend(collect_face_signatures(&topo, b).unwrap());

    let result = boolean::boolean(&mut topo, BooleanOp::Fuse, a, b).unwrap();
    let outputs = collect_face_signatures(&topo, result).unwrap();

    build_evolution_by_geometry(&inputs, &outputs)
}

fn normalized(evo: &EvolutionMap) -> String {
    let mut modified: Vec<(usize, Vec<usize>)> = evo
        .modified
        .iter()
        .map(|(k, v)| {
            let mut v = v.clone();
            v.sort_unstable();
            (*k, v)
        })
        .collect();
    modified.sort();
    let mut generated: Vec<(usize, Vec<usize>)> = evo
        .generated
        .iter()
        .map(|(k, v)| {
            let mut v = v.clone();
            v.sort_unstable();
            (*k, v)
        })
        .collect();
    generated.sort();
    let mut deleted: Vec<usize> = evo.deleted.iter().copied().collect();
    deleted.sort_unstable();
    format!("modified={modified:?} generated={generated:?} deleted={deleted:?}")
}

#[test]
fn fuse_provenance_is_the_same_at_every_scale() {
    let at_1 = fused_pair_evolution(1.0);
    let at_1000 = fused_pair_evolution(1000.0);
    let at_milli = fused_pair_evolution(0.001);

    assert_eq!(
        normalized(&at_1),
        normalized(&at_1000),
        "1x vs 1000x\n  1x:    {}\n  1000x: {}",
        normalized(&at_1),
        normalized(&at_1000)
    );
    assert_eq!(
        normalized(&at_1),
        normalized(&at_milli),
        "1x vs 0.001x\n  1x:     {}\n  0.001x: {}",
        normalized(&at_1),
        normalized(&at_milli)
    );
}

/// A synthetic pair of unit cubes' worth of face signatures, scaled.
///
/// Removes the boolean engine from the picture entirely: only the matcher is
/// under test, and the correct answer is a 1:1 correspondence.
fn box_signatures(s: f64, base: usize) -> Vec<(usize, brepkit_math::vec::Vec3, Point3)> {
    use brepkit_math::vec::Vec3;
    let h = 5.0 * s;
    vec![
        (base, Vec3::new(0.0, 0.0, -1.0), Point3::new(h, h, 0.0)),
        (
            base + 1,
            Vec3::new(0.0, 0.0, 1.0),
            Point3::new(h, h, 10.0 * s),
        ),
        (base + 2, Vec3::new(0.0, -1.0, 0.0), Point3::new(h, 0.0, h)),
        (
            base + 3,
            Vec3::new(0.0, 1.0, 0.0),
            Point3::new(h, 10.0 * s, h),
        ),
        (base + 4, Vec3::new(-1.0, 0.0, 0.0), Point3::new(0.0, h, h)),
        (
            base + 5,
            Vec3::new(1.0, 0.0, 0.0),
            Point3::new(10.0 * s, h, h),
        ),
    ]
}

#[test]
fn matcher_answer_does_not_move_with_scale() {
    for &s in &[1.0_f64, 1000.0, 0.001] {
        let inputs = box_signatures(s, 0);
        let outputs = box_signatures(s, 100);
        let evo = build_evolution_by_geometry(&inputs, &outputs);
        for i in 0..6 {
            assert_eq!(
                evo.modified.get(&i),
                Some(&vec![100 + i]),
                "scale {s}: face {i} must map to exactly one output, got {:?}",
                evo.modified.get(&i)
            );
        }
        assert!(evo.deleted.is_empty(), "scale {s}: nothing was deleted");
        assert!(evo.generated.is_empty(), "scale {s}: nothing was generated");
    }
}
