//! Versioned face-evolution payload construction and validation.
//!
//! The operation layer records topology history as arena indices. This module
//! is the trust boundary that turns those records into stable WASM handles. It
//! derives both coverage sets from the topology, then refuses incomplete,
//! duplicate, contradictory, or out-of-result claims before JavaScript sees
//! them.

use std::collections::{BTreeMap, HashSet};

use brepkit_operations::evolution::{EvolutionMap, EvolutionOrigin, FaceSignature};
use brepkit_topology::Topology;
use brepkit_topology::explorer::solid_faces;
use brepkit_topology::solid::SolidId;

use crate::error::WasmError;
use crate::handles::solid_id_to_u32;
use crate::types::{
    EvolutionOriginV1, FaceEvolutionV1, GeneratedFaceV1, ModifiedFacesV1,
    TopologyEvolutionResultV1, UnresolvedFaceV1,
};

/// The only payload version accepted by this decoder.
const EVOLUTION_PAYLOAD_VERSION: u32 = 1;

fn invalid(reason: impl Into<String>) -> WasmError {
    WasmError::InvalidInput {
        reason: format!("invalid topology evolution payload: {}", reason.into()),
    }
}

fn to_handle(index: usize, label: &str) -> Result<u32, WasmError> {
    u32::try_from(index).map_err(|_| invalid(format!("{label} face index {index} exceeds u32")))
}

fn unique_set(values: &[u32], label: &str) -> Result<HashSet<u32>, WasmError> {
    let mut set = HashSet::with_capacity(values.len());
    for &value in values {
        if !set.insert(value) {
            return Err(invalid(format!("duplicate {label} face {value}")));
        }
    }
    Ok(set)
}

/// Build and validate a version 1 payload from a kernel evolution map.
///
/// `input_faces` must be captured before the operation. `result_solid` is
/// inspected after every production engine and acceptance check has completed,
/// so `result_faces` describes the handle actually returned to JavaScript.
pub fn build_payload_v1(
    topo: &Topology,
    result_solid: SolidId,
    input_faces: &[FaceSignature],
    map: &EvolutionMap,
) -> Result<TopologyEvolutionResultV1, WasmError> {
    let mut source_faces = input_faces
        .iter()
        .map(|(index, _, _)| to_handle(*index, "source"))
        .collect::<Result<Vec<_>, _>>()?;
    source_faces.sort_unstable();

    let mut result_faces = solid_faces(topo, result_solid)?
        .into_iter()
        .map(|face| to_handle(face.index(), "result"))
        .collect::<Result<Vec<_>, _>>()?;
    result_faces.sort_unstable();

    let mut modified = map
        .modified
        .iter()
        .map(|(&source, results)| {
            let mut results = results
                .iter()
                .map(|&result| to_handle(result, "modified result"))
                .collect::<Result<Vec<_>, _>>()?;
            results.sort_unstable();
            Ok(ModifiedFacesV1 {
                source: to_handle(source, "modified source")?,
                results,
            })
        })
        .collect::<Result<Vec<_>, WasmError>>()?;
    modified.sort_by_key(|entry| entry.source);

    // EvolutionMap stores generated adjacency as source -> results. The WASM
    // contract groups it by result so a many-source blend band is one claim,
    // not several apparently contradictory claims about the same face.
    let mut generated_by_result: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (&source, results) in &map.generated {
        let source = to_handle(source, "generated source")?;
        for &result in results {
            generated_by_result
                .entry(to_handle(result, "generated result")?)
                .or_default()
                .push(source);
        }
    }
    let generated = generated_by_result
        .into_iter()
        .map(|(result, mut sources)| {
            sources.sort_unstable();
            GeneratedFaceV1 { sources, result }
        })
        .collect();

    let mut deleted = map
        .deleted
        .iter()
        .map(|&source| to_handle(source, "deleted source"))
        .collect::<Result<Vec<_>, _>>()?;
    deleted.sort_unstable();

    let unresolved = map
        .unresolved
        .iter()
        .map(|(&result, candidates)| {
            let mut candidates = candidates
                .iter()
                .map(|&source| to_handle(source, "unresolved candidate"))
                .collect::<Result<Vec<_>, _>>()?;
            candidates.sort_unstable();
            Ok(UnresolvedFaceV1 {
                result: to_handle(result, "unresolved result")?,
                candidates,
            })
        })
        .collect::<Result<Vec<_>, WasmError>>()?;

    let origin = match map.origin {
        EvolutionOrigin::Construction => EvolutionOriginV1::Construction,
        EvolutionOrigin::Geometry => EvolutionOriginV1::Geometry,
    };
    let payload = TopologyEvolutionResultV1 {
        version: EVOLUTION_PAYLOAD_VERSION,
        solid: solid_id_to_u32(result_solid),
        source_faces,
        result_faces,
        evolution: FaceEvolutionV1 {
            modified,
            generated,
            deleted,
            unresolved,
            origin,
        },
    };
    validate_payload_v1(&payload, Some(topo))?;
    Ok(payload)
}

/// Decode persisted or transported JSON and apply the same strict validation
/// used for freshly produced kernel payloads.
pub fn decode_payload_v1(
    topo: &Topology,
    json: &str,
) -> Result<TopologyEvolutionResultV1, WasmError> {
    let payload: TopologyEvolutionResultV1 =
        serde_json::from_str(json).map_err(|error| invalid(format!("malformed JSON: {error}")))?;
    validate_payload_v1(&payload, Some(topo))?;
    Ok(payload)
}

/// Validate the version 1 set invariants, optionally against a live topology.
fn validate_payload_v1(
    payload: &TopologyEvolutionResultV1,
    topo: Option<&Topology>,
) -> Result<(), WasmError> {
    if payload.version != EVOLUTION_PAYLOAD_VERSION {
        return Err(invalid(format!(
            "unsupported version {}; expected {EVOLUTION_PAYLOAD_VERSION}",
            payload.version
        )));
    }

    let sources = unique_set(&payload.source_faces, "source")?;
    let results = unique_set(&payload.result_faces, "result")?;
    let mut accounted_sources = HashSet::with_capacity(sources.len());
    let mut claimed_results = HashSet::with_capacity(results.len());

    for entry in &payload.evolution.modified {
        if !sources.contains(&entry.source) {
            return Err(invalid(format!(
                "modified source {} is not an input face",
                entry.source
            )));
        }
        if !accounted_sources.insert(entry.source) {
            return Err(invalid(format!(
                "source face {} has duplicate or contradictory claims",
                entry.source
            )));
        }
        if entry.results.is_empty() {
            return Err(invalid(format!(
                "modified source {} has no result face",
                entry.source
            )));
        }
        unique_set(&entry.results, "modified result")?;
        for &result in &entry.results {
            claim_result(result, "modified", &results, &mut claimed_results)?;
        }
    }

    let deleted = unique_set(&payload.evolution.deleted, "deleted")?;
    for source in deleted {
        if !sources.contains(&source) {
            return Err(invalid(format!(
                "deleted source {source} is not an input face"
            )));
        }
        if !accounted_sources.insert(source) {
            return Err(invalid(format!(
                "source face {source} is both modified and deleted"
            )));
        }
    }

    for entry in &payload.evolution.generated {
        if entry.sources.is_empty() {
            return Err(invalid(format!(
                "generated result {} has no source",
                entry.result
            )));
        }
        let generated_sources = unique_set(&entry.sources, "generated source")?;
        if let Some(source) = generated_sources
            .iter()
            .find(|source| !sources.contains(source))
        {
            return Err(invalid(format!(
                "generated source {source} is not an input face"
            )));
        }
        claim_result(entry.result, "generated", &results, &mut claimed_results)?;
    }

    for entry in &payload.evolution.unresolved {
        let candidates = unique_set(&entry.candidates, "unresolved candidate")?;
        if let Some(source) = candidates.iter().find(|source| !sources.contains(source)) {
            return Err(invalid(format!(
                "unresolved candidate {source} is not an input face"
            )));
        }
        claim_result(entry.result, "unresolved", &results, &mut claimed_results)?;
    }

    if accounted_sources != sources {
        let mut missing: Vec<u32> = sources.difference(&accounted_sources).copied().collect();
        missing.sort_unstable();
        return Err(invalid(format!(
            "source faces {missing:?} are neither modified nor deleted"
        )));
    }
    if claimed_results != results {
        let mut missing: Vec<u32> = results.difference(&claimed_results).copied().collect();
        missing.sort_unstable();
        return Err(invalid(format!(
            "result faces {missing:?} have no evolution claim"
        )));
    }

    if let Some(topo) = topo {
        let solid = topo
            .solid_id_from_index(payload.solid as usize)
            .ok_or_else(|| invalid(format!("solid handle {} does not exist", payload.solid)))?;
        let actual_results: HashSet<u32> = solid_faces(topo, solid)?
            .into_iter()
            .map(|face| to_handle(face.index(), "live result"))
            .collect::<Result<_, _>>()?;
        if actual_results != results {
            return Err(invalid(
                "resultFaces do not exactly match the faces of the final solid",
            ));
        }
        for &source in &sources {
            if topo.face_id_from_index(source as usize).is_none() {
                return Err(invalid(format!(
                    "source face handle {source} does not exist"
                )));
            }
        }
    }

    Ok(())
}

fn claim_result(
    result: u32,
    claim: &str,
    result_faces: &HashSet<u32>,
    claimed: &mut HashSet<u32>,
) -> Result<(), WasmError> {
    if !result_faces.contains(&result) {
        return Err(invalid(format!(
            "{claim} result {result} does not belong to the final solid"
        )));
    }
    if !claimed.insert(result) {
        return Err(invalid(format!(
            "result face {result} has duplicate or contradictory claims"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use brepkit_operations::boolean::collect_face_signatures;
    use brepkit_operations::{blend_ops, primitives};
    use brepkit_topology::Topology;
    use brepkit_topology::explorer::solid_edges;

    use super::*;

    fn valid_box_fillet() -> (Topology, TopologyEvolutionResultV1) {
        let mut topo = Topology::new();
        let solid = primitives::make_box(&mut topo, 10.0, 10.0, 10.0).unwrap();
        let edges = solid_edges(&topo, solid).unwrap();
        let input = collect_face_signatures(&topo, solid).unwrap();
        let (result, map) =
            blend_ops::fillet_with_evolution(&mut topo, solid, &[edges[0]], 1.0).unwrap();
        let payload = build_payload_v1(&topo, result.solid, &input, &map).unwrap();
        (topo, payload)
    }

    #[test]
    fn valid_payload_round_trips_through_the_strict_decoder() {
        let (topo, payload) = valid_box_fillet();
        let json = serde_json::to_string(&payload).unwrap();
        assert_eq!(decode_payload_v1(&topo, &json).unwrap(), payload);
    }

    #[test]
    fn decoder_rejects_malformed_incomplete_and_contradictory_payloads() {
        let (topo, payload) = valid_box_fillet();

        assert!(decode_payload_v1(&topo, "{not json}").is_err());

        let mut incomplete = payload.clone();
        incomplete.evolution.modified.pop();
        assert!(decode_payload_v1(&topo, &serde_json::to_string(&incomplete).unwrap()).is_err());

        let mut contradictory = payload.clone();
        contradictory
            .evolution
            .deleted
            .push(contradictory.evolution.modified[0].source);
        assert!(decode_payload_v1(&topo, &serde_json::to_string(&contradictory).unwrap()).is_err());

        let mut duplicate = payload;
        duplicate
            .evolution
            .generated
            .push(duplicate.evolution.generated[0].clone());
        assert!(decode_payload_v1(&topo, &serde_json::to_string(&duplicate).unwrap()).is_err());
    }
}
