# Test Suite for unxml-rs

This directory contains a comprehensive test suite for the unxml-rs project, which includes sample XML and HTML files and a Python script to run regression tests.

## Files Created

### Sample Files (`sample-output/` directory)

The test suite includes various XML and HTML files to test different scenarios:

**XML Files:**
- `simple.xml` - Basic XML structure with simple elements and attributes
- `complex.xml` - Complex nested XML with namespaces, CDATA, comments, and various data types
- `config.xml` - Configuration-style XML with nested structures and mixed content
- `data.xml` - User data XML with special characters, empty elements, and mixed content
- `namespace.xml` - XML with multiple namespaces and prefixed elements
- `empty.xml` - Minimal XML file to test edge cases
- `malformed.xml` - Intentionally malformed XML to test error handling

**HTML Files:**
- `simple.html` - Basic HTML page with semantic structure
- `form.html` - HTML form with various input types and embedded CSS/JavaScript
- `complex.html` - Complex HTML page with multiple sections, embedded content, and nested structures

### Test Script

- `test-suite.py` - Python script that runs the unxml tool on all sample files and compares outputs
- `expected-output/` - Directory containing expected output files (e.g., `simple.xml.txt`, `complex.html.txt`)

## Usage

### Running the Test Suite

```bash
# Run tests (will build the binary automatically if needed)
python test-suite.py

# Or run with explicit build step
python test-suite.py --build

# Run tests and update the baseline
python test-suite.py --update-baseline
```

### Command Line Options

- `--sample-dir DIR` - Specify a different directory for sample files (default: `sample-output`)
- `--baseline FILE` - Specify a different baseline file (default: `test-baseline.json`)
- `--output-dir DIR` - Specify a different directory for expected output files (default: `expected-output`)
- `--update-baseline` - Update the baseline with current test results
- `--show-output FILENAME` - Show detailed output for a specific file
- `--build` - Build the unxml binary before running tests

### How It Works

1. **Discovery**: The script finds all `.xml`, `.html`, and `.htm` files in the sample directory
2. **Execution**: For each file, it runs the unxml tool and captures stdout, stderr, and return code
3. **Output Storage**: The stdout is saved to individual `.txt` files in the expected-output directory
   - `sample-output/simple.xml` → `expected-output/simple.xml.txt`
   - `sample-output/form.html` → `expected-output/form.html.txt`
4. **Comparison**: It compares the current output with the stored expected output files
5. **Reporting**: It reports any changes, new files, or failures

### Expected Output Files

The test suite creates individual `.txt` files containing the expected output for each test case. These files:

- **Enable easy visual inspection** of what the unxml tool produces
- **Support version control** - you can see exactly what changed in diffs
- **Allow manual comparison** using standard diff tools
- **Provide examples** of the unxml output format for documentation

### Test Results

The script will show:
- **NEW**: Files that weren't in the previous baseline
- **PASS**: Files that produce the same output as the baseline
- **CHANGED**: Files where the output differs from the baseline
- **FAILED**: Files that caused the unxml tool to exit with a non-zero code

### Baseline Management

The first time you run the test suite, all files will be marked as "NEW". To establish a baseline:

```bash
python test-suite.py --update-baseline
```

This saves the current results as the baseline for future comparisons.

### Viewing Detailed Output

To see the actual output produced by unxml for a specific file:

```bash
python test-suite.py --show-output simple.xml
```

You can also directly view the expected output files:

```bash
# View expected output
cat expected-output/simple.xml.txt

# Compare current output with expected
diff expected-output/simple.xml.txt <(unxml sample-output/simple.xml)

# Or on Windows
fc expected-output\simple.xml.txt output.tmp
```

## Integration with Development Workflow

### Typical Usage Patterns

1. **Initial Setup**: Run `python test-suite.py --update-baseline` to create the initial baseline
2. **Development**: Make changes to the unxml code
3. **Testing**: Run `python test-suite.py` to check for regressions
4. **Update**: If changes are intentional, run `python test-suite.py --update-baseline`

### CI/CD Integration

The test script returns appropriate exit codes:
- `0` - All tests passed (no changes detected)
- `1` - Tests failed or changes detected

This makes it suitable for use in CI/CD pipelines:

```bash
# In your CI script
python test-suite.py
if [ $? -ne 0 ]; then
    echo "Tests failed or output changed!"
    exit 1
fi
```

## Adding New Test Cases

To add new test cases:

1. Add new XML or HTML files to the `sample-output/` directory
2. Run the test suite - new files will be automatically detected
3. Update the baseline if the new files are expected to pass

## File Coverage

The test suite covers various scenarios:

- **XML Features**: Elements, attributes, namespaces, CDATA, comments, empty elements
- **HTML Features**: Forms, tables, embedded CSS/JavaScript, semantic elements
- **Edge Cases**: Empty files, malformed XML, special characters
- **Error Handling**: Files that should fail parsing

This comprehensive coverage helps ensure that changes to the unxml tool don't introduce regressions across different types of input files. 