//! `--select`'s pattern language: a small XPath subset over the parsed
//! `XmlElement` tree.
//!
//! Supported:
//! - location paths: `/a/b` (absolute), `a/b` and `//a` (anywhere), `a//b`
//! - name tests: `name`, `prefix:name`, `*`
//! - `..` (parent) and `.` (self) steps
//! - attribute predicates: `[@attr]` (present), `[@attr="v"]` / `[@attr='v']`
//!   (exact value), chainable: `item[@id="2"][@lang]`
//!
//! Where it deliberately differs from XPath:
//! - a relative path matches anywhere, as if it started with `//` — so the
//!   plain `--select item` means "every `item`", not "a root named item".
//! - a bare name ignores namespace prefixes (matches the local name), while a
//!   prefixed name matches the full name; no namespace URIs are involved.
//!   Attribute names match the same way.
//! - the result renders as the *topmost* selected elements: one nested
//!   inside another selected element is already shown as part of it.
//!
//! Not supported: other axes, positional or text predicates, functions,
//! operators, and selecting attributes or text instead of elements.
//! (`pathsel.rs` is a different, `name[k]` anchor syntax used by diff/patch.)

use anyhow::{Result, bail};

use crate::document::name_matches_select;
use crate::model::XmlElement;

/// A parsed `--select` pattern; see the module docs for the syntax.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct XPathMini {
    steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq)]
struct Step {
    axis: Axis,
    test: NodeTest,
}

/// How a step reaches from each context element: `/` or `//`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Axis {
    Child,
    Descendant,
}

#[derive(Debug, Clone, PartialEq)]
enum NodeTest {
    Parent,
    SelfNode,
    Element {
        name: String,
        attrs: Vec<AttrPredicate>,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct AttrPredicate {
    name: String,
    value: Option<String>,
}

impl XPathMini {
    pub(crate) fn parse(pattern: &str) -> Result<Self> {
        let original = pattern.trim();
        let err = |msg: &str| anyhow::anyhow!("bad --select '{original}': {msg}");

        // A leading `/` anchors the path at the document; `//` or no slash at
        // all matches anywhere.
        let (mut axis, mut rest) = if let Some(r) = original.strip_prefix("//") {
            (Axis::Descendant, r)
        } else if let Some(r) = original.strip_prefix('/') {
            (Axis::Child, r)
        } else {
            (Axis::Descendant, original)
        };

        let mut steps = Vec::new();
        loop {
            let (test, after) = parse_step(rest).map_err(|msg| err(&msg))?;
            if axis == Axis::Descendant && !matches!(test, NodeTest::Element { .. }) {
                bail!(err("'..' and '.' must follow a single '/'"));
            }
            steps.push(Step { axis, test });
            rest = after;
            if rest.is_empty() {
                break;
            }
            (axis, rest) = if let Some(r) = rest.strip_prefix("//") {
                (Axis::Descendant, r)
            } else if let Some(r) = rest.strip_prefix('/') {
                (Axis::Child, r)
            } else {
                bail!(err(&format!("unexpected '{rest}'")));
            };
        }
        Ok(Self { steps })
    }

    /// The topmost elements this path selects, in document order.
    pub(crate) fn select<'a>(&self, roots: &'a [XmlElement]) -> Vec<&'a XmlElement> {
        let tree = Tree::new(roots);
        let mut context = vec![DOCUMENT];
        for step in &self.steps {
            context = tree.apply(step, &context);
        }
        // The document node itself has nothing to render (e.g. `/root/..`),
        // and a node inside an already-selected subtree is shown within it.
        let mut out = Vec::new();
        let mut covered_until = 0;
        for id in context {
            if let Some(elem) = tree.nodes[id].elem
                && id >= covered_until
            {
                out.push(elem);
                covered_until = tree.nodes[id].end;
            }
        }
        out
    }

    /// Literals that must all appear in an XML document's raw text for it to
    /// possibly match, so a document missing one can skip parsing entirely —
    /// the main win when searching many files. Every element a path steps
    /// through must exist, so this covers all steps. Tag and attribute names
    /// can't be entity-escaped, so their local parts always qualify. A value
    /// only qualifies when it is plain (alphanumerics and `-_.:/@+`):
    /// anything else might be written with entities (`&amp;`, `&quot;`) in
    /// the source. (A plain value spelled as numeric character references,
    /// e.g. `&#52;2` for `42`, would be missed — a trade-off accepted for the
    /// speed-up.)
    pub(crate) fn required_literals(&self) -> impl Iterator<Item = &str> {
        self.steps
            .iter()
            .filter_map(|step| match &step.test {
                NodeTest::Element { name, attrs } => Some((name, attrs)),
                _ => None,
            })
            .flat_map(|(name, attrs)| {
                let names = std::iter::once(name.as_str())
                    .filter(|n| *n != "*")
                    .chain(attrs.iter().map(|p| p.name.as_str()))
                    .map(|n| n.rsplit(':').next().unwrap_or(n));
                let values = attrs.iter().filter_map(|p| p.value.as_deref()).filter(|v| {
                    !v.is_empty()
                        && v.chars()
                            .all(|c| c.is_ascii_alphanumeric() || "-_.:/@+".contains(c))
                });
                names.chain(values)
            })
    }
}

/// Parse one step at the start of `input`, returning it and the rest.
fn parse_step(input: &str) -> Result<(NodeTest, &str), String> {
    if let Some(rest) = input.strip_prefix("..") {
        return Ok((NodeTest::Parent, rest));
    }
    if let Some(rest) = input.strip_prefix('.')
        && !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_')
    {
        return Ok((NodeTest::SelfNode, rest));
    }
    let name_end = input.find(['[', '/']).unwrap_or(input.len());
    let name = &input[..name_end];
    if name.is_empty() {
        return Err("expected an element name, '*', '..' or '.'".to_string());
    }
    if name.contains(|c: char| c.is_whitespace() || "@=]\"'".contains(c)) {
        return Err(format!("invalid element name '{name}'"));
    }
    let mut rest = &input[name_end..];
    let mut attrs = Vec::new();
    while rest.starts_with('[') {
        let Some(body) = rest.strip_prefix("[@") else {
            return Err("expected '[@attr]' or '[@attr=\"value\"]'".to_string());
        };
        let Some(close) = find_predicate_end(body) else {
            return Err("unterminated '[' predicate".to_string());
        };
        attrs.push(parse_attr_predicate(&body[..close])?);
        rest = &body[close + 1..];
    }
    let test = NodeTest::Element {
        name: name.to_string(),
        attrs,
    };
    Ok((test, rest))
}

/// Index of the `]` closing a predicate body, skipping over quoted values so
/// `[@x="a]b"]` works.
fn find_predicate_end(body: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in body.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, ']') => return Some(i),
            _ => {}
        }
    }
    None
}

fn parse_attr_predicate(body: &str) -> Result<AttrPredicate, String> {
    let (name, value) = match body.split_once('=') {
        None => (body.trim(), None),
        Some((name, raw)) => {
            let raw = raw.trim();
            let unquoted = ['"', '\'']
                .iter()
                .find_map(|q| raw.strip_prefix(*q)?.strip_suffix(*q));
            let Some(value) = unquoted else {
                return Err("attribute value must be quoted, e.g. [@id=\"42\"]".to_string());
            };
            (name.trim(), Some(value.to_string()))
        }
    };
    if name.is_empty() {
        return Err("missing attribute name after '@'".to_string());
    }
    Ok(AttrPredicate {
        name: name.to_string(),
        value,
    })
}

impl NodeTest {
    fn matches(&self, elem: &XmlElement) -> bool {
        let NodeTest::Element { name, attrs } = self else {
            unreachable!("only element tests are matched against elements")
        };
        (name == "*" || name_matches_select(&elem.name, name))
            && attrs.iter().all(|pred| {
                elem.attributes.iter().any(|(key, value)| {
                    name_matches_select(key, &pred.name)
                        && pred.value.as_ref().is_none_or(|v| v == value)
                })
            })
    }
}

/// Id of the synthetic document node, the parent of the root elements.
const DOCUMENT: usize = 0;

/// The element tree flattened in document order, so node sets are sorted id
/// lists, parents are an index away, and a node's descendants are the
/// contiguous id range `id + 1 .. end`.
struct Tree<'a> {
    nodes: Vec<TreeNode<'a>>,
}

struct TreeNode<'a> {
    /// `None` only for the document node.
    elem: Option<&'a XmlElement>,
    parent: usize,
    children: Vec<usize>,
    /// One past the last id in this node's subtree.
    end: usize,
}

impl<'a> Tree<'a> {
    fn new(roots: &'a [XmlElement]) -> Self {
        let mut tree = Tree {
            nodes: vec![TreeNode {
                elem: None,
                parent: DOCUMENT,
                children: Vec::new(),
                end: 0,
            }],
        };
        for root in roots {
            tree.add(root, DOCUMENT);
        }
        tree.nodes[DOCUMENT].end = tree.nodes.len();
        tree
    }

    fn add(&mut self, elem: &'a XmlElement, parent: usize) {
        let id = self.nodes.len();
        self.nodes.push(TreeNode {
            elem: Some(elem),
            parent,
            children: Vec::new(),
            end: 0,
        });
        self.nodes[parent].children.push(id);
        for child in &elem.children {
            self.add(child, id);
        }
        self.nodes[id].end = self.nodes.len();
    }

    fn element_matches(&self, id: usize, test: &NodeTest) -> bool {
        self.nodes[id].elem.is_some_and(|e| test.matches(e))
    }

    /// Apply one step to a sorted, duplicate-free context, returning the next.
    fn apply(&self, step: &Step, context: &[usize]) -> Vec<usize> {
        let mut next = Vec::new();
        match (&step.test, step.axis) {
            (NodeTest::Parent, _) => {
                next.extend(
                    context
                        .iter()
                        .filter(|&&id| id != DOCUMENT)
                        .map(|&id| self.nodes[id].parent),
                );
            }
            (NodeTest::SelfNode, _) => next.extend_from_slice(context),
            (test, Axis::Child) => {
                for &id in context {
                    next.extend(
                        self.nodes[id]
                            .children
                            .iter()
                            .copied()
                            .filter(|&c| self.element_matches(c, test)),
                    );
                }
            }
            (test, Axis::Descendant) => {
                // Context subtrees are nested or disjoint; scan each id once.
                let mut scanned_until = 0;
                for &id in context {
                    let start = (id + 1).max(scanned_until);
                    let end = self.nodes[id].end;
                    next.extend((start..end).filter(|&d| self.element_matches(d, test)));
                    scanned_until = scanned_until.max(end);
                }
            }
        }
        next.sort_unstable();
        next.dedup();
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elem(name: &str, attrs: &[(&str, &str)], children: Vec<XmlElement>) -> XmlElement {
        let mut e = XmlElement::new(name.to_string());
        for (k, v) in attrs {
            e.attributes.insert(k.to_string(), v.to_string());
        }
        e.children = children;
        e
    }

    /// <root>
    ///   <order id="1"><qty unit="kg"/><qty unit="kg"/></order>
    ///   <order id="2"><box><qty unit="pc"/></box></order>
    ///   <cac:InvoiceLine xml:lang="fi" expr="a]b"/>
    /// </root>
    fn doc() -> Vec<XmlElement> {
        vec![elem(
            "root",
            &[],
            vec![
                elem(
                    "order",
                    &[("id", "1")],
                    vec![
                        elem("qty", &[("unit", "kg")], vec![]),
                        elem("qty", &[("unit", "kg")], vec![]),
                    ],
                ),
                elem(
                    "order",
                    &[("id", "2")],
                    vec![elem(
                        "box",
                        &[],
                        vec![elem("qty", &[("unit", "pc")], vec![])],
                    )],
                ),
                elem(
                    "cac:InvoiceLine",
                    &[("xml:lang", "fi"), ("expr", "a]b")],
                    vec![],
                ),
            ],
        )]
    }

    /// Selected elements as `name` or `name#id`.
    fn select(pattern: &str) -> Vec<String> {
        let roots = doc();
        XPathMini::parse(pattern)
            .unwrap()
            .select(&roots)
            .iter()
            .map(|e| match e.attributes.get("id") {
                Some(id) => format!("{}#{id}", e.name),
                None => e.name.clone(),
            })
            .collect()
    }

    #[test]
    fn relative_names_match_anywhere() {
        assert_eq!(select("order"), ["order#1", "order#2"]);
        assert_eq!(select("//order"), ["order#1", "order#2"]);
        assert_eq!(select("qty").len(), 3);
    }

    #[test]
    fn absolute_and_child_paths() {
        assert_eq!(select("/root/order"), ["order#1", "order#2"]);
        assert!(select("/order").is_empty());
        assert_eq!(select("order/qty").len(), 2);
        assert_eq!(select("order//qty").len(), 3);
        assert_eq!(select("/root/*/box"), ["box"]);
    }

    #[test]
    fn attribute_predicates() {
        assert_eq!(select(r#"order[@id="2"]"#), ["order#2"]);
        assert_eq!(select("order[@id='1']/qty[@unit]").len(), 2);
        assert!(select(r#"order[@id="1"][@missing]"#).is_empty());
        assert_eq!(select(r#"InvoiceLine[@lang="fi"]"#), ["cac:InvoiceLine"]);
        assert_eq!(select(r#"*[@xml:lang="fi"]"#), ["cac:InvoiceLine"]);
        assert_eq!(select(r#"*[@expr="a]b"]"#), ["cac:InvoiceLine"]);
        assert!(select("cbc:InvoiceLine").is_empty());
    }

    #[test]
    fn parent_and_self_steps() {
        // A parent holding several matches is selected once.
        assert_eq!(select(r#"qty[@unit="kg"]/.."#), ["order#1"]);
        assert_eq!(select(r#"qty[@unit="pc"]/../.."#), ["order#2"]);
        assert_eq!(select("box/../qty"), Vec::<String>::new());
        assert_eq!(select("qty/../../order[@id='2']"), ["order#2"]);
        assert_eq!(select("order/."), ["order#1", "order#2"]);
        // Up past the root reaches the document node, which renders nothing.
        assert!(select("/root/..").is_empty());
    }

    #[test]
    fn selection_is_topmost_only() {
        // `root` contains every other match, so it alone is shown.
        assert_eq!(select("*"), ["root"]);
        assert_eq!(select("*/qty/.."), ["order#1", "box"]);
    }

    #[test]
    fn malformed_patterns_are_rejected() {
        for bad in [
            "",
            "/",
            "a/",
            "a//",
            "[@id]",
            "item[id]",
            "item[@id=2]",
            r#"item[@id="2""#,
            "item[@]",
            "item[@id]x",
            "//..",
            "a//.",
            "a b",
            "@id",
        ] {
            assert!(XPathMini::parse(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn required_literals_cover_all_steps() {
        let lits = |p: &str| -> Vec<String> {
            XPathMini::parse(p)
                .unwrap()
                .required_literals()
                .map(str::to_string)
                .collect()
        };
        assert_eq!(
            lits(r#"/cac:Line[@cbc:id="A-1"]/../Note"#),
            ["Line", "id", "A-1", "Note"]
        );
        assert_eq!(lits("*[@id]"), ["id"]);
        assert_eq!(lits(r#"x[@v="a&b"][@w="two words"]"#), ["x", "v", "w"]);
    }
}
