use downloads_khinsider_com_dl as lib;
use lib::{config::Config, event::Event};

use anyhow::Result;
use clap::Parser;
use crossterm::{cursor, execute, terminal::{Clear, ClearType}};
use std::collections::HashMap;
use std::io::Write;

#[derive(Parser)]
#[command(about = "TODO: fill in description")]
struct Args {
    /// Link to the album page on downloads.khinsider.com (example https://downloads.khinsider.com/game-soundtracks/album/synthetik-2-windows-gamerip-2021)
    url: String,

    /// Download flacs. Download MP3 if not set.
    #[arg(short = 'f', long = "flac")]
    flac: bool,

    /// TODO: fill in --images flag description
    #[arg(short = 'i', long = "images")]
    images: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = parse_args()?;
    let (tx, rx) = std::sync::mpsc::channel::<Event>();
    let handle = tokio::task::spawn_blocking(move || {
        let mut state = ProgressState::default();
        for event in rx {
            handle_event(event, &mut state);
        }
    });

    lib::run(config, tx).await?;
    handle.await?;
    Ok(())
}

fn parse_args() -> Result<Config> {
    let args = Args::parse();
    Ok(Config {
        url: args.url,
        flac: args.flac,
        images: args.images,
    })
}

#[derive(Default)]
struct ProgressState {
    total: usize,
    completed: usize,
    failed: usize,
    /// Each slot holds the display name of an active download, or None if free.
    slots: Vec<Option<String>>,
    id_to_slot: HashMap<usize, usize>,
    /// How many lines the progress block currently occupies on screen.
    lines_printed: usize,
}

impl ProgressState {
    fn alloc_slot(&mut self, id: usize, name: String) {
        let slot = self.slots.iter().position(|s| s.is_none()).unwrap_or_else(|| {
            self.slots.push(None);
            self.slots.len() - 1
        });
        self.slots[slot] = Some(name);
        self.id_to_slot.insert(id, slot);
    }

    fn free_slot(&mut self, id: usize) {
        if let Some(&slot) = self.id_to_slot.get(&id) {
            self.slots[slot] = None;
            self.id_to_slot.remove(&id);
        }
    }

    fn render(&mut self) {
        let mut stdout = std::io::stdout();

        if self.lines_printed > 0 {
            let _ = execute!(stdout, cursor::MoveUp(self.lines_printed as u16));
        }

        // Summary line
        let _ = execute!(stdout, Clear(ClearType::CurrentLine));
        if self.failed > 0 {
            println!("[{}/{}] complete, {} failed", self.completed, self.total, self.failed);
        } else {
            println!("[{}/{}] complete", self.completed, self.total);
        }

        // One line per slot
        for slot in &self.slots {
            let _ = execute!(stdout, Clear(ClearType::CurrentLine));
            match slot {
                Some(name) => println!("  {name}"),
                None => println!(),
            }
        }

        let _ = stdout.flush();
        self.lines_printed = 1 + self.slots.len();
    }
}

fn handle_event(event: Event, state: &mut ProgressState) {
    match event {
        Event::GetPageStarted => println!("Fetching page..."),
        Event::GetPageCompleted => println!("Page fetched."),
        Event::TotalDownloads(n) => {
            state.total = n;
            state.render();
        }
        Event::DlStarted { id, name } => {
            state.alloc_slot(id, name);
            state.render();
        }
        Event::DlCompleted { id } => {
            state.completed += 1;
            state.free_slot(id);
            state.render();
        }
        Event::DlFailed { id, error } => {
            state.failed += 1;
            state.free_slot(id);
            // Print error above the progress block by moving up, printing, then re-rendering.
            if state.lines_printed > 0 {
                let _ = execute!(
                    std::io::stdout(),
                    cursor::MoveUp(state.lines_printed as u16)
                );
                state.lines_printed = 0;
            }
            println!("Failed: {error}");
            state.render();
        }
    }
}
