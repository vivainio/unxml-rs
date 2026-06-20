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

A trailing colon (`:`) marks a line that opens an indented block, the way `:`
opens a suite in Python. Every construct that introduces a body — `match`,
`template`, `if`, `choose`, `when`, `else`, `foreach` — ends in `:`. `element`
and `call` take the colon only when they actually have a body; childless
constructs (`apply`, `<-`, `copy`, assignments) never do.

## Quick reference

| XSLT construct | unxml output |
| --- | --- |
| `xsl:template match="X"` | `match X:` |
| `xsl:template name="X"` | `template X:` |
| `xsl:apply-templates select="X"` | `apply X` |
| `xsl:value-of select="X"` | `<- X` |
| `xsl:copy-of select="X"` | `copy X` |
| `xsl:if test="X"` | `if X:` |
| `xsl:choose` | `choose:` |
| `xsl:when test="X"` | `when X:` |
| `xsl:otherwise` | `else:` |
| `xsl:for-each select="X"` | `foreach X:` |
| `xsl:variable name="x" select="…"` | `x := …` |
| `xsl:param name="x" select="…"` | `param x := …` |
| `xsl:with-param name="x" select="…"` | `x := …` |
| `xsl:call-template name="X"` | `call X:` (`call X` if no params) |
| `xsl:element name="X"` | `element X:` (`element X` if empty) |
| `xsl:attribute name="X"` | `@X = …` |
| `xsl:text` | `"…"` (quoted literal) |

Each construct is detailed below.

## Templates

An `xsl:template` is the unit of an XSLT stylesheet, and it comes in two
flavours that `unxml` deliberately renders differently:

- **`match` templates** are *declarative rules*. You don't call them by name;
  the XSLT processor walks the source tree and, for each node, fires whichever
  template's `match` pattern fits. This is the rule-based half of XSLT — you
  describe what to do with a kind of node and trust the engine to apply it
  wherever that node appears. These are triggered by `apply` (see below).
- **`name` templates** are *procedures*. They have no match pattern and never
  fire on their own; you invoke one explicitly by name with `call`, optionally
  passing parameters. This is the subroutine half of XSLT.

`unxml` gives each its own keyword so you can tell at a glance whether a block
is a rule the engine fires for you or a routine something else calls.

### `xsl:template` with `match` → `match X:`

A match template is a declarative rule, fired by `apply`. It leads with
`match` to mirror that — and to pair with the `apply` that triggers it:

```xml
<xsl:template match="item">
  ...
</xsl:template>
```
```text
match item:
  ...
```

A `mode` (or any attribute other than `match`/`name`) is preserved in
parentheses, before the colon:

```xml
<xsl:template match="item" mode="summary">
  ...
</xsl:template>
```
```text
match item (mode="summary"):
  ...
```

### `xsl:template` with `name` → `template X:`

A named template is a procedure, invoked by `call`. It leads with `template`
(no sigil needed — the keyword alone distinguishes it from a `match` rule):

```xml
<xsl:template name="formatDate">
  <xsl:param name="date"/>
</xsl:template>
```
```text
template formatDate:
  param date
```

## Applying and calling templates

### `xsl:apply-templates select="X"` → `apply X`

`apply-templates` is how the rule engine is set in motion. It selects a set of
nodes (`select="X"`) and, for each one, hands it to whichever `match` template
fits — so `apply` is the call site that triggers `match` rules without naming
them. With no `select` it processes the current node's children, which is what
drives the recursive descent typical of XSLT.

```xml
<xsl:apply-templates select="root/items"/>
```
```text
apply root/items
```

With no `select`, it renders as a bare `apply`. `apply` never takes a colon — it
delegates to other templates rather than opening a body of its own.

### `xsl:call-template` + `xsl:with-param`

`call-template` is the procedure call that invokes a named `template` by name.
It becomes `call`, and each `with-param` becomes an assignment (`name := value`)
— the same `:=` form used for variables. With parameters, `call` opens a block
and takes a colon:

```xml
<xsl:call-template name="formatDate">
  <xsl:with-param name="date" select="@created"/>
</xsl:call-template>
```
```text
call formatDate:
  date := @created
```

A parameterless `<xsl:call-template name="X"/>` renders as a bare `call X`.

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
if count(item) > 0:
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
choose:
  when @type = 'A':
    type = Type A
  else:
    type = Unknown
```

### `xsl:for-each select="X"` → `foreach X`

```xml
<xsl:for-each select="item">
  ...
</xsl:for-each>
```
```text
foreach item:
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
element dynamic:
  ...
```

An empty `<xsl:element name="X"/>` renders as a bare `element X`.

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
  match /Invoice:
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
