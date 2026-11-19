//! Manual resolution of mods that could not be found automatically.
//!
//! A CurseForge mod that is on neither Modrinth nor any reachable download source
//! "needs a manual file": the user opens the CurseForge page, downloads the exact
//! file, and hands it to [`apply_files`]. The file is **verified against the exact
//! file the pack asks for** before it is accepted:
//!
//! | what is known about the wanted file | check                              | verdict            |
//! | ----------------------------------- | ---------------------------------- | ------------------ |
//! | SHA-1 (Core API)                    | hashes are equal                   | `ExactHash`        |
//! | exact file name (cfwidget)          | names are equal                    | `ExactFileName`    |
//! | nothing                             | cannot be checked                  | `Unverified`       |
//!
//! Different hash or name is a `Mismatch` and is refused unless forced. Accepted files
//! are looked up on Modrinth by hash (exact only, never "a similar version"); if they
//! are not there they are bundled into the pack as they are.

use crate::hash::{sha1_hex, sha512_hex};
use crate::model::{ModEntry, Origin};
use crate::modrinth::Modrinth;
use crate::providers::jarmeta;
use crate::resolve::{exact_by_hash, Resolution, ResolvedMod};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How strictly a supplied file can be checked for one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exactness {
    Hash,
    FileName,
    Unverifiable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualTask {
    pub project_id: u64,
    pub file_id: u64,
    pub project_name: Option<String>,
    /// CurseForge project page.
    pub project_url: String,
    /// CurseForge page of this exact file (only when the project slug is known).
    pub file_url: Option<String>,
    pub expected_file_name: Option<String>,
    pub expected_sha1: Option<String>,
    pub exactness: Exactness,
}

/// What a person has to do for this mod, if anything.
pub fn task(entry: &ModEntry) -> Option<ManualTask> {
    let cf = entry.cf?;
    let site = entry
        .cf_url
        .clone()
        .filter(|u| u.starts_with("https://www.curseforge.com/"));
    let project_url = site
        .clone()
        .unwrap_or_else(|| format!("https://www.curseforge.com/projects/{}", cf.project_id));
    let file_url = site.map(|u| format!("{}/files/{}", u.trim_end_matches('/'), cf.file_id));
    let exactness = if entry.sha1.is_some() {
        Exactness::Hash
    } else if entry.expected_file_name.is_some() {
        Exactness::FileName
    } else {
        Exactness::Unverifiable
    };
    Some(ManualTask {
        project_id: cf.project_id,
        file_id: cf.file_id,
        project_name: entry.name_hint.clone(),
        project_url,
        file_url,
        expected_file_name: entry.expected_file_name.clone(),
        expected_sha1: entry.sha1.clone(),
        exactness,
    })
}

/// True when the mod is unresolved and there is nothing the app can fetch itself.
pub fn needs_manual(r: &ResolvedMod) -> bool {
    !matches!(r.resolution, Resolution::Matched { .. })
        && r.entry.cf.is_some()
        && r.entry.origin.is_none()
        && r.entry.download_url.is_none()
}

/// Browsers rename repeated downloads to `name (1).jar`; undo that for comparisons.
pub fn normalize_download_name(name: &str) -> String {
    let re = regex::Regex::new(r"^(.*?)(?:\s*\(\d+\))?(\.[A-Za-z0-9]+)$").unwrap();
    match re.captures(name) {
        Some(c) => format!("{}{}", &c[1], &c[2]),
        None => name.to_string(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Verdict {
    ExactHash,
    ExactFileName,
    Unverified { note: String },
    Mismatch { reason: String },
}

/// Check a supplied file against the exact file `entry` stands for.
pub fn verify(entry: &ModEntry, supplied_name: &str, bytes: &[u8]) -> Verdict {
    if bytes.len() < 4 || &bytes[..2] != b"PK" {
        return Verdict::Mismatch {
            reason: "this is not a .jar file".into(),
        };
    }
    if let Some(want) = &entry.sha1 {
        let got = sha1_hex(bytes);
        return if want.eq_ignore_ascii_case(&got) {
            Verdict::ExactHash
        } else {
            Verdict::Mismatch {
                reason: format!(
                    "the file's SHA-1 ({}…) differs from the exact file CurseForge lists ({}…): \
                     it is a different version or build",
                    &got[..8],
                    &want[..want.len().min(8)]
                ),
            }
        };
    }
    let name = normalize_download_name(supplied_name);
    if let Some(want) = &entry.expected_file_name {
        return if want.eq_ignore_ascii_case(&name) {
            Verdict::ExactFileName
        } else {
            Verdict::Mismatch {
                reason: format!("expected the file '{want}' but this is '{name}'"),
            }
        };
    }
    let meta = jarmeta::inspect(bytes);
    let norm = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase()
    };
    let same_mod = [&entry.slug_hint, &entry.name_hint]
        .into_iter()
        .flatten()
        .any(|h| {
            [&meta.mod_id, &meta.name]
                .into_iter()
                .flatten()
                .any(|m| norm(m) == norm(h))
        });
    Verdict::Unverified {
        note: if same_mod {
            "It is the same mod, but the exact version can't be confirmed without a CurseForge API key."
        } else {
            "The exact mod and version can't be confirmed without a CurseForge API key."
        }
        .into(),
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ManualOptions {
    /// Accept files that can't be verified either way.
    pub accept_unverified: bool,
    /// Accept files that are verifiably a different file. Use with care.
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualResult {
    pub path: String,
    pub file_name: String,
    pub index: Option<usize>,
    pub mod_name: Option<String>,
    pub verdict: Option<Verdict>,
    pub applied: bool,
    pub on_modrinth: bool,
    pub message: String,
}

/// Expand directories (non-recursive) into `.jar` files. `from_dir` marks files that came
/// from scanning a folder, which are skipped quietly when they don't belong to the pack.
pub fn collect_jars(paths: &[PathBuf]) -> Vec<(PathBuf, bool)> {
    let mut out = vec![];
    for p in paths {
        if p.is_dir() {
            let mut jars: Vec<PathBuf> = std::fs::read_dir(p)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|f| {
                    f.is_file() && f.extension().is_some_and(|x| x.eq_ignore_ascii_case("jar"))
                })
                .collect();
            jars.sort();
            out.extend(jars.into_iter().map(|j| (j, true)));
        } else {
            out.push((p.clone(), false));
        }
    }
    out
}

fn auto_target(resolved: &[ResolvedMod], name: &str, sha1: &str, bytes: &[u8]) -> Option<usize> {
    let norm = normalize_download_name(name);
    let candidates = || resolved.iter().enumerate().filter(|(_, r)| needs_manual(r));
    candidates()
        .find(|(_, r)| {
            r.entry
                .sha1
                .as_deref()
                .is_some_and(|h| h.eq_ignore_ascii_case(sha1))
        })
        .or_else(|| {
            candidates().find(|(_, r)| {
                r.entry
                    .expected_file_name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(&norm))
            })
        })
        .or_else(|| {
            // Last resort for entries we know nothing exact about: the mod id/name inside the jar
            // equals the project's slug/name. The verdict will still be `Unverified`.
            let meta = jarmeta::inspect(bytes);
            let jar_ids: Vec<String> = [meta.mod_id, meta.name]
                .into_iter()
                .flatten()
                .map(|x| squash(&x))
                .collect();
            candidates()
                .filter(|(_, r)| r.entry.sha1.is_none() && r.entry.expected_file_name.is_none())
                .find(|(_, r)| {
                    [&r.entry.slug_hint, &r.entry.name_hint]
                        .into_iter()
                        .flatten()
                        .any(|h| jar_ids.contains(&squash(h)))
                })
        })
        .map(|(i, _)| i)
}

fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

fn display_name(e: &ModEntry) -> String {
    e.display_name()
}

/// Verify and apply supplied files. Each item is `(path, explicit target index, from_dir)`;
/// without a target the file is matched to a waiting mod by hash, then by exact file name.
pub async fn apply_files(
    resolved: &mut [ResolvedMod],
    files: &[(PathBuf, Option<usize>, bool)],
    mr: &Modrinth,
    opts: ManualOptions,
) -> Vec<ManualResult> {
    let mut results = vec![];
    let mut applied: Vec<usize> = vec![];

    for (path, target, from_dir) in files {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut res = ManualResult {
            path: path.display().to_string(),
            file_name: file_name.clone(),
            index: None,
            mod_name: None,
            verdict: None,
            applied: false,
            on_modrinth: false,
            message: String::new(),
        };
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                res.message = format!("could not read the file: {e}");
                results.push(res);
                continue;
            }
        };
        let sha1 = sha1_hex(&bytes);
        let idx = target.or_else(|| auto_target(resolved, &file_name, &sha1, &bytes));
        let Some(i) = idx.filter(|i| *i < resolved.len()) else {
            if !from_dir {
                res.message = "this file doesn't match any mod that still needs a file".into();
                results.push(res);
            }
            continue;
        };
        res.index = Some(i);
        res.mod_name = Some(display_name(&resolved[i].entry));
        let verdict = verify(&resolved[i].entry, &file_name, &bytes);
        let accept = match &verdict {
            Verdict::ExactHash | Verdict::ExactFileName => true,
            Verdict::Unverified { .. } => opts.accept_unverified,
            Verdict::Mismatch { .. } => opts.force,
        };
        res.message = match &verdict {
            Verdict::ExactHash => "exact file (SHA-1 matches CurseForge)".into(),
            Verdict::ExactFileName => "exact file (file name matches CurseForge)".into(),
            Verdict::Unverified { note } if accept => format!("accepted unverified. {note}"),
            Verdict::Unverified { note } => format!("not accepted yet. {note}"),
            Verdict::Mismatch { reason } if accept => format!("accepted anyway: {reason}"),
            Verdict::Mismatch { reason } => format!("rejected: {reason}"),
        };
        res.verdict = Some(verdict);
        if accept {
            let e = &mut resolved[i].entry;
            e.file_name = e
                .expected_file_name
                .clone()
                .unwrap_or_else(|| normalize_download_name(&file_name));
            e.sha1 = Some(sha1);
            e.sha512 = Some(sha512_hex(&bytes));
            e.size = Some(bytes.len() as u64);
            e.origin = Some(Origin::File { path: path.clone() });
            res.applied = true;
            applied.push(i);
        }
        results.push(res);
    }

    // Exact lookup on Modrinth for what was accepted (never a "similar" version).
    if !applied.is_empty() {
        let entries: Vec<ModEntry> = applied.iter().map(|i| resolved[*i].entry.clone()).collect();
        let found = exact_by_hash(mr, &entries).await.unwrap_or_else(|e| {
            crate::debuglog::log(format!("manual: Modrinth hash lookup failed: {e}"));
            vec![None; entries.len()]
        });
        for (k, i) in applied.iter().enumerate() {
            let on_modrinth = found[k].is_some();
            resolved[*i].resolution = found[k].clone().unwrap_or(Resolution::Unmatched {
                reason: "not on Modrinth; the file you supplied is bundled in the pack".into(),
            });
            for r in results
                .iter_mut()
                .filter(|r| r.index == Some(*i) && r.applied)
            {
                r.on_modrinth = on_modrinth;
            }
        }
    }
    results
}

/// Expand a leading `~` and trim quotes/whitespace that terminals add to dragged paths.
pub fn expand_path(input: &str) -> PathBuf {
    let t = input.trim().trim_matches(|c| c == '"' || c == '\'');
    if let Some(rest) = t.strip_prefix("~/").or_else(|| (t == "~").then_some("")) {
        if let Some(b) = directories::BaseDirs::new() {
            return b.home_dir().join(rest);
        }
    }
    PathBuf::from(t)
}

/// Folder most browsers save downloads to.
pub fn default_downloads_dir() -> Option<PathBuf> {
    directories::UserDirs::new().and_then(|d| d.download_dir().map(Path::to_path_buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jar(extra: &str) -> Vec<u8> {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(vec![]);
        let mut z = zip::ZipWriter::new(&mut buf);
        z.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(format!(r#"{{"id":"jei","name":"JEI {extra}"}}"#).as_bytes())
            .unwrap();
        z.finish().unwrap();
        buf.into_inner()
    }

    fn entry() -> ModEntry {
        let mut e = ModEntry::new("jei-1.20.1.jar");
        e.cf = Some(crate::model::CfRef {
            project_id: 238222,
            file_id: 9701664,
        });
        e
    }

    #[test]
    fn browser_suffixes_are_ignored() {
        assert_eq!(
            normalize_download_name("jei-1.20.1 (1).jar"),
            "jei-1.20.1.jar"
        );
        assert_eq!(
            normalize_download_name("jei-1.20.1(12).jar"),
            "jei-1.20.1.jar"
        );
        assert_eq!(normalize_download_name("jei-1.20.1.jar"), "jei-1.20.1.jar");
        assert_eq!(normalize_download_name("mod (beta).jar"), "mod (beta).jar");
    }

    #[test]
    fn hash_is_authoritative() {
        let bytes = jar("a");
        let mut e = entry();
        e.sha1 = Some(sha1_hex(&bytes));
        // even with a different file name, the hash decides
        assert_eq!(verify(&e, "whatever.jar", &bytes), Verdict::ExactHash);
        let other = jar("b");
        assert!(matches!(
            verify(&e, "jei-1.20.1.jar", &other),
            Verdict::Mismatch { .. }
        ));
    }

    #[test]
    fn file_name_is_checked_when_no_hash() {
        let bytes = jar("a");
        let mut e = entry();
        e.expected_file_name = Some("jei-1.20.1-fabric-15.0.jar".into());
        assert_eq!(
            verify(&e, "jei-1.20.1-fabric-15.0 (2).jar", &bytes),
            Verdict::ExactFileName
        );
        assert_eq!(
            verify(&e, "JEI-1.20.1-FABRIC-15.0.JAR", &bytes),
            Verdict::ExactFileName
        );
        match verify(&e, "jei-1.20.1-fabric-14.0.jar", &bytes) {
            Verdict::Mismatch { reason } => {
                assert!(reason.contains("15.0") && reason.contains("14.0"))
            }
            v => panic!("{v:?}"),
        }
    }

    #[test]
    fn unknowns_are_flagged_not_trusted() {
        let bytes = jar("a");
        let mut e = entry();
        e.slug_hint = Some("jei".into());
        assert!(matches!(
            verify(&e, "x.jar", &bytes),
            Verdict::Unverified { .. }
        ));
        assert!(matches!(
            verify(&e, "x.jar", b"<html>not a jar</html>"),
            Verdict::Mismatch { .. }
        ));
    }

    #[test]
    fn links_prefer_the_site_page_and_fall_back_to_the_project_id() {
        let mut e = entry();
        let t = task(&e).unwrap();
        assert_eq!(t.project_url, "https://www.curseforge.com/projects/238222");
        assert!(t.file_url.is_none());
        assert_eq!(t.exactness, Exactness::Unverifiable);
        e.cf_url = Some("https://www.curseforge.com/minecraft/mc-mods/jei".into());
        e.expected_file_name = Some("a.jar".into());
        let t = task(&e).unwrap();
        assert_eq!(
            t.file_url.as_deref(),
            Some("https://www.curseforge.com/minecraft/mc-mods/jei/files/9701664")
        );
        assert_eq!(t.exactness, Exactness::FileName);
        e.sha1 = Some("ab".into());
        assert_eq!(task(&e).unwrap().exactness, Exactness::Hash);
        // links to anything but curseforge.com are never offered
        e.cf_url = Some("https://evil.example/x".into());
        assert!(task(&e)
            .unwrap()
            .project_url
            .starts_with("https://www.curseforge.com/projects/"));
    }
}
