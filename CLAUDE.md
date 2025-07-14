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
# Run the comprehensive test suite
python test-suite.py

# Update expected outputs after intentional changes
python test-suite.py --update

# Run specific test on single file
./target/release/unxml test-input/simple.xml
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

### Angular Template Processing
```bash
# Process Angular templates with control flow preservation
python preprocess-angular.py test-input/angular-control-flow.html

# Standard HTML processing
cargo run -- test-input/simple.html
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

**Test Suite (`test-suite.py`):**
- Comprehensive Python test runner that builds binaries and runs regression tests
- Compares outputs against expected results in `expected-output/` directory
- Supports `--update` mode to refresh expected outputs
- Handles both XML and HTML test files in `test-input/` directory
- Automatically detects and uses `--special` flag for `special-elements.xml`

**Test Files:**
- `test-input/`: Sample XML and HTML files covering various scenarios
- `expected-output/`: Expected `.pug` output files for regression testing
- Includes Angular template tests with control flow preservation

### Angular Template Support

**Preprocessing (`preprocess-angular.py`):**
- Converts Angular control flow syntax (`@if`, `@for`, `@switch`) to temporary XML elements
- Processes through standard parser pipeline
- Restores Angular syntax in final output
- Supports complex nesting, conditions, and contextual variables

**Angular Constructs:**
- `@if/@else-if/@else` conditions
- `@for` loops with `@empty` fallbacks
- `@switch/@case/@default` statements
- Variable assignments and contextual variables

## Development Workflow

1. **Make Code Changes**: Edit `src/main.rs` or related files
2. **Run Tests**: Execute `python test-suite.py` to check for regressions
3. **Code Quality**: Run `cargo fmt` and `cargo clippy` before committing
4. **Update Tests**: Use `python test-suite.py --update` if output changes are intentional

## Key Dependencies

- `quick-xml`: Fast XML parsing
- `scraper`: HTML parsing with CSS selector support
- `clap`: Command-line argument parsing
- `anyhow`: Error handling
- `glob`: Pattern matching for file inputs

## File Organization

- `src/main.rs`: Main parser implementation
- `test-input/`: Test files (XML, HTML, Angular templates)
- `expected-output/`: Expected output files for regression testing
- `test-suite.py`: Test runner script
- `preprocess-angular.py`: Angular template preprocessing
- `.cursor/rules/rust-format-lint.mdc`: Formatting and linting requirements