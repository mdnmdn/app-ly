// Prototype plugin host — runs in the webview, on top of today's shell.spawn.
//
// It exists to prove the sidecar model end to end without touching the Rust
// binary. In the real design this logic lives in src-tauri/src/plugins/, the
// registry is built during startup, and the page just sees `shell.plugins`;
// see _docs/plugins.md.
//
// The plugin set is fixed once discovered — adding a plugin means relaunching,
// never rebuilding. Manifests declare their methods, so the whole JS surface
// exists before any process starts; $describe only confirms it on start.
//
// Exposes window.plugins:
//   await plugins.discover(["tls-inspect", "os-trust"])  -> registry, no spawning
//   plugins.api.tls.inspect({ host: "example.com" })     -> starts on first call
//   plugins.get("tls").on("progress", (data) => ...)
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

      // Built from the manifest, so it is a real object with real functions
      // before anything is spawned — no Proxy, and a typo is a TypeError here
      // rather than a rejected promise from a live process.
      this.api = Object.fromEntries(
        (manifest.methods ?? []).map((method) => [
          method.name,
          (params, options) => this.call(method.name, params, options),
        ]),
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

      // The manifest already defined the surface; this is the drift check.
      this.describe = await this.call("$describe");
      const declared = (this.manifest.methods ?? []).map((method) => method.name).sort();
      const actual = (this.describe.methods ?? []).map((method) => method.name).sort();
      if (String(declared) !== String(actual)) {
        window.shell?.log?.(
          `plugin "${this.name}": manifest declares [${declared}], plugin reports [${actual}]`,
          "warn",
        );
        this.emit("drift", { declared, actual });
      }

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
    // Stands in for what the Rust host does during startup. Manifests are
    // fetched over the shell:// protocol here, so the plugin folders have to
    // sit inside `contents`; a Rust-side host reads them from disk in
    // plan_startup() and can keep plugins out of the UI tree entirely.
    //
    // No plugin process is started: the manifest is enough to build the API.
    discover: async (dirs) => {
      for (const dir of dirs) {
        const response = await fetch(`./${dir}/plugin.json`);
        if (!response.ok) throw new Error(`no manifest in ${dir} (${response.status})`);
        const manifest = await response.json();
        if (manifest.apiVersion !== PROTOCOL) {
          throw new Error(`${dir}: apiVersion ${manifest.apiVersion}, host speaks ${PROTOCOL}`);
        }
        const plugin = new Plugin(manifest, dir);
        registry.set(manifest.name, plugin);
        window.plugins.api[manifest.name] = plugin.api;
      }
      return window.plugins.list();
    },

    // `plugins.api.tls.inspect(...)` — populated by discover, before any spawn.
    api: {},

    list: () => [...registry.values()].map((plugin) => ({
      name: plugin.name,
      title: plugin.manifest.title,
      kind: plugin.manifest.kind,
      capabilities: plugin.manifest.capabilities ?? [],
      running: Boolean(plugin.proc),
      methods: plugin.manifest.methods ?? [],
    })),

    get: (name) => registry.get(name) ?? null,

    open: async (name) => {
      const plugin = registry.get(name);
      if (!plugin) throw new Error(`no plugin named "${name}" — call discover() first`);
      return plugin.start();
    },

    call: (name, method, params, options) => {
      const plugin = registry.get(name);
      if (!plugin) throw new Error(`no plugin named "${name}" — call discover() first`);
      return plugin.call(method, params, options);
    },

    stopAll: () => Promise.all([...registry.values()].map((plugin) => plugin.stop())),
  };

  // Don't leave sidecars behind when the window goes away.
  window.addEventListener("beforeunload", () => window.plugins.stopAll());
})();
