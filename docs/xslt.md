# XSLT transformations

When `unxml` runs in XSLT mode it rewrites the `xsl:*` vocabulary into a terse,
Pug-like pseudocode so a stylesheet reads like the program it actually is. This
page lists every transformation with side-by-side samples.

## Enabling XSLT mode

XSLT mode is selected automatically for `.xsl` / `.xslt` files, or forced with
the `--xslt` flag:

```bash
# Auto-detected from the extension
unxml stylesheet.xsl

# Forced (e.g. when reading from stdin)
cat stylesheet.xsl | unxml --stdin --xslt
```

Anything that is *not* an `xsl:*` element (literal result elements, `cbc:`/`cac:`
output, comments, etc.) is rendered with the normal `unxml` rules, so a
stylesheet shows both its control flow and the markup it emits.

## Quick reference

| XSLT construct | unxml output |
| --- | --- |
| `xsl:template match="X"` | `template X` |
| `xsl:template name="X"` | `template #X` |
| `xsl:apply-templates select="X"` | `apply X` |
| `xsl:value-of select="X"` | `<- X` |
| `xsl:copy-of select="X"` | `copy X` |
| `xsl:if test="X"` | `if X` |
| `xsl:choose` | `choose` |
| `xsl:when test="X"` | `when X` |
| `xsl:otherwise` | `else` |
| `xsl:for-each select="X"` | `foreach X` |
| `xsl:variable name="x" select="…"` | `x := …` |
| `xsl:param name="x" select="…"` | `param x := …` |
| `xsl:with-param name="x" select="…"` | `x := …` |
| `xsl:call-template name="X"` | `call X` |
| `xsl:element name="X"` | `element X` |
| `xsl:attribute name="X"` | `@X = …` |
| `xsl:text` | `"…"` (quoted literal) |

Each construct is detailed below.

## Templates

### `xsl:template` with `match` → `template X`

```xml
<xsl:template match="item">
  ...
</xsl:template>
```
```text
template item
  ...
```

A `mode` (or any attribute other than `match`/`name`) is preserved in
parentheses:

```xml
<xsl:template match="item" mode="summary">
  ...
</xsl:template>
```
```text
template item (mode="summary")
  ...
```

### `xsl:template` with `name` → `template #X`

A named template is prefixed with `#` to distinguish it from a match pattern:

```xml
<xsl:template name="formatDate">
  <xsl:param name="date"/>
</xsl:template>
```
```text
template #formatDate
  param date
```

## Applying and calling templates

### `xsl:apply-templates select="X"` → `apply X`

```xml
<xsl:apply-templates select="root/items"/>
```
```text
apply root/items
```

With no `select`, it renders as a bare `apply`.

### `xsl:call-template` + `xsl:with-param`

`call-template` becomes `call`, and each `with-param` becomes an assignment
(`name := value`) — the same `:=` form used for variables:

```xml
<xsl:call-template name="formatDate">
  <xsl:with-param name="date" select="@created"/>
</xsl:call-template>
```
```text
call formatDate
  date := @created
```

`with-param` accepts both the `select="…"` form (above) and the element-body
form (`<xsl:with-param>literal</xsl:with-param>`); both collapse to `:=`.

## Output: values and copies

### `xsl:value-of select="X"` → `<- X`

The `<-` arrow reads as "emit the value of":

```xml
<xsl:value-of select="$total"/>
```
```text
<- $total
```

### `xsl:copy-of select="X"` → `copy X`

```xml
<xsl:copy-of select="details/*"/>
```
```text
copy details/*
```

## Control flow

### `xsl:if test="X"` → `if X`

```xml
<xsl:if test="count(item) > 0">
  <hasItems>true</hasItems>
</xsl:if>
```
```text
if count(item) > 0
  hasItems = true
```

### `xsl:choose` / `xsl:when` / `xsl:otherwise`

`choose` is kept as a header; each `when` becomes `when X` and `otherwise`
becomes `else`:

```xml
<xsl:choose>
  <xsl:when test="@type = 'A'">
    <type>Type A</type>
  </xsl:when>
  <xsl:otherwise>
    <type>Unknown</type>
  </xsl:otherwise>
</xsl:choose>
```
```text
choose
  when @type = 'A'
    type = Type A
  else
    type = Unknown
```

### `xsl:for-each select="X"` → `foreach X`

```xml
<xsl:for-each select="item">
  ...
</xsl:for-each>
```
```text
foreach item
  ...
```

## Variables and parameters

### `xsl:variable` → `x := …`

A variable with `select` collapses to a one-liner:

```xml
<xsl:variable name="globalVar" select="/root/config/value"/>
```
```text
globalVar := /root/config/value
```

A variable whose value is in its body uses the body text (or nested children):

```xml
<xsl:variable name="literalVar">literal value</xsl:variable>
```
```text
literalVar := literal value
```

### `xsl:param` → `param x := …`

Parameters are prefixed with `param` so they stand out from local variables.
With `select`:

```xml
<xsl:param name="format" select="'YYYY-MM-DD'"/>
```
```text
param format := 'YYYY-MM-DD'
```

Without a default value:

```xml
<xsl:param name="date"/>
```
```text
param date
```

## Constructing result markup

### `xsl:element name="X"` → `element X`

```xml
<xsl:element name="dynamic">
  ...
</xsl:element>
```
```text
element dynamic
  ...
```

### `xsl:attribute name="X"` → `@X = …`

An attribute with simple text content collapses to one line:

```xml
<xsl:attribute name="source">generated</xsl:attribute>
```
```text
@source = generated
```

When the attribute value is computed (nested children), the children are
indented underneath `@X`:

```xml
<xsl:attribute name="id">
  <xsl:value-of select="@id"/>
</xsl:attribute>
```
```text
@id
  <- @id
```

### `xsl:text` → quoted literal

`xsl:text` is reduced to its quoted content; empty `xsl:text` is dropped:

```xml
<xsl:text>Some text content</xsl:text>
```
```text
"Some text content"
```

## Template expansion (`--expand`)

The `--expand` flag inlines the body of a matching template at each
`xsl:apply-templates` site instead of emitting `apply X`. This lets you read a
transformation end-to-end without jumping between templates.

`unxml` builds a registry of all templates (following `xsl:import` /
`xsl:include` recursively), then resolves the `select` against it. Matching is
forgiving: it tries an exact match, then the last path segment (so
`select="Input/Header"` matches `template Header`), and it understands union
patterns (`match="PAYEE|RECEIVER"` matches a select of `PAYEE`).

Given a `template items` defined elsewhere and an apply site:

```xml
<xsl:apply-templates select="root/items"/>
```

Without `--expand`:

```text
apply root/items
```

With `--expand`, the matching template (here resolved via the `items` path
segment) is inlined in place, marked with a comment:

```text
# [expanded: apply root/items]
output
  ...
```

```bash
unxml --xslt --expand stylesheet.xsl
```

## Non-XSLT content in a stylesheet

Elements outside the `xsl:` namespace are rendered with the standard `unxml`
formatter, so a stylesheet shows both its logic and the markup it produces.
Literal result elements with simple text use the `name = text` form; elements
with children nest normally; and `name`/attribute rendering follows the usual
rules.

A stylesheet that emits UBL keeps the literal `cac:`/`cbc:` result elements and
their XPath untouched (these are output, not instance noise):

```xml
<xsl:stylesheet version="1.0"
                xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
                xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
  <xsl:template match="/Invoice">
    <cac:Party>
      <cbc:Name><xsl:value-of select="cbc:Name"/></cbc:Name>
    </cac:Party>
  </xsl:template>
</xsl:stylesheet>
```
```text
xsl:stylesheet(
    version="1.0",
    xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2",
    xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2",
    xmlns:xsl="http://www.w3.org/1999/XSL/Transform")
  template /Invoice
    cac:Party
      cbc:Name
        <- cbc:Name
```

Note that `xsl:stylesheet`, `xsl:output`, and `xsl:strip-space` are *not*
specially transformed — they fall through to the generic formatter and keep
their attributes in parentheses.

For a stylesheet exercising every construct end-to-end, see
[`test-input/xslt-constructs.xsl`](../test-input/xslt-constructs.xsl) and run
`unxml --xslt test-input/xslt-constructs.xsl`.
