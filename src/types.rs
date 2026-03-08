use serde::{Deserialize, Serialize};

// Streaming events

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    TextStart { text: String },
    #[serde(rename = "thinking")]
    ThinkingStart { thinking: String },
    #[serde(rename = "tool_use")]
    ToolUseStart { id: String, name: String },
}

#[derive(Deserialize)]
pub struct ContentBlockStart {
    pub index: u32,
    pub content_block: ContentBlock,
}

#[derive(Deserialize)]
pub struct ContentBlockDelta {
    pub index: u32,
    pub delta: Delta,
}

#[derive(Deserialize)]
pub struct ContentBlockStop {
    pub index: u32,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum Delta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
    #[serde(rename = "signature_delta")]
    SignatureDelta { signature: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
}

// Tools

pub struct ToolCall {
    pub id: String,
    pub name: String,
    // accumulates the arguments as a json string
    pub args_json: String,
    pub args_parsed: Option<ToolArgs>,
}

impl ToolCall {
    pub fn new(id: String, name: String) -> ToolCall {
        ToolCall {
            id,
            name,
            args_json: String::new(),
            args_parsed: None,
        }
    }
}

pub struct TextMessage {
    pub text: String,
}

pub struct ThinkingMessage {
    pub thinking: String,
    pub signature: String,
}

#[derive(Clone, Deserialize)]
pub struct BashToolArgs {
    pub command: String,
}

#[derive(Clone, Deserialize)]
pub struct WriteFileToolArgs {
    pub file_path: String,
    pub content: String,
}

#[derive(Clone, Deserialize)]
pub struct EditFileToolArgs {
    pub file_path: String,
    pub old_string: String,
    pub new_string: String,
}

#[derive(Clone)]
pub enum ToolArgs {
    Bash(BashToolArgs),
    WriteFile(WriteFileToolArgs),
    EditFile(EditFileToolArgs),
}

// Messages

#[derive(Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    User { content: Vec<UserContent> },
    Assistant { content: Vec<AssistantContent> },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserContent {
    Text {
        text: String,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssistantContent {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    InvalidToolUse {
        id: String,
        name: String,
        input: String,
    },
}

// Permissions
pub enum AgentPermissions {
    AllowAll,
    ConfirmAll,
}
