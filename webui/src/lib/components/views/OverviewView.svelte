<script lang="ts">
import {
  Activity,
  Bot,
  Boxes,
  CheckCircle2,
  Clock,
  Cpu,
  MessageSquare,
  Server,
  Terminal,
  Zap,
} from 'lucide-svelte';
import { api } from '../../api/client';
import { nodeStore } from '../../stores/node.svelte';

let { onNavigate = (_tab: string) => {} } = $props<{
  onNavigate?: (tab: string) => void;
}>();

let metricsRaw = $state<string>('');
let loadingMetrics = $state(false);

function formatUptime(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m ${s}s`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

async function loadMetrics() {
  loadingMetrics = true;
  try {
    metricsRaw = await api.getMetrics();
  } catch {
    metricsRaw = 'Failed to load prometheus metrics.';
  } finally {
    loadingMetrics = false;
  }
}

$effect(() => {
  loadMetrics();
});
</script>

<div class="p-6 space-y-6 max-w-7xl mx-auto">
  <!-- Key Metrics Row -->
  <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
    <!-- Status & Version Card -->
    <div class="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs">
      <div class="flex items-center justify-between text-zinc-500 text-xs font-medium">
        <span>Node Health</span>
        <Activity class="w-4 h-4 text-emerald-500" />
      </div>
      <div class="mt-2 flex items-baseline gap-2">
        <span class="text-xl font-bold font-mono tracking-tight text-zinc-900 dark:text-zinc-100 capitalize">
          {nodeStore.health?.status ?? 'Connecting'}
        </span>
        <span class="text-xs font-mono text-zinc-400">v{nodeStore.health?.version ?? '0.1.0'}</span>
      </div>
      <p class="mt-1 text-[11px] text-zinc-500">Standalone Microkernel Runtime</p>
    </div>

    <!-- Uptime Card -->
    <div class="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs">
      <div class="flex items-center justify-between text-zinc-500 text-xs font-medium">
        <span>Uptime</span>
        <Clock class="w-4 h-4 text-zinc-400" />
      </div>
      <div class="mt-2 text-xl font-bold font-mono tracking-tight text-zinc-900 dark:text-zinc-100">
        {nodeStore.health?.uptime_secs !== undefined ? formatUptime(nodeStore.health.uptime_secs) : '--'}
      </div>
      <p class="mt-1 text-[11px] text-zinc-500">Continuous scheduler uptime</p>
    </div>

    <!-- Managed Hosts & Plugins -->
    <div class="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs">
      <div class="flex items-center justify-between text-zinc-500 text-xs font-medium">
        <span>Supervisor Hosts</span>
        <Boxes class="w-4 h-4 text-zinc-400" />
      </div>
      <div class="mt-2 flex items-baseline gap-2">
        <span class="text-xl font-bold font-mono tracking-tight text-zinc-900 dark:text-zinc-100">
          {nodeStore.health?.supervisor.managed_hosts ?? 0}
        </span>
        <span class="text-xs font-mono text-zinc-400">({nodeStore.health?.supervisor.total_plugins ?? 0} plugins)</span>
      </div>
      <p class="mt-1 text-[11px] text-zinc-500">Out-of-process isolated sub-processes</p>
    </div>

    <!-- AI Model Gateway -->
    <div class="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs">
      <div class="flex items-center justify-between text-zinc-500 text-xs font-medium">
        <span>LLM Model Gateway</span>
        <Bot class="w-4 h-4 {nodeStore.health?.chat_enabled ? 'text-emerald-500' : 'text-zinc-400'}" />
      </div>
      <div class="mt-2 flex items-baseline gap-1.5 truncate">
        <span class="text-sm font-bold font-mono tracking-tight text-zinc-900 dark:text-zinc-100 truncate">
          {nodeStore.health?.default_model ?? 'Disabled'}
        </span>
      </div>
      <p class="mt-1 text-[11px] text-zinc-500 truncate">
        {nodeStore.health?.chat_enabled ? 'Tool calling & reasoning active' : 'Chat completions disabled'}
      </p>
    </div>
  </div>

  <!-- Quick Action Jump Cards -->
  <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
    <button
      onclick={() => onNavigate('pipeline')}
      class="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 hover:border-zinc-300 dark:hover:border-zinc-700 transition text-left cursor-pointer group shadow-2xs"
    >
      <div class="flex items-center gap-2 text-zinc-900 dark:text-zinc-100 font-semibold text-xs group-hover:text-emerald-600 dark:group-hover:text-emerald-400">
        <Terminal class="w-4 h-4" />
        <span>Stream Logs & Pipeline Events</span>
      </div>
      <p class="mt-1.5 text-xs text-zinc-500 leading-relaxed">
        Observe live events traversing PreFilter, Command, LLM, and Outbound delivery stages.
      </p>
    </button>

    <button
      onclick={() => onNavigate('plugins')}
      class="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 hover:border-zinc-300 dark:hover:border-zinc-700 transition text-left cursor-pointer group shadow-2xs"
    >
      <div class="flex items-center gap-2 text-zinc-900 dark:text-zinc-100 font-semibold text-xs group-hover:text-emerald-600 dark:group-hover:text-emerald-400">
        <Boxes class="w-4 h-4" />
        <span>Manage Plugins & Schema Configs</span>
      </div>
      <p class="mt-1.5 text-xs text-zinc-500 leading-relaxed">
        Inspect loaded plugin descriptors, edit configuration JSON with CAS locks, and restart host processes.
      </p>
    </button>

    <button
      onclick={() => onNavigate('playground')}
      class="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 hover:border-zinc-300 dark:hover:border-zinc-700 transition text-left cursor-pointer group shadow-2xs"
    >
      <div class="flex items-center gap-2 text-zinc-900 dark:text-zinc-100 font-semibold text-xs group-hover:text-emerald-600 dark:group-hover:text-emerald-400">
        <Bot class="w-4 h-4" />
        <span>Interactive AI Sandbox Playground</span>
      </div>
      <p class="mt-1.5 text-xs text-zinc-500 leading-relaxed">
        Conduct streaming multi-turn chat sessions with live function calling and tool execution inspection.
      </p>
    </button>
  </div>

  <!-- Prometheus Metrics Preview -->
  <div class="rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 overflow-hidden shadow-2xs">
    <div class="px-4 py-3 border-b border-zinc-200 dark:border-zinc-800 flex items-center justify-between">
      <div class="flex items-center gap-2">
        <Server class="w-4 h-4 text-zinc-400" />
        <h3 class="text-xs font-semibold text-zinc-900 dark:text-zinc-100 tracking-tight">Prometheus Exposition (/metrics)</h3>
      </div>
      <button
        onclick={loadMetrics}
        class="text-xs text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100 font-mono transition cursor-pointer"
      >
        Refresh
      </button>
    </div>
    <div class="p-4 bg-zinc-950 font-mono text-[11px] text-zinc-300 leading-relaxed max-h-72 overflow-y-auto">
      {#if loadingMetrics}
        <div class="text-zinc-500">Loading metrics exposition...</div>
      {:else}
        <pre class="whitespace-pre-wrap">{metricsRaw}</pre>
      {/if}
    </div>
  </div>
</div>
