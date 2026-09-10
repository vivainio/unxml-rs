//! Anchor syntax for `diff`/`patch`: a `name[k]`-per-occurrence path selector,
//! not real XPath — no axes, no attribute predicates, no wildcards. `k` is
//! 1-based, counting only same-name siblings under the same parent (the
//! ordinal of the element among its same-name siblings, in document order).
//! A bare `name` segment (no `[k]`) is shorthand for `[1]`, but only when it
//! is unambiguous — if more than one sibling matches, resolution errors
//! rather than silently picking the first.
//!
//! Segment name matching reuses `--select`'s convention
//! (`document::name_matches_select`): a prefixed segment (`cac:InvoiceLine`)
//! matches the full name, a bare segment (`InvoiceLine`) matches the local
//! name regardless of prefix.

use anyhow::{Result, bail};

use crate::document::name_matches_select;
use crate::model::{NodeRef, XmlElement};

#[derive(Debug, Clone, PartialEq)]
struct Segment {
    name: String,
    index: Option<usize>,
}

fn parse_segment(raw: &str) -> Result<Segment> {
    match raw.find('[') {
        None => Ok(Segment {
            name: raw.to_string(),
            index: None,
        }),
        Some(open) => {
            if !raw.ends_with(']') {
                bail!("bad selector segment '{raw}': expected 'name[k]'");
            }
            let name = &raw[..open];
            let idx_str = &raw[open + 1..raw.len() - 1];
            let index: usize = idx_str.parse().map_err(|_| {
                anyhow::anyhow!("bad selector segment '{raw}': index must be a positive integer")
            })?;
            if index == 0 {
                bail!("bad selector segment '{raw}': index is 1-based, 0 is invalid");
            }
            if name.is_empty() {
                bail!("bad selector segment '{raw}': missing element name before '['");
            }
            Ok(Segment {
                name: name.to_string(),
                index: Some(index),
            })
        }
    }
}

fn parse_sel(sel: &str) -> Result<Vec<Segment>> {
    if sel.is_empty() {
        bail!("empty selector");
    }
    sel.split('/').map(parse_segment).collect()
}

/// 1-based count of `target` among its own same-name siblings in `siblings`,
/// in document order — the ordinal a generated anchor's `[k]` uses. Matches
/// by identity (pointer equality), not structural equality, since sibling
/// elements can be structurally identical.
pub(crate) fn ordinal_among(siblings: &[XmlElement], target: &XmlElement) -> usize {
    let mut count = 0;
    for e in siblings {
        if e.name == target.name {
            count += 1;
            if std::ptr::eq(e, target) {
                return count;
            }
        }
    }
    count
}

/// Position(s) among `siblings` matching `seg.name`, in document order. Used
/// both to resolve an explicit `[k]` and to detect ambiguity when `k` is
/// omitted.
fn match_indices(siblings: &[XmlElement], seg: &Segment) -> Vec<usize> {
    siblings
        .iter()
        .enumerate()
        .filter(|(_, e)| name_matches_select(&e.name, &seg.name))
        .map(|(i, _)| i)
        .collect()
}

fn resolve_index(siblings: &[XmlElement], seg: &Segment) -> Result<usize> {
    let idxs = match_indices(siblings, seg);
    match seg.index {
        Some(k) => idxs.get(k - 1).copied().ok_or_else(|| {
            anyhow::anyhow!(
                "selector segment '{}[{}]' has no match: only {} occurrence(s) of '{}' found",
                seg.name,
                k,
                idxs.len(),
                seg.name
            )
        }),
        None => match idxs.len() {
            0 => bail!("selector segment '{}' has no match", seg.name),
            1 => Ok(idxs[0]),
            n => bail!(
                "selector segment '{}' is ambiguous: {n} occurrences found, use '{}[k]' to disambiguate",
                seg.name,
                seg.name
            ),
        },
    }
}

/// A mutable handle on the list a resolved element lives in: either the
/// document's top-level root list, or the `children` of some owning element.
/// The `Element` case also carries the owning element so structural edits
/// (`insert_children`/`remove_child`) can keep its `nodes` document-order
/// list in sync with `children` — dropping that sync would silently lose
/// interleaved comments/text on every insert or remove.
pub(crate) enum Parent<'a> {
    Root(&'a mut Vec<XmlElement>),
    Element(&'a mut XmlElement),
}

impl<'a> Parent<'a> {
    pub(crate) fn children_mut(&mut self) -> &mut Vec<XmlElement> {
        match self {
            Parent::Root(v) => v,
            Parent::Element(e) => &mut e.children,
        }
    }

    /// Insert `fragments` at children-index `pos` (0-based; `pos ==
    /// children.len()` appends). For an `Element` parent, `nodes` is kept in
    /// sync: existing `Child` entries at/after `pos` shift up, and the new
    /// entries are spliced into `nodes` at the same document-order position
    /// their children-index occupied — so a comment or text run standing
    /// next to the insertion point stays exactly where it was, not silently
    /// dropped. A `Root` parent has no owning element/`nodes` to sync; not
    /// reachable from `diff`/`patch` today since both require a single
    /// document root, but handled anyway rather than panicking.
    pub(crate) fn insert_children(&mut self, pos: usize, fragments: Vec<XmlElement>) {
        let k = fragments.len();
        if k == 0 {
            return;
        }
        match self {
            Parent::Root(v) => {
                for (offset, frag) in fragments.into_iter().enumerate() {
                    v.insert(pos + offset, frag);
                }
            }
            Parent::Element(e) => {
                for (offset, frag) in fragments.into_iter().enumerate() {
                    e.children.insert(pos + offset, frag);
                }
                let node_pos = e
                    .nodes
                    .iter()
                    .position(|n| matches!(n, NodeRef::Child(i) if *i == pos))
                    .unwrap_or(e.nodes.len());
                for n in e.nodes.iter_mut() {
                    if let NodeRef::Child(i) = n
                        && *i >= pos
                    {
                        *i += k;
                    }
                }
                let new_nodes: Vec<NodeRef> = (0..k).map(|o| NodeRef::Child(pos + o)).collect();
                e.nodes.splice(node_pos..node_pos, new_nodes);
            }
        }
    }

    /// Remove the child at `idx`, keeping `nodes` in sync the same way
    /// `insert_children` does.
    pub(crate) fn remove_child(&mut self, idx: usize) {
        match self {
            Parent::Root(v) => {
                v.remove(idx);
            }
            Parent::Element(e) => {
                e.children.remove(idx);
                let node_pos = e
                    .nodes
                    .iter()
                    .position(|n| matches!(n, NodeRef::Child(i) if *i == idx));
                if let Some(np) = node_pos {
                    e.nodes.remove(np);
                }
                for n in e.nodes.iter_mut() {
                    if let NodeRef::Child(i) = n
                        && *i > idx
                    {
                        *i -= 1;
                    }
                }
            }
        }
    }
}

/// Resolve `sel` to the list it lives in (`Parent`) plus its index within
/// that list — the shape `patch::apply` needs for every op: `SetAttr`/
/// `ReplaceText` reach the element via `parent.children_mut()[idx]`;
/// `Remove`/`InsertAfter`/`InsertBefore` need the list itself.
pub(crate) fn resolve_parent_mut<'a>(
    roots: &'a mut Vec<XmlElement>,
    sel: &str,
) -> Result<(Parent<'a>, usize)> {
    let segs = parse_sel(sel)?;
    let (last, ancestors) = segs
        .split_last()
        .expect("parse_sel guarantees at least one segment");

    if ancestors.is_empty() {
        let idx = resolve_index(roots, last)?;
        return Ok((Parent::Root(roots), idx));
    }

    let idx0 = resolve_index(roots, &ancestors[0])?;
    let mut current: &mut XmlElement = &mut roots[idx0];
    for seg in &ancestors[1..] {
        let idx = resolve_index(&current.children, seg)?;
        current = &mut current.children[idx];
    }
    let idx = resolve_index(&current.children, last)?;
    Ok((Parent::Element(current), idx))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elem(name: &str) -> XmlElement {
        XmlElement::new(name.to_string())
    }

    fn doc() -> Vec<XmlElement> {
        let mut root = elem("Invoice");
        root.children.push(elem("cbc:ID"));
        let mut line1 = elem("cac:InvoiceLine");
        line1.children.push(elem("cbc:Note"));
        let line2 = elem("cac:InvoiceLine");
        let line3 = elem("cac:InvoiceLine");
        root.children.push(line1);
        root.children.push(line2);
        root.children.push(line3);
        vec![root]
    }

    /// Resolve `sel` and return the matched element's name, plus enough of
    /// its identity (its parent's children-index) to assert on without
    /// exposing `Parent` mutability concerns to every test.
    fn resolve_name(roots: &mut Vec<XmlElement>, sel: &str) -> Result<(String, usize)> {
        let (mut parent, idx) = resolve_parent_mut(roots, sel)?;
        Ok((parent.children_mut()[idx].name.clone(), idx))
    }

    #[test]
    fn resolves_root() {
        let mut roots = doc();
        let (name, _) = resolve_name(&mut roots, "Invoice[1]").unwrap();
        assert_eq!(name, "Invoice");
    }

    #[test]
    fn resolves_unique_child_without_explicit_index() {
        let mut roots = doc();
        let (name, _) = resolve_name(&mut roots, "Invoice/cbc:ID").unwrap();
        assert_eq!(name, "cbc:ID");
    }

    #[test]
    fn bare_segment_matches_local_name() {
        let mut roots = doc();
        let (name, _) = resolve_name(&mut roots, "Invoice/ID").unwrap();
        assert_eq!(name, "cbc:ID");
    }

    #[test]
    fn resolves_by_explicit_index() {
        let mut roots = doc();
        let (name, idx) = resolve_name(&mut roots, "Invoice/cac:InvoiceLine[2]").unwrap();
        assert_eq!(name, "cac:InvoiceLine");
        assert_eq!(idx, 2);
    }

    #[test]
    fn nested_index_resolves_grandchild() {
        let mut roots = doc();
        let (name, _) = resolve_name(&mut roots, "Invoice/cac:InvoiceLine[1]/cbc:Note").unwrap();
        assert_eq!(name, "cbc:Note");
    }

    #[test]
    fn ambiguous_without_index_errors() {
        let mut roots = doc();
        let err = resolve_name(&mut roots, "Invoice/cac:InvoiceLine").unwrap_err();
        assert!(err.to_string().contains("ambiguous"), "{err}");
    }

    #[test]
    fn missing_index_errors() {
        let mut roots = doc();
        let err = resolve_name(&mut roots, "Invoice/cac:InvoiceLine[5]").unwrap_err();
        assert!(err.to_string().contains("no match"), "{err}");
    }

    #[test]
    fn missing_name_errors() {
        let mut roots = doc();
        let err = resolve_name(&mut roots, "Invoice/NoSuchElement").unwrap_err();
        assert!(err.to_string().contains("no match"), "{err}");
    }

    #[test]
    fn bad_syntax_errors() {
        assert!(parse_sel("Invoice/cac:InvoiceLine[").is_err());
        assert!(parse_sel("Invoice/cac:InvoiceLine[0]").is_err());
        assert!(parse_sel("Invoice/[1]").is_err());
        assert!(parse_sel("").is_err());
    }
}
