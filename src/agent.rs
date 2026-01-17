use crate::types::{
    AssistantContent, BashToolArgs, ContentBlock, ContentBlockDelta, ContentBlockStart,
    ContentBlockStop, Delta, Message, TextMessageAcc, ToolCallAcc, UserContent,
};
use futures_util::StreamExt;
use reqwest::Client;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest_eventsource::{Event, EventSource};
use serde_json::json;
use std::collections::HashMap;
use std::io::Write;
use std::process::Stdio;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::process::Command;

pub struct Agent {
    model: String,
    messages: Vec<Message>,
    api_key: String,
}

impl Agent {
    pub fn new(model: String, messages: Vec<Message>, api_key: String) -> Agent {
        Agent {
            model,
            messages,
            api_key,
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
        let system_prompt: &str = "You are Boosty, a helpful command line assistant. You help people accomplish programming tasks and learn new things.";
        let client = Client::new();

        self.messages.extend(messages);
        let mut num_processed: usize = 0;

        while num_processed < self.messages.len() {
            num_processed = self.messages.len();
            let body = json!({
                "model":self.model,
                "max_tokens": 1024,
                "tools": [
                    {
                        "type": "bash_20250124",
                        "name": "bash"
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

            let mut tool_calls: HashMap<u32, ToolCallAcc> = HashMap::new();
            let mut text_messages: HashMap<u32, TextMessageAcc> = HashMap::new();
            let mut assistant_messages: Vec<AssistantContent> = vec![];
            let mut messages_to_process: Vec<Message> = vec![];
            let mut stdin = BufReader::new(io::stdin());

            while let Some(event) = event_source.next().await {
                match event {
                    Ok(Event::Open) => {}
                    Ok(Event::Message(msg)) => {
                        if msg.event == String::from("content_block_start") {
                            let data: ContentBlockStart = serde_json::from_str(&msg.data).unwrap();
                            match data.content_block {
                                ContentBlock::TextStart { text } => {
                                    print!("{}", text);
                                    text_messages.insert(data.index, TextMessageAcc { text });
                                }
                                ContentBlock::ToolUseStart { id, name } => {
                                    tool_calls.insert(
                                        data.index,
                                        ToolCallAcc {
                                            id,
                                            name,
                                            args: String::from(""),
                                        },
                                    );
                                }
                            }
                            std::io::stdout().flush().unwrap()
                        } else if msg.event == String::from("content_block_delta") {
                            let data: ContentBlockDelta = serde_json::from_str(&msg.data).unwrap();
                            match data.delta {
                                Delta::TextDelta { text } => {
                                    print!("{}", text);
                                    let cur_text_message =
                                        text_messages.get_mut(&data.index).unwrap();
                                    cur_text_message.text.push_str(&text);
                                }
                                Delta::InputJsonDelta { partial_json } => {
                                    let cur_tool_call = tool_calls.get_mut(&data.index).unwrap();
                                    cur_tool_call.args.push_str(&partial_json);
                                }
                            }
                            std::io::stdout().flush().unwrap();
                        } else if msg.event == String::from("content_block_stop") {
                            let data: ContentBlockStop = serde_json::from_str(&msg.data).unwrap();
                            if text_messages.contains_key(&data.index) {
                                let text = text_messages.get(&data.index).unwrap().text.clone();
                                assistant_messages.push(AssistantContent::Text { text });
                            } else if tool_calls.contains_key(&data.index) {
                                let tool_call = tool_calls.get(&data.index).unwrap();

                                // Parse tool args and execute tool
                                let tool_args: BashToolArgs =
                                    serde_json::from_str(&tool_call.args).unwrap();

                                println!("Requesting to run:");
                                println!("{}", tool_args.command);
                                println!("(y/n)?");
                                let mut input = String::new();
                                stdin.read_line(&mut input).await.unwrap();

                                assistant_messages.push(AssistantContent::ToolUse {
                                    id: tool_call.id.clone(),
                                    name: tool_call.name.clone(),
                                    input: tool_args.clone(),
                                });
                                messages_to_process.push(Message::Assistant {
                                    content: assistant_messages,
                                });
                                assistant_messages = vec![];
                                if input.trim().to_lowercase() == "y" {
                                    let mut child = Command::new("sh")
                                        .arg("-c")
                                        .arg(&tool_args.command)
                                        .stdout(Stdio::piped())
                                        .stderr(Stdio::piped())
                                        .spawn()
                                        .expect("Failed to execute command");

                                    let stdout =
                                        child.stdout.take().expect("Failed to capture stdout");
                                    let stderr =
                                        child.stderr.take().expect("Failed to capture stderr");

                                    let mut stdout_reader = BufReader::new(stdout).lines();
                                    let mut stderr_reader = BufReader::new(stderr).lines();

                                    let mut captured_output = String::new();

                                    loop {
                                        tokio::select! {
                                            line = stdout_reader.next_line() => {
                                                match line {
                                                    Ok(Some(line)) => {
                                                        println!("{}", line);
                                                        captured_output.push_str(&line);
                                                        captured_output.push('\n');
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
                                                        captured_output.push_str(&line);
                                                        captured_output.push('\n');
                                                    }
                                                    Ok(None) => {}
                                                    Err(e) => {
                                                        eprintln!("Error reading stderr: {}", e);
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    messages_to_process.push(Message::User {
                                        content: vec![UserContent::ToolResult {
                                            tool_use_id: tool_call.id.clone(),
                                            content: captured_output,
                                        }],
                                    });
                                } else {
                                    println!("Enter rejection reason:");
                                    let mut reason = String::new();
                                    stdin.read_line(&mut reason).await.unwrap();
                                    messages_to_process.push(Message::User {
                                        content: vec![UserContent::ToolResult {
                                            tool_use_id: tool_call.id.clone(),
                                            content: format!(
                                                "User rejected tool call with reason: {}",
                                                reason
                                            ),
                                        }],
                                    });
                                }
                            }
                        }
                    }
                    Err(_err) => {
                        event_source.close();
                    }
                }
            }
            self.messages.extend(messages_to_process);
        }
    }
}
