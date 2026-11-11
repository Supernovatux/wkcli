mod answer;
mod api;
mod audio;
mod config;
mod demo;
mod gfx;
mod kana;
mod model;
mod session;
mod ui;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use config::Config;
use rand::prelude::*;
use std::io::{IsTerminal, Write};
use std::sync::Arc;

#[derive(Parser)]
#[command(version, about = "Unofficial WaniKani reviews in the terminal")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Review for real, but do not submit any results to WaniKani
    #[arg(long, global = true)]
    dry_run: bool,
    /// Practice with a few built-in items; no account or network needed
    #[arg(long, global = true)]
    demo: bool,
    /// Review at most this many items
    #[arg(long, global = true)]
    limit: Option<usize>,
}

#[derive(Subcommand)]
enum Command {
    /// Do the reviews that are available now (default)
    Review,
    /// Show how many reviews and lessons are waiting
    Status,
    /// Store a WaniKani API token
    Login,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut cfg = Config::load()?;
    match cli.command {
        Some(Command::Login) => login(&mut cfg).map(|_| ()),
        Some(Command::Status) => status(&mut cfg),
        Some(Command::Review) | None => review(&cli, &mut cfg),
    }
}

/// Ask for a token, verify it and save it.
fn login(cfg: &mut Config) -> Result<api::Client> {
    if !std::io::stdin().is_terminal() {
        bail!("no API token configured; set WANIKANI_API_TOKEN or run `wkcli login`");
    }
    println!("Create a personal access token at https://www.wanikani.com/settings/personal_access_tokens");
    println!("Tick the “reviews:create” permission, otherwise results cannot be submitted.");
    print!("API token: ");
    std::io::stdout().flush()?;
    let mut token = String::new();
    std::io::stdin().read_line(&mut token)?;
    let token = token.trim().to_string();
    if token.is_empty() {
        bail!("no token entered");
    }
    let client = api::Client::new(token.clone());
    let user = client.user()?;
    cfg.api_token = Some(token);
    cfg.save()?;
    println!("Logged in as {} (level {}). Token saved to {}", user.username, user.level, config::config_path()?.display());
    Ok(client)
}

fn client(cfg: &mut Config) -> Result<api::Client> {
    match cfg.token() {
        Some(token) => Ok(api::Client::new(token)),
        None => login(cfg),
    }
}

fn status(cfg: &mut Config) -> Result<()> {
    let client = client(cfg)?;
    let user = client.user()?;
    let summary = client.summary()?;
    let now_count = |slots: &[api::SummarySlot]| slots.first().map(|s| s.subject_ids.len()).unwrap_or(0);
    println!("{} · level {}", user.username, user.level);
    println!("Reviews available: {}", now_count(&summary.reviews));
    println!("Lessons available: {}", now_count(&summary.lessons));
    if let Some(slot) = summary.reviews.iter().skip(1).find(|s| !s.subject_ids.is_empty()) {
        println!("Next reviews: {} at {}", slot.subject_ids.len(), slot.available_at);
    } else if let Some(next) = summary.next_reviews_at {
        println!("Next reviews at {next}");
    }
    Ok(())
}

fn review(cli: &Cli, cfg: &mut Config) -> Result<()> {
    if !std::io::stdout().is_terminal() {
        bail!("reviews need an interactive terminal");
    }
    let (mut items, api_client) = if cli.demo {
        (demo::items(), None)
    } else {
        let client = client(cfg)?;
        let items = client.load_reviews(|msg| {
            eprint!("\r\x1b[K{msg}");
            let _ = std::io::stderr().flush();
        })?;
        eprint!("\r\x1b[K");
        (items, Some(Arc::new(client)))
    };
    if items.is_empty() {
        println!("No reviews available right now. `wkcli status` shows when the next ones arrive.");
        return Ok(());
    }
    if let Some(limit) = cli.limit {
        items.shuffle(&mut rand::rng());
        items.truncate(limit.max(1));
    }

    let submitter = match api_client {
        Some(client) if !cli.dry_run => Some(api::spawn_submitter(client)),
        _ => None,
    };
    let report = ui::run(items, cfg, submitter.as_ref())?;

    let s = &report.session;
    let done = s.completed.len();
    let clean = s.completed.iter().filter(|&&i| !s.items[i].had_mistakes()).count();
    println!("Reviewed {done} of {} items, {clean} without mistakes.", s.total());

    let mut failures = report.failures;
    match submitter {
        Some(sub) => {
            if report.pending > 0 {
                println!("Submitting {} remaining review(s)…", report.pending);
            }
            drop(sub.tx);
            for res in sub.results.iter() {
                if let Some(err) = res.error {
                    failures.push(format!("{}: {err}", res.label));
                }
            }
            sub.handle.join().ok().context("submission thread crashed")?;
            if failures.is_empty() {
                if done > 0 {
                    println!("All results submitted to WaniKani.");
                }
            } else {
                eprintln!("{} review(s) could not be submitted and are still pending on WaniKani:", failures.len());
                for f in &failures {
                    eprintln!("  {f}");
                }
                std::process::exit(1);
            }
        }
        None => println!("Nothing was submitted to WaniKani."),
    }
    Ok(())
}
