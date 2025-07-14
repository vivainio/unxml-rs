use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;
use glob::glob;
use quick_xml::Reader;
use quick_xml::events::Event;
use scraper::{ElementRef, Html, Selector};

#[derive(Parser)]
#[command(name = "unxml")]
#[command(about = "Simplify and 'flatten' XML and HTML files")]
#[command(version = "1.0.0")]
struct Cli {
    /// XML or HTML files to process (supports glob patterns)
    files: Vec<String>,

    /// Force input format (xml or html). If not specified, format is auto-detected
    #[arg(short, long)]
    format: Option<String>,

    /// Enable proprietary special element handling rules
    #[arg(long)]
    special: bool,

    /// Read input from stdin (assumes XML format)
    #[arg(long)]
    stdin: bool,
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

    fn format_yaml_like(&self, indent: usize, special: bool) -> String {
        let mut result = String::new();
        let indent_str = "  ".repeat(indent);

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
            };

            // Always process the modified element normally (section should still appear)
            result.push_str(&modified_element.format_yaml_like(indent + 1, special));

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
            };

            // Special handling for section elements after include processing
            if self.name == "section" {
                // For sections, check if after removing include, we only have a name attribute
                if let Some(name) = modified_element.attributes.get("name") {
                    if modified_element.attributes.len() == 1 {
                        // Apply the #name transformation
                        result.push_str(&format!("{}#{}", "  ".repeat(indent + 1), name));
                        result.push('\n');

                        // Process children elements
                        for child in &modified_element.children {
                            result.push_str(&child.format_yaml_like(indent + 2, special));
                        }

                        return result;
                    }
                }
            }

            // Process the modified element - if it has no attributes left and no text content,
            // just process its children directly
            if modified_element.attributes.is_empty()
                && modified_element.text_content.trim().is_empty()
            {
                // Process children directly
                for child in &modified_element.children {
                    result.push_str(&child.format_yaml_like(indent + 1, special));
                }
            } else {
                // Process the modified element normally
                result.push_str(&modified_element.format_yaml_like(indent + 1, special));
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
                            result.push_str(&child.format_yaml_like(indent + 1, special));
                        }

                        return result;
                    }
                }
                "parameter" => {
                    // Only apply special transformation if element has only the 'name' attribute
                    if let Some(name) = self.attributes.get("name") {
                        if self.attributes.len() == 1 {
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
                                result.push_str(&child.format_yaml_like(indent + 1, special));
                            }

                            return result;
                        }
                    }
                }
                "variable" => {
                    // Only apply special transformation if element has only the 'name' attribute
                    if let Some(name) = self.attributes.get("name") {
                        if self.attributes.len() == 1 {
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
                                result.push_str(&child.format_yaml_like(indent + 1, special));
                            }

                            return result;
                        }
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
                            result.push_str(&child.format_yaml_like(indent + 1, special));
                        }

                        return result;
                    }
                }
                "section" => {
                    // Only apply special transformation if element has only the 'name' attribute
                    if let Some(name) = self.attributes.get("name") {
                        if self.attributes.len() == 1 {
                            result.push_str(&format!("{indent_str}#{name}"));
                            result.push('\n');

                            // Process children elements
                            for child in &self.children {
                                result.push_str(&child.format_yaml_like(indent + 1, special));
                            }

                            return result;
                        }
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

                            if !attr_parts.is_empty() {
                                result.push_str(&format!("({})", attr_parts.join(", ")));
                            }
                        }

                        // Text content with = assignment
                        if !self.text_content.trim().is_empty() {
                            result.push_str(&format!(" = {}", self.text_content.trim()));
                        }

                        result.push('\n');

                        // Process children elements
                        for child in &self.children {
                            result.push_str(&child.format_yaml_like(indent + 1, special));
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

            if !attr_parts.is_empty() {
                result.push_str(&format!("({})", attr_parts.join(", ")));
            }
        }

        // Text content with = assignment
        if !self.text_content.trim().is_empty() {
            result.push_str(&format!(" = {}", self.text_content.trim()));
        }

        result.push('\n');

        // Children elements
        for child in &self.children {
            result.push_str(&child.format_yaml_like(indent + 1, special));
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

    // Process child elements first to know if we have any
    for child in element.children() {
        if let Some(child_element) = ElementRef::wrap(child) {
            xml_element
                .children
                .push(convert_element_to_xml(child_element, format));
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

fn process_content(
    content: &str,
    file_path: &str,
    format_override: Option<&str>,
    special: bool,
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
    let elements = match format {
        InputFormat::Html => parse_html(content, &format).context("Failed to parse HTML")?,
        InputFormat::Xml => parse_xml(content).context("Failed to parse XML")?,
    };

    // Format output
    let mut output = String::new();
    for element in elements {
        output.push_str(&element.format_yaml_like(0, special));
    }

    Ok(output)
}

fn process_file(file_path: &str, format_override: Option<&str>, special: bool) -> Result<String> {
    // Read the file
    let content = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read file: {file_path}"))?;

    process_content(&content, file_path, format_override, special)
}

fn process_stdin(format_override: Option<&str>, special: bool) -> Result<String> {
    // Read from stdin
    let mut content = String::new();
    io::stdin()
        .read_to_string(&mut content)
        .context("Failed to read from stdin")?;

    process_content(&content, "stdin", format_override, special)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle stdin input
    if cli.stdin {
        // When using stdin, files should be empty
        if !cli.files.is_empty() {
            return Err(anyhow::anyhow!(
                "Cannot specify both --stdin and file arguments"
            ));
        }

        // Process stdin input
        match process_stdin(cli.format.as_deref(), cli.special) {
            Ok(output) => print!("{output}"),
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

    // Process each file
    for (i, file_path) in all_files.iter().enumerate() {
        // Add file separator comment (except for the first file)
        if i > 0 {
            println!();
        }

        // Add file header comment only if there are multiple files
        if all_files.len() > 1 {
            println!("// FILE: {file_path}");
        }

        // Process and output the file
        match process_file(file_path, cli.format.as_deref(), cli.special) {
            Ok(output) => print!("{output}"),
            Err(e) => {
                eprintln!("Error processing file '{file_path}': {e}");
                // Continue processing other files instead of stopping
            }
        }
    }

    Ok(())
}
