# WASM topology evolution v1

This contract was characterized against fork revision
`65e3840c221b20b3d8fd64ca45513d5687c868d6`
(`v2.129.0-138-g65e3840c`). The package version remains `2.129.0`; this work
does not publish or release a package.

## Production operation contract

`filletWithEvolution(solid, edges, radius)` runs the same production cascade
used before this contract was typed: validate the radius and handles, snapshot
the input faces, run the v2 walking builder first, then the rolling-ball and
legacy fillet fallbacks, accept only a closed result, and restore the input
topology after every rejected attempt. The result receives no extra healing,
unification, or topology post-processing for provenance. Evolution is built
only after the successful engine and its existing acceptance checks finish, so
every result handle is checked against the final solid returned to JavaScript.
The same whole-selection guard as `fillet` rejects a result that would cover
only a subset of the requested edges.

`chamferWithEvolution(solid, edges, distance)` is the equivalent entry point
for the existing chamfer cascade. It keeps the production order unchanged:
the planar chamfer runs first and the v2 walking builder is the fallback. It
uses the same validation, rollback, tolerances, exact solid, and failure path as
`chamfer`.

The operation-layer history has these meanings:

- `modified`: an input face and the final face or faces that preserve its
  identity. A consumer may rebind a persistent selection through this claim.
- `generated`: a new final face and every input face the builder says
  participated in constructing it. A fillet band or chamfer bevel normally
  names both faces separated by the selected edge; this is provenance, not an
  identity claim.
- `deleted`: an input face with no identity-preserving face in the final solid.
- `unresolved`: a final face whose source could not be established, with any
  tied input candidates. Persistent-reference consumers must fail closed on
  these entries.
- `origin`: `construction` only when the successful builder recorded the
  correspondence while assembling the solid. `geometry` identifies the
  existing fallback matcher used by engines that rebuild faces without a
  construction record.

No construction provenance is inferred from proximity, traversal position, or
approximate matching. Geometry-matched fallback results remain explicitly
labeled `geometry`.

## Typed payload

Both blend entry points return a `TopologyEvolutionResultV1` JavaScript object:

```ts
interface TopologyEvolutionResultV1 {
  version: number; // v1 requires exactly 1 at runtime
  solid: number;
  sourceFaces: number[];
  resultFaces: number[];
  evolution: {
    modified: Array<{ source: number; results: number[] }>;
    generated: Array<{ sources: number[]; result: number }>;
    deleted: number[];
    unresolved: Array<{ result: number; candidates: number[] }>;
    origin: 'construction' | 'geometry';
  };
}
```

The old `filletWithEvolution` value was a JSON string with an untyped `any`
declaration. Version 1 intentionally replaces that runtime-only value with the
generated TypeScript interface above. The version is part of the data; a v1
decoder rejects every other value.

Before return, the kernel proves all of the following as set invariants:

- `sourceFaces` is exactly the pre-operation face snapshot, with no duplicate.
- `resultFaces` is exactly the faces belonging to `solid`, with no duplicate.
- every source is claimed once by `modified` or `deleted`;
- every result is claimed once by `modified`, `generated`, or `unresolved`;
- every source/result named in a claim belongs to the appropriate coverage set;
- modified sources and result claims are neither duplicate nor contradictory;
- a generated result may have multiple distinct source faces, but the result
  itself is claimed once;
- deleted inputs are explicit; and
- every handle resolves in the live kernel and `resultFaces` still equals the
  final solid when a persisted payload is decoded.

`decodeEvolutionPayload(json)` is the strict decoder for persisted or
transported v1 JSON. It rejects malformed JSON, unknown fields, unsupported
versions, invalid handles, incomplete coverage, duplicate claims, contradictory
claims, and result sets that do not match the live final solid.

## Compatibility and geometry

This is a WASM return-format change for `filletWithEvolution`, plus the new
`chamferWithEvolution` and `decodeEvolutionPayload` methods. It does not change
the `fillet`, `chamfer`, boolean, STEP, or topology formats. Evolution entry
points call the same geometry engines as their non-evolution counterparts and
do not alter tolerances, post-processing, validation, or failure semantics to
obtain provenance.
