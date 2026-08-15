# pr-workflow reference

Deep catalog behind SKILL.md. Everything here was verified against the repo; when it disagrees with prose elsewhere (including CLAUDE.md's "Git Conventions"), the workflow files (`.husky/*`, `.github/workflows/*.yml`) are authoritative.

## CI jobs (`.github/workflows/ci.yml`)

Jobs fan into `ci-pass` (display name `CI Pass`), with `wasm-size` and `semver` deliberately excluded. `CI Pass` is an aggregate signal only — branch protection is off, so no check is *required*; see "Repo merge settings".

| Job | Display name | What it runs |
|-----|--------------|--------------|
| `fmt` | Format | `cargo fmt --all -- --check` |
| `clippy` | Clippy | `cargo clippy --all-targets --all-features -- -D warnings` |
| `test` | Test | `cargo nextest run --workspace`, doc tests, complexity guards |
| `platform-test` | Test (macos-latest), Test (windows-latest) | the same suite on the OS matrix; lands later than `Test` |
| `coverage` | Coverage | llvm-cov coverage report (60% line floor) |
| `msrv` | MSRV (1.88) | build on the minimum supported Rust |
| `wasm` | WASM Build & Validate | `cargo xtask wasm-build --skip-opt` (wasm-pack build + validation for `wasm32-unknown-unknown`) |
| `wasm-size` | WASM Size Report | PR-only size delta comment (informational, NOT in `ci-pass`) |
| `fuzz-check` | Fuzz Targets Compile | fuzz targets build |
| `render` | Software Rendering | software Vulkan render checks |
| `boundaries` | Layer Boundaries | `./scripts/check-boundaries.sh` |
| `doc-paths` | Doc Paths | `./scripts/check-doc-paths.sh` |
| `deny` | Cargo Deny | license/advisory/ban checks |
| `audit` | Security Audit | `cargo audit` |
| `docs` | Documentation | doc build with warnings denied |
| `publish-dry-run` | Publish Dry Run | `cargo publish --workspace --dry-run` against crates.io |
| `semver` | SemVer Check (advisory) | advisory only, NOT in `ci-pass` — a red one does not block |
| `machete` | Unused Dependencies | `cargo machete` |
| `secrets-scan` | Secrets Scan | secret scanning |
| `taplo` | TOML Format | `taplo fmt --check` |

Two non-CI entries also appear in the rollup: `cla` (a CLA bot) and `[code]smith`, a Blacksmith autofix advertisement that reports `skipping` and is not a check at all.

Regenerate this table rather than trusting it:

```bash
awk '/^  [a-z0-9_-]+:$/{j=$1} /^    name:/{print j" "$0}' .github/workflows/ci.yml
```

Local pre-commit covers only fmt, clippy, taplo, machete, and the last two only when the binaries are installed (the hook skips them silently otherwise). Everything else (tests, boundaries, deny, docs) first runs in CI unless you run it yourself. Before pushing, run at minimum the tests for touched crates and, on any `Cargo.toml` change, `./scripts/check-boundaries.sh`.

## Repo merge settings (verified via `gh api repos/esaueng/brepkit`, 2026-08-14)

`origin` is `https://github.com/esaueng/brepkit` — the production fork. Some older notes name `andymai/brepkit`; that is the upstream project, it is not `origin`, and it is not where your PRs go. It is also not configured as a remote by default (`git remote -v` shows `origin` only), so the fork-maintenance commands in CLAUDE.md need `upstream` added first.

- `allow_squash_merge: true`. Merge commits and rebase merges are ALSO enabled (`allow_merge_commit`, `allow_rebase_merge`) — squash-only is a convention here, not a setting.
- `allow_auto_merge: false` — `gh pr merge --auto` errors out. Merge after you have read CI yourself.
- `delete_branch_on_merge: false` — remote branches survive a merge. Clean up with `git push origin --delete <branch>`.
- **`main` has no branch protection and no rulesets**: `branches/main/protection` returns 404 `Branch not protected`, AND `rules/branches/main` returns `[]`. Both matter — rulesets are a separate system that the `protection` endpoint does not report, so a lone 404 proves nothing. No required checks, no linear-history requirement, no force-push block, no required approvals. A direct push to `main` succeeds. "Never commit to main" is policy with zero technical enforcement.
- Squash commit titles on `main` look like `type(scope): subject (#N)`.

One gate does still exist, and it is client-side: `gh pr merge` refuses a PR whose `mergeStateStatus` is `BEHIND` with `the head branch is not up to date with the base branch`, then suggests `--auto` (unavailable here) or `--admin`. Neither is the answer — rebase onto `origin/main`, force-push, wait for the fresh CI, and merge. See SKILL.md, "The CI gate", step 5.

Re-verify rather than trusting this table; settings drift:

```bash
gh api repos/esaueng/brepkit --jq '"squash=\(.allow_squash_merge) auto=\(.allow_auto_merge) delete=\(.delete_branch_on_merge)"'
gh api repos/esaueng/brepkit/branches/main/protection --jq '.required_status_checks.contexts'
gh api repos/esaueng/brepkit/rules/branches/main
```

## AI reviewers: none on this fork

There are none. No `cubic`, no `cubic-dev-ai`, no `copilot-pull-request-reviewer`, and no `cubic · AI code reviewer` status check. Checked across PRs #224–#245: every one has zero inline comments and zero reviews, except a single `github-actions[bot]` review; the only recurring PR comment is the automated WASM size report.

This is the single most dangerous stale belief about this repo, because the failure is silent. Polling for a check name that does not exist returns nothing forever, which is indistinguishable from a clean review. Guard against it by listing the rollup unfiltered before filtering:

```bash
gh pr view <N> --json statusCheckRollup --jq '.statusCheckRollup[] | (.name // .context)'
```

If a reviewer is ever installed on the fork, both comment surfaces need checking, since inline findings do not appear in `gh pr view --comments`:

```bash
gh api repos/esaueng/brepkit/pulls/<N>/comments   # inline (diff-anchored) comments
gh api repos/esaueng/brepkit/issues/<N>/comments  # issue-level comments
```

## Push details

Plain `git push -u origin <branch>` works. `origin` is an HTTPS URL and `git config --get-regexp 'url\..*insteadof'` returns nothing, so tracking refs update normally and `git rev-parse origin/<branch>` is trustworthy.

Two conditions once made this fail, and the token-embedded fallback below exists for them: an SSH `origin` (port 22 is blocked in this sandbox) and a global `insteadOf` rewrite converting `https://github.com/...` back to SSH. Neither is configured now. If a push ever hangs with no output, re-check both before reaching for the workaround:

```bash
git remote -v
git config --get-regexp 'url\..*insteadof'

# fallback only if one of the above is the problem
git push "https://x-access-token:$(gh auth token)@github.com/esaueng/brepkit.git" <branch> \
  2>&1 | sed 's/x-access-token:[^@]*@/x-access-token:***@/g'
```

An explicit-URL push does not update local `origin/<branch>`, so if you use the fallback, verify the remote head directly:

```bash
gh pr view <N> --json headRefOid --jq .headRefOid
gh api repos/esaueng/brepkit/commits/<branch> --jq .sha
```

- All `gh` operations (create, view, merge, api) go over HTTPS with the CLI token and work normally.

## Release-please (`.github/workflows/publish.yml`)

- Runs on every push to `main` using `googleapis/release-please-action` v5 with a bot app token.
- Config: `release-please-config.json`; current version manifest: `.release-please-manifest.json`. Single package rooted at `.`, component `brepkit-wasm`; the version is also bumped in `crates/wasm/Cargo.toml`.
- Flow: merging a `feat`/`fix`/`perf` PR creates or updates the pending release PR (`chore(main): release X.Y.Z`, head branch `release-please--branches--main--components--brepkit-wasm`). Merging that release PR creates the tag and GitHub release and publishes to npm.
- Version-neutral changes: `docs` and `chore` commits are changelog-hidden; changes only under excluded paths (`.github`, `book`, `scripts`, `benches`, `bench-results`, `examples`, `bindings`) do not bump.
- Manual escape hatch: `workflow_dispatch` on the Publish workflow with a `publish_version` input skips release-please.
- Cross-repo: brepjs (`~/Git/brepjs`) consumes the published wasm package; see the release-flow skill for the two-repo runbook.

## CI failures you did not cause

Supply-chain and toolchain jobs can fail on a PR that never touched dependencies. Root cause: `Cargo.lock` is gitignored (see `.gitignore`), so every CI run resolves dependencies fresh. The `audit` job even runs `cargo generate-lockfile` explicitly. A new advisory or a new dep release changes the verdict with zero diff on your branch.

### The `cla` check (currently red on every PR)

`.github/workflows/cla.yml` runs `contributor-assistant/github-action` in its own workflow run, so it is NOT part of `ci-pass` and does not affect `CI Pass`. On this fork it fails for every contributor:

```
Error occurred when creating the signed contributors file:
  Branch cla-signatures not found.
Committers of pull request <N> have to sign the CLA
```

It is inherited from upstream and never adapted: `path-to-document` still points at `https://github.com/andymai/brepkit/...`, the allowlist is `andymai,web-flow,...`, and the `cla-signatures` branch the action writes to does not exist on the fork. Confirmed red on #240, #241, #243, #245 concurrently.

Treat it as pre-existing infrastructure breakage, not a signal about your diff. Do not try to satisfy it by rewriting commit authorship. Fixing it properly means either creating the `cla-signatures` branch and repointing the config at the fork, or dropping the workflow — its own change, and the repo owner's call.

### cargo-deny / audit / OSV advisories

The `deny` job runs cargo-deny-action against `deny.toml`; the `audit` job runs rustsec/audit-check on a freshly generated lockfile. Both gate PRs through `CI Pass`. OSV lives in its own workflow (`.github/workflows/osv-scan.yml`): on PRs it is report-only and does not gate; pushes to `main` and the Monday 06:00 UTC schedule fail closed.

Policy: do NOT widen the `deny.toml` license allowlist and do NOT add blanket ignore entries to get green. Current state to preserve: `licenses.allow` has eight permissive entries, the four non-obvious ones (CC0-1.0, ISC, BSD-2-Clause, BSD-3-Clause) carrying a provenance comment that names the dependency tree pulling them in, and there is no `[advisories] ignore` list at all (`[advisories]` sets only `unmaintained` and `yanked`).

Triage order when an advisory lands on your PR:

1. Check whether a patched version exists (`cargo audit` output or the advisory page names it). Semver-compatible patches are picked up automatically by the fresh resolution, so a persistent failure usually means the fix is in a new major or minor. Raise the version requirement in the relevant `Cargo.toml` in a separate commit, not mixed into your feature diff.
2. If no patched version exists, add a narrowly scoped `[advisories] ignore` entry to `deny.toml`: the specific advisory id, a comment explaining why it does not apply or cannot be fixed yet, and a concrete re-check trigger (e.g. "remove when <crate> X.Y ships"). State the ignore and its rationale in the PR body.
3. If unsure whether the advisory is exploitable or how to scope it, stop and report per the blocked-state rule. Silencing a security check is never a way to unblock a PR.

### MSRV job

Workspace `Cargo.toml` declares `rust-version = "1.88"`; the CI job `MSRV (1.88)` runs `cargo check --workspace --all-features` on toolchain 1.88.0. Because resolution is fresh, a dependency releasing a version that needs newer Rust breaks this job on unrelated PRs. The errors are confusing: syntax, edition, or feature errors deep inside the dependency, never the word MSRV. Fix: constrain that dependency in `Cargo.toml` to the last version that builds on 1.88. Do not bump `rust-version` casually; it is a public contract, and raising it is its own PR with its own justification.

### wasm-bindgen pin

The workspace `Cargo.toml` pins `wasm-bindgen = "=0.2.125"` and says why in an inline comment: the crate version must match the wasm-bindgen-cli tooling. The coupling is enforced in `xtask/src/wasm.rs` via the `WASM_BINDGEN_VERSION` constant; `cargo xtask wasm-build` bails when a locally installed wasm-bindgen-cli differs. The two locations must move together, and the xtask constant has lagged the Cargo.toml pin before, so verify both:

```bash
rg -n 'wasm-bindgen' Cargo.toml xtask/src/wasm.rs
```

Bumping wasm-bindgen is its own change with its own PR. Never bump it as a drive-by to fix an unrelated failure, and never let a general `cargo update`-style version bump move it silently.

### Scheduled workflows

- `.github/workflows/mutants.yml` (Mutation Testing): Sundays 02:00 UTC plus manual dispatch. Runs cargo-mutants on `brepkit-math` and `brepkit-algo` and uploads a report artifact. It never runs on PRs and never gates them; a red scheduled run is a signal to improve tests, not a merge blocker.
- `.github/workflows/osv-scan.yml`: Mondays 06:00 UTC plus main pushes, fail-closed there; report-only on PRs (see above).
- `benchmark.yml` runs on pushes and PRs, not on a schedule, and is not part of `CI Pass`.

## Symptom table

| Symptom | Cause | Action |
|---------|-------|--------|
| `git push` hangs, no output | An SSH `origin` plus blocked port 22, or an `insteadOf` rewrite. Neither is configured now | Check `git remote -v` and `git config --get-regexp 'url\..*insteadof'` before using the token-URL fallback |
| `origin/<branch>` does not match what you pushed | You used the explicit-URL fallback; those pushes never update tracking refs | Verify with `gh pr view <N> --json headRefOid` |
| Hook fails with `cargo: command not found` | Hooks run a bare shell without the toolchain on PATH | Export the toolchain path in the same command as the commit. Never `--no-verify` |
| `gh pr merge --auto` errors | `allow_auto_merge` is false on this fork | Read CI yourself, then plain `gh pr merge <N> --squash` |
| Remote branch still present after merge | `delete_branch_on_merge` is false | `git push origin --delete <branch>` |
| A commit landed directly on `main` | `main` is NOT protected on this fork; nothing rejects it | Policy-only rule. Branch first; there is no backstop |
| commitlint prints `✖ found N problems` yet the commit succeeded | The commit-msg hook swallows commitlint failures and exits 0 by design of its fallback | Treat as a rejection: `git commit --amend` to `type(scope): subject` form |
| pre-commit fails on clippy warnings you did not write | Pre-existing breakage on the branch base | Stop and report; never `--no-verify` |
| `⚠️ commitlint not available` warning on commit | `node_modules` missing, but only if no `✖` lines print above it; the same warning also follows a real lint failure (see the `✖` row) because the hook's fallback fires on any nonzero commitlint exit | If `✖` lines precede it, fix the message; otherwise `npm install` at repo root. Either way the commit went through unchecked, re-verify the message manually |
| PR shows mergeable while CI is red or still running | `main` has no required checks, so `mergeStateStatus` reflects nothing about CI | Read `gh pr checks <N>` yourself before merging |
| AI review check never appears | No AI reviewer runs on this fork. It is not late; it does not exist | Stop waiting. `CI Pass` is the gate. See "AI reviewers: none on this fork" |
| `CI Pass` missing from the rollup | It only appears once every job it aggregates has finished | Not a failure; keep polling |
| `cla` check red | Inherited upstream workflow; `cla-signatures` branch missing, config points at upstream. Red on every PR | Pre-existing, outside `ci-pass`. See "The `cla` check" |
| CI `boundaries` job fails | A crate dependency violates the layer rules | Run `./scripts/check-boundaries.sh` locally; see the layer-boundaries skill |
| CI `taplo` or `machete` fails but pre-commit passed | Tool not installed locally; the hook skips it silently | `cargo install taplo-cli cargo-machete`, fix, re-commit |
| Compliance grep hits in a file you touched | You introduced a banned reference-kernel name, or you touched a grandfathered file | Remove new occurrences; leave grandfathered ones as-is |
| Release PR did not update after merge | Commit type was `docs`/`chore`, or all changes fell under excluded paths | Expected; only `feat`/`fix`/`perf` in versioned paths bump |
| `deny` or `audit` fails on a PR that never touched deps | `Cargo.lock` is gitignored; CI resolved a newly-advisoried or newly-released dep | Follow the triage order in "CI failures you did not cause"; never blanket-ignore |
| MSRV job fails with syntax or feature errors inside a dependency | A dep released a version requiring Rust newer than 1.88 | Constrain that dep in `Cargo.toml`; do not bump `rust-version` |
| `cargo xtask wasm-build` bails with a wasm-bindgen-cli version mismatch | Local CLI differs from the pin; the crate pin and `xtask/src/wasm.rs` constant must match | Install the pinned CLI version; bump the pin only as its own PR |
| Push rejected on `main` | Branch protection; direct pushes to main are not allowed | Branch and open a PR |

## Anti-patterns (what NOT to conclude)

- "The AI reviewer had no findings": no AI reviewer runs here. Silence from a filtered check poll is the absence of the check, not the absence of findings.
- "The PR is mergeable, so it passed": nothing is a required check on this fork. Mergeable means only that there are no conflicts.
- "The pre-push hook printed one line and passed, so the change is validated": the hook intentionally runs nothing. Validation is CI plus the local tests you ran yourself.
- "CLAUDE.md says pre-push runs tests and cargo-deny": stale. The hook file delegates to CI; do not re-add local suites to it and do not cite the stale description.
- "High-risk change, better wait for a human": no human gate and no bot gate exist. You are the only reviewer, so read your own diff before merging.
- "Branch protection will stop me doing something stupid": it will not. `main` is unprotected.
- "The plan doc helps reviewers, commit it": working plans and specs never get committed.
- "The commit went through, so the message passed commitlint": the commit-msg hook never blocks. Check the hook output for `✖` lines and amend if any appeared.
