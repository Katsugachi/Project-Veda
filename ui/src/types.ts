export type View = "chat" | "docs" | "downloads";
export type Theme = "dark" | "light";
export type ReasoningMode = "fast" | "think";
export type InstallState = "available" | "updateAvailable" | "queued" | "downloading" | "indexing" | "removing" | "installed" | "error";

export interface PreflightReport {
  totalMemoryBytes: number;
  availableMemoryBytes: number;
  freeDiskBytes: number;
  diskKind: "ssd" | "hdd" | "unknown";
  architecture: string;
  operatingSystem: string;
  recommendedQuant: "q5" | "q8";
  recommendedContext: number;
  hardFailures: string[];
  warnings: string[];
}

export interface LocalDocFile {
  path: string;
  content: string;
}

export interface Docset {
  id: string;
  name: string;
  detail: string;
  version: string;
  compressedBytes: number;
  installedBytes: number;
  state: InstallState;
  progress: number;
  pages?: number;
  accent: string;
  initials: string;
}

export interface DownloadItem {
  id: string;
  name: string;
  detail: string;
  state: InstallState;
  progress: number;
  downloadedBytes: number;
  totalBytes: number;
  speedBytes?: number;
  /** The docset this download belongs to (source archives and search
   *  indexes), so the Docs tab can mirror live progress on the doc card. */
  docset?: string;
}

export interface ReaderSource {
  title: string;
  section: string;
  docset: string;
  text: string;
  url: string;
}

export interface SourceRef {
  id: string;
  docset: string;
  title: string;
  section: string;
  url: string;
  score: number;
}

export interface Attachment {
  id: string;
  name: string;
  language: string;
  bytes: number;
  content?: string;
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  createdAt: number;
  sources?: SourceRef[];
  attachments?: Attachment[];
  streaming?: boolean;
  /** Human-readable activity shown while the reply is still streaming and
   *  `content` is empty (e.g. "Pondering…", "Searching installed docs…"). */
  status?: string;
  /** Set when the user interrupted this reply. */
  stopped?: boolean;
  /** Set when the request failed, so the UI can offer a retry. */
  failed?: boolean;
}

export interface Chat {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messages: ChatMessage[];
}

export interface AskRequest {
  chatId: string;
  message: string;
  mode: ReasoningMode;
  docsets: string[];
  attachments: Attachment[];
  /** 0 means automatic (16,384 tokens). An explicit value is honoured. */
  contextTokens?: number;
  /** The model quantization the UI is configured for; the backend loads the
   *  matching model file when it is installed. */
  modelQuant?: "q5" | "q8";
  /** Lets an in-flight local request be interrupted by the user. */
  signal?: AbortSignal;
}

export interface AskResponse {
  messageId: string;
  content: string;
  sources: SourceRef[];
  trace?: {
    queries: string[];
    lexicalHits: number;
    semanticHits: number;
    elapsedMs: number;
  };
}
