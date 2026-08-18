/**
 * One test per issue reported against the Veda UI.
 *
 * These drive the real `ui/src/main.ts` in jsdom. Only the Tauri/network
 * bridge is stubbed — exactly as the shipped browser preview already does —
 * so the code under test is the code that ships.
 */
import { beforeEach, describe, expect, it } from "vitest";
import { $, $$, click, cssSource, flush, loadApp, press, type, waitFor } from "./harness";

const bodyText = () => document.body.textContent ?? "";

async function sendAndWait(message: string) {
  type("#composerInput", message);
  click("#sendButton");
  await waitFor(() => !$("#stopButton"), 6000);
}

describe("Issue 1 — the Think/Fast menu drops down instead of floating", () => {
  beforeEach(async () => {
    await loadApp();
  });

  it("anchors the menu to its trigger, not to the whole composer", async () => {
    click("#modelButton");
    await flush(20);
    const menu = $(".mode-popover")!;
    expect(menu).toBeTruthy();
    // The menu must live inside the anchor that wraps the button, so it is
    // positioned relative to the control that opened it.
    const anchor = menu.parentElement!;
    expect(anchor.classList.contains("menu-anchor")).toBe(true);
    expect(anchor.contains($("#modelButton"))).toBe(true);
  });

  it("is positioned below the trigger, not above it", () => {
    const css = cssSource();
    const rule = /\.popover\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
    expect(rule).toContain("top: calc(100% + 8px)");
    // The old "floating above the composer" behaviour is gone.
    expect(rule).not.toMatch(/bottom:\s*calc\(100% \+ 10px\)/);
    expect(/\.menu-anchor\s*\{[^}]*position:\s*relative/.test(css)).toBe(true);
  });

  it("opens, closes on a second press and closes on outside click", async () => {
    click("#modelButton");
    await flush(10);
    expect($(".mode-popover")).toBeTruthy();
    click("#modelButton");
    await flush(10);
    expect($(".mode-popover")).toBeNull();

    click("#modelButton");
    await flush(10);
    $(".page-title")!.click();
    await flush(10);
    expect($(".mode-popover")).toBeNull();
  });
});

describe("Issue 2 — the useless dot next to MiniCPM 5 is gone", () => {
  it("renders no mode-indicator dot anywhere", async () => {
    await loadApp();
    expect($$(".mode-indicator")).toHaveLength(0);
    expect(cssSource()).not.toContain(".mode-indicator");
  });
});

describe("Issue 3 & 12 — docs and docsets are viewable, even when a load fails", () => {
  it("lists every docset with its real metadata", async () => {
    await loadApp();
    click('[data-view="docs"]');
    await flush(30);
    expect($$(".doc-card").length).toBe(5);
    expect(bodyText()).toContain("Python");
    expect(bodyText()).toContain("JavaScript");
    expect($(".library-summary")?.textContent).toMatch(/installed pages across \d+ docsets/);
  });

  it("shows installed docsets in the composer scope menu", async () => {
    await loadApp();
    click("#scopeButton");
    await flush(20);
    const menu = $(".scope-popover")!;
    expect(menu).toBeTruthy();
    expect(menu.textContent).toContain("Python");
    expect(menu.textContent).toContain("C++");
  });

  it("keeps Docs usable when the docset bridge call rejects, and can retry", async () => {
    let fail = true;
    await loadApp({
      bridge: (actual) => ({
        ...actual,
        docsets: () => (fail ? Promise.reject(new Error("index unavailable")) : actual.docsets()),
      }),
    });
    click('[data-view="docs"]');
    await flush(30);
    // An explicit, recoverable error state — not a blank page.
    expect($(".error-state")).toBeTruthy();
    expect(bodyText()).toContain("index unavailable");

    fail = false;
    click("#retryDocsets");
    await waitFor(() => $$(".doc-card").length === 5);
    expect($(".error-state")).toBeNull();
  });

  it("still renders Downloads when only the docset call fails", async () => {
    await loadApp({
      bridge: (actual) => ({ ...actual, docsets: () => Promise.reject(new Error("nope")) }),
    });
    click('[data-view="downloads"]');
    await flush(30);
    expect($$(".download-row").length).toBeGreaterThan(0);
  });
});

describe("Issue 4 — the context window is configurable from 0 to 131k", () => {
  it("exposes a slider and a number field with the full range", async () => {
    await loadApp();
    click("#settingsButton");
    await flush(20);
    const range = $<HTMLInputElement>("#contextRange")!;
    const number = $<HTMLInputElement>("#contextNumber")!;
    expect(range.min).toBe("0");
    expect(range.max).toBe("131072");
    expect(number.min).toBe("0");
    expect(number.max).toBe("131072");
  });

  it("treats 0 as automatic and reports the chosen value", async () => {
    await loadApp();
    click("#settingsButton");
    await flush(20);
    type("#contextRange", "0");
    await flush(10);
    expect($("#contextDetail")?.textContent).toContain("Automatic");

    type("#contextRange", "131072");
    await flush(10);
    expect($("#contextDetail")?.textContent).toContain("131,072 tokens");
  });

  it("keeps the slider and the number field in step", async () => {
    await loadApp();
    click("#settingsButton");
    await flush(20);
    type("#contextNumber", "65536");
    await flush(10);
    expect($<HTMLInputElement>("#contextRange")!.value).toBe("65536");
    type("#contextRange", "8192");
    await flush(10);
    expect($<HTMLInputElement>("#contextNumber")!.value).toBe("8192");
  });

  it("persists the choice and sends it with the next request", async () => {
    let seen: number | undefined;
    await loadApp({
      bridge: (actual) => ({
        ...actual,
        ask: (request: any) => {
          seen = request.contextTokens;
          return actual.ask(request);
        },
      }),
    });
    click("#settingsButton");
    await flush(20);
    type("#contextNumber", "32768");
    await flush(10);
    expect(localStorage.getItem("veda:context")).toBe("32768");
    click("#closeSettings");
    await flush(10);
    await sendAndWait("hello");
    expect(seen).toBe(32768);
  });
});

describe("Issue 5 & 8 — messages send, never freeze, and land while away", () => {
  it("sends a message and renders the reply", async () => {
    await loadApp();
    await sendAndWait("how do task groups work?");
    expect($$(".message").length).toBe(2);
    expect($$(".message-body")[1].textContent).toContain("TaskGroup");
  });

  it("keeps working for many consecutive messages", async () => {
    await loadApp();
    for (const text of ["one", "two", "three"]) await sendAndWait(text);
    expect($$(".message").length).toBe(6);
    // The composer is never left disabled.
    expect($<HTMLTextAreaElement>("#composerInput")!.disabled).toBe(false);
  });

  it("answers while the user is on another page, and shows it on return", async () => {
    await loadApp();
    type("#composerInput", "vector move");
    click("#sendButton");
    await flush(20);
    click('[data-view="downloads"]');
    await waitFor(() => !$("#stopButton") || $(".download-list") !== null, 1000);
    await flush(900);
    click('[data-view="chat"]');
    await flush(30);
    const reply = $$(".message-body")[1]?.textContent ?? "";
    expect(reply).toContain("std::vector");
    expect(reply).not.toContain("Searching installed docs");
  });

  it("does not freeze when the chat is switched mid-request", async () => {
    await loadApp();
    type("#composerInput", "first question");
    click("#sendButton");
    await flush(20);
    click("#newChat");
    await flush(20);
    // The fresh chat is immediately usable even though the other one is busy.
    expect($<HTMLTextAreaElement>("#composerInput")!.disabled).toBe(false);
    expect($("#sendButton")).toBeTruthy();
    await sendAndWait("second question");
    expect($$(".message").length).toBe(2);
  });

  it("recovers the composer after a failing request", async () => {
    await loadApp({
      bridge: (actual) => ({ ...actual, ask: () => Promise.reject(new Error("model offline")) }),
    });
    type("#composerInput", "hello");
    click("#sendButton");
    await waitFor(() => !$("#stopButton"), 4000);
    expect(bodyText()).toContain("model offline");
    // Still fully interactive after the failure.
    expect($<HTMLTextAreaElement>("#composerInput")!.disabled).toBe(false);
    expect($("#sendButton")).toBeTruthy();
  });
});

describe("Issue 6 — the active mode is legible in light mode", () => {
  it("states the mode in words on the button, not just as a colour", async () => {
    await loadApp({ theme: "light" });
    // Fast is the default but is deliberately not spelled out on the button;
    // it simply reads "MiniCPM 5" until Think is selected.
    expect($("#modelButton")!.textContent).toContain("MiniCPM 5");
    expect($("#modelButton")!.textContent).not.toContain("Fast");
    expect($(".mode-tag")).toBeNull();
    click("#modelButton");
    await flush(10);
    click('[data-mode="think"]');
    await flush(10);
    expect($("#modelButton")!.textContent).toContain("MiniCPM 5 Think");
    expect($("#modelButton")!.className).toContain("mode-think");
  });

  it("marks the selected row with a check, not only a tint", async () => {
    await loadApp({ theme: "light" });
    click("#modelButton");
    await flush(10);
    const fast = $('[data-mode="fast"]')!;
    expect(fast.getAttribute("aria-checked")).toBe("true");
    expect(fast.querySelector(".popover-item-check svg")).toBeTruthy();
    expect($('[data-mode="think"]')!.getAttribute("aria-checked")).toBe("false");
  });

  it("remembers the mode across a reload", async () => {
    await loadApp({ theme: "light" });
    click("#modelButton");
    await flush(10);
    click('[data-mode="think"]');
    await flush(10);
    await loadApp({ theme: "light", keepStorage: true });
    expect($("#modelButton")!.textContent).toContain("Think");
  });
});

describe("Issue 7 — a running message can be interrupted", () => {
  it("swaps Send for a live Stop control while generating", async () => {
    await loadApp();
    type("#composerInput", "hello");
    click("#sendButton");
    await flush(20);
    const stop = $<HTMLButtonElement>("#stopButton")!;
    expect(stop).toBeTruthy();
    // The old build showed a pause glyph on a *disabled* button.
    expect(stop.disabled).toBe(false);
    await waitFor(() => !$("#stopButton"), 4000);
  });

  it("stops generation immediately and records that the user stopped it", async () => {
    await loadApp({
      bridge: (actual) => ({
        ...actual,
        ask: (request: any) =>
          new Promise((resolve, reject) => {
            const timer = setTimeout(() => resolve(actual.ask(request)), 5000);
            request.signal?.addEventListener("abort", () => {
              clearTimeout(timer);
              reject(new DOMException("Aborted", "AbortError"));
            });
          }),
      }),
    });
    type("#composerInput", "a long question");
    click("#sendButton");
    await flush(30);
    click("#stopButton");
    await waitFor(() => !$("#stopButton"), 3000);
    expect(bodyText()).toContain("Stopped");
    expect($(".stream-caret")).toBeNull();
    // And the composer is usable again right away.
    expect($("#sendButton")).toBeTruthy();
  });
});

describe("Issue 9 — past chats can be seen, edited and deleted", () => {
  it("lists past chats in the sidebar with a derived title", async () => {
    await loadApp();
    await sendAndWait("How do I use asyncio TaskGroup?");
    expect($$(".recent").length).toBe(1);
    expect($(".recent")!.textContent).toContain("How do I use asyncio TaskGroup?");
  });

  it("keeps separate conversations and switches between them", async () => {
    await loadApp();
    await sendAndWait("first conversation");
    click("#newChat");
    await flush(20);
    await sendAndWait("second conversation");
    expect($$(".recent").length).toBe(2);

    const first = $$(".recent").find((node) => node.textContent?.includes("first"))!;
    first.click();
    await flush(20);
    expect($$(".message-body")[0].textContent).toContain("first conversation");
    expect(bodyText()).not.toContain("second conversation…");
  });

  it("renames a chat", async () => {
    await loadApp();
    await sendAndWait("rename me");
    click("[data-chat-menu]");
    await flush(10);
    click("[data-chat-rename]");
    await flush(10);
    const input = $<HTMLInputElement>("#renameInput")!;
    input.value = "Renamed conversation";
    press("#renameInput", "Enter");
    await flush(20);
    expect($(".recent")!.textContent).toContain("Renamed conversation");
  });

  it("deletes a chat", async () => {
    const confirms: string[] = [];
    window.confirm = (message?: string) => {
      confirms.push(message ?? "");
      return true;
    };
    await loadApp();
    await sendAndWait("delete me");
    expect($$(".recent").length).toBe(1);
    click("[data-chat-menu]");
    await flush(10);
    click("[data-chat-delete]");
    await flush(20);
    expect(confirms[0]).toContain("delete me");
    expect($$(".recent").length).toBe(0);
  });

  it("persists chats across a reload", async () => {
    await loadApp();
    await sendAndWait("survive the reload");
    await loadApp({ keepStorage: true });
    expect($$(".recent").length).toBe(1);
    expect($(".recent")!.textContent).toContain("survive the reload");
    $(".recent")!.click();
    await flush(20);
    expect($$(".message-body")[0].textContent).toContain("survive the reload");
  });

  it("never restores a stuck streaming caret from storage", async () => {
    await loadApp();
    type("#composerInput", "interrupted by a crash");
    click("#sendButton");
    await flush(20);
    // Reload while the reply is still in flight.
    await loadApp({ keepStorage: true });
    $(".recent")?.click();
    await flush(20);
    expect($(".stream-caret")).toBeNull();
    expect($("#sendButton")).toBeTruthy();
  });
});

describe("Issue 10 — the useless download logos are gone", () => {
  it("renders no icon column in the downloads list", async () => {
    await loadApp();
    click('[data-view="downloads"]');
    await flush(20);
    expect($$(".download-row").length).toBeGreaterThan(0);
    expect($$(".download-file-icon")).toHaveLength(0);
    expect($$(".download-row svg")).toHaveLength(0);
    expect(cssSource()).not.toContain(".download-file-icon");
  });

  it("still shows the name, detail, progress and state of each row", async () => {
    await loadApp();
    click('[data-view="downloads"]');
    await flush(20);
    const row = $(".download-row")!;
    expect(row.querySelector(".download-name")?.textContent).toBeTruthy();
    // The first row is the model, whose filler "Verified and ready" detail was
    // removed; its detail is now deliberately empty. A row with real detail
    // (the search support or an indexed docset) must still show it.
    const detailed = $$(".download-row").find((candidate) => (candidate.querySelector(".download-detail")?.textContent ?? "").length > 0)!;
    expect(detailed).toBeTruthy();
    expect(detailed.querySelector(".download-detail")?.textContent).toBeTruthy();
    expect(bodyText()).not.toContain("Verified and ready");
    expect(row.querySelector(".progress-track")).toBeTruthy();
    expect(row.querySelector(".download-state")?.textContent).toBeTruthy();
  });
});

describe("Issue 11 — the data-handling message is gone", () => {
  it("removes the privacy box from Settings", async () => {
    await loadApp();
    click("#settingsButton");
    await flush(20);
    expect($$(".privacy-box")).toHaveLength(0);
    expect(bodyText()).not.toContain("Data handling");
    expect(bodyText()).not.toContain("Telemetry is disabled");
  });

  it("removes it from the source reader too", async () => {
    await loadApp();
    await sendAndWait("vector move");
    $$(".source-chip")[0].click();
    await waitFor(() => Boolean($("#readerTitle")));
    await flush(250);
    expect($$(".privacy-box")).toHaveLength(0);
    expect(bodyText()).not.toContain("Local source.");
  });
});

describe("Issue 13 — transitions are smooth and nothing flashes after a press", () => {
  it("morphs the DOM instead of rebuilding it, so nodes survive a render", async () => {
    const mod: any = await loadApp();
    const sidebar = $(".sidebar")!;
    const composer = $("#composerInput")!;
    mod.__test.render();
    mod.__test.render();
    // Identity is preserved: CSS transitions can only interpolate if the very
    // same element is still in the document.
    expect($(".sidebar")).toBe(sidebar);
    expect($("#composerInput")).toBe(composer);
  });

  it("declares transitions on the interactive surfaces", () => {
    const css = cssSource();
    expect(css).toMatch(/\.nav-item[^{]*\{[^}]*transition:/s);
    expect(/transition:\s*[^;]*background-color \d+ms/.test(css)).toBe(true);
  });

  it("never replays entrance animations on unrelated re-renders (no flash)", async () => {
    const mod: any = await loadApp();
    click("#settingsButton");
    await flush(20);
    expect($(".modal-backdrop")!.className).toContain("entering");
    // Any subsequent render of the already-open modal must not re-trigger it.
    mod.__test.render();
    mod.__test.render();
    expect($(".modal-backdrop")!.className).not.toContain("entering");
  });

  it("does not re-animate messages that are already on screen", async () => {
    const mod: any = await loadApp();
    await sendAndWait("hello");
    mod.__test.render();
    expect($$(".message-enter")).toHaveLength(0);
  });

  it("does not animate the page background, which is what caused the flash", () => {
    const css = cssSource();
    const bodyRule = /(^|\})\s*body\s*\{([^}]*)\}/m.exec(css)?.[2] ?? "";
    expect(bodyRule).not.toContain("transition");
    // The app shell itself must not fade either.
    expect(/\.app-shell[^{]*\{[^}]*animation:/s.test(css)).toBe(false);
  });

  it("survives rapid view switching with every control still live", async () => {
    await loadApp();
    for (let i = 0; i < 8; i += 1) {
      click('[data-view="docs"]');
      click('[data-view="downloads"]');
      click('[data-view="chat"]');
    }
    await flush(30);
    expect($("#composerInput")).toBeTruthy();
    await sendAndWait("still responsive");
    expect($$(".message").length).toBe(2);
  });
});

describe("Issue 14 — light → dark round trips leave nothing broken", () => {
  it("returns to the original theme and keeps the toggle correct", async () => {
    await loadApp({ theme: "dark" });
    click("#themeToggle");
    await flush(20);
    expect(document.documentElement.dataset.theme).toBe("light");
    click("#themeToggle");
    await flush(20);
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect($("#themeToggle")!.getAttribute("aria-label")).toContain("light");
  });

  it("leaves the whole app interactive after a round trip", async () => {
    await loadApp({ theme: "dark" });
    click("#themeToggle");
    await flush(20);
    click("#themeToggle");
    await flush(20);

    // Navigation.
    click('[data-view="docs"]');
    await flush(20);
    expect($$(".doc-card").length).toBe(5);
    click('[data-view="chat"]');
    await flush(20);

    // The mode menu.
    click("#modelButton");
    await flush(10);
    expect($(".mode-popover")).toBeTruthy();
    click('[data-mode="think"]');
    await flush(10);
    expect($("#modelButton")!.textContent).toContain("Think");

    // Sending.
    await sendAndWait("after theme round trip");
    expect($$(".message").length).toBe(2);

    // History.
    expect($$(".recent").length).toBe(1);
  });

  it("keeps Settings consistent when the theme is changed from inside it", async () => {
    await loadApp({ theme: "dark" });
    click("#settingsButton");
    await flush(20);
    click("#settingsTheme");
    await flush(20);
    expect(document.documentElement.dataset.theme).toBe("light");
    expect($(".setting-detail")!.textContent).toBe("Light");
    click("#settingsTheme");
    await flush(20);
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect($(".setting-detail")!.textContent).toBe("Dark");
    // Settings is still open and closable.
    click("#closeSettings");
    await flush(20);
    expect($(".settings-panel")).toBeNull();
  });

  it("preserves the theme and an in-flight conversation together", async () => {
    await loadApp({ theme: "dark" });
    await sendAndWait("before the toggle");
    click("#themeToggle");
    await flush(20);
    click("#themeToggle");
    await flush(20);
    expect($$(".message").length).toBe(2);
    expect($$(".message-body")[1].textContent).toContain("TaskGroup");
  });

  it("uses theme variables for scrims and shadows, never hardcoded dark values", () => {
    const css = cssSource();
    // Every scrim/shadow must resolve through a variable so light mode adapts.
    expect(css).toContain("--scrim");
    expect(/\.modal-backdrop\s*\{[^}]*background:\s*var\(--scrim\)/.test(css)).toBe(true);
    const offenders = css
      .split("\n")
      .filter((line) => /rgba\(\s*0\s*,\s*0\s*,\s*0/.test(line))
      .filter((line) => !line.includes("--shadow") && !line.includes("--scrim"));
    expect(offenders).toEqual([]);
  });

  it("persists the theme across a reload", async () => {
    await loadApp({ theme: "dark" });
    click("#themeToggle");
    await flush(20);
    await loadApp({ keepStorage: true });
    expect(document.documentElement.dataset.theme).toBe("light");
  });
});

describe("Q5 default, Q8 gating and setup failures", () => {
  it("defaults to Q5 on a fresh install", async () => {
    await loadApp({ onboarded: false });
    click("#setupNext");
    await flush(20);
    expect(bodyText()).toContain("Choose a model size");
    expect($('[data-quant="q5"]')!.className).toContain("selected");
    expect($('[data-quant="q8"]')!.className).not.toContain("selected");
    // Nothing is persisted until the user actually makes a choice.
    expect(localStorage.getItem("veda:quant")).toBeNull();
  });

  it("disables Q8 below the 12 GiB memory floor and explains why", async () => {
    await loadApp({
      onboarded: false,
      bridge: (actual) => ({
        ...actual,
        preflight: async () => ({ ...(await actual.preflight()), totalMemoryBytes: 8 * 1024 ** 3 }),
      }),
    });
    click("#setupNext");
    await flush(20);
    const q8 = $<HTMLButtonElement>('[data-quant="q8"]')!;
    expect(q8.disabled).toBe(true);
    expect(q8.getAttribute("aria-disabled")).toBe("true");
    expect(bodyText()).toContain("Q8 requires 12 GB of physical memory");
    // Clicking the disabled card must not change the selection.
    q8.click();
    await flush(20);
    expect($('[data-quant="q5"]')!.className).toContain("selected");
    expect($('[data-quant="q8"]')!.className).not.toContain("selected");
  });

  it("remembers the chosen model across restarts", async () => {
    await loadApp({ onboarded: false });
    click("#setupNext");
    await flush(20);
    click('[data-quant="q8"]');
    await flush(20);
    click("#setupNext");
    await flush(20);
    click("#setupNext");
    await waitFor(() => !$(".onboarding"), 15000);
    expect(localStorage.getItem("veda:quant")).toBe("q8");

    // On restart the q8 model the setup downloaded is still on disk, so the
    // stored choice is honoured.
    const reloaded: any = await loadApp({
      keepStorage: true,
      bridge: (actual) => ({
        ...actual,
        downloads: async () => {
          const items = await actual.downloads();
          return items.some((item) => item.id === "minicpm5-q8")
            ? items
            : [
                { id: "minicpm5-q8", name: "MiniCPM 5 · Q8", detail: "", state: "installed", progress: 100, downloadedBytes: 1_153_529_261, totalBytes: 1_153_529_261 },
                ...items,
              ];
        },
      }),
    });
    expect(reloaded.__test.state.selectedQuant).toBe("q8");
  });

  it("surfaces preflight warnings (e.g. low available memory) on the system check", async () => {
    await loadApp({
      onboarded: false,
      bridge: (actual) => ({
        ...actual,
        preflight: async () => ({
          ...(await actual.preflight()),
          warnings: ["Less than 3 GiB of memory is currently available; close other apps before loading the model."],
        }),
      }),
    });
    expect(bodyText()).toContain("Check this device");
    expect(bodyText()).toContain("Less than 3 GiB of memory is currently available");
    expect($$(".preflight-warning").length).toBe(1);
  });

  it("sends the selected model quant with each request so the backend loads the same model Settings shows", async () => {
    let seen: string | undefined;
    await loadApp({
      bridge: (actual) => ({
        ...actual,
        ask: (request: any) => {
          seen = request.modelQuant;
          return actual.ask(request);
        },
      }),
    });
    await sendAndWait("hello");
    expect(seen).toBe("q5");
  });

  it("keeps the docs filter applied across a re-render", async () => {
    const mod: any = await loadApp();
    click('[data-view="docs"]');
    await flush(20);
    type("#docSearch", "python");
    await flush(10);
    expect($$('.doc-card:not([hidden])').length).toBe(1);
    // A re-render (download progress, theme toggle, …) must not silently
    // clear the filter that is still showing in the search box.
    mod.__test.render();
    mod.__test.render();
    expect($$('.doc-card:not([hidden])').length).toBe(1);
    // Clearing the box restores every card.
    type("#docSearch", "");
    await flush(10);
    expect($$('.doc-card:not([hidden])').length).toBe(5);
  });

  it("clamps a stored q8 choice on a machine below the floor and keeps storage truthful", async () => {
    localStorage.setItem("veda:quant", "q8");
    const mod: any = await loadApp({
      keepStorage: true,
      bridge: (actual) => ({ ...actual, preflight: async () => ({ ...(await actual.preflight()), totalMemoryBytes: 8 * 1024 ** 3 }) }),
    });
    expect(mod.__test.state.selectedQuant).toBe("q5");
    expect(localStorage.getItem("veda:quant")).toBe("q5");
  });

  it("shows the setup failure with guidance and returns Back to the model step", async () => {
    await loadApp({
      onboarded: false,
      bridge: (actual) => ({
        ...actual,
        prepareResources: () => Promise.reject(new Error("disk full")),
      }),
    });
    click("#setupNext");
    await flush(20);
    click("#setupNext");
    await flush(20);
    click("#setupNext");
    await waitFor(() => bodyText().includes("Something went wrong"), 6000);
    expect(bodyText()).toContain("disk full");
    expect(bodyText()).toContain("retrying resumes where it stopped");
    // Back returns to the step that failed (the model choice), not the end.
    click("#setupCancelError");
    await flush(20);
    expect(bodyText()).toContain("Choose a model size");
    expect($('[data-quant="q5"]')!.className).toContain("selected");
  });

  it("reports the device-sized automatic context in Settings", async () => {
    await loadApp();
    click("#settingsButton");
    await flush(20);
    expect($("#contextDetail")!.textContent).toContain("Automatic");
    expect($("#contextDetail")!.textContent).toContain("16,384 on this device");
  });
});

describe("Issue 26 — the Think/Fast chip and the brain/bolt SVGs are gone from the model control", () => {
  it("renders only the model name on the button: 'MiniCPM 5' or 'MiniCPM 5 Think'", async () => {
    await loadApp();
    const button = $("#modelButton")!;
    expect(button.textContent).toBe("MiniCPM 5");
    // No mode chip anywhere.
    expect($(".mode-tag")).toBeNull();
    expect(cssSource()).not.toContain(".mode-tag");
    // The only SVG on the button is the chevron affordance; the bolt that
    // used to sit next to the name is gone.
    expect(button.querySelectorAll("svg").length).toBe(1);
    expect(button.querySelector('[viewBox="0 0 24 24"] path[d^="M13 2"]')).toBeNull();

    // Selecting Think spells it out on the button.
    click("#modelButton");
    await flush(10);
    click('[data-mode="think"]');
    await flush(10);
    expect($("#modelButton")!.textContent).toBe("MiniCPM 5 Think");
  });

  it("keeps the mode menu switchable but without brain/bolt icons", async () => {
    await loadApp();
    click("#modelButton");
    await flush(10);
    const menu = $(".mode-popover")!;
    expect(menu.querySelectorAll(".popover-item-icon").length).toBe(0);
    expect(menu.textContent).toContain("Think");
    expect(menu.textContent).toContain("Fast");
    click('[data-mode="fast"]');
    await flush(10);
    expect($("#modelButton")!.textContent).toBe("MiniCPM 5");
  });
});

describe("Issue 27 — the Docs tab is reachable even when setup is not complete", () => {
  it("lets setup be skipped, then Docs can be browsed and docs downloaded", async () => {
    await loadApp({ onboarded: false });
    expect($(".onboarding")).toBeTruthy();
    // Skipping closes the modal instead of locking the whole app.
    click("#setupSkip");
    await flush(20);
    expect($(".onboarding")).toBeNull();

    // The Docs tab is fully usable without the model.
    click('[data-view="docs"]');
    await flush(30);
    expect($$(".doc-card").length).toBe(5);
    expect($(".library-summary")!.textContent).toMatch(/installed pages across \d+ docsets/);

    // Downloads still work after skipping setup, and stay inline on the Docs
    // tab instead of reopening the first-run modal.
    const install = $$(".install-doc").find((node) => node.dataset.docset === "html")!;
    expect(install).toBeTruthy();
    expect($(".onboarding")).toBeNull();
    install.click();
    await flush(20);
    expect($(".onboarding")).toBeNull();
    await waitFor(() => {
      const card = $$(".doc-card").find((c) => c.textContent?.includes("HTML"));
      return Boolean(card?.textContent?.includes("Installed"));
    }, 12000);
    const htmlCard = $$(".doc-card").find((card) => card.textContent?.includes("HTML"))!;
    expect(htmlCard.textContent).toContain("Installed");

    // The skip is remembered across restarts, so the app never re-traps the
    // user behind the modal on every launch even when onboarding is still
    // flagged as incomplete.
    const mod: any = await loadApp({ onboarded: false, keepStorage: true });
    expect(mod.__test.state.onboardingOpen).toBe(false);
    expect(localStorage.getItem("veda:onboarding-skipped")).toBe("true");
  });

  it("offers 'Set up Veda' from Settings when the model is missing", async () => {
    await loadApp({
      bridge: (actual) => ({
        ...actual,
        downloads: async () => {
          const items = await actual.downloads();
          return items.filter((item: any) => !item.id.startsWith("minicpm5-"));
        },
      }),
    });
    click("#settingsButton");
    await flush(20);
    expect($("#settingsSetup")).toBeTruthy();
    click("#settingsSetup");
    await flush(20);
    // The full setup flow opens from Settings and the skip flag is cleared.
    expect($(".onboarding")).toBeTruthy();
    expect(bodyText()).toContain("Check this device");
    expect(localStorage.getItem("veda:onboarding-skipped")).toBeNull();
  });
});

describe("Docs install/remove can never trap the app", () => {
  it("keeps the Docs tab inline during an install and never opens the onboarding modal", async () => {
    await loadApp({ onboarded: false });
    click("#setupSkip");
    await flush(20);
    click('[data-view="docs"]');
    await flush(20);
    const install = $$(".install-doc").find((node) => node.dataset.docset === "html")!;
    install.click();
    await flush(20);
    // The first-run modal must NOT be hijacked by a Docs-tab install.
    expect($(".onboarding")).toBeNull();
    // The card transitions to an in-progress state, not a stuck "Download".
    const card = $$(".doc-card").find((c) => c.textContent?.includes("HTML"))!;
    expect(/downloading|indexing/.test(card.className + card.textContent)).toBe(true);
    await waitFor(() => Boolean($$(".doc-card").find((c) => c.textContent?.includes("HTML") && c.textContent?.includes("Installed"))), 12000);
  });

  it("recovers from a failed install without trapping or stranding the card", async () => {
    await loadApp({
      onboarded: true,
      bridge: (actual) => ({
        ...actual,
        installDocset: (_id: string) => Promise.reject(new Error("embedding runtime missing")),
      }),
    });
    click('[data-view="docs"]');
    await flush(20);
    const install = $$(".install-doc").find((node) => node.dataset.docset === "html")!;
    install.click();
    await waitFor(() => bodyText().includes("Could not install HTML"), 4000);
    // No modal, and the card is back to an actionable state (Download again).
    expect($(".onboarding")).toBeNull();
    const card = $$(".doc-card").find((c) => c.textContent?.includes("HTML"))!;
    expect(card.textContent).not.toContain("downloading");
    expect($$(".doc-card").find((c) => c.textContent?.includes("HTML"))!.querySelector(".install-doc")).toBeTruthy();
  });
});
