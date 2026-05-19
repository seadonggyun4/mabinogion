use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const STALE_PHRASES: &[&str] = &[
    concat!("Industrial Protocol ", "Simulator Server"),
    concat!("OT Protocol ", "Simulator"),
    concat!("trap", "-", "sim"),
    concat!("TRAP protocol ", "simulator"),
];

const CRATE_READMES: &[(&str, &str)] = &[
    ("mabi-core", "crates/mabi-core/README.md"),
    ("mabi-runtime", "crates/mabi-runtime/README.md"),
    ("mabi-cli", "crates/mabi-cli/README.md"),
    ("mabi-modbus", "crates/mabi-modbus/README.md"),
    ("mabi-opcua", "crates/mabi-opcua/README.md"),
    ("mabi-bacnet", "crates/mabi-bacnet/README.md"),
    ("mabi-knx", "crates/mabi-knx/README.md"),
    ("mabi-scenario", "crates/mabi-scenario/README.md"),
    ("mabi-chaos", "crates/mabi-chaos/README.md"),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root should resolve")
}

fn read(root: &Path, relative_path: &str) -> String {
    let path = root.join(relative_path);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

fn assert_no_stale_phrases(path: &str, contents: &str) {
    for phrase in STALE_PHRASES {
        assert!(
            !contents.contains(phrase),
            "{path} should not contain stale product phrase {phrase:?}"
        );
    }
}

fn collect_text_files(path: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(path)
        .unwrap_or_else(|error| panic!("failed to read directory {}: {error}", path.display()))
    {
        let entry = entry.expect("directory entry should be readable");
        let path = entry.path();
        if path.is_dir() {
            collect_text_files(&path, files);
            continue;
        }
        let extension = path.extension().and_then(|value| value.to_str());
        if matches!(extension, Some("md" | "rs" | "yaml" | "yml" | "toml")) {
            files.push(path);
        }
    }
}

fn is_runtime_legacy_identifier(path: &Path, contents: &str) -> bool {
    let path = path.to_string_lossy();
    contents.contains(concat!("trap", "-", "simulator"))
        && (path.ends_with("crates/mabi-core/src/config/mod.rs")
            || path.ends_with("crates/mabi-core/src/logging/config.rs")
            || path.ends_with("crates/mabi-core/src/logging/mod.rs")
            || path.ends_with("crates/mabi-opcua/src/factory.rs"))
}

#[test]
fn public_docs_use_product_family_positioning() {
    let root = repo_root();
    let root_readme = read(&root, "README.md");
    let cli_docs = read(&root, "docs/cli/README.md");
    let docs_index = read(&root, "docs/README.md");

    for (path, contents) in [
        ("README.md", root_readme.as_str()),
        ("docs/cli/README.md", cli_docs.as_str()),
        ("docs/README.md", docs_index.as_str()),
    ] {
        assert_no_stale_phrases(path, contents);
        for required in [
            "protocol resilience engine",
            "Mabinogion trials",
            "Forge",
            "Trials",
            "official certification",
        ] {
            assert!(
                contents.contains(required),
                "{path} should contain product-family phrase {required:?}"
            );
        }
    }
}

#[test]
fn crate_readmes_are_crate_specific_and_boundary_aware() {
    let root = repo_root();
    for (crate_name, path) in CRATE_READMES {
        let contents = read(&root, path);
        assert_no_stale_phrases(path, &contents);
        assert!(
            contents.contains(crate_name),
            "{path} should identify {crate_name}"
        );
        for required in [
            "What this crate owns",
            "How it fits in Mabinogion",
            "Versioning / contracts",
            "Not owned here",
            "Mabinogion",
            "official certification",
        ] {
            assert!(
                contents.contains(required),
                "{path} should contain crate README section or boundary {required:?}"
            );
        }
    }
}

#[test]
fn source_and_docs_do_not_reintroduce_stale_product_phrases() {
    let root = repo_root();
    let mut files = vec![root.join("README.md")];
    collect_text_files(&root.join("crates"), &mut files);
    collect_text_files(&root.join("docs"), &mut files);

    for path in files {
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        if is_runtime_legacy_identifier(&path, &contents) {
            continue;
        }
        let relative = path
            .strip_prefix(&root)
            .expect("path should be inside root")
            .display()
            .to_string();
        assert_no_stale_phrases(&relative, &contents);
    }
}

#[test]
fn cli_help_uses_product_family_wording() {
    let output = Command::new(env!("CARGO_BIN_EXE_mabi"))
        .arg("--help")
        .output()
        .expect("mabi --help should run");
    assert!(
        output.status.success(),
        "mabi --help failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8_lossy(&output.stdout);
    assert_no_stale_phrases("mabi --help", &help);
    for required in [
        "protocol resilience engine for Mabinogion trials",
        "shared runtime",
        "Forge",
        "Trials",
        "runner surfaces",
    ] {
        assert!(
            help.contains(required),
            "mabi --help should contain {required:?}"
        );
    }
    assert!(
        !help.contains("Industrial protocol simulator for testing and development"),
        "mabi --help should not use the old about text"
    );
}

#[test]
fn serve_help_keeps_local_execution_without_simulator_only_positioning() {
    let output = Command::new(env!("CARGO_BIN_EXE_mabi"))
        .args(["serve", "--help"])
        .output()
        .expect("mabi serve --help should run");
    assert!(
        output.status.success(),
        "mabi serve --help failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8_lossy(&output.stdout);
    assert_no_stale_phrases("mabi serve --help", &help);
    assert!(help.contains("Run a local protocol service through the shared runtime"));
}
