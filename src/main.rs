use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use colored::Colorize;
use dirs::home_dir;
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
    home: PathBuf,
}

impl Store {
    fn new() -> Result<Self> {
        let home = home_dir().context("Cannot determine home directory")?;
        Self::with_paths(home.join(".claudectx"), home)
    }

    fn with_paths(root: PathBuf, home: PathBuf) -> Result<Self> {
        fs::create_dir_all(root.join("contexts")).context("Cannot create contexts directory")?;
        #[cfg(unix)]
        {
            // Store contains copies of claude.json which holds OAuth tokens
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                .context("Cannot set permissions on store root")?;
        }
        Ok(Self { root, home })
    }

    fn claude_json_path(&self) -> PathBuf {
        self.home.join(".claude.json")
    }

    fn settings_json_path(&self) -> PathBuf {
        self.home.join(".claude").join("settings.json")
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

    let claude_json = store.claude_json_path();
    let settings_json = store.settings_json_path();

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
    let claude_json_dest = store.claude_json_path();
    let settings_json_dest = store.settings_json_path();

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
    println!(
        "{} Deleted context {}",
        "✓".green().bold(),
        name.cyan().bold()
    );
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
            if let Ok(text) = fs::read_to_string(&path)
                && let Ok(val) = serde_json::from_str::<serde_json::Value>(&text)
                && let Some(obj) = val.as_object()
            {
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

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Create a Store backed by a temp directory.
    fn test_store() -> (TempDir, Store) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join(".claudectx");
        let home = tmp.path().to_path_buf();
        let store = Store::with_paths(root, home).unwrap();
        (tmp, store)
    }

    /// Write fake Claude config files into the test home directory.
    fn write_fake_configs(store: &Store) {
        let claude_json = store.claude_json_path();
        fs::write(&claude_json, r#"{"oauthToken":"fake"}"#).unwrap();

        let settings_dir = store.settings_json_path().parent().unwrap().to_path_buf();
        fs::create_dir_all(&settings_dir).unwrap();
        fs::write(
            store.settings_json_path(),
            r#"{"model":"opus","permissions":[]}"#,
        )
        .unwrap();
    }

    // ── Pure function tests ──────────────────────────────────────────────

    #[test]
    fn test_validate_name_valid() {
        assert!(validate_name("work").is_ok());
        assert!(validate_name("my-context").is_ok());
        assert!(validate_name("ctx_123").is_ok());
    }

    #[test]
    fn test_validate_name_empty() {
        assert!(validate_name("").is_err());
    }

    #[test]
    fn test_validate_name_slash() {
        assert!(validate_name("a/b").is_err());
    }

    #[test]
    fn test_validate_name_backslash() {
        assert!(validate_name("a\\b").is_err());
    }

    #[test]
    fn test_validate_name_dot() {
        assert!(validate_name("a.b").is_err());
    }

    #[test]
    fn test_validate_name_whitespace() {
        assert!(validate_name("a b").is_err());
        assert!(validate_name("a\tb").is_err());
    }

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn test_format_size_kilobytes() {
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(2048), "2.0 KB");
    }

    // ── Store method tests ───────────────────────────────────────────────

    #[test]
    fn test_store_with_paths_creates_dirs() {
        let (_tmp, store) = test_store();
        assert!(store.root.join("contexts").is_dir());
    }

    #[test]
    fn test_store_list_empty() {
        let (_tmp, store) = test_store();
        assert_eq!(store.list().unwrap(), Vec::<String>::new());
    }

    #[test]
    fn test_store_list_sorted() {
        let (_tmp, store) = test_store();
        for name in ["charlie", "alpha", "bravo"] {
            fs::create_dir(store.ctx_dir(name)).unwrap();
        }
        assert_eq!(store.list().unwrap(), vec!["alpha", "bravo", "charlie"]);
    }

    #[test]
    fn test_store_exists() {
        let (_tmp, store) = test_store();
        assert!(!store.exists("work"));
        fs::create_dir(store.ctx_dir("work")).unwrap();
        assert!(store.exists("work"));
    }

    #[test]
    fn test_store_current_none() {
        let (_tmp, store) = test_store();
        assert_eq!(store.get_current(), None);
    }

    #[test]
    fn test_store_set_and_get_current() {
        let (_tmp, store) = test_store();
        store.set_current("work").unwrap();
        assert_eq!(store.get_current(), Some("work".to_string()));
    }

    #[test]
    fn test_store_ctx_dir() {
        let (_tmp, store) = test_store();
        assert_eq!(
            store.ctx_dir("work"),
            store.root.join("contexts").join("work")
        );
    }

    // ── Command integration tests ────────────────────────────────────────

    #[test]
    fn test_cmd_save_and_use_roundtrip() {
        let (_tmp, store) = test_store();
        write_fake_configs(&store);

        cmd_save(&store, "work").unwrap();
        assert!(store.exists("work"));
        assert_eq!(store.get_current(), Some("work".to_string()));

        // Verify saved files exist
        let ctx = store.ctx_dir("work");
        assert!(ctx.join("claude.json").exists());
        assert!(ctx.join("settings.json").exists());

        // Modify the live config, then restore
        fs::write(store.claude_json_path(), r#"{"modified":true}"#).unwrap();
        cmd_use(&store, "work").unwrap();

        // Verify restored content matches original
        let restored = fs::read_to_string(store.claude_json_path()).unwrap();
        assert_eq!(restored, r#"{"oauthToken":"fake"}"#);
    }

    #[test]
    fn test_cmd_save_creates_backup_on_use() {
        let (_tmp, store) = test_store();
        write_fake_configs(&store);

        cmd_save(&store, "work").unwrap();

        // Use the context — should create .bak files
        cmd_use(&store, "work").unwrap();
        assert!(store.claude_json_path().with_extension("json.bak").exists());
    }

    #[test]
    fn test_cmd_save_no_config_files() {
        let (_tmp, store) = test_store();
        // No config files written — should fail
        let result = cmd_save(&store, "empty");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No Claude Code config files found")
        );
    }

    #[test]
    fn test_cmd_save_partial_config() {
        let (_tmp, store) = test_store();
        // Only claude.json, no settings.json
        fs::write(store.claude_json_path(), r#"{"token":"x"}"#).unwrap();

        cmd_save(&store, "partial").unwrap();
        assert!(store.ctx_dir("partial").join("claude.json").exists());
        assert!(!store.ctx_dir("partial").join("settings.json").exists());
    }

    #[test]
    fn test_cmd_use_nonexistent() {
        let (_tmp, store) = test_store();
        let result = cmd_use(&store, "nope");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_cmd_delete() {
        let (_tmp, store) = test_store();
        write_fake_configs(&store);
        cmd_save(&store, "work").unwrap();

        cmd_delete(&store, "work").unwrap();
        assert!(!store.exists("work"));
    }

    #[test]
    fn test_cmd_delete_clears_current() {
        let (_tmp, store) = test_store();
        write_fake_configs(&store);
        cmd_save(&store, "work").unwrap();
        assert_eq!(store.get_current(), Some("work".to_string()));

        cmd_delete(&store, "work").unwrap();
        assert_eq!(store.get_current(), None);
    }

    #[test]
    fn test_cmd_delete_nonexistent() {
        let (_tmp, store) = test_store();
        assert!(cmd_delete(&store, "nope").is_err());
    }

    #[test]
    fn test_cmd_rename() {
        let (_tmp, store) = test_store();
        write_fake_configs(&store);
        cmd_save(&store, "old").unwrap();

        cmd_rename(&store, "old", "new").unwrap();
        assert!(!store.exists("old"));
        assert!(store.exists("new"));
        assert_eq!(store.get_current(), Some("new".to_string()));
    }

    #[test]
    fn test_cmd_rename_target_exists() {
        let (_tmp, store) = test_store();
        write_fake_configs(&store);
        cmd_save(&store, "a").unwrap();
        cmd_save(&store, "b").unwrap();

        let result = cmd_rename(&store, "a", "b");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn test_cmd_copy() {
        let (_tmp, store) = test_store();
        write_fake_configs(&store);
        cmd_save(&store, "src").unwrap();

        cmd_copy(&store, "src", "dst").unwrap();
        assert!(store.exists("src"));
        assert!(store.exists("dst"));

        // Verify file contents match
        let src_content = fs::read_to_string(store.ctx_dir("src").join("claude.json")).unwrap();
        let dst_content = fs::read_to_string(store.ctx_dir("dst").join("claude.json")).unwrap();
        assert_eq!(src_content, dst_content);
    }

    #[test]
    fn test_cmd_copy_target_exists() {
        let (_tmp, store) = test_store();
        write_fake_configs(&store);
        cmd_save(&store, "a").unwrap();
        cmd_save(&store, "b").unwrap();

        let result = cmd_copy(&store, "a", "b");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn test_cmd_list_empty_ok() {
        let (_tmp, store) = test_store();
        assert!(cmd_list(&store).is_ok());
    }

    #[test]
    fn test_cmd_current_none_ok() {
        let (_tmp, store) = test_store();
        assert!(cmd_current(&store).is_ok());
    }

    #[test]
    fn test_cmd_inspect() {
        let (_tmp, store) = test_store();
        write_fake_configs(&store);
        cmd_save(&store, "work").unwrap();

        assert!(cmd_inspect(&store, "work").is_ok());
    }

    #[test]
    fn test_cmd_inspect_nonexistent() {
        let (_tmp, store) = test_store();
        assert!(cmd_inspect(&store, "nope").is_err());
    }

    // ── Helper tests ─────────────────────────────────────────────────────

    #[test]
    fn test_copy_dir_all() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");

        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.txt"), "hello").unwrap();
        fs::write(src.join("sub").join("b.txt"), "world").unwrap();

        copy_dir_all(&src, &dst).unwrap();

        assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "hello");
        assert_eq!(
            fs::read_to_string(dst.join("sub").join("b.txt")).unwrap(),
            "world"
        );
    }
}
