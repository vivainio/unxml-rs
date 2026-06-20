//! Orchestration: read input, parse, hide namespaces, select subtrees,
//! render, and emit (optionally through `bat`).

use std::collections::HashSet;
use std::io::{self, Read};

use anyhow::{Context, Result};

use crate::document::{hide_namespaces, select_subtrees, sniff_hidden_prefixes};
use crate::model::FormatOpts;
use crate::parse::{InputFormat, detect_format, parse_html, parse_xml, read_file_lenient};
use crate::xslt::TemplateRegistry;

#[allow(clippy::too_many_arguments)]
pub(crate) fn process_content(
    content: &str,
    file_path: &str,
    format_override: Option<&str>,
    opts: &FormatOpts,
    registry: Option<&TemplateRegistry>,
    hide_ns: &HashSet<String>,
    sniff: bool,
    select: Option<&str>,
) -> Result<String> {
    // Determine input format
    let format = if let Some(format_str) = format_override {
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
    let mut hidden = hide_ns.clone();
    if sniff {
        hidden.extend(sniff_hidden_prefixes(&elements));
    }
    if !hidden.is_empty() {
        for element in &mut elements {
            hide_namespaces(element, &hidden);
        }
    }

    // Format output. With --select, render each matched subtree as a top-level
    // fragment separated by a blank line; otherwise render every root element.
    let mut output = String::new();
    if let Some(pattern) = select {
        let mut matched = Vec::new();
        select_subtrees(&elements, pattern, &mut matched);
        for (i, elem) in matched.iter().enumerate() {
            if i > 0 {
                output.push('\n');
            }
            output.push_str(&elem.format_yaml_like(0, opts, registry));
        }
    } else {
        for element in elements {
            output.push_str(&element.format_yaml_like(0, opts, registry));
        }
    }

    Ok(output)
}

pub(crate) fn process_file(
    file_path: &str,
    format_override: Option<&str>,
    opts: &FormatOpts,
    expand: bool,
    hide_ns: &HashSet<String>,
    sniff: bool,
    select: Option<&str>,
) -> Result<String> {
    // Build template registry if expand mode is enabled
    let registry = if expand && opts.xslt {
        Some(TemplateRegistry::build_from_file(file_path)?)
    } else {
        None
    };

    // Read the file
    let content = read_file_lenient(file_path)?;

    process_content(
        &content,
        file_path,
        format_override,
        opts,
        registry.as_ref(),
        hide_ns,
        sniff,
        select,
    )
}

pub(crate) fn process_stdin(
    format_override: Option<&str>,
    opts: &FormatOpts,
    hide_ns: &HashSet<String>,
    sniff: bool,
    select: Option<&str>,
) -> Result<String> {
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
    process_content(
        &content,
        "stdin",
        format_override,
        opts,
        None,
        hide_ns,
        sniff,
        select,
    )
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
