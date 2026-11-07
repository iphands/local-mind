mod config;

use anyhow::{Context, Result};
use config::*;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::env;
use std::io::{self, Write};

fn print_usage() {
    eprintln!("Usage: model-switcher <COMMAND>");
    eprintln!();
    eprintln!("Edits opencode 2's config under ./container_data/config/opencode (run from opencode/;");
    eprintln!("bin/model-switcher does that for you).");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  lm, local-mind        Interactive: system default model, then each agent's model (default)");
    eprintln!("  all                   Interactive: set the system default and every agent to one model");
    eprintln!("  status                Show the default model, agent models, and each model's limits/image input");
    eprintln!("  ctx <SIZE> <PRESET>   Set limit.context and limit.input on all models (safe/balanced/aggressive)");
    eprintln!("  ctx off               Remove limit.input from all models (limit.context stays)");
    eprintln!("  image [on]            Add \"image\" to capabilities.input on all models (default: on)");
    eprintln!("  image off             Remove \"image\" from capabilities.input on all models");
    eprintln!("  help                  Show this help message");
    eprintln!();
    eprintln!("ctx can be followed by an image command: ctx 262144 balanced image on");
    eprintln!("Models come from the providers block of opencode.jsonc; agents are agents/*.md with a model: line.");
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("lm");

    if matches!(cmd, "help" | "-h" | "--help") {
        print_usage();
        return Ok(());
    }
    if matches!(
        cmd,
        "omo" | "oh-my-opencode" | "restore-omo-defaults" | "vision"
    ) {
        eprintln!(
            "'{}' managed oh-my-openagent, which opencode 2 doesn't use. Nothing to switch.",
            cmd
        );
        std::process::exit(1);
    }

    check_layout()?;

    match cmd {
        "lm" | "local-mind" => run_agent_mode(),
        "all" => run_all_mode(),
        "status" => run_status(),
        "image" => run_image(args.get(2).map(String::as_str).unwrap_or("on")),
        "ctx" => {
            if args.len() >= 3 && args[2] == "off" {
                return run_ctx_off_mode();
            }
            if args.len() < 4 {
                eprintln!("Usage:");
                eprintln!("  model-switcher ctx <SIZE> <PRESET> [image on|off]");
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
            if args.get(4).map(String::as_str) == Some("image") {
                run_image(args.get(5).map(String::as_str).unwrap_or("on"))?;
            }
            Ok(())
        }
        other => {
            eprintln!("Unknown command: {}", other);
            print_usage();
            std::process::exit(1);
        }
    }
}

fn load_models() -> Result<Vec<String>> {
    let models = read_models(OPENCODE_CONFIG_PATH)?;
    if models.is_empty() {
        anyhow::bail!("No models declared under providers in {}", OPENCODE_CONFIG_PATH);
    }
    Ok(models)
}

fn run_agent_mode() -> Result<()> {
    let models = load_models()?;
    let agents = discover_agents(AGENTS_DIR)?;

    let total_steps = agents.len() + 1; // +1 for system default

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

    for (i, agent) in agents.iter().enumerate() {
        let current = get_current_model(&agent.path).unwrap_or_else(|_| "unknown".to_string());

        match select_model(
            &models,
            &current,
            &agent.name,
            i + 2,
            total_steps,
            &selections,
            &default_selection,
        )? {
            Some(selected) => {
                selections.push((&agent.name, &agent.path, current, selected));
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

    if let Some((ref old, ref new)) = default_selection {
        if old != new {
            update_default_model(OPENCODE_CONFIG_PATH, new)?;
            println!("Updated system default -> {}", new);
            changes += 1;
        } else {
            println!("system default: no change");
        }
    }

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
    let models = load_models()?;
    let agents = discover_agents(AGENTS_DIR)?;

    let current_default =
        get_default_model(OPENCODE_CONFIG_PATH).unwrap_or_else(|_| "unknown".to_string());
    let current_agent = agents
        .first()
        .and_then(|a| get_current_model(&a.path).ok())
        .unwrap_or_else(|| "unknown".to_string());
    let agent_names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();

    let Some(selected) = select_all_model(&models, &current_default, &current_agent, &agent_names)?
    else {
        println!("Aborted. No changes made.");
        return Ok(());
    };

    let mut changes = 0;

    if current_default != selected {
        update_default_model(OPENCODE_CONFIG_PATH, &selected)?;
        println!("Updated system default -> {}", selected);
        changes += 1;
    } else {
        println!("system default: no change");
    }

    for agent in &agents {
        let current = get_current_model(&agent.path).unwrap_or_else(|_| "unknown".to_string());
        if current != selected {
            update_agent_model(&agent.path, &selected)?;
            println!("Updated {} -> {}", agent.name, selected);
            changes += 1;
        } else {
            println!("{}: no change", agent.name);
        }
    }

    if changes == 0 {
        println!("No changes made.");
    } else {
        println!("Updated ALL configurations to: {}", selected);
    }

    Ok(())
}

fn run_status() -> Result<()> {
    let default = get_default_model(OPENCODE_CONFIG_PATH)?;
    println!("Config: {}", OPENCODE_CONFIG_PATH);
    println!();
    println!("  system default : {}", default);
    for agent in discover_agents(AGENTS_DIR)? {
        let model = get_current_model(&agent.path)?;
        println!("  {:<14} : {}", agent.name, model);
    }
    for name in unpinned_agents(AGENTS_DIR)? {
        println!("  {:<14} : (inherits default)", name);
    }
    println!();

    let opt = |v: Option<u64>| v.map(format_tokens).unwrap_or_else(|| "-".to_string());
    let infos = read_model_info(OPENCODE_CONFIG_PATH)?;
    let width = infos.iter().map(|m| m.id.len()).max().unwrap_or(5).max(5);
    println!(
        "  {:<width$}  {:>9}  {:>9}  {:>9}  image",
        "model", "context", "input", "output"
    );
    for m in &infos {
        let pct = match (m.input, m.context) {
            (Some(i), Some(c)) if c > 0 => format!(" ({}%)", (i * 100 + c / 2) / c),
            _ => String::new(),
        };
        println!(
            "  {:<width$}  {:>9}  {:>9}  {:>9}  {}{}",
            m.id,
            opt(m.context),
            opt(m.input),
            opt(m.output),
            if m.image { "yes" } else { "no" },
            pct,
        );
    }
    Ok(())
}

fn run_image(sub: &str) -> Result<()> {
    let enable = match sub {
        "on" => true,
        "off" => false,
        other => {
            eprintln!("Unknown image subcommand: '{}'. Valid: on, off", other);
            std::process::exit(1);
        }
    };
    let changes = update_image(OPENCODE_CONFIG_PATH, enable)?;
    let verb = if enable { "Added image input to" } else { "Removed image input from" };
    println!("  {} {} model(s) in {}", verb, changes, OPENCODE_CONFIG_PATH);
    let wrong: Vec<String> = read_model_info(OPENCODE_CONFIG_PATH)?
        .into_iter()
        .filter(|m| m.image != enable)
        .map(|m| m.id)
        .collect();
    if !wrong.is_empty() {
        eprintln!("  WARNING: not updated (not a one-line model entry?): {}", wrong.join(", "));
    }
    Ok(())
}

fn run_ctx_mode(ctx_size: u64, preset: CtxPreset) -> Result<()> {
    let input_limit = (ctx_size as f64 * preset.fraction()) as u64;

    println!("  Context size : {}", format_tokens(ctx_size));
    println!(
        "  Preset       : {} ({}%)",
        preset.name(),
        (preset.fraction() * 100.0) as u64
    );
    println!("  limit.context: {}", format_tokens(ctx_size));
    println!(
        "  limit.input  : {}  ({}% of context)",
        format_tokens(input_limit),
        (preset.fraction() * 100.0) as u64
    );
    println!();

    let changes = update_limits(OPENCODE_CONFIG_PATH, Some(ctx_size), Some(input_limit))?;
    println!("  Updated {} model(s) in {}", changes, OPENCODE_CONFIG_PATH);
    warn_unapplied_limits(Some(ctx_size), Some(input_limit))
}

fn run_ctx_off_mode() -> Result<()> {
    let changes = update_limits(OPENCODE_CONFIG_PATH, None, None)?;
    println!(
        "  Removed limit.input from {} model(s) in {}",
        changes, OPENCODE_CONFIG_PATH
    );
    warn_unapplied_limits(None, None)
}

/// Re-read the config and name any model whose limits didn't end up as intended
/// (e.g. a multi-line model entry the line editor can't see).
fn warn_unapplied_limits(context: Option<u64>, input: Option<u64>) -> Result<()> {
    let wrong: Vec<String> = read_model_info(OPENCODE_CONFIG_PATH)?
        .into_iter()
        .filter(|m| (context.is_some() && m.context != context) || m.input != input)
        .map(|m| m.id)
        .collect();
    if !wrong.is_empty() {
        eprintln!(
            "  WARNING: not updated (not a one-line model entry?): {}",
            wrong.join(", ")
        );
    }
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

fn select_all_model(
    models: &[String],
    current_default: &str,
    current_agent: &str,
    agent_names: &[&str],
) -> Result<Option<String>> {
    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, Hide)?;

    let result = run_all_selector(
        &mut stdout,
        models,
        current_default,
        current_agent,
        agent_names,
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
    agent_names: &[&str],
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
            agent_names,
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
    agent_names: &[&str],
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
        Print(format!(
            "  - All {} agents ({})\r\n",
            agent_names.len(),
            agent_names.join(", ")
        )),
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
                *item == current_default || *item == current_agent;

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
