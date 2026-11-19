use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("{provider} API returned {status}: {body}")]
    Api {
        provider: &'static str,
        status: u16,
        body: String,
    },
    #[error("could not recognise '{0}' as a supported pack, instance or mods folder")]
    UnknownFormat(String),
    #[error("{0}")]
    Invalid(String),
    #[error("missing credential: {0}")]
    MissingCredential(&'static str),
}
