use std::process::Command;

fn run_unxml(args: &[&str]) -> String {
    let output = Command::new("cargo")
        .args(["run", "--quiet", "--"])
        .args(args)
        .output()
        .expect("Failed to execute unxml");

    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn test_xslt_without_expand() {
    let output = run_unxml(&["--xslt", "tests/fixtures/main.xsl"]);

    // Without --expand, should show "apply Header" not expanded
    assert!(output.contains("apply Input/Header"));
    assert!(output.contains("apply Input/Body"));
    // Should not contain expansion comment
    assert!(!output.contains("# [expanded:"));
}

#[test]
fn test_xslt_with_expand() {
    let output = run_unxml(&["--xslt", "--expand", "tests/fixtures/main.xsl"]);

    // With --expand, Header template should be inlined (matches Header|Footer)
    assert!(output.contains("# [expanded: apply Input/Header]"));

    // The expanded template content should appear
    assert!(output.contains("Section"));
    assert!(output.contains("<- Name"));
    assert!(output.contains("<- Timestamp"));
}

#[test]
fn test_expand_union_pattern() {
    // Test that "Header" matches template with "Header|Footer" pattern
    let output = run_unxml(&["--xslt", "--expand", "tests/fixtures/main.xsl"]);

    // Should expand Header even though template is "Header|Footer"
    assert!(output.contains("# [expanded: apply Input/Header]"));
}

#[test]
fn test_expand_preserves_local_templates() {
    let output = run_unxml(&["--xslt", "--expand", "tests/fixtures/main.xsl"]);

    // Body template is defined locally, should still work
    assert!(output.contains("template Body"));
    assert!(output.contains("<- Text"));
}
