(function () {
const state = {
  sources: [],
  source: "",
  categories: [],
  parentCategory: "",
  category: "",
  page: 1,
  pageCount: 1,
  libraryLoading: false,
  current: null,
  episodeIndex: 0,
  vodHls: null,
  vodFlv: null,
  vodArt: null,
  skip: { intro: 0, outro: 0, enabled: true },
  pendingStart: 0,
  playSideOpen: true,
  settingsSection: "vod",
  settingsSources: [],
  settingsSource: "",
  settingsSelectedSource: "",
  historyOpen: false,
  playTab: "episodes",
  sourceCandidates: [],
  sourceSearchTitle: "",
  sourceSpeeds: {},
  sourceSpeedTesting: new Set(),
  sourceSpeedQueueId: 0,
  playerSettingsOpen: false,
  volumeOpen: false,
  audioBalance: "off",
  videoEnhance: "off",
  playbackRate: 1,
  fillVod: false,
  compactMode: false,
  fullscreenMode: false,
  compactRestoreView: "home",
  compactRestorePlaySide: true,
  fullscreenRestorePlaySide: true,
  hideControlsTimer: null,
  playbackToggleBusy: false,
};

const $ = (id) => document.getElementById(id);

window.addEventListener("error", (event) => {
  const message = event.message || "脚本错误";
  console.error(message, event.error);
  ipc(`client_error:${message}`);
  const target = $("settingsMsg");
  if (target) {
    target.textContent = message;
    target.classList.add("error-text");
  }
});

window.addEventListener("unhandledrejection", (event) => {
  const message = (event.reason && event.reason.message) || String(event.reason || "异步错误");
  console.error(message, event.reason);
  ipc(`client_error:${message}`);
  const target = $("settingsMsg");
  if (target) {
    target.textContent = message;
    target.classList.add("error-text");
  }
});

const video = $("video");

function on(id, eventName, handler) {
  const element = $(id);
  if (element) element.addEventListener(eventName, handler);
}

function ipc(message) {
  if (window.ipc && window.ipc.postMessage) {
    window.ipc.postMessage(message);
  }
}

function clientLog(label, data = {}) {
  try {
    ipc(`client_log:${label} ${JSON.stringify(data)}`);
  } catch {
    ipc(`client_log:${label}`);
  }
}

function describeElement(element) {
  if (!element) return "";
  const id = element.id ? `#${element.id}` : "";
  const classes = typeof element.className === "string" && element.className.trim()
    ? `.${element.className.trim().replace(/\s+/g, ".")}`
    : "";
  return `${element.tagName ? element.tagName.toLowerCase() : "node"}${id}${classes}`;
}

function videoDebugState(targetVideo, centerStateId) {
  if (!targetVideo) return {};
  const rect = targetVideo.getBoundingClientRect();
  const style = getComputedStyle(targetVideo);
  const cx = rect.left + rect.width / 2;
  const cy = rect.top + rect.height / 2;
  const topElement = rect.width > 0 && rect.height > 0 ? document.elementFromPoint(cx, cy) : null;
  const center = $(centerStateId);
  const centerStyle = center ? getComputedStyle(center) : null;
  return {
    videoWidth: targetVideo.videoWidth,
    videoHeight: targetVideo.videoHeight,
    readyState: targetVideo.readyState,
    networkState: targetVideo.networkState,
    paused: targetVideo.paused,
    currentTime: Number(targetVideo.currentTime || 0).toFixed(2),
    src: targetVideo.currentSrc || targetVideo.src || "",
    rect: {
      left: Math.round(rect.left),
      top: Math.round(rect.top),
      width: Math.round(rect.width),
      height: Math.round(rect.height),
    },
    style: {
      display: style.display,
      visibility: style.visibility,
      opacity: style.opacity,
      position: style.position,
      zIndex: style.zIndex,
      objectFit: style.objectFit,
    },
    topElement: describeElement(topElement),
    centerState: center
      ? {
          hidden: center.classList.contains("hidden"),
          classes: center.className,
          text: (center.textContent || "").trim(),
          display: centerStyle ? centerStyle.display : "",
        }
      : null,
  };
}

async function api(path, options) {
  const response = await fetch(path, options);
  if (!response.ok) {
    const text = await response.text();
    try {
      throw new Error(JSON.parse(text).error || text);
    } catch {
      throw new Error(text || `${response.status} ${response.statusText}`);
    }
  }
  if (response.status === 204) return null;
  return response.json();
}

function postJson(path, body) {
  return api(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
}

function openModal(id) {
  const element = $(id);
  if (element) element.classList.remove("hidden");
}

function closeModal(id) {
  const element = $(id);
  if (element) element.classList.add("hidden");
}

window.openSettingsDialog = () => openSettings();

function cloneJson(value) {
  return JSON.parse(JSON.stringify(value || []));
}

function uniqueSourceKey(list, base) {
  let index = 1;
  let key = base;
  const used = new Set(list.map((item) => item.key));
  while (used.has(key)) {
    index += 1;
    key = `${base}${index}`;
  }
  return key;
}

function setSettingsMessage(text, isError = false) {
  const target = $("settingsMsg");
  if (!target) return;
  target.textContent = text || "";
  target.classList.toggle("error-text", Boolean(isError));
}

function setSettingsSection(section) {
  state.settingsSection = section;
  document.querySelectorAll(".settings-tab").forEach((button) => {
    button.classList.toggle("active", button.dataset.settingsTab === section);
  });
  document.querySelectorAll(".settings-panel").forEach((panel) => panel.classList.remove("active"));
  const panel = $(`${section}Settings`);
  if (panel) panel.classList.add("active");
  $("saveSettingsBtn").disabled = section === "about";
  setSettingsMessage("");
}

async function openSettings() {
  openModal("settingsDialog");
  try {
    if (!state.sources.length) {
      const data = await api("/api/bootstrap").catch(() => null);
      if (data) {
        state.sources = data.sources || [];
        state.source = data.selected_source || firstKey(state.sources);
      }
    }
    state.settingsSources = cloneJson(state.sources);
    state.settingsSource = state.source || firstKey(state.settingsSources);
    state.settingsSelectedSource = firstKey(state.settingsSources);
    $("sourceImport").value = "";
    renderSettingsSources();
    setSettingsSection("vod");
    setSettingsMessage("");
  } catch (error) {
    console.error("open settings failed", error);
    setSettingsMessage(error.message || "设置初始化失败", true);
  }
}

window.openSettings = openSettings;

function closeSettings() {
  closeModal("settingsDialog");
}

function setHistoryPopup(open) {
  state.historyOpen = Boolean(open);
  const popup = $("historyPopup");
  if (popup) popup.classList.toggle("hidden", !state.historyOpen);
}

function firstKey(list) {
  return list && list.length ? list[0].key || "" : "";
}

function firstEnabledKey(list) {
  const item = (list || []).find((source) => source.enabled);
  return item ? item.key || "" : "";
}

function startTitleDrag(event) {
  if (event.button !== 0) return;
  if (event.target.closest("button, input, select, textarea, a")) return;
  if (event.target.closest("#historyPopup, .modal")) return;
  event.preventDefault();
  if (event.detail === 2) ipc("maximize");
  else ipc("drag_window");
}

function renderSettingsSourceOptions() {
  const sourceSelect = $("settingsSourceSelect");
  if (sourceSelect) {
    sourceSelect.innerHTML = state.settingsSources
      .map((source) => `<option value="${escapeAttr(source.key)}">${escapeHtml(source.name)}</option>`)
      .join("");
    sourceSelect.value = state.settingsSource;
  }
}

function renderSettingsSources() {
  renderSettingsSourceOptions();
  const list = $("sourceList");
  if (!list) return;
  list.innerHTML = state.settingsSources
    .map((source) => settingsSourceRow(source, source.key === state.settingsSelectedSource))
    .join("") || `<div class="settings-empty">暂无点播源，点击上方新增资源。</div>`;
}

function settingsSourceRow(source, expanded) {
  return `
    <article class="settings-source-item ${expanded ? "expanded" : ""}" data-settings-source="${escapeAttr(source.key)}">
      <button class="settings-source-head" type="button" data-select-settings-source="${escapeAttr(source.key)}">
        <span class="settings-chevron">${expanded ? "⌄" : "›"}</span>
        <strong>${escapeHtml(source.name || "未命名资源")}</strong>
        <span>${escapeHtml(source.key)}</span>
        ${state.settingsSource === source.key ? `<em>默认</em>` : ""}
      </button>
      ${expanded ? `
        <div class="settings-source-editor">
          ${settingsInput("资源标识", "key", source.key, "例如 dyttzy")}
          ${settingsInput("显示名称", "name", source.name, "例如 电影天堂资源")}
          ${settingsInput("采集接口", "api", source.api, "https://example.com/api.php/provide/vod")}
          ${settingsInput("详情地址", "detail", source.detail || "", "可选，例如 https://example.com")}
          ${settingsCheck("启用", "enabled", source.enabled)}
          <div class="settings-editor-actions">
            <button type="button" class="danger" data-delete-settings-source="${escapeAttr(source.key)}">删除</button>
          </div>
        </div>` : ""}
    </article>
  `;
}

function settingsInput(label, field, value, placeholder) {
  return `
    <label class="settings-edit-row">
      <span>${escapeHtml(label)}</span>
      <input data-settings-field="${escapeAttr(field)}" value="${escapeAttr(value || "")}" placeholder="${escapeAttr(placeholder || "")}" ${field === "key" ? "data-key-field=\"true\"" : ""} />
    </label>
  `;
}

function settingsCheck(label, field, checked) {
  return `
    <label class="settings-edit-row settings-check-row">
      <span>${escapeHtml(label)}</span>
      <input type="checkbox" data-settings-field="${escapeAttr(field)}" ${checked !== false ? "checked" : ""} />
    </label>
  `;
}

function updateSettingsSourceField(key, field, value) {
  const source = state.settingsSources.find((item) => item.key === key);
  if (!source) return;
  const previousKey = source.key;
  if (field === "enabled") source.enabled = Boolean(value);
  else if (field === "detail") source.detail = value;
  else source[field] = value;
  if (field === "key") {
    state.settingsSelectedSource = value;
    if (state.settingsSource === previousKey) state.settingsSource = value;
    source.key = value;
  }
}

function addSettingsSource() {
  const key = uniqueSourceKey(state.settingsSources, "custom");
  state.settingsSources.unshift({ key, name: "自定义资源", api: "", detail: "", enabled: true });
  state.settingsSelectedSource = key;
  if (!state.settingsSource) state.settingsSource = key;
  renderSettingsSources();
}

function deleteSettingsSource(key) {
  if (state.settingsSources.length <= 1) {
    setSettingsMessage("至少保留一个点播源", true);
    return;
  }
  state.settingsSources = state.settingsSources.filter((source) => source.key !== key);
  if (state.settingsSource === key) state.settingsSource = firstKey(state.settingsSources);
  state.settingsSelectedSource = firstKey(state.settingsSources);
  renderSettingsSources();
}

function mergeByKey(existing, imported) {
  const next = [...existing];
  for (const item of imported) {
    const index = next.findIndex((current) => current.key === item.key);
    if (index >= 0) next[index] = item;
    else next.push(item);
  }
  return next;
}

async function importSettingsSources() {
  try {
    const text = $("sourceImport").value;
    if (!text.trim()) throw new Error("没有可导入的点播源");
    await postJson("/api/sources/import", { text });
    const data = await api("/api/bootstrap");
    state.sources = data.sources || [];
    state.source = data.selected_source || firstEnabledKey(state.sources) || firstKey(state.sources);
    state.settingsSources = cloneJson(state.sources);
    state.settingsSource = state.source || firstKey(state.settingsSources);
    state.settingsSelectedSource = firstKey(state.settingsSources);
    renderSources();
    renderSettingsSources();
    await loadCategories();
    await loadLibrary(true);
    closeModal("sourceImportDialog");
    setSettingsMessage("已覆盖写入 sources.json");
  } catch (error) {
    setSettingsMessage(error.message || "导入点播源失败", true);
  }
}
async function saveSettings() {
  try {
    if (state.settingsSection === "vod") {
      const data = await postJson("/api/sources/save", {
        sources: state.settingsSources,
        default_source: state.settingsSource,
      });
      state.sources = data.sources || [];
      state.source = data.selected_source || state.source;
      renderSources();
      await loadCategories();
      await loadLibrary(true);
    }
    closeSettings();
  } catch (error) {
    setSettingsMessage(error.message || "保存失败", true);
  }
}

function setView(name) {
  const appBody = document.querySelector(".app-body");
  if (appBody) appBody.classList.toggle("media-mode", name === "player");
  document.querySelectorAll(".view").forEach((view) => view.classList.remove("active"));
  document.querySelectorAll(".nav").forEach((button) => button.classList.remove("active"));
  document.querySelectorAll(".side-nav").forEach((button) => button.classList.remove("active"));
  const view = $(`${name}View`);
  const topNav = document.querySelector(`.nav[data-view="${name}"]`);
  const sideNav = name === "search"
    ? document.querySelector('.side-nav[data-side-view="search"]')
    : null;
  if (view) view.classList.add("active");
  if (topNav) topNav.classList.add("active");
  if (sideNav) sideNav.classList.add("active");
}

function formatTime(value) {
  if (!Number.isFinite(value) || value < 0) return "00:00";
  const total = Math.floor(value);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return h > 0
    ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
    : `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

function updateSeekVisual() {
  const seek = $("seek");
  if (!seek) return;
  const value = Number(seek.value) || 0;
  const max = Number(seek.max) || 1000;
  const progress = Math.max(0, Math.min(100, (value / max) * 100));
  seek.style.setProperty("--seek-progress", `${progress}%`);
}

function showControls() {
  document.body.classList.remove("controls-hidden");
  document.querySelectorAll(".media-controls").forEach((controls) => controls.classList.add("force-visible"));
  clearTimeout(state.hideControlsTimer);
  state.hideControlsTimer = setTimeout(() => {
    if (!state.playerSettingsOpen && !state.volumeOpen) {
      document.body.classList.add("controls-hidden");
      document.querySelectorAll(".media-controls").forEach((controls) => controls.classList.remove("force-visible"));
    }
  }, 3000);
}

function hideControlsNow() {
  if (state.playerSettingsOpen || state.volumeOpen) return;
  document.body.classList.add("controls-hidden");
  document.querySelectorAll(".media-controls").forEach((controls) => controls.classList.remove("force-visible"));
}

function setMenuOpenClasses() {
  const vodOpen = state.playerSettingsOpen || state.volumeOpen;
  const vodControls = document.querySelector("#playerView .media-controls");
  if (vodControls) vodControls.classList.toggle("menu-open", vodOpen);
}

function setPopup(id, open) {
  const popup = $(id);
  if (popup) popup.classList.toggle("hidden", !open);
  setMenuOpenClasses();
  if (open) showControls();
}

function closePlayerPopups(except = "") {
  if (except !== "vod-settings") state.playerSettingsOpen = false;
  if (except !== "vod-volume") state.volumeOpen = false;
  setPopup("playerSettingsPopup", state.playerSettingsOpen);
  setPopup("volumePopup", state.volumeOpen);
}

function setSettingsPage(kind, page = "main") {
  const popup = $("playerSettingsPopup");
  if (!popup) return;
  popup.querySelectorAll(".settings-page").forEach((panel) => {
    panel.classList.toggle("active", panel.dataset.settingsPage === page);
  });
}

function speedText(speed) {
  const value = Number(speed) || 1;
  return Math.abs(value - 1) < 0.01 ? "正常" : `${value.toFixed(value % 1 ? 2 : 1).replace(/\.0$/, "")}x`;
}

function updatePlayerSettingLabels() {
  const speedLabel = $("speedLabel");
  if (speedLabel) speedLabel.textContent = speedText(state.playbackRate || 1);
  const audio = state.audioBalance === "off" ? "关闭" : state.audioBalance === "standard" ? "标准" : "强";
  const enhance = state.videoEnhance === "off" ? "关闭" : state.videoEnhance === "standard" ? "标准" : "清晰";
  if ($("audioBalanceLabel")) $("audioBalanceLabel").textContent = audio;
  if ($("videoEnhanceLabel")) $("videoEnhanceLabel").textContent = enhance;
  if ($("fillVideoLabel")) $("fillVideoLabel").textContent = state.fillVod ? "填充" : "原始";
  if ($("compactLabel")) $("compactLabel").textContent = state.compactMode ? "退出" : "开启";
  if ($("fullscreenLabel")) $("fullscreenLabel").textContent = state.fullscreenMode ? "退出" : "开启";

  document.querySelectorAll("[data-player-speed]").forEach((button) => {
    button.classList.toggle("active", Math.abs(Number(button.dataset.playerSpeed) - (state.playbackRate || 1)) < 0.02);
  });
  document.querySelectorAll("#playerSettingsPopup [data-audio-balance]").forEach((button) => {
    button.classList.toggle("active", button.dataset.audioBalance === state.audioBalance);
  });
  document.querySelectorAll("#playerSettingsPopup [data-video-enhance]").forEach((button) => {
    button.classList.toggle("active", button.dataset.videoEnhance === state.videoEnhance);
  });
  document.body.classList.toggle("fill-vod", state.fillVod);
  document.body.classList.toggle("app-compact", state.compactMode);
  document.body.classList.toggle("app-fullscreen", state.fullscreenMode);
  document.body.classList.toggle("video-enhance-standard", state.videoEnhance === "standard");
  document.body.classList.toggle("video-enhance-clear", state.videoEnhance === "clear");
}

function setVolume(targetVideo, sliderId, valueId, value) {
  const next = Math.max(0, Math.min(1, Number(value)));
  if (targetVideo) targetVideo.volume = next;
  const slider = $(sliderId);
  if (slider) slider.value = String(next);
  const label = $(valueId);
  if (label) label.textContent = String(Math.round(next * 100));
}

function activeArt(kind) {
  return state.vodArt;
}

function activeVideoForKind(kind) {
  const art = activeArt(kind);
  return art && art.video ? art.video : video;
}

function setPlayerPlayState(kind, playing) {
  const button = $("playBtn");
  if (!button) return;
  button.classList.toggle("is-playing", playing);
  button.dataset.state = playing ? "playing" : "paused";
}

function setAudioBalance(kind, mode) {
  state.audioBalance = mode;
  updatePlayerSettingLabels();
}

function setVideoEnhance(kind, mode) {
  state.videoEnhance = mode;
  updatePlayerSettingLabels();
}

function toggleFill(kind) {
  state.fillVod = !state.fillVod;
  updatePlayerSettingLabels();
}

function toggleCompactMode() {
  const entering = !state.compactMode;
  if (entering) {
    const activeView = document.querySelector(".view.active");
    state.compactRestoreView = activeView ? activeView.id.replace(/View$/, "") : "home";
    state.compactRestorePlaySide = state.playSideOpen;
    state.compactMode = true;
    if (state.fullscreenMode) state.fullscreenMode = false;
    setView("player");
    setDrawer("play", false);
  } else {
    state.compactMode = false;
    setView(state.compactRestoreView || "home");
    setDrawer("play", state.compactRestorePlaySide);
  }
  closePlayerPopups();
  ipc("toggle_compact");
  updatePlayerSettingLabels();
  showControls();
}

function toggleFullscreenMode() {
  const entering = !state.fullscreenMode;
  state.fullscreenMode = entering;
  if (entering) {
    state.fullscreenRestorePlaySide = state.playSideOpen;
    state.playSideOpen = false;
  }
  if (state.fullscreenMode && state.compactMode) {
    state.compactMode = false;
    setView(state.compactRestoreView || "player");
    setDrawer("play", state.compactRestorePlaySide);
  }
  setDrawer("play", entering ? false : state.fullscreenRestorePlaySide);
  closePlayerPopups();
  ipc("toggle_fullscreen");
  updatePlayerSettingLabels();
  if (state.fullscreenMode) showControls();
  else hideControlsNow();
}

function setCenterStateIcon(targetId, mode) {
  const target = $(targetId);
  if (!target) return;
  target.textContent = "";
  target.classList.toggle("icon-play", mode === "play");
  target.classList.toggle("icon-pause", mode === "pause");
}

function paintCenterState(targetId, mode) {
  const target = $(targetId);
  if (!target) return;
  setCenterStateIcon(targetId, mode);
  target.classList.remove("hidden");
  setTimeout(() => target.classList.add("hidden"), 700);
}

async function toggleVodPlayback(showCenter = true) {
  const targetVideo = activeVideoForKind("vod");
  if (!targetVideo || state.playbackToggleBusy) return;
  state.playbackToggleBusy = true;
  const willPlay = targetVideo.paused;
  try {
    if (willPlay) await targetVideo.play();
    else targetVideo.pause();
    if (showCenter) paintCenterState("centerState", willPlay ? "play" : "pause");
  } catch (error) {
    console.warn("toggle playback failed", error);
  } finally {
    setTimeout(() => {
      state.playbackToggleBusy = false;
    }, 160);
  }
}

function setStatus(target, text) {
  target.innerHTML = `<div class="empty-state">${escapeHtml(text)}</div>`;
}

async function bootstrap() {
  await bootstrapVod();
}

async function bootstrapVod() {
  const data = await api("/api/bootstrap");
  state.sources = data.sources || [];
  state.source = data.selected_source || firstEnabledKey(state.sources) || firstKey(state.sources);
  renderSources();
  if (!state.source) {
    renderCategories();
    setStatus($("cards"), "请先在设置中导入点播源。");
    setLibrarySentinel(false);
    return;
  }
  await loadCategories();
  await loadLibrary(true);
}

function renderSources() {
  const select = $("settingsSourceSelect");
  if (!select) return;
  select.innerHTML = state.sources
    .map((source) => `<option value="${escapeAttr(source.key)}">${escapeHtml(source.name)}</option>`)
    .join("");
  select.value = state.source;
}

async function loadCategories() {
  if (!state.source) return;
  state.categories = await api(`/api/categories/${encodeURIComponent(state.source)}`);
  const parent = visibleParentCategories()[0];
  state.parentCategory = parent ? parent.id || "" : "";
  const firstChild = childCategories(state.parentCategory)[0];
  state.category = (firstChild ? firstChild.id : "") || state.parentCategory || "";
  renderCategories();
}

function renderCategories() {
  if (!$("parentCategoryList") || !$("categoryList")) return;
  $("parentCategoryList").innerHTML = visibleParentCategories()
    .map((category) => {
      const active = category.id === state.parentCategory ? "active" : "";
      return `<button class="${active}" data-parent-category="${escapeAttr(category.id)}">${escapeHtml(category.name)}</button>`;
    })
    .join("");

  const children = childCategories(state.parentCategory);
  $("categoryList").innerHTML = children
    .map((category) => {
      const active = category.id === state.category ? "active" : "";
      return `<button class="${active}" data-category="${escapeAttr(category.id)}">${escapeHtml(category.name)}</button>`;
    })
    .join("");
}

function visibleParentCategories() {
  const parents = state.categories.filter((category) => category.parent_id === "0" && category.id);
  const orphanChildren = state.categories.filter((category) => {
    return (
      category.id &&
      category.parent_id !== "0" &&
      !parents.some((parent) => parent.id === category.parent_id)
    );
  });
  return [...parents, ...orphanChildren];
}

function childCategories(parentId) {
  if (!parentId) return [];
  const children = state.categories.filter((category) => category.parent_id === parentId);
  return children.length ? children : state.categories.filter((category) => category.id === parentId);
}

async function loadLibrary(reset) {
  if (!state.source) {
    setStatus($("cards"), "请先在设置中导入点播源。");
    setLibrarySentinel(false);
    return;
  }
  if (state.libraryLoading) return;
  if (reset) {
    state.page = 1;
    $("cards").innerHTML = "";
  }
  state.libraryLoading = true;
  setLibrarySentinel(true);
  try {
    const page = await api(
      `/api/library?source=${encodeURIComponent(state.source)}&category=${encodeURIComponent(state.category)}&page=${state.page}`,
    );
    state.page = page.page || state.page;
    state.pageCount = page.page_count || 1;
    const sourceItem = state.sources.find((source) => source.key === state.source);
    const sourceName = sourceItem ? sourceItem.name || "" : "";
    $("libraryInfo").textContent = `${sourceName} · ${page.total || 0} 条内容`;
    appendCards($("cards"), page.items || []);
    setLibrarySentinel(state.page < state.pageCount);
  } finally {
    state.libraryLoading = false;
    setLibrarySentinel(state.page < state.pageCount);
    setTimeout(ensureLibraryFilled, 0);
  }
}

function setLibrarySentinel(active) {
  const sentinel = $("librarySentinel");
  if (!sentinel) return;
  sentinel.classList.toggle("hidden", !active);
  sentinel.textContent = state.libraryLoading ? "加载中..." : "";
}

async function loadNextLibraryPage() {
  if (state.libraryLoading || state.page >= state.pageCount) return;
  state.page += 1;
  await loadLibrary(false);
}

function ensureLibraryFilled() {
  const content = document.querySelector("#homeView .content");
  if (!content || state.libraryLoading || state.page >= state.pageCount) return;
  if (content.scrollHeight <= content.clientHeight + 80) {
    loadNextLibraryPage().catch(console.error);
  }
}

function appendCards(container, items) {
  const html = items.map(cardHtml).join("");
  if (html) container.insertAdjacentHTML("beforeend", html);
  else if (!container.children.length) setStatus(container, "没有内容");
}

function cardHtml(item) {
  return `
    <article class="card" data-source="${escapeAttr(item.source)}" data-id="${escapeAttr(item.id)}">
      <div class="poster-wrap">
        <img class="poster" src="${escapeAttr(item.poster || "")}" loading="lazy" />
        ${item.remarks ? `<span class="remarks">${escapeHtml(item.remarks)}</span>` : ""}
      </div>
      <div class="card-title">${escapeHtml(item.title)}</div>
    </article>
  `;
}

async function openDetail(source, id, autoplay = true, episodeIndex = 0, start = 0, options = {}) {
  const detail = await api(`/api/detail?source=${encodeURIComponent(source)}&id=${encodeURIComponent(id)}`);
  state.current = detail;
  state.episodeIndex = clampEpisodeIndex(episodeIndex, detail);
  const history = options.skipHistoryLookup ? null : await lookupHistory(detail);
  if (history && autoplay && episodeIndex === 0 && start === 0) {
    state.episodeIndex = clampEpisodeIndex(history.episode_index, detail);
    start = Number(history.progress_sec) || 0;
  }
  await loadSkipConfig(detail);
  renderDetail();
  setView("player");
  setPlayTab("episodes");
  if (autoplay) await playEpisode(state.episodeIndex, start);
  loadSourceCandidates(detail).catch(console.error);
}

function clampEpisodeIndex(index, detail) {
  const count = detail && detail.episodes ? detail.episodes.length : 0;
  if (count <= 0) return 0;
  return Math.min(Math.max(Number(index) || 0, 0), count - 1);
}

async function lookupHistory(detail) {
  if (!detail) return null;
  const params = new URLSearchParams({
    source: detail.source,
    id: detail.id,
    title: detail.title || "",
  });
  return api(`/api/history/lookup?${params.toString()}`).catch(() => null);
}

async function loadSkipConfig(detail) {
  state.skip = { intro: 0, outro: 0, enabled: true };
  if (!detail) return;
  const params = new URLSearchParams({ source: detail.source, id: detail.id });
  const config = await api(`/api/skip?${params.toString()}`).catch(() => null);
  if (config) {
    state.skip = {
      intro: Number(config.intro_end_sec) || 0,
      outro: Number(config.outro_offset_sec) || 0,
      enabled: config.enabled !== false,
    };
  }
  $("introInput").value = state.skip.intro;
  $("outroInput").value = state.skip.outro;
  if ($("skipEnabledInput")) $("skipEnabledInput").checked = state.skip.enabled;
}

function renderDetail() {
  const item = state.current;
  if (!item) return;
  const episode = item.episodes && item.episodes[state.episodeIndex];
  $("episodeHeader").innerHTML = `
    <h3>${escapeHtml(item.title || "")}</h3>
    <p>${episode ? `正在播放：${escapeHtml(episode.title || `第${state.episodeIndex + 1}集`)}` : ""}</p>
    <span>共 ${(item.episodes || []).length} 集</span>
  `;
  $("detailBox").innerHTML = `
    <div class="detail-head">
      <img src="${escapeAttr(item.poster || "")}" />
      <div>
        <h3>${escapeHtml(item.title)}</h3>
        <p>${escapeHtml([item.year, item.category, item.area].filter(Boolean).join(" / ") || "暂无分类信息")}</p>
        <p>主演：${escapeHtml(item.actor || "未知")}</p>
        <p>导演：${escapeHtml(item.director || "未知")}</p>
      </div>
    </div>
    <div class="detail-desc">${escapeHtml(item.description || "暂无简介")}</div>
  `;
  $("episodeList").innerHTML = (item.episodes || [])
    .map((episode, index) => {
      const active = index === state.episodeIndex ? "active" : "";
      return `<button class="${active}" data-episode="${index}" title="${escapeAttr(episode.title)}">${escapeHtml(episode.title || `第${index + 1}集`)}</button>`;
    })
    .join("");
}

function setPlayTab(tab) {
  state.playTab = tab;
  document.querySelectorAll("[data-play-tab]").forEach((button) => {
    button.classList.toggle("active", button.dataset.playTab === tab);
  });
  document.querySelectorAll(".play-side-panel").forEach((panel) => panel.classList.remove("active"));
  if (tab === "episodes" && $("episodesPanel")) $("episodesPanel").classList.add("active");
  if (tab === "sources" && $("sourcesPanel")) $("sourcesPanel").classList.add("active");
  if (tab === "intro" && $("introPanel")) $("introPanel").classList.add("active");
}

async function loadSourceCandidates(detail) {
  if (!detail || !detail.title) return;
  if (state.sourceSearchTitle === detail.title) {
    renderSourceCandidates();
    return;
  }
  state.sourceSearchTitle = detail.title;
  state.sourceCandidates = [];
  $("sourceCandidateStatus").textContent = "正在搜索可用来源...";
  $("sourceCandidateList").innerHTML = "";
  const results = await api(`/api/search?q=${encodeURIComponent(detail.title)}`);
  state.sourceCandidates = (results || []).filter((item) => item.id && item.source);
  renderSourceCandidates();
  retestUntestedSources();
}

function renderSourceCandidates() {
  if (!state.current) return;
  const current = state.current;
  const list = $("sourceCandidateList");
  const status = $("sourceCandidateStatus");
  const candidates = sourceCandidatesForCurrent();
  if (!candidates.length) {
    status.textContent = "没有找到其它可用来源。";
    list.innerHTML = "";
    return;
  }
  const testing = candidates.some((item) => state.sourceSpeedTesting.has(sourceCandidateKey(item)));
  status.innerHTML = `
    <div class="source-header-row">
      <span>当前：${escapeHtml(current.source_name || current.source)}</span>
      <button id="retestAllSourcesBtn" type="button">${testing ? "测速中..." : "全部重测"}</button>
    </div>
    <p>找到 ${candidates.length} 个来源</p>
  `;
  list.innerHTML = candidates
    .sort((a, b) => compareSourceCandidates(a, b, current))
    .map((item) => {
      const active = item.source === current.source && item.id === current.id ? "active" : "";
      const key = sourceCandidateKey(item);
      const speed = state.sourceSpeeds[key];
      const testing = state.sourceSpeedTesting.has(key);
      const matchLabel = sourceMatchLabel(current, item);
      const quality = speed && speed.quality && speed.quality !== "未知" ? speed.quality : "";
      const speedHtml = testing
        ? `<span class="source-speed testing">测速中...</span>`
        : speed
          ? `<span class="source-speed ${speed.has_error ? "error" : ""}">${escapeHtml(speed.load_speed)}</span><span>${escapeHtml(speed.latency_ms)}ms</span><span>${escapeHtml(speed.bitrate)}</span>`
          : `<span class="source-speed muted">等待测速</span>`;
      return `
        <article class="source-candidate ${active}" data-source-candidate-source="${escapeAttr(item.source)}" data-source-candidate-id="${escapeAttr(item.id)}">
          <img src="${escapeAttr(item.poster || current.poster || "")}" alt="" />
          <span class="source-card-main">
            <span class="source-card-title">${escapeHtml(item.title)}</span>
            <span class="source-badges">
              <em>${escapeHtml(item.source_name || item.source)}</em>
              ${matchLabel ? `<em>${escapeHtml(matchLabel)}</em>` : ""}
            </span>
            <span class="source-speed-row">${speedHtml}</span>
          </span>
          <span class="source-card-side">
            ${quality ? `<em class="quality">${escapeHtml(quality)}</em>` : ""}
            <span>${(item.episodes || []).length || (current.episodes || []).length || 0} 集</span>
            <button data-source-retest="${escapeAttr(key)}" type="button">${testing ? "测试中" : "重新测速"}</button>
          </span>
        </article>`;
    })
    .join("");
}

function sourceCandidateKey(item) {
  return `${item.source}:${item.id}`;
}

function sourceCandidatesForCurrent() {
  const current = state.current;
  if (!current) return [];
  const seen = new Set();
  return state.sourceCandidates.filter((item) => {
    if (!item || !item.id || !item.source) return false;
    if (normalizeTitle(item.title) !== normalizeTitle(current.title)) return false;
    const key = sourceCandidateKey(item);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function compareSourceCandidates(a, b, current) {
  const aCurrent = a.source === current.source && a.id === current.id;
  const bCurrent = b.source === current.source && b.id === current.id;
  if (aCurrent !== bCurrent) return aCurrent ? -1 : 1;
  const aSpeed = state.sourceSpeeds[sourceCandidateKey(a)];
  const bSpeed = state.sourceSpeeds[sourceCandidateKey(b)];
  const ak = aSpeed && !aSpeed.has_error ? aSpeed.kbps || 0 : 0;
  const bk = bSpeed && !bSpeed.has_error ? bSpeed.kbps || 0 : 0;
  if (ak !== bk) return bk - ak;
  return String(a.source_name || "").localeCompare(String(b.source_name || ""), "zh-Hans-CN");
}

function contentKind(item) {
  const text = `${item.category || ""} ${item.type_name || ""}`;
  if (/电影|片/.test(text) && !/连续|剧|动漫|综艺/.test(text)) return "电影";
  if (/连续|剧/.test(text)) return "剧集";
  if (/动漫|动画/.test(text)) return "动漫";
  if (/综艺/.test(text)) return "综艺";
  return "";
}

function sourceMatchLabel(current, candidate) {
  const currentKind = contentKind(current);
  const candidateKind = contentKind(candidate);
  if (currentKind && candidateKind && currentKind !== candidateKind) return `${candidateKind}不符`;
  return candidateKind || currentKind;
}

function retestUntestedSources() {
  const queueId = state.sourceSpeedQueueId;
  const items = sourceCandidatesForCurrent().filter((item) => {
    const key = sourceCandidateKey(item);
    return !state.sourceSpeeds[key] && !state.sourceSpeedTesting.has(key);
  });
  runSourceSpeedQueue(items, queueId).catch(console.error);
}

function retestAllSources() {
  const items = sourceCandidatesForCurrent();
  state.sourceSpeedQueueId += 1;
  for (const item of items) {
    const key = sourceCandidateKey(item);
    delete state.sourceSpeeds[key];
  }
  renderSourceCandidates();
  runSourceSpeedQueue(items, state.sourceSpeedQueueId).catch(console.error);
}

async function runSourceSpeedQueue(items, queueId) {
  if (!items.length) return;
  const batchSize = Math.max(1, Math.min(5, Math.ceil(items.length / 2)));
  for (let start = 0; start < items.length; start += batchSize) {
    if (queueId !== state.sourceSpeedQueueId) return;
    const batch = items.slice(start, start + batchSize).filter((item) => {
      const key = sourceCandidateKey(item);
      return !state.sourceSpeedTesting.has(key) && !state.sourceSpeeds[key];
    });
    if (!batch.length) continue;
    batch.forEach((item) => state.sourceSpeedTesting.add(sourceCandidateKey(item)));
    renderSourceCandidates();
    await Promise.all(
      batch.map(async (item) => {
        const key = sourceCandidateKey(item);
        try {
          const info = await measureSourceSpeed(item);
          if (queueId === state.sourceSpeedQueueId) state.sourceSpeeds[key] = info;
        } catch {
          if (queueId === state.sourceSpeedQueueId) state.sourceSpeeds[key] = failedSpeedInfo();
        } finally {
          state.sourceSpeedTesting.delete(key);
        }
      }),
    );
    renderSourceCandidates();
  }
}

async function startSourceSpeedTest(item) {
  const key = sourceCandidateKey(item);
  if (state.sourceSpeedTesting.has(key)) return;
  state.sourceSpeedTesting.add(key);
  renderSourceCandidates();
  try {
    state.sourceSpeeds[key] = await measureSourceSpeed(item);
  } catch {
    state.sourceSpeeds[key] = failedSpeedInfo();
  } finally {
    state.sourceSpeedTesting.delete(key);
    renderSourceCandidates();
  }
}

async function measureSourceSpeed(item) {
  let detail = item;
  if (!detail.episodes || !detail.episodes.length) {
    detail = await api(`/api/detail?source=${encodeURIComponent(item.source)}&id=${encodeURIComponent(item.id)}`);
  }
  const episodes = detail.episodes || [];
  const episode = episodes.length > 1 ? episodes[1] : episodes[0];
  if (!episode || !episode.url) return failedSpeedInfo();
  const play = await api(`/api/play-url?url=${encodeURIComponent(episode.url)}`);
  return measureMediaUrl(play.url);
}

async function measureMediaUrl(url) {
  const lower = String(url).toLowerCase();
  if (lower.includes(".m3u8") || lower.includes("/proxy/m3u8")) {
    try {
      return await measureHlsMediaUrl(url);
    } catch (error) {
      console.warn("hls metadata speed test failed, falling back to playlist probe", error);
      return measurePlaylistMediaUrl(url);
    }
  }

  const started = performance.now();
  const response = await fetch(url, { cache: "no-store", headers: { range: "bytes=0-524287" } });
  const bytes = await response.arrayBuffer();
  const elapsed = Math.max((performance.now() - started) / 1000, 0.001);
  const kbps = bytes.byteLength / 1024 / elapsed;
  return {
    quality: "未知",
    load_speed: formatSpeed(kbps),
    latency_ms: Math.round(performance.now() - started),
    bitrate: "未知",
    kbps,
    has_error: false,
  };
}

async function measurePlaylistMediaUrl(url) {
  const started = performance.now();
  const response = await fetch(url, { cache: "no-store" });
  const latency = Math.round(performance.now() - started);
  const text = await response.text();
  const target = parsePlaylistTarget(text, url);
  if (target && target.variant) {
    const nested = await fetch(target.url, { cache: "no-store" }).then((response) => response.text());
    const nestedTarget = parsePlaylistTarget(nested, target.url);
    if (nestedTarget) {
      return measureSegment(nestedTarget.url, latency, nestedTarget.duration, target.quality || nestedTarget.quality);
    }
  }
  if (target) return measureSegment(target.url, latency, target.duration, target.quality);
  return failedSpeedInfo();
}

async function measureHlsMediaUrl(url, timeoutMs = 6000) {
  await ensureHlsLoaded();
  if (!window.Hls || !window.Hls.isSupported || !window.Hls.isSupported()) {
    return measureNativeMetadataUrl(url, timeoutMs);
  }

  return new Promise((resolve, reject) => {
    const probeVideo = document.createElement("video");
    probeVideo.muted = true;
    probeVideo.preload = "metadata";
    probeVideo.playsInline = true;
    probeVideo.style.cssText = "position:absolute;width:1px;height:1px;opacity:0;pointer-events:none;left:-9999px;top:-9999px;";
    document.body.appendChild(probeVideo);

    const hls = new Hls();
    const pingStart = performance.now();
    let pingTime = 0;
    let fragmentStartTime = 0;
    let kbps = 0;
    let loadSpeed = "未知";
    let bitrate = "未知";
    let hasSpeedCalculated = false;
    let hasMetadataLoaded = false;
    let settled = false;

    fetch(url, { method: "HEAD", cache: "no-store", mode: "no-cors" })
      .then(() => {
        pingTime = performance.now() - pingStart;
      })
      .catch(() => {
        pingTime = performance.now() - pingStart;
      });

    const cleanup = () => {
      hls.destroy();
      probeVideo.remove();
    };

    const finish = () => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      const quality = qualityFromVideoWidth(probeVideo.videoWidth);
      cleanup();
      resolve({
        quality,
        load_speed: loadSpeed,
        latency_ms: Math.round(pingTime),
        bitrate,
        kbps,
        has_error: false,
      });
    };

    const fail = (error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      cleanup();
      reject(error);
    };

    const checkAndFinish = () => {
      if (hasMetadataLoaded && hasSpeedCalculated) finish();
    };

    const timeout = setTimeout(() => {
      if (hasMetadataLoaded || hasSpeedCalculated) finish();
      else fail(new Error("Timeout loading video metadata"));
    }, timeoutMs);

    probeVideo.onerror = () => fail(new Error("Failed to load video metadata"));
    probeVideo.onloadedmetadata = () => {
      hasMetadataLoaded = true;
      checkAndFinish();
    };

    hls.on(Hls.Events.FRAG_LOADING, () => {
      fragmentStartTime = performance.now();
    });

    hls.on(Hls.Events.FRAG_LOADED, (_, data) => {
      if (hasSpeedCalculated || fragmentStartTime <= 0 || !data || !data.payload) return;
      const loadTime = Math.max((performance.now() - fragmentStartTime) / 1000, 0.001);
      const size = data.payload.byteLength || 0;
      if (size <= 0) return;
      kbps = size / 1024 / loadTime;
      loadSpeed = formatSpeed(kbps);
      if (data.frag && data.frag.duration > 0) {
        bitrate = formatBitrate((size * 8) / data.frag.duration);
      }
      hasSpeedCalculated = true;
      checkAndFinish();
    });

    hls.config.xhrSetup = (xhr, requestUrl) => {
      const separator = requestUrl.includes("?") ? "&" : "?";
      xhr.open("GET", `${requestUrl}${separator}_t=${Date.now()}`, true);
    };

    hls.on(Hls.Events.ERROR, (_, data) => {
      if (!data || !data.fatal) return;
      const statusCode = data.response && (data.response.code || data.response.status);
      if (statusCode === 415 && url.includes("/proxy/m3u8")) {
        if (settled) return;
        settled = true;
        clearTimeout(timeout);
        cleanup();
        resolve({
          quality: "原生画质",
          load_speed: "直连",
          latency_ms: Math.round(pingTime || 10),
          bitrate: "未知",
          kbps: 0,
          has_error: false,
        });
        return;
      }
      fail(new Error(`HLS测速失败: ${data.type || "unknown"}`));
    });

    hls.loadSource(url);
    hls.attachMedia(probeVideo);
  });
}

function parsePlaylistTarget(text, baseUrl) {
  const lines = String(text || "").split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (line.startsWith("#EXT-X-STREAM-INF")) {
      const next = nextPlaylistUrl(lines, index + 1);
      if (next) {
        return {
          url: absoluteUrl(next, baseUrl),
          quality: qualityFromStreamInfo(line),
          duration: null,
          variant: true,
        };
      }
    }
  }
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (line.startsWith("#EXTINF")) {
      const next = nextPlaylistUrl(lines, index + 1);
      if (next) {
        return {
          url: absoluteUrl(next, baseUrl),
          quality: "未知",
          duration: durationFromExtinf(line),
          variant: false,
        };
      }
    }
  }
  const firstUrl = lines.find((line) => !line.startsWith("#"));
  return firstUrl ? { url: absoluteUrl(firstUrl, baseUrl), quality: "未知", duration: null, variant: false } : null;
}

function nextPlaylistUrl(lines, start) {
  for (let index = start; index < lines.length; index += 1) {
    if (!lines[index].startsWith("#")) return lines[index];
  }
  return "";
}

function absoluteUrl(value, baseUrl) {
  try {
    return new URL(value, baseUrl).toString();
  } catch {
    return value;
  }
}

function qualityFromStreamInfo(line) {
  const match = /RESOLUTION=\d+x(\d+)/i.exec(line);
  if (!match) return "未知";
  return qualityFromResolutionHeight(Number(match[1]));
}

function qualityFromResolutionHeight(height) {
  const value = Number(height) || 0;
  if (value >= 2160) return "4K";
  if (value >= 1440) return "2K";
  if (value >= 1080) return "1080p";
  if (value >= 720) return "720p";
  if (value >= 480) return "480p";
  if (value > 0) return "SD";
  return "未知";
}

function durationFromExtinf(line) {
  const match = /#EXTINF:([\d.]+)/i.exec(line);
  return match ? Number(match[1]) : null;
}

async function measureSegment(url, latency, duration, quality) {
  const started = performance.now();
  const response = await fetch(url, { cache: "no-store" });
  const bytes = await response.arrayBuffer();
  const elapsed = Math.max((performance.now() - started) / 1000, 0.001);
  const kbps = bytes.byteLength / 1024 / elapsed;
  return {
    quality: quality || "未知",
    load_speed: formatSpeed(kbps),
    latency_ms: latency,
    bitrate: duration && duration > 0 ? formatBitrate((bytes.byteLength * 8) / duration) : "未知",
    kbps,
    has_error: false,
  };
}

function measureNativeMetadataUrl(url, timeoutMs) {
  return new Promise((resolve, reject) => {
    const probeVideo = document.createElement("video");
    probeVideo.muted = true;
    probeVideo.preload = "metadata";
    probeVideo.playsInline = true;
    probeVideo.style.cssText = "position:absolute;width:1px;height:1px;opacity:0;pointer-events:none;left:-9999px;top:-9999px;";
    document.body.appendChild(probeVideo);
    const started = performance.now();
    let settled = false;

    const cleanup = () => probeVideo.remove();
    const finish = () => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      const latency = Math.round(performance.now() - started);
      cleanup();
      resolve({
        quality: qualityFromVideoWidth(probeVideo.videoWidth),
        load_speed: "直连",
        latency_ms: latency,
        bitrate: "未知",
        kbps: 0,
        has_error: false,
      });
    };
    const fail = () => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      cleanup();
      reject(new Error("Failed to load video metadata"));
    };
    const timeout = setTimeout(fail, timeoutMs);
    probeVideo.onloadedmetadata = finish;
    probeVideo.onerror = fail;
    probeVideo.src = url;
  });
}

function qualityFromVideoWidth(width) {
  const value = Number(width) || 0;
  if (value >= 3840) return "4K";
  if (value >= 2560) return "2K";
  if (value >= 1920) return "1080p";
  if (value >= 1280) return "720p";
  if (value >= 854) return "480p";
  if (value > 0) return "SD";
  return "未知";
}

function formatSpeed(kbps) {
  if (!Number.isFinite(kbps) || kbps <= 0) return "未知";
  return kbps >= 1024 ? `${(kbps / 1024).toFixed(1)} MB/s` : `${Math.round(kbps)} KB/s`;
}

function formatBitrate(bitsPerSecond) {
  if (!Number.isFinite(bitsPerSecond) || bitsPerSecond <= 0) return "未知";
  return bitsPerSecond >= 1000000 ? `${(bitsPerSecond / 1000000).toFixed(1)} Mbps` : `${Math.round(bitsPerSecond / 1000)} Kbps`;
}

function failedSpeedInfo() {
  return { quality: "未知", load_speed: "测速失败", latency_ms: 0, bitrate: "未知", kbps: 0, has_error: true };
}

function normalizeTitle(value) {
  return String(value || "")
    .replace(/[\s()[\]{}（）【】《》<>「」『』]/g, "")
    .toLowerCase();
}

async function playEpisode(index, start = 0) {
  const item = state.current;
  if (!item || !item.episodes || !item.episodes[index]) return;
  saveHistorySoon();
  state.episodeIndex = index;
  state.pendingStart = Number(start) || 0;
  renderDetail();
  $("loadingState").classList.remove("hidden");
  const episode = item.episodes[index];
  updateTitleText(`${item.title} ${episode.title || `第${index + 1}集`}`.trim());
  const { url } = await api(`/api/play-url?url=${encodeURIComponent(episode.url)}`);
  await loadIntoVideo({
    video,
    url,
    hlsKey: "vodHls",
    start: state.pendingStart,
    autoplay: true,
    onReady: () => $("loadingState").classList.add("hidden"),
  });
}

async function loadIntoVideo({ video, url, hlsKey, start = 0, autoplay = true, onReady = null, forceHls = false }) {
  await loadArtPlayer({
    kind: "vod",
    url,
    mediaType: forceHls ? "m3u8" : mediaTypeFromUrl(url),
    start,
    autoplay,
    onReady,
  });
}

function ensureHlsLoaded() {
  if (window.Hls) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const script = document.createElement("script");
    script.src = "/hls.min.js";
    script.onload = () => resolve();
    script.onerror = () => reject(new Error("hls.js 加载失败"));
    document.head.appendChild(script);
  });
}

function ensureArtPlayerLoaded() {
  if (window.Artplayer) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const script = document.createElement("script");
    script.src = "/artplayer.js";
    script.onload = () => resolve();
    script.onerror = () => reject(new Error("ArtPlayer 加载失败"));
    document.head.appendChild(script);
  });
}

function ensureFlvLoaded() {
  if (window.flvjs) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const script = document.createElement("script");
    script.src = "/flv.min.js";
    script.onload = () => resolve();
    script.onerror = () => reject(new Error("flv.js 加载失败"));
    document.head.appendChild(script);
  });
}

function mediaTypeFromUrl(url) {
  const lower = String(url || "").toLowerCase();
  if (lower.includes(".mp4")) return "mp4";
  if (lower.includes(".m3u8") || lower.includes(".m3u") || lower.includes("/proxy/m3u8")) return "m3u8";
  return "m3u8";
}

async function loadArtPlayer({ kind, url, mediaType = "m3u8", start = 0, autoplay = true, onReady = null }) {
  await ensureArtPlayerLoaded();
  if (mediaType === "m3u8") await ensureHlsLoaded();
  destroyArtPlayer(kind);
  const container = $("vodArtContainer");
  if (!container) return;
  const art = new Artplayer({
    container,
    url,
    type: mediaType,
    customType: {
      m3u8: artHlsLoader(kind),
    },
    autoplay,
    muted: false,
    volume: activeVideoForKind(kind).volume || 1,
    isLive: false,
    autoSize: false,
    autoMini: false,
    fullscreen: false,
    fullscreenWeb: false,
    pip: false,
    screenshot: false,
    setting: false,
    playbackRate: false,
    aspectRatio: false,
    hotkey: false,
    miniProgressBar: false,
    mutex: true,
    playsInline: true,
    theme: "#22c55e",
    lang: "zh-cn",
    moreVideoAttr: {
      playsInline: true,
      preload: "metadata",
    },
  });
  state.vodArt = art;
  bindArtEvents(kind, art, start, onReady);
}

function artHlsLoader(kind) {
  return (targetVideo, url) => {
    if (!window.Hls || !window.Hls.isSupported()) {
      targetVideo.src = url;
      return;
    }
    destroyHls("vodHls");
    const hls = new Hls({
      enableWorker: true,
      lowLatencyMode: true,
      maxBufferLength: 60,
      backBufferLength: 60,
      maxBufferSize: 60 * 1000 * 1000,
    });
    state.vodHls = hls;
    hls.loadSource(url);
    hls.attachMedia(targetVideo);
    hls.on(Hls.Events.ERROR, (_, data) => {
      if (data && data.fatal) {
        if (data.type === Hls.ErrorTypes.NETWORK_ERROR) hls.startLoad();
        else if (data.type === Hls.ErrorTypes.MEDIA_ERROR) hls.recoverMediaError();
        else destroyHls("vodHls");
      }
    });
  };
}

function artFlvLoader(kind) {
  return (targetVideo, url) => {
    if (!window.flvjs || !window.flvjs.isSupported()) {
      console.warn("当前环境不支持 FLV 播放。");
      return;
    }
    destroyFlv(kind);
    const flv = flvjs.createPlayer({ type: "flv", url, isLive: false });
    flv.attachMediaElement(targetVideo);
    flv.load();
    state.vodFlv = flv;
  };
}

function bindArtEvents(kind, art, start, onReady) {
  const targetVideo = art.video;
  targetVideo.volume = Number($("volumeSlider")?.value || 1);
  targetVideo.playbackRate = state.playbackRate || 1;
  targetVideo.addEventListener("loadedmetadata", () => applyStart(targetVideo, start), { once: true });
  targetVideo.addEventListener("play", () => {
    setPlayerPlayState(kind, true);
    $("centerState").classList.add("hidden");
    showControls();
  });
  targetVideo.addEventListener("pause", () => {
    setPlayerPlayState(kind, false);
    $("centerState").classList.add("hidden");
  });
  art.on("ready", () => {
    applyStart(targetVideo, start);
    if (onReady) onReady();
  });
  art.on("play", () => {
    setPlayerPlayState(kind, true);
    $("centerState").classList.add("hidden");
    showControls();
  });
  art.on("pause", () => {
    setPlayerPlayState(kind, false);
    $("centerState").classList.add("hidden");
  });
  art.on("video:waiting", () => {
    $("loadingState").classList.remove("hidden");
  });
  art.on("video:playing", () => {
    $("loadingState").classList.add("hidden");
  });
  art.on("video:timeupdate", () => {
    updateVodProgressFromVideo(targetVideo);
  });
}

function updateVodProgressFromVideo(targetVideo) {
  $("timeText").textContent = `${formatTime(targetVideo.currentTime)} / ${formatTime(targetVideo.duration)}`;
  if (Number.isFinite(targetVideo.duration) && targetVideo.duration > 0) {
    $("seek").value = String(Math.floor((targetVideo.currentTime / targetVideo.duration) * 1000));
    updateSeekVisual();
    if (state.skip.enabled && state.skip.intro > 0 && targetVideo.currentTime > 0 && targetVideo.currentTime < state.skip.intro) {
      targetVideo.currentTime = state.skip.intro;
    }
    if (state.skip.enabled && state.skip.outro > 0 && targetVideo.duration - targetVideo.currentTime <= state.skip.outro) {
      playEpisode(state.episodeIndex + 1);
    }
  }
}

function applyStart(targetVideo, start) {
  if (Number(start) > 0 && Number.isFinite(targetVideo.duration)) {
    targetVideo.currentTime = Math.min(Number(start), Math.max(0, targetVideo.duration - 3));
  } else if (Number(start) > 0) {
    const once = () => {
      targetVideo.currentTime = Number(start);
      targetVideo.removeEventListener("durationchange", once);
    };
    targetVideo.addEventListener("durationchange", once);
  }
}

function destroyHls(hlsKey) {
  if (state[hlsKey]) {
    state[hlsKey].destroy();
    state[hlsKey] = null;
  }
}

function destroyFlv(kind) {
  const key = "vodFlv";
  if (state[key]) {
    try {
      if (state[key].unload) state[key].unload();
      state[key].destroy();
    } catch (error) {
      console.warn("destroy flv failed", error);
    }
    state[key] = null;
  }
}

function destroyArtPlayer(kind) {
  const artKey = "vodArt";
  destroyHls("vodHls");
  destroyFlv(kind);
  if (state[artKey]) {
    try {
      state[artKey].destroy(false);
    } catch (error) {
      console.warn("destroy artplayer failed", error);
    }
    state[artKey] = null;
  }
  setPlayerPlayState(kind, false);
}

function resetVideo(targetVideo, hlsKey) {
  destroyArtPlayer("vod");
  if (targetVideo) {
    targetVideo.dataset.resetting = "true";
    targetVideo.pause();
    targetVideo.removeAttribute("src");
    targetVideo.load();
    setTimeout(() => {
      delete targetVideo.dataset.resetting;
    }, 0);
  }
}

function stopVodPlayback() {
  if (video) resetVideo(video, "vodHls");
  const loading = $("loadingState");
  if (loading) loading.classList.add("hidden");
}

function saveHistorySoon() {
  const item = state.current;
  if (!item || !item.episodes || !item.episodes[state.episodeIndex]) return;
  const targetVideo = activeVideoForKind("vod");
  const progress = Math.floor(targetVideo.currentTime || 0);
  const duration = Math.floor(Number.isFinite(targetVideo.duration) ? targetVideo.duration : 0);
  if (progress < 2 && state.episodeIndex === 0) return;
  postJson("/api/history", {
    source: item.source,
    video_id: item.id,
    episode_index: state.episodeIndex,
    progress_sec: progress,
    duration_sec: duration,
    title: item.title,
    episode_title: item.episodes[state.episodeIndex].title,
    poster: item.poster || "",
  }).catch(console.error);
}

async function renderHistory() {
  const rows = await api("/api/history");
  $("historyList").innerHTML = (rows || [])
    .map(
      (row) => `
      <button class="history-item" type="button" data-source="${escapeAttr(row.source)}" data-id="${escapeAttr(row.video_id)}" data-episode="${row.episode_index}" data-progress="${row.progress_sec}">
        <img src="${escapeAttr(row.poster || "")}" />
        <div>
          <strong>${escapeHtml(row.title)}</strong>
          <p class="muted">${escapeHtml(row.episode_title || "")} · ${formatTime(row.progress_sec)} / ${formatTime(row.duration_sec)}</p>
        </div>
      </button>`,
    )
    .join("") || `<p class="muted">暂无历史</p>`;
}

async function openHistoryItem(element) {
  if (!element) return;
  const source = element.dataset.source || "";
  const id = element.dataset.id || "";
  if (!source || !id) return;
  setHistoryPopup(false);
  await openDetail(
    source,
    id,
    true,
    Number(element.dataset.episode),
    Number(element.dataset.progress),
    { skipHistoryLookup: true },
  );
}

async function search() {
  const q = $("searchInput").value.trim();
  if (!q) return;
  setStatus($("searchResults"), "搜索中...");
  const results = await api(`/api/search?q=${encodeURIComponent(q)}`);
  $("searchResults").innerHTML = "";
  appendCards($("searchResults"), results || []);
}

function updateTitleText(text = "") {
  const title = text || "ePlayer";
  $("titleText").textContent = title;
  document.title = title;
  ipc(`set_title:${title}`);
}

function setDrawer(name, open) {
  const side = $("playSide");
  const button = $("playSideToggle");
  const view = $("playerView");
  side.classList.toggle("open", open);
  button.classList.toggle("open", open);
  if (view) view.classList.toggle("drawer-open", open);
  button.textContent = open ? "›" : "‹";
  state.playSideOpen = open;
}

function escapeHtml(value) {
  return String(value == null ? "" : value).replace(/[&<>"']/g, (ch) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#39;",
  })[ch]);
}

function escapeAttr(value) {
  return escapeHtml(value).replace(/`/g, "&#96;");
}

document.addEventListener("click", async (event) => {
  const actionButton = event.target.closest("[data-settings-action]");
  if (actionButton) {
    const actions = {
      addSource: addSettingsSource,
      openImport: () => openModal("sourceImportDialog"),
      save: saveSettings,
      cancel: closeSettings,
      close: closeSettings,
      importSource: importSettingsSources,
      closeSourceImport: () => closeModal("sourceImportDialog"),
    };
    const action = actions[actionButton.dataset.settingsAction];
    if (action) {
      event.preventDefault();
      event.stopPropagation();
      await action();
      return;
    }
  }

  const closeModalTarget = event.target.closest("[data-close-modal]");
  if (closeModalTarget) {
    event.preventDefault();
    event.stopPropagation();
    closeModal(`${closeModalTarget.dataset.closeModal}Dialog`);
    return;
  }

  const nav = event.target.closest(".nav");
  if (nav) {
    setView(nav.dataset.view);
    return;
  }

  const sideNav = event.target.closest("[data-side-view]");
  if (sideNav) {
    setView(sideNav.dataset.sideView);
    return;
  }

  const settingsTab = event.target.closest(".settings-tab");
  if (settingsTab) {
    setSettingsSection(settingsTab.dataset.settingsTab);
    return;
  }

  const sourceHead = event.target.closest("[data-select-settings-source]");
  if (sourceHead) {
    const key = sourceHead.dataset.selectSettingsSource;
    state.settingsSelectedSource = state.settingsSelectedSource === key ? "" : key;
    renderSettingsSources();
    return;
  }
  const deleteSource = event.target.closest("[data-delete-settings-source]");
  if (deleteSource) {
    event.preventDefault();
    event.stopPropagation();
    deleteSettingsSource(deleteSource.dataset.deleteSettingsSource);
    return;
  }
  const parentCategory = event.target.closest("[data-parent-category]");
  if (parentCategory) {
    state.parentCategory = parentCategory.dataset.parentCategory;
    const firstChild = childCategories(state.parentCategory)[0];
    state.category = (firstChild ? firstChild.id : "") || state.parentCategory;
    renderCategories();
    setView("home");
    await loadLibrary(true);
    return;
  }

  const category = event.target.closest("[data-category]");
  if (category) {
    state.category = category.dataset.category;
    renderCategories();
    await loadLibrary(true);
    return;
  }

  const card = event.target.closest(".card");
  if (card) {
    await openDetail(card.dataset.source, card.dataset.id);
    return;
  }

  const episode = event.target.closest("[data-episode]");
  if (episode) {
    await playEpisode(Number(episode.dataset.episode));
    return;
  }

  const playTab = event.target.closest("[data-play-tab]");
  if (playTab) {
    setPlayTab(playTab.dataset.playTab);
    return;
  }

  const retestAll = event.target.closest("#retestAllSourcesBtn");
  if (retestAll) {
    event.preventDefault();
    event.stopPropagation();
    retestAllSources();
    return;
  }

  const settingsPage = event.target.closest("[data-player-settings-page]");
  if (settingsPage) {
    event.preventDefault();
    event.stopPropagation();
    const kind = settingsPage.closest("[data-settings-kind]")?.dataset.settingsKind || "vod";
    setSettingsPage(kind, settingsPage.dataset.playerSettingsPage);
    showControls();
    return;
  }

  const settingsBack = event.target.closest("[data-player-settings-back]");
  if (settingsBack) {
    event.preventDefault();
    event.stopPropagation();
    const kind = settingsBack.closest("[data-settings-kind]")?.dataset.settingsKind || "vod";
    setSettingsPage(kind, "main");
    showControls();
    return;
  }

  const speedChoice = event.target.closest("[data-player-speed]");
  if (speedChoice) {
    event.preventDefault();
    event.stopPropagation();
    state.playbackRate = Number(speedChoice.dataset.playerSpeed) || 1;
    activeVideoForKind("vod").playbackRate = state.playbackRate;
    updatePlayerSettingLabels();
    showControls();
    return;
  }

  const audioChoice = event.target.closest("[data-audio-balance]");
  if (audioChoice) {
    event.preventDefault();
    event.stopPropagation();
    const kind = audioChoice.closest("[data-settings-kind]")?.dataset.settingsKind || "vod";
    setAudioBalance(kind, audioChoice.dataset.audioBalance);
    showControls();
    return;
  }

  const enhanceChoice = event.target.closest("[data-video-enhance]");
  if (enhanceChoice) {
    event.preventDefault();
    event.stopPropagation();
    const kind = enhanceChoice.closest("[data-settings-kind]")?.dataset.settingsKind || "vod";
    setVideoEnhance(kind, enhanceChoice.dataset.videoEnhance);
    showControls();
    return;
  }

  const playerAction = event.target.closest("[data-player-action]");
  if (playerAction) {
    event.preventDefault();
    event.stopPropagation();
    const action = playerAction.dataset.playerAction;
    if (action === "vod-fill") toggleFill("vod");
    if (action === "compact") toggleCompactMode();
    if (action === "fullscreen") toggleFullscreenMode();
    showControls();
    return;
  }

  const retestOne = event.target.closest("[data-source-retest]");
  if (retestOne) {
    event.preventDefault();
    event.stopPropagation();
    const item = state.sourceCandidates.find((candidate) => sourceCandidateKey(candidate) === retestOne.dataset.sourceRetest);
    if (item) {
      delete state.sourceSpeeds[retestOne.dataset.sourceRetest];
      startSourceSpeedTest(item);
    }
    return;
  }

  const candidate = event.target.closest("[data-source-candidate-source]");
  if (candidate) {
    const keepIndex = state.episodeIndex;
    const keepTime = Math.floor(activeVideoForKind("vod").currentTime || 0);
    await openDetail(
      candidate.dataset.sourceCandidateSource,
      candidate.dataset.sourceCandidateId,
      true,
      keepIndex,
      keepTime,
    );
    state.playSideOpen = true;
    setDrawer("play", true);
    setPlayTab("episodes");
    return;
  }

  const history = event.target.closest(".history-item");
  if (history) {
    event.preventDefault();
    event.stopPropagation();
    await openHistoryItem(history);
    return;
  }
  if (state.historyOpen && !event.target.closest("#historyPopup") && !event.target.closest("#historyBtn")) {
    setHistoryPopup(false);
  }
});

document.addEventListener("change", (event) => {
  const field = event.target.closest("[data-settings-field]");
  if (!field) return;
  event.stopPropagation();
  const sourceItem = field.closest("[data-settings-source]");
  if (sourceItem) {
    const key = field.dataset.keyField ? state.settingsSelectedSource : sourceItem.dataset.settingsSource;
    updateSettingsSourceField(
      key,
      field.dataset.settingsField,
      field.type === "checkbox" ? field.checked : field.value,
    );
    renderSettingsSources();
    return;
  }
});

on("settingsSourceSelect", "change", (event) => {
  state.settingsSource = event.target.value;
  renderSettingsSources();
});

on("searchBtn", "click", search);
on("searchInput", "keydown", (event) => {
  if (event.key === "Enter") search();
});

on("sideSettingsBtn", "click", openSettings);on("minimizeBtn", "click", () => ipc("minimize"));
on("maximizeBtn", "click", () => ipc("maximize"));
on("closeBtn", "click", () => ipc("close"));
const titlebar = document.querySelector(".titlebar");
if (titlebar) titlebar.addEventListener("mousedown", startTitleDrag);
on("playSideToggle", "click", () => setDrawer("play", !state.playSideOpen));
on("historyBtn", "click", async (event) => {
  event.stopPropagation();
  if (state.historyOpen) {
    setHistoryPopup(false);
    return;
  }
  await renderHistory();
  setHistoryPopup(true);
});
on("closeHistoryBtn", "click", () => setHistoryPopup(false));
on("clearHistoryBtn", "click", async () => {
  await api("/api/history/clear", { method: "POST" });
  await renderHistory();
});
const historyPopup = $("historyPopup");
if (historyPopup) {
  historyPopup.addEventListener("pointerdown", (event) => event.stopPropagation());
}
on("historyList", "click", async (event) => {
  const item = event.target.closest(".history-item");
  if (!item) return;
  event.preventDefault();
  event.stopPropagation();
  await openHistoryItem(item);
});

on("playBtn", "click", () => {
  toggleVodPlayback(false);
});
on("nextEpisodeBtn", "click", () => playEpisode(state.episodeIndex + 1));
if (video) {
  video.addEventListener("play", () => {
    $("playBtn").classList.add("is-playing");
    $("playBtn").dataset.state = "playing";
    $("centerState").classList.add("hidden");
    showControls();
  });
  video.addEventListener("pause", () => {
    $("playBtn").classList.remove("is-playing");
    $("playBtn").dataset.state = "paused";
    $("centerState").classList.add("hidden");
  });
  video.addEventListener("waiting", () => $("loadingState").classList.remove("hidden"));
  video.addEventListener("playing", () => $("loadingState").classList.add("hidden"));
  video.addEventListener("timeupdate", () => {
    $("timeText").textContent = `${formatTime(video.currentTime)} / ${formatTime(video.duration)}`;
    if (Number.isFinite(video.duration) && video.duration > 0) {
      $("seek").value = String(Math.floor((video.currentTime / video.duration) * 1000));
      updateSeekVisual();
      if (state.skip.enabled && state.skip.intro > 0 && video.currentTime > 0 && video.currentTime < state.skip.intro) {
        video.currentTime = state.skip.intro;
      }
      if (state.skip.enabled && state.skip.outro > 0 && video.duration - video.currentTime <= state.skip.outro) {
        playEpisode(state.episodeIndex + 1);
      }
    }
  });
}

const content = document.querySelector("#homeView .content");
if (content) {
  content.addEventListener("scroll", async () => {
    const distance = content.scrollHeight - content.scrollTop - content.clientHeight;
    if (distance < 420) await loadNextLibraryPage();
  });
}

const librarySentinel = $("librarySentinel");
if (content && librarySentinel && "IntersectionObserver" in window) {
  const observer = new IntersectionObserver((entries) => {
    if (entries.some((entry) => entry.isIntersecting)) {
      loadNextLibraryPage().catch(console.error);
    }
  }, { root: content, rootMargin: "480px 0px" });
  observer.observe(librarySentinel);
}

setInterval(saveHistorySoon, 5000);
window.addEventListener("beforeunload", saveHistorySoon);

on("seek", "input", () => {
  updateSeekVisual();
  const targetVideo = activeVideoForKind("vod");
  if (Number.isFinite(targetVideo.duration) && targetVideo.duration > 0) {
    targetVideo.currentTime = (Number($("seek").value) / 1000) * targetVideo.duration;
  }
});
on("volumeBtn", "click", (event) => {
  event.stopPropagation();
  state.volumeOpen = !state.volumeOpen;
  closePlayerPopups("vod-volume");
  setPopup("volumePopup", state.volumeOpen);
});on("volumeSlider", "input", (event) => {
  setVolume(activeVideoForKind("vod"), "volumeSlider", "volumeValue", event.target.value);
});on("playerSettingsBtn", "click", (event) => {
  event.stopPropagation();
  state.playerSettingsOpen = !state.playerSettingsOpen;
  closePlayerPopups("vod-settings");
  setSettingsPage("vod", "main");
  setPopup("playerSettingsPopup", state.playerSettingsOpen);
});on("compactBtn", "click", toggleCompactMode);
on("fullscreenBtn", "click", toggleFullscreenMode);

document.addEventListener("pointermove", (event) => {
  if (event.target.closest(".player-shell")) {
    showControls();
    if (state.fullscreenMode) {
      clearTimeout(state.hideControlsTimer);
      state.hideControlsTimer = setTimeout(() => {
        if (!state.playerSettingsOpen && !state.volumeOpen) {
          hideControlsNow();
        }
      }, 2000);
    }
    return;
  }
  if (state.compactMode) {
    const shell = document.querySelector("#playerView .player-shell");
    if (shell) {
      const rect = shell.getBoundingClientRect();
      if (
        event.clientX >= rect.left
        && event.clientX <= rect.right
        && event.clientY >= rect.bottom - 76
        && event.clientY <= rect.bottom
      ) {
        showControls();
      }
    }
  }
});

document.addEventListener("pointerdown", (event) => {
  if (
    state.compactMode
    && event.button === 0
    && event.target.closest("#playerView .player-shell")
    && !event.target.closest(".media-controls, .player-settings-popup, .volume-popup, button, input, select, textarea")
  ) {
    const shell = event.target.closest("#playerView .player-shell");
    const rect = shell.getBoundingClientRect();
    if (event.clientY - rect.top <= 38) {
      event.preventDefault();
      ipc("drag_window");
      return;
    }
  }
  if (!event.target.closest(".player-settings") && !event.target.closest(".volume-control")) {
    closePlayerPopups();
  }
});
on("skipBtn", "click", () => {
  closePlayerPopups();
  $("skipDialog").showModal();
});
on("introCurrentBtn", "click", () => {
  $("introInput").value = String(Math.max(0, Math.round(activeVideoForKind("vod").currentTime || 0)));
});
on("outroCurrentBtn", "click", () => {
  const targetVideo = activeVideoForKind("vod");
  const remaining = Number.isFinite(targetVideo.duration) ? Math.max(0, Math.round(targetVideo.duration - (targetVideo.currentTime || 0))) : 0;
  $("outroInput").value = String(remaining);
});
on("saveSkipBtn", "click", async () => {
  if (!state.current) return;
  state.skip.intro = Number($("introInput").value) || 0;
  state.skip.outro = Number($("outroInput").value) || 0;
  state.skip.enabled = $("skipEnabledInput") ? $("skipEnabledInput").checked : true;
  await postJson("/api/skip", {
    source: state.current.source,
    video_id: state.current.id,
    intro_end_sec: state.skip.intro,
    outro_offset_sec: state.skip.outro,
    enabled: state.skip.enabled,
  });
  $("skipDialog").close();
});
on("clearSkipBtn", "click", async () => {
  $("introInput").value = "0";
  $("outroInput").value = "0";
  if ($("skipEnabledInput")) $("skipEnabledInput").checked = false;
  $("saveSkipBtn").click();
});

document.querySelector("#playerView .player-shell")?.addEventListener("click", (event) => {
  if (event.target.closest(".media-controls, .player-settings-popup, .volume-popup, button, input, select, textarea")) return;
  event.preventDefault();
  event.stopPropagation();
  toggleVodPlayback(true);
  showControls();
}, true);

document.addEventListener("keydown", (event) => {
  if (event.target.matches("input, textarea, select")) return;
  const activeVideo = activeVideoForKind("vod");
  if (event.key === "Escape" && state.fullscreenMode) {
    event.preventDefault();
    toggleFullscreenMode();
    return;
  }
  if (event.code === "Space") {
    event.preventDefault();
    toggleVodPlayback(false);
  }
  if (event.key === "ArrowLeft") activeVideo.currentTime = Math.max(0, activeVideo.currentTime - 10);
  if (event.key === "ArrowRight") activeVideo.currentTime = Math.min(activeVideo.duration || 0, activeVideo.currentTime + 10);
});

bootstrap().catch((error) => {
  setStatus($("cards"), error.message);
});

setDrawer("play", state.playSideOpen);
updateSeekVisual();
setVolume(activeVideoForKind("vod"), "volumeSlider", "volumeValue", activeVideoForKind("vod").volume || 1);
updatePlayerSettingLabels();
})();

