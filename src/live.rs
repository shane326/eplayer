use crate::paths;
use anyhow::{Context, Result};
use encoding_rs::{GBK, UTF_8};
use html_escape::decode_html_entities;
use reqwest::{Client, Url, header};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_LIVE_USER_AGENT: &str = "AptvPlayer/1.4.10";

#[derive(Clone)]
pub struct LiveClient {
    client: Client,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct LiveConfigFile {
    #[serde(default, alias = "live_sources")]
    pub lives: BTreeMap<String, LiveSourceEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveSourceEntry {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub ua: Option<String>,
    #[serde(default)]
    pub epg: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LiveSource {
    pub key: String,
    pub name: String,
    pub url: String,
    pub ua: Option<String>,
    pub epg: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LiveChannel {
    pub id: String,
    pub source: String,
    pub source_name: String,
    pub tvg_id: String,
    pub name: String,
    pub logo: String,
    pub group: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LivePlaylist {
    #[serde(default)]
    pub epg_url: Option<String>,
    #[serde(default)]
    pub channels: Vec<LiveChannel>,
}

impl LiveClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::limited(5))
            .user_agent(DEFAULT_LIVE_USER_AGENT)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self { client }
    }

    pub async fn channels(&self, source: &LiveSource) -> Result<LivePlaylist> {
        let ua = source.ua.as_deref().unwrap_or(DEFAULT_LIVE_USER_AGENT);
        let response = self
            .client
            .get(&source.url)
            .header(header::USER_AGENT, ua)
            .send()
            .await
            .with_context(|| format!("request live source {}", source.name))?
            .error_for_status()
            .with_context(|| format!("live source returned error {}", source.name))?;
        let bytes = response.bytes().await?;
        let text = decode_text(&bytes);
        Ok(parse_live_channels(source, &text))
    }
}

pub fn default_live_config_path() -> PathBuf {
    paths::app_data_dir().join("live-sources.json")
}

pub fn ensure_live_config_exists(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    if let Some(legacy_path) = paths::legacy_current_dir_file("live-sources.json") {
        if legacy_path.exists() && legacy_path != path {
            fs::copy(&legacy_path, path)
                .with_context(|| format!("copy {} to {}", legacy_path.display(), path.display()))?;
            return Ok(());
        }
    }
    let example = "{\n  \"lives\": {}\n}\n";
    fs::write(path, example).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn load_live_config(path: &Path) -> Result<LiveConfigFile> {
    ensure_live_config_exists(path)?;
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).context("parse live source config")
}

pub fn load_live_sources(path: &Path) -> Result<Vec<LiveSource>> {
    let file = load_live_config(path)?;
    let mut sources = file
        .lives
        .into_iter()
        .map(|(key, entry)| LiveSource {
            key,
            name: entry.name,
            url: entry.url,
            ua: entry.ua.filter(|value| !value.trim().is_empty()),
            epg: entry.epg.filter(|value| !value.trim().is_empty()),
            enabled: entry.enabled.unwrap_or(true),
        })
        .collect::<Vec<_>>();
    sources.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(sources)
}

pub fn save_live_sources(path: &Path, sources: &[LiveSource]) -> Result<()> {
    let lives = sources
        .iter()
        .map(|source| {
            (
                source.key.clone(),
                LiveSourceEntry {
                    name: source.name.clone(),
                    url: source.url.clone(),
                    ua: source.ua.clone().filter(|value| !value.trim().is_empty()),
                    epg: source.epg.clone().filter(|value| !value.trim().is_empty()),
                    enabled: Some(source.enabled),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let file = LiveConfigFile { lives };
    let text = serde_json::to_string_pretty(&file).context("serialize live source config")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, format!("{text}\n")).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn parse_live_channels(source: &LiveSource, content: &str) -> LivePlaylist {
    if is_m3u_content(content) {
        parse_m3u(source, content)
    } else {
        parse_txt_live(source, content)
    }
}

fn decode_text(bytes: &[u8]) -> String {
    let (text, _, had_errors) = UTF_8.decode(bytes);
    if !had_errors {
        return text.into_owned();
    }
    let (text, _, _) = GBK.decode(bytes);
    text.into_owned()
}

fn strip_bom(value: &str) -> &str {
    value.strip_prefix('\u{feff}').unwrap_or(value)
}

fn is_m3u_content(content: &str) -> bool {
    let normalized = strip_bom(content).trim();
    normalized.contains("#EXTM3U") || normalized.contains("#EXTINF")
}

fn parse_txt_live(source: &LiveSource, content: &str) -> LivePlaylist {
    let mut channels = Vec::new();
    let mut current_group = "未分组".to_string();
    let mut pending_line = String::new();

    for raw_line in content
        .lines()
        .map(|line| strip_bom(line).trim())
        .filter(|line| !line.is_empty())
    {
        let line = clean_live_text(raw_line);
        if let Some(group) = txt_group_name(&line) {
            current_group = group;
            pending_line.clear();
            continue;
        }

        let candidate = if pending_line.is_empty() {
            line
        } else {
            format!("{} {}", pending_line, line)
        };

        let Some((name, value)) = split_txt_channel_line(&candidate) else {
            pending_line = candidate;
            continue;
        };
        let Some(url) = resolve_live_url(&source.url, value) else {
            pending_line.clear();
            continue;
        };

        let index = channels.len();
        channels.push(LiveChannel {
            id: format!("{}-{index}", source.key),
            source: source.key.clone(),
            source_name: source.name.clone(),
            tvg_id: name.clone(),
            name,
            logo: String::new(),
            group: current_group.clone(),
            url,
        });
        pending_line.clear();
    }

    LivePlaylist {
        epg_url: source.epg.clone().filter(|value| !value.trim().is_empty()),
        channels,
    }
}

fn parse_m3u(source: &LiveSource, content: &str) -> LivePlaylist {
    let lines = content
        .lines()
        .map(|line| strip_bom(line).trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let mut channels = Vec::new();
    let epg_url = extract_m3u_epg_url(&lines)
        .or_else(|| source.epg.clone().filter(|value| !value.trim().is_empty()));

    let mut index = 0;
    let mut pending_group: Option<String> = None;
    while index < lines.len() {
        let line = lines[index];
        if let Some(group) = line.strip_prefix("#EXTGRP:") {
            let group = clean_live_text(group);
            if !group.is_empty() {
                pending_group = Some(group);
            }
            index += 1;
            continue;
        }
        if !line.starts_with("#EXTINF:") {
            index += 1;
            continue;
        }

        let tvg_name = attr_value(line, "tvg-name").unwrap_or_default();
        let title = line
            .rsplit_once(',')
            .map(|(_, title)| clean_live_text(title))
            .unwrap_or_default();
        let name = if !title.is_empty() { title } else { tvg_name };
        if name.is_empty() {
            index += 1;
            continue;
        }

        let mut url = None;
        let mut url_index = index + 1;
        while url_index < lines.len() {
            let candidate = lines[url_index];
            if candidate.starts_with("#EXTINF:") {
                break;
            }
            if !candidate.starts_with('#') {
                url = resolve_live_url(&source.url, candidate);
                break;
            }
            url_index += 1;
        }

        let found_url = url.is_some();
        if let Some(url) = url {
            let channel_index = channels.len();
            let tvg_id = attr_value(line, "tvg-id").unwrap_or_else(|| name.clone());
            let logo = attr_value(line, "tvg-logo")
                .and_then(|value| resolve_live_url(&source.url, &value))
                .unwrap_or_default();
            let group = attr_value(line, "group-title")
                .or_else(|| pending_group.take())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "未分组".to_string());
            channels.push(LiveChannel {
                id: format!("{}-{channel_index}", source.key),
                source: source.key.clone(),
                source_name: source.name.clone(),
                tvg_id,
                name,
                logo,
                group,
                url,
            });
        }

        index = if found_url {
            url_index.saturating_add(1)
        } else {
            url_index.max(index + 1)
        };
    }

    LivePlaylist { epg_url, channels }
}

fn extract_m3u_epg_url(lines: &[&str]) -> Option<String> {
    for line in lines {
        if line.starts_with("#EXTINF:") {
            break;
        }
        if !line.starts_with("#EXTM3U") {
            continue;
        }
        if let Some(value) = attr_value(line, "x-tvg-url")
            .or_else(|| attr_value(line, "url-tvg"))
            .map(|value| value.trim().trim_end_matches(',').to_string())
            .filter(|value| !value.is_empty())
        {
            return Some(value);
        }
    }
    None
}

fn attr_value(line: &str, key: &str) -> Option<String> {
    let pattern = format!("{key}=\"");
    let start = line.find(&pattern)? + pattern.len();
    let end = line[start..].find('"')? + start;
    Some(clean_live_text(&line[start..end]))
}

fn resolve_live_url(base: &str, value: &str) -> Option<String> {
    let value = clean_live_text(value);
    let value = value.trim();
    if value.contains("://") {
        return Some(value.to_string());
    }
    if value.starts_with("//") {
        let base = Url::parse(base).ok()?;
        return Some(format!("{}:{value}", base.scheme()));
    }
    Url::parse(base)
        .ok()
        .and_then(|base| base.join(value).ok())
        .map(|url| url.to_string())
}

fn clean_live_text(value: &str) -> String {
    decode_html_entities(value.trim())
        .into_owned()
        .replace("&amp%3B", "&")
        .replace("&amp%3b", "&")
}

fn txt_group_name(line: &str) -> Option<String> {
    let (name, value) = line.rsplit_once(',')?;
    if value.trim() == "#genre#" {
        let name = name.trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

fn split_txt_channel_line(line: &str) -> Option<(String, &str)> {
    let lower = line.to_ascii_lowercase();
    let split_at = [
        ",http://",
        ",https://",
        ",rtmp://",
        ",rtsp://",
        ",rtp://",
        ",udp://",
    ]
    .iter()
    .filter_map(|marker| lower.find(marker))
    .min()?;
    let name = line[..split_at].trim();
    let value = line[split_at + 1..].trim();
    if name.is_empty() || value.is_empty() {
        None
    } else {
        Some((name.to_string(), value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> LiveSource {
        LiveSource {
            key: "test".to_string(),
            name: "测试直播".to_string(),
            url: "https://example.com/live/list.m3u".to_string(),
            ua: None,
            epg: None,
            enabled: true,
        }
    }

    #[test]
    fn parses_m3u_channels() {
        let content = r#"#EXTM3U x-tvg-url="https://example.com/epg.xml"
#EXTINF:-1 tvg-id="cctv1" tvg-name="CCTV-1" tvg-logo="https://img/logo.png" group-title="央视",CCTV-1 综合
https://stream.example.com/cctv1.m3u8
#EXTINF:-1 group-title="卫视",湖南卫视
/hunan.m3u8
"#;
        let playlist = parse_live_channels(&source(), content);
        assert_eq!(playlist.channels.len(), 2);
        assert_eq!(
            playlist.epg_url.as_deref(),
            Some("https://example.com/epg.xml")
        );
        assert_eq!(playlist.channels[0].name, "CCTV-1 综合");
        assert_eq!(playlist.channels[0].group, "央视");
        assert_eq!(playlist.channels[0].tvg_id, "cctv1");
        assert_eq!(playlist.channels[0].logo, "https://img/logo.png");
        assert_eq!(playlist.channels[1].url, "https://example.com/hunan.m3u8");
    }

    #[test]
    fn parses_gntv_style_m3u_channels() {
        let content = r#"#EXTM3U
#EXTINF:-1 group-title="央视频道",CCTV-1综合
http://207.56.13.146:81/cdnlive/cctv1.m3u8
#EXTINF:-1 group-title="央视频道",CCTV-2财经
http://207.56.13.146:81/cdnlive/cctv2.m3u8
"#;
        let playlist = parse_live_channels(&source(), content);
        assert_eq!(playlist.channels.len(), 2);
        assert_eq!(playlist.channels[0].name, "CCTV-1综合");
        assert_eq!(playlist.channels[0].group, "央视频道");
        assert_eq!(
            playlist.channels[0].url,
            "http://207.56.13.146:81/cdnlive/cctv1.m3u8"
        );
    }

    #[test]
    fn parses_txt_channels() {
        let content =
            "央视,#genre#\nCCTV-1,http://example.com/1.m3u8\nCCTV-2,https://example.com/2.m3u8\n";
        let playlist = parse_live_channels(&source(), content);
        assert_eq!(playlist.channels.len(), 2);
        assert_eq!(playlist.channels[0].name, "CCTV-1");
        assert_eq!(playlist.channels[0].group, "央视");
    }

    #[test]
    fn parses_txt_channel_names_with_commas_and_multiline_titles() {
        let content = "体育,#genre#\n城超 领头羊VS一轮游 2026赛季武汉篮球城市超级联赛淘汰赛\n16进8: 领头羊vs一轮游 17:00,http://192.168.1.8:1905/964991542\n";
        let playlist = parse_live_channels(&source(), content);
        assert_eq!(playlist.channels.len(), 1);
        assert_eq!(
            playlist.channels[0].name,
            "城超 领头羊VS一轮游 2026赛季武汉篮球城市超级联赛淘汰赛 16进8: 领头羊vs一轮游 17:00"
        );
        assert_eq!(
            playlist.channels[0].url,
            "http://192.168.1.8:1905/964991542"
        );
    }

    #[test]
    fn decodes_html_entities_in_live_urls() {
        let content = "#EXTM3U\n#EXTINF:-1 group-title=\"Local\",Demo\nhttps://example.com/live.m3u8?txTime=1&amp;txSecret=2\n";
        let playlist = parse_live_channels(&source(), content);
        assert_eq!(
            playlist.channels[0].url,
            "https://example.com/live.m3u8?txTime=1&txSecret=2"
        );
    }
}
