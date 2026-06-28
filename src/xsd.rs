//! XML Schema (XSD) dialect: elements, types, content models, facets, and
//! the helpers that inline simple types and fold transparent sequences.

use std::collections::HashMap;

use crate::model::{FormatOpts, NodeRef, XmlElement};
use crate::render::push_comment;
use crate::types::{is_true, xsd_local};
use crate::xslt::TemplateRegistry;

impl XmlElement {
    pub(crate) fn format_xsd_element(
        &self,
        indent: usize,
        indent_str: &str,
        registry: Option<&TemplateRegistry>,
    ) -> Option<String> {
        let local = xsd_local(&self.name);
        let opts = &FormatOpts {
            xsd: true,
            ..FormatOpts::default()
        };
        let occurs = format_occurs(&self.attributes);
        let mut result = String::new();

        match local {
            "schema" => {
                let tns = self
                    .attributes
                    .get("targetNamespace")
                    .map(|s| format!(" {s}"))
                    .unwrap_or_default();
                let mut flags: Vec<String> = Vec::new();
                if self
                    .attributes
                    .get("elementFormDefault")
                    .map(|s| s.as_str())
                    == Some("qualified")
                {
                    flags.push("elementFormDefault=qualified".into());
                }
                if self
                    .attributes
                    .get("attributeFormDefault")
                    .map(|s| s.as_str())
                    == Some("qualified")
                {
                    flags.push("attributeFormDefault=qualified".into());
                }
                let flag_suffix = if flags.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", flags.join(", "))
                };
                result.push_str(&format!("{indent_str}schema{tns}{flag_suffix}\n"));

                // Emit xmlns:* declarations as 'ns prefix = uri' lines so the
                // prefix bindings used elsewhere (ref cbc:Foo, type udt:Bar) are
                // documented. Skip the XSD vocabulary itself (xmlns:xs / xmlns:xsd)
                // which is implied by every XSD.
                let inner_indent = "  ".repeat(indent + 1);
                let mut xmlns_decls: Vec<(String, &String)> = Vec::new();
                for (k, v) in &self.attributes {
                    if let Some(prefix) = k.strip_prefix("xmlns:") {
                        // Skip the XSD vocabulary itself, whatever prefix it is
                        // bound to (xs:/xsd:, or s: in .NET schemas).
                        if prefix == "xs"
                            || prefix == "xsd"
                            || v == "http://www.w3.org/2001/XMLSchema"
                        {
                            continue;
                        }
                        xmlns_decls.push((prefix.to_string(), v));
                    } else if k == "xmlns" {
                        xmlns_decls.push((String::new(), v));
                    }
                }
                xmlns_decls.sort_by(|a, b| a.0.cmp(&b.0));
                for (prefix, uri) in xmlns_decls {
                    if prefix.is_empty() {
                        result.push_str(&format!("{inner_indent}xmlns = {uri}\n"));
                    } else {
                        result.push_str(&format!("{inner_indent}ns {prefix} = {uri}\n"));
                    }
                }

                result.push_str(&self.render_children(indent + 1, opts, registry));
                Some(result)
            }
            "import" => {
                let ns = self.attributes.get("namespace");
                let loc = self.attributes.get("schemaLocation");
                match (ns, loc) {
                    (Some(n), Some(l)) => {
                        result.push_str(&format!("{indent_str}import {n} from {l}\n"))
                    }
                    (Some(n), None) => result.push_str(&format!("{indent_str}import {n}\n")),
                    (None, Some(l)) => result.push_str(&format!("{indent_str}import {l}\n")),
                    (None, None) => result.push_str(&format!("{indent_str}import\n")),
                }
                Some(result)
            }
            "include" | "redefine" => {
                if let Some(loc) = self.attributes.get("schemaLocation") {
                    result.push_str(&format!("{indent_str}{local} {loc}\n"));
                } else {
                    result.push_str(&format!("{indent_str}{local}\n"));
                }
                result.push_str(&self.render_children(indent + 1, opts, registry));
                Some(result)
            }
            "element" => {
                let prefix = if is_true(self.attributes.get("abstract")) {
                    "abstract "
                } else {
                    ""
                };
                let mut tail = String::new();
                if is_true(self.attributes.get("nillable")) {
                    tail.push_str(" nillable");
                }
                if let Some(sg) = self.attributes.get("substitutionGroup") {
                    tail.push_str(&format!(" substitutes {sg}"));
                }
                if let Some(b) = self.attributes.get("block") {
                    tail.push_str(&format!(" block {b}"));
                }
                if let Some(f) = self.attributes.get("final") {
                    tail.push_str(&format!(" final {f}"));
                }
                if let Some(d) = self.attributes.get("default") {
                    tail.push_str(&format!(" = {d}"));
                } else if let Some(f) = self.attributes.get("fixed") {
                    tail.push_str(&format!(" == {f}"));
                }
                if let Some(r) = self.attributes.get("ref") {
                    result.push_str(&format!("{indent_str}{prefix}ref {r}{occurs}{tail}\n"));
                    return Some(result);
                }
                let n = self.attributes.get("name")?;
                if let Some(t) = self.attributes.get("type") {
                    result.push_str(&format!(
                        "{indent_str}{prefix}element {n} : {t}{occurs}{tail}\n"
                    ));
                    return Some(result);
                }
                // Anonymous nested type — try to inline a simpleType.
                let content: Vec<&XmlElement> = self
                    .children
                    .iter()
                    .filter(|c| xsd_local(&c.name) != "annotation")
                    .collect();
                if content.len() == 1
                    && xsd_local(&content[0].name) == "simpleType"
                    && let Some((suffix, body)) = try_inline_simple_type(content[0], indent)
                {
                    result.push_str(&format!(
                        "{indent_str}{prefix}element {n}{suffix}{occurs}{tail}\n"
                    ));
                    result.push_str(&body);
                    return Some(result);
                }
                // Anonymous nested complexType — fold its body directly under
                // the element, dropping the redundant bare `type` line, when the
                // type carries no extra semantics (no name, not mixed/abstract,
                // not a complexContent/simpleContent derivation).
                if content.len() == 1 && xsd_local(&content[0].name) == "complexType" {
                    let ct = content[0];
                    let ct_structural: Vec<&XmlElement> = ct
                        .children
                        .iter()
                        .filter(|c| xsd_local(&c.name) != "annotation")
                        .collect();
                    let is_derivation = ct_structural.len() == 1
                        && matches!(
                            xsd_local(&ct_structural[0].name),
                            "complexContent" | "simpleContent"
                        );
                    let plain = !ct.attributes.contains_key("name")
                        && !is_true(ct.attributes.get("abstract"))
                        && !is_true(ct.attributes.get("mixed"))
                        && !ct.attributes.contains_key("block")
                        && !ct.attributes.contains_key("final")
                        && !is_derivation;
                    if plain {
                        result
                            .push_str(&format!("{indent_str}{prefix}element {n}{occurs}{tail}\n"));
                        let inner_indent = "  ".repeat(indent + 1);
                        for ct_child in &ct.children {
                            emit_complextype_child(
                                ct_child,
                                indent + 1,
                                &inner_indent,
                                opts,
                                registry,
                                &mut result,
                            );
                        }
                        return Some(result);
                    }
                }
                result.push_str(&format!("{indent_str}{prefix}element {n}{occurs}{tail}\n"));
                result.push_str(&self.render_children(indent + 1, opts, registry));
                Some(result)
            }
            "attribute" => {
                let suffix = match (
                    self.attributes.get("use").map(|s| s.as_str()),
                    self.attributes.get("default"),
                    self.attributes.get("fixed"),
                ) {
                    (Some("required"), _, _) => " (required)".to_string(),
                    (Some("prohibited"), _, _) => " (prohibited)".to_string(),
                    (_, Some(d), _) => format!(" = {d}"),
                    (_, _, Some(f)) => format!(" == {f}"),
                    _ => String::new(),
                };
                if let Some(r) = self.attributes.get("ref") {
                    result.push_str(&format!("{indent_str}@ref {r}{suffix}\n"));
                    result.push_str(&self.render_children(indent + 1, opts, registry));
                    return Some(result);
                }
                let n = self.attributes.get("name")?;
                if let Some(t) = self.attributes.get("type") {
                    result.push_str(&format!("{indent_str}@{n} : {t}{suffix}\n"));
                    result.push_str(&self.render_children(indent + 1, opts, registry));
                    return Some(result);
                }
                // Anonymous nested type — inline a simpleType the same way
                // elements do, so an attribute restriction reads as
                // `@name : base (required)` rather than a nested bare `type`.
                let content: Vec<&XmlElement> = self
                    .children
                    .iter()
                    .filter(|c| xsd_local(&c.name) != "annotation")
                    .collect();
                if content.len() == 1
                    && xsd_local(&content[0].name) == "simpleType"
                    && let Some((type_suffix, body)) = try_inline_simple_type(content[0], indent)
                {
                    result.push_str(&format!("{indent_str}@{n}{type_suffix}{suffix}\n"));
                    result.push_str(&body);
                    for child in &self.children {
                        if xsd_local(&child.name) == "annotation" {
                            result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                        }
                    }
                    return Some(result);
                }
                result.push_str(&format!("{indent_str}@{n}{suffix}\n"));
                result.push_str(&self.render_children(indent + 1, opts, registry));
                Some(result)
            }
            "complexType" => {
                let prefix = if is_true(self.attributes.get("abstract")) {
                    "abstract "
                } else {
                    ""
                };
                let mut tail = String::new();
                if is_true(self.attributes.get("mixed")) {
                    tail.push_str(" mixed");
                }
                if let Some(b) = self.attributes.get("block") {
                    tail.push_str(&format!(" block {b}"));
                }
                if let Some(f) = self.attributes.get("final") {
                    tail.push_str(&format!(" final {f}"));
                }

                // Detect "type X extends Y" / "type X restricts Y" pattern:
                // a single complexContent/simpleContent wrapping a single extension/restriction.
                let structural: Vec<&XmlElement> = self
                    .children
                    .iter()
                    .filter(|c| xsd_local(&c.name) != "annotation")
                    .collect();
                let mut header_extension: Option<(&str, &str, &XmlElement)> = None;
                if structural.len() == 1 {
                    let content = structural[0];
                    let cl = xsd_local(&content.name);
                    if cl == "complexContent" || cl == "simpleContent" {
                        let inner: Vec<&XmlElement> = content
                            .children
                            .iter()
                            .filter(|c| xsd_local(&c.name) != "annotation")
                            .collect();
                        if inner.len() == 1 {
                            let der = inner[0];
                            let dl = xsd_local(&der.name);
                            if (dl == "extension" || dl == "restriction")
                                && let Some(base) = der.attributes.get("base")
                            {
                                let kw = if dl == "extension" {
                                    "extends"
                                } else {
                                    "restricts"
                                };
                                header_extension = Some((kw, base, der));
                            }
                        }
                    }
                }

                let name_part = self
                    .attributes
                    .get("name")
                    .map(|n| format!(" {n}"))
                    .unwrap_or_default();

                if let Some((kw, base, der)) = header_extension {
                    result.push_str(&format!(
                        "{indent_str}{prefix}type{name_part} {kw} {base}{tail}\n"
                    ));
                    emit_complextype_body(der, indent + 1, opts, registry, &mut result);
                } else {
                    result.push_str(&format!("{indent_str}{prefix}type{name_part}{tail}\n"));
                    emit_complextype_body(self, indent + 1, opts, registry, &mut result);
                }
                Some(result)
            }
            "simpleType" => {
                let name_part = self
                    .attributes
                    .get("name")
                    .map(|n| format!(" {n}"))
                    .unwrap_or_default();
                if let Some((suffix, body)) = try_inline_simple_type(self, indent) {
                    result.push_str(&format!("{indent_str}type{name_part}{suffix}\n"));
                    result.push_str(&body);
                } else {
                    result.push_str(&format!("{indent_str}type{name_part}\n"));
                    result.push_str(&self.render_children(indent + 1, opts, registry));
                }
                Some(result)
            }
            "sequence" | "choice" | "all" => {
                result.push_str(&format!("{indent_str}{local}{occurs}\n"));
                let inner_indent = "  ".repeat(indent + 1);
                for child in &self.children {
                    if let Some(s) = child.format_xsd_member(indent + 1, &inner_indent, registry) {
                        result.push_str(&s);
                    } else {
                        result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                    }
                }
                Some(result)
            }
            "restriction" | "extension" => {
                if let Some(b) = self.attributes.get("base") {
                    result.push_str(&format!("{indent_str}{local} {b}\n"));
                } else {
                    result.push_str(&format!("{indent_str}{local}\n"));
                }
                result.push_str(&self.render_children(indent + 1, opts, registry));
                Some(result)
            }
            "complexContent" | "simpleContent" => {
                result.push_str(&format!("{indent_str}{local}\n"));
                result.push_str(&self.render_children(indent + 1, opts, registry));
                Some(result)
            }
            "enumeration" => {
                if let Some(v) = self.attributes.get("value") {
                    result.push_str(&format!("{indent_str}| {v}\n"));
                } else {
                    result.push_str(&format!("{indent_str}|\n"));
                }
                result.push_str(&self.render_children(indent + 1, opts, registry));
                Some(result)
            }
            "pattern" | "minLength" | "maxLength" | "length" | "minInclusive" | "maxInclusive"
            | "minExclusive" | "maxExclusive" | "totalDigits" | "fractionDigits" | "whiteSpace" => {
                if let Some(v) = self.attributes.get("value") {
                    result.push_str(&format!("{indent_str}{local} {v}\n"));
                } else {
                    result.push_str(&format!("{indent_str}{local}\n"));
                }
                result.push_str(&self.render_children(indent + 1, opts, registry));
                Some(result)
            }
            "annotation" => {
                result.push_str(&self.render_children(indent, opts, registry));
                Some(result)
            }
            "documentation" | "appinfo" => {
                let text = self.text_content.trim();
                if !text.is_empty() {
                    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    result.push_str(&format!("{indent_str}// {clean}\n"));
                } else {
                    // UBL/CCTS-style: prose is buried in nested elements like
                    // <ccts:Definition>...</ccts:Definition>. Pull those out.
                    let mut prose = Vec::new();
                    extract_doc_prose(self, &mut prose);
                    for line in prose {
                        result.push_str(&format!("{indent_str}// {line}\n"));
                    }
                }
                Some(result)
            }
            "group" | "attributeGroup" => {
                if let Some(r) = self.attributes.get("ref") {
                    result.push_str(&format!("{indent_str}{local} ref {r}{occurs}\n"));
                } else if let Some(n) = self.attributes.get("name") {
                    result.push_str(&format!("{indent_str}{local} {n}\n"));
                } else {
                    return None;
                }
                let inner_indent = "  ".repeat(indent + 1);
                for child in &self.children {
                    emit_complextype_child(
                        child,
                        indent + 1,
                        &inner_indent,
                        opts,
                        registry,
                        &mut result,
                    );
                }
                Some(result)
            }
            "union" => {
                if let Some(m) = self.attributes.get("memberTypes") {
                    result.push_str(&format!("{indent_str}union {m}\n"));
                } else {
                    result.push_str(&format!("{indent_str}union\n"));
                }
                result.push_str(&self.render_children(indent + 1, opts, registry));
                Some(result)
            }
            "list" => {
                if let Some(t) = self.attributes.get("itemType") {
                    result.push_str(&format!("{indent_str}list {t}\n"));
                } else {
                    result.push_str(&format!("{indent_str}list\n"));
                }
                result.push_str(&self.render_children(indent + 1, opts, registry));
                Some(result)
            }
            "any" => {
                let ns = self
                    .attributes
                    .get("namespace")
                    .map(|s| format!(" {s}"))
                    .unwrap_or_default();
                let pc = match self.attributes.get("processContents").map(|s| s.as_str()) {
                    Some("skip") => " (skip)",
                    Some("lax") => " (lax)",
                    _ => "",
                };
                result.push_str(&format!("{indent_str}any{ns}{occurs}{pc}\n"));
                Some(result)
            }
            "anyAttribute" => {
                let ns = self
                    .attributes
                    .get("namespace")
                    .map(|s| format!(" {s}"))
                    .unwrap_or_default();
                let pc = match self.attributes.get("processContents").map(|s| s.as_str()) {
                    Some("skip") => " (skip)",
                    Some("lax") => " (lax)",
                    _ => "",
                };
                result.push_str(&format!("{indent_str}@any{ns}{pc}\n"));
                Some(result)
            }
            "key" | "keyref" | "unique" => {
                if let Some(n) = self.attributes.get("name") {
                    result.push_str(&format!("{indent_str}{local} {n}\n"));
                } else {
                    result.push_str(&format!("{indent_str}{local}\n"));
                }
                result.push_str(&self.render_children(indent + 1, opts, registry));
                Some(result)
            }
            "selector" | "field" => {
                if let Some(x) = self.attributes.get("xpath") {
                    result.push_str(&format!("{indent_str}{local} {x}\n"));
                    Some(result)
                } else {
                    None
                }
            }
            "notation" => {
                if let Some(n) = self.attributes.get("name") {
                    result.push_str(&format!("{indent_str}notation {n}\n"));
                } else {
                    result.push_str(&format!("{indent_str}notation\n"));
                }
                Some(result)
            }
            _ => None,
        }
    }

    /// Format this element as a member of a content model (sequence/choice/all,
    /// or the implicit body of a complexType). Drops the leading `element`
    /// keyword on element declarations since the context makes it obvious.
    /// Returns None if the caller should fall back to format_yaml_like.
    pub(crate) fn format_xsd_member(
        &self,
        indent: usize,
        indent_str: &str,
        registry: Option<&TemplateRegistry>,
    ) -> Option<String> {
        if xsd_local(&self.name) != "element" {
            return None;
        }
        let opts = &FormatOpts {
            xsd: true,
            ..FormatOpts::default()
        };
        let occurs = format_occurs(&self.attributes);
        let prefix = if is_true(self.attributes.get("abstract")) {
            "abstract "
        } else {
            ""
        };
        let mut tail = String::new();
        if is_true(self.attributes.get("nillable")) {
            tail.push_str(" nillable");
        }
        if let Some(sg) = self.attributes.get("substitutionGroup") {
            tail.push_str(&format!(" substitutes {sg}"));
        }
        if let Some(b) = self.attributes.get("block") {
            tail.push_str(&format!(" block {b}"));
        }
        if let Some(f) = self.attributes.get("final") {
            tail.push_str(&format!(" final {f}"));
        }
        if let Some(d) = self.attributes.get("default") {
            tail.push_str(&format!(" = {d}"));
        } else if let Some(f) = self.attributes.get("fixed") {
            tail.push_str(&format!(" == {f}"));
        }
        let mut result = String::new();

        if let Some(r) = self.attributes.get("ref") {
            result.push_str(&format!("{indent_str}{prefix}ref {r}{occurs}{tail}\n"));
            // Emit annotation/documentation children indented under the ref line.
            for child in &self.children {
                if xsd_local(&child.name) == "annotation" {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
            }
            return Some(result);
        }
        let n = self.attributes.get("name")?;

        // If there's an explicit type, emit one-liner with any annotations below.
        if let Some(t) = self.attributes.get("type") {
            result.push_str(&format!("{indent_str}{prefix}{n} : {t}{occurs}{tail}\n"));
            for child in &self.children {
                if xsd_local(&child.name) == "annotation" {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
            }
            return Some(result);
        }

        // Anonymous nested type — try to inline a simpleType.
        let content: Vec<&XmlElement> = self
            .children
            .iter()
            .filter(|c| xsd_local(&c.name) != "annotation")
            .collect();
        if content.len() == 1
            && xsd_local(&content[0].name) == "simpleType"
            && let Some((suffix, body)) = try_inline_simple_type(content[0], indent)
        {
            result.push_str(&format!("{indent_str}{prefix}{n}{suffix}{occurs}{tail}\n"));
            result.push_str(&body);
            return Some(result);
        }

        // Fall back: emit name and recurse for nested type/complex content.
        result.push_str(&format!("{indent_str}{prefix}{n}{occurs}{tail}\n"));
        result.push_str(&self.render_children(indent + 1, opts, registry));
        Some(result)
    }
}

/// Walk an xsd:documentation subtree and collect prose text from elements
/// whose local name is "Definition" or "Description" (CCTS convention used
/// by UBL: <ccts:Definition>A class to define ...</ccts:Definition>).
pub(crate) fn extract_doc_prose(elem: &XmlElement, out: &mut Vec<String>) {
    let local = elem.name.rsplit(':').next().unwrap_or(&elem.name);
    if matches!(local, "Definition" | "Description") {
        let text = elem.text_content.trim();
        if !text.is_empty() {
            let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
            out.push(clean);
            return;
        }
    }
    for child in &elem.children {
        extract_doc_prose(child, out);
    }
}

/// Emit a complexType (or extension/restriction) body in document order,
/// interleaving any XML comments with the structural children. Falls back to
/// the plain child list for synthetic elements whose `nodes` is empty.
pub(crate) fn emit_complextype_body(
    container: &XmlElement,
    indent: usize,
    opts: &FormatOpts,
    registry: Option<&TemplateRegistry>,
    out: &mut String,
) {
    let indent_str = "  ".repeat(indent);
    if container
        .nodes
        .iter()
        .any(|n| matches!(n, NodeRef::Comment { .. }))
    {
        for node in &container.nodes {
            match node {
                NodeRef::Comment { text, inline } => push_comment(out, text, *inline, indent),
                NodeRef::Child(i) => emit_complextype_child(
                    &container.children[*i],
                    indent,
                    &indent_str,
                    opts,
                    registry,
                    out,
                ),
                NodeRef::Text(_) => {}
            }
        }
    } else {
        for child in &container.children {
            emit_complextype_child(child, indent, &indent_str, opts, registry, out);
        }
    }
}

/// Inside a complexType (or extension/restriction body), emit a child:
///   - a transparent xs:sequence (no occurs constraints) is folded; its members are
///     emitted at this level via format_xsd_member (which drops the 'element' keyword)
///   - everything else is emitted via standard format_yaml_like
pub(crate) fn emit_complextype_child(
    child: &XmlElement,
    indent: usize,
    indent_str: &str,
    opts: &FormatOpts,
    registry: Option<&TemplateRegistry>,
    out: &mut String,
) {
    let cl = xsd_local(&child.name);
    let is_transparent = cl == "sequence"
        && !child.attributes.contains_key("minOccurs")
        && !child.attributes.contains_key("maxOccurs");
    if is_transparent {
        // Walk in document order so comments inside the sequence interleave with
        // the hoisted members; `format_xsd_member` drops the `element` keyword.
        for node in &child.nodes {
            match node {
                NodeRef::Comment { text, inline } => push_comment(out, text, *inline, indent),
                NodeRef::Child(i) => {
                    let grandchild = &child.children[*i];
                    if let Some(s) = grandchild.format_xsd_member(indent, indent_str, registry) {
                        out.push_str(&s);
                    } else {
                        out.push_str(&grandchild.format_yaml_like(indent, opts, registry));
                    }
                }
                NodeRef::Text(_) => {}
            }
        }
    } else if let Some(s) = child.format_xsd_member(indent, indent_str, registry) {
        out.push_str(&s);
    } else {
        out.push_str(&child.format_yaml_like(indent, opts, registry));
    }
}

/// Try to render a <simpleType> compactly. Returns (header_suffix, body) where
/// header_suffix is appended after `type [name]` (e.g. " : list xs:int"), and
/// body is the indented block below (e.g. enum lines).
pub(crate) fn try_inline_simple_type(elem: &XmlElement, indent: usize) -> Option<(String, String)> {
    let content: Vec<&XmlElement> = elem
        .children
        .iter()
        .filter(|c| xsd_local(&c.name) != "annotation")
        .collect();
    if content.len() != 1 {
        return None;
    }
    let child = content[0];
    let inner_indent = "  ".repeat(indent + 1);

    match xsd_local(&child.name) {
        "list" => child
            .attributes
            .get("itemType")
            .map(|t| (format!(" : list {t}"), String::new())),
        "union" => child
            .attributes
            .get("memberTypes")
            .map(|m| (format!(" : union {m}"), String::new())),
        "restriction" => {
            let base = child.attributes.get("base")?;
            let mut enums: Vec<&String> = Vec::new();
            let mut patterns: Vec<&String> = Vec::new();
            let mut min_inc: Option<&String> = None;
            let mut max_inc: Option<&String> = None;
            let mut other = false;
            for facet in &child.children {
                let fl = xsd_local(&facet.name);
                let value = facet.attributes.get("value");
                match (fl, value) {
                    ("enumeration", Some(v)) => enums.push(v),
                    ("pattern", Some(v)) => patterns.push(v),
                    ("minInclusive", Some(v)) => min_inc = Some(v),
                    ("maxInclusive", Some(v)) => max_inc = Some(v),
                    ("annotation", _) => {}
                    _ => other = true,
                }
            }
            if other {
                return None;
            }

            // Range only (no enums, no patterns)
            if enums.is_empty()
                && patterns.is_empty()
                && let (Some(m), Some(n)) = (min_inc, max_inc)
            {
                return Some((format!(" : {base} [{m}..{n}]"), String::new()));
            }

            // Enumerations (and optional patterns), no range facets
            if !enums.is_empty() && min_inc.is_none() && max_inc.is_none() {
                let mut body = String::new();
                for v in &enums {
                    body.push_str(&format!("{inner_indent}| {v}\n"));
                }
                for p in &patterns {
                    body.push_str(&format!("{inner_indent}pattern {p}\n"));
                }
                return Some((format!(" : {base}"), body));
            }

            None
        }
        _ => None,
    }
}

pub(crate) fn format_occurs(attrs: &HashMap<String, String>) -> String {
    let min = attrs.get("minOccurs").map(|s| s.as_str());
    let max = attrs.get("maxOccurs").map(|s| s.as_str());
    match (min, max) {
        (None, None) => String::new(),
        (Some("1"), Some("1")) => String::new(),
        (Some("0"), Some("1")) | (Some("0"), None) => " ?".to_string(),
        (Some("0"), Some("unbounded")) => " *".to_string(),
        (Some("1"), Some("unbounded")) | (None, Some("unbounded")) => " +".to_string(),
        (Some(m), Some(n)) => format!(" [{m}..{n}]"),
        (Some(m), None) => format!(" [{m}..1]"),
        (None, Some(n)) => format!(" [1..{n}]"),
    }
}
