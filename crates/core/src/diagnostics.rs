//! Self-check shared by `mrimport doctor` and the app's Settings page.

use crate::config::{secret_source, Secret};
use crate::curseforge::CurseForge;
use crate::debuglog;
use crate::modrinth::Modrinth;

/// Run connectivity and credential checks and return a human-readable report.
/// Everything is also written to the debug log. Secrets are described, never shown.
pub async fn run(mr: &Modrinth, keyless: bool) -> String {
    let mut out = vec![format!(
        "Modrinth Importer {} on {} {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    )];

    match mr.ping().await {
        Ok(()) => out.push("Modrinth API: reachable".into()),
        Err(e) => out.push(format!("Modrinth API: FAILED. {e}")),
    }

    match secret_source(Secret::ModrinthToken) {
        Some((_, src)) => out.push(format!("Modrinth token: set ({src})")),
        None => out.push("Modrinth token: not set (only needed for publishing)".into()),
    }

    match secret_source(Secret::CurseForgeKey) {
        None => out.push(
            "CurseForge key: not set (needed to import CurseForge modpack zips; \
             get one at https://console.curseforge.com)"
                .into(),
        ),
        Some((key, src)) => {
            out.push(format!(
                "CurseForge key: set ({src}); {}",
                debuglog::describe_key(&key)
            ));
            match CurseForge::new(key).check_key().await {
                Ok(()) => out.push("CurseForge API: key accepted".into()),
                Err(e) => out.push(format!("CurseForge API: FAILED. {e}")),
            }
        }
    }

    if keyless {
        out.push(match keyless_ping().await {
            Ok(()) => "Keyless lookup: ON (cursemaven.com reachable)".into(),
            Err(e) => format!("Keyless lookup: ON, but cursemaven.com is not reachable ({e})"),
        });
    } else {
        out.push(
            "Keyless lookup: off (experimental opt-in that works without a CurseForge key)".into(),
        );
    }

    for line in &out {
        debuglog::log(format!("doctor: {line}"));
    }
    out.join("\n")
}

/// HEAD request for a well-known file; confirms the keyless service is reachable.
async fn keyless_ping() -> Result<(), String> {
    let url = format!(
        "{}/curse/maven/x-238222/9701664/x-238222-9701664.jar",
        crate::cflookup::CURSEMAVEN_BASE
    );
    let http = reqwest::Client::builder()
        .user_agent(crate::user_agent())
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let r = http.head(&url).send().await.map_err(|e| e.to_string())?;
    debuglog::log(format!("doctor: HEAD {url} -> {}", r.status().as_u16()));
    if r.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", r.status().as_u16()))
    }
}
