# Test Suite for unxml-rs

This directory contains a comprehensive test suite for the unxml-rs project, which includes sample XML and HTML files and a Python script to run regression tests.

## Files Created

### Sample Files (`test-input/` directory)

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
# Run tests (will build the binary automatically)
python test-suite.py
```

### Command Line Options

- `--sample-dir DIR` - Specify a different directory for sample files (default: `test-input`)
- `--output-dir DIR` - Specify a different directory for expected output files (default: `expected-output`)

### How It Works

1. **Discovery**: The script finds all `.xml`, `.html`, and `.htm` files in the sample directory
2. **Execution**: For each file, it runs the unxml tool and captures stdout, stderr, and return code
3. **Comparison**: It compares the current output with expected output files
   - `test-input/simple.xml` → compared with `expected-output/simple.xml.txt`
   - `test-input/form.html` → compared with `expected-output/form.html.txt`
4. **Reporting**: It reports any changes, new files, or failures

### Expected Output Files

The test suite creates individual `.txt` files containing the expected output for each test case. These files:

- **Enable easy visual inspection** of what the unxml tool produces
- **Support version control** - you can see exactly what changed in diffs
- **Allow manual comparison** using standard diff tools
- **Provide examples** of the unxml output format for documentation

### Test Results

The script will show:
- **NEW**: Files that don't have expected output files yet
- **PASS**: Files that produce output matching their expected output file
- **CHANGED**: Files where the output differs from the expected output
- **FAILED**: Files that caused the unxml tool to exit with a non-zero code

### Viewing Output

You can directly view the expected output files or compare them with current output:

```bash
# View expected output
cat expected-output/simple.xml.txt

# Compare current output with expected
diff expected-output/simple.xml.txt <(unxml test-input/simple.xml)

# Or on Windows
fc expected-output\simple.xml.txt output.tmp
```

## Integration with Development Workflow

### Typical Usage Patterns

1. **Development**: Make changes to the unxml code
2. **Testing**: Run `python test-suite.py` to check for regressions
3. **Update**: If changes are intentional, manually update the expected output files:
   ```bash
   # Update a specific expected output file
   unxml test-input/simple.xml > expected-output/simple.xml.txt
   ```

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

1. Add new XML or HTML files to the `test-input/` directory
2. Create expected output files by running unxml on them:
   ```bash
   unxml test-input/newfile.xml > expected-output/newfile.xml.txt
   ```
3. Run the test suite to verify everything works

## File Coverage

The test suite covers various scenarios:

- **XML Features**: Elements, attributes, namespaces, CDATA, comments, empty elements
- **HTML Features**: Forms, tables, embedded CSS/JavaScript, semantic elements
- **Edge Cases**: Empty files, malformed XML, special characters
- **Error Handling**: Files that should fail parsing

This comprehensive coverage helps ensure that changes to the unxml tool don't introduce regressions across different types of input files. 