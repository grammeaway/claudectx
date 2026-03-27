use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use dirs::home_dir;
use serde_json;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

// ── CLI definition ──────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "claudectx",
    about = "Switch between Claude Code configs (work, personal, …)",
    long_about = "claudectx lets you manage multiple Claude Code identities.\n\
                  Each context stores ~/.claude.json and ~/.claude/settings.json.\n\n\
                  Similar to kubectx, but for Claude Code.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Switch to this context directly (no subcommand needed)
    context: Option<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// List all saved contexts
    #[command(alias = "ls")]
    List,

    /// Save the current config as a named context
    Save {
        /// Name for this context (e.g. "work", "personal")
        name: String,
    },

    /// Switch to a saved context
    Use {
        /// Name of the context to activate
        name: String,
    },

    /// Delete a saved context
    #[command(alias = "rm")]
    Delete {
        /// Name of the context to delete
        name: String,
    },

    /// Show which context is currently active
    Current,

    /// Rename a context
    Rename {
        /// Existing context name
        old_name: String,
        /// New context name
        new_name: String,
    },

    /// Show what files a context contains
    Inspect {
        /// Name of the context to inspect
        name: String,
    },

    /// Copy a context to a new name
    Copy {
        /// Source context name
        source: String,
        /// Destination context name
        dest: String,
    },
}

// ── Config store ─────────────────────────────────────────────────────────────
//
//  ~/.claudectx/
//      current              — plain-text name of active context
//      contexts/
//          work/
//              claude.json
//              settings.json
//          personal/
//              claude.json
//              settings.json

struct Store {
    root: PathBuf,
}

impl Store {
    fn new() -> Result<Self> {
        let home = home_dir().context("Cannot determine home directory")?;
        let root = home.join(".claudectx");
        fs::create_dir_all(root.join("contexts"))
            .context("Cannot create ~/.claudectx/contexts/")?;
        #[cfg(unix)]
        {
            // Store contains copies of claude.json which holds OAuth tokens
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                .context("Cannot set permissions on ~/.claudectx/")?;
        }
        Ok(Self { root })
    }

    fn ctx_dir(&self, name: &str) -> PathBuf {
        self.root.join("contexts").join(name)
    }

    fn current_file(&self) -> PathBuf {
        self.root.join("current")
    }

    fn get_current(&self) -> Option<String> {
        fs::read_to_string(self.current_file())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn set_current(&self, name: &str) -> Result<()> {
        fs::write(self.current_file(), name).context("Cannot write current context file")
    }

    fn list(&self) -> Result<Vec<String>> {
        let contexts_dir = self.root.join("contexts");
        let mut names: Vec<String> = fs::read_dir(&contexts_dir)
            .context("Cannot read contexts directory")?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();
        Ok(names)
    }

    fn exists(&self, name: &str) -> bool {
        self.ctx_dir(name).is_dir()
    }
}

// ── Claude Code file paths ───────────────────────────────────────────────────

fn claude_json_path() -> Result<PathBuf> {
    let home = home_dir().context("Cannot determine home directory")?;
    Ok(home.join(".claude.json"))
}

fn settings_json_path() -> Result<PathBuf> {
    let home = home_dir().context("Cannot determine home directory")?;
    Ok(home.join(".claude").join("settings.json"))
}

// ── Commands ──────────────────────────────────────────────────────────────────

fn cmd_list(store: &Store) -> Result<()> {
    let names = store.list()?;
    let current = store.get_current();

    if names.is_empty() {
        println!(
            "{}",
            "No contexts saved yet. Use `claudectx save <name>` to save the current config."
                .yellow()
        );
        return Ok(());
    }

    for name in &names {
        if current.as_deref() == Some(name.as_str()) {
            println!("{} {}", "✓".green().bold(), name.green().bold());
        } else {
            println!("  {}", name);
        }
    }
    Ok(())
}

fn cmd_save(store: &Store, name: &str) -> Result<()> {
    validate_name(name)?;
    let ctx_dir = store.ctx_dir(name);
    fs::create_dir_all(&ctx_dir).context("Cannot create context directory")?;

    let claude_json = claude_json_path()?;
    let settings_json = settings_json_path()?;

    let mut saved = Vec::new();

    if claude_json.exists() {
        let dest = ctx_dir.join("claude.json");
        fs::copy(&claude_json, &dest).context("Cannot copy ~/.claude.json")?;
        #[cfg(unix)]
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o600))
            .context("Cannot set permissions on saved claude.json")?;
        saved.push("~/.claude.json");
    }

    if settings_json.exists() {
        let dest = ctx_dir.join("settings.json");
        fs::copy(&settings_json, &dest).context("Cannot copy ~/.claude/settings.json")?;
        saved.push("~/.claude/settings.json");
    }

    if saved.is_empty() {
        bail!(
            "No Claude Code config files found.\n  \
             Expected: ~/.claude.json and/or ~/.claude/settings.json"
        );
    }

    store.set_current(name)?;

    println!(
        "{} Saved context {} ({})",
        "✓".green().bold(),
        name.cyan().bold(),
        saved.join(", ")
    );
    Ok(())
}

fn cmd_use(store: &Store, name: &str) -> Result<()> {
    if !store.exists(name) {
        bail!(
            "Context '{}' not found. Run `claudectx list` to see available contexts.",
            name
        );
    }

    let ctx_dir = store.ctx_dir(name);
    let claude_json_dest = claude_json_path()?;
    let settings_json_dest = settings_json_path()?;

    let saved_claude = ctx_dir.join("claude.json");
    let saved_settings = ctx_dir.join("settings.json");

    let mut restored = Vec::new();

    if saved_claude.exists() {
        if claude_json_dest.exists() {
            let backup = claude_json_dest.with_extension("json.bak");
            fs::copy(&claude_json_dest, &backup).ok();
        }
        fs::copy(&saved_claude, &claude_json_dest).context("Cannot restore ~/.claude.json")?;
        #[cfg(unix)]
        fs::set_permissions(&claude_json_dest, fs::Permissions::from_mode(0o600))
            .context("Cannot set permissions on ~/.claude.json")?;
        restored.push("~/.claude.json");
    }

    if saved_settings.exists() {
        if let Some(parent) = settings_json_dest.parent() {
            fs::create_dir_all(parent).context("Cannot create ~/.claude/")?;
        }
        if settings_json_dest.exists() {
            let backup = settings_json_dest.with_extension("json.bak");
            fs::copy(&settings_json_dest, &backup).ok();
        }
        fs::copy(&saved_settings, &settings_json_dest)
            .context("Cannot restore ~/.claude/settings.json")?;
        restored.push("~/.claude/settings.json");
    }

    if restored.is_empty() {
        bail!(
            "Context '{}' exists but contains no config files. Try re-saving it.",
            name
        );
    }

    store.set_current(name)?;

    println!(
        "{} Switched to context {} ({})",
        "✓".green().bold(),
        name.cyan().bold(),
        restored.join(", ")
    );
    println!(
        "{}",
        "  Restart Claude Code for changes to take effect.".dimmed()
    );
    Ok(())
}

fn cmd_delete(store: &Store, name: &str) -> Result<()> {
    if !store.exists(name) {
        bail!("Context '{}' not found.", name);
    }
    fs::remove_dir_all(store.ctx_dir(name)).context("Cannot delete context directory")?;
    if store.get_current().as_deref() == Some(name) {
        let _ = fs::remove_file(store.current_file());
    }
    println!("{} Deleted context {}", "✓".green().bold(), name.cyan().bold());
    Ok(())
}

fn cmd_current(store: &Store) -> Result<()> {
    match store.get_current() {
        Some(name) => println!("{}", name.cyan().bold()),
        None => println!("{}", "(none)".dimmed()),
    }
    Ok(())
}

fn cmd_rename(store: &Store, old: &str, new: &str) -> Result<()> {
    if !store.exists(old) {
        bail!("Context '{}' not found.", old);
    }
    validate_name(new)?;
    if store.exists(new) {
        bail!("Context '{}' already exists.", new);
    }
    fs::rename(store.ctx_dir(old), store.ctx_dir(new))
        .context("Cannot rename context directory")?;
    if store.get_current().as_deref() == Some(old) {
        store.set_current(new)?;
    }
    println!(
        "{} Renamed {} → {}",
        "✓".green().bold(),
        old.yellow(),
        new.cyan().bold()
    );
    Ok(())
}

fn cmd_inspect(store: &Store, name: &str) -> Result<()> {
    if !store.exists(name) {
        bail!("Context '{}' not found.", name);
    }
    let ctx_dir = store.ctx_dir(name);
    let current = store.get_current();
    let marker = if current.as_deref() == Some(name) {
        format!(" {}", "(current)".green())
    } else {
        String::new()
    };

    println!("Context: {}{}", name.cyan().bold(), marker);
    println!("Stored at: {}", ctx_dir.display().to_string().dimmed());
    println!();

    let files = [
        ("claude.json", "~/.claude.json"),
        ("settings.json", "~/.claude/settings.json"),
    ];

    let mut found_any = false;
    for (filename, display_name) in &files {
        let path = ctx_dir.join(filename);
        if path.exists() {
            found_any = true;
            let meta = fs::metadata(&path)?;
            println!(
                "  {} {} ({})",
                "·".green(),
                display_name.bold(),
                format_size(meta.len())
            );
            if let Ok(text) = fs::read_to_string(&path) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(obj) = val.as_object() {
                        let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).take(8).collect();
                        let suffix = if obj.len() > 8 {
                            format!(" +{} more", obj.len() - 8)
                        } else {
                            String::new()
                        };
                        println!(
                            "    {}",
                            format!("keys: {}{}", keys.join(", "), suffix).dimmed()
                        );
                    }
                }
            }
        }
    }

    if !found_any {
        println!("  {}", "(empty — no config files stored)".yellow());
    }

    Ok(())
}

fn cmd_copy(store: &Store, source: &str, dest: &str) -> Result<()> {
    if !store.exists(source) {
        bail!("Context '{}' not found.", source);
    }
    validate_name(dest)?;
    if store.exists(dest) {
        bail!("Context '{}' already exists.", dest);
    }
    copy_dir_all(store.ctx_dir(source), store.ctx_dir(dest))
        .context("Cannot copy context directory")?;
    #[cfg(unix)]
    {
        let claude_json = store.ctx_dir(dest).join("claude.json");
        if claude_json.exists() {
            fs::set_permissions(&claude_json, fs::Permissions::from_mode(0o600))
                .context("Cannot set permissions on copied claude.json")?;
        }
    }
    println!(
        "{} Copied {} → {}",
        "✓".green().bold(),
        source.yellow(),
        dest.cyan().bold()
    );
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Context name cannot be empty.");
    }
    if name.contains(|c: char| c == '/' || c == '\\' || c == '.' || c.is_whitespace()) {
        bail!(
            "Context name must not contain /, \\, ., or whitespace. Got: {:?}",
            name
        );
    }
    Ok(())
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    let store = match Store::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{} {}", "error:".red().bold(), e);
            std::process::exit(1);
        }
    };

    let result = match (cli.command, cli.context) {
        (None, Some(name)) => cmd_use(&store, &name),
        (Some(Cmd::List), _) | (None, None) => cmd_list(&store),
        (Some(Cmd::Save { name }), _) => cmd_save(&store, &name),
        (Some(Cmd::Use { name }), _) => cmd_use(&store, &name),
        (Some(Cmd::Delete { name }), _) => cmd_delete(&store, &name),
        (Some(Cmd::Current), _) => cmd_current(&store),
        (Some(Cmd::Rename { old_name, new_name }), _) => cmd_rename(&store, &old_name, &new_name),
        (Some(Cmd::Inspect { name }), _) => cmd_inspect(&store, &name),
        (Some(Cmd::Copy { source, dest }), _) => cmd_copy(&store, &source, &dest),
    };

    if let Err(e) = result {
        eprintln!("{} {}", "error:".red().bold(), e);
        std::process::exit(1);
    }
}
