pub enum Event {
    GetPageStarted,
    GetPageCompleted,

    DlStarted { url: String },
    DlCompleted { url: String },
    DlFailed { error: anyhow::Error },
}