pub mod config;
pub mod event;

use config::Config;
use event::Event;

use anyhow::{Context, Result};
use reqwest::Url;

use std::process::exit;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

pub async fn run(config: Config, tx: Sender<Event>) -> Result<()> {
    let client = reqwest::Client::new();

    tx.send(Event::GetPageStarted)?;
    let main_page = client.get(&config.url).send().await.context("Failed to get main page")?;
    tx.send(Event::GetPageCompleted)?;

    let (name, image_urls, track_urls) = parse_page(main_page, config.images).await?;
    let dest_dir = std::path::Path::new(&name).to_path_buf();
    std::fs::create_dir_all(&dest_dir)?;

    let image_count = if config.images { image_urls.len() } else { 0 };
    tx.send(Event::TotalDownloads(track_urls.len() + image_count))?;

    let mut joinset = tokio::task::JoinSet::new();

    for url in track_urls {
        joinset.spawn(download(client.clone(), url, dest_dir.clone(), tx.clone(), config.flac));
    }

    if config.images {
        let dest_dir = std::path::Path::new(&name).join("images");
        std::fs::create_dir_all(&dest_dir)?;
        for url in image_urls {
            joinset.spawn(download(client.clone(), url, dest_dir.clone(), tx.clone(), false));
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

    let track_sel = scraper::Selector::parse("#songlist tbody tr td:nth-child(5) a").unwrap();
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

async fn resolve_flac_url(client: &reqwest::Client, track_page_url: &Url) -> Result<Url> {
    let html = client.get(track_page_url.clone()).send().await?.text().await?;
    let document = scraper::Html::parse_document(&html);
    let sel = scraper::Selector::parse(
        "#pageContent > p:nth-child(10) > a"
    ).map_err(|e| anyhow::anyhow!("Failed to parse FLAC link selector: {e}"))?;
    let href = document
        .select(&sel)
        .next()
        .ok_or_else(|| anyhow::anyhow!("FLAC link not found on page: {}", track_page_url))?
        .value()
        .attr("href")
        .ok_or_else(|| anyhow::anyhow!("FLAC link has no href on page: {}", track_page_url))?;
    track_page_url.join(href).context("Failed to parse FLAC URL")
}

async fn download(
    client: reqwest::Client,
    url: Url,
    dest_dir: std::path::PathBuf,
    tx: Sender<Event>,
    flac: bool,
) -> Result<()> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mut name = url
        .path_segments()
        .and_then(|s| s.last())
        .map(|s| percent_decode(&percent_decode(s)))
        .unwrap_or_else(|| url.to_string());
    if flac && name.ends_with(".mp3") {
        name.truncate(name.len() - 3);
        name.push_str("flac");
    }

    tx.send(Event::DlStarted { id, name })?;

    let download_url = if flac {
        match resolve_flac_url(&client, &url).await {
            Ok(u) => u,
            Err(e) => {
                tx.send(Event::DlFailed { id, error: e })?;
                return Ok(());
            }
        }
    } else {
        url.clone()
    };

    let mut response = match client.get(download_url.clone()).send().await {
        Ok(r) => r,
        Err(e) => {
            tx.send(Event::DlFailed { id, error: e.into() })?;
            return Ok(());
        }
    };

    let total: Option<usize> = response.content_length().and_then(|l| l.try_into().ok());
    let mut downloaded: usize = 0;
    let mut file_bytes = Vec::new();

    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                downloaded += chunk.len();
                file_bytes.extend_from_slice(&chunk);
                let _ = tx.send(Event::DlProgress { id, downloaded, total });
            }
            Ok(None) => break,
            Err(e) => {
                tx.send(Event::DlFailed { id, error: e.into() })?;
                return Ok(());
            }
        }
    }

    // khinsider double-encodes URLs (%20 → %2520), so decode twice
    let filename = match download_url.path_segments().and_then(|s| s.last()) {
        Some(s) => percent_decode(&percent_decode(s)),
        None => {
            tx.send(Event::DlFailed {
                id,
                error: anyhow::anyhow!("Failed to get filename from url"),
            })?;
            return Ok(());
        }
    };

    match tokio::fs::write(dest_dir.join(&filename), &file_bytes).await {
        Err(e) => tx.send(Event::DlFailed { id, error: e.into() })?,
        Ok(()) => tx.send(Event::DlCompleted { id })?,
    };

    Ok(())
}
