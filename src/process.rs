//! Orchestration: read input, parse, hide namespaces, select subtrees,
//! render, and emit (optionally through `bat`).

use std::collections::HashSet;
use std::io::{self, Read};

use anyhow::{Context, Result};

use crate::canonical::canonicalize;
use crate::document::{HIDE_NS_ALL, hide_namespaces, select_subtrees, sniff_hidden_prefixes};
use crate::model::{FormatOpts, XmlElement};
use crate::parse::{InputFormat, detect_format, parse_html, parse_xml, read_file_lenient};
use crate::paths::dump_paths;
use crate::xslt::TemplateRegistry;

/// The cross-cutting, CLI-derived options shared by every input. Built once and
/// passed by reference, so the process functions stay narrow even as flags grow.
/// The per-file processing *mode* (`FormatOpts`) is passed separately because it
/// can vary per file under `--auto`.
pub(crate) struct ProcessOptions<'a> {
    pub(crate) format_override: Option<&'a str>,
    pub(crate) hide_ns: &'a HashSet<String>,
    pub(crate) sniff: bool,
    pub(crate) select: Option<&'a str>,
    pub(crate) canonical: bool,
    pub(crate) paths: bool,
    pub(crate) depth: usize,
    pub(crate) no_attrs: bool,
    pub(crate) fold: bool,
    pub(crate) expand: bool,
}

pub(crate) fn process_content(
    content: &str,
    file_path: &str,
    opts: &FormatOpts,
    registry: Option<&TemplateRegistry>,
    cfg: &ProcessOptions,
) -> Result<String> {
    // Determine input format
    let format = if let Some(format_str) = cfg.format_override {
        match format_str.to_lowercase().as_str() {
            "html" => InputFormat::Html,
            "xml" => InputFormat::Xml,
            _ => {
                return Err(anyhow::anyhow!(
                    "Unsupported format: {}. Use 'xml' or 'html'",
                    format_str
                ));
            }
        }
    } else {
        detect_format(content, file_path)
    };

    // Parse the content based on detected/specified format
    let mut elements = match format {
        InputFormat::Html => parse_html(content, &format).context("Failed to parse HTML")?,
        InputFormat::Xml => parse_xml(content).context("Failed to parse XML")?,
    };

    // Build the effective set of prefixes to hide: those requested explicitly,
    // plus any inferred by sniffing the document type (only under --auto/--bat).
    // The `ALL` sentinel hides every prefix regardless of the rest of the set.
    let mut hidden = cfg.hide_ns.clone();
    if cfg.sniff {
        hidden.extend(sniff_hidden_prefixes(&elements));
    }
    let hide_all = hidden.contains(HIDE_NS_ALL);
    if hide_all || !hidden.is_empty() {
        for element in &mut elements {
            hide_namespaces(element, &hidden, hide_all);
        }
    }

    // Canonicalise for diff-friendly output: always rebind prefixes to stable
    // names, but only sort siblings in plain XML mode — in a dialect/`--special`
    // mode element order is significant, so sorting would misrepresent it.
    if cfg.canonical {
        canonicalize(&mut elements, !opts.has_mode());
    }

    // Determine the roots to emit: the whole document, or just the subtrees
    // matched by --select.
    let roots: Vec<&XmlElement> = if let Some(pattern) = cfg.select {
        let mut matched = Vec::new();
        select_subtrees(&elements, pattern, &mut matched);
        matched
    } else {
        elements.iter().collect()
    };

    // --paths dumps the distinct element paths; otherwise render the tree. Under
    // --select, render each matched subtree as a fragment separated by a blank
    // line; the whole-document case emits roots back-to-back.
    let output = if cfg.paths {
        dump_paths(&roots, cfg.depth, cfg.no_attrs, cfg.fold)
    } else {
        let mut out = String::new();
        for (i, elem) in roots.iter().enumerate() {
            if i > 0 && cfg.select.is_some() {
                out.push('\n');
            }
            out.push_str(&elem.format_yaml_like(0, opts, registry));
        }
        out
    };

    Ok(output)
}

pub(crate) fn process_file(
    file_path: &str,
    opts: &FormatOpts,
    cfg: &ProcessOptions,
) -> Result<String> {
    // Build template registry if expand mode is enabled
    let registry = if cfg.expand && opts.xslt {
        Some(TemplateRegistry::build_from_file(file_path)?)
    } else {
        None
    };

    // Read the file
    let content = read_file_lenient(file_path)?;

    process_content(&content, file_path, opts, registry.as_ref(), cfg)
}

pub(crate) fn process_stdin(opts: &FormatOpts, cfg: &ProcessOptions) -> Result<String> {
    // Read from stdin, tolerating non-UTF-8 input (see read_file_lenient).
    let mut bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut bytes)
        .context("Failed to read from stdin")?;
    let content = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => e.into_bytes().into_iter().map(|b| b as char).collect(),
    };

    // Note: expand mode not supported for stdin since we need file paths for imports
    process_content(&content, "stdin", opts, None, cfg)
}

/// Emit rendered output, optionally through `bat` for syntax highlighting.
/// When `use_bat` is set we pipe to `bat -l unxml`; if no `bat` binary is
/// found we fall back to plain stdout so `--bat` degrades gracefully.
pub(crate) fn emit(output: &str, use_bat: bool) {
    if use_bat && pipe_to_bat(output) {
        return;
    }
    print!("{output}");
}

/// Try to display `output` via `bat -l unxml`. Returns true if a `bat` (or
/// `batcat`, the Debian/Ubuntu name) process was launched and handed the
/// output, false if no such binary exists.
pub(crate) fn pipe_to_bat(output: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};

    for bin in ["bat", "batcat"] {
        // Only stdin is piped; bat inherits our stdout/stderr so its pager
        // draws straight to the terminal.
        let mut child = match Command::new(bin)
            .args(["-l", "unxml"])
            .stdin(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => continue, // binary not found — try the next name
        };
        if let Some(mut stdin) = child.stdin.take() {
            // Ignore a broken pipe if the user quits the pager early.
            let _ = stdin.write_all(output.as_bytes());
        }
        let _ = child.wait();
        return true;
    }
    false
}
