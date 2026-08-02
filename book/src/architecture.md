# Architecture

Remus is the computational engine behind brepjs. While brepjs provides the
TypeScript API that developers interact with, Remus handles the underlying
B-Rep modeling: geometry evaluation, boolean operations, tessellation, and
data exchange.

Remus uses a strict layered architecture. Each layer may only depend on
layers below it, never above or sideways.

```
┌─────────────────────────────────────┐
│  L4: remus-wasm                   │  WASM bindings (JS API)
├─────────────────┬───────────────────┤
│  L3: operations │  L3: io/render    │  Modeling, exchange, rendering
├─────────────────┴───────────────────┤
│  L2: algo/blend/check/...           │  Geometry algorithms
├─────────────────────────────────────┤
│  L1: topology / geometry            │  B-Rep and geometry ownership
├─────────────────────────────────────┤
│  L0: math                           │  Vectors, NURBS, predicates
└─────────────────────────────────────┘
```

## Layer Rules

| Crate | Layer | Allowed Dependencies |
|-------|-------|---------------------|
| `remus-math` | L0 | External crates only |
| `remus-topology`, `remus-geometry` | L1 | `remus-math` |
| `remus-algo`, `remus-blend`, `remus-check`, `remus-heal`, `remus-offset` | L2 | Lower layers only |
| `remus-operations`, `remus-io`, `remus-render` | L3 | Lower layers only |
| `remus-wasm` | L4 | All workspace crates except render |

These rules are enforced by `scripts/check-boundaries.sh`.

`remus-render` is an optional native wgpu consumer. It supports offscreen
RGBA rendering and face-ID picking; it is not included in the browser WASM
package.

## Arena-Based Topology

All topological entities (vertices, edges, faces, etc.) are stored in a
central `Arena` and referenced by typed index handles. This approach:

- Avoids reference counting overhead (`Rc`/`Arc`)
- Enables cache-friendly traversal (data locality)
- Makes ownership clear (the arena owns everything)
- Provides O(1) entity lookup

## NURBS-Native Geometry

Geometric entities (curves, surfaces) use NURBS as the native
representation. This means:

- Exact representation of conics (circles, ellipses) via rational NURBS
- Uniform algorithms for evaluation, subdivision, and intersection
- No special-casing for different curve/surface types
