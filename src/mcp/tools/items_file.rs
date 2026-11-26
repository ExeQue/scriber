//! add_items `path`: big batches come from a JSON file instead of being inlined in the call.

use super::NewItem;
use serde_json::Value;
use std::io::Read;

/// Largest items file add_items reads.
pub const MAX_BYTES: u64 = 16 * 1024 * 1024;

/// The items in a JSON file: the same array as add_items `items`, or `{"items": [...]}`.
pub fn load(path: &str) -> Result<Vec<NewItem>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let meta = file.metadata().map_err(|e| format!("cannot read {path}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{path} is not a regular file"));
    }
    if meta.len() > MAX_BYTES {
        return Err(format!("{path} is {} bytes; the limit is {MAX_BYTES}", meta.len()));
    }
    let mut text = String::new();
    file.take(MAX_BYTES + 1).read_to_string(&mut text).map_err(|e| format!("cannot read {path}: {e}"))?;
    let json: Value = serde_json::from_str(&text).map_err(|e| format!("{path} is not JSON: {e}"))?;
    let list = match json {
        Value::Array(list) => list,
        Value::Object(mut o) => match o.remove("items") {
            Some(Value::Array(list)) => list,
            _ => return Err(format!("{path}: expected an array of items or {{\"items\": [...]}}")),
        },
        _ => return Err(format!("{path}: expected an array of items or {{\"items\": [...]}}")),
    };
    list.into_iter().enumerate().map(|(k, v)| serde_json::from_value(v).map_err(|e| format!("{path}: item {k}: {e}"))).collect()
}
