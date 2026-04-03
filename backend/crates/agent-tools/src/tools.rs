use serde::Serialize;

/// Definition of a tool available to agents.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Get all tools available to agents.
pub fn available_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "create_schedule".into(),
            description: "Create a recurring scheduled task. The task message will be sent to you on the specified cron schedule.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "cron_expr": {
                        "type": "string",
                        "description": "Cron expression (5-field: min hour dom month dow). Example: '0 */3 * * *' for every 3 hours"
                    },
                    "task_message": {
                        "type": "string",
                        "description": "The message/task to execute on schedule"
                    }
                },
                "required": ["cron_expr", "task_message"]
            }),
        },
        ToolDefinition {
            name: "list_schedules".into(),
            description: "List all your active scheduled tasks.".into(),
            parameters: serde_json::json!({ "type": "object", "properties": {} }),
        },
        ToolDefinition {
            name: "delete_schedule".into(),
            description: "Delete a scheduled task by ID.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "schedule_id": {
                        "type": "string",
                        "description": "UUID of the schedule to delete"
                    }
                },
                "required": ["schedule_id"]
            }),
        },
        ToolDefinition {
            name: "send_notification".into(),
            description: "Send a notification to the user (visible in their dashboard).".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Notification title"
                    },
                    "body": {
                        "type": "string",
                        "description": "Notification body text"
                    }
                },
                "required": ["title", "body"]
            }),
        },
    ]
}

/// Generate the tools configuration JSON for an OpenClaw agent.
///
/// This is mounted into the agent container at a known path and tells
/// OpenClaw about the available tool functions.
pub fn generate_tool_config(
    backend_url: &str,
    agent_id: &str,
    user_id: &str,
) -> serde_json::Value {
    serde_json::json!({
        "tools": available_tools().iter().map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "parameters": t.parameters,
                "endpoint": format!("{}/internal/{}", backend_url,
                    match t.name.as_str() {
                        "create_schedule" | "list_schedules" | "delete_schedule" => "schedules",
                        "send_notification" => "notifications",
                        _ => "unknown"
                    }
                ),
                "method": match t.name.as_str() {
                    "create_schedule" | "send_notification" => "POST",
                    "list_schedules" => "GET",
                    "delete_schedule" => "DELETE",
                    _ => "POST"
                },
                "headers": {
                    "X-Agent-Id": agent_id,
                    "X-User-Id": user_id
                }
            })
        }).collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_tools_count() {
        assert_eq!(available_tools().len(), 4);
    }

    #[test]
    fn test_generate_tool_config_structure() {
        let config = generate_tool_config("http://backend:8080", "agent-1", "user-1");
        let tools = config["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 4);

        let first = &tools[0];
        assert_eq!(first["name"], "create_schedule");
        assert_eq!(first["method"], "POST");
        assert_eq!(first["endpoint"], "http://backend:8080/internal/schedules");
        assert_eq!(first["headers"]["X-Agent-Id"], "agent-1");
        assert_eq!(first["headers"]["X-User-Id"], "user-1");
    }
}
