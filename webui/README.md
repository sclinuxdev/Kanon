# Kanon WebUI Console

Modern, decoupled web console and AI playground for Kanon microkernel node.

## Tech Stack
- **Framework**: Svelte 5 (Runes `$state`, `$derived`, `$effect`) + TypeScript
- **Build Tool**: Vite 6
- **Styling**: Tailwind CSS v4
- **Icons**: lucide-svelte
- **Tooling**: Bun + Biome (Rust-based ultra-fast linter & formatter)

## Features
- **Overview**: Node health, uptime, Supervisor host counters, Prometheus exposition.
- **Pipeline & Logs**:
  - Live pipeline stage transitions from `/ws/v1/events` (Ingested -> PreFilter -> Command -> LLM -> Tool -> Outbound).
  - High-density scrolling log terminal from `/ws/v1/logs` with level filtering (`DEBUG`, `INFO`, `WARN`, `ERROR`), search, and auto-scroll lock.
- **Plugins & Adapters**: Inspect active plugin hosts, view & edit configuration schemas with CAS concurrency protection, restart hosts, and simulate inbound Fast-ACK events.
- **Sessions & Personas**: Review conversation context memory, token counters, reset session contexts, and switch dynamic personas.
- **AI Playground**: Interactive streaming chat via SSE with live tool execution audit breakdown.
- **Command Palette**: Press `Cmd + K` or `Ctrl + K` anytime to switch views or execute actions.

## Development

```bash
# Install dependencies
bun install

# Start Vite dev server (proxies /api and /ws to http://127.0.0.1:8080)
bun run dev

# Run typecheck
bun run check

# Lint & format with Biome
bun run lint
bun run format

# Production build (outputs to dist/)
bun run build
```
