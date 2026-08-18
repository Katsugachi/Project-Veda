/**
 * Regression tests for the Round 8 repairs:
 *   - the composer menus no longer run off the bottom of the screen,
 *   - the streaming activity line rotates through Claude-style status verbs,
 *   - inline [S1] citations are clickable and open the source,
 *   - the Docs tab gates installs on the model and mirrors live progress.
 */
import { describe, expect, it } from "vitest";
import { $, $$, click, cssSource, flush, loadApp, type, waitFor } from "./harness";
import { renderMarkdown } from "../src/markdown";

describe("Round 8 — dropdown stays on screen", () => {
  it("flips up only when there is no room below the trigger", async () => {
    const mod: any = await loadApp();
    const flip = mod.__test.popoverFlipsUp;

    // Trigger near the bottom, menu taller than the remaining space → up.
    expect(flip({ top: 700, bottom: 733 }, 260, 800)).toBe(true);
    // Plenty of room below → stays down.
    expect(flip({ top: 100, bottom: 133 }, 260, 800)).toBe(false);
    // Neither side fits; more room above → up.
    expect(flip({ top: 400, bottom: 433 }, 500, 800)).toBe(true);
    // Neither side fits; more room below → down.
    expect(flip({ top: 10, bottom: 43 }, 790, 800)).toBe(false);
  });

  it("declares the upward-flip geometry in CSS", () => {
    const css = cssSource();
    expect(/\.popover\.flip-up\s*\{[^}]*top:\s*auto/.test(css)).toBe(true);
    expect(/\.popover\.flip-up\s*\{[^}]*bottom:\s*calc\(100% \+ 8px\)/.test(css)).toBe(true);
  });

  it("keeps the mode menu anchored to its trigger", async () => {
    await loadApp();
    click("#modelButton");
    await flush(20);
    const menu = $(".mode-popover")!;
    expect(menu).toBeTruthy();
    expect(menu.parentElement!.classList.contains("menu-anchor")).toBe(true);
  });
});

describe("Round 8 — rotating Claude-style activity status", () => {
  it("rotates status verbs instead of a static 'Searching installed docs…'", async () => {
    let resolveAsk!: (value: unknown) => void;
    const mod: any = await loadApp({
      bridge: (actual) => ({
        ...actual,
        ask: () => new Promise((resolve) => (resolveAsk = resolve)),
      }),
    });

    type("#composerInput", "hi");
    click("#sendButton");
    await flush(30);

    const status = $(".assistant-status .status-text");
    expect(status).toBeTruthy();
    expect(status!.textContent).toBe("Pondering…");

    mod.__test.advanceStatuses();
    await flush(10);
    expect($(".assistant-status .status-text")!.textContent).toBe("Thinking…");

    // Once the answer lands the status line is replaced by real content.
    resolveAsk({ content: "Hello!", sources: [] });
    await flush(60);
    expect($(".assistant-status")).toBeNull();
    expect($$(".message-body")[1].textContent).toContain("Hello!");
  });

  it("exposes every expected stage in the rotation", async () => {
    const mod: any = await loadApp();
    const phrases: string[] = mod.__test.statusPhrases;
    expect(phrases).toContain("Pondering…");
    expect(phrases).toContain("Thinking…");
    expect(phrases).toContain("Searching installed docs…");
    expect(phrases).toContain("Crystallising…");
    expect(phrases).toContain("Substituting…");
    expect(phrases).toContain("Verifying citations…");
  });
});

describe("Round 8 — citations are clickable", () => {
  it("renders inline [S1] as a clickable citation button", () => {
    const html = renderMarkdown("See [S1] for details.");
    expect(html).toContain("<button");
    expect(html).toContain('data-cite="S1"');
    expect(html).toContain("[S1]");
  });

  it("opens the source when an inline citation is clicked", async () => {
    await loadApp();
    type("#composerInput", "vector move");
    click("#sendButton");
    await waitFor(() => !$("#stopButton"), 6000);

    const cite = $(".inline-cite");
    expect(cite).toBeTruthy();
    cite!.click();
    await waitFor(() => Boolean($("#readerTitle")), 3000);
    await flush(250);
    expect($("#readerTitle")!.textContent).toBeTruthy();
  });
});

describe("Round 8 — Docs tab: model gating and live progress", () => {
  it("lets docs be installed without the model, and still offers setup for semantic search", async () => {
    await loadApp({
      bridge: (actual) => ({
        ...actual,
        downloads: async () => {
          const items = await actual.downloads();
          return items.filter((item: any) => !item.id.startsWith("minicpm5-"));
        },
      }),
    });
    click('[data-view="docs"]');
    await flush(30);
    expect($(".setup-required")).toBeTruthy();
    expect(document.body.textContent).toContain("Keyword search works now");

    // Download must stay on the Docs tab — adding docs cannot be gated on
    // MiniCPM. The previous gate is what made the tab feel nonexistent.
    const install = $$(".install-doc").find((node) => node.dataset.docset === "html")!;
    install.click();
    await flush(20);
    expect($(".onboarding")).toBeNull();
    await waitFor(() => {
      const card = $$(".doc-card").find((c) => c.textContent?.includes("HTML"));
      return Boolean(card?.textContent?.includes("Installed"));
    }, 12000);
  });

  it("mirrors source-download progress on the doc card, not just the index phase", async () => {
    await loadApp();
    click('[data-view="docs"]');
    await flush(20);
    const install = $$(".install-doc").find((node) => node.dataset.docset === "html")!;
    install.click();
    await waitFor(() => {
      const card = $$(".doc-card").find((c) => c.textContent?.includes("HTML"));
      return Boolean(card && /downloading/.test(card.textContent ?? ""));
    }, 4000);
    const card = $$(".doc-card").find((c) => c.textContent?.includes("HTML"))!;
    expect(/downloading/.test(card.textContent)).toBe(true);

    await waitFor(() => {
      const c = $$(".doc-card").find((x) => x.textContent?.includes("HTML"));
      return Boolean(c?.textContent?.includes("Installed"));
    }, 12000);
  });
});
