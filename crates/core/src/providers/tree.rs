//! A read-only view over a directory or a zip archive, so every provider can
//! treat both the same way.

use crate::model::Origin;
use crate::{Error, Result};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Tree {
    /// `(relative path with '/' separators, where to read it)`
    pub files: Vec<(String, Origin)>,
}

impl Tree {
    pub fn from_path(path: &Path) -> Result<Self> {
        if path.is_dir() {
            let mut files = vec![];
            for e in walkdir::WalkDir::new(path)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if !e.file_type().is_file() {
                    continue;
                }
                let rel = e
                    .path()
                    .strip_prefix(path)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                files.push((
                    rel,
                    Origin::File {
                        path: e.path().to_path_buf(),
                    },
                ));
            }
            files.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(Tree { files })
        } else if path.is_file() {
            let mut a = zip::ZipArchive::new(std::fs::File::open(path)?)?;
            let mut files = vec![];
            for i in 0..a.len() {
                let f = a.by_index(i)?;
                if f.is_dir() {
                    continue;
                }
                // `enclosed_name` rejects zip-slip paths such as `../x`.
                let Some(name) = f.enclosed_name() else {
                    continue;
                };
                let Ok(raw) = f.name().map(|n| n.into_owned()) else {
                    continue;
                };
                files.push((
                    name.to_string_lossy().replace('\\', "/"),
                    Origin::ZipEntry {
                        zip: path.to_path_buf(),
                        name: raw,
                    },
                ));
            }
            Ok(Tree { files })
        } else {
            Err(Error::Invalid(format!(
                "'{}' does not exist",
                path.display()
            )))
        }
    }

    pub fn get(&self, rel: &str) -> Option<&Origin> {
        self.files.iter().find(|(p, _)| p == rel).map(|(_, o)| o)
    }

    pub fn has(&self, rel: &str) -> bool {
        self.get(rel).is_some()
    }

    pub fn read(&self, rel: &str) -> Result<Vec<u8>> {
        self.get(rel)
            .ok_or_else(|| Error::Invalid(format!("'{rel}' not found")))?
            .read()
    }

    pub fn read_json<T: serde::de::DeserializeOwned>(&self, rel: &str) -> Result<T> {
        Ok(serde_json::from_slice(&self.read(rel)?)?)
    }

    pub fn read_string(&self, rel: &str) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.read(rel)?).into_owned())
    }

    /// Files below `prefix/` with the prefix removed.
    pub fn under(&self, prefix: &str) -> Vec<(String, Origin)> {
        let p = format!("{}/", prefix.trim_end_matches('/'));
        self.files
            .iter()
            .filter_map(|(path, o)| path.strip_prefix(&p).map(|r| (r.to_string(), o.clone())))
            .collect()
    }

    pub fn stripped(&self, prefix: &str) -> Tree {
        Tree {
            files: self.under(prefix),
        }
    }

    /// If every file lives in one top-level directory, return its name.
    pub fn single_root(&self) -> Option<String> {
        let first = self.files.first()?.0.split('/').next()?.to_string();
        let all = self
            .files
            .iter()
            .all(|(p, _)| p.starts_with(&format!("{first}/")));
        all.then_some(first)
    }
}
