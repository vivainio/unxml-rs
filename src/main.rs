use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;
use glob::glob;
use quick_xml::Reader;
use quick_xml::events::Event;
use scraper::{ElementRef, Html, Selector};

/// Read a file as text, tolerating non-UTF-8 inputs.
///
/// Many real-world XML files (e.g. SAP/EDI invoice exports) are encoded as
/// ISO-8859-1 / Latin-1 and often carry no `<?xml encoding=...?>` declaration.
/// `fs::read_to_string` rejects any non-UTF-8 byte, so we read raw bytes and
/// fall back to a Latin-1 decode (every byte 0x00-0xFF maps directly to the
/// matching Unicode code point, so this never fails).
fn read_file_lenient(file_path: &str) -> Result<String> {
    let bytes = fs::read(file_path).with_context(|| format!("Failed to read file: {file_path}"))?;
    Ok(match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => e.into_bytes().into_iter().map(|b| b as char).collect(),
    })
}

#[derive(Parser)]
#[command(name = "unxml")]
#[command(about = "Simplify and 'flatten' XML and HTML files")]
#[command(version)]
struct Cli {
    /// XML or HTML files to process (supports glob patterns)
    files: Vec<String>,

    /// Force input format (xml or html). If not specified, format is auto-detected
    #[arg(short, long)]
    format: Option<String>,

    /// Enable proprietary special element handling rules
    #[arg(long)]
    special: bool,

    /// Enable XSLT-specific formatting transformations
    #[arg(long)]
    xslt: bool,

    /// Enable Schematron-specific formatting transformations
    #[arg(long)]
    schematron: bool,

    /// Enable XML Schema (XSD) specific formatting transformations
    #[arg(long)]
    xsd: bool,

    /// Expand xsl:apply-templates by inlining matching templates from imports
    #[arg(long)]
    expand: bool,

    /// Autodetect the processing mode from each file's extension
    /// (.xsl/.xslt -> xslt, .sch -> schematron, .xsd -> xsd). Without this
    /// (and without an explicit mode flag) files render as plain XML.
    #[arg(long)]
    auto: bool,

    /// Pipe the rendered output through `bat -l unxml` for syntax-highlighted,
    /// paged display. Implies --auto. Falls back to plain stdout if `bat` is
    /// not installed.
    #[arg(long)]
    bat: bool,

    /// Hide one or more namespace prefixes from element names to cut noise,
    /// e.g. `--hide-ns cbc,cac`. Repeatable and comma-separated. The matching
    /// xmlns: declarations are dropped too. Under --auto/--bat, well-known
    /// document types (e.g. UBL) also get a sensible set hidden automatically.
    #[arg(long, value_delimiter = ',')]
    hide_ns: Vec<String>,

    /// Render only the subtrees whose element name matches this tag, instead of
    /// the whole document. Matching is by tag name only (no paths or
    /// predicates): a bare name like `InvoiceLine` matches on the local name so
    /// it ignores namespace prefixes, while a prefixed name like
    /// `cac:InvoiceLine` matches the full name. Each matched subtree is rendered
    /// as a top-level fragment.
    #[arg(long)]
    select: Option<String>,

    /// Read input from stdin (assumes XML format)
    #[arg(long)]
    stdin: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct FormatOpts {
    special: bool,
    xslt: bool,
    schematron: bool,
    xsd: bool,
}

impl FormatOpts {
    const XSLT: FormatOpts = FormatOpts {
        special: false,
        xslt: true,
        schematron: false,
        xsd: false,
    };

    /// True if the user explicitly selected any processing mode. When none is
    /// set we fall back to autodetecting the mode from the file extension.
    fn has_mode(&self) -> bool {
        self.special || self.xslt || self.schematron || self.xsd
    }
}

/// Render Pug-style attribute parentheses for an element at the given indent.
///
/// Short lists stay on one line: `(a="1", b="2")`. A list whose single-line
/// form would exceed `WRAP_WIDTH` columns wraps to one attribute per line,
/// indented two levels deeper than the element (so it sits clearly below the
/// children), with the closing paren attached to the last attribute. Width is
/// the trigger rather than attribute count: a single long namespace URI should
/// wrap even when it's the only attribute.
///
/// `leading_space` prefixes a space before the opening paren (used where the
/// attributes follow already-emitted text rather than the bare element name).
/// `col` is the number of columns already used on the current line (indent plus
/// the element name and any text emitted before the attributes), so the width
/// decision reflects the whole line, not just the parenthesised part.
/// Columns used by the last (unterminated) line of `s` — i.e. characters after
/// the final newline. Used to tell `render_attrs` how much of the line the
/// element name has already consumed.
fn current_col(s: &str) -> usize {
    match s.rfind('\n') {
        Some(i) => s.len() - i - 1,
        None => s.len(),
    }
}

/// Append an element's text content to a line whose element has already been
/// rendered. Single-line text is emitted inline as ` = text`. Multi-line text
/// is emitted as a pug-style piped block: each line prefixed with `| ` and
/// indented one level deeper than the element, so it is clear where the value
/// begins and ends rather than continuation lines bleeding to column zero.
/// Does not emit a trailing newline; the caller appends one.
fn render_text(result: &mut String, text: &str, indent: usize) {
    if text.trim().is_empty() {
        return;
    }
    if text.trim().contains('\n') {
        // Drop fully-blank leading/trailing lines but keep the original
        // indentation of the inner lines so we can dedent them as a block.
        let lines: Vec<&str> = text.lines().collect();
        let start = lines.iter().position(|l| !l.trim().is_empty()).unwrap();
        let end = lines.iter().rposition(|l| !l.trim().is_empty()).unwrap();
        let lines = &lines[start..=end];

        // Strip the common leading whitespace shared by all non-empty lines so
        // the block sits flush under the element, while preserving each line's
        // relative indentation (meaningful for embedded code such as <script>).
        let common = lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.len() - l.trim_start().len())
            .min()
            .unwrap_or(0);

        let block_indent = "  ".repeat(indent + 1);
        result.push_str(" =");
        for line in lines {
            result.push('\n');
            result.push_str(&block_indent);
            let line = if line.trim().is_empty() {
                ""
            } else {
                line[common..].trim_end()
            };
            if line.is_empty() {
                result.push('|');
            } else {
                result.push_str("| ");
                result.push_str(line);
            }
        }
    } else {
        result.push_str(&format!(" = {}", text.trim()));
    }
}

fn render_attrs(attr_parts: &[String], col: usize, indent: usize, leading_space: bool) -> String {
    const WRAP_WIDTH: usize = 100;

    if attr_parts.is_empty() {
        return String::new();
    }

    let sep = if leading_space { " " } else { "" };
    let single = format!("{sep}({})", attr_parts.join(", "));

    if col + single.len() <= WRAP_WIDTH {
        return single;
    }

    let attr_indent = "  ".repeat(indent + 2);
    let mut out = format!("{sep}(\n");
    for (i, part) in attr_parts.iter().enumerate() {
        out.push_str(&attr_indent);
        out.push_str(part);
        if i + 1 < attr_parts.len() {
            out.push_str(",\n");
        } else {
            out.push(')');
        }
    }
    out
}

/// One item in an element's content in document order. `Text` is a literal text
/// run; `Child` is an index into the element's `children`. This preserves the
/// faithful interleaving of text and child elements (mixed content) that the
/// flat `text_content` + `children` split alone cannot represent.
#[derive(Debug, Clone)]
enum NodeRef {
    Text(String),
    Child(usize),
}

#[derive(Debug, Clone)]
struct XmlElement {
    name: String,
    attributes: HashMap<String, String>,
    /// All text runs concatenated — kept for the common scalar case
    /// (`<a>text</a>` → `a = text`) and for paths that don't need ordering.
    text_content: String,
    children: Vec<XmlElement>,
    /// Document-order view of text runs and child elements. Used to render mixed
    /// content faithfully; empty on elements built outside the parsers.
    nodes: Vec<NodeRef>,
}

impl XmlElement {
    fn new(name: String) -> Self {
        Self {
            name,
            attributes: HashMap::new(),
            text_content: String::new(),
            children: Vec::new(),
            nodes: Vec::new(),
        }
    }

    /// True when this element interleaves non-empty text with child elements —
    /// the case the scalar `name = text` form cannot represent faithfully.
    fn is_mixed(&self) -> bool {
        let has_text = self
            .nodes
            .iter()
            .any(|n| matches!(n, NodeRef::Text(t) if !t.trim().is_empty()));
        let has_child = self.nodes.iter().any(|n| matches!(n, NodeRef::Child(_)));
        has_text && has_child
    }

    /// Render this element's content in document order: text runs as quoted
    /// lines, child elements recursed. Used for mixed content in every mode so
    /// a text run between two elements keeps its position.
    fn render_mixed_body(
        &self,
        indent: usize,
        opts: &FormatOpts,
        registry: Option<&TemplateRegistry>,
    ) -> String {
        let ind = "  ".repeat(indent);
        let mut out = String::new();
        for node in &self.nodes {
            match node {
                NodeRef::Text(text) => {
                    // Collapse internal whitespace so a run spanning several
                    // source lines renders as one clean quoted line.
                    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !text.is_empty() {
                        out.push_str(&format!("{ind}\"{text}\"\n"));
                    }
                }
                NodeRef::Child(i) => {
                    out.push_str(&self.children[*i].format_yaml_like(indent, opts, registry));
                }
            }
        }
        out
    }

    fn format_yaml_like(
        &self,
        indent: usize,
        opts: &FormatOpts,
        registry: Option<&TemplateRegistry>,
    ) -> String {
        let mut result = String::new();
        let indent_str = "  ".repeat(indent);

        // Schematron-specific transformations (also handle xsl:* inside schematron files)
        if opts.schematron
            && let Some(transformed) = self
                .format_schematron_element(indent, &indent_str, registry)
                .or_else(|| self.format_xslt_element(indent, &indent_str, registry))
        {
            return transformed;
        }

        // XSD-specific transformations
        if opts.xsd
            && let Some(transformed) = self.format_xsd_element(indent, &indent_str, registry)
        {
            return transformed;
        }

        // XSLT-specific transformations
        if opts.xslt
            && let Some(transformed) = self.format_xslt_element(indent, &indent_str, registry)
        {
            return transformed;
        }

        let special = opts.special;

        // Special handling for elements with loopDataSource attribute
        if special && let Some(loop_data_source) = self.attributes.get("loopDataSource") {
            // Parse loopDataSource - two formats:
            // 1. "dataItem;/ROOT/CONTAINER/ITEMS/ITEM/ENTRIES/ENTRY"
            // 2. "foo" (just emit "each foo")
            if let Some(semicolon_pos) = loop_data_source.find(';') {
                let variable_name = &loop_data_source[..semicolon_pos];
                let xpath = &loop_data_source[semicolon_pos + 1..];

                result.push_str(&format!("{indent_str}each {variable_name} in {xpath}"));
            } else {
                // No semicolon, just emit "each {value}"
                result.push_str(&format!("{indent_str}each {loop_data_source}"));
            }
            result.push('\n');

            // Create a modified element without the loopDataSource attribute
            let mut modified_attributes = self.attributes.clone();
            modified_attributes.remove("loopDataSource");

            let modified_element = XmlElement {
                name: self.name.clone(),
                attributes: modified_attributes,
                text_content: self.text_content.clone(),
                children: self.children.clone(),
                nodes: self.nodes.clone(),
            };

            // Always process the modified element normally (section should still appear)
            result.push_str(&modified_element.format_yaml_like(indent + 1, opts, registry));

            return result;
        }

        // Special handling for elements with include="foo" attribute
        if special && let Some(include_value) = self.attributes.get("include") {
            result.push_str(&format!("{indent_str}if {include_value}"));
            result.push('\n');

            // Create a modified element without the include attribute
            let mut modified_attributes = self.attributes.clone();
            modified_attributes.remove("include");

            let modified_element = XmlElement {
                name: self.name.clone(),
                attributes: modified_attributes,
                text_content: self.text_content.clone(),
                children: self.children.clone(),
                nodes: self.nodes.clone(),
            };

            // Special handling for section elements after include processing
            if self.name == "section"
                && let Some(name) = modified_element.attributes.get("name")
                && modified_element.attributes.len() == 1
            {
                // Apply the #name transformation
                result.push_str(&format!("{}#{}", "  ".repeat(indent + 1), name));
                result.push('\n');

                // Process children elements
                for child in &modified_element.children {
                    result.push_str(&child.format_yaml_like(indent + 2, opts, registry));
                }

                return result;
            }

            // Process the modified element - if it has no attributes left and no text content,
            // just process its children directly
            if modified_element.attributes.is_empty()
                && modified_element.text_content.trim().is_empty()
            {
                // Process children directly
                for child in &modified_element.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
            } else {
                // Process the modified element normally
                result.push_str(&modified_element.format_yaml_like(indent + 1, opts, registry));
            }

            return result;
        }

        // Special handling for specific XML elements
        if special {
            match self.name.as_str() {
                "builtInMethodParameterList" | "builtinmethodparameterlist" => {
                    if let Some(name) = self.attributes.get("name") {
                        result.push_str(&format!("{indent_str}{name}()"));
                        result.push('\n');

                        // Process children elements
                        for child in &self.children {
                            result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                        }

                        return result;
                    }
                }
                "parameter" => {
                    // Only apply special transformation if element has only the 'name' attribute
                    if let Some(name) = self.attributes.get("name")
                        && self.attributes.len() == 1
                    {
                        if !self.text_content.trim().is_empty() {
                            result.push_str(&format!(
                                "{}{} := {}",
                                indent_str,
                                name,
                                self.text_content.trim()
                            ));
                        } else {
                            result.push_str(&format!("{indent_str}{name} := "));
                        }
                        result.push('\n');

                        // Process children elements
                        for child in &self.children {
                            result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                        }

                        return result;
                    }
                }
                "variable" => {
                    // Only apply special transformation if element has only the 'name' attribute
                    if let Some(name) = self.attributes.get("name")
                        && self.attributes.len() == 1
                    {
                        if !self.text_content.trim().is_empty() {
                            result.push_str(&format!(
                                "{}{} :== {}",
                                indent_str,
                                name,
                                self.text_content.trim()
                            ));
                        } else {
                            result.push_str(&format!("{indent_str}{name} :== "));
                        }
                        result.push('\n');

                        // Process children elements
                        for child in &self.children {
                            result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                        }

                        return result;
                    }
                }
                "method" => {
                    // Special transformation for method elements with jumpToXmlFile and jumpToXPath
                    if let (Some(jump_to_xml_file), Some(jump_to_xpath)) = (
                        self.attributes.get("jumpToXmlFile"),
                        self.attributes.get("jumpToXPath"),
                    ) {
                        // Extract the file name from jumpToXmlFile (remove {v, prefix and } suffix)
                        let xml_file = if jump_to_xml_file.starts_with("{v,")
                            && jump_to_xml_file.ends_with('}')
                        {
                            &jump_to_xml_file[3..jump_to_xml_file.len() - 1]
                        } else {
                            jump_to_xml_file
                        };

                        // Extract section name from jumpToXPath using pattern //section[@name='SECTION_NAME']
                        let section_name = if let Some(start) = jump_to_xpath.find("[@name='") {
                            let start_idx = start + 8; // Length of "[@name='"
                            if let Some(end) = jump_to_xpath[start_idx..].find("']") {
                                &jump_to_xpath[start_idx..start_idx + end]
                            } else {
                                "UnknownSection"
                            }
                        } else {
                            "UnknownSection"
                        };

                        // Build the transformation: XmlFile::SectionName(name="methodName")
                        result.push_str(&format!("{indent_str}{xml_file}::{section_name}"));

                        // Add name parameter if present
                        if let Some(name) = self.attributes.get("name") {
                            result.push_str(&format!("(name=\"{name}\")"));
                        } else {
                            result.push_str("()");
                        }

                        result.push('\n');

                        // Process children elements
                        for child in &self.children {
                            result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                        }

                        return result;
                    }
                }
                "section" => {
                    // Only apply special transformation if element has only the 'name' attribute
                    if let Some(name) = self.attributes.get("name")
                        && self.attributes.len() == 1
                    {
                        result.push_str(&format!("{indent_str}#{name}"));
                        result.push('\n');

                        // Process children elements
                        for child in &self.children {
                            result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                        }

                        return result;
                    }
                }
                "command" => {
                    // Special transformation for command elements with type attribute
                    if let Some(command_type) = self.attributes.get("type") {
                        // Create a modified element without the type attribute
                        let mut modified_attributes = self.attributes.clone();
                        modified_attributes.remove("type");

                        // Build the command.{type} name
                        result.push_str(&format!("{indent_str}command.{command_type}"));

                        // Add remaining attributes in Pug-style parentheses if any
                        if !modified_attributes.is_empty() {
                            // For XML (--special context), treat empty values as boolean attributes
                            let mut boolean_attrs: Vec<_> = modified_attributes
                                .iter()
                                .filter(|(_, value)| value.is_empty())
                                .collect();
                            let mut non_boolean_attrs: Vec<_> = modified_attributes
                                .iter()
                                .filter(|(_, value)| !value.is_empty())
                                .collect();

                            // Sort attributes for deterministic output
                            boolean_attrs.sort_by_key(|(key, _)| *key);
                            non_boolean_attrs.sort_by_key(|(key, _)| *key);

                            // Build all attributes in Pug-style parentheses
                            let mut attr_parts = Vec::new();

                            // Add non-boolean attributes first with quoted values
                            for (key, value) in non_boolean_attrs {
                                let escaped_value = value.replace('"', "&quot;");
                                attr_parts.push(format!("{key}=\"{escaped_value}\""));
                            }

                            // Add boolean attributes (just the attribute name)
                            for (key, _) in boolean_attrs {
                                attr_parts.push(key.to_string());
                            }

                            let col = current_col(&result);
                            result.push_str(&render_attrs(&attr_parts, col, indent, false));
                        }

                        // Text content with = assignment
                        render_text(&mut result, &self.text_content, indent);

                        result.push('\n');

                        // Process children elements
                        for child in &self.children {
                            result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                        }

                        return result;
                    }
                }
                _ => {}
            }
        }

        // Element name
        result.push_str(&format!("{}{}", indent_str, self.name));

        // Attributes in Pug-style parentheses
        if !self.attributes.is_empty() {
            // Separate boolean attributes from others
            // Boolean attributes are any empty-valued attributes EXCEPT those commonly used with empty values
            let non_boolean_empty_attrs = [
                "value",
                "alt",
                "title",
                "placeholder",
                "data-",
                "aria-",
                "content",
                "href",
                "src",
            ];

            let mut boolean_attrs: Vec<_> = self
                .attributes
                .iter()
                .filter(|(key, value)| {
                    value.is_empty()
                        && !non_boolean_empty_attrs.iter().any(|&prefix| {
                            if prefix.ends_with('-') {
                                key.starts_with(prefix)
                            } else {
                                key.as_str() == prefix
                            }
                        })
                })
                .collect();
            let mut non_boolean_attrs: Vec<_> = self
                .attributes
                .iter()
                .filter(|(key, value)| {
                    !value.is_empty()
                        || non_boolean_empty_attrs.iter().any(|&prefix| {
                            if prefix.ends_with('-') {
                                key.starts_with(prefix)
                            } else {
                                key.as_str() == prefix
                            }
                        })
                })
                .collect();

            // Sort attributes for deterministic output
            boolean_attrs.sort_by_key(|(key, _)| *key);
            non_boolean_attrs.sort_by_key(|(key, _)| *key);

            // Build all attributes in Pug-style parentheses
            let mut attr_parts = Vec::new();

            // Add non-boolean attributes first with quoted values
            for (key, value) in non_boolean_attrs {
                // Always quote all attribute values for consistency and safety
                let escaped_value = value.replace('"', "&quot;");
                attr_parts.push(format!("{key}=\"{escaped_value}\""));
            }

            // Add boolean attributes (just the attribute name)
            for (key, _) in boolean_attrs {
                attr_parts.push(key.to_string());
            }

            let col = current_col(&result);
            result.push_str(&render_attrs(&attr_parts, col, indent, false));
        }

        if self.is_mixed() {
            // Mixed content: render text runs and child elements in order.
            result.push('\n');
            result.push_str(&self.render_mixed_body(indent + 1, opts, registry));
        } else {
            // Text content with = assignment
            render_text(&mut result, &self.text_content, indent);

            result.push('\n');

            // Children elements
            for child in &self.children {
                result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
            }
        }

        result
    }

    fn format_xslt_element(
        &self,
        indent: usize,
        indent_str: &str,
        registry: Option<&TemplateRegistry>,
    ) -> Option<String> {
        let mut result = String::new();

        match self.name.as_str() {
            "xsl:template" => {
                // xsl:template(match="X") → template X
                // xsl:template(name="X") → template #X
                if let Some(match_val) = self.attributes.get("match") {
                    result.push_str(&format!("{indent_str}template {match_val}"));
                } else if let Some(name_val) = self.attributes.get("name") {
                    result.push_str(&format!("{indent_str}template #{name_val}"));
                } else {
                    return None;
                }

                // Add any extra attributes (like xmlns declarations)
                let extra_attrs: Vec<_> = self
                    .attributes
                    .iter()
                    .filter(|(k, _)| *k != "match" && *k != "name")
                    .collect();
                if !extra_attrs.is_empty() {
                    let mut sorted_attrs: Vec<_> = extra_attrs.into_iter().collect();
                    sorted_attrs.sort_by_key(|(k, _)| *k);
                    let attr_str: Vec<String> = sorted_attrs
                        .iter()
                        .map(|(k, v)| format!("{k}=\"{v}\""))
                        .collect();
                    let col = current_col(&result);
                    result.push_str(&render_attrs(&attr_str, col, indent, true));
                }

                result.push('\n');
                result.push_str(&self.render_mixed_body(indent + 1, &FormatOpts::XSLT, registry));
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
                // Body content (a fallback value, or an XSLT-2.0+ sequence
                // constructor used instead of select) is rendered beneath it.
                let has_body = !self.nodes.is_empty();
                if let Some(select) = self.attributes.get("select") {
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
                    result.push_str(&format!("{indent_str}if {test}\n"));
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
                result.push_str(&format!("{indent_str}choose\n"));
                result.push_str(&self.render_mixed_body(indent + 1, &FormatOpts::XSLT, registry));
                Some(result)
            }
            "xsl:when" => {
                // xsl:when(test="X") → when X
                if let Some(test) = self.attributes.get("test") {
                    result.push_str(&format!("{indent_str}when {test}\n"));
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
                result.push_str(&format!("{indent_str}else\n"));
                result.push_str(&self.render_mixed_body(indent + 1, &FormatOpts::XSLT, registry));
                Some(result)
            }
            "xsl:variable" => {
                // xsl:variable(name="x", select="...") → x := ...
                if let Some(name) = self.attributes.get("name") {
                    if let Some(select) = self.attributes.get("select") {
                        result.push_str(&format!("{indent_str}{name} := {select}\n"));
                    } else if !self.text_content.trim().is_empty() {
                        result.push_str(&format!(
                            "{indent_str}{name} := {}\n",
                            self.text_content.trim()
                        ));
                    } else if !self.children.is_empty() {
                        result.push_str(&format!("{indent_str}{name} :=\n"));
                        for child in &self.children {
                            result.push_str(&child.format_yaml_like(
                                indent + 1,
                                &FormatOpts::XSLT,
                                registry,
                            ));
                        }
                    } else {
                        result.push_str(&format!("{indent_str}{name} :=\n"));
                    }
                    Some(result)
                } else {
                    None
                }
            }
            "xsl:with-param" => {
                // xsl:with-param(name="x", select="...") → x := ...
                if let Some(name) = self.attributes.get("name") {
                    if let Some(select) = self.attributes.get("select") {
                        result.push_str(&format!("{indent_str}{name} := {select}\n"));
                    } else if !self.text_content.trim().is_empty() {
                        result.push_str(&format!(
                            "{indent_str}{name} := {}\n",
                            self.text_content.trim()
                        ));
                    } else if !self.children.is_empty() {
                        result.push_str(&format!("{indent_str}{name} :=\n"));
                        for child in &self.children {
                            result.push_str(&child.format_yaml_like(
                                indent + 1,
                                &FormatOpts::XSLT,
                                registry,
                            ));
                        }
                    } else {
                        result.push_str(&format!("{indent_str}{name} :=\n"));
                    }
                    Some(result)
                } else {
                    None
                }
            }
            "xsl:call-template" => {
                // xsl:call-template(name="X") → call X
                if let Some(name) = self.attributes.get("name") {
                    result.push_str(&format!("{indent_str}call {name}\n"));
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
                    result.push_str(&format!("{indent_str}foreach {select}\n"));
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
                    result.push_str(&format!("{indent_str}element {name}\n"));
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
                // xsl:param(name="x", select="...") → param x := ...
                if let Some(name) = self.attributes.get("name") {
                    if let Some(select) = self.attributes.get("select") {
                        result.push_str(&format!("{indent_str}param {name} := {select}\n"));
                    } else {
                        result.push_str(&format!("{indent_str}param {name}\n"));
                    }
                    Some(result)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn format_schematron_element(
        &self,
        indent: usize,
        indent_str: &str,
        registry: Option<&TemplateRegistry>,
    ) -> Option<String> {
        // Match against the local name, ignoring an optional "sch:" prefix.
        let local = self.name.strip_prefix("sch:").unwrap_or(&self.name);
        let opts = &FormatOpts {
            schematron: true,
            ..FormatOpts::default()
        };
        let mut result = String::new();

        match local {
            "schema" => {
                // Drop the wrapper, just process children at the same indent.
                if let Some(title) = self.attributes.get("title") {
                    result.push_str(&format!("{indent_str}schema {title}\n"));
                } else {
                    result.push_str(&format!("{indent_str}schema\n"));
                }
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "title" => {
                let text = self.text_content.trim();
                result.push_str(&format!("{indent_str}title = {text}\n"));
                Some(result)
            }
            "ns" => {
                // ns(prefix="x", uri="...") → ns x = uri
                let prefix = self.attributes.get("prefix")?;
                let uri = self.attributes.get("uri")?;
                result.push_str(&format!("{indent_str}ns {prefix} = {uri}\n"));
                Some(result)
            }
            "phase" => {
                if let Some(id) = self.attributes.get("id") {
                    result.push_str(&format!("{indent_str}phase {id}\n"));
                } else {
                    result.push_str(&format!("{indent_str}phase\n"));
                }
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "active" => {
                let pat = self.attributes.get("pattern")?;
                result.push_str(&format!("{indent_str}active {pat}\n"));
                Some(result)
            }
            "pattern" => {
                if let Some(id) = self.attributes.get("id") {
                    result.push_str(&format!("{indent_str}pattern {id}\n"));
                } else {
                    result.push_str(&format!("{indent_str}pattern\n"));
                }
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "rule" => {
                let context = self.attributes.get("context")?;
                let context_clean = context.split_whitespace().collect::<Vec<_>>().join(" ");
                result.push_str(&format!("{indent_str}rule {context_clean}\n"));
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "assert" | "report" => {
                let test = self.attributes.get("test")?;
                let test_clean = test.split_whitespace().collect::<Vec<_>>().join(" ");
                let id = self.attributes.get("id");
                let flag = self.attributes.get("flag");
                let header = match (id, flag) {
                    (Some(i), Some(f)) => format!("{local} {i} [{f}] {test_clean}"),
                    (Some(i), None) => format!("{local} {i} {test_clean}"),
                    (None, Some(f)) => format!("{local} [{f}] {test_clean}"),
                    (None, None) => format!("{local} {test_clean}"),
                };
                result.push_str(&format!("{indent_str}{header}\n"));
                let msg = self.text_content.trim();
                if !msg.is_empty() {
                    let msg_clean = msg.split_whitespace().collect::<Vec<_>>().join(" ");
                    let inner_indent = "  ".repeat(indent + 1);
                    result.push_str(&format!("{inner_indent}= {msg_clean}\n"));
                }
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "let" => {
                // let(name="x", value="...") → x := ...
                let name = self.attributes.get("name")?;
                if let Some(value) = self.attributes.get("value") {
                    result.push_str(&format!("{indent_str}{name} := {value}\n"));
                } else if !self.text_content.trim().is_empty() {
                    result.push_str(&format!(
                        "{indent_str}{name} := {}\n",
                        self.text_content.trim()
                    ));
                } else {
                    result.push_str(&format!("{indent_str}{name} :=\n"));
                }
                Some(result)
            }
            _ => None,
        }
    }

    fn format_xsd_element(
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
                        if prefix == "xs" || prefix == "xsd" {
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

                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
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
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
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
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
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
                    for child in &self.children {
                        result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                    }
                    return Some(result);
                }
                let n = self.attributes.get("name")?;
                if let Some(t) = self.attributes.get("type") {
                    result.push_str(&format!("{indent_str}@{n} : {t}{suffix}\n"));
                    for child in &self.children {
                        result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                    }
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
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
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
                    let inner_indent = "  ".repeat(indent + 1);
                    for child in &der.children {
                        emit_complextype_child(
                            child,
                            indent + 1,
                            &inner_indent,
                            opts,
                            registry,
                            &mut result,
                        );
                    }
                } else {
                    result.push_str(&format!("{indent_str}{prefix}type{name_part}{tail}\n"));
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
                    for child in &self.children {
                        result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                    }
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
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "complexContent" | "simpleContent" => {
                result.push_str(&format!("{indent_str}{local}\n"));
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "enumeration" => {
                if let Some(v) = self.attributes.get("value") {
                    result.push_str(&format!("{indent_str}| {v}\n"));
                } else {
                    result.push_str(&format!("{indent_str}|\n"));
                }
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "pattern" | "minLength" | "maxLength" | "length" | "minInclusive" | "maxInclusive"
            | "minExclusive" | "maxExclusive" | "totalDigits" | "fractionDigits" | "whiteSpace" => {
                if let Some(v) = self.attributes.get("value") {
                    result.push_str(&format!("{indent_str}{local} {v}\n"));
                } else {
                    result.push_str(&format!("{indent_str}{local}\n"));
                }
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "annotation" => {
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent, opts, registry));
                }
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
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "list" => {
                if let Some(t) = self.attributes.get("itemType") {
                    result.push_str(&format!("{indent_str}list {t}\n"));
                } else {
                    result.push_str(&format!("{indent_str}list\n"));
                }
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
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
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
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
    fn format_xsd_member(
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
        for child in &self.children {
            result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
        }
        Some(result)
    }
}

fn is_true(value: Option<&String>) -> bool {
    matches!(value.map(|s| s.as_str()), Some("true") | Some("1"))
}

/// Walk an xsd:documentation subtree and collect prose text from elements
/// whose local name is "Definition" or "Description" (CCTS convention used
/// by UBL: <ccts:Definition>A class to define ...</ccts:Definition>).
fn extract_doc_prose(elem: &XmlElement, out: &mut Vec<String>) {
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

fn xsd_local(name: &str) -> &str {
    name.strip_prefix("xs:")
        .or_else(|| name.strip_prefix("xsd:"))
        .unwrap_or(name)
}

/// Inside a complexType (or extension/restriction body), emit a child:
///   - a transparent xs:sequence (no occurs constraints) is folded; its members are
///     emitted at this level via format_xsd_member (which drops the 'element' keyword)
///   - everything else is emitted via standard format_yaml_like
fn emit_complextype_child(
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
        for grandchild in &child.children {
            if let Some(s) = grandchild.format_xsd_member(indent, indent_str, registry) {
                out.push_str(&s);
            } else {
                out.push_str(&grandchild.format_yaml_like(indent, opts, registry));
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
fn try_inline_simple_type(elem: &XmlElement, indent: usize) -> Option<(String, String)> {
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

fn format_occurs(attrs: &HashMap<String, String>) -> String {
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

/// Registry of templates collected from XSLT files for expansion
#[derive(Debug, Default)]
struct TemplateRegistry {
    /// Map from match pattern to template element
    templates: HashMap<String, XmlElement>,
}

impl TemplateRegistry {
    fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    /// Get a template by its match pattern
    /// Handles XSLT union patterns like "PAYEE|RECEIVER" matching "PAYEE"
    /// Also handles path selects like "Input/Header" matching template "Header"
    fn get(&self, select: &str) -> Option<&XmlElement> {
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
    fn collect_from_element(&mut self, element: &XmlElement) {
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
    fn collect_imports(element: &XmlElement) -> Vec<String> {
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
    fn build_from_file(file_path: &str) -> Result<Self> {
        let mut registry = Self::new();
        let mut processed = std::collections::HashSet::new();
        registry.process_file_recursive(file_path, &mut processed)?;
        Ok(registry)
    }

    fn process_file_recursive(
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

#[derive(Debug, PartialEq)]
enum InputFormat {
    Xml,
    Html,
}

/// Pick a processing mode from a file's extension when the user hasn't forced
/// one. Mirrors the extension->flag mapping the test suite applies:
///   .xsl / .xslt -> --xslt,  .sch -> --schematron,  .xsd -> --xsd.
/// `--special` is intentionally excluded: it is proprietary and selected by
/// file name, not extension. Returns the default (no mode) for anything else.
fn detect_mode_from_ext(file_path: &str) -> FormatOpts {
    let ext = Path::new(file_path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "xsl" | "xslt" => FormatOpts {
            xslt: true,
            ..FormatOpts::default()
        },
        "sch" => FormatOpts {
            schematron: true,
            ..FormatOpts::default()
        },
        "xsd" => FormatOpts {
            xsd: true,
            ..FormatOpts::default()
        },
        _ => FormatOpts::default(),
    }
}

/// Recursively strip the given namespace prefixes from element names and drop
/// the matching `xmlns:<prefix>` declarations. Purely cosmetic: it makes a
/// prefix-heavy vocabulary (e.g. UBL's `cbc:`/`cac:`) read as bare local names
/// while leaving signal-carrying prefixes (e.g. `ext:`/`bim:`) untouched.
fn hide_namespaces(elem: &mut XmlElement, prefixes: &HashSet<String>) {
    if let Some((pfx, local)) = elem.name.split_once(':')
        && prefixes.contains(pfx)
    {
        elem.name = local.to_string();
    }
    // Drop the now-redundant xmlns: declarations for the hidden prefixes.
    elem.attributes
        .retain(|k, _| match k.strip_prefix("xmlns:") {
            Some(pfx) => !prefixes.contains(pfx),
            None => true,
        });
    for child in &mut elem.children {
        hide_namespaces(child, prefixes);
    }
}

/// Whether an element's tag matches a Tier-A `--select` pattern. A pattern
/// containing a `:` matches the full prefixed name; a bare pattern matches the
/// local name (the part after any prefix), so `InvoiceLine` finds
/// `cac:InvoiceLine` and is robust to `--hide-ns` having stripped the prefix.
fn name_matches_select(name: &str, pattern: &str) -> bool {
    if pattern.contains(':') {
        name == pattern
    } else {
        let local = name.rsplit(':').next().unwrap_or(name);
        local == pattern
    }
}

/// Collect the topmost subtrees whose element name matches `pattern` (Tier-A
/// `--select`). Matching is by tag name only — no paths, axes, or predicates.
/// A matched subtree is returned whole and not descended into, so a nested
/// element of the same name doesn't also produce a separate fragment.
fn select_subtrees<'a>(elements: &'a [XmlElement], pattern: &str, out: &mut Vec<&'a XmlElement>) {
    for elem in elements {
        if name_matches_select(&elem.name, pattern) {
            out.push(elem);
        } else {
            select_subtrees(&elem.children, pattern, out);
        }
    }
}

/// True if `root` is a genuine UBL *instance* document, i.e. an unprefixed
/// document element (e.g. `<Invoice>`, `<CreditNote>`) whose default namespace
/// is a UBL document schema. This deliberately excludes files that merely
/// *reference* UBL namespaces — an XSLT translating to/from UBL has a prefixed
/// root (`xsl:stylesheet`) and carries literal `cbc:`/`cac:` result elements
/// and XPath that must keep their prefixes.
fn is_ubl_document(root: &XmlElement) -> bool {
    const UBL_NS: &str = "urn:oasis:names:specification:ubl:schema:xsd:";
    !root.name.contains(':')
        && root
            .attributes
            .get("xmlns")
            .is_some_and(|uri| uri.contains(UBL_NS))
}

/// Sniff well-known document types from the root elements' namespace bindings
/// and return the set of prefixes worth hiding. Currently recognises the UBL
/// family: for a genuine UBL instance document, any prefix bound to the
/// CommonBasicComponents or CommonAggregateComponents namespace is returned
/// (matched by URI, so it works regardless of the actual prefix the document
/// chose). Non-UBL documents, and stylesheets/schemas that merely reference UBL,
/// contribute nothing.
fn sniff_hidden_prefixes(elements: &[XmlElement]) -> HashSet<String> {
    const UBL_MARKERS: [&str; 2] = ["CommonBasicComponents", "CommonAggregateComponents"];
    let mut hidden = HashSet::new();
    for root in elements {
        if !is_ubl_document(root) {
            continue;
        }
        for (key, value) in &root.attributes {
            if let Some(pfx) = key.strip_prefix("xmlns:")
                && UBL_MARKERS.iter().any(|m| value.contains(m))
            {
                hidden.insert(pfx.to_string());
            }
        }
    }
    hidden
}

fn detect_format(content: &str, file_path: &str) -> InputFormat {
    // Check file extension first
    if let Some(extension) = Path::new(file_path).extension() {
        let ext = extension.to_string_lossy().to_lowercase();
        match ext.as_str() {
            "html" | "htm" => return InputFormat::Html,
            "xml" | "xsl" | "xsd" | "wsdl" => return InputFormat::Xml,
            _ => {}
        }
    }

    // Check content for HTML-specific indicators
    let content_lower = content.to_lowercase();

    // Look for common HTML indicators
    if content_lower.contains("<!doctype html")
        || content_lower.contains("<html")
        || content_lower.contains("<head>")
        || content_lower.contains("<body>")
    {
        return InputFormat::Html;
    }

    // Look for XML declaration
    if content.trim_start().starts_with("<?xml") {
        return InputFormat::Xml;
    }

    // Default to XML for ambiguous cases
    InputFormat::Xml
}

fn convert_element_to_xml(element: ElementRef, format: &InputFormat) -> XmlElement {
    let element_name = element.value().name().to_string();
    let mut name = element_name.clone();
    let mut xml_element = XmlElement::new(name.clone());

    // Extract attributes
    for (attr_name, attr_value) in element.value().attrs() {
        if *format == InputFormat::Html && attr_name == "class" {
            // For HTML mode, attach classes to the element name
            let classes: Vec<&str> = attr_value.split_whitespace().collect();

            // If it's a div with classes, omit the div part and just use .class1.class2
            if element_name == "div" && !classes.is_empty() {
                name = String::new();
            }

            for class in classes {
                name.push('.');
                name.push_str(class);
            }
            xml_element.name = name.clone();
        } else {
            // For XML mode or non-class attributes, keep as regular attributes
            xml_element
                .attributes
                .insert(attr_name.to_string(), attr_value.to_string());
        }
    }

    // Walk child nodes in document order, recording both element children and
    // text runs so mixed content keeps its interleaving.
    for child in element.children() {
        if let Some(child_element) = ElementRef::wrap(child) {
            xml_element
                .nodes
                .push(NodeRef::Child(xml_element.children.len()));
            xml_element
                .children
                .push(convert_element_to_xml(child_element, format));
        } else if let Some(text) = child.value().as_text() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                xml_element.nodes.push(NodeRef::Text(trimmed.to_string()));
            }
        }
    }

    // Only set the flat text_content for leaf nodes (no child elements), matching
    // XML behaviour; mixed content is rendered from `nodes` instead.
    if xml_element.children.is_empty() {
        let text_content: String = element
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();

        if !text_content.is_empty() {
            xml_element.text_content = text_content;
        }
    }

    xml_element
}

fn parse_html(content: &str, format: &InputFormat) -> Result<Vec<XmlElement>> {
    let document = Html::parse_document(content);
    let mut root_elements = Vec::new();

    // Use a universal selector to find all top-level elements
    let selector = Selector::parse("html").unwrap_or_else(|_| {
        // Fallback: try to get body or any top-level element
        Selector::parse("body").unwrap_or_else(|_| Selector::parse("*").unwrap())
    });

    // First try to find html element
    if let Some(html_element) = document.select(&selector).next() {
        root_elements.push(convert_element_to_xml(html_element, format));
    } else {
        // Fallback: get all top-level elements
        let all_selector =
            Selector::parse("body > *, html > *").unwrap_or_else(|_| Selector::parse("*").unwrap());

        for element in document.select(&all_selector) {
            // Only include elements that don't have a parent element in our selection
            let is_root = element
                .parent()
                .is_none_or(|parent| ElementRef::wrap(parent).is_none());

            if is_root {
                root_elements.push(convert_element_to_xml(element, format));
            }
        }
    }

    // If we still don't have anything, try a more aggressive approach
    if root_elements.is_empty() {
        let fallback_selector = Selector::parse("*").unwrap();
        for element in document.select(&fallback_selector).take(1) {
            root_elements.push(convert_element_to_xml(element, format));
        }
    }

    Ok(root_elements)
}

fn parse_xml(content: &str) -> Result<Vec<XmlElement>> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut elements_stack: Vec<XmlElement> = Vec::new();
    let mut root_elements: Vec<XmlElement> = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut element = XmlElement::new(name);

                // Parse attributes
                for attr in e.attributes() {
                    let attr = attr.context("Failed to parse attribute")?;
                    let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                    let value = String::from_utf8_lossy(&attr.value).to_string();
                    element.attributes.insert(key, value);
                }

                elements_stack.push(element);
            }
            Ok(Event::End(_)) => {
                if let Some(completed_element) = elements_stack.pop() {
                    if let Some(parent) = elements_stack.last_mut() {
                        parent.nodes.push(NodeRef::Child(parent.children.len()));
                        parent.children.push(completed_element);
                    } else {
                        root_elements.push(completed_element);
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().context("Failed to unescape text")?;
                let text_content = text.trim();

                if !text_content.is_empty()
                    && let Some(current_element) = elements_stack.last_mut()
                {
                    if !current_element.text_content.is_empty() {
                        current_element.text_content.push(' ');
                    }
                    current_element.text_content.push_str(text_content);
                    current_element
                        .nodes
                        .push(NodeRef::Text(text_content.to_string()));
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut element = XmlElement::new(name);

                // Parse attributes for empty elements
                for attr in e.attributes() {
                    let attr = attr.context("Failed to parse attribute")?;
                    let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                    let value = String::from_utf8_lossy(&attr.value).to_string();
                    element.attributes.insert(key, value);
                }

                if let Some(parent) = elements_stack.last_mut() {
                    parent.nodes.push(NodeRef::Child(parent.children.len()));
                    parent.children.push(element);
                } else {
                    root_elements.push(element);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Error at position {}: {:?}",
                    reader.error_position(),
                    e
                ));
            }
            _ => {} // Ignore other events like comments, CDATA, etc.
        }
        buf.clear();
    }

    Ok(root_elements)
}

#[allow(clippy::too_many_arguments)]
fn process_content(
    content: &str,
    file_path: &str,
    format_override: Option<&str>,
    opts: &FormatOpts,
    registry: Option<&TemplateRegistry>,
    hide_ns: &HashSet<String>,
    sniff: bool,
    select: Option<&str>,
) -> Result<String> {
    // Determine input format
    let format = if let Some(format_str) = format_override {
        match format_str.to_lowercase().as_str() {
            "html" => InputFormat::Html,
            "xml" => InputFormat::Xml,
            _ => {
                return Err(anyhow::anyhow!(
                    "Unsupported format: {}. Use 'xml' or 'html'",
                    format_str
                ));
            }
        }
    } else {
        detect_format(content, file_path)
    };

    // Parse the content based on detected/specified format
    let mut elements = match format {
        InputFormat::Html => parse_html(content, &format).context("Failed to parse HTML")?,
        InputFormat::Xml => parse_xml(content).context("Failed to parse XML")?,
    };

    // Build the effective set of prefixes to hide: those requested explicitly,
    // plus any inferred by sniffing the document type (only under --auto/--bat).
    let mut hidden = hide_ns.clone();
    if sniff {
        hidden.extend(sniff_hidden_prefixes(&elements));
    }
    if !hidden.is_empty() {
        for element in &mut elements {
            hide_namespaces(element, &hidden);
        }
    }

    // Format output. With --select, render each matched subtree as a top-level
    // fragment separated by a blank line; otherwise render every root element.
    let mut output = String::new();
    if let Some(pattern) = select {
        let mut matched = Vec::new();
        select_subtrees(&elements, pattern, &mut matched);
        for (i, elem) in matched.iter().enumerate() {
            if i > 0 {
                output.push('\n');
            }
            output.push_str(&elem.format_yaml_like(0, opts, registry));
        }
    } else {
        for element in elements {
            output.push_str(&element.format_yaml_like(0, opts, registry));
        }
    }

    Ok(output)
}

fn process_file(
    file_path: &str,
    format_override: Option<&str>,
    opts: &FormatOpts,
    expand: bool,
    hide_ns: &HashSet<String>,
    sniff: bool,
    select: Option<&str>,
) -> Result<String> {
    // Build template registry if expand mode is enabled
    let registry = if expand && opts.xslt {
        Some(TemplateRegistry::build_from_file(file_path)?)
    } else {
        None
    };

    // Read the file
    let content = read_file_lenient(file_path)?;

    process_content(
        &content,
        file_path,
        format_override,
        opts,
        registry.as_ref(),
        hide_ns,
        sniff,
        select,
    )
}

fn process_stdin(
    format_override: Option<&str>,
    opts: &FormatOpts,
    hide_ns: &HashSet<String>,
    sniff: bool,
    select: Option<&str>,
) -> Result<String> {
    // Read from stdin, tolerating non-UTF-8 input (see read_file_lenient).
    let mut bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut bytes)
        .context("Failed to read from stdin")?;
    let content = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => e.into_bytes().into_iter().map(|b| b as char).collect(),
    };

    // Note: expand mode not supported for stdin since we need file paths for imports
    process_content(
        &content,
        "stdin",
        format_override,
        opts,
        None,
        hide_ns,
        sniff,
        select,
    )
}

/// Emit rendered output, optionally through `bat` for syntax highlighting.
/// When `use_bat` is set we pipe to `bat -l unxml`; if no `bat` binary is
/// found we fall back to plain stdout so `--bat` degrades gracefully.
fn emit(output: &str, use_bat: bool) {
    if use_bat && pipe_to_bat(output) {
        return;
    }
    print!("{output}");
}

/// Try to display `output` via `bat -l unxml`. Returns true if a `bat` (or
/// `batcat`, the Debian/Ubuntu name) process was launched and handed the
/// output, false if no such binary exists.
fn pipe_to_bat(output: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};

    for bin in ["bat", "batcat"] {
        // Only stdin is piped; bat inherits our stdout/stderr so its pager
        // draws straight to the terminal.
        let mut child = match Command::new(bin)
            .args(["-l", "unxml"])
            .stdin(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => continue, // binary not found — try the next name
        };
        if let Some(mut stdin) = child.stdin.take() {
            // Ignore a broken pipe if the user quits the pager early.
            let _ = stdin.write_all(output.as_bytes());
        }
        let _ = child.wait();
        return true;
    }
    false
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let opts = FormatOpts {
        special: cli.special,
        xslt: cli.xslt,
        schematron: cli.schematron,
        xsd: cli.xsd,
    };

    // Plain XML rendering is the default. Suffix-based mode autodetection is
    // opt-in via `--auto` (or implied by `--bat`), and only fills in a mode
    // when the user hasn't already forced one explicitly.
    let autodetect = (cli.auto || cli.bat) && !opts.has_mode();

    // Prefixes to hide from element names: the explicit --hide-ns list, plus
    // (under --auto/--bat) any inferred by sniffing the document type.
    let hide_ns: HashSet<String> = cli.hide_ns.iter().cloned().collect();
    let sniff = cli.auto || cli.bat;

    // Handle stdin input
    if cli.stdin {
        // When using stdin, files should be empty
        if !cli.files.is_empty() {
            return Err(anyhow::anyhow!(
                "Cannot specify both --stdin and file arguments"
            ));
        }

        // Process stdin input (no path, so nothing to autodetect from).
        match process_stdin(
            cli.format.as_deref(),
            &opts,
            &hide_ns,
            sniff,
            cli.select.as_deref(),
        ) {
            Ok(output) => emit(&output, cli.bat),
            Err(e) => {
                eprintln!("Error processing stdin: {e}");
                return Err(e);
            }
        }
        return Ok(());
    }

    // Handle file input
    if cli.files.is_empty() {
        return Err(anyhow::anyhow!(
            "No files specified. Please provide at least one file or glob pattern, or use --stdin."
        ));
    }

    let mut all_files = Vec::new();

    // Expand glob patterns and collect all files
    for pattern in &cli.files {
        if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
            // This is a glob pattern
            match glob(pattern) {
                Ok(paths) => {
                    for entry in paths {
                        match entry {
                            Ok(path) => {
                                if let Some(path_str) = path.to_str() {
                                    all_files.push(path_str.to_string());
                                }
                            }
                            Err(e) => {
                                eprintln!("Warning: Error reading glob entry: {e}");
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Invalid glob pattern '{}': {}", pattern, e));
                }
            }
        } else {
            // This is a regular file path
            all_files.push(pattern.clone());
        }
    }

    if all_files.is_empty() {
        return Err(anyhow::anyhow!(
            "No files found matching the specified patterns."
        ));
    }

    // Process each file, accumulating output so it can be sent to the pager
    // (or stdout) in one stream.
    let multiple = all_files.len() > 1;
    let mut combined = String::new();
    for (i, file_path) in all_files.iter().enumerate() {
        // Blank separator line between files (not before the first).
        if i > 0 {
            combined.push('\n');
        }

        // File header comment only when processing more than one file.
        if multiple {
            combined.push_str(&format!("// FILE: {file_path}\n"));
        }

        // When the user didn't force a mode, pick one from this file's
        // extension; otherwise honour the explicit flags for every file.
        let file_opts = if autodetect {
            detect_mode_from_ext(file_path)
        } else {
            opts
        };

        match process_file(
            file_path,
            cli.format.as_deref(),
            &file_opts,
            cli.expand,
            &hide_ns,
            sniff,
            cli.select.as_deref(),
        ) {
            Ok(output) => combined.push_str(&output),
            Err(e) => {
                eprintln!("Error processing file '{file_path}': {e}");
                // Continue processing other files instead of stopping
            }
        }
    }

    emit(&combined, cli.bat);
    Ok(())
}
