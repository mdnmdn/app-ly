window.shell = {
  saveFile: (name, contents) =>
    window.__TAURI__.core.invoke("shell_save_file", { name, contents }),
  readFile: (name) => window.__TAURI__.core.invoke("shell_read_file", { name }),
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
};