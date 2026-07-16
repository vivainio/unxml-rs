//! MSBuild dialect: folds the `Condition="..."` attribute that MSBuild
//! permits on almost any element (Target, PropertyGroup, ItemGroup,
//! individual items, tasks, ...) into a leading `if COND:` guard instead of
//! leaving it to sort alphabetically among the element's other attributes.

use crate::model::{FormatOpts, XmlElement};
use crate::xslt::TemplateRegistry;

impl XmlElement {
    pub(crate) fn format_msbuild_element(
        &self,
        indent: usize,
        indent_str: &str,
        registry: Option<&TemplateRegistry>,
    ) -> Option<String> {
        let condition = self.attributes.get("Condition")?;
        // MSBuild conditions are routinely wrapped across source lines with
        // trailing `and`/`or`, and often padded with stray spaces
        // (`" '$(X)' == 'true' "`) — collapse to one clean line.
        let condition = condition.split_whitespace().collect::<Vec<_>>().join(" ");

        let mut result = format!("{indent_str}if {condition}:\n");

        let mut attributes = self.attributes.clone();
        attributes.remove("Condition");
        let rest = XmlElement {
            name: self.name.clone(),
            attributes,
            text_content: self.text_content.clone(),
            children: self.children.clone(),
            nodes: self.nodes.clone(),
            inner_source: self.inner_source.clone(),
        };
        result.push_str(&rest.format_yaml_like(indent + 1, &FormatOpts::MSBUILD, registry));
        Some(result)
    }
}
