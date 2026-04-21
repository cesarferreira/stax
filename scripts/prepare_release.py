#!/usr/bin/env python3

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from collections import OrderedDict
from datetime import date
from pathlib import Path

VERSION_TAG_RE = re.compile(r"^v\d+\.\d+\.\d+$")
CONVENTIONAL_RE = re.compile(r"^(?P<type>[a-z]+)(\([^)]+\))?(?P<breaking>!)?: (?P<desc>.+)$")
SKIP_SUBJECT_RE = re.compile(r"^chore: Release stax version \d+\.\d+\.\d+$")
UNRELEASED_BLOCK_RE = re.compile(
    r"(?ms)^## \[Unreleased\] - ReleaseDate\s*\n.*?(?=^## \[|^<!-- next-url -->|\Z)"
)
UNRELEASED_LINK_RE = re.compile(
    r"(?m)^\[Unreleased\]: (?P<compare_prefix>.+?/compare/)v\d+\.\d+\.\d+\.\.\.HEAD$"
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
    parser.add_argument(
        "--release-version",
        help="Finalize the changelog for this released version instead of only refreshing Unreleased notes.",
    )
    parser.add_argument(
        "--previous-version",
        help="Previous released version used to update compare links when finalizing a release.",
    )
    parser.add_argument(
        "--release-date",
        help="Release date to stamp into the finalized changelog entry (defaults to today).",
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


def rewrite_release_section(
    changelog: str,
    release_notes: str,
    release_version: str,
    release_date: str,
) -> str:
    replacement = (
        "## [Unreleased] - ReleaseDate\n\n"
        f"## [{release_version}] - {release_date}\n\n"
        f"{release_notes}\n\n"
    )
    updated, count = UNRELEASED_BLOCK_RE.subn(replacement, changelog, count=1)
    if count != 1:
        raise ReleasePrepError("Failed to locate the Unreleased section in CHANGELOG.md")
    return updated


def rewrite_release_links(changelog: str, previous_version: str, release_version: str) -> str:
    unreleased_match = UNRELEASED_LINK_RE.search(changelog)
    if not unreleased_match:
        raise ReleasePrepError("Failed to locate the Unreleased compare link in CHANGELOG.md")

    compare_prefix = unreleased_match.group("compare_prefix")
    updated = UNRELEASED_LINK_RE.sub(
        f"[Unreleased]: {compare_prefix}v{release_version}...HEAD",
        changelog,
        count=1,
    )

    previous_release_link_re = re.compile(rf"(?m)^\[{re.escape(previous_version)}\]: .*$")
    new_release_link = (
        f"[{release_version}]: {compare_prefix}v{previous_version}...v{release_version}"
    )
    updated, count = previous_release_link_re.subn(
        lambda match: f"{new_release_link}\n{match.group(0)}",
        updated,
        count=1,
    )
    if count != 1:
        raise ReleasePrepError(
            f"Failed to locate the compare link for previous version {previous_version}"
        )

    return updated


def resolve_release_context(args: argparse.Namespace) -> tuple[str, str, str] | None:
    release_version = args.release_version or os.environ.get("NEW_VERSION")
    previous_version = args.previous_version or os.environ.get("PREV_VERSION")
    release_date = args.release_date or os.environ.get("RELEASE_DATE") or date.today().isoformat()

    if not release_version and not previous_version and not args.release_date:
        return None

    if not release_version or not previous_version:
        raise ReleasePrepError(
            "Release finalization requires both --release-version and --previous-version "
            "(or NEW_VERSION and PREV_VERSION)."
        )

    return release_version, previous_version, release_date


def is_dry_run() -> bool:
    return os.environ.get("DRY_RUN", "").strip().lower() == "true"


def main() -> int:
    args = parse_args()
    repo = Path(args.repo).resolve()
    changelog_path = Path(args.changelog).resolve() if args.changelog else repo / "CHANGELOG.md"

    try:
        release_context = resolve_release_context(args)
        if release_context and is_dry_run():
            print(f"Dry run: leaving {changelog_path} unchanged")
            return 0

        tag = latest_version_tag(repo)
        subjects = commits_since_tag(repo, tag)
        release_notes = build_release_notes(subjects)
        changelog = changelog_path.read_text(encoding="utf-8")

        if release_context:
            release_version, previous_version, release_date = release_context
            updated = rewrite_release_section(
                changelog,
                release_notes,
                release_version,
                release_date,
            )
            updated = rewrite_release_links(updated, previous_version, release_version)
        else:
            updated = rewrite_unreleased_section(changelog, release_notes)

        if updated != changelog:
            changelog_path.write_text(updated, encoding="utf-8")
    except ReleasePrepError as exc:
        print(str(exc), file=sys.stderr)
        return 1

    if release_context:
        print(f"Finalized {changelog_path} for v{release_context[0]} using {len(subjects)} commits since {tag}")
    else:
        print(f"Updated {changelog_path} using {len(subjects)} commits since {tag}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
