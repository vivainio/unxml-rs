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
    /// Verbatim source between this element's start and end tags, captured by the
    /// XML parser. Used to render shallow mixed content (prose with inline spans)
    /// as a single line of original XML rather than a stack of flattened nodes.
    /// `None` for elements built outside the XML parser (e.g. the HTML path).
    pub(crate) inner_source: Option<String>,
}

impl XmlElement {
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
            attributes: HashMap::new(),
            text_content: String::new(),
            children: Vec::new(),
            nodes: Vec::new(),
            inner_source: None,
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

    /// True when this element's whole subtree can sit inside a flowing line of
    /// prose. A leaf is inline-safe only if its text is single-line (which
    /// excludes preformatted blocks like `<programlisting>`/`<screen>`, whose
    /// interior newlines are significant). An element with children is inline-
    /// safe iff every child is — so nested inline markup (`<emphasis><command>`)
    /// collapses, while a leaf with significant newlines anywhere blocks it. The
    /// incidental line wraps in an element's own mixed text are ignored here;
    /// they are collapsed away by `inline_xml_body`.
    pub(crate) fn is_inline_safe(&self) -> bool {
        if self.children.is_empty() {
            !self.text_content.trim().contains('\n')
        } else {
            self.children.iter().all(Self::is_inline_safe)
        }
    }

    /// True when this element should render inline as one line of verbatim XML:
    /// it is mixed content (real text interleaved with elements) whose every
    /// child subtree is inline-safe, and the parser captured its source.
    /// Otherwise it flattens as usual. This keeps the document skeleton
    /// bracket-free while letting prose-with-spans read as the original
    /// `text <tag>span</tag> text`.
    pub(crate) fn renders_inline(&self) -> bool {
        self.is_mixed()
            && self.inner_source.is_some()
            && self.children.iter().all(Self::is_inline_safe)
    }

    /// The inline body: this element's original inner XML, with runs of
    /// whitespace (including the source's incidental line wraps) collapsed to a
    /// single space so it sits on one line.
    pub(crate) fn inline_xml_body(&self) -> String {
        self.inner_source
            .as_deref()
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
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
