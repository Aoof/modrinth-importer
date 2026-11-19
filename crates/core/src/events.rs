use serde::Serialize;
use std::sync::Arc;

/// Progress events emitted by long-running operations. The CLI renders them
/// as progress bars; the GUI forwards them to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Stage { name: String },
    Progress { done: usize, total: usize },
    Log { message: String },
}

#[derive(Clone)]
pub struct Reporter(Arc<dyn Fn(Event) + Send + Sync>);

impl Reporter {
    pub fn new(f: impl Fn(Event) + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }
    pub fn silent() -> Self {
        Self::new(|_| {})
    }
    pub fn emit(&self, e: Event) {
        (self.0)(e)
    }
    pub fn stage(&self, name: impl Into<String>) {
        self.emit(Event::Stage { name: name.into() })
    }
    pub fn progress(&self, done: usize, total: usize) {
        self.emit(Event::Progress { done, total })
    }
    pub fn log(&self, message: impl Into<String>) {
        self.emit(Event::Log {
            message: message.into(),
        })
    }
}
