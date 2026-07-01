# app-ly

A generic Tauri desktop shell that loads an app's identity and UI from `app.toml` and a
`contents/` folder — so a real desktop app can be built with plain HTML/JS/CSS, no Rust,
build step, or native toolchain required.

## What it does

One prebuilt shell binary + your `contents/` HTML *is* your app. The shell exposes
`window.shell` to that HTML for the things plain web pages can't do: persistent files,
SQLite databases, CORS-free HTTP, desktop notifications, window/screen control, and child
windows for flows like OAuth. Instead of hard-coding a UI into the Rust/Tauri project, each
deployment just supplies its own `app.toml` + `contents/` + icon.

## Quick start (authoring an app)

```
myapp/
├── app-ly.app        # (or platform executable) — the pre-built shell binary
├── app.toml
├── icon.png
└── contents/
    └── index.html
```

```toml
# myapp/app.toml
icon = "icon.png"
name = "My App"
contents = "contents/index.html"
dataPath = "data"
```

Launch `app-ly.app` (or the executable) sitting next to `app.toml` — it's auto-discovered,
no flags needed. Full walkthrough and `window.shell` reference:
[`_docs/app-agent-guide.md`](_docs/app-agent-guide.md).

## Working on the shell itself

```bash
npm install
npm run tauri dev                              # run with ./app.toml
npm run tauri dev -- --config ./path/app.toml   # run with a different app config
npm run tauri build                             # release bundle
```

Or via the [`justfile`](justfile): `just dev`, `just build`, `just check`, `just fmt`,
`just clean`.

## Documentation

- [`AGENTS.md`](AGENTS.md) — architecture overview and JS API summary
- [`_docs/README.md`](_docs/README.md) — config reference, dev vs. release behavior
- [`_docs/app-agent-guide.md`](_docs/app-agent-guide.md) — full guide for authoring an app
- [`_docs/js-api.md`](_docs/js-api.md) — complete `window.shell` API reference
- [`_docs/project-structure.md`](_docs/project-structure.md) — repo layout and module
  responsibilities, for anyone modifying the shell itself

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
