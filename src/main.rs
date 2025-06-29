use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;
use quick_xml::Reader;
use quick_xml::events::Event;
use scraper::{Html, ElementRef, Selector};

#[derive(Parser)]
#[command(name = "unxml")]
#[command(about = "Simplify and 'flatten' XML and HTML files")]
#[command(version = "1.0.0")]
struct Cli {
    /// XML or HTML file to process
    file: String,
    
    /// Force input format (xml or html). If not specified, format is auto-detected
    #[arg(short, long)]
    format: Option<String>,
}

#[derive(Debug, Clone)]
struct XmlElement {
    name: String,
    attributes: HashMap<String, String>,
    text_content: String,
    children: Vec<XmlElement>,
}

impl XmlElement {
    fn new(name: String) -> Self {
        Self {
            name,
            attributes: HashMap::new(),
            text_content: String::new(),
            children: Vec::new(),
        }
    }

    fn format_yaml_like(&self, indent: usize) -> String {
        let mut result = String::new();
        let indent_str = "  ".repeat(indent);

        // Element name
        result.push_str(&format!("{}{}", indent_str, self.name));

        // Attributes in [square brackets]
        if !self.attributes.is_empty() {
            for (key, value) in &self.attributes {
                result.push_str(&format!("\n{indent_str}  [{key}]: {value}"));
            }
        }

        // Text content with = assignment
        if !self.text_content.trim().is_empty() {
            result.push_str(&format!(" = {}", self.text_content.trim()));
        }

        result.push('\n');

        // Children elements
        for child in &self.children {
            result.push_str(&child.format_yaml_like(indent + 1));
        }

        result
    }
}

#[derive(Debug, PartialEq)]
enum InputFormat {
    Xml,
    Html,
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
        || content_lower.contains("<body>") {
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
            xml_element.attributes.insert(attr_name.to_string(), attr_value.to_string());
        }
    }

    // Process child elements first to know if we have any
    for child in element.children() {
        if let Some(child_element) = ElementRef::wrap(child) {
            xml_element.children.push(convert_element_to_xml(child_element, format));
        }
    }

    // Only get text content if this element has no child elements (leaf node)
    // This matches XML behavior better and avoids duplicate text content
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
        Selector::parse("body").unwrap_or_else(|_| {
            Selector::parse("*").unwrap()
        })
    });

    // First try to find html element
    if let Some(html_element) = document.select(&selector).next() {
        root_elements.push(convert_element_to_xml(html_element, format));
    } else {
        // Fallback: get all top-level elements
        let all_selector = Selector::parse("body > *, html > *").unwrap_or_else(|_| {
            Selector::parse("*").unwrap()
        });
        
        for element in document.select(&all_selector) {
            // Only include elements that don't have a parent element in our selection
            let is_root = element.parent().map_or(true, |parent| {
                !ElementRef::wrap(parent).is_some()
            });
            
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
                        parent.children.push(completed_element);
                    } else {
                        root_elements.push(completed_element);
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().context("Failed to unescape text")?;
                let text_content = text.trim();

                if !text_content.is_empty() {
                    if let Some(current_element) = elements_stack.last_mut() {
                        if !current_element.text_content.is_empty() {
                            current_element.text_content.push(' ');
                        }
                        current_element.text_content.push_str(text_content);
                    }
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

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Read the file
    let content = fs::read_to_string(&cli.file)
        .with_context(|| format!("Failed to read file: {}", cli.file))?;

    // Determine input format
    let format = if let Some(format_str) = &cli.format {
        match format_str.to_lowercase().as_str() {
            "html" => InputFormat::Html,
            "xml" => InputFormat::Xml,
            _ => return Err(anyhow::anyhow!("Unsupported format: {}. Use 'xml' or 'html'", format_str)),
        }
    } else {
        detect_format(&content, &cli.file)
    };

    // Parse the content based on detected/specified format
    let elements = match format {
        InputFormat::Html => parse_html(&content, &format).context("Failed to parse HTML")?,
        InputFormat::Xml => parse_xml(&content).context("Failed to parse XML")?,
    };

    // Output in YAML-like format
    for element in elements {
        print!("{}", element.format_yaml_like(0));
    }

    Ok(())
}
