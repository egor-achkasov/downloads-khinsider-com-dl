pub enum Event {
    GetPageStarted,
    GetPageCompleted,

    TotalDownloads(usize),
    DlStarted { id: usize, name: String },
    DlProgress { id: usize, downloaded: usize, total: Option<usize> },
    DlCompleted { id: usize },
    DlFailed { id: usize, error: anyhow::Error },
}
