use anyhow::Result;
use memvid_agent_core::agent::Agent;
use memvid_agent_core::types::WriterConfig;

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let config = WriterConfig::default();
    let model_path = std::env::var("MODEL_PATH")
        .unwrap_or_else(|_| "models/llama-model.gguf".to_string());
    let model_name = std::env::var("MODEL_NAME")
        .unwrap_or_else(|_| "llama-3.2-3b-tq".to_string());

    let mut agent = Agent::init(&model_path, &model_name, 4096, config)?;
    tracing::info!("Agent initialized. Model: {}", model_name);

    // Interactive loop
    loop {
        let mut input = String::new();
        print!("> ");
        std::io::Write::flush(&mut std::io::stdout())?;

        if std::io::stdin().read_line(&mut input)? == 0 {
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        if input == "/exit" || input == "/quit" {
            break;
        }

        match agent.chat(input) {
            Ok(response) => println!("{}", response),
            Err(e) => eprintln!("Error: {}", e),
        }
    }

    Ok(())
}
