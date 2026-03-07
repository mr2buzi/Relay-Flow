use crate::models::RunContext;
use serde_json::{Map, Value};

pub fn render_json(value: &Value, context: &RunContext) -> Value {
    match value {
        Value::String(text) => Value::String(render_string(text, context)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| render_json(item, context))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), render_json(value, context)))
                .collect::<Map<String, Value>>(),
        ),
        other => other.clone(),
    }
}

pub fn render_string(template: &str, context: &RunContext) -> String {
    let mut rendered = template.to_string();
    while let Some(start) = rendered.find("{{") {
        let Some(end) = rendered[start + 2..].find("}}") else {
            break;
        };
        let end = start + 2 + end;
        let token = rendered[start + 2..end].trim();
        let replacement = resolve_reference(token, context).unwrap_or(Value::Null);
        let replacement = match replacement {
            Value::String(text) => text,
            Value::Null => String::new(),
            other => other.to_string(),
        };
        rendered.replace_range(start..end + 2, &replacement);
    }
    rendered
}

pub fn resolve_reference(token: &str, context: &RunContext) -> Option<Value> {
    if let Some(path) = token.strip_prefix("input.") {
        lookup_path(&context.input, path)
    } else if token == "input" {
        Some(context.input.clone())
    } else if let Some(path) = token.strip_prefix("steps.") {
        let mut parts = path.splitn(2, '.');
        let index = parts.next()?.parse::<usize>().ok()?;
        let remainder = parts.next().unwrap_or("output");
        let step = context.steps.get(index)?;
        if remainder == "output" {
            Some(step.output.clone())
        } else if let Some(output_path) = remainder.strip_prefix("output.") {
            lookup_path(&step.output, output_path)
        } else {
            None
        }
    } else {
        None
    }
}

pub fn lookup_path(value: &Value, path: &str) -> Option<Value> {
    let mut current = value;
    for segment in path.split('.') {
        match current {
            Value::Object(map) => current = map.get(segment)?,
            Value::Array(items) => {
                let index = segment.parse::<usize>().ok()?;
                current = items.get(index)?;
            }
            _ => return None,
        }
    }
    Some(current.clone())
}

#[cfg(test)]
mod tests {
    use super::{lookup_path, resolve_reference};
    use crate::models::{RunContext, StepContext};
    use chrono::Utc;
    use serde_json::json;

    #[test]
    fn resolves_input_and_step_references() {
        let context = RunContext {
            input: json!({
                "user": {
                    "plan": "pro"
                }
            }),
            steps: vec![StepContext {
                name: "create-customer".to_string(),
                output: json!({
                    "customer_id": "cus_123",
                    "nested": {
                        "region": "eu"
                    }
                }),
                finished_at: Utc::now(),
            }],
            execution_plan: Vec::new(),
            branch_decisions: Vec::new(),
        };

        assert_eq!(
            resolve_reference("input.user.plan", &context),
            Some(json!("pro"))
        );
        assert_eq!(
            resolve_reference("steps.0.output.customer_id", &context),
            Some(json!("cus_123"))
        );
        assert_eq!(
            resolve_reference("steps.0.output.nested.region", &context),
            Some(json!("eu"))
        );
    }

    #[test]
    fn lookup_path_handles_arrays() {
        let value = json!({
            "items": [
                {"kind": "first"},
                {"kind": "second"}
            ]
        });

        assert_eq!(lookup_path(&value, "items.1.kind"), Some(json!("second")));
        assert_eq!(lookup_path(&value, "items.2.kind"), None);
    }
}
