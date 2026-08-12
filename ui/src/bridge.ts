import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { initialDocsets, initialDownloads, mockAsk, mockPreflight } from "./mock";
import type { AskRequest, AskResponse, Docset, DownloadItem, PreflightReport, ReaderSource } from "./types";

const isTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(command: string, args?: Record<string, unknown>, fallback?: () => Promise<T> | T): Promise<T> {
  if (isTauri()) return invoke<T>(command, args);
  if (!fallback) throw new Error(`${command} is only available in the Palor desktop app.`);
  return fallback();
}

export const bridge = {
  isDesktop: isTauri,
  preflight: () => call<PreflightReport>("system_preflight", undefined, () => mockPreflight),
  docsets: () => call<Docset[]>("list_docsets", undefined, () => structuredClone(initialDocsets)),
  downloads: () => call<DownloadItem[]>("list_downloads", undefined, () => structuredClone(initialDownloads)),
  onDownloadProgress: (handler: (item: DownloadItem) => void): Promise<UnlistenFn> => {
    if (!isTauri()) return Promise.resolve(() => undefined);
    return listen<DownloadItem>("download-progress", (event) => handler(event.payload));
  },
  prepareResources: (quant: "q5" | "q8") => call<void>("prepare_resources", { quant }, async () => undefined),
  installDocset: (id: string) => call<void>("install_docset", { id }, async () => undefined),
  ask: (request: AskRequest) => call<AskResponse>("ask_palor", { request }, () => mockAsk(request)),
  readSource: (url: string) => call<ReaderSource>("read_source", { url }, () => ({ title: "Coroutines and Tasks", section: "Task Groups", docset: "Python 3.14.7", text: "Task groups combine a task creation API with a convenient and reliable way to wait for all tasks in the group to finish. The async with statement waits for all tasks in the group. The first non-cancellation failure cancels the remaining tasks, and failures are combined in an exception group.", url })),
  openSource: (url: string) => call<void>("open_source", { url }, async () => undefined),
  revealDataFolder: () => call<void>("reveal_data_folder", undefined, async () => undefined),
};
