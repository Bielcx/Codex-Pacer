# Codex Pacer

App de bandeja (system tray) para acompanhar e dar ritmo ao uso do [Codex CLI](https://github.com/openai/codex). Foco principal em **Windows**, com **Linux** como plataforma secundária (mesma base de código via Tauri).

Inspirado no [codex-limits](https://github.com/thrr87/codex-limits) (macOS/SwiftUI), reescrito para rodar fora do ecossistema Apple.

> Codex Pacer é um projeto independente, não afiliado à OpenAI.

## Stack

- [Tauri v2](https://tauri.app) (Rust no backend, WebView nativo do SO)
- Frontend simples em HTML/JS/CSS, sem framework (app pequeno o suficiente pra não precisar)

## Status atual

Esqueleto inicial do projeto. Já existe:

- Estrutura do projeto Tauri (`src-tauri` + `src`)
- Tray icon com menu (Quit) e clique pra abrir/fechar a janela popover
- Detecção do binário `codex` no PATH (`where`/`which`), com fallback de caminhos conhecidos
- Comando `get_usage` exposto ao frontend, hoje retornando dado **stub** (fixo)

Ainda **não implementado**:

- Integração real com a interface app-server do Codex CLI (protocolo precisa ser confirmado e implementado em `src-tauri/src/codex.rs`)
- Histórico local (samples diários, JSON versionado)
- Cálculo de ritmo/burn-down comparado com a meta
- Ícones do app (ver seção abaixo)

## Requisitos de desenvolvimento

- [Rust](https://www.rust-lang.org/tools/install) (toolchain estável) + Cargo
- [Node.js](https://nodejs.org/) 18+
- Windows: [Build Tools for Visual Studio](https://tauri.app/start/prerequisites/) (C++ workload) e WebView2 (já vem no Windows 11; no Windows 10 pode precisar instalar)
- Linux: dependências de sistema listadas nos [pré-requisitos do Tauri](https://tauri.app/start/prerequisites/#linux) (webkit2gtk, libappindicator, etc.)
- Codex CLI instalado e no PATH (`npm i -g @openai/codex` ou instalador oficial)

## Rodando em desenvolvimento

```bash
npm install
npm run dev
```

## Gerando os ícones

O projeto ainda não tem os ícones reais. Antes do primeiro build, gere a partir de uma imagem-fonte (PNG quadrado, de preferência 1024x1024):

```bash
npx tauri icon caminho/para/logo.png
```

Isso cria a pasta `src-tauri/icons` com os formatos que o `tauri.conf.json` já espera.

## Build

```bash
npm run build
```

Gera instaladores Windows (NSIS/MSI) e, quando rodado em Linux, pacotes `.deb`/AppImage.

## Licença

MIT.
