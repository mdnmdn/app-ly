# Plugin host prototype

A working sidecar-plugin system built entirely on today's `app-ly` binary — no Rust changes. It
exists to validate the design in [`_docs/plugins.md`](../../_docs/plugins.md) before any of it is
built into the shell.

```bash
npm run tauri dev -- --config ./example/plugins/app.toml
```

| File | Role |
|------|------|
| `app.toml` | Runnable config. Its single `[[allowedCommands]]` entry — `node` on a `.mjs` below this folder — is what makes the whole thing possible |
| `index.html` | Demo UI: discover plugins, inspect certificate chains, read OS facts and trust anchors |
| `plugin-host.js` | The host, running in the webview: manifest discovery, spawn, NDJSON framing, call correlation, events, timeouts, shutdown |
| `sidecar-sdk.mjs` | Plugin-side runtime: framing, dispatch, `$describe` / `$cancel` / `$shutdown` |
| `tls-inspect/` | Plugin: certificate chain, expiry, bulk check streaming `progress` events |
| `os-trust/` | Plugin: platform facts, environment variables, OS trust-store anchors |

Both plugins can be driven without the shell, which is half the appeal of the sidecar model:

```bash
printf '%s\n' \
  '{"v":1,"id":"1","method":"$describe"}' \
  '{"v":1,"id":"2","method":"expiry","params":{"host":"example.com"}}' \
  | node example/plugins/tls-inspect/main.mjs
```

The prototype is not the proposed design — the host belongs in Rust, manifests are `plugin.toml`
rather than `plugin.json`, and grants are unenforced here. `_docs/plugins.md` §9 lists every
difference.

> A plugin runs as a normal child process with your full privileges. Adding one is a trust
> decision like installing an application.
