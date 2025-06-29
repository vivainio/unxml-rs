use std::collections::HashMap;
use std::fs;

use anyhow::{Context, Result};
use clap::Parser;
use quick_xml::Reader;
use quick_xml::events::Event;

#[derive(Parser)]
#[command(name = "unxml")]
#[command(about = "Simplify and 'flatten' XML files")]
#[command(version = "1.0.0")]
struct Cli {
    /// XML file to process
    file: String,
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
                result.push_str(&format!("\n{}  [{}]: {}", indent_str, key, value));
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

    // Read the XML file
    let content = fs::read_to_string(&cli.file)
        .with_context(|| format!("Failed to read file: {}", cli.file))?;

    // Parse the XML
    let elements = parse_xml(&content).context("Failed to parse XML")?;

    // Output in YAML-like format
    for element in elements {
        print!("{}", element.format_yaml_like(0));
    }

    Ok(())
}
