(function () {
  const isMac =
    navigator.platform.toUpperCase().includes("MAC") ||
    /Mac/i.test(navigator.userAgent);

  function hasPrimaryModifier(event) {
    return isMac ? event.metaKey : event.ctrlKey;
  }

  function toggleDevtools() {
    if (!window.__SHELL_DEV__ || !window.__TAURI__?.core) {
      return;
    }

    window.__TAURI__.core
      .invoke("shell_toggle_devtools")
      .catch((error) => {
        console.error("devtools toggle failed:", error);
      });
  }

  document.addEventListener(
    "keydown",
    (event) => {
      if (!hasPrimaryModifier(event) || !event.shiftKey) {
        return;
      }

      if (event.code === "KeyM" || event.code === "KeyI") {
        event.preventDefault();
        toggleDevtools();
        return;
      }

      if (event.code === "KeyR") {
        event.preventDefault();
        window.location.reload();
      }
    },
    true,
  );
})();