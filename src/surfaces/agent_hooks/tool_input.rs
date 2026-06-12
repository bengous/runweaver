use serde_json::Value;

pub fn touched_path_candidates(tool_input: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut paths = Vec::new();
    for key in ["file_path", "filePath", "path"] {
        if let Some(Value::String(value)) = tool_input.get(key) {
            paths.push(value.clone());
        }
    }
    if let Some(Value::Array(edits)) = tool_input.get("edits") {
        for edit in edits {
            if let Value::Object(record) = edit {
                for key in ["file_path", "filePath", "path"] {
                    if let Some(Value::String(value)) = record.get(key) {
                        paths.push(value.clone());
                        break;
                    }
                }
            }
        }
    }
    paths
}
