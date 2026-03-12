pub mod config;
pub mod event;

use config::Config;
use event::Event;

use anyhow::{Context, Result};
use reqwest::Url;

use std::sync::mpsc::Sender;

pub async fn run(config: Config, tx: Sender<Event>) -> Result<()> {
    let client = reqwest::Client::new();

    tx.send(Event::GetPageStarted)?;
    let main_page = client.get(&config.url).send().await.context("Failed to get main page")?;
    tx.send(Event::GetPageCompleted)?;

    let (name, image_urls, track_urls) = parse_page(main_page, config.images, config.flac).await?;
    let dest_dir = std::path::Path::new(&name).to_path_buf();
    std::fs::create_dir_all(&dest_dir)?;

    let mut joinset = tokio::task::JoinSet::new();

    for url in track_urls {
        joinset.spawn(download(client.clone(), url, dest_dir.clone(), tx.clone()));
    }

    if config.images {
        let dest_dir = std::path::Path::new(&name).join("images");
        std::fs::create_dir_all(&dest_dir)?;
        for url in image_urls {
            joinset.spawn(download(client.clone(), url, dest_dir.clone(), tx.clone()));
        }
    }

    while let Some(result) = joinset.join_next().await {
        result.context("download task panicked")??;
    }

    Ok(())
}

async fn parse_page(
    main_page: reqwest::Response,
    images: bool,
    flac: bool
) -> Result<(String, Vec<Url>, Vec<Url>)> {
    let base_url = main_page.url().clone();
    let html = main_page.text().await.context("Failed to read page body")?;
    let document = scraper::Html::parse_document(&html);

    let name = {
        let sel = scraper::Selector::parse("#pageContent h2").unwrap();
        document
            .select(&sel)
            .next()
            .ok_or_else(|| anyhow::anyhow!("Album name element not found"))?
            .text()
            .collect::<String>()
            .trim()
            .to_string()
    };

    let image_urls = if images {
        let sel = scraper::Selector::parse(
            "#pageContent table:first-of-type tr td div:first-of-type a"
        ).unwrap();
        document
            .select(&sel)
            .filter_map(|el| el.value().attr("href"))
            .filter_map(|href| base_url.join(href).ok())
            .collect()
    } else {
        Vec::new()
    };

    let td = if flac { 6 } else { 5 };
    let track_sel_str = format!("#songlist tbody tr td:nth-child({td}) a");
    let track_sel = scraper::Selector::parse(&track_sel_str).unwrap();
    let track_urls = document
        .select(&track_sel)
        .filter_map(|el| el.value().attr("href"))
        .filter_map(|href| base_url.join(href).ok())
        .collect();

    Ok((name, image_urls, track_urls))
}

fn percent_decode(s: &str) -> String {
    let s = s.as_bytes();
    let mut bytes = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' && i + 2 < s.len() {
            if let (Some(h), Some(l)) = (
                (s[i + 1] as char).to_digit(16),
                (s[i + 2] as char).to_digit(16),
            ) {
                bytes.push((h << 4 | l) as u8);
                i += 3;
                continue;
            }
        }
        bytes.push(s[i]);
        i += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

async fn download(
    client: reqwest::Client,
    url: Url,
    dest_dir: std::path::PathBuf,
    tx: Sender<Event>,
) -> Result<()> {
    async fn dl(client: reqwest::Client, url: &Url, dest_dir: std::path::PathBuf) -> Result<()> {
        let filename = url
            .path_segments()
            .and_then(|s| s.last())
            .ok_or(anyhow::anyhow!("Failed to get filename from url"))?;
        // khinsider double-encodes URLs (%20 → %2520), so decode twice
        let filename = percent_decode(&percent_decode(filename));
        let bytes = client.get(url.clone()).send().await?.bytes().await?;
        tokio::fs::write(dest_dir.join(&filename), &bytes).await?;

        Ok(())
    }

    tx.send(Event::DlStarted { url: url.to_string() })?;
    match dl(client, &url, dest_dir).await {
        Err(e) => tx.send(Event::DlFailed { error: e })?,
        Ok(()) => tx.send(Event::DlCompleted { url: url.to_string() })?
    };

    Ok(())
}
