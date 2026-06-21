use std::process::Command;

fn run_unxml(args: &[&str]) -> String {
    let output = Command::new("cargo")
        .args(["run", "--quiet", "--"])
        .args(args)
        .output()
        .expect("Failed to execute unxml");

    String::from_utf8_lossy(&output.stdout).to_string()
}

// Shallow mixed content (prose interleaved with inline elements) renders as one
// line of verbatim XML rather than a stack of flattened text runs and children.
#[test]
fn test_inline_span_is_verbatim() {
    let output = run_unxml(&["test-input/inline-mixed.xml"]);

    assert!(output.contains(
        "para = The <command>widget</command> daemon keeps its state in a single database."
    ));
    // The old stacked form must be gone for this paragraph.
    assert!(!output.contains("  \"The\""));
}

// An inline span keeps its attributes verbatim, and void elements work too.
#[test]
fn test_inline_span_with_attribute_and_void() {
    let output = run_unxml(&["test-input/inline-mixed.xml"]);

    assert!(
        output.contains(
            "para = See <link href=\"install.html\">the install guide</link> for details."
        )
    );
    assert!(output.contains("para = A void ref <xref linkend=\"recovery\"/> mid-sentence."));
}

// Nested inline markup collapses all the way up (recursive inline-safety).
#[test]
fn test_nested_inline_collapses() {
    let output = run_unxml(&["test-input/inline-mixed.xml"]);

    assert!(
        output.contains("para = Nested <emphasis>the <command>x</command> flag</emphasis> here.")
    );
}

// A preformatted leaf (multiline text) keeps its parent block: it is not
// inline-safe, so the paragraph stays flattened and the listing is piped.
#[test]
fn test_preformatted_child_stays_block() {
    let output = run_unxml(&["test-input/inline-mixed.xml"]);

    assert!(output.contains("    \"Run this:\""));
    assert!(output.contains("    programlisting ="));
    assert!(output.contains("      | line one"));
}
