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

// `--canonical` makes semantically equivalent documents render identically:
// two documents differing only in namespace-prefix spelling, default-vs-explicit
// namespace, and sibling order produce byte-identical output so they diff clean.
#[test]
fn test_canonical_equivalent_docs_match() {
    let dir = std::env::temp_dir().join("unxml-canonical-test");
    std::fs::create_dir_all(&dir).unwrap();

    let a = dir.join("a.xml");
    let b = dir.join("b.xml");
    std::fs::write(
        &a,
        r#"<?xml version="1.0"?>
<a:Order xmlns:a="urn:shop:order" xmlns:c="urn:shop:cust">
  <a:Line sku="X1"><a:Qty>2</a:Qty></a:Line>
  <a:Line sku="A9"><a:Qty>1</a:Qty></a:Line>
  <c:Customer id="42">Acme</c:Customer>
</a:Order>"#,
    )
    .unwrap();
    std::fs::write(
        &b,
        r#"<?xml version="1.0"?>
<Order xmlns="urn:shop:order" xmlns:cust="urn:shop:cust">
  <cust:Customer id="42">Acme</cust:Customer>
  <Line sku="A9"><Qty>1</Qty></Line>
  <Line sku="X1"><Qty>2</Qty></Line>
</Order>"#,
    )
    .unwrap();

    let out_a = run_unxml(&["--canonical", a.to_str().unwrap()]);
    let out_b = run_unxml(&["--canonical", b.to_str().unwrap()]);

    assert_eq!(out_a, out_b, "canonical output should be identical");
    // Default namespace is rebound to an explicit canonical prefix.
    assert!(out_a.contains(":Order("));
    // Without --canonical the two differ.
    assert_ne!(
        run_unxml(&[a.to_str().unwrap()]),
        run_unxml(&[b.to_str().unwrap()])
    );
}

// Recognised vocabularies keep their conventional prefix regardless of the
// prefix the source chose.
#[test]
fn test_canonical_well_known_prefixes() {
    let dir = std::env::temp_dir().join("unxml-canonical-test");
    std::fs::create_dir_all(&dir).unwrap();

    let f = dir.join("wk.xml");
    std::fs::write(
        &f,
        r#"<?xml version="1.0"?>
<foo:stylesheet xmlns:foo="http://www.w3.org/1999/XSL/Transform" xmlns:s="http://www.w3.org/2001/XMLSchema">
  <foo:template match="/"><s:element/></foo:template>
</foo:stylesheet>"#,
    )
    .unwrap();

    let out = run_unxml(&["--canonical", f.to_str().unwrap()]);
    assert!(out.contains("xsl:stylesheet"), "got: {out}");
    assert!(out.contains("xsl:template"), "got: {out}");
    assert!(out.contains("xs:element"), "got: {out}");
}

// `--paths` dumps distinct element paths, sorted, each annotated with the union
// of attribute names seen at that path (across all occurrences).
#[test]
fn test_paths_dump_with_attribute_union() {
    let dir = std::env::temp_dir().join("unxml-paths-test");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("o.xml");
    std::fs::write(
        &f,
        r#"<?xml version="1.0"?>
<order id="7" date="2026-01-01">
  <line sku="X1"><qty unit="ea">2</qty></line>
  <line sku="A9" discount="0.1"><qty>1</qty></line>
  <customer id="42"><name>Acme</name></customer>
</order>"#,
    )
    .unwrap();

    let out = run_unxml(&["--paths", f.to_str().unwrap()]);
    let lines: Vec<&str> = out.lines().collect();

    // Distinct, sorted paths; attribute names unioned across occurrences
    // (discount appears on only one <line> but still shows up).
    assert_eq!(
        lines,
        vec![
            "order(date, id)",
            "order/customer(id)",
            "order/customer/name",
            "order/line(discount, sku)",
            "order/line/qty(unit)",
        ]
    );

    // --select scopes the dump to matched subtrees, rooted at the match.
    let scoped = run_unxml(&["--paths", "--select", "line", f.to_str().unwrap()]);
    assert_eq!(
        scoped.lines().collect::<Vec<_>>(),
        vec!["line(discount, sku)", "line/qty(unit)"]
    );
}

// `--paths` prefixes a legend explaining each namespace prefix, so the prefixed
// path segments (and --canonical's generated ns1/ns2) are interpretable.
#[test]
fn test_paths_namespace_legend() {
    let dir = std::env::temp_dir().join("unxml-paths-test");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("ns.xml");
    std::fs::write(
        &f,
        r#"<?xml version="1.0"?>
<a:order xmlns:a="urn:shop:order" xmlns:c="urn:shop:cust">
  <a:line><a:qty>2</a:qty></a:line>
  <c:customer id="42"/>
</a:order>"#,
    )
    .unwrap();

    let out = run_unxml(&["--paths", f.to_str().unwrap()]);
    assert!(out.contains("// a = urn:shop:order"), "got: {out}");
    assert!(out.contains("// c = urn:shop:cust"), "got: {out}");
    assert!(out.contains("a:order/c:customer(id)"), "got: {out}");

    // Under --canonical the legend resolves the generated ns1/ns2 names.
    let canon = run_unxml(&["--paths", "--canonical", f.to_str().unwrap()]);
    assert!(canon.contains("// ns2 = urn:shop:order"), "got: {canon}");
}

// --hide-ns strips the hidden prefix from attribute names too, not just element
// names, so a hidden vocabulary reads as bare local names throughout.
#[test]
fn test_hide_ns_strips_attribute_prefixes() {
    let dir = std::env::temp_dir().join("unxml-hidens-test");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("ns.xml");
    std::fs::write(
        &f,
        r#"<?xml version="1.0"?>
<a:order xmlns:a="urn:shop:order">
  <a:line a:sku="X1">2</a:line>
</a:order>"#,
    )
    .unwrap();

    let out = run_unxml(&["--hide-ns", "a", f.to_str().unwrap()]);
    assert!(out.contains(r#"line(sku="X1")"#), "got: {out}");
    assert!(!out.contains("a:sku"), "prefix should be gone: {out}");
}

// --canonical rebinds prefixes in every mode but only sorts siblings in plain
// XML: under a dialect mode (--xslt) element order is significant, so it is
// preserved while the namespace prefix is still normalised.
#[test]
fn test_canonical_preserves_order_in_dialect_mode() {
    let dir = std::env::temp_dir().join("unxml-canonical-test");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("s.xsl");
    std::fs::write(
        &f,
        r#"<?xml version="1.0"?>
<x:stylesheet version="1.0" xmlns:x="http://www.w3.org/1999/XSL/Transform">
  <x:template match="/"><zzz/><aaa/></x:template>
</x:stylesheet>"#,
    )
    .unwrap();

    // --xslt: prefix rebound (x -> xsl, so the renderer recognises it), order kept.
    let dialect = run_unxml(&["--canonical", "--xslt", f.to_str().unwrap()]);
    assert!(dialect.contains("match /:"), "got: {dialect}");
    assert!(
        dialect.find("zzz").unwrap() < dialect.find("aaa").unwrap(),
        "dialect mode must preserve order: {dialect}"
    );

    // Plain XML: siblings sorted, so aaa precedes zzz.
    let plain = run_unxml(&["--canonical", f.to_str().unwrap()]);
    assert!(
        plain.find("aaa").unwrap() < plain.find("zzz").unwrap(),
        "plain mode should sort: {plain}"
    );
}
