import type { AskRequest, AskResponse, Docset, DownloadItem, PreflightReport } from "./types";

export const mockPreflight: PreflightReport = {
  totalMemoryBytes: 16 * 1024 ** 3,
  availableMemoryBytes: 10.8 * 1024 ** 3,
  freeDiskBytes: 82.4 * 1024 ** 3,
  diskKind: "ssd",
  architecture: "arm64",
  operatingSystem: "macOS 15.6",
  recommendedQuant: "q8",
  recommendedContext: 16384,
  hardFailures: [],
  warnings: [],
};

export const initialDocsets: Docset[] = [
  { id: "python", name: "Python", detail: "Language reference, standard library and tutorials from Python.org.", version: "3.14.7", compressedBytes: 16_737_282, installedBytes: 80_059_722, state: "installed", progress: 100, pages: 571, accent: "#8fc7b0", initials: "PY" },
  { id: "cpp", name: "C++", detail: "C and C++ language and standard library reference from cppreference.", version: "cppreference 2025.02", compressedBytes: 55_740_889, installedBytes: 346_973_285, state: "installed", progress: 100, pages: 6640, accent: "#81a7c8", initials: "C++" },
  { id: "html", name: "HTML", detail: "Elements, attributes, forms, semantics and accessibility guides from MDN.", version: "MDN 2026.08", compressedBytes: 73_684_713, installedBytes: 3_035_451, state: "available", progress: 0, pages: 254, accent: "#dc9078", initials: "<>" },
  { id: "css", name: "CSS", detail: "Properties, selectors, layout, animation and responsive design from MDN.", version: "MDN 2026.08", compressedBytes: 73_684_713, installedBytes: 11_696_917, state: "available", progress: 0, pages: 1252, accent: "#889bd0", initials: "#" },
  { id: "javascript", name: "JavaScript", detail: "JavaScript reference, operators, built-ins and language guides from MDN.", version: "MDN 2026.08", compressedBytes: 73_684_713, installedBytes: 6_950_441, state: "available", progress: 0, pages: 1333, accent: "#d9c273", initials: "JS" },
];

export const initialDownloads: DownloadItem[] = [
  { id: "model", name: "MiniCPM 5 · Q8", detail: "Verified and ready", state: "installed", progress: 100, downloadedBytes: 1153529261, totalBytes: 1153529261 },
  { id: "embeddings", name: "Offline search", detail: "Ready", state: "installed", progress: 100, downloadedBytes: 36806944, totalBytes: 36806944 },
  { id: "python", name: "Python 3.14.7", detail: "571 pages indexed", state: "installed", progress: 100, downloadedBytes: 16_737_282, totalBytes: 16_737_282 },
  { id: "cpp", name: "cppreference", detail: "6,640 pages indexed", state: "installed", progress: 100, downloadedBytes: 55_740_889, totalBytes: 55_740_889 },
];

const sources = [
  { id: "S1", docset: "Python 3.14", title: "Coroutines and Tasks", section: "Task Groups", url: "veda://docs/python/library/asyncio-task#task-groups", score: .94 },
  { id: "S2", docset: "Python 3.14", title: "Exceptions", section: "Exception groups", url: "veda://docs/python/library/exceptions#ExceptionGroup", score: .87 },
];

export async function mockAsk(request: AskRequest): Promise<AskResponse> {
  await new Promise((resolve) => setTimeout(resolve, 520));
  const lower = request.message.toLowerCase();
  if (lower.includes("vector") || lower.includes("move")) {
    return {
      messageId: crypto.randomUUID(),
      content: "`std::vector` move construction is normally constant time because ownership of the allocation is transferred to the destination. The moved-from vector remains valid but its state is unspecified, so you may destroy it, assign to it, or call operations that do not rely on its previous contents. [S1]\n\n```cpp\nstd::vector<int> source{1, 2, 3};\nauto destination = std::move(source);\n// source is valid, but do not assume it is empty.\n```\n\nAllocator rules can change the complexity of move assignment when the allocators cannot be propagated and do not compare equal. [S2]",
      sources: [
        { id: "S1", docset: "cppreference", title: "std::vector::vector", section: "Move constructor", url: "veda://docs/cpp/container/vector/vector", score: .96 },
        { id: "S2", docset: "cppreference", title: "std::vector::operator=", section: "Move assignment", url: "veda://docs/cpp/container/vector/operator_assign", score: .9 },
      ],
      trace: { queries: ["std::vector move construction complexity", "vector move allocator propagation"], lexicalHits: 24, semanticHits: 24, elapsedMs: 84 },
    };
  }
  return {
    messageId: crypto.randomUUID(),
    content: "Use `asyncio.TaskGroup` when a set of related tasks should succeed or fail as a unit. Leaving the context waits for every task; if one task raises a non-cancellation exception, the remaining tasks are cancelled and the failures are raised as an exception group. [S1]\n\n```python\nasync with asyncio.TaskGroup() as group:\n    first = group.create_task(fetch_one())\n    second = group.create_task(fetch_two())\n\nresult = first.result(), second.result()\n```\n\nThis gives structured concurrency and is safer than retaining unrelated background tasks manually. Handle grouped failures with `except*` where appropriate. [S2]",
    sources,
    trace: { queries: [request.message, "asyncio TaskGroup structured concurrency exceptions"], lexicalHits: 24, semanticHits: 24, elapsedMs: 71 },
  };
}
