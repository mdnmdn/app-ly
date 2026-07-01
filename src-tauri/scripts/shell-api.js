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
};