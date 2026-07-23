# Tranova

Tranova is a Tauri-based AI translation studio for text and documents. The desktop app starts a local Web service, so the same interface and features are available in the installed app and at `http://127.0.0.1:48731`.

## Features

- Text translation with reusable prompt templates and any number of selected glossaries.
- Import and export of prompts and glossaries as JSON.
- Cloud AI providers: OpenAI, DeepSeek and Doubao.
- Local AI providers: llama.cpp, LM Studio and Ollama.
- HTTP, HTTPS, SOCKS5 and SOCKS5H proxy support.
- English and Simplified Chinese interfaces.
- Document translation for DOCX, PPTX, XLSX, TXT, Markdown, HTML, CSV, JSON, SRT and VTT.
- Image translation for PNG, JPG and WebP, including images embedded in Office documents, when the configured AI provider supports image editing and returns `data[0].b64_json`.
- Local-only Web access with the same React interface and Rust API used by the desktop app.

Office Open XML documents are unpacked locally. Tranova extracts paragraph or text-block content, translates it, replaces the original text, and writes a new document while retaining the package structure and formatting data. Images are left unchanged unless image support is explicitly enabled for the selected provider.

## Development

Prerequisites: Node.js 20 or newer, Rust 1.77 or newer, and the platform prerequisites for Tauri 2.

```powershell
npm install
npm run tauri dev
```

The Tauri development command starts Vite at `http://localhost:1420` and the local API/Web service at `http://127.0.0.1:48731`.

To run the Web service without opening a desktop window, first build the frontend and Rust binary:

```powershell
npm run build
cargo build --manifest-path src-tauri/Cargo.toml
src-tauri\target\debug\tranova.exe --server
```

On Linux or macOS, run the equivalent binary at `src-tauri/target/debug/tranova --server`.

## Verification

```powershell
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build -- --no-bundle
```

`npm run test:e2e` starts an isolated Web service and verifies desktop, mobile and Simplified Chinese layouts. Run `npx playwright install chromium` once if no Playwright browser is installed.

## Configuration and data

Configure at least one enabled AI provider before translating. Provider API keys and all application data are stored in the current user's platform data directory and are never sent to the Tranova Web frontend except through the local loopback service. Web binding is restricted to loopback addresses (`127.0.0.1` or `::1`) so another device cannot read the local configuration.

Changes to the Web listening address or port take effect after restarting Tranova. The default prompt can be edited but is retained if deletion is requested, ensuring a usable translation instruction is always available.

## Packaging

Tauri is configured for Windows, Linux and macOS. Platform icons are included for Windows, macOS, Linux, Android and iOS. Desktop artifacts can be built on each target operating system with:

```powershell
npm run tauri build
```

Mobile packaging is not enabled in this release because the current architecture intentionally exposes no Web service on mobile and would require a separate Tauri command transport for feature parity.
