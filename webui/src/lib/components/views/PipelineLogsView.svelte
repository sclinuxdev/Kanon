<script lang="ts">
import {
  Activity,
  AlertCircle,
  ArrowDown,
  Bot,
  CheckCircle2,
  Clock,
  Filter,
  Layers,
  RefreshCw,
  Search,
  Terminal,
  Trash2,
  Zap,
} from 'lucide-svelte';
import { onMount } from 'svelte';
import { t } from '../../stores/i18n.svelte';
import { logStore } from '../../stores/logs.svelte';
import { nodeStore } from '../../stores/node.svelte';
import { pipelineStore } from '../../stores/pipeline.svelte';
import type { LogLevel } from '../../types';

let logContainer = $state<HTMLDivElement | null>(null);

onMount(() => {
  if (pipelineStore.status === 'disconnected') {
    pipelineStore.reconnect();
  }
  if (logStore.status === 'disconnected') {
    logStore.reconnect();
  }
});

// Auto scroll to bottom when new logs arrive if autoScroll is enabled
$effect(() => {
  if (logStore.autoScroll && logContainer) {
    logContainer.scrollTop = logContainer.scrollHeight;
  }
});

const levels: (LogLevel | 'ALL')[] = ['ALL', 'INFO', 'WARN', 'ERROR', 'DEBUG'];

function formatTime(timestampMs: number): string {
  const d = new Date(timestampMs);
  return (
    d.toTimeString().split(' ')[0] +
    '.' +
    String(d.getMilliseconds()).padStart(3, '0')
  );
}

function getLevelBadgeClass(level: LogLevel): string {
  switch (level) {
    case 'ERROR':
      return 'bg-rose-500/10 text-rose-600 dark:text-rose-400 border border-rose-500/20';
    case 'WARN':
      return 'bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20';
    case 'INFO':
      return 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20';
    case 'DEBUG':
      return 'bg-zinc-500/10 text-zinc-600 dark:text-zinc-400 border border-zinc-500/20';
  }
}

function getStageColor(stage: string): string {
  if (stage === 'ingested')
    return 'text-blue-500 bg-blue-500/10 border-blue-500/20';
  if (stage.includes('blocked') || stage.includes('failed'))
    return 'text-rose-500 bg-rose-500/10 border-rose-500/20';
  if (stage.includes('command'))
    return 'text-violet-500 bg-violet-500/10 border-violet-500/20';
  if (stage.includes('llm') || stage.includes('tool'))
    return 'text-indigo-500 bg-indigo-500/10 border-indigo-500/20';
  if (stage.includes('outbound') || stage.includes('passed'))
    return 'text-emerald-500 bg-emerald-500/10 border-emerald-500/20';
  if (stage.includes('breaker'))
    return 'text-amber-500 bg-amber-500/10 border-amber-500/20';
  return 'text-zinc-400 bg-zinc-500/10 border-zinc-500/20';
}
</script>

<div class="h-full flex flex-col md:flex-row overflow-hidden divide-y md:divide-y-0 md:divide-x divide-zinc-200 dark:divide-zinc-800">
  <!-- Left Column: Pipeline Stages & Events Stream -->
  <div class="w-full md:w-5/12 flex flex-col h-1/2 md:h-full bg-zinc-50/40 dark:bg-zinc-950/40">
    <!-- Header -->
    <div class="p-3 border-b border-zinc-200 dark:border-zinc-800 flex items-center justify-between shrink-0 bg-white dark:bg-zinc-900">
      <div class="flex items-center gap-2">
        <Layers class="w-4.5 h-4.5 text-zinc-500" />
        <span class="text-sm font-semibold text-zinc-900 dark:text-zinc-100 tracking-tight">{t('pipeline.live_events')}</span>
      </div>
      <div class="flex items-center gap-2">
        <span class="inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-xs font-mono {pipelineStore.status === 'connected' ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400' : pipelineStore.status === 'connecting' || pipelineStore.status === 'reconnecting' ? 'bg-amber-500/10 text-amber-600 dark:text-amber-400' : 'bg-rose-500/10 text-rose-600 dark:text-rose-400'}">
          <span class="w-1.5 h-1.5 rounded-full {pipelineStore.status === 'connected' ? 'bg-emerald-500' : pipelineStore.status === 'connecting' || pipelineStore.status === 'reconnecting' ? 'bg-amber-500 animate-pulse' : 'bg-rose-500'}"></span>
          {pipelineStore.status}
        </span>
        {#if pipelineStore.status !== 'connected'}
          <button
            onclick={() => pipelineStore.reconnect()}
            class="p-1 text-indigo-500 hover:text-indigo-600 transition cursor-pointer"
            title="Reconnect Pipeline WebSocket"
          >
            <RefreshCw class="w-3.5 h-3.5" />
          </button>
        {/if}
        <span class="text-xs font-mono text-zinc-400">({pipelineStore.records.length})</span>
        <button
          onclick={() => pipelineStore.clear()}
          class="p-1 text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 transition cursor-pointer"
          title={t('common.clear')}
        >
          <Trash2 class="w-4 h-4" />
        </button>
      </div>
    </div>

    <!-- Quick stage stats chips -->
    <div class="p-2.5 border-b border-zinc-200/80 dark:border-zinc-800/80 flex items-center gap-2 overflow-x-auto text-xs font-mono shrink-0">
      <button
        onclick={() => (pipelineStore.selectedStage = 'ALL')}
        class="px-2.5 py-1 rounded-md cursor-pointer transition {pipelineStore.selectedStage === 'ALL' ? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900' : 'text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800'}"
      >
        All ({pipelineStore.records.length})
      </button>
      <button
        onclick={() => (pipelineStore.selectedStage = 'ingested')}
        class="px-2.5 py-1 rounded-md cursor-pointer transition {pipelineStore.selectedStage === 'ingested' ? 'bg-blue-600 text-white' : 'text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800'}"
      >
        Ingest ({pipelineStore.stats.ingested})
      </button>
      <button
        onclick={() => (pipelineStore.selectedStage = 'pre_filter')}
        class="px-2.5 py-1 rounded-md cursor-pointer transition {pipelineStore.selectedStage === 'pre_filter' ? 'bg-zinc-600 text-white' : 'text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800'}"
      >
        PreFilter ({pipelineStore.stats.pre_filter})
      </button>
      <button
        onclick={() => (pipelineStore.selectedStage = 'command')}
        class="px-2.5 py-1 rounded-md cursor-pointer transition {pipelineStore.selectedStage === 'command' ? 'bg-violet-600 text-white' : 'text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800'}"
      >
        Cmd ({pipelineStore.stats.command})
      </button>
      <button
        onclick={() => (pipelineStore.selectedStage = 'llm')}
        class="px-2.5 py-1 rounded-md cursor-pointer transition {pipelineStore.selectedStage === 'llm' ? 'bg-indigo-600 text-white' : 'text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800'}"
      >
        LLM ({pipelineStore.stats.llm})
      </button>
      <button
        onclick={() => (pipelineStore.selectedStage = 'outbound')}
        class="px-2.5 py-1 rounded-md cursor-pointer transition {pipelineStore.selectedStage === 'outbound' ? 'bg-emerald-600 text-white' : 'text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800'}"
      >
        Outbound ({pipelineStore.stats.outbound})
      </button>
    </div>

    <!-- Event Timeline List -->
    <div class="flex-1 overflow-y-auto p-3 space-y-2.5 font-mono text-xs sm:text-sm">
      {#if pipelineStore.filteredRecords.length === 0}
        <div class="p-8 text-center text-zinc-400 text-sm font-sans space-y-3">
          {#if pipelineStore.status !== 'connected'}
            <!-- An empty panel caused by a dead stream must say so, otherwise it looks
                 like the node simply has nothing to report. -->
            <p>{t('pipeline.offline')} <span class="font-mono">({pipelineStore.status})</span></p>
            <button
              onclick={() => pipelineStore.reconnect()}
              class="px-3 py-1.5 rounded-lg border border-zinc-300 dark:border-zinc-700 text-zinc-600 dark:text-zinc-300 text-xs cursor-pointer hover:border-indigo-400"
            >
              {t('pipeline.reconnect')}
            </button>
          {:else}
            <p>{t('pipeline.empty_events')}</p>
            {#if (nodeStore.health?.instances?.enabled ?? 0) === 0}
              <!-- The most common reason for a silent pipeline: nothing claims the adapter, so
                   inbound messages are dropped before any stage is emitted. -->
              <p class="text-amber-600 dark:text-amber-400 text-xs">
                {t('pipeline.no_instance_hint')}
              </p>
            {/if}
          {/if}
        </div>
      {:else}
        {#each pipelineStore.filteredRecords.slice().reverse() as record, i (record.seq ?? i)}
          <div class="p-3 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs space-y-2">
            <div class="flex items-center justify-between text-xs">
              <span class="inline-flex items-center px-2 py-0.5 rounded text-xs font-semibold border {getStageColor(record.event?.stage ?? '')}">
                {record.event?.stage ?? 'unknown'}
              </span>
              <span class="text-zinc-400 text-xs">{formatTime(record.timestamp_ms)}</span>
            </div>

            <!-- Stage Context Details -->
            <div class="text-xs sm:text-sm text-zinc-700 dark:text-zinc-300 space-y-1">
              {#if record.event.model}
                <div class="text-indigo-600 dark:text-indigo-400 font-semibold">
                  Model: {record.event.model}
                  {#if record.event.message_count !== undefined}
                    <span class="text-zinc-400 font-normal">({record.event.message_count} msgs, {record.event.tool_count ?? 0} tools)</span>
                  {/if}
                </div>
              {/if}
              {#if record.event.content_length !== undefined}
                <div class="text-emerald-600 dark:text-emerald-400">
                  Response: {record.event.content_length} chars
                  {#if record.event.finish_reason}
                    <span class="text-zinc-400">({record.event.finish_reason})</span>
                  {/if}
                </div>
              {/if}
              {#if record.event.tool_name}
                <div class="text-violet-600 dark:text-violet-400 font-semibold flex items-center gap-1.5">
                  <span>Tool: {record.event.tool_name}</span>
                  {#if record.event.success !== undefined}
                    <span class="{record.event.success ? 'text-emerald-500' : 'text-rose-500'} font-bold">
                      {record.event.success ? '✓' : '✗'}
                    </span>
                  {/if}
                </div>
              {/if}
              {#if record.event.event_id}
                <div class="flex items-center gap-1.5 text-zinc-500 text-[10px]">
                  <span>ID:</span>
                  <span class="text-zinc-700 dark:text-zinc-300 truncate">{record.event.event_id}</span>
                </div>
              {/if}
              {#if record.event.platform}
                <div class="flex items-center gap-1.5 text-zinc-500 text-[10px]">
                  <span>Platform:</span>
                  <span class="font-semibold text-zinc-800 dark:text-zinc-200">{record.event.platform}</span>
                  {#if record.event.channel_id}
                    <span>• {record.event.channel_id}</span>
                  {/if}
                </div>
              {/if}
              {#if record.event.command}
                <div class="text-violet-600 dark:text-violet-400 font-semibold">
                  /{record.event.command} (plugin: {record.event.plugin_id ?? 'unknown'})
                </div>
              {/if}
              {#if record.event.session_id}
                <div class="text-indigo-600 dark:text-indigo-400">
                  Session: {record.event.session_id}
                </div>
              {/if}
              {#if record.event.raw_text}
                <div class="text-zinc-500 text-[10px] truncate">
                  {record.event.raw_text}
                </div>
              {/if}
              {#if record.event.reason}
                <div class="text-rose-500 text-[10px]">
                  Reason: {record.event.reason}
                </div>
              {/if}
            </div>
          </div>
        {/each}
      {/if}
    </div>
  </div>

  <!-- Right Column: Rolling Terminal Console -->
  <div class="w-full md:w-7/12 flex flex-col h-1/2 md:h-full bg-zinc-950 text-zinc-200 font-mono text-xs sm:text-[13px]">
    <!-- Toolbar -->
    <div class="p-3 bg-zinc-900 border-b border-zinc-800 flex flex-wrap items-center justify-between gap-2.5 shrink-0">
      <div class="flex items-center gap-1.5 text-xs">
        {#each levels as lvl}
          <button
            onclick={() => logStore.setFilter(lvl)}
            class="px-2.5 py-1 rounded cursor-pointer transition text-xs font-semibold {logStore.filterLevel === lvl ? 'bg-zinc-800 text-white border border-zinc-700' : 'text-zinc-400 hover:text-zinc-200'}"
          >
            {lvl}
          </button>
        {/each}
      </div>

      <div class="flex items-center gap-2.5">
        <span class="inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-xs font-mono {logStore.status === 'connected' ? 'bg-emerald-500/10 text-emerald-400' : logStore.status === 'connecting' || logStore.status === 'reconnecting' ? 'bg-amber-500/10 text-amber-400' : 'bg-rose-500/10 text-rose-400'}">
          <span class="w-1.5 h-1.5 rounded-full {logStore.status === 'connected' ? 'bg-emerald-500' : logStore.status === 'connecting' || logStore.status === 'reconnecting' ? 'bg-amber-500 animate-pulse' : 'bg-rose-500'}"></span>
          {logStore.status}
        </span>
        {#if logStore.status !== 'connected'}
          <button
            onclick={() => logStore.reconnect()}
            class="p-1 text-indigo-400 hover:text-indigo-300 transition cursor-pointer"
            title="Reconnect Logs WebSocket"
          >
            <RefreshCw class="w-3.5 h-3.5" />
          </button>
        {/if}

        <!-- Search filter input -->
        <div class="relative">
          <input
            type="text"
            bind:value={logStore.searchQuery}
            placeholder={t('common.search')}
            class="w-40 sm:w-52 px-2.5 py-1.5 pl-7 text-xs bg-zinc-950 border border-zinc-800 rounded-md text-zinc-200 placeholder-zinc-500 focus:outline-hidden focus:border-zinc-700"
          />
          <Search class="w-3.5 h-3.5 text-zinc-500 absolute left-2 top-2" />
        </div>

        <!-- Auto-scroll lock toggle -->
        <button
          onclick={() => (logStore.autoScroll = !logStore.autoScroll)}
          class="p-1.5 rounded cursor-pointer transition {logStore.autoScroll ? 'text-emerald-400' : 'text-zinc-500'}"
          title={logStore.autoScroll ? 'Auto-scroll enabled' : 'Auto-scroll paused'}
        >
          <ArrowDown class="w-4 h-4" />
        </button>

        <!-- Clear logs -->
        <button
          onclick={() => logStore.clear()}
          class="p-1.5 text-zinc-400 hover:text-rose-400 transition cursor-pointer"
          title={t('common.clear')}
        >
          <Trash2 class="w-4 h-4" />
        </button>
      </div>
    </div>

    <!-- Terminal logs viewport -->
    <div
      bind:this={logContainer}
      class="flex-1 p-3.5 overflow-y-auto space-y-1.5 font-mono text-xs sm:text-[13px] leading-relaxed select-text"
    >
      {#if logStore.filteredRecords.length === 0}
        <div class="text-zinc-600 p-8 text-center font-sans text-sm space-y-3">
          {#if logStore.status !== 'connected'}
            <p>{t('pipeline.offline')} <span class="font-mono">({logStore.status})</span></p>
            <button
              onclick={() => logStore.reconnect()}
              class="px-3 py-1.5 rounded-lg border border-zinc-300 dark:border-zinc-700 text-zinc-600 dark:text-zinc-300 text-xs cursor-pointer hover:border-indigo-400"
            >
              {t('pipeline.reconnect')}
            </button>
          {:else}
            <p>{t('pipeline.empty_logs')}</p>
            {#if logStore.filterLevel !== 'ALL'}
              <!-- An empty log panel is usually a level filter, not a dead node. -->
              <p class="text-xs">
                {t('pipeline.level_filter_hint').replace('{level}', logStore.filterLevel)}
              </p>
            {/if}
          {/if}
        </div>
      {:else}
        {#each logStore.filteredRecords as record, i (`${record.timestamp_ms}_${i}`)}
          <div class="flex items-start gap-2.5 hover:bg-zinc-900/60 p-1 rounded">
            <span class="text-zinc-500 shrink-0 text-xs">{formatTime(record.timestamp_ms)}</span>
            <span class="px-1.5 rounded text-[10px] sm:text-xs font-bold shrink-0 {getLevelBadgeClass(record.level)}">
              {record.level}
            </span>
            <span class="text-zinc-400 shrink-0 max-w-[160px] truncate text-xs" title={record.target}>
              {(record.target || 'kanon_core').split('::').slice(-2).join('::')}
            </span>
            <span class="text-zinc-200 break-all">{record.message}</span>
          </div>
        {/each}
      {/if}
    </div>
  </div>
</div>
