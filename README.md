# Veda 
[![Unsigned desktop release](https://github.com/Katsugachi/Project-Veda/actions/workflows/release.yml/badge.svg)](https://github.com/Katsugachi/Project-Veda/actions/workflows/release.yml) <br><br>
Veda is a Rust-first, local-only desktop assistant for Python, C++, HTML, CSS, JavaScript and attached source code. It runs **MiniCPM 5** through a pinned native `llama.cpp` runtime, asks the model to plan documentation searches, fuses BM25 and semantic results, and answers with local source citations.
## Get Started
### Download your corresponding setup file
Windows x64 <br>
Windows ARM64 <br>
MacOS 
## Targets

- macOS 12+ universal DMG: Intel and Apple Silicon application shell; native Metal runtime selected after launch.
- Windows 10 22H2+ x64 NSIS Setup EXE.
- Windows 11 ARM64 NSIS Setup EXE with a native ARM64 Veda application.

The Windows installer creates a **Veda** Start Menu folder. Public builds are intentionally unsigned for now; signing and Apple notarization can be added through release secrets later.

## Resource preflight

Setup does not download resources unless all hard requirements pass:

- **10 GiB free disk space**. Windows checks this in the NSIS pre-install hook and every platform checks again before setup.
- SSD strongly recommended; an HDD or unknown drive produces a warning.
- **Q5:** 6 GiB hard memory floor, 8 GiB recommended. Q5 is the default model on every device.
- **Q8:** 12 GiB memory floor. The setup screen disables Q8 on machines below the floor so setup cannot fail later.
- Automatic context is sized to the memory that is **actually available right now** (total RAM minus what other apps are using), with a 1.4 GB safety margin reserved for the OS and other applications. It picks the largest context whose KV cache fits in the remaining RAM, capped at MiniCPM's 131K ceiling. The explicit setting in Settings overrides it.

## Pinned model assets

The UI calls both choices **MiniCPM 5**.

| Choice | File | Bytes | SHA-256 |
|---|---|---:|---|
| Q5, default | `minicpm5-1b-Q5_K_M.gguf` | 786,862,688 | `a9408d2e911e3b29ef40a7d9bf5d25e480d770733e30958df018c7be65a77e30` |
| Q8 | `minicpm5-1b-Q8_0.gguf` | 1,153,529,261 | `60b7e21be12abb44725e18ff4feecfbba53e216e8ea52e49112c40252a839f5d` |

Q5 uses the supplied URL:

`https://huggingface.co/Abiray/MiniCPM5-1B-GGUF/resolve/main/minicpm5-1b-Q5_K_M.gguf`

Hybrid retrieval uses the small BGE English v1.5 Q8 GGUF through the same llama.cpp binary. All assets and runtime archives are downloaded to `.part` files, resumed where supported, size checked, SHA-256 verified, and atomically renamed.

## Model-directed Hybrid RAG

A question follows this local pipeline:

1. MiniCPM receives the search-planner system prompt and returns a bounded JSON plan with up to four queries, docset scopes and exact symbols.
2. Rust validates that plan.
3. Veda embeds each query locally with BGE.
4. BM25/symbol search and cosine semantic search run over installed docs.
5. Reciprocal-rank fusion combines both result lists, with an exact-symbol boost.
6. MiniCPM receives only the selected source chunks, attachments and the final grounding system prompt.
7. The answer cites `[S1]`, `[S2]`, etc. Citation chips open the corresponding local indexed excerpt.

Retrieved pages and source files are explicitly marked as untrusted data in the final system prompt, so text inside a page cannot override Veda's instructions.

## Documentation sources

Sources are pinned, checksum verified, parsed locally and converted into `.vedadoc` packs plus compressed semantic indexes.

| Pack | Pinned source | Pages/files in source |
|---|---|---:|
| Python | Python 3.14.7 official HTML archive | 571 HTML pages |
| C++ | cppreference HTML book, 2025-02-09 | 6,640 HTML pages |
| HTML | MDN content commit `83cd10f1…` | 254 Markdown pages |
| CSS | MDN content commit `83cd10f1…` | 1,252 Markdown pages |
| JavaScript | MDN content commit `83cd10f1…` | 1,333 Markdown pages |

The shared 73.7 MB MDN source archive is downloaded only once when more than one MDN pack is selected.

## Workspace

```text
apps/desktop/src-tauri/    Tauri shell, preflight, setup and native commands
crates/veda-core/         catalog, RAM/context policy, types and system prompts
crates/veda-downloads/    resumable HTTPS downloads and SHA-256 verification
crates/veda-docs/         safe .vedadoc format, parsing and chunking
crates/veda-search/       BM25, vectors and reciprocal-rank hybrid fusion
crates/veda-runtime/      llama.cpp sidecars, planner, embeddings and RAG orchestration
tools/docpack/             reproducible standalone documentation pack builder
ui/                        framework-free TypeScript/CSS interface
```

The runtime, retrieval, storage-sensitive logic and OS integration are Rust. The view layer is small framework-free TypeScript so the supplied HTML/CSS design can be reproduced without Electron or a Node runtime on the user's machine.

## Development

Requirements: current stable Rust, Node 20+, and the platform prerequisites listed by Tauri 2.

```bash
npm install
npm run dev                 # browser/live UI development
npm run build               # typecheck and production frontend build
cargo test --workspace      # Rust tests; native Tauri prerequisites are required
npm run tauri dev           # native desktop app
```

The browser preview uses deterministic local demo data. Native Tauri builds call the Rust commands and real local runtimes.

## Native packaging

### macOS universal DMG

Run on macOS with Xcode command-line tools:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm ci
npm run tauri build -- --target universal-apple-darwin --bundles dmg
```

### Windows x64 Setup EXE

```powershell
rustup target add x86_64-pc-windows-msvc
npm ci
npm run tauri build -- --target x86_64-pc-windows-msvc --bundles nsis
```

### Windows ARM64 Setup EXE

Use a Windows ARM64 runner, or install the Visual Studio C++ ARM64 build tools:

```powershell
rustup target add aarch64-pc-windows-msvc
npm ci
npm run tauri build -- --target aarch64-pc-windows-msvc --bundles nsis
```

Unsigned files are emitted under each target's `release/bundle` directory. Signing is deliberately not configured.

## Security properties

- llama.cpp binds to `127.0.0.1` on a random port with an ephemeral bearer token.
- Child processes are killed with Veda and after health probes.
- Accelerated backends must pass a real model-loading health check; otherwise Windows rolls back to CPU.
- No remote UI, fonts, JavaScript, telemetry or analytics.
- Strict Tauri CSP and minimal capabilities.
- Source archives reject traversal paths and have pinned byte counts and hashes.
- Attached files are read-only, size limited and never executed.
- Logs must not include prompts or file contents.

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for source and model licenses.
