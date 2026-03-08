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

    /// Model to use: haiku, sonnet, or opus
    #[arg(short, long, default_value = "sonnet")]
    model: String,
}

fn resolve_model(name: &str) -> (String, String) {
    match name.to_lowercase().as_str() {
        "haiku" => (String::from("claude-haiku-4-5"), String::from("Haiku 4.5")),
        "sonnet" => (String::from("claude-sonnet-4-6"), String::from("Sonnet 4.6")),
        "opus" => (String::from("claude-opus-4-6"), String::from("Opus 4.6")),
        other => {
            eprintln!("Unknown model '{}'. Valid options: haiku, sonnet, opus", other);
            std::process::exit(1);
        }
    }
}

#[tokio::main]
async fn main() {
    ctrlc::set_handler(|| {
        println!();
        std::process::exit(0);
    })
    .expect("Failed to set Ctrl+C handler");

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
            let (model, _) = resolve_model(&args.model);
            let mut agent = Agent::new(
                model,
                vec![],
                api_key,
                permissions,
            );
            agent.run(messages).await;
        }
        None => {
            let (model, display_name) = resolve_model(&args.model);
            println!(
                r#"
        *
       / \
      / | \
     /  |  \
    /___|___\
    |   B   |
    |   O   |
    |   O   |
    |   S   |
    |   T   |
   /|   E   |\
  / |   R   | \
 /  |___|___|  \
/   |   |   |   \
\   |   |   |   /
 \  \  /|\  /  /
  \  \/ | \/  /
   \    |    /
       /|\
      / | \

    {}
"#,
                display_name
            );
            // Interactive mode
            let mut agent = Agent::new(
                model,
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
