/**
 * Round 10 / bug-scan — these tests fail on the *actual* line-level bugs,
 * not on the presence of a button.
 *
 * Bugs they pin:
 *   - #localDocsInput had no change listener, so "Add your own docs" was dead
 *   - local packs leaked into the first-run setup chooser
 *   - automatic context still advertised 131K
 */
import { describe, expect, it } from "vitest";
import { $, $$, click, flush, loadApp, readSource, waitFor } from "./harness";

const bodyText = () => document.body.textContent ?? "";

function localMarkdown(name: string, body: string): File {
  return new File([body], name, { type: "text/markdown" });
}

async function pickLocalFile(file: File): Promise<void> {
  const input = $<HTMLInputElement>("#localDocsInput");
  if (!input) throw new Error("local docs file input is missing");
  Object.defineProperty(input, "files", { configurable: true, value: [file] });
  input.dispatchEvent(new Event("change", { bubbles: true }));
  await flush(80);
}

describe("Docs tab — Add your own docs is wired, not decorative", () => {
  it("the change handler actually calls importLocalDocFiles", () => {
    // Calling __test.importLocalDocFiles hid the fact that the real file
    // input did nothing. This is the line that was missing.
    expect(readSource("main.ts")).toMatch(
      /localDocsInput[\s\S]{0,80}importLocalDocFiles|importLocalDocFiles[\s\S]{0,80}localDocsInput/,
    );
  });

  it("picking a file through the input creates an installed local card", async () => {
    await loadApp();
    click('[data-view="docs"]');
    await flush(20);
    click("#addLocalDocs");
    await flush(10);
    await pickLocalFile(localMarkdown("notes.md", "# Local notes\n\nstd::vector house style.\n"));
    await waitFor(() => $$(".doc-card").length === 6, 3000);
    const local = $$(".doc-card").find((card) => card.textContent?.includes("Your files"));
    expect(local).toBeTruthy();
    expect(local!.textContent).toContain("Installed");
    expect(local!.querySelector(".remove-doc")).toBeTruthy();
    expect(bodyText()).toMatch(/Added /);
  });

  it("can remove a local library without touching official packs", async () => {
    window.confirm = () => true;
    await loadApp();
    click('[data-view="docs"]');
    await flush(20);
    await pickLocalFile(localMarkdown("tmp.md", "# Temp\n\nbody\n"));
    await waitFor(() => $$(".doc-card").length === 6, 3000);
    const localRemove = $$(".doc-card")
      .find((card) => card.textContent?.includes("Your files"))
      ?.querySelector<HTMLElement>(".remove-doc");
    expect(localRemove).toBeTruthy();
    localRemove!.click();
    await waitFor(() => $$(".doc-card").length === 5, 4000);
    expect(bodyText()).toContain("Python");
    expect(bodyText()).toContain("C++");
  });

  it("does not put user libraries on the first-run setup chooser", async () => {
    await loadApp({
      bridge: (actual) => ({
        ...actual,
        downloads: async () => {
          const items = await actual.downloads();
          return items.filter((item: { id: string }) => !item.id.startsWith("minicpm5-"));
        },
      }),
    });
    click('[data-view="docs"]');
    await flush(20);
    await pickLocalFile(localMarkdown("notes.md", "# Notes\n\nbody\n"));
    await waitFor(() => $$(".doc-card").length === 6, 3000);

    // Re-open setup. Local cards must not become setup-doc toggles — those
    // would call install_docset("local-…") and fail.
    click("#docsSetupNow");
    await flush(20);
    click("#setupNext");
    await flush(20);
    click("#setupNext");
    await flush(20);
    expect(bodyText()).toContain("Choose documentation");
    const setupIds = $$("[data-setup-doc]").map((node) => node.getAttribute("data-setup-doc"));
    expect(setupIds.some((id) => id?.startsWith("local-"))).toBe(false);
    expect(setupIds).toEqual(expect.arrayContaining(["python", "cpp", "html", "css", "javascript"]));
  });
});

describe("Docs tab — official install is not gated on the model", () => {
  it("installs CSS with no MiniCPM and never opens setup", async () => {
    await loadApp({
      bridge: (actual) => ({
        ...actual,
        downloads: async () => {
          const items = await actual.downloads();
          return items.filter((item: { id: string }) => !item.id.startsWith("minicpm5-"));
        },
      }),
    });
    click('[data-view="docs"]');
    await flush(20);
    expect($(".setup-required")).toBeTruthy();
    const install = $$(".install-doc").find((node) => node.dataset.docset === "css")!;
    install.click();
    await flush(20);
    expect($(".onboarding")).toBeNull();
    await waitFor(() => {
      const card = $$(".doc-card").find((c) => c.textContent?.includes("CSS"));
      return Boolean(card?.textContent?.includes("Installed"));
    }, 12000);
  });
});

describe("Automatic context is the fast 16K window", () => {
  it("Settings reports 16,384, not a 131K RAM-filling default", async () => {
    await loadApp();
    click("#settingsButton");
    await flush(20);
    expect($("#contextDetail")!.textContent).toContain("Automatic");
    expect($("#contextDetail")!.textContent).toContain("16,384");
    expect($("#contextDetail")!.textContent).not.toContain("131,072 on this device");
  });
});
