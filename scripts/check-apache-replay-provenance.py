#!/usr/bin/env python3
"""Validate the checked-in Apache replay provenance ledger."""

from __future__ import annotations

import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
LEDGER_PATH = ROOT / "docs/production-readiness/apache-replay-provenance.json"
EXPECTED_PRS = {
    145, 149, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 175, 177,
    178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191,
    192, 193, 194, 195, 198, 200, 201, 202, 203, 204, 209, 210, 211, 212,
    215, 216, 218, 220, 221, 222, 223, 224, 225, 226, 227, 228, 229, 230,
    233, 234, 235, 236, 237, 238, 239, 240, 241, 242, 243, 244, 245, 247,
}
EXPECTED_EXCLUSIONS = {206, 207, 219, 246}
EXPECTED_AUTHOR = "petergstfsn"
EXPECTED_SOURCE_RECORDS_SHA256 = (
    "438fe61088187e5dfe10a5f3599e44085ff9142aa2b0508fdeea12f9db8ffa6c"
)
EXPECTED_REPLAY = {
    "parent_commit": "6b0ff51aae89de89accb576177f33ab701f3583d",
    "parent_tree": "4c4650e0aa43cc3443c8d6eddcf53b5031198d13",
    "first_commit": "ea8099ca7b26ad6987d92e7e368ca0560eda0380",
    "last_commit": "42dc503dafef990a60727c3835480efd2740d552",
}
SHA_PATTERN = re.compile(r"^[0-9a-f]{40}$")


def fail(message: str) -> None:
    print(f"Apache replay provenance violation: {message}", file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def git(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )


def load_ledger() -> dict[str, Any]:
    try:
        value = json.loads(LEDGER_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {LEDGER_PATH.relative_to(ROOT)}: {error}")
    require(isinstance(value, dict), "ledger root must be an object")
    return value


def validate_static(ledger: dict[str, Any]) -> tuple[set[str], dict[str, str]]:
    require(ledger.get("schema_version") == 1, "unsupported schema version")
    replay = ledger.get("replay")
    require(isinstance(replay, dict), "missing replay object")
    require(replay.get("commit_count") == 73, "replay commit count must be 73")
    require(
        replay.get("source_pull_request_count") == 70,
        "source pull-request count must be 70",
    )
    require(replay.get("source_author") == EXPECTED_AUTHOR, "wrong source author")
    for field, expected in EXPECTED_REPLAY.items():
        require(replay.get(field) == expected, f"replay {field} changed")

    allowed_authors = replay.get("allowed_replay_authors")
    require(isinstance(allowed_authors, list), "missing replay author allowlist")
    allowed_emails = {
        entry.get("email")
        for entry in allowed_authors
        if isinstance(entry, dict) and entry.get("name") == "Peter"
    }
    require(
        allowed_emails
        == {
            "171875562+petergustafson@users.noreply.github.com",
            "171875562+petergstfsn@users.noreply.github.com",
        },
        "replay author allowlist changed",
    )

    records = ledger.get("source_pull_requests")
    require(isinstance(records, list), "missing source pull-request records")
    require(len(records) == 70, "source pull-request records must be unique")
    source_payload = json.dumps(
        records, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode()
    require(
        hashlib.sha256(source_payload).hexdigest() == EXPECTED_SOURCE_RECORDS_SHA256,
        "pinned source pull-request records changed",
    )
    numbers = {
        record.get("number") for record in records if isinstance(record, dict)
    }
    require(numbers == EXPECTED_PRS, "source pull-request set changed")

    mapped_commits: set[str] = set()
    recorded_subjects: dict[str, str] = {}
    shared_commits: dict[str, set[int]] = {}
    for record in records:
        require(isinstance(record, dict), "pull-request record must be an object")
        number = record["number"]
        require(
            record.get("url")
            == f"https://github.com/esaueng/brepkit/pull/{number}",
            f"PR #{number} URL changed",
        )
        require(record.get("author") == EXPECTED_AUTHOR, f"PR #{number} author changed")
        require(
            SHA_PATTERN.fullmatch(str(record.get("audited_head_sha"))) is not None,
            f"PR #{number} has an invalid audited head",
        )
        commits = record.get("replay_commits")
        require(isinstance(commits, list) and commits, f"PR #{number} is unmapped")
        for commit in commits:
            require(
                isinstance(commit, str) and SHA_PATTERN.fullmatch(commit) is not None,
                f"PR #{number} has an invalid replay commit",
            )
            mapped_commits.add(commit)
            shared_commits.setdefault(commit, set()).add(number)

    expected_shared = {
        "c08bcbfb137a888df8688468d2428680a503d1ce": {185, 210},
    }
    actual_shared = {
        commit: prs for commit, prs in shared_commits.items() if len(prs) > 1
    }
    require(actual_shared == expected_shared, "shared replay mapping changed")

    independent = ledger.get("independent_replay_commits")
    require(
        isinstance(independent, list) and len(independent) == 1,
        "expected one independent replay commit",
    )
    independent_record = independent[0]
    require(isinstance(independent_record, dict), "invalid independent commit record")
    independent_sha = independent_record.get("sha")
    require(
        independent_sha == "42dc503dafef990a60727c3835480efd2740d552",
        "independent replay commit changed",
    )
    recorded_subjects[independent_sha] = str(independent_record.get("subject"))
    mapped_commits.add(independent_sha)
    require(len(mapped_commits) == 73, "mapped replay commit union must contain 73 SHAs")

    exclusions = ledger.get("deliberately_excluded")
    require(isinstance(exclusions, list), "missing exclusion records")
    excluded_numbers = {
        number
        for record in exclusions
        if isinstance(record, dict)
        for number in record.get("source_pull_requests", [])
    }
    require(excluded_numbers == EXPECTED_EXCLUSIONS, "excluded PR set changed")
    require(not (numbers & excluded_numbers), "an excluded PR is mapped")

    adaptations = ledger.get("adaptations")
    require(isinstance(adaptations, list), "missing adaptation records")
    adaptation_sets = {
        tuple(record.get("source_pull_requests", []))
        for record in adaptations
        if isinstance(record, dict)
    }
    require(
        adaptation_sets == {(243,), (229,), (218,), (185, 210), (224,)},
        "adaptation set changed",
    )

    return mapped_commits, recorded_subjects


def validate_available_history(
    ledger: dict[str, Any], mapped_commits: set[str], recorded_subjects: dict[str, str]
) -> None:
    available = {
        commit for commit in mapped_commits if git("cat-file", "-e", f"{commit}^{{commit}}").returncode == 0
    }
    if not available:
        print("Replay commits are not present; static provenance ledger verified.")
        return
    require(
        available == mapped_commits,
        "only part of the replay commit set is available in Git",
    )

    replay = ledger["replay"]
    allowed = {
        (entry["name"], entry["email"]) for entry in replay["allowed_replay_authors"]
    }
    for commit in sorted(mapped_commits):
        result = git("show", "-s", "--format=%an%x09%ae%x09%s", commit)
        require(result.returncode == 0, f"cannot inspect replay commit {commit}")
        author_name, author_email, subject = result.stdout.rstrip("\n").split("\t", 2)
        require(
            (author_name, author_email) in allowed,
            f"unexpected author on replay commit {commit}",
        )
        if commit in recorded_subjects:
            require(
                subject == recorded_subjects[commit],
                f"independent commit subject changed for {commit}",
            )

    parent = replay["parent_commit"]
    last = replay["last_commit"]
    count_result = git("rev-list", "--count", f"{parent}..{last}")
    require(count_result.returncode == 0, "cannot count replay history")
    require(count_result.stdout.strip() == "73", "Git replay range must contain 73 commits")
    tree_result = git("rev-parse", f"{parent}^{{tree}}")
    require(tree_result.returncode == 0, "cannot inspect replay parent tree")
    require(
        tree_result.stdout.strip() == replay["parent_tree"],
        "replay parent tree does not match the recorded PR #252 tree",
    )
    ancestry = git("merge-base", "--is-ancestor", replay["first_commit"], last)
    require(ancestry.returncode == 0, "first replay commit is not an ancestor of last")
    print("Static ledger and all 73 replay commits verified.")


def main() -> None:
    ledger = load_ledger()
    mapped_commits, recorded_subjects = validate_static(ledger)
    validate_available_history(ledger, mapped_commits, recorded_subjects)


if __name__ == "__main__":
    main()
