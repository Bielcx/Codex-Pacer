import { invoke } from "@tauri-apps/api/core";

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
