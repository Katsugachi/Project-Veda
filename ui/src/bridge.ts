import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AskRequest, AskResponse, Docset, DownloadItem, PreflightReport, ReaderSource } from "./types";

const isTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const desktopOnly = (command: string): Error => new Error(`${command} is only available in the Palor desktop app.`);

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw desktopOnly(command);
  return invoke<T>(command, args);
}

const browserPreflight: PreflightReport = {
  totalMemoryBytes: 0,
  availableMemoryBytes: 0,
  freeDiskBytes: 0,
  diskKind: "unknown",
  architecture: "Unavailable",
  operatingSystem: "Browser preview",
  recommendedQuant: "q5",
  recommendedContext: 4096,
  hardFailures: ["Open the Palor desktop app to check this computer and install local resources."],
  warnings: [],
};

const browserDocsets: Docset[] = [
  { id: "python", name: "Python", detail: "Language reference, standard library and tutorials from Python.org.", version: "3.14.7", compressedBytes: 16_737_282, installedBytes: 0, state: "available", progress: 0, accent: "#8fc7b0", initials: "PY" },
  { id: "cpp", name: "C++", detail: "C and C++ language and standard library reference from cppreference.", version: "cppreference 2025.02", compressedBytes: 55_740_889, installedBytes: 0, state: "available", progress: 0, accent: "#81a7c8", initials: "C++" },
  { id: "html", name: "HTML", detail: "Elements, attributes, forms, semantics and accessibility guides from MDN.", version: "MDN 2026.08", compressedBytes: 73_684_713, installedBytes: 0, state: "available", progress: 0, accent: "#dc9078", initials: "<>" },
  { id: "css", name: "CSS", detail: "Properties, selectors, layout, animation and responsive design from MDN.", version: "MDN 2026.08", compressedBytes: 73_684_713, installedBytes: 0, state: "available", progress: 0, accent: "#889bd0", initials: "#" },
  { id: "javascript", name: "JavaScript", detail: "JavaScript reference, operators, built-ins and language guides from MDN.", version: "MDN 2026.08", compressedBytes: 73_684_713, installedBytes: 0, state: "available", progress: 0, accent: "#d9c273", initials: "JS" },
];

export const bridge = {
  isDesktop: isTauri,
  preflight: () => isTauri() ? call<PreflightReport>("system_preflight") : Promise.resolve(structuredClone(browserPreflight)),
  docsets: () => isTauri() ? call<Docset[]>("list_docsets") : Promise.resolve(structuredClone(browserDocsets)),
  downloads: () => isTauri() ? call<DownloadItem[]>("list_downloads") : Promise.resolve([]),
  onDownloadProgress: (handler: (item: DownloadItem) => void): Promise<UnlistenFn> => {
    if (!isTauri()) return Promise.resolve(() => undefined);
    return listen<DownloadItem>("download-progress", (event) => handler(event.payload));
  },
  prepareResources: (quant: "q5" | "q8") => call<void>("prepare_resources", { quant }),
  installDocset: (id: string) => call<void>("install_docset", { id }),
  removeDocset: (id: string) => call<void>("remove_docset", { id }),
  ask: (request: AskRequest) => call<AskResponse>("ask_palor", { request }),
  readSource: (url: string) => call<ReaderSource>("read_source", { url }),
  openSource: (url: string) => call<void>("open_source", { url }),
  revealDataFolder: () => call<void>("reveal_data_folder"),
};
