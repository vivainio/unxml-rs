//! Input handling: lenient file reads, format detection, and the XML/HTML
//! parsers that build the `XmlElement` tree.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use glob::glob;
use quick_xml::Reader;
use quick_xml::events::Event;
use scraper::{ElementRef, Html, Selector};

use crate::model::{NodeRef, XmlElement};

/// Expand a mix of literal file paths and glob patterns (e.g. `*.xml`) into
/// the concrete list of files to process. An existing file takes precedence
/// over glob interpretation: real filenames can contain glob metacharacters
/// (e.g. `Invoice-[uuid].xml`), and an explicitly-passed file that exists
/// should be read verbatim rather than treated as a (likely non-matching)
/// pattern.
pub(crate) fn expand_file_args(patterns: &[String]) -> Result<Vec<String>> {
    let mut all_files = Vec::new();
    for pattern in patterns {
        if Path::new(pattern).is_file() {
            all_files.push(pattern.clone());
        } else if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
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
                Err(e) => bail!("Invalid glob pattern '{pattern}': {e}"),
            }
        } else {
            all_files.push(pattern.clone());
        }
    }
    Ok(all_files)
}

/// Read a file as text, tolerating non-UTF-8 inputs.
///
/// Many real-world XML files (e.g. SAP/EDI invoice exports) are encoded as
/// ISO-8859-1 / Latin-1 and often carry no `<?xml encoding=...?>` declaration.
/// `fs::read_to_string` rejects any non-UTF-8 byte, so we read raw bytes and
/// fall back to a Latin-1 decode (every byte 0x00-0xFF maps directly to the
/// matching Unicode code point, so this never fails).
pub(crate) fn read_file_lenient(file_path: &str) -> Result<String> {
    let bytes = fs::read(file_path).with_context(|| format!("Failed to read file: {file_path}"))?;
    Ok(decode_lenient(bytes))
}

/// Decode bytes as UTF-8, falling back to Latin-1 (see `read_file_lenient`).
pub(crate) fn decode_lenient(bytes: Vec<u8>) -> String {
    decode_with_fallback(bytes).0
}

/// `decode_lenient`, also reporting whether the Latin-1 fallback was used —
/// byte offsets into the decoded text then differ from the original bytes.
pub(crate) fn decode_with_fallback(bytes: Vec<u8>) -> (String, bool) {
    match String::from_utf8(bytes) {
        Ok(text) => (text, false),
        Err(e) => (
            e.into_bytes().into_iter().map(|b| b as char).collect(),
            true,
        ),
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum InputFormat {
    Xml,
    Html,
    Json,
}

impl InputFormat {
    pub(crate) fn syntax_name(&self) -> &'static str {
        match self {
            Self::Xml => "XML",
            Self::Html => "HTML",
            Self::Json => "JSON",
        }
    }
}

/// The format implied by a file's extension alone, if it is decisive.
pub(crate) fn format_from_extension(file_path: &str) -> Option<InputFormat> {
    let ext = Path::new(file_path)
        .extension()?
        .to_string_lossy()
        .to_lowercase();
    match ext.as_str() {
        "html" | "htm" => Some(InputFormat::Html),
        "xml" | "xsl" | "xsd" | "wsdl" => Some(InputFormat::Xml),
        "json" => Some(InputFormat::Json),
        _ => None,
    }
}

pub(crate) fn detect_format(content: &str, file_path: &str) -> InputFormat {
    // Check file extension first
    if let Some(format) = format_from_extension(file_path) {
        return format;
    }

    // Check content for HTML-specific indicators
    let content_lower = content.to_lowercase();

    // JSON has no declaration. For extensionless input (notably stdin), only
    // claim an object/array when it parses successfully so XML fragments and
    // ordinary text keep the established XML fallback.
    let trimmed = content.trim_start();
    if matches!(trimmed.as_bytes().first(), Some(b'{') | Some(b'['))
        && serde_json::from_str::<serde_json::Value>(content).is_ok()
    {
        return InputFormat::Json;
    }

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

pub(crate) fn convert_element_to_xml(element: ElementRef, format: &InputFormat) -> XmlElement {
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

pub(crate) fn parse_html(content: &str, format: &InputFormat) -> Result<Vec<XmlElement>> {
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

/// The result of parsing an XML document: its root element(s) plus any
/// comments that live outside them (in the prolog or epilog). A top-level
/// comment is paired with the number of roots that preceded it, i.e. its
/// insertion point in document order, so the renderer can place it back.
pub(crate) struct ParsedXml {
    pub(crate) roots: Vec<XmlElement>,
    pub(crate) top_comments: Vec<(usize, String)>,
}

/// Byte offset (0-indexed) where each source line begins, for translating a
/// `quick_xml` `buffer_position()` into a 1-indexed line number. `line_starts[0]`
/// is always `0`; the line containing a given offset is
/// `line_starts.partition_point(|&s| s <= offset)`.
fn line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(
        content
            .bytes()
            .enumerate()
            .filter(|(_, b)| *b == b'\n')
            .map(|(i, _)| i + 1),
    );
    starts
}

fn offset_to_line(line_starts: &[usize], offset: usize) -> usize {
    line_starts.partition_point(|&s| s <= offset)
}

pub(crate) fn parse_xml(content: &str) -> Result<ParsedXml> {
    // quick-xml skips a leading byte-order mark without counting it in
    // `buffer_position()`, so every offset would land that many bytes early.
    // Parse the text after it instead, and add it back for `byte_range`.
    let bom = if content.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    };
    let content = &content[bom..];
    let line_starts = line_starts(content);
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut top_comments: Vec<(usize, String)> = Vec::new();
    let mut elements_stack: Vec<XmlElement> = Vec::new();
    // Byte offset of each open element's start tag, parallel to `elements_stack`.
    let mut tag_start_stack: Vec<usize> = Vec::new();
    // Byte offset where each open element's inner content begins (just past its
    // start tag), parallel to `elements_stack`. Used to capture verbatim inner
    // source for inline mixed-content rendering.
    let mut inner_start_stack: Vec<usize> = Vec::new();
    let mut root_elements: Vec<XmlElement> = Vec::new();
    let mut buf = Vec::new();
    // Byte offset just past the most recently closed sibling element (End or
    // Empty). A comment is "inline" (rides the previous line) when the source
    // between that offset and the comment's start holds no newline — i.e. they
    // were on the same source line. Updated only on element-ending events so the
    // intervening whitespace (its own trimmed Text event) is still in the slice.
    let mut last_sibling_end: usize = 0;

    loop {
        // Position before reading this event: for an End event, this is where
        // the `</name>` tag begins, i.e. the end of the parent's inner content.
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut element = XmlElement::new(name);
                // `pos_before` is unreliable here: with `trim_text(true)`,
                // insignificant whitespace between the previous tag and this
                // one is skipped silently within this same read, so
                // `pos_before` lands at the *previous* tag's end, not this
                // tag's `<`. Derive the true start by walking back from the
                // now-current (post-tag) position by this tag's raw length
                // (`<` + `e.as_ref()` + `>`) — the same trick already used
                // below for a comment's start offset.
                let tag_start = (reader.buffer_position() as usize).saturating_sub(e.len() + 2);
                element.start_line = offset_to_line(&line_starts, tag_start);

                // Parse attributes
                for attr in e.attributes() {
                    let attr = attr.context("Failed to parse attribute")?;
                    let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                    // Decode XML entities (e.g. &lt; &gt; &amp;) so comparison
                    // operators in XSLT/XPath expressions render as < > & rather
                    // than their escaped source form.
                    let value = attr
                        .unescape_value()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|_| String::from_utf8_lossy(&attr.value).to_string());
                    element.attributes.insert(key, value);
                }

                elements_stack.push(element);
                tag_start_stack.push(tag_start);
                // Inner content starts right after the start tag we just read.
                inner_start_stack.push(reader.buffer_position() as usize);
            }
            Ok(Event::End(_)) => {
                if let Some(mut completed_element) = elements_stack.pop() {
                    if let Some(inner_start) = inner_start_stack.pop()
                        && inner_start <= pos_before
                    {
                        completed_element.inner_source =
                            content.get(inner_start..pos_before).map(str::to_string);
                    }
                    completed_element.end_line =
                        offset_to_line(&line_starts, reader.buffer_position() as usize);
                    if let Some(tag_start) = tag_start_stack.pop() {
                        completed_element.byte_range =
                            Some((bom + tag_start, bom + reader.buffer_position() as usize));
                    }
                    if let Some(parent) = elements_stack.last_mut() {
                        parent.nodes.push(NodeRef::Child(parent.children.len()));
                        parent.children.push(completed_element);
                    } else {
                        root_elements.push(completed_element);
                    }
                    last_sibling_end = reader.buffer_position() as usize;
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
            Ok(Event::CData(ref e)) => {
                // CDATA is literal character data, not an ignorable declaration.
                // Keep its whitespace and do not entity-decode its contents.
                let text = std::str::from_utf8(e.as_ref()).context("Invalid UTF-8 in CDATA")?;
                if let Some(current) = elements_stack.last_mut() {
                    current.text_content.push_str(text);
                    current.nodes.push(NodeRef::Text(text.to_string()));
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut element = XmlElement::new(name);
                // See the matching comment in the `Start` arm: `pos_before` is
                // unreliable, so derive the start from the post-tag position
                // minus this self-closing tag's raw length (`<` + `e.as_ref()`
                // + `/>`).
                let tag_start = (reader.buffer_position() as usize).saturating_sub(e.len() + 3);
                element.start_line = offset_to_line(&line_starts, tag_start);

                // Parse attributes for empty elements
                for attr in e.attributes() {
                    let attr = attr.context("Failed to parse attribute")?;
                    let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                    // Decode XML entities (e.g. &lt; &gt; &amp;) so comparison
                    // operators in XSLT/XPath expressions render as < > & rather
                    // than their escaped source form.
                    let value = attr
                        .unescape_value()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|_| String::from_utf8_lossy(&attr.value).to_string());
                    element.attributes.insert(key, value);
                }

                element.end_line = offset_to_line(&line_starts, reader.buffer_position() as usize);
                element.byte_range =
                    Some((bom + tag_start, bom + reader.buffer_position() as usize));
                if let Some(parent) = elements_stack.last_mut() {
                    parent.nodes.push(NodeRef::Child(parent.children.len()));
                    parent.children.push(element);
                } else {
                    root_elements.push(element);
                }
                last_sibling_end = reader.buffer_position() as usize;
            }
            Ok(Event::Comment(ref e)) => {
                // Comments are content: keep them in document order so they
                // render and diff. A comment inside an element rides on that
                // element's node list; a top-level comment (prolog/epilog, no
                // open element — e.g. a licence header) is recorded separately
                // with its insertion point among the roots.
                let text = e
                    .unescape()
                    .map(|c| c.into_owned())
                    .unwrap_or_else(|_| String::from_utf8_lossy(e.as_ref()).into_owned());
                let text = text.trim();
                if !text.is_empty() {
                    // Inline when the previous sibling closed on this same line:
                    // the source between its end and this comment holds no
                    // newline. The comment's start is derived from its end
                    // position and token length (`<!--` + raw inner + `-->`)
                    // because `trim_text` drops the intervening whitespace, so
                    // `pos_before` alone would not locate the `<!--`.
                    let comment_start =
                        (reader.buffer_position() as usize).saturating_sub(e.len() + 7);
                    let inline = last_sibling_end > 0
                        && content
                            .get(last_sibling_end..comment_start)
                            .is_some_and(|gap| !gap.contains('\n'));
                    if let Some(current) = elements_stack.last_mut() {
                        let inline =
                            inline && matches!(current.nodes.last(), Some(NodeRef::Child(_)));
                        current.nodes.push(NodeRef::Comment {
                            text: text.to_string(),
                            inline,
                        });
                    } else {
                        top_comments.push((root_elements.len(), text.to_string()));
                    }
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
            _ => {} // Ignore declarations and processing instructions.
        }
        buf.clear();
    }

    Ok(ParsedXml {
        roots: root_elements,
        top_comments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_ranges_slice_back_to_each_element_even_after_a_bom() {
        for prefix in ["", "\u{feff}"] {
            let xml = format!(
                "{prefix}<?xml version=\"1.0\"?>\r\n<r>\r\n  <a k=\"v\">caf\u{e9}</a>\r\n  <b/>\r\n</r>"
            );
            let root = &parse_xml(&xml).unwrap().roots[0];
            let slice = |e: &XmlElement| {
                let (start, end) = e.byte_range.unwrap();
                &xml[start..end]
            };
            assert!(slice(root).starts_with("<r>") && slice(root).ends_with("</r>"));
            assert_eq!(slice(&root.children[0]), "<a k=\"v\">caf\u{e9}</a>");
            assert_eq!(slice(&root.children[1]), "<b/>");
            assert_eq!(root.children[0].start_line, 3);
        }
    }

    /// Regression test for a bug the `outline` feature surfaced: with
    /// `trim_text(true)`, `quick_xml` silently skips insignificant whitespace
    /// between tags rather than emitting a Text event for it, so a naive
    /// "position before this read" offset lands at the *previous* tag's end,
    /// not this tag's `<` — drifting further off with every extra blank line
    /// between elements. Each sibling below is separated by a different
    /// amount of blank-line padding specifically to catch that drift
    /// accumulating (a fix that's merely "off by one" everywhere would still
    /// fail this).
    #[test]
    fn start_and_end_lines_are_exact_despite_varying_inter_tag_whitespace() {
        let xml = "<root>\n\
                    <a>1</a>\n\
                    \n\
                    <b>2</b>\n\
                    \n\n\
                    <c>\n  <d/>\n</c>\n\
                    </root>\n";
        let root = &parse_xml(xml).unwrap().roots[0];
        assert_eq!((root.start_line, root.end_line), (1, 10));
        assert_eq!(
            (root.children[0].start_line, root.children[0].end_line),
            (2, 2)
        );
        assert_eq!(
            (root.children[1].start_line, root.children[1].end_line),
            (4, 4)
        );
        assert_eq!(
            (root.children[2].start_line, root.children[2].end_line),
            (7, 9)
        );
        let d = &root.children[2].children[0];
        assert_eq!((d.start_line, d.end_line), (8, 8));
    }
}
