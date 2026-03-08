use crate::streaming::{TextStreamer, ToolArgStreamer, smooth_printer};
use crate::types::{
    AgentPermissions, AssistantContent, ContentBlock, ContentBlockDelta, ContentBlockStart,
    ContentBlockStop, Delta, EditFileToolArgs, Message, ReadFileToolArgs, TextMessage,
    ThinkingMessage, ToolArgs, ToolCall, UserContent, WriteFileToolArgs,
};
use futures_util::StreamExt;
use reqwest::Client;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest_eventsource::{Event, EventSource};
use serde_json::json;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::Duration;

pub struct Agent {
    model: String,
    messages: Vec<Message>,
    api_key: String,
    permissions: AgentPermissions,
}

impl Agent {
    pub fn new(
        model: String,
        messages: Vec<Message>,
        api_key: String,
        permissions: AgentPermissions,
    ) -> Agent {
        Agent {
            model,
            messages,
            api_key,
            permissions,
        }
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let api_key_header =
            HeaderValue::from_str(&self.api_key).expect("ANTHROPIC_API_KEY is not a valid value");

        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("x-api-key", api_key_header);
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

        headers
    }

    pub async fn run(&mut self, messages: Vec<Message>) {
        let system_prompt: &str = "You are Booster, a helpful command line assistant. You help people accomplish programming tasks and learn new things.";
        let client = Client::new();

        self.messages.extend(messages);
        let mut num_processed: usize = 0;

        while num_processed < self.messages.len() {
            num_processed = self.messages.len();
            let body = json!({
                "model":self.model,
                "max_tokens": 16384,
                "thinking": {
                    "type": "adaptive"
                },
                "output_config": {
                    "effort": "low"
                },
                "tools": [
                    {
                        "type": "bash_20250124",
                        "name": "bash"
                    },
                    {
                        "name": "write_file",
                        "description": "Write content to a file. Creates the file if it doesn't exist, or overwrites it if it does.",
                        "input_schema": {
                            "type": "object",
                            "properties": {
                                "file_path": {
                                    "type": "string",
                                    "description": "The path to the file to write"
                                },
                                "content": {
                                    "type": "string",
                                    "description": "The content to write to the file"
                                }
                            },
                            "required": ["file_path", "content"]
                        }
                    },
                    {
                        "name": "edit_file",
                        "description": "Edit a file by replacing an exact string match with a new string. The old_string must appear exactly once in the file.",
                        "input_schema": {
                            "type": "object",
                            "properties": {
                                "file_path": {
                                    "type": "string",
                                    "description": "The path to the file to edit"
                                },
                                "old_string": {
                                    "type": "string",
                                    "description": "The exact string to find and replace"
                                },
                                "new_string": {
                                    "type": "string",
                                    "description": "The string to replace it with"
                                }
                            },
                            "required": ["file_path", "old_string", "new_string"]
                        }
                    },
                    {
                        "name": "read_file",
                        "description": "Read the contents of a file and return them.",
                        "input_schema": {
                            "type": "object",
                            "properties": {
                                "file_path": {
                                    "type": "string",
                                    "description": "The path to the file to read"
                                }
                            },
                            "required": ["file_path"]
                        }
                    }
                ],
                "stream": true,
                "system": system_prompt,
                "messages": &self.messages
            });

            let request = client
                .post("https://api.anthropic.com/v1/messages")
                .headers(self.headers())
                .json(&body);

            let mut event_source = match EventSource::new(request) {
                Ok(es) => es,
                Err(err) => panic!("{}", err),
            };

            let mut tool_calls: HashMap<u32, ToolCall> = HashMap::new();
            let mut text_messages: HashMap<u32, TextMessage> = HashMap::new();
            let mut thinking_messages: HashMap<u32, ThinkingMessage> = HashMap::new();
            let mut tool_streamers: HashMap<u32, ToolArgStreamer> = HashMap::new();
            let mut text_streamers: HashMap<u32, TextStreamer> = HashMap::new();
            let mut printer_handles: HashMap<u32, tokio::task::JoinHandle<()>> = HashMap::new();
            let mut assistant_content: Vec<AssistantContent> = vec![];
            let mut tool_results: Vec<UserContent> = vec![];

            while let Some(event) = event_source.next().await {
                match event {
                    Ok(Event::Open) => {}
                    Ok(Event::Message(msg)) => {
                        if msg.event == String::from("content_block_start") {
                            let data: ContentBlockStart = serde_json::from_str(&msg.data).unwrap();
                            match data.content_block {
                                ContentBlock::TextStart { text } => {
                                    let (mut streamer, rx) = TextStreamer::new();
                                    streamer.label("response");
                                    if !text.is_empty() {
                                        streamer.feed(&text);
                                    }
                                    text_streamers.insert(data.index, streamer);
                                    let handle = tokio::spawn(smooth_printer(
                                        rx,
                                        Duration::from_millis(20),
                                    ));
                                    printer_handles.insert(data.index, handle);
                                    text_messages.insert(data.index, TextMessage { text });
                                }
                                ContentBlock::ThinkingStart { thinking } => {
                                    let (mut streamer, rx) = TextStreamer::new();
                                    streamer.label("thinking");
                                    if !thinking.is_empty() {
                                        streamer.feed(&thinking);
                                    }
                                    text_streamers.insert(data.index, streamer);
                                    let handle = tokio::spawn(smooth_printer(
                                        rx,
                                        Duration::from_millis(20),
                                    ));
                                    printer_handles.insert(data.index, handle);
                                    thinking_messages.insert(
                                        data.index,
                                        ThinkingMessage {
                                            thinking,
                                            signature: String::new(),
                                        },
                                    );
                                }
                                ContentBlock::ToolUseStart { id, name } => {
                                    let display_keys = match name.as_str() {
                                        "bash" => vec!["command"],
                                        "write_file" => vec!["file_path", "content"],
                                        "edit_file" => {
                                            vec!["file_path", "old_string", "new_string"]
                                        }
                                        "read_file" => vec!["file_path"],
                                        _ => vec![],
                                    };
                                    let (streamer, rx) = ToolArgStreamer::new(display_keys);
                                    tool_streamers.insert(data.index, streamer);
                                    let handle = tokio::spawn(smooth_printer(
                                        rx,
                                        Duration::from_millis(20),
                                    ));
                                    printer_handles.insert(data.index, handle);
                                    tool_calls.insert(data.index, ToolCall::new(id, name));
                                }
                            }
                        } else if msg.event == String::from("content_block_delta") {
                            let data: ContentBlockDelta = serde_json::from_str(&msg.data).unwrap();
                            match data.delta {
                                Delta::TextDelta { text } => {
                                    let cur_text_message =
                                        text_messages.get_mut(&data.index).unwrap();
                                    cur_text_message.text.push_str(&text);
                                    if let Some(streamer) = text_streamers.get_mut(&data.index) {
                                        streamer.feed(&text);
                                    }
                                }
                                Delta::ThinkingDelta { thinking } => {
                                    let cur_thinking =
                                        thinking_messages.get_mut(&data.index).unwrap();
                                    cur_thinking.thinking.push_str(&thinking);
                                    if let Some(streamer) = text_streamers.get_mut(&data.index) {
                                        streamer.feed(&thinking);
                                    }
                                }
                                Delta::SignatureDelta { signature } => {
                                    if let Some(msg) = thinking_messages.get_mut(&data.index) {
                                        msg.signature.push_str(&signature);
                                    }
                                }
                                Delta::InputJsonDelta { partial_json } => {
                                    let cur_tool_call = tool_calls.get_mut(&data.index).unwrap();
                                    cur_tool_call.args_json.push_str(&partial_json);
                                    if let Some(streamer) = tool_streamers.get_mut(&data.index) {
                                        streamer.feed(&partial_json);
                                    }
                                }
                            }
                        } else if msg.event == String::from("content_block_stop") {
                            let data: ContentBlockStop = serde_json::from_str(&msg.data).unwrap();

                            // Finish any streamer and wait for its printer
                            if let Some(mut streamer) = text_streamers.remove(&data.index) {
                                streamer.finish();
                            }
                            if let Some(mut streamer) = tool_streamers.remove(&data.index) {
                                streamer.finish();
                            }
                            if let Some(handle) = printer_handles.remove(&data.index) {
                                let _ = handle.await;
                            }

                            if text_messages.contains_key(&data.index) {
                                let text = text_messages.get(&data.index).unwrap().text.clone();
                                assistant_content.push(AssistantContent::Text { text });
                            } else if thinking_messages.contains_key(&data.index) {
                                let msg = thinking_messages.get(&data.index).unwrap();
                                assistant_content.push(AssistantContent::Thinking {
                                    thinking: msg.thinking.clone(),
                                    signature: msg.signature.clone(),
                                });
                            } else if tool_calls.contains_key(&data.index) {
                                let tool_call = tool_calls.get_mut(&data.index).unwrap();

                                tool_call.args_parsed = self.parse_tool_args(tool_call);

                                match &tool_call.args_parsed {
                                    Some(args) => {
                                        let input: serde_json::Value =
                                            serde_json::from_str(&tool_call.args_json)
                                                .unwrap_or(serde_json::Value::Null);
                                        assistant_content.push(AssistantContent::ToolUse {
                                            id: tool_call.id.clone(),
                                            name: tool_call.name.clone(),
                                            input,
                                        });
                                        let result = match self.permissions {
                                            AgentPermissions::ConfirmAll => {
                                                self.request_confirmation(tool_call, args).await
                                            }
                                            AgentPermissions::AllowAll => {
                                                self.execute_tool(args).await
                                            }
                                        };
                                        tool_results.push(UserContent::ToolResult {
                                            tool_use_id: tool_call.id.clone(),
                                            content: result,
                                        });
                                    }
                                    None => {
                                        assistant_content.push(AssistantContent::InvalidToolUse {
                                            id: tool_call.id.clone(),
                                            name: tool_call.name.clone(),
                                            input: tool_call.args_json.clone(),
                                        });
                                        tool_results.push(UserContent::ToolResult {
                                            tool_use_id: tool_call.id.clone(),
                                            content: String::from("Invalid tool arguments."),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Err(_err) => {
                        event_source.close();
                    }
                }
            }

            if !assistant_content.is_empty() {
                self.messages.push(Message::Assistant {
                    content: assistant_content,
                });
            }
            if !tool_results.is_empty() {
                self.messages.push(Message::User {
                    content: tool_results,
                });
            }
            if tool_calls.len() == 0 {
                break;
            }
        }
    }

    fn parse_tool_args(&self, tool_call: &ToolCall) -> Option<ToolArgs> {
        match tool_call.name.as_str() {
            "bash" => serde_json::from_str(&tool_call.args_json)
                .ok()
                .map(ToolArgs::Bash),
            "write_file" => serde_json::from_str(&tool_call.args_json)
                .ok()
                .map(ToolArgs::WriteFile),
            "edit_file" => serde_json::from_str(&tool_call.args_json)
                .ok()
                .map(ToolArgs::EditFile),
            "read_file" => serde_json::from_str(&tool_call.args_json)
                .ok()
                .map(ToolArgs::ReadFile),
            _ => None,
        }
    }

    async fn execute_tool(&self, args: &ToolArgs) -> String {
        match args {
            ToolArgs::Bash(bash_args) => self.execute_command(&bash_args.command).await,
            ToolArgs::WriteFile(write_args) => self.execute_write_file(write_args),
            ToolArgs::EditFile(edit_args) => self.execute_edit_file(edit_args),
            ToolArgs::ReadFile(read_args) => self.execute_read_file(read_args),
        }
    }

    fn execute_write_file(&self, args: &WriteFileToolArgs) -> String {
        if let Some(parent) = std::path::Path::new(&args.file_path).parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return format!("Error creating directories: {}", e);
            }
        }
        match std::fs::write(&args.file_path, &args.content) {
            Ok(_) => format!("Successfully wrote to {}", args.file_path),
            Err(e) => format!("Error writing file: {}", e),
        }
    }

    fn execute_edit_file(&self, args: &EditFileToolArgs) -> String {
        let content = match std::fs::read_to_string(&args.file_path) {
            Ok(c) => c,
            Err(e) => return format!("Error reading file: {}", e),
        };

        let count = content.matches(&args.old_string).count();
        if count == 0 {
            return String::from("Error: old_string not found in file.");
        }
        if count > 1 {
            return format!(
                "Error: old_string found {} times in file. It must appear exactly once.",
                count
            );
        }

        let new_content = content.replacen(&args.old_string, &args.new_string, 1);
        match std::fs::write(&args.file_path, new_content) {
            Ok(_) => format!("Successfully edited {}", args.file_path),
            Err(e) => format!("Error writing file: {}", e),
        }
    }

    fn execute_read_file(&self, args: &ReadFileToolArgs) -> String {
        const MAX_FILE_SIZE: usize = 100_000;
        match std::fs::read_to_string(&args.file_path) {
            Ok(content) => {
                if content.len() > MAX_FILE_SIZE {
                    format!(
                        "Error: file is too large ({} characters, max {}). Use the bash tool to read a smaller section with head, tail, or sed.",
                        content.len(),
                        MAX_FILE_SIZE
                    )
                } else {
                    content
                }
            }
            Err(e) => format!("Error reading file: {}", e),
        }
    }

    async fn execute_command(&self, command: &str) -> String {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to execute command");

        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let stderr = child.stderr.take().expect("Failed to capture stderr");
        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        let mut command_output = String::new();

        loop {
            tokio::select! {
                line = stdout_reader.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            println!("{}", line);
                            command_output.push_str(&line);
                            command_output.push('\n');
                        }
                        Ok(None) => break,
                        Err(e) => {
                            eprintln!("Error reading stdout: {}", e);
                            break;
                        }
                    }
                }
                line = stderr_reader.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            eprintln!("{}", line);
                            command_output.push_str(&line);
                            command_output.push('\n');
                        }
                        Ok(None) => {}
                        Err(e) => {
                            eprintln!("Error reading stderr: {}", e);
                        }
                    }
                }
            }
        }

        return command_output;
    }

    async fn request_confirmation(&self, _tool_call: &ToolCall, args: &ToolArgs) -> String {
        let description = match args {
            ToolArgs::Bash(bash_args) => format!("Run command: {}", bash_args.command),
            ToolArgs::WriteFile(write_args) => format!("Write file: {}", write_args.file_path),
            ToolArgs::EditFile(edit_args) => format!("Edit file: {}", edit_args.file_path),
            ToolArgs::ReadFile(read_args) => format!("Read file: {}", read_args.file_path),
        };

        println!("{}\n(y/n)?", description);

        let mut input = String::new();
        let mut stdin = BufReader::new(io::stdin());
        stdin.read_line(&mut input).await.unwrap();

        if input.trim().to_lowercase() == "y" {
            self.execute_tool(args).await
        } else {
            println!("Enter rejection reason:");
            let mut reason = String::new();
            stdin.read_line(&mut reason).await.unwrap();
            format!("User rejected tool call with reason: {}", reason)
        }
    }
}
