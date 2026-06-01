#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cms;
mod config;
mod model;
mod paths;
mod proxy;
mod storage;

use anyhow::{Context, Result};
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use model::{PlayHistory, SearchResult, SkipConfig, Source};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::sync::Arc;
use tao::dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{CursorIcon, Fullscreen, Icon, ResizeDirection, Window, WindowBuilder};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use wry::{WebViewBuilder, http::Request};

#[derive(Debug)]
enum UserEvent {
    Minimize,
    Maximize,
    ToggleFullscreen,
    ToggleCompact,
    DragWindow,
    CloseWindow,
    SetTitle(String),
    MouseDown(i32, i32),
    MouseMove(i32, i32),
}

#[derive(Debug)]
enum HitTestResult {
    Client,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    NoWhere,
}

impl HitTestResult {
    fn drag_resize_window(&self, window: &Window) {
        let direction = match self {
            HitTestResult::Left => ResizeDirection::West,
            HitTestResult::Right => ResizeDirection::East,
            HitTestResult::Top => ResizeDirection::North,
            HitTestResult::Bottom => ResizeDirection::South,
            HitTestResult::TopLeft => ResizeDirection::NorthWest,
            HitTestResult::TopRight => ResizeDirection::NorthEast,
            HitTestResult::BottomLeft => ResizeDirection::SouthWest,
            HitTestResult::BottomRight => ResizeDirection::SouthEast,
            _ => return,
        };
        let _ = window.drag_resize_window(direction);
    }

    fn change_cursor(&self, window: &Window) {
        let cursor = match self {
            HitTestResult::Left => CursorIcon::WResize,
            HitTestResult::Right => CursorIcon::EResize,
            HitTestResult::Top => CursorIcon::NResize,
            HitTestResult::Bottom => CursorIcon::SResize,
            HitTestResult::TopLeft => CursorIcon::NwResize,
            HitTestResult::TopRight => CursorIcon::NeResize,
            HitTestResult::BottomLeft => CursorIcon::SwResize,
            HitTestResult::BottomRight => CursorIcon::SeResize,
            _ => CursorIcon::Default,
        };
        window.set_cursor_icon(cursor);
    }
}

#[derive(Clone)]
struct AppState {
    cms: cms::CmsClient,
    storage: storage::Storage,
    proxy: Option<proxy::ProxyServer>,
    config_path: std::path::PathBuf,
    cache_time: u64,
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: String,
}

#[derive(Debug, Deserialize)]
struct LibraryQuery {
    source: Option<String>,
    category: Option<String>,
    page: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct DetailQuery {
    source: String,
    id: String,
}

#[derive(Debug, Deserialize)]
struct PlayUrlQuery {
    url: String,
}

#[derive(Debug, Deserialize)]
struct HistoryLookupQuery {
    source: String,
    id: String,
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SaveHistoryRequest {
    source: String,
    video_id: String,
    episode_index: i64,
    progress_sec: i64,
    duration_sec: i64,
    title: String,
    episode_title: String,
    poster: String,
}

#[derive(Debug, Deserialize)]
struct ImportSourcesRequest {
    text: String,
}

#[derive(Debug, Deserialize)]
struct SaveDefaultSourceRequest {
    source: String,
}

#[derive(Debug, Deserialize)]
struct SaveSourcesRequest {
    sources: Vec<Source>,
    default_source: String,
}

#[derive(Debug, Deserialize)]
struct SkipQuery {
    source: String,
    id: String,
}

#[derive(Debug, Deserialize)]
struct SaveSkipRequest {
    source: String,
    video_id: String,
    intro_end_sec: i64,
    outro_offset_sec: i64,
    enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
struct AppBootstrap {
    sources: Vec<Source>,
    selected_source: String,
}

fn main() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("no available vod source")?;

    let config_path = config::default_config_path();
    let source_config = config::load_config(&config_path)?;
    let cms = cms::CmsClient::new()?;
    let storage = storage::Storage::open(&storage::default_db_path())?;
    let proxy = runtime.block_on(proxy::ProxyServer::start()).ok();
    let state = AppState {
        cms,
        storage,
        proxy,
        config_path,
        cache_time: source_config.cache_time,
    };
    let app_url = runtime.block_on(start_app_server(state))?;
    run_webview(&app_url)
}

async fn start_app_server(state: AppState) -> Result<String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let addr = listener.local_addr()?;
    let base_url = format!("http://{addr}");
    let state = Arc::new(state);
    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/styles.css", get(styles_css))
        .route("/artplayer.js", get(artplayer_js))
        .route("/hls.min.js", get(hls_js))
        .route("/flv.min.js", get(flv_js))
        .route("/api/bootstrap", get(api_bootstrap))
        .route("/api/sources/import", post(api_import_sources))
        .route("/api/sources/save", post(api_save_sources))
        .route(
            "/api/settings/default-source",
            post(api_save_default_source),
        )
        .route("/api/search", get(api_search))
        .route("/api/library", get(api_library))
        .route("/api/categories/{source}", get(api_categories))
        .route("/api/detail", get(api_detail))
        .route("/api/play-url", get(api_play_url))
        .route("/api/history", get(api_history).post(api_save_history))
        .route("/api/history/lookup", get(api_history_lookup))
        .route("/api/history/clear", post(api_clear_history))
        .route("/api/skip", get(api_get_skip).post(api_save_skip))
        .layer(CorsLayer::permissive())
        .with_state(state);

    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            eprintln!("ePlayer server failed: {error}");
        }
    });
    Ok(base_url)
}

fn run_webview(app_url: &str) -> Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title("ePlayer")
        .with_inner_size(LogicalSize::new(1280.0, 820.0))
        .with_min_inner_size(LogicalSize::new(960.0, 640.0))
        .with_window_icon(Some(app_icon()?))
        .with_decorations(false)
        .with_resizable(true)
        .build(&event_loop)
        .context("no available vod source")?;

    let mut compact_mode = false;
    let mut compact_restore_size: Option<PhysicalSize<u32>> = None;
    let mut compact_restore_pos: Option<PhysicalPosition<i32>> = None;
    let mut restore_maximized_after_fullscreen = false;
    let proxy = event_loop.create_proxy();
    let handler = move |request: Request<String>| {
        let body = request.body();
        if let Some(message) = body.strip_prefix("client_log:") {
            eprintln!("webview log: {message}");
            return;
        }
        if let Some(message) = body.strip_prefix("client_error:") {
            eprintln!("webview error: {message}");
            return;
        }
        if let Some(title) = body.strip_prefix("set_title:") {
            let _ = proxy.send_event(UserEvent::SetTitle(title.trim().to_string()));
            return;
        }
        let mut parts = body.split([':', ',']);
        match parts.next().unwrap_or_default() {
            "minimize" => {
                let _ = proxy.send_event(UserEvent::Minimize);
            }
            "maximize" => {
                let _ = proxy.send_event(UserEvent::Maximize);
            }
            "toggle_fullscreen" => {
                let _ = proxy.send_event(UserEvent::ToggleFullscreen);
            }
            "toggle_compact" => {
                let _ = proxy.send_event(UserEvent::ToggleCompact);
            }
            "drag_window" => {
                let _ = proxy.send_event(UserEvent::DragWindow);
            }
            "close" => {
                let _ = proxy.send_event(UserEvent::CloseWindow);
            }
            "mousedown" => {
                if let (Some(x), Some(y)) = (parts.next(), parts.next()) {
                    if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
                        let _ = proxy.send_event(UserEvent::MouseDown(x, y));
                    }
                }
            }
            "mousemove" => {
                if let (Some(x), Some(y)) = (parts.next(), parts.next()) {
                    if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
                        let _ = proxy.send_event(UserEvent::MouseMove(x, y));
                    }
                }
            }
            _ => {}
        }
    };

    let webview = WebViewBuilder::new()
        .with_url(app_url)
        .with_ipc_handler(handler)
        .with_accept_first_mouse(true)
        .build(&window)
        .context("no available vod source")?;

    let mut webview = Some(webview);
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {}
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(UserEvent::CloseWindow) => {
                let _ = webview.take();
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(UserEvent::SetTitle(title)) => {
                if title.trim().is_empty() {
                    window.set_title("ePlayer");
                } else {
                    window.set_title(&title);
                }
            }
            Event::UserEvent(UserEvent::Minimize) => window.set_minimized(true),
            Event::UserEvent(UserEvent::Maximize) => window.set_maximized(!window.is_maximized()),
            Event::UserEvent(UserEvent::ToggleFullscreen) => {
                let fullscreen = window.fullscreen().is_none();
                if fullscreen {
                    restore_maximized_after_fullscreen = window.is_maximized();
                    if window.is_maximized() {
                        window.set_maximized(false);
                    }
                    if compact_mode {
                        compact_mode = false;
                        window.set_always_on_top(false);
                        if let Some(size) = compact_restore_size.take() {
                            window.set_inner_size(size);
                        }
                        if let Some(pos) = compact_restore_pos.take() {
                            window.set_outer_position(pos);
                        }
                        window.set_min_inner_size(Some(LogicalSize::new(960.0, 640.0)));
                    }
                    window.set_fullscreen(Some(Fullscreen::Borderless(None)));
                } else {
                    window.set_fullscreen(None);
                    if restore_maximized_after_fullscreen {
                        window.set_maximized(true);
                    }
                    restore_maximized_after_fullscreen = false;
                }
            }
            Event::UserEvent(UserEvent::ToggleCompact) => {
                if compact_mode {
                    compact_mode = false;
                    window.set_always_on_top(false);
                    window.set_min_inner_size(Some(LogicalSize::new(960.0, 640.0)));
                    if let Some(size) = compact_restore_size.take() {
                        window.set_inner_size(size);
                    }
                    if let Some(pos) = compact_restore_pos.take() {
                        window.set_outer_position(pos);
                    }
                } else {
                    compact_mode = true;
                    if window.fullscreen().is_some() {
                        window.set_fullscreen(None);
                    }
                    compact_restore_size = Some(window.inner_size());
                    compact_restore_pos = window.outer_position().ok();
                    window.set_min_inner_size(Some(LogicalSize::new(360.0, 220.0)));
                    window.set_inner_size(LogicalSize::new(430.0, 300.0));
                    window.set_always_on_top(true);
                    if let Some(monitor) = window.current_monitor() {
                        let position = monitor.position();
                        let size = monitor.size();
                        let scale = window.scale_factor();
                        let x = (position.x as f64 / scale) + (size.width as f64 / scale) - 454.0;
                        let y =
                            (position.y as f64 / scale) + (size.height as f64 / scale) - 348.0;
                        window.set_outer_position(LogicalPosition::new(x.max(8.0), y.max(8.0)));
                    }
                }
            }
            Event::UserEvent(UserEvent::DragWindow) => {
                let _ = window.drag_window();
            }
            Event::UserEvent(UserEvent::MouseDown(x, y)) => {
                hit_test(window.inner_size(), x, y, window.scale_factor())
                    .drag_resize_window(&window);
            }
            Event::UserEvent(UserEvent::MouseMove(x, y)) => {
                hit_test(window.inner_size(), x, y, window.scale_factor()).change_cursor(&window);
            }
            _ => {}
        }
    });
}

fn app_icon() -> Result<Icon> {
    const SIZE: u32 = 64;
    let mut rgba = vec![0_u8; (SIZE * SIZE * 4) as usize];
    let center = (SIZE as f32 - 1.0) / 2.0;
    let radius = 28.0_f32;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let distance = (dx * dx + dy * dy).sqrt();
            let index = ((y * SIZE + x) * 4) as usize;

            if distance <= radius {
                rgba[index] = 18;
                rgba[index + 1] = 24;
                rgba[index + 2] = 36;
                rgba[index + 3] = 255;
            }

            if is_play_triangle(x as f32, y as f32) {
                rgba[index] = 57;
                rgba[index + 1] = 226;
                rgba[index + 2] = 188;
                rgba[index + 3] = 255;
            }
        }
    }

    Icon::from_rgba(rgba, SIZE, SIZE).context("no available vod source")
}

fn is_play_triangle(x: f32, y: f32) -> bool {
    let ax = 26.0;
    let ay = 20.0;
    let bx = 26.0;
    let by = 44.0;
    let cx = 46.0;
    let cy = 32.0;

    let area = edge(ax, ay, bx, by, cx, cy).abs();
    let a = edge(x, y, bx, by, cx, cy).abs();
    let b = edge(ax, ay, x, y, cx, cy).abs();
    let c = edge(ax, ay, bx, by, x, y).abs();
    (a + b + c - area).abs() <= 0.8
}

fn edge(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
    (cx - ax) * (by - ay) - (cy - ay) * (bx - ax)
}

fn hit_test(window_size: PhysicalSize<u32>, x: i32, y: i32, scale: f64) -> HitTestResult {
    const BORDERLESS_RESIZE_INSET: f64 = 5.0;

    const CLIENT: isize = 0b0000;
    const LEFT: isize = 0b0001;
    const RIGHT: isize = 0b0010;
    const TOP: isize = 0b0100;
    const BOTTOM: isize = 0b1000;
    const TOPLEFT: isize = TOP | LEFT;
    const TOPRIGHT: isize = TOP | RIGHT;
    const BOTTOMLEFT: isize = BOTTOM | LEFT;
    const BOTTOMRIGHT: isize = BOTTOM | RIGHT;

    let bottom = window_size.height as i32;
    let right = window_size.width as i32;
    let inset = (BORDERLESS_RESIZE_INSET * scale) as i32;

    #[rustfmt::skip]
    let result =
          (LEFT * (if x < inset { 1 } else { 0 }))
        | (RIGHT * (if x >= right - inset { 1 } else { 0 }))
        | (TOP * (if y < inset { 1 } else { 0 }))
        | (BOTTOM * (if y >= bottom - inset { 1 } else { 0 }));

    match result {
        CLIENT => HitTestResult::Client,
        LEFT => HitTestResult::Left,
        RIGHT => HitTestResult::Right,
        TOP => HitTestResult::Top,
        BOTTOM => HitTestResult::Bottom,
        TOPLEFT => HitTestResult::TopLeft,
        TOPRIGHT => HitTestResult::TopRight,
        BOTTOMLEFT => HitTestResult::BottomLeft,
        BOTTOMRIGHT => HitTestResult::BottomRight,
        _ => HitTestResult::NoWhere,
    }
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../assets/index.html"))
}

async fn app_js() -> Response {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "no-store, no-cache, must-revalidate"),
        ],
        include_str!("../assets/app.js"),
    )
        .into_response()
}

async fn styles_css() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store, no-cache, must-revalidate"),
        ],
        include_str!("../assets/styles.css"),
    )
        .into_response()
}

async fn artplayer_js() -> Response {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../assets/artplayer.js"),
    )
        .into_response()
}

async fn hls_js() -> Response {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../assets/hls.min.js"),
    )
        .into_response()
}

async fn flv_js() -> Response {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../assets/flv.min.js"),
    )
        .into_response()
}

async fn api_bootstrap(State(state): State<Arc<AppState>>) -> Result<Json<AppBootstrap>, AppError> {
    let sources = config::load_sources(&state.config_path)?;
    let selected_source = state
        .storage
        .get_setting("default_source")?
        .filter(|key| sources.iter().any(|source| source.key == *key))
        .or_else(|| sources.first().map(|source| source.key.clone()))
        .unwrap_or_default();
    Ok(Json(AppBootstrap {
        sources,
        selected_source,
    }))
}

async fn api_import_sources(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ImportSourcesRequest>,
) -> Result<StatusCode, AppError> {
    if let Some(parent) = state.config_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(&state.config_path, payload.text)
        .with_context(|| format!("write {}", state.config_path.display()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn api_save_sources(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SaveSourcesRequest>,
) -> Result<Json<AppBootstrap>, AppError> {
    let sources = normalize_sources(payload.sources);
    let default_source = normalize_source_key(&payload.default_source);
    validate_sources(&sources, &default_source)?;
    config::save_sources(&state.config_path, state.cache_time, &sources)?;
    let sources = config::load_sources(&state.config_path)?;
    state
        .storage
        .save_setting("default_source", &default_source)?;
    Ok(Json(AppBootstrap {
        sources,
        selected_source: default_source,
    }))
}

async fn api_save_default_source(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SaveDefaultSourceRequest>,
) -> Result<StatusCode, AppError> {
    state
        .storage
        .save_setting("default_source", payload.source.trim())?;
    Ok(StatusCode::NO_CONTENT)
}

async fn api_search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<SearchResult>>, AppError> {
    let sources = config::load_sources(&state.config_path)?;
    let results = state.cms.search_all(&sources, query.q.trim()).await;
    Ok(Json(results))
}

async fn api_library(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LibraryQuery>,
) -> Result<Json<model::LibraryPage>, AppError> {
    let sources = config::load_sources(&state.config_path)?;
    let source = find_source(&sources, query.source.as_deref())?;
    let page = state
        .cms
        .videos(
            &source,
            query.category.as_deref().unwrap_or(""),
            query.page.unwrap_or(1),
        )
        .await?;
    Ok(Json(page))
}

async fn api_categories(
    State(state): State<Arc<AppState>>,
    Path(source_key): Path<String>,
) -> Result<Json<Vec<model::Category>>, AppError> {
    let sources = config::load_sources(&state.config_path)?;
    let source = find_source(&sources, Some(&source_key))?;
    Ok(Json(state.cms.categories(&source).await?))
}

async fn api_detail(
    State(state): State<Arc<AppState>>,
    Query(query): Query<DetailQuery>,
) -> Result<Json<SearchResult>, AppError> {
    let sources = config::load_sources(&state.config_path)?;
    let source = find_source(&sources, Some(&query.source))?;
    Ok(Json(state.cms.detail(&source, &query.id).await?))
}

async fn api_play_url(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PlayUrlQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let lower = query.url.to_ascii_lowercase();
    let url = if lower.contains(".m3u8") || lower.contains("/m3u8") {
        state
            .proxy
            .as_ref()
            .map(|proxy| proxy.proxied_m3u8_url(&query.url))
            .unwrap_or(query.url)
    } else {
        query.url
    };
    Ok(Json(serde_json::json!({ "url": url })))
}

async fn api_history(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<PlayHistory>>, AppError> {
    Ok(Json(state.storage.list_history(30)?))
}

async fn api_history_lookup(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HistoryLookupQuery>,
) -> Result<Json<Option<PlayHistory>>, AppError> {
    let history = state
        .storage
        .get_history(&query.source, &query.id)?
        .or_else(|| {
            query
                .title
                .as_deref()
                .and_then(|title| state.storage.find_history_by_title(title).ok().flatten())
        })
        .filter(|history| history.progress_sec > 2 || history.episode_index > 0);
    Ok(Json(history))
}

async fn api_save_history(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SaveHistoryRequest>,
) -> Result<StatusCode, AppError> {
    state.storage.save_history(&PlayHistory {
        source: payload.source,
        video_id: payload.video_id,
        episode_index: payload.episode_index,
        progress_sec: payload.progress_sec,
        duration_sec: payload.duration_sec,
        title: payload.title,
        episode_title: payload.episode_title,
        poster: payload.poster,
        updated_at: storage::now_ts(),
    })?;
    Ok(StatusCode::NO_CONTENT)
}

async fn api_clear_history(State(state): State<Arc<AppState>>) -> Result<StatusCode, AppError> {
    state.storage.clear_history()?;
    Ok(StatusCode::NO_CONTENT)
}

async fn api_get_skip(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SkipQuery>,
) -> Result<Json<Option<SkipConfig>>, AppError> {
    Ok(Json(
        state.storage.get_skip_config(&query.source, &query.id)?,
    ))
}

async fn api_save_skip(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SaveSkipRequest>,
) -> Result<StatusCode, AppError> {
    state.storage.save_skip_config(&SkipConfig {
        source: payload.source,
        video_id: payload.video_id,
        intro_end_sec: payload.intro_end_sec.max(0),
        outro_offset_sec: payload.outro_offset_sec.max(0),
        enabled: payload.enabled.unwrap_or(true),
        updated_at: storage::now_ts(),
    })?;
    Ok(StatusCode::NO_CONTENT)
}

fn find_source(sources: &[Source], key: Option<&str>) -> Result<Source> {
    if let Some(key) = key.filter(|key| !key.is_empty()) {
        if let Some(source) = sources.iter().find(|source| source.key == key) {
            return Ok(source.clone());
        }
    }
    sources
        .iter()
        .find(|source| source.enabled)
        .cloned()
        .context("no available vod source")
}

fn normalize_source_key(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        .collect()
}

fn normalize_sources(sources: Vec<Source>) -> Vec<Source> {
    sources
        .into_iter()
        .map(|source| Source {
            key: normalize_source_key(&source.key),
            name: source.name.trim().to_string(),
            api: source.api.trim().trim_end_matches('/').to_string(),
            detail: source
                .detail
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            enabled: source.enabled,
        })
        .filter(|source| {
            !source.key.is_empty() && !source.name.is_empty() && !source.api.is_empty()
        })
        .collect()
}

fn validate_sources(sources: &[Source], default_source: &str) -> Result<(), AppError> {
    if sources.is_empty() {
        return Err(AppError(anyhow::anyhow!(
            "at least one vod source is required"
        )));
    }
    let default_source = default_source.trim();
    let mut keys = std::collections::HashSet::new();
    for source in sources {
        if !keys.insert(source.key.as_str()) {
            return Err(AppError(anyhow::anyhow!(
                "duplicate vod source key: {}", source.key
            )));
        }
    }
    if !sources.iter().any(|source| source.key == default_source) {
        return Err(AppError(anyhow::anyhow!(
            "please select a valid default vod source"
        )));
    }
    Ok(())
}

#[derive(Debug)]
struct AppError(anyhow::Error);

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(error: E) -> Self {
        Self(error.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": self.0.to_string() })),
        )
            .into_response()
    }
}
