//! Path dump (`--paths`): a compact structural summary of a document.
//!
//! Emit the set of *distinct* element paths as an indented tree, each node shown
//! once with the union of attribute names ever seen on elements at that path:
//!
//! ```text
//! order(id)
//!   line(sku)
//!     qty
//! ```
//!
//! This is a de-duplicated view of "what shapes exist in this document"
//! regardless of how many times each occurs — repeated siblings collapse to one
//! node. The tree falls straight out of the sorted distinct paths: with `/` the
//! lowest separator byte, every node's whole subtree sorts contiguously right
//! after it and before any sibling, so a running prefix diff against the
//! previous path yields correct nesting. Namespace declarations (`xmlns*`) are
//! not treated as attributes. Composes with `--hide-ns`, `--canonical`, and
//! `--select`, which transform the element names / subtree set before this runs.

use std::collections::{BTreeMap, BTreeSet};

use crate::canonical::well_known_prefix;
use crate::model::XmlElement;

/// Collect the namespace bindings (`prefix -> URI`, with the default namespace
/// keyed as `(default)`) declared anywhere in the subtree, so the path dump can
/// print a legend explaining what each prefix means.
fn collect_ns(elem: &XmlElement, acc: &mut BTreeSet<(String, String)>) {
    for (key, value) in &elem.attributes {
        if key == "xmlns" {
            acc.insert(("(default)".to_string(), value.clone()));
        } else if let Some(prefix) = key.strip_prefix("xmlns:") {
            acc.insert((prefix.to_string(), value.clone()));
        }
    }
    for child in &elem.children {
        collect_ns(child, acc);
    }
}

/// Accumulate `path -> {attribute names}` for every element in the subtree,
/// unioning attribute names across all occurrences of each path.
fn collect(elem: &XmlElement, parent: &str, acc: &mut BTreeMap<String, BTreeSet<String>>) {
    let path = if parent.is_empty() {
        elem.name.clone()
    } else {
        format!("{parent}/{}", elem.name)
    };
    let attrs = acc.entry(path.clone()).or_default();
    for key in elem.attributes.keys() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        attrs.insert(key.clone());
    }
    for child in &elem.children {
        collect(child, &path, acc);
    }
}

/// Render the distinct element paths of `roots` as an indented tree, each node
/// shown once with its union of attribute names in parentheses (omitted when
/// there are none), under a `//` legend of the namespace prefixes.
pub(crate) fn dump_paths(roots: &[&XmlElement]) -> String {
    let mut acc = BTreeMap::new();
    for root in roots {
        collect(root, "", &mut acc);
    }

    let mut out = String::new();

    // Legend: explain each namespace prefix used in the paths. Without it the
    // prefixes (and especially `--canonical`'s generated `ns1`/`ns2`) would be
    // opaque. Emitted as `//` comments so it is easy to strip or grep past.
    let mut ns = BTreeSet::new();
    for root in roots {
        collect_ns(root, &mut ns);
    }
    for (prefix, uri) in &ns {
        // Skip recognised vocabularies bound to their conventional prefix
        // (`xsl`, `xs`, `cac`, …): those are self-explanatory, so listing them
        // is noise. A non-standard prefix on a well-known URI still gets a line.
        if well_known_prefix(uri) == Some(prefix.as_str()) {
            continue;
        }
        out.push_str(&format!("// {prefix} = {uri}\n"));
    }

    // Walk the sorted paths, emitting only the segments that differ from the
    // previous path (its subtree is contiguous, so this nests correctly). A
    // node's attribute union attaches to its final segment.
    let mut prev: Vec<&str> = Vec::new();
    for (path, attrs) in &acc {
        let segs: Vec<&str> = path.split('/').collect();
        let mut shared = 0;
        while shared < prev.len() && shared < segs.len() && prev[shared] == segs[shared] {
            shared += 1;
        }
        for (depth, seg) in segs.iter().enumerate().skip(shared) {
            let indent = "  ".repeat(depth);
            if depth + 1 == segs.len() && !attrs.is_empty() {
                let names: Vec<&str> = attrs.iter().map(String::as_str).collect();
                out.push_str(&format!("{indent}{seg}({})\n", names.join(", ")));
            } else {
                out.push_str(&format!("{indent}{seg}\n"));
            }
        }
        prev = segs;
    }
    out
}
