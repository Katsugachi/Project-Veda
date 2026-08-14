/**
 * Guards behaviour that already worked before the repairs, so the fixes
 * cannot silently break anything else.
 */
import { describe, expect, it } from "vitest";
import { $, $$, click, flush, loadApp, press, type, waitFor } from "./harness";
import { renderMarkdown, escapeHtml } from "../src/markdown";
import { bytes, timeLabel } from "../src/format";
import { groupChats, deriveTitle, loadChats, saveChats } from "../src/chats";
import { morph } from "../src/dom";

const bodyText = () => document.body.textContent ?? "";

async function sendAndWait(message: string) {
  type("#composerInput", message);
  click("#sendButton");
  await waitFor(() => !$("#stopButton"), 6000);
}

describe("shell", () => {
  it("renders the sidebar, topbar and chat view", async () => {
    await loadApp();
    expect($(".sidebar")).toBeTruthy();
    expect($(".topbar")).toBeTruthy();
    expect($(".chat-view")).toBeTruthy();
    expect($(".page-title")!.textContent).toBe("New chat");
  });

  it("collapses and expands the sidebar, and remembers the choice", async () => {
    await loadApp();
    click("#collapseSidebar");
    await flush(20);
    expect($(".app-shell")!.className).toContain("sidebar-collapsed");
    expect(localStorage.getItem("veda:sidebar")).toBe("collapsed");
    click("#wordmark");
    await flush(20);
    expect($(".app-shell")!.className).not.toContain("sidebar-collapsed");

    click("#collapseSidebar");
    await flush(20);
    await loadApp({ keepStorage: true });
    expect($(".app-shell")!.className).toContain("sidebar-collapsed");
  });

  it("navigates between all three views", async () => {
    await loadApp();
    click('[data-view="docs"]');
    await flush(20);
    expect($(".content-view")).toBeTruthy();
    expect($(".page-title")!.textContent).toBe("Documentation library");
    click('[data-view="downloads"]');
    await flush(20);
    expect($(".page-title")!.textContent).toBe("Downloads & storage");
    click('[data-view="chat"]');
    await flush(20);
    expect($(".chat-view")).toBeTruthy();
  });

  it("marks the active nav item", async () => {
    await loadApp();
    click('[data-view="docs"]');
    await flush(20);
    expect($('[data-view="docs"]')!.className).toContain("active");
    expect($('[data-view="downloads"]')!.className).not.toContain("active");
  });
});

describe("composer", () => {
  it("keeps the draft across an unrelated re-render", async () => {
    const mod: any = await loadApp();
    type("#composerInput", "a half-written question");
    mod.__test.render();
    expect($<HTMLTextAreaElement>("#composerInput")!.value).toBe("a half-written question");
  });

  it("keeps the draft when the theme is toggled", async () => {
    await loadApp();
    type("#composerInput", "keep me");
    click("#themeToggle");
    await flush(20);
    expect($<HTMLTextAreaElement>("#composerInput")!.value).toBe("keep me");
  });

  it("sends on Enter but not on Shift+Enter", async () => {
    await loadApp();
    type("#composerInput", "enter sends");
    press("#composerInput", "Enter");
    await waitFor(() => !$("#stopButton"), 5000);
    expect($$(".message").length).toBe(2);

    const before = $$(".message").length;
    type("#composerInput", "shift does not");
    $("#composerInput")!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", shiftKey: true, bubbles: true, cancelable: true }),
    );
    await flush(60);
    expect($$(".message").length).toBe(before);
  });

  it("ignores an empty or whitespace-only message", async () => {
    await loadApp();
    type("#composerInput", "   ");
    click("#sendButton");
    await flush(80);
    expect($$(".message").length).toBe(0);
  });

  it("clears the draft after sending", async () => {
    await loadApp();
    await sendAndWait("clear me");
    expect($<HTMLTextAreaElement>("#composerInput")!.value).toBe("");
  });

  it("shows the installed docset count", async () => {
    await loadApp();
    expect($("#scopeButton")!.textContent).toContain("2 docsets");
  });
});

describe("sources and the reader", () => {
  it("renders source chips on an answer", async () => {
    await loadApp();
    await sendAndWait("vector move");
    expect($$(".source-chip").length).toBe(2);
    expect($(".source-chip")!.textContent).toContain("cppreference");
  });

  it("opens a source and closes it again", async () => {
    await loadApp();
    await sendAndWait("vector move");
    $$(".source-chip")[0].click();
    await waitFor(() => Boolean($("#readerTitle")));
    await flush(250);
    expect($("#readerTitle")!.textContent).toBe("Coroutines and Tasks");
    expect($(".message-body pre")).toBeTruthy();
    click("#closeReader");
    await flush(20);
    expect($("#readerTitle")).toBeNull();
  });

  it("closes the reader with Escape", async () => {
    await loadApp();
    await sendAndWait("vector move");
    $$(".source-chip")[0].click();
    await waitFor(() => Boolean($("#readerTitle")));
    await flush(250);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await flush(20);
    expect($("#readerTitle")).toBeNull();
  });

  it("reports a source that cannot be opened", async () => {
    await loadApp({
      bridge: (actual) => ({ ...actual, readSource: () => Promise.reject(new Error("chunk missing")) }),
    });
    await sendAndWait("vector move");
    $$(".source-chip")[0].click();
    await waitFor(() => bodyText().includes("chunk missing"), 3000);
    expect($(".toast")).toBeTruthy();
  });
});

describe("settings", () => {
  it("opens, closes with the button, the backdrop and Escape", async () => {
    await loadApp();
    click("#settingsButton");
    await flush(20);
    expect($(".settings-panel")).toBeTruthy();
    click("#closeSettings");
    await flush(20);
    expect($(".settings-panel")).toBeNull();

    click("#settingsButton");
    await flush(20);
    click("#settingsBackdrop");
    await flush(20);
    expect($(".settings-panel")).toBeNull();

    click("#settingsButton");
    await flush(20);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await flush(20);
    expect($(".settings-panel")).toBeNull();
  });

  it("shows the model, appearance and version rows", async () => {
    await loadApp();
    click("#settingsButton");
    await flush(20);
    const text = $(".settings-body")!.textContent ?? "";
    expect(text).toContain("Appearance");
    expect(text).toContain("MiniCPM 5");
    expect(text).toContain("Version 0.1.0");
  });

  it("changes the reasoning mode from Settings", async () => {
    await loadApp();
    click("#settingsButton");
    await flush(20);
    click("#settingsMode");
    await flush(20);
    expect($(".settings-body")!.textContent).toContain("Think");
    click("#closeSettings");
    await flush(20);
    expect($("#modelButton")!.textContent).toContain("Think");
  });

  it("clears chat history on request", async () => {
    window.confirm = () => true;
    await loadApp();
    await sendAndWait("something to clear");
    expect($$(".recent").length).toBe(1);
    click("#settingsButton");
    await flush(20);
    click("#clearHistory");
    await flush(30);
    expect($$(".recent").length).toBe(0);
  });
});

describe("docs install and remove", () => {
  it("installs an available docset through the blocking flow", async () => {
    await loadApp();
    click('[data-view="docs"]');
    await flush(20);
    const install = $$(".install-doc").find((node) => node.dataset.docset === "html")!;
    expect(install).toBeTruthy();
    install.click();
    await waitFor(() => !$(".onboarding"), 12000);
    const htmlCard = $$(".doc-card").find((card) => card.textContent?.includes("HTML"))!;
    expect(htmlCard.textContent).toContain("Installed");
  });

  it("refuses to remove the last remaining docset", async () => {
    window.confirm = () => true;
    await loadApp();
    click('[data-view="docs"]');
    await flush(20);
    // Two are installed by default; remove one, then the guard trips.
    $$(".remove-doc")[0].click();
    await waitFor(() => $$(".remove-doc").length === 1, 8000);
    $$(".remove-doc")[0].click();
    await waitFor(() => bodyText().includes("Keep at least one"), 3000);
    expect($$(".remove-doc").length).toBe(1);
  });

  it("filters the docs list", async () => {
    await loadApp();
    click('[data-view="docs"]');
    await flush(20);
    type("#docSearch", "python");
    await flush(20);
    const visible = $$(".doc-card").filter((card) => !card.hidden);
    expect(visible.length).toBe(1);
    expect(visible[0].textContent).toContain("Python");
  });

  it("jumps to Docs from the composer scope menu", async () => {
    await loadApp();
    click("#scopeButton");
    await flush(20);
    $('.popover-item.action')!.click();
    await flush(20);
    expect($(".page-title")!.textContent).toBe("Documentation library");
  });
});

describe("onboarding", () => {
  it("runs the three-step setup and closes", async () => {
    await loadApp({ onboarded: false });
    expect($(".onboarding")).toBeTruthy();
    expect(bodyText()).toContain("Check this device");
    click("#setupNext");
    await flush(20);
    expect(bodyText()).toContain("Choose a model size");
    click('[data-quant="q8"]');
    await flush(20);
    expect($('[data-quant="q8"]')!.className).toContain("selected");
    click("#setupNext");
    await flush(20);
    expect(bodyText()).toContain("Choose documentation");
    click("#setupNext");
    await waitFor(() => !$(".onboarding"), 15000);
    expect(localStorage.getItem("veda:onboarded")).toBe("true");
  });

  it("can go back a step", async () => {
    await loadApp({ onboarded: false });
    click("#setupNext");
    await flush(20);
    click("#setupBack");
    await flush(20);
    expect(bodyText()).toContain("Check this device");
  });

  it("surfaces a setup failure and allows a retry", async () => {
    let fail = true;
    await loadApp({
      onboarded: false,
      bridge: (actual) => ({
        ...actual,
        prepareResources: (quant: any) =>
          fail ? Promise.reject(new Error("disk full")) : actual.prepareResources(quant),
      }),
    });
    click("#setupNext");
    await flush(20);
    click("#setupNext");
    await flush(20);
    click("#setupNext");
    await waitFor(() => bodyText().includes("disk full"), 6000);
    fail = false;
    click("#setupRetry");
    await waitFor(() => !$(".onboarding"), 15000);
  });
});

describe("attachments", () => {
  it("does not render an attachment row when there is nothing attached", async () => {
    await loadApp();
    expect($(".attachment-row")).toBeNull();
  });
});

describe("pure helpers", () => {
  it("formats byte sizes", () => {
    expect(bytes(512)).toBe("512 B");
    expect(bytes(1024)).toBe("1.0 KB");
    expect(bytes(16_737_282)).toBe("16 MB");
  });

  it("formats a time label", () => {
    expect(typeof timeLabel(Date.now())).toBe("string");
  });

  it("escapes HTML", () => {
    expect(escapeHtml('<script>"x"</script>')).toBe(
      "&lt;script&gt;&quot;x&quot;&lt;/script&gt;",
    );
  });

  it("renders markdown: code, lists, headings and emphasis", () => {
    expect(renderMarkdown("```python\nx = 1\n```")).toContain('<pre data-language="python">');
    expect(renderMarkdown("- one\n- two")).toContain("<ul><li>one</li><li>two</li></ul>");
    expect(renderMarkdown("# Title")).toContain("<h1>Title</h1>");
    expect(renderMarkdown("**bold**")).toContain("<strong>bold</strong>");
    expect(renderMarkdown("`code`")).toContain("<code>code</code>");
  });

  it("does not let markdown inject HTML", () => {
    expect(renderMarkdown("<img src=x onerror=alert(1)>")).not.toContain("<img");
  });

  it("derives chat titles", () => {
    expect(deriveTitle("  hello   world ")).toBe("hello world");
    expect(deriveTitle("")).toBe("New chat");
    expect(deriveTitle("x".repeat(80)).length).toBeLessThanOrEqual(48);
  });

  it("groups chats by recency", () => {
    const now = Date.now();
    const day = 86_400_000;
    const groups = groupChats(
      [
        { id: "a", title: "a", createdAt: now, updatedAt: now, messages: [] },
        { id: "b", title: "b", createdAt: now, updatedAt: now - 3 * day, messages: [] },
        { id: "c", title: "c", createdAt: now, updatedAt: now - 40 * day, messages: [] },
      ],
      now,
    );
    expect(groups.map((group) => group.label)).toEqual(["Today", "Previous 7 days", "Older"]);
  });

  it("survives corrupt persisted chat data", () => {
    localStorage.setItem("veda:chats", "{not json");
    expect(loadChats()).toEqual([]);
    localStorage.setItem("veda:chats", JSON.stringify([{ nope: true }, null, 5]));
    expect(loadChats()).toEqual([]);
    localStorage.removeItem("veda:chats");
  });

  it("round-trips chats through storage without the streaming flag", () => {
    saveChats([
      {
        id: "x",
        title: "t",
        createdAt: 1,
        updatedAt: 2,
        messages: [{ id: "m", role: "assistant", content: "hi", createdAt: 3, streaming: true }],
      },
    ]);
    const [chat] = loadChats();
    expect(chat.messages[0].streaming).toBe(false);
    localStorage.removeItem("veda:chats");
  });
});

describe("dom morphing", () => {
  it("preserves matching nodes and updates text in place", () => {
    const root = document.createElement("div");
    morph(root, "<p id='a'>one</p>");
    const paragraph = root.querySelector("#a")!;
    morph(root, "<p id='a'>two</p>");
    expect(root.querySelector("#a")).toBe(paragraph);
    expect(paragraph.textContent).toBe("two");
  });

  it("reorders keyed nodes instead of recreating them", () => {
    const root = document.createElement("div");
    morph(root, "<i data-key='1'>1</i><i data-key='2'>2</i>");
    const first = root.querySelector("[data-key='1']")!;
    morph(root, "<i data-key='2'>2</i><i data-key='1'>1</i>");
    expect(root.querySelector("[data-key='1']")).toBe(first);
    expect(root.firstElementChild!.getAttribute("data-key")).toBe("2");
  });

  it("adds and removes nodes and drops stale attributes", () => {
    const root = document.createElement("div");
    morph(root, "<p class='x' title='t'>a</p><p>b</p>");
    expect(root.children.length).toBe(2);
    morph(root, "<p class='y'>a</p>");
    expect(root.children.length).toBe(1);
    expect(root.firstElementChild!.getAttribute("title")).toBeNull();
    expect(root.firstElementChild!.className).toBe("y");
  });

  it("never clobbers what the user is typing in a textarea", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    morph(root, "<textarea id='t'></textarea>");
    const area = root.querySelector<HTMLTextAreaElement>("#t")!;
    area.value = "in progress";
    morph(root, "<textarea id='t'></textarea>");
    expect(root.querySelector("#t")).toBe(area);
    expect(area.value).toBe("in progress");
    root.remove();
  });

  it("replaces a node when the tag changes", () => {
    const root = document.createElement("div");
    morph(root, "<p>a</p>");
    morph(root, "<span>a</span>");
    expect(root.firstElementChild!.tagName).toBe("SPAN");
  });
});

describe("sidebar history layout", () => {
  it("hides chat history when the sidebar is collapsed", async () => {
    await loadApp();
    await sendAndWait("a saved chat");
    expect($(".recent-area")).toBeTruthy();
    click("#collapseSidebar");
    await flush(20);
    // The rule is CSS-driven; assert the collapsed shell is applied and the
    // stylesheet hides the history region.
    expect($(".app-shell")!.className).toContain("sidebar-collapsed");
    const css = (await import("./harness")).cssSource();
    expect(/\.sidebar-collapsed[^{]*\.recent-area[^{]*\{[^}]*display:\s*none/.test(css)).toBe(true);
  });

  it("gives the history list its own scroll area", async () => {
    const css = (await import("./harness")).cssSource();
    expect(/\.recents\s*\{[^}]*overflow-y:\s*auto/.test(css)).toBe(true);
  });

  it("groups saved chats under a heading", async () => {
    await loadApp();
    await sendAndWait("grouped chat");
    expect($(".section-label")!.textContent).toBe("Today");
  });

  it("does not list a chat that has no messages", async () => {
    await loadApp();
    expect($$(".recent").length).toBe(0);
    expect(bodyText()).toContain("Your chats appear here.");
  });

  it("reuses the empty chat instead of stacking new ones", async () => {
    const mod: any = await loadApp();
    click("#newChat");
    click("#newChat");
    await flush(20);
    expect(mod.__test.state.chats.length).toBe(1);
  });
});
