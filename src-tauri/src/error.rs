use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("audio device error: {0}")]
    Device(String),

    #[error("audio host error: {0}")]
    Host(String),

    #[error("audio stream error: {0}")]
    Stream(String),

    #[error("invalid graph: {0}")]
    Validation(String),

    #[error("not running")]
    NotRunning,

    #[error("already running")]
    AlreadyRunning,
}

// Serialize to a plain string so the frontend gets a readable message.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.to_string().as_str())
    }
}

pub type AppResult<T> = Result<T, AppError>;
