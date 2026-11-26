//! User-level cleanup for `aten-ia --purge-data`.
//!
//! Debian packages must not delete files in a user's home from maintainer
//! scripts. This command removes exactly the paths aten-ia creates for the
//! current user, after showing the plan and asking for confirmation.

use anyhow::{Context, Result, bail};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

struct Target {
    path: PathBuf,
    kind: &'static str,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn resolve(cwd: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

/// Reject `/`, `/home`, `$HOME`, the CWD itself and anything outside the
/// current directory or the user's home. This keeps a corrupted config from
/// turning `--purge-data` into a destructive `rm -rf`.
fn safe_target(path: &Path, cwd: &Path) -> bool {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let home = home_dir().and_then(|h| h.canonicalize().ok());

    if canonical == Path::new("/")
        || canonical == Path::new("/home")
        || canonical == cwd
        || home.as_deref() == Some(canonical.as_path())
    {
        return false;
    }

    canonical.starts_with(&cwd)
        || home
            .as_deref()
            .is_some_and(|home| canonical.starts_with(home))
}

fn read_config(cwd: &Path) -> Result<serde_json::Value> {
    let path = cwd.join("config.json");
    if !path.exists() {
        return Ok(serde_json::Value::Null);
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("no se pudo leer {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("JSON inválido en {}", path.display()))
}

fn data_dir_from_value(value: &serde_json::Value, cwd: &Path) -> PathBuf {
    value
        .get("data_dir")
        .and_then(|v| v.as_str())
        .map(|raw| resolve(cwd, raw))
        .unwrap_or_else(|| {
            home_dir()
                .map(|home| home.join(".aten-ia"))
                .unwrap_or_else(|| cwd.join("memvid_data"))
        })
}

fn model_path_from_value(value: &serde_json::Value, cwd: &Path) -> Option<PathBuf> {
    value
        .get("model")
        .and_then(|m| m.get("path"))
        .and_then(|v| v.as_str())
        .map(|raw| resolve(cwd, raw))
}

fn collect(cwd: &Path) -> Result<Vec<Target>> {
    let value = read_config(cwd)?;
    let data_dir = data_dir_from_value(&value, cwd);
    let model = model_path_from_value(&value, cwd);

    let mut targets = Vec::new();
    if data_dir.exists() {
        targets.push(Target {
            path: data_dir,
            kind: "datos (conversaciones, conocimiento, índice)",
        });
    }

    if let Some(model) = model {
        if model.exists() {
            let parent = model.parent().map(Path::to_path_buf);
            targets.push(Target {
                path: model,
                kind: "modelo GGUF configurado",
            });
            if let Some(parent) = parent {
                let finetuned = parent.join("finetuned");
                if finetuned.is_dir() {
                    targets.push(Target {
                        path: finetuned,
                        kind: "modelos ajustados",
                    });
                }
            }
        }
    }

    let config = cwd.join("config.json");
    if config.exists() {
        targets.push(Target {
            path: config,
            kind: "configuración y token de API",
        });
    }

    let mut safe = Vec::new();
    for target in targets {
        if safe_target(&target.path, cwd) {
            safe.push(target);
        } else {
            eprintln!(
                "  omitido (fuera del directorio actual o del HOME): {}",
                target.path.display()
            );
        }
    }
    Ok(safe)
}

fn remove(target: &Target, cwd: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(&target.path)
        .with_context(|| format!("no se pudo inspeccionar {}", target.path.display()))?;
    if meta.file_type().is_symlink() || meta.is_file() {
        std::fs::remove_file(&target.path)
            .with_context(|| format!("no se pudo borrar {}", target.path.display()))?;
        // Remove an empty `models/` directory left after the GGUF file.
        if let Some(parent) = target.path.parent() {
            if parent.file_name().and_then(|n| n.to_str()) == Some("models")
                && safe_target(parent, cwd)
                && parent
                    .read_dir()
                    .map(|mut entries| entries.next().is_none())
                    .unwrap_or(false)
            {
                let _ = std::fs::remove_dir(parent);
            }
        }
    } else if meta.is_dir() {
        std::fs::remove_dir_all(&target.path)
            .with_context(|| format!("no se pudo borrar {}", target.path.display()))?;
    }
    Ok(())
}

fn print_shared_paths(cwd: &Path) {
    let mut shared = Vec::new();
    if let Some(parent) = cwd.parent() {
        let project = parent.join("opencode.json");
        if project.exists() {
            shared.push(project);
        }
    }
    if let Some(home) = home_dir() {
        let global = home.join(".config/opencode/opencode.json");
        if global.exists() {
            shared.push(global);
        }
    }
    if !shared.is_empty() {
        println!();
        println!("  No se eliminan (compartidos con OpenCode/otros proyectos):");
        for path in shared {
            println!("    • {}", path.display());
        }
        println!("    Revísalos manualmente si no los necesitas.");
    }
}

pub fn run(cwd: &Path, assume_yes: bool) -> Result<()> {
    let targets = collect(cwd)?;
    println!("aten-ia · limpieza de datos de usuario");
    println!("  Directorio: {}", cwd.display());
    println!();

    if targets.is_empty() {
        println!("  No se encontraron datos generados por aten-ia.");
        print_shared_paths(cwd);
        return Ok(());
    }

    println!("  Se eliminarán:");
    for target in &targets {
        println!("    • {}  [{}]", target.path.display(), target.kind);
    }

    if !assume_yes {
        if !std::io::stdin().is_terminal() {
            bail!("se requiere --yes cuando no hay terminal interactiva");
        }
        print!("\n  Escribe ELIMINAR para continuar: ");
        std::io::stdout().flush().ok();
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if answer.trim() != "ELIMINAR" {
            bail!("cancelado; no se ha eliminado nada");
        }
    }

    for target in &targets {
        remove(target, cwd)?;
        println!("  ✓ eliminado: {}", target.path.display());
    }
    println!();
    println!("  Limpieza completada. Ya puedes desinstalar el paquete:");
    println!("    sudo apt purge aten-ia");
    print_shared_paths(cwd);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_relative_and_absolute() {
        let cwd = Path::new("/tmp/example");
        assert_eq!(resolve(cwd, "data"), PathBuf::from("/tmp/example/data"));
        assert_eq!(resolve(cwd, "/var/data"), PathBuf::from("/var/data"));
    }

    #[test]
    fn data_dir_defaults_to_home_dot_aten_ia() {
        let value = serde_json::Value::Null;
        let dir = data_dir_from_value(&value, Path::new("/tmp/example"));
        assert!(dir.ends_with(".aten-ia") || dir.ends_with("memvid_data"));
    }
}
