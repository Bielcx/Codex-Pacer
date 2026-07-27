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

/// Humanizes the time left until `resetAtIso`, e.g. "Resets in 3h 53m".
/// Only includes the units that matter (no "0d" on a same-day reset).
function formatResetCountdown(resetAtIso) {
  const diffMs = new Date(resetAtIso).getTime() - Date.now();
  if (diffMs <= 0) return "Resets shortly";

  const totalMinutes = Math.round(diffMs / 60000);
  const days = Math.floor(totalMinutes / (24 * 60));
  const hours = Math.floor((totalMinutes % (24 * 60)) / 60);
  const minutes = totalMinutes % 60;

  const parts = [];
  if (days > 0) parts.push(`${days}d`);
  if (days > 0 || hours > 0) parts.push(`${hours}h`);
  parts.push(`${minutes}m`);

  return `Resets in ${parts.join(" ")}`;
}

/// Rounded delta between actual and target remaining %, e.g. "-18%" or "+4%".
/// `|| 0` folds Math.round's possible -0 (e.g. from -0.4) into a plain 0.
function formatPaceDelta(remainingPercent, targetRemainingPercent) {
  const rounded = Math.round(remainingPercent - targetRemainingPercent) || 0;
  const sign = rounded > 0 ? "+" : "";
  return `${sign}${rounded}%`;
}

/// Compact token count, e.g. 15420 -> "15K", 218400000 -> "218M".
function formatTokenCount(tokens) {
  if (tokens >= 1_000_000) return `${Math.round(tokens / 1_000_000)}M`;
  if (tokens >= 1_000) return `${Math.round(tokens / 1_000)}K`;
  return `${tokens}`;
}

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

async function refreshCodex() {
  const percentEl = document.getElementById("percent");
  const progressFillEl = document.getElementById("progress-fill");
  const resetEl = document.getElementById("reset");
  const sourceEl = document.getElementById("source");
  const paceEl = document.getElementById("pace");
  const chartEl = document.getElementById("chart");
  const tokensTodayEl = document.getElementById("tokens-today");
  const tokens30dEl = document.getElementById("tokens-30d");

  try {
    const [pacing, history, costSummary] = await Promise.all([
      invoke("get_pacing"),
      invoke("get_history"),
      invoke("get_cost_summary"),
    ]);

    percentEl.textContent = pacing.remaining_percent.toFixed(0);
    progressFillEl.style.width = `${Math.min(Math.max(pacing.remaining_percent, 0), 100)}%`;
    resetEl.textContent = formatResetCountdown(pacing.reset_at);
    sourceEl.textContent = `codex: ${pacing.source}`;

    const verdictClass = `verdict-${pacing.verdict.replace(/_/g, "-")}`;
    const verdictLabel = VERDICT_LABELS[pacing.verdict] ?? pacing.verdict;
    const delta = formatPaceDelta(pacing.remaining_percent, pacing.target_remaining_percent);
    paceEl.textContent = `${delta} vs target · ${verdictLabel}`;
    paceEl.className = `pace ${verdictClass}`;

    chartEl.innerHTML = buildChartSvg(
      history,
      pacing.window_start,
      pacing.reset_at,
      pacing.safety_buffer_percent
    );

    tokensTodayEl.textContent = formatTokenCount(costSummary.today_tokens);
    tokens30dEl.textContent = formatTokenCount(costSummary.last_30_days_tokens);
  } catch (err) {
    percentEl.textContent = "!";
    progressFillEl.style.width = "0%";
    resetEl.textContent = "--";
    paceEl.textContent = "";
    paceEl.className = "pace";
    chartEl.innerHTML = "";
    tokensTodayEl.textContent = "--";
    tokens30dEl.textContent = "--";
    sourceEl.textContent = err;
  }
}

/// Renders one rate-limit window (five_hour or seven_day) as a mini version
/// of the Codex tab's remaining/pace cards, reusing the same CSS classes.
function renderClaudeWindowCard(label, rateLimitWindow) {
  const verdictClass = `verdict-${rateLimitWindow.verdict.replace(/_/g, "-")}`;
  const verdictLabel = VERDICT_LABELS[rateLimitWindow.verdict] ?? rateLimitWindow.verdict;
  const delta = formatPaceDelta(rateLimitWindow.remaining_percent, rateLimitWindow.target_remaining_percent);
  const width = Math.min(Math.max(rateLimitWindow.remaining_percent, 0), 100);

  return `
    <section class="card">
      <p class="label">${label}</p>
      <p class="remaining">${rateLimitWindow.remaining_percent.toFixed(0)}%</p>
      <div class="progress-track">
        <div class="progress-fill" style="width: ${width}%"></div>
      </div>
      <p class="reset">${formatResetCountdown(rateLimitWindow.reset_at)}</p>
      <p class="pace ${verdictClass}">${delta} vs target · ${verdictLabel}</p>
    </section>
  `;
}

function renderClaudeTab(result) {
  const container = document.getElementById("claude-content");

  if (!result.installed) {
    container.innerHTML = `<p class="source">Claude Code CLI not found on PATH. Install it to enable tracking.</p>`;
    return;
  }

  if (!result.configured) {
    container.innerHTML = `
      <p class="source">
        Track Claude Code's session and weekly limits via its statusLine hook.
        No OAuth, no cookies, no Keychain access - just a local JSON file.
      </p>
      <button id="claude-setup">Enable tracking</button>
    `;
    document.getElementById("claude-setup").addEventListener("click", async () => {
      await invoke("setup_claude_integration");
      refreshClaude();
    });
    return;
  }

  const parts = [];
  if (result.stale) {
    parts.push(`<p class="source claude-stale">Stale - Claude Code hasn't run recently.</p>`);
  }

  if (!result.captured_at) {
    parts.push(`<p class="source">Waiting for Claude Code to run. Open a session and send a message.</p>`);
  } else if (!result.five_hour && !result.seven_day) {
    parts.push(`
      <p class="source">
        No rate-limit data in the last status update - you may be on API-key/free-tier
        billing (no session or weekly limits), or haven't sent a message yet.
      </p>
    `);
  } else {
    if (result.five_hour) parts.push(renderClaudeWindowCard("Session (5h)", result.five_hour));
    if (result.seven_day) parts.push(renderClaudeWindowCard("Weekly (7d)", result.seven_day));
  }

  parts.push(`<button id="claude-unsetup" class="claude-unsetup">Disable tracking</button>`);
  container.innerHTML = parts.join("");
  document.getElementById("claude-unsetup").addEventListener("click", async () => {
    await invoke("unsetup_claude_integration");
    refreshClaude();
  });
}

async function refreshClaude() {
  try {
    const result = await invoke("get_claude_pacing");
    renderClaudeTab(result);
  } catch (err) {
    document.getElementById("claude-content").innerHTML = `<p class="source">${err}</p>`;
  }
}

function refreshAll() {
  refreshCodex();
  refreshClaude();
}

function initTabs() {
  const buttons = document.querySelectorAll(".tab-btn");
  const panels = document.querySelectorAll(".tab-panel");

  const activate = (tab) => {
    buttons.forEach((b) => b.setAttribute("aria-selected", String(b.dataset.tab === tab)));
    panels.forEach((p) => {
      p.hidden = p.id !== `tab-${tab}`;
    });
    localStorage.setItem("activeTab", tab);
  };

  buttons.forEach((b) => b.addEventListener("click", () => activate(b.dataset.tab)));
  activate(localStorage.getItem("activeTab") || "codex");
}

document.getElementById("refresh").addEventListener("click", refreshAll);
window.addEventListener("DOMContentLoaded", () => {
  initTabs();
  refreshAll();
});

// The popover window is shown/hidden rather than reloaded, so it never
// fires DOMContentLoaded again after the first launch. Refresh whenever the
// window regains focus - covers the "user clicked the tray icon to open it"
// trigger - plus a 10-minute timer as a fallback. Tauri has no simple
// cross-platform "system woke from sleep" event, so this interval also
// doubles as the practical stand-in for that trigger: worst case, the
// numbers are up to 10 minutes stale right after waking, which then
// self-corrects on the next tick.
window.addEventListener("focus", refreshAll);
setInterval(refreshAll, REFRESH_INTERVAL_MS);
