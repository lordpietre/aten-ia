use anyhow::Result;
use colored::*;
use memvid_agent_core::agent::Agent;
use memvid_agent_core::api::ApiServer;
use memvid_agent_core::books_catalog::BooksCatalog;
use memvid_agent_core::config::Config;
use memvid_agent_core::finetune::{self, FinetuneOutcome, FinetunePlan};
use memvid_agent_core::languages_catalog::LanguagesCatalog;
use memvid_agent_core::models;
use memvid_agent_core::models_catalog::{self, ModelsCatalog};
use memvid_agent_core::queue::FeedQueue;
use memvid_agent_core::shutdown;
use memvid_agent_core::utils::FileLock;
use std::io::Write;
use std::sync::{Arc, Mutex};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "--version" | "-V" => {
                println!("aten-ia {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--self-test" | "--verify" => {
                memvid_agent_core::selfcheck::run()?;
                return Ok(());
            }
            "--purge-data" => {
                let yes = args.iter().any(|a| a == "--yes");
                let cwd = std::env::current_dir()?;
                memvid_agent_core::purge::run(&cwd, yes)?;
                return Ok(());
            }
            _ => {}
        }
    }

    dotenvy::dotenv().ok();
    // Keep the terminal clean: dependency crates (tantivy, memvid, rustls…)
    // log at INFO by default and flood the UI. Only warnings/errors are shown
    // unless ATEN_LOG explicitly requests another level.
    let log_level = std::env::var("ATEN_LOG")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(tracing::Level::WARN);
    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(false)
        .init();
    shutdown::install_handlers();

    let config_path = std::path::Path::new("config.json");
    let is_first_run = !config_path.exists();

    let mut config = Config::load_or_create()?;
    config.validate()?;

    let catalog = ModelsCatalog::load();

    if is_first_run {
        run_setup_wizard(&mut config, &catalog)?;
    }

    models::ensure_model(&config.model)?;

    let _lock = match FileLock::acquire(&config.data_dir) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!(
                "{} No se pudo bloquear el directorio de datos ({}).\n  Puede haber otra instancia de aten-ia en ejecución. \
                Si no es así, borra {} e inténtalo de nuevo.",
                "✗".red(),
                e,
                config.data_dir.join(".lock").display()
            );
            std::process::exit(1);
        }
    };

    let spinner = indicatif::ProgressBar::new_spinner();
    spinner.set_style(
        indicatif::ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .expect("Invalid spinner template"),
    );
    spinner.set_message("Cargando modelo…");
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));
    let agent = Arc::new(Mutex::new(Agent::init(&config)?));
    spinner.finish_and_clear();

    let mut loaded_files: Vec<String> = Vec::new();

    {
        let a = agent.lock().unwrap();
        print_banner();
        eprintln!(
            "  {} {}",
            "Modelo   ".dimmed(),
            config.model.name.bright_cyan()
        );
        eprintln!("  {} {} tokens", "Contexto ".dimmed(), config.model.n_ctx);
        eprintln!("  {} {}", "Datos    ".dimmed(), config.data_dir.display());
        eprintln!("  {} {}", "Memoria  ".dimmed(), a.memory_summary()?);
        if is_first_run {
            eprintln!("  {} Configuración inicial completada.", "↳".dimmed());
        }
        print_startup_help();
        eprintln!();
    }

    loop {
        if shutdown::is_shutdown_requested() {
            break;
        }
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

        if input_lower == "/debug" {
            {
                let mut a = agent.lock().unwrap();
                let new_state = !a.is_debug();
                a.set_debug(new_state);
                let state_str = if new_state {
                    "ON".bright_green()
                } else {
                    "OFF".dimmed()
                };
                println!("{} Debug mode {}", "↳".dimmed(), state_str);
                if new_state && a.get_last_rag_debug().is_empty() {
                    println!("{} Ejecuta una consulta para ver el RAG", "↳".dimmed());
                } else if !a.get_last_rag_debug().is_empty() {
                    print_rag_debug(a.get_last_rag_debug());
                }
            }
            continue;
        }

        if input_lower == "/kv" || input_lower.starts_with("/kv ") {
            let args: Vec<&str> = input.split_whitespace().skip(1).collect();
            if args.is_empty() {
                println!(
                    "{} Caché KV: K={} / V={}",
                    "↳".dimmed(),
                    config.model.kv_type_k,
                    config.model.kv_type_v
                );
                println!(
                    "  uso: /kv <tipo_k> <tipo_v>  (f16 f32 bf16 q8_0 q4_0 turbo2 turbo3 turbo4)"
                );
                continue;
            }
            if args.len() != 2 {
                eprintln!("{} uso: /kv <tipo_k> <tipo_v>", "✗".red());
                continue;
            }
            let (k, v) = (args[0], args[1]);
            let invalid: Vec<&str> = [k, v]
                .into_iter()
                .filter(|t| !memvid_agent_core::llama::context::is_valid_kv_cache_type(t))
                .collect();
            if !invalid.is_empty() {
                eprintln!(
                    "{} tipo(s) de caché KV desconocidos: {}. Válidos: f16 f32 bf16 q8_0 q4_0 turbo2 turbo3 turbo4",
                    "✗".red(),
                    invalid.join(", ")
                );
                continue;
            }

            config.model.kv_type_k = k.to_string();
            config.model.kv_type_v = v.to_string();
            if let Err(e) = config.save() {
                eprintln!("{} No se pudo guardar la configuración: {:#}", "✗".red(), e);
                continue;
            }

            // Re-initialize the context with the new KV cache types.
            let switch = {
                let mut a = agent.lock().unwrap();
                a.switch_model(
                    &config.model.path,
                    config.model.n_ctx,
                    config.model.n_gpu_layers,
                    &config.model.kv_type_k,
                    &config.model.kv_type_v,
                    &config.model.name,
                    &config.model.chat_template,
                    config.generation.top_k,
                    config.generation.top_p,
                    config.generation.temp,
                )
            };
            match switch {
                Ok(()) => println!(
                    "{} Caché KV establecida en K={} / V={} (contexto recargado)",
                    "✓".green(),
                    config.model.kv_type_k,
                    config.model.kv_type_v
                ),
                Err(e) => eprintln!(
                    "{} Saved to config, but context reload failed: {:#}",
                    "✗".red(),
                    e
                ),
            }
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
            print_models_table(&catalog, &config.model.name, true);
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
                        "{} Modelo desconocido '{}'. Usa /models para ver los disponibles.",
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
                    spinner.set_message("Cargando nuevo modelo…");
                    spinner.enable_steady_tick(std::time::Duration::from_millis(80));
                    {
                        let mut a = agent.lock().unwrap();
                        a.switch_model(
                            &config.model.path,
                            config.model.n_ctx,
                            config.model.n_gpu_layers,
                            &config.model.kv_type_k,
                            &config.model.kv_type_v,
                            &config.model.name,
                            &config.model.chat_template,
                            config.generation.top_k,
                            config.generation.top_p,
                            config.generation.temp,
                        )?;
                    }
                    spinner.finish_and_clear();
                    println!(
                        "{} Modelo cambiado a: {} ({})",
                        "✓".green(),
                        entry.name.bold(),
                        config.model.n_ctx
                    );
                }
                Err(e) => {
                    eprintln!("{} No se pudo descargar el modelo: {:#}", "✗".red(), e);
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
            spinner.set_message("Obteniendo catálogo de lenguajes…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));
            let lang_catalog = LanguagesCatalog::load_or_fetch(&config.data_dir)?;
            spinner.finish_and_clear();

            println!(
                "{}",
                format!("━━━ Lenguajes disponibles ({}) ━━━", lang_catalog.len()).bold()
            );
            for entry in lang_catalog.list().iter().take(30) {
                let installed = if config.languages.installed.contains(&entry.key) {
                    " ◄ instalado".green().to_string()
                } else {
                    String::new()
                };
                println!(
                    "  {:<20} {} recursos{}",
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
            spinner.set_message("Obteniendo catálogo de libros…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));
            let catalog = match BooksCatalog::fetch() {
                Ok(c) => c,
                Err(e) => {
                    spinner.finish_and_clear();
                    eprintln!(
                        "{} No se pudo obtener el catálogo de libros: {}",
                        "✗".red(),
                        e
                    );
                    continue;
                }
            };
            spinner.finish_and_clear();

            let langs = catalog.list_languages();
            println!(
                "{}",
                format!("━━━ Libros gratuitos — Lenguajes ({}) ━━━", langs.len()).bold()
            );
            for lang in langs.iter().take(60) {
                println!("  • {}", lang.bright_cyan());
            }
            if langs.len() > 60 {
                println!("  ... y {} más", langs.len() - 60);
            }
            println!("Usa /download-books <lenguaje> [límite] para descargar e indexar.");
            continue;
        }

        if input_lower == "/languages-installed" || input_lower == "/lang-installed" {
            if config.languages.installed.is_empty() {
                println!("  No hay lenguajes instalados. Usa /learn <lang> para instalar uno.");
            } else {
                println!("{}", "━━━ Lenguajes instalados ━━━".bold());
                for lang in &config.languages.installed {
                    println!("  • {}", lang.bright_cyan());
                }
            }
            continue;
        }

        if let Some(lang_id) = input.strip_prefix("/learn ") {
            let lang_id = lang_id.trim().to_string();
            if lang_id.is_empty() {
                eprintln!("{} Uso: /learn <language>", "✗".red());
                continue;
            }

            let lang_catalog = LanguagesCatalog::load_or_fetch(&config.data_dir)?;
            let lang = match lang_catalog.find(&lang_id) {
                Some(l) => l,
                None => {
                    eprintln!(
                        "{} Lenguaje desconocido '{}'. Usa /languages para ver los disponibles.",
                        "✗".red(),
                        lang_id
                    );
                    continue;
                }
            };

            let max_resources = std::cmp::min(lang.resources.len(), 10);
            let entries = memvid_agent_core::languages_catalog::download_language_resources(
                lang,
                max_resources,
            )?;

            if entries.is_empty() {
                eprintln!(
                    "{} No se descargaron recursos para {}.",
                    "✗".red(),
                    lang.name
                );
                continue;
            }

            let spinner = indicatif::ProgressBar::new_spinner();
            spinner.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Invalid spinner template"),
            );
            spinner.set_message("Indexando…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));

            let indexed = {
                let mut a = agent.lock().unwrap();
                let mut added = 0usize;
                for entry in &entries {
                    if a.store_knowledge_dedup(&entry.source, &entry.content)? {
                        added += 1;
                    }
                }
                added
            };
            spinner.finish_and_clear();

            config.languages.mark_installed(&lang.key);
            config.save()?;

            println!(
                "{} Instalado {} — {} fragmentos indexados como conocimiento ({} ya presentes).",
                "✓".green(),
                lang.name.bold(),
                indexed,
                entries.len().saturating_sub(indexed)
            );

            // Auto-offer a fine-tune of the active model on what we just learned.
            let lang_key = lang.key.clone();
            let lang_name = lang.name.clone();
            run_finetune_flow(&lang_key, &lang_name, true, &mut config, &agent)?;
            continue;
        }

        if let Some(lang_key) = input.strip_prefix("/unlearn ") {
            let lang_key = lang_key.trim();
            if lang_key.is_empty() {
                eprintln!("{} Uso: /unlearn <language>", "✗".red());
                continue;
            }

            let removed = {
                let mut a = agent.lock().unwrap();
                a.unlearn_language(lang_key)?
            };
            config.languages.mark_uninstalled(lang_key);
            config.save()?;

            println!(
                "{} Olvidado '{}' — {} entradas de conocimiento eliminadas.",
                "✓".green(),
                lang_key.bold(),
                removed
            );
            continue;
        }

        if let Some(lang_id) = input.strip_prefix("/finetune ") {
            let lang_id = lang_id.trim().to_string();
            if lang_id.is_empty() {
                eprintln!("{} Uso: /finetune <language>", "✗".red());
                continue;
            }
            // Resolve the canonical key + display name (same source as /learn).
            let lang_catalog = LanguagesCatalog::load_or_fetch(&config.data_dir)?;
            let (lang_key, lang_name) = match lang_catalog.find(&lang_id) {
                Some(l) => (l.key.clone(), l.name.clone()),
                None => {
                    // Fall back to the raw id so a user can still target an
                    // already-installed key even if the catalog is offline.
                    (lang_id.to_lowercase().replace(' ', "-"), lang_id.clone())
                }
            };
            run_finetune_flow(&lang_key, &lang_name, false, &mut config, &agent)?;
            continue;
        }

        if input_lower == "/reindex" {
            let spinner = indicatif::ProgressBar::new_spinner();
            spinner.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Invalid spinner template"),
            );
            spinner.set_message("Reconstruyendo índice desde los .mv2…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));
            {
                let mut a = agent.lock().unwrap();
                a.reindex_from_mv2(&config.data_dir)?;
            }
            spinner.finish_and_clear();
            println!(
                "{} Reindexación completada. {} knowledge entradas indexadas.",
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

            println!();
            println!(
                "{} Servidor API escuchando en http://{}:{}",
                "●".bright_green(),
                host,
                port
            );
            if let Some(ref t) = token {
                println!("{} Token: {}", "  clave".dimmed(), t.bright_yellow());
                println!("{} Usa: Authorization: Bearer {}", "  auth".dimmed(), t);
            } else {
                println!("{} Sin autenticación configurada", "  auth".dimmed());
            }

            println!();
            println!("{}", "━━━ Configuración de clientes ━━━".bold());
            println!();
            println!("  1. OpenCode     - asistente de código (opencode.ai)");
            println!("  2. VS Code      - con la extensión Continue");
            println!("  3. Omitir       - sin instrucciones");
            println!();

            loop {
                print!("{} Elige cliente (1/2/3): ", "?".yellow());
                std::io::stdout().flush().ok();
                let mut choice = String::new();
                if std::io::stdin().read_line(&mut choice).is_err() {
                    break;
                }
                let choice = choice.trim();

                match choice {
                    "1" => {
                        let config_content = format!(
                            r#"{{
  "$schema": "https://opencode.ai/config.json",
  "model": "openai/{model}",
  "provider": {{
    "openai": {{
      "options": {{
        "baseURL": "http://localhost:{port}/v1",
        "apiKey": "{token}"
      }}
    }}
  }},
  "agent": {{
    "aten-ia": {{
      "description": "Agente local de aten-ia — conoce toda la codebase y arquitectura del proyecto. Usa el LLM local para responder preguntas sobre el código, RAG, persistencia, y más.",
      "mode": "primary",
      "model": "openai/{model}",
      "instructions": ["AGENTS.md"]
    }}
  }},
  "default_agent": "aten-ia"
}}"#,
                            model = model_name,
                            port = port,
                            token = token.as_ref().unwrap()
                        );

                        let write_config = |path: &std::path::Path| -> Result<()> {
                            if let Some(parent) = path.parent() {
                                std::fs::create_dir_all(parent).ok();
                            }
                            memvid_agent_core::utils::atomic_write(path, &config_content)?;
                            println!(
                                "{} Configuración de OpenCode escrita en {}",
                                "✓".green(),
                                path.display()
                            );
                            Ok(())
                        };

                        // Global config
                        let global_path = std::env::var("HOME")
                            .map(|h| {
                                std::path::PathBuf::from(h).join(".config/opencode/opencode.json")
                            })
                            .unwrap_or_else(|_| std::path::PathBuf::from(".opencode.json"));
                        if let Err(e) = write_config(&global_path) {
                            println!(
                                "{} No se pudo escribir la configuración global: {}",
                                "✗".red(),
                                e
                            );
                        }

                        // Project-level config (parent of CWD, i.e. project root)
                        if let Ok(cwd) = std::env::current_dir() {
                            if let Some(parent) = cwd.parent() {
                                let project_path = parent.join("opencode.json");
                                let should_write = if project_path.exists() {
                                    true
                                } else {
                                    let ans = read_line_prompt(&format!(
                                        "{} ¿Escribir configuración del proyecto en {}? (S/n): ",
                                        "?".yellow(),
                                        project_path.display()
                                    ));
                                    ans.is_empty() || ans.to_lowercase() == "y"
                                };
                                if should_write {
                                    if let Err(e) = write_config(&project_path) {
                                        println!(
                                            "{} No se pudo escribir la configuración del proyecto: {}",
                                            "✗".red(),
                                            e
                                        );
                                    }
                                }
                            }
                        }

                        println!("  {} Reinicia opencode tras crear el archivo", "↳".dimmed());
                        break;
                    }
                    "2" => {
                        println!();
                        println!("{} Configuración de VS Code", "━━━".dimmed());
                        println!();
                        println!("  1. Instala la extensión 'Continue' en VS Code");
                        println!("  2. Ve a Ajustes → Extensiones → Continue");
                        println!("  3. Configura:");
                        println!();
                        let config_lines = format!(
                            r#"{{
  "openai-compatible": {{
    "api_base": "http://localhost:{port}/v1",
    "api_key": "{token}",
    "title": "aten-ia"
  }}
}}"#,
                            port = port,
                            token = token.as_ref().unwrap()
                        );
                        for line in config_lines.lines() {
                            println!("    {}", line);
                        }
                        println!();
                        println!(
                            "  {} O usa config.json en la carpeta .continue",
                            "↳".dimmed()
                        );
                        break;
                    }
                    "3" | "" => {
                        println!("  {} Omitido", "↳".dimmed());
                        break;
                    }
                    _ => {
                        println!("  {} Opción no válida, inténtalo de nuevo", "✗".red());
                    }
                }
            }
            println!();

            std::thread::spawn(move || {
                let server = ApiServer::new(api_agent, model_name, host, port, token);
                if let Err(e) = server.run() {
                    eprintln!("[api] Error del servidor: {}", e);
                }
            });

            println!(
                "{} La API se está ejecutando en segundo plano.",
                "✓".green()
            );
            continue;
        }

        if input_lower.starts_with("/search ") {
            let raw = input
                .strip_prefix("/search ")
                .or_else(|| input.strip_prefix("/SEARCH "))
                .unwrap_or("")
                .trim();
            if raw.is_empty() {
                eprintln!("{} Uso: /search <query> [from:<source>]", "✗".red());
                continue;
            }
            let (query, source_filter) = if let Some(pos) = raw.find(" from:") {
                let q = raw[..pos].trim().to_string();
                let filter = raw[pos + 6..].trim().to_string();
                (q, Some(filter))
            } else if let Some(pos) = raw.find(" FROM:") {
                let q = raw[..pos].trim().to_string();
                let filter = raw[pos + 6..].trim().to_string();
                (q, Some(filter))
            } else {
                (raw.to_string(), None)
            };

            if query.is_empty() {
                eprintln!("{} Uso: /search <query> [from:<source>]", "✗".red());
                continue;
            }

            let a = agent.lock().unwrap();
            let results = a.search_knowledge_filtered(&query, 5, source_filter.as_deref());
            if results.is_empty() {
                println!("  No se encontró conocimiento coincidente.");
            } else {
                let display_query = if let Some(ref f) = source_filter {
                    format!("{} (fuente: {})", query, f)
                } else {
                    query.clone()
                };
                println!(
                    "{} Resultados de búsqueda para '{}':",
                    "━━━".bold(),
                    display_query.bold()
                );
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
                        "{} Cargado {} ({} bytes)",
                        "✓".green(),
                        filename.bold(),
                        content.len()
                    );
                }
                Err(e) => eprintln!("{} Error al leer {}: {}", "✗".red(), path, e),
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
                        let title = extracted.title.as_deref().unwrap_or("sin título");
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
                        spinner.set_message("Indexando…");
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
                    Err(e) => eprintln!("{} Error al leer {}: {}", "✗".red(), path, e),
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
                    let title = extracted.title.as_deref().unwrap_or("sin título");
                    let preview: String = extracted.content.chars().take(200).collect();
                    println!(
                        "{} Ingested: {} ({} format, {} chars)",
                        "✓".green(),
                        title.bold(),
                        extracted.format,
                        extracted.content.len()
                    );
                    println!("  {} vista previa: {}", "↳".dimmed(), preview.dimmed());
                }
                Err(e) => eprintln!("{} No se pudo indexar: {:#}", "✗".red(), e),
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
                eprintln!("{} Uso: /fetch <url>", "✗".red());
                continue;
            }

            let spinner = indicatif::ProgressBar::new_spinner();
            spinner.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Invalid spinner template"),
            );
            spinner.set_message("Descargando…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));

            let result = {
                let mut a = agent.lock().unwrap();
                a.fetch_and_ingest(url, &config.ingestion)
            };

            spinner.finish_and_clear();

            match result {
                Ok((content, indexed)) => {
                    let title = content.title.as_deref().unwrap_or("sin título");
                    let preview: String = content.content.chars().take(200).collect();
                    println!(
                        "{} Descargado: {} ({})",
                        "✓".green(),
                        title.bold(),
                        url.dimmed()
                    );
                    println!("  {} tipo: {}", "↳".dimmed(), content.content_type);
                    println!("  {} tamaño: {} bytes", "↳".dimmed(), content.size_bytes);
                    println!("  {} fragmentos indexados: {}", "↳".dimmed(), indexed);
                    println!("  {} vista previa: {}", "↳".dimmed(), preview.dimmed());
                }
                Err(e) => {
                    eprintln!("{} No se pudo descargar: {:#}", "✗".red(), e);
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
                eprintln!("{} Uso: /fetch-md <url>", "✗".red());
                continue;
            }

            let spinner = indicatif::ProgressBar::new_spinner();
            spinner.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Invalid spinner template"),
            );
            spinner.set_message("Descargando…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));

            // No agent lock here: /fetch-md only fetches and prints. Locking the
            // agent during network I/O would needlessly block other operations.
            let result = {
                let mut fetcher =
                    memvid_agent_core::web_fetcher::WebFetcher::new(&config.ingestion);
                fetcher.fetch_and_retry(url)
            };

            spinner.finish_and_clear();

            match result {
                Ok(content) => {
                    let md = if content.content_type.contains("html") {
                        let html = match content.content.contains('<') {
                            true => {
                                memvid_agent_core::extractor::html_to_markdown(&content.content)
                            }
                            false => content.content.clone(),
                        };
                        html
                    } else {
                        content.content.clone()
                    };
                    let title = content.title.as_deref().unwrap_or("sin título");
                    println!(
                        "{} {} — Markdown ({} caracteres)",
                        "━━━".bold(),
                        title.bold(),
                        md.len()
                    );
                    println!("{}", md.dimmed());
                }
                Err(e) => {
                    eprintln!("{} No se pudo descargar: {:#}", "✗".red(), e);
                }
            }
            continue;
        }

        if input_lower.starts_with("/feed ") {
            let url = input
                .strip_prefix("/feed ")
                .or_else(|| input.strip_prefix("/FEED "))
                .unwrap_or("")
                .trim();
            if url.is_empty() {
                eprintln!("{} Uso: /feed <url>", "✗".red());
                continue;
            }

            let spinner = indicatif::ProgressBar::new_spinner();
            spinner.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Invalid spinner template"),
            );
            spinner.set_message("Descargando feed…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));

            let result = {
                let mut a = agent.lock().unwrap();
                a.fetch_and_ingest_feed(url, &config.ingestion, 20)
            };

            spinner.finish_and_clear();

            match result {
                Ok(stats) => {
                    println!(
                        "{} Feed: {}",
                        "✓".green(),
                        stats.feed_title.as_deref().unwrap_or(url).bold()
                    );
                    println!(
                        "  {} entradas encontradas: {}",
                        "↳".dimmed(),
                        stats.entries_found
                    );
                    println!(
                        "  {} entradas indexadas: {}",
                        "↳".dimmed(),
                        stats.entries_indexed
                    );
                    if !stats.failures.is_empty() {
                        println!("  {} fallos:", "↳".dimmed());
                        for f in &stats.failures {
                            eprintln!("    {} {}", "✗".red(), f);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("{} No se pudo descargar feed: {:#}", "✗".red(), e);
                }
            }
            continue;
        }

        if input_lower == "/queue" {
            let queue = FeedQueue::new(&config.data_dir);
            let stats = queue.stats();
            if queue.len() == 0 {
                println!("{} La cola está vacía.", "ℹ".cyan());
            } else {
                println!("{} Cola de feeds ({} en total):", "ℹ".cyan(), queue.len());
                println!("  {} pendientes:   {}", "↳".dimmed(), stats.pending);
                println!("  {} en proceso:  {}", "↳".dimmed(), stats.processing);
                println!("  {} completadas: {}", "↳".dimmed(), stats.done);
                println!("  {} fallidas:    {}", "↳".dimmed(), stats.failed);
            }
            continue;
        }

        if input_lower.starts_with("/queue-add ") {
            let url = input
                .strip_prefix("/queue-add ")
                .or_else(|| input.strip_prefix("/QUEUE-ADD "))
                .unwrap_or("")
                .trim();
            if url.is_empty() {
                eprintln!("{} Uso: /queue-add <url>", "✗".red());
                continue;
            }
            let mut queue = FeedQueue::new(&config.data_dir);
            match queue.add(url) {
                Ok(()) => println!("{} Añadido a la cola: {}", "✓".green(), url.dimmed()),
                Err(e) => eprintln!("{} No se pudo añadir: {:#}", "✗".red(), e),
            }
            continue;
        }

        if input_lower == "/queue-process" || input_lower == "/QUEUE-PROCESS" {
            let spinner = indicatif::ProgressBar::new_spinner();
            spinner.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Invalid spinner template"),
            );
            spinner.set_message("Procesando cola…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));

            let recovered = {
                let mut q = FeedQueue::new(&config.data_dir);
                q.reset_stale_processing()?
            };
            if recovered > 0 {
                eprintln!(
                    "{} Recovered {} queue entr{} from a previous run.",
                    "i".cyan(),
                    recovered,
                    if recovered == 1 { "y" } else { "ies" }
                );
            }

            let pending: Vec<(String, String)> = {
                let q = FeedQueue::new(&config.data_dir);
                q.pending()
                    .into_iter()
                    .map(|e| (e.id.clone(), e.url.clone()))
                    .collect()
            };
            let total = pending.len();

            if total == 0 {
                spinner.finish_and_clear();
                println!("{} No hay URLs pendientes en la cola.", "ℹ".cyan());
                continue;
            }

            let mut successes = 0usize;
            let mut failures = 0usize;

            for (id, url) in &pending {
                {
                    let mut q = FeedQueue::new(&config.data_dir);
                    q.mark_processing(id)?;
                }

                let result = {
                    let mut a = agent.lock().unwrap();
                    a.fetch_and_ingest(url, &config.ingestion)
                };

                let mut q = FeedQueue::new(&config.data_dir);
                match result {
                    Ok(_) => {
                        q.mark_done(id)?;
                        successes += 1;
                    }
                    Err(e) => {
                        q.mark_failed(id, &format!("{:#}", e))?;
                        failures += 1;
                    }
                }
            }

            spinner.finish_and_clear();

            println!(
                "{} Cola procesada: {} correctas, {} fallidas (de {} en total)",
                "✓".green(),
                successes,
                failures,
                total
            );
            continue;
        }

        if input_lower.starts_with("/batch ") {
            let path = input
                .strip_prefix("/batch ")
                .or_else(|| input.strip_prefix("/BATCH "))
                .unwrap_or("")
                .trim();
            if path.is_empty() {
                eprintln!("{} Uso: /batch <file>", "✗".red());
                continue;
            }

            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{} Error al leer {}: {}", "✗".red(), path, e);
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
                eprintln!("{} No se encontraron URLs en {}", "✗".red(), path);
                continue;
            }

            println!(
                "{} Procesando {} URLs desde {}…",
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
                a.process_url_batch_with_progress(&urls, &config.ingestion, || pb.inc(1))
            };

            pb.finish_and_clear();

            match result {
                Ok(stats) => {
                    println!(
                        "{} Lote completado: {} correctas, {} fallidas, ~{} fragmentos",
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
                    eprintln!("{} Lote fallido: {:#}", "✗".red(), e);
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
                eprintln!("{} Uso: /download-books <lenguaje> [límite]", "✗".red());
                continue;
            }

            let mut parts = rest.split_whitespace();
            let lang = match parts.next() {
                Some(l) => l,
                None => {
                    eprintln!("{} Uso: /download-books <lenguaje> [límite]", "✗".red());
                    continue;
                }
            };
            let limit: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(10);

            println!(
                "{} Descargando e indexando hasta {} recursos para '{}'…",
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
            spinner.set_message("Procesando…");
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
                        eprintln!("{} No se pudo guardar la configuración: {}", "✗".red(), e);
                    }
                    println!(
                        "{} Ingested {} resource(s) for '{}'.",
                        "✓".green(),
                        count,
                        lang.bold()
                    );
                }
                Err(e) => {
                    eprintln!(
                        "{} No se pudo descargar/indexar los libros: {:#}",
                        "✗".red(),
                        e
                    );
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
        spinner.set_message("Pensando…");
        spinner.enable_steady_tick(std::time::Duration::from_millis(80));

        let response = {
            let mut a = agent.lock().unwrap();
            a.chat(&input)
        };

        match response {
            Ok(response) => {
                spinner.finish_and_clear();
                println!("{}", response.bright_cyan());
                if let Ok(a) = agent.lock() {
                    if !a.get_last_rag_debug().is_empty() {
                        print_rag_debug(a.get_last_rag_debug());
                    }
                }
            }
            Err(e) => {
                spinner.finish_and_clear();
                eprintln!("{} Error: {:#}", "✗".red(), e);
            }
        }
    }

    eprintln!();
    if shutdown::is_shutdown_requested() {
        eprintln!("{} Shutting down aten-ia…", "●".bright_yellow());
    } else {
        eprintln!("{} ¡Hasta luego!", "●".bright_green());
    }
    Ok(())
}

fn read_line() -> Option<String> {
    use std::io::Write;
    use std::sync::mpsc;

    let prompt = format!("{} ", ">".green());
    print!("{}", prompt);
    std::io::stdout().flush().ok()?;

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let result = std::io::stdin().read_line(&mut buf);
        let _ = tx.send(result.map(|n| (n, buf)));
    });

    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(Ok((0, _))) => return None,
            Ok(Ok((_, buf))) => return Some(buf),
            Ok(Err(_)) => return None,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if shutdown::is_shutdown_requested() {
                    return None;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
}

fn print_startup_help() {
    eprintln!();
    eprintln!("{}", "━━━ Inicio rápido ━━━".bold());
    eprintln!("  {:<20}  Chatea con el agente", "<mensaje>".dimmed());
    eprintln!(
        "  {:<20}  Lista y cambia de modelo",
        "/models | /model <id>".dimmed()
    );
    eprintln!("  {:<20}  Muestra todos los comandos", "/help".dimmed());
    eprintln!("  {:<20}  Salir", "/exit".dimmed());
}

fn print_help() {
    println!();
    println!("{}", "━━━ Comandos ━━━".bold());
    println!("  {:<20}  Chatea con el agente", "<mensaje>".dimmed());
    println!(
        "  {:<20}  Lista los modelos disponibles",
        "/models".bright_blue()
    );
    println!(
        "  {:<20}  Muestra, descarga o cambia de modelo",
        "/model [id|current]".bright_blue()
    );
    println!(
        "  {:<20}  Lista los lenguajes disponibles",
        "/languages".bright_blue()
    );
    println!(
        "  {:<20}  Lista los lenguajes instalados",
        "/languages-installed".bright_blue()
    );
    println!(
        "  {:<20}  Lista lenguajes con libros gratuitos",
        "/books".bright_blue()
    );
    println!(
        "  {:<20}  Descarga e indexa libros de un lenguaje",
        "/download-books <lang> [limit]".bright_blue()
    );
    println!(
        "  {:<20}  Descarga documentación de un lenguaje",
        "/learn <lang>".bright_blue()
    );
    println!(
        "  {:<20}  Elimina la documentación de un lenguaje",
        "/unlearn <lang>".bright_blue()
    );
    println!(
        "  {:<20}  Ajusta el modelo con un lenguaje aprendido",
        "/finetune <lang>".bright_blue()
    );
    println!(
        "  {:<20}  Descarga una URL y la indexa",
        "/fetch <url>".bright_blue()
    );
    println!(
        "  {:<20}  Descarga una URL y la muestra en Markdown",
        "/fetch-md <url>".bright_blue()
    );
    println!(
        "  {:<20}  Procesa un lote de URLs desde archivo",
        "/batch <file>".bright_blue()
    );
    println!(
        "  {:<20}  Carga un archivo de texto en la sesión",
        "/load <file>".bright_blue()
    );
    println!(
        "  {:<20}  Carga e indexa un archivo (PDF/EPUB/MD/HTML/TXT)",
        "/ingest <file>".bright_blue()
    );
    println!(
        "  {:<20}  Extrae e indexa un PDF/EPUB",
        "/ingest-pdf <file>".bright_blue()
    );
    println!(
        "  {:<20}  Busca en el conocimiento indexado",
        "/search <query>".bright_blue()
    );
    println!(
        "  {:<20}  Reconstruye el índice desde los .mv2",
        "/reindex".bright_blue()
    );
    println!(
        "  {:<20}  Inicia la API o muestra el token",
        "/token".bright_blue()
    );
    println!(
        "  {:<20}  Muestra el historial de conversaciones",
        "/history".bright_blue()
    );
    println!(
        "  {:<20}  Muestra estadísticas del agente",
        "/stats".bright_blue()
    );
    println!(
        "  {:<20}  Muestra la configuración actual",
        "/config".bright_blue()
    );
    println!(
        "  {:<20}  Muestra el RAG de la última consulta",
        "/debug".bright_blue()
    );
    println!(
        "  {:<20}  Consulta o cambia la caché KV (ej. /kv f16 turbo3)",
        "/kv [k] [v]".bright_blue()
    );
    println!("  {:<20}  Muestra esta ayuda", "/help".bright_blue());
    println!("  {:<20}  Sale del agente", "/exit".bright_blue());
    println!();
    println!(
        "{} Los comandos también aceptan mayúsculas: /MODELS, /MODEL, /LANGUAGES, /SEARCH, /REINDEX, /TOKEN, /LOAD, /INGEST",
        "↳".dimmed()
    );
    println!();
}

fn print_models_table(catalog: &ModelsCatalog, active_name: &str, show_desc: bool) {
    println!();
    println!(
        "{}",
        format!("━━━ Modelos disponibles ({}) ━━━", catalog.list().len()).bold()
    );
    println!();
    println!(
        "  {:<3} {:<19} {:<28} {:>8} {:>8}  {}",
        "#".dimmed(),
        "ID".dimmed(),
        "Modelo".dimmed(),
        "Tamaño".dimmed(),
        "Contexto".dimmed(),
        "Estado".dimmed()
    );
    println!("  {}", "─".repeat(80).dimmed());
    for (i, entry) in catalog.list().iter().enumerate() {
        let id = format!("{:<19}", entry.id);
        let size = format!("{} MB", entry.size_mb);
        let active = if entry.name == active_name {
            "◄ activo".green().to_string()
        } else {
            String::new()
        };
        println!(
            "  {:<3} {} {:<28} {:>8} {:>8}  {}",
            format!("{}.", i + 1).bright_cyan(),
            id.bright_cyan(),
            entry.name,
            size,
            entry.n_ctx_recommended,
            active
        );
        if show_desc {
            println!("      {}", entry.description.dimmed());
        }
    }
    println!();
    if !show_desc {
        println!("  {} Usa el número o el ID con /model <id>.", "↳".dimmed());
        println!();
    }
}

fn print_banner() {
    eprintln!();
    eprintln!(
        "  {}  {}",
        "aten-ia".bold().bright_cyan(),
        format!("v{}", env!("CARGO_PKG_VERSION")).dimmed()
    );
    eprintln!(
        "  {}",
        "IA local · memoria persistente · RAG · API OpenAI".dimmed()
    );
    eprintln!("  {}", "─".repeat(48).dimmed());
    eprintln!();
}

fn print_stats(agent: &Agent, config: &Config, files: &[String]) -> Result<()> {
    println!();
    println!("{}", "━━━ Estadísticas ━━━".bold());
    println!("  {} {}", "Modelo:".dimmed(), config.model.name);
    println!(
        "  {} {} interacciones",
        "Interacciones:".dimmed(),
        agent.interaction_count()
    );
    println!("  {} {}", "Memoria:".dimmed(), agent.memory_summary()?);
    if !files.is_empty() {
        println!(
            "  {} {} archivos cargados",
            "Archivos:".dimmed(),
            files.len()
        );
        for f in files {
            println!("    • {}", f.dimmed());
        }
    }
    println!();
    Ok(())
}

fn print_history(agent: &Agent) -> Result<()> {
    println!();
    println!("{}", "━━━ Historial de conversaciones ━━━".bold());
    let lines = agent.read_conversation_history()?;
    for line in &lines {
        if line.starts_with("  Tú:") {
            println!("{}", line.bright_green());
        } else if line.starts_with("  Asistente:") {
            println!("{}", line.bright_cyan());
        } else if line.starts_with("  Sistema:") {
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
    println!("{}", "━━━ Configuración ━━━".bold());
    println!("  {} {}", "data_dir:".dimmed(), config.data_dir.display());
    println!("  {} {}", "developer_mode:".dimmed(), config.developer_mode);
    println!(
        "  {} {}",
        "developer_prompt:".dimmed(),
        config
            .developer_prompt
            .as_deref()
            .unwrap_or("(por defecto)")
    );
    println!("  {} Modelo", "──".dimmed());
    println!("    {} {}", "nombre:".dimmed(), config.model.name);
    println!("    {} {}", "ruta:".dimmed(), config.model.path);
    println!("    {} {}", "contexto:".dimmed(), config.model.n_ctx);
    println!(
        "    {} {}",
        "capas_gpu:".dimmed(),
        config.model.n_gpu_layers
    );
    println!(
        "    {} {}",
        "plantilla:".dimmed(),
        config.model.chat_template
    );
    println!(
        "    {} K={} / V={}",
        "caché_kv:".dimmed(),
        config.model.kv_type_k,
        config.model.kv_type_v
    );
    println!("  {} Generación", "──".dimmed());
    println!("    {} {}", "top_k:".dimmed(), config.generation.top_k);
    println!("    {} {}", "top_p:".dimmed(), config.generation.top_p);
    println!("    {} {}", "temp:".dimmed(), config.generation.temp);
    println!(
        "    {} {}",
        "max_tokens:".dimmed(),
        config.generation.max_tokens
    );
    println!("  {} API", "──".dimmed());
    println!("    {} {}", "activada:".dimmed(), config.api.enabled);
    println!("    {} {}", "host:".dimmed(), config.api.host);
    println!("    {} {}", "port:".dimmed(), config.api.port);
    println!(
        "    {} {}",
        "token:".dimmed(),
        config.api.token.as_deref().unwrap_or("(ninguno)")
    );
    println!("  {} Lenguajes", "──".dimmed());
    if config.languages.installed.is_empty() {
        println!("    {} (ninguno instalado)", "instalados:".dimmed());
    } else {
        for lang in &config.languages.installed {
            println!("    • {}", lang);
        }
    }
    println!();
}

fn print_rag_debug(entries: &[memvid_agent_core::generation::RagEntryDebug]) {
    println!();
    let title = format!(
        "{} RAG ({} entradas)",
        "━━━ Diagnóstico RAG ━━━".bold(),
        entries.len()
    );
    println!("{}", title);
    for (i, entry) in entries.iter().enumerate() {
        println!(
            "  {}. [{}] score={:.1} tokens={}",
            i + 1,
            entry.source.bright_yellow(),
            entry.score,
            entry.tokens
        );
        let preview = if entry.content.chars().count() > 100 {
            format!(
                "{}...",
                memvid_agent_core::utils::truncate_chars(&entry.content, 100, "")
            )
        } else {
            entry.content.clone()
        };
        println!("     {}", preview.dimmed());
    }
    println!();
}

fn print_model_current(config: &Config) {
    println!();
    println!("{}", "━━━ Modelo activo ━━━".bold());
    println!(
        "  {} {}",
        "Nombre:".dimmed(),
        config.model.name.bright_cyan()
    );
    println!("  {} {}", "Ruta:".dimmed(), config.model.path);
    println!("  {} {}", "Contexto:".dimmed(), config.model.n_ctx);
    println!("  {} {}", "Capas GPU:".dimmed(), config.model.n_gpu_layers);
    println!("  {} {}", "Plantilla:".dimmed(), config.model.chat_template);
    println!(
        "  {} K={} / V={}",
        "Caché KV:".dimmed(),
        config.model.kv_type_k,
        config.model.kv_type_v
    );
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

/// Drive a full fine-tune of the active model on the `/learn` knowledge for one
/// language. Estimates RAM/time first, refuses (unless confirmed) when it cannot
/// fit, then either runs `llama-finetune` or writes a portable run script.
///
/// `lang_key`/`lang_name` are already resolved (catalog key + display name).
/// When `auto` is set the flow is being offered automatically after `/learn`, so
/// it asks once up front and bails quietly on "no".
fn run_finetune_flow(
    lang_key: &str,
    lang_name: &str,
    auto: bool,
    config: &mut Config,
    agent: &Arc<Mutex<Agent>>,
) -> Result<()> {
    if auto {
        let ans = read_line_prompt(&format!(
            "{} You can fine-tune the current model on the {} docs you just learned. Estimate it now? (y/N): ",
            "✦".bright_magenta(),
            lang_name.bold()
        ));
        if !ans.eq_ignore_ascii_case("y") {
            return Ok(());
        }
    }

    // 1. Build the corpus from indexed `/learn` knowledge for this language.
    let (corpus, chunks) = {
        let a = agent.lock().unwrap();
        finetune::build_corpus(a.knowledge_entries(), lang_key)
    };
    if chunks == 0 || corpus.trim().is_empty() {
        eprintln!(
            "{} No hay conocimiento indexado para '{}'. Ejecuta /learn {} primero.",
            "✗".red(),
            lang_key,
            lang_key
        );
        return Ok(());
    }

    // 2. Estimate parameter count from the model file on disk (Q4_K_M).
    let model_bytes = std::fs::metadata(&config.model.path)
        .map(|m| m.len())
        .unwrap_or(0);
    let param_count = if model_bytes > 0 {
        finetune::estimate_params_from_size_mb(model_bytes / 1_048_576)
    } else {
        500_000_000
    };

    let threads = finetune::suggested_threads();
    let epochs = config.finetune.epochs.max(1);
    let tokens = finetune::approx_token_count(&corpus);
    let est = finetune::estimate(tokens, param_count, epochs, threads);

    // 3. Show the estimate.
    println!();
    println!("{}", "━━━ Estimación de ajuste fino ━━━".bold());
    println!("  {} {}", "Lenguaje:".dimmed(), lang_name.bright_cyan());
    println!("  {} {}", "Modelo base:".dimmed(), config.model.name);
    println!(
        "  {} {} fragmentos (~{} tokens)",
        "Corpus:".dimmed(),
        chunks,
        tokens
    );
    println!(
        "  {} ~{:.1} B parámetros · {} épocas · {} hilos",
        "Ejecución:".dimmed(),
        param_count as f64 / 1e9,
        epochs,
        threads
    );
    println!(
        "  {} ~{:.1} GB de RAM pico (ajuste completo AdamW)",
        "Memoria:".dimmed(),
        est.ram_gb()
    );
    println!("  {} {} (aproximado)", "Tiempo:".dimmed(), est.human_time());

    // 4. RAM pre-flight.
    if let Some(avail) = finetune::available_ram_bytes() {
        let avail_gb = avail as f64 / 1_073_741_824.0;
        if !est.fits_in_ram(avail) {
            println!();
            eprintln!(
                "{} RAM insuficiente: se necesitan ~{:.1} GB y solo hay ~{:.1} GB disponibles.",
                "⚠".yellow(),
                est.ram_gb(),
                avail_gb
            );
            eprintln!(
                "  {} Esta máquina probablemente se quede sin memoria. Configura finetune.binary_path / LLAMA_FINETUNE_BIN",
                "→".dimmed()
            );
            eprintln!(
                "  {} en una máquina mayor, o continúa solo para escribir el corpus y el script.",
                "→".dimmed()
            );
        }
    }

    // 5. Locate the binary and confirm.
    let binary = finetune::locate_binary(config.finetune.binary_path.as_deref());
    println!();
    let prompt = match &binary {
        Some(bin) => format!(
            "{} ¿Ejecutar {} ahora? Puede tardar {}. (s/N): ",
            "•".dimmed(),
            bin.display(),
            est.human_time()
        ),
        None => format!(
            "{} No se encontró llama-finetune. ¿Escribir corpus y script para otra máquina? (S/n): ",
            "•".dimmed()
        ),
    };
    let ans = read_line_prompt(&prompt);
    let proceed = match binary {
        Some(_) => ans.eq_ignore_ascii_case("y"),
        // Deferred path is cheap (just writes files) → default yes.
        None => !ans.eq_ignore_ascii_case("n"),
    };
    if !proceed {
        println!("  {} Cancelado.", "i".yellow());
        return Ok(());
    }

    // 6. Run (or defer).
    let output_dir = std::path::PathBuf::from(&config.finetune.output_dir);
    let plan = FinetunePlan {
        lang_key,
        lang_name,
        model_path: &config.model.path,
        model_name: &config.model.name,
        param_count,
        n_ctx: config.model.n_ctx,
        epochs,
        n_threads: threads,
        output_dir: &output_dir,
        binary,
    };

    match finetune::prepare_and_run(&corpus, &plan)? {
        FinetuneOutcome::Trained(path) => {
            println!(
                "{} Ajuste fino completado: {}",
                "✓".green(),
                path.display().to_string().bright_cyan()
            );
            let switch = read_line_prompt(&format!(
                "{} ¿Cambiar ahora al modelo ajustado? (s/N): ",
                "•".dimmed()
            ));
            if switch.eq_ignore_ascii_case("y") {
                config.model.path = path.to_string_lossy().to_string();
                config.model.name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| config.model.name.clone());
                config.model.download_url = None;
                config.model.sha256 = None;
                config.save()?;
                let mut a = agent.lock().unwrap();
                a.switch_model(
                    &config.model.path,
                    config.model.n_ctx,
                    config.model.n_gpu_layers,
                    &config.model.kv_type_k,
                    &config.model.kv_type_v,
                    &config.model.name,
                    &config.model.chat_template,
                    config.generation.top_k,
                    config.generation.top_p,
                    config.generation.temp,
                )?;
                println!("{} Ahora usando {}", "✓".green(), config.model.name.bold());
            }
        }
        FinetuneOutcome::Deferred { corpus, script } => {
            println!(
                "{} No se ejecutó el entrenamiento. Se escribió:",
                "i".yellow()
            );
            println!("  {} {}", "corpus:".dimmed(), corpus.display());
            println!("  {} {}", "script:".dimmed(), script.display());
            println!(
                "  {} Compila llama-finetune y ejecuta el script en una máquina con ~{:.0} GB de RAM.",
                "→".dimmed(),
                est.ram_gb()
            );
        }
    }
    Ok(())
}

fn run_setup_wizard(config: &mut Config, catalog: &ModelsCatalog) -> Result<()> {
    println!();
    println!(
        "{} ¡Bienvenido a aten-ia!",
        "━━━ Configuración inicial ━━━".bold().bright_green()
    );
    println!("  Configuración inicial en unos pasos.");
    println!();

    let data_dir = read_line_prompt(&format!(
        "{} Directorio de datos [{}]: ",
        "•".dimmed(),
        config.data_dir.display()
    ));
    if !data_dir.is_empty() {
        config.data_dir = std::path::PathBuf::from(&data_dir);
    }

    println!();
    print_models_table(catalog, "", false);
    let model_choice = read_line_prompt(&format!("{} Elige modelo [1]: ", "•".dimmed()));
    let model_idx: usize = loop {
        let parsed = model_choice.parse::<usize>();
        if model_choice.is_empty() {
            break 1;
        }
        if let Ok(idx) = parsed {
            if idx > 0 && idx <= catalog.list().len() {
                break idx;
            }
        }
        eprintln!(
            "{} Opción no válida. Escribe un número entre 1 y {}.",
            "✗".red(),
            catalog.list().len()
        );
        break 1;
    };
    if model_idx > 0 && model_idx <= catalog.list().len() {
        let entry = &catalog.list()[model_idx - 1];
        let models_dir = std::path::Path::new("models");
        // Use the exact filename `download_model` writes. `with_extension`
        // corrupts ids that contain a dot (qwen2.5-1.5b -> qwen2.5-1.gguf).
        let expected_path = models_catalog::model_path(models_dir, &entry.id);
        let selected_path = match models_catalog::download_model(entry, models_dir) {
            Ok(path) => {
                println!(
                    "  {} Modelo seleccionado: {}",
                    "✓".green(),
                    entry.name.bold()
                );
                path
            }
            Err(e) => {
                println!(
                    "  {} Descarga diferida (se reintentará al arrancar): {:#}",
                    "i".yellow(),
                    e
                );
                expected_path.clone()
            }
        };
        models_catalog::apply_model_to_config(&selected_path, entry, config)?;
    }

    let install_langs = read_line_prompt(&format!(
        "{} ¿Instalar documentación de un lenguaje? (s/N): ",
        "•".dimmed()
    ));
    if install_langs.to_lowercase() == "y" {
        match LanguagesCatalog::load_or_fetch(&config.data_dir) {
            Ok(lang_catalog) => {
                println!("  {} Obteniendo catálogo de lenguajes…", "↳".dimmed());
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
                    read_line_prompt("  Escribe la clave del lenguaje (vacío para omitir): ");
                if !lang_choice.is_empty() && lang_catalog.find(&lang_choice).is_some() {
                    println!(
                        "  {} El lenguaje '{}' se instalará en el próximo /learn",
                        "↳".dimmed(),
                        lang_choice
                    );
                }
            }
            Err(e) => {
                println!(
                    "  {} No se pudo obtener el catálogo de lenguajes: {}",
                    "✗".red(),
                    e
                );
            }
        }
    }

    let enable_api = read_line_prompt(&format!(
        "{} ¿Activar el servidor API? (s/N): ",
        "•".dimmed()
    ));
    if enable_api.to_lowercase() == "y" {
        config.api.enabled = true;
        let port = read_line_prompt(&format!(
            "{} Puerto de la API [{}]: ",
            "•".dimmed(),
            config.api.port
        ));
        if !port.is_empty() {
            if let Ok(p) = port.parse() {
                config.api.port = p;
            }
        }
        config.api.token = Some(uuid::Uuid::new_v4().to_string());
        let token = config.api.token.as_ref().unwrap().clone();
        println!(
            "  {} Token de API generado: {}",
            "✓".green(),
            token.bright_yellow()
        );

        let setup_opencode = read_line_prompt(&format!(
            "{} ¿Generar configuración de OpenCode? (S/n): ",
            "•".dimmed()
        ));
        if setup_opencode.is_empty() || setup_opencode.to_lowercase() == "y" {
            let oc_content = format!(
                r#"{{
  "$schema": "https://opencode.ai/config.json",
  "model": "openai/{model}",
  "provider": {{
    "openai": {{
      "options": {{
        "baseURL": "http://localhost:{port}/v1",
        "apiKey": "{token}"
      }}
    }}
  }},
  "agent": {{
    "aten-ia": {{
      "description": "Agente local de aten-ia — conoce toda la codebase y arquitectura del proyecto. Usa el LLM local para responder preguntas sobre el código, RAG, persistencia, y más.",
      "mode": "primary",
      "model": "openai/{model}",
      "instructions": ["AGENTS.md"]
    }}
  }},
  "default_agent": "aten-ia"
}}"#,
                model = config.model.name,
                port = config.api.port,
                token = token
            );

            let global_path = std::env::var("HOME")
                .map(|h| std::path::PathBuf::from(h).join(".config/opencode/opencode.json"))
                .unwrap_or_else(|_| std::path::PathBuf::from(".opencode.json"));
            if let Some(parent) = global_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            if memvid_agent_core::utils::atomic_write(&global_path, &oc_content).is_ok() {
                println!(
                    "  {} Configuración global de OpenCode escrita en {}",
                    "✓".green(),
                    global_path.display()
                );
            }

            if let Ok(cwd) = std::env::current_dir() {
                if let Some(parent) = cwd.parent() {
                    let project_path = parent.join("opencode.json");
                    if project_path.exists() || {
                        let ans = read_line_prompt(&format!(
                            "{} ¿Escribir configuración del proyecto en {}? (S/n): ",
                            "•".dimmed(),
                            project_path.display()
                        ));
                        ans.is_empty() || ans.to_lowercase() == "y"
                    } {
                        if memvid_agent_core::utils::atomic_write(&project_path, &oc_content)
                            .is_ok()
                        {
                            println!(
                                "  {} Configuración del proyecto de OpenCode escrita en {}",
                                "✓".green(),
                                project_path.display()
                            );
                        }
                    }
                }
            }

            println!(
                "  {} Reinicia opencode después de actualizar la configuración",
                "↳".dimmed()
            );
        }
    }

    config.save()?;
    println!();
    println!(
        "{} ¡Configuración completada! Guardada.",
        "✓".green().bold()
    );
    println!();
    Ok(())
}
