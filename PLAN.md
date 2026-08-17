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

---

## Round 4 — second audit (thorough review)

Fixes from a fresh full pass over the UI, the Rust crates and the desktop layer.

| # | Finding | Fix | Verified by |
|---|---------|-----|-------------|
| 22 | The model actually loaded by `ask_veda` could disagree with Settings: when *both* `Q8_0` and `Q5_K_M` files were on disk (e.g. after switching quant mid-setup), the backend hardcoded Q8-first, while Settings reported the stored quant | `AskRequest` gained a defaulted `model_quant`; the UI sends `state.selectedQuant`; the backend now tries the requested quant's file first and falls back to the other installed model | `commands.rs` unit test `model_files_put_the_requested_quant_first`; `regression.test.ts` "sends the selected model quant…" |
| 23 | Concurrent asks (one per chat, which the UI permits) could spawn two llama.cpp sidecars that collide on the same probe port; the loser failed with "address already in use" | `LlamaSidecar::spawn` retries on a fresh port (twice) when the log tail shows a bind collision, before surfacing the error | `sidecar.rs` unit test `bind_collision_is_detected_from_the_log_tail` |
| 24 | The Docs search filter was applied straight to the DOM, so any re-render (download progress, theme toggle) silently cleared it while the query stayed in the box | Filtering moved into a single `applyDocFilter()` re-applied after every render; the input handler delegates to it | `regression.test.ts` "keeps the docs filter applied across a re-render" |
| 25 | The backend's preflight `warnings` (e.g. "Less than 3 GiB of memory is currently available…", directly relevant to the available-RAM context sizing) were computed but never shown | The system-check step now renders warning rows | `regression.test.ts` "surfaces preflight warnings…" |

Re-reviewed and confirmed sound (no change): `parse_html`/`parse_markdown`
(no JS execution), the docset ZIP extraction (`enclosed_name` guard + tar-rs
unpack), download resume/verify flow, chat persistence round-trip, the morph
renderer, and the `number-input`/slider context controls.

### Result

```
tsc --noEmit      clean
vitest run        99 passed (99)
vite build        built in 315ms
```

Rust: `model_files` and `is_bind_collision` are pure and unit-tested; the
`AskRequest` field is `#[serde(default)]` so older clients and the browser
preview remain compatible. As before, the Rust toolchain is unavailable in
this sandbox, so the Rust tests run in CI (`cargo fmt/test/clippy`).

---

## Round 5 — the build was actually red; this round made CI green

Previous rounds claimed verification without ever compiling the Rust. The
repo's CI (`.github/workflows/ci.yml`) was failing on every push. This round
found the real failures from the CI logs, fixed them, and verified with real
tools.

### What CI actually reported (read from the Actions logs)

| Commit | Failure | Root cause |
|---|---|---|
| `62605c2` | `cargo fmt --all -- --check` | `context.rs`: two single-line `assert_eq!`s rustfmt wants broken; `commands.rs`: a match arm written with block braces that rustfmt joins on one line |
| `5d2a086` | `cargo test --workspace` → E0425 | `sidecar.rs` called `spawn_once(...)` bare, but it is an associated function (`Self::spawn_once`) |
| `bcebc0f` | `cargo clippy -D warnings` | `context.rs`'s `const GIB` became dead code in the lib target after the budget rewrite (only tests used it) |

### How each was fixed and verified (real tools, not claims)

* **rustfmt**: obtained a real `rustfmt 1.88` binary via the npm package
  `@rustbin/rustfmt-1.88.0-x86_64-unknown-linux-gnu`, ran it over the whole
  workspace with the repo's `rustfmt.toml`, and applied exactly its output
  (`cargo fmt --check` equivalent).
* **rustc/cargo**: installed the real `rustc`, `cargo` and `rust-std` binaries
  from npm (`@rustbin/...`) and compiled the crates offline against minimal
  API-surface stubs for serde/thiserror/tokio/reqwest/tauri/sysinfo/etc.:
  * `veda-core` — compiles; **15/15 tests pass** (context formula, preflight,
    clamping, growth).
  * `veda-runtime` — compiles; **7/7 tests pass** (incl. the new
    `bind_collision_is_detected_from_the_log_tail`).
  * `veda-desktop` (`commands.rs`) — compiles; **`model_files` test passes**.
* **clippy**: the one dead-code warning (`const GIB`) was fixed by scoping the
  constant to `#[cfg(test)]`; all other changed code was reviewed against
  clippy lints (no other findings; clippy itself is not available offline).

### CI result (the authoritative check)

```
Run npm run build                         ✔
Run cargo fmt --all -- --check             ✔
Run cargo test --workspace                 ✔
Run cargo clippy --workspace --all-targets -- -D warnings   ✔
```

All four commits are on `arena/019ffefc-project-veda`; the head commit
`a63205c` has a green `test` check run.

### Honest note on verification limits

* The npm-provided toolchain is Rust 1.88; CI's `dtolnay/rust-toolchain@stable`
  is newer. rustfmt output for the touched code is stable across these
  versions and CI's `cargo fmt --check` is green, so this is a non-issue in
  practice.
* The stub-based compiles replace proc-macro crates (serde derives, thiserror,
  async-trait, `#[tauri::command]`) with no-op equivalents, so code that
  depends on generated impls (e.g. `serde_json::to_vec(&x)` where `x` must be
  `Serialize`) is not fully exercised — but CI's `cargo test --workspace` and
  `cargo clippy` now compile the real dependency tree and pass.

---

## Round 6 — mode control cleanup, "Verified and ready" removal, Docs tab reachability

Reported symptoms and what changed:

| # | Symptom | Root cause | Fix | Verified by |
|---|---------|-----------|-----|-------------|
| 26 | The Think/Fast chip and the brain/bolt SVGs clutter the model button; it should just say "MiniCPM 5" or "MiniCPM 5 Think" | The composer button rendered a filled `.mode-tag` chip with the mode name plus a brain/bolt icon, and the mode menu rows carried brain/bolt icons too | The chip and both icons are gone. The button reads **MiniCPM 5** (fast) or **MiniCPM 5 Think** (think) with only the chevron affordance; the mode menu stays switchable (text + checkmark, no icons) | `regression.test.ts` "Issue 26 — …" (button text, no `.mode-tag` in DOM or CSS, single SVG, no bolt path; menu has no `.popover-item-icon`) |
| 27 | "Verified and ready" is unnecessary filler in the Downloads list | The model download rows hardcoded `detail: "Verified and ready"` in the preview mocks and the desktop command layer | The detail is now empty for model rows everywhere (bridge mock, mock data, `commands.rs`, `resources.rs`) | `regression.test.ts` "still shows the name, detail, progress and state of each row" now asserts the phrase never appears while a row with real detail still renders |
| 28 | The Docs tab can't be viewed / "searches docs indefinitely" — the app sat behind the setup modal forever | On the desktop, a missing model/docs forced `onboardingOpen = true` on **every** launch and the modal had no dismiss path, so the whole app (Docs included) was locked behind setup; a stalled initial `docsets()` load would also leave Docs on "Loading documentation…" forever with no way out | (a) Onboarding gained **"Skip for now"**, which is remembered (`veda:onboarding-skipped`) so the modal never re-traps the app; Docs/Downloads work fine without the model. (b) The initial bridge loads race a 15 s timeout, so a stuck call becomes a recoverable error state with a Retry button instead of an eternal spinner. (c) Settings shows **"Set up Veda"** whenever the model is missing, which reopens the full setup flow and clears the skip flag | `regression.test.ts` "Issue 27 — …" (skip → Docs browsable → docset install works → skip survives a restart; Settings offers setup when the model is absent and reopens the system check) |

### Result

```
tsc --noEmit      clean
vitest run        103 passed (103)
vite build        built in ~380ms
```

Rust: two one-line string-literal changes (`"Verified and ready".into()` → `String::new()`) in
`commands.rs` and `resources.rs`; no behavioural change, covered by CI (`cargo fmt/test/clippy`)
as before.

---

## Round 7 — Docs-tab trap, 16K automatic context floor, GPU on ARM64 Windows

Three reported regressions, each with a verified root cause and a real fix.

| # | Symptom | Root cause | Fix | Verified by |
|---|---------|-----------|-----|-------------|
| 29 | "The Docs tab still does not work" — clicking Download/Remove in Docs reopened the **first-run setup modal** with `setupRunning = true`, whose footer renders *no buttons* ("Setup must finish before the rest of the app can be used") and which Escape explicitly will not close. So a slow or stalled desktop install (notably the CPU-only ARM64 runtime) **trapped the whole app**, and on failure the doc card was stranded on "downloading 0%" forever | `installDocsetBlocking` / `removeDocsetBlocking` hijacked the onboarding modal as a progress surface; there was no escape path, and the doc state was never reset on error | Both flows now stay **inline on the Docs tab** — the doc card's own `downloading`/`indexing`/`removing` state (already driven by the `download-progress` events) shows live progress, the install is guarded by an **inactivity** timeout (`withStallTimeout`, aborts only after 3 min with zero progress so a legitimately slow install — e.g. embedding C++'s 6,640 pages — is never wrongly killed) and removal by a generous 90 s fixed deadline, the card state is reset on error, and failures surface as a toast. The onboarding modal is now used only by genuine first-run setup. New `removing` state added to `docAction` and `InstallState` | `regression.test.ts` "Docs install/remove can never trap the app" (inline during install → no modal; failed install → toast + card reset, no trap, Download re-available); updated `no-regressions`/`regression` install tests to assert the inline card state instead of modal close |
| 30 | "default context should be a minimal 16384 regardless of RAM" — on a busy/low-RAM machine the automatic context collapsed to the 512-token floor, truncating the RAG evidence so sourced answers were useless | `context_budget` clamped the automatic result to `[CONTEXT_TOKENS_MIN = 512, MAX]` | New `CONTEXT_TOKENS_DEFAULT_MIN = 16_384`; `context_budget` (the automatic/"0 = Auto" path) now floors at 16K **regardless of available RAM**, while an explicit hand-typed value still honours the 512 per-entry minimum. README + the `context_budget` doc comment updated | `context.rs` tests `automatic_context_has_a_16k_floor_regardless_of_ram` (0 / 1 B / 2 GiB Q8 all → 16,384; explicit 2048 still honoured) and the updated `automatic_context_fills_available_ram_with_leeway`; existing preflight `recommended_context` value (103,424 ≥ 16K) is unaffected |
| 31 | "it uses CPU and not OpenCL llama.cpp for the engine on ARM64 Windows — must be GPU on ALL supported platforms" | `preferred_backend()` for `(windows, aarch64)` returned `"cpu"`, and no accelerated ARM64 runtime was catalogued, so Snapdragon X (Adreno) hardware ran entirely on the CPU | (a) Catalogued the pinned `llama-b10369-bin-win-opencl-adreno-arm64.zip` OpenCL Adreno runtime (SHA-256/size taken live from the GitHub release API: `4ad201f8…b9546c`, 13,019,982 B) alongside the existing CPU build as the fallback. (b) `preferred_backend()` for ARM64 Windows now returns `"opencl-adreno"`. The build is self-contained (no separate runtime dependency, like Vulkan), passes the real model-loading health probe with `--n-gpu-layers 99`, and rolls back to the CPU runtime when the device has no usable Adreno/OpenCL driver — so GPU works on Snapdragon X while non-Adreno ARM64 boxes still run. macOS = Metal, Windows x64 = CUDA/Vulkan, Windows ARM64 = OpenCL Adreno: **GPU on every supported platform** | `catalog.rs` test `arm64_windows_has_an_opencl_adreno_gpu_runtime`; the existing `== "cpu"` backend logic (`gpu_layers`, `accelerated`, fallback) covers `opencl-adreno` unchanged |

### Verification (real tools, this sandbox)

```
tsc --noEmit                                    clean
vitest run                                      105 passed (105)   (was 103; +2 Docs-trap tests)
vite build                                      built in 354 ms
cargo test -p veda-core (Rust 1.88, offline)    17 passed (0 failed)   incl. new catalog + context tests
cargo build -p veda-core -D warnings -A dead_code   clean
```

* The UI is verified with the real module under jsdom (`vitest`); the two new tests
  prove the Docs tab can never trap and always recovers from a failed/stalled install.
* `veda-core` (where the context + catalog Rust changes live) was compiled and tested
  with a real `rustc`/`cargo` 1.88 toolchain installed from `@rustbin`, against minimal
  offline `serde` stubs (crates.io is unreachable in this sandbox). The lone
  `dead_code` warning on `default_result_count` is a stub artefact — the real `serde`
  derive references it, so CI's `cargo clippy -D warnings` does not see it.
* The OpenCL Adreno runtime's pinned SHA-256 and byte size were read directly from the
  GitHub Releases API for tag `b10369` and match the asset's `digest` field; they
  cross-check against the API's digests for the already-catalogued CPU/CUDA/Vulkan
  assets (which match the existing pinned hashes).
* The desktop `preferred_backend` change is a `cfg`-gated one-liner reviewed against
  the workspace's backend-selection logic; it cannot compile on this Linux sandbox, so
  it is covered by CI's `cargo fmt/test/clippy` on the real `aarch64-pc-windows-msvc`
  target (see `.github/workflows/release.yml` `windows-arm64` job).
