//! CurseForge modpack zips (`manifest.json` + `overrides/`).

use super::{jarmeta, tree::Tree, LoadCtx, LoadOptions};
use crate::hash::{sha1_hex, sha512_hex};
use crate::model::*;
use crate::Result;
use serde::Deserialize;

#[derive(Deserialize)]
struct Manifest {
    minecraft: Mc,
    name: Option<String>,
    version: Option<String>,
    #[serde(default)]
    files: Vec<MFile>,
    overrides: Option<String>,
}
#[derive(Deserialize)]
struct Mc {
    version: String,
    #[serde(rename = "modLoaders", default)]
    mod_loaders: Vec<ML>,
}
#[derive(Deserialize)]
struct ML {
    id: String,
    #[serde(default)]
    primary: bool,
}
#[derive(Deserialize)]
struct MFile {
    #[serde(rename = "projectID")]
    project_id: u64,
    #[serde(rename = "fileID")]
    file_id: u64,
}

pub fn is_manifest(t: &Tree) -> bool {
    t.read_json::<serde_json::Value>("manifest.json")
        .map(|v| v["manifestType"] == "minecraftModpack" || v["minecraft"].is_object())
        .unwrap_or(false)
}

pub async fn load(t: &Tree, opts: &LoadOptions, ctx: &LoadCtx) -> Result<ImportedPack> {
    let m: Manifest = t.read_json("manifest.json")?;
    let primary = m
        .minecraft
        .mod_loaders
        .iter()
        .find(|l| l.primary)
        .or(m.minecraft.mod_loaders.first());
    let (loader, loader_version) = primary
        .map(|l| match l.id.split_once('-') {
            Some((k, v)) => (
                Loader::parse(k).unwrap_or(Loader::Vanilla),
                Some(v.to_string()),
            ),
            None => (Loader::parse(&l.id).unwrap_or(Loader::Vanilla), None),
        })
        .unwrap_or((Loader::Vanilla, None));

    let refs: Vec<CfRef> = m
        .files
        .iter()
        .map(|f| CfRef {
            project_id: f.project_id,
            file_id: f.file_id,
        })
        .collect();
    let outcome = ctx.lookup.files(&refs, &ctx.reporter).await?;
    for w in &outcome.warnings {
        ctx.reporter.log(w.clone());
    }

    let mut mods = vec![];
    for r in &refs {
        let info = outcome.infos.get(r);
        let mut e = ModEntry::new(
            info.and_then(|i| i.file_name.clone())
                .unwrap_or_else(|| format!("cf-{}-{}.jar", r.project_id, r.file_id)),
        );
        e.cf = Some(*r);
        if let Some(i) = info {
            e.expected_file_name = i.file_name.clone();
            e.name_hint = i.project_name.clone();
            e.slug_hint = i.slug.clone();
            e.cf_url = i.project_url.clone();
            e.sha1 = i.sha1.clone();
            e.sha512 = i.sha512.clone();
            e.size = i.size;
            e.download_url = i.download_url.clone();
            e.lookup_sources = i.sources.clone();
        }
        mods.push(e);
    }

    let prefix = m.overrides.clone().unwrap_or_else(|| "overrides".into());
    let mut overrides = vec![];
    for (rel, origin) in t.under(&prefix) {
        if rel.starts_with("mods/") && rel.to_ascii_lowercase().ends_with(".jar") {
            let bytes = origin.read()?;
            let jm = jarmeta::inspect(&bytes);
            let mut e = ModEntry::new(rel.rsplit('/').next().unwrap_or(&rel));
            e.sha1 = Some(sha1_hex(&bytes));
            e.sha512 = Some(sha512_hex(&bytes));
            e.size = Some(bytes.len() as u64);
            e.name_hint = jm.name;
            e.slug_hint = jm.mod_id;
            e.origin = Some(origin);
            mods.push(e);
        } else {
            overrides.push(OverrideFile { path: rel, origin });
        }
    }

    Ok(ImportedPack {
        source: SourceKind::CurseForge,
        name: opts
            .name
            .clone()
            .or(m.name)
            .unwrap_or_else(|| "CurseForge pack".into()),
        version: m.version.unwrap_or_else(|| "1.0.0".into()),
        summary: None,
        mc_version: opts.mc_version.clone().unwrap_or(m.minecraft.version),
        loader: opts.loader.unwrap_or(loader),
        loader_version,
        mods,
        overrides,
    })
}
