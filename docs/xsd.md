# XSD transformations

When `unxml` runs in XSD mode it rewrites the `xs:*` / `xsd:*` vocabulary of an
XML Schema into a terse, type-declaration-like pseudocode, so a schema reads like
the data model it actually describes. This page lists every transformation with
side-by-side samples.

## Enabling XSD mode

XSD mode is selected automatically for `.xsd` files, or forced with the `--xsd`
flag:

```bash
# Auto-detected from the extension
unxml schema.xsd

# Forced (e.g. when reading from stdin)
cat schema.xsd | unxml --stdin --xsd
```

Both the `xs:` and `xsd:` prefixes are recognized. The XSD vocabulary prefix
itself (`xmlns:xs` / `xmlns:xsd`) is treated as implied and not echoed back.

## Quick reference

| XSD construct | unxml output |
| --- | --- |
| `xs:schema` | `schema [targetNamespace] (flags)` |
| `xs:import namespace="N" schemaLocation="L"` | `import N from L` |
| `xs:include schemaLocation="L"` | `include L` |
| `xs:redefine schemaLocation="L"` | `redefine L` |
| `xs:element name="N" type="T"` | `element N : T` |
| `xs:element ref="R"` | `ref R` |
| `xs:attribute name="N" type="T"` | `@N : T` |
| `xs:attribute ref="R"` | `@ref R` |
| `xs:complexType name="N"` | `type N` |
| `xs:simpleType name="N"` | `type N : …` |
| `xs:sequence` / `xs:choice` / `xs:all` | `sequence` / `choice` / `all` |
| `xs:extension base="B"` | `… extends B` |
| `xs:restriction base="B"` | `… restricts B` (or `restriction B`) |
| `xs:enumeration value="V"` | `\| V` |
| `xs:pattern value="V"` | `pattern V` |
| `xs:minInclusive` / `maxLength` / … | `minInclusive V` / `maxLength V` / … |
| `xs:group ref="R"` | `group ref R` |
| `xs:attributeGroup ref="R"` | `attributeGroup ref R` |
| `xs:union memberTypes="M"` | `union M` |
| `xs:list itemType="T"` | `list T` |
| `xs:any` | `any [ns] [occurs] [(skip\|lax)]` |
| `xs:anyAttribute` | `@any [ns] [(skip\|lax)]` |
| `xs:key` / `xs:keyref` / `xs:unique` | `key N` / `keyref N` / `unique N` |
| `xs:selector` / `xs:field` | `selector X` / `field X` |
| `xs:notation name="N"` | `notation N` |
| `xs:documentation` | `// text` |

A handful of symbol conventions recur across constructs:

| Symbol | Meaning |
| --- | --- |
| `: T` | typed as `T` |
| `= V` | default value `V` |
| `== V` | fixed value `V` |
| `?` | optional (`minOccurs="0"`) |
| `*` | zero or more |
| `+` | one or more |
| `[m..n]` | bounded occurrence or value range |
| `\| V` | one enumeration choice |
| `@N` | an attribute |

Each construct is detailed below.

## Schema and module structure

### `xs:schema`

The target namespace follows `schema`; `elementFormDefault`/`attributeFormDefault`
are shown in parentheses only when `qualified`:

```xml
<xs:schema targetNamespace="http://example.com/orders"
           elementFormDefault="qualified">
```
```text
schema http://example.com/orders (elementFormDefault=qualified)
```

Namespace bindings are documented as child lines so prefixes used elsewhere
(`ref cbc:Foo`, `type udt:Bar`) are traceable. The default namespace becomes
`xmlns = uri`; prefixed ones become `ns prefix = uri`:

```xml
<xs:schema xmlns="http://example.com/orders"
           xmlns:cbc="urn:…:CommonBasicComponents-2">
```
```text
schema
  xmlns = http://example.com/orders
  ns cbc = urn:…:CommonBasicComponents-2
```

### `xs:import` → `import N from L`

```xml
<xs:import namespace="http://example.com/customer" schemaLocation="customer.xsd"/>
```
```text
import http://example.com/customer from customer.xsd
```

With only a namespace it renders as `import N`; with only a location, `import L`.

### `xs:include` / `xs:redefine` → `include L` / `redefine L`

```xml
<xs:include schemaLocation="common.xsd"/>
```
```text
include common.xsd
```

## Elements

### `xs:element name="N" type="T"` → `element N : T`

```xml
<xs:element name="Order" type="OrderType"/>
```
```text
element Order : OrderType
```

Inside a content model (a `sequence`/`choice`/`all` or a complex type body) the
`element` keyword is dropped, since the context makes it obvious:

```xml
<xs:element name="OrderId" type="xs:string"/>
```
```text
OrderId : xs:string
```

### `xs:element ref="R"` → `ref R`

```xml
<xs:element ref="Invoice" maxOccurs="unbounded"/>
```
```text
ref Invoice +
```

### Element modifiers

`abstract` becomes a leading keyword; the rest become trailing annotations.

```xml
<xs:element name="AbstractItem" type="LineItemType" abstract="true"/>
```
```text
abstract element AbstractItem : LineItemType
```

```xml
<xs:element name="DiscountedItem" type="DiscountedLineItemType"
            substitutionGroup="AbstractItem"/>
```
```text
element DiscountedItem : DiscountedLineItemType substitutes AbstractItem
```

```xml
<xs:element name="OptionalNote" type="xs:string" nillable="true" minOccurs="0"/>
```
```text
element OptionalNote : xs:string ? nillable
```

```xml
<xs:element name="Color" type="ColorType" block="extension" final="restriction"/>
```
```text
element Color : ColorType block extension final restriction
```

`default` renders as `= V`, `fixed` as `== V`:

```xml
<xs:element name="DefaultedFlag" type="xs:boolean" default="false"/>
```
```text
element DefaultedFlag : xs:boolean = false
```

### Anonymous nested types

An element (or attribute) whose type is declared inline, rather than referenced
by name, has that type folded directly beneath it — the redundant bare `type`
line is dropped. A nested `simpleType` is inlined onto the same line:

```xml
<xs:element name="Batch">
  <xs:complexType>
    <xs:sequence>
      <xs:element name="Period" type="xsd:string"/>
    </xs:sequence>
    <xs:attribute name="type" use="required">
      <xs:simpleType>
        <xs:restriction base="xsd:NMTOKEN">
          <xs:enumeration value="xml02"/>
        </xs:restriction>
      </xs:simpleType>
    </xs:attribute>
  </xs:complexType>
</xs:element>
```
```text
element Batch
  Period : xsd:string
  @type : xsd:NMTOKEN (required)
    | xml02
```

## Occurrence constraints

`minOccurs`/`maxOccurs` are folded into a compact suffix on whatever they
annotate (element, group, wildcard):

| minOccurs / maxOccurs | suffix |
| --- | --- |
| `1` / `1` (or unspecified) | *(none)* |
| `0` / `1` | ` ?` |
| `0` / `unbounded` | ` *` |
| `1` / `unbounded` | ` +` |
| `m` / `n` | ` [m..n]` |

```xml
<xs:element name="LineItem" type="LineItemType" minOccurs="1" maxOccurs="unbounded"/>
<xs:element name="Tag" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
<xs:element name="Comment" type="xs:string" minOccurs="0"/>
<xs:element ref="Sender" minOccurs="3" maxOccurs="3"/>
```
```text
LineItem : LineItemType +
Tag : xs:string *
Comment : xs:string ?
ref Sender [3..3]
```

## Attributes

### `xs:attribute name="N" type="T"` → `@N : T`

```xml
<xs:attribute name="priority" type="xs:int" default="0"/>
```
```text
@priority : xs:int = 0
```

`use="required"`/`"prohibited"` render in parentheses; `fixed` uses `==`:

```xml
<xs:attribute name="currency" type="CurrencyCode" use="required"/>
<xs:attribute name="version" type="xs:string" fixed="1.0"/>
<xs:attribute name="legacy" type="xs:string" use="prohibited"/>
```
```text
@currency : CurrencyCode (required)
@version : xs:string == 1.0
@legacy : xs:string (prohibited)
```

### `xs:attribute ref="R"` → `@ref R`

```xml
<xs:attribute ref="xml:lang"/>
```
```text
@ref xml:lang
```

## Complex types

### `xs:complexType name="N"` → `type N`

A transparent `xs:sequence` (one with no occurrence constraints) is folded away,
so members sit directly under the type:

```xml
<xs:complexType name="LineItemType">
  <xs:sequence>
    <xs:element name="Sku" type="xs:string"/>
    <xs:element name="UnitPrice" type="xs:decimal"/>
  </xs:sequence>
</xs:complexType>
```
```text
type LineItemType
  Sku : xs:string
  UnitPrice : xs:decimal
```

`abstract` becomes a leading keyword; `mixed` a trailing one:

```xml
<xs:complexType name="MixedNoteType" mixed="true">
  ...
</xs:complexType>
```
```text
type MixedNoteType mixed
  ...
```

### Derivation: `extends` / `restricts`

A `complexContent`/`simpleContent` wrapping a single `extension`/`restriction`
collapses into the type header, and the derived members fold up beneath it:

```xml
<xs:complexType name="DiscountedLineItemType">
  <xs:complexContent>
    <xs:extension base="LineItemType">
      <xs:sequence>
        <xs:element name="DiscountPct" type="xs:decimal"/>
      </xs:sequence>
    </xs:extension>
  </xs:complexContent>
</xs:complexType>
```
```text
type DiscountedLineItemType extends LineItemType
  DiscountPct : xs:decimal
```

A `restriction` base reads as `type N restricts Base`.

### Content models: `sequence` / `choice` / `all`

A non-transparent compositor (e.g. one nested inside another, or carrying
occurrence constraints) is kept as a header line:

```xml
<xs:choice>
  <xs:element name="Email" type="xs:string"/>
  <xs:element name="Phone" type="xs:string"/>
</xs:choice>
```
```text
choice
  Email : xs:string
  Phone : xs:string
```

## Simple types

`xs:simpleType` becomes `type N`, with its restriction/list/union inlined into
the header where possible.

### Enumerations

A `restriction` carrying enumerations (and optional patterns) inlines the base
and lists each value with `|`:

```xml
<xs:simpleType name="CurrencyCode">
  <xs:restriction base="xs:string">
    <xs:enumeration value="USD"/>
    <xs:enumeration value="EUR"/>
    <xs:pattern value="[A-Z]{3}"/>
  </xs:restriction>
</xs:simpleType>
```
```text
type CurrencyCode : xs:string
  | USD
  | EUR
  pattern [A-Z]{3}
```

### Numeric range

A restriction with only `minInclusive`/`maxInclusive` collapses to a range:

```xml
<xs:simpleType name="PositiveQuantity">
  <xs:restriction base="xs:int">
    <xs:minInclusive value="1"/>
    <xs:maxInclusive value="9999"/>
  </xs:restriction>
</xs:simpleType>
```
```text
type PositiveQuantity : xs:int [1..9999]
```

### List and union

```xml
<xs:simpleType name="StringList">
  <xs:list itemType="xs:string"/>
</xs:simpleType>
```
```text
type StringList : list xs:string
```

```xml
<xs:simpleType name="StringOrInt">
  <xs:union memberTypes="xs:string xs:int"/>
</xs:simpleType>
```
```text
type StringOrInt : union xs:string xs:int
```

### Facets (when not inlined)

When a restriction can't be inlined, its facets render as their own lines:
`enumeration` as `| V`, everything else as `local V`.

| Facet | output |
| --- | --- |
| `xs:enumeration value="V"` | `\| V` |
| `xs:pattern value="V"` | `pattern V` |
| `xs:minLength` / `xs:maxLength` / `xs:length` | `minLength V` / … |
| `xs:minInclusive` / `xs:maxInclusive` | `minInclusive V` / … |
| `xs:minExclusive` / `xs:maxExclusive` | `minExclusive V` / … |
| `xs:totalDigits` / `xs:fractionDigits` | `totalDigits V` / … |
| `xs:whiteSpace` | `whiteSpace V` |

## Groups

### `xs:group` / `xs:attributeGroup`

A reference uses `ref`; a definition uses the name and folds its members
underneath:

```xml
<xs:group ref="AddressGroup" minOccurs="0"/>
```
```text
group ref AddressGroup ?
```

```xml
<xs:attributeGroup name="AuditAttrs">
  <xs:attribute name="createdBy" type="xs:string" use="required"/>
</xs:attributeGroup>
```
```text
attributeGroup AuditAttrs
  @createdBy : xs:string (required)
```

## Wildcards

### `xs:any` → `any`

The namespace, occurrence, and `processContents` (`skip`/`lax`) are all shown:

```xml
<xs:any namespace="##other" minOccurs="0" maxOccurs="unbounded" processContents="lax"/>
```
```text
any ##other * (lax)
```

### `xs:anyAttribute` → `@any`

```xml
<xs:anyAttribute namespace="##other"/>
```
```text
@any ##other
```

## Identity constraints

### `xs:key` / `xs:keyref` / `xs:unique`

Each takes its `name`; the nested `selector`/`field` render their `xpath`:

```xml
<xs:key name="orderKey">
  <xs:selector xpath="LineItem"/>
  <xs:field xpath="@sku"/>
</xs:key>
```
```text
key orderKey
  selector LineItem
  field @sku
```

`keyref` and `unique` follow the same shape.

## Annotations and documentation

`xs:annotation` is transparent. `xs:documentation` (and `xs:appinfo`) become
`//` comments, with whitespace collapsed:

```xml
<xs:annotation>
  <xs:documentation>An order with header and line items.</xs:documentation>
</xs:annotation>
```
```text
// An order with header and line items.
```

For UBL/CCTS schemas where the prose is buried in nested elements
(`<ccts:Definition>…</ccts:Definition>`), the text is pulled out of
`Definition`/`Description` children into the same `//` form.

## A worked example

For a schema exercising the full vocabulary end-to-end, see
[`test-input/xsd-constructs.xsd`](../test-input/xsd-constructs.xsd) and run
`unxml --xsd test-input/xsd-constructs.xsd`.
