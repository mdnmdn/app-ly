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

  // ── AI (internal plumbing) ────────────────────────────────────────
  // Backs shell.ai.*. Kept outside window.shell so only the documented API
  // surface is exposed to contents HTML. Streaming follows the same
  // ordering discipline as shellProc: the event listeners are registered
  // once and awaited before the invoke, and anything that arrives before a
  // consumer exists is buffered until one shows up.
  const shellAi = {
    states: new Map(), // request id -> stream dispatch state
    tools: new Map(), // request id -> Map(tool name -> handler)
    listeners: null,
    requests: 0,

    // JS owns the request id: handlers can be keyed before the invoke is even
    // issued, so a tool call that arrives first still binds deterministically.
    // Rust treats it as opaque and echoes it back on every event for the request.
    requestId: () =>
      `ai-${++shellAi.requests}-${Math.random().toString(36).slice(2, 10)}`,

    // The three event listeners are registered once, lazily. Every request
    // that can produce events awaits this before invoking, so no chunk and
    // no tool call can arrive before a listener exists.
    ready: () => {
      if (!shellAi.listeners) {
        shellAi.listeners = Promise.all([
          window.__TAURI__.event.listen("shell://ai-chunk", (event) =>
            shellAi.chunk(event.payload.id, event.payload.text),
          ),
          window.__TAURI__.event.listen("shell://ai-done", (event) =>
            shellAi.finished(event.payload),
          ),
          window.__TAURI__.event.listen("shell://ai-tool-call", (event) =>
            shellAi.toolCall(event.payload),
          ),
        ]).catch((error) => {
          shellAi.listeners = null; // let the next request retry
          throw error;
        });
      }
      return shellAi.listeners;
    },

    // Events may be delivered before the shell_ai_stream invoke resolves, so
    // state is created by whichever side reaches the id first.
    state: (id) => {
      let state = shellAi.states.get(id);
      if (!state) {
        state = {
          handlers: [], // onText callbacks
          buffered: [], // deltas with no consumer yet, in arrival order
          iterators: [],
          done: null, // { text, model, toolCalls }
          error: null, // message from shell://ai-done, if any
          completed: null,
          resolveCompleted: null,
          rejectCompleted: null,
          claimed: false,
        };
        shellAi.states.set(id, state);
      }
      return state;
    },

    // Once a request is done no further events can arrive for its id, so the
    // dispatch entry is dropped as soon as both sides are done with it. The
    // AiStream keeps working through its own closure over `state`.
    release: (id, state) => {
      if (state.claimed && state.done) shellAi.states.delete(id);
    },

    chunk: (id, text) => {
      const state = shellAi.state(id);
      if (!state.handlers.length && !state.iterators.length) {
        state.buffered.push(text);
        return;
      }
      state.handlers.slice().forEach((handler) => handler(text));
      state.iterators.slice().forEach((iterator) => shellAi.push(iterator, text));
    },

    finished: (payload) => {
      const state = shellAi.state(payload.id);
      state.error = payload.error ?? null;
      state.done = {
        text: payload.text ?? "",
        model: payload.model ?? null,
        toolCalls: payload.toolCalls || [],
      };
      if (state.error) {
        if (state.rejectCompleted) state.rejectCompleted(new Error(state.error));
      } else if (state.resolveCompleted) {
        state.resolveCompleted(state.done);
      }
      state.resolveCompleted = null;
      state.rejectCompleted = null;
      // Iterators with buffered deltas left finish once those are drained.
      state.iterators.slice().forEach((iterator) => shellAi.push(iterator, null));
      shellAi.unregister(payload.id);
      shellAi.release(payload.id, state);
    },

    // Hand a delta (or `null` for end-of-stream) to a live iterator: straight
    // to a waiting next(), otherwise queued so nothing is dropped while the
    // consumer is awaiting. An empty delta is still a delta, so the
    // end-of-stream sentinel is checked explicitly.
    push: (iterator, text) => {
      if (iterator.done) return;
      if (!iterator.resolve) {
        if (text !== null) iterator.queue.push(text);
        return;
      }
      const resolve = iterator.resolve;
      iterator.resolve = null;
      resolve(text !== null ? { value: text, done: false } : iterator.finish());
    },

    // The first handler also drains whatever arrived before it.
    handler: (state, callback) => {
      state.handlers.push(callback);
      if (state.handlers.length === 1) {
        state.buffered.splice(0).forEach((text) => callback(text));
      }
      return () => {
        const index = state.handlers.indexOf(callback);
        if (index !== -1) state.handlers.splice(index, 1);
      };
    },

    handle: (id) => {
      const state = shellAi.state(id);
      state.claimed = true;
      shellAi.release(id, state);
      return {
        id,
        onText: (callback) => shellAi.handler(state, callback),
        cancel: () => {
          if (state.done) return Promise.resolve();
          return window.__TAURI__.core
            .invoke("shell_ai_cancel", { id })
            .catch(() => {});
        },
        get completed() {
          if (!state.completed) {
            if (state.done) {
              state.completed = state.error
                ? Promise.reject(new Error(state.error))
                : Promise.resolve(state.done);
            } else {
              state.completed = new Promise((resolve, reject) => {
                state.resolveCompleted = resolve;
                state.rejectCompleted = reject;
              });
            }
          }
          return state.completed;
        },
        [Symbol.asyncIterator]: () => {
          // A new iterator adopts anything buffered so far, so iteration
          // started after the first deltas arrived still sees them.
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
              if (state.done) return Promise.resolve(iterator.finish());
              return new Promise((resolve) => {
                iterator.resolve = resolve;
              });
            },
            return: () => Promise.resolve(iterator.finish()),
          };
        },
      };
    },

    // ── Tool bridge ─────────────────────────────────────────────────
    // Handlers stay in JS keyed by request id; only { name, description,
    // parameters } is sent.
    register: (id, tools) => {
      const handlers = new Map();
      (tools || []).forEach((tool) => {
        if (tool && tool.name && typeof tool.handler === "function")
          handlers.set(tool.name, tool.handler);
      });
      if (handlers.size) shellAi.tools.set(id, handlers);
      return handlers;
    },

    unregister: (id) => shellAi.tools.delete(id),

    handlerFor: (id, name) => {
      const handlers = shellAi.tools.get(id);
      return handlers && handlers.get(name);
    },

    // A handler that throws reports the failure to the model; it must never
    // leave the backend waiting for its tool timeout.
    toolCall: async (payload) => {
      const result = { callId: payload.callId, ok: true, value: null, error: null };
      try {
        const handler = shellAi.handlerFor(payload.id, payload.name);
        if (!handler) throw new Error(`unknown tool "${payload.name}"`);
        const value = await handler(payload.arguments ?? {});
        result.value = value === undefined ? null : value;
      } catch (error) {
        result.ok = false;
        result.value = null;
        result.error = (error && error.message) || String(error);
      }
      return window.__TAURI__.core
        .invoke("shell_ai_tool_result", result)
        .catch(() => {});
    },

    specs: (tools) =>
      (tools || []).map((tool) => ({
        name: tool.name,
        description: tool.description || "",
        parameters: tool.parameters || { type: "object", properties: {} },
      })),

    options: (options) => ({
      model: options.model ?? null,
      instructions: options.instructions ?? null,
      temperature: options.temperature ?? null,
      maxTokens: options.maxTokens ?? null,
      toolTimeoutMs: options.toolTimeoutMs ?? null,
      tools: shellAi.specs(options.tools),
    }),

    unavailable: (error) => ({
      available: false,
      reason: "unavailable",
      detail: (error && error.message) || String(error),
      models: [],
      features: { text: false, structured: false, tools: false, streaming: false },
    }),

    // shell.ai.generate / shell.ai.generateObject: tool handlers are registered
    // under this request's id and torn down when it finishes, so tools never
    // leak across requests and concurrent requests never cross-wire.
    request: async (command, prompt, options, extra) => {
      const opts = options || {};
      const requestId = shellAi.requestId();
      const handlers = shellAi.register(requestId, opts.tools);
      try {
        if (handlers.size) await shellAi.ready();
        const payload = { requestId, prompt, options: shellAi.options(opts) };
        if (extra) Object.assign(payload, extra);
        return await window.__TAURI__.core.invoke(command, payload);
      } finally {
        shellAi.unregister(requestId);
      }
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
    dbClose: (dbName) =>
      window.__TAURI__.core.invoke("shell_db_close", { dbName: dbName ?? null }),
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

    // ── AI (on-device LLM) ────────────────────────────────────────────
    ai: {
      // Never rejects: an unreachable backend reads as "unavailable".
      info: () =>
        window.__TAURI__.core
          .invoke("shell_ai_info")
          .catch((error) => shellAi.unavailable(error)),
      available: () => window.shell.ai.info().then((info) => info.available === true),
      models: () =>
        window.shell.ai.info().then((info) => (Array.isArray(info.models) ? info.models : [])),
      generate: (prompt, options) =>
        shellAi.request("shell_ai_generate", prompt, options),
      generateObject: (prompt, schema, options) =>
        shellAi.request("shell_ai_generate_object", prompt, options, { schema }),
      stream: async (prompt, options) => {
        const opts = options || {};
        const requestId = shellAi.requestId();
        shellAi.register(requestId, opts.tools);
        try {
          await shellAi.ready();
          // Resolves once the listeners are live and the backend has accepted
          // the request; the handle echoes back the id we supplied.
          await window.__TAURI__.core.invoke("shell_ai_stream", {
            requestId,
            prompt,
            options: shellAi.options(opts),
          });
          return shellAi.handle(requestId);
        } catch (error) {
          shellAi.unregister(requestId);
          throw error;
        }
      },
    },
  };
})();