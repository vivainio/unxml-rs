//! unxml — simplify and "flatten" XML and HTML into a light, Pug/YAML-like
//! readable form. This file wires the modules together and drives the CLI.

mod cli;
mod document;
mod model;
mod parse;
mod process;
mod render;
mod schematron;
mod types;
mod wsdl;
mod xsd;
mod xslt;

use std::collections::HashSet;

use anyhow::Result;
use clap::Parser;
use glob::glob;

use crate::cli::Cli;
use crate::document::detect_mode_from_ext;
use crate::model::FormatOpts;
use crate::process::{emit, process_file, process_stdin};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let opts = FormatOpts {
        special: cli.special,
        xslt: cli.xslt,
        schematron: cli.schematron,
        xsd: cli.xsd,
        wsdl: cli.wsdl,
    };

    // Plain XML rendering is the default. Suffix-based mode autodetection is
    // opt-in via `--auto` (or implied by `--bat`), and only fills in a mode
    // when the user hasn't already forced one explicitly.
    let autodetect = (cli.auto || cli.bat) && !opts.has_mode();

    // Prefixes to hide from element names: the explicit --hide-ns list, plus
    // (under --auto/--bat) any inferred by sniffing the document type.
    let hide_ns: HashSet<String> = cli.hide_ns.iter().cloned().collect();
    let sniff = cli.auto || cli.bat;

    // Handle stdin input
    if cli.stdin {
        // When using stdin, files should be empty
        if !cli.files.is_empty() {
            return Err(anyhow::anyhow!(
                "Cannot specify both --stdin and file arguments"
            ));
        }

        // Process stdin input (no path, so nothing to autodetect from).
        match process_stdin(
            cli.format.as_deref(),
            &opts,
            &hide_ns,
            sniff,
            cli.select.as_deref(),
        ) {
            Ok(output) => emit(&output, cli.bat),
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
        if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
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
        let file_opts = if autodetect {
            detect_mode_from_ext(file_path)
        } else {
            opts
        };

        match process_file(
            file_path,
            cli.format.as_deref(),
            &file_opts,
            cli.expand,
            &hide_ns,
            sniff,
            cli.select.as_deref(),
        ) {
            Ok(output) => combined.push_str(&output),
            Err(e) => {
                eprintln!("Error processing file '{file_path}': {e}");
                // Continue processing other files instead of stopping
            }
        }
    }

    emit(&combined, cli.bat);
    Ok(())
}
