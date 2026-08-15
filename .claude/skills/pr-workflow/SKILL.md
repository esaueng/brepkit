---
name: pr-workflow
description: Use when committing, pushing, opening, or merging a pull request in brepkit, or when a git hook fails, a push hangs or is rejected, commitlint flags a message, a PR seems to be waiting on an AI review, CI checks need reading before a merge, or parallel work needs a worktree. Covers hooks, conventional commits, the self-enforced CI merge gate (this fork has no branch protection and no AI reviewer), pushing to the esaueng fork, and release-please.
---

# PR Workflow

End-to-end change flow for this repo: branch, commit, push, PR, CI gate, squash-merge, release. Every change lands as a squash-merged PR.

**Nothing is enforced server-side.** `origin` is the `esaueng/brepkit` fork, and on it `main` is NOT a protected branch, auto-merge is disabled, and no AI reviewer runs. Verify before trusting any claim to the contrary:

```bash
gh api repos/esaueng/brepkit/branches/main/protection   # 404 "Branch not protected"
gh api repos/esaueng/brepkit/rules/branches/main        # [] — rulesets are SEPARATE
gh api repos/esaueng/brepkit --jq '.allow_auto_merge'   # false
```

Check the second one too. Rulesets are a distinct system from legacy branch protection and do not appear in the `protection` endpoint, so a 404 there alone does not prove a branch is unguarded.

The consequences are the whole reason this file exists: `CI Pass` is not a required check, a direct push to `main` will NOT be rejected, and `--auto` will error. The discipline below is policy, held by you, with no backstop.

## Quick reference

| Task | Command |
|------|---------|
| Branch | `git checkout -b <type>/<kebab-description>` (e.g. `feat/render-lod`, `fix/ci-crates-io-flake`) |
| Local gate before push | `cargo nextest run -p <touched-crate>` and, if any `Cargo.toml` changed, `./scripts/check-boundaries.sh` |
| Compliance grep | See "Banned-name compliance" below |
| Push | `git push -u origin <branch>` (plain push works; see "Push details" in reference.md if it ever hangs) |
| CI poll | `gh pr checks <N>` until nothing is `pending` |
| Merge | `gh pr merge <N> --squash` (only after CI is green — `--auto` is unavailable) |
| Post-merge | `git checkout main && git pull --ff-only && git push origin --delete <branch>` |
| Worktree | `git worktree add .worktrees/<branch-name> <branch>` |

## Hooks: what actually runs

Hooks live in `.husky/`. Read the hook files themselves when in doubt; the "Git Conventions" section of CLAUDE.md describes an older pre-push behavior and the hook file is authoritative.

- `pre-commit`: fmt, clippy, taplo, and cargo-machete run in parallel. No tests. Expect `✅ Pre-commit checks passed.` If it fails, fix and re-commit. Caveat: the hook silently skips taplo and cargo-machete when the binaries are not installed (`command -v` guards in `.husky/pre-commit`), so a passing hook does not prove TOML formatting or unused-dep cleanliness. Install both with `cargo install taplo-cli cargo-machete`.
- `commit-msg`: runs commitlint (`@commitlint/config-conventional`, `commitlint.config.js`) but always exits 0: its not-installed fallback also swallows real lint failures, so violations print `✖` lines without blocking the commit. Treat any `✖` output as a hard failure and `git commit --amend` the message. Shape: `type(scope): subject`, e.g. `feat(render): screen-space adaptive LOD`. Nothing in CI lints messages either, and release-please parses the squash-commit title (the PR title), so a malformed title silently skips the version bump.
- `pre-push`: prints one info line and exits 0. Validation is deliberately delegated to CI (`.github/workflows/ci.yml`). Do not re-add local test runs to this hook, and do not treat its emptiness as a reason to skip local testing: run touched-crate tests yourself before pushing.

Hard rules:
- Never `--no-verify`, never `HUSKY=0`, never edit a hook to get past it. If a hook fails on pre-existing breakage you did not cause, stop and report; do not bypass.
- If a hook fails with `cargo: command not found`, that is PATH, not breakage. The hooks run a bare shell; export the toolchain path in the same command as the commit rather than bypassing the hook.
- CI is the real gate: fmt, clippy, test (nextest workspace + doc tests, plus macOS and Windows matrix legs), coverage, MSRV, wasm, boundaries, deny, audit, docs, machete, taplo, fuzz, render, secrets, and publish dry-run, all fanned into one aggregate check named `CI Pass`. It aggregates but does not gate — branch protection is off, so GitHub will let you merge a red PR. Read it yourself. Job catalog: see [reference.md](reference.md), "CI jobs".

## Procedure: land a change

1. **Branch** off `main` as `<type>/<kebab-description>`. Never commit to `main` directly.
2. **Develop and test locally.** `cargo nextest run -p <crate>` for touched crates; `./scripts/check-boundaries.sh` if you touched any `Cargo.toml` (this check only runs in CI, catch it early).
3. **Commit.** Conventional message. Never commit plan/spec working documents: if `git status` shows untracked planning docs (ad-hoc `*-plan.md` or `*-spec.md` working documents), leave them untracked.
4. **Compliance grep** (below). Expect zero output.
5. **Push.** `git push -u origin <branch>` works: `origin` is an HTTPS URL and there is no `insteadOf` rewrite configured. (Both were once true and are the reason older notes reach for a token-embedded URL — that fallback still works and is kept in reference.md, but do not reach for it first.) Checkpoint: `gh pr view <N> --json headRefOid` matches `git rev-parse HEAD`.
6. **Create the PR** with `gh pr create`, as a normal ready-for-review PR. Never `--draft`.
7. **CI gate** (next section).
8. **After merge:** `git checkout main && git pull --ff-only`. `delete_branch_on_merge` is false on this fork, so delete the remote branch yourself: `git push origin --delete <branch>`.

## The CI gate

**No AI reviewer runs on this fork.** There is no `cubic · AI code reviewer` check, no Copilot reviewer, and no human approval requirement (`required_approving_review_count` is moot — `main` has no protection at all). Verified across PRs #224–#245: zero inline review comments and zero reviews, the sole exception being `github-actions[bot]` posting the WASM size report.

Do not wait for a review that will never arrive. `CI Pass` is the entire gate, and you enforce it:

1. After `gh pr create`, work on the next independent task while CI runs (roughly 10 to 20 minutes for the full fan-out).
2. Poll until nothing is pending:
   ```bash
   gh pr checks <N>                                   # human-readable
   gh pr checks <N> --json name,bucket --jq '[.[] | select(.bucket=="fail")] | length'
   ```
   `CI Pass` does not appear in the rollup at all until every job it aggregates has finished, so its absence early on is not a failure.
3. Investigate every non-green check. `SemVer Check (advisory)` is deliberately excluded from `CI Pass`; a red one is informational. `[code]smith` reporting `skipping` is a vendor promo, not a check. For supply-chain jobs failing on a diff that never touched dependencies, see reference.md, "CI failures you did not cause".
4. Only then: `gh pr merge <N> --squash`. Auto-merge is disabled repo-wide, so `--auto` errors out — you merge when you have seen green, which means you must actually look.
5. If the merge is refused with `the head branch is not up to date with the base branch`, the PR is `BEHIND`: main moved since you branched. This is a `gh` client-side refusal, not branch protection (there is none) — do not reach for `--admin`. Update and re-merge:
   ```bash
   gh pr view <N> --json mergeStateStatus --jq .mergeStateStatus   # BEHIND
   git rebase origin/main && git push --force-with-lease origin <branch>
   ```
   That restarts CI, so you wait and read it again before merging. `gh pr update-branch <N>` does the same server-side with a merge commit.
6. This applies to every PR including high-risk core changes (GFA boolean engine, public WASM API). Nothing and nobody else will catch a bad one.

Because the safety net is missing, the burden shifts left: run the change-specific gates from CLAUDE.md locally before pushing, rather than discovering it in CI or, worse, after merge.

Anti-patterns:
- Do NOT trust a filtered check poll before listing the rollup unfiltered once. A poll keyed to a name that does not exist on this fork (`cubic · AI code reviewer` is exactly such a name) stays silent forever and reads identically to "all clear".
- Do NOT read `mergeStateStatus: CLEAN` as "CI passed". With no required checks, a PR is mergeable while CI is still running or outright red.
- Do NOT merge on a green `Test` alone; the matrix legs (`Test (macos-latest)`, `Test (windows-latest)`) are separate checks that land later.

## Stacked PRs

Basing PR B on PR A's branch works, but landing them needs two manual steps that a protected repo would handle for you. Both bite because `delete_branch_on_merge` is false here.

After A squash-merges, its branch commit never becomes an ancestor of `main` — main gets one NEW squashed commit instead. So B is left based on an orphaned commit, and because A's branch still exists, GitHub does NOT retarget B:

```bash
# 1. drop the now-duplicated commit; replay only B's own work onto main
git checkout main && git pull --ff-only
git checkout <branch-B> && git rebase --onto main <A-head-sha>
git push --force-with-lease origin <branch-B>

# 2. retarget B, which GitHub will not do while A's branch survives
gh pr edit <N-B> --base main
```

Verify both landed: `gh pr view <N-B> --json baseRefName,headRefOid` should show `main` and your new local HEAD, and `gh pr diff <N-B> --name-only` should list only B's files. If it still lists A's files, the rebase did not take and merging would re-apply them.

## Banned-name compliance

The names of the reference kernel (the incumbent C++ CAD kernel) must not appear in changed files, commit messages, or PR titles/bodies. Run before pushing:

```bash
# pattern split so this file itself stays clean
banned='oc''ct|open''cascade'
git diff main... --name-only | xargs -r rg -n -i "$banned"
git log main.. --format='%s%n%b' | rg -n -i "$banned"
```

Pass condition: no output (rg exits 1). Grandfathered files that legitimately contain the names: `README.md`, `CHANGELOG.md`, `crates/wasm/CHANGELOG.md`, `scripts/bench-compare.sh`, `scripts/bench-report.ts`, `scripts/parity-loop.sh`. Do not add new occurrences anywhere, and do not "clean up" the grandfathered ones. Reading the reference kernel's source locally to study an approach is fine; naming it in committed text is not. For benchmark instructions, point at the brepjs harness scripts by path instead (see the parity-benchmarking skill).

## Worktrees

Parallel work lives inside the repo under `.worktrees/` (gitignored):

```bash
git worktree add .worktrees/<branch-name> <branch>
```

Ignore the older `../feat-branch` sibling-directory form in CLAUDE.md; in-repo `.worktrees/` is the rule. Each worktree pushes and PRs independently with the same procedure above.

## Release flow

release-please (`.github/workflows/publish.yml`) maintains a pending `chore(main): release X.Y.Z` PR. Merging a `feat`/`fix`/`perf` PR updates it; merging the release PR itself tags, creates the GitHub release, and publishes the wasm package to npm. `docs`/`chore` commits and changes under excluded paths (`.github`, `scripts`, `benches`, `examples`, and similar) do not bump the version. Cross-repo consumption by brepjs: see the release-flow skill. Details and manual escape hatch: [reference.md](reference.md), "Release-please".

## CI failures you did not cause

`Cargo.lock` is gitignored, so deny, audit, and MSRV re-resolve dependencies on every CI run; a new advisory or dep release can fail an unrelated PR with zero diff. Never widen `deny.toml` to get green. Triage order, the MSRV and wasm-bindgen pins, and scheduled workflows: see [reference.md](reference.md), "CI failures you did not cause".

## Symptoms

Symptom-to-cause table (push hangs, commitlint rejects, review check missing, release PR not updating): see [reference.md](reference.md), "Symptom table".
