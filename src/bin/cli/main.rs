use downloads_khinsider_com_dl as lib;
use lib::{config::Config, event::Event};

use anyhow::Result;
use clap::Parser;
use crossterm::{cursor, execute, terminal::{Clear, ClearType}};
use std::collections::HashMap;
use std::io::Write;

#[derive(Parser)]
#[command(about = "Download game soundtracks from downloads.khinsider.com")]
struct Args {
    /// Link to the album page on downloads.khinsider.com (example https://downloads.khinsider.com/game-soundtracks/album/synthetik-2-windows-gamerip-2021)
    url: String,

    /// Download flacs. Download MP3 if not set
    #[arg(short = 'f', long = "flac")]
    flac: bool,

    /// Download images into "images" directory
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

struct SlotInfo {
    name: String,
    downloaded: usize,
    total: Option<usize>,
}

impl SlotInfo {
    fn format_line(&self) -> String {
        match self.total {
            Some(total) if total > 0 => {
                let pct = (self.downloaded * 100 / total) as u8;
                format!("{pct:3}% {}", self.name)
            }
            _ => format!("     {}", self.name),
        }
    }
}

#[derive(Default)]
struct ProgressState {
    total: usize,
    completed: usize,
    failed: usize,
    slots: Vec<Option<SlotInfo>>,
    id_to_slot: HashMap<usize, usize>,
    lines_printed: usize,
}

impl ProgressState {
    fn alloc_slot(&mut self, id: usize, name: String) {
        let slot = self.slots.iter().position(|s| s.is_none()).unwrap_or_else(|| {
            self.slots.push(None);
            self.slots.len() - 1
        });
        self.slots[slot] = Some(SlotInfo { name, downloaded: 0, total: None });
        self.id_to_slot.insert(id, slot);
    }

    fn update_progress(&mut self, id: usize, downloaded: usize, total: Option<usize>) {
        if let Some(&slot) = self.id_to_slot.get(&id) {
            if let Some(Some(info)) = self.slots.get_mut(slot) {
                info.downloaded = downloaded;
                info.total = total;
            }
        }
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
                Some(info) => println!("  {}", info.format_line()),
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
        Event::DlProgress { id, downloaded, total } => {
            state.update_progress(id, downloaded, total);
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
            // Print error above the progress block
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
