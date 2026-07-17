# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Common Commands

### Build Commands
```bash
# Build release version (recommended for testing)
cargo build --release

# Build debug version
cargo build

# Install locally
cargo install --path .
```

### Testing Commands
```bash
# Run the comprehensive test suite (unit tests + golden-file e2e test)
cargo test

# Update golden files after intentional output changes
UNXML_TEST_UPDATE=1 cargo test --test e2e_test

# Run specific test on single file
./target/release/unxml test-input/simple.xml

# Process XML from stdin (assumes XML format)
echo '<root><item>test</item></root>' | ./target/release/unxml --stdin

# Process from stdin with format override
cat some_file.html | ./target/release/unxml --stdin --format html

# Process from stdin with special transformations
cat business_file.xml | ./target/release/unxml --stdin --special
```

### Code Quality Commands
```bash
# Format code (must be run before commits)
cargo fmt

# Check formatting without changes
cargo fmt -- --check

# Run linting with clippy
cargo clippy

# Treat clippy warnings as errors
cargo clippy -- -D warnings

# Run all quality checks
cargo fmt -- --check && cargo clippy -- -D warnings && cargo test
```

## Architecture Overview

### Core Components

**Main Parser (`src/main.rs`):**
- `XmlElement` struct: Represents parsed XML/HTML elements with name, attributes, text content, and children
- `InputFormat` enum: Distinguishes between XML and HTML parsing modes
- `detect_format()`: Auto-detects input format based on file extension and content
- `parse_xml()`: Uses `quick-xml` for XML parsing with proper error handling
- `parse_html()`: Uses `scraper` for HTML parsing with CSS class handling
- `format_yaml_like()`: Converts parsed elements to Pug-like YAML output format

**Special Transformations:**
- `--special` flag enables proprietary transformation rules for specific XML elements
- Handles business workflow elements like `builtInMethodParameterList`, `parameter`, `variable`, `method`, `section`
- Transforms `include="foo"` attributes into `if foo` constructs
- Converts method references with `jumpToXmlFile` and `jumpToXPath` into structured calls

**Output Format:**
- Pug-like syntax with indentation for hierarchy
- Attributes in parentheses: `element(attr="value", boolean-attr)`
- Text content with equals: `element = text content`
- CSS classes attached to element names: `div.class1.class2`

### Test Infrastructure

**E2E Golden-File Test (`tests/e2e_test.rs`):**
- Runs the built `unxml` binary over every fixture in `test-input/` (XML, HTML, XSLT, Schematron, XSD, WSDL, MSBuild)
- Compares stdout against expected results in `expected-output/` directory
- Supports `UNXML_TEST_UPDATE=1` env var to refresh expected outputs
- Applies the same per-filename flag rules as the CLI (e.g. `--special` for `special-elements.xml`, `--xslt` for `.xsl`)
- Runs as part of `cargo test`, so it's covered by CI automatically

**Test Files:**
- `test-input/`: Sample XML and HTML files covering various scenarios
- `expected-output/`: Expected `.unxml` output files for regression testing

## Development Workflow

1. **Make Code Changes**: Edit `src/main.rs` or related files
2. **Run Tests**: Execute `cargo test` to check for regressions
3. **Code Quality**: Run `cargo fmt` and `cargo clippy` before committing
4. **Update Tests**: Use `UNXML_TEST_UPDATE=1 cargo test --test e2e_test` if output changes are intentional

## Key Dependencies

- `quick-xml`: Fast XML parsing
- `scraper`: HTML parsing with CSS selector support
- `clap`: Command-line argument parsing
- `anyhow`: Error handling
- `glob`: Pattern matching for file inputs

## File Organization

- `src/main.rs`: Main parser implementation
- `test-input/`: Test files (XML, HTML)
- `expected-output/`: Expected output files for regression testing
- `tests/e2e_test.rs`: Golden-file e2e test runner