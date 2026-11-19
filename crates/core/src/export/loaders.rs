//! Looks up a sensible default loader version when the source pack did not record one.

use crate::model::Loader;
use crate::{Error, Result};

pub async fn latest_loader_version(loader: Loader, mc: &str) -> Result<Option<String>> {
    let http = reqwest::Client::builder()
        .user_agent(crate::user_agent())
        .build()?;
    let fail = |what: &str| {
        Error::Invalid(format!(
            "could not look up a {what} version for Minecraft {mc}; pass --loader-version"
        ))
    };
    match loader {
        Loader::Vanilla => Ok(None),
        Loader::Fabric | Loader::Quilt => {
            let url = if loader == Loader::Fabric {
                "https://meta.fabricmc.net/v2/versions/loader"
            } else {
                "https://meta.quiltmc.org/v3/versions/loader"
            };
            let v: Vec<serde_json::Value> = http.get(url).send().await?.json().await?;
            v.iter()
                .find(|e| {
                    e["stable"].as_bool().unwrap_or(true)
                        && !e["version"].as_str().unwrap_or("").contains("beta")
                })
                .and_then(|e| e["version"].as_str().map(String::from))
                .map(Some)
                .ok_or_else(|| fail("loader"))
        }
        Loader::Forge => {
            let v: serde_json::Value = http
                .get("https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json")
                .send()
                .await?
                .json()
                .await?;
            let p = &v["promos"];
            p[format!("{mc}-recommended")]
                .as_str()
                .or_else(|| p[format!("{mc}-latest")].as_str())
                .map(|s| Some(s.to_string()))
                .ok_or_else(|| fail("Forge"))
        }
        Loader::NeoForge => {
            // 1.21.1 -> 21.1.x ; 1.21 -> 21.0.x
            let parts: Vec<&str> = mc.split('.').collect();
            let prefix = match parts.as_slice() {
                ["1", minor] => format!("{minor}.0."),
                ["1", minor, patch] => format!("{minor}.{patch}."),
                _ => return Err(fail("NeoForge")),
            };
            let v: serde_json::Value = http
                .get("https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge")
                .send()
                .await?
                .json()
                .await?;
            v["versions"]
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .rfind(|s| s.starts_with(&prefix) && !s.contains("beta"))
                })
                .map(|s| Some(s.to_string()))
                .ok_or_else(|| fail("NeoForge"))
        }
    }
}
