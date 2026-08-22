// Prototype plugin host — runs in the webview, on top of today's shell.spawn.
//
// It exists to prove the sidecar model end to end without touching the Rust
// binary. In the real design this logic lives in src-tauri/src/plugins/ and
// the page just sees `shell.plugins`; see _docs/plugins.md.
//
// Exposes window.plugins:
//   await plugins.discover(["tls-inspect", "os-trust"])  -> manifests
//   const tls = await plugins.open("tls")                -> starts on first use
//   await tls.call("inspect", { host: "example.com" })
//   await tls.api.inspect({ host: "example.com" })       -> same, via Proxy
//   tls.on("progress", (data) => ...)
//   await plugins.stopAll()
(() => {
  const PROTOCOL = 1;

  class Plugin {
    constructor(manifest, dir) {
      this.manifest = manifest;
      this.dir = dir;
      this.name = manifest.name;
      this.proc = null;
      this.pending = new Map();
      this.listeners = new Map();
      this.stderr = [];
      this.buffer = "";
      this.nextId = 1;
      this.describe = null;

      // `plugin.api.method(params)` sugar. The method list is advisory — the
      // sidecar is the authority, so unknown names still go over the wire.
      this.api = new Proxy(
        {},
        { get: (_target, method) => (params) => this.call(String(method), params) },
      );
    }

    on(event, handler) {
      const handlers = this.listeners.get(event) ?? [];
      handlers.push(handler);
      this.listeners.set(event, handlers);
      return () => this.listeners.set(event, handlers.filter((h) => h !== handler));
    }

    emit(event, data) {
      (this.listeners.get(event) ?? []).forEach((handler) => handler(data));
      (this.listeners.get("*") ?? []).forEach((handler) => handler({ event, data }));
    }

    async start() {
      if (this.proc) return this;

      const entry = `${this.dir}/${this.manifest.entry}`;
      const args = [entry, ...(this.manifest.args ?? [])];
      this.proc = await window.shell.spawn(this.manifest.command, args);

      // stdout is protocol traffic and arrives in arbitrary chunks; stderr is
      // free-form plugin logging and is only kept for diagnostics.
      this.proc.onStdout((chunk) => this.feed(chunk));
      this.proc.onStderr((chunk) => {
        this.stderr.push(chunk);
        this.emit("stderr", chunk);
      });
      this.proc.onExit((exit) => this.finish(exit));

      this.describe = await this.call("$describe");
      this.emit("started", this.describe);
      return this;
    }

    feed(chunk) {
      this.buffer += chunk;
      let newline;
      while ((newline = this.buffer.indexOf("\n")) !== -1) {
        const line = this.buffer.slice(0, newline).trim();
        this.buffer = this.buffer.slice(newline + 1);
        if (!line) continue;
        try {
          this.receive(JSON.parse(line));
        } catch (error) {
          this.emit("protocol-error", { line, message: String(error) });
        }
      }
    }

    receive(message) {
      if (message.event) {
        this.emit(message.event, message.data);
        return;
      }

      const waiter = this.pending.get(message.id);
      if (!waiter) return; // late reply to a cancelled call
      this.pending.delete(message.id);
      if (message.ok) waiter.resolve(message.result);
      else waiter.reject(new Error(message.error?.message ?? "plugin call failed"));
    }

    finish(exit) {
      const reason = new Error(
        `plugin "${this.name}" exited (code ${exit.code}${exit.timedOut ? ", timed out" : ""})`,
      );
      this.pending.forEach((waiter) => waiter.reject(reason));
      this.pending.clear();
      this.proc = null;
      this.emit("exit", exit);
    }

    async call(method, params, { timeoutMs = 30000 } = {}) {
      if (!this.proc && method !== "$describe") await this.start();
      if (!this.proc) throw new Error(`plugin "${this.name}" is not running`);

      const id = String(this.nextId++);
      const settled = new Promise((resolve, reject) => {
        this.pending.set(id, { resolve, reject });
      });

      this.proc.write(`${JSON.stringify({ v: PROTOCOL, id, method, params })}\n`);

      const timer = setTimeout(() => {
        if (!this.pending.has(id)) return;
        this.pending.get(id).reject(new Error(`${this.name}.${method} timed out`));
        this.pending.delete(id);
        this.call("$cancel", { id }).catch(() => {});
      }, timeoutMs);

      return settled.finally(() => clearTimeout(timer));
    }

    async stop() {
      if (!this.proc) return;
      try {
        await this.call("$shutdown", null, { timeoutMs: 2000 });
      } catch {
        // A plugin that won't answer gets killed instead.
      }
      await this.proc?.kill().catch(() => {});
      this.proc = null;
    }
  }

  const registry = new Map();

  window.plugins = {
    // Manifests are fetched over the shell:// protocol, so the plugin folders
    // have to sit inside `contents` for this prototype. A Rust-side host would
    // read them from disk instead and could keep plugins out of the UI tree.
    discover: async (dirs) => {
      const found = [];
      for (const dir of dirs) {
        const response = await fetch(`./${dir}/plugin.json`);
        if (!response.ok) throw new Error(`no manifest in ${dir} (${response.status})`);
        const manifest = await response.json();
        if (manifest.apiVersion !== PROTOCOL) {
          throw new Error(`${dir}: apiVersion ${manifest.apiVersion}, host speaks ${PROTOCOL}`);
        }
        registry.set(manifest.name, new Plugin(manifest, dir));
        found.push({ dir, ...manifest });
      }
      return found;
    },

    list: () => [...registry.values()].map((plugin) => ({
      name: plugin.name,
      title: plugin.manifest.title,
      running: Boolean(plugin.proc),
      methods: plugin.describe?.methods ?? null,
    })),

    open: async (name) => {
      const plugin = registry.get(name);
      if (!plugin) throw new Error(`no plugin named "${name}" — call discover() first`);
      return plugin.start();
    },

    get: (name) => registry.get(name) ?? null,

    stopAll: () => Promise.all([...registry.values()].map((plugin) => plugin.stop())),
  };

  // Don't leave sidecars behind when the window goes away.
  window.addEventListener("beforeunload", () => window.plugins.stopAll());
})();
