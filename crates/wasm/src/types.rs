//! Typed result structs for structured WASM returns.
//!
//! Types annotated with `Tsify` automatically generate TypeScript definitions
//! and can be serialized via `serde-wasm-bindgen` for zero-copy JS interop.

use tsify::Tsify;

/// Typed result for `tessellateSolidGrouped`.
#[derive(serde::Serialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(into_wasm_abi)]
pub struct GroupedMeshResult {
    pub positions: Vec<f64>,
    pub normals: Vec<f64>,
    pub indices: Vec<u32>,
    pub face_offsets: Vec<u32>,
}

/// Typed result for `tessellateSolidUV`.
#[derive(serde::Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct UvMeshResult {
    pub positions: Vec<f64>,
    pub normals: Vec<f64>,
    pub indices: Vec<u32>,
    pub uvs: Vec<f64>,
}

/// Typed result for `boundingBox`.
#[derive(serde::Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct BoundingBoxResult {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

/// Typed result for boolean operations with evolution tracking.
#[derive(serde::Serialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(into_wasm_abi)]
pub struct EvolutionResult {
    pub solid: u32,
    pub generated: Vec<u32>,
    pub modified: Vec<u32>,
}

/// Typed result for `sketchSolve`.
#[derive(serde::Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct SketchSolveResult {
    pub converged: bool,
    pub points: Vec<f64>,
    pub residual: f64,
}

/// Per-step entry in a `HealPipelineResult`.
#[derive(Debug, serde::Serialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(into_wasm_abi)]
pub struct HealStepResult {
    /// Operator name that ran.
    pub step: String,
    /// Number of individual repair actions taken.
    pub actions_taken: u32,
    /// At least one fix was applied.
    pub done: bool,
    /// At least one fix could not be applied.
    pub failed: bool,
}

/// Typed result for `fixShapeWithConfig`.
#[derive(Debug, serde::Serialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(into_wasm_abi)]
pub struct HealFixResult {
    /// Handle of the healed solid (may differ from the input).
    pub solid: u32,
    /// Number of individual repair actions taken.
    pub actions_taken: u32,
    /// At least one fix was applied.
    pub done: bool,
    /// At least one fix could not be applied.
    pub failed: bool,
}

/// Typed result for `runHealPipeline`.
#[derive(Debug, serde::Serialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(into_wasm_abi)]
pub struct HealPipelineResult {
    /// Handle of the healed solid (may differ from the input).
    pub solid: u32,
    /// One entry per executed step, in order.
    pub steps: Vec<HealStepResult>,
}
