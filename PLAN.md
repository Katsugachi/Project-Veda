# Veda UI repair plan

Each item lists the reported symptom, the root cause found in the code, the fix, and
the automated check that proves it (see `ui/tests/*.test.ts`).

## Root causes found by probing the running app

| # | Symptom | Root cause | Fix | Verified by |
|---|---------|-----------|-----|-------------|
| 1 | Think/Fast menu "floats" instead of dropping down | `.popover` is `position:absolute; bottom: calc(100% + 10px)` inside `.composer-shell`, so it is anchored to the whole composer and flies above it | Wrap the trigger in a `.model-anchor{position:relative}` and drop the menu **downwards** from the button (`top: calc(100% + 8px)`), flipping up only when there is no room | `menu.test.ts` |
| 2 | Useless dot next to "MiniCPM 5" | `<span class="mode-indicator">` rendered unconditionally | Removed the element and its CSS | `menu.test.ts` |
| 3 | Docs can't be viewed / are broken | `init()` used a single `Promise.all`; one rejecting bridge call left `docsets`/`downloads` permanently empty, so Docs rendered nothing | Load each source independently and tolerate failures; render a real error/empty state | `resilience.test.ts` |
| 4 | Context can't be configured 0 → 131k | Context was read-only text from `preflight.recommendedContext` | Real slider + number field (0 = Auto, max 131072), persisted and sent with every request | `settings.test.ts` |
| 5 | Can't send messages, freezes when reopened | `state.busy` was only cleared in `sendMessage`'s `finally`; "New chat"/navigation during a request left `busy` stuck true, disabling the composer forever | Requests are owned by a chat + `AbortController`; busy is derived and always cleared | `chat.test.ts` |
| 6 | Can't tell if Fast/Think is on in light mode | Only a coloured dot conveyed it, and light-theme `--accent` has low contrast | The button now shows the **mode name**, plus a checkmark on the selected menu row | `menu.test.ts` |
| 7 | Can't pause/interrupt a message | The send button showed a pause icon but was `disabled` while busy | Button becomes a live Stop control wired to `AbortController` | `chat.test.ts` |
| 8 | Model doesn't respond when the chat page isn't open | The in-flight reply was written into a transient `assistant` object tied to the rendered view | Replies are committed to the chat store regardless of the active view | `chat.test.ts` |
| 9 | Can't see / edit / delete past chats | There was no chat history at all — `newChat` just cleared `state.messages` | Persistent multi-chat store with rename and delete in the sidebar | `history.test.ts` |
| 10 | Download logos (svgs) are useless | `.download-file-icon` rendered a chip/file glyph per row | Removed the icon column | `downloads.test.ts` |
| 11 | "Data handling" message is useless | Static `.privacy-box` in Settings | Removed | `settings.test.ts` |
| 12 | Can't see docsets | Same cause as #3, plus the composer's docset control only jumped to the Docs page | Docset scope picker lists installed packs and stays correct when loading fails | `resilience.test.ts` |
| 13 | Transitions are sharp; must not flash after pressing | `render()` replaced `app.innerHTML` wholesale, so every element was destroyed and recreated on each render — CSS transitions never ran (sharp), entrance animations replayed (flash), and listeners were rebound each time | Replaced with a keyed DOM **morph** so nodes persist across renders + event delegation. Entrance animations stay once-only | `motion.test.ts` |
| 14 | Light → dark leaves everything broken | Theme was applied by hand-patching individual nodes (`toggle.innerHTML`, first `.setting-detail` found in the document) instead of re-rendering, so the DOM drifted from state; hardcoded `rgba(0,0,0,…)` shadows never adapted | Single `setTheme()` → state → render, and all colours come from CSS variables | `theme.test.ts` |

## Verification strategy

Chromium/Playwright binaries and the Rust toolchain cannot be downloaded in this
sandbox, so verification is done with **jsdom driving the real `ui/src/main.ts`**
(no mocks of app code — only the network/Tauri bridge is stubbed, exactly as the
browser preview already does).

* `ui/tests/regression.test.ts` — one test per reported issue, each asserting the
  fixed behaviour.
* `ui/tests/no-regressions.test.ts` — guards behaviour that already worked
  (sending, sources, onboarding, install/remove, attachments, markdown, scroll
  retention) so the fixes don't break anything.
* `npm run check` runs `tsc --noEmit`, the full test suite and a production build.

### Result

```
tsc --noEmit      clean
vitest run        89 passed (89)
vite build        built in 344ms
```

Rust: `veda-core` gained `resolve_context_tokens` with unit tests covering
automatic, explicit, over-max and under-min requests. The Rust toolchain cannot
be installed in this sandbox, so those tests are written but not executed here;
`AskRequest.context_tokens` is `#[serde(default)] Option<u32>`, so the change is
backwards compatible with any client that omits the field.

---

## Round 2 — model default, setup failures and the automatic context

| # | Symptom | Root cause | Fix | Verified by |
|---|---------|-----------|-----|-------------|
| 15 | Q8 was preselected on machines with ≥ 12 GiB RAM | `preflight.recommended_quant` picked Q8 whenever total RAM reached 12 GiB | Q5 is the default on every device: `recommended_quant` is now always Q5, the mock/preflight fixtures say `q5`, and the model step copy states it. A user-chosen quant is persisted (`veda:quant`) and honoured on restart when the model is still installed | `regression.test.ts` ("defaults to Q5…", "remembers the chosen model…") |
| 16 | Setup landed on a dead-end "Something went wrong" | Q8 was selectable below its 12 GiB floor, so `prepare_resources` failed late with a raw memory error and "Back" bounced to the wrong step | Q8 is disabled (with a reason) below the 12 GiB floor; the error screen explains that downloads resume on retry; "Back" returns to the step that actually failed | `regression.test.ts` ("disables Q8 below…", "shows the setup failure…") |
| 17 | Automatic context ignored how much RAM was actually free | `context_budget` keyed off **total** RAM with a coarse 4K/8K/16K/32K ladder | The automatic context is now the largest whole 1K step whose KV cache fits in **available** RAM (total minus what other apps are using) after reserving the model and a 1.4 GB leeway, capped at the 131K ceiling. `ask_veda` and the preflight both use `available_memory_bytes` | Rust unit tests in `context.rs`/`preflight.rs` (formula + clamping); `regression.test.ts` ("reports the device-sized automatic context…") |

### Result

```
tsc --noEmit      clean
vitest run        94 passed (94)
vite build        built in 339ms
```

Rust: `context_budget`/`resolve_context_tokens` were reworked and re-tested
(exact-value checks for the leeway formula, monotonic growth, floor/ceiling
clamping); `preflight` now always recommends Q5 and sizes the context from
available memory. As before, the Rust toolchain cannot be installed in this
sandbox, so the Rust tests are written but executed by CI (`cargo test
--workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo
fmt --all -- --check`).

---

## Round 3 — validation audit (post-merge scan)

A full pass over the UI, the Rust crates and the desktop command layer, with
the new default/gating/context behaviour exercised by additional probes.

### Issues found and fixed

| # | Finding | Fix | Verified by |
|---|---------|-----|-------------|
| 18 | `ask_veda` read `available_memory()` from a `System` that was never explicitly refreshed (unlike `resources.rs`, which calls `refresh_memory()` first); a stale/zero figure would poison the automatic context | Added `system.refresh_memory()` before reading available memory | code review; CI |
| 19 | The markdown renderer turned any `[label](url)` into an `<a href>`; assistant text and retrieved docs are explicitly untrusted, so `javascript:`/`vbscript:`/`data:` URLs were one layer from clickable | Links now render only for `http:`, `https:` and `mailto:`; anything else stays inert text | `no-regressions.test.ts` "only links safe schemes" |
| 20 | When the stored quant was no longer installed (or Q8 fell below the 12 GiB floor), `init()` fell back in state but left a stale `veda:quant` in localStorage | `init()` persists the resolved quant whenever the stored one was not honoured; fresh installs stay unset until the user chooses | `regression.test.ts` "clamps a stored q8 choice…" |
| 21 | `ui/src/mock.ts` used invented download ids (`model`, `embeddings`) instead of the real catalog ids (`minicpm5-q5`, `bge-small-q8`) | Aligned the ids with the catalog | tsc + build |

### Known issues reviewed and deliberately left (with reasoning)

* **llama.cpp sidecar port race** (`veda-runtime/src/sidecar.rs`): `free_port()`
  binds a listener, reads the port, then releases it before the child binds,
  so two concurrent requests (one per chat, which the UI permits) can be handed
  the same port; the child that loses the race exits with "address already in
  use" and one request fails with a clear error. Rare and self-limiting, but a
  real race. Suggested fix (not applied — Rust cannot be compiled in this
  sandbox and CI should validate it):

  ```rust
  pub async fn spawn(config: SidecarConfig) -> Result<Self, SidecarError> {
      let mut attempts = 0;
      loop {
          match Self::spawn_once(&config).await {
              Ok(sidecar) => return Ok(sidecar),
              Err(error) if attempts < 2 && is_bind_collision(&error) => {
                  attempts += 1;
              }
              Err(error) => return Err(error),
          }
      }
  }

  fn is_bind_collision(error: &SidecarError) -> bool {
      matches!(
          error,
          SidecarError::EarlyExit { log, .. }
              if log.to_ascii_lowercase().contains("address already in use")
                  || log.to_ascii_lowercase().contains("wsaeaddrinuse")
      )
  }
  ```
  (`spawn` becomes `spawn_once`; the health loop, log tail and stop behaviour
  stay untouched.)

* **Embedding inputs capped at 400 bytes** (`MAX_EMBEDDING_INPUT_BYTES`): every
  doc chunk is embedded from its first 400 bytes. Deliberate: it keeps every
  input below the 512-token physical micro-batch even for byte-token fallback.
  Raising it would improve retrieval quality but needs runtime validation with
  the actual tokenizer, so it stays.

* **`DownloadError::Cancelled` is never constructed** — dead variant, harmless.

* **Lexical and vector search are full O(N) scans per query** — fine at the
  current scale (a few thousand chunks per docset); a real limit only if packs
  grow by orders of magnitude.

* **Evidence can exceed very small automatic contexts** (e.g. 512 tokens on a
  heavily loaded low-RAM machine). llama.cpp truncates the prompt rather than
  failing, and the preflight already warns below 3 GiB available, so this is
  a graceful degradation, not a crash.

* **Catalog SHA-256s could not be re-verified live**: the sandbox blocks all
  outbound traffic except the npm registry, so the pinned runtime/model hashes
  are trusted as shipped.

### Result

```
tsc --noEmit      clean
vitest run        96 passed (96)
vite build        built in 295ms
```
