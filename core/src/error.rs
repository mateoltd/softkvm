use thiserror::Error;

#[derive(Error, Debug)]
pub enum SoftKvmError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("DDC/CI error: {0}")]
    Ddc(String),

    #[error("monitor not found: {0}")]
    MonitorNotFound(String),

    #[error("deskflow error: {0}")]
    Deskflow(String),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("JSON error: {0}")]
    SerdeJson(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, SoftKvmError>;
