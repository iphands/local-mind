use anyhow::{Context, Result};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};

const OMO_CONFIG_PATH: &str = "container_data/config/opencode/oh-my-opencode.json";
const OMO_BACKUP_PATH: &str = "container_data/config/opencode/oh-my-opencode.json.original";
const OPENCODE_CONFIG_PATH: &str = "container_data/config/opencode/opencode.jsonc";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OmoConfig {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    schema: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tmux: Option<serde_json::Value>,
    #[serde(default)]
    agents: HashMap<String, ModelConfig>,
    #[serde(default)]
    categories: HashMap<String, ModelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModelConfig {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    variant: Option<String>,
}

const AGENTS: &[(&str, &str)] = &[
    (
        "local-mind",
        "container_data/config/opencode/agent/local-mind.md",
    ),
    ("hoodie", "container_data/config/opencode/agent/hoodie.md"),
    (
        "neckbeard",
        "container_data/config/opencode/agent/neckbeard.md",
    ),
];

fn print_usage() {
    eprintln!("Usage: model-switcher <COMMAND>");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  lm, local-mind        Interactive agent model selection (default)");
    eprintln!(
        "  omo, oh-my-opencode   Replace all models in oh-my-opencode.json with a chosen model"
    );
    eprintln!("  all                    Change all agents (system default + agents) to same model");
    eprintln!("  restore-omo-defaults  Restore oh-my-opencode.json from backup");
    eprintln!("  ctx <SIZE> <PRESET>   Set limit.input for all models (safe/balanced/aggressive)");
    eprintln!("  ctx off               Remove limit.input from all models");
    eprintln!("  image [on]            Add image input modality to all models in opencode.jsonc (default: on)");
    eprintln!("  image off             Remove image input modality from all models");
    eprintln!("  help                   Show this help message");
    eprintln!();
    eprintln!("Without command, runs interactive agent model selection.");
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    // Handle command line arguments
    if args.len() > 1 {
        match args[1].as_str() {
            "omo" | "oh-my-opencode" => return run_omo_mode(),
            "all" => return run_all_mode(),
            "restore-omo-defaults" => return restore_omo_defaults(),
            "lm" | "local-mind" => return run_agent_mode(),
            "image" => {
                let sub = args.get(2).map(|s| s.as_str()).unwrap_or("on");
                match sub {
                    "on" => return run_image_on_mode(),
                    "off" => return run_image_off_mode(),
                    other => {
                        eprintln!("Unknown image subcommand: '{}'. Valid: on, off", other);
                        std::process::exit(1);
                    }
                }
            }
            "ctx" => {
                if args.len() >= 3 && args[2] == "off" {
                    return run_ctx_off_mode();
                }
                if args.len() < 4 {
                    eprintln!("Usage:");
                    eprintln!("  model-switcher ctx <SIZE> <PRESET>");
                    eprintln!("  model-switcher ctx off");
                    eprintln!();
                    eprintln!("  SIZE   : context window size (e.g. 65536, 262144, 1048576)");
                    eprintln!("  PRESET : safe (76%) | balanced (85%) | aggressive (92%)");
                    std::process::exit(1);
                }
                let ctx_size: u64 = args[2].parse().with_context(|| {
                    format!(
                        "Invalid context size '{}': must be a positive integer",
                        args[2]
                    )
                })?;
                if ctx_size == 0 {
                    anyhow::bail!("Context size must be greater than 0");
                }
                let preset = match args[3].as_str() {
                    "safe" => CtxPreset::Safe,
                    "balanced" => CtxPreset::Balanced,
                    "aggressive" => CtxPreset::Aggressive,
                    other => {
                        eprintln!(
                            "Unknown preset: '{}'. Valid: safe, balanced, aggressive",
                            other
                        );
                        std::process::exit(1);
                    }
                };
                run_ctx_mode(ctx_size, preset)?;
                if let Some(img_cmd) = args.get(4) {
                    if img_cmd == "image" {
                        let sub = args.get(5).map(|s| s.as_str()).unwrap_or("on");
                        match sub {
                            "on" => run_image_on_mode()?,
                            "off" => run_image_off_mode()?,
                            other => {
                                eprintln!("Unknown image subcommand: '{}'. Valid: on, off", other);
                                std::process::exit(1);
                            }
                        }
                    }
                }
                return Ok(());
            }
            "help" | "-h" | "--help" => {
                print_usage();
                return Ok(());
            }
            other => {
                eprintln!("Unknown command: {}", other);
                print_usage();
                std::process::exit(1);
            }
        }
    }

    // Default: run interactive agent model selection
    run_agent_mode()
}

fn run_agent_mode() -> Result<()> {
    let models = read_models("data/models")?;
    if models.is_empty() {
        eprintln!("No models found in data/models");
        return Ok(());
    }

    let total_steps = AGENTS.len() + 1; // +1 for system default

    // Step 1: Select system default model
    let current_default =
        get_default_model(OPENCODE_CONFIG_PATH).unwrap_or_else(|_| "unknown".to_string());
    let default_selection: Option<(String, String)> =
        match select_default_model(&models, &current_default, 1, total_steps)? {
            Some(selected) => Some((current_default, selected)),
            None => {
                println!("Aborted. No changes made.");
                return Ok(());
            }
        };

    // Collect all agent selections
    let mut selections: Vec<(&str, &str, String, String)> = Vec::new(); // (name, path, current, selected)

    for (i, (name, path)) in AGENTS.iter().enumerate() {
        let current = get_current_model(path).unwrap_or_else(|_| "unknown".to_string());

        match select_model(
            &models,
            &current,
            name,
            i + 2,
            total_steps,
            &selections,
            &default_selection,
        )? {
            Some(selected) => {
                selections.push((name, path, current, selected));
            }
            None => {
                println!("Aborted. No changes made.");
                return Ok(());
            }
        }
    }

    // Confirmation step
    if !confirm_changes(&selections, &default_selection)? {
        println!("Aborted. No changes made.");
        return Ok(());
    }

    // All confirmed - now write to disk
    let mut changes = 0;

    // Write system default model
    if let Some((ref old, ref new)) = default_selection {
        if old != new {
            update_default_model(OPENCODE_CONFIG_PATH, new)?;
            println!("Updated system default -> {}", new);
            changes += 1;
        } else {
            println!("system default: no change");
        }
    }

    // Write agent models
    for (name, path, current, selected) in &selections {
        if selected != current {
            update_agent_model(path, selected)?;
            println!("Updated {} -> {}", name, selected);
            changes += 1;
        } else {
            println!("{}: no change", name);
        }
    }

    if changes == 0 {
        println!("No changes made.");
    }

    Ok(())
}

fn run_all_mode() -> Result<()> {
    let models = read_models("data/models")?;
    if models.is_empty() {
        eprintln!("No models found in data/models");
        return Ok(());
    }

    // Get current models from all sources for display
    let current_default =
        get_default_model(OPENCODE_CONFIG_PATH).unwrap_or_else(|_| "unknown".to_string());
    let current_agent = get_current_model(AGENTS[0].1).unwrap_or_else(|_| "unknown".to_string());

    // Get current OMO model
    let current_omo = if std::path::Path::new(OMO_CONFIG_PATH).exists() {
        let config = read_omo_config(OMO_CONFIG_PATH)?;
        config
            .agents
            .values()
            .next()
            .map(|m| m.model.clone())
            .unwrap_or_else(|| "unknown".to_string())
    } else {
        "not configured".to_string()
    };

    match select_all_model(&models, &current_default, &current_agent, &current_omo)? {
        Some(selected) => {
            let mut changes = 0;

            // Update system default
            if current_default != selected {
                update_default_model(OPENCODE_CONFIG_PATH, &selected)?;
                println!("Updated system default -> {}", selected);
                changes += 1;
            } else {
                println!("system default: no change");
            }

            // Update all agent models
            for (name, path) in AGENTS.iter() {
                let current = get_current_model(path).unwrap_or_else(|_| "unknown".to_string());
                if current != selected {
                    update_agent_model(path, &selected)?;
                    println!("Updated {} -> {}", name, selected);
                    changes += 1;
                } else {
                    println!("{}: no change", name);
                }
            }

            // Update OMO config if it exists
            if std::path::Path::new(OMO_CONFIG_PATH).exists() {
                let config = read_omo_config(OMO_CONFIG_PATH)?;
                let agent_count = config.agents.len();
                let category_count = config.categories.len();

                if agent_count > 0 || category_count > 0 {
                    let mut new_config = config.clone();
                    for agent in new_config.agents.values_mut() {
                        if agent.model != selected {
                            agent.model = selected.clone();
                            changes += 1;
                        }
                    }
                    for category in new_config.categories.values_mut() {
                        if category.model != selected {
                            category.model = selected.clone();
                            changes += 1;
                        }
                    }

                    write_omo_config(OMO_CONFIG_PATH, &new_config)?;
                    println!(
                        "Updated oh-my-opencode.json: {} agents, {} categories -> {}",
                        agent_count, category_count, selected
                    );
                } else {
                    println!("oh-my-opencode.json: no agents or categories to update");
                }
            } else {
                println!("oh-my-opencode.json: not found, skipping");
            }

            if changes == 0 {
                println!("No changes made.");
            } else {
                println!("Updated ALL configurations to: {}", selected);
            }
        }
        None => {
            println!("Aborted. No changes made.");
        }
    }

    Ok(())
}

fn run_omo_mode() -> Result<()> {
    let models = read_models("data/models")?;
    if models.is_empty() {
        eprintln!("No models found in data/models");
        return Ok(());
    }

    // Read current omo config
    let config = read_omo_config(OMO_CONFIG_PATH)?;

    // Count current models
    let agent_count = config.agents.len();
    let category_count = config.categories.len();

    // Get a sample of current models for display
    let current_model = config
        .agents
        .values()
        .next()
        .map(|m| m.model.clone())
        .unwrap_or_else(|| "unknown".to_string());

    // Select a model using the TUI
    match select_omo_model(&models, &current_model, agent_count, category_count)? {
        Some(selected) => {
            // Update all models in config
            let mut new_config = config.clone();
            for agent in new_config.agents.values_mut() {
                agent.model = selected.clone();
            }
            for category in new_config.categories.values_mut() {
                category.model = selected.clone();
            }

            // Write back
            write_omo_config(OMO_CONFIG_PATH, &new_config)?;
            println!(
                "Updated {} agents and {} categories to: {}",
                agent_count, category_count, selected
            );
        }
        None => {
            println!("Aborted. No changes made.");
        }
    }

    Ok(())
}

fn restore_omo_defaults() -> Result<()> {
    // Check if backup exists
    if !std::path::Path::new(OMO_BACKUP_PATH).exists() {
        anyhow::bail!(
            "Backup file not found: {}\nPlease create a backup first.",
            OMO_BACKUP_PATH
        );
    }

    // Read backup and write to main config
    let backup_content = fs::read_to_string(OMO_BACKUP_PATH)
        .with_context(|| format!("Failed to read backup: {}", OMO_BACKUP_PATH))?;

    fs::write(OMO_CONFIG_PATH, &backup_content)
        .with_context(|| format!("Failed to write config: {}", OMO_CONFIG_PATH))?;

    println!("Restored oh-my-opencode.json from backup.");
    Ok(())
}

fn read_omo_config(path: &str) -> Result<OmoConfig> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read config: {}", path))?;
    let config: OmoConfig = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse JSON: {}", path))?;
    Ok(config)
}

fn write_omo_config(path: &str, config: &OmoConfig) -> Result<()> {
    let content = serde_json::to_string_pretty(config)
        .with_context(|| "Failed to serialize config to JSON")?;
    fs::write(path, content + "\n").with_context(|| format!("Failed to write config: {}", path))?;
    Ok(())
}

fn select_omo_model(
    models: &[String],
    current: &str,
    agent_count: usize,
    category_count: usize,
) -> Result<Option<String>> {
    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, Hide)?;

    let result = run_omo_selector(&mut stdout, models, current, agent_count, category_count);

    execute!(stdout, Show, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    result
}

fn run_omo_selector(
    stdout: &mut io::Stdout,
    models: &[String],
    current: &str,
    agent_count: usize,
    category_count: usize,
) -> Result<Option<String>> {
    let mut filter = String::new();
    let mut selected: usize = 0;
    let mut initialized = false;

    loop {
        let filtered: Vec<&String> = models
            .iter()
            .filter(|m| filter.is_empty() || m.to_lowercase().contains(&filter.to_lowercase()))
            .collect();

        // Clamp selection
        if selected >= filtered.len() {
            selected = filtered.len().saturating_sub(1);
        }

        // Set initial selection to current model only once
        if !initialized && filter.is_empty() {
            if let Some(idx) = filtered.iter().position(|m| *m == current) {
                selected = idx;
            }
            initialized = true;
        }

        draw_omo(
            stdout,
            &filtered,
            selected,
            &filter,
            current,
            agent_count,
            category_count,
        )?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                }
                KeyCode::Down => {
                    if selected + 1 < filtered.len() {
                        selected += 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some(m) = filtered.get(selected) {
                        return Ok(Some((*m).clone()));
                    }
                }
                KeyCode::Esc => {
                    return Ok(None);
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None);
                }
                KeyCode::Char(c) => {
                    filter.push(c);
                    selected = 0;
                    initialized = true;
                }
                KeyCode::Backspace => {
                    filter.pop();
                    selected = 0;
                }
                _ => {}
            }
        }
    }
}

fn draw_omo(
    stdout: &mut io::Stdout,
    items: &[&String],
    selected: usize,
    filter: &str,
    current: &str,
    agent_count: usize,
    category_count: usize,
) -> Result<()> {
    let (_, term_height) = terminal::size()?;
    let max_visible = (term_height as usize).saturating_sub(14);

    execute!(stdout, MoveTo(0, 0), Clear(ClearType::All))?;

    // Big banner
    let banner = ">>> OH-MY-OPENCODE MODEL OVERRIDE <<<";
    let padding = "=".repeat(banner.len());

    execute!(
        stdout,
        SetForegroundColor(Color::DarkGrey),
        SetAttribute(Attribute::Bold),
        Print(format!("  {}\r\n", padding)),
        SetForegroundColor(Color::Magenta),
        Print(format!("  {}\r\n", banner)),
        SetForegroundColor(Color::DarkGrey),
        Print(format!("  {}\r\n", padding)),
        ResetColor,
        SetAttribute(Attribute::Reset),
    )?;

    // Info about what will change
    execute!(
        stdout,
        Print("\r\n  This will replace ALL models in oh-my-opencode.json\r\n"),
        Print(format!(
            "  Affecting: {} agents, {} categories\r\n",
            agent_count, category_count
        )),
        Print(format!("\r\n  Sample current model: ")),
        SetForegroundColor(Color::Cyan),
        Print(format!("{}\r\n", current)),
        ResetColor,
    )?;

    // Instructions
    if filter.is_empty() {
        execute!(
            stdout,
            Print("  Type to filter | ↑↓ navigate | Enter select | Esc cancel\r\n\r\n")
        )?;
    } else {
        execute!(
            stdout,
            Print(format!(
                "  Filter: {} | Backspace to clear | Esc cancel\r\n\r\n",
                filter
            ))
        )?;
    }

    if items.is_empty() {
        execute!(
            stdout,
            SetForegroundColor(Color::Red),
            Print("    No matches\r\n"),
            ResetColor,
        )?;
    } else {
        // Calculate scroll window
        let start = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };
        let end = (start + max_visible).min(items.len());

        for (i, item) in items.iter().enumerate().skip(start).take(end - start) {
            let is_selected = i == selected;
            let is_current = *item == current;

            let prefix = match (is_selected, is_current) {
                (true, true) => "  > * ",
                (true, false) => "  >   ",
                (false, true) => "    * ",
                (false, false) => "      ",
            };

            if is_selected {
                execute!(
                    stdout,
                    SetForegroundColor(Color::Green),
                    SetAttribute(Attribute::Bold),
                    Print(format!("{}{}\r\n", prefix, item)),
                    SetAttribute(Attribute::Reset),
                    ResetColor,
                )?;
            } else if is_current {
                execute!(
                    stdout,
                    SetForegroundColor(Color::Yellow),
                    Print(format!("{}{}\r\n", prefix, item)),
                    ResetColor,
                )?;
            } else {
                execute!(stdout, Print(format!("{}{}\r\n", prefix, item)))?;
            }
        }

        // Show scroll indicator if needed
        if items.len() > max_visible {
            execute!(
                stdout,
                SetForegroundColor(Color::DarkGrey),
                Print(format!("\r\n    [{}/{}]\r\n", selected + 1, items.len())),
                ResetColor,
            )?;
        }
    }

    stdout.flush()?;
    Ok(())
}

fn select_all_model(
    models: &[String],
    current_default: &str,
    current_agent: &str,
    current_omo: &str,
) -> Result<Option<String>> {
    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, Hide)?;

    let result = run_all_selector(
        &mut stdout,
        models,
        current_default,
        current_agent,
        current_omo,
    );

    execute!(stdout, Show, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    result
}

fn run_all_selector(
    stdout: &mut io::Stdout,
    models: &[String],
    current_default: &str,
    current_agent: &str,
    current_omo: &str,
) -> Result<Option<String>> {
    let mut filter = String::new();
    let mut selected: usize = 0;
    let mut initialized = false;

    loop {
        let filtered: Vec<&String> = models
            .iter()
            .filter(|m| filter.is_empty() || m.to_lowercase().contains(&filter.to_lowercase()))
            .collect();

        // Clamp selection
        if selected >= filtered.len() {
            selected = filtered.len().saturating_sub(1);
        }

        // Set initial selection to current model only once
        if !initialized && filter.is_empty() {
            if let Some(idx) = filtered.iter().position(|m| *m == current_default) {
                selected = idx;
            }
            initialized = true;
        }

        draw_all(
            stdout,
            &filtered,
            selected,
            &filter,
            current_default,
            current_agent,
            current_omo,
        )?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                }
                KeyCode::Down => {
                    if selected + 1 < filtered.len() {
                        selected += 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some(m) = filtered.get(selected) {
                        return Ok(Some((*m).clone()));
                    }
                }
                KeyCode::Esc => {
                    return Ok(None);
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None);
                }
                KeyCode::Char(c) => {
                    filter.push(c);
                    selected = 0;
                    initialized = true;
                }
                KeyCode::Backspace => {
                    filter.pop();
                    selected = 0;
                }
                _ => {}
            }
        }
    }
}

fn draw_all(
    stdout: &mut io::Stdout,
    items: &[&String],
    selected: usize,
    filter: &str,
    current_default: &str,
    current_agent: &str,
    current_omo: &str,
) -> Result<()> {
    let (_, term_height) = terminal::size()?;
    let max_visible = (term_height as usize).saturating_sub(16);

    execute!(stdout, MoveTo(0, 0), Clear(ClearType::All))?;

    // Big banner
    let banner = ">>> CHANGE ALL MODELS TO: <<<";
    let padding = "=".repeat(banner.len());

    execute!(
        stdout,
        SetForegroundColor(Color::DarkGrey),
        SetAttribute(Attribute::Bold),
        Print(format!("  {}\r\n", padding)),
        SetForegroundColor(Color::Magenta),
        Print(format!("  {}\r\n", banner)),
        SetForegroundColor(Color::DarkGrey),
        Print(format!("  {}\r\n", padding)),
        ResetColor,
        SetAttribute(Attribute::Reset),
    )?;

    // Info about what will change
    execute!(
        stdout,
        Print("\r\n  This will change ALL models to the same selection:\r\n"),
        Print("  - System default (opencode.jsonc)\r\n"),
        Print("  - All 3 agents (local-mind, hoodie, neckbeard)\r\n"),
        Print(format!("\r\n  Current system default: ")),
        SetForegroundColor(Color::Cyan),
        Print(format!("{}\r\n", current_default)),
        ResetColor,
        Print(format!("  Current agent model: ")),
        SetForegroundColor(Color::Cyan),
        Print(format!("{}\r\n", current_agent)),
        ResetColor,
    )?;

    // Instructions
    if filter.is_empty() {
        execute!(
            stdout,
            Print("  Type to filter | ↑↓ navigate | Enter select | Esc cancel\r\n\r\n")
        )?;
    } else {
        execute!(
            stdout,
            Print(format!(
                "  Filter: {} | Backspace to clear | Esc cancel\r\n\r\n",
                filter
            ))
        )?;
    }

    if items.is_empty() {
        execute!(
            stdout,
            SetForegroundColor(Color::Red),
            Print("    No matches\r\n"),
            ResetColor,
        )?;
    } else {
        // Calculate scroll window
        let start = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };
        let end = (start + max_visible).min(items.len());

        for (i, item) in items.iter().enumerate().skip(start).take(end - start) {
            let is_selected = i == selected;
            let is_current =
                *item == current_default || *item == current_agent || *item == current_omo;

            let prefix = match (is_selected, is_current) {
                (true, true) => "  > * ",
                (true, false) => "  >   ",
                (false, true) => "    * ",
                (false, false) => "      ",
            };

            if is_selected {
                execute!(
                    stdout,
                    SetForegroundColor(Color::Green),
                    SetAttribute(Attribute::Bold),
                    Print(format!("{}{}\r\n", prefix, item)),
                    SetAttribute(Attribute::Reset),
                    ResetColor,
                )?;
            } else if is_current {
                execute!(
                    stdout,
                    SetForegroundColor(Color::Yellow),
                    Print(format!("{}{}\r\n", prefix, item)),
                    ResetColor,
                )?;
            } else {
                execute!(stdout, Print(format!("{}{}\r\n", prefix, item)))?;
            }
        }

        // Show scroll indicator if needed
        if items.len() > max_visible {
            execute!(
                stdout,
                SetForegroundColor(Color::DarkGrey),
                Print(format!("\r\n    [{}/{}]\r\n", selected + 1, items.len())),
                ResetColor,
            )?;
        }
    }

    stdout.flush()?;
    Ok(())
}

enum CtxPreset {
    Safe,
    Balanced,
    Aggressive,
}

impl CtxPreset {
    fn fraction(&self) -> f64 {
        match self {
            CtxPreset::Safe => 0.76,
            CtxPreset::Balanced => 0.85,
            CtxPreset::Aggressive => 0.92,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            CtxPreset::Safe => "safe",
            CtxPreset::Balanced => "balanced",
            CtxPreset::Aggressive => "aggressive",
        }
    }
}

fn run_image_on_mode() -> Result<()> {
    let changes = update_image_modality(OPENCODE_CONFIG_PATH, true)?;
    println!("  Added image modality to {} model(s) in {}", changes, OPENCODE_CONFIG_PATH);
    Ok(())
}

fn run_image_off_mode() -> Result<()> {
    let changes = update_image_modality(OPENCODE_CONFIG_PATH, false)?;
    println!("  Removed image modality from {} model(s) in {}", changes, OPENCODE_CONFIG_PATH);
    Ok(())
}

/// Find the byte position just after the closing `}` of the `"limit": { ... }` object on a line.
fn find_limit_object_end(line: &str) -> Option<usize> {
    let limit_key = "\"limit\":";
    let key_pos = line.find(limit_key)?;
    let after_key = key_pos + limit_key.len();
    let brace_offset = line[after_key..].find('{')?;
    let start = after_key + brace_offset;

    let mut depth = 0usize;
    for (i, c) in line[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

pub fn add_image_modality_to_line(line: &str) -> String {
    if !line.contains("\"limit\":") || !line.contains("\"name\":") {
        return line.to_string();
    }
    if line.contains("\"modalities\":") {
        return line.to_string();
    }
    let Some(end) = find_limit_object_end(line) else {
        return line.to_string();
    };
    const MODALITIES: &str = r#", "modalities": { "input": ["text", "image"], "output": ["text"] }"#;
    format!("{}{}{}", &line[..end], MODALITIES, &line[end..])
}

pub fn remove_image_modality_from_line(line: &str) -> String {
    if !line.contains("\"modalities\":") {
        return line.to_string();
    }

    let key = "\"modalities\":";
    let Some(key_pos) = line.find(key) else {
        return line.to_string();
    };
    let after_key = key_pos + key.len();
    let Some(brace_offset) = line[after_key..].find('{') else {
        return line.to_string();
    };
    let brace_start = after_key + brace_offset;

    // Find closing brace of the modalities object
    let mut depth = 0usize;
    let mut obj_end = brace_start;
    for (i, c) in line[brace_start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    obj_end = brace_start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    // Prefer removing the preceding ", " before "modalities"
    let before = &line[..key_pos];
    if let Some(comma_pos) = before.rfind(',') {
        if before[comma_pos + 1..].chars().all(|c| c == ' ') {
            return format!("{}{}", &line[..comma_pos], &line[obj_end..]);
        }
    }

    // Fall back: remove trailing comma after the object
    let after = &line[obj_end..];
    let ws = after.bytes().take_while(|&b| b == b' ').count();
    if after[ws..].starts_with(',') {
        return format!("{}{}", &line[..key_pos], &line[obj_end + ws + 1..]);
    }

    format!("{}{}", &line[..key_pos], &line[obj_end..])
}

pub fn update_image_modality(path: &str, enable: bool) -> Result<usize> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path))?;
    let mut changes = 0usize;

    let new_content: String = content
        .lines()
        .map(|line| {
            let new_line = if enable {
                add_image_modality_to_line(line)
            } else {
                remove_image_modality_from_line(line)
            };
            if new_line != line {
                changes += 1;
            }
            new_line
        })
        .collect::<Vec<_>>()
        .join("\n");

    let new_content = if content.ends_with('\n') {
        new_content + "\n"
    } else {
        new_content
    };

    fs::write(path, &new_content)
        .with_context(|| format!("Failed to write {}", path))?;
    Ok(changes)
}

fn run_ctx_mode(ctx_size: u64, preset: CtxPreset) -> Result<()> {
    let input_limit = (ctx_size as f64 * preset.fraction()) as u64;

    println!("  Context size : {}", format_tokens(ctx_size));
    println!(
        "  Preset       : {} ({}%)",
        preset.name(),
        (preset.fraction() * 100.0) as u64
    );
    println!("  limit.input  : {}", format_tokens(input_limit));
    println!();

    let changes = update_input_limits(OPENCODE_CONFIG_PATH, Some(input_limit))?;
    println!("  Updated {} model(s) in {}", changes, OPENCODE_CONFIG_PATH);

    Ok(())
}

fn run_ctx_off_mode() -> Result<()> {
    let changes = update_input_limits(OPENCODE_CONFIG_PATH, None)?;
    println!(
        "  Removed limit.input from {} model(s) in {}",
        changes, OPENCODE_CONFIG_PATH
    );
    Ok(())
}

fn format_tokens(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

pub fn update_input_limits(path: &str, input_limit: Option<u64>) -> Result<usize> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path))?;
    let mut changes = 0usize;

    let new_content: String = content
        .lines()
        .map(|line| {
            if !line.contains("\"limit\":") {
                return line.to_string();
            }
            let new_line = update_line_input_limit(line, input_limit);
            if new_line != line {
                changes += 1;
            }
            new_line
        })
        .collect::<Vec<_>>()
        .join("\n");

    let new_content = if content.ends_with('\n') {
        new_content + "\n"
    } else {
        new_content
    };

    fs::write(path, &new_content)
        .with_context(|| format!("Failed to write {}", path))?;
    Ok(changes)
}

pub fn update_line_input_limit(line: &str, input_limit: Option<u64>) -> String {
    if !line.contains("\"limit\":") {
        return line.to_string();
    }

    let has_input = line.contains("\"input\":");

    match input_limit {
        Some(val) => {
            if has_input {
                replace_input_value(line, val)
            } else {
                add_input_to_limit(line, val)
            }
        }
        None => {
            if has_input {
                remove_input_from_limit(line)
            } else {
                line.to_string()
            }
        }
    }
}

fn replace_input_value(line: &str, val: u64) -> String {
    let key = "\"input\":";
    let Some(key_pos) = line.find(key) else {
        return line.to_string();
    };
    let after_key = key_pos + key.len();
    let ws_count = line[after_key..].bytes().take_while(|&b| b == b' ').count();
    let digit_start = after_key + ws_count;
    let digit_len = line[digit_start..]
        .bytes()
        .take_while(|b| b.is_ascii_digit())
        .count();
    format!("{}{}{}", &line[..digit_start], val, &line[digit_start + digit_len..])
}

fn add_input_to_limit(line: &str, val: u64) -> String {
    let limit_key = "\"limit\":";
    let Some(limit_pos) = line.find(limit_key) else {
        return line.to_string();
    };
    let after_limit = limit_pos + limit_key.len();
    let Some(brace_offset) = line[after_limit..].find('{') else {
        return line.to_string();
    };
    let content_start = after_limit + brace_offset + 1;
    let Some(close_offset) = line[content_start..].find('}') else {
        return line.to_string();
    };
    let close_pos = content_start + close_offset;
    format!(
        "{}, \"input\": {}{}",
        &line[..close_pos],
        val,
        &line[close_pos..]
    )
}

fn remove_input_from_limit(line: &str) -> String {
    let key = "\"input\":";
    let Some(key_pos) = line.find(key) else {
        return line.to_string();
    };
    let after_key = key_pos + key.len();
    let ws_count = line[after_key..].bytes().take_while(|&b| b == b' ').count();
    let digit_start = after_key + ws_count;
    let digit_len = line[digit_start..]
        .bytes()
        .take_while(|b| b.is_ascii_digit())
        .count();
    let after_num = digit_start + digit_len;

    // Prefer removing the preceding comma: ..., "input": NNN
    if let Some(comma_pos) = line[..key_pos].rfind(',') {
        let between = &line[comma_pos + 1..key_pos];
        if between.chars().all(|c| c == ' ') {
            return format!("{}{}", &line[..comma_pos], &line[after_num..]);
        }
    }

    // Fall back to removing the following comma: "input": NNN, ...
    let after = &line[after_num..];
    let ws_after = after.bytes().take_while(|&b| b == b' ').count();
    if after[ws_after..].starts_with(',') {
        let comma_end = after_num + ws_after + 1;
        return format!("{}{}", &line[..key_pos], &line[comma_end..]);
    }

    format!("{}{}", &line[..key_pos], &line[after_num..])
}

pub fn read_models(path: &str) -> Result<Vec<String>> {
    let content = fs::read_to_string(path).with_context(|| format!("Failed to read {}", path))?;
    Ok(content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .collect())
}

pub fn get_current_model(path: &str) -> Result<String> {
    let content = fs::read_to_string(path)?;
    for line in content.lines() {
        if line.trim().starts_with("model:") {
            if let Some(m) = line.split(':').nth(1) {
                return Ok(m.trim().to_string());
            }
        }
    }
    anyhow::bail!("No model line found")
}

/// Get the default model from opencode.jsonc using line-based parsing to preserve JSONC comments
pub fn get_default_model(path: &str) -> Result<String> {
    let content = fs::read_to_string(path)?;
    for line in content.lines() {
        let trimmed = line.trim();
        // Match "model": "value" pattern
        if trimmed.starts_with("\"model\"") {
            // Extract the value after the colon
            if let Some(colon_idx) = trimmed.find(':') {
                let value_part = trimmed[colon_idx + 1..].trim();
                // Remove quotes and trailing comma
                let value = value_part
                    .trim_start_matches('"')
                    .trim_end_matches(',')
                    .trim_end_matches('"');
                return Ok(value.to_string());
            }
        }
    }
    anyhow::bail!("No model key found in {}", path)
}

/// Update the default model in opencode.jsonc using line-based replacement to preserve JSONC comments
pub fn update_default_model(path: &str, model: &str) -> Result<()> {
    let content = fs::read_to_string(path)?;
    let mut found = false;

    let new_content: String = content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("\"model\"") {
                found = true;
                // Preserve leading whitespace
                let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                format!("{}\"model\": \"{}\",", leading_ws, model)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    if !found {
        anyhow::bail!("No model key found in {}", path);
    }

    let new_content = if content.ends_with('\n') {
        new_content + "\n"
    } else {
        new_content
    };

    fs::write(path, new_content)?;
    Ok(())
}

pub fn update_agent_model(path: &str, model: &str) -> Result<()> {
    let content = fs::read_to_string(path)?;
    let mut found = false;

    for line in content.lines() {
        if line.trim().starts_with("model:") {
            found = true;
            break;
        }
    }

    if !found {
        anyhow::bail!("No model line found in {}", path);
    }

    let new_content: String = content
        .lines()
        .map(|l| {
            if l.trim().starts_with("model:") {
                format!("model: {}", model)
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let new_content = if content.ends_with('\n') {
        new_content + "\n"
    } else {
        new_content
    };

    fs::write(path, new_content)?;
    Ok(())
}

fn select_default_model(
    models: &[String],
    current: &str,
    step: usize,
    total: usize,
) -> Result<Option<String>> {
    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, Hide)?;

    let result = run_default_selector(&mut stdout, models, current, step, total);

    execute!(stdout, Show, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    result
}

fn run_default_selector(
    stdout: &mut io::Stdout,
    models: &[String],
    current: &str,
    step: usize,
    total: usize,
) -> Result<Option<String>> {
    let mut filter = String::new();
    let mut selected: usize = 0;
    let mut initialized = false;

    loop {
        let filtered: Vec<&String> = models
            .iter()
            .filter(|m| filter.is_empty() || m.to_lowercase().contains(&filter.to_lowercase()))
            .collect();

        // Clamp selection
        if selected >= filtered.len() {
            selected = filtered.len().saturating_sub(1);
        }

        // Set initial selection to current model only once
        if !initialized && filter.is_empty() {
            if let Some(idx) = filtered.iter().position(|m| *m == current) {
                selected = idx;
            }
            initialized = true;
        }

        draw_default(stdout, &filtered, selected, &filter, current, step, total)?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                }
                KeyCode::Down => {
                    if selected + 1 < filtered.len() {
                        selected += 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some(m) = filtered.get(selected) {
                        return Ok(Some((*m).clone()));
                    }
                }
                KeyCode::Esc => {
                    return Ok(None);
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None);
                }
                KeyCode::Char(c) => {
                    filter.push(c);
                    selected = 0;
                    initialized = true;
                }
                KeyCode::Backspace => {
                    filter.pop();
                    selected = 0;
                }
                _ => {}
            }
        }
    }
}

fn draw_default(
    stdout: &mut io::Stdout,
    items: &[&String],
    selected: usize,
    filter: &str,
    current: &str,
    step: usize,
    total: usize,
) -> Result<()> {
    let (_, term_height) = terminal::size()?;
    let max_visible = (term_height as usize).saturating_sub(12);

    execute!(stdout, MoveTo(0, 0), Clear(ClearType::All))?;

    // Big banner for system default
    let banner = ">>> SELECTING SYSTEM DEFAULT MODEL <<<";
    let padding = "=".repeat(banner.len());

    execute!(
        stdout,
        SetForegroundColor(Color::DarkGrey),
        SetAttribute(Attribute::Bold),
        Print(format!("  {}\r\n", padding)),
        SetForegroundColor(Color::Magenta),
        Print(format!("  {}\r\n", banner)),
        SetForegroundColor(Color::DarkGrey),
        Print(format!("  {}\r\n", padding)),
        ResetColor,
        SetAttribute(Attribute::Reset),
    )?;

    // Progress and current model
    execute!(
        stdout,
        Print(format!("\r\n  Step {}/{} | Current: ", step, total)),
        SetForegroundColor(Color::Cyan),
        Print(format!("{}\r\n", current)),
        ResetColor,
    )?;

    // Instructions
    if filter.is_empty() {
        execute!(
            stdout,
            Print("  Type to filter | ↑↓ navigate | Enter select | Esc ABORT ALL\r\n\r\n")
        )?;
    } else {
        execute!(
            stdout,
            Print(format!(
                "  Filter: {} | Backspace to clear | Esc ABORT ALL\r\n\r\n",
                filter
            ))
        )?;
    }

    if items.is_empty() {
        execute!(
            stdout,
            SetForegroundColor(Color::Red),
            Print("    No matches\r\n"),
            ResetColor,
        )?;
    } else {
        // Calculate scroll window
        let start = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };
        let end = (start + max_visible).min(items.len());

        for (i, item) in items.iter().enumerate().skip(start).take(end - start) {
            let is_selected = i == selected;
            let is_current = *item == current;

            let prefix = match (is_selected, is_current) {
                (true, true) => "  > * ",
                (true, false) => "  >   ",
                (false, true) => "    * ",
                (false, false) => "      ",
            };

            if is_selected {
                execute!(
                    stdout,
                    SetForegroundColor(Color::Green),
                    SetAttribute(Attribute::Bold),
                    Print(format!("{}{}\r\n", prefix, item)),
                    SetAttribute(Attribute::Reset),
                    ResetColor,
                )?;
            } else if is_current {
                execute!(
                    stdout,
                    SetForegroundColor(Color::Yellow),
                    Print(format!("{}{}\r\n", prefix, item)),
                    ResetColor,
                )?;
            } else {
                execute!(stdout, Print(format!("{}{}\r\n", prefix, item)))?;
            }
        }

        // Show scroll indicator if needed
        if items.len() > max_visible {
            execute!(
                stdout,
                SetForegroundColor(Color::DarkGrey),
                Print(format!("\r\n    [{}/{}]\r\n", selected + 1, items.len())),
                ResetColor,
            )?;
        }
    }

    stdout.flush()?;
    Ok(())
}

fn select_model(
    models: &[String],
    current: &str,
    agent_name: &str,
    step: usize,
    total: usize,
    previous_selections: &[(&str, &str, String, String)],
    default_selection: &Option<(String, String)>,
) -> Result<Option<String>> {
    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, Hide)?;

    let result = run_selector(
        &mut stdout,
        models,
        current,
        agent_name,
        step,
        total,
        previous_selections,
        default_selection,
    );

    execute!(stdout, Show, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    result
}

fn run_selector(
    stdout: &mut io::Stdout,
    models: &[String],
    current: &str,
    agent_name: &str,
    step: usize,
    total: usize,
    previous_selections: &[(&str, &str, String, String)],
    default_selection: &Option<(String, String)>,
) -> Result<Option<String>> {
    let mut filter = String::new();
    let mut selected: usize = 0;
    let mut initialized = false;

    loop {
        let filtered: Vec<&String> = models
            .iter()
            .filter(|m| filter.is_empty() || m.to_lowercase().contains(&filter.to_lowercase()))
            .collect();

        // Clamp selection
        if selected >= filtered.len() {
            selected = filtered.len().saturating_sub(1);
        }

        // Set initial selection to current model only once
        if !initialized && filter.is_empty() {
            if let Some(idx) = filtered.iter().position(|m| *m == current) {
                selected = idx;
            }
            initialized = true;
        }

        draw(
            stdout,
            &filtered,
            selected,
            &filter,
            current,
            agent_name,
            step,
            total,
            previous_selections,
            default_selection,
        )?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                }
                KeyCode::Down => {
                    if selected + 1 < filtered.len() {
                        selected += 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some(m) = filtered.get(selected) {
                        return Ok(Some((*m).clone()));
                    }
                }
                KeyCode::Esc => {
                    return Ok(None);
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None);
                }
                KeyCode::Char(c) => {
                    filter.push(c);
                    selected = 0;
                    initialized = true; // Don't reset to current after user starts filtering
                }
                KeyCode::Backspace => {
                    filter.pop();
                    selected = 0;
                }
                _ => {}
            }
        }
    }
}

fn draw(
    stdout: &mut io::Stdout,
    items: &[&String],
    selected: usize,
    filter: &str,
    current: &str,
    agent_name: &str,
    step: usize,
    total: usize,
    previous_selections: &[(&str, &str, String, String)],
    default_selection: &Option<(String, String)>,
) -> Result<()> {
    let (_, term_height) = terminal::size()?;
    let default_lines = if default_selection.is_some() { 1 } else { 0 };
    let ledger_lines = previous_selections.len()
        + default_lines
        + if previous_selections.is_empty() && default_selection.is_none() {
            0
        } else {
            2
        };
    let max_visible = (term_height as usize).saturating_sub(12 + ledger_lines);

    execute!(stdout, MoveTo(0, 0), Clear(ClearType::All))?;

    // Show previous selections ledger (including system default)
    if default_selection.is_some() || !previous_selections.is_empty() {
        execute!(
            stdout,
            SetForegroundColor(Color::DarkGrey),
            Print("  ── Previous selections ──\r\n"),
            ResetColor,
        )?;

        // Show system default first
        if let Some((ref old, ref new)) = default_selection {
            if old == new {
                execute!(
                    stdout,
                    Print(format!("    system default : {} (unchanged)\r\n", new)),
                )?;
            } else {
                execute!(
                    stdout,
                    Print("    system default : "),
                    SetForegroundColor(Color::Red),
                    Print(format!("{}", old)),
                    ResetColor,
                    Print(" → "),
                    SetForegroundColor(Color::Green),
                    Print(format!("{}\r\n", new)),
                    ResetColor,
                )?;
            }
        }

        // Show agent selections
        for (name, _, old, new) in previous_selections {
            if old == new {
                execute!(
                    stdout,
                    Print(format!("    {} : {} (unchanged)\r\n", name, new)),
                )?;
            } else {
                execute!(
                    stdout,
                    Print(format!("    {} : ", name)),
                    SetForegroundColor(Color::Red),
                    Print(format!("{}", old)),
                    ResetColor,
                    Print(" → "),
                    SetForegroundColor(Color::Green),
                    Print(format!("{}\r\n", new)),
                    ResetColor,
                )?;
            }
        }
        execute!(stdout, Print("\r\n"))?;
    }

    // Big banner for current agent
    let banner = format!(">>> SELECTING MODEL FOR: {} <<<", agent_name.to_uppercase());
    let padding = "=".repeat(banner.len());

    execute!(
        stdout,
        SetForegroundColor(Color::DarkGrey),
        SetAttribute(Attribute::Bold),
        Print(format!("  {}\r\n", padding)),
        SetForegroundColor(Color::Yellow),
        Print(format!("  {}\r\n", banner)),
        SetForegroundColor(Color::DarkGrey),
        Print(format!("  {}\r\n", padding)),
        ResetColor,
        SetAttribute(Attribute::Reset),
    )?;

    // Progress and current model
    execute!(
        stdout,
        Print(format!("\r\n  Step {}/{} | Current: ", step, total)),
        SetForegroundColor(Color::Cyan),
        Print(format!("{}\r\n", current)),
        ResetColor,
    )?;

    // Instructions
    if filter.is_empty() {
        execute!(
            stdout,
            Print("  Type to filter | ↑↓ navigate | Enter select | Esc ABORT ALL\r\n\r\n")
        )?;
    } else {
        execute!(
            stdout,
            Print(format!(
                "  Filter: {} | Backspace to clear | Esc ABORT ALL\r\n\r\n",
                filter
            ))
        )?;
    }

    if items.is_empty() {
        execute!(
            stdout,
            SetForegroundColor(Color::Red),
            Print("    No matches\r\n"),
            ResetColor,
        )?;
    } else {
        // Calculate scroll window
        let start = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };
        let end = (start + max_visible).min(items.len());

        for (i, item) in items.iter().enumerate().skip(start).take(end - start) {
            let is_selected = i == selected;
            let is_current = *item == current;

            let prefix = match (is_selected, is_current) {
                (true, true) => "  > * ",
                (true, false) => "  >   ",
                (false, true) => "    * ",
                (false, false) => "      ",
            };

            if is_selected {
                execute!(
                    stdout,
                    SetForegroundColor(Color::Green),
                    SetAttribute(Attribute::Bold),
                    Print(format!("{}{}\r\n", prefix, item)),
                    SetAttribute(Attribute::Reset),
                    ResetColor,
                )?;
            } else if is_current {
                execute!(
                    stdout,
                    SetForegroundColor(Color::Yellow),
                    Print(format!("{}{}\r\n", prefix, item)),
                    ResetColor,
                )?;
            } else {
                execute!(stdout, Print(format!("{}{}\r\n", prefix, item)))?;
            }
        }

        // Show scroll indicator if needed
        if items.len() > max_visible {
            execute!(
                stdout,
                SetForegroundColor(Color::DarkGrey),
                Print(format!("\r\n    [{}/{}]\r\n", selected + 1, items.len())),
                ResetColor,
            )?;
        }
    }

    stdout.flush()?;
    Ok(())
}

fn confirm_changes(
    selections: &[(&str, &str, String, String)],
    default_selection: &Option<(String, String)>,
) -> Result<bool> {
    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, Hide)?;

    let result = run_confirm(&mut stdout, selections, default_selection);

    execute!(stdout, Show, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    result
}

fn run_confirm(
    stdout: &mut io::Stdout,
    selections: &[(&str, &str, String, String)],
    default_selection: &Option<(String, String)>,
) -> Result<bool> {
    loop {
        execute!(stdout, MoveTo(0, 0), Clear(ClearType::All))?;

        execute!(
            stdout,
            SetForegroundColor(Color::Yellow),
            SetAttribute(Attribute::Bold),
            Print("\r\n  ========== CONFIRM CHANGES ==========\r\n\r\n"),
            SetAttribute(Attribute::Reset),
            ResetColor,
        )?;

        let mut has_changes = false;

        // Show system default first
        if let Some((ref old, ref new)) = default_selection {
            if old == new {
                execute!(
                    stdout,
                    Print(format!("    system default : {} (unchanged)\r\n", new)),
                )?;
            } else {
                has_changes = true;
                execute!(
                    stdout,
                    Print("    system default : "),
                    SetForegroundColor(Color::Red),
                    Print(format!("{}", old)),
                    ResetColor,
                    Print(" → "),
                    SetForegroundColor(Color::Green),
                    Print(format!("{}\r\n", new)),
                    ResetColor,
                )?;
            }
        }

        // Show agent selections
        for (name, _, old, new) in selections {
            if old == new {
                execute!(
                    stdout,
                    Print(format!("    {} : {} (unchanged)\r\n", name, new)),
                )?;
            } else {
                has_changes = true;
                execute!(
                    stdout,
                    Print(format!("    {} : ", name)),
                    SetForegroundColor(Color::Red),
                    Print(format!("{}", old)),
                    ResetColor,
                    Print(" → "),
                    SetForegroundColor(Color::Green),
                    Print(format!("{}\r\n", new)),
                    ResetColor,
                )?;
            }
        }

        execute!(stdout, Print("\r\n"))?;

        if has_changes {
            execute!(
                stdout,
                Print("  Apply these changes? ["),
                SetForegroundColor(Color::Green),
                SetAttribute(Attribute::Bold),
                Print("Y"),
                SetAttribute(Attribute::Reset),
                ResetColor,
                Print("/n] (Enter = Yes): "),
            )?;
        } else {
            execute!(
                stdout,
                SetForegroundColor(Color::DarkGrey),
                Print("  No changes to apply. Press Enter or Esc to exit.\r\n"),
                ResetColor,
            )?;
        }

        stdout.flush()?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    return Ok(true);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    return Ok(false);
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(false);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests;
