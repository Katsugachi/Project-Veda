/**
 * Round 9 — clip-proof popovers.
 *
 * The placement decision is a pure function; the DOM glue in `placePopovers`
 * cannot run under jsdom (zero layout), so these tests lock the decision logic
 * and the CSS that the runtime toggles.
 */
import { describe, expect, it } from "vitest";
import { $, cssSource, flush, loadApp, type } from "./harness";

const viewport = { viewportTop: 0, viewportBottom: 800 };

describe("Round 9 — popoverPlacement is pure and total", () => {
  it("stays down and unclamped when there is room below", async () => {
    const mod: any = await loadApp();
    const place = mod.__test.popoverPlacement;
    const result = place({ anchorTop: 100, anchorBottom: 133, menuHeight: 260, ...viewport });
    expect(result.flipUp).toBe(false);
    expect(result.maxHeight).toBeUndefined();
  });

  it("flips up when there is no room below but room above", async () => {
    const mod: any = await loadApp();
    const result = mod.__test.popoverPlacement({
      anchorTop: 700,
      anchorBottom: 733,
      menuHeight: 260,
      ...viewport,
    });
    expect(result.flipUp).toBe(true);
    expect(result.maxHeight).toBeUndefined();
  });

  it("clamps to the larger side when neither side fits", async () => {
    const mod: any = await loadApp();
    // More room above → up, clamped to the above space minus margin.
    const up = mod.__test.popoverPlacement({
      anchorTop: 400,
      anchorBottom: 433,
      menuHeight: 500,
      ...viewport,
    });
    expect(up.flipUp).toBe(true);
    expect(up.maxHeight).toBe(400 - 8);

    // More room below → down, clamped to the below space minus margin.
    const down = mod.__test.popoverPlacement({
      anchorTop: 10,
      anchorBottom: 43,
      menuHeight: 790,
      ...viewport,
    });
    expect(down.flipUp).toBe(false);
    expect(down.maxHeight).toBe(800 - 43 - 8);
  });

  it("never produces a negative clamp", async () => {
    const mod: any = await loadApp();
    const result = mod.__test.popoverPlacement({
      anchorTop: 0,
      anchorBottom: 33,
      menuHeight: 9999,
      viewportTop: 0,
      viewportBottom: 40,
    });
    expect(result.maxHeight).toBeGreaterThanOrEqual(0);
  });

  it("respects a scroll-container viewport top offset", async () => {
    const mod: any = await loadApp();
    // Anchor at the top of a scroll box that starts 200px down the page.
    const result = mod.__test.popoverPlacement({
      anchorTop: 205,
      anchorBottom: 238,
      menuHeight: 80,
      viewportTop: 200,
      viewportBottom: 600,
    });
    // No room above (5px), plenty below → down.
    expect(result.flipUp).toBe(false);
  });
});

describe("Round 9 — flip-up geometry is declared in CSS", () => {
  it("declares the recent-menu upward flip", () => {
    const css = cssSource();
    expect(/\.recent-menu\.flip-up\s*\{[^}]*top:\s*auto/.test(css)).toBe(true);
    expect(/\.recent-menu\.flip-up\s*\{[^}]*bottom:\s*calc\(100% \+ 4px\)/.test(css)).toBe(true);
  });

  it("declares the popover upward flip", () => {
    const css = cssSource();
    expect(/\.popover\.flip-up\s*\{[^}]*top:\s*auto/.test(css)).toBe(true);
  });
});

describe("Round 9 — no caret while only the status line shows", () => {
  it("shows the status line without a caret during a pending reply", async () => {
    let resolveAsk!: (value: unknown) => void;
    await loadApp({
      bridge: (actual) => ({
        ...actual,
        ask: () => new Promise((resolve) => (resolveAsk = resolve)),
      }),
    });
    type("#composerInput", "hi");
    $<HTMLButtonElement>("#sendButton")!.click();
    await flush(30);

    expect($(".assistant-status")).toBeTruthy();
    expect($(".stream-caret")).toBeNull();

    resolveAsk({ content: "Hello!", sources: [] });
    await flush(60);
    expect($(".assistant-status")).toBeNull();
  });
});
