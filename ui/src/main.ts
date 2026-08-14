import "./styles.css";
import "./shape-overrides.css";
import { bridge } from "./bridge";
import { createChat, deriveTitle, groupChats, loadChats, newId, saveChats } from "./chats";
import { morph } from "./dom";
import { bytes, timeLabel } from "./format";
import { icon, logo } from "./icons";
import { escapeHtml, renderMarkdown } from "./markdown";
import type {
  Attachment,
  Chat,
  ChatMessage,
  Docset,
  DownloadItem,
  PreflightReport,
  ReaderSource,
  ReasoningMode,
  Theme,
  View,
} from "./types";

// Context is user-configurable from 0 (automatic) up to MiniCPM 5's ceiling.
const CONTEXT_MAX = 131_072;
const CONTEXT_STEP = 1_024;
const GIB_BYTES = 1024 ** 3;

// The backend refuses to prepare Q8 below this floor, so the option is
// disabled up front instead of failing later with "Something went wrong".
const Q8_MEMORY_FLOOR_BYTES = 12 * GIB_BYTES;

type AppState = {
  view: View;
  theme: Theme;
  sidebarCollapsed: boolean;
  settingsOpen: boolean;
  reader?: ReaderSource;
  readerLoading: boolean;
  modelOpen: boolean;
  scopeOpen: boolean;
  onboardingOpen: boolean;
  setupStep: number;
  setupRunning: boolean;
  setupError?: string;
  /** The onboarding step to return to after a failed setup ("Back"). */
  setupFailureStep: number;
  setupStatus: string;
  setupProgress: number;
  selectedQuant: "q5" | "q8";
  setupDocsets: Set<string>;
  mode: ReasoningMode;
  contextTokens: number;
  preflight?: PreflightReport;
  docsets: Docset[];
  docsetsError?: string;
  downloads: DownloadItem[];
  downloadsError?: string;
  chats: Chat[];
  activeChatId: string;
  renamingChatId?: string;
  menuChatId?: string;
  attachments: Attachment[];
  toasts: { id: string; text: string }[];
};

// Storage access is defensive: some embedded webviews block localStorage and
// an exception here must never take the app down.
function storageGet(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function storageRemove(key: string): void {
  try {
    localStorage.removeItem(key);
  } catch {
    // Storage unavailable; nothing to remove.
  }
}
function storageSet(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Storage unavailable; keep the in-memory state.
  }
}

// Legacy keys from the pre-rename "Palor" builds are read once so existing
// users keep their theme, sidebar and onboarding choices after upgrading.
function migrated(key: string): string | null {
  return storageGet(`veda:${key}`) ?? storageGet(`palor:${key}`);
}

// Raw OS error codes and transport noise are never shown to users, even if a
// lower layer leaks them (the backend cleans them up too; this is the last
// line of defense).
function errorText(error: unknown): string {
  let raw = error instanceof Error ? error.message : String(error);
  raw = raw.replace(/^Error:\s*/i, "");
  raw = raw.replace(/^error sending request for url \(.+?\):\s*/i, "");
  raw = raw.replace(/^client error \(connect\)[\s:]*/i, "");
  raw = raw.replace(/[\s:;]*\(os error \d+\)[\s.;]*$/i, "");
  const cleaned = raw.trim();
  return cleaned || raw;
}

const isAbort = (error: unknown): boolean =>
  error instanceof DOMException ? error.name === "AbortError" : (error as Error)?.name === "AbortError";

function readContextSetting(): number {
  const raw = Number(migrated("context"));
  if (!Number.isFinite(raw) || raw <= 0) return 0;
  return Math.min(CONTEXT_MAX, Math.round(raw));
}

const storedChats = loadChats();
const firstChat = storedChats[0] ?? createChat();

const state: AppState = {
  view: "chat",
  theme: (migrated("theme") as Theme | null) === "light" ? "light" : "dark",
  sidebarCollapsed: migrated("sidebar") === "collapsed",
  settingsOpen: false,
  readerLoading: false,
  modelOpen: false,
  scopeOpen: false,
  // Setup is offered on the first run, but "Skip for now" must stick: once
  // skipped, the modal does not reappear on launch (the Docs tab stays
  // reachable) until the user opens setup again from Settings.
  onboardingOpen: migrated("onboarded") !== "true" && migrated("onboarding-skipped") !== "true",
  setupStep: 0,
  setupRunning: false,
  setupStatus: "Preparing setup…",
  setupProgress: 0,
  setupFailureStep: 1,
  selectedQuant: "q5",
  setupDocsets: new Set(["python"]),
  mode: (migrated("mode") as ReasoningMode | null) === "think" ? "think" : "fast",
  contextTokens: readContextSetting(),
  docsets: [],
  downloads: [],
  chats: storedChats.length ? storedChats : [firstChat],
  activeChatId: firstChat.id,
  attachments: [],
  toasts: [],
};

const mount = document.querySelector<HTMLDivElement>("#app");
if (!mount) throw new Error("Veda app mount was not found");
const app: HTMLDivElement = mount;

document.documentElement.dataset.theme = state.theme;

// ---------------------------------------------------------------------------
// In-flight requests.
//
// A request belongs to a chat, not to the rendered view. The reply is written
// back into the chat store when it lands, so answers still arrive while the
// user is on Docs or Downloads, or reading another conversation.
// ---------------------------------------------------------------------------
type Pending = { chatId: string; messageId: string; controller: AbortController };
const pending = new Map<string, Pending>();

const activeChat = (): Chat =>
  state.chats.find((chat) => chat.id === state.activeChatId) ?? state.chats[0];

const isBusy = (chatId = state.activeChatId): boolean => pending.has(chatId);

function isDocInstalled(doc: Docset): boolean {
  return doc.state === "installed" || doc.state === "updateAvailable";
}

// Q8 needs a 12 GiB machine (the same floor the backend enforces before it
// will download the larger model). When the preflight knows the device cannot
// run it, the choice is disabled up front — a clear reason instead of a late
// "Something went wrong" during setup.
function q8Supported(): boolean {
  const total = state.preflight?.totalMemoryBytes;
  return total === undefined || total >= Q8_MEMORY_FLOOR_BYTES;
}

const contextLabel = (value: number): string =>
  value <= 0 ? "Automatic" : `${value.toLocaleString()} tokens`;

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

function navItem(view: View, label: string, iconName: "chat" | "book" | "download", badge?: number): string {
  const active = state.view === view ? " active" : "";
  return `<button class="nav-item${active}" data-key="nav-${view}" data-view="${view}" title="${label}">${icon(iconName)}<span class="nav-label">${label}</span>${badge ? `<span class="badge">${badge}</span>` : ""}</button>`;
}

function renderChatRow(chat: Chat): string {
  const active = chat.id === state.activeChatId && state.view === "chat" ? " active" : "";
  if (state.renamingChatId === chat.id) {
    return `<div class="recent renaming" data-key="chat-${chat.id}">
      <input class="recent-input" id="renameInput" data-rename="${escapeHtml(chat.id)}" value="${escapeHtml(chat.title)}" aria-label="Rename chat" />
    </div>`;
  }
  const menuOpen = state.menuChatId === chat.id;
  return `<div class="recent-wrap${menuOpen ? " menu-open" : ""}" data-key="chat-${chat.id}">
    <button class="recent${active}" data-chat="${escapeHtml(chat.id)}" title="${escapeHtml(chat.title)}">${escapeHtml(chat.title)}</button>
    <button class="recent-more" data-chat-menu="${escapeHtml(chat.id)}" aria-label="Chat options for ${escapeHtml(chat.title)}" aria-expanded="${menuOpen}">⋯</button>
    ${menuOpen ? `<div class="recent-menu" role="menu">
      <button class="recent-menu-item" data-chat-rename="${escapeHtml(chat.id)}" role="menuitem">${icon("pencil")} Rename</button>
      <button class="recent-menu-item danger" data-chat-delete="${escapeHtml(chat.id)}" role="menuitem">${icon("trash")} Delete</button>
    </div>` : ""}
  </div>`;
}

function renderHistory(): string {
  const groups = groupChats(state.chats.filter((chat) => chat.messages.length > 0));
  if (!groups.length) {
    return `<div class="recent-area"><div class="recents-empty">Your chats appear here.</div></div>`;
  }
  return `<div class="recent-area">
    <div class="recents">${groups
      .map(
        (group) => `<div class="recent-group" data-key="group-${group.label}">
          <div class="section-label">${group.label}</div>
          ${group.chats.map(renderChatRow).join("")}
        </div>`,
      )
      .join("")}</div>
  </div>`;
}

function renderSidebar(): string {
  const activeDownloads = state.downloads.filter((item) => item.state === "downloading" || item.state === "indexing").length;
  return `<aside class="sidebar" data-key="sidebar">
    <div class="side-top">
      <button class="wordmark" id="wordmark" title="${state.sidebarCollapsed ? "Expand sidebar" : "Veda"}" aria-label="${state.sidebarCollapsed ? "Expand sidebar" : "Veda"}">${logo()}<span class="wordmark-label">Veda</span></button>
      <button class="icon-button" id="collapseSidebar" title="Collapse sidebar" aria-label="Collapse sidebar">${icon("panel")}</button>
    </div>
    <nav class="primary-nav" aria-label="Primary">
      <button class="nav-item" id="newChat" title="New chat">${icon("plus")}<span class="nav-label">New chat</span></button>
      ${navItem("chat", "Chats", "chat")}
      ${navItem("docs", "Docs", "book")}
      ${navItem("downloads", "Downloads", "download", activeDownloads)}
    </nav>
    ${renderHistory()}
    <div class="side-bottom">
      <button class="nav-item" id="settingsButton" title="Settings">${icon("settings")}<span class="nav-label">Settings</span></button>
    </div>
  </aside>`;
}

function renderTopbar(): string {
  const chat = activeChat();
  const title =
    state.view === "chat"
      ? chat.messages.length
        ? chat.title
        : "New chat"
      : state.view === "docs"
        ? "Documentation library"
        : "Downloads & storage";
  return `<header class="topbar" data-key="topbar">
    <div class="page-title">${escapeHtml(title)}</div>
    <div class="topbar-actions">
      <button class="icon-button" id="themeToggle" title="Switch to ${state.theme === "dark" ? "light" : "dark"} mode" aria-label="Switch to ${state.theme === "dark" ? "light" : "dark"} mode">${icon(state.theme === "dark" ? "moon" : "sun")}</button>
    </div>
  </header>`;
}

function attachmentChips(): string {
  if (!state.attachments.length) return "";
  return `<div class="attachment-row">${state.attachments
    .map(
      (file) =>
        `<span class="attachment-chip" data-key="att-${file.id}">${icon("file")}<span>${escapeHtml(file.name)}</span><button class="icon-button remove-attachment" data-attachment="${escapeHtml(file.id)}" title="Remove ${escapeHtml(file.name)}" aria-label="Remove ${escapeHtml(file.name)}">${icon("x")}</button></span>`,
    )
    .join("")}</div>`;
}

const modeLabel = (mode: ReasoningMode) => (mode === "think" ? "Think" : "Fast");

function renderModeMenu(): string {
  const option = (mode: ReasoningMode, detail: string) => {
    const selected = state.mode === mode;
    return `<button class="popover-item${selected ? " selected" : ""}" data-mode="${mode}" role="menuitemradio" aria-checked="${selected}">
      <span class="popover-item-copy">${modeLabel(mode)}<small>${detail}</small></span>
      <span class="popover-item-check">${selected ? icon("check") : ""}</span>
    </button>`;
  };
  return `<div class="popover mode-popover" id="modelPopover" role="menu" aria-label="Reasoning mode">
    <div class="popover-title">MiniCPM 5 mode</div>
    ${option("fast", "Direct answers · lower latency")}
    ${option("think", "Deeper reasoning · more tokens")}
  </div>`;
}

function renderScopeMenu(): string {
  const installed = state.docsets.filter(isDocInstalled);
  const body = installed.length
    ? installed
        .map(
          (doc) =>
            `<div class="popover-item static" data-key="scope-${doc.id}"><span class="popover-item-icon" style="color:${doc.accent}">${icon("book")}</span><span class="popover-item-copy">${escapeHtml(doc.name)}<small>${doc.pages ? `${doc.pages.toLocaleString()} pages` : "Installed"}</small></span></div>`,
        )
        .join("")
    : `<div class="popover-empty">No documentation installed yet.</div>`;
  return `<div class="popover scope-popover" id="scopePopover" role="menu" aria-label="Documentation in scope">
    <div class="popover-title">Docs in scope</div>
    ${body}
    <button class="popover-item action" data-view="docs" role="menuitem"><span class="popover-item-copy">Manage documentation…</span></button>
  </div>`;
}

function renderComposer(): string {
  const installed = state.docsets.filter(isDocInstalled).length;
  const busy = isBusy();
  return `<div class="composer-wrap" data-key="composer">
    <div class="composer-shell">
      ${attachmentChips()}
      <div class="composer">
        <textarea id="composerInput" rows="1" placeholder="Ask your offline docs." aria-label="Message"></textarea>
        <div class="composer-row">
          <div class="composer-left">
            <button class="tool-button" id="attachButton" title="Attach code" aria-label="Attach code">${icon("paperclip")}</button>
            <input id="fileInput" type="file" multiple hidden accept=".py,.pyi,.c,.h,.cc,.cpp,.cxx,.hpp,.html,.css,.js,.mjs,.cjs,.ts,.tsx,.jsx,.json,.md,.txt" />
            <div class="menu-anchor">
              <button class="scope-button" id="scopeButton" title="Documentation in scope" aria-expanded="${state.scopeOpen}" aria-haspopup="menu">${icon("layers")} ${installed || "No"} docsets ${icon("chevron")}</button>
              ${state.scopeOpen ? renderScopeMenu() : ""}
            </div>
          </div>
          <div class="composer-right">
            <div class="menu-anchor">
              <button class="model-button mode-${state.mode}" id="modelButton" aria-expanded="${state.modelOpen}" aria-haspopup="menu" title="Reasoning mode: ${modeLabel(state.mode)}"><span class="model-name">MiniCPM 5${state.mode === "think" ? " Think" : ""}</span>${icon("chevron")}</button>
              ${state.modelOpen ? renderModeMenu() : ""}
            </div>
            ${busy
              ? `<button class="send-button stopping" id="stopButton" title="Stop generating" aria-label="Stop generating">${icon("stop")}</button>`
              : `<button class="send-button" id="sendButton" title="Send" aria-label="Send message">${icon("arrowUp")}</button>`}
          </div>
        </div>
      </div>
    </div>
  </div>`;
}

function renderEmptyChat(): string {
  return `<section class="chat-view" data-key="chat-view">
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
    ? `<div class="message-sources">${message.sources
        .map(
          (source) =>
            `<button class="source-chip" data-source="${escapeHtml(source.url)}" title="Open ${escapeHtml(source.title)}, ${escapeHtml(source.section)}"><span class="source-n">${escapeHtml(source.id)}</span>${escapeHtml(source.docset)} · ${escapeHtml(source.section)}</button>`,
        )
        .join("")}</div>`
    : "";
  const attachments = message.attachments?.length
    ? `<div class="attachment-row">${message.attachments
        .map((file) => `<span class="attachment-chip">${icon("file")}<span>${escapeHtml(file.name)}</span></span>`)
        .join("")}</div>`
    : "";
  const note = message.stopped
    ? `<div class="message-note">Stopped by you.</div>`
    : message.failed
      ? `<div class="message-note error">This request did not finish.</div>`
      : "";
  return `<article class="message ${message.role}${messageEnterClass(message.id)}" data-key="msg-${message.id}" data-message-id="${message.id}">
    <div class="message-avatar">${message.role === "assistant" ? logo() : "A"}</div>
    <div class="message-main">
      <div class="message-head">${message.role === "assistant" ? "Veda" : "You"}<span class="message-time">${timeLabel(message.createdAt)}</span></div>
      ${attachments}
      <div class="message-body">${renderMarkdown(message.content)}${message.streaming ? '<span class="stream-caret"></span>' : ""}</div>
      ${note}
      ${sourceMarkup}
    </div>
  </article>`;
}

function renderChat(): string {
  const chat = activeChat();
  if (!chat.messages.length) return renderEmptyChat();
  return `<section class="chat-view" data-key="chat-view">
    <div class="messages" id="messagesScroller"><div class="message-list">${chat.messages.map(renderMessage).join("")}</div></div>
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
  if (doc.state === "downloading" || doc.state === "indexing")
    return `<div class="progress-track"><div class="progress-value" style="width:${doc.progress}%"></div></div><span class="download-state">${doc.state} ${Math.round(doc.progress)}%</span>`;
  return `<span></span><button class="button primary install-doc" data-docset="${doc.id}">${icon("download")} Download</button>`;
}

function renderDocs(): string {
  const installed = state.docsets.filter(isDocInstalled);
  const indexedPages = installed.reduce((sum, doc) => sum + (doc.pages ?? 0), 0);
  let body: string;
  if (state.docsetsError) {
    body = `<div class="empty-list error-state">
      <p>Documentation could not be loaded.</p>
      <p class="empty-detail">${escapeHtml(state.docsetsError)}</p>
      <button class="button primary" id="retryDocsets">Try again</button>
    </div>`;
  } else if (!state.docsets.length) {
    body = `<div class="empty-list">Loading documentation…</div>`;
  } else {
    body = `<div class="doc-grid${contentEnterClass()}">${state.docsets
      .map(
        (doc) => `<article class="doc-card" data-key="doc-${doc.id}" data-doc-filter="${escapeHtml(`${doc.name} ${doc.detail} ${doc.version}`.toLowerCase())}" style="--doc-color:${doc.accent}">
      <div class="doc-head"><div class="doc-icon">${escapeHtml(doc.initials)}</div><div class="doc-copy"><div class="doc-name">${escapeHtml(doc.name)}</div><div class="doc-version">${escapeHtml(doc.version)}</div></div></div>
      <div class="doc-description">${escapeHtml(doc.detail)}</div>
      <div class="doc-meta"><span>${bytes(doc.compressedBytes)} download</span>${doc.pages !== undefined ? `<span>${doc.pages.toLocaleString()} installed pages</span>` : ""}</div>
      <div class="doc-footer">${docAction(doc)}</div>
    </article>`,
      )
      .join("")}</div>`;
  }
  return `<section class="content-view" data-key="docs-view"><div class="content-inner">
    <div class="content-header">
      <div><h1>Docs</h1><p>Install, update, or remove documentation.</p></div>
      <label class="search-box">${icon("search")}<input id="docSearch" placeholder="Filter documentation" aria-label="Filter documentation" /></label>
    </div>
    <div class="library-summary">${indexedPages.toLocaleString()} installed pages across ${installed.length} ${installed.length === 1 ? "docset" : "docsets"}</div>
    ${body}
  </div></section>`;
}

function renderDownloads(): string {
  let rows: string;
  if (state.downloadsError) {
    rows = `<div class="empty-list error-state">
      <p>Downloads could not be loaded.</p>
      <p class="empty-detail">${escapeHtml(state.downloadsError)}</p>
      <button class="button primary" id="retryDownloads">Try again</button>
    </div>`;
  } else if (!state.downloads.length) {
    rows = `<div class="empty-list">No downloads yet.</div>`;
  } else {
    rows = state.downloads
      .map((item) => {
        const indexing = item.id.endsWith("-index") || item.state === "indexing";
        const progressCopy = indexing
          ? `${Math.round(item.downloadedBytes).toLocaleString()} of ${Math.round(item.totalBytes).toLocaleString()} sections`
          : `${bytes(item.downloadedBytes)} of ${bytes(item.totalBytes)}${item.speedBytes ? ` · ${bytes(item.speedBytes)}/s` : ""}`;
        return `<div class="download-row" data-key="dl-${item.id}">
      <div class="download-copy"><div class="download-name">${escapeHtml(item.name)}</div><div class="download-detail">${escapeHtml(item.detail)}</div></div>
      <div><div class="progress-track"><div class="progress-value" style="width:${item.progress}%"></div></div><div class="download-progress-copy">${progressCopy}</div></div>
      <div class="download-state">${item.state === "installed" ? "Ready" : escapeHtml(item.state)}</div>
    </div>`;
      })
      .join("");
  }
  return `<section class="content-view" data-key="downloads-view"><div class="content-inner">
    <div class="content-header"><div><h1>Downloads</h1><p>Manage downloaded files.</p></div><button class="button" id="dataFolder">${icon("folder")} Data folder</button></div>
    <div class="download-list">${rows}</div>
  </div></section>`;
}

function renderSettings(): string {
  const entering = enterClass("settings", state.settingsOpen);
  if (!state.settingsOpen) return "";
  const recommended = state.preflight?.recommendedContext;
  const modelInstalled = state.downloads.some((item) => item.id.startsWith("minicpm5-") && item.state === "installed");
  return `<div class="modal-backdrop${entering}" id="settingsBackdrop" data-key="settings"><section class="settings-panel" role="dialog" aria-modal="true" aria-labelledby="settingsTitle">
    <header class="settings-header"><h2 id="settingsTitle">Settings</h2><button class="icon-button" id="closeSettings" aria-label="Close settings">${icon("x")}</button></header>
    <div class="settings-body">
      <div class="setting-row"><div class="setting-copy"><div class="setting-name">Appearance</div><div class="setting-detail">${state.theme === "dark" ? "Dark" : "Light"}</div></div><button class="button" id="settingsTheme">Change</button></div>
      ${modelInstalled ? "" : `<div class="setting-row"><div class="setting-copy"><div class="setting-name">Setup</div><div class="setting-detail">Model files are not installed yet.</div></div><button class="button primary" id="settingsSetup">Set up Veda</button></div>`}
      <div class="setting-row"><div class="setting-copy"><div class="setting-name">Model</div><div class="setting-detail">MiniCPM 5 · ${state.selectedQuant.toUpperCase()}</div></div></div>
      <div class="setting-row"><div class="setting-copy"><div class="setting-name">Reasoning</div><div class="setting-detail">${modeLabel(state.mode)}</div></div><button class="button" id="settingsMode">Change</button></div>
      <div class="setting-block">
        <div class="setting-copy">
          <div class="setting-name">Context window</div>
          <div class="setting-detail" id="contextDetail">${contextLabel(state.contextTokens)}${state.contextTokens <= 0 && recommended ? ` · ${recommended.toLocaleString()} on this device` : ""}</div>
        </div>
        <div class="context-control">
          <input type="range" id="contextRange" class="range" min="0" max="${CONTEXT_MAX}" step="${CONTEXT_STEP}" value="${state.contextTokens}" aria-label="Context window tokens" />
          <input type="number" id="contextNumber" class="number-input" min="0" max="${CONTEXT_MAX}" step="${CONTEXT_STEP}" value="${state.contextTokens}" aria-label="Context window tokens" />
        </div>
        <div class="context-scale"><span>0 · Auto</span><span>${(CONTEXT_MAX / 1024).toFixed(0)}K</span></div>
      </div>
      <div class="setting-row"><div class="setting-copy"><div class="setting-name">Files</div><div class="setting-detail">Open Veda's data folder</div></div><button class="button" id="settingsDataFolder">Open</button></div>
      <div class="setting-row"><div class="setting-copy"><div class="setting-name">Chat history</div><div class="setting-detail">${state.chats.filter((chat) => chat.messages.length).length} saved on this device</div></div><button class="button danger" id="clearHistory">Clear</button></div>
      <div class="setting-row"><div class="setting-copy"><div class="setting-name">Veda</div><div class="setting-detail">Version 0.1.0</div></div></div>
    </div>
  </section></div>`;
}

function checkRow(kind: "memory" | "drive" | "chip", name: string, detail: string, value: string, status: "ok" | "warn" | "fail"): string {
  return `<div class="preflight-row"><div class="check-icon ${status === "ok" ? "" : status}">${icon(status === "ok" ? "check" : kind)}</div><div class="preflight-copy"><div class="preflight-name">${name}</div><div class="preflight-detail">${detail}</div></div><div class="preflight-value">${value}</div></div>`;
}

function renderOnboardingBody(): string {
  const report = state.preflight;
  if (state.setupError) {
    return `<div class="onboarding-kicker">Setup stopped</div><h1>Something went wrong</h1>
      <p class="onboarding-lead">The download or import stopped before it finished. Files already downloaded are kept, so retrying resumes where it stopped. You can also go back and choose a smaller model or fewer documentation packs.</p>
      <p class="onboarding-lead setup-error">${escapeHtml(state.setupError)}</p>`;
  }
  if (state.setupRunning) {
    return `<div class="onboarding-kicker">Setup</div><h1>Preparing Veda</h1><p class="onboarding-lead">Keep Veda open until setup finishes.</p>
      <div class="setup-progress"><div class="progress-track"><div class="progress-value" id="setupProgressBar" style="width:${state.setupProgress}%"></div></div><div class="setup-progress-copy" id="setupProgressText">${escapeHtml(state.setupStatus)}</div></div>`;
  }
  if (state.setupStep === 0) {
    const hasFailures = Boolean(report?.hardFailures.length);
    const ramStatus = hasFailures && report?.hardFailures.some((value) => value.toLowerCase().includes("memory")) ? "fail" : "ok";
    const diskStatus = hasFailures && report?.hardFailures.some((value) => value.toLowerCase().includes("disk")) ? "fail" : "ok";
    const warnings = report?.warnings ?? [];
    return `<div class="onboarding-kicker">System check</div><h1>Check this device</h1><p class="onboarding-lead">Before downloading, Veda checks available memory and storage. At least 10 GB of free SSD space is required.</p>
      <div class="preflight-list">
        ${report ? checkRow("memory", "Memory and context", `Recommended context: ${report.recommendedContext.toLocaleString()} tokens`, bytes(report.totalMemoryBytes), ramStatus) : checkRow("memory", "Memory and context", "Checking available RAM…", "—", "warn")}
        ${report ? checkRow("drive", "Fast local storage", "10 GB minimum free space", `${bytes(report.freeDiskBytes)} · ${report.diskKind.toUpperCase()}`, diskStatus) : checkRow("drive", "Fast local storage", "Checking disk and free space…", "—", "warn")}
        ${report ? checkRow("chip", "Native runtime", `${report.operatingSystem} · ${report.architecture}`, "Auto-detect", "ok") : checkRow("chip", "Native runtime", "Finding the best llama.cpp build…", "—", "warn")}
      </div>
      ${warnings.length ? `<div class="preflight-warnings">${warnings.map((warning) => `<div class="preflight-warning">${icon("chip")}${escapeHtml(warning)}</div>`).join("")}</div>` : ""}`;
  }
  if (state.setupStep === 1) {
    const q8Disabled = !q8Supported();
    const totalGiB = state.preflight ? `${Math.round(state.preflight.totalMemoryBytes / GIB_BYTES)} GB` : "this device";
    const card = (quant: "q5" | "q8", name: string, detail: string, meta: string[], disabled: boolean) =>
      `<button class="option-card${state.selectedQuant === quant ? " selected" : ""}" data-quant="${quant}" ${disabled ? 'disabled aria-disabled="true"' : ""}><div class="option-name">${name}</div><div class="option-detail">${detail}</div><div class="option-meta">${meta.map((tag) => `<span class="meta-tag">${tag}</span>`).join("")}${disabled ? '<span class="meta-tag">Not available</span>' : ""}</div></button>`;
    return `<div class="onboarding-kicker">MiniCPM 5</div><h1>Choose a model size</h1><p class="onboarding-lead">Q5 is the default on every device. Q8 uses more memory and provides slightly higher fidelity.</p>
      <div class="option-grid">
        ${card("q5", "Q5", "Uses less memory · the default choice.", ["751 MiB", "6 GB RAM"], false)}
        ${card("q8", "Q8", q8Disabled ? "Needs 12 GB of RAM to run." : "Uses more memory.", ["1.07 GiB", "12 GB RAM"], q8Disabled)}
      </div>
      ${q8Disabled ? `<p class="option-note">Q8 requires 12 GB of physical memory; ${totalGiB} is not enough, so it is disabled. Q5 is selected automatically.</p>` : ""}`;
  }
  const total = state.docsets.filter((doc) => state.setupDocsets.has(doc.id)).reduce((sum, doc) => sum + doc.compressedBytes, 0);
  return `<div class="onboarding-kicker">Documentation</div><h1>Choose documentation</h1><p class="onboarding-lead">Download only what you need. You can change this later.</p>
    <div class="setup-docs">${state.docsets
      .map(
        (doc) =>
          `<button class="setup-doc${state.setupDocsets.has(doc.id) ? " selected" : ""}" data-setup-doc="${doc.id}"><div class="setup-doc-abbr">${escapeHtml(doc.initials)}</div><div class="setup-doc-name">${escapeHtml(doc.name)}</div></button>`,
      )
      .join("")}</div>
    <div class="setup-summary">${state.setupDocsets.size} packs · ${bytes(total)} download</div>`;
}

function renderOnboarding(): string {
  const entering = enterClass("onboarding", state.onboardingOpen);
  if (!state.onboardingOpen) return "";
  const hasFailures = Boolean(state.preflight?.hardFailures.length);
  const last = state.setupStep === 2;
  const progressMode = state.setupRunning || Boolean(state.setupError);
  const footer = state.setupError
    ? `<div class="onboarding-note">Downloaded files are kept, so retrying will resume where possible.</div><div class="button-row"><button class="button" id="setupCancelError">Back</button><button class="button primary" id="setupRetry">Retry</button></div>`
    : state.setupRunning
      ? `<div class="onboarding-note">Setup must finish before the rest of the app can be used.</div>`
      : `<div class="onboarding-note">You can change these options later in Settings.</div><div class="button-row">${state.setupStep > 0 ? '<button class="button" id="setupBack">Back</button>' : ""}<button class="button" id="setupSkip">Skip for now</button><button class="button primary" id="setupNext" ${(hasFailures && state.setupStep === 0) || (last && state.setupDocsets.size === 0) ? "disabled" : ""}>${last ? "Set up Veda" : "Continue"}</button></div>`;
  return `<div class="modal-backdrop setup-backdrop${entering}" data-key="onboarding"><section class="onboarding" role="dialog" aria-modal="true" aria-labelledby="setupTitle">
    <div class="onboarding-top"><div class="onboarding-brand">${logo()} Veda</div>${progressMode ? "" : `<div class="step-dots">${[0, 1, 2].map((step) => `<span class="step-dot${state.setupStep === step ? " active" : ""}"></span>`).join("")}</div>`}</div>
    <div class="onboarding-body${entering}" id="setupTitle">${renderOnboardingBody()}</div>
    <div class="onboarding-bottom">${footer}</div>
  </section></div>`;
}

function renderReader(): string {
  const open = Boolean(state.reader) || state.readerLoading;
  const entering = enterClass("reader", open);
  if (!open) return "";
  const body = state.reader
    ? `<article class="setting-section message-body">${renderMarkdown(state.reader.text)}</article>
       <div class="reader-origin">Local excerpt · ${escapeHtml(state.reader.url)}</div>`
    : `<div class="empty-list">Opening source…</div>`;
  return `<div class="modal-backdrop${entering}" id="readerBackdrop" data-key="reader"><section class="settings-panel reader-panel" role="dialog" aria-modal="true" aria-labelledby="readerTitle">
    <header class="settings-header"><div><h2 id="readerTitle">${escapeHtml(state.reader?.title ?? "Source")}</h2><div class="setting-detail">${escapeHtml(state.reader ? `${state.reader.docset} · ${state.reader.section}` : "")}</div></div><button class="icon-button" id="closeReader" aria-label="Close source">${icon("x")}</button></header>
    <div class="settings-body">${body}</div>
  </section></div>`;
}

// Toasts animate only the first time each toast appears. Re-renders (setup
// progress, downloads, theme changes) must not replay the entrance.
const enteredToasts = new Set<string>();
function renderToasts(): string {
  return `<div class="toast-stack" data-key="toasts">${state.toasts
    .map((toast) => {
      const isNew = !enteredToasts.has(toast.id);
      enteredToasts.add(toast.id);
      return `<div class="toast${isNew ? " enter" : ""}" data-key="toast-${toast.id}"><span class="status-dot"></span>${escapeHtml(toast.text)}</div>`;
    })
    .join("")}</div>`;
}

// Messages animate in the first time they appear. The set is consulted during
// rendering, so a full re-render never replays the animation.
const enteredMessages = new Set<string>();
function messageEnterClass(id: string): string {
  if (enteredMessages.has(id)) return "";
  enteredMessages.add(id);
  return " message-enter";
}

// Modals and popovers only animate when they first open. Step changes,
// progress updates and other re-renders must not replay the entrance.
// The onboarding surface is part of the initial cold open, so when it starts
// visible it is treated as already presented and does not animate.
const modalEnterState: Record<string, boolean> = { onboarding: state.onboardingOpen };
function enterClass(kind: string, open: boolean): string {
  const entering = open && !modalEnterState[kind];
  modalEnterState[kind] = open;
  return entering ? " entering" : "";
}

// Content blocks that should animate on view entry, but never again.
let animateContent = false;
function contentEnterClass(): string {
  if (!animateContent) return "";
  animateContent = false;
  return " enter";
}

// The composer's draft belongs to the user, not to the render pass. Morphing
// preserves the live textarea node, but the draft is still tracked so a new
// chat can clear it deliberately.
let composerDraft = "";

// ---------------------------------------------------------------------------
// Rendering is a keyed morph, not an innerHTML swap. Nodes survive across
// renders, so CSS transitions actually interpolate, entrance animations do not
// replay (no flash), and focus/scroll/selection are retained.
// ---------------------------------------------------------------------------
function render(): void {
  try {
    const view = state.view === "chat" ? renderChat() : state.view === "docs" ? renderDocs() : renderDownloads();
    morph(
      app,
      `<div class="app-shell${state.sidebarCollapsed ? " sidebar-collapsed" : ""}" data-key="shell">${renderSidebar()}<main class="main">${renderTopbar()}<div class="view">${view}</div></main></div>${renderSettings()}${renderReader()}${renderOnboarding()}${renderToasts()}`,
    );
    syncComposer();
    applyDocFilter();
    focusPendingInput();
    if (activeChat().messages.length) requestAnimationFrame(scrollIfNewContent);
  } catch (error) {
    // A rendering failure must never silently kill the app.
    console.error("Veda render failed:", error);
  }
}

function syncComposer(): void {
  const composer = document.querySelector<HTMLTextAreaElement>("#composerInput");
  if (!composer) return;
  if (composer.value !== composerDraft) composer.value = composerDraft;
  autoGrow(composer);
}

// The docs filter is applied straight to the DOM so typing stays snappy. A
// later re-render (a download progress event, a theme toggle, …) rebuilds the
// cards, which would silently clear the filtering; re-applying after every
// render keeps the query and the visible cards in step.
function applyDocFilter(): void {
  const input = document.querySelector<HTMLInputElement>("#docSearch");
  if (!input) return;
  const query = input.value.trim().toLowerCase();
  document.querySelectorAll<HTMLElement>("[data-doc-filter]").forEach((card) => {
    card.hidden = Boolean(query) && !(card.dataset.docFilter ?? "").includes(query);
  });
}

function autoGrow(element: HTMLTextAreaElement): void {
  element.style.height = "auto";
  element.style.height = `${Math.min(element.scrollHeight, 190)}px`;
}

// A newly revealed rename field should receive focus exactly once.
let focusTarget: string | undefined;
function focusPendingInput(): void {
  if (!focusTarget) return;
  const element = document.querySelector<HTMLInputElement>(focusTarget);
  focusTarget = undefined;
  if (!element) return;
  element.focus();
  element.select();
}

let renderQueued = false;
function scheduleRender(): void {
  if (renderQueued) return;
  renderQueued = true;
  // Coalescing on a microtask keeps rapid progress events cheap without
  // introducing the visible lag a timeout caused.
  queueMicrotask(() => {
    renderQueued = false;
    render();
  });
}

function setTheme(theme: Theme): void {
  state.theme = theme;
  document.documentElement.dataset.theme = theme;
  storageSet("veda:theme", theme);
  // Everything else is derived from state by the renderer. Patching individual
  // nodes by hand is what previously left the DOM inconsistent with state
  // after a light/dark round trip.
  render();
}

const toggleTheme = () => setTheme(state.theme === "dark" ? "light" : "dark");

function toast(message: string): void {
  const entry = { id: newId(), text: message };
  state.toasts.push(entry);
  render();
  window.setTimeout(() => {
    state.toasts = state.toasts.filter((item) => item.id !== entry.id);
    enteredToasts.delete(entry.id);
    render();
  }, 3200);
}

function scrollMessages(): void {
  const scroller = document.querySelector<HTMLElement>("#messagesScroller");
  if (scroller) scroller.scrollTop = scroller.scrollHeight;
}

// Auto-scroll only follows new content (a message being added or the
// assistant answer growing). Re-renders caused by theme toggles, menus or
// downloads must not yank the user back to the bottom of the chat.
let lastScrollSignal = "";
function messageScrollSignal(): string {
  const chat = activeChat();
  const last = chat.messages[chat.messages.length - 1];
  return `${chat.id}:${chat.messages.length}:${last?.content.length ?? 0}`;
}
function scrollIfNewContent(): void {
  const signal = messageScrollSignal();
  if (signal === lastScrollSignal) return;
  lastScrollSignal = signal;
  scrollMessages();
}

function closeMenus(): boolean {
  const wasOpen = state.modelOpen || state.scopeOpen || Boolean(state.menuChatId);
  state.modelOpen = false;
  state.scopeOpen = false;
  state.menuChatId = undefined;
  return wasOpen;
}

function setView(view: View): void {
  closeMenus();
  const changed = state.view !== view;
  state.view = view;
  // The docs grid animates when navigation actually enters it; clicking the
  // already-active tab must not replay anything.
  animateContent = changed && view === "docs";
  render();
}

// ---------------------------------------------------------------------------
// Chat store operations
// ---------------------------------------------------------------------------

function persistChats(): void {
  saveChats(state.chats);
}

function openChat(id: string): void {
  closeMenus();
  state.renamingChatId = undefined;
  if (state.activeChatId !== id) {
    state.activeChatId = id;
    composerDraft = "";
  }
  state.view = "chat";
  render();
}

function startNewChat(): void {
  closeMenus();
  state.renamingChatId = undefined;
  // Reuse an existing untouched chat instead of stacking up empty ones.
  const empty = state.chats.find((chat) => chat.messages.length === 0);
  const chat = empty ?? createChat();
  if (!empty) state.chats.unshift(chat);
  state.activeChatId = chat.id;
  state.attachments = [];
  composerDraft = "";
  state.view = "chat";
  render();
}

function deleteChat(id: string): void {
  const chat = state.chats.find((entry) => entry.id === id);
  if (!chat) return;
  // A conversation still being answered is stopped first so no reply can be
  // written back into a chat the user has deleted.
  pending.get(id)?.controller.abort();
  pending.delete(id);
  state.chats = state.chats.filter((entry) => entry.id !== id);
  if (!state.chats.length) state.chats = [createChat()];
  if (state.activeChatId === id) {
    state.activeChatId = state.chats[0].id;
    composerDraft = "";
  }
  closeMenus();
  state.renamingChatId = undefined;
  persistChats();
  render();
  toast("Chat deleted.");
}

function renameChat(id: string, title: string): void {
  const chat = state.chats.find((entry) => entry.id === id);
  if (chat) {
    const clean = title.replace(/\s+/g, " ").trim();
    chat.title = clean ? clean.slice(0, 80) : chat.title;
    persistChats();
  }
  state.renamingChatId = undefined;
  render();
}

// ---------------------------------------------------------------------------
// Resource loading
// ---------------------------------------------------------------------------

async function loadDocsets(): Promise<void> {
  try {
    state.docsets = await bridge.docsets();
    state.docsetsError = undefined;
  } catch (error) {
    state.docsetsError = errorText(error);
  }
}

async function loadDownloads(): Promise<void> {
  try {
    state.downloads = await bridge.downloads();
    state.downloadsError = undefined;
  } catch (error) {
    state.downloadsError = errorText(error);
  }
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
  render();
  try {
    await bridge.installDocset(id);
    await Promise.all([loadDocsets(), loadDownloads()]);
    state.setupRunning = false;
    state.onboardingOpen = false;
    render();
  } catch (error) {
    state.setupError = errorText(error);
    state.setupRunning = false;
    state.setupFailureStep = 2;
    render();
  }
}

function updateSetupProgress(status: string, progress = 0): void {
  state.setupStatus = status;
  state.setupProgress = progress;
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
  render();
  try {
    await bridge.removeDocset(id);
    await Promise.all([loadDocsets(), loadDownloads()]);
    state.setupRunning = false;
    state.onboardingOpen = false;
    render();
  } catch (error) {
    state.setupRunning = false;
    state.onboardingOpen = false;
    render();
    toast(`Could not remove ${doc.name}: ${errorText(error)}`);
  }
}

async function runSetup(): Promise<void> {
  state.onboardingOpen = true;
  state.setupRunning = true;
  state.setupError = undefined;
  state.setupProgress = 0;
  state.setupStatus = "Preparing model files…";
  let modelPrepared = false;
  render();
  try {
    await bridge.prepareResources(state.selectedQuant);
    modelPrepared = true;
    for (const id of state.setupDocsets) {
      const doc = state.docsets.find((item) => item.id === id);
      if (doc?.state === "installed") continue;
      updateSetupProgress(`Preparing ${doc?.name ?? id}…`);
      render();
      await bridge.installDocset(id);
    }
    await Promise.all([loadDocsets(), loadDownloads()]);
    storageSet("veda:onboarded", "true");
    storageRemove("veda:onboarding-skipped");
    state.setupRunning = false;
    state.onboardingOpen = false;
    render();
  } catch (error) {
    state.setupError = errorText(error);
    state.setupRunning = false;
    // "Back" from the error screen returns to the step that actually failed:
    // the model choice when resources could not be prepared, otherwise the
    // documentation step.
    state.setupFailureStep = modelPrepared ? 2 : 1;
    render();
  }
}

async function addFiles(files: FileList): Promise<void> {
  const maxBytes = 512 * 1024;
  let added = false;
  for (const file of Array.from(files).slice(0, 8)) {
    if (file.size > maxBytes) {
      toast(`${file.name} is larger than the 512 KB attachment limit.`);
      continue;
    }
    const content = await file.text();
    const language = file.name.split(".").pop()?.toLowerCase() ?? "text";
    state.attachments.push({ id: newId(), name: file.name, bytes: file.size, language, content });
    added = true;
  }
  if (added) render();
}

// ---------------------------------------------------------------------------
// Asking
// ---------------------------------------------------------------------------

async function sendMessage(): Promise<void> {
  const chat = activeChat();
  if (isBusy(chat.id)) return;
  const input = document.querySelector<HTMLTextAreaElement>("#composerInput");
  const text = (input?.value ?? composerDraft).trim();
  if (!text) return;

  const attachments = state.attachments.map((file) => ({ ...file }));
  const chatId = chat.id;
  chat.messages.push({ id: newId(), role: "user", content: text, createdAt: Date.now(), attachments });
  if (chat.title === "New chat") chat.title = deriveTitle(text);
  const assistantId = newId();
  chat.messages.push({ id: assistantId, role: "assistant", content: "Searching installed docs…", createdAt: Date.now(), streaming: true });
  chat.updatedAt = Date.now();

  state.attachments = [];
  composerDraft = "";
  if (input) input.value = "";

  const controller = new AbortController();
  pending.set(chatId, { chatId, messageId: assistantId, controller });
  persistChats();
  render();

  try {
    const response = await bridge.ask({
      chatId,
      message: text,
      mode: state.mode,
      docsets: state.docsets.filter(isDocInstalled).map((doc) => doc.id),
      attachments,
      contextTokens: state.contextTokens,
      modelQuant: state.selectedQuant,
      signal: controller.signal,
    });
    // The reply is written into the store, so it lands even if the user is on
    // another view or in a different conversation.
    applyReply(chatId, assistantId, (message) => {
      message.content = response.content;
      message.sources = response.sources;
      message.streaming = false;
    });
  } catch (error) {
    if (isAbort(error)) {
      applyReply(chatId, assistantId, (message) => {
        message.content = message.content === "Searching installed docs…" ? "Stopped before an answer was produced." : message.content;
        message.streaming = false;
        message.stopped = true;
      });
    } else {
      applyReply(chatId, assistantId, (message) => {
        message.content = `Veda could not complete the local request. ${errorText(error)}`;
        message.streaming = false;
        message.failed = true;
      });
    }
  } finally {
    // Busy is always released, whatever happened, so the composer can never
    // be left permanently disabled.
    if (pending.get(chatId)?.messageId === assistantId) pending.delete(chatId);
    persistChats();
    render();
  }
}

/** Applies a reply to a message that may belong to a background conversation. */
function applyReply(chatId: string, messageId: string, mutate: (message: ChatMessage) => void): void {
  const chat = state.chats.find((entry) => entry.id === chatId);
  const message = chat?.messages.find((entry) => entry.id === messageId);
  if (!chat || !message) return;
  mutate(message);
  chat.updatedAt = Date.now();
}

function stopGenerating(): void {
  const request = pending.get(state.activeChatId);
  if (!request) return;
  request.controller.abort();
  // The pending entry is cleared by sendMessage's finally block.
}

async function openSource(url: string): Promise<void> {
  state.readerLoading = true;
  state.reader = undefined;
  render();
  try {
    state.reader = await bridge.readSource(url);
  } catch (error) {
    toast(`Could not open source: ${errorText(error)}`);
  } finally {
    state.readerLoading = false;
    render();
  }
}

function setContext(value: number): void {
  const clamped = Number.isFinite(value) ? Math.min(CONTEXT_MAX, Math.max(0, Math.round(value))) : 0;
  state.contextTokens = clamped;
  storageSet("veda:context", String(clamped));
}

// ---------------------------------------------------------------------------
// Events
//
// Delegated once at the root. Morphing keeps nodes alive across renders, and
// delegation means no listener is ever bound twice or lost after a re-render.
// ---------------------------------------------------------------------------

function closest(target: EventTarget | null, selector: string): HTMLElement | null {
  return target instanceof Element ? target.closest<HTMLElement>(selector) : null;
}

function bindGlobalEvents(): void {
  app.addEventListener("click", (event) => {
    const target = event.target;

    // Any click outside an open menu dismisses it first.
    const insideMenu = closest(target, ".menu-anchor, .recent-wrap");
    if (!insideMenu && closeMenus()) render();

    const view = closest(target, "[data-view]");
    if (view) {
      setView(view.dataset.view as View);
      return;
    }

    const chatButton = closest(target, "[data-chat]");
    if (chatButton) {
      openChat(chatButton.dataset.chat ?? "");
      return;
    }
    const chatMenu = closest(target, "[data-chat-menu]");
    if (chatMenu) {
      const id = chatMenu.dataset.chatMenu ?? "";
      state.menuChatId = state.menuChatId === id ? undefined : id;
      render();
      return;
    }
    const rename = closest(target, "[data-chat-rename]");
    if (rename) {
      state.renamingChatId = rename.dataset.chatRename;
      state.menuChatId = undefined;
      focusTarget = "#renameInput";
      render();
      return;
    }
    const remove = closest(target, "[data-chat-delete]");
    if (remove) {
      const id = remove.dataset.chatDelete ?? "";
      const chat = state.chats.find((entry) => entry.id === id);
      state.menuChatId = undefined;
      if (chat && window.confirm(`Delete "${chat.title}"? This cannot be undone.`)) deleteChat(id);
      else render();
      return;
    }

    const mode = closest(target, "[data-mode]");
    if (mode) {
      state.mode = mode.dataset.mode as ReasoningMode;
      storageSet("veda:mode", state.mode);
      state.modelOpen = false;
      render();
      return;
    }

    const attachment = closest(target, ".remove-attachment");
    if (attachment) {
      state.attachments = state.attachments.filter((file) => file.id !== attachment.dataset.attachment);
      render();
      return;
    }

    const install = closest(target, ".install-doc");
    if (install) {
      void installDocsetBlocking(install.dataset.docset ?? "");
      return;
    }
    const removeDoc = closest(target, ".remove-doc");
    if (removeDoc) {
      void removeDocsetBlocking(removeDoc.dataset.docset ?? "");
      return;
    }
    const source = closest(target, "[data-source]");
    if (source) {
      void openSource(source.dataset.source ?? "");
      return;
    }
    const quant = closest(target, "[data-quant]");
    if (quant) {
      // A disabled card (Q8 below the 12 GiB floor) is not a choice.
      if (quant.hasAttribute("disabled")) return;
      state.selectedQuant = quant.dataset.quant as "q5" | "q8";
      storageSet("veda:quant", state.selectedQuant);
      render();
      return;
    }
    const setupDoc = closest(target, "[data-setup-doc]");
    if (setupDoc) {
      const id = setupDoc.dataset.setupDoc ?? "";
      if (state.setupDocsets.has(id)) state.setupDocsets.delete(id);
      else state.setupDocsets.add(id);
      render();
      return;
    }

    const id = (target as HTMLElement | null)?.closest<HTMLElement>("[id]")?.id;
    switch (id) {
      case "newChat":
        startNewChat();
        return;
      case "wordmark":
        if (state.sidebarCollapsed) {
          state.sidebarCollapsed = false;
          storageSet("veda:sidebar", "open");
          render();
        }
        return;
      case "collapseSidebar":
        state.sidebarCollapsed = !state.sidebarCollapsed;
        storageSet("veda:sidebar", state.sidebarCollapsed ? "collapsed" : "open");
        render();
        return;
      case "themeToggle":
      case "settingsTheme":
        toggleTheme();
        return;
      case "settingsMode":
        state.mode = state.mode === "fast" ? "think" : "fast";
        storageSet("veda:mode", state.mode);
        render();
        return;
      case "settingsButton":
        state.settingsOpen = true;
        render();
        return;
      case "closeSettings":
        state.settingsOpen = false;
        render();
        return;
      case "settingsBackdrop":
        if (event.target === event.currentTarget || (event.target as HTMLElement).id === "settingsBackdrop") {
          state.settingsOpen = false;
          render();
        }
        return;
      case "clearHistory":
        if (window.confirm("Delete every saved chat on this device?")) {
          for (const request of pending.values()) request.controller.abort();
          pending.clear();
          state.chats = [createChat()];
          state.activeChatId = state.chats[0].id;
          composerDraft = "";
          persistChats();
          render();
          toast("Chat history cleared.");
        }
        return;
      case "modelButton":
        state.scopeOpen = false;
        state.modelOpen = !state.modelOpen;
        render();
        return;
      case "scopeButton":
        state.modelOpen = false;
        state.scopeOpen = !state.scopeOpen;
        render();
        return;
      case "sendButton":
        void sendMessage();
        return;
      case "stopButton":
        stopGenerating();
        return;
      case "attachButton":
        document.querySelector<HTMLInputElement>("#fileInput")?.click();
        return;
      case "closeReader":
        state.reader = undefined;
        state.readerLoading = false;
        render();
        return;
      case "readerBackdrop":
        if ((event.target as HTMLElement).id === "readerBackdrop") {
          state.reader = undefined;
          state.readerLoading = false;
          render();
        }
        return;
      case "dataFolder":
      case "settingsDataFolder":
        void bridge.revealDataFolder().catch((error: unknown) => toast(`Could not open the data folder: ${errorText(error)}`));
        return;
      case "retryDocsets":
        void loadDocsets().then(render);
        return;
      case "retryDownloads":
        void loadDownloads().then(render);
        return;
      case "setupBack":
        state.setupStep = Math.max(0, state.setupStep - 1);
        render();
        return;
      case "setupSkip":
        // Setup is optional: Docs and Downloads work without the model, so
        // closing the modal must never leave the app unusable. The choice is
        // remembered so it does not reappear on every launch.
        state.onboardingOpen = false;
        state.setupRunning = false;
        state.setupError = undefined;
        storageSet("veda:onboarding-skipped", "true");
        render();
        return;
      case "settingsSetup":
        // The skipped flow is reopened from Settings, which also clears the
        // skip flag so the next launch with a missing model offers setup again.
        state.settingsOpen = false;
        state.onboardingOpen = true;
        state.setupRunning = false;
        state.setupError = undefined;
        state.setupStep = 0;
        storageRemove("veda:onboarding-skipped");
        render();
        return;
      case "setupNext":
        if (state.setupStep === 1 && state.selectedQuant === "q8" && !q8Supported()) {
          state.selectedQuant = "q5";
          storageSet("veda:quant", "q5");
        }
        if (state.setupStep < 2) {
          state.setupStep += 1;
          render();
        } else void runSetup();
        return;
      case "setupRetry":
        state.setupError = undefined;
        void runSetup();
        return;
      case "setupCancelError":
        state.setupError = undefined;
        state.setupRunning = false;
        // Return to the step that failed so the user can change the model or
        // the documentation selection instead of being bounced to the end.
        state.setupStep = Math.min(2, Math.max(1, state.setupFailureStep));
        render();
        return;
      default:
        break;
    }
  });

  app.addEventListener("input", (event) => {
    const target = event.target as HTMLElement;
    if (target.id === "composerInput") {
      const input = target as HTMLTextAreaElement;
      composerDraft = input.value;
      autoGrow(input);
      return;
    }
    if (target.id === "docSearch") {
      applyDocFilter();
      return;
    }
    if (target.id === "contextRange" || target.id === "contextNumber") {
      setContext(Number((target as HTMLInputElement).value));
      // The paired control and the label are updated directly so dragging the
      // slider stays smooth and never fights the user's pointer.
      const range = document.querySelector<HTMLInputElement>("#contextRange");
      const number = document.querySelector<HTMLInputElement>("#contextNumber");
      if (range && range !== target) range.value = String(state.contextTokens);
      if (number && number !== target) number.value = String(state.contextTokens);
      const detail = document.querySelector<HTMLElement>("#contextDetail");
      const recommended = state.preflight?.recommendedContext;
      if (detail) {
        detail.textContent = `${contextLabel(state.contextTokens)}${state.contextTokens <= 0 && recommended ? ` · ${recommended.toLocaleString()} on this device` : ""}`;
      }
    }
  });

  app.addEventListener(
    "change",
    (event) => {
      const target = event.target as HTMLInputElement;
      if (target.id === "fileInput" && target.files) void addFiles(target.files);
    },
    true,
  );

  app.addEventListener("keydown", (event) => {
    const target = event.target as HTMLElement;
    if (target.id === "composerInput") {
      // Ignore Enter while an IME is composing (CJK input), and ignore
      // repeats so a held key can't fire the request twice.
      if (event.key !== "Enter" || event.shiftKey || event.isComposing || event.repeat) return;
      event.preventDefault();
      void sendMessage();
      return;
    }
    if (target.id === "renameInput") {
      if (event.key === "Enter") {
        event.preventDefault();
        renameChat(target.dataset.rename ?? "", (target as HTMLInputElement).value);
      } else if (event.key === "Escape") {
        event.preventDefault();
        state.renamingChatId = undefined;
        render();
      }
    }
  });

  // Committing a rename on blur avoids stranding the field open.
  app.addEventListener(
    "blur",
    (event) => {
      const target = event.target as HTMLElement;
      if (target.id === "renameInput" && state.renamingChatId) {
        renameChat(target.dataset.rename ?? "", (target as HTMLInputElement).value);
      }
    },
    true,
  );

  // Escape closes transient surfaces (menus, source reader, settings).
  // The onboarding flow intentionally stays open until setup completes.
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    if (closeMenus()) {
      render();
      return;
    }
    if (state.renamingChatId) {
      state.renamingChatId = undefined;
      render();
      return;
    }
    if (state.reader || state.readerLoading) {
      state.reader = undefined;
      state.readerLoading = false;
      render();
      return;
    }
    if (state.settingsOpen) {
      state.settingsOpen = false;
      render();
    }
  });
}

// Local bridge calls (preflight, docsets, downloads) resolve in milliseconds
// on a healthy device. If one ever stalls, the page must not sit on
// "Loading documentation…" forever: the call is raced against a timeout so a
// stuck load becomes a recoverable error state with a Retry button.
function withTimeout<T>(promise: Promise<T>, label: string, milliseconds = 15_000): Promise<T> {
  return Promise.race([
    promise,
    new Promise<never>((_resolve, reject) => {
      setTimeout(() => reject(new Error(`${label} took too long. Try again.`)), milliseconds);
    }),
  ]);
}

async function init(): Promise<void> {
  bindGlobalEvents();
  render();

  // Each source is awaited independently: a single failure must never leave
  // the Docs or Downloads pages permanently blank.
  const results = await Promise.allSettled([
    withTimeout(bridge.preflight(), "The system check"),
    withTimeout(bridge.docsets(), "The documentation list"),
    withTimeout(bridge.downloads(), "The downloads list"),
  ]);
  const [preflightResult, docsetsResult, downloadsResult] = results;

  if (preflightResult.status === "fulfilled") state.preflight = preflightResult.value;
  if (docsetsResult.status === "fulfilled") state.docsets = docsetsResult.value;
  else state.docsetsError = errorText(docsetsResult.reason);
  if (downloadsResult.status === "fulfilled") state.downloads = downloadsResult.value;
  else state.downloadsError = errorText(downloadsResult.reason);

  const downloads = state.downloads;
  const installedModel = (quant: string): boolean =>
    downloads.some((item) => item.id === `minicpm5-${quant}` && item.state === "installed");
  const storedQuant = migrated("quant");
  // A previously chosen model wins when it is still installed; otherwise the
  // installed model is shown; otherwise Q5, the default on every device.
  state.selectedQuant =
    (storedQuant === "q5" || storedQuant === "q8") && installedModel(storedQuant)
      ? storedQuant
      : installedModel("q8")
        ? "q8"
        : installedModel("q5")
          ? "q5"
          : (state.preflight?.recommendedQuant ?? "q5");
  if (state.selectedQuant === "q8" && !q8Supported()) {
    state.selectedQuant = "q5";
    storageSet("veda:quant", "q5");
  } else if ((storedQuant === "q5" || storedQuant === "q8") && !installedModel(storedQuant)) {
    // The stored preference is no longer backed by an installed model, so the
    // fallback is what is actually in use; record it so later reads do not
    // misreport a stale choice. Fresh installs stay unset until the user
    // picks a model themselves.
    storageSet("veda:quant", state.selectedQuant);
  }

  const modelReady = downloads.some((item) => item.id.startsWith("minicpm5-") && item.state === "installed");
  const docsReady = state.docsets.some(isDocInstalled);
  // Setup is offered when something is missing, but it must never lock the
  // app behind a modal: Docs and Downloads work without the model, so a
  // user who skips setup ("Skip for now") can still browse and install
  // documentation. The choice is remembered so the modal does not reappear
  // on every launch until they open it again from Settings.
  if (bridge.isDesktop() && !migrated("onboarding-skipped") && (!modelReady || !docsReady)) {
    storageRemove("veda:onboarded");
    state.onboardingOpen = true;
  }

  try {
    await bridge.onDownloadProgress((item) => {
      const existing = state.downloads.findIndex((download) => download.id === item.id);
      if (existing >= 0) state.downloads[existing] = item;
      else state.downloads.push(item);
      const docId = item.id.endsWith("-index") ? item.id.slice(0, -6) : undefined;
      const doc = docId ? state.docsets.find((candidate) => candidate.id === docId) : undefined;
      if (doc) {
        doc.progress = item.progress;
        doc.state = item.state === "installed" ? "installed" : "indexing";
      }
      if (state.setupRunning) updateSetupProgress(item.detail || item.name, item.progress);
      scheduleRender();
    });
  } catch (error) {
    console.error("Veda could not subscribe to download progress:", error);
  }

  render();
}

void init();

// Exposed for the automated UI tests, which drive the real module rather than
// a reimplementation of it.
export const __test = {
  state,
  render,
  pending,
  contextMax: CONTEXT_MAX,
};
