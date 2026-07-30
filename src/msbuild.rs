//! Readability-oriented rendering for MSBuild projects.

use std::collections::HashSet;

use crate::model::{FormatOpts, NodeRef, XmlElement};
use crate::render::{current_col, push_comment, render_attrs, render_text};
use crate::xslt::TemplateRegistry;

fn escaped(value: &str) -> String {
    value.replace('"', "&quot;")
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", escaped(value))
}

fn clean_condition(condition: &str) -> String {
    condition.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Split a semicolon-delimited MSBuild list without splitting semicolons inside
/// quoted strings or nested property/item expressions.
fn split_list(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    let mut quote = None;

    for (index, ch) in value.char_indices() {
        match ch {
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            '(' if quote.is_none() => depth += 1,
            ')' if quote.is_none() && depth > 0 => depth -= 1,
            ';' if quote.is_none() && depth == 0 => {
                let part = value[start..index].trim();
                if !part.is_empty() {
                    parts.push(part);
                }
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }

    let part = value[start..].trim();
    if !part.is_empty() {
        parts.push(part);
    }
    parts
}

fn attribute_parts(element: &XmlElement, skip: &[&str]) -> Vec<String> {
    let skip: HashSet<_> = skip.iter().copied().collect();
    let mut attributes: Vec<_> = element
        .attributes
        .iter()
        .filter(|(key, _)| !skip.contains(key.as_str()))
        .collect();
    attributes.sort_by_key(|(key, _)| *key);
    attributes
        .into_iter()
        .map(|(key, value)| format!("{key}={}", quoted(value)))
        .collect()
}

fn finish_heading(
    element: &XmlElement,
    mut result: String,
    attr_parts: &[String],
    indent: usize,
    registry: Option<&TemplateRegistry>,
) -> String {
    let col = current_col(&result);
    result.push_str(&render_attrs(attr_parts, col, indent, false));
    render_text(&mut result, &element.text_content, indent);
    result.push('\n');
    result.push_str(&element.render_children(indent + 1, &FormatOpts::MSBUILD, registry));
    result
}

impl XmlElement {
    fn is_self_named_property_copy(&self) -> bool {
        self.attributes.is_empty()
            && self.children.is_empty()
            && self.text_content.trim() == format!("$({})", self.name)
    }

    fn render_child_run(
        &self,
        child_indexes: &[usize],
        indent: usize,
        registry: Option<&TemplateRegistry>,
    ) -> String {
        let mut result = String::new();
        let mut index = 0;
        while index < child_indexes.len() {
            let run_start = index;
            while index < child_indexes.len()
                && self.children[child_indexes[index]].is_self_named_property_copy()
            {
                index += 1;
            }

            if index - run_start >= 2 {
                result.push_str(&format!("{}copy properties:\n", "  ".repeat(indent)));
                let name_indent = "  ".repeat(indent + 1);
                for child_index in &child_indexes[run_start..index] {
                    result.push_str(&format!(
                        "{name_indent}{}\n",
                        self.children[*child_index].name
                    ));
                }
            } else if index - run_start == 1 {
                result.push_str(&self.children[child_indexes[run_start]].format_yaml_like(
                    indent,
                    &FormatOpts::MSBUILD,
                    registry,
                ));
            }

            if index < child_indexes.len() {
                result.push_str(&self.children[child_indexes[index]].format_yaml_like(
                    indent,
                    &FormatOpts::MSBUILD,
                    registry,
                ));
                index += 1;
            }
        }
        result
    }

    pub(crate) fn render_msbuild_children(
        &self,
        indent: usize,
        registry: Option<&TemplateRegistry>,
    ) -> String {
        if !self
            .nodes
            .iter()
            .any(|node| matches!(node, NodeRef::Comment { .. }))
        {
            let indexes: Vec<_> = (0..self.children.len()).collect();
            return self.render_child_run(&indexes, indent, registry);
        }

        let mut result = String::new();
        let mut run = Vec::new();
        for node in &self.nodes {
            match node {
                NodeRef::Child(index) => run.push(*index),
                NodeRef::Comment { text, inline } => {
                    result.push_str(&self.render_child_run(&run, indent, registry));
                    run.clear();
                    push_comment(&mut result, text, *inline, indent);
                }
                NodeRef::Text(_) => {}
            }
        }
        result.push_str(&self.render_child_run(&run, indent, registry));
        result
    }

    fn format_choose(&self, indent: usize, registry: Option<&TemplateRegistry>) -> Option<String> {
        if self.name != "Choose"
            || !self.attributes.is_empty()
            || self
                .nodes
                .iter()
                .any(|node| matches!(node, NodeRef::Comment { .. }))
            || self.children.is_empty()
        {
            return None;
        }

        let mut result = String::new();
        let mut saw_when = false;
        let mut saw_otherwise = false;

        for branch in &self.children {
            let branch_indent = "  ".repeat(indent);
            match branch.name.as_str() {
                "When" if !saw_otherwise => {
                    let condition = branch.attributes.get("Condition")?;
                    if branch.attributes.len() != 1 {
                        return None;
                    }
                    let keyword = if saw_when { "else if" } else { "if" };
                    result.push_str(&format!(
                        "{branch_indent}{keyword} {}:\n",
                        clean_condition(condition)
                    ));
                    result.push_str(&branch.render_children(
                        indent + 1,
                        &FormatOpts::MSBUILD,
                        registry,
                    ));
                    saw_when = true;
                }
                "Otherwise" if saw_when && !saw_otherwise && branch.attributes.is_empty() => {
                    result.push_str(&format!("{branch_indent}else:\n"));
                    result.push_str(&branch.render_children(
                        indent + 1,
                        &FormatOpts::MSBUILD,
                        registry,
                    ));
                    saw_otherwise = true;
                }
                _ => return None,
            }
        }

        saw_when.then_some(result)
    }

    fn format_target(
        &self,
        indent: usize,
        indent_str: &str,
        registry: Option<&TemplateRegistry>,
    ) -> Option<String> {
        let name = self.attributes.get("Name")?;
        let dependencies = self
            .attributes
            .get("DependsOnTargets")
            .map(|value| split_list(value));

        if let Some(dependencies) = dependencies
            && dependencies.len() > 1
        {
            let mut attributes: Vec<_> = self
                .attributes
                .iter()
                .filter(|(key, _)| key.as_str() != "Name" && key.as_str() != "DependsOnTargets")
                .collect();
            attributes.sort_by_key(|(key, _)| *key);

            let attr_indent = "  ".repeat(indent + 2);
            let item_indent = "  ".repeat(indent + 3);
            let mut result = format!("{indent_str}Target {name}(\n");
            result.push_str(&format!("{attr_indent}DependsOnTargets=[\n"));
            for dependency in dependencies {
                result.push_str(&format!("{item_indent}{dependency}\n"));
            }
            result.push_str(&format!("{attr_indent}]"));
            if attributes.is_empty() {
                result.push(')');
            } else {
                result.push_str(",\n");
                for (index, (key, value)) in attributes.iter().enumerate() {
                    result.push_str(&format!("{attr_indent}{key}={}", quoted(value)));
                    if index + 1 == attributes.len() {
                        result.push(')');
                    } else {
                        result.push_str(",\n");
                    }
                }
            }
            render_text(&mut result, &self.text_content, indent);
            result.push('\n');
            result.push_str(&self.render_children(indent + 1, &FormatOpts::MSBUILD, registry));
            return Some(result);
        }

        Some(finish_heading(
            self,
            format!("{indent_str}Target {name}"),
            &attribute_parts(self, &["Name"]),
            indent,
            registry,
        ))
    }

    fn format_promoted_attribute(
        &self,
        indent: usize,
        indent_str: &str,
        element_name: &str,
        attribute: &str,
        quote_value: bool,
        registry: Option<&TemplateRegistry>,
    ) -> Option<String> {
        let value = self.attributes.get(attribute)?;
        let value = if quote_value {
            quoted(value)
        } else {
            value.clone()
        };
        Some(finish_heading(
            self,
            format!("{indent_str}{element_name} {value}"),
            &attribute_parts(self, &[attribute]),
            indent,
            registry,
        ))
    }

    fn format_item_operation(
        &self,
        indent: usize,
        indent_str: &str,
        registry: Option<&TemplateRegistry>,
    ) -> Option<String> {
        let operations = [("Include", "+="), ("Remove", "-="), ("Update", "update")];
        let present: Vec<_> = operations
            .iter()
            .filter_map(|(attribute, keyword)| {
                self.attributes
                    .get(*attribute)
                    .map(|value| (*attribute, *keyword, value))
            })
            .collect();
        if present.len() != 1 {
            return None;
        }
        let (attribute, keyword, value) = present[0];
        Some(finish_heading(
            self,
            format!("{indent_str}{} {keyword} {}", self.name, quoted(value)),
            &attribute_parts(self, &[attribute]),
            indent,
            registry,
        ))
    }

    pub(crate) fn format_msbuild_element(
        &self,
        indent: usize,
        indent_str: &str,
        registry: Option<&TemplateRegistry>,
    ) -> Option<String> {
        if let Some(condition) = self.attributes.get("Condition") {
            let mut result = format!("{indent_str}if {}:\n", clean_condition(condition));
            let mut rest = self.clone();
            rest.attributes.remove("Condition");
            result.push_str(&rest.format_yaml_like(indent + 1, &FormatOpts::MSBUILD, registry));
            return Some(result);
        }

        if let Some(result) = self.format_choose(indent, registry) {
            return Some(result);
        }

        match self.name.as_str() {
            "Target" => self.format_target(indent, indent_str, registry),
            "Import" => self
                .format_promoted_attribute(indent, indent_str, "Import", "Project", true, registry),
            "UsingTask" => self.format_promoted_attribute(
                indent,
                indent_str,
                "UsingTask",
                "TaskName",
                false,
                registry,
            ),
            "PropertyGroup" if self.attributes.contains_key("Label") => self
                .format_promoted_attribute(
                    indent,
                    indent_str,
                    "PropertyGroup",
                    "Label",
                    true,
                    registry,
                ),
            _ => self.format_item_operation(indent, indent_str, registry),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::split_list;

    #[test]
    fn dependency_lists_preserve_nested_semicolons() {
        assert_eq!(
            split_list("Prepare;$([System.String]::Join(';', @(Items)));Build"),
            [
                "Prepare",
                "$([System.String]::Join(';', @(Items)))",
                "Build"
            ]
        );
    }
}
