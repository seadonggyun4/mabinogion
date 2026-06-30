use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const STALE_PHRASES: &[&str] = &[
    concat!("Industrial Protocol ", "Simulator Server"),
    concat!("OT Protocol ", "Simulator"),
    concat!("trap", "-", "sim"),
    concat!("TRAP protocol ", "simulator"),
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
fn root_readme_does_not_reintroduce_product_family_ownership_table() {
    let root = repo_root();
    let root_readme = read(&root, "README.md");

    assert_no_stale_phrases("README.md", &root_readme);
    for forbidden in [
        "## Product Family Role",
        "| `mabinogion-trials` |",
        "| `imugi-back` |",
        "| `imugi-front` |",
        "The boundary is intentional",
    ] {
        assert!(
            !root_readme.contains(forbidden),
            "README.md should not reintroduce product-family ownership text {forbidden:?}"
        );
    }
}

#[test]
fn public_markdown_docs_do_not_reintroduce_stale_product_phrases() {
    let root = repo_root();
    let mut files = vec![root.join("README.md")];
    collect_text_files(&root.join("docs"), &mut files);

    for path in files {
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let relative = path
            .strip_prefix(&root)
            .expect("path should be inside root")
            .display()
            .to_string();
        assert_no_stale_phrases(&relative, &contents);
    }
}

#[test]
fn crate_readmes_do_not_reintroduce_stale_product_phrases() {
    let root = repo_root();
    let crates_root = root.join("crates");
    for entry in fs::read_dir(&crates_root)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", crates_root.display()))
    {
        let entry = entry.expect("crate directory entry should be readable");
        let path = entry.path().join("README.md");
        if !path.is_file() {
            continue;
        }
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let relative = path
            .strip_prefix(&root)
            .expect("path should be inside root")
            .display()
            .to_string();
        assert_no_stale_phrases(&relative, &contents);
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
fn cli_help_does_not_reintroduce_stale_product_phrases() {
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
