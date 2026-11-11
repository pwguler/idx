//! Raw zone: verbatim upstream payloads, one zstd NDJSON file per request key at
//! `<root>/<source>/<dataset>/date=YYYY-MM-DD/<key>.ndjson.zst`.
//!
//! A re-fetch of a key replaces that key's file atomically (temp file + rename),
//! so a partial run never leaves a torn file and never touches other keys. The
//! raw zone is the permanent record of what the APIs returned (ADR 0001).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One fetched payload plus the request key it answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    /// RFC 3339 UTC instant of the fetch; becomes `updated_at` downstream.
    pub fetched_at: String,
    /// Request key within the partition: a ticker, or a fixed name for
    /// market-wide endpoints.
    pub key: String,
    pub payload: Value,
}

/// `<source>/<dataset>/date=<date>` within the raw zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    source: String,
    dataset: String,
    date: String,
}

impl Partition {
    #[must_use]
    pub fn new(source: &str, dataset: &str, date: &str) -> Self {
        Self {
            source: source.to_string(),
            dataset: dataset.to_string(),
            date: date.to_string(),
        }
    }

    fn dir(&self, root: &Path) -> PathBuf {
        root.join(&self.source)
            .join(&self.dataset)
            .join(format!("date={}", self.date))
    }
}

#[derive(Debug, Clone)]
pub struct RawStore {
    root: PathBuf,
}

impl RawStore {
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    fn file(&self, p: &Partition, key: &str) -> Result<PathBuf> {
        if key.is_empty() || key.starts_with('.') || key.contains(['/', '\\']) {
            bail!("invalid raw key {key:?}");
        }
        Ok(p.dir(&self.root).join(format!("{key}.ndjson.zst")))
    }

    /// Write `env` as the only line of its key's file, replacing any previous fetch.
    ///
    /// # Errors
    ///
    /// Fails on an invalid key or a filesystem error.
    pub fn write(&self, p: &Partition, env: &Envelope) -> Result<()> {
        let path = self.file(p, &env.key)?;
        let dir = path.parent().context("raw file has a parent")?;
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        let mut line = serde_json::to_vec(env)?;
        line.push(b'\n');
        let tmp = dir.join(format!(".{}.tmp", env.key));
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&zstd::encode_all(&line[..], 3)?)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &path).with_context(|| format!("replace {}", path.display()))?;
        Ok(())
    }

    #[must_use]
    pub fn exists(&self, p: &Partition, key: &str) -> bool {
        self.file(p, key).is_ok_and(|f| f.exists())
    }

    /// Newest partition date of a dataset, if any.
    #[must_use]
    pub fn latest_date(&self, source: &str, dataset: &str) -> Option<String> {
        fs::read_dir(self.root.join(source).join(dataset))
            .ok()?
            .filter_map(|e| {
                let name = e.ok()?.file_name().to_string_lossy().into_owned();
                name.strip_prefix("date=").map(str::to_string)
            })
            .max()
    }

    /// Every envelope of a dataset with its partition date, in date then key order.
    ///
    /// # Errors
    ///
    /// Fails on unreadable or malformed files.
    pub fn read_dataset(&self, source: &str, dataset: &str) -> Result<Vec<(String, Envelope)>> {
        let root = self.root.join(source).join(dataset);
        let mut out = Vec::new();
        let Ok(parts) = fs::read_dir(&root) else {
            return Ok(out);
        };
        let mut parts: Vec<_> = parts.collect::<std::io::Result<_>>()?;
        parts.sort_by_key(fs::DirEntry::file_name);
        for part in parts {
            let name = part.file_name().to_string_lossy().into_owned();
            let Some(date) = name.strip_prefix("date=") else {
                continue;
            };
            let mut files: Vec<_> = fs::read_dir(part.path())?.collect::<std::io::Result<_>>()?;
            files.sort_by_key(fs::DirEntry::file_name);
            for f in files {
                let fname = f.file_name().to_string_lossy().into_owned();
                if !fname.ends_with(".ndjson.zst") || fname.starts_with('.') {
                    continue;
                }
                let bytes = fs::read(f.path())?;
                let text = String::from_utf8(zstd::decode_all(&bytes[..])?)
                    .with_context(|| format!("{} is not UTF-8", f.path().display()))?;
                for line in text.lines().filter(|l| !l.trim().is_empty()) {
                    let env: Envelope = serde_json::from_str(line)
                        .with_context(|| format!("malformed line in {}", f.path().display()))?;
                    out.push((date.to_string(), env));
                }
            }
        }
        Ok(out)
    }
}
