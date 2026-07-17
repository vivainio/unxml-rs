//! Golden-file end-to-end test: runs the built `unxml` binary over every
//! fixture in `test-input/` and diffs stdout against `expected-output/*.unxml`.
//!
//! Run with `UNXML_TEST_UPDATE=1 cargo test --test e2e_test` to refresh the
//! golden files after an intentional output change.

use similar::{ChangeTag, TextDiff};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FIXTURE_EXTENSIONS: &[&str] = &[
    "xml", "html", "htm", "xsl", "sch", "xsd", "wsdl", "targets", "props",
];

fn find_fixtures() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir("test-input")
        .expect("failed to read test-input/")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| FIXTURE_EXTENSIONS.contains(&ext))
        })
        .collect();
    files.sort();
    files
}

/// Extra CLI flags a fixture needs, based on the same filename/extension
/// conventions `test-suite.py::run_unxml` uses.
fn extra_args(path: &Path) -> Vec<&'static str> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let mut args = Vec::new();

    if name == "special-elements.xml" {
        args.push("--special");
    }

    match ext {
        "xsl" => args.push("--xslt"),
        "sch" => args.push("--schematron"),
        "xsd" => args.push("--xsd"),
        "wsdl" => args.push("--wsdl"),
        "targets" | "props" => args.push("--msbuild"),
        _ => {}
    }

    if name.starts_with("select-") {
        args.push("--select");
        args.push("item");
    }

    if name.starts_with("ubl-") || name.starts_with("cii-") || name.starts_with("msbuild-sniff-") {
        args.push("--auto");
    }

    if name.starts_with("collapse-only-") {
        args.push("--collapse=ext:UBLExtensions");
    } else if name.starts_with("collapse-") {
        args.push("--collapse");
    }

    args
}

fn run_unxml(input: &Path, extra: &[&str]) -> String {
    let bin = env!("CARGO_BIN_EXE_unxml");
    let output = Command::new(bin)
        .args(extra)
        .arg(input)
        .output()
        .unwrap_or_else(|e| panic!("failed to execute {bin} on {}: {e}", input.display()));

    assert!(
        output.status.success(),
        "unxml exited with {} on {}\nstderr:\n{}",
        output.status,
        input.display(),
        String::from_utf8_lossy(&output.stderr),
    );

    String::from_utf8_lossy(&output.stdout).to_string()
}

fn expected_output_path(input: &Path) -> PathBuf {
    let name = input.file_name().and_then(|n| n.to_str()).unwrap();
    PathBuf::from("expected-output").join(format!("{name}.unxml"))
}

fn render_diff(expected: &str, actual: &str) -> String {
    let diff = TextDiff::from_lines(expected, actual);
    let mut buf = String::new();
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        buf.push_str(sign);
        buf.push_str(change.value());
    }
    buf
}

#[test]
fn golden_files_match() {
    let update = std::env::var_os("UNXML_TEST_UPDATE").is_some();
    let fixtures = find_fixtures();
    assert!(!fixtures.is_empty(), "no fixtures found in test-input/");

    let mut failures = Vec::new();

    for input in &fixtures {
        let actual = run_unxml(input, &extra_args(input));
        let expected_path = expected_output_path(input);

        if update {
            fs::write(&expected_path, &actual)
                .unwrap_or_else(|e| panic!("failed to write {}: {e}", expected_path.display()));
            continue;
        }

        let expected = fs::read_to_string(&expected_path).unwrap_or_else(|_| {
            panic!(
                "missing expected output {} — run `UNXML_TEST_UPDATE=1 cargo test --test e2e_test` to create it",
                expected_path.display()
            )
        });

        if actual != expected {
            failures.push(format!(
                "{}:\n--- {} (expected)\n+++ (actual)\n{}",
                input.display(),
                expected_path.display(),
                render_diff(&expected, &actual)
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} of {} fixture(s) mismatched:\n\n{}",
            failures.len(),
            fixtures.len(),
            failures.join("\n")
        );
    }
}
