//! unxml — simplify and "flatten" XML and HTML into a light, Pug/YAML-like
//! readable form. This file wires the modules together and drives the CLI.

mod canonical;
mod cli;
mod diff;
mod diffcmd;
mod document;
mod highlight;
mod inputs;
mod install;
mod json;
mod leo;
mod model;
mod msbuild;
mod outline;
mod outlinecmd;
mod parse;
mod patch;
mod patchcmd;
mod paths;
mod pathsel;
mod process;
mod render;
mod schematron;
mod types;
mod wsdl;
mod xmlwrite;
mod xpathmini;
mod xsd;
mod xslt;

use std::collections::HashSet;

use anyhow::{Context, Result};
use clap::Parser;

use crate::cli::Cli;
use crate::inputs::{Output, Renderer, ZipSpec};
use crate::model::{Collapse, FormatOpts};
use crate::parse::{decode_lenient, detect_format, expand_file_args, read_file_lenient};
use crate::process::{ProcessOptions, emit, process_stdin};
use crate::xpathmini::XPathMini;

fn main() -> Result<()> {
    // `unxml git <args>` is a thin passthrough to `git <args>` with the unxml
    // textconv driver applied for just this invocation. `unxml diff`/`unxml
    // patch`/`unxml outline` are intercepted the same way, for the same
    // reason: `Cli::files: Vec<String>` is a greedy positional that would
    // otherwise swallow "diff"/"patch"/"outline" and everything after it as
    // filenames rather than dispatching to a subcommand. All four are
    // checked ahead of the normal `Cli::parse()` below.
    let rest: Vec<String> = std::env::args().skip(1).collect();
    match rest.first().map(String::as_str) {
        Some("git") => return install::git_passthrough(&rest[1..]),
        Some("diff") => return diffcmd::run(&rest[1..]),
        Some("patch") => return patchcmd::run(&rest[1..]),
        Some("outline") => return outlinecmd::run(&rest[1..]),
        _ => {}
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
        leo: cli.leo,
        collapse: Collapse::Off,
    };

    // Plain XML rendering is the default. Suffix-based mode autodetection and
    // document-type sniffing are opt-in via `--auto`, which `--bat`/`--html`/
    // `--cat` also imply unless `--no-auto` cancels that implication (used
    // when one of those wants native highlighting on the exact literal,
    // non-auto output).
    let auto = cli.auto || ((cli.bat || cli.html || cli.cat) && !cli.no_auto);

    // Suffix-based mode autodetection only fills in a mode when the user
    // hasn't already forced one explicitly.
    let autodetect = auto && !opts.has_mode();

    // Prefixes to hide from element names: the explicit --hide-ns list, plus
    // (under --auto) any inferred by sniffing the document type.
    let hide_ns: HashSet<String> = cli.hide_ns.iter().cloned().collect();
    let sniff = auto;

    // Parse --select up front so a malformed pattern fails once, not per file.
    let selector = cli.select.as_deref().map(XPathMini::parse).transpose()?;

    // The cross-cutting options shared by every input. The per-file mode
    // (`file_opts`) is passed separately because it can vary under `--auto`.
    let cfg = ProcessOptions {
        format_override: cli.format.as_deref(),
        hide_ns: &hide_ns,
        sniff,
        select: selector.as_ref(),
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
        if !cli.files.is_empty() || !cli.zip.is_empty() {
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
            let format = detect_format(&content, "stdin");
            if cli.html {
                print!(
                    "{}",
                    highlight::html_page_raw(&content, format.syntax_name(), cli.html_embed_css)?
                );
            } else {
                print!("{}", highlight::ansi_raw(&content, format.syntax_name())?);
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
    if cli.files.is_empty() && cli.zip.is_empty() {
        return Err(anyhow::anyhow!(
            "No files specified. Please provide at least one file or glob pattern, or use --stdin."
        ));
    }

    // `archive.zip!/entry` arguments name entries inside an archive (the same
    // form a `// FILE:` header shows); they join the --zip archives.
    let (entry_args, file_args): (Vec<String>, Vec<String>) = cli
        .files
        .into_iter()
        .partition(|f| ZipSpec::is_entry_arg(f));
    let all_files = expand_file_args(&file_args)?;
    let mut zip_specs = ZipSpec::parse_all(&cli.zip)?;
    zip_specs.extend(ZipSpec::parse_all(&entry_args)?);

    if all_files.is_empty() && zip_specs.is_empty() {
        return Err(anyhow::anyhow!(
            "No files found matching the specified patterns."
        ));
    }

    // File header comment only when there is more than one input, which any
    // archive (or entry glob) may hold.
    let multiple =
        all_files.len() + zip_specs.len() > 1 || zip_specs.iter().any(|zip| !zip.is_single_entry());

    // --raw skips the unxml transform entirely: read each file's original
    // text and highlight it as-is (XML or HTML, picked from the first file).
    if cli.raw {
        let mut inputs = Vec::new();
        for file_path in &all_files {
            inputs.push((file_path.clone(), read_file_lenient(file_path)?));
        }
        for zip in &zip_specs {
            zip.for_each_entry(|name, bytes| inputs.push((name, decode_lenient(bytes))))
                .with_context(|| format!("Error reading zip '{}'", zip.archive))?;
        }
        let mut combined = String::new();
        let mut syntax_name = "XML";
        for (i, (name, content)) in inputs.iter().enumerate() {
            if i > 0 {
                combined.push('\n');
            }
            if i == 0 {
                syntax_name = detect_format(content, name).syntax_name();
            }
            if multiple {
                combined.push_str(&format!("<!-- FILE: {name} -->\n"));
            }
            combined.push_str(content);
        }
        if cli.html {
            print!(
                "{}",
                highlight::html_page_raw(&combined, syntax_name, cli.html_embed_css)?
            );
        } else {
            print!("{}", highlight::ansi_raw(&combined, syntax_name)?);
        }
        return Ok(());
    }

    // Render every input (in parallel, emitted in input order). Output streams
    // to stdout as it is produced, unless it must be post-processed as a whole
    // (highlighted, or handed to the `bat` pager).
    let mut out = if cli.html || cli.cat || cli.bat {
        Output::Buffer(String::new())
    } else {
        Output::stdout()
    };
    let renderer = Renderer {
        opts: &opts,
        autodetect,
        collapse: &collapse,
        cfg: &cfg,
    };
    let mut first = true;
    renderer.run(&all_files, &zip_specs, |rendered| {
        let output = match rendered.result {
            Ok(output) => Some(output),
            Err(e) => {
                // Continue processing other files instead of stopping
                eprintln!("Error processing file '{}': {e}", rendered.name);
                None
            }
        };
        // Under --select an input without matches is left out entirely, so a
        // search over many files lists only the hits.
        if cfg.select.is_some() && output.as_deref().is_none_or(str::is_empty) {
            return;
        }
        // Blank separator line between files (not before the first).
        if !first {
            out.push("\n");
        }
        first = false;
        if multiple {
            out.push(&format!("// FILE: {}\n", rendered.name));
        }
        if let Some(output) = output {
            out.push(&output);
        }
    });

    let Some(combined) = out.finish() else {
        return Ok(());
    };
    if cli.html {
        print!("{}", highlight::html_page(&combined, cli.html_embed_css)?);
    } else if cli.cat {
        print!("{}", highlight::ansi(&combined)?);
    } else {
        emit(&combined, cli.bat);
    }
    Ok(())
}
