//! CurseForge modpack zips (`manifest.json` + `overrides/`).

use super::{jarmeta, tree::Tree, LoadCtx, LoadOptions};
use crate::hash::{sha1_hex, sha512_hex};
use crate::model::*;
use crate::{Error, Result};
use serde::Deserialize;
use std::collections::HashMap;

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
    let cf = ctx.curseforge.as_ref().ok_or(Error::MissingCredential(
        "CurseForge API key (set CURSEFORGE_API_KEY or run `mrimport auth curseforge`)",
    ))?;

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

    ctx.reporter.stage("Looking up files on CurseForge");
    let ids: Vec<u64> = m.files.iter().map(|f| f.file_id).collect();
    let mut cf_files = HashMap::new();
    for chunk in ids.chunks(400) {
        for f in cf.files(chunk).await? {
            cf_files.insert(f.id, f);
        }
    }
    let mod_ids: Vec<u64> = {
        let mut v: Vec<u64> = m.files.iter().map(|f| f.project_id).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let mut cf_mods = HashMap::new();
    for chunk in mod_ids.chunks(400) {
        for x in cf.mods(chunk).await? {
            cf_mods.insert(x.id, x);
        }
    }

    let mut mods = vec![];
    for f in &m.files {
        let file = cf_files.get(&f.file_id);
        let info = cf_mods.get(&f.project_id);
        let mut e = ModEntry::new(
            file.map(|x| x.file_name.clone())
                .unwrap_or_else(|| format!("cf-{}-{}.jar", f.project_id, f.file_id)),
        );
        e.cf = Some(CfRef {
            project_id: f.project_id,
            file_id: f.file_id,
        });
        e.name_hint = info.map(|i| i.name.clone());
        e.slug_hint = info.map(|i| i.slug.clone());
        if let Some(x) = file {
            e.sha1 = x.sha1().map(String::from);
            e.size = Some(x.file_length);
            e.download_url = x.download_url.clone();
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
