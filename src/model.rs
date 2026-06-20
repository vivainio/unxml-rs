//! Core data model: the parsed-element tree and formatting options.

use std::collections::HashMap;

use crate::xslt::TemplateRegistry;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FormatOpts {
    pub(crate) special: bool,
    pub(crate) xslt: bool,
    pub(crate) schematron: bool,
    pub(crate) xsd: bool,
    pub(crate) wsdl: bool,
}

impl FormatOpts {
    pub(crate) const XSLT: FormatOpts = FormatOpts {
        special: false,
        xslt: true,
        schematron: false,
        xsd: false,
        wsdl: false,
    };

    /// True if the user explicitly selected any processing mode. When none is
    /// set we fall back to autodetecting the mode from the file extension.
    pub(crate) fn has_mode(&self) -> bool {
        self.special || self.xslt || self.schematron || self.xsd || self.wsdl
    }
}

/// One item in an element's content in document order. `Text` is a literal text
/// run; `Child` is an index into the element's `children`. This preserves the
/// faithful interleaving of text and child elements (mixed content) that the
/// flat `text_content` + `children` split alone cannot represent.
#[derive(Debug, Clone)]
pub(crate) enum NodeRef {
    Text(String),
    Child(usize),
}

#[derive(Debug, Clone)]
pub(crate) struct XmlElement {
    pub(crate) name: String,
    pub(crate) attributes: HashMap<String, String>,
    /// All text runs concatenated — kept for the common scalar case
    /// (`<a>text</a>` → `a = text`) and for paths that don't need ordering.
    pub(crate) text_content: String,
    pub(crate) children: Vec<XmlElement>,
    /// Document-order view of text runs and child elements. Used to render mixed
    /// content faithfully; empty on elements built outside the parsers.
    pub(crate) nodes: Vec<NodeRef>,
}

impl XmlElement {
    pub(crate) fn new(name: String) -> Self {
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
    pub(crate) fn is_mixed(&self) -> bool {
        let has_text = self
            .nodes
            .iter()
            .any(|n| matches!(n, NodeRef::Text(t) if !t.trim().is_empty()));
        let has_child = self.nodes.iter().any(|n| matches!(n, NodeRef::Child(_)));
        has_text && has_child
    }

    /// True when this element has something to render beneath it — a child
    /// element or a non-empty text run. Used to decide whether a block keyword
    /// (`call`, `element`) should carry a trailing `:`.
    pub(crate) fn has_renderable_body(&self) -> bool {
        self.nodes.iter().any(|n| match n {
            NodeRef::Text(t) => !t.trim().is_empty(),
            NodeRef::Child(_) => true,
        })
    }

    /// Render this element's content in document order: text runs as quoted
    /// lines, child elements recursed. Used for mixed content in every mode so
    /// a text run between two elements keeps its position.
    pub(crate) fn render_mixed_body(
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
}
