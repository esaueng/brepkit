# Apache replay provenance

This is the engineering provenance record for the second Apache-only
contribution replay. It is not legal advice. The machine-readable source of
truth is [`apache-replay-provenance.json`](apache-replay-provenance.json), and
`scripts/check-apache-replay-provenance.py` validates it offline.

## Recorded scope

- Replay parent: `6b0ff51aae89de89accb576177f33ab701f3583d`
- Parent tree: `4c4650e0aa43cc3443c8d6eddcf53b5031198d13`
- Replay range: `ea8099ca7b26ad6987d92e7e368ca0560eda0380` through
  `42dc503dafef990a60727c3835480efd2740d552`
- Local replay commits: 73
- Source pull requests: 70
- Source pull-request author: `petergstfsn`
- Replay commit author: Peter, using the two recorded GitHub noreply addresses

Each source pull request is pinned to the exact head SHA inspected on
2026-08-14. This matters for PRs #236, #237, #238, #244, and #247, which were
still open at the time of the audit.

## Adaptations

The replay preserves contribution deltas rather than importing post-license
branch ancestry. Where the Apache staging architecture differed, the delta was
ported manually and verified against the staging tree.

- PR #243 required a follow-up tessellation adaptation after the ordered edge
  map port.
- PR #229 was split into a clean source subset and the remaining compatible
  fixes.
- PR #218 has a separate lockfile refresh.
- PRs #185 and #210 were combined into one Apache-safe package-refresh
  workflow.
- PR #224 contributes its regressions; its implementation was superseded.
- `42dc503d` is independent fork-authored CI hardening that binds generated
  WASM to the triggering source commit.

PRs #206 and #207 were not applicable because their target blend architecture
does not exist on the Apache staging tree. PR #219 is release-only metadata.
PR #246 hard-codes repository-specific BrepKit workflow settings and is to be
regenerated after the standalone Remus repository exists.

## Verification behavior

The checker always validates the ledger schema, exact PR set, pinned source
heads, authors, counts, mappings, exceptions, and exclusions. When all 73
individual replay commits are present in the checkout, it additionally verifies
their recorded authors and subjects from Git. That history check is expected on
the replay PR; after a squash merge or a fresh shallow clone, the structured
ledger remains the durable evidence.
