// No bundler here, so we use the window.__TAURI__ global (enabled via
// `app.withGlobalTauri` in tauri.conf.json) instead of importing
// "@tauri-apps/api/core" as a bare module specifier, which the browser
// can't resolve without a bundler/import map.
const { invoke } = window.__TAURI__.core;

const VERDICT_LABELS = {
  slow_down: "Slow down",
  on_track: "On track",
  room_to_use_more: "Room to use more",
};

const REFRESH_INTERVAL_MS = 10 * 60 * 1000; // 10 minutes

/// Builds the inner SVG markup for a simple burn-down chart: a dashed
/// straight target line from 100% (window start) to the safety buffer
/// (reset), and a solid line through the actual samples recorded so far in
/// the current window.
function buildChartSvg(historyPoints, windowStartIso, resetAtIso, bufferPercent) {
  const width = 300;
  const height = 100;
  const padding = 6;

  const startMs = new Date(windowStartIso).getTime();
  const endMs = new Date(resetAtIso).getTime();
  const span = endMs - startMs || 1;

  const x = (iso) => {
    const ratio = (new Date(iso).getTime() - startMs) / span;
    return padding + Math.min(Math.max(ratio, 0), 1) * (width - 2 * padding);
  };
  const y = (percent) => padding + (1 - percent / 100) * (height - 2 * padding);

  const targetPath = `M ${x(windowStartIso)} ${y(100)} L ${x(resetAtIso)} ${y(bufferPercent)}`;

  const windowed = historyPoints
    .filter((p) => new Date(p.observed_at).getTime() >= startMs)
    .sort((a, b) => new Date(a.observed_at) - new Date(b.observed_at));

  const actualPath = windowed
    .map((p, i) => `${i === 0 ? "M" : "L"} ${x(p.observed_at)} ${y(p.remaining_percent)}`)
    .join(" ");

  return `
    <line x1="${padding}" y1="${y(0)}" x2="${width - padding}" y2="${y(0)}" class="chart-axis" />
    <path d="${targetPath}" class="chart-target" />
    ${actualPath ? `<path d="${actualPath}" class="chart-actual" />` : ""}
  `;
}

async function refresh() {
  const percentEl = document.getElementById("percent");
  const resetEl = document.getElementById("reset");
  const sourceEl = document.getElementById("source");
  const verdictEl = document.getElementById("verdict");
  const chartEl = document.getElementById("chart");

  try {
    const [pacing, history] = await Promise.all([invoke("get_pacing"), invoke("get_history")]);

    percentEl.textContent = pacing.remaining_percent.toFixed(0);
    resetEl.textContent = pacing.reset_at;
    sourceEl.textContent = `codex: ${pacing.source}`;

    verdictEl.textContent = VERDICT_LABELS[pacing.verdict] ?? pacing.verdict;
    verdictEl.className = `verdict verdict-${pacing.verdict.replace(/_/g, "-")}`;

    chartEl.innerHTML = buildChartSvg(
      history,
      pacing.window_start,
      pacing.reset_at,
      pacing.safety_buffer_percent
    );
  } catch (err) {
    percentEl.textContent = "!";
    verdictEl.textContent = "";
    chartEl.innerHTML = "";
    sourceEl.textContent = err;
  }
}

document.getElementById("refresh").addEventListener("click", refresh);
window.addEventListener("DOMContentLoaded", refresh);

// The popover window is shown/hidden rather than reloaded, so it never
// fires DOMContentLoaded again after the first launch. Refresh whenever the
// window regains focus - covers the "user clicked the tray icon to open it"
// trigger - plus a 10-minute timer as a fallback. Tauri has no simple
// cross-platform "system woke from sleep" event, so this interval also
// doubles as the practical stand-in for that trigger: worst case, the
// numbers are up to 10 minutes stale right after waking, which then
// self-corrects on the next tick.
window.addEventListener("focus", refresh);
setInterval(refresh, REFRESH_INTERVAL_MS);
