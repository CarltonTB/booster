use std::io::Write;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};

pub enum StreamEvent {
    NewKey(String),
    Word(String),
    Done,
}

enum JsonParseState {
    Init,
    InKey,
    KeyEscape,
    AfterKey,
    AfterColon,
    InStringValue,
    StringEscape,
    InOtherValue,
}

pub struct ToolArgStreamer {
    state: JsonParseState,
    current_key: String,
    display_keys: Vec<String>,
    in_display_key: bool,
    sender: mpsc::UnboundedSender<StreamEvent>,
    word_buf: String,
}

impl ToolArgStreamer {
    pub fn new(display_keys: Vec<&str>) -> (Self, mpsc::UnboundedReceiver<StreamEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            ToolArgStreamer {
                state: JsonParseState::Init,
                current_key: String::new(),
                display_keys: display_keys.into_iter().map(String::from).collect(),
                in_display_key: false,
                sender: tx,
                word_buf: String::new(),
            },
            rx,
        )
    }

    pub fn feed(&mut self, partial_json: &str) {
        for ch in partial_json.chars() {
            self.process_char(ch);
        }
    }

    pub fn finish(&mut self) {
        if !self.word_buf.is_empty() {
            let word = std::mem::take(&mut self.word_buf);
            let _ = self.sender.send(StreamEvent::Word(word));
        }
        let _ = self.sender.send(StreamEvent::Done);
    }

    fn process_char(&mut self, ch: char) {
        match self.state {
            JsonParseState::Init => {
                if ch == '"' {
                    self.current_key.clear();
                    self.state = JsonParseState::InKey;
                }
            }
            JsonParseState::InKey => match ch {
                '\\' => self.state = JsonParseState::KeyEscape,
                '"' => {
                    self.in_display_key = self.display_keys.contains(&self.current_key);
                    if self.in_display_key {
                        let _ = self
                            .sender
                            .send(StreamEvent::NewKey(self.current_key.clone()));
                    }
                    self.state = JsonParseState::AfterKey;
                }
                _ => self.current_key.push(ch),
            },
            JsonParseState::KeyEscape => {
                self.current_key.push(ch);
                self.state = JsonParseState::InKey;
            }
            JsonParseState::AfterKey => {
                if ch == ':' {
                    self.state = JsonParseState::AfterColon;
                }
            }
            JsonParseState::AfterColon => {
                if ch == '"' {
                    self.state = JsonParseState::InStringValue;
                } else if !ch.is_whitespace() {
                    self.state = JsonParseState::InOtherValue;
                }
            }
            JsonParseState::InStringValue => match ch {
                '\\' => self.state = JsonParseState::StringEscape,
                '"' => {
                    if self.in_display_key && !self.word_buf.is_empty() {
                        let word = std::mem::take(&mut self.word_buf);
                        let _ = self.sender.send(StreamEvent::Word(word));
                    }
                    self.in_display_key = false;
                    self.state = JsonParseState::Init;
                }
                _ => {
                    if self.in_display_key {
                        self.emit_char(ch);
                    }
                }
            },
            JsonParseState::StringEscape => {
                if self.in_display_key {
                    let unescaped = match ch {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        _ => ch,
                    };
                    self.emit_char(unescaped);
                }
                self.state = JsonParseState::InStringValue;
            }
            JsonParseState::InOtherValue => {
                if ch == ',' || ch == '}' {
                    self.in_display_key = false;
                    self.state = JsonParseState::Init;
                }
            }
        }
    }

    fn emit_char(&mut self, ch: char) {
        if ch == ' ' || ch == '\n' || ch == '\t' {
            if !self.word_buf.is_empty() {
                let mut word = std::mem::take(&mut self.word_buf);
                word.push(ch);
                let _ = self.sender.send(StreamEvent::Word(word));
            } else {
                let _ = self.sender.send(StreamEvent::Word(ch.to_string()));
            }
        } else {
            self.word_buf.push(ch);
        }
    }
}

pub struct TextStreamer {
    sender: mpsc::UnboundedSender<StreamEvent>,
    word_buf: String,
}

impl TextStreamer {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<StreamEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            TextStreamer {
                sender: tx,
                word_buf: String::new(),
            },
            rx,
        )
    }

    pub fn label(&self, name: &str) {
        let _ = self.sender.send(StreamEvent::NewKey(name.to_string()));
    }

    pub fn feed(&mut self, text: &str) {
        for ch in text.chars() {
            if ch == ' ' || ch == '\n' || ch == '\t' {
                if !self.word_buf.is_empty() {
                    let mut word = std::mem::take(&mut self.word_buf);
                    word.push(ch);
                    let _ = self.sender.send(StreamEvent::Word(word));
                } else {
                    let _ = self.sender.send(StreamEvent::Word(ch.to_string()));
                }
            } else {
                self.word_buf.push(ch);
            }
        }
    }

    pub fn finish(&mut self) {
        if !self.word_buf.is_empty() {
            let word = std::mem::take(&mut self.word_buf);
            let _ = self.sender.send(StreamEvent::Word(word));
        }
        let _ = self.sender.send(StreamEvent::Done);
    }
}

pub async fn smooth_printer(mut rx: mpsc::UnboundedReceiver<StreamEvent>, word_delay: Duration) {
    while let Some(event) = rx.recv().await {
        match event {
            StreamEvent::NewKey(key) => {
                print!("\n[{}] ", key);
                std::io::stdout().flush().unwrap();
            }
            StreamEvent::Word(word) => {
                print!("{}", word);
                std::io::stdout().flush().unwrap();
                sleep(word_delay).await;
            }
            StreamEvent::Done => {
                println!();
                std::io::stdout().flush().unwrap();
                break;
            }
        }
    }
}
