use anyhow::Result;
use colored::*;
use memvid_agent_core::agent::Agent;
use memvid_agent_core::api::ApiServer;
use memvid_agent_core::books_catalog::BooksCatalog;
use memvid_agent_core::config::Config;
use memvid_agent_core::languages_catalog::LanguagesCatalog;
use memvid_agent_core::models;
use memvid_agent_core::models_catalog::{self, ModelsCatalog};
use memvid_agent_core::utils::FileLock;
use std::sync::{Arc, Mutex};

fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let config_path = std::path::Path::new("config.json");
    let is_first_run = !config_path.exists();

    let mut config = Config::load_or_create()?;
    config.validate()?;

    let catalog = ModelsCatalog::load();

    if is_first_run {
        run_setup_wizard(&mut config, &catalog)?;
    }

    models::ensure_model(&config.model)?;

    let _lock = FileLock::acquire(&config.data_dir).expect("Failed to acquire data directory lock");
    let agent = Arc::new(Mutex::new(Agent::init(&config)?));

    let mut loaded_files: Vec<String> = Vec::new();

    {
        let a = agent.lock().unwrap();
        eprintln!(
            "{} Agent initialized. Model: {}",
            "●".bright_green(),
            config.model.name.bright_cyan()
        );
        eprintln!(
            "{} {} knowledge entries indexed",
            "↳".dimmed(),
            a.knowledge_count()
        );
        if is_first_run {
            eprintln!("{} First-time setup complete.", "↳".dimmed());
        }
        print_startup_help();
        eprintln!();
    }

    loop {
        let input = match read_line() {
            Some(s) => s,
            None => break,
        };
        let input = input.trim().to_string();
        let input_lower = input.to_lowercase();

        if input.is_empty() {
            continue;
        }

        if input_lower == "/exit" || input_lower == "/quit" {
            break;
        }

        if input_lower == "/help" {
            print_help();
            continue;
        }

        if input_lower == "/config" {
            print_config(&config);
            continue;
        }

        if input_lower == "/stats" {
            let a = agent.lock().unwrap();
            print_stats(&a, &config, &loaded_files)?;
            continue;
        }

        if input_lower == "/history" || input_lower == "/ls" {
            let a = agent.lock().unwrap();
            print_history(&a)?;
            continue;
        }

        if input_lower == "/models" || input == "/MODELS" {
            println!("{} Available Models", "━━━ Models ━━━".bold());
            for entry in catalog.list() {
                let current = if entry.name == config.model.name {
                    " ◄ active"
                } else {
                    ""
                };
                println!(
                    "  {:<20} {} ({} MB){}",
                    entry.id.bright_cyan(),
                    entry.name,
                    entry.size_mb,
                    current.green(),
                );
                println!("  {:<20} {}", "", entry.description.dimmed());
                println!();
            }
            continue;
        }

        if input_lower == "/model" && input == "/model" || input == "/MODEL" {
            print_model_current(&config);
            continue;
        }

        if let Some(model_id) = input.strip_prefix("/model ") {
            let model_id = model_id.trim();
            if model_id.eq_ignore_ascii_case("current") {
                print_model_current(&config);
                continue;
            }
            let entry = match catalog.find(model_id) {
                Some(e) => e,
                None => {
                    eprintln!(
                        "{} Unknown model '{}'. Use /models to list available.",
                        "✗".red(),
                        model_id
                    );
                    continue;
                }
            };

            let models_dir = std::path::Path::new("models");
            match models_catalog::download_model(entry, models_dir) {
                Ok(model_path) => {
                    models_catalog::apply_model_to_config(&model_path, entry, &mut config)?;
                    let spinner = indicatif::ProgressBar::new_spinner();
                    spinner.set_style(
                        indicatif::ProgressStyle::default_spinner()
                            .template("{spinner:.cyan} {msg}")
                            .expect("Invalid spinner template"),
                    );
                    spinner.set_message("Loading new model…");
                    spinner.enable_steady_tick(std::time::Duration::from_millis(80));
                    {
                        let mut a = agent.lock().unwrap();
                        a.switch_model(
                            &config.model.path,
                            config.model.n_ctx,
                            config.model.n_gpu_layers,
                            &config.model.name,
                            &config.model.chat_template,
                            config.generation.top_k,
                            config.generation.top_p,
                            config.generation.temp,
                        )?;
                    }
                    spinner.finish_and_clear();
                    println!(
                        "{} Switched to model: {} ({})",
                        "✓".green(),
                        entry.name.bold(),
                        config.model.n_ctx
                    );
                }
                Err(e) => {
                    eprintln!("{} Failed to download model: {:#}", "✗".red(), e);
                }
            }
            continue;
        }

        if input_lower == "/languages" || input == "/LANGUAGES" {
            let spinner = indicatif::ProgressBar::new_spinner();
            spinner.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Invalid spinner template"),
            );
            spinner.set_message("Fetching language catalog…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));
            let lang_catalog = LanguagesCatalog::load_or_fetch(&config.data_dir)?;
            spinner.finish_and_clear();

            println!(
                "{} Available Languages ({})",
                "━━━ Languages ━━━".bold(),
                lang_catalog.len()
            );
            for entry in lang_catalog.list().iter().take(30) {
                let installed = if config.languages.installed.contains(&entry.key) {
                    " ◄ installed".green().to_string()
                } else {
                    String::new()
                };
                println!(
                    "  {:<20} {} resources{}",
                    entry.key.bright_cyan(),
                    entry.resources.len(),
                    installed
                );
            }
            if lang_catalog.len() > 30 {
                println!(
                    "  ... and {} more (use /learn <lang> to fetch docs)",
                    lang_catalog.len() - 30
                );
            }
            continue;
        }

        if input_lower == "/books" || input_lower == "/BOOKS" {
            let spinner = indicatif::ProgressBar::new_spinner();
            spinner.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Invalid spinner template"),
            );
            spinner.set_message("Fetching books catalog…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));
            let catalog = match BooksCatalog::fetch() {
                Ok(c) => c,
                Err(e) => {
                    spinner.finish_and_clear();
                    eprintln!("{} Failed to fetch books catalog: {}", "✗".red(), e);
                    continue;
                }
            };
            spinner.finish_and_clear();

            let langs = catalog.list_languages();
            println!(
                "{} Free Programming Books - Languages ({})",
                "━━━ Books ━━━".bold(),
                langs.len()
            );
            for lang in langs.iter().take(60) {
                println!("  • {}", lang.bright_cyan());
            }
            if langs.len() > 60 {
                println!("  ... and {} more", langs.len() - 60);
            }
            println!("Use /download-books <language> [limit] to download and ingest.");
            continue;
        }

        if input_lower == "/languages-installed" || input_lower == "/lang-installed" {
            if config.languages.installed.is_empty() {
                println!("  No languages installed. Use /learn <lang> to install one.");
            } else {
                println!("{} Installed Languages", "━━━ Installed ━━━".bold());
                for lang in &config.languages.installed {
                    println!("  • {}", lang.bright_cyan());
                }
            }
            continue;
        }

        if let Some(lang_id) = input.strip_prefix("/learn ") {
            let lang_id = lang_id.trim().to_string();
            if lang_id.is_empty() {
                eprintln!("{} Usage: /learn <language>", "✗".red());
                continue;
            }

            let lang_catalog = LanguagesCatalog::load_or_fetch(&config.data_dir)?;
            let lang = match lang_catalog.find(&lang_id) {
                Some(l) => l,
                None => {
                    eprintln!(
                        "{} Unknown language '{}'. Use /languages to list available.",
                        "✗".red(),
                        lang_id
                    );
                    continue;
                }
            };

            println!(
                "{} Downloading documentation for {} ({} resources)…",
                "↓".yellow(),
                lang.name.bold(),
                lang.resources.len()
            );

            let max_resources = std::cmp::min(lang.resources.len(), 10);
            let entries = memvid_agent_core::languages_catalog::download_language_resources(
                lang,
                max_resources,
            )?;

            if entries.is_empty() {
                eprintln!("{} No resources downloaded for {}.", "✗".red(), lang.name);
                continue;
            }

            let spinner = indicatif::ProgressBar::new_spinner();
            spinner.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Invalid spinner template"),
            );
            spinner.set_message("Indexing…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));

            {
                let mut a = agent.lock().unwrap();
                for entry in &entries {
                    a.store_knowledge_direct(&entry.source, &entry.content)?;
                }
            }
            spinner.finish_and_clear();

            config.languages.mark_installed(&lang.key);
            config.save()?;

            println!(
                "{} Installed {} — {} chunks indexed as knowledge.",
                "✓".green(),
                lang.name.bold(),
                entries.len()
            );
            continue;
        }

        if let Some(lang_key) = input.strip_prefix("/unlearn ") {
            let lang_key = lang_key.trim();
            if lang_key.is_empty() {
                eprintln!("{} Usage: /unlearn <language>", "✗".red());
                continue;
            }

            let removed = {
                let mut a = agent.lock().unwrap();
                a.unlearn_language(lang_key)?
            };
            config.languages.mark_uninstalled(lang_key);
            config.save()?;

            println!(
                "{} Unlearned '{}' — {} knowledge entries removed.",
                "✓".green(),
                lang_key.bold(),
                removed
            );
            continue;
        }

        if input_lower == "/reindex" {
            let spinner = indicatif::ProgressBar::new_spinner();
            spinner.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Invalid spinner template"),
            );
            spinner.set_message("Rebuilding index from .mv2 files…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));
            {
                let mut a = agent.lock().unwrap();
                a.reindex_from_mv2(&config.data_dir)?;
            }
            spinner.finish_and_clear();
            println!(
                "{} Reindex complete. {} knowledge entries indexed.",
                "✓".green(),
                agent.lock().unwrap().knowledge_count()
            );
            continue;
        }

        if input_lower.starts_with("/token") {
            if !config.api.enabled {
                config.api.enabled = true;
            }
            if config.api.token.is_none() {
                config.api.token = Some(uuid::Uuid::new_v4().to_string());
            }
            config.save()?;

            let api_agent = agent.clone();
            let model_name = config.model.name.clone();
            let host = config.api.host.clone();
            let port = config.api.port;
            let token = config.api.token.clone();

            println!(
                "{} API server starting on http://{}:{}",
                "●".bright_green(),
                host,
                port
            );
            if let Some(ref t) = token {
                println!("{} Token: {}", "  key".dimmed(), t.bright_yellow());
                println!("{} Use: Authorization: Bearer {}", "  auth".dimmed(), t);
            } else {
                println!("{} No authentication configured", "  auth".dimmed());
            }

            std::thread::spawn(move || {
                let server = ApiServer::new(api_agent, model_name, host, port, token);
                if let Err(e) = server.run() {
                    eprintln!("[api] Server error: {}", e);
                }
            });

            println!("{} API server is running in the background.", "✓".green());
            continue;
        }

        if input_lower.starts_with("/search ") {
            let query = input
                .strip_prefix("/search ")
                .or_else(|| input.strip_prefix("/SEARCH "))
                .unwrap_or("")
                .trim()
                .to_string();
            if query.is_empty() {
                eprintln!("{} Usage: /search <query>", "✗".red());
                continue;
            }
            let a = agent.lock().unwrap();
            let results = a.search_knowledge(&query, 5);
            if results.is_empty() {
                println!("  No matching knowledge found.");
            } else {
                println!("{} Search results for '{}':", "━━━".bold(), query.bold());
                for (i, entry) in results.iter().enumerate() {
                    let preview: String = entry.content.chars().take(120).collect();
                    println!("  {}. [{}] {}", (i + 1), entry.source.dimmed(), preview);
                }
            }
            continue;
        }

        if input_lower.starts_with("/load ") {
            let path = input
                .strip_prefix("/load ")
                .or_else(|| input.strip_prefix("/LOAD "))
                .unwrap_or("")
                .trim();
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    let filename = std::path::Path::new(path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    {
                        let mut a = agent.lock().unwrap();
                        a.ingest_raw(&filename, &content)?;
                    }
                    loaded_files.push(filename.to_string());
                    println!(
                        "{} Loaded {} ({} bytes)",
                        "✓".green(),
                        filename.bold(),
                        content.len()
                    );
                }
                Err(e) => eprintln!("{} Error reading {}: {}", "✗".red(), path, e),
            }
            continue;
        }

        if input_lower.starts_with("/ingest ") {
            let path = input
                .strip_prefix("/ingest ")
                .or_else(|| input.strip_prefix("/INGEST "))
                .unwrap_or("")
                .trim();
            let file_path = std::path::Path::new(path);
            let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let is_binary = matches!(ext, "pdf" | "epub");

            if is_binary {
                match agent.lock().unwrap().ingest_file(file_path) {
                    Ok(extracted) => {
                        let title = extracted.title.as_deref().unwrap_or("untitled");
                        println!(
                            "{} Ingested: {} ({} format, {} chars)",
                            "✓".green(),
                            title.bold(),
                            extracted.format,
                            extracted.content.len()
                        );
                    }
                    Err(e) => eprintln!("{} {}", "✗".red(), e),
                }
            } else {
                match std::fs::read_to_string(path) {
                    Ok(content) => {
                        let filename = file_path.file_name().unwrap_or_default().to_string_lossy();
                        {
                            let mut a = agent.lock().unwrap();
                            a.ingest_knowledge(&filename, &content)?;
                        }
                        loaded_files.push(filename.to_string());
                        let spinner = indicatif::ProgressBar::new_spinner();
                        spinner.set_style(
                            indicatif::ProgressStyle::default_spinner()
                                .template("{spinner:.cyan} {msg}")
                                .expect("Invalid spinner template"),
                        );
                        spinner.set_message("Indexing…");
                        spinner.enable_steady_tick(std::time::Duration::from_millis(80));
                        let response = {
                            let mut a = agent.lock().unwrap();
                            a.chat(&format!(
                                "I just loaded the file '{}'. Briefly explain what it contains.",
                                filename
                            ))?
                        };
                        spinner.finish_and_clear();
                        println!("{}", response.bright_cyan());
                    }
                    Err(e) => eprintln!("{} Error reading {}: {}", "✗".red(), path, e),
                }
            }
            continue;
        }

        if input_lower.starts_with("/ingest-pdf ") || input_lower.starts_with("/INGEST-PDF ") {
            let path = input
                .strip_prefix("/ingest-pdf ")
                .or_else(|| input.strip_prefix("/INGEST-PDF "))
                .unwrap_or("")
                .trim();
            let file_path = std::path::Path::new(path);
            match agent.lock().unwrap().ingest_file(file_path) {
                Ok(extracted) => {
                    let title = extracted.title.as_deref().unwrap_or("untitled");
                    let preview: String = extracted.content.chars().take(200).collect();
                    println!(
                        "{} Ingested: {} ({} format, {} chars)",
                        "✓".green(),
                        title.bold(),
                        extracted.format,
                        extracted.content.len()
                    );
                    println!("  {} preview: {}", "↳".dimmed(), preview.dimmed());
                }
                Err(e) => eprintln!("{} Failed to ingest: {:#}", "✗".red(), e),
            }
            continue;
        }

        if input_lower.starts_with("/fetch ") {
            let url = input
                .strip_prefix("/fetch ")
                .or_else(|| input.strip_prefix("/FETCH "))
                .unwrap_or("")
                .trim();
            if url.is_empty() {
                eprintln!("{} Usage: /fetch <url>", "✗".red());
                continue;
            }

            let spinner = indicatif::ProgressBar::new_spinner();
            spinner.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Invalid spinner template"),
            );
            spinner.set_message("Fetching…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));

            let result = {
                let mut a = agent.lock().unwrap();
                a.fetch_and_ingest(url, &config.ingestion)
            };

            spinner.finish_and_clear();

            match result {
                Ok(content) => {
                    let title = content.title.as_deref().unwrap_or("untitled");
                    let preview: String = content.content.chars().take(200).collect();
                    println!(
                        "{} Fetched: {} ({})",
                        "✓".green(),
                        title.bold(),
                        url.dimmed()
                    );
                    println!("  {} type: {}", "↳".dimmed(), content.content_type);
                    println!("  {} size: {} bytes", "↳".dimmed(), content.size_bytes);
                    println!("  {} preview: {}", "↳".dimmed(), preview.dimmed());
                }
                Err(e) => {
                    eprintln!("{} Failed to fetch: {:#}", "✗".red(), e);
                }
            }
            continue;
        }

        if input_lower.starts_with("/fetch-md ") {
            let url = input
                .strip_prefix("/fetch-md ")
                .or_else(|| input.strip_prefix("/FETCH-MD "))
                .unwrap_or("")
                .trim();
            if url.is_empty() {
                eprintln!("{} Usage: /fetch-md <url>", "✗".red());
                continue;
            }

            let spinner = indicatif::ProgressBar::new_spinner();
            spinner.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Invalid spinner template"),
            );
            spinner.set_message("Fetching…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));

            let result = {
                let mut a = agent.lock().unwrap();
                let mut fetcher =
                    memvid_agent_core::web_fetcher::WebFetcher::new(&config.ingestion);
                fetcher.fetch_and_retry(url)
            };

            spinner.finish_and_clear();

            match result {
                Ok(content) => {
                    let body = ureq::get(url).call();
                    let html = match body {
                        Ok(r) => r.into_body().read_to_string().unwrap_or_default(),
                        Err(_) => String::new(),
                    };
                    let md = if !html.is_empty() && content.content_type.contains("html") {
                        memvid_agent_core::extractor::html_to_markdown(&html)
                    } else {
                        content.content.clone()
                    };
                    let title = content.title.as_deref().unwrap_or("untitled");
                    println!(
                        "{} {} — Markdown ({} chars)",
                        "━━━".bold(),
                        title.bold(),
                        md.len()
                    );
                    println!("{}", md.dimmed());
                }
                Err(e) => {
                    eprintln!("{} Failed to fetch: {:#}", "✗".red(), e);
                }
            }
            continue;
        }

        if input_lower.starts_with("/batch ") {
            let path = input
                .strip_prefix("/batch ")
                .or_else(|| input.strip_prefix("/BATCH "))
                .unwrap_or("")
                .trim();
            if path.is_empty() {
                eprintln!("{} Usage: /batch <file>", "✗".red());
                continue;
            }

            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{} Error reading {}: {}", "✗".red(), path, e);
                    continue;
                }
            };

            let urls: Vec<String> = content
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| l.to_string())
                .collect();

            if urls.is_empty() {
                eprintln!("{} No URLs found in {}", "✗".red(), path);
                continue;
            }

            println!(
                "{} Processing {} URLs from {}…",
                "↓".yellow(),
                urls.len(),
                path.dimmed()
            );

            let pb = indicatif::ProgressBar::new(urls.len() as u64);
            pb.set_style(
                indicatif::ProgressStyle::default_bar()
                    .template("{bar:40.cyan/blue} {pos}/{len} ({eta})")
                    .expect("Invalid progress bar template")
                    .progress_chars("##-"),
            );

            let result = {
                let mut a = agent.lock().unwrap();
                a.process_url_batch(&urls, &config.ingestion)
            };

            pb.finish_and_clear();

            match result {
                Ok(stats) => {
                    println!(
                        "{} Batch complete: {} ok, {} failed, ~{} chunks",
                        "✓".green(),
                        stats.success.len(),
                        stats.failures.len(),
                        stats.total_chunks
                    );
                    for (url, err) in &stats.failures {
                        eprintln!("  {} {} — {}", "✗".red(), url.dimmed(), err);
                    }
                }
                Err(e) => {
                    eprintln!("{} Batch failed: {:#}", "✗".red(), e);
                }
            }
            continue;
        }

        if input_lower.starts_with("/download-books ")
            || input_lower.starts_with("/DOWNLOAD-BOOKS ")
        {
            let rest = input
                .strip_prefix("/download-books ")
                .or_else(|| input.strip_prefix("/DOWNLOAD-BOOKS "))
                .unwrap_or("")
                .trim();
            if rest.is_empty() {
                eprintln!("{} Usage: /download-books <language> [limit]", "✗".red());
                continue;
            }

            let mut parts = rest.split_whitespace();
            let lang = parts.next().unwrap();
            let limit: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(10);

            println!(
                "{} Downloading and ingesting up to {} resources for '{}'…",
                "↓".yellow(),
                limit,
                lang.bold()
            );
            let spinner = indicatif::ProgressBar::new_spinner();
            spinner.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Invalid spinner template"),
            );
            spinner.set_message("Processing…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));

            let result = {
                let mut a = agent.lock().unwrap();
                a.download_and_ingest_books(lang, limit)
            };

            spinner.finish_and_clear();

            match result {
                Ok(count) => {
                    config.languages.mark_installed(lang);
                    if let Err(e) = config.save() {
                        eprintln!("{} Failed to save config: {}", "✗".red(), e);
                    }
                    println!(
                        "{} Ingested {} resource(s) for '{}'.",
                        "✓".green(),
                        count,
                        lang.bold()
                    );
                }
                Err(e) => {
                    eprintln!("{} Failed to download/ingest books: {:#}", "✗".red(), e);
                }
            }
            continue;
        }

        let spinner = indicatif::ProgressBar::new_spinner();
        spinner.set_style(
            indicatif::ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .expect("Invalid spinner template"),
        );
        spinner.set_message("Thinking…");
        spinner.enable_steady_tick(std::time::Duration::from_millis(80));

        let response = {
            let mut a = agent.lock().unwrap();
            a.chat(&input)
        };

        match response {
            Ok(response) => {
                spinner.finish_and_clear();
                println!("{}", response.bright_cyan());
            }
            Err(e) => {
                spinner.finish_and_clear();
                eprintln!("{} Error: {:#}", "✗".red(), e);
            }
        }
    }

    eprintln!();
    eprintln!("{} Bye!", "●".bright_green());
    Ok(())
}

fn read_line() -> Option<String> {
    use std::io::Write;
    let prompt = format!("{} ", ">".green());
    print!("{}", prompt);
    std::io::stdout().flush().ok()?;
    let mut buf = String::new();
    if std::io::stdin().read_line(&mut buf).ok()? == 0 {
        return None;
    }
    Some(buf)
}

fn print_startup_help() {
    eprintln!("{}", "━━━ Quick ━━━".bold());
    eprintln!("  {:<20}  Chat with the agent", "<message>".dimmed());
    eprintln!(
        "  {:<20}  List / switch models",
        "/models | /model <id>".dimmed()
    );
    eprintln!("  {:<20}  List all commands", "/help".dimmed());
    eprintln!("  {:<20}  Exit", "/exit".dimmed());
}

fn print_help() {
    println!();
    println!("{} Commands", "━━━ Commands ━━━".bold());
    println!("  {:<20}  Chat with the agent", "<message>".dimmed());
    println!("  {:<20}  List available models", "/models".bright_blue());
    println!(
        "  {:<20}  Show / download / switch model",
        "/model [id|current]".bright_blue()
    );
    println!(
        "  {:<20}  List available languages",
        "/languages".bright_blue()
    );
    println!(
        "  {:<20}  List installed languages",
        "/languages-installed".bright_blue()
    );
    println!(
        "  {:<20}  List programming-book languages (free-programming-books)",
        "/books".bright_blue()
    );
    println!(
        "  {:<20}  Download & ingest books for a language",
        "/download-books <lang> [limit]".bright_blue()
    );
    println!(
        "  {:<20}  Download docs for a language",
        "/learn <lang>".bright_blue()
    );
    println!(
        "  {:<20}  Remove language docs from knowledge",
        "/unlearn <lang>".bright_blue()
    );
    println!(
        "  {:<20}  Fetch URL and index as knowledge",
        "/fetch <url>".bright_blue()
    );
    println!(
        "  {:<20}  Fetch URL and show markdown",
        "/fetch-md <url>".bright_blue()
    );
    println!(
        "  {:<20}  Process batch of URLs from file",
        "/batch <file>".bright_blue()
    );
    println!(
        "  {:<20}  Load a text file into session context",
        "/load <file>".bright_blue()
    );
    println!(
        "  {:<20}  Load file and index as knowledge (auto-detects PDF/EPUB)",
        "/ingest <file>".bright_blue()
    );
    println!(
        "  {:<20}  Extract & index a PDF/EPUB file",
        "/ingest-pdf <file>".bright_blue()
    );
    println!(
        "  {:<20}  Search indexed knowledge",
        "/search <query>".bright_blue()
    );
    println!(
        "  {:<20}  Rebuild knowledge index from .mv2 files",
        "/reindex".bright_blue()
    );
    println!(
        "  {:<20}  Start API server / show token",
        "/token".bright_blue()
    );
    println!(
        "  {:<20}  Show conversation history",
        "/history".bright_blue()
    );
    println!("  {:<20}  Show agent statistics", "/stats".bright_blue());
    println!(
        "  {:<20}  Show current configuration",
        "/config".bright_blue()
    );
    println!("  {:<20}  Show this help", "/help".bright_blue());
    println!("  {:<20}  Exit the agent", "/exit".bright_blue());
    println!();
    println!(
        "{} Commands also accept uppercase: /MODELS, /MODEL, /LANGUAGES, /SEARCH, /REINDEX, /TOKEN, /LOAD, /INGEST",
        "↳".dimmed()
    );
    println!();
}

fn print_stats(agent: &Agent, config: &Config, files: &[String]) -> Result<()> {
    println!();
    println!("{} Agent Stats", "━━━ Stats ━━━".bold());
    println!("  {} {}", "Model:".dimmed(), config.model.name);
    println!(
        "  {} {} interactions",
        "Chats:".dimmed(),
        agent.interaction_count()
    );
    println!("  {} {}", "Memory:".dimmed(), agent.memory_summary()?);
    if !files.is_empty() {
        println!("  {} {} files loaded", "Files:".dimmed(), files.len());
        for f in files {
            println!("    • {}", f.dimmed());
        }
    }
    println!();
    Ok(())
}

fn print_history(agent: &Agent) -> Result<()> {
    println!();
    println!("{} Conversation History", "━━━ History ━━━".bold());
    let lines = agent.read_conversation_history()?;
    for line in &lines {
        if line.starts_with("  You:") {
            println!("{}", line.bright_green());
        } else if line.starts_with("  Assistant:") {
            println!("{}", line.bright_cyan());
        } else if line.starts_with("  System:") {
            println!("{}", line.dimmed());
        } else {
            println!("{}", line);
        }
    }
    println!();
    Ok(())
}

fn print_config(config: &Config) {
    println!();
    println!("{} Configuration", "━━━ Config ━━━".bold());
    println!("  {} {}", "data_dir:".dimmed(), config.data_dir.display());
    println!("  {} {}", "developer_mode:".dimmed(), config.developer_mode);
    println!(
        "  {} {}",
        "developer_prompt:".dimmed(),
        config.developer_prompt.as_deref().unwrap_or("(default)")
    );
    println!("  {} Model", "──".dimmed());
    println!("    {} {}", "name:".dimmed(), config.model.name);
    println!("    {} {}", "path:".dimmed(), config.model.path);
    println!("    {} {}", "n_ctx:".dimmed(), config.model.n_ctx);
    println!(
        "    {} {}",
        "n_gpu_layers:".dimmed(),
        config.model.n_gpu_layers
    );
    println!(
        "    {} {}",
        "chat_template:".dimmed(),
        config.model.chat_template
    );
    println!("  {} Generation", "──".dimmed());
    println!("    {} {}", "top_k:".dimmed(), config.generation.top_k);
    println!("    {} {}", "top_p:".dimmed(), config.generation.top_p);
    println!("    {} {}", "temp:".dimmed(), config.generation.temp);
    println!(
        "    {} {}",
        "max_tokens:".dimmed(),
        config.generation.max_tokens
    );
    println!("  {} API", "──".dimmed());
    println!("    {} {}", "enabled:".dimmed(), config.api.enabled);
    println!("    {} {}", "host:".dimmed(), config.api.host);
    println!("    {} {}", "port:".dimmed(), config.api.port);
    println!(
        "    {} {}",
        "token:".dimmed(),
        config.api.token.as_deref().unwrap_or("(none)")
    );
    println!("  {} Languages", "──".dimmed());
    if config.languages.installed.is_empty() {
        println!("    {} (none installed)", "installed:".dimmed());
    } else {
        for lang in &config.languages.installed {
            println!("    • {}", lang);
        }
    }
    println!();
}

fn print_model_current(config: &Config) {
    println!();
    println!("{} Active Model", "━━━ Model ━━━".bold());
    println!("  {} {}", "Name:".dimmed(), config.model.name.bright_cyan());
    println!("  {} {}", "Path:".dimmed(), config.model.path);
    println!("  {} {}", "Context:".dimmed(), config.model.n_ctx);
    println!("  {} {}", "GPU layers:".dimmed(), config.model.n_gpu_layers);
    println!("  {} {}", "Template:".dimmed(), config.model.chat_template);
    println!();
}

fn read_line_prompt(prompt: &str) -> String {
    use std::io::Write;
    print!("{}", prompt);
    std::io::stdout().flush().ok();
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf).ok();
    buf.trim().to_string()
}

fn run_setup_wizard(config: &mut Config, catalog: &ModelsCatalog) -> Result<()> {
    println!();
    println!(
        "{} Welcome to memvid-agent-core!",
        "━━━ Setup ━━━".bold().bright_green()
    );
    println!("  First-time setup — let's get you configured.");
    println!();

    let data_dir = read_line_prompt(&format!(
        "{} Data directory [{}]: ",
        "•".dimmed(),
        config.data_dir.display()
    ));
    if !data_dir.is_empty() {
        config.data_dir = std::path::PathBuf::from(&data_dir);
    }

    println!();
    println!("  {} Available models:", "•".dimmed());
    for (i, entry) in catalog.list().iter().enumerate() {
        println!(
            "     {}. {:<16} {} — {}",
            i + 1,
            entry.id.bright_cyan(),
            entry.name,
            entry.description
        );
    }
    let model_choice = read_line_prompt(&format!("{} Select model [1]: ", "•".dimmed()));
    let model_idx: usize = model_choice.parse().unwrap_or(1);
    if model_idx > 0 && model_idx <= catalog.list().len() {
        let entry = &catalog.list()[model_idx - 1];
        let models_dir = std::path::Path::new("models");
        if models_catalog::download_model(entry, models_dir).is_ok() {
            let model_path = models_dir.join(&entry.id).with_extension("gguf");
            models_catalog::apply_model_to_config(&model_path, entry, config)?;
            println!("  {} Selected model: {}", "✓".green(), entry.name.bold());
        } else {
            println!(
                "  {} Model download deferred — will download on next launch.",
                "i".yellow()
            );
        }
    }

    let install_langs = read_line_prompt(&format!(
        "{} Install language documentation? (y/N): ",
        "•".dimmed()
    ));
    if install_langs.to_lowercase() == "y" {
        match LanguagesCatalog::load_or_fetch(&config.data_dir) {
            Ok(lang_catalog) => {
                println!("  {} Fetching language catalog…", "↳".dimmed());
                for entry in lang_catalog.list().iter().take(5) {
                    println!("     • {}", entry.key.bright_cyan());
                }
                if lang_catalog.len() > 5 {
                    println!(
                        "     ... and {} more (install later with /learn)",
                        lang_catalog.len() - 5
                    );
                }
                let lang_choice =
                    read_line_prompt("  Enter language key (or leave blank to skip): ");
                if !lang_choice.is_empty() && lang_catalog.find(&lang_choice).is_some() {
                    println!(
                        "  {} Language '{}' will be installed on next /learn",
                        "↳".dimmed(),
                        lang_choice
                    );
                }
            }
            Err(e) => {
                println!("  {} Could not fetch language catalog: {}", "✗".red(), e);
            }
        }
    }

    let enable_api = read_line_prompt(&format!("{} Enable API server? (y/N): ", "•".dimmed()));
    if enable_api.to_lowercase() == "y" {
        config.api.enabled = true;
        let port = read_line_prompt(&format!(
            "{} API port [{}]: ",
            "•".dimmed(),
            config.api.port
        ));
        if !port.is_empty() {
            if let Ok(p) = port.parse() {
                config.api.port = p;
            }
        }
        config.api.token = Some(uuid::Uuid::new_v4().to_string());
        println!(
            "  {} API token generated: {}",
            "✓".green(),
            config.api.token.as_ref().unwrap()
        );
    }

    config.save()?;
    println!();
    println!(
        "{} Setup complete! Configuration saved.",
        "✓".green().bold()
    );
    println!();
    Ok(())
}
