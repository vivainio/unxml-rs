//! Compact, diff-friendly JSON rendering.
//!
//! Objects use the same indentation and `key = value` vocabulary as generic
//! XML output. Uniform arrays of scalar objects use a TOON-inspired tabular
//! form so repeated field names are written only once.

use anyhow::{Context, Result};
use serde_json::{Map, Value};

pub(crate) fn render_json(content: &str, canonical: bool) -> Result<String> {
    let value: Value = serde_json::from_str(content).context("Failed to parse JSON")?;
    let mut out = String::new();
    render_root(&value, canonical, &mut out);
    Ok(out)
}

fn render_root(value: &Value, canonical: bool, out: &mut String) {
    match value {
        Value::Object(object) => render_object(object, 0, canonical, out),
        Value::Array(array) => render_array(None, array, 0, canonical, out),
        scalar => {
            out.push_str("= ");
            out.push_str(&render_scalar(scalar));
            out.push('\n');
        }
    }
}

fn render_object(object: &Map<String, Value>, indent: usize, canonical: bool, out: &mut String) {
    for key in ordered_keys(object, canonical) {
        render_named(key, &object[key], indent, canonical, out);
    }
}

fn render_named(key: &str, value: &Value, indent: usize, canonical: bool, out: &mut String) {
    let ind = "  ".repeat(indent);
    let key = render_key(key);
    match value {
        scalar if is_scalar(scalar) => {
            out.push_str(&format!("{ind}{key} = {}\n", render_scalar(scalar)));
        }
        Value::Object(object) if object.is_empty() => {
            out.push_str(&format!("{ind}{key} = {{}}\n"));
        }
        Value::Object(object) => {
            out.push_str(&format!("{ind}{key}\n"));
            render_object(object, indent + 1, canonical, out);
        }
        Value::Array(array) => render_array(Some(&key), array, indent, canonical, out),
        _ => unreachable!(),
    }
}

fn render_array(
    key: Option<&str>,
    array: &[Value],
    indent: usize,
    canonical: bool,
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
            .map(render_scalar)
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
                .map(|column| render_scalar(&object[*column]))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("{row_indent}{row}\n"));
        }
        return;
    }

    for item in array {
        match item {
            scalar if is_scalar(scalar) => {
                out.push_str(&format!("{ind}{name}[] = {}\n", render_scalar(scalar)));
            }
            Value::Object(object) if object.is_empty() => {
                out.push_str(&format!("{ind}{name}[] = {{}}\n"));
            }
            Value::Object(object) => {
                out.push_str(&format!("{ind}{name}[]\n"));
                render_object(object, indent + 1, canonical, out);
            }
            Value::Array(nested) => {
                out.push_str(&format!("{ind}{name}[]\n"));
                render_array(None, nested, indent + 1, canonical, out);
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

fn is_scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

fn render_scalar(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => {
            serde_json::to_string(value).expect("serializing a string cannot fail")
        }
        _ => unreachable!("containers are not scalar values"),
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
            render_json(input, false).unwrap(),
            "users[]{id,name}\n  1, \"Ada\"\n  2, \"Lin\"\n"
        );
    }

    #[test]
    fn falls_back_for_nested_rows() {
        let input = r#"{"users":[{"id":1,"meta":{"active":true}},{"id":2,"meta":null}]}"#;
        assert_eq!(
            render_json(input, false).unwrap(),
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
            render_json(input, true).unwrap(),
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
            render_json(r#"[{"first name":"Ada","active":true}]"#, false).unwrap(),
            concat!("[]\n", "  \"first name\" = \"Ada\"\n", "  active = true\n")
        );
        assert_eq!(
            render_json(r#"["null",null,-1.5e2,true]"#, false).unwrap(),
            "[\"null\", null, -150.0, true]\n"
        );
    }

    #[test]
    fn tables_escape_strings_and_require_two_rows() {
        assert_eq!(
            render_json(
                r#"{"rows":[{"text":"comma, quote: \""},{"text":"line\nbreak"}]}"#,
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
            render_json(r#"{"rows":[{"id":1}]}"#, false).unwrap(),
            "rows[]\n  id = 1\n"
        );
    }

    #[test]
    fn distinguishes_empty_and_mixed_containers() {
        let input = r#"{"emptyObject":{},"emptyArray":[],"mixed":[1,{"x":2},[3,4]]}"#;
        assert_eq!(
            render_json(input, false).unwrap(),
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
}
