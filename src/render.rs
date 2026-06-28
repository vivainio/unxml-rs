//! Generic Pug-like rendering: width-aware attributes, text blocks, and the
//! central `format_yaml_like` dispatcher that routes to each dialect.

use crate::document::name_matches_select;
use crate::model::{Collapse, FormatOpts, XmlElement};
use crate::xslt::TemplateRegistry;

/// Maximum line width before a parenthesised list (attributes, or a folded
/// `function`/`template` param signature) wraps to one item per line.
pub(crate) const WRAP_WIDTH: usize = 100;

/// Columns used by the last (unterminated) line of `s` — i.e. characters after
/// the final newline. Used to tell `render_attrs` how much of the line the
/// element name has already consumed.
pub(crate) fn current_col(s: &str) -> usize {
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
pub(crate) fn render_text(result: &mut String, text: &str, indent: usize) {
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
pub(crate) fn render_attrs(
    attr_parts: &[String],
    col: usize,
    indent: usize,
    leading_space: bool,
) -> String {
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

impl XmlElement {
    pub(crate) fn format_yaml_like(
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

        // WSDL-specific transformations. The embedded <types> schema is XSD, so
        // fall through to the XSD renderer for any element WSDL doesn't claim
        // (xs:schema and everything below it).
        if opts.wsdl
            && let Some(transformed) = self
                .format_wsdl_element(indent, &indent_str, registry)
                .or_else(|| self.format_xsd_element(indent, &indent_str, registry))
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
                inner_source: self.inner_source.clone(),
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
                inner_source: self.inner_source.clone(),
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

        // Dialect modes (XSLT/XSD/WSDL/Schematron) render through their own
        // transforms above and treat element order as significant, so chain
        // collapsing and the verbatim-XML inline path apply only here.
        let dialect = opts.xslt || opts.xsd || opts.wsdl || opts.schematron;

        // `--collapse`: fold a run of single-child, attr/text-free wrapper
        // elements onto one `parent/child/grandchild` line. The variant decides
        // only where a chain may *start* (any wrapper, or one whose name is
        // listed); the descent then walks structurally through every pass-through
        // descendant regardless of name. `el` ends at the first node carrying
        // real content — it renders normally, so no information is dropped.
        let may_start = !dialect
            && !opts.special
            && self.is_chain_link()
            && match &opts.collapse {
                Collapse::Off => false,
                Collapse::All => true,
                Collapse::Only(names) => names.iter().any(|n| name_matches_select(&self.name, n)),
            };
        let mut el = self;
        let mut prefix = String::new();
        if may_start {
            while el.is_chain_link() {
                prefix.push_str(&el.name);
                prefix.push('/');
                el = &el.children[0];
            }
        }

        // Element name
        result.push_str(&format!("{indent_str}{prefix}{}", el.name));

        // Attributes in Pug-style parentheses
        if !el.attributes.is_empty() {
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

            let mut boolean_attrs: Vec<_> = el
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
            let mut non_boolean_attrs: Vec<_> = el
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

        if !dialect && el.renders_inline() {
            // Shallow mixed content (prose with inline spans): show the body as
            // one line of original XML, e.g. `para = The <command>x</command> …`.
            render_text(&mut result, &el.inline_xml_body(), indent);
            result.push('\n');
        } else if el.is_mixed() {
            // Mixed content: render text runs and child elements in order.
            result.push('\n');
            result.push_str(&el.render_mixed_body(indent + 1, opts, registry));
        } else {
            // Text content with = assignment
            render_text(&mut result, &el.text_content, indent);

            result.push('\n');

            // Children elements
            for child in &el.children {
                result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
            }
        }

        result
    }
}
