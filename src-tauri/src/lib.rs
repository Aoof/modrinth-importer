//! Tauri shell: thin commands over `mrimport-core`.

use mrimport_core::{
    config::{self, Secret},
    curseforge::CurseForge,
    events::{Event, Reporter},
    export::mrpack::{open_with_default_app, write_mrpack, ExportOptions, ExportReport},
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

fn cf_client() -> Option<CurseForge> {
    config::get_secret(Secret::CurseForgeKey).map(CurseForge::new)
}

#[derive(Serialize, Deserialize)]
struct ScanResult {
    pack: ImportedPack,
    resolved: Vec<ResolvedMod>,
}

#[tauri::command]
async fn scan_pack(
    path: String,
    name: Option<String>,
    mc_version: Option<String>,
    loader: Option<String>,
    on_event: Channel<Event>,
) -> CmdResult<ScanResult> {
    let rep = reporter(on_event);
    let opts = LoadOptions {
        name,
        mc_version,
        loader: loader.as_deref().and_then(Loader::parse),
    };
    let ctx = LoadCtx {
        curseforge: cf_client(),
        reporter: rep.clone(),
    };
    let pack = providers::load(&PathBuf::from(path), &opts, &ctx)
        .await
        .map_err(err)?;
    let mr = Modrinth::new(config::get_secret(Secret::ModrinthToken));
    let resolved = resolve::resolve(&pack, &mr, &rep).await.map_err(err)?;
    Ok(ScanResult { pack, resolved })
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn export_pack(
    pack: ImportedPack,
    resolved: Vec<ResolvedMod>,
    output: String,
    bundle_unmatched: bool,
    download_remote: bool,
    loader_version: Option<String>,
    on_event: Channel<Event>,
) -> CmdResult<ExportReport> {
    let mr = Modrinth::new(None);
    let opts = ExportOptions {
        bundle_unmatched,
        download_remote,
        loader_version,
    };
    write_mrpack(
        &pack,
        &resolved,
        &PathBuf::from(output),
        &opts,
        &mr,
        &reporter(on_event),
    )
    .await
    .map_err(err)
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
fn save_secret(kind: String, value: String) -> CmdResult<()> {
    config::set_secret(secret_kind(&kind)?, &value).map_err(err)
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

/// Connectivity and credential check; the result is shown in Settings.
#[tauri::command]
async fn run_diagnostics() -> String {
    mrimport_core::diagnostics::run(&Modrinth::new(None)).await
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
            debug_log_text
        ])
        .run(tauri::generate_context!())
        .expect("error while running Modrinth Importer");
}
