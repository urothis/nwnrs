#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from collections import defaultdict
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
ROOT_CARGO_TOML = ROOT / "Cargo.toml"
SEMVER_TAG_PATTERN = re.compile(r"^v([0-9]+)\.([0-9]+)\.([0-9]+)$")


def run_command(*args: str) -> str:
    result = subprocess.run(
        list(args),
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def read_workspace_version() -> str:
    content = ROOT_CARGO_TOML.read_text(encoding="utf-8")
    match = re.search(
        r"(?ms)^\[workspace\.package\]\n.*?^version = \"([0-9]+\.[0-9]+\.[0-9]+)\"$",
        content,
    )
    if match is None:
        raise SystemExit("failed to locate [workspace.package] version in Cargo.toml")
    return match.group(1)


def bump_version(version: str, level: str) -> str:
    major, minor, patch = (int(part) for part in version.split("."))
    if level == "major":
        return f"{major + 1}.0.0"
    if level == "minor":
        return f"{major}.{minor + 1}.0"
    if level == "patch":
        return f"{major}.{minor}.{patch + 1}"
    raise ValueError(f"unsupported bump level: {level}")


def latest_tag() -> str | None:
    tags = run_command("git", "tag", "--list", "v*").splitlines()

    parsed_tags: list[tuple[tuple[int, int, int], str]] = []
    for tag in tags:
        match = SEMVER_TAG_PATTERN.fullmatch(tag)
        if match is None:
            continue
        parsed_tags.append(
            (
                (int(match.group(1)), int(match.group(2)), int(match.group(3))),
                tag,
            )
        )

    if not parsed_tags:
        return None

    _, tag = max(parsed_tags)
    return tag


def latest_version() -> str:
    tag = latest_tag()
    if tag is None:
        return read_workspace_version()
    return tag.removeprefix("v")


def update_root_manifest(new_version: str) -> None:
    content = ROOT_CARGO_TOML.read_text(encoding="utf-8")
    current_version = read_workspace_version()

    content, workspace_replacements = re.subn(
        r"(?ms)^(\[workspace\.package\]\n.*?^version = \")([0-9]+\.[0-9]+\.[0-9]+)(\"$)",
        rf"\g<1>{new_version}\g<3>",
        content,
        count=1,
    )
    if workspace_replacements != 1:
        raise SystemExit("failed to update [workspace.package] version")

    dependency_pattern = re.compile(
        r'^(?P<prefix>[A-Za-z0-9_-]+\s*=\s*\{[^}\n]*\bpath\s*=\s*"[^"]+"[^}\n]*\bversion\s*=\s*")'
        + re.escape(current_version)
        + r'(?P<suffix>"[^}\n]*\})$'
    )

    in_workspace_dependencies = False
    updated_lines: list[str] = []
    dependency_replacements = 0

    for line in content.splitlines():
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            in_workspace_dependencies = stripped == "[workspace.dependencies]"
            updated_lines.append(line)
            continue

        if in_workspace_dependencies:
            replaced_line, count = dependency_pattern.subn(
                rf"\g<prefix>{new_version}\g<suffix>",
                line,
            )
            if count:
                dependency_replacements += count
                updated_lines.append(replaced_line)
                continue

        updated_lines.append(line)

    if dependency_replacements == 0:
        raise SystemExit("failed to update internal workspace dependency versions")

    ROOT_CARGO_TOML.write_text("\n".join(updated_lines) + "\n", encoding="utf-8")


def cargo_metadata() -> dict:
    result = run_command(
        "cargo",
        "metadata",
        "--format-version",
        "1",
        "--no-deps",
    )
    return json.loads(result)


def publishable_packages(metadata: dict) -> list[dict]:
    workspace_members = set(metadata["workspace_members"])
    packages = []
    for package in metadata["packages"]:
        if package["id"] not in workspace_members:
            continue
        publish = package.get("publish")
        if publish == []:
            continue
        packages.append(package)
    return packages


def publish_order() -> list[str]:
    metadata = cargo_metadata()
    packages = publishable_packages(metadata)
    package_by_name = {package["name"]: package for package in packages}

    adjacency: dict[str, set[str]] = defaultdict(set)
    indegree = {package["name"]: 0 for package in packages}

    for package in packages:
        package_name = package["name"]
        internal_dependencies = set()
        for dependency in package["dependencies"]:
            if dependency["kind"] not in (None, "build"):
                continue
            dependency_name = dependency["name"]
            if dependency_name in package_by_name:
                internal_dependencies.add(dependency_name)

        for dependency_name in internal_dependencies:
            adjacency[dependency_name].add(package_name)
            indegree[package_name] += 1

    ready = sorted(name for name, degree in indegree.items() if degree == 0)
    ordered: list[str] = []

    while ready:
        name = ready.pop(0)
        ordered.append(name)
        for dependent in sorted(adjacency[name]):
            indegree[dependent] -= 1
            if indegree[dependent] == 0:
                ready.append(dependent)
                ready.sort()

    if len(ordered) != len(packages):
        unresolved = sorted(name for name, degree in indegree.items() if degree > 0)
        raise SystemExit(
            "failed to compute publish order; cycle detected among publishable crates: "
            + ", ".join(unresolved)
        )

    return ordered


def crates_io_has_version(crate_name: str, version: str) -> bool:
    url = (
        "https://crates.io/api/v1/crates/"
        + urllib.parse.quote(crate_name, safe="")
        + "/"
        + urllib.parse.quote(version, safe="")
    )

    try:
        with urllib.request.urlopen(url, timeout=20) as response:
            payload = json.load(response)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return False
        raise

    return payload.get("version", {}).get("num") == version


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Workspace release helpers")
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("latest-tag")
    subparsers.add_parser("latest-version")
    subparsers.add_parser("version")

    bump_parser = subparsers.add_parser("bump")
    bump_parser.add_argument("level", choices=["patch", "minor", "major"])

    next_version_parser = subparsers.add_parser("next-version")
    next_version_parser.add_argument("level", choices=["patch", "minor", "major"])

    order_parser = subparsers.add_parser("publish-order")
    order_parser.add_argument("--format", choices=["names", "json"], default="names")

    published_parser = subparsers.add_parser("is-published")
    published_parser.add_argument("crate_name")
    published_parser.add_argument("version")

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    if args.command == "latest-tag":
        tag = latest_tag()
        if tag is not None:
            print(tag)
        return 0

    if args.command == "latest-version":
        print(latest_version())
        return 0

    if args.command == "version":
        print(read_workspace_version())
        return 0

    if args.command == "bump":
        current_version = read_workspace_version()
        new_version = bump_version(current_version, args.level)
        update_root_manifest(new_version)
        print(new_version)
        return 0

    if args.command == "next-version":
        current_version = latest_version()
        new_version = bump_version(current_version, args.level)
        update_root_manifest(new_version)
        print(new_version)
        return 0

    if args.command == "publish-order":
        ordered = publish_order()
        if args.format == "json":
            print(json.dumps(ordered))
        else:
            print("\n".join(ordered))
        return 0

    if args.command == "is-published":
        return 0 if crates_io_has_version(args.crate_name, args.version) else 1

    parser.error(f"unknown command: {args.command}")
    return 2


if __name__ == "__main__":
    sys.exit(main())
