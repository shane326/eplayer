use crate::model::{Category, Episode, LibraryPage, SearchResult, Source};
use anyhow::{Context, Result};
use encoding_rs::{GBK, UTF_8};
use futures_util::future::join_all;
use html_escape::decode_html_entities;
use reqwest::{Client, Url, header};
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone)]
pub struct CmsClient {
    client: Client,
}

#[derive(Debug, Deserialize)]
struct MacCmsResponse {
    #[serde(default)]
    list: Vec<MacCmsItem>,
    #[serde(default, rename = "class")]
    classes: Vec<MacCmsClass>,
    #[serde(default, deserialize_with = "deserialize_u32")]
    total: u32,
    #[serde(default, deserialize_with = "deserialize_u32")]
    page: u32,
    #[serde(default, deserialize_with = "deserialize_u32")]
    pagecount: u32,
}

#[derive(Debug, Deserialize)]
struct MacCmsClass {
    #[serde(default)]
    type_id: serde_json::Value,
    #[serde(default)]
    type_pid: serde_json::Value,
    #[serde(default)]
    type_name: String,
}

#[derive(Debug, Deserialize)]
struct MacCmsItem {
    #[serde(default)]
    vod_id: serde_json::Value,
    #[serde(default)]
    vod_name: String,
    #[serde(default)]
    type_name: String,
    #[serde(default)]
    vod_pic: String,
    #[serde(default)]
    vod_year: String,
    #[serde(default)]
    vod_actor: String,
    #[serde(default)]
    vod_director: String,
    #[serde(default)]
    vod_area: String,
    #[serde(default)]
    vod_lang: String,
    #[serde(default)]
    vod_class: String,
    #[serde(default)]
    vod_content: String,
    #[serde(default)]
    vod_remarks: String,
    #[serde(default)]
    vod_play_url: String,
}

impl CmsClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .build()
            .context("build http client")?;

        Ok(Self { client })
    }

    pub async fn search_all(&self, sources: &[Source], keyword: &str) -> Vec<SearchResult> {
        let tasks = sources
            .iter()
            .filter(|source| source.enabled)
            .cloned()
            .map(|source| {
                let client = self.clone();
                let keyword = keyword.to_string();
                async move {
                    client
                        .search_source(&source, &keyword)
                        .await
                        .unwrap_or_default()
                }
            });

        join_all(tasks).await.into_iter().flatten().collect()
    }

    pub async fn search_source(&self, source: &Source, keyword: &str) -> Result<Vec<SearchResult>> {
        let url = self.build_url(
            &source.api,
            &[("ac", "videolist"), ("wd", keyword), ("pg", "1")],
        )?;
        let data = self.fetch_json(url).await?;

        Ok(data
            .list
            .into_iter()
            .map(|item| item.into_search_result(source))
            .filter(|item| !item.title.is_empty())
            .collect())
    }

    pub async fn categories(&self, source: &Source) -> Result<Vec<Category>> {
        let url = self.build_url(&source.api, &[("ac", "list")])?;
        let data = self.fetch_json(url).await?;

        let mut categories = vec![Category {
            id: String::new(),
            parent_id: String::new(),
            name: "最新".to_string(),
        }];
        let classes = data.classes;
        categories.extend(
            classes
                .into_iter()
                .filter(MacCmsClass::is_visible_category)
                .map(MacCmsClass::into_category)
                .filter(|item| !item.id.is_empty() && !item.name.is_empty()),
        );
        Ok(categories)
    }

    pub async fn videos(
        &self,
        source: &Source,
        category_id: &str,
        page: u32,
    ) -> Result<LibraryPage> {
        let page = page.max(1).to_string();
        let params = if category_id.is_empty() {
            vec![("ac", "videolist"), ("pg", page.as_str())]
        } else {
            vec![
                ("ac", "videolist"),
                ("t", category_id),
                ("pg", page.as_str()),
            ]
        };
        let url = self.build_url(&source.api, &params)?;
        let data = self.fetch_json(url).await?;
        let page = data.page.max(page.parse().unwrap_or(1));
        let items = data
            .list
            .into_iter()
            .map(|item| item.into_search_result(source))
            .filter(|item| !item.title.is_empty())
            .collect();

        Ok(LibraryPage {
            items,
            page,
            page_count: data.pagecount,
            total: data.total,
        })
    }

    pub async fn detail(&self, source: &Source, id: &str) -> Result<SearchResult> {
        let url = self.build_url(&source.api, &[("ac", "detail"), ("ids", id)])?;
        let data = self.fetch_json(url).await?;
        let item = data
            .list
            .into_iter()
            .next()
            .context("empty detail response")?;
        Ok(item.into_search_result(source))
    }

    fn build_url(&self, base: &str, params: &[(&str, &str)]) -> Result<Url> {
        let mut url = Url::parse(base).with_context(|| format!("invalid source url: {base}"))?;
        {
            let mut query = url.query_pairs_mut();
            for (key, value) in params {
                query.append_pair(key, value);
            }
        }
        Ok(url)
    }

    async fn fetch_json(&self, url: Url) -> Result<MacCmsResponse> {
        let response = self
            .client
            .get(url.clone())
            .header(header::ACCEPT, "application/json")
            .send()
            .await?
            .error_for_status()?;
        let charset = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(content_type_charset);
        let bytes = response.bytes().await?;
        let text = decode_response_body(&bytes, charset);
        serde_json::from_str::<MacCmsResponse>(&text)
            .with_context(|| format!("parse cms json from {url}"))
    }
}

impl MacCmsClass {
    fn is_visible_category(&self) -> bool {
        let id = value_to_id(&self.type_id);
        !id.is_empty()
    }

    fn into_category(self) -> Category {
        Category {
            id: value_to_id(&self.type_id),
            parent_id: value_to_id(&self.type_pid),
            name: clean_text(&self.type_name),
        }
    }
}

impl MacCmsItem {
    fn into_search_result(self, source: &Source) -> SearchResult {
        let episodes = parse_episodes(&self.vod_play_url);

        SearchResult {
            id: value_to_id(&self.vod_id),
            source: source.key.clone(),
            source_name: source.name.clone(),
            title: clean_text(&self.vod_name),
            poster: self.vod_pic,
            category: clean_text(&self.type_name),
            year: clean_text(&self.vod_year),
            actor: clean_text(&self.vod_actor),
            director: clean_text(&self.vod_director),
            area: clean_text(&self.vod_area),
            language: clean_text(&self.vod_lang),
            genre: clean_text(&self.vod_class),
            description: clean_html(&self.vod_content),
            remarks: clean_text(&self.vod_remarks),
            episodes,
        }
    }
}

fn value_to_id(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(v) => v.clone(),
        serde_json::Value::Number(v) => v.to_string(),
        _ => String::new(),
    }
}

pub fn parse_episodes(play_url: &str) -> Vec<Episode> {
    let groups = play_url
        .split("$$$")
        .map(parse_episode_group)
        .collect::<Vec<_>>();

    groups
        .iter()
        .map(|episodes| {
            episodes
                .iter()
                .filter(|episode| is_m3u8_url(&episode.url))
                .cloned()
                .collect::<Vec<_>>()
        })
        .max_by_key(|episodes| episodes.len())
        .filter(|episodes| !episodes.is_empty())
        .or_else(|| groups.into_iter().max_by_key(|episodes| episodes.len()))
        .unwrap_or_default()
}

fn parse_episode_group(group: &str) -> Vec<Episode> {
    group
        .split('#')
        .filter_map(|part| {
            let (title, url) = part.split_once('$')?;
            let url = url.trim();
            if url.is_empty() {
                return None;
            }
            let title = clean_text(title);
            if is_ad_like_episode(&title, url) {
                return None;
            }
            Some(Episode {
                title,
                url: url.to_string(),
            })
        })
        .collect()
}

fn is_m3u8_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower
        .split('?')
        .next()
        .map(|path| path.ends_with(".m3u8"))
        .unwrap_or(false)
        || lower.contains(".m3u8?")
        || lower.contains("/m3u8/")
}

fn is_ad_like_episode(title: &str, url: &str) -> bool {
    let title = title.trim().to_lowercase();
    title == "ad"
        || title == "ads"
        || title.contains("广告")
        || title.contains("贴片")
        || title.contains("赞助")
        || contains_ad_marker(url)
}

fn contains_ad_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "sponsor",
        "/ad/",
        "/ads/",
        "advert",
        "advertisement",
        "/adjump",
        "redtraffic",
        "googleads",
        "doubleclick",
        "imasdk",
        "adservice",
        "adserver",
        "pre-roll",
        "preroll",
        "midroll",
    ]
    .iter()
    .any(|keyword| lower.contains(keyword))
}

fn clean_text(value: &str) -> String {
    decode_html_entities(value).trim().replace('\u{a0}', " ")
}

fn clean_html(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    clean_text(&output)
}

fn content_type_charset(content_type: &str) -> Option<&'static encoding_rs::Encoding> {
    content_type
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("charset="))
        .and_then(|name| encoding_rs::Encoding::for_label(name.trim_matches('"').as_bytes()))
}

fn decode_response_body(bytes: &[u8], charset: Option<&'static encoding_rs::Encoding>) -> String {
    if let Some(encoding) = charset {
        let (text, _, had_errors) = encoding.decode(bytes);
        if !had_errors {
            return text.into_owned();
        }
    }

    let (text, _, had_errors) = UTF_8.decode(bytes);
    if !had_errors {
        return text.into_owned();
    }

    let (text, _, _) = GBK.decode(bytes);
    text.into_owned()
}

fn deserialize_u32<'de, D>(deserializer: D) -> std::result::Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Number(number) => number.as_u64().unwrap_or(0) as u32,
        serde_json::Value::String(text) => text.parse::<u32>().unwrap_or(0),
        _ => 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_largest_episode_group() {
        let episodes = parse_episodes("A$u1#B$u2$$$A$u3#B$u4#C$u5");
        assert_eq!(episodes.len(), 3);
        assert_eq!(episodes[2].title, "C");
        assert_eq!(episodes[2].url, "u5");
    }

    #[test]
    fn prefers_m3u8_episode_group() {
        let episodes = parse_episodes(
            "AD$https://cdn.example.com/ad.mp4#正片$https://cdn.example.com/movie.mp4$$$第1集$https://cdn.example.com/ep1.m3u8#第2集$https://cdn.example.com/ep2.m3u8",
        );
        assert_eq!(episodes.len(), 2);
        assert_eq!(episodes[0].title, "第1集");
        assert!(episodes[0].url.ends_with("ep1.m3u8"));
    }

    #[test]
    fn filters_ad_episode_entries_before_group_selection() {
        let episodes = parse_episodes(
            "广告$https://cdn.example.com/ads/preroll.m3u8#第1集$https://cdn.example.com/ep1.m3u8?token=1#第2集$https://cdn.example.com/ep2.m3u8",
        );
        assert_eq!(episodes.len(), 2);
        assert_eq!(episodes[0].title, "第1集");
        assert!(
            !episodes
                .iter()
                .any(|episode| episode.title.contains("广告"))
        );
    }
}
