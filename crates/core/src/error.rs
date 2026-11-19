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

impl Error {
    /// Build an API error, appending a plain-language hint for common failures.
    pub fn api(provider: &'static str, status: u16, body: String) -> Self {
        let hint = match (provider, status) {
            ("CurseForge", 401 | 403) => Some(
                "CurseForge rejected the API key. Check that the key is valid and copied in full: \
                 it is ~60 characters starting with $2a$, and shells expand the $ parts unless the \
                 key is in single quotes. Re-save it with `mrimport auth curseforge` (paste when \
                 prompted) or in Settings. Run `mrimport doctor` for a full check.",
            ),
            ("Modrinth", 401) => Some("Modrinth rejected the token. Create a new personal access token and run `mrimport auth modrinth`."),
            (_, 429) => Some("Rate limited; wait a minute and try again."),
            _ => None,
        };
        let body = match hint {
            Some(h) => format!("{body}\n  hint: {h}"),
            None => body,
        };
        Error::Api {
            provider,
            status,
            body,
        }
    }
}
