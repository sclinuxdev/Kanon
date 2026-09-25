<script lang="ts">
import { Radio, RefreshCw } from 'lucide-svelte';
import { logStore } from '../../stores/logs.svelte';
import { nodeStore } from '../../stores/node.svelte';
import { pipelineStore } from '../../stores/pipeline.svelte';

let { title = 'Overview', subtitle = '' } = $props<{
  title?: string;
  subtitle?: string;
}>();

let isRefreshing = $state(false);

async function handleRefresh() {
  isRefreshing = true;
  await nodeStore.refresh();
  setTimeout(() => {
    isRefreshing = false;
  }, 400);
}
</script>

<header class="h-14 border-b border-zinc-200 dark:border-zinc-800 bg-white/60 dark:bg-zinc-950/60 backdrop-blur-md px-6 flex items-center justify-between shrink-0">
  <div>
    <h2 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100 tracking-tight">{title}</h2>
    {#if subtitle}
      <p class="text-[11px] text-zinc-500 dark:text-zinc-400">{subtitle}</p>
    {/if}
  </div>

  <div class="flex items-center gap-3">
    <!-- WebSocket connection status indicators -->
    <div class="hidden sm:flex items-center gap-2 text-[11px] font-mono text-zinc-500 dark:text-zinc-400 bg-zinc-100 dark:bg-zinc-900 px-2.5 py-1 rounded-md border border-zinc-200 dark:border-zinc-800">
      <span class="flex items-center gap-1">
        <Radio class="w-3 h-3 {pipelineStore.status === 'connected' ? 'text-emerald-500' : 'text-amber-500'}" />
        <span>Events</span>
      </span>
      <span class="text-zinc-300 dark:text-zinc-700">|</span>
      <span class="flex items-center gap-1">
        <Radio class="w-3 h-3 {logStore.status === 'connected' ? 'text-emerald-500' : 'text-amber-500'}" />
        <span>Logs</span>
      </span>
    </div>

    <!-- Manual refresh button -->
    <button
      onclick={handleRefresh}
      class="p-1.5 text-zinc-500 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-zinc-100 rounded-md hover:bg-zinc-100 dark:hover:bg-zinc-900 transition border border-transparent hover:border-zinc-200 dark:hover:border-zinc-800 cursor-pointer"
      title="Refresh node status"
    >
      <RefreshCw class="w-3.5 h-3.5 {isRefreshing ? 'animate-spin text-zinc-900 dark:text-zinc-100' : ''}" />
    </button>
  </div>
</header>
