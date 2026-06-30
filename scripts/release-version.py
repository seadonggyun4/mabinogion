#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ROOT_CARGO = ROOT / "Cargo.toml"

INTERNAL_DEPENDENCY_CRATES = [
    "mabi-core",
    "mabi-runtime",
    "mabi-modbus",
    "mabi-opcua",
    "mabi-bacnet",
    "mabi-knx",
    "mabi-scenario",
    "mabi-chaos",
]

WORKSPACE_CRATES = INTERNAL_DEPENDENCY_CRATES + ["mabi-cli"]

BEGIN_MARKER = "# BEGIN generated-internal-release-mirror (sync via scripts/release-version.py)"
END_MARKER = "# END generated-internal-release-mirror"

SURFACE_RULES = {
    ROOT / "crates/mabi-core/src/factory.rs": {
        "required": ["RELEASE_VERSION"],
        "forbidden": ['"1.0.0"'],
    },
    ROOT / "crates/mabi-core/src/version.rs": {
        "required": ['pub const RELEASE_VERSION: &str = env!("CARGO_PKG_VERSION");'],
        "forbidden": [],
    },
    ROOT / "crates/mabi-opcua/src/lib.rs": {
        "required": ["pub const VERSION: &str = mabi_core::RELEASE_VERSION;"],
        "forbidden": ['env!("CARGO_PKG_VERSION")'],
    },
    ROOT / "crates/mabi-opcua/src/factory.rs": {
        "required": ["RELEASE_VERSION.to_string()", "fn version(&self) -> &str {\n        RELEASE_VERSION"],
        "forbidden": ['env!("CARGO_PKG_VERSION")'],
    },
    ROOT / "crates/mabi-cli/src/main.rs": {
        "required": ["mabi_core::RELEASE_VERSION"],
        "forbidden": ['env!("CARGO_PKG_VERSION")'],
    },
    ROOT / "crates/mabi-modbus/src/testing/report.rs": {
        "required": ["RELEASE_VERSION.to_string()"],
        "forbidden": ['version: "1.0.0".to_string()'],
    },
    ROOT / "crates/mabi-bacnet/src/object/device.rs": {
        "required": ["RELEASE_VERSION.into()"],
        "forbidden": ['"1.0.0".into()'],
    },
    ROOT / "crates/mabi-bacnet/src/server/bacnet_server.rs": {
        "required": ["RELEASE_VERSION.into()"],
        "forbidden": ['"1.0.0".into()'],
    },
}

VERSION_METADATA_CONTRACT_VERSION = "version-metadata-contract-v1"
COMPATIBILITY_MATRIX_VERSION = "compatibility-matrix-v1"
PROTOCOL_READINESS_MATRIX_VERSION = "protocol-readiness-matrix-v1"
PROTOCOL_KEYS = ["modbus", "opcua", "bacnet", "knx"]
REQUIRED_RELEASE_CONTRACTS = [
    "local-runner-contract-v1",
    "cli-output-envelope-v1",
    "runtime-contract-v1",
    "snapshot-metadata-v1",
    "unified-readiness-contract-v1",
    "run-evidence-schema-v1",
    "trial-artifact-contract-v1",
    VERSION_METADATA_CONTRACT_VERSION,
]


def fail(message: str) -> None:
    print(f"release-version check failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def write_text(path: Path, text: str) -> None:
    path.write_text(text, encoding="utf-8")


def root_release_version() -> str:
    cargo = tomllib.loads(read_text(ROOT_CARGO))
    return cargo["workspace"]["package"]["version"]


def root_rust_version() -> str:
    cargo = tomllib.loads(read_text(ROOT_CARGO))
    return cargo["workspace"]["package"]["rust-version"]


def expected_internal_dependency_block(version: str) -> str:
    lines = [BEGIN_MARKER]
    for crate in INTERNAL_DEPENDENCY_CRATES:
        lines.append(f'{crate} = {{ version = "{version}", path = "crates/{crate}" }}')
    lines.append(END_MARKER)
    return "\n".join(lines)


def sync_root_cargo(version: str) -> str:
    text = read_text(ROOT_CARGO)
    pattern = re.compile(
        rf"{re.escape(BEGIN_MARKER)}.*?{re.escape(END_MARKER)}",
        re.DOTALL,
    )
    replacement = expected_internal_dependency_block(version)
    if not pattern.search(text):
        fail("generated internal dependency mirror markers are missing from Cargo.toml")
    return pattern.sub(replacement, text, count=1)


def run_sync() -> None:
    version = root_release_version()
    changed: list[Path] = []

    cargo_text = sync_root_cargo(version)
    if cargo_text != read_text(ROOT_CARGO):
        write_text(ROOT_CARGO, cargo_text)
        changed.append(ROOT_CARGO)

    if changed:
        for path in changed:
            print(path.relative_to(ROOT))
    else:
        print("release version surfaces already in sync")


def check_root_cargo(version: str) -> None:
    text = read_text(ROOT_CARGO)
    expected = expected_internal_dependency_block(version)
    pattern = re.compile(
        rf"{re.escape(BEGIN_MARKER)}.*?{re.escape(END_MARKER)}",
        re.DOTALL,
    )
    match = pattern.search(text)
    if not match:
        fail("generated internal dependency mirror markers are missing from Cargo.toml")
    if match.group(0) != expected:
        fail("workspace internal dependency versions are out of sync with workspace.package.version")


def check_workspace_metadata(version: str) -> None:
    output = subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=ROOT,
        text=True,
    )
    metadata = json.loads(output)
    versions = {}
    for package in metadata["packages"]:
        if package["name"] in WORKSPACE_CRATES:
            versions[package["name"]] = package["version"]

    missing = [crate for crate in WORKSPACE_CRATES if crate not in versions]
    if missing:
        fail(f"workspace metadata did not include expected crates: {', '.join(missing)}")

    mismatched = {name: value for name, value in versions.items() if value != version}
    if mismatched:
        details = ", ".join(f"{name}={value}" for name, value in sorted(mismatched.items()))
        fail(f"workspace crate versions drifted from root release version {version}: {details}")


def check_workspace_manifests() -> None:
    for crate in WORKSPACE_CRATES:
        manifest = ROOT / "crates" / crate / "Cargo.toml"
        text = read_text(manifest)
        if "version.workspace = true" not in text:
            fail(f"{manifest.relative_to(ROOT)} does not use version.workspace = true")


def check_surface_files() -> None:
    for path, rules in SURFACE_RULES.items():
        text = read_text(path)
        for required in rules["required"]:
            if required not in text:
                fail(f"{path.relative_to(ROOT)} is missing expected release-version binding: {required}")
        for forbidden in rules["forbidden"]:
            if forbidden in text:
                fail(f"{path.relative_to(ROOT)} still contains forbidden version surface: {forbidden}")


def yaml_scalar(text: str, key: str, path: Path) -> str:
    pattern = re.compile(rf"^\s*{re.escape(key)}:\s*(?:\"([^\"]+)\"|([^#\n]+))", re.MULTILINE)
    match = pattern.search(text)
    if not match:
        fail(f"{path.relative_to(ROOT)} is missing required key {key}")
    return (match.group(1) or match.group(2)).strip()


def check_release_docs(version: str) -> None:
    release_docs = [
        ROOT / "docs/release/version-metadata-contract.yaml",
        ROOT / "docs/release/compatibility-matrix.yaml",
        ROOT / "docs/release/release-checklist.md",
        ROOT / "docs/release/changelog-policy.md",
    ]
    for path in release_docs:
        if not path.is_file():
            fail(f"{path.relative_to(ROOT)} is missing")

    version_contract = read_text(ROOT / "docs/release/version-metadata-contract.yaml")
    matrix_path = ROOT / "docs/release/compatibility-matrix.yaml"
    matrix = read_text(matrix_path)
    local_runner_contract = read_text(ROOT / "docs/cli/local-runner-contract.yaml")
    runner_contract = read_text(ROOT / "crates/mabi-cli/src/runner_contract.rs")
    runtime_registry = read_text(ROOT / "crates/mabi-cli/src/runtime_registry.rs")
    readiness_matrix = read_text(ROOT / "docs/protocol-readiness/protocol-readiness-matrix.yaml")

    if yaml_scalar(version_contract, "contract_version", ROOT / "docs/release/version-metadata-contract.yaml") != VERSION_METADATA_CONTRACT_VERSION:
        fail("version metadata contract version is not version-metadata-contract-v1")
    if yaml_scalar(matrix, "matrix_version", matrix_path) != COMPATIBILITY_MATRIX_VERSION:
        fail("compatibility matrix version is not compatibility-matrix-v1")
    if yaml_scalar(matrix, "current_engine_version", matrix_path) != version:
        fail("compatibility matrix current_engine_version is out of sync with workspace.package.version")
    if yaml_scalar(matrix, "workspace_rust_version", matrix_path) != root_rust_version():
        fail("compatibility matrix workspace_rust_version is out of sync with workspace.package.rust-version")
    if yaml_scalar(matrix, "readiness_matrix_version", matrix_path) != PROTOCOL_READINESS_MATRIX_VERSION:
        fail("compatibility matrix readiness_matrix_version is out of sync with protocol readiness matrix")

    for contract in REQUIRED_RELEASE_CONTRACTS:
        for label, text in [
            ("version metadata contract", version_contract),
            ("compatibility matrix", matrix),
        ]:
            if contract not in text:
                fail(f"{label} is missing required contract version {contract}")
    if VERSION_METADATA_CONTRACT_VERSION not in local_runner_contract:
        fail("local runner contract does not report version-metadata-contract-v1")
    if VERSION_METADATA_CONTRACT_VERSION not in runner_contract or "version_metadata_contract" not in runner_contract:
        fail("CLI version output source does not expose version-metadata-contract-v1")

    for protocol in PROTOCOL_KEYS:
        if f"protocol_key: {protocol}" not in matrix:
            fail(f"compatibility matrix is missing protocol capability entry for {protocol}")
        if f"mabi_{protocol}" not in runtime_registry:
            fail(f"runtime registry no longer registers mabi_{protocol}")
        if f"- id: {protocol}" not in readiness_matrix:
            fail(f"protocol readiness matrix is missing {protocol}")


def run_check() -> None:
    version = root_release_version()
    check_root_cargo(version)
    check_workspace_manifests()
    check_workspace_metadata(version)
    check_surface_files()
    check_release_docs(version)
    print(f"release version surfaces are in sync for {version}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Sync and validate Mabinogion release version surfaces")
    parser.add_argument("mode", choices=["sync", "check"])
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if args.mode == "sync":
        run_sync()
    else:
        run_check()


if __name__ == "__main__":
    main()
