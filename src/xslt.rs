//! XSLT dialect: instruction rendering, function/template signature folding,
//! and the import-following template registry used by `--expand`.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::model::{FormatOpts, NodeRef, XmlElement};
use crate::parse::{parse_xml, read_file_lenient};
use crate::render::{WRAP_WIDTH, current_col, render_attrs};
use crate::types::simplify_type;

impl XmlElement {
    /// The inline core of an `xsl:param` / `xsl:variable` / `xsl:with-param`:
    /// `name`, `name as T`, `name := v`, or `name as T := v`. The `as` type is
    /// carried through (XSLT's own keyword). Returns `None` when the value is a
    /// complex element-content default that cannot sit on one line and must
    /// nest beneath the binding instead.
    pub(crate) fn binding_signature(&self) -> Option<String> {
        let name = self.attributes.get("name")?;
        let mut sig = name.clone();
        if let Some(t) = self.attributes.get("as") {
            sig.push_str(&format!(" as {}", simplify_type(t)));
        }
        if let Some(select) = self.attributes.get("select") {
            sig.push_str(&format!(" := {select}"));
        } else if !self.children.is_empty() {
            return None; // element-content default → caller nests it
        } else if !self.text_content.trim().is_empty() {
            sig.push_str(&format!(" := {}", self.text_content.trim()));
        }
        Some(sig)
    }

    /// `name`, or `name as T`, ignoring any value — used as the header stub when
    /// a binding's complex default is rendered nested beneath it.
    pub(crate) fn typed_name(&self) -> Option<String> {
        let name = self.attributes.get("name")?;
        Some(match self.attributes.get("as") {
            Some(t) => format!("{name} as {}", simplify_type(t)),
            None => name.clone(),
        })
    }

    /// Fold the leading run of `xsl:param` children into signature tokens for a
    /// `function` / `template` header (e.g. `["date", "format := 'Y'"]`). An
    /// empty vec means there are no leading params (caller omits the parens);
    /// `None` means a leading param has a complex default and the run can't be
    /// inlined, so the caller should render the params per-line instead.
    pub(crate) fn fold_param_signature(&self) -> Option<Vec<String>> {
        let mut tokens = Vec::new();
        for node in &self.nodes {
            match node {
                NodeRef::Text(t) if t.trim().is_empty() => continue,
                NodeRef::Text(_) => break,
                NodeRef::Child(i) => {
                    let child = &self.children[*i];
                    if child.name != "xsl:param" {
                        break;
                    }
                    tokens.push(child.binding_signature()?);
                }
            }
        }
        Some(tokens)
    }

    /// Like `render_mixed_body`, but skip the leading run of `xsl:param`
    /// children (and surrounding whitespace) — used once their signatures have
    /// been folded into a `function` / `template` header.
    pub(crate) fn render_body_skipping_leading_params(
        &self,
        indent: usize,
        opts: &FormatOpts,
        registry: Option<&TemplateRegistry>,
    ) -> String {
        let ind = "  ".repeat(indent);
        let mut out = String::new();
        let mut leading = true;
        for node in &self.nodes {
            match node {
                NodeRef::Text(text) => {
                    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    if text.is_empty() {
                        continue;
                    }
                    leading = false;
                    out.push_str(&format!("{ind}\"{text}\"\n"));
                }
                NodeRef::Child(i) => {
                    let child = &self.children[*i];
                    if leading && child.name == "xsl:param" {
                        continue;
                    }
                    leading = false;
                    out.push_str(&child.format_yaml_like(indent, opts, registry));
                }
            }
        }
        out
    }

    /// Append the signature tail of a `function`/`template`/`match` header to
    /// `result`: the folded `(params)`, a `-> T` return type, then any leftover
    /// attributes (those not in `skip`, and not `as`) in parentheses. Returns
    /// `true` when the leading params were folded, so the caller knows to skip
    /// them while rendering the body.
    pub(crate) fn push_signature_tail(
        &self,
        result: &mut String,
        indent: usize,
        skip: &[&str],
    ) -> bool {
        let tokens = self.fold_param_signature();
        // Fold the params onto the header only when they all inline *and* the
        // resulting line fits — otherwise fall back to per-line `param` lines so
        // a wide signature doesn't run off the edge.
        // " -> T", using the simplified type so the width check matches output.
        let ret_len = self
            .attributes
            .get("as")
            .map_or(0, |t| simplify_type(t).len() + 4);
        let fold = match &tokens {
            Some(toks) if !toks.is_empty() => {
                let params = format!("({})", toks.join(", "));
                // +1 (folded into the `<`) leaves room for the trailing colon.
                current_col(result) + params.len() + ret_len < WRAP_WIDTH
            }
            _ => false,
        };
        if fold && let Some(toks) = &tokens {
            result.push_str(&format!("({})", toks.join(", ")));
        }
        if let Some(t) = self.attributes.get("as") {
            result.push_str(&format!(" -> {}", simplify_type(t)));
        }
        let extra: Vec<_> = self
            .attributes
            .iter()
            .filter(|(k, _)| *k != "as" && !skip.contains(&k.as_str()))
            .collect();
        if !extra.is_empty() {
            let mut sorted: Vec<_> = extra.into_iter().collect();
            sorted.sort_by_key(|(k, _)| *k);
            let attr_str: Vec<String> =
                sorted.iter().map(|(k, v)| format!("{k}=\"{v}\"")).collect();
            let col = current_col(result);
            result.push_str(&render_attrs(&attr_str, col, indent, true));
        }
        fold
    }

    pub(crate) fn format_xslt_element(
        &self,
        indent: usize,
        indent_str: &str,
        registry: Option<&TemplateRegistry>,
    ) -> Option<String> {
        let mut result = String::new();

        match self.name.as_str() {
            "xsl:template" => {
                // xsl:template(match="X") → match X   (declarative rule)
                // xsl:template(name="X")  → template X (named def, invoked via `call`)
                // Leading xsl:param children fold into a (signature); a 3.0
                // `as` type becomes a `-> T` result annotation.
                if let Some(match_val) = self.attributes.get("match") {
                    result.push_str(&format!("{indent_str}match {match_val}"));
                } else if let Some(name_val) = self.attributes.get("name") {
                    result.push_str(&format!("{indent_str}template {name_val}"));
                } else {
                    return None;
                }
                let folded = self.push_signature_tail(&mut result, indent, &["match", "name"]);
                result.push_str(":\n");
                result.push_str(&if folded {
                    self.render_body_skipping_leading_params(
                        indent + 1,
                        &FormatOpts::XSLT,
                        registry,
                    )
                } else {
                    self.render_mixed_body(indent + 1, &FormatOpts::XSLT, registry)
                });
                Some(result)
            }
            "xsl:apply-templates" => {
                // xsl:apply-templates(select="X") → apply X
                // With --expand, inline the matching template if found
                if let Some(select) = self.attributes.get("select") {
                    // Check if we should expand this template
                    if let Some(reg) = registry
                        && let Some(template) = reg.get(select)
                    {
                        // Expand: add comment and inline template content
                        result.push_str(&format!("{indent_str}# [expanded: apply {select}]\n"));
                        for child in &template.children {
                            result.push_str(&child.format_yaml_like(
                                indent,
                                &FormatOpts::XSLT,
                                Some(reg),
                            ));
                        }
                        return Some(result);
                    }
                    // No expansion, just output apply
                    result.push_str(&format!("{indent_str}apply {select}\n"));
                } else {
                    result.push_str(&format!("{indent_str}apply\n"));
                }
                Some(result)
            }
            "xsl:value-of" => {
                // xsl:value-of(select="X") → <- X
                // A text-only default/fallback body renders inline as a null-
                // coalesce (`<- X ?? "text"`); a body with element content (or
                // an XSLT-2.0+ sequence constructor used instead of select) is
                // rendered nested beneath instead.
                let has_body = !self.nodes.is_empty();
                if let Some(select) = self.attributes.get("select") {
                    // A text-only default (no child elements) collapses inline.
                    if self.children.is_empty() {
                        let text = self
                            .text_content
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ");
                        if !text.is_empty() {
                            result.push_str(&format!("{indent_str}<- {select} ?? \"{text}\"\n"));
                            return Some(result);
                        }
                    }
                    result.push_str(&format!("{indent_str}<- {select}\n"));
                } else if has_body {
                    result.push_str(&format!("{indent_str}<-\n"));
                } else {
                    return None;
                }
                result.push_str(&self.render_mixed_body(indent + 1, &FormatOpts::XSLT, registry));
                Some(result)
            }
            "xsl:copy-of" => {
                // xsl:copy-of(select="X") → copy X
                if let Some(select) = self.attributes.get("select") {
                    result.push_str(&format!("{indent_str}copy {select}\n"));
                    Some(result)
                } else {
                    None
                }
            }
            "xsl:if" => {
                // xsl:if(test="X") → if X
                if let Some(test) = self.attributes.get("test") {
                    result.push_str(&format!("{indent_str}if {test}:\n"));
                    result.push_str(&self.render_mixed_body(
                        indent + 1,
                        &FormatOpts::XSLT,
                        registry,
                    ));
                    Some(result)
                } else {
                    None
                }
            }
            "xsl:choose" => {
                // xsl:choose stays as choose but children get transformed
                result.push_str(&format!("{indent_str}choose:\n"));
                result.push_str(&self.render_mixed_body(indent + 1, &FormatOpts::XSLT, registry));
                Some(result)
            }
            "xsl:when" => {
                // xsl:when(test="X") → when X
                if let Some(test) = self.attributes.get("test") {
                    result.push_str(&format!("{indent_str}when {test}:\n"));
                    result.push_str(&self.render_mixed_body(
                        indent + 1,
                        &FormatOpts::XSLT,
                        registry,
                    ));
                    Some(result)
                } else {
                    None
                }
            }
            "xsl:otherwise" => {
                // xsl:otherwise → else
                result.push_str(&format!("{indent_str}else:\n"));
                result.push_str(&self.render_mixed_body(indent + 1, &FormatOpts::XSLT, registry));
                Some(result)
            }
            "xsl:variable" | "xsl:with-param" => {
                // Both render as a binding: `x := …`, `x as T := …`, or — when
                // the value is element content — `x :=` with the value nested.
                let name = self.attributes.get("name")?;
                if let Some(select) = self.attributes.get("select") {
                    let typed = self.typed_name().unwrap_or_else(|| name.clone());
                    result.push_str(&format!("{indent_str}{typed} := {select}\n"));
                } else if self.children.is_empty() && !self.text_content.trim().is_empty() {
                    let typed = self.typed_name().unwrap_or_else(|| name.clone());
                    result.push_str(&format!(
                        "{indent_str}{typed} := {}\n",
                        self.text_content.trim()
                    ));
                } else {
                    let typed = self.typed_name().unwrap_or_else(|| name.clone());
                    result.push_str(&format!("{indent_str}{typed} :=\n"));
                    for child in &self.children {
                        result.push_str(&child.format_yaml_like(
                            indent + 1,
                            &FormatOpts::XSLT,
                            registry,
                        ));
                    }
                }
                Some(result)
            }
            "xsl:call-template" => {
                // xsl:call-template(name="X") → call X
                if let Some(name) = self.attributes.get("name") {
                    let colon = if self.has_renderable_body() { ":" } else { "" };
                    result.push_str(&format!("{indent_str}call {name}{colon}\n"));
                    result.push_str(&self.render_mixed_body(
                        indent + 1,
                        &FormatOpts::XSLT,
                        registry,
                    ));
                    Some(result)
                } else {
                    None
                }
            }
            "xsl:for-each" => {
                // xsl:for-each(select="X") → foreach X
                if let Some(select) = self.attributes.get("select") {
                    result.push_str(&format!("{indent_str}foreach {select}:\n"));
                    result.push_str(&self.render_mixed_body(
                        indent + 1,
                        &FormatOpts::XSLT,
                        registry,
                    ));
                    Some(result)
                } else {
                    None
                }
            }
            "xsl:text" => {
                // xsl:text → just the text content
                if !self.text_content.trim().is_empty() {
                    result.push_str(&format!("{indent_str}\"{}\"", self.text_content.trim()));
                    result.push('\n');
                    Some(result)
                } else {
                    Some(String::new()) // Empty xsl:text, skip it
                }
            }
            "xsl:element" => {
                // xsl:element(name="X") → element X
                if let Some(name) = self.attributes.get("name") {
                    let colon = if self.has_renderable_body() { ":" } else { "" };
                    result.push_str(&format!("{indent_str}element {name}{colon}\n"));
                    result.push_str(&self.render_mixed_body(
                        indent + 1,
                        &FormatOpts::XSLT,
                        registry,
                    ));
                    Some(result)
                } else {
                    None
                }
            }
            "xsl:attribute" => {
                // xsl:attribute(name="X") → @X = ...
                if let Some(name) = self.attributes.get("name") {
                    if !self.text_content.trim().is_empty() {
                        result.push_str(&format!(
                            "{indent_str}@{name} = {}\n",
                            self.text_content.trim()
                        ));
                    } else if !self.children.is_empty() {
                        result.push_str(&format!("{indent_str}@{name}\n"));
                        for child in &self.children {
                            result.push_str(&child.format_yaml_like(
                                indent + 1,
                                &FormatOpts::XSLT,
                                registry,
                            ));
                        }
                    } else {
                        result.push_str(&format!("{indent_str}@{name}\n"));
                    }
                    Some(result)
                } else {
                    None
                }
            }
            "xsl:param" => {
                // xsl:param → `param x`, `param x as T`, `param x := default`.
                // (Leading params of a function/template are folded into its
                // signature instead; this renders any that aren't.)
                match self.binding_signature() {
                    Some(sig) => {
                        result.push_str(&format!("{indent_str}param {sig}\n"));
                    }
                    None => {
                        // Element-content default — nest it beneath the param.
                        let typed = self.typed_name()?;
                        result.push_str(&format!("{indent_str}param {typed} :=\n"));
                        for child in &self.children {
                            result.push_str(&child.format_yaml_like(
                                indent + 1,
                                &FormatOpts::XSLT,
                                registry,
                            ));
                        }
                    }
                }
                Some(result)
            }
            "xsl:sequence" => {
                // xsl:sequence(select="X") → <-- X. The doubled arrow mirrors
                // value-of's `<-` but marks a *sequence* result (nodes/values),
                // not an atomized string. A bodied sequence constructor nests.
                if let Some(select) = self.attributes.get("select") {
                    result.push_str(&format!("{indent_str}<-- {select}\n"));
                    Some(result)
                } else if !self.nodes.is_empty() {
                    result.push_str(&format!("{indent_str}<--\n"));
                    result.push_str(&self.render_mixed_body(
                        indent + 1,
                        &FormatOpts::XSLT,
                        registry,
                    ));
                    Some(result)
                } else {
                    None
                }
            }
            "xsl:function" => {
                // xsl:function(name="f", as="T") → function f(params) -> T:
                // Leading params fold into the signature (with their `as` types);
                // a wide signature falls back to per-line params. Any other
                // attribute (e.g. visibility) stays in parens.
                if let Some(name) = self.attributes.get("name") {
                    result.push_str(&format!("{indent_str}function {name}"));
                    let folded = self.push_signature_tail(&mut result, indent, &["name"]);
                    result.push_str(":\n");
                    result.push_str(&if folded {
                        self.render_body_skipping_leading_params(
                            indent + 1,
                            &FormatOpts::XSLT,
                            registry,
                        )
                    } else {
                        self.render_mixed_body(indent + 1, &FormatOpts::XSLT, registry)
                    });
                    Some(result)
                } else {
                    None
                }
            }
            "xsl:next-match" | "xsl:apply-imports" => {
                // Re-dispatch instructions: bare keyword, taking a colon only
                // when they carry a body (with-param / fallback).
                let kw = self.name.strip_prefix("xsl:").unwrap_or(&self.name);
                let colon = if self.has_renderable_body() { ":" } else { "" };
                result.push_str(&format!("{indent_str}{kw}{colon}\n"));
                result.push_str(&self.render_mixed_body(indent + 1, &FormatOpts::XSLT, registry));
                Some(result)
            }
            "xsl:copy" => {
                // xsl:copy → copy (shallow copy of the current node). Distinct
                // from copy-of's `copy X`; any attributes (select, namespaces)
                // stay in parens to avoid colliding with that form.
                result.push_str(&format!("{indent_str}copy"));
                if !self.attributes.is_empty() {
                    let mut sorted_attrs: Vec<_> = self.attributes.iter().collect();
                    sorted_attrs.sort_by_key(|(k, _)| *k);
                    let attr_str: Vec<String> = sorted_attrs
                        .iter()
                        .map(|(k, v)| format!("{k}=\"{v}\""))
                        .collect();
                    let col = current_col(&result);
                    result.push_str(&render_attrs(&attr_str, col, indent, true));
                }
                let colon = if self.has_renderable_body() { ":" } else { "" };
                result.push_str(&format!("{colon}\n"));
                result.push_str(&self.render_mixed_body(indent + 1, &FormatOpts::XSLT, registry));
                Some(result)
            }
            _ => None,
        }
    }
}

/// Registry of templates collected from XSLT files for expansion
#[derive(Debug, Default)]
pub(crate) struct TemplateRegistry {
    /// Map from match pattern to template element
    templates: HashMap<String, XmlElement>,
}

impl TemplateRegistry {
    pub(crate) fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    /// Get a template by its match pattern
    /// Handles XSLT union patterns like "PAYEE|RECEIVER" matching "PAYEE"
    /// Also handles path selects like "Input/Header" matching template "Header"
    pub(crate) fn get(&self, select: &str) -> Option<&XmlElement> {
        // First try exact match
        if let Some(template) = self.templates.get(select) {
            return Some(template);
        }

        // Get the last segment of the select path for matching
        // e.g., "Input/Header" -> "Header"
        let select_element = select.rsplit('/').next().unwrap_or(select);

        // Try matching against union patterns (e.g., "PAYEE|RECEIVER" matches "PAYEE")
        for (pattern, template) in &self.templates {
            // Split on | and check if any part matches
            for part in pattern.split('|') {
                let part = part.trim();
                if part == select || part == select_element {
                    return Some(template);
                }
            }
        }

        None
    }

    /// Collect templates from an XmlElement tree (looks for xsl:template elements)
    pub(crate) fn collect_from_element(&mut self, element: &XmlElement) {
        if element.name == "xsl:template"
            && let Some(match_attr) = element.attributes.get("match")
        {
            self.templates.insert(match_attr.clone(), element.clone());
        }
        for child in &element.children {
            self.collect_from_element(child);
        }
    }

    /// Collect xsl:import hrefs from an element tree
    pub(crate) fn collect_imports(element: &XmlElement) -> Vec<String> {
        let mut imports = Vec::new();
        if (element.name == "xsl:import" || element.name == "xsl:include")
            && let Some(href) = element.attributes.get("href")
        {
            imports.push(href.clone());
        }
        for child in &element.children {
            imports.extend(Self::collect_imports(child));
        }
        imports
    }

    /// Build registry from a file, following imports recursively
    pub(crate) fn build_from_file(file_path: &str) -> Result<Self> {
        let mut registry = Self::new();
        let mut processed = std::collections::HashSet::new();
        registry.process_file_recursive(file_path, &mut processed)?;
        Ok(registry)
    }

    pub(crate) fn process_file_recursive(
        &mut self,
        file_path: &str,
        processed: &mut std::collections::HashSet<String>,
    ) -> Result<()> {
        let canonical = std::fs::canonicalize(file_path)
            .unwrap_or_else(|_| std::path::PathBuf::from(file_path));
        let canonical_str = canonical.to_string_lossy().to_string();

        if processed.contains(&canonical_str) {
            return Ok(());
        }
        processed.insert(canonical_str);

        let content = read_file_lenient(file_path)
            .with_context(|| format!("Failed to read file for template expansion: {file_path}"))?;

        let elements = parse_xml(&content)?;

        // Collect templates from this file
        for element in &elements {
            self.collect_from_element(element);
        }

        // Find and process imports
        let base_dir = Path::new(file_path).parent().unwrap_or(Path::new("."));
        for element in &elements {
            for import_href in Self::collect_imports(element) {
                let import_path = base_dir.join(&import_href);
                if import_path.exists() {
                    self.process_file_recursive(import_path.to_string_lossy().as_ref(), processed)?;
                }
            }
        }

        Ok(())
    }
}
