# Veda audit report

Date: 2026-08-19  
Branch: `arena/01a01510-project-veda` (fix-forward from `main` @ `e6fd22e`)  
Scope: full app (UI, desktop commands, runtime, search, docs, core).

This is a review of how the pieces actually join, not a test-count claim.
Each defect below names the file and the line-level cause. Architecture
debt that is real but not a current break is listed separately and was
**not** rewritten.

---

## How the live path fits together

```
UI sendMessage
  → bridge.ask (Tauri ask_veda)
      → warm ModelSession (reuse unless model/ctx/GPU changed)
      → greeting? complete_with_sink(CONVERSATIONAL, 192 tokens)
      → else plan_from_question (Rust, microseconds)
          → LocalRetriever (BGE if present, else empty vectors)
          → HybridIndex (BM25 postings + padded cosine, filter by pack first)
          → complete_with_sink(FINAL_ANSWER, 512/768) + ask-token events
  → UI appends tokens, then commits the full AskResponse
```

Docs:

```
Add files  → change#localDocsInput → install_local_docs
Add folder → pick_and_install_local_docs (desktop dialog)
Official   → install_docset (works without MiniCPM; lexical index)
Index file → indexes/{python|local-<slug>}.json.zst
```

---

## What was broken (and is now fixed)

These were line-level defects in the seams between files. Each one has a
reproducer that fails if the fix is reverted.

| # | Symptom | Root cause | Fix | Proof |
|---|---------|------------|-----|-------|
| 1 | MiniCPM ~4 min with GPU offload | Automatic context filled RAM to **131K**; llama.cpp allocated that KV cache and warmed it up | Auto context pinned at **16,384**; `--no-warmup`, `--flash-attn`, q8 KV, batch 2048/512 | `automatic_context_stays_fast_regardless_of_ram`; `chat_sidecar_uses_the_fast_gpu_flags` |
| 2 | Extra multi-second wait on every technical question | A second MiniCPM generation planned search as JSON | `plan_from_question` in Rust | planner tests; orchestrator calls the Rust planner, not `LlamaClient::plan_search` |
| 3 | “Add your own docs” did nothing in the browser | `#localDocsInput` had no `change` listener; tests called `__test.importLocalDocFiles` and hid it | Listener wired to `importLocalDocFiles` | `picking a file through the input creates an installed local card` dispatches a real `change` event |
| 4 | Desktop would not compile / could not remove local libs | `lib.rs` registered commands `commands.rs` did not define; `remove_docset` rejected `local-*` | Commands exist; remove routes to `remove_local_docset` | Handler list matches bridge; remove test |
| 5 | Local docs vanished after a model-backed index | `index_docset` wrote `local.json.zst` (`DocsetId::Local.as_str()`) while listing looked for `local-<slug>.json.zst` | `index_stem()` uses the folder slug | `local_index_stem_uses_the_folder_slug_not_the_shared_enum` |
| 6 | Adding a local pack broke *official* search | `VectorIndex::build` required every vector to share one length; empty local vecs + 384-d BGE → `DimensionMismatch` | Pad empty vectors to the first non-zero dim | `mixed_empty_and_real_vectors_do_not_poison_the_index`; `mixed_official_and_local_chunks_still_search` |
| 7 | User docs never searched for “python …” | Planner kept only inferred official packs | Always keep `DocsetId::Local` if it is in scope | `local_libraries_stay_in_scope_when_a_language_is_inferred` |
| 8 | Technical answers never streamed | `ask_with_sink` called `complete()` and ignored `on_token` | `complete_with_sink(..., on_token)` | Code path is the sink; SSE unit tests |
| 9 | Desktop type error: optional embed into required field | `ask_veda` built `Option<EmbeddingClient>` but `LocalRetriever.embed` was not `Option` | Field is `Option`; lexical fallback on miss/error | Struct and call site now agree |
| 10 | Unused `CONTEXT_MEMORY_LEEWAY_BYTES` | Auto-context rewrite left the const dead → `clippy -D warnings` red | Used in `estimated_total_bytes` | rustc/clippy dead_code |
| 11 | Markdown quote-in-URL | First `escapeHtml` of the whole block already turns `"` into `&quot;` before the `<a>` rewrite. A second `escapeHtml(url)` would turn real `&` into `&amp;amp;` | Left the first-pass escape; added a regression that `&` stays `&amp;` once | `only links safe schemes` |
| 12 | Last streamed tokens vanished / runtime tests did not compile | `read_sse_completion` dropped the leftover buffer when the body had no trailing newline; the unit test called `append_sse_delta` which did not exist | Extracted `append_sse_delta` + `drain_sse_buffer(..., flush_tail)` | `sse_flushes_the_last_line_without_a_newline` **fails** if `flush_tail` is ignored; `sse_delta_appends_content_and_ignores_done` **fails to compile** if the helper is deleted |
| 13 | Two local libraries with `readme.md` hid each other | `chunk_page` built `id = local:{path}:{n}`; `LocalRetriever` merges by that id | Id is `local:{library-slug}:{path}:{n}` | `local_libraries_do_not_share_chunk_ids` **fails** on the old format; `two_local_libraries_with_the_same_filename_both_retrieve` |
| 14 | `cargo test --workspace` red on main; `veda-runtime` never compiled on CI | `on_token` is already `Option<&mut (dyn FnMut(&str) + Send)>`. `.as_deref_mut()` reborrows that local; the reborrow is stored in the future and held across `.await` → **E0597**. Same bug twice: `client.rs` (`read_sse_completion(..., on_token.as_deref_mut()).await`) and `orchestrator.rs` (`complete_with_sink(..., on_token.as_deref_mut())` then `.await`). Clippy never ran. | Pass `on_token` by value. Drop the now-needless `mut` on both params. | rustc 1.88.0 snippet with the same types: broken file is **E0597**; passing `on_token` compiles. Workspace compile is CI — crates.io TLS is blocked here. |

---

## Architecture: what will hurt later (not changed)

These are real, but changing them now would be a rewrite. They are documented
so they are not “forgotten bugs”.

1. **No conversation history is sent to MiniCPM.** Each ask is one user turn + evidence. Follow-ups like “make it shorter” have no prior answer. Needs a bounded history window in `ask_veda`.
2. **`DocsetId::Local` collapses every user library into one filter bucket.** Search can include/exclude “all local docs”, not one folder. Fine at current scale; a string newtype is the long-term fix. Chunk *ids* are now unique per library (item 13); the *filter* is still one bucket.
3. **One in-memory `HybridIndex` of every installed pack.** First ask after launch deserialises all `*.json.zst` (C++ alone is thousands of chunks × 384 floats). Cache helps the second ask; a per-pack index map would bound RAM and first-ask latency.
4. **Session mutex held for the whole generation.** Correct (one GPU, one decode) but the janitor and a second chat wait. Streaming does not release the lock.
5. **`--flash-attn` + q8 KV on every chat sidecar**, including CPU. If a backend rejects the flag, spawn fails and setup falls through the backend chain. Probe uses the same `server_args`, so a bad flag fails health and rolls back — that is the safety net.
6. **JSON.zst embeddings** are large and slow to load compared with a packed f32 dump. Retrieval quality is unchanged; startup cost grows with packs.
7. **`LlamaClient::plan_search` is still compiled.** Nothing on the ask path calls it. Leaving it avoids a drive-by API delete; calling it again would reintroduce the planner round-trip.
8. **`load_search_index` ignores the selected-docset argument** and always loads every installed pack. Filtering happens later. Fine until the on-disk set is huge.

---

## Verification (2026-08-19)

Do not merge this branch until GitHub CI is fully green. That is the
only place that compiles the desktop crate and runs Clippy 1.97.

What CI has actually executed on this branch:

| Run | Commit | fmt | `cargo test --workspace` | `cargo clippy -D warnings` |
|-----|--------|-----|--------------------------|----------------------------|
| [32189633433](https://github.com/Katsugachi/Project-Veda/actions/runs/32189633433) | `74b1dd3` | green | **green** | **red**, exit 101 |
| [32224440611](https://github.com/Katsugachi/Project-Veda/actions/runs/32224440611) | `a09906d` | — | job did not start (3s, empty steps) | — |

Item 14 (E0597) is fixed: CI compiled and tested the whole workspace on
`74b1dd3`. The remaining red is Clippy on Rust 1.97. Annotations only
show `Process completed with exit code 101` — Azure log blobs are not
readable from this sandbox.

What this sandbox actually ran:

```
rustc / clippy 1.88.0 (npm @rustbin; static.rust-lang.org TLS is blocked)
cargo fmt --all -- --check                         clean
cargo clippy -p veda-core --lib -- -D warnings     clean (path-vendored serde/thiserror)
cargo clippy -p veda-search --all-targets -D warnings  clean

cargo test --workspace           NOT RUN (crates.io TLS blocked)
cargo clippy --workspace         NOT RUN (same)
veda-runtime / veda-desktop      NOT compiled here
```

Calling the workspace “verified” would be a lie. The remaining 1.97
Clippy site is still unknown until a full CI job prints it.

The product items in the table (16K auto context, Rust planner, Docs
tab, BM25, local index stems, chunk ids) landed in PR #8. Their logic
is unchanged by this pass.

---

## Deliberately not changed

- Explicit context slider still goes to 131K (user choice; they pay the cost).
- Conversational allow-list stays conservative (misrouting a real question is worse than a slow “hi”).
- Official pack set stays the five catalogued languages.
- No Electron, no remote fonts, CSP unchanged.
- Session lock is not split (one local decode at a time is the product).
- Status-line flavour verbs stay; only “Planning the search…” was renamed because the LLM planner is gone.

---

## Files touched in this audit pass

This fix-forward pass:

- `crates/veda-runtime/src/client.rs`
- `crates/veda-runtime/src/orchestrator.rs`
- `crates/veda-search/src/vector.rs`
- `.github/workflows/ci.yml`
- `AUDIT_REPORT.md`
