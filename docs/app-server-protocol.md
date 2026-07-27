# Codex CLI app-server protocol notes

Source: [developers.openai.com/codex/app-server](https://developers.openai.com/codex/app-server) and [openai/codex/codex-rs/app-server](https://github.com/openai/codex/tree/main/codex-rs/app-server).

## Transport

`codex app-server` speaks JSON-RPC 2.0 (the `"jsonrpc":"2.0"` field is omitted on the wire). Default transport is stdio: newline-delimited JSON, one message per line. Also supports `--listen ws://...` (experimental) and Unix sockets — Codex Pacer only uses stdio, which needs no extra flags and works the same on Windows, macOS, and Linux.

## Handshake

Every connection must, in order:

1. Send `initialize` (request, with `id`) including `clientInfo: { name, title, version }`.
2. Send `initialized` (notification, no `id`).

Any other method called before this handshake returns a `Not initialized` error. Codex Pacer does this once per spawned `app-server` process in `AppServerClient::spawn`.

## Reading usage

Method: `account/rateLimits/read` (request, empty params).

Example response:

```json
{
  "id": 1,
  "result": {
    "rateLimits": {
      "primary": { "usedPercent": 25, "windowDurationMins": 15, "resetsAt": 1730947200 },
      "rateLimitReachedType": null
    },
    "rateLimitsByLimitId": {
      "codex": {
        "primary": { "usedPercent": 42, "windowDurationMins": 60, "resetsAt": 1730950800 },
        "rateLimitReachedType": null
      }
    },
    "rateLimitResetCredits": { "availableCount": 2 }
  }
}
```

Fields we use today (`rateLimits.primary`):

- `usedPercent` — current usage within the quota window (0-100). Codex Pacer stores `100 - usedPercent` as `remaining_percent`.
- `resetsAt` — Unix timestamp (seconds) for the next reset. Converted to RFC 3339 via `chrono`.

Not used yet, worth revisiting for issue #2/#3 (history + pacing):

- `rateLimitsByLimitId` — per-model breakdown (keyed by `limit_id`, e.g. `"codex"`). The original macOS app also tracked independent model-specific limits; this is the equivalent here.
- `account/rateLimits/updated` — server-pushed notification when limits change; could replace polling later, but the app-server process would need to stay alive across the whole app lifetime instead of being spawned per poll.

## Reading token usage

Method: `account/usage/read` (request, empty params).

Confirmed response shape (captured against a real logged-in `codex` CLI):

```json
{
  "dailyUsageBuckets": [
    { "startDate": "2026-07-02", "tokens": 26651227 },
    { "startDate": "2026-07-24", "tokens": 146903926 }
  ],
  "summary": {
    "currentStreakDays": 0,
    "lifetimeTokens": 763755773,
    "longestRunningTurnSec": 73369,
    "longestStreakDays": 7,
    "peakDailyTokens": 146903926
  }
}
```

Notes:

- `dailyUsageBuckets` is sparse — days with zero usage are simply absent, not zero-valued entries. `startDate` is a plain `YYYY-MM-DD` string, not RFC 3339.
- There is **no cost/dollar figure anywhere in this response**, and no per-model or input/output/cached token breakdown — just a total `tokens` count per day. Computing a dollar amount (like CodexBar's "$0.04") would require per-model pricing and a model-level usage breakdown that this endpoint doesn't provide, so Codex Pacer surfaces tokens only (`get_cost_summary` / `codex::read_cost_summary`, despite the name, returns `today_tokens` and `last_30_days_tokens`, no cost field).
- `summary` is lifetime, not windowed — not used by Codex Pacer today.

## Current implementation choice

`AppServerClient` spawns a fresh `codex app-server` process for every `read_usage()` call and kills it right after (see `Drop` impl). Simple and correct for a refresh-every-10-minutes tray app. If polling frequency increases or we want to consume `account/rateLimits/updated` notifications, switch to a long-lived client held in Tauri app state instead.
