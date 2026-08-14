import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { vi } from "vitest";

const here = path.dirname(fileURLToPath(import.meta.url));
export const srcDir = path.resolve(here, "..", "src");

export function readSource(file: string): string {
  return fs.readFileSync(path.join(srcDir, file), "utf8");
}

/** The real stylesheets, concatenated, for assertions about CSS rules. */
export function cssSource(): string {
  return `${readSource("styles.css")}\n${readSource("shape-overrides.css")}`;
}

/** Injects the real stylesheets into the document so styling is present. */
export function installStyles(): void {
  for (const file of ["reset.css", "styles.css", "shape-overrides.css"]) {
    const style = document.createElement("style");
    style.textContent = readSource(file).replace(/@import[^;]+;/g, "");
    document.head.appendChild(style);
  }
}

export function resetDom(): void {
  document.head.innerHTML = "";
  document.body.innerHTML = '<div id="app"></div>';
  document.documentElement.removeAttribute("data-theme");
}

type BridgeOverride = (actual: any) => Record<string, unknown>;

export interface LoadOptions {
  theme?: string;
  onboarded?: boolean;
  /** Keeps localStorage from the previous load, simulating an app restart. */
  keepStorage?: boolean;
  /** Replaces parts of the Tauri/network bridge for this load. */
  bridge?: BridgeOverride;
}

/**
 * Loads a fresh copy of the real app module.
 *
 * Only the bridge (the Tauri/network boundary) can be stubbed; all application
 * code under test is the real thing.
 */
export async function loadApp(options: LoadOptions = {}) {
  vi.resetModules();
  vi.doUnmock("../src/bridge");
  resetDom();

  if (!options.keepStorage) localStorage.clear();
  localStorage.setItem("veda:onboarded", options.onboarded === false ? "false" : "true");
  if (options.theme) localStorage.setItem("veda:theme", options.theme);

  if (options.bridge) {
    vi.doMock("../src/bridge", async () => {
      const actual: any = await vi.importActual("../src/bridge");
      return { bridge: { ...actual.bridge, ...options.bridge!(actual.bridge) } };
    });
  }

  installStyles();
  const mod = await import("../src/main");
  // Let init()'s settled promises and the first render land.
  await flush(60);
  return mod;
}

export const flush = async (ms = 0) => {
  await new Promise((resolve) => setTimeout(resolve, ms));
};

/** Waits until `predicate` is true, polling the real event loop. */
export async function waitFor(predicate: () => boolean, timeout = 4000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (predicate()) return;
    await flush(15);
  }
  throw new Error(`waitFor timed out after ${timeout}ms`);
}

export const $ = <T extends Element = HTMLElement>(selector: string) =>
  document.querySelector<T>(selector);
export const $$ = <T extends Element = HTMLElement>(selector: string) =>
  Array.from(document.querySelectorAll<T>(selector));

export function click(selector: string): void {
  const element = $(selector);
  if (!element) throw new Error(`click target not found: ${selector}`);
  (element as HTMLElement).click();
}

export function type(selector: string, value: string): void {
  const element = $<HTMLTextAreaElement | HTMLInputElement>(selector);
  if (!element) throw new Error(`input not found: ${selector}`);
  element.value = value;
  element.dispatchEvent(new Event("input", { bubbles: true }));
}

export function press(selector: string, key: string): void {
  const element = $(selector);
  if (!element) throw new Error(`key target not found: ${selector}`);
  element.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
}
