# Unxml

Simplify and "flatten" XML files into a YAML-like readable format.

This is a Rust clone of the original [unxml](https://github.com/vivainio/unxml) F# tool.

## Installation

### From Source

```bash
git clone https://github.com/yourusername/unxml-rs
cd unxml-rs
cargo install --path .
```

### Using Cargo

```bash
cargo install unxml
```

## Usage

```bash
unxml <xml_file>
```

## Introduction

This command line application was developed for comparing XML files (e.g. database/application state dumps). It takes an XML file and converts it to a YAML-like syntax that is easier to read and compare.

### Example

Given this XML input:

```xml
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>netcoreapp2.1</TargetFramework>
    <PackAsTool>true</PackAsTool>
    <Description>Unxml 'pretty-prints' xml files in light, yamly, readable format</Description>
    <PackageVersion>1.0.0</PackageVersion>
  </PropertyGroup>
  <ItemGroup>
    <Compile Include="FileSystemHelper.fs"/>
    <Compile Include="MutableCol.fs"/>
    <Compile Include="Program.fs" />
  </ItemGroup>
</Project>
```

The output would be:

```
Project
  [Sdk]: Microsoft.NET.Sdk
  PropertyGroup
    OutputType = Exe
    TargetFramework = netcoreapp2.1
    PackAsTool = true
    Description = Unxml 'pretty-prints' xml files in light, yamly, readable format
    PackageVersion = 1.0.0
  ItemGroup
    Compile
      [Include]: FileSystemHelper.fs
    Compile
      [Include]: MutableCol.fs
    Compile
      [Include]: Program.fs
```

### Key Features

- **Attributes in Square Brackets**: Element attributes are displayed as `[attribute]: value`
- **Text Content with Equals**: Element text content is shown as `ElementName = text content`
- **Hierarchical Indentation**: Nested elements are properly indented
- **Clean Format**: Easy to read and compare, great for diffing

## Technical Details

- Built with Rust for performance and safety
- Uses `quick-xml` for fast XML parsing
- Uses `clap` for command-line argument parsing
- Proper error handling with `anyhow`

## License

MIT License - see LICENSE file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request. 