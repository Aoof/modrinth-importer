//! A small, always-on debug log meant to be attached to bug reports.
//!
//! * Entries are kept in memory (last [`MAX_LINES`]) and optionally mirrored to a file.
//! * API clients record method, URL, status, timing and a truncated error body.
//! * Secrets are never recorded: credentials are described by shape only
//!   (see [`describe_key`]) and the home directory is shown as `~`.

use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_LINES: usize = 1000;

static BUFFER: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// Mirror every new entry to `path` (created/truncated; starts with [`header`]).
pub fn enable_file(path: &Path) -> std::io::Result<()> {
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "{}", header())?;
    for line in BUFFER.lock().unwrap().iter() {
        writeln!(f, "{line}")?;
    }
    *FILE.lock().unwrap() = Some(f);
    Ok(())
}

pub fn log(msg: impl AsRef<str>) {
    let line = format!("{} {}", timestamp(), redact(msg.as_ref()));
    if let Some(f) = FILE.lock().unwrap().as_mut() {
        let _ = writeln!(f, "{line}");
    }
    let mut b = BUFFER.lock().unwrap();
    if b.len() >= MAX_LINES {
        b.pop_front();
    }
    b.push_back(line);
}

/// The whole log so far, with a descriptive header.
pub fn snapshot() -> String {
    let b = BUFFER.lock().unwrap();
    let mut s = header();
    s.push('\n');
    for l in b.iter() {
        s.push_str(l);
        s.push('\n');
    }
    s
}

pub fn save(path: &Path) -> std::io::Result<()> {
    std::fs::write(path, snapshot())
}

pub fn header() -> String {
    format!(
        "# Modrinth Importer debug log\n# version {} | {} {} | contains no credentials; paths shortened to ~",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// Replace the home directory with `~` so logs don't leak the user name.
pub fn redact(s: &str) -> String {
    match directories::BaseDirs::new() {
        Some(b) => {
            let home = b.home_dir().to_string_lossy().into_owned();
            if home.len() > 3 {
                s.replace(&home, "~")
            } else {
                s.to_string()
            }
        }
        None => s.to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyShape {
    /// 60 characters starting with `$2`: what the CurseForge Core API issues.
    CoreApiLike,
    /// 8-4-4-4-12 hex: what CurseForge's author (Upload API) tokens look like.
    AuthorTokenLike,
    Unknown,
}

pub fn key_shape(key: &str) -> KeyShape {
    let k = key.trim();
    let uuid = k.len() == 36
        && k.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });
    if uuid {
        KeyShape::AuthorTokenLike
    } else if k.starts_with("$2") && k.chars().count() == 60 {
        KeyShape::CoreApiLike
    } else {
        KeyShape::Unknown
    }
}

/// A user-facing warning when a pasted CurseForge key looks wrong.
pub fn key_warning(key: &str) -> Option<String> {
    match key_shape(key) {
        KeyShape::CoreApiLike => None,
        KeyShape::AuthorTokenLike => Some(
            "This looks like a CurseForge author upload token (the 8-4-4-4-12 form from \
             authors.curseforge.com), which the Core API rejects. Importing needs a Core API \
             key from console.curseforge.com (about 60 characters, starting with $2a$)."
                .into(),
        ),
        KeyShape::Unknown => Some(
            "This doesn't look like a CurseForge Core API key (usually 60 characters starting \
             with $2a$). If a shell expanded the $ parts, wrap the key in single quotes."
                .into(),
        ),
    }
}

/// Describe an API key without revealing it. CurseForge keys are 60-character
/// bcrypt-style strings starting with `$2a$`; shells expand the `$` parts if the
/// key isn't single-quoted, which produces a 403 from the API.
pub fn describe_key(key: &str) -> String {
    let mut notes = vec![format!("length {}", key.chars().count())];
    if key != key.trim() {
        notes.push("has leading/trailing whitespace".into());
    }
    if key.contains(char::is_whitespace) {
        notes.push("contains whitespace".into());
    }
    match key_shape(key) {
        KeyShape::CoreApiLike => notes.push("shape looks like a CurseForge Core API key".into()),
        KeyShape::AuthorTokenLike => notes.push(
            "WRONG KIND OF TOKEN? it looks like an author upload token (X-Api-Token), not a \
             Core API key; create one at console.curseforge.com"
                .into(),
        ),
        KeyShape::Unknown => notes.push(
            "unexpected shape (CurseForge keys are usually 60 characters starting with $2a$; \
             a shell may have expanded the $ parts, so wrap the key in single quotes)"
                .into(),
        ),
    }
    notes.join(", ")
}

/// Truncate a response body for logging.
pub fn snippet(body: &str) -> String {
    let flat: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 500 {
        format!(
            "{}… [truncated]",
            flat.chars().take(500).collect::<String>()
        )
    } else {
        flat
    }
}

fn timestamp() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant), UTC.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        d.subsec_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_description_never_contains_the_key() {
        let key = "$2a$10$abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123";
        let d = describe_key(key);
        assert!(!d.contains("abcdefgh"));
        assert!(d.contains("length"));
        assert!(describe_key("short").contains("unexpected shape"));
        assert!(
            describe_key("12345678-1234-1234-1234-123456789abc").contains("author upload token")
        );
        assert!(describe_key(" $2a$ ").contains("whitespace"));
    }

    #[test]
    fn key_shapes_and_warnings() {
        let core = format!("$2a$10${}", "a".repeat(53)); // 60 chars, like a real key
        assert_eq!(core.len(), 60);
        assert_eq!(key_shape(&core), KeyShape::CoreApiLike);
        assert!(key_warning(&core).is_none());
        // one character short is not accepted as a Core API key
        assert_eq!(key_shape(&core[..59]), KeyShape::Unknown);
        let uuid = "0f8fad5b-d9cb-469f-a165-70867728950e";
        assert_eq!(key_shape(uuid), KeyShape::AuthorTokenLike);
        assert!(key_warning(uuid).unwrap().contains("author upload token"));
        assert_eq!(key_shape(&format!("  {uuid}\n")), KeyShape::AuthorTokenLike);
        assert_eq!(key_shape("2a10abc"), KeyShape::Unknown);
        assert!(key_warning("2a10abc").unwrap().contains("single quotes"));
        // 36 chars but not hex groups
        assert_eq!(
            key_shape("zzzzzzzz-zzzz-zzzz-zzzz-zzzzzzzzzzzz"),
            KeyShape::Unknown
        );
    }

    #[test]
    fn timestamp_is_iso_utc() {
        let t = timestamp();
        assert_eq!(t.len(), 24);
        assert!(t.ends_with('Z') && t.as_bytes()[10] == b'T');
    }

    #[test]
    fn snippet_truncates() {
        assert!(snippet(&"x".repeat(900)).ends_with("[truncated]"));
    }
}
