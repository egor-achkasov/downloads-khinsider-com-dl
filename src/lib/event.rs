pub enum Event {
    GetPageStarted,
    GetPageCompleted,

    TotalDownloads(usize),
    DlStarted { id: usize, name: String },
    DlProgress { id: usize, downloaded: u64, total: Option<u64> },
    DlCompleted { id: usize },
    DlFailed { id: usize, error: anyhow::Error },
}
