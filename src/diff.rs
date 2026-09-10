//! Structural diff between two parsed XML documents: `generate` walks two
//! `XmlElement` forests and produces the small ordered `patch::Op` list that
//! `patch::apply` replays against `base` to reproduce `modified`.
//!
//! Deliberately conservative: anything this can't confidently translate — an attribute *removed*, mixed
//! element/text content, an element that changed between leaf (text-only)
//! and container (has children), a differing root, or an insertion into a
//! parent that had no children left in `base` to anchor on — is collected
//! and reported as a single error naming every such difference, rather than
//! silently dropped or guessed at. The generated patch is also self-checked:
//! before `generate` returns, its own ops are applied to a fresh clone of
//! `base` and the result is required to canonically match `modified`, using
//! the same `canonicalize()` + `format_yaml_like()` view `--canonical`
//! already provides — a bug in this differ fails loudly here, not silently
//! in a shipped patch.

use anyhow::{Result, bail};
use similar::{Algorithm, DiffOp, capture_diff_slices};

use crate::canonical::canonicalize;
use crate::model::{FormatOpts, XmlElement};
use crate::patch::{self, Op};
use crate::pathsel::ordinal_among;
use crate::xmlwrite::write_elements;

/// Generate the op list that turns `base` into `modified`. Both must be
/// single-root XML documents sharing the same root element name; a
/// differing root, or more than one root element on either side, is out of
/// scope (out-of-band XML declaration mismatches, top-level comments before/
/// after the root, and HTML input are likewise not diffed).
pub(crate) fn generate(base: &[XmlElement], modified: &[XmlElement]) -> Result<Vec<Op>> {
    if base.len() != 1 || modified.len() != 1 {
        bail!(
            "diff only supports single-root documents (base has {}, modified has {})",
            base.len(),
            modified.len()
        );
    }
    if base[0].name != modified[0].name {
        bail!(
            "root element differs ('{}' vs '{}') — replacing the document root isn't supported",
            base[0].name,
            modified[0].name
        );
    }

    let root_sel = format!("{}[1]", base[0].name);
    let mut ops = Vec::new();
    let mut unhandled = Vec::new();
    diff_element(&root_sel, &base[0], &modified[0], &mut ops, &mut unhandled);

    if !unhandled.is_empty() {
        bail!(
            "cannot generate a patch — {} difference(s) this differ doesn't know how to translate:\n  {}",
            unhandled.len(),
            unhandled.join("\n  ")
        );
    }

    let ops = reorder(ops);
    verify(base, modified, &ops)?;
    Ok(ops)
}

fn normalize_text(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn diff_element(
    sel: &str,
    base_e: &XmlElement,
    mod_e: &XmlElement,
    ops: &mut Vec<Op>,
    unhandled: &mut Vec<String>,
) {
    diff_attrs(sel, base_e, mod_e, ops, unhandled);

    if base_e.is_mixed() || mod_e.is_mixed() {
        unhandled.push(format!("{sel}: mixed element/text content isn't supported"));
        return;
    }

    let base_leaf = base_e.children.is_empty();
    let mod_leaf = mod_e.children.is_empty();

    if base_leaf && mod_leaf {
        let bt = normalize_text(&base_e.text_content);
        let mt = normalize_text(&mod_e.text_content);
        if bt != mt {
            ops.push(Op::ReplaceText {
                sel: sel.to_string(),
                text: mod_e.text_content.trim().to_string(),
            });
        }
        return;
    }

    if base_leaf != mod_leaf {
        unhandled.push(format!(
            "{sel}: element changed between leaf (text-only) and container (has children) — not supported"
        ));
        return;
    }

    diff_children(sel, &base_e.children, &mod_e.children, ops, unhandled);
}

fn diff_attrs(
    sel: &str,
    base_e: &XmlElement,
    mod_e: &XmlElement,
    ops: &mut Vec<Op>,
    unhandled: &mut Vec<String>,
) {
    let mut changed: Vec<&String> = mod_e
        .attributes
        .keys()
        .filter(|k| base_e.attributes.get(k.as_str()) != mod_e.attributes.get(k.as_str()))
        .collect();
    changed.sort();
    for k in changed {
        ops.push(Op::SetAttr {
            sel: sel.to_string(),
            name: k.clone(),
            value: mod_e.attributes[k].clone(),
        });
    }

    let mut removed: Vec<&String> = base_e
        .attributes
        .keys()
        .filter(|k| !mod_e.attributes.contains_key(k.as_str()))
        .collect();
    removed.sort();
    for k in removed {
        unhandled.push(format!("{sel}: attribute '{k}' removed — not supported"));
    }
}

fn diff_children(
    sel: &str,
    base_kids: &[XmlElement],
    mod_kids: &[XmlElement],
    ops: &mut Vec<Op>,
    unhandled: &mut Vec<String>,
) {
    let base_keys: Vec<&str> = base_kids.iter().map(|e| e.name.as_str()).collect();
    let mod_keys: Vec<&str> = mod_kids.iter().map(|e| e.name.as_str()).collect();
    let diff_ops = capture_diff_slices(Algorithm::Myers, &base_keys, &mod_keys);

    let mut last_matched_base: Option<usize> = None;

    for (block_idx, dop) in diff_ops.iter().enumerate() {
        match *dop {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
                ..
            } => {
                for k in 0..len {
                    let bi = old_index + k;
                    let mi = new_index + k;
                    let child_sel = format!(
                        "{sel}/{}[{}]",
                        base_kids[bi].name,
                        ordinal_among(base_kids, &base_kids[bi])
                    );
                    diff_element(&child_sel, &base_kids[bi], &mod_kids[mi], ops, unhandled);
                    last_matched_base = Some(bi);
                }
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                for k in 0..old_len {
                    let bi = old_index + k;
                    ops.push(Op::Remove {
                        sel: format!(
                            "{sel}/{}[{}]",
                            base_kids[bi].name,
                            ordinal_among(base_kids, &base_kids[bi])
                        ),
                    });
                }
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                emit_insert(
                    sel,
                    base_kids,
                    mod_kids,
                    new_index,
                    new_len,
                    &diff_ops,
                    block_idx,
                    last_matched_base,
                    ops,
                    unhandled,
                );
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                for k in 0..old_len {
                    let bi = old_index + k;
                    ops.push(Op::Remove {
                        sel: format!(
                            "{sel}/{}[{}]",
                            base_kids[bi].name,
                            ordinal_among(base_kids, &base_kids[bi])
                        ),
                    });
                }
                emit_insert(
                    sel,
                    base_kids,
                    mod_kids,
                    new_index,
                    new_len,
                    &diff_ops,
                    block_idx,
                    last_matched_base,
                    ops,
                    unhandled,
                );
            }
        }
    }
}

/// Emit one grouped `insert-after`/`insert-before` op for a contiguous run of
/// `new_len` newly-inserted siblings (`mod_kids[new_index..new_index+new_len]`),
/// anchored on the nearest already-present base sibling: the most recent
/// `Equal`-matched one if there is one, otherwise the first base index of the
/// next `Equal` block (a first-child insertion). Reports `unhandled` if
/// neither exists — the parent has no already-present sibling in `base` at
/// all to anchor on.
#[allow(clippy::too_many_arguments)]
fn emit_insert(
    sel: &str,
    base_kids: &[XmlElement],
    mod_kids: &[XmlElement],
    new_index: usize,
    new_len: usize,
    diff_ops: &[DiffOp],
    block_idx: usize,
    last_matched_base: Option<usize>,
    ops: &mut Vec<Op>,
    unhandled: &mut Vec<String>,
) {
    if new_len == 0 {
        return;
    }
    let content = write_elements(&mod_kids[new_index..new_index + new_len]);
    if let Some(anchor_bi) = last_matched_base {
        let anchor = &base_kids[anchor_bi];
        ops.push(Op::InsertAfter {
            sel: format!(
                "{sel}/{}[{}]",
                anchor.name,
                ordinal_among(base_kids, anchor)
            ),
            content,
        });
        return;
    }
    if let Some(next_bi) = next_equal_base_index(diff_ops, block_idx) {
        let anchor = &base_kids[next_bi];
        ops.push(Op::InsertBefore {
            sel: format!(
                "{sel}/{}[{}]",
                anchor.name,
                ordinal_among(base_kids, anchor)
            ),
            content,
        });
        return;
    }
    unhandled.push(format!(
        "{sel}: insertion with no already-present sibling in either direction to anchor on — the parent must have had no other children in base at all"
    ));
}

/// The first base index of the next `Equal` block after `from_block_idx`, if
/// any — the anchor a leading (first-child) insertion falls back to.
fn next_equal_base_index(diff_ops: &[DiffOp], from_block_idx: usize) -> Option<usize> {
    diff_ops[from_block_idx + 1..]
        .iter()
        .find_map(|dop| match *dop {
            DiffOp::Equal { old_index, .. } => Some(old_index),
            _ => None,
        })
}

/// Order generated ops so applying them in sequence can't have one op's `sel`
/// invalidated by an earlier one in the same list: attribute/text edits never
/// change sibling counts, so they run first; removals run in descending
/// same-name-ordinal order (globally — a removal only ever affects the
/// numbering of not-yet-processed *same-parent, same-name* siblings at a
/// lower index, so a single global descending sort is sufficient regardless
/// of interleaving with unrelated removals elsewhere in the tree); inserts
/// run last, since every insert anchors on an unmodified, already-present
/// sibling — never on another op's target.
fn reorder(ops: Vec<Op>) -> Vec<Op> {
    let mut attr_or_text = Vec::new();
    let mut removes = Vec::new();
    let mut inserts = Vec::new();
    for op in ops {
        match op {
            Op::Remove { .. } => removes.push(op),
            Op::InsertAfter { .. } | Op::InsertBefore { .. } => inserts.push(op),
            other => attr_or_text.push(other),
        }
    }
    removes.sort_by_key(|op| std::cmp::Reverse(trailing_index(op.sel())));
    let mut result = Vec::with_capacity(attr_or_text.len() + removes.len() + inserts.len());
    result.extend(attr_or_text);
    result.extend(removes);
    result.extend(inserts);
    result
}

fn trailing_index(sel: &str) -> usize {
    let Some(open) = sel.rfind('[') else {
        return 0;
    };
    let Some(close) = sel[open..].find(']') else {
        return 0;
    };
    sel[open + 1..open + close].parse().unwrap_or(0)
}

/// Apply `ops` to a fresh clone of `base` and require the result to
/// canonically match `modified` — the real correctness backstop: applying
/// its own generated patch and comparing the result is a much stronger
/// guarantee than trusting the diff logic to be exhaustively correct. A
/// mismatch is a bug in this differ, not a user error, so the message
/// includes a text diff of the two canonical renderings to make that bug
/// immediately visible.
fn verify(base: &[XmlElement], modified: &[XmlElement], ops: &[Op]) -> Result<()> {
    let mut patched = base.to_vec();
    patch::apply(&mut patched, ops)?;

    let patched_text = canonical_render(patched);
    let modified_text = canonical_render(modified.to_vec());

    if patched_text == modified_text {
        return Ok(());
    }

    let text_diff = similar::TextDiff::from_lines(&patched_text, &modified_text);
    let mut rendered_diff = String::new();
    for change in text_diff.iter_all_changes() {
        let tag = match change.tag() {
            similar::ChangeTag::Delete => '-',
            similar::ChangeTag::Insert => '+',
            similar::ChangeTag::Equal => ' ',
        };
        rendered_diff.push_str(&format!("{tag}{change}"));
    }
    bail!(
        "internal error: the generated patch does not reproduce the modified document.\n\
         This is a bug in unxml's differ, not a problem with your input — please report it.\n\
         Canonical diff (patched base vs. modified), - is patched, + is modified:\n{rendered_diff}"
    );
}

fn canonical_render(mut roots: Vec<XmlElement>) -> String {
    canonicalize(&mut roots, true);
    let opts = FormatOpts::default();
    roots
        .iter()
        .map(|e| e.format_yaml_like(0, &opts, None))
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_xml;

    fn parse(xml: &str) -> Vec<XmlElement> {
        parse_xml(xml).unwrap().roots
    }

    #[test]
    fn no_difference_yields_no_ops() {
        let base = parse("<Invoice><Line id=\"1\"/></Invoice>");
        let modified = parse("<Invoice><Line id=\"1\"/></Invoice>");
        let ops = generate(&base, &modified).unwrap();
        assert!(ops.is_empty());
    }

    #[test]
    fn detects_an_attribute_change() {
        let base = parse("<Invoice id=\"1\"/>");
        let modified = parse("<Invoice id=\"2\"/>");
        let ops = generate(&base, &modified).unwrap();
        assert_eq!(
            ops,
            vec![Op::SetAttr {
                sel: "Invoice[1]".to_string(),
                name: "id".to_string(),
                value: "2".to_string(),
            }]
        );
    }

    #[test]
    fn detects_a_text_change() {
        let base = parse("<Invoice><Note>old</Note></Invoice>");
        let modified = parse("<Invoice><Note>new</Note></Invoice>");
        let ops = generate(&base, &modified).unwrap();
        assert_eq!(
            ops,
            vec![Op::ReplaceText {
                sel: "Invoice[1]/Note[1]".to_string(),
                text: "new".to_string(),
            }]
        );
    }

    #[test]
    fn detects_a_removed_child() {
        let base = parse("<Invoice><Line id=\"1\"/><Line id=\"2\"/></Invoice>");
        let modified = parse("<Invoice><Line id=\"1\"/></Invoice>");
        let ops = generate(&base, &modified).unwrap();
        assert_eq!(
            ops,
            vec![Op::Remove {
                sel: "Invoice[1]/Line[2]".to_string(),
            }]
        );
    }

    #[test]
    fn detects_an_appended_child() {
        let base = parse("<Invoice><Line id=\"1\"/></Invoice>");
        let modified = parse("<Invoice><Line id=\"1\"/><Line id=\"2\"/></Invoice>");
        let ops = generate(&base, &modified).unwrap();
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::InsertAfter { sel, content } => {
                assert_eq!(sel, "Invoice[1]/Line[1]");
                assert!(content.contains("id=\"2\""));
            }
            other => panic!("expected InsertAfter, got {other:?}"),
        }
    }

    #[test]
    fn detects_a_first_child_insertion() {
        // Distinct tag names, not just a differing attribute: with same-name
        // siblings the name-only key gives Myers multiple equal-length LCS
        // alignments to choose from (matching base's lone element to either
        // modified copy is equally "correct" by tag name alone), so which
        // specific ops come out is implementation-defined, not a case this
        // test can pin down. A distinct name for the inserted sibling removes
        // that ambiguity by construction.
        let base = parse("<Invoice><Note/></Invoice>");
        let modified = parse("<Invoice><Header/><Note/></Invoice>");
        let ops = generate(&base, &modified).unwrap();
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::InsertBefore { sel, content } => {
                assert_eq!(sel, "Invoice[1]/Note[1]");
                assert!(content.contains("<Header"));
            }
            other => panic!("expected InsertBefore, got {other:?}"),
        }
    }

    #[test]
    fn groups_a_contiguous_insertion_run_into_one_op() {
        // See `detects_a_first_child_insertion` on why distinct names, not
        // just distinct attributes, are needed to pin down which alignment
        // Myers picks among same-length same-name-key LCS candidates.
        let base = parse("<Invoice><A/><D/></Invoice>");
        let modified = parse("<Invoice><A/><B/><C/><D/></Invoice>");
        let ops = generate(&base, &modified).unwrap();
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::InsertAfter { sel, content } => {
                assert_eq!(sel, "Invoice[1]/A[1]");
                let idx_b = content.find("<B").unwrap();
                let idx_c = content.find("<C").unwrap();
                assert!(idx_b < idx_c);
            }
            other => panic!("expected InsertAfter, got {other:?}"),
        }
    }

    #[test]
    fn rejects_a_differing_root() {
        let base = parse("<Invoice/>");
        let modified = parse("<CreditNote/>");
        assert!(generate(&base, &modified).is_err());
    }

    #[test]
    fn rejects_a_removed_attribute() {
        let base = parse("<Invoice id=\"1\"/>");
        let modified = parse("<Invoice/>");
        let err = generate(&base, &modified).unwrap_err();
        assert!(err.to_string().contains("attribute 'id' removed"), "{err}");
    }

    #[test]
    fn rejects_mixed_content() {
        let base = parse("<p>hello <b>world</b></p>");
        let modified = parse("<p>hello <b>there</b></p>");
        assert!(generate(&base, &modified).is_err());
    }

    #[test]
    fn multiple_changes_apply_cleanly_via_self_verify() {
        let base =
            parse("<Invoice id=\"1\"><Line id=\"a\"/><Line id=\"b\"/><Line id=\"c\"/></Invoice>");
        let modified =
            parse("<Invoice id=\"2\"><Line id=\"a\"/><Line id=\"c\"/><Line id=\"d\"/></Invoice>");
        // Removes Line[b], appends Line[d], changes the root id — exercises
        // reorder() ordering multiple op kinds together.
        let ops = generate(&base, &modified).unwrap();
        assert!(!ops.is_empty());
    }
}
