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
