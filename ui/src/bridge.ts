import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { mockAsk } from "./mock";
import type { AskRequest, AskResponse, Docset, DownloadItem, LocalDocFile, PreflightReport, ReaderSource } from "./types";

const isTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const desktopOnly = (command: string): Error => new Error(`${command} is only available in the Veda desktop app.`);

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw desktopOnly(command);
  return invoke<T>(command, args);
}

const delay = (milliseconds: number) => new Promise<void>((resolve) => setTimeout(resolve, milliseconds));

// ---------------------------------------------------------------------------
// Browser preview state. The preview is a fully clickable mock of the desktop
// app: two documentation packs start installed and setup is simulated so the
// whole onboarding flow can be exercised without a desktop runtime.
// ---------------------------------------------------------------------------

const browserPreflight: PreflightReport = {
  totalMemoryBytes: 16 * 1024 ** 3,
  availableMemoryBytes: 10.8 * 1024 ** 3,
  freeDiskBytes: 82.4 * 1024 ** 3,
  diskKind: "ssd",
  architecture: "arm64",
  operatingSystem: "Browser preview",
  recommendedQuant: "q5",
  recommendedContext: 16384,
  hardFailures: [],
  warnings: ["This is a browser preview. Download the Veda desktop app to install local resources on this device."],
};

const browserDocsets: Docset[] = [
  { id: "python", name: "Python", detail: "Language reference, standard library and tutorials from Python.org.", version: "3.14.7", compressedBytes: 16_737_282, installedBytes: 80_059_722, state: "installed", progress: 100, pages: 571, accent: "#8fc7b0", initials: "PY" },
  { id: "cpp", name: "C++", detail: "C and C++ language and standard library reference from cppreference.", version: "cppreference 2025.02", compressedBytes: 55_740_889, installedBytes: 346_973_285, state: "installed", progress: 100, pages: 6640, accent: "#81a7c8", initials: "C++" },
  { id: "html", name: "HTML", detail: "Elements, attributes, forms, semantics and accessibility guides from MDN.", version: "MDN 2026.08", compressedBytes: 73_684_713, installedBytes: 0, state: "available", progress: 0, pages: 254, accent: "#dc9078", initials: "<>" },
  { id: "css", name: "CSS", detail: "Properties, selectors, layout, animation and responsive design from MDN.", version: "MDN 2026.08", compressedBytes: 73_684_713, installedBytes: 0, state: "available", progress: 0, pages: 1252, accent: "#889bd0", initials: "#" },
  { id: "javascript", name: "JavaScript", detail: "JavaScript reference, operators, built-ins and language guides from MDN.", version: "MDN 2026.08", compressedBytes: 73_684_713, installedBytes: 0, state: "available", progress: 0, pages: 1333, accent: "#d9c273", initials: "JS" },
];

const browserDownloads: DownloadItem[] = [
  { id: "minicpm5-q5", name: "MiniCPM 5 · Q5", detail: "", state: "installed", progress: 100, downloadedBytes: 786_862_688, totalBytes: 786_862_688 },
  { id: "bge-small-q8", name: "Offline search support", detail: "Ready", state: "installed", progress: 100, downloadedBytes: 36_806_944, totalBytes: 36_806_944 },
  { id: "python-source-3.14.7", name: "Python 3.14.7", detail: "571 pages indexed", state: "installed", progress: 100, downloadedBytes: 16_737_282, totalBytes: 16_737_282 },
  { id: "cppreference-source-20250209", name: "cppreference", detail: "6,640 pages indexed", state: "installed", progress: 100, downloadedBytes: 55_740_889, totalBytes: 55_740_889 },
];

const progressHandlers = new Set<(item: DownloadItem) => void>();

function emitProgress(item: DownloadItem): void {
  for (const handler of progressHandlers) handler(structuredClone(item));
}

async function simulatedInstall(id: string, name: string, totalBytes: number): Promise<void> {
  // Source archive download phase. The doc card mirrors this via the `docset`
  // field, so a real install shows progress for the download *and* the index.
  for (let step = 1; step <= 2; step += 1) {
    await delay(300);
    emitProgress({
      id: `${id}-source`,
      name,
      detail: `Downloading ${name} · step ${step} of 2`,
      state: "downloading",
      progress: step * 40,
      downloadedBytes: Math.round((totalBytes * step * 40) / 100),
      totalBytes,
      docset: id,
    });
  }
  // Search index / embedding phase.
  for (let step = 1; step <= 4; step += 1) {
    await delay(420);
    emitProgress({
      id: `${id}-index`,
      name,
      detail: `Preparing search · step ${step} of 4`,
      state: "indexing",
      progress: 40 + step * 15,
      downloadedBytes: Math.round((totalBytes * (40 + step * 15)) / 100),
      totalBytes,
      docset: id,
    });
  }
  const doc = browserDocsets.find((candidate) => candidate.id === id);
  if (doc) {
    doc.state = "installed";
    doc.progress = 100;
    doc.installedBytes = totalBytes;
  }
  const existing = browserDownloads.findIndex((item) => item.id === `${id}-index`);
  const ready: DownloadItem = { id: `${id}-index`, name, detail: "Ready", state: "installed", progress: 100, downloadedBytes: totalBytes, totalBytes, docset: id };
  if (existing >= 0) browserDownloads[existing] = ready; else browserDownloads.push(ready);
  emitProgress(ready);
}

export const bridge = {
  isDesktop: isTauri,
  preflight: () => (isTauri() ? call<PreflightReport>("system_preflight") : Promise.resolve(structuredClone(browserPreflight))),
  docsets: () => (isTauri() ? call<Docset[]>("list_docsets") : Promise.resolve(structuredClone(browserDocsets))),
  downloads: () => (isTauri() ? call<DownloadItem[]>("list_downloads") : Promise.resolve(structuredClone(browserDownloads))),
  onDownloadProgress: (handler: (item: DownloadItem) => void): Promise<UnlistenFn> => {
    if (isTauri()) return listen<DownloadItem>("download-progress", (event) => handler(event.payload));
    progressHandlers.add(handler);
    return Promise.resolve(() => {
      progressHandlers.delete(handler);
    });
  },
  prepareResources: async (quant: "q5" | "q8"): Promise<void> => {
    if (isTauri()) return call<void>("prepare_resources", { quant });
    await delay(900);
    const model: DownloadItem =
      quant === "q8"
        ? { id: "minicpm5-q8", name: "MiniCPM 5 · Q8", detail: "", state: "installed", progress: 100, downloadedBytes: 1_153_529_261, totalBytes: 1_153_529_261 }
        : { id: "minicpm5-q5", name: "MiniCPM 5 · Q5", detail: "", state: "installed", progress: 100, downloadedBytes: 786_862_688, totalBytes: 786_862_688 };
    for (const item of [model, browserDownloads.find((candidate) => candidate.id === "bge-small-q8")].filter(Boolean) as DownloadItem[]) {
      const existing = browserDownloads.findIndex((candidate) => candidate.id === item.id);
      if (existing >= 0) browserDownloads[existing] = item; else browserDownloads.push(item);
      emitProgress(item);
    }
  },
  installDocset: (id: string): Promise<void> => {
    if (isTauri()) return call<void>("install_docset", { id });
    const doc = browserDocsets.find((candidate) => candidate.id === id);
    if (!doc || doc.state === "installed") return Promise.resolve();
    return simulatedInstall(doc.id, `${doc.name} ${doc.version}`, doc.compressedBytes);
  },
  installLocalDocs: async (name: string, files: LocalDocFile[]): Promise<Docset> => {
    if (isTauri()) return call<Docset>("install_local_docs", { name, files });
    await delay(200);
    const slug = name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "")
      .slice(0, 40) || "docs";
    let id = `local-${slug}`;
    let suffix = 2;
    while (browserDocsets.some((doc) => doc.id === id)) {
      id = `local-${slug}-${suffix}`;
      suffix += 1;
    }
    const doc: Docset = {
      id,
      name,
      detail: "Your documentation, searchable offline.",
      version: "local",
      compressedBytes: 0,
      installedBytes: files.reduce((sum, file) => sum + file.content.length, 0),
      state: "installed",
      progress: 100,
      pages: files.length,
      accent: "#c4a574",
      initials: "YO",
    };
    browserDocsets.push(doc);
    return structuredClone(doc);
  },
  pickAndInstallLocalDocs: (): Promise<Docset | null> => {
    if (isTauri()) return call<Docset | null>("pick_and_install_local_docs");
    return Promise.resolve(null);
  },
  onAskToken: (handler: (event: { chatId: string; text: string }) => void): Promise<UnlistenFn> => {
    if (isTauri()) return listen<{ chatId: string; text: string }>("ask-token", (event) => handler(event.payload));
    return Promise.resolve(() => undefined);
  },
  removeDocset: async (id: string): Promise<void> => {
    if (isTauri()) return call<void>("remove_docset", { id });
    await delay(500);
    const index = browserDocsets.findIndex((candidate) => candidate.id === id);
    if (index < 0) return;
    if (id.startsWith("local-")) {
      browserDocsets.splice(index, 1);
      return;
    }
    const doc = browserDocsets[index];
    doc.state = "available";
    doc.progress = 0;
    doc.installedBytes = 0;
    doc.pages = undefined;
  },
  ask: (request: AskRequest): Promise<AskResponse> => {
    // The abort signal is a UI-side concern: it must not be serialised into
    // the Tauri command payload.
    const { signal, ...payload } = request;
    if (signal?.aborted) return Promise.reject(new DOMException("Aborted", "AbortError"));
    const work = isTauri() ? call<AskResponse>("ask_veda", { request: payload }) : mockAsk(payload);
    if (!signal) return work;
    // Stopping is immediate for the user; the backend call is abandoned.
    return Promise.race([
      work,
      new Promise<never>((_resolve, reject) => {
        signal.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true });
      }),
    ]);
  },
  readSource: async (url: string): Promise<ReaderSource> => {
    if (isTauri()) return call<ReaderSource>("read_source", { url });
    await delay(200);
    return {
      title: "Coroutines and Tasks",
      section: "Task Groups",
      docset: "Python 3.14",
      text: "A `TaskGroup` holds a collection of tasks that can be conveniently awaited together. Leaving the group context waits for every task, and if one task raises, the others are cancelled and the failures are raised as an exception group.\n\n```python\nasync with asyncio.TaskGroup() as group:\n    group.create_task(fetch_one())\n    group.create_task(fetch_two())\n```\n\nThis is the structured concurrency pattern described by the retrieved source.",
      url,
    };
  },
  openSource: (url: string) => (isTauri() ? call<void>("open_source", { url }) : Promise.resolve()),
  revealDataFolder: () => (isTauri() ? call<void>("reveal_data_folder") : Promise.resolve()),
};
