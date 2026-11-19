//! Pack providers: detect what a path is and load it into an [`ImportedPack`].

mod curseforge_pack;
mod instance;
pub mod jarmeta;
mod packwiz;
pub mod tree;

use crate::cflookup::Lookup;
use crate::curseforge::CurseForge;
use crate::events::Reporter;
use crate::model::*;
use crate::{Error, Result};
use std::path::Path;
use tree::Tree;

/// Optional overrides supplied by the user.
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    pub name: Option<String>,
    pub mc_version: Option<String>,
    pub loader: Option<Loader>,
}

pub struct LoadCtx {
    /// How CurseForge `(projectID, fileID)` pairs are resolved.
    pub lookup: Lookup,
    pub reporter: Reporter,
}

impl LoadCtx {
    /// Core API only (no keyless fallbacks), silent progress.
    pub fn new(curseforge: Option<CurseForge>) -> Self {
        Self {
            lookup: Lookup::new(curseforge, false),
            reporter: Reporter::silent(),
        }
    }

    pub fn with_lookup(lookup: Lookup, reporter: Reporter) -> Self {
        Self { lookup, reporter }
    }
}

/// Identify the format of `path` without loading everything.
pub fn detect(path: &Path) -> Result<SourceKind> {
    let (_, kind) = detect_tree(path)?;
    Ok(kind)
}

fn detect_tree(path: &Path) -> Result<(Tree, SourceKind)> {
    let full = Tree::from_path(path)?;
    // Zip exports often wrap everything in one folder: try the root first, then that folder.
    if let Some(k) = classify(&full) {
        return Ok((full, k));
    }
    if let Some(root) = full.single_root() {
        let inner = full.stripped(&root);
        if let Some(k) = classify(&inner) {
            return Ok((inner, k));
        }
    }
    // A bare mods directory: jars live at the top level.
    if full
        .files
        .iter()
        .any(|(p, _)| !p.contains('/') && p.ends_with(".jar"))
    {
        let wrapped = Tree {
            files: full
                .files
                .iter()
                .map(|(p, o)| (format!("mods/{p}"), o.clone()))
                .collect(),
        };
        return Ok((wrapped, SourceKind::Folder));
    }
    Err(Error::UnknownFormat(path.display().to_string()))
}

fn classify(t: &Tree) -> Option<SourceKind> {
    if t.has("modrinth.index.json") {
        return Some(SourceKind::Modrinth);
    }
    if t.has("manifest.json") && curseforge_pack::is_manifest(t) {
        return Some(SourceKind::CurseForge);
    }
    if t.has("pack.toml") {
        return Some(SourceKind::Packwiz);
    }
    if t.has("mmc-pack.json") {
        return Some(SourceKind::Prism);
    }
    if t.has("minecraftinstance.json") {
        return Some(SourceKind::CurseForge);
    }
    if let Ok(v) = t.read_json::<serde_json::Value>("instance.json") {
        if v["launcher"].is_object() {
            return Some(SourceKind::ATLauncher);
        }
        if v["uuid"].is_string() || v["mcVersion"].is_string() {
            return Some(SourceKind::Ftb);
        }
    }
    if t.has("bin/version.json") || t.has("bin/modpack.jar") {
        return Some(SourceKind::Technic);
    }
    if t.files
        .iter()
        .any(|(p, _)| p.starts_with("mods/") && p.ends_with(".jar"))
    {
        return Some(SourceKind::Folder);
    }
    None
}

/// Detect and load any supported pack, instance or mods folder.
pub async fn load(path: &Path, opts: &LoadOptions, ctx: &LoadCtx) -> Result<ImportedPack> {
    ctx.reporter.stage("Reading pack");
    let (t, kind) = detect_tree(path)?;
    match kind {
        SourceKind::Modrinth => Err(Error::Invalid(
            "this is already a Modrinth pack (.mrpack); nothing to import".into(),
        )),
        SourceKind::CurseForge if t.has("manifest.json") && curseforge_pack::is_manifest(&t) => {
            curseforge_pack::load(&t, opts, ctx).await
        }
        SourceKind::CurseForge => instance::curseforge_instance(&t, opts),
        SourceKind::Packwiz => packwiz::load(&t, opts),
        SourceKind::Prism => instance::prism(&t, opts),
        SourceKind::ATLauncher => instance::atlauncher(&t, opts),
        SourceKind::Ftb => instance::ftb(&t, opts),
        SourceKind::Technic => instance::technic(&t, opts),
        SourceKind::Folder => instance::folder(&t, opts),
    }
}
