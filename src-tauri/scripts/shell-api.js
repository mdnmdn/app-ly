// Injected as an initialization script, so this file shares the global
// lexical scope with the contents page. Everything lives inside an IIFE so
// the only name it adds to that scope is `window.shell`.
(() => {
  // ── Process execution (internal plumbing) ─────────────────────────
  // Backs shell.run / shell.spawn. Kept outside window.shell so only the
  // documented API surface is exposed to contents HTML.
  const shellProc = {
    states: new Map(),
    listeners: null,

    // The three event listeners are registered once, lazily. shell.spawn awaits
    // this before invoking shell_spawn, so no chunk can arrive before a
    // listener exists.
    ready: () => {
      if (!shellProc.listeners) {
        shellProc.listeners = Promise.all([
          window.__TAURI__.event.listen("shell://process-stdout", (event) =>
            shellProc.output(event.payload.id, "stdout", event.payload.data),
          ),
          window.__TAURI__.event.listen("shell://process-stderr", (event) =>
            shellProc.output(event.payload.id, "stderr", event.payload.data),
          ),
          window.__TAURI__.event.listen("shell://process-exit", (event) =>
            shellProc.exited(event.payload),
          ),
        ]).catch((error) => {
          shellProc.listeners = null; // let the next spawn retry
          throw error;
        });
      }
      return shellProc.listeners;
    },

    // Events may be delivered before the shell_spawn invoke resolves, so state
    // is created by whichever side reaches the id first.
    state: (id) => {
      let state = shellProc.states.get(id);
      if (!state) {
        state = {
          handlers: { stdout: [], stderr: [] },
          exitHandlers: [],
          buffered: [], // chunks with no consumer yet, in arrival order
          iterators: [],
          exit: null,
          exitPromise: null,
          resolveExit: null,
          claimed: false,
        };
        shellProc.states.set(id, state);
      }
      return state;
    },

    // Once a process has exited no further events can arrive for its id, so the
    // dispatch entry is dropped as soon as both sides are done with it. The
    // ChildProcess keeps working through its own closure over `state`.
    release: (id, state) => {
      if (state.claimed && state.exit) shellProc.states.delete(id);
    },

    output: (id, stream, data) => {
      const state = shellProc.state(id);
      const handlers = state.handlers[stream];
      if (!handlers.length && !state.iterators.length) {
        state.buffered.push({ stream, data });
        return;
      }
      handlers.slice().forEach((handler) => handler(data));
      state.iterators
        .slice()
        .forEach((iterator) => shellProc.push(iterator, { stream, data }));
    },

    exited: (payload) => {
      const state = shellProc.state(payload.id);
      const result = {
        code: payload.code ?? null,
        signal: payload.signal ?? null,
        timedOut: payload.timedOut === true,
      };
      state.exit = result;
      if (state.resolveExit) {
        state.resolveExit(result);
        state.resolveExit = null;
      }
      state.exitHandlers.splice(0).forEach((handler) => handler(result));
      // Iterators with buffered chunks left finish once those are drained.
      state.iterators.slice().forEach((iterator) => shellProc.push(iterator, null));
      shellProc.release(payload.id, state);
    },

    // Hand a chunk (or `null` for end-of-stream) to a live iterator: straight to
    // a waiting next(), otherwise queued so nothing is dropped while the
    // consumer is awaiting.
    push: (iterator, chunk) => {
      if (iterator.done) return;
      if (!iterator.resolve) {
        if (chunk) iterator.queue.push(chunk);
        return;
      }
      const resolve = iterator.resolve;
      iterator.resolve = null;
      resolve(chunk ? { value: chunk, done: false } : iterator.finish());
    },

    // The first handler on a stream also drains whatever arrived before it.
    handler: (state, stream, callback) => {
      const handlers = state.handlers[stream];
      handlers.push(callback);
      if (handlers.length === 1) {
        const backlog = state.buffered.filter((chunk) => chunk.stream === stream);
        state.buffered = state.buffered.filter((chunk) => chunk.stream !== stream);
        backlog.forEach((chunk) => callback(chunk.data));
      }
      return () => {
        const index = handlers.indexOf(callback);
        if (index !== -1) handlers.splice(index, 1);
      };
    },

    child: (id, pid) => {
      const state = shellProc.state(id);
      state.claimed = true;
      shellProc.release(id, state);
      return {
        id,
        pid,
        onStdout: (callback) => shellProc.handler(state, "stdout", callback),
        onStderr: (callback) => shellProc.handler(state, "stderr", callback),
        onExit: (callback) => {
          if (state.exit) {
            callback(state.exit);
            return () => {};
          }
          state.exitHandlers.push(callback);
          return () => {
            const index = state.exitHandlers.indexOf(callback);
            if (index !== -1) state.exitHandlers.splice(index, 1);
          };
        },
        write: (data) =>
          window.__TAURI__.core.invoke("shell_process_write", { id, data }),
        closeStdin: () =>
          window.__TAURI__.core.invoke("shell_process_close_stdin", { id }),
        kill: () => window.__TAURI__.core.invoke("shell_process_kill", { id }),
        exit: () => window.__TAURI__.core.invoke("shell_process_exit", { id }),
        setTimeout: (timeoutMs) =>
          window.__TAURI__.core.invoke("shell_process_set_timeout", {
            id,
            timeoutMs: timeoutMs ?? null,
          }),
        get exited() {
          if (!state.exitPromise) {
            state.exitPromise = state.exit
              ? Promise.resolve(state.exit)
              : new Promise((resolve) => {
                  state.resolveExit = resolve;
                });
          }
          return state.exitPromise;
        },
        [Symbol.asyncIterator]: () => {
          // A new iterator adopts anything buffered so far, so iteration started
          // after the first chunks arrived still sees them.
          const iterator = {
            queue: state.buffered.splice(0),
            resolve: null,
            done: false,
            finish: () => {
              iterator.done = true;
              const index = state.iterators.indexOf(iterator);
              if (index !== -1) state.iterators.splice(index, 1);
              return { value: undefined, done: true };
            },
          };
          state.iterators.push(iterator);
          return {
            next: () => {
              if (iterator.done) return Promise.resolve({ value: undefined, done: true });
              if (iterator.queue.length)
                return Promise.resolve({ value: iterator.queue.shift(), done: false });
              if (state.exit) return Promise.resolve(iterator.finish());
              return new Promise((resolve) => {
                iterator.resolve = resolve;
              });
            },
            return: () => Promise.resolve(iterator.finish()),
          };
        },
      };
    },

    // shell.run(name, options) / shell.spawn(name, options): a non-array second
    // argument means "no arguments".
    call: (args, options) => {
      if (Array.isArray(args)) return { args, options: options || {} };
      if (args && typeof args === "object") return { args: [], options: args };
      return { args: [], options: options || {} };
    },
  };

  window.shell = {
    settings: window.__SHELL_SETTINGS__ || {},
    saveFile: (name, contents) =>
      window.__TAURI__.core.invoke("shell_save_file", { name, contents }),
    readFile: (name) => window.__TAURI__.core.invoke("shell_read_file", { name }),
    deleteFile: (name) => window.__TAURI__.core.invoke("shell_delete_file", { name }),
    renameFile: (name, newName) =>
      window.__TAURI__.core.invoke("shell_rename_file", { name, newName }),
    openFile: (name) => window.__TAURI__.core.invoke("shell_open_file", { name }),
    openFileLocation: (name) =>
      window.__TAURI__.core.invoke("shell_open_file_location", { name }),
    log: (message, level) =>
      window.__TAURI__.core.invoke("shell_log", {
        message,
        level: level || "info",
      }),
    notify: (title, body) =>
      window.__TAURI__.core.invoke("shell_notify", { title, body }),
    fetch: (url, opts = {}) =>
      window.__TAURI__.core.invoke("shell_fetch", {
        url,
        method: opts.method,
        headers: opts.headers,
        body: opts.body,
      }),
    get: (url, headers) => window.shell.fetch(url, { method: "GET", headers }),
    post: (url, body, headers) =>
      window.shell.fetch(url, { method: "POST", body, headers }),
    dbQuery: (dbName, query, params = []) =>
      window.__TAURI__.core.invoke("shell_db_query", { dbName, query, params }),
    dbExecute: (dbName, query, params = []) =>
      window.__TAURI__.core.invoke("shell_db_execute", { dbName, query, params }),
    getWindowPosition: () =>
      window.__TAURI__.core.invoke("shell_get_window_position"),
    setWindowPosition: (x, y) =>
      window.__TAURI__.core.invoke("shell_set_window_position", { x, y }),
    getWindowSize: () => window.__TAURI__.core.invoke("shell_get_window_size"),
    setWindowSize: (width, height) =>
      window.__TAURI__.core.invoke("shell_set_window_size", { width, height }),
    minimize: () => window.__TAURI__.core.invoke("shell_minimize_window"),
    getScreens: () => window.__TAURI__.core.invoke("shell_get_screens"),
    getScreenAt: (x, y) =>
      window.__TAURI__.core.invoke("shell_get_screen_at", { x, y }),
    openWindow: (url, options = {}) =>
      window.__TAURI__.core.invoke("shell_open_window", {
        url,
        options: {
          title: options.title,
          width: options.width,
          height: options.height,
        },
      }),
    closeWindow: (id) => window.__TAURI__.core.invoke("shell_close_window", { id }),
    getWindowBody: (id) =>
      window.__TAURI__.core.invoke("shell_get_window_body", { id }),
    evalWindow: (id, code) =>
      window.__TAURI__.core.invoke("shell_eval_window", { id, code }),
    authViaBrowser: (authUrl, options) => {
      let timeoutMs;
      let returnUrl;
      if (typeof options === "number") {
        timeoutMs = options;
      } else if (options && typeof options === "object") {
        timeoutMs = options.timeoutMs;
        returnUrl = options.returnUrl;
      }
      return window.__TAURI__.core.invoke("shell_auth_via_browser", {
        authUrl,
        timeoutMs,
        returnUrl,
      });
    },
    onWindowNavigated: (callback) =>
      window.__TAURI__.event.listen("shell://window-navigated", (event) =>
        callback(event.payload.id, event.payload.url),
      ),
    onWindowLoaded: (callback) =>
      window.__TAURI__.event.listen("shell://window-loaded", (event) =>
        callback(event.payload.id, event.payload.url),
      ),
    onWindowClosed: (callback) =>
      window.__TAURI__.event.listen("shell://window-closed", (event) =>
        callback(event.payload.id),
      ),

    // ── Secure Store (keyring-rs) ─────────────────────────────────────
    secretSet: (service, account, password) =>
      window.__TAURI__.core.invoke("shell_secret_set", { service, account, password }),
    secretGet: (service, account) =>
      window.__TAURI__.core.invoke("shell_secret_get", { service, account }),
    secretDelete: (service, account) =>
      window.__TAURI__.core.invoke("shell_secret_delete", { service, account }),

    // ── HTTP Server ───────────────────────────────────────────────────
    httpStart: (options = {}) =>
      window.__TAURI__.core.invoke("shell_http_start", { port: options.port || null }),
    httpRespond: (id, status, headers, body) =>
      window.__TAURI__.core.invoke("shell_http_respond", { id, status, headers: headers || null, body: body || null }),
    httpStop: () =>
      window.__TAURI__.core.invoke("shell_http_stop"),
    onHttpRequest: (callback) =>
      window.__TAURI__.event.listen("shell://http-request", (event) =>
        callback(event.payload),
      ),

    // ── WebSocket Server ──────────────────────────────────────────────
    wsStart: (options = {}) =>
      window.__TAURI__.core.invoke("shell_ws_start", { port: options.port || null }),
    wsSend: (id, data) =>
      window.__TAURI__.core.invoke("shell_ws_send", { id, data }),
    wsClose: (id) =>
      window.__TAURI__.core.invoke("shell_ws_close", { id }),
    wsStop: () =>
      window.__TAURI__.core.invoke("shell_ws_stop"),
    onWsConnection: (callback) =>
      window.__TAURI__.event.listen("shell://ws-connection", (event) =>
        callback(event.payload),
      ),
    onWsMessage: (callback) =>
      window.__TAURI__.event.listen("shell://ws-message", (event) =>
        callback(event.payload),
      ),
    onWsClose: (callback) =>
      window.__TAURI__.event.listen("shell://ws-close", (event) =>
        callback(event.payload),
      ),

    // ── Process execution ─────────────────────────────────────────────
    run: (name, args, options) => {
      const call = shellProc.call(args, options);
      return window.__TAURI__.core.invoke("shell_run", {
        name,
        args: call.args,
        timeoutMs: call.options.timeoutMs ?? null,
        stdin: call.options.stdin ?? null,
      });
    },
    spawn: async (name, args, options) => {
      const call = shellProc.call(args, options);
      await shellProc.ready();
      const { id, pid } = await window.__TAURI__.core.invoke("shell_spawn", {
        name,
        args: call.args,
        timeoutMs: call.options.timeoutMs ?? null,
      });
      return shellProc.child(id, pid);
    },
    listCommands: () => window.__TAURI__.core.invoke("shell_list_commands"),
  };
})();