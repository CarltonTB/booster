use clap::Parser;

use std::io::Write;
use tokio::io::{self, AsyncBufReadExt, BufReader};

mod agent;
use agent::Agent;

mod types;
use types::Message;

use crate::types::{AgentPermissions, UserContent};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    prompt: Option<String>,

    #[arg(short, long)]
    yolo: bool,
}

#[tokio::main]
async fn main() {
    let api_key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY not set");

    let args = Args::parse();
    let permissions = match args.yolo {
        true => AgentPermissions::AllowAll,
        false => AgentPermissions::ConfirmAll,
    };

    match args.prompt {
        Some(prompt) => {
            // Single prompt mode
            let messages = vec![Message::User {
                content: vec![UserContent::Text { text: prompt }],
            }];
            let mut agent = Agent::new(
                String::from("claude-haiku-4-5"),
                vec![],
                api_key,
                permissions,
            );
            agent.run(messages).await;
        }
        None => {
            // Interactive mode
            let mut agent = Agent::new(
                String::from("claude-haiku-4-5"),
                vec![],
                api_key,
                permissions,
            );
            loop {
                println!();
                print!("> ");
                std::io::stdout().flush().unwrap();
                let mut stdin = BufReader::new(io::stdin());
                let mut input = String::new();
                stdin.read_line(&mut input).await.unwrap();
                let messages = vec![Message::User {
                    content: vec![UserContent::Text { text: input }],
                }];
                agent.run(messages).await;
            }
        }
    };
}
