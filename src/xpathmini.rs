//! `--select`'s pattern language: a small XPath subset over the parsed
//! `XmlElement` tree.
//!
//! Supported:
//! - location paths: `/a/b` (absolute), `a/b` and `//a` (anywhere), `a//b`
//! - name tests: `name`, `prefix:name`, `*`
//! - `..` (parent) and `.` (self) steps
//! - predicates, chainable (`a[p1][p2]`), each one of:
//!   - `[@attr]`, `[@attr="v"]` — attribute present / equal
//!   - `[path]`, `[path="v"]` — a relative path (`b`, `b/c`, `.//c`, `../b`,
//!     with its own predicates) selects something / something equal to `v`
//!   - `[.="v"]`, `[text()="v"]` — this element's text equals `v`
//!   - `[path/@attr="v"]`, `[path/text()="v"]` — the same, at the path's end
//!   - `[contains(X, "v")]` — as `X="v"`, but substring match
//!
//! Where it deliberately differs from XPath:
//! - a relative path matches anywhere, as if it started with `//` — so the
//!   plain `--select item` means "every `item`", not "a root named item".
//! - a bare name ignores namespace prefixes (matches the local name), while a
//!   prefixed name matches the full name; no namespace URIs are involved.
//!   Attribute names match the same way.
//! - `=` compares with surrounding whitespace trimmed from the document's
//!   text, so `<command>\n  decrypt\n</command>` equals `"decrypt"`.
//! - the result renders as the *topmost* selected elements: one nested
//!   inside another selected element is already shown as part of it.
//!
//! Not supported: other axes, positional predicates, functions other than
//! `contains`, `and`/`or`/`!=`, and selecting attributes or text instead of
//! elements. (`pathsel.rs` is a different, `name[k]` anchor syntax used by
//! diff/patch.)

use anyhow::Result;

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
        predicates: Vec<Predicate>,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum Predicate {
    Exists(ValuePath),
    Equals(ValuePath, String),
    Contains(ValuePath, String),
}

/// A predicate's operand: element steps relative to the element being
/// tested, then what to read from the elements they reach.
#[derive(Debug, Clone, PartialEq)]
struct ValuePath {
    steps: Vec<Step>,
    target: Target,
}

#[derive(Debug, Clone, PartialEq)]
enum Target {
    /// The element's full text, descendants included (XPath string-value).
    StringValue,
    /// `text()`: the element's own text.
    Text,
    /// `@name`
    Attr(String),
}

impl XPathMini {
    pub(crate) fn parse(pattern: &str) -> Result<Self> {
        let original = pattern.trim();
        let mut parser = Parser {
            src: original,
            pos: 0,
        };
        let path = parser
            .pattern()
            .map_err(|msg| anyhow::anyhow!("bad --select '{original}': {msg}"))?;
        Ok(path)
    }

    /// The topmost elements this path selects, in document order.
    pub(crate) fn select<'a>(&self, roots: &'a [XmlElement]) -> Vec<Hit<'a>> {
        let tree = Tree::new(roots);
        let context = tree.walk(&self.steps, vec![DOCUMENT]);
        // The document node itself has nothing to render (e.g. `/root/..`),
        // and a node inside an already-selected subtree is shown within it.
        let mut out = Vec::new();
        let mut covered_until = 0;
        for id in context {
            if let Some(elem) = tree.nodes[id].elem
                && id >= covered_until
            {
                out.push(Hit {
                    elem,
                    path: tree.path(id),
                });
                covered_until = tree.nodes[id].end;
            }
        }
        out
    }

    /// Literals that must all appear in an XML document's raw text for it to
    /// possibly match, so a document missing one can skip parsing entirely —
    /// the main win when searching many files. Every predicate must hold and
    /// every element a path steps through must exist, so names from all
    /// steps and predicates count. Tag and attribute names can't be
    /// entity-escaped, so their local parts always qualify. A compared value
    /// only qualifies when it is plain (alphanumerics and `-_.:/@+`):
    /// anything else might be written with entities (`&amp;`, `&quot;`) in
    /// the source. (A plain value spelled as numeric character references,
    /// e.g. `&#52;2` for `42`, would be missed — a trade-off accepted for the
    /// speed-up.)
    pub(crate) fn required_literals(&self) -> impl Iterator<Item = &str> {
        let mut out = Vec::new();
        collect_literals(&self.steps, &mut out);
        out.into_iter()
    }
}

fn collect_literals<'a>(steps: &'a [Step], out: &mut Vec<&'a str>) {
    let local = |n: &'a str| n.rsplit(':').next().unwrap_or(n);
    let plain = |v: &str| {
        !v.is_empty()
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.:/@+".contains(c))
    };
    for step in steps {
        let NodeTest::Element { name, predicates } = &step.test else {
            continue;
        };
        if name != "*" {
            out.push(local(name));
        }
        for predicate in predicates {
            let (path, value) = match predicate {
                Predicate::Exists(path) => (path, None),
                Predicate::Equals(path, v) | Predicate::Contains(path, v) => (path, Some(v)),
            };
            collect_literals(&path.steps, out);
            if let Target::Attr(attr) = &path.target {
                out.push(local(attr));
            }
            if let Some(v) = value.filter(|v| plain(v)) {
                out.push(v);
            }
        }
    }
}

/// A selected element and its unique `name[k]/name[k]` path from the root —
/// the anchor syntax `unxml diff`/`patch` use (see `pathsel.rs`).
pub(crate) struct Hit<'a> {
    pub(crate) elem: &'a XmlElement,
    pub(crate) path: String,
}

/// Recursive-descent parser over the pattern text. Errors are plain messages;
/// `XPathMini::parse` adds the pattern for context.
struct Parser<'s> {
    src: &'s str,
    pos: usize,
}

type ParseResult<T> = std::result::Result<T, String>;

impl<'s> Parser<'s> {
    fn rest(&self) -> &'s str {
        &self.src[self.pos..]
    }

    fn eat(&mut self, token: &str) -> bool {
        if self.rest().starts_with(token) {
            self.pos += token.len();
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        let trimmed = self.rest().trim_start();
        self.pos = self.src.len() - trimmed.len();
    }

    fn expect(&mut self, token: &str) -> ParseResult<()> {
        self.skip_ws();
        if self.eat(token) {
            Ok(())
        } else {
            Err(self.unexpected(&format!("'{token}'")))
        }
    }

    fn unexpected(&self, wanted: &str) -> String {
        match self.rest() {
            "" => format!("expected {wanted} at the end"),
            rest => format!("expected {wanted} at '{rest}'"),
        }
    }

    /// The whole `--select` pattern. A leading `/` anchors it at the
    /// document; `//` or no slash at all matches anywhere.
    fn pattern(&mut self) -> ParseResult<XPathMini> {
        let axis = if self.eat("//") {
            Axis::Descendant
        } else if self.eat("/") {
            Axis::Child
        } else {
            Axis::Descendant
        };
        let (steps, target) = self.steps(axis)?;
        if target.is_some() {
            return Err(
                "--select picks elements; use '@attr' and 'text()' inside [...] filters"
                    .to_string(),
            );
        }
        if !self.rest().is_empty() {
            return Err(self.unexpected("'/', '//' or '['"));
        }
        Ok(XPathMini { steps })
    }

    /// Steps separated by `/` or `//`, optionally ending in `@attr` or
    /// `text()` (returned as the target).
    fn steps(&mut self, mut axis: Axis) -> ParseResult<(Vec<Step>, Option<Target>)> {
        let mut steps = Vec::new();
        loop {
            if let Some(target) = self.target()? {
                if axis == Axis::Descendant && !steps.is_empty() {
                    return Err("'//' can't lead to '@attr' or 'text()'".to_string());
                }
                return Ok((steps, Some(target)));
            }
            let test = self.node_test()?;
            if axis == Axis::Descendant && !matches!(test, NodeTest::Element { .. }) {
                return Err("'..' and '.' must follow a single '/'".to_string());
            }
            steps.push(Step { axis, test });
            axis = if self.eat("//") {
                Axis::Descendant
            } else if self.eat("/") {
                Axis::Child
            } else {
                return Ok((steps, None));
            };
        }
    }

    fn target(&mut self) -> ParseResult<Option<Target>> {
        if self.eat("@") {
            let name = self.name()?;
            return Ok(Some(Target::Attr(name.to_string())));
        }
        if self.eat("text()") {
            return Ok(Some(Target::Text));
        }
        Ok(None)
    }

    fn node_test(&mut self) -> ParseResult<NodeTest> {
        if self.eat("..") {
            return Ok(NodeTest::Parent);
        }
        if self.eat(".") {
            return Ok(NodeTest::SelfNode);
        }
        let name = if self.eat("*") { "*" } else { self.name()? };
        let mut predicates = Vec::new();
        while self.eat("[") {
            predicates.push(self.predicate()?);
            self.expect("]")?;
        }
        Ok(NodeTest::Element {
            name: name.to_string(),
            predicates,
        })
    }

    /// An XML-ish name: letters, digits, `_ - . :`, and any non-ASCII.
    fn name(&mut self) -> ParseResult<&'s str> {
        let rest = self.rest();
        let len = rest
            .find(|c: char| !(c.is_alphanumeric() || "_-.:".contains(c)))
            .unwrap_or(rest.len());
        if len == 0 {
            return Err(self.unexpected("a name, '*', '..' or '.'"));
        }
        self.pos += len;
        Ok(&rest[..len])
    }

    fn predicate(&mut self) -> ParseResult<Predicate> {
        self.skip_ws();
        let rest = self.rest();
        if let Some(args) = rest.strip_prefix("contains")
            && args.trim_start().starts_with('(')
        {
            self.pos += "contains".len();
            self.expect("(")?;
            self.skip_ws();
            let path = self.value_path()?;
            self.expect(",")?;
            let value = self.literal()?;
            self.expect(")")?;
            self.skip_ws();
            return Ok(Predicate::Contains(path, value));
        }
        let path = self.value_path()?;
        self.skip_ws();
        if self.eat("=") {
            let value = self.literal()?;
            self.skip_ws();
            return Ok(Predicate::Equals(path, value));
        }
        Ok(Predicate::Exists(path))
    }

    /// A predicate operand: relative steps (children by default), optionally
    /// ending in `@attr` or `text()`. A path that ends on elements reads
    /// their full text.
    fn value_path(&mut self) -> ParseResult<ValuePath> {
        if self.rest().starts_with('/') {
            return Err("paths inside [...] must be relative (e.g. 'a/b', './/b')".to_string());
        }
        let (steps, target) = self.steps(Axis::Child)?;
        Ok(ValuePath {
            steps,
            target: target.unwrap_or(Target::StringValue),
        })
    }

    fn literal(&mut self) -> ParseResult<String> {
        self.skip_ws();
        let rest = self.rest();
        let Some(quote) = rest.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
            return Err(self.unexpected("a quoted value, e.g. \"42\""));
        };
        let Some(close) = rest[1..].find(quote) else {
            return Err("unterminated quoted value".to_string());
        };
        self.pos += close + 2;
        Ok(rest[1..close + 1].to_string())
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

    /// `name[k]` segments from the root down to `id`, where `k` counts
    /// same-name siblings (as `pathsel::ordinal_among` does).
    fn path(&self, id: usize) -> String {
        let name = |n: usize| self.nodes[n].elem.map_or("", |e| e.name.as_str());
        let mut segments = Vec::new();
        let mut id = id;
        while id != DOCUMENT {
            let parent = self.nodes[id].parent;
            let ordinal = self.nodes[parent]
                .children
                .iter()
                .take_while(|&&sibling| sibling != id)
                .filter(|&&sibling| name(sibling) == name(id))
                .count()
                + 1;
            segments.push(format!("{}[{ordinal}]", name(id)));
            id = parent;
        }
        segments.reverse();
        segments.join("/")
    }

    fn walk(&self, steps: &[Step], start: Vec<usize>) -> Vec<usize> {
        steps
            .iter()
            .fold(start, |context, step| self.apply(step, &context))
    }

    fn element_matches(&self, id: usize, test: &NodeTest) -> bool {
        let NodeTest::Element { name, predicates } = test else {
            unreachable!("only element tests are matched against elements")
        };
        self.nodes[id]
            .elem
            .is_some_and(|e| name == "*" || name_matches_select(&e.name, name))
            && predicates.iter().all(|p| self.holds(id, p))
    }

    fn holds(&self, id: usize, predicate: &Predicate) -> bool {
        match predicate {
            Predicate::Exists(path) => match path.target {
                // An element path exists if it reaches any element.
                Target::StringValue => self
                    .walk(&path.steps, vec![id])
                    .iter()
                    .any(|&n| self.nodes[n].elem.is_some()),
                // `[text()]` wants non-blank text; `[@a]` any value.
                Target::Text => self.any_value(id, path, |v| !v.trim().is_empty()),
                Target::Attr(_) => self.any_value(id, path, |_| true),
            },
            Predicate::Equals(path, value) => self.any_value(id, path, |v| v.trim() == value),
            Predicate::Contains(path, value) => {
                self.any_value(id, path, |v| v.contains(value.as_str()))
            }
        }
    }

    /// Whether any value `path` reads from element `id` satisfies `test`.
    fn any_value(&self, id: usize, path: &ValuePath, test: impl Fn(&str) -> bool) -> bool {
        self.walk(&path.steps, vec![id])
            .iter()
            .filter_map(|&n| self.nodes[n].elem)
            .any(|e| match &path.target {
                Target::Attr(attr) => e
                    .attributes
                    .iter()
                    .any(|(key, v)| name_matches_select(key, attr) && test(v)),
                Target::Text => test(&e.text_content),
                Target::StringValue if e.children.is_empty() => test(&e.text_content),
                Target::StringValue => test(&string_value(e)),
            })
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

/// An element's text including all descendants' (XPath string-value). Text
/// runs are joined per element and then per child, so for mixed content the
/// run order is approximate — fine for `contains`, and `=` on a leaf reads
/// the leaf's own text.
fn string_value(elem: &XmlElement) -> String {
    let mut out = elem.text_content.clone();
    for child in &elem.children {
        out.push_str(&string_value(child));
    }
    out
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

    fn text(name: &str, attrs: &[(&str, &str)], text: &str) -> XmlElement {
        let mut e = elem(name, attrs, vec![]);
        e.text_content = text.to_string();
        e
    }

    /// <root>
    ///   <order id="1"><qty unit="kg"/><qty unit="kg"/></order>
    ///   <order id="2"><box><qty unit="pc"/></box></order>
    ///   <cac:InvoiceLine xml:lang="fi" expr="a]b"/>
    ///   <call id="3" name="acme_file_functions">
    ///     <parameter name="command"> decrypt </parameter>
    ///     <parameter name="path">/tmp</parameter>
    ///   </call>
    ///   <call id="4" name="acme_file_functions">
    ///     <parameter name="command">copy</parameter>
    ///   </call>
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
                elem(
                    "call",
                    &[("id", "3"), ("name", "acme_file_functions")],
                    vec![
                        text("parameter", &[("name", "command")], " decrypt "),
                        text("parameter", &[("name", "path")], "/tmp"),
                    ],
                ),
                elem(
                    "call",
                    &[("id", "4"), ("name", "acme_file_functions")],
                    vec![text("parameter", &[("name", "command")], "copy")],
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
            .map(|hit| hit.elem)
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
        assert_eq!(select(r#"*[ @id = "2" ]"#), ["order#2"]);
        assert!(select("cbc:InvoiceLine").is_empty());
    }

    #[test]
    fn text_and_path_predicates() {
        // Whole call blocks whose command parameter is `decrypt`.
        let decrypt = r#"call[@name="acme_file_functions"][parameter[@name="command"]="decrypt"]"#;
        assert_eq!(select(decrypt), ["call#3"]);
        assert_eq!(
            select(r#"parameter[@name="command"][.="decrypt"]/.."#),
            ["call#3"]
        );
        assert_eq!(select(r#"parameter[text()="copy"]/.."#), ["call#4"]);
        assert_eq!(select(r#"call[parameter="/tmp"]"#), ["call#3"]);
        assert_eq!(select("call[parameter[@name='path']]"), ["call#3"]);
        assert_eq!(select("order[box/qty]"), ["order#2"]);
        assert_eq!(select("order[.//qty/@unit='pc']"), ["order#2"]);
        assert_eq!(select("qty[../@id='1']").len(), 2);
        assert_eq!(select("root/call[@id='4'][../order]"), ["call#4"]);
        assert!(select(r#"call[parameter="decr"]"#).is_empty());
    }

    #[test]
    fn contains_predicates() {
        assert_eq!(select(r#"call[contains(parameter, "op")]"#), ["call#4"]);
        assert_eq!(
            select(r#"*[contains(@name, "file")]"#),
            ["call#3", "call#4"]
        );
        assert_eq!(select(r#"parameter[contains(., "cryp")]/.."#), ["call#3"]);
        // `root` holds every call, so its full text contains "decrypt".
        assert_eq!(select(r#"*[contains(., "decrypt")]"#), ["root"]);
        // An element named `contains` is still just a name.
        assert!(select("contains").is_empty());
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
    fn hits_carry_diff_patch_paths() {
        let roots = doc();
        let paths: Vec<String> = XPathMini::parse("qty")
            .unwrap()
            .select(&roots)
            .into_iter()
            .map(|hit| hit.path)
            .collect();
        assert_eq!(
            paths,
            [
                "root[1]/order[1]/qty[1]",
                "root[1]/order[1]/qty[2]",
                "root[1]/order[2]/box[1]/qty[1]"
            ]
        );
    }

    #[test]
    fn malformed_patterns_are_rejected() {
        for bad in [
            "",
            "/",
            "a/",
            "a//",
            "[@id]",
            "item[id",
            "item[@id=2]",
            r#"item[@id="2""#,
            r#"item[@id="2"#,
            "item[@]",
            "item[@id]x",
            "//..",
            "a//.",
            "a b",
            "@id",
            "a/@id",
            "a/text()",
            "a[/b]",
            "a[b//@c]",
            r#"a[contains(b)]"#,
            r#"a[contains(b, c)]"#,
        ] {
            assert!(XPathMini::parse(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn required_literals_cover_all_steps_and_predicates() {
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
        assert_eq!(
            lits(r#"call[parameter[@name="command"]="decrypt"]"#),
            ["call", "parameter", "name", "command", "decrypt"]
        );
        assert_eq!(lits(r#"a[contains(., "x y")]"#), ["a"]);
    }
}
