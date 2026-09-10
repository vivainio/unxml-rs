//! Serialises an `XmlElement` subtree back to XML text — the one direction
//! unxml never needed before `diff`/`patch`, since every other mode is
//! one-way (XML/HTML in, the pug-like unxml view out).
//!
//! Namespace-oblivious like the rest of the parser: a prefix
//! (`cac:InvoiceLine`, `xmlns:cac="..."`) is just literal text in the
//! name/attribute here, never resolved or validated, so a serialized
//! fragment doesn't need synthetic namespace scaffolding to parse cleanly
//! again — unlike a DOM-based approach, which has to re-declare every
//! ancestor namespace on a fragment wrapper.
//!
//! Attribute order is not preserved: `XmlElement::attributes` is a
//! `HashMap`, so original order is already lost by parse time. Attributes
//! are emitted sorted by name instead, for deterministic output — the one
//! cosmetic divergence from a hand-authored base file, consistent with the
//! rest of unxml already being attribute-order-agnostic.

use crate::model::{NodeRef, XmlElement};

/// Serialize a sequence of sibling elements, each terminated by a newline,
/// indented two spaces per level like the rest of unxml's own output. A
/// container element (one with element children) gets one child per line;
/// a leaf (text-only, or empty) stays on a single line so its text content
/// is never touched by added whitespace. Used both for a whole document
/// (usually a single root) and for a multi-fragment `insert-after`/
/// `insert-before` `content`.
pub(crate) fn write_elements(elems: &[XmlElement]) -> String {
    let mut out = String::new();
    for e in elems {
        write_into(e, 0, &mut out);
        out.push('\n');
    }
    out
}

fn write_into(elem: &XmlElement, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    out.push_str(&pad);
    out.push('<');
    out.push_str(&elem.name);
    let mut names: Vec<&String> = elem.attributes.keys().collect();
    names.sort();
    for name in names {
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        out.push_str(&escape_attr(&elem.attributes[name]));
        out.push('"');
    }
    let has_body = !elem.nodes.is_empty() || !elem.text_content.is_empty();
    if !has_body {
        out.push_str("/>");
        return;
    }
    out.push('>');

    let has_element_child = elem.nodes.iter().any(|n| matches!(n, NodeRef::Child(_)));
    if !has_element_child {
        // Leaf: text/comments render inline, on the same line as the tags,
        // so no whitespace is added inside meaningful text content.
        if elem.nodes.is_empty() {
            out.push_str(&escape_text(&elem.text_content));
        } else {
            for node in &elem.nodes {
                match node {
                    NodeRef::Text(t) => out.push_str(&escape_text(t)),
                    NodeRef::Comment { text, .. } => {
                        out.push_str("<!--");
                        out.push_str(text);
                        out.push_str("-->");
                    }
                    NodeRef::Child(_) => unreachable!("has_element_child is false"),
                }
            }
        }
        out.push_str("</");
        out.push_str(&elem.name);
        out.push('>');
        return;
    }

    // Container: one child (or comment) per line, indented one level deeper.
    // Whitespace-only text runs between elements are dropped rather than
    // copied — this function re-indents from scratch, so the source's own
    // incidental line wraps would otherwise double up with the new ones.
    out.push('\n');
    for node in &elem.nodes {
        match node {
            NodeRef::Child(i) => {
                write_into(&elem.children[*i], indent + 1, out);
                out.push('\n');
            }
            NodeRef::Comment { text, .. } => {
                out.push_str(&"  ".repeat(indent + 1));
                out.push_str("<!--");
                out.push_str(text);
                out.push_str("-->\n");
            }
            NodeRef::Text(t) if !t.trim().is_empty() => {
                out.push_str(&"  ".repeat(indent + 1));
                out.push_str(&escape_text(t.trim()));
                out.push('\n');
            }
            NodeRef::Text(_) => {}
        }
    }
    out.push_str(&pad);
    out.push_str("</");
    out.push_str(&elem.name);
    out.push('>');
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attr(s: &str) -> String {
    escape_text(s).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_closes_an_empty_leaf() {
        let e = XmlElement::new("Foo".to_string());
        assert_eq!(write_elements(std::slice::from_ref(&e)), "<Foo/>\n");
    }

    #[test]
    fn sorts_attributes_by_name() {
        let mut e = XmlElement::new("Foo".to_string());
        e.attributes.insert("z".to_string(), "1".to_string());
        e.attributes.insert("a".to_string(), "2".to_string());
        assert_eq!(write_elements(&[e]), "<Foo a=\"2\" z=\"1\"/>\n");
    }

    #[test]
    fn escapes_text_and_attribute_values() {
        let mut e = XmlElement::new("Foo".to_string());
        e.attributes
            .insert("a".to_string(), "1 & \"two\"".to_string());
        e.text_content = "x < y & y > z".to_string();
        e.nodes.push(NodeRef::Text(e.text_content.clone()));
        assert_eq!(
            write_elements(&[e]),
            "<Foo a=\"1 &amp; &quot;two&quot;\">x &lt; y &amp; y &gt; z</Foo>\n"
        );
    }

    #[test]
    fn indents_a_container_one_child_per_line() {
        let mut child_a = XmlElement::new("A".to_string());
        child_a.text_content = "1".to_string();
        child_a.nodes.push(NodeRef::Text("1".to_string()));
        let mut child_b = XmlElement::new("B".to_string());
        child_b.text_content = "2".to_string();
        child_b.nodes.push(NodeRef::Text("2".to_string()));

        let mut root = XmlElement::new("Root".to_string());
        root.nodes.push(NodeRef::Child(0));
        root.nodes.push(NodeRef::Child(1));
        root.children.push(child_a);
        root.children.push(child_b);

        assert_eq!(
            write_elements(&[root]),
            "<Root>\n  <A>1</A>\n  <B>2</B>\n</Root>\n"
        );
    }

    #[test]
    fn writes_multiple_fragments_in_order() {
        let a = XmlElement::new("A".to_string());
        let b = XmlElement::new("B".to_string());
        assert_eq!(write_elements(&[a, b]), "<A/>\n<B/>\n");
    }

    #[test]
    fn round_trips_through_the_xml_parser() {
        use crate::parse::parse_xml;

        let mut child = XmlElement::new("Bar".to_string());
        child.text_content = "hi".to_string();
        child.nodes.push(NodeRef::Text("hi".to_string()));

        let mut root = XmlElement::new("Foo".to_string());
        root.nodes.push(NodeRef::Child(0));
        root.children.push(child);

        let xml = write_elements(&[root]);
        let parsed = parse_xml(&xml).unwrap();
        assert_eq!(parsed.roots.len(), 1);
        assert_eq!(parsed.roots[0].name, "Foo");
        assert_eq!(parsed.roots[0].children[0].name, "Bar");
        assert_eq!(parsed.roots[0].children[0].text_content, "hi");
    }
}
