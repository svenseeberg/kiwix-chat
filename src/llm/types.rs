use serde::{Deserialize, Serialize};

/// Role of a chat message in the OpenAI chat-completions schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A single chat message. Shapes both request and (assembled) response messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Present on assistant messages that request tool invocations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// Present on `tool` role messages: the id of the call being answered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self::text(Role::System, content)
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self::text(Role::User, content)
    }
    #[allow(dead_code)] // provided for symmetry / future use
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::text(Role::Assistant, content)
    }

    fn text(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Build a `tool` role message answering a specific tool call.
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

/// A tool/function the model may call.
#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub kind: &'static str, // always "function"
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl Tool {
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            kind: "function",
            function: FunctionDef {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

/// A concrete tool invocation requested by the assistant.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    /// Raw JSON arguments as a string (may arrive in fragments while streaming).
    pub arguments: String,
}
