# MSBuild transformations

When `unxml` runs in MSBuild mode it promotes the attributes that identify
declarations and operations into the readable part of the line:

- `Target Name="Build"` → `Target Build`
- `UsingTask TaskName="GenerateManifest"` → `UsingTask GenerateManifest`
- `Import Project="build\Common.targets"` → `Import "build\Common.targets"`
- `PropertyGroup Label="Compiler settings"` →
  `PropertyGroup "Compiler settings"`
- item `Include` / `Remove` / `Update` operations become `+=` / `-=` /
  `update`

It also folds `Condition="..."` — present on almost any MSBuild element — into
a leading `if COND:` guard, converts `Choose` branches to an
`if` / `else if` / `else` chain, and expands multi-entry
`DependsOnTargets` values into vertical lists.

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
| `Target Condition="C" Name="N" ...` | `if C:` / `  Target N(...)` |
| `UsingTask TaskName="T" ...` | `UsingTask T(...)` |
| `Import Project="P" ...` | `Import "P"(...)` |
| item `Include="X"` | `Item += "X"` |
| item `Remove="X"` | `Item -= "X"` |
| item `Update="X"` | `Item update "X"` |
| `Choose` / `When` / `Otherwise` | `if` / `else if` / `else` |

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
  Target Build(
      DependsOnTargets="$(BuildDependsOn)",
      Returns="@(TargetPathWithTargetPlatformMoniker)")
```

When `DependsOnTargets` contains multiple top-level semicolon-separated
entries, the dependencies become a vertical list. Semicolons inside quoted or
nested MSBuild expressions are left intact.

```xml
<Target Name="Build"
        DependsOnTargets="Prepare;Compile;CopyFiles"
        Returns="@(TargetPath)" />
```
```text
Target Build(
    DependsOnTargets=[
      Prepare
      Compile
      CopyFiles
    ],
    Returns="@(TargetPath)")
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

## Declarations and item operations

Identifying attributes move into the heading; other attributes remain in
parentheses and children keep their normal indentation.

```xml
<UsingTask TaskName="GenerateManifest"
           AssemblyFile="$(TasksPath)\BuildTasks.dll" />
<Import Project="build\Common.targets" Label="Shared build logic" />
<PropertyGroup Label="Compiler settings">
  <LangVersion>latest</LangVersion>
</PropertyGroup>
```
```text
UsingTask GenerateManifest(AssemblyFile="$(TasksPath)\BuildTasks.dll")
Import "build\Common.targets"(Label="Shared build logic")
PropertyGroup "Compiler settings"
  LangVersion = latest
```

Item operations use a compact verb while retaining metadata as children:

```xml
<ItemGroup>
  <Compile Include="Generated.cs" />
  <Compile Remove="Legacy\**\*.cs" />
  <Content Update="settings.json">
    <CopyToOutputDirectory>Always</CopyToOutputDirectory>
  </Content>
</ItemGroup>
```
```text
ItemGroup
  Compile += "Generated.cs"
  Compile -= "Legacy\**\*.cs"
  Content update "settings.json"
    CopyToOutputDirectory = Always
```

## Self-named property copies

Runs of two or more child assignments whose value is the same-named MSBuild
property are folded into a `copy properties:` block:

```xml
<_RestoreGraphEntry Include="$([System.Guid]::NewGuid())">
  <TargetFrameworkIdentifier>$(TargetFrameworkIdentifier)</TargetFrameworkIdentifier>
  <TargetFrameworkVersion>$(TargetFrameworkVersion)</TargetFrameworkVersion>
  <TargetFrameworkMoniker>$(TargetFrameworkMoniker)</TargetFrameworkMoniker>
  <TargetFrameworkProfile>$(TargetFrameworkProfile)</TargetFrameworkProfile>
</_RestoreGraphEntry>
```
```text
_RestoreGraphEntry += "$([System.Guid]::NewGuid())"
  copy properties:
    TargetFrameworkIdentifier
    TargetFrameworkVersion
    TargetFrameworkMoniker
    TargetFrameworkProfile
```

An isolated copy stays expanded, as do renamed copies and literal values.

## `Choose` → conditional chain

A well-formed `Choose` containing `When` branches and an optional final
`Otherwise` loses the XML scaffolding:

```text
if '$(OS)' == 'Windows_NT':
  PropertyGroup
    PathSeparator = ;
else if '$(OS)' == 'Unix':
  PropertyGroup
    PathSeparator = :
else:
  PropertyGroup
    PathSeparator = |
```

Unusual `Choose` structures carrying extra attributes or inter-branch comments
fall back to the ordinary structural rendering so information is not discarded.

## A worked example

For a `.targets` file exercising the constructs above, see
[`test-input/msbuild-constructs.targets`](../test-input/msbuild-constructs.targets)
and run `unxml --msbuild test-input/msbuild-constructs.targets`.
