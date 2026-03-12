pub enum Event {
    GetPageStarted,
    GetPageCompleted,

    TotalDownloads(usize),
    DlStarted { id: usize, name: String },
    DlCompleted { id: usize },
    DlFailed { id: usize, error: anyhow::Error },
}
