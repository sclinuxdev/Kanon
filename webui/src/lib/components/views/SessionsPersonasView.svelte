<script lang="ts">
import {
  Check,
  Key,
  MessageSquare,
  RefreshCw,
  Sparkles,
  Trash2,
  Users,
} from 'lucide-svelte';
import { api } from '../../api/client';
import type { PersonaItem, SessionSummary } from '../../types';

let sessions = $state<SessionSummary[]>([]);
let personas = $state<PersonaItem[]>([]);
let loading = $state(true);
let error = $state<string | null>(null);

// Selected session for persona binding modal
let bindingSession = $state<SessionSummary | null>(null);
let selectedPersona = $state<string>('');
let bindingStatus = $state<string | null>(null);

async function loadData() {
  loading = true;
  error = null;
  try {
    const [sessRes, persRes] = await Promise.all([
      api.getSessions(),
      api.getPersonas(),
    ]);
    sessions = sessRes.sessions;
    personas = persRes.personas;
  } catch (e) {
    error = e instanceof Error ? e.message : String(e);
  } finally {
    loading = false;
  }
}

async function handleResetSession(sessionId: string) {
  if (!confirm(`Reset conversation history for session '${sessionId}'?`))
    return;
  try {
    await api.resetSession(sessionId);
    await loadData();
  } catch (e) {
    alert(`Reset failed: ${e instanceof Error ? e.message : String(e)}`);
  }
}

async function applyPersonaSwitch() {
  if (!bindingSession || !selectedPersona) return;
  bindingStatus = 'Switching persona...';
  try {
    await api.setSessionPersona(bindingSession.session_id, selectedPersona);
    bindingStatus = null;
    bindingSession = null;
    await loadData();
  } catch (e) {
    bindingStatus = `Switch failed: ${e instanceof Error ? e.message : String(e)}`;
  }
}

$effect(() => {
  loadData();
});
</script>

<div class="p-6 space-y-6 max-w-7xl mx-auto">
  <!-- Top bar -->
  <div class="flex items-center justify-between">
    <div>
      <h3 class="text-base font-semibold text-zinc-900 dark:text-zinc-100 tracking-tight">Sessions & Personas</h3>
      <p class="text-xs text-zinc-500">Inspect active conversation memory, token budget usage, and persona prompt catalogs</p>
    </div>
    <button
      onclick={loadData}
      class="px-2.5 py-1.5 text-xs font-medium rounded-md bg-white dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 text-zinc-700 dark:text-zinc-300 hover:bg-zinc-50 dark:hover:bg-zinc-700 transition cursor-pointer flex items-center gap-1.5 shadow-2xs"
    >
      <RefreshCw class="w-3.5 h-3.5" />
      <span>Reload</span>
    </button>
  </div>

  {#if loading}
    <div class="p-12 text-center text-xs text-zinc-400">Loading conversation sessions...</div>
  {:else if error}
    <div class="p-4 rounded-lg bg-rose-500/10 border border-rose-500/20 text-rose-600 dark:text-rose-400 text-xs">
      {error}
    </div>
  {:else}
    <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
      <!-- Left 2 Cols: Sessions List -->
      <div class="lg:col-span-2 space-y-3">
        <h4 class="text-xs font-semibold text-zinc-700 dark:text-zinc-300 uppercase tracking-wider font-mono">
          Active Sessions ({sessions.length})
        </h4>

        {#if sessions.length === 0}
          <div class="p-8 rounded-xl border border-dashed border-zinc-300 dark:border-zinc-800 text-center text-zinc-400 text-xs">
            No active conversation sessions in memory yet. Initiate a chat turn in the Playground or via adapter ingress.
          </div>
        {:else}
          <div class="space-y-2">
            {#each sessions as session}
              <div class="p-3.5 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs flex items-center justify-between gap-4">
                <div class="space-y-1 min-w-0">
                  <div class="flex items-center gap-2">
                    <span class="font-mono font-bold text-xs text-zinc-900 dark:text-zinc-100 truncate">
                      {session.session_id}
                    </span>
                    {#if session.active_persona}
                      <span class="px-1.5 py-0.2 rounded text-[10px] font-mono bg-violet-500/10 text-violet-600 dark:text-violet-400 border border-violet-500/20">
                        {session.active_persona}
                      </span>
                    {/if}
                  </div>
                  <div class="flex items-center gap-3 text-[11px] text-zinc-500 font-mono">
                    <span>Turns: <b class="text-zinc-800 dark:text-zinc-200 font-semibold">{session.turn_count}</b></span>
                    <span>Tokens: <b class="text-zinc-800 dark:text-zinc-200 font-semibold">{session.total_tokens_used}</b></span>
                  </div>
                </div>

                <div class="flex items-center gap-2 shrink-0">
                  <button
                    onclick={() => {
                      bindingSession = session;
                      selectedPersona = session.active_persona ?? '';
                    }}
                    class="px-2 py-1 text-xs text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100 border border-zinc-200 dark:border-zinc-700 rounded hover:bg-zinc-50 dark:hover:bg-zinc-800 transition cursor-pointer"
                  >
                    Persona
                  </button>
                  <button
                    onclick={() => handleResetSession(session.session_id)}
                    class="p-1 text-zinc-400 hover:text-rose-500 transition cursor-pointer"
                    title="Reset Session History"
                  >
                    <Trash2 class="w-3.5 h-3.5" />
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>

      <!-- Right 1 Col: Personas Catalog -->
      <div class="space-y-3">
        <h4 class="text-xs font-semibold text-zinc-700 dark:text-zinc-300 uppercase tracking-wider font-mono">
          Persona Catalog ({personas.length})
        </h4>

        <div class="space-y-2">
          {#each personas as persona}
            <div class="p-3 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs space-y-1.5">
              <div class="flex items-center gap-1.5 text-xs font-semibold text-zinc-900 dark:text-zinc-100">
                <Sparkles class="w-3.5 h-3.5 text-indigo-500" />
                <span>{persona.name}</span>
              </div>
              <p class="text-[11px] text-zinc-500 dark:text-zinc-400 line-clamp-3 leading-relaxed font-mono">
                {persona.system_prompt}
              </p>
            </div>
          {/each}
        </div>
      </div>
    </div>
  {/if}
</div>

<!-- Persona Switch Modal -->
{#if bindingSession}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="fixed inset-0 bg-black/40 backdrop-blur-xs z-50 flex items-center justify-center p-4"
    onclick={() => (bindingSession = null)}
    role="button"
    tabindex="-1"
  >
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="w-full max-w-md bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl shadow-2xl p-5 space-y-4"
      onclick={(e) => e.stopPropagation()}
      role="dialog"
      tabindex="-1"
    >
      <div class="flex items-center justify-between border-b border-zinc-200 dark:border-zinc-800 pb-3">
        <h3 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">
          Switch Persona for Session
        </h3>
        <button
          onclick={() => (bindingSession = null)}
          class="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 text-xs font-mono cursor-pointer"
        >
          Cancel
        </button>
      </div>

      <div class="space-y-2">
        <!-- svelte-ignore a11y_label_has_associated_control -->
        <label class="block text-xs font-medium text-zinc-600 dark:text-zinc-400">Select Persona</label>
        <select
          bind:value={selectedPersona}
          class="w-full p-2 bg-zinc-50 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 rounded-lg text-xs font-mono text-zinc-900 dark:text-zinc-100"
        >
          <option value="">(None - Default System Persona)</option>
          {#each personas as p}
            <option value={p.name}>{p.name}</option>
          {/each}
        </select>
      </div>

      {#if bindingStatus}
        <div class="text-xs text-rose-500 font-mono">{bindingStatus}</div>
      {/if}

      <div class="flex justify-end gap-2 pt-2">
        <button
          onclick={() => (bindingSession = null)}
          class="px-3 py-1.5 text-xs text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800 rounded transition cursor-pointer"
        >
          Cancel
        </button>
        <button
          onclick={applyPersonaSwitch}
          class="px-3 py-1.5 text-xs bg-indigo-600 hover:bg-indigo-500 text-white rounded font-medium transition cursor-pointer"
        >
          Bind Persona
        </button>
      </div>
    </div>
  </div>
{/if}
