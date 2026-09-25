<script lang="ts">
import {
  Activity,
  Blocks,
  Bot,
  Laptop,
  Moon,
  Radio,
  Sun,
  Terminal,
  Users,
} from 'lucide-svelte';
import { nodeStore } from '../../stores/node.svelte';
import { theme } from '../../stores/theme.svelte';

let {
  currentTab = 'overview',
  onTabChange = (_tab: string) => {},
  onOpenCommand = () => {},
} = $props<{
  currentTab?: string;
  onTabChange?: (tab: string) => void;
  onOpenCommand?: () => void;
}>();

const navItems = [
  { id: 'overview', label: 'Overview', icon: Activity },
  { id: 'pipeline', label: 'Pipeline & Logs', icon: Terminal },
  { id: 'plugins', label: 'Plugins & Adapters', icon: Blocks },
  { id: 'sessions', label: 'Sessions & Personas', icon: Users },
  { id: 'playground', label: 'Playground', icon: Bot },
];
</script>

<aside class="w-64 border-r border-zinc-200 dark:border-zinc-800 bg-zinc-50/60 dark:bg-zinc-950 flex flex-col justify-between shrink-0 select-none">
  <!-- Brand Header -->
  <div class="p-4 border-b border-zinc-200/80 dark:border-zinc-800/80">
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-2.5">
        <div class="w-7 h-7 rounded-lg bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900 flex items-center justify-center font-bold text-sm tracking-tight shadow-xs">
          K
        </div>
        <div>
          <h1 class="text-sm font-semibold tracking-tight text-zinc-900 dark:text-zinc-100">Kanon</h1>
          <p class="text-[11px] text-zinc-500 dark:text-zinc-400 font-mono">v{nodeStore.health?.version ?? '0.1.0'}</p>
        </div>
      </div>
      <span class="inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-medium font-mono uppercase tracking-wider
        {nodeStore.health?.status === 'healthy' ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20' : 'bg-rose-500/10 text-rose-600 dark:text-rose-400 border border-rose-500/20'}">
        <span class="w-1.5 h-1.5 rounded-full mr-1 {nodeStore.health?.status === 'healthy' ? 'bg-emerald-500 animate-pulse' : 'bg-rose-500'}"></span>
        {nodeStore.health?.status ?? 'connecting'}
      </span>
    </div>

    <!-- Quick search hotkey hint -->
    <button
      onclick={onOpenCommand}
      class="mt-3 w-full flex items-center justify-between px-2.5 py-1.5 text-xs text-zinc-500 dark:text-zinc-400 bg-white dark:bg-zinc-900/80 border border-zinc-200 dark:border-zinc-800 rounded-md hover:border-zinc-300 dark:hover:border-zinc-700 transition shadow-2xs cursor-pointer"
    >
      <span class="flex items-center gap-1.5">
        <Radio class="w-3.5 h-3.5 text-zinc-400" />
        <span>Command Menu</span>
      </span>
      <kbd class="px-1.5 py-0.5 text-[10px] font-mono bg-zinc-100 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded text-zinc-500">⌘K</kbd>
    </button>
  </div>

  <!-- Navigation items -->
  <nav class="flex-1 p-3 space-y-1 overflow-y-auto">
    {#each navItems as item}
      {@const Icon = item.icon}
      <button
        onclick={() => onTabChange(item.id)}
        class="w-full flex items-center gap-2.5 px-3 py-2 rounded-md text-xs font-medium transition cursor-pointer
          {currentTab === item.id
            ? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-950 font-semibold shadow-xs'
            : 'text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100 hover:bg-zinc-200/50 dark:hover:bg-zinc-900/60'}"
      >
        <Icon class="w-4 h-4 shrink-0" />
        <span>{item.label}</span>
      </button>
    {/each}
  </nav>

  <!-- Bottom toolbar: theme & runtime summary -->
  <div class="p-3 border-t border-zinc-200/80 dark:border-zinc-800/80 space-y-2">
    <div class="flex items-center justify-between px-1">
      <span class="text-[11px] text-zinc-500 font-mono">Appearance</span>
      <div class="flex items-center gap-1 bg-zinc-200/60 dark:bg-zinc-900 p-0.5 rounded-md border border-zinc-200 dark:border-zinc-800">
        <button
          onclick={() => theme.setMode('light')}
          class="p-1 rounded text-xs transition cursor-pointer {theme.currentMode === 'light' ? 'bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 shadow-2xs' : 'text-zinc-400 hover:text-zinc-600'}"
          title="Light mode"
        >
          <Sun class="w-3.5 h-3.5" />
        </button>
        <button
          onclick={() => theme.setMode('system')}
          class="p-1 rounded text-xs transition cursor-pointer {theme.currentMode === 'system' ? 'bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 shadow-2xs' : 'text-zinc-400 hover:text-zinc-600'}"
          title="System sync"
        >
          <Laptop class="w-3.5 h-3.5" />
        </button>
        <button
          onclick={() => theme.setMode('dark')}
          class="p-1 rounded text-xs transition cursor-pointer {theme.currentMode === 'dark' ? 'bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 shadow-2xs' : 'text-zinc-400 hover:text-zinc-600'}"
          title="Dark mode"
        >
          <Moon class="w-3.5 h-3.5" />
        </button>
      </div>
    </div>
  </div>
</aside>
