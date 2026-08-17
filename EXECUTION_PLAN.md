# Veda — 120-part execution & verification plan

This is the "prove it, don't claim it" plan for the Round 8 repairs. Every part is a
concrete, checkable unit of work: it names the thing to examine, the action to take,
and the exact acceptance check. The point is to make "it works" mean something
measurable instead of a hand-wave. Parts are grouped by subsystem so each can be run
independently, in order, or as a regression sweep before a release.

## Phase A — Reproduce and baseline (1–8)

1. Confirm the working branch is `arena/01a00f8f-project-veda` and the tree is clean before starting.
2. Run `npm ci` and record the dependency versions in `package-lock.json`.
3. Run `npm run check` and capture the baseline: tsc clean, vitest count, vite build time.
4. Open the browser preview and reproduce the dropdown bug: send nothing, open the model menu at the bottom of a filled chat, and note that it draws off-screen.
5. Reproduce the status bug: send a question and confirm the reply shows only the static string "Searching installed docs…".
6. Reproduce the citation bug: on an answer with `[S1]`, confirm clicking the inline `[S1]` does nothing.
7. Re-read the current `ask_veda` and confirm it spawns a fresh chat + embedding sidecar per request (this is the latency root cause).
8. Re-read `prepare` and confirm a GPU→CPU fallback leaves the failed accelerated runtime (and `cudart`) on disk.

## Phase B — Dropdown must never run off screen (9–18)

9. Confirm every popover is wrapped in a `.menu-anchor` with `position: relative`.
10. Remove any viewport-height media-query hacks from the popover CSS (fixed breakpoints cannot know real geometry).
11. Add a pure `popoverFlipsUp(anchor, menuHeight, viewportHeight, margin)` that returns `false` when the menu fits below, `true` when it must open above, and otherwise prefers the larger side.
12. Unit-test `popoverFlipsUp` with: room below → down; no room below → up; neither side fits, more above → up; neither side fits, more below → down.
13. Add `placePopovers()` that measures each open `.popover` against its anchor and toggles the `flip-up` class.
14. Call `placePopovers()` synchronously after every render so there is no first-paint flash.
15. Call `placePopovers()` on window resize so the menu stays correct while the window changes.
16. Add the `.popover.flip-up` CSS rule (`top: auto; bottom: calc(100% + 8px)`) and a matching upward entrance animation.
17. Verify the menu still opens/closes on click, toggles on a second press, and dismisses on outside click.
18. Verify the scope menu (left-anchored) and model menu (right-anchored) both behave identically.

## Phase C — "hi" must answer fast (19–34)

19. Introduce `is_conversational(message)` in `veda-core`: empty input and an explicit allow-list of greetings/pleasantries return true; anything code-like or technical returns false.
20. Unit-test `is_conversational`: greetings match; punctuation on greetings still matches; code (`print(1)`, `def f()`, `import os`) and technical questions never match.
21. Add `CONVERSATIONAL_SYSTEM_PROMPT` (short, casual, no citations expected) and test it is present and rejects source fabrication.
22. Add a `ModelSession` type that owns the chat sidecar, its `LlamaClient`, a lazily-created embedding session, and a config `fingerprint`.
23. Add a `SessionFingerprint` (model path, context, GPU layers, backend, quant) so a config change is detectable.
24. Add `SessionHandle = Arc<tokio::sync::Mutex<Option<ModelSession>>>` to `AppState` so the runtime survives across commands.
25. In `ask_veda`, compute the fingerprint from the resolved model/context/backend before touching the runtime.
26. Reuse the warm session when the fingerprint matches; otherwise stop the stale session and spawn a fresh chat sidecar.
27. Serialize asks through the session mutex (a single local model can only generate one reply at a time anyway).
28. Route greetings through the fast path: complete with `CONVERSATIONAL_SYSTEM_PROMPT` at low `max_tokens`, returning empty sources and an elapsed trace.
29. Ensure the fast path never loads the search index or spawns the embedding sidecar.
30. Keep the retrieval path unchanged in behaviour but reuse the warm chat sidecar instead of spawning/killing one per request.
31. Lazily spawn the embedding sidecar only on the first retrieval-backed ask, and reuse it afterwards.
32. Confirm the session is returned to `AppState` on every exit path (success, error, empty-index) so it is never leaked or double-stopped.
33. Confirm `ModelSession::stop_all` stops both sidecars when a stale session is replaced.
34. Confirm `Drop` for the session still kills the llama.cpp children when the app closes.

## Phase D — Docs tab must actually work (35–48)

35. Add `docset` to `DownloadItem` in the desktop command layer (`#[serde(default, skip_serializing_if)]`) and to the TS `DownloadItem`.
36. Emit `docset` on every source-archive download item in `download_asset`.
37. Emit `docset` on every `{docset}-index` item in `index_docset`.
38. In the UI progress handler, mirror both the source and index passes onto the doc card (state: downloading → indexing → installed).
39. Verify the doc card shows progress during the source-download phase instead of sitting at "downloading 0%".
40. Add a `modelInstalled()` helper and a Docs banner ("Set up the local model…") shown only when the model is missing.
41. Gate the Docs Download button on the model: when missing, open first-run setup with a toast instead of wasting bandwidth on an install that would fail at embedding.
42. Verify an already-installed docset can still be removed when the model is missing.
43. Verify a failed install resets the card to an actionable state and surfaces a toast (no modal trap, no stranded 0%).
44. Verify an install that is slow but making progress is never killed by a fixed deadline.
45. Verify the Docs filter survives a re-render (progress event, theme toggle).
46. Verify the composer's scope menu lists only installed packs and jumps to Docs from "Manage documentation…".
47. Verify the Downloads tab renders alongside a failing `docsets()` call (independent failure isolation).
48. Verify `read_source` still resolves the exact `veda://docs/…` URL stored in a source ref.

## Phase E — Downloads: one backend per device (49–62)

49. Confirm `preferred_backend()` selects Metal on macOS, CUDA-or-Vulkan by `nvidia-smi` on Windows x64, OpenCL Adreno on ARM64 Windows.
50. Replace the single CPU fallback with a chain: `cuda → vulkan → cpu`, `vulkan → cpu`, `opencl-adreno → cpu`, `metal` (terminal).
51. Add a pure `next_backend(current)` and unit-test the whole chain including the `metal` and `cpu` terminal cases.
52. In `prepare`, walk the chain, probing each runtime with a real model load (`--n-gpu-layers 99` for accelerated backends).
53. On any failed probe, retire that runtime **before** trying the next, so a failed accelerator is never left installed.
54. Retire the runtime's `RuntimeDependency` entries too (e.g. `cudart`), so the Downloads tab never shows a phantom library row.
55. Retire both the extracted runtime directory and the in-memory download record for each retired asset.
56. Verify `list_downloads` reports exactly one runtime backend after a successful (or fallback) setup.
57. Verify a partial `.part` download still resumes and the SHA-256 check still gates the atomic rename.
58. Verify `prepare_resources` still enforces the Q5/Q8 memory floors and the 10 GiB disk floor.
59. Verify the model default stays Q5 on every device and a stored quant is honoured when still installed.
60. Verify Q8 is still disabled below 12 GiB in the setup screen with a clear reason.
61. Verify the setup error screen still offers Back-to-the-failed-step and Retry with resume.
62. Re-run `cargo test -p veda-core` (catalog pinning + context + preflight) and confirm no regression.

## Phase F — Claude-style activity status (63–72)

63. Add the ordered `STATUS_PHRASES` list (Pondering, Thinking, Planning the search, Searching installed docs, Reading sources, Crystallising, Substituting, Composing, Verifying citations).
64. Start the assistant placeholder with empty `content` and `status = firstStatus()`.
65. Render an `.assistant-status` line (spinner + status text) while `streaming && !content`.
66. Advance the status on a timer only while a reply is streaming, and stop the timer when it finishes.
67. Guard the timer against detached DOM (reloaded test modules) so timers never accumulate across sessions.
68. Clear `status` when the answer lands, on abort, and on failure.
69. Strip `status` from persisted chats and clear it on load so a crash never restores a stuck spinner.
70. Verify the status line is replaced by the real answer once the request resolves.
71. Verify the rotating status is visible for a slow request and each verb in the list is reachable.
72. Verify the composer is never left disabled after any of these paths.

## Phase G — Citations must be real and clickable (73–84)

73. Render inline `[S1]`-style markers as a `<button class="inline-cite" data-cite="S1">` in the markdown pass.
74. Keep the citation id strictly `[S\d+]` so the attribute is constrained to a safe token.
75. Style `.inline-cite` to read as a link (accent colour, underline on hover, focus ring) without button chrome.
76. Delegate `.inline-cite` clicks: resolve the enclosing message id, find the matching source by id, and open the reader.
77. Show a toast when an inline citation has no matching source (e.g. the model cited an id that was never returned).
78. Keep the bottom source chips functional and consistent with the inline markers.
79. Confirm the reader opens the exact `veda://docs/…` excerpt for the clicked source.
80. Confirm unknown or non-`veda://docs/` URLs are rejected by `read_source` and `open_source`.
81. Confirm markdown still renders code, lists, headings, emphasis, and fenced blocks with the inline-cite change.
82. Confirm markdown still renders `javascript:`/`vbscript:`/`data:` links inert (no `<a>`).
83. Confirm source chips and inline citations both render when sources exist, and neither renders when they do not.
84. Confirm the reader closes with the button, the backdrop, and Escape.

## Phase H — Cross-cutting UI/UX and robustness (85–98)

85. Confirm the DOM morph still preserves textarea input, focus, and scroll across renders after the new popover/status code.
86. Confirm no entrance animation replays on an unrelated re-render (no window flash).
87. Confirm light→dark→light round trips leave every control functional.
88. Confirm sidebar collapse/expand, new chat, rename, and delete still work.
89. Confirm chat history groups by recency and survives a reload without a stuck streaming caret.
90. Confirm attachments still upload (≤ 8 files, 512 KB each) and render as chips, and are never executed.
91. Confirm the stop control still aborts a running request and marks the reply "Stopped".
92. Confirm concurrent chats (one busy, one fresh) never freeze the composer.
93. Confirm Settings still opens/closes, changes theme, mode, and context (0 = Auto, slider + number stay in step).
94. Confirm the context setting is persisted and sent with each request (`contextTokens`), and the model quant is sent (`modelQuant`).
95. Confirm onboarding still runs three steps, remembers Skip, and can be reopened from Settings or the Docs banner.
96. Confirm `tsc --noEmit` reports zero errors with `strict`, `noUnusedLocals`, and `noUnusedParameters`.
97. Confirm `vite build` produces the production bundle without warnings.
98. Confirm the full `vitest run` suite passes (regression + no-regressions + round8).

## Phase I — Rust correctness and CI parity (99–110)

99. Confirm `rustfmt --check` is clean on every touched Rust file.
100. Confirm `cargo test -p veda-core` passes with the new `is_conversational` and prompt tests.
101. Confirm the desktop crate's new `next_backend` unit test is present and would run under `cargo test --workspace`.
102. Confirm `model_files` still returns the requested quant first (existing test).
103. Confirm the `DownloadItem` `docset` field is `#[serde(default)]` so older clients and the browser preview remain compatible.
104. Confirm the `AskRequest`/`AskResponse` shapes are unchanged so the Tauri command boundary stays backward-compatible.
105. Confirm `AskResponse.trace` for the fast path carries an elapsed time and empty queries/hits.
106. Confirm no log path ever prints prompts, file contents, or the ephemeral llama.cpp bearer token.
107. Confirm `llama.cpp` still binds `127.0.0.1` on a random port with the ephemeral bearer token in the persistent session.
108. Confirm child processes are still killed on session drop and after a stale-session `stop_all`.
109. Confirm CI's `cargo fmt --check`, `cargo test --workspace`, and `cargo clippy -D warnings` cover the desktop changes.
110. Confirm the release workflow still targets Windows x64, Windows ARM64, and macOS universal with the unchanged catalog hashes.

## Phase J — Final self-review (111–120)

111. Walk the browser preview end-to-end as a first-time user: setup, skip, Docs, Downloads, a greeting, a technical question, a citation, a stop.
112. Walk the desktop code paths mentally for the failure cases: model missing, runtime missing, index missing, probe failure, download checksum mismatch, aborted ask.
113. Re-read every diff line with the question "could this reintroduce a bug the previous rounds fixed?" (modal trap, stuck busy, draft loss, filter clear, theme drift).
114. Grep for the old literals that must be gone: the static "Searching installed docs…" content, the `@media (max-height: 620px)` flip, inert `<strong>` citations.
115. Grep for any remaining `DownloadItem {` literal missing the `docset` field (a struct-literal completeness check).
116. Re-run the full `npm run check` one final time and record the numbers.
117. Re-run `cargo test -p veda-core` and `rustfmt --check` one final time and record the numbers.
118. Ask explicitly: does every reported symptom have a test that fails on the old code and passes on the new code?
119. Ask explicitly: is each fix root-cause driven (the reason for the bug is removed), not a symptom patch?
120. Write the honest verification statement: what was executed in this sandbox, what could only be reviewed (Tauri/Windows/macOS paths), and exactly which CI jobs close that gap.
