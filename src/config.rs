use crate::model::{Source, SourceConfigFile, SourceEntry};
use crate::paths;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const EMPTY_SOURCE_CONFIG: &str = "{\n  \"cache_time\": 7200,\n  \"api_site\": {}\n}\n";

pub fn default_config_path() -> PathBuf {
    paths::app_data_dir().join("sources.json")
}

pub fn ensure_config_exists(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, EMPTY_SOURCE_CONFIG).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn load_config(path: &Path) -> Result<SourceConfigFile> {
    ensure_config_exists(path)?;
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).context("parse source config")
}

pub fn load_sources(path: &Path) -> Result<Vec<Source>> {
    let file = load_config(path)?;

    let mut sources = file
        .api_site
        .into_iter()
        .map(|(key, entry)| Source {
            detail: entry
                .detail
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            key,
            name: entry.name,
            api: entry.api.trim_end_matches('/').to_string(),
            enabled: entry.enabled.unwrap_or(true),
        })
        .collect::<Vec<_>>();

    sources.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(sources)
}

pub fn save_sources(path: &Path, cache_time: u64, sources: &[Source]) -> Result<()> {
    let api_site = sources
        .iter()
        .map(|source| {
            (
                source.key.clone(),
                SourceEntry {
                    api: source.api.trim_end_matches('/').to_string(),
                    name: source.name.clone(),
                    detail: source
                        .detail
                        .clone()
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty()),
                    enabled: Some(source.enabled),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let file = SourceConfigFile {
        cache_time,
        api_site,
    };
    let text = serde_json::to_string_pretty(&file).context("serialize source config")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, format!("{text}\n")).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn missing_config_creates_empty_sources_file() {
        let dir = std::env::temp_dir().join(format!(
            "moontv-source-config-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sources.json");

        ensure_config_exists(&path).unwrap();
        let config = load_config(&path).unwrap();

        assert_eq!(config.cache_time, 7200);
        assert!(config.api_site.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_sources_keeps_missing_detail_empty() {
        let dir = std::env::temp_dir().join(format!(
            "moontv-source-config-detail-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sources.json");
        fs::write(
            &path,
            r#"{
  "cache_time": 7200,
  "api_site": {
    "zy360": {
      "api": "https://360zy.com/api.php/provide/vod",
      "name": "360资源",
      "detail": null,
      "enabled": true
    }
  }
}
"#,
        )
        .unwrap();

        let sources = load_sources(&path).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].detail, None);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn save_sources_preserves_explicit_detail_only() {
        let dir = std::env::temp_dir().join(format!(
            "moontv-source-config-save-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sources.json");

        save_sources(
            &path,
            7200,
            &[Source {
                key: "zy360".to_string(),
                name: "360资源".to_string(),
                api: "https://360zy.com/api.php/provide/vod".to_string(),
                detail: Some("https://360zy.com".to_string()),
                enabled: true,
            }],
        )
        .unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"detail\": \"https://360zy.com\""));

        let _ = fs::remove_dir_all(dir);
    }
}
