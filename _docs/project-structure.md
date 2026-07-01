# Project structure

This document describes the layout of the `app-ly` shell repository itself — i.e. for someone
modifying the shell binary, not for someone authoring an app on top of it (see
[`app-agent-guide.md`](app-agent-guide.md) for that).

```
app-ly/
├── app.toml                # dev config, loaded by `npm run tauri dev`
├── app.toml.example        # template for new app identities
├── bundle/
│   └── app.toml             # config baked into release builds (see tauri.conf.json resources)
├── example/
│   ├── contents/index.html  # sample HTML app demoing window.shell
│   ├── icon.png
│   └── data/                 # dataPath for the example app (created at runtime)
├── src/                      # unused Tauri template frontend — required by tauri.conf.json's
│                              # `frontendDist`, but never actually loaded (contents are served
│                              # from the `shell://` protocol instead, see lib.rs)
├── src-tauri/
│   ├── Cargo.toml / Cargo.lock
│   ├── build.rs
│   ├── tauri.conf.json       # bundle config: icons, resources, identifier
│   ├── capabilities/
│   │   └── default.json      # which permissions the "main" window capability grants
│   ├── permissions/
│   │   └── shell.toml        # ACL allowlist of invokable `shell_*` commands — every new
│   │                          # command in commands.rs must be added here or the frontend
│   │                          # invoke() call is silently denied
│   ├── icons/                 # app icon set for bundling (all platforms)
│   ├── scripts/
│   │   ├── shell-api.js       # defines window.shell; injected via initialization_script
│   │   └── shell-shortcuts.js # devtools/reload keyboard shortcuts; injected alongside it
│   ├── src/
│   │   ├── main.rs            # binary entrypoint, calls lib::run()
│   │   ├── lib.rs              # app setup: discovers app.toml, builds the main window,
│   │   │                        # registers the `shell://` protocol, assembles the init script,
│   │   │                        # registers the tauri invoke_handler
│   │   ├── config.rs            # app.toml + .env parsing/merging
│   │   ├── paths.rs              # resolves icon/contents/dataPath relative to config dir
│   │   ├── commands.rs            # #[tauri::command] handlers backing window.shell (files,
│   │   │                          # notifications, fetch, window/screen control, child windows)
│   │   ├── db.rs                  # SQLite dbQuery/dbExecute handlers
│   │   └── menu.rs                # native app menu (Reload, Open DevTools)
│   └── gen/schemas/               # Tauri-generated ACL schemas, not hand-edited
├── _docs/                    # this documentation set
├── justfile                  # dev/build/check/fmt/clean task shortcuts
└── package.json               # npm scripts wrapping `tauri dev`/`tauri build`
```

`app1/` at the repo root is a working app folder built on top of this shell (its own `app.toml`,
copied `app-ly.app` binary, and design assets) — it is not part of the shell itself and is
untracked.

## How a request flows through the shell

1. `main.rs` → `lib.rs::run()` builds the Tauri app, registers the `shell://` protocol handler,
   and runs `setup()`.
2. `setup()` calls `plan_startup()`, which uses `config.rs`/`paths.rs` to discover and resolve
   `app.toml`, then builds the main window pointed at `shell://localhost/<entry_filename>` with
   `shell_init_script()` (from `lib.rs`, concatenating `shell-api.js` + `shell-shortcuts.js`) as
   its `initialization_script`.
3. The `shell://` protocol handler (`serve_shell_request` in `lib.rs`) serves files out of the
   resolved `contents_dir`, path-traversal-checked against that root.
4. Contents HTML calls `window.shell.*`, which is `window.__TAURI__.core.invoke("shell_*", ...)`
   under the hood (`shell-api.js`) — routed by the ACL in `permissions/shell.toml` and
   `capabilities/default.json` to the matching handler in `commands.rs` (or `db.rs`).

## Adding a new `window.shell` method

Touch all four of these, in order, or the method will compile but silently fail (or not appear)
at runtime:

1. `src-tauri/src/commands.rs` (or `db.rs`) — add the `#[tauri::command]` handler.
2. `src-tauri/src/lib.rs` — import it and add it to the `tauri::generate_handler![...]` list.
3. `src-tauri/permissions/shell.toml` — add the command name to `commands.allow`.
4. `src-tauri/scripts/shell-api.js` — expose it on `window.shell`.

Then document it in [`js-api.md`](js-api.md) (full reference) and
[`app-agent-guide.md`](app-agent-guide.md) (summary table + narrative section) for anyone
building an app against the shell.
