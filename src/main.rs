//! unxml — simplify and "flatten" XML and HTML into a light, Pug/YAML-like
//! readable form. This file wires the modules together and drives the CLI.

mod canonical;
mod cli;
mod document;
mod highlight;
mod install;
mod model;
mod msbuild;
mod parse;
mod paths;
mod process;
mod render;
mod schematron;
mod types;
mod wsdl;
mod xsd;
mod xslt;

use std::collections::HashSet;

use anyhow::{Context, Result};
use clap::Parser;
use glob::glob;

use crate::cli::Cli;
use crate::document::detect_mode_from_ext;
use crate::model::{Collapse, FormatOpts};
use crate::parse::{InputFormat, detect_format, read_file_lenient};
use crate::process::{ProcessOptions, emit, process_file, process_stdin};

fn main() -> Result<()> {
    // `unxml git <args>` is a thin passthrough to `git <args>` with the unxml
    // textconv driver applied for just this invocation. It's intercepted
    // ahead of the normal `Cli::parse()` below, since `files: Vec<String>`
    // would otherwise swallow "git" and everything after it as filenames.
    let rest: Vec<String> = std::env::args().skip(1).collect();
    if rest.first().map(String::as_str) == Some("git") {
        return install::git_passthrough(&rest[1..]);
    }

    let cli = Cli::parse();

    // Side-channel action: install the bundled skill and exit before any
    // input handling (no files required).
    if cli.install_skills {
        return install::install_skills();
    }

    // Side-channel action: register the .unxml grammar with bat and exit.
    if cli.install_bat {
        return install::install_bat();
    }

    // Side-channel action: print the --html stylesheet and exit.
    if cli.html_css {
        print!("{}", highlight::html_css()?);
        return Ok(());
    }

    // Side-channel action: wire unxml in as the current repo's XML/HTML diff
    // driver and exit (no input files required).
    if cli.init_git {
        return install::init_git();
    }

    if cli.raw && !(cli.html || cli.cat) {
        return Err(anyhow::anyhow!("--raw requires --html or --cat"));
    }

    // `--collapse` is orthogonal to the processing mode, so it is applied to
    // every file's opts below (after --auto picks a mode), not baked in here.
    let collapse = match cli.collapse {
        None => Collapse::Off,
        Some(names) if names.is_empty() => Collapse::All,
        Some(names) => Collapse::Only(names.into_iter().collect()),
    };

    let opts = FormatOpts {
        special: cli.special,
        xslt: cli.xslt,
        schematron: cli.schematron,
        xsd: cli.xsd,
        wsdl: cli.wsdl,
        msbuild: cli.msbuild,
        collapse: Collapse::Off,
    };

    // Plain XML rendering is the default. Suffix-based mode autodetection is
    // opt-in via `--auto` (or implied by `--bat`/`--html`), and only fills in
    // a mode when the user hasn't already forced one explicitly.
    let autodetect = (cli.auto || cli.bat || cli.html || cli.cat) && !opts.has_mode();

    // Prefixes to hide from element names: the explicit --hide-ns list, plus
    // (under --auto/--bat/--html/--cat) any inferred by sniffing the document type.
    let hide_ns: HashSet<String> = cli.hide_ns.iter().cloned().collect();
    let sniff = cli.auto || cli.bat || cli.html || cli.cat;

    // The cross-cutting options shared by every input. The per-file mode
    // (`file_opts`) is passed separately because it can vary under `--auto`.
    let cfg = ProcessOptions {
        format_override: cli.format.as_deref(),
        hide_ns: &hide_ns,
        sniff,
        select: cli.select.as_deref(),
        canonical: cli.canonical,
        paths: cli.paths,
        depth: cli.depth.unwrap_or(0),
        no_attrs: cli.no_attrs,
        fold: cli.fold,
        expand: cli.expand,
    };

    // Handle stdin input
    if cli.stdin {
        // When using stdin, files should be empty
        if !cli.files.is_empty() {
            return Err(anyhow::anyhow!(
                "Cannot specify both --stdin and file arguments"
            ));
        }

        // --raw skips the unxml transform entirely: highlight the stdin
        // text as-is (XML or HTML, same detection as normal processing).
        if cli.raw {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut std::io::stdin(), &mut bytes)
                .context("Failed to read from stdin")?;
            let content = match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(e) => e.into_bytes().into_iter().map(|b| b as char).collect(),
            };
            let is_html = detect_format(&content, "stdin") == InputFormat::Html;
            if cli.html {
                print!(
                    "{}",
                    highlight::html_page_raw(&content, is_html, cli.html_embed_css)?
                );
            } else {
                print!("{}", highlight::ansi_raw(&content, is_html)?);
            }
            return Ok(());
        }

        // Process stdin input (no path, so nothing to autodetect from).
        let mut stdin_opts = opts.clone();
        stdin_opts.collapse = collapse.clone();
        match process_stdin(&stdin_opts, &cfg) {
            Ok(output) => {
                if cli.html {
                    print!("{}", highlight::html_page(&output, cli.html_embed_css)?);
                } else if cli.cat {
                    print!("{}", highlight::ansi(&output)?);
                } else {
                    emit(&output, cli.bat);
                }
            }
            Err(e) => {
                eprintln!("Error processing stdin: {e}");
                return Err(e);
            }
        }
        return Ok(());
    }

    // Handle file input
    if cli.files.is_empty() {
        return Err(anyhow::anyhow!(
            "No files specified. Please provide at least one file or glob pattern, or use --stdin."
        ));
    }

    let mut all_files = Vec::new();

    // Expand glob patterns and collect all files
    for pattern in &cli.files {
        // An existing file takes precedence over glob interpretation: real
        // filenames can contain glob metacharacters (e.g. `Invoice-[uuid].xml`),
        // and an explicitly-passed file that exists should be read verbatim
        // rather than treated as a (likely non-matching) pattern.
        if std::path::Path::new(pattern).is_file() {
            all_files.push(pattern.clone());
        } else if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
            // This is a glob pattern
            match glob(pattern) {
                Ok(paths) => {
                    for entry in paths {
                        match entry {
                            Ok(path) => {
                                if let Some(path_str) = path.to_str() {
                                    all_files.push(path_str.to_string());
                                }
                            }
                            Err(e) => {
                                eprintln!("Warning: Error reading glob entry: {e}");
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Invalid glob pattern '{}': {}", pattern, e));
                }
            }
        } else {
            // This is a regular file path
            all_files.push(pattern.clone());
        }
    }

    if all_files.is_empty() {
        return Err(anyhow::anyhow!(
            "No files found matching the specified patterns."
        ));
    }

    // --raw skips the unxml transform entirely: read each file's original
    // text and highlight it as-is (XML or HTML, picked from the first file).
    if cli.raw {
        let multiple = all_files.len() > 1;
        let mut combined = String::new();
        let mut is_html = false;
        for (i, file_path) in all_files.iter().enumerate() {
            if i > 0 {
                combined.push('\n');
            }
            let content = read_file_lenient(file_path)?;
            if i == 0 {
                is_html = detect_format(&content, file_path) == InputFormat::Html;
            }
            if multiple {
                combined.push_str(&format!("<!-- FILE: {file_path} -->\n"));
            }
            combined.push_str(&content);
        }
        if cli.html {
            print!(
                "{}",
                highlight::html_page_raw(&combined, is_html, cli.html_embed_css)?
            );
        } else {
            print!("{}", highlight::ansi_raw(&combined, is_html)?);
        }
        return Ok(());
    }

    // Process each file, accumulating output so it can be sent to the pager
    // (or stdout) in one stream.
    let multiple = all_files.len() > 1;
    let mut combined = String::new();
    for (i, file_path) in all_files.iter().enumerate() {
        // Blank separator line between files (not before the first).
        if i > 0 {
            combined.push('\n');
        }

        // File header comment only when processing more than one file.
        if multiple {
            combined.push_str(&format!("// FILE: {file_path}\n"));
        }

        // When the user didn't force a mode, pick one from this file's
        // extension; otherwise honour the explicit flags for every file.
        let mut file_opts = if autodetect {
            detect_mode_from_ext(file_path)
        } else {
            opts.clone()
        };
        file_opts.collapse = collapse.clone();

        match process_file(file_path, &file_opts, &cfg) {
            Ok(output) => combined.push_str(&output),
            Err(e) => {
                eprintln!("Error processing file '{file_path}': {e}");
                // Continue processing other files instead of stopping
            }
        }
    }

    if cli.html {
        print!("{}", highlight::html_page(&combined, cli.html_embed_css)?);
    } else if cli.cat {
        print!("{}", highlight::ansi(&combined)?);
    } else {
        emit(&combined, cli.bat);
    }
    Ok(())
}
