//! `unxml outline`: a flat landmark list — sections, method definitions and
//! calls (`--special`), or templates/functions and calls (`--xslt`) — each
//! with its source line span, for skimming or letting an AI agent jump
//! straight to the part of a large document it needs without reading the
//! whole thing.
//!
//! Entry labels reuse the exact text the normal dialect renderers
//! (`render.rs`, `xslt.rs`) would print for that node, so outline output
//! stays in the same vocabulary as `unxml --special`/`--xslt`.

use crate::model::XmlElement;
use crate::render::method_call_target;

#[derive(Clone, Copy)]
pub(crate) enum Dialect {
    Special,
    Xslt,
    /// No dialect flag given: fall back to any element carrying a `name` or
    /// `id` attribute.
    Generic,
}

struct Entry {
    depth: usize,
    label: String,
    start_line: usize,
    end_line: usize,
}

/// The label for `element` under `dialect`, if it's a recognized landmark.
fn label_for(element: &XmlElement, dialect: Dialect) -> Option<String> {
    match dialect {
        Dialect::Special => match element.name.as_str() {
            "builtInMethodParameterList" | "builtinmethodparameterlist" => {
                let name = element.attributes.get("name")?;
                Some(format!("{name}()"))
            }
            "section" if element.attributes.len() == 1 => {
                let name = element.attributes.get("name")?;
                Some(format!("#{name}"))
            }
            "method" => {
                let jump_to_xml_file = element.attributes.get("jumpToXmlFile")?;
                let jump_to_xpath = element.attributes.get("jumpToXPath")?;
                Some(method_call_target(
                    jump_to_xml_file,
                    jump_to_xpath,
                    element.attributes.get("name").map(String::as_str),
                ))
            }
            _ => None,
        },
        Dialect::Xslt => match element.name.as_str() {
            "xsl:template" => {
                if let Some(match_val) = element.attributes.get("match") {
                    Some(format!("match {match_val}"))
                } else {
                    let name = element.attributes.get("name")?;
                    Some(format!("template {name}"))
                }
            }
            "xsl:function" => {
                let name = element.attributes.get("name")?;
                Some(format!("function {name}"))
            }
            "xsl:call-template" => {
                let name = element.attributes.get("name")?;
                Some(format!("call {name}"))
            }
            "xsl:apply-templates" => match element.attributes.get("select") {
                Some(select) => Some(format!("apply {select}")),
                None => Some("apply".to_string()),
            },
            _ => None,
        },
        Dialect::Generic => {
            let name = element
                .attributes
                .get("name")
                .or_else(|| element.attributes.get("id"))?;
            Some(format!("{} {name}", element.name))
        }
    }
}

/// Depth only increases at a recognized landmark; a non-landmark element (a
/// transparent wrapper, e.g. `dataProcessing`) is skipped without changing
/// depth, so its children still nest under the nearest enclosing landmark.
fn collect(element: &XmlElement, depth: usize, dialect: Dialect, out: &mut Vec<Entry>) {
    let next_depth = match label_for(element, dialect) {
        Some(label) => {
            out.push(Entry {
                depth,
                label,
                start_line: element.start_line,
                end_line: element.end_line,
            });
            depth + 1
        }
        None => depth,
    };
    for child in &element.children {
        collect(child, next_depth, dialect, out);
    }
}

/// Render the outline for one document's roots. Empty when nothing matched.
pub(crate) fn render_outline(roots: &[XmlElement], dialect: Dialect) -> String {
    let mut entries = Vec::new();
    for root in roots {
        collect(root, 0, dialect, &mut entries);
    }
    if entries.is_empty() {
        return String::new();
    }

    let indented_labels: Vec<String> = entries
        .iter()
        .map(|e| format!("{}{}", "  ".repeat(e.depth), e.label))
        .collect();
    let width = indented_labels
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0);

    let mut out = String::new();
    for (entry, left) in entries.iter().zip(indented_labels) {
        let line_ref = if entry.start_line == entry.end_line {
            entry.start_line.to_string()
        } else {
            format!("{}-{}", entry.start_line, entry.end_line)
        };
        out.push_str(&format!("{left:width$}  {line_ref}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_xml;

    /// (depth, label, start_line, end_line) for every recognized landmark, in
    /// document order — the same shape `collect` builds, minus the
    /// intermediate `Entry` type so tests can compare with a plain tuple.
    fn outline(xml: &str, dialect: Dialect) -> Vec<(usize, String, usize, usize)> {
        let roots = parse_xml(xml).unwrap().roots;
        let mut entries = Vec::new();
        for root in &roots {
            collect(root, 0, dialect, &mut entries);
        }
        entries
            .into_iter()
            .map(|e| (e.depth, e.label, e.start_line, e.end_line))
            .collect()
    }

    #[test]
    fn special_nests_calls_and_defs_under_their_enclosing_section_through_transparent_wrappers() {
        // `dataProcessing` is not itself a landmark, but `LoadOrder()` still
        // nests one level under `#ProcessOrder` rather than resetting to
        // depth 0 — the whole point of skipping transparent wrappers instead
        // of just dropping them from the tree.
        let xml = r#"<section name="ProcessOrder">
  <method name="Validate" jumpToXmlFile="{v,Validators}" jumpToXPath="//section[@name='ValidateOrder']"/>
  <dataProcessing>
    <builtInMethodParameterList name="LoadOrder">
      <parameter name="id">1</parameter>
    </builtInMethodParameterList>
  </dataProcessing>
</section>
"#;
        assert_eq!(
            outline(xml, Dialect::Special),
            vec![
                (0, "#ProcessOrder".to_string(), 1, 8),
                (
                    1,
                    "Validators::ValidateOrder(name=\"Validate\")".to_string(),
                    2,
                    2
                ),
                (1, "LoadOrder()".to_string(), 4, 6),
            ]
        );
    }

    #[test]
    fn special_section_requires_a_bare_name_attribute() {
        // A `section` with any attribute besides `name` isn't itself a
        // landmark (matches render.rs's `#Name` guard) — but its call child
        // is still found, at depth 0 since the non-landmark section adds no
        // nesting.
        let xml = r#"<section name="Foo" other="x"><method jumpToXmlFile="{v,X}" jumpToXPath="//section[@name='Y']"/></section>"#;
        assert_eq!(
            outline(xml, Dialect::Special),
            vec![(0, "X::Y()".to_string(), 1, 1)]
        );
    }

    #[test]
    fn special_method_without_jump_attributes_is_not_a_landmark() {
        let xml = r#"<method name="justAName"><parameter name="p">1</parameter></method>"#;
        assert_eq!(outline(xml, Dialect::Special), vec![]);
    }

    #[test]
    fn special_sibling_landmarks_stay_flat_when_never_nested_in_a_section() {
        // Mirrors test-input/special-elements.xml: method defs and calls
        // sitting directly under the document root (or behind transparent
        // wrappers) with no enclosing `section`, so every entry is depth 0.
        let xml = r#"<businessWorkflow>
  <builtInMethodParameterList name="foo_XML_Input">
    <parameter name="x">1</parameter>
  </builtInMethodParameterList>
  <dataProcessing>
    <builtInMethodParameterList name="foo_ProcessXML"/>
  </dataProcessing>
</businessWorkflow>
"#;
        let entries = outline(xml, Dialect::Special);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|(depth, ..)| *depth == 0));
    }

    #[test]
    fn render_outline_collapses_equal_start_and_end_to_a_single_line_number() {
        let xml = r#"<method name="X" jumpToXmlFile="{v,F}" jumpToXPath="//section[@name='S']"/>"#;
        let roots = parse_xml(xml).unwrap().roots;
        assert_eq!(
            render_outline(&roots, Dialect::Special),
            "F::S(name=\"X\")  1\n"
        );
    }

    #[test]
    fn xslt_recognizes_function_and_nests_calls_under_their_template() {
        let xml = r#"<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:function name="f:double" as="xs:integer">
    <xsl:sequence select=". * 2"/>
  </xsl:function>
  <xsl:template match="/">
    <xsl:call-template name="go"/>
    <xsl:apply-templates select="items"/>
  </xsl:template>
</xsl:stylesheet>
"#;
        assert_eq!(
            outline(xml, Dialect::Xslt),
            vec![
                (0, "function f:double".to_string(), 2, 4),
                (0, "match /".to_string(), 5, 8),
                (1, "call go".to_string(), 6, 6),
                (1, "apply items".to_string(), 7, 7),
            ]
        );
    }

    #[test]
    fn generic_dialect_falls_back_to_name_or_id_and_still_nests() {
        let xml = r#"<pattern id="model"><assert id="BR-1">x</assert></pattern>"#;
        assert_eq!(
            outline(xml, Dialect::Generic),
            vec![
                (0, "pattern model".to_string(), 1, 1),
                (1, "assert BR-1".to_string(), 1, 1),
            ]
        );
    }
}
