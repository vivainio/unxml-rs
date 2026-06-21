//! Input handling: lenient file reads, format detection, and the XML/HTML
//! parsers that build the `XmlElement` tree.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::Event;
use scraper::{ElementRef, Html, Selector};

use crate::model::{NodeRef, XmlElement};

/// Read a file as text, tolerating non-UTF-8 inputs.
///
/// Many real-world XML files (e.g. SAP/EDI invoice exports) are encoded as
/// ISO-8859-1 / Latin-1 and often carry no `<?xml encoding=...?>` declaration.
/// `fs::read_to_string` rejects any non-UTF-8 byte, so we read raw bytes and
/// fall back to a Latin-1 decode (every byte 0x00-0xFF maps directly to the
/// matching Unicode code point, so this never fails).
pub(crate) fn read_file_lenient(file_path: &str) -> Result<String> {
    let bytes = fs::read(file_path).with_context(|| format!("Failed to read file: {file_path}"))?;
    Ok(match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => e.into_bytes().into_iter().map(|b| b as char).collect(),
    })
}

#[derive(Debug, PartialEq)]
pub(crate) enum InputFormat {
    Xml,
    Html,
}

pub(crate) fn detect_format(content: &str, file_path: &str) -> InputFormat {
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

pub(crate) fn parse_xml(content: &str) -> Result<Vec<XmlElement>> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut elements_stack: Vec<XmlElement> = Vec::new();
    // Byte offset where each open element's inner content begins (just past its
    // start tag), parallel to `elements_stack`. Used to capture verbatim inner
    // source for inline mixed-content rendering.
    let mut inner_start_stack: Vec<usize> = Vec::new();
    let mut root_elements: Vec<XmlElement> = Vec::new();
    let mut buf = Vec::new();

    loop {
        // Position before reading this event: for an End event, this is where
        // the `</name>` tag begins, i.e. the end of the parent's inner content.
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut element = XmlElement::new(name);

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
                    // Decode XML entities (e.g. &lt; &gt; &amp;) so comparison
                    // operators in XSLT/XPath expressions render as < > & rather
                    // than their escaped source form.
                    let value = attr
                        .unescape_value()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|_| String::from_utf8_lossy(&attr.value).to_string());
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
