//! NURBS curve and surface representations.
//!
//! Non-Uniform Rational B-Spline (NURBS) geometry is the standard
//! representation for free-form curves and surfaces in CAD.

pub mod basis;
pub mod bezier_clip;
pub mod curve;
pub mod decompose;
pub mod evaluator;
pub mod fitting;
pub mod intersection;
pub mod knot_ops;
pub mod power_basis;
pub mod projection;
pub mod self_intersection;
pub mod surface;
pub mod surface_fitting;

pub use bezier_clip::{
    CurveCurveHit, CurveCurveOverlap, CurveCurveResult, curve_curve_intersect,
    curve_curve_intersect_full,
};
pub use curve::NurbsCurve;
pub use decompose::{
    BezierPatch, curve_degree_elevate, curve_to_bezier_segments, surface_to_bezier_patches,
};
pub use evaluator::SurfaceEvaluator;
pub use fitting::{approximate, interpolate};
pub use knot_ops::{
    curve_knot_insert, curve_knot_refine, curve_knot_remove, curve_split, surface_knot_insert_u,
    surface_knot_insert_v,
};
pub use power_basis::PowerBasis1D;
pub use projection::{
    CurveProjection, SurfaceProjection, SurfaceSeedGrid, project_point_to_curve,
    project_point_to_surface, project_point_to_surface_seeded, project_point_to_surface_with_grid,
};
pub use surface::NurbsSurface;
pub use surface_fitting::interpolate_surface;

fn validate_knot_values(knots: &[f64]) -> Result<(), crate::MathError> {
    for (index, &value) in knots.iter().enumerate() {
        if !value.is_finite() {
            return Err(crate::MathError::InvalidKnotValue { index, value });
        }
        // A wobble of a few ulps between adjacent knots is representation
        // noise, not a malformed vector: `NurbsCurve::reversed` mirrors knots
        // and clamps the endpoints back, which can leave the last interior
        // knot one ulp above the clamped end. Only a decrease larger than
        // that scale is rejected.
        let prev = knots[index.saturating_sub(1)];
        let wobble = 8.0 * f64::EPSILON * prev.abs().max(value.abs()).max(1.0);
        if index > 0 && value < prev - wobble {
            return Err(crate::MathError::InvalidKnotValue { index, value });
        }
    }
    Ok(())
}
