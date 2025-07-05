# Angular Control Flow Preservation Guide

This guide explains how to preserve Angular control flow structures (`@if`, `@for`, `@switch`) when using the unxml-rs parser.

## The Problem

By default, the HTML parser treats Angular control flow constructs as regular text content or ignores them completely:

```html
<!-- Original Angular template -->
@if (user.isLoggedIn) {
  <p>Welcome back, {{user.name}}!</p>
} @else {
  <p>Please log in</p>
}
```

```yaml
# Default parser output (structures lost)
p = Welcome back, {{user.name}}!
p = Please log in
```

## Solution: Three Approaches

### Approach 1: Pre-processing Script (✅ RECOMMENDED)

Use the provided `preprocess-angular.py` script to convert Angular syntax to temporary XML elements, parse, then restore:

```bash
# Usage
python preprocess-angular.py test-input/angular-control-flow.html
```

**How it works:**
1. Converts `@if (condition) {` → `<ng-if condition="condition">`
2. Runs unxml parser on the converted file
3. Converts back to Angular syntax in the output

**Example output:**
```yaml
@if (user.isLoggedIn) {
  p.highlight = Welcome back, {{user.name}}!
} @else {
  p = Please log in
}
```

### Approach 2: Manual HTML Comments (for specific cases)

For targeted preservation, manually mark Angular constructs with HTML comments:

```html
<!-- ng-if: user.isLoggedIn -->
<p class="highlight">Welcome back, {{user.name}}!</p>
<!-- ng-else -->
<p>Please log in</p>
<!-- /ng-if -->
```

Then use `restore-angular.py` to convert comments back to Angular syntax.

### Approach 3: Parser Modification (for advanced users)

Modify the Rust parser directly to recognize Angular constructs. This requires:
- Adding regex patterns for Angular syntax
- Creating special handling in the parsing logic
- Implementing custom output formatting

## Supported Angular Constructs

### ✅ Fully Supported

- **@if / @else-if / @else**
  ```typescript
  @if (condition) {
    // content
  } @else if (otherCondition) {
    // content  
  } @else {
    // content
  }
  ```

- **@for with @empty**
  ```typescript
  @for (item of items; track item.id) {
    // content
  } @empty {
    // fallback content
  }
  ```

- **@switch / @case / @default**
  ```typescript
  @switch (expression) {
    @case ('value1') {
      // content
    }
    @case ('value2') {
      // content
    }
    @default {
      // content
    }
  }
  ```

- **Variable assignment in @if**
  ```typescript
  @if (user.profile; as profile) {
    // use profile
  }
  ```

- **Contextual variables in @for**
  ```typescript
  @for (item of items; track item.id; let idx = $index, isFirst = $first) {
    // use idx, isFirst, etc.
  }
  ```

### ⚠️ Limitations

- Complex nested expressions may need manual adjustment
- Some edge cases in brace matching might require refinement
- Template expressions `{{}}` are preserved as-is (not evaluated)

## Usage Examples

### Basic Usage
```bash
# Parse Angular template with preservation
python preprocess-angular.py my-component.html

# Parse regular HTML (no preprocessing needed)
cargo run -- my-page.html
```

### Integration in Build Pipeline
```bash
# Add to your build script
for file in src/app/**/*.html; do
    if grep -q "@if\|@for\|@switch" "$file"; then
        python preprocess-angular.py "$file" > "dist/parsed-$(basename $file).yaml"
    else
        cargo run -- "$file" > "dist/parsed-$(basename $file).yaml"
    fi
done
```

## Testing

The repository includes comprehensive test files:

- `test-input/angular-control-flow.html` - Complete Angular control flow examples
- `test-input/angular-marked.html` - HTML comment approach example
- `expected-output/angular-control-flow.html.txt` - Expected output format

Run tests:
```bash
# Test preprocessing approach
python preprocess-angular.py test-input/angular-control-flow.html

# Test standard parsing
cargo run -- test-input/simple.html

# Run full test suite
python test-suite.py
```

## Best Practices

1. **Use preprocessing for files with Angular control flow** - Most reliable approach
2. **Keep template expressions simple** - Complex expressions may need manual formatting
3. **Test output thoroughly** - Verify that control flow logic is preserved correctly
4. **Use consistent indentation** - Helps with readability in the parsed output
5. **Consider build integration** - Automate the preprocessing step in your build pipeline

## Troubleshooting

### Common Issues

**Issue**: Braces `{}` appearing in output
```yaml
# Wrong
@if (condition) {{}
```
**Solution**: Use the preprocessing script which handles brace escaping automatically.

**Issue**: Nested structures not parsing correctly
```yaml
# Wrong - structure lost
p = content from if block
p = content from else block
```
**Solution**: Ensure proper closing of all Angular blocks and use preprocessing script.

**Issue**: Template expressions broken
```yaml
# Wrong
p = Welcome back, {{user.name!
```
**Solution**: Check for unescaped quotes in Angular expressions; the preprocessor handles most cases automatically.

## Contributing

To extend support for additional Angular features:

1. Add regex patterns in `preprocess_angular_to_xml()`
2. Add corresponding restoration patterns in `postprocess_xml_to_angular()`
3. Create test cases in `test-input/`
4. Update this documentation

## Files Reference

- `preprocess-angular.py` - Main preprocessing script
- `restore-angular.py` - HTML comment restoration script  
- `test-input/angular-control-flow.html` - Comprehensive test file
- `test-input/angular-marked.html` - HTML comment approach example
- `ANGULAR_CONTROL_FLOW_GUIDE.md` - This guide 