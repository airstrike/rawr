#!/usr/bin/env python3
"""Materialize verified reference source; never fetch or execute Cargo/fixtures."""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import tarfile
import tomllib

HERE = Path(__file__).resolve().parent
PIN = json.loads((HERE / "provenance.json").read_text())


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify_source(destination):
    cargo = destination / "cargo"
    for name, expected in PIN["cargo"]["members"].items():
        if digest(cargo / name) != expected:
            raise ValueError(f"upstream member mismatch: {name}")
    schema = tomllib.loads((cargo / "crates/cargo-util-schemas/Cargo.toml").read_text())
    if schema["package"]["version"] != "0.14.2":
        raise ValueError("wrong cargo-util-schemas version")


def prepare(archive, destination):
    if digest(archive) != PIN["cargo"]["archive_sha256"]:
        raise ValueError("Cargo source archive SHA-256 mismatch")
    destination.mkdir(parents=True, exist_ok=False)
    cargo = destination / "cargo"
    cargo.mkdir()
    prefix = "cargo-" + PIN["cargo"]["commit"]
    links = []
    with tarfile.open(archive, "r:gz") as source:
        for member in source:
            relative = PurePosixPath(member.name)
            if relative.is_absolute() or ".." in relative.parts or relative.parts[0] != prefix:
                raise ValueError("unexpected Cargo archive member")
            target = cargo.joinpath(*relative.parts[1:])
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                target.parent.mkdir(parents=True, exist_ok=True)
                with source.extractfile(member) as src, target.open("xb") as dst:
                    shutil.copyfileobj(src, dst)
                target.chmod(member.mode & 0o777)
            elif member.issym():
                links.append((target, member.linkname))
            else:
                raise ValueError(f"unsupported archive member kind: {member.name}")
    for target, link in links:
        if Path(link).is_absolute() or not (target.parent / link).resolve().is_relative_to(cargo.resolve()):
            raise ValueError("Cargo archive link escapes owned source tree")
        target.parent.mkdir(parents=True, exist_ok=True)
        target.symlink_to(link)
    verify_source(destination)
    for name in ("src", "fixtures"):
        shutil.copytree(HERE / name, destination / name)
    for name in ("Cargo.toml", "provenance.json", "LICENSE-MIT", "LICENSE-APACHE"):
        shutil.copyfile(HERE / name, destination / name)
    # A root-reviewed isolated lock, when present, wins. Until first resolution,
    # preserve Cargo's exact lock choices rather than generating from scratch.
    lock = HERE / "Cargo.lock"
    shutil.copyfile(lock if lock.exists() else cargo / "Cargo.lock", destination / "Cargo.lock")
    (destination / ".reference-source").write_text(PIN["cargo"]["commit"] + "\n")
    print(json.dumps({"prepared": str(destination), "source_verified": True,
                      "lock": "isolated" if lock.exists() else "pinned-Cargo seed"}))


def verify_lock(destination):
    verify_source(destination)
    pinned = tomllib.loads((destination / "cargo/Cargo.lock").read_text())["package"]
    known = {(p["name"], p["version"], p.get("source")): p.get("checksum") for p in pinned}
    resolved = tomllib.loads((destination / "Cargo.lock").read_text())["package"]
    failures = []
    for package in resolved:
        key = (package["name"], package["version"], package.get("source"))
        if package.get("source"):
            if key not in known or known[key] != package.get("checksum"):
                failures.append(key)
        elif (package["name"], package["version"]) not in {
            ("rawr-manifest-reference", "0.0.0"), ("cargo-util-schemas", "0.14.2")
        }:
            failures.append(key)
    for name, expected in PIN["required_packages"].items():
        actual = [p for p in resolved if p["name"] == name]
        if len(actual) != 1 or actual[0]["version"] != expected["version"] or actual[0].get("checksum") != expected["checksum"]:
            failures.append((name, "required exact singleton missing or mismatched", None))
    if failures:
        raise ValueError(f"isolated lock is not pinned: {failures}")
    print(json.dumps({"verified": True, "packages": len(resolved),
                      "lock_sha256": digest(destination / "Cargo.lock")}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    setup = commands.add_parser("prepare")
    setup.add_argument("--archive", required=True, type=Path)
    setup.add_argument("--destination", required=True, type=Path)
    check = commands.add_parser("verify-lock")
    check.add_argument("--destination", required=True, type=Path)
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.archive.resolve(), args.destination.resolve())
    else:
        verify_lock(args.destination.resolve())


if __name__ == "__main__":
    main()
