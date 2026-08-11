//! Recognition of endpoint-connected full-turn rim chains.

use std::collections::HashMap;
use std::f64::consts::TAU;

use brepkit_math::vec::Point3;
use brepkit_topology::Topology;
use brepkit_topology::vertex::VertexId;

/// One endpoint-connected curved-edge cycle that winds a periodic parameter.
pub struct RimCycle {
    /// Raw topology indices of the edges in traversal order.
    pub edge_indices: Vec<usize>,
    /// Whether the cycle contains a by-construction closed edge.
    pub has_closed_edge: bool,
}

/// Collect curved-edge cycles whose projected parameter winds one full turn.
///
/// `curved` contains unique topology edge indices and their stored endpoints.
/// The recognizer walks each connected run by endpoint identity, rejects open
/// runs or an unexpected cycle count, and then requires every non-closed cycle
/// to accumulate a wrapped winding of approximately `2*pi`.
pub fn collect_full_turn_rim_cycles(
    topo: &Topology,
    curved: &[(usize, VertexId, VertexId)],
    project_u: &dyn Fn(Point3) -> f64,
    expected_cycles: usize,
) -> Result<Option<Vec<RimCycle>>, crate::OperationsError> {
    let mut by_vertex: HashMap<VertexId, Vec<usize>> = HashMap::new();
    for (position, &(_, start, end)) in curved.iter().enumerate() {
        by_vertex.entry(start).or_default().push(position);
        by_vertex.entry(end).or_default().push(position);
    }

    let mut used = vec![false; curved.len()];
    let mut cycles = Vec::new();
    for start_position in 0..curved.len() {
        if used[start_position] {
            continue;
        }

        let (_, origin, mut at) = curved[start_position];
        used[start_position] = true;
        let mut positions = vec![start_position];
        let mut closed = curved[start_position].1 == curved[start_position].2 || at == origin;
        while !closed {
            let Some(&next) = by_vertex
                .get(&at)
                .and_then(|candidates| candidates.iter().find(|&&position| !used[position]))
            else {
                break;
            };
            used[next] = true;
            at = if curved[next].1 == at {
                curved[next].2
            } else {
                curved[next].1
            };
            positions.push(next);
            closed = at == origin;
        }
        if !closed {
            return Ok(None);
        }

        let mut winding = 0.0_f64;
        let mut traversal_vertex: Option<VertexId> = None;
        let mut has_closed_edge = false;
        for &position in &positions {
            let (_, start, end) = curved[position];
            if start == end {
                has_closed_edge = true;
                continue;
            }
            let (from, to) = match traversal_vertex {
                None => (start, end),
                Some(vertex) if vertex == start => (start, end),
                Some(_) => (end, start),
            };
            let u0 = project_u(topo.vertex(from)?.point());
            let u1 = project_u(topo.vertex(to)?.point());
            winding += wrap_pi(u1 - u0);
            traversal_vertex = Some(to);
        }
        if !has_closed_edge && (winding.abs() - TAU).abs() > 1e-6 {
            return Ok(None);
        }

        cycles.push(RimCycle {
            edge_indices: positions
                .into_iter()
                .map(|position| curved[position].0)
                .collect(),
            has_closed_edge,
        });
    }

    if cycles.len() != expected_cycles {
        return Ok(None);
    }
    Ok(Some(cycles))
}

fn wrap_pi(delta: f64) -> f64 {
    (delta + TAU / 2.0).rem_euclid(TAU) - TAU / 2.0
}
