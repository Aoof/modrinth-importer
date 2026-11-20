//! Tauri shell: thin commands over `mrimport-core`.

use mrimport_core::{
    cflookup::Lookup,
    cfmodpack,
    config::{self, Secret},
    curseforge::CurseForge,
    events::{Event, Reporter},
    export::mrpack::{open_with_default_app, write_mrpack, ExportOptions, ExportReport},
    fetch,
    manual::{self, ManualOptions, ManualResult, ManualTask},
    model::{ImportedPack, Loader},
    modrinth::Modrinth,
    providers::{self, LoadCtx, LoadOptions},
    publish::{self, ProjectSpec, PublishPlan, PublishReport, ReleaseDefaults},
    resolve::{self, ResolvedMod},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::ipc::Channel;

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    let msg = e.to_string();
    mrimport_core::debuglog::log(format!("app error: {msg}"));
    msg
}

fn reporter(ch: Channel<Event>) -> Reporter {
    Reporter::new(move |e| {
        let _ = ch.send(e);
    })
}

/// Core API client, used by publishing (which reads a project you own).
fn cf_client() -> Option<CurseForge> {
    config::get_secret(Secret::CurseForgeKey).map(CurseForge::new)
}

fn lookup() -> Lookup {
    Lookup::new(
        config::get_secret(Secret::CurseForgeKey).map(CurseForge::new),
        config::load_settings().keyless_lookup,
    )
    .with_cache_dir(config::cache_dir())
}

/// A mod that needs a manual file, with its index in the list.
#[derive(Serialize, Deserialize, Clone)]
struct IndexedTask {
    index: usize,
    #[serde(flatten)]
    task: ManualTask,
}

fn tasks_for(resolved: &[ResolvedMod]) -> Vec<IndexedTask> {
    resolved
        .iter()
        .enumerate()
        .filter(|(_, r)| manual::needs_manual(r))
        .filter_map(|(index, r)| manual::task(&r.entry).map(|task| IndexedTask { index, task }))
        .collect()
}

/// Where mods fetched during a scan wait until the pack is exported.
fn downloads_dir() -> PathBuf {
    config::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("downloads")
}

/// Where a modpack zip fetched from CurseForge lives while it is being imported. Kept apart from
/// `downloads_dir` because starting a scan clears that one, and the pack must outlive the scan.
fn modpacks_dir() -> PathBuf {
    config::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("modpacks")
}

/// Forget the mods fetched for the previous scan (the pack being scanned stays).
fn clear_downloads() {
    let _ = std::fs::remove_dir_all(downloads_dir());
}

/// Forget everything fetched, including a modpack zip downloaded from CurseForge.
fn clear_everything() {
    clear_downloads();
    let _ = std::fs::remove_dir_all(modpacks_dir());
}

/// Discard files fetched for a scan the user abandoned.
#[tauri::command]
fn discard_scan() {
    clear_everything();
}

#[derive(Serialize, Deserialize)]
struct ScanResult {
    pack: ImportedPack,
    resolved: Vec<ResolvedMod>,
    /// Mods that need a manually downloaded file.
    tasks: Vec<IndexedTask>,
    /// Non-fatal messages worth showing (e.g. a rejected key and the fallback used).
    notices: Vec<String>,
}

#[tauri::command]
async fn scan_pack(
    path: String,
    name: Option<String>,
    mc_version: Option<String>,
    loader: Option<String>,
    on_event: Channel<Event>,
) -> CmdResult<ScanResult> {
    let notices = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let (n2, ch) = (notices.clone(), on_event);
    let rep = Reporter::new(move |e| {
        if let Event::Log { message } = &e {
            n2.lock().unwrap().push(message.clone());
        }
        let _ = ch.send(e);
    });
    let opts = LoadOptions {
        name,
        mc_version,
        loader: loader.as_deref().and_then(Loader::parse),
    };
    let settings = config::load_settings();
    let lk = lookup();
    let ctx = LoadCtx::with_lookup(lk.clone(), rep.clone());
    let pack = providers::load(&PathBuf::from(path), &opts, &ctx)
        .await
        .map_err(err)?;
    let mr = Modrinth::new(config::get_secret(Secret::ModrinthToken));
    // Only exact (hash) matches come from Modrinth; a by-name search is an opt-in advanced setting.
    let mut resolved = resolve::resolve_with(
        &pack,
        &mr,
        &rep,
        resolve::ResolveOptions {
            enricher: Some(&lk),
            fuzzy: settings.match_by_name,
        },
    )
    .await
    .map_err(err)?;
    // Everything else is downloaded from its source; what can't be awaits the user.
    if settings.download_unmatched {
        clear_downloads();
        let r = fetch::fetch_unmatched(&mut resolved, &downloads_dir(), &mr, &rep)
            .await
            .map_err(err)?;
        if r.fetched > 0 || !r.failed.is_empty() {
            rep.log(format!(
                "Downloaded {} mod(s) that aren't on Modrinth ({} turned out to be on Modrinth, {} could not be downloaded).",
                r.fetched, r.found_on_modrinth, r.failed.len()
            ));
        }
    }
    let tasks = tasks_for(&resolved);
    let notices = notices.lock().unwrap().clone();
    Ok(ScanResult {
        pack,
        resolved,
        tasks,
        notices,
    })
}

#[derive(Deserialize)]
struct ManualFile {
    path: String,
    /// Explicit target mod; without it the file is matched by hash / exact name.
    index: Option<usize>,
}

#[derive(Serialize)]
struct ManualBatch {
    resolved: Vec<ResolvedMod>,
    results: Vec<ManualResult>,
    tasks: Vec<IndexedTask>,
}

/// Verify and apply files (or folders of files) the user downloaded by hand.
#[tauri::command]
async fn apply_manual(
    mut resolved: Vec<ResolvedMod>,
    files: Vec<ManualFile>,
    accept_unverified: bool,
    force: bool,
) -> CmdResult<ManualBatch> {
    let mut items: Vec<(PathBuf, Option<usize>, bool)> = vec![];
    for f in files {
        let p = PathBuf::from(&f.path);
        match f.index {
            Some(i) => items.push((p, Some(i), false)),
            None => items.extend(
                manual::collect_jars(&[p])
                    .into_iter()
                    .map(|(p, d)| (p, None, d)),
            ),
        }
    }
    let mr = Modrinth::new(config::get_secret(Secret::ModrinthToken));
    let results = manual::apply_files(
        &mut resolved,
        &items,
        &mr,
        ManualOptions {
            accept_unverified,
            force,
        },
    )
    .await;
    let tasks = tasks_for(&resolved);
    Ok(ManualBatch {
        resolved,
        results,
        tasks,
    })
}

#[tauri::command]
fn default_downloads_dir() -> Option<String> {
    manual::default_downloads_dir().map(|p| p.display().to_string())
}

/// Open a CurseForge page in the default browser. Anything else is refused.
#[tauri::command]
fn open_link(url: String) -> CmdResult<()> {
    let ok = url.starts_with("https://www.curseforge.com/")
        && !url.contains(|c: char| c.is_whitespace() || "\"&|<>^%`".contains(c));
    if !ok {
        return Err("only curseforge.com links can be opened from here".into());
    }
    open_with_default_app(&PathBuf::from(url)).map_err(err)
}

#[derive(Serialize)]
struct SettingDto {
    key: &'static str,
    title: &'static str,
    description: &'static str,
    value: bool,
}

/// The advanced settings, with the texts the UI shows next to each switch.
#[tauri::command]
fn get_settings() -> Vec<SettingDto> {
    config::load_settings()
        .describe()
        .into_iter()
        .map(|i| SettingDto {
            key: i.key,
            title: i.title,
            description: i.description,
            value: i.value,
        })
        .collect()
}

#[tauri::command]
fn set_setting(key: String, value: bool) -> CmdResult<()> {
    let mut s = config::load_settings();
    if !s.set(&key, value) {
        return Err(format!("unknown setting '{key}'"));
    }
    config::save_settings(&s).map_err(err)
}

#[tauri::command]
async fn export_pack(
    pack: ImportedPack,
    resolved: Vec<ResolvedMod>,
    output: String,
    loader_version: Option<String>,
    on_event: Channel<Event>,
) -> CmdResult<ExportReport> {
    let settings = config::load_settings();
    let mr = Modrinth::new(None);
    let opts = ExportOptions {
        bundle_unmatched: settings.bundle_unmatched,
        download_remote: settings.download_unmatched,
        loader_version,
    };
    let report = write_mrpack(
        &pack,
        &resolved,
        &PathBuf::from(output),
        &opts,
        &mr,
        &reporter(on_event),
    )
    .await
    .map_err(err)?;
    clear_everything();
    Ok(report)
}

#[tauri::command]
fn open_path(path: String) -> CmdResult<()> {
    open_with_default_app(&PathBuf::from(path)).map_err(err)
}

#[derive(Serialize)]
struct SecretStatus {
    modrinth: bool,
    curseforge: bool,
}

fn secret_kind(kind: &str) -> CmdResult<Secret> {
    match kind {
        "modrinth" => Ok(Secret::ModrinthToken),
        "curseforge" => Ok(Secret::CurseForgeKey),
        _ => Err("unknown credential".into()),
    }
}

#[tauri::command]
fn secret_status() -> SecretStatus {
    SecretStatus {
        modrinth: config::get_secret(Secret::ModrinthToken).is_some(),
        curseforge: config::get_secret(Secret::CurseForgeKey).is_some(),
    }
}

#[tauri::command]
fn save_secret(kind: String, value: String) -> CmdResult<Option<String>> {
    let k = secret_kind(&kind)?;
    config::set_secret(k, value.trim()).map_err(err)?;
    // The key is saved either way; a mistyped or wrong-kind key is flagged so it can be fixed.
    Ok(if kind == "curseforge" {
        mrimport_core::debuglog::key_warning(&value)
    } else {
        None
    })
}

#[tauri::command]
fn clear_secret(kind: String) -> CmdResult<()> {
    config::delete_secret(secret_kind(&kind)?).map_err(err)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishRequest {
    source: String,
    value: String,
    version_number: String,
    slug: String,
    title: String,
    summary: String,
    body: String,
    license: String,
    categories: String,
    client_side: String,
    server_side: String,
    game_versions: String,
    loaders: String,
    existing: bool,
    dry_run: bool,
}

fn split(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

#[tauri::command]
async fn publish_project(
    req: PublishRequest,
    on_event: Channel<Event>,
) -> CmdResult<PublishReport> {
    let rep = reporter(on_event);
    let defaults = ReleaseDefaults {
        game_versions: split(&req.game_versions),
        loaders: split(&req.loaders)
            .iter()
            .filter_map(|l| Loader::parse(l))
            .collect(),
        version_type: None,
    };
    let (base, releases) = match req.source.as_str() {
        "jars" => {
            let paths: Vec<PathBuf> = req
                .value
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(PathBuf::from)
                .collect();
            (
                None,
                publish::releases_from_jars(&paths, &defaults, &req.version_number).map_err(err)?,
            )
        }
        "github" => (
            None,
            publish::releases_from_github(req.value.trim(), &defaults)
                .await
                .map_err(err)?,
        ),
        "curseforge" => {
            let cf = cf_client().ok_or("Add your CurseForge API key in Settings first")?;
            let id: u64 = req
                .value
                .trim()
                .parse()
                .map_err(|_| "CurseForge project id must be a number")?;
            let (p, r) = publish::releases_from_curseforge(&cf, id, &defaults)
                .await
                .map_err(err)?;
            (Some(p), r)
        }
        _ => return Err("unknown source".into()),
    };
    let b = base.unwrap_or(ProjectSpec {
        title: String::new(),
        slug: String::new(),
        summary: String::new(),
        body: String::new(),
        categories: vec![],
        client_side: "optional".into(),
        server_side: "optional".into(),
        license_id: String::new(),
        source_url: None,
        issues_url: None,
    });
    let pick = |a: &str, d: String| {
        if a.trim().is_empty() {
            d
        } else {
            a.trim().to_string()
        }
    };
    let summary = pick(&req.summary, b.summary);
    let spec = ProjectSpec {
        title: pick(&req.title, b.title),
        slug: pick(&req.slug, b.slug),
        body: pick(
            &req.body,
            if b.body.is_empty() {
                summary.clone()
            } else {
                b.body
            },
        ),
        summary,
        categories: if req.categories.trim().is_empty() {
            b.categories
        } else {
            split(&req.categories)
        },
        client_side: req.client_side,
        server_side: req.server_side,
        license_id: req.license,
        source_url: b.source_url,
        issues_url: b.issues_url,
    };
    let plan = PublishPlan {
        create_project: !req.existing,
        project: spec,
        releases,
    };
    let token = config::get_secret(Secret::ModrinthToken);
    if token.is_none() && !req.dry_run {
        return Err("Add your Modrinth token in Settings first".into());
    }
    publish::publish(&plan, &Modrinth::new(token), req.dry_run, &rep)
        .await
        .map_err(err)
}

// ---------------------------------------------------------------------------------------------
// Find a modpack on CurseForge: search by name, or paste a link.
// ---------------------------------------------------------------------------------------------

#[derive(Serialize)]
struct ModpackHit {
    project_id: u64,
    name: String,
    summary: String,
    downloads: u64,
    thumbnail: Option<String>,
    url: String,
}

fn hit(p: &mrimport_core::curseforge::CfModpack) -> ModpackHit {
    ModpackHit {
        project_id: p.id,
        name: p.name.clone(),
        summary: p.summary.clone(),
        downloads: p.download_count as u64,
        thumbnail: p.logo.as_ref().and_then(|l| l.thumbnail_url.clone()),
        url: cfmodpack::page_url(p),
    }
}

#[derive(Serialize)]
struct FileOption {
    id: u64,
    name: String,
    game_versions: Vec<String>,
    /// 1 release, 2 beta, 3 alpha
    release_type: u8,
    date: Option<String>,
    /// False when CurseForge gave no direct download link (it may still work, or need the browser).
    downloadable: bool,
}

#[derive(Serialize)]
struct ModpackDetail {
    #[serde(flatten)]
    hit: ModpackHit,
    files: Vec<FileOption>,
    selected_file_id: u64,
}

#[derive(Serialize)]
struct FetchOutcome {
    path: Option<String>,
    /// Set when the file has to be downloaded in the browser.
    manual_url: Option<String>,
    message: String,
}

/// The last project looked up, with its files, so importing a version needn't ask CurseForge again.
type LastLookup = Option<(
    mrimport_core::curseforge::CfModpack,
    Vec<mrimport_core::curseforge::CfProjectFile>,
)>;
static LAST_LOOKUP: std::sync::Mutex<LastLookup> = std::sync::Mutex::new(None);

/// A previously looked-up file, if it belongs to `project_id`.
fn cached_selection(
    cache: &LastLookup,
    project_id: u64,
    file_id: u64,
) -> Option<cfmodpack::Selected> {
    let (project, files) = cache.as_ref().filter(|(p, _)| p.id == project_id)?;
    let file = files.iter().find(|f| f.id == file_id)?;
    Some(cfmodpack::Selected {
        project: project.clone(),
        file: file.clone(),
    })
}

fn core_client() -> CmdResult<CurseForge> {
    config::get_secret(Secret::CurseForgeKey)
        .map(CurseForge::new)
        .ok_or_else(|| {
            "Add a CurseForge Core API key in Settings to search and import by link.".to_string()
        })
}

#[tauri::command]
async fn cf_search(query: String) -> CmdResult<Vec<ModpackHit>> {
    let q = query.trim();
    if q.chars().count() < 2 {
        return Ok(vec![]);
    }
    let hits = core_client()?.search_modpacks(q, 12).await.map_err(err)?;
    Ok(hits.iter().map(hit).collect())
}

/// Resolve a pasted link, `cf:<id>` or project id (or a search hit's id) to a project, its
/// recent files, and the file that would be imported.
#[tauri::command]
async fn cf_resolve(input: String) -> CmdResult<ModpackDetail> {
    let cf = core_client()?;
    let r = cfmodpack::parse_ref(&input).map_err(err)?;
    let sel = cfmodpack::resolve(&cf, &r).await.map_err(err)?;
    let mut files = sel.project.latest_files.clone();
    for f in cf
        .recent_files(sel.project.id, 30)
        .await
        .unwrap_or_default()
    {
        if !files.iter().any(|x| x.id == f.id) {
            files.push(f);
        }
    }
    if !files.iter().any(|x| x.id == sel.file.id) {
        files.push(sel.file.clone());
    }
    *LAST_LOOKUP.lock().unwrap() = Some((sel.project.clone(), files.clone()));
    files.retain(|f| f.is_server_pack != Some(true));
    files.sort_by(|a, b| b.file_date.cmp(&a.file_date).then(b.id.cmp(&a.id)));
    Ok(ModpackDetail {
        hit: hit(&sel.project),
        selected_file_id: sel.file.id,
        files: files
            .iter()
            .map(|f| FileOption {
                id: f.id,
                name: f.display_name.clone(),
                game_versions: f.game_versions.clone(),
                release_type: f.release_type,
                date: f.file_date.clone(),
                downloadable: f.download_url.is_some(),
            })
            .collect(),
    })
}

/// Download one version of a modpack. The returned path is then imported like any dropped pack.
#[tauri::command]
async fn cf_fetch(
    project_id: u64,
    file_id: u64,
    on_event: Channel<Event>,
) -> CmdResult<FetchOutcome> {
    let cf = core_client()?;
    let cached = cached_selection(&LAST_LOOKUP.lock().unwrap(), project_id, file_id);
    let sel = match cached {
        Some(sel) => sel,
        None => cfmodpack::resolve(
            &cf,
            &cfmodpack::ModpackRef::Id {
                project_id,
                file_id: Some(file_id),
            },
        )
        .await
        .map_err(err)?,
    };
    let _ = std::fs::remove_dir_all(modpacks_dir());
    match cfmodpack::download(&cf, &sel, &modpacks_dir(), &reporter(on_event)).await {
        Ok(p) => Ok(FetchOutcome {
            path: Some(p.display().to_string()),
            manual_url: None,
            message: format!("Downloaded {}", sel.file.display_name),
        }),
        Err(mrimport_core::Error::NeedsManualDownload { name, url }) => Ok(FetchOutcome {
            path: None,
            message: format!(
                "The author of '{name}' only allows downloads on CurseForge. Download it in your browser, then drop the .zip here."
            ),
            manual_url: Some(url),
        }),
        Err(e) => Err(err(e)),
    }
}

/// Connectivity and credential check; the result is shown in Settings.
#[tauri::command]
async fn run_diagnostics() -> String {
    mrimport_core::diagnostics::run(&Modrinth::new(None), config::load_settings().keyless_lookup)
        .await
}

#[tauri::command]
fn save_debug_log(path: String) -> CmdResult<()> {
    mrimport_core::debuglog::save(&PathBuf::from(path)).map_err(err)
}

#[tauri::command]
fn debug_log_text() -> String {
    mrimport_core::debuglog::snapshot()
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            scan_pack,
            export_pack,
            open_path,
            secret_status,
            save_secret,
            clear_secret,
            publish_project,
            run_diagnostics,
            save_debug_log,
            debug_log_text,
            apply_manual,
            default_downloads_dir,
            open_link,
            get_settings,
            set_setting,
            discard_scan,
            cf_search,
            cf_resolve,
            cf_fetch
        ])
        .run(tauri::generate_context!())
        .expect("error while running Modrinth Importer");
}

#[cfg(test)]
mod tests {
    use super::*;
    use mrimport_core::curseforge::{CfModpack, CfProjectFile};

    fn project(id: u64) -> CfModpack {
        serde_json::from_value(serde_json::json!({"id": id, "name": "P", "slug": "p"})).unwrap()
    }
    fn file(id: u64) -> CfProjectFile {
        serde_json::from_value(serde_json::json!({
            "id": id, "fileName": "f.zip", "displayName": "f", "downloadUrl": null, "releaseType": 1
        }))
        .unwrap()
    }

    #[test]
    fn a_cached_file_is_only_used_for_its_own_project() {
        let cache: LastLookup = Some((project(7), vec![file(1), file(2)]));
        assert_eq!(cached_selection(&cache, 7, 2).unwrap().file.id, 2);
        assert!(
            cached_selection(&cache, 7, 99).is_none(),
            "unknown file falls back to a fresh lookup"
        );
        assert!(
            cached_selection(&cache, 8, 1).is_none(),
            "another project must not reuse the cache"
        );
        assert!(cached_selection(&None, 7, 1).is_none());
    }

    /// Search-result thumbnails come from CurseForge's image host; the webview's policy must allow it.
    #[test]
    fn the_content_security_policy_allows_curseforge_thumbnails_and_nothing_wild() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let csp = conf["app"]["security"]["csp"].as_str().unwrap();
        let img = csp
            .split(';')
            .map(str::trim)
            .find(|d| d.starts_with("img-src"))
            .unwrap();
        assert!(img.contains("https://*.forgecdn.net"), "{img}");
        assert!(
            !img.contains(" * ") && !img.ends_with(" *") && !img.contains("http:"),
            "{img}"
        );
        assert!(csp.contains("default-src 'self'"));
    }
}
