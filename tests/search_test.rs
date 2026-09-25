//! `--select` as a search over many inputs: attribute predicates, silent
//! skipping of non-matching documents, and `--zip` archives.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

fn run_unxml(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_unxml"))
        .args(args)
        .output()
        .expect("Failed to execute unxml");
    assert!(
        output.status.success(),
        "unxml failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("unxml-search-test-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// Only the documents with a match appear, each under its own header.
#[test]
fn test_select_attribute_skips_non_matching_files() {
    let out = run_unxml(&[
        "--select",
        r#"item[@id="2"]"#,
        "test-input/simple.xml",
        "test-input/data.xml",
    ]);
    assert_eq!(
        out,
        "// FILE: test-input/simple.xml\nitem(id=\"2\")\n  name = Second Item\n  value = 200\n"
    );
}

// Archive entries are searched too, named `archive!/entry`; entries that
// aren't markup (here a PNG) and non-matching entries are skipped.
#[test]
fn test_zip_entries_are_searched() {
    let zip_path = scratch("zip").join("bundle.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    for (name, body) in [
        ("a/hit.xml", r#"<r><order id="7"><qty>3</qty></order></r>"#),
        ("a/miss.xml", r#"<r><order id="8"/></r>"#),
        ("logo.png", "\u{89}PNG"),
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap();

    let zip_arg = zip_path.to_str().unwrap();
    let out = run_unxml(&["--select", r#"order[@id="7"]"#, "--zip", zip_arg]);
    assert_eq!(
        out,
        format!("// FILE: {zip_arg}!/a/hit.xml\norder(id=\"7\")\n  qty = 3\n")
    );
}

// A `// FILE:` name from a search, passed back as an argument, dumps that
// whole entry; a single entry reads like a single file, so no header.
#[test]
fn test_zip_entry_argument_dumps_whole_document() {
    let zip_path = scratch("entry").join("bundle.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    for (name, body) in [
        ("orders/1.xml", r#"<r><order id="1"/><note>one</note></r>"#),
        ("orders/2.xml", r#"<r><order id="2"/><note>two</note></r>"#),
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    let zip_arg = zip_path.to_str().unwrap();

    let entry = format!("{zip_arg}!/orders/2.xml");
    assert_eq!(run_unxml(&[&entry]), "r\n  order(id=\"2\")\n  note = two\n");

    // An entry glob selects several entries, each under its own header.
    let glob = format!("{zip_arg}!/orders/*.xml");
    let out = run_unxml(&["--select", "note", &glob]);
    assert_eq!(
        out,
        format!(
            "// FILE: {zip_arg}!/orders/1.xml\nnote = one\n\n// FILE: {zip_arg}!/orders/2.xml\nnote = two\n"
        )
    );
}
