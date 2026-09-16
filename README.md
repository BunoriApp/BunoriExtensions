# Bunori Extensions

Universal WebAssembly (WASM) extension repository for the **Bunori** light novel reader and crawler ecosystem.

This repository decouples novel source crawlers from the Android/JVM platform, compiling extensions into cross-platform `.wasm` bytecode packaged inside `.bext` archives.

---

## 🏗 Architecture

Each extension compiles to a WebAssembly module (`source.wasm`) running in a sandboxed WASM runtime. Extensions delegate network I/O to the host application (Bunori Android app or Desktop TUI) via the host ABI (`host_http`), allowing the host to seamlessly provide:
- Cloudflare clearance and bot protection cookies
- Custom HTTP headers and User-Agent rotation
- Automatic rate-limiting and connection pooling
- In-memory response caching

```
┌────────────────────────────────────────┐
│             Bunori Host                │
│ (Android App / Desktop TUI Client)     │
│                                        │
│   ┌───────────────┐  ┌──────────────┐  │
│   │  WASM Runtime │  │  HttpClient  │  │
│   │(Wasmtime/Wasmer) (Cloudflare+TLS)  │
│   └───────▲───────┘  └──────▲───────┘  │
└───────────┼─────────────────┼──────────┘
            │ host_http() FFI │
┌───────────▼─────────────────▼──────────┐
│           Extension (.bext)            │
│  ├── manifest.json                     │
│  ├── source.wasm (Pure Rust Fallback)  │
│  └── artifacts/                        │
│      ├── arm64-v8a/extension.aot       │
│      └── x86_64/extension.aot          │
└────────────────────────────────────────┘
```

---

## 📁 Repository Structure

```
BunoriExtensions/
├── .github/
│   └── workflows/
│       └── release.yml          # GitHub Actions CI & Pages deployment
├── sdk/                         # Bunori Rust SDK (bunori-sdk)
│   ├── Cargo.toml
│   └── src/
│       ├── abi.rs               # FFI export_source! macros & memory management
│       ├── host.rs              # host_http & host_log bindings
│       ├── models.rs            # DTOs (NovelDto, ChapterDto, SearchResultDto)
│       └── lib.rs
├── sources/                     # Rust-based crawler extensions
│   ├── asianovel/
│   ├── novelarchive/
│   ├── novelbins/
│   ├── novelfire/
│   ├── novelfull/
│   ├── novelphoenix/
│   ├── novgo/
│   └── royalroad/
├── tools/
│   └── package_extensions.py    # Automated WASM & WAMR AOT packager
├── wamr/                        # Pinned WAMR compiler (wamrc)
│   └── wamrc-2.4.3
├── Cargo.toml                   # Root Cargo workspace
└── README.md
```

---

## 🚀 Building & Packaging

### Prerequisites
- **Rust** 1.75+ with WebAssembly target:
  ```bash
  rustup target add wasm32-unknown-unknown
  # On Fedora (if using RPM rust):
  # sudo dnf install -y rust-std-static-wasm32-unknown-unknown
  ```
- **Python** 3.10+

### Verification & Local Build
- **Check all sources**:
  ```bash
  cargo check --workspace
  ```
- **Package extensions locally**:
  ```bash
  python3 tools/package_extensions.py --no-remote-baseline
  ```

Packaged extensions and the repository index will be created in `repo/`:
- `repo/index.json` (Catalog consumed by Bunori)
- `repo/<extension_id>.bext` (Zip container with `manifest.json` + `source.wasm` + `artifacts/<abi>/extension.aot`)

---

## 🌐 Automated CI/CD Deployment

The repository uses GitHub Actions (`.github/workflows/release.yml`) to build and publish updates:
1. Whenever code is pushed to `main`, the CI checks extension versions against the published `repo/index.json` baseline.
2. If an extension's version is bumped, it compiles the WASM binary and packages the `.bext`.
3. The catalog and packages are published to the `repo` branch using GitHub Pages.
4. Users can add the repository URL to their Bunori app to browse and install extensions:
   ```
   https://<owner>.github.io/<repo>/index.json
   ```
