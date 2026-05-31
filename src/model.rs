use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SourceConfigFile {
    #[serde(default = "default_cache_time")]
    pub cache_time: u64,
    #[serde(default)]
    pub api_site: BTreeMap<String, SourceEntry>,
}

fn default_cache_time() -> u64 {
    7200
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SourceEntry {
    pub api: String,
    pub name: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub key: String,
    pub name: String,
    pub api: String,
    pub detail: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchResult {
    pub id: String,
    pub source: String,
    pub source_name: String,
    pub title: String,
    pub poster: String,
    pub category: String,
    pub year: String,
    pub actor: String,
    pub director: String,
    pub area: String,
    pub language: String,
    pub genre: String,
    pub description: String,
    pub remarks: String,
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Category {
    pub id: String,
    pub parent_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LibraryPage {
    pub items: Vec<SearchResult>,
    pub page: u32,
    pub page_count: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Episode {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkipConfig {
    pub source: String,
    pub video_id: String,
    pub intro_end_sec: i64,
    pub outro_offset_sec: i64,
    pub enabled: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayHistory {
    pub source: String,
    pub video_id: String,
    pub episode_index: i64,
    pub progress_sec: i64,
    pub duration_sec: i64,
    pub title: String,
    pub episode_title: String,
    pub poster: String,
    pub updated_at: i64,
}
