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

/// True if `roots` contain this proprietary business-workflow vocabulary's
/// unambiguous markers (`builtInMethodParameterList`, or a `method` with
/// `jumpToXmlFile`/`jumpToXPath`) — used to pick `Dialect::Special`
/// automatically even without `--special`, since these tag/attribute names
/// are distinctive enough that a false positive is effectively impossible
/// (mirrors how `--auto` already sniffs UBL/CII documents elsewhere).
pub(crate) fn sniff_special(roots: &[XmlElement]) -> bool {
    fn any(element: &XmlElement) -> bool {
        let is_marker = matches!(
            element.name.as_str(),
            "builtInMethodParameterList" | "builtinmethodparameterlist"
        ) || (element.name == "method"
            && element.attributes.contains_key("jumpToXmlFile")
            && element.attributes.contains_key("jumpToXPath"));
        is_marker || element.children.iter().any(any)
    }
    roots.iter().any(any)
}

/// The label for `element` under `dialect`, if it's a recognized landmark.
fn label_for(element: &XmlElement, dialect: Dialect) -> Option<String> {
    match dialect {
        Dialect::Special => match element.name.as_str() {
            "builtInMethodParameterList" | "builtinmethodparameterlist" => builtin_label(element),
            "section" if element.attributes.len() == 1 => {
                let name = element.attributes.get("name")?;
                Some(format!("#{name}"))
            }
            "method" => method_label(element),
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

/// A `method` element's own label: this vocabulary has two flavors. One is a
/// cross-file jump-call (`jumpToXmlFile`/`jumpToXPath`, `name` optional),
/// rendered via `method_call_target`. The other — the far more common one in
/// practice — is just a named local step (`name`, optionally with an
/// `include` guard, no jump attributes at all), which carries no special
/// rendering of its own but is still the human-authored label for what the
/// step does and belongs in the outline. A `method` with neither is skipped
/// (nothing to label it with).
fn method_label(element: &XmlElement) -> Option<String> {
    match (
        element.attributes.get("jumpToXmlFile"),
        element.attributes.get("jumpToXPath"),
    ) {
        (Some(jump_to_xml_file), Some(jump_to_xpath)) => Some(method_call_target(
            jump_to_xml_file,
            jump_to_xpath,
            element.attributes.get("name").map(String::as_str),
        )),
        _ => element.attributes.get("name").cloned(),
    }
}

/// A `builtInMethodParameterList`'s `<parameter name="command">` child, if
/// present — the operation it performs (e.g. `decrypt`, `copy`, or a full SQL
/// statement for `acme_db_functions`). Without this, every call to the same
/// shared builtin looks identical in the outline; with it, the label shows
/// what that specific call site actually does. Long values (SQL in
/// particular) are truncated so one verbose call doesn't blow out the
/// column alignment for every other entry in the file.
fn builtin_command(element: &XmlElement) -> Option<String> {
    let command = element.children.iter().find(|c| {
        c.name == "parameter" && c.attributes.get("name").map(String::as_str) == Some("command")
    })?;
    let text = command.text_content.trim();
    if text.is_empty() {
        return None;
    }
    const MAX: usize = 24;
    if text.chars().count() > MAX {
        Some(format!(
            "{}\u{2026}",
            text.chars().take(MAX).collect::<String>()
        ))
    } else {
        Some(text.to_string())
    }
}

/// A `builtInMethodParameterList`'s own label: its `name` (with a trailing
/// `_functions` stripped — it's on most of these shared builtins and adds
/// nothing, e.g. `acme_file_functions` \u{2192} `acme_file`), plus its `command`
/// parameter chained on with `\u{2192}` when present (`acme_file`, or
/// `acme_file \u{2192} createdir`) — no parens, so a call with no command
/// doesn't end in a bare, meaningless `()`.
fn builtin_label(element: &XmlElement) -> Option<String> {
    let name = element.attributes.get("name")?;
    let name = name.strip_suffix("_functions").unwrap_or(name);
    Some(match builtin_command(element) {
        Some(command) => format!("{name} \u{2192} {command}"),
        None => name.to_string(),
    })
}

/// A `method` (either flavor — see `method_label`) whose entire body is a
/// single `builtInMethodParameterList` is this vocabulary's common "one step,
/// one builtin call" pattern — two structurally nested landmarks describing
/// one call site. Collapsing them onto a single entry (and recursing past the
/// builtin into its own children, in case it ever nests further) cuts that
/// redundant nesting without losing either the method's own label or the
/// builtin invoked. Only applies under `--special`.
fn special_collapsed_call(element: &XmlElement) -> Option<(String, &XmlElement)> {
    if element.name != "method" {
        return None;
    }
    let label = method_label(element)?;
    if element.children.len() != 1 {
        return None;
    }
    let only_child = &element.children[0];
    if !matches!(
        only_child.name.as_str(),
        "builtInMethodParameterList" | "builtinmethodparameterlist"
    ) {
        return None;
    }
    let builtin_label = builtin_label(only_child)?;
    Some((format!("{label} \u{2192} {builtin_label}"), only_child))
}

/// Depth only increases at a recognized landmark; a non-landmark element (a
/// transparent wrapper, e.g. `dataProcessing`) is skipped without changing
/// depth, so its children still nest under the nearest enclosing landmark.
fn collect(element: &XmlElement, depth: usize, dialect: Dialect, out: &mut Vec<Entry>) {
    if matches!(dialect, Dialect::Special)
        && let Some((label, only_child)) = special_collapsed_call(element)
    {
        out.push(Entry {
            depth,
            label,
            start_line: element.start_line,
            end_line: element.end_line,
        });
        for child in &only_child.children {
            collect(child, depth + 1, dialect, out);
        }
        return;
    }

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
                (1, "LoadOrder".to_string(), 4, 6),
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
    fn special_named_method_without_jump_attributes_is_still_a_landmark() {
        // The far more common flavor in practice: a named local step with no
        // jumpToXmlFile/jumpToXPath at all. It has no special rendering of
        // its own, but it's still the human-authored label for what the step
        // does, so it must not be silently dropped.
        let xml = r#"<method name="justAName"><parameter name="p">1</parameter></method>"#;
        assert_eq!(
            outline(xml, Dialect::Special),
            vec![(0, "justAName".to_string(), 1, 1)]
        );
    }

    #[test]
    fn special_method_with_neither_name_nor_jump_attributes_is_not_a_landmark() {
        let xml = r#"<method include="foo"><parameter name="p">1</parameter></method>"#;
        assert_eq!(outline(xml, Dialect::Special), vec![]);
    }

    #[test]
    fn special_collapses_a_method_wrapping_a_single_builtin_onto_one_line() {
        // The common "look up a target, then invoke this one shared builtin"
        // pattern: two structurally nested landmarks describing one call
        // site collapse to a single entry, and the builtin's `parameter`
        // children (not landmarks themselves) are still walked correctly.
        let xml = r#"<method name="splitPONumList" jumpToXmlFile="{v,BuiltInLibrary}" jumpToXPath="//section[@name='StringFunctions']">
  <builtInMethodParameterList name="acme_string_functions">
    <parameter name="operation">split</parameter>
  </builtInMethodParameterList>
</method>
"#;
        assert_eq!(
            outline(xml, Dialect::Special),
            vec![(
                0,
                "BuiltInLibrary::StringFunctions(name=\"splitPONumList\") \u{2192} acme_string"
                    .to_string(),
                1,
                5
            )]
        );
    }

    #[test]
    fn special_does_not_collapse_a_method_with_more_than_one_child() {
        let xml = r#"<method jumpToXmlFile="{v,F}" jumpToXPath="//section[@name='S']">
  <builtInMethodParameterList name="a"/>
  <builtInMethodParameterList name="b"/>
</method>
"#;
        let entries = outline(xml, Dialect::Special);
        assert_eq!(entries.len(), 3); // the call, plus both builtins nested under it
        assert_eq!(entries[0].0, 0);
        assert!(entries[1..].iter().all(|(depth, ..)| *depth == 1));
    }

    #[test]
    fn special_command_element_is_not_a_landmark() {
        // A `<command type="...">` element was tried as a landmark and
        // reverted: in real documents it's used for one-per-node XML-writer
        // primitives (startelement/endelement/write/startattribute/
        // endattribute/dbField/Format/Field) — hundreds of them per file,
        // drowning out the actual landmarks.
        let xml =
            r#"<command type="startelement"><parameter name="element">root</parameter></command>"#;
        assert_eq!(outline(xml, Dialect::Special), vec![]);
    }

    #[test]
    fn builtin_command_parameter_is_folded_into_the_call_label() {
        // Real usage: a `builtInMethodParameterList`'s `<parameter
        // name="command">` child holds the operation it performs (or, for
        // acme_db_functions, a full SQL statement) — without it, every call to
        // the same shared builtin is indistinguishable in the outline.
        let xml = r#"<builtInMethodParameterList name="acme_file_functions">
  <parameter name="filename">a.txt</parameter>
  <parameter name="command">decrypt</parameter>
</builtInMethodParameterList>"#;
        assert_eq!(
            outline(xml, Dialect::Special),
            vec![(0, "acme_file \u{2192} decrypt".to_string(), 1, 4)]
        );
    }

    #[test]
    fn builtin_command_parameter_is_truncated_when_long() {
        let long_sql = "select ".to_string() + &"x".repeat(80);
        let xml = format!(
            r#"<builtInMethodParameterList name="acme_db_functions"><parameter name="command">{long_sql}</parameter></builtInMethodParameterList>"#
        );
        let entries = outline(&xml, Dialect::Special);
        let (_, label, ..) = &entries[0];
        assert!(label.starts_with("acme_db \u{2192} select "));
        assert!(label.ends_with('\u{2026}'));
        assert!(label.len() < long_sql.len());
    }

    #[test]
    fn builtin_label_strips_a_trailing_functions_suffix_but_leaves_other_names_alone() {
        let xml = r#"<root>
  <builtInMethodParameterList name="acme_file_functions"/>
  <builtInMethodParameterList name="acme_join"/>
</root>
"#;
        assert_eq!(
            outline(xml, Dialect::Special),
            vec![
                (0, "acme_file".to_string(), 2, 2),
                (0, "acme_join".to_string(), 3, 3),
            ]
        );
    }

    #[test]
    fn sniff_special_detects_proprietary_markers_without_the_flag() {
        let with_marker =
            parse_xml(r#"<root><builtInMethodParameterList name="x"/></root>"#).unwrap();
        assert!(sniff_special(&with_marker.roots));

        let with_jump_method = parse_xml(
            r#"<root><method jumpToXmlFile="{v,F}" jumpToXPath="//section[@name='S']"/></root>"#,
        )
        .unwrap();
        assert!(sniff_special(&with_jump_method.roots));

        let plain = parse_xml(r#"<root><item name="x"/></root>"#).unwrap();
        assert!(!sniff_special(&plain.roots));
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

    /// The canonical `--special` example: also vendored (verbatim) as
    /// unxml-demos' Outline showcase for this dialect, so the public demo
    /// and this regression test never drift apart. Entirely self-authored —
    /// `--special` is a proprietary vocabulary with no public real-world
    /// corpus to draw a fixture from.
    const WORKFLOW_EXAMPLE: &str = r#"<workflow>
  <section name="ImportOrders">
    <method name="FetchPendingOrders">
      <builtInMethodParameterList name="acme_db_functions">
        <parameter name="connectionstring">{v,connstr}</parameter>
        <parameter name="command">select * from orders where status = 'pending'</parameter>
      </builtInMethodParameterList>
    </method>
    <method name="ArchiveOriginal">
      <builtInMethodParameterList name="acme_file_functions">
        <parameter name="command">copy</parameter>
      </builtInMethodParameterList>
    </method>
  </section>
</workflow>
"#;

    #[test]
    fn workflow_example_is_auto_detected_and_renders_as_expected() {
        let roots = parse_xml(WORKFLOW_EXAMPLE).unwrap().roots;
        assert!(sniff_special(&roots), "no --special flag should be needed");
        assert_eq!(
            render_outline(&roots, Dialect::Special),
            "\
#ImportOrders                                               2-14
  FetchPendingOrders \u{2192} acme_db \u{2192} select * from orders whe…  3-8
  ArchiveOriginal \u{2192} acme_file \u{2192} copy                        9-13
"
        );
    }
}
