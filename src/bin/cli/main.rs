use downloads_khinsider_com_dl as lib;
use lib::{config::Config, event::Event};

use anyhow::Result;
use clap::Parser;
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
        for event in rx {
            render_event(&event);
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

fn render_event(event: &Event) {
    match event {
        Event::GetPageStarted => println!("Fetching page..."),
        Event::GetPageCompleted => println!("Page fetched."),
        Event::DlStarted { url } => {
            print!("\rDownloading: {url}");
            let _ = std::io::stdout().flush();
        }
        Event::DlCompleted { url } => {
            print!("\rDownloaded: {url}\n");
            let _ = std::io::stdout().flush();
        }
        Event::DlFailed { error } => eprintln!("Failed: {error}"),
    }
}
