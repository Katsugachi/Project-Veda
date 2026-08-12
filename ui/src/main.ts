import "./styles.css";
import "./shape-overrides.css";
import { bridge } from "./bridge";
import { bytes, timeLabel } from "./format";
import { icon, logo } from "./icons";
import { escapeHtml, renderMarkdown } from "./markdown";
import type { Attachment, ChatMessage, Docset, DownloadItem, PreflightReport, ReaderSource, ReasoningMode, Theme, View } from "./types";

type AppState = {
  view: View;
  theme: Theme;
  sidebarCollapsed: boolean;
  settingsOpen: boolean;
  reader?: ReaderSource;
  modelOpen: boolean;
  onboardingOpen: boolean;
  setupStep: number;
  setupRunning: boolean;
  setupError?: string;
  setupStatus: string;
  setupProgress: number;
  selectedQuant: "q5" | "q8";
  setupDocsets: Set<string>;
  mode: ReasoningMode;
  preflight?: PreflightReport;
  docsets: Docset[];
  downloads: DownloadItem[];
  messages: ChatMessage[];
  attachments: Attachment[];
  busy: boolean;
  toasts: string[];
};

const state: AppState = {
  view: "chat",
  theme: (localStorage.getItem("palor:theme") as Theme | null) ?? "dark",
  sidebarCollapsed: localStorage.getItem("palor:sidebar") === "collapsed",
  settingsOpen: false,
  modelOpen: false,
  onboardingOpen: localStorage.getItem("palor:onboarded") !== "true",
  setupStep: 0,
  setupRunning: false,
  setupStatus: "Preparing setup…",
  setupProgress: 0,
  selectedQuant: "q5",
  setupDocsets: new Set(["python"]),
  mode: "fast",
  docsets: [],
  downloads: [],
  messages: [],
  attachments: [],
  busy: false,
  toasts: [],
};

const mount = document.querySelector<HTMLDivElement>("#app");
if (!mount) throw new Error("Palor app mount was not found");
const app: HTMLDivElement = mount;

document.documentElement.dataset.theme = state.theme;

function isDocInstalled(doc: Docset): boolean {
  return doc.state === "installed" || doc.state === "updateAvailable";
}

function navItem(view: View, label: string, iconName: "chat" | "book" | "download", badge?: number): string {
  const active = state.view === view ? " active" : "";
  return `<button class="nav-item${active}" data-view="${view}" title="${label}">${icon(iconName)}<span class="nav-label">${label}</span>${badge ? `<span class="badge">${badge}</span>` : ""}</button>`;
}

function renderSidebar(): string {
  const activeDownloads = state.downloads.filter((item) => item.state === "downloading" || item.state === "indexing").length;
  return `<aside class="sidebar">
    <div class="side-top">
      <div class="wordmark">${logo()}<span class="wordmark-label">Palor</span></div>
      <button class="icon-button" id="collapseSidebar" title="Collapse sidebar" aria-label="Collapse sidebar">${icon("panel")}</button>
    </div>
    <nav class="primary-nav" aria-label="Primary">
      <button class="nav-item" id="newChat" title="New chat">${icon("plus")}<span class="nav-label">New chat</span></button>
      ${navItem("chat", "Chats", "chat")}
      ${navItem("docs", "Docs", "book")}
      ${navItem("downloads", "Downloads", "download", activeDownloads)}
    </nav>
    <div class="side-spacer"></div>
    <div class="side-bottom">
      <button class="nav-item" id="settingsButton" title="Settings">${icon("settings")}<span class="nav-label">Settings</span></button>
    </div>
  </aside>`;
}

function renderTopbar(): string {
  const title = state.view === "chat" ? (state.messages.length ? "Documentation chat" : "New chat") : state.view === "docs" ? "Documentation library" : "Downloads & storage";
  return `<header class="topbar">
    <div class="page-title">${title}</div>
    <div class="topbar-actions">
      <button class="icon-button" id="themeToggle" title="Toggle theme" aria-label="Toggle theme">${icon(state.theme === "dark" ? "moon" : "sun")}</button>
    </div>
  </header>`;
}

function attachmentChips(removable = true): string {
  if (!state.attachments.length) return "";
  return `<div class="attachment-row">${state.attachments.map((file) => `<span class="attachment-chip">${icon("file")}<span>${escapeHtml(file.name)}</span>${removable ? `<button class="icon-button remove-attachment" data-attachment="${escapeHtml(file.id)}" title="Remove ${escapeHtml(file.name)}" style="width:18px;height:18px">${icon("x")}</button>` : ""}</span>`).join("")}</div>`;
}

function renderComposer(): string {
  const installed = state.docsets.filter(isDocInstalled).length;
  return `<div class="composer-wrap">
    <div class="composer-shell">
      ${attachmentChips()}
      <div class="composer">
        <textarea id="composerInput" rows="1" placeholder="Ask your offline docs." ${state.busy ? "disabled" : ""}></textarea>
        <div class="composer-row">
          <div class="composer-left">
            <button class="tool-button" id="attachButton" title="Attach code" aria-label="Attach code">${icon("paperclip")}</button>
            <input id="fileInput" type="file" multiple hidden accept=".py,.pyi,.c,.h,.cc,.cpp,.cxx,.hpp,.html,.css,.js,.mjs,.cjs,.ts,.tsx,.jsx,.json,.md,.txt" />
            <button class="scope-button" id="scopeButton" title="Choose documentation sources">${icon("layers")} ${installed || "No"} docsets</button>
          </div>
          <div class="composer-right">
            <button class="model-button" id="modelButton" aria-expanded="${state.modelOpen}"><span class="mode-indicator"></span>MiniCPM 5 ${icon("chevron")}</button>
            <button class="send-button" id="sendButton" title="Send" aria-label="Send message" ${state.busy ? "disabled" : ""}>${state.busy ? icon("pause") : icon("arrowUp")}</button>
          </div>
        </div>
      </div>
    </div>
    ${state.modelOpen ? renderModelPopover() : ""}
  </div>`;
}

function renderModelPopover(): string {
  return `<div class="popover" id="modelPopover">
    <div class="popover-title">MiniCPM 5 mode</div>
    <button class="popover-item${state.mode === "fast" ? " selected" : ""}" data-mode="fast">
      ${icon("spark")}<span class="popover-item-copy">Fast<small>Direct answers · lower latency</small></span>${state.mode === "fast" ? icon("check") : ""}
    </button>
    <button class="popover-item${state.mode === "think" ? " selected" : ""}" data-mode="think">
      ${icon("chip")}<span class="popover-item-copy">Think<small>Deeper reasoning · more tokens</small></span>${state.mode === "think" ? icon("check") : ""}
    </button>
  </div>`;
}

function renderEmptyChat(): string {
  return `<section class="chat-view">
    <div class="empty-chat">
      <div class="hero">
        <div class="greeting">${logo()}<h1>Ask Anything</h1></div>
        ${renderComposer()}
      </div>
    </div>
  </section>`;
}

function renderMessage(message: ChatMessage): string {
  const sourceMarkup = message.sources?.length
    ? `<div class="message-sources">${message.sources.map((source) => `<button class="source-chip" data-source="${escapeHtml(source.url)}" title="Open ${escapeHtml(source.title)}, ${escapeHtml(source.section)}"><span class="source-n">${escapeHtml(source.id)}</span>${escapeHtml(source.docset)} · ${escapeHtml(source.section)}</button>`).join("")}</div>`
    : "";
  const attachments = message.attachments?.length
    ? `<div class="attachment-row">${message.attachments.map((file) => `<span class="attachment-chip">${icon("file")}<span>${escapeHtml(file.name)}</span></span>`).join("")}</div>`
    : "";
  return `<article class="message ${message.role}" data-message-id="${message.id}">
    <div class="message-avatar">${message.role === "assistant" ? logo() : "A"}</div>
    <div>
      <div class="message-head">${message.role === "assistant" ? "Palor" : "You"}<span class="message-time">${timeLabel(message.createdAt)}</span></div>
      ${attachments}
      <div class="message-body">${renderMarkdown(message.content)}${message.streaming ? '<span class="stream-caret"></span>' : ""}</div>
      ${sourceMarkup}
    </div>
  </article>`;
}

function renderChat(): string {
  if (!state.messages.length) return renderEmptyChat();
  return `<section class="chat-view">
    <div class="messages" id="messagesScroller"><div class="message-list">${state.messages.map(renderMessage).join("")}</div></div>
    ${renderComposer()}
  </section>`;
}

function docAction(doc: Docset): string {
  if (doc.state === "installed") {
    return `<div class="installed-check">${icon("check")} Installed</div><button class="button remove-doc" data-docset="${doc.id}">Remove</button>`;
  }
  if (doc.state === "updateAvailable") {
    return `<div class="update-check">Update available</div><div class="doc-action-buttons"><button class="button remove-doc" data-docset="${doc.id}">Remove</button><button class="button primary install-doc" data-docset="${doc.id}">Update</button></div>`;
  }
  if (doc.state === "downloading" || doc.state === "indexing") return `<div class="progress-track"><div class="progress-value" style="width:${doc.progress}%"></div></div><span class="download-state">${doc.state} ${Math.round(doc.progress)}%</span>`;
  return `<span></span><button class="button primary install-doc" data-docset="${doc.id}">${icon("download")} Download</button>`;
}

function renderDocs(): string {
  const installed = state.docsets.filter(isDocInstalled);
  const indexedPages = installed.reduce((sum, doc) => sum + (doc.pages ?? 0), 0);
  return `<section class="content-view"><div class="content-inner">
    <div class="content-header">
      <div><h1>Docs</h1><p>Install, update, or remove documentation.</p></div>
      <label class="search-box">${icon("search")}<input id="docSearch" placeholder="Filter documentation" /></label>
    </div>
    <div class="library-summary">${indexedPages.toLocaleString()} installed pages</div>
    <div class="doc-grid">${state.docsets.map((doc) => `<article class="doc-card" data-doc-filter="${escapeHtml(`${doc.name} ${doc.detail} ${doc.version}`.toLowerCase())}" style="--doc-color:${doc.accent}">
      <div class="doc-head"><div class="doc-icon">${doc.initials}</div><div class="doc-copy"><div class="doc-name">${doc.name}</div><div class="doc-version">${doc.version}</div></div></div>
      <div class="doc-description">${doc.detail}</div>
      <div class="doc-meta"><span>${bytes(doc.compressedBytes)} download</span>${doc.pages !== undefined ? `<span>${doc.pages.toLocaleString()} installed pages</span>` : ""}</div>
      <div class="doc-footer">${docAction(doc)}</div>
    </article>`).join("")}</div>
  </div></section>`;
}

function renderDownloads(): string {
  const rows = state.downloads.length ? state.downloads.map((item) => {
    const indexing = item.id.endsWith("-index") || item.state === "indexing";
    const progressCopy = indexing
      ? `${Math.round(item.downloadedBytes).toLocaleString()} of ${Math.round(item.totalBytes).toLocaleString()} sections`
      : `${bytes(item.downloadedBytes)} of ${bytes(item.totalBytes)}${item.speedBytes ? ` · ${bytes(item.speedBytes)}/s` : ""}`;
    return `<div class="download-row">
      <div class="download-file-icon">${item.id.startsWith("minicpm") ? icon("chip") : icon("file")}</div>
      <div><div class="download-name">${escapeHtml(item.name)}</div><div class="download-detail">${escapeHtml(item.detail)}</div></div>
      <div><div class="progress-track"><div class="progress-value" style="width:${item.progress}%"></div></div><div class="download-progress-copy">${progressCopy}</div></div>
      <div class="download-state">${item.state === "installed" ? "Ready" : escapeHtml(item.state)}</div>
    </div>`;
  }).join("") : `<div class="empty-list">No downloads yet.</div>`;
  return `<section class="content-view"><div class="content-inner">
    <div class="content-header"><div><h1>Downloads</h1><p>Manage downloaded files.</p></div><button class="button" id="dataFolder">${icon("folder")} Data folder</button></div>
    <div class="download-list">${rows}</div>
  </div></section>`;
}

function renderSettings(): string {
  if (!state.settingsOpen) return "";
  const context = state.preflight?.recommendedContext.toLocaleString() ?? "Automatic";
  return `<div class="modal-backdrop" id="settingsBackdrop"><section class="settings-panel" role="dialog" aria-modal="true" aria-labelledby="settingsTitle">
    <header class="settings-header"><h2 id="settingsTitle">Settings</h2><button class="icon-button" id="closeSettings" aria-label="Close settings">${icon("x")}</button></header>
    <div class="settings-body">
      <div class="setting-row"><div class="setting-copy"><div class="setting-name">Appearance</div><div class="setting-detail">${state.theme === "dark" ? "Dark" : "Light"}</div></div><button class="button" id="settingsTheme">Change</button></div>
      <div class="setting-row"><div class="setting-copy"><div class="setting-name">Model</div><div class="setting-detail">MiniCPM 5 · ${state.selectedQuant.toUpperCase()}</div></div></div>
      <div class="setting-row"><div class="setting-copy"><div class="setting-name">Context</div><div class="setting-detail">${context} tokens</div></div></div>
      <div class="setting-row"><div class="setting-copy"><div class="setting-name">Files</div><div class="setting-detail">Open Palor's data folder</div></div><button class="button" id="settingsDataFolder">Open</button></div>
      <div class="privacy-box"><strong>Data handling.</strong> Prompts, documentation and attached code remain on this device. Telemetry is disabled.</div>
      <div class="setting-row"><div class="setting-copy"><div class="setting-name">Palor</div><div class="setting-detail">Version 0.1.0</div></div></div>
    </div>
  </section></div>`;
}

function checkRow(kind: "memory" | "drive" | "chip", name: string, detail: string, value: string, status: "ok" | "warn" | "fail"): string {
  return `<div class="preflight-row"><div class="check-icon ${status === "ok" ? "" : status}">${icon(status === "ok" ? "check" : kind)}</div><div class="preflight-copy"><div class="preflight-name">${name}</div><div class="preflight-detail">${detail}</div></div><div class="preflight-value">${value}</div></div>`;
}

function renderOnboardingBody(): string {
  const report = state.preflight;
  if (state.setupError) {
    return `<div class="onboarding-kicker">Setup stopped</div><h1>Something went wrong</h1><p class="onboarding-lead setup-error">${escapeHtml(state.setupError)}</p>`;
  }
  if (state.setupRunning) {
    return `<div class="onboarding-kicker">Setup</div><h1>Preparing Palor</h1><p class="onboarding-lead">Keep Palor open until setup finishes.</p>
      <div class="setup-progress"><div class="progress-track"><div class="progress-value" id="setupProgressBar" style="width:${state.setupProgress}%"></div></div><div class="setup-progress-copy" id="setupProgressText">${escapeHtml(state.setupStatus)}</div></div>`;
  }
  if (state.setupStep === 0) {
    const hasFailures = Boolean(report?.hardFailures.length);
    const ramStatus = hasFailures && report?.hardFailures.some((value) => value.toLowerCase().includes("memory")) ? "fail" : "ok";
    const diskStatus = hasFailures && report?.hardFailures.some((value) => value.toLowerCase().includes("disk")) ? "fail" : "ok";
    return `<div class="onboarding-kicker">System check</div><h1>Check this device</h1><p class="onboarding-lead">Before downloading, Palor checks available memory and storage. At least 10 GB of free SSD space is required.</p>
      <div class="preflight-list">
        ${report ? checkRow("memory", "Memory and context", `Recommended context: ${report.recommendedContext.toLocaleString()} tokens`, bytes(report.totalMemoryBytes), ramStatus) : checkRow("memory", "Memory and context", "Checking available RAM…", "—", "warn")}
        ${report ? checkRow("drive", "Fast local storage", "10 GB minimum free space", `${bytes(report.freeDiskBytes)} · ${report.diskKind.toUpperCase()}`, diskStatus) : checkRow("drive", "Fast local storage", "Checking disk and free space…", "—", "warn")}
        ${report ? checkRow("chip", "Native runtime", `${report.operatingSystem} · ${report.architecture}`, "Auto-detect", "ok") : checkRow("chip", "Native runtime", "Finding the best llama.cpp build…", "—", "warn")}
      </div>`;
  }
  if (state.setupStep === 1) {
    return `<div class="onboarding-kicker">MiniCPM 5</div><h1>Choose a model size</h1><p class="onboarding-lead">Q5 is suitable for most devices. Q8 uses more memory and provides slightly higher fidelity.</p>
      <div class="option-grid">
        <button class="option-card${state.selectedQuant === "q5" ? " selected" : ""}" data-quant="q5"><div class="option-name">Q5</div><div class="option-detail">Uses less memory.</div><div class="option-meta"><span class="meta-tag">751 MiB</span><span class="meta-tag">8 GB RAM</span></div></button>
        <button class="option-card${state.selectedQuant === "q8" ? " selected" : ""}" data-quant="q8"><div class="option-name">Q8</div><div class="option-detail">Uses more memory.</div><div class="option-meta"><span class="meta-tag">1.07 GiB</span><span class="meta-tag">12 GB RAM</span></div></button>
      </div>`;
  }
  const total = state.docsets.filter((doc) => state.setupDocsets.has(doc.id)).reduce((sum, doc) => sum + doc.compressedBytes, 0);
  return `<div class="onboarding-kicker">Documentation</div><h1>Choose documentation</h1><p class="onboarding-lead">Download only what you need. You can change this later.</p>
    <div class="setup-docs">${state.docsets.map((doc) => `<button class="setup-doc${state.setupDocsets.has(doc.id) ? " selected" : ""}" data-setup-doc="${doc.id}"><div class="setup-doc-abbr">${doc.initials}</div><div class="setup-doc-name">${doc.name}</div></button>`).join("")}</div>
    <div class="setup-summary">${state.setupDocsets.size} packs · ${bytes(total)} download</div>`;
}

function renderOnboarding(): string {
  if (!state.onboardingOpen) return "";
  const hasFailures = Boolean(state.preflight?.hardFailures.length);
  const last = state.setupStep === 2;
  const progressMode = state.setupRunning || Boolean(state.setupError);
  const footer = state.setupError
    ? `<div class="onboarding-note">Downloaded files are kept, so retrying will resume where possible.</div><div class="button-row"><button class="button" id="setupCancelError">Back</button><button class="button primary" id="setupRetry">Retry</button></div>`
    : state.setupRunning
      ? `<div class="onboarding-note">Setup must finish before the rest of the app can be used.</div>`
      : `<div class="onboarding-note">You can change these options later in Settings.</div><div class="button-row">${state.setupStep > 0 ? '<button class="button" id="setupBack">Back</button>' : ""}<button class="button primary" id="setupNext" ${(hasFailures && state.setupStep === 0) || (last && state.setupDocsets.size === 0) ? "disabled" : ""}>${last ? "Set up Palor" : "Continue"}</button></div>`;
  return `<div class="modal-backdrop setup-backdrop"><section class="onboarding" role="dialog" aria-modal="true" aria-labelledby="setupTitle">
    <div class="onboarding-top"><div class="onboarding-brand">${logo()} Palor</div>${progressMode ? "" : `<div class="step-dots">${[0, 1, 2].map((step) => `<span class="step-dot${state.setupStep === step ? " active" : ""}"></span>`).join("")}</div>`}</div>
    <div class="onboarding-body" id="setupTitle">${renderOnboardingBody()}</div>
    <div class="onboarding-bottom">${footer}</div>
  </section></div>`;
}

function renderReader(): string {
  if (!state.reader) return "";
  return `<div class="modal-backdrop" id="readerBackdrop"><section class="settings-panel" role="dialog" aria-modal="true" aria-labelledby="readerTitle">
    <header class="settings-header"><div><h2 id="readerTitle">${escapeHtml(state.reader.title)}</h2><div class="setting-detail">${escapeHtml(state.reader.docset)} · ${escapeHtml(state.reader.section)}</div></div><button class="icon-button" id="closeReader" aria-label="Close source">${icon("x")}</button></header>
    <div class="settings-body"><article class="setting-section message-body">${renderMarkdown(state.reader.text)}</article><div class="privacy-box"><strong>Local source.</strong> This excerpt came from the downloaded documentation index. Canonical reference: ${escapeHtml(state.reader.url)}</div></div>
  </section></div>`;
}

function renderToasts(): string {
  return `<div class="toast-stack">${state.toasts.map((toast) => `<div class="toast"><span class="status-dot"></span>${toast}</div>`).join("")}</div>`;
}

function render(): void {
  const view = state.view === "chat" ? renderChat() : state.view === "docs" ? renderDocs() : renderDownloads();
  app.innerHTML = `<div class="app-shell${state.sidebarCollapsed ? " sidebar-collapsed" : ""}">${renderSidebar()}<main class="main">${renderTopbar()}<div class="view">${view}</div></main></div>${renderSettings()}${renderReader()}${renderOnboarding()}${renderToasts()}`;
  bindEvents();
  if (state.messages.length) requestAnimationFrame(scrollMessages);
}

type TransitionDocument = Document & {
  startViewTransition?: (update: () => void) => unknown;
};

function smoothRender(): void {
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const start = (document as TransitionDocument).startViewTransition;
  if (!reducedMotion && start) {
    start.call(document, () => render());
  } else {
    render();
  }
}

let renderTimer: number | undefined;
function scheduleRender(): void {
  if (renderTimer !== undefined) return;
  renderTimer = window.setTimeout(() => {
    renderTimer = undefined;
    if (state.setupRunning && document.querySelector("#setupProgressBar")) return;
    render();
  }, 250);
}

function toggleTheme(): void {
  state.theme = state.theme === "dark" ? "light" : "dark";
  document.documentElement.dataset.theme = state.theme;
  localStorage.setItem("palor:theme", state.theme);
  const toggle = document.querySelector<HTMLElement>("#themeToggle");
  if (toggle) toggle.innerHTML = icon(state.theme === "dark" ? "moon" : "sun");
  const detail = document.querySelector<HTMLElement>("#settingsTheme")?.closest(".setting-row")?.querySelector<HTMLElement>(".setting-detail");
  if (detail) detail.textContent = state.theme === "dark" ? "Dark" : "Light";
}

function toast(message: string): void {
  state.toasts.push(message);
  render();
  window.setTimeout(() => { state.toasts.shift(); render(); }, 3200);
}

function scrollMessages(): void {
  const scroller = document.querySelector<HTMLElement>("#messagesScroller");
  if (scroller) scroller.scrollTop = scroller.scrollHeight;
}

function setView(view: View): void {
  state.view = view;
  state.modelOpen = false;
  smoothRender();
}

async function installDocsetBlocking(id: string): Promise<void> {
  const doc = state.docsets.find((item) => item.id === id);
  if (!doc || doc.state === "installed") return;
  state.onboardingOpen = true;
  state.setupRunning = true;
  state.setupError = undefined;
  state.setupProgress = 0;
  state.setupStatus = `Preparing ${doc.name}…`;
  doc.state = "downloading";
  smoothRender();
  try {
    await bridge.installDocset(id);
    [state.docsets, state.downloads] = await Promise.all([bridge.docsets(), bridge.downloads()]);
    state.setupRunning = false;
    state.onboardingOpen = false;
    smoothRender();
  } catch (error) {
    state.setupError = error instanceof Error ? error.message : String(error);
    state.setupRunning = false;
    smoothRender();
  }
}

function updateSetupProgress(status: string, progress = 0): void {
  state.setupStatus = status;
  state.setupProgress = progress;
  const bar = document.querySelector<HTMLElement>("#setupProgressBar");
  const text = document.querySelector<HTMLElement>("#setupProgressText");
  if (bar) bar.style.width = `${progress}%`;
  if (text) text.textContent = status;
}

async function removeDocsetBlocking(id: string): Promise<void> {
  const doc = state.docsets.find((item) => item.id === id);
  if (!doc || !isDocInstalled(doc)) return;
  if (state.docsets.filter(isDocInstalled).length <= 1) {
    toast("Keep at least one documentation pack installed.");
    return;
  }
  if (!window.confirm(`Remove ${doc.name} documentation from this device?`)) return;

  state.onboardingOpen = true;
  state.setupRunning = true;
  state.setupError = undefined;
  state.setupProgress = 0;
  state.setupStatus = `Removing ${doc.name}…`;
  smoothRender();
  try {
    await bridge.removeDocset(id);
    [state.docsets, state.downloads] = await Promise.all([bridge.docsets(), bridge.downloads()]);
    state.setupRunning = false;
    state.onboardingOpen = false;
    smoothRender();
  } catch (error) {
    state.setupRunning = false;
    state.onboardingOpen = false;
    smoothRender();
    toast(`Could not remove ${doc.name}: ${error instanceof Error ? error.message : String(error)}`);
  }
}

async function runSetup(): Promise<void> {
  state.onboardingOpen = true;
  state.setupRunning = true;
  state.setupError = undefined;
  state.setupProgress = 0;
  state.setupStatus = "Preparing model files…";
  smoothRender();
  try {
    await bridge.prepareResources(state.selectedQuant);
    for (const id of state.setupDocsets) {
      const doc = state.docsets.find((item) => item.id === id);
      if (doc?.state === "installed") continue;
      updateSetupProgress(`Preparing ${doc?.name ?? id}…`);
      await bridge.installDocset(id);
    }
    state.docsets = await bridge.docsets();
    state.downloads = await bridge.downloads();
    localStorage.setItem("palor:onboarded", "true");
    state.setupRunning = false;
    state.onboardingOpen = false;
    smoothRender();
  } catch (error) {
    state.setupError = error instanceof Error ? error.message : String(error);
    state.setupRunning = false;
    smoothRender();
  }
}

async function addFiles(files: FileList): Promise<void> {
  const maxBytes = 512 * 1024;
  for (const file of Array.from(files).slice(0, 8)) {
    if (file.size > maxBytes) { toast(`${file.name} is larger than the 512 KB attachment limit.`); continue; }
    const content = await file.text();
    const language = file.name.split(".").pop()?.toLowerCase() ?? "text";
    state.attachments.push({ id: crypto.randomUUID(), name: file.name, bytes: file.size, language, content });
  }
  smoothRender();
}

async function sendMessage(): Promise<void> {
  if (state.busy) return;
  const input = document.querySelector<HTMLTextAreaElement>("#composerInput");
  const text = input?.value.trim() ?? "";
  if (!text) return;
  const attachments = structuredClone(state.attachments);
  state.messages.push({ id: crypto.randomUUID(), role: "user", content: text, createdAt: Date.now(), attachments });
  const assistant: ChatMessage = { id: crypto.randomUUID(), role: "assistant", content: "Searching installed docs…", createdAt: Date.now(), streaming: true };
  state.messages.push(assistant);
  state.attachments = [];
  state.busy = true;
  smoothRender();
  try {
    const response = await bridge.ask({ chatId: "local", message: text, mode: state.mode, docsets: state.docsets.filter(isDocInstalled).map((doc) => doc.id), attachments });
    assistant.content = response.content;
    assistant.sources = response.sources;
    assistant.streaming = false;
  } catch (error) {
    assistant.content = `Palor could not complete the local request. ${error instanceof Error ? error.message : String(error)}`;
    assistant.streaming = false;
  } finally {
    state.busy = false;
    smoothRender();
  }
}

function bindEvents(): void {
  document.querySelectorAll<HTMLElement>("[data-view]").forEach((button) => button.addEventListener("click", () => setView(button.dataset.view as View)));
  document.querySelector("#newChat")?.addEventListener("click", () => { state.messages = []; setView("chat"); });
  document.querySelector("#collapseSidebar")?.addEventListener("click", () => {
    state.sidebarCollapsed = !state.sidebarCollapsed;
    localStorage.setItem("palor:sidebar", state.sidebarCollapsed ? "collapsed" : "open");
    document.querySelector(".app-shell")?.classList.toggle("sidebar-collapsed", state.sidebarCollapsed);
  });
  document.querySelector("#themeToggle")?.addEventListener("click", toggleTheme);
  document.querySelector("#settingsTheme")?.addEventListener("click", toggleTheme);
  document.querySelector("#settingsButton")?.addEventListener("click", () => { state.settingsOpen = true; smoothRender(); });
  document.querySelector("#closeSettings")?.addEventListener("click", () => { state.settingsOpen = false; smoothRender(); });
  document.querySelector("#settingsBackdrop")?.addEventListener("click", (event) => { if (event.target === event.currentTarget) { state.settingsOpen = false; smoothRender(); } });
  document.querySelector("#modelButton")?.addEventListener("click", () => { state.modelOpen = !state.modelOpen; smoothRender(); });
  document.querySelectorAll<HTMLElement>("[data-mode]").forEach((button) => button.addEventListener("click", () => { state.mode = button.dataset.mode as ReasoningMode; state.modelOpen = false; smoothRender(); }));
  document.querySelector("#sendButton")?.addEventListener("click", () => void sendMessage());
  const input = document.querySelector<HTMLTextAreaElement>("#composerInput");
  input?.addEventListener("input", () => { input.style.height = "auto"; input.style.height = `${Math.min(input.scrollHeight, 190)}px`; });
  input?.addEventListener("keydown", (event) => { if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); void sendMessage(); } });
  document.querySelector("#attachButton")?.addEventListener("click", () => document.querySelector<HTMLInputElement>("#fileInput")?.click());
  document.querySelector<HTMLInputElement>("#fileInput")?.addEventListener("change", (event) => { const files = (event.currentTarget as HTMLInputElement).files; if (files) void addFiles(files); });
  document.querySelectorAll<HTMLElement>(".remove-attachment").forEach((button) => button.addEventListener("click", () => { state.attachments = state.attachments.filter((file) => file.id !== button.dataset.attachment); smoothRender(); }));
  document.querySelectorAll<HTMLElement>(".install-doc").forEach((button) => button.addEventListener("click", () => void installDocsetBlocking(button.dataset.docset ?? "")));
  document.querySelectorAll<HTMLElement>(".remove-doc").forEach((button) => button.addEventListener("click", () => void removeDocsetBlocking(button.dataset.docset ?? "")));
  document.querySelectorAll<HTMLElement>("[data-source]").forEach((button) => button.addEventListener("click", () => {
    const url = button.dataset.source ?? "";
    void bridge.readSource(url).then((source) => { state.reader = source; smoothRender(); }).catch((error: unknown) => toast(`Could not open source: ${error instanceof Error ? error.message : String(error)}`));
  }));
  document.querySelector("#closeReader")?.addEventListener("click", () => { state.reader = undefined; smoothRender(); });
  document.querySelector("#readerBackdrop")?.addEventListener("click", (event) => { if (event.target === event.currentTarget) { state.reader = undefined; smoothRender(); } });
  document.querySelector("#dataFolder")?.addEventListener("click", () => void bridge.revealDataFolder());
  document.querySelector("#settingsDataFolder")?.addEventListener("click", () => void bridge.revealDataFolder());
  document.querySelector<HTMLInputElement>("#docSearch")?.addEventListener("input", (event) => {
    const query = (event.currentTarget as HTMLInputElement).value.trim().toLowerCase();
    document.querySelectorAll<HTMLElement>("[data-doc-filter]").forEach((card) => {
      card.hidden = Boolean(query) && !(card.dataset.docFilter ?? "").includes(query);
    });
  });
  document.querySelector("#scopeButton")?.addEventListener("click", () => setView("docs"));
  document.querySelector("#setupBack")?.addEventListener("click", () => { state.setupStep = Math.max(0, state.setupStep - 1); smoothRender(); });
  document.querySelector("#setupNext")?.addEventListener("click", () => {
    if (state.setupStep < 2) { state.setupStep += 1; smoothRender(); return; }
    void runSetup();
  });
  document.querySelector("#setupRetry")?.addEventListener("click", () => {
    state.setupError = undefined;
    void runSetup();
  });
  document.querySelector("#setupCancelError")?.addEventListener("click", () => {
    state.setupError = undefined;
    state.setupRunning = false;
    state.setupStep = 2;
    smoothRender();
  });
  document.querySelectorAll<HTMLElement>("[data-quant]").forEach((button) => button.addEventListener("click", () => { state.selectedQuant = button.dataset.quant as "q5" | "q8"; smoothRender(); }));
  document.querySelectorAll<HTMLElement>("[data-setup-doc]").forEach((button) => button.addEventListener("click", () => { const id = button.dataset.setupDoc ?? ""; if (state.setupDocsets.has(id)) state.setupDocsets.delete(id); else state.setupDocsets.add(id); smoothRender(); }));
}

async function init(): Promise<void> {
  render();
  const [preflight, docsets, downloads] = await Promise.all([bridge.preflight(), bridge.docsets(), bridge.downloads()]);
  state.preflight = preflight;
  state.selectedQuant = downloads.some((item) => item.id === "minicpm5-q8" && item.state === "installed")
    ? "q8"
    : downloads.some((item) => item.id === "minicpm5-q5" && item.state === "installed") ? "q5" : preflight.recommendedQuant;
  state.docsets = docsets;
  state.downloads = downloads;
  const modelReady = downloads.some((item) => item.id.startsWith("minicpm5-") && item.state === "installed");
  const docsReady = docsets.some(isDocInstalled);
  if (bridge.isDesktop() && (!modelReady || !docsReady)) {
    localStorage.removeItem("palor:onboarded");
    state.onboardingOpen = true;
  }
  await bridge.onDownloadProgress((item) => {
    const existing = state.downloads.findIndex((download) => download.id === item.id);
    if (existing >= 0) state.downloads[existing] = item; else state.downloads.push(item);
    const docId = item.id.endsWith("-index") ? item.id.slice(0, -6) : undefined;
    const doc = docId ? state.docsets.find((candidate) => candidate.id === docId) : undefined;
    if (doc) {
      doc.progress = item.progress;
      doc.state = item.state === "installed" ? "installed" : "indexing";
    }
    if (state.setupRunning) {
      updateSetupProgress(item.detail || item.name, item.progress);
      if (document.querySelector("#setupProgressBar")) return;
    }
    scheduleRender();
  });
  render();
}

void init();
