use clap::Parser;
use rustyline::DefaultEditor;

mod agent;
use agent::Agent;

mod streaming;

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
    tokio::spawn(async {
        tokio::signal::ctrl_c().await.ok();
        println!();
        std::process::exit(0);
    });

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
                String::from("claude-sonnet-4-6"),
                vec![],
                api_key,
                permissions,
            );
            agent.run(messages).await;
        }
        None => {
            // Interactive mode
            let mut agent = Agent::new(
                String::from("claude-sonnet-4-6"),
                vec![],
                api_key,
                permissions,
            );
            let mut rl = DefaultEditor::new().expect("Failed to initialize line editor");
            loop {
                println!();
                let input = match rl.readline("> ") {
                    Ok(line) => line,
                    Err(_) => break,
                };
                rl.add_history_entry(&input).ok();
                let messages = vec![Message::User {
                    content: vec![UserContent::Text { text: input }],
                }];
                agent.run(messages).await;
            }
        }
    };
}
