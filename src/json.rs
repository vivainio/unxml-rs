//! Compact, diff-friendly JSON rendering.
//!
//! Objects use the same indentation and `key = value` vocabulary as generic
//! XML output. Uniform arrays of scalar objects use a TOON-inspired tabular
//! form so repeated field names are written only once.

use anyhow::{Context, Result};
use serde_json::{Map, Value};

pub(crate) fn render_json(content: &str, canonical: bool, auto: bool) -> Result<String> {
    let value: Value = serde_json::from_str(content).context("Failed to parse JSON")?;
    let mut out = String::new();
    if auto && is_json_schema_document(&value) {
        render_schema_document(&value, canonical, &mut out);
    } else {
        let context = if auto && is_openapi_document(&value) {
            JsonContext::OpenApi
        } else {
            JsonContext::Generic
        };
        render_root(&value, canonical, context, &mut out);
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq)]
enum JsonContext {
    Generic,
    OpenApi,
    OpenApiComponents,
}

fn render_root(value: &Value, canonical: bool, context: JsonContext, out: &mut String) {
    match value {
        Value::Object(object) => render_object(object, 0, canonical, context, out),
        Value::Array(array) => render_array(None, array, 0, canonical, context, out),
        Value::String(value) if is_block_string(value) => {
            out.push_str("=\n");
            render_block_string(value, 1, out);
        }
        scalar => {
            out.push_str("= ");
            out.push_str(&render_scalar(scalar, ScalarContext::Plain));
            out.push('\n');
        }
    }
}

fn render_object(
    object: &Map<String, Value>,
    indent: usize,
    canonical: bool,
    context: JsonContext,
    out: &mut String,
) {
    for key in ordered_keys(object, canonical) {
        render_named(key, &object[key], indent, canonical, context, out);
    }
}

fn render_named(
    key: &str,
    value: &Value,
    indent: usize,
    canonical: bool,
    context: JsonContext,
    out: &mut String,
) {
    let ind = "  ".repeat(indent);
    let rendered_key = render_key(key);

    if context == JsonContext::OpenApi
        && key == "schema"
        && let Value::Object(schema) = value
    {
        render_schema_node(&rendered_key, schema, false, indent, canonical, out);
        return;
    }

    if context == JsonContext::OpenApiComponents
        && key == "schemas"
        && let Value::Object(schemas) = value
    {
        out.push_str(&format!("{ind}{rendered_key}\n"));
        for name in ordered_keys(schemas, canonical) {
            if let Some(schema) = schemas[name].as_object() {
                render_schema_node(name, schema, false, indent + 1, canonical, out);
            } else {
                render_named(
                    name,
                    &schemas[name],
                    indent + 1,
                    canonical,
                    JsonContext::Generic,
                    out,
                );
            }
        }
        return;
    }

    let child_context = if context == JsonContext::OpenApi && key == "components" {
        JsonContext::OpenApiComponents
    } else if context == JsonContext::OpenApiComponents {
        JsonContext::OpenApi
    } else {
        context
    };

    match value {
        Value::String(value) if is_block_string(value) => {
            out.push_str(&format!("{ind}{rendered_key} =\n"));
            render_block_string(value, indent + 1, out);
        }
        scalar if is_scalar(scalar) => {
            out.push_str(&format!(
                "{ind}{rendered_key} = {}\n",
                render_scalar(scalar, ScalarContext::Plain)
            ));
        }
        Value::Object(object) if object.is_empty() => {
            out.push_str(&format!("{ind}{rendered_key} = {{}}\n"));
        }
        Value::Object(object) => {
            out.push_str(&format!("{ind}{rendered_key}\n"));
            render_object(object, indent + 1, canonical, child_context, out);
        }
        Value::Array(array) => render_array(
            Some(&rendered_key),
            array,
            indent,
            canonical,
            child_context,
            out,
        ),
        _ => unreachable!(),
    }
}

fn render_array(
    key: Option<&str>,
    array: &[Value],
    indent: usize,
    canonical: bool,
    context: JsonContext,
    out: &mut String,
) {
    let ind = "  ".repeat(indent);
    let name = key.unwrap_or("");

    if array.is_empty() {
        if key.is_some() {
            out.push_str(&format!("{ind}{name} = []\n"));
        } else {
            out.push_str(&format!("{ind}[]\n"));
        }
        return;
    }

    if array.iter().all(is_scalar) {
        let values = array
            .iter()
            .map(|value| render_scalar(value, ScalarContext::Delimited))
            .collect::<Vec<_>>()
            .join(", ");
        if key.is_some() {
            out.push_str(&format!("{ind}{name} = [{values}]\n"));
        } else {
            out.push_str(&format!("{ind}[{values}]\n"));
        }
        return;
    }

    if let Some(columns) = table_columns(array, canonical) {
        let header = columns
            .iter()
            .map(|column| render_key(column))
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&format!("{ind}{name}[]{{{header}}}\n"));
        let row_indent = "  ".repeat(indent + 1);
        for item in array {
            let object = item.as_object().expect("table rows are objects");
            let row = columns
                .iter()
                .map(|column| render_scalar(&object[*column], ScalarContext::Delimited))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("{row_indent}{row}\n"));
        }
        return;
    }

    for item in array {
        match item {
            Value::String(value) if is_block_string(value) => {
                out.push_str(&format!("{ind}{name}[] =\n"));
                render_block_string(value, indent + 1, out);
            }
            scalar if is_scalar(scalar) => {
                out.push_str(&format!(
                    "{ind}{name}[] = {}\n",
                    render_scalar(scalar, ScalarContext::Plain)
                ));
            }
            Value::Object(object) if object.is_empty() => {
                out.push_str(&format!("{ind}{name}[] = {{}}\n"));
            }
            Value::Object(object) => {
                out.push_str(&format!("{ind}{name}[]\n"));
                render_object(object, indent + 1, canonical, context, out);
            }
            Value::Array(nested) => {
                out.push_str(&format!("{ind}{name}[]\n"));
                render_array(None, nested, indent + 1, canonical, context, out);
            }
            _ => unreachable!(),
        }
    }
}

fn table_columns(array: &[Value], canonical: bool) -> Option<Vec<&str>> {
    if array.len() < 2 {
        return None;
    }

    let first = array.first()?.as_object()?;
    if first.is_empty() || !first.values().all(is_scalar) {
        return None;
    }

    let mut columns: Vec<&str> = first.keys().map(String::as_str).collect();
    if canonical {
        columns.sort_unstable();
    }

    let uniform = array.iter().all(|item| {
        let Some(object) = item.as_object() else {
            return false;
        };
        object.len() == columns.len()
            && columns
                .iter()
                .all(|key| object.get(*key).is_some_and(is_scalar))
    });
    uniform.then_some(columns)
}

fn ordered_keys(object: &Map<String, Value>, canonical: bool) -> Vec<&str> {
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    if canonical {
        keys.sort_unstable();
    }
    keys
}

fn is_json_schema_document(value: &Value) -> bool {
    value
        .as_object()
        .and_then(|object| object.get("$schema"))
        .and_then(Value::as_str)
        .is_some_and(|schema| schema.contains("json-schema.org/"))
}

fn is_openapi_document(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object
        .get("openapi")
        .and_then(Value::as_str)
        .is_some_and(|version| version.starts_with("3."))
        && (object.contains_key("paths") || object.contains_key("components"))
}

fn render_schema_document(value: &Value, canonical: bool, out: &mut String) {
    let object = value.as_object().expect("JSON Schema root is an object");
    for key in ["$schema", "$id"] {
        if let Some(value) = object.get(key) {
            render_named(key, value, 0, canonical, JsonContext::Generic, out);
        }
    }
    render_schema_node("schema", object, false, 0, canonical, out);
}

fn render_schema_node(
    name: &str,
    schema: &Map<String, Value>,
    required: bool,
    indent: usize,
    canonical: bool,
    out: &mut String,
) {
    let ind = "  ".repeat(indent);
    out.push_str(&ind);
    out.push_str(&render_schema_label(name));
    if required {
        out.push('!');
    }
    if let Some(schema_type) = schema_type_inline(schema) {
        out.push_str(" : ");
        out.push_str(&schema_type);
    }
    out.push('\n');

    let required_names: std::collections::HashSet<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();

    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for property in ordered_keys(properties, canonical) {
            if let Some(property_schema) = properties[property].as_object() {
                render_schema_node(
                    property,
                    property_schema,
                    required_names.contains(property),
                    indent + 1,
                    canonical,
                    out,
                );
            } else {
                render_named(
                    property,
                    &properties[property],
                    indent + 1,
                    canonical,
                    JsonContext::Generic,
                    out,
                );
            }
        }
    }

    for definitions_key in ["$defs", "definitions"] {
        if let Some(definitions) = schema.get(definitions_key).and_then(Value::as_object) {
            let rendered_key = definitions_key.trim_start_matches('$');
            out.push_str(&format!("{}{rendered_key}\n", "  ".repeat(indent + 1)));
            for definition in ordered_keys(definitions, canonical) {
                if let Some(definition_schema) = definitions[definition].as_object() {
                    render_schema_node(
                        definition,
                        definition_schema,
                        false,
                        indent + 2,
                        canonical,
                        out,
                    );
                } else {
                    render_named(
                        definition,
                        &definitions[definition],
                        indent + 2,
                        canonical,
                        JsonContext::Generic,
                        out,
                    );
                }
            }
        }
    }

    for keyword in ["oneOf", "anyOf", "allOf"] {
        if let Some(schemas) = schema.get(keyword).and_then(Value::as_array) {
            for child in schemas {
                if let Some(child_schema) = child.as_object() {
                    render_schema_node(
                        &format!("{keyword}[]"),
                        child_schema,
                        false,
                        indent + 1,
                        canonical,
                        out,
                    );
                } else if is_scalar(child) {
                    let child_indent = "  ".repeat(indent + 1);
                    out.push_str(&format!(
                        "{child_indent}{keyword}[] = {}\n",
                        render_scalar(child, ScalarContext::Plain)
                    ));
                } else {
                    render_array(
                        Some(keyword),
                        std::slice::from_ref(child),
                        indent + 1,
                        canonical,
                        JsonContext::Generic,
                        out,
                    );
                }
            }
        }
    }

    for keyword in [
        "items",
        "additionalProperties",
        "contains",
        "not",
        "if",
        "then",
        "else",
        "propertyNames",
    ] {
        if keyword == "items" && should_inline_items(schema) {
            continue;
        }
        if let Some(child_schema) = schema.get(keyword).and_then(Value::as_object) {
            render_schema_node(keyword, child_schema, false, indent + 1, canonical, out);
        }
    }

    for key in ordered_keys(schema, canonical) {
        if is_consumed_schema_keyword(key, schema) {
            continue;
        }
        render_named(
            key,
            &schema[key],
            indent + 1,
            canonical,
            JsonContext::Generic,
            out,
        );
    }
}

fn render_schema_label(name: &str) -> String {
    if let Some(name) = name.strip_suffix("[]") {
        format!("{}[]", render_key(name))
    } else {
        render_key(name)
    }
}

fn schema_type_inline(schema: &Map<String, Value>) -> Option<String> {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        return Some(format!("ref {}", short_reference(reference)));
    }

    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        let values = values
            .iter()
            .map(|value| render_scalar(value, ScalarContext::Delimited))
            .collect::<Vec<_>>()
            .join(", ");
        return Some(format!("enum [{values}]"));
    }

    let mut schema_type = match schema.get("type") {
        Some(Value::String(value)) if value == "array" => {
            let item_type = schema
                .get("items")
                .and_then(Value::as_object)
                .and_then(schema_type_inline)
                .unwrap_or_else(|| "any".to_string());
            Some(format!("{item_type}[]"))
        }
        Some(Value::String(value)) => Some(value.clone()),
        Some(Value::Array(values)) => {
            let values = values
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" | ");
            (!values.is_empty()).then_some(values)
        }
        _ if schema.contains_key("properties") => Some("object".to_string()),
        _ if schema.contains_key("oneOf") => Some("oneOf".to_string()),
        _ if schema.contains_key("anyOf") => Some("anyOf".to_string()),
        _ if schema.contains_key("allOf") => Some("allOf".to_string()),
        _ => None,
    };

    if let Some(format) = schema.get("format").and_then(Value::as_str)
        && let Some(schema_type) = &mut schema_type
    {
        schema_type.push(' ');
        schema_type.push_str(format);
    }
    schema_type
}

fn short_reference(reference: &str) -> &str {
    reference
        .strip_prefix("#/components/schemas/")
        .or_else(|| reference.strip_prefix("#/$defs/"))
        .or_else(|| reference.strip_prefix("#/definitions/"))
        .unwrap_or(reference)
}

fn is_consumed_schema_keyword(key: &str, schema: &Map<String, Value>) -> bool {
    matches!(
        key,
        "$schema"
            | "$id"
            | "$ref"
            | "$defs"
            | "definitions"
            | "type"
            | "properties"
            | "required"
            | "format"
            | "enum"
            | "oneOf"
            | "anyOf"
            | "allOf"
    ) || key == "items" && schema.get("items").is_some_and(Value::is_object)
        || matches!(
            key,
            "additionalProperties" | "contains" | "not" | "if" | "then" | "else" | "propertyNames"
        ) && schema.get(key).is_some_and(Value::is_object)
}

fn should_inline_items(schema: &Map<String, Value>) -> bool {
    schema
        .get("items")
        .and_then(Value::as_object)
        .is_some_and(schema_is_inline_only)
}

fn schema_is_inline_only(schema: &Map<String, Value>) -> bool {
    schema
        .keys()
        .all(|key| matches!(key.as_str(), "$ref" | "type" | "format" | "enum" | "items"))
        && schema
            .get("items")
            .and_then(Value::as_object)
            .is_none_or(schema_is_inline_only)
}

fn is_scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

#[derive(Clone, Copy)]
enum ScalarContext {
    Plain,
    Delimited,
}

fn render_scalar(value: &Value, context: ScalarContext) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) if string_needs_quotes(value, context) => {
            serde_json::to_string(value).expect("serializing a string cannot fail")
        }
        Value::String(value) => value.to_owned(),
        _ => unreachable!("containers are not scalar values"),
    }
}

fn string_needs_quotes(value: &str, context: ScalarContext) -> bool {
    if value.is_empty()
        || value.trim() != value
        || value.chars().any(char::is_control)
        || value.contains(['"', '\\', '[', ']', '{', '}', '='])
        || matches!(context, ScalarContext::Delimited) && value.contains(',')
    {
        return true;
    }

    matches!(
        serde_json::from_str::<Value>(value),
        Ok(Value::Null | Value::Bool(_) | Value::Number(_))
    )
}

fn is_block_string(value: &str) -> bool {
    value.contains('\n') && value.chars().all(|c| c == '\n' || !c.is_control())
}

fn render_block_string(value: &str, indent: usize, out: &mut String) {
    let ind = "  ".repeat(indent);
    for line in value.split('\n') {
        out.push_str(&ind);
        out.push('|');
        if !line.is_empty() {
            out.push(' ');
            out.push_str(line);
        }
        out.push('\n');
    }
}

fn render_key(key: &str) -> String {
    let mut chars = key.chars();
    let plain = chars.next().is_some_and(|c| c == '_' || c.is_alphabetic())
        && chars.all(|c| c == '_' || c == '-' || c.is_alphanumeric());
    if plain {
        key.to_string()
    } else {
        serde_json::to_string(key).expect("serializing a string cannot fail")
    }
}

#[cfg(test)]
mod tests {
    use super::render_json;

    #[test]
    fn renders_uniform_objects_as_a_table() {
        let input = r#"{"users":[{"id":1,"name":"Ada"},{"name":"Lin","id":2}]}"#;
        assert_eq!(
            render_json(input, false, false).unwrap(),
            "users[]{id,name}\n  1, Ada\n  2, Lin\n"
        );
    }

    #[test]
    fn falls_back_for_nested_rows() {
        let input = r#"{"users":[{"id":1,"meta":{"active":true}},{"id":2,"meta":null}]}"#;
        assert_eq!(
            render_json(input, false, false).unwrap(),
            concat!(
                "users[]\n",
                "  id = 1\n",
                "  meta\n",
                "    active = true\n",
                "users[]\n",
                "  id = 2\n",
                "  meta = null\n",
            )
        );
    }

    #[test]
    fn canonical_sorts_object_keys_and_table_columns() {
        let input = r#"{"z":0,"rows":[{"b":2,"a":1},{"a":3,"b":4}],"a":9}"#;
        assert_eq!(
            render_json(input, true, false).unwrap(),
            concat!(
                "a = 9\n",
                "rows[]{a,b}\n",
                "  1, 2\n",
                "  3, 4\n",
                "z = 0\n",
            )
        );
    }

    #[test]
    fn renders_root_arrays_and_quoted_keys() {
        assert_eq!(
            render_json(r#"[{"first name":"Ada","active":true}]"#, false, false).unwrap(),
            concat!("[]\n", "  \"first name\" = Ada\n", "  active = true\n")
        );
        assert_eq!(
            render_json(r#"["null",null,-1.5e2,true]"#, false, false).unwrap(),
            "[\"null\", null, -150.0, true]\n"
        );
    }

    #[test]
    fn tables_escape_strings_and_require_two_rows() {
        assert_eq!(
            render_json(
                r#"{"rows":[{"text":"comma, quote: \""},{"text":"line\nbreak"}]}"#,
                false,
                false,
            )
            .unwrap(),
            concat!(
                "rows[]{text}\n",
                "  \"comma, quote: \\\"\"\n",
                "  \"line\\nbreak\"\n",
            )
        );
        assert_eq!(
            render_json(r#"{"rows":[{"id":1}]}"#, false, false).unwrap(),
            "rows[]\n  id = 1\n"
        );
    }

    #[test]
    fn distinguishes_empty_and_mixed_containers() {
        let input = r#"{"emptyObject":{},"emptyArray":[],"mixed":[1,{"x":2},[3,4]]}"#;
        assert_eq!(
            render_json(input, false, false).unwrap(),
            concat!(
                "emptyObject = {}\n",
                "emptyArray = []\n",
                "mixed[] = 1\n",
                "mixed[]\n",
                "  x = 2\n",
                "mixed[]\n",
                "  [3, 4]\n",
            )
        );
    }

    #[test]
    fn quotes_only_strings_that_would_be_ambiguous() {
        let input = r##"{
            "name":"Ada Lovelace",
            "url":"https://example.test/a",
            "version":"3.0.4",
            "comma":"data, search",
            "empty":"",
            "bool":"true",
            "number":"123",
            "structural":"[value]"
        }"##;
        assert_eq!(
            render_json(input, false, false).unwrap(),
            concat!(
                "name = Ada Lovelace\n",
                "url = https://example.test/a\n",
                "version = 3.0.4\n",
                "comma = data, search\n",
                "empty = \"\"\n",
                "bool = \"true\"\n",
                "number = \"123\"\n",
                "structural = \"[value]\"\n",
            )
        );
    }

    #[test]
    fn renders_named_multiline_strings_as_blocks() {
        let input = r#"{"description":"first\n\nthird","mixed":["one\nline",{"x":1}]}"#;
        assert_eq!(
            render_json(input, false, false).unwrap(),
            concat!(
                "description =\n",
                "  | first\n",
                "  |\n",
                "  | third\n",
                "mixed[] =\n",
                "  | one\n",
                "  | line\n",
                "mixed[]\n",
                "  x = 1\n",
            )
        );
    }

    #[test]
    fn keeps_multiline_strings_escaped_in_delimited_values() {
        assert_eq!(
            render_json(r#"["one\nline","two"]"#, false, false).unwrap(),
            "[\"one\\nline\", two]\n"
        );
        assert_eq!(
            render_json(r#""one\n\nthree""#, false, false).unwrap(),
            "=\n  | one\n  |\n  | three\n"
        );
        assert_eq!(
            render_json(r#"{"text":"one\n\ttwo"}"#, false, false).unwrap(),
            "text = \"one\\n\\ttwo\"\n"
        );
    }

    #[test]
    fn auto_simplifies_a_standalone_json_schema() {
        // Representative subset of dtolator's test-zod-all-features.json.
        let input = r#"{
          "$schema":"http://json-schema.org/draft-07/schema#",
          "type":"object",
          "title":"ComprehensiveZodTest",
          "properties":{
            "basicString":{"type":"string","description":"Basic string field"},
            "uuid":{"type":"string","format":"uuid"},
            "stringArray":{"type":"array","items":{"type":"string"}},
            "objectArray":{"type":"array","items":{
              "type":"object",
              "properties":{"id":{"type":"integer"},"name":{"type":"string"}},
              "required":["id"]
            }},
            "enumField":{"type":"string","enum":["active","inactive"]},
            "unionField":{"oneOf":[{"type":"string"},{"type":"number"}]},
            "custom":{"type":"string","x-extra":{"kept":true}}
          },
          "required":["basicString","uuid","stringArray","objectArray"]
        }"#;
        assert_eq!(
            render_json(input, false, true).unwrap(),
            concat!(
                "\"$schema\" = http://json-schema.org/draft-07/schema#\n",
                "schema : object\n",
                "  basicString! : string\n",
                "    description = Basic string field\n",
                "  uuid! : string uuid\n",
                "  stringArray! : string[]\n",
                "  objectArray! : object[]\n",
                "    items : object\n",
                "      id! : integer\n",
                "      name : string\n",
                "  enumField : enum [active, inactive]\n",
                "  unionField : oneOf\n",
                "    oneOf[] : string\n",
                "    oneOf[] : number\n",
                "  custom : string\n",
                "    x-extra\n",
                "      kept = true\n",
                "  title = ComprehensiveZodTest\n",
            )
        );
    }

    #[test]
    fn auto_simplifies_only_known_openapi_schema_locations() {
        // Reduced from dtolator's openapi/simple-sample.json.
        let input = r##"{
          "openapi":"3.0.3",
          "info":{"title":"Sample API","version":"1.0.0"},
          "paths":{"/users":{"get":{"responses":{"200":{"content":{
            "application/json":{"schema":{"type":"array","items":{
              "$ref":"#/components/schemas/User"
            }}}
          }}}}}},
          "components":{"schemas":{
            "User":{"type":"object","properties":{
              "id":{"type":"integer","format":"int64"},
              "email":{"type":"string","format":"email"}
            },"required":["id","email"]}
          }},
          "unrelated":{"type":"object","properties":{"leave":"generic"}}
        }"##;
        let output = render_json(input, false, true).unwrap();
        assert!(output.contains("schema : ref User[]\n"));
        assert!(output.contains("schemas\n    User : object\n"));
        assert!(output.contains("      id! : integer int64\n"));
        assert!(output.contains("      email! : string email\n"));
        assert!(output.contains("unrelated\n  type = object\n  properties\n    leave = generic\n"));
    }

    #[test]
    fn schema_simplification_requires_auto_and_a_strong_root_signal() {
        let schema =
            r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"string"}"#;
        assert_eq!(
            render_json(schema, false, false).unwrap(),
            "\"$schema\" = https://json-schema.org/draft/2020-12/schema\ntype = string\n"
        );

        let ordinary = r#"{"type":"object","properties":{"name":"Ada"}}"#;
        assert_eq!(
            render_json(ordinary, false, true).unwrap(),
            "type = object\nproperties\n  name = Ada\n"
        );
    }
}
