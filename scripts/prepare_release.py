#!/usr/bin/env python3

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from collections import OrderedDict
from pathlib import Path

VERSION_TAG_RE = re.compile(r"^v\d+\.\d+\.\d+$")
CONVENTIONAL_RE = re.compile(r"^(?P<type>[a-z]+)(\([^)]+\))?(?P<breaking>!)?: (?P<desc>.+)$")
SKIP_SUBJECT_RE = re.compile(r"^chore: Release stax version \d+\.\d+\.\d+$")
UNRELEASED_BLOCK_RE = re.compile(
    r"(?ms)^## \[Unreleased\] - ReleaseDate\s*\n.*?(?=^## \[|^<!-- next-url -->|\Z)"
)

CATEGORY_ORDER = ["Added", "Changed", "Fixed", "Documentation"]


class ReleasePrepError(Exception):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate Keep a Changelog notes under the Unreleased section."
    )
    parser.add_argument(
        "--repo",
        default=".",
        help="Repository root to inspect. Defaults to the current directory.",
    )
    parser.add_argument(
        "--changelog",
        help="Changelog path to rewrite. Defaults to <repo>/CHANGELOG.md.",
    )
    return parser.parse_args()


def run_git(repo: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=repo,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise ReleasePrepError(result.stderr.strip() or f"git {' '.join(args)} failed")
    return result.stdout


def latest_version_tag(repo: Path) -> str:
    output = run_git(repo, "tag", "--list", "v*", "--sort=-creatordate")
    for line in output.splitlines():
        tag = line.strip()
        if VERSION_TAG_RE.match(tag):
            return tag
    raise ReleasePrepError("No version tags found matching v<major>.<minor>.<patch>")


def commits_since_tag(repo: Path, tag: str) -> list[str]:
    output = run_git(repo, "log", "--reverse", "--no-merges", "--format=%s", f"{tag}..HEAD")
    subjects = []
    for line in output.splitlines():
        subject = line.strip()
        if not subject or SKIP_SUBJECT_RE.match(subject):
            continue
        subjects.append(subject)
    if not subjects:
        raise ReleasePrepError("No commits found since last tag")
    return subjects


def categorize_subject(subject: str) -> tuple[str, str]:
    match = CONVENTIONAL_RE.match(subject)
    commit_type = None
    description = subject
    if match:
        commit_type = match.group("type").lower()
        description = match.group("desc").strip()

    if commit_type in {"feat", "feature", "add"}:
        category = "Added"
    elif commit_type in {"fix", "bugfix"}:
        category = "Fixed"
    elif commit_type in {"docs", "doc"}:
        category = "Documentation"
    else:
        category = "Changed"

    if description and description[0].islower():
        description = description[0].upper() + description[1:]

    return category, description


def build_release_notes(subjects: list[str]) -> str:
    categorized: OrderedDict[str, list[str]] = OrderedDict(
        (category, []) for category in CATEGORY_ORDER
    )

    for subject in subjects:
        category, description = categorize_subject(subject)
        categorized[category].append(description)

    lines: list[str] = []
    for category, entries in categorized.items():
        if not entries:
            continue
        lines.append(f"### {category}")
        for entry in entries:
            lines.append(f"- {entry}")
        lines.append("")

    return "\n".join(lines).rstrip()


def rewrite_unreleased_section(changelog: str, release_notes: str) -> str:
    replacement = f"## [Unreleased] - ReleaseDate\n\n{release_notes}\n\n"
    updated, count = UNRELEASED_BLOCK_RE.subn(replacement, changelog, count=1)
    if count != 1:
        raise ReleasePrepError("Failed to locate the Unreleased section in CHANGELOG.md")
    return updated


def main() -> int:
    args = parse_args()
    repo = Path(args.repo).resolve()
    changelog_path = Path(args.changelog).resolve() if args.changelog else repo / "CHANGELOG.md"

    try:
        tag = latest_version_tag(repo)
        subjects = commits_since_tag(repo, tag)
        release_notes = build_release_notes(subjects)
        changelog = changelog_path.read_text(encoding="utf-8")
        updated = rewrite_unreleased_section(changelog, release_notes)
        changelog_path.write_text(updated, encoding="utf-8")
    except ReleasePrepError as exc:
        print(str(exc), file=sys.stderr)
        return 1

    print(f"Updated {changelog_path} using {len(subjects)} commits since {tag}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
