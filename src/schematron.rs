//! Schematron dialect rendering (schema/pattern/rule/assert/report/let).

use crate::model::{FormatOpts, XmlElement};
use crate::xslt::TemplateRegistry;

impl XmlElement {
    pub(crate) fn format_schematron_element(
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
}
