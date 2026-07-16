# MSBuild transformations

When `unxml` runs in MSBuild mode it folds the `Condition="..."` attribute —
present on almost any MSBuild element (`Target`, `PropertyGroup`,
`ItemGroup`, individual items, tasks, `When`, ...) — out of the attribute
list and into a leading `if COND:` guard. This is the single biggest source
of noise in real-world SDK `.targets`/`.props` files: `Condition` sorts
alphabetically among an element's other attributes, so it routinely forces a
short declaration to wrap across several lines just to fit one gate.

This mode currently covers Condition-folding only; everything else (item
metadata, `Choose`/`When`/`Otherwise`, `PropertyGroup`/`ItemGroup` nesting,
...) renders through the same generic Pug-like conversion as plain XML.

## Enabling MSBuild mode

MSBuild mode is selected automatically under `--auto` for `.targets`,
`.props`, `.csproj`, `.vbproj`, `.fsproj` and `.sqlproj` files, or forced
with the `--msbuild` flag:

```bash
# Auto-detected from the extension
unxml --auto Directory.Build.props

# Forced (e.g. when reading from stdin)
cat MyLib.targets | unxml --stdin --msbuild
```

Under `--auto`/`--bat`, an unrecognised extension (or stdin, which has no
path at all) still gets `--msbuild` if the parsed document *looks* like one:
an unprefixed `<Project>` root carrying the legacy
`xmlns="http://schemas.microsoft.com/developer/msbuild/2003"` or
`ToolsVersion`/`Sdk` attribute, or — since modern SDK-style files often carry
none of those — at least one child recognisable as MSBuild's own syntax
(`PropertyGroup`, `ItemGroup`, `Target`, `ItemDefinitionGroup`, `UsingTask`,
`Import`, `Choose`). This mirrors how a UBL/CII instance gets its namespace
prefixes hidden by content rather than extension. A `<Project>` root with
none of those markers is left as plain XML.

```bash
cat MyLib.targets | unxml --stdin --auto   # sniffed from content, no extension involved
```

## Quick reference

| MSBuild construct | unxml output |
| --- | --- |
| `Target Condition="C" Name="N" ...` | `if C:` / `  Target(Name="N", ...)` |

## `Condition="..."` → `if COND:`

The condition is pulled out as a leading guard, indented one level above the
element it gated; the element renders normally underneath with `Condition`
removed from its attribute list. Whitespace inside the condition — MSBuild
conditions are routinely wrapped across source lines with trailing
`and`/`or` — is collapsed to a single line, and incidental padding
(`" '$(X)' == 'true' "`) is trimmed.

```xml
<Target
    Condition="'$(_InvalidConfigurationWarning)' != 'true'"
    DependsOnTargets="$(BuildDependsOn)"
    Name="Build"
    Returns="@(TargetPathWithTargetPlatformMoniker)" />
```
```text
if '$(_InvalidConfigurationWarning)' != 'true':
  Target(
      DependsOnTargets="$(BuildDependsOn)",
      Name="Build",
      Returns="@(TargetPathWithTargetPlatformMoniker)")
```

A multi-line condition collapses onto one line:

```xml
<PropertyGroup
    Condition="'$(TargetFrameworkIdentifier)' == '.NETFramework' and
                          '$(HasRuntimeOutput)' == 'true' and
                          '$(RuntimeIdentifier)' == ''">
  <_UsingDefaultRuntimeIdentifier>true</_UsingDefaultRuntimeIdentifier>
</PropertyGroup>
```
```text
if '$(TargetFrameworkIdentifier)' == '.NETFramework' and '$(HasRuntimeOutput)' == 'true' and '$(RuntimeIdentifier)' == '':
  PropertyGroup
    _UsingDefaultRuntimeIdentifier = true
```

When `Condition` is the element's only attribute, the element renders bare
underneath the guard:

```xml
<PropertyGroup Condition="'$(Configuration)' == ''">
  <Configuration>Debug</Configuration>
</PropertyGroup>
```
```text
if '$(Configuration)' == '':
  PropertyGroup
    Configuration = Debug
```

## A worked example

For a `.targets` file exercising the constructs above, see
[`test-input/msbuild-constructs.targets`](../test-input/msbuild-constructs.targets)
and run `unxml --msbuild test-input/msbuild-constructs.targets`.
