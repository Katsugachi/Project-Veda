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
    expect($("#modelButton")!.textContent).toContain("Fast");
    click("#modelButton");
    await flush(10);
    click('[data-mode="think"]');
    await flush(10);
    expect($("#modelButton")!.textContent).toContain("Think");
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
    expect(row.querySelector(".download-detail")?.textContent).toBeTruthy();
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
