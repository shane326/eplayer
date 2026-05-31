use anyhow::{Context, Result, anyhow};
use axum::Router;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use reqwest::{Client, Url};
use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::TcpListener;

#[derive(Clone)]
pub struct ProxyServer {
    base_url: String,
}

#[derive(Clone)]
struct ProxyState {
    client: Client,
    base_url: String,
}

#[derive(Debug, Deserialize)]
struct ProxyQuery {
    url: String,
}

impl ProxyServer {
    pub async fn start() -> Result<Self> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::limited(5))
            .user_agent(default_user_agent())
            .build()
            .context("build proxy client")?;

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .context("bind local proxy")?;
        let addr = listener.local_addr()?;
        let base_url = format!("http://{}", addr);

        let state = Arc::new(ProxyState {
            client,
            base_url: base_url.clone(),
        });

        let app = Router::new()
            .route("/proxy/m3u8", get(proxy_m3u8))
            .route("/proxy/segment", get(proxy_segment))
            .with_state(state);

        tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app).await {
                eprintln!("proxy server failed: {error}");
            }
        });

        Ok(Self { base_url })
    }

    pub fn proxied_m3u8_url(&self, original: &str) -> String {
        format!("{}/proxy/m3u8?url={}", self.base_url, url_encode(original))
    }
}

async fn proxy_m3u8(
    State(state): State<Arc<ProxyState>>,
    Query(query): Query<ProxyQuery>,
) -> Response {
    match fetch_m3u8(&state, &query.url).await {
        Ok(text) => {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/vnd.apple.mpegurl; charset=utf-8"),
            );
            headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            allow_webview_cors(&mut headers);
            (headers, text).into_response()
        }
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            format!("m3u8 proxy failed: {error}"),
        )
            .into_response(),
    }
}

async fn proxy_segment(
    State(state): State<Arc<ProxyState>>,
    request_headers: HeaderMap,
    Query(query): Query<ProxyQuery>,
) -> Response {
    match safe_url(&query.url) {
        Ok(url) => match state
            .client
            .get(url.clone())
            .headers({
                let mut headers = upstream_headers(&url);
                copy_request_header(&request_headers, &mut headers, header::RANGE);
                copy_request_header(&request_headers, &mut headers, header::IF_RANGE);
                headers
            })
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                let response_headers = response.headers().clone();
                let body = Body::from_stream(response.bytes_stream());
                let mut out = Response::new(body);
                *out.status_mut() = status;
                copy_response_header(
                    &response_headers,
                    out.headers_mut(),
                    header::CONTENT_TYPE,
                    Some(HeaderValue::from_static("application/octet-stream")),
                );
                copy_response_header(
                    &response_headers,
                    out.headers_mut(),
                    header::CONTENT_LENGTH,
                    None,
                );
                copy_response_header(
                    &response_headers,
                    out.headers_mut(),
                    header::CONTENT_RANGE,
                    None,
                );
                copy_response_header(
                    &response_headers,
                    out.headers_mut(),
                    header::ACCEPT_RANGES,
                    None,
                );
                copy_response_header(
                    &response_headers,
                    out.headers_mut(),
                    header::CACHE_CONTROL,
                    None,
                );
                allow_webview_cors(out.headers_mut());
                out
            }
            Err(error) => (
                StatusCode::BAD_GATEWAY,
                format!("segment proxy failed: {error}"),
            )
                .into_response(),
        },
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

fn allow_webview_cors(headers: &mut HeaderMap) {
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("range, if-range, content-type"),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("content-length, content-range, accept-ranges"),
    );
}

async fn fetch_m3u8(state: &ProxyState, original: &str) -> Result<String> {
    let url = safe_url(original)?;
    let response = state
        .client
        .get(url.clone())
        .headers(upstream_headers(&url))
        .send()
        .await?
        .error_for_status()?;
    let text = response.text().await?;
    let filtered = filter_ads_from_m3u8(&text);
    rewrite_m3u8(&filtered, &url, &state.base_url)
}

pub fn filter_ads_from_m3u8(content: &str) -> String {
    if content.trim().is_empty() {
        return String::new();
    }

    let ad_keywords = [
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
    ];
    let lines = content.lines().collect::<Vec<_>>();
    let mut filtered = Vec::new();
    let mut index = 0;
    let mut discontinuity_block_start = 0;

    while index < lines.len() {
        let line = lines[index];
        let lower = line.to_ascii_lowercase();
        if line.contains("#EXT-X-DISCONTINUITY") {
            discontinuity_block_start = filtered.len();
            index += 1;
            continue;
        }

        if ad_keywords.iter().any(|keyword| lower.contains(keyword)) {
            if discontinuity_block_start < filtered.len() {
                filtered.truncate(discontinuity_block_start);
            }
            index += 1;
            continue;
        }

        if line.contains("#EXTINF:") {
            let media_url_index = next_media_line_index(&lines, index + 1);
            if let Some(media_url_index) = media_url_index {
                let media_line = lines[media_url_index].to_ascii_lowercase();
                if ad_keywords
                    .iter()
                    .any(|keyword| media_line.contains(keyword))
                {
                    if discontinuity_block_start < filtered.len() {
                        filtered.truncate(discontinuity_block_start);
                    }
                    index = media_url_index + 1;
                    continue;
                }
            }
        }

        if !line.trim().starts_with('#') && looks_like_short_ad_name(line) {
            if discontinuity_block_start < filtered.len() {
                filtered.truncate(discontinuity_block_start);
            }
            index += 1;
            continue;
        }

        filtered.push(line.to_string());
        index += 1;
    }

    filtered.join("\n")
}

pub fn rewrite_m3u8(content: &str, base_url: &Url, proxy_base: &str) -> Result<String> {
    let mut output = Vec::new();
    let mut next_line_is_playlist = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#EXT-X-KEY:") {
            output.push(rewrite_key_line(trimmed, base_url, proxy_base)?);
            continue;
        }
        if trimmed.starts_with("#EXT-X-MAP:") {
            output.push(rewrite_uri_attribute_line(trimmed, base_url, proxy_base)?);
            continue;
        }

        if trimmed.starts_with('#') {
            if trimmed.starts_with("#EXT-X-STREAM-INF") {
                next_line_is_playlist = true;
            }
            output.push(line.to_string());
            continue;
        }

        if trimmed.is_empty() {
            output.push(line.to_string());
            continue;
        }

        let absolute = base_url.join(trimmed)?;
        let rewritten = if next_line_is_playlist || is_playlist_url(absolute.as_str()) {
            format!(
                "{proxy_base}/proxy/m3u8?url={}",
                url_encode(absolute.as_str())
            )
        } else {
            format!(
                "{proxy_base}/proxy/segment?url={}",
                url_encode(absolute.as_str())
            )
        };
        output.push(rewritten);
        next_line_is_playlist = false;
    }

    Ok(output.join("\n"))
}

fn rewrite_key_line(line: &str, base_url: &Url, proxy_base: &str) -> Result<String> {
    rewrite_uri_attribute_line(line, base_url, proxy_base)
}

fn rewrite_uri_attribute_line(line: &str, base_url: &Url, proxy_base: &str) -> Result<String> {
    let Some(start) = line.find("URI=\"") else {
        return Ok(line.to_string());
    };
    let uri_start = start + 5;
    let Some(relative_end) = line[uri_start..].find('"') else {
        return Ok(line.to_string());
    };
    let uri_end = uri_start + relative_end;
    let key_url = base_url.join(&line[uri_start..uri_end])?;
    let proxied = format!(
        "{proxy_base}/proxy/segment?url={}",
        url_encode(key_url.as_str())
    );
    Ok(format!(
        "{}{}{}",
        &line[..uri_start],
        proxied,
        &line[uri_end..]
    ))
}

fn next_media_line_index(lines: &[&str], start: usize) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                None
            } else {
                Some(index)
            }
        })
}

fn looks_like_short_ad_name(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower == "ad.ts"
        || lower == "ads.ts"
        || lower == "ad.m4s"
        || lower == "ads.m4s"
        || lower.ends_with("/ad.ts")
        || lower.ends_with("/ads.ts")
        || lower.ends_with("/ad.m4s")
        || lower.ends_with("/ads.m4s")
}

fn upstream_headers(url: &Url) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::USER_AGENT,
        HeaderValue::from_static(default_user_agent()),
    );
    if let Some(host) = url.host_str() {
        let referer = format!("{}://{host}/", url.scheme());
        if let Ok(value) = HeaderValue::from_str(&referer) {
            headers.insert(header::REFERER, value);
        }
    }
    headers
}

fn copy_request_header(source: &HeaderMap, target: &mut HeaderMap, name: header::HeaderName) {
    if let Some(value) = source.get(&name).cloned() {
        target.insert(name, value);
    }
}

fn copy_response_header(
    source: &HeaderMap,
    target: &mut HeaderMap,
    name: header::HeaderName,
    fallback: Option<HeaderValue>,
) {
    if let Some(value) = source.get(&name).cloned().or(fallback) {
        target.insert(name, value);
    }
}

fn safe_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).context("invalid url")?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(anyhow!("unsupported url scheme"));
    }
    if url.username() != "" || url.password().is_some() {
        return Err(anyhow!("url credentials are not allowed"));
    }
    if let Some(host) = url.host_str() {
        if host.eq_ignore_ascii_case("localhost") {
            return Err(anyhow!("localhost is not allowed"));
        }
        if let Ok(ip) = host.parse::<IpAddr>() {
            if is_private_ip(ip) {
                return Err(anyhow!("private network url is not allowed"));
            }
        }
    }
    Ok(url)
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.octets()[0] == 0
        }
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local(),
    }
}

fn is_playlist_url(value: &str) -> bool {
    value
        .split('?')
        .next()
        .map(|path| path.to_ascii_lowercase().ends_with(".m3u8"))
        .unwrap_or(false)
}

fn url_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn default_user_agent() -> &'static str {
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"
}

#[allow(dead_code)]
fn _addr_to_string(addr: SocketAddr) -> String {
    addr.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_relative_links() {
        let base = Url::parse("https://example.com/video/master/index.m3u8").unwrap();
        let input = "#EXTM3U\n#EXTINF:10,\nseg-1.ts\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.key\"";
        let out = rewrite_m3u8(input, &base, "http://127.0.0.1:1234").unwrap();
        assert!(
            out.contains(
                "/proxy/segment?url=https%3A%2F%2Fexample.com%2Fvideo%2Fmaster%2Fseg-1.ts"
            )
        );
        assert!(out.contains("URI=\"http://127.0.0.1:1234/proxy/segment?url=https%3A%2F%2Fexample.com%2Fvideo%2Fmaster%2Fkey.key\""));
    }

    #[test]
    fn filters_ad_segments_before_rewrite() {
        let input = "#EXTM3U\n#EXTINF:8,\nseg-1.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:5,\nhttps://cdn.example.com/ads/ad-1.ts\n#EXTINF:9,\nseg-2.ts";
        let out = filter_ads_from_m3u8(input);
        assert!(out.contains("seg-1.ts"));
        assert!(out.contains("seg-2.ts"));
        assert!(!out.contains("ad-1.ts"));
        assert!(!out.contains("#EXT-X-DISCONTINUITY"));
    }

    #[test]
    fn removes_discontinuity_ad_block_before_keyword_segment() {
        let input = "#EXTM3U\n#EXTINF:8,\nmain-1.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:3,\npre-ad.ts\n#EXTINF:5,\nhttps://cdn.example.com/ads/ad-1.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:9,\nmain-2.ts";
        let out = filter_ads_from_m3u8(input);
        assert!(out.contains("main-1.ts"));
        assert!(out.contains("main-2.ts"));
        assert!(!out.contains("pre-ad.ts"));
        assert!(!out.contains("ad-1.ts"));
        assert!(!out.contains("#EXT-X-DISCONTINUITY"));
    }

    #[test]
    fn filters_ad_segment_when_tag_is_between_extinf_and_url() {
        let input = "#EXTM3U\n#EXTINF:8,\nmain-1.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:6,\n#EXT-X-BYTERANGE:1000@0\nhttps://cdn.example.com/googleads/seg.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:8,\nmain-2.ts";
        let out = filter_ads_from_m3u8(input);
        assert!(out.contains("main-1.ts"));
        assert!(out.contains("main-2.ts"));
        assert!(!out.contains("googleads"));
        assert!(!out.contains("BYTERANGE"));
    }

    #[test]
    fn rewrites_ext_x_map_uri() {
        let base = Url::parse("https://example.com/video/master/index.m3u8").unwrap();
        let input = "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:10,\nseg-1.m4s";
        let out = rewrite_m3u8(input, &base, "http://127.0.0.1:1234").unwrap();
        assert!(out.contains("URI=\"http://127.0.0.1:1234/proxy/segment?url=https%3A%2F%2Fexample.com%2Fvideo%2Fmaster%2Finit.mp4\""));
        assert!(
            out.contains(
                "/proxy/segment?url=https%3A%2F%2Fexample.com%2Fvideo%2Fmaster%2Fseg-1.m4s"
            )
        );
    }
}
