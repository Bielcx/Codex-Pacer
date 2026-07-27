// No bundler here, so we use the window.__TAURI__ global (enabled via
// `app.withGlobalTauri` in tauri.conf.json) instead of importing
// "@tauri-apps/api/core" as a bare module specifier, which the browser
// can't resolve without a bundler/import map.
const { invoke } = window.__TAURI__.core;

async function loadUsage() {
  const percentEl = document.getElementById("percent");
  const resetEl = document.getElementById("reset");
  const sourceEl = document.getElementById("source");

  try {
    const usage = await invoke("get_usage");
    percentEl.textContent = usage.remaining_percent.toFixed(0);
    resetEl.textContent = usage.reset_at;
    sourceEl.textContent = `codex: ${usage.source}`;
  } catch (err) {
    percentEl.textContent = "!";
    sourceEl.textContent = err;
  }
}

document.getElementById("refresh").addEventListener("click", loadUsage);
window.addEventListener("DOMContentLoaded", loadUsage);
