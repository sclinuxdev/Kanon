<script lang="ts">
import {
  AlertTriangle,
  Check,
  Plug,
  Plus,
  Power,
  RefreshCw,
  Save,
  Server,
  Trash2,
  X,
} from 'lucide-svelte';
import { api } from '../../api/client';
import { t } from '../../stores/i18n.svelte';
import type { McpServerView, McpTransport } from '../../types';

/**
 * MCP server console.
 *
 * The node talks to every enabled server through the same tool router as plugin hosts, so this
 * view only edits definitions and shows health: adding a server here makes its tools callable on
 * the next model turn without restarting the node.
 */
let servers = $state<McpServerView[]>([]);
let loading = $state(true);
let error = $state<string | null>(null);
let notice = $state<string | null>(null);

// Editor state. `null` id means "create"; an id means "replace that definition".
let editorOpen = $state(false);
let editingId = $state<string | null>(null);
let formId = $state('');
let formName = $state('');
let formKind = $state<'stdio' | 'http'>('stdio');
let formCommand = $state('');
let formArgs = $state('');
let formUrl = $state('');
let formKeyValues = $state('');
let saving = $state(false);
let formError = $state<string | null>(null);

async function loadData() {
  loading = true;
  error = null;
  try {
    const res = await api.getMcpServers();
    servers = res.servers;
  } catch (e) {
    error = e instanceof Error ? e.message : String(e);
  } finally {
    loading = false;
  }
}

/** Renders `KEY=VALUE` / `Key: Value` lines back into a map. */
function parseKeyValues(raw: string): Record<string, string> {
  const parsed: Record<string, string> = {};
  for (const line of raw.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const separator = trimmed.includes('=') ? '=' : ':';
    const index = trimmed.indexOf(separator);
    if (index <= 0) {
      throw new Error(`Line '${trimmed}' is not a KEY=VALUE pair`);
    }
    const key = trimmed.slice(0, index).trim();
    const value = trimmed.slice(index + 1).trim();
    if (!key || !value) {
      throw new Error(`Line '${trimmed}' is not a KEY=VALUE pair`);
    }
    parsed[key] = value;
  }
  return parsed;
}

/** Renders a map back into editable lines. */
function formatKeyValues(values: Record<string, string>): string {
  return Object.entries(values)
    .map(([key, value]) => `${key}=${value}`)
    .join('\n');
}

function openCreate() {
  editingId = null;
  formId = '';
  formName = '';
  formKind = 'stdio';
  formCommand = '';
  formArgs = '';
  formUrl = '';
  formKeyValues = '';
  formError = null;
  editorOpen = true;
}

function openEdit(server: McpServerView) {
  editingId = server.id;
  formId = server.id;
  formName = server.name;
  formKind = server.transport.type;
  if (server.transport.type === 'stdio') {
    formCommand = server.transport.command;
    formArgs = server.transport.args.join(' ');
    formKeyValues = formatKeyValues(server.transport.env);
  } else {
    formUrl = server.transport.url;
    formKeyValues = formatKeyValues(server.transport.headers);
  }
  formError = null;
  editorOpen = true;
}

function closeEditor() {
  editorOpen = false;
  formError = null;
}

async function save() {
  saving = true;
  formError = null;
  try {
    const id = formId.trim();
    if (!id) throw new Error('A server identifier is required');

    let transport: McpTransport;
    if (formKind === 'stdio') {
      const command = formCommand.trim();
      if (!command) throw new Error('A command is required for a stdio server');
      transport = {
        type: 'stdio',
        command,
        args: formArgs.trim() ? formArgs.trim().split(/\s+/) : [],
        env: parseKeyValues(formKeyValues),
      };
    } else {
      const url = formUrl.trim();
      if (!url) throw new Error('A URL is required for an HTTP server');
      transport = { type: 'http', url, headers: parseKeyValues(formKeyValues) };
    }

    const saved = await api.upsertMcpServer(id, {
      name: formName.trim() || null,
      transport,
    });
    notice = `MCP server '${saved.id}' saved (state: ${saved.health.state}, tools: ${saved.health.tools})`;
    editorOpen = false;
    await loadData();
  } catch (e) {
    formError = e instanceof Error ? e.message : String(e);
  } finally {
    saving = false;
  }
}

async function toggle(server: McpServerView) {
  try {
    const res = await api.setMcpServerEnabled(server.id, !server.enabled);
    notice = res.message;
    await loadData();
  } catch (e) {
    error = e instanceof Error ? e.message : String(e);
  }
}

async function remove(server: McpServerView) {
  if (
    !confirm(
      `Remove MCP server '${server.id}'? Its tools disappear from every instance.`,
    )
  ) {
    return;
  }
  try {
    const res = await api.removeMcpServer(server.id);
    notice = res.message;
    await loadData();
  } catch (e) {
    error = e instanceof Error ? e.message : String(e);
  }
}

/** Maps a health state onto a badge class. */
function healthClass(server: McpServerView): string {
  if (!server.enabled)
    return 'border-zinc-200 dark:border-zinc-700 text-zinc-500';
  switch (server.health.state) {
    case 'connected':
      return 'border-emerald-200 dark:border-emerald-800/60 text-emerald-600 dark:text-emerald-400 bg-emerald-50 dark:bg-emerald-950/60';
    case 'failed':
      return 'border-rose-200 dark:border-rose-800/60 text-rose-600 dark:text-rose-400 bg-rose-50 dark:bg-rose-950/60';
    default:
      return 'border-amber-200 dark:border-amber-800/60 text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-950/60';
  }
}

$effect(() => {
  void loadData();
});
</script>

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h3 class="text-base sm:text-lg font-semibold text-zinc-900 dark:text-zinc-100 tracking-tight">
        {t('mcp.title')}
      </h3>
      <p class="text-xs sm:text-sm text-zinc-500">{t('mcp.subtitle')}</p>
    </div>
    <div class="flex items-center gap-2">
      <button
        onclick={openCreate}
        class="px-3 py-1.5 text-xs sm:text-sm font-medium rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white transition cursor-pointer flex items-center gap-1.5 shadow-2xs"
      >
        <Plus class="w-4 h-4" />
        <span>{t('mcp.add')}</span>
      </button>
      <button
        onclick={loadData}
        class="px-3 py-1.5 text-xs sm:text-sm font-medium rounded-lg bg-white dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 text-zinc-700 dark:text-zinc-300 hover:bg-zinc-50 dark:hover:bg-zinc-700 transition cursor-pointer flex items-center gap-1.5 shadow-2xs"
      >
        <RefreshCw class="w-4 h-4" />
        <span>{t('common.refresh')}</span>
      </button>
    </div>
  </div>

  {#if notice}
    <p class="text-sm text-emerald-600 dark:text-emerald-400 flex items-center gap-2">
      <Check class="w-4 h-4" /> {notice}
    </p>
  {/if}
  {#if error}
    <p class="text-sm text-rose-600 dark:text-rose-400 flex items-center gap-2">
      <AlertTriangle class="w-4 h-4" /> {error}
    </p>
  {/if}

  {#if loading}
    <div class="p-12 text-center text-sm text-zinc-400">{t('common.loading')}</div>
  {:else if servers.length === 0}
    <div
      class="p-8 rounded-xl border border-dashed border-zinc-300 dark:border-zinc-800 text-center text-zinc-400 text-sm space-y-2"
    >
      <Server class="w-6 h-6 mx-auto stroke-[1.5]" />
      <p>{t('mcp.empty')}</p>
      <p class="text-xs">{t('mcp.empty_hint')}</p>
    </div>
  {:else}
    <div class="grid grid-cols-1 gap-4">
      {#each servers as server (server.id)}
        <div
          class="p-5 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs space-y-3"
        >
          <div class="flex flex-wrap items-start justify-between gap-3">
            <div class="flex items-start gap-3">
              <div class="p-2.5 rounded-xl bg-indigo-500/10 text-indigo-600 dark:text-indigo-400">
                <Plug class="w-5 h-5" />
              </div>
              <div>
                <div class="flex items-center gap-2 flex-wrap">
                  <span class="font-semibold text-zinc-900 dark:text-zinc-100">{server.name}</span>
                  <code class="text-xs font-mono text-zinc-400">{server.id}</code>
                  <span class="px-2 py-0.5 rounded-md text-xs font-medium border {healthClass(server)}">
                    {server.enabled ? server.health.state : t('mcp.disabled')}
                  </span>
                  {#if server.enabled}
                    <span class="text-xs text-zinc-500">
                      {server.health.tools} {t('mcp.tools')}
                      {#if server.health.failures > 0}
                        · {server.health.failures} {t('mcp.failures')}
                      {/if}
                    </span>
                  {/if}
                </div>
                <code class="block text-xs font-mono text-zinc-500 mt-1.5 break-all">
                  {server.transport.type === 'stdio'
                    ? `${server.transport.command} ${server.transport.args.join(' ')}`
                    : server.transport.url}
                </code>
                <code class="block text-[11px] font-mono text-zinc-400 mt-0.5">
                  {server.host_id}
                </code>
                {#if server.health.last_error}
                  <p class="text-xs text-rose-500 mt-1 break-all">{server.health.last_error}</p>
                {/if}
              </div>
            </div>

            <div class="flex items-center gap-2">
              <button
                onclick={() => toggle(server)}
                class="px-3 py-1.5 rounded-lg text-xs font-medium border transition cursor-pointer
                  {server.enabled
                  ? 'border-zinc-200 dark:border-zinc-700 text-zinc-600 dark:text-zinc-300 hover:border-zinc-400'
                  : 'border-emerald-300 dark:border-emerald-800 text-emerald-600 dark:text-emerald-400 hover:bg-emerald-50 dark:hover:bg-emerald-950/40'}"
              >
                <span class="flex items-center gap-1.5">
                  <Power class="w-3.5 h-3.5" />
                  {server.enabled ? t('plugins.disable') : t('plugins.enable')}
                </span>
              </button>
              <button
                onclick={() => openEdit(server)}
                class="p-1.5 rounded-lg text-zinc-500 hover:text-indigo-600 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition cursor-pointer"
                title={t('mcp.edit')}
              >
                <Save class="w-4 h-4" />
              </button>
              <button
                onclick={() => remove(server)}
                class="p-1.5 rounded-lg text-zinc-500 hover:text-rose-600 hover:bg-rose-50 dark:hover:bg-rose-950/30 transition cursor-pointer"
                title={t('mcp.remove')}
              >
                <Trash2 class="w-4 h-4" />
              </button>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if editorOpen}
  <div class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/40 backdrop-blur-sm">
    <div
      class="bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-2xl shadow-xl w-full max-w-xl max-h-[90vh] overflow-y-auto"
    >
      <div class="flex items-center justify-between px-5 py-4 border-b border-zinc-200 dark:border-zinc-800">
        <h3 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">
          {editingId ? t('mcp.edit_title') : t('mcp.add_title')}
        </h3>
        <button
          onclick={closeEditor}
          class="p-1.5 rounded-lg text-zinc-400 hover:text-zinc-700 dark:hover:text-zinc-200 transition cursor-pointer"
        >
          <X class="w-4.5 h-4.5" />
        </button>
      </div>

      <div class="p-5 space-y-4">
        {#if formError}
          <p class="text-sm text-rose-600 dark:text-rose-400 flex items-center gap-2">
            <AlertTriangle class="w-4 h-4" /> {formError}
          </p>
        {/if}

        <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <label class="space-y-1.5">
            <span class="text-xs font-medium text-zinc-500">{t('mcp.field_id')}</span>
            <input
              bind:value={formId}
              disabled={Boolean(editingId)}
              placeholder="filesystem"
              class="w-full px-3 py-2 text-sm font-mono bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg focus:outline-hidden focus:border-indigo-400 disabled:opacity-60"
            />
          </label>
          <label class="space-y-1.5">
            <span class="text-xs font-medium text-zinc-500">{t('mcp.field_name')}</span>
            <input
              bind:value={formName}
              placeholder="Filesystem"
              class="w-full px-3 py-2 text-sm bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg focus:outline-hidden focus:border-indigo-400"
            />
          </label>
        </div>

        <label class="space-y-1.5 block">
          <span class="text-xs font-medium text-zinc-500">{t('mcp.field_transport')}</span>
          <select
            bind:value={formKind}
            class="w-full px-3 py-2 text-sm bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg focus:outline-hidden cursor-pointer"
          >
            <option value="stdio">{t('mcp.transport_stdio')}</option>
            <option value="http">{t('mcp.transport_http')}</option>
          </select>
        </label>

        {#if formKind === 'stdio'}
          <label class="space-y-1.5 block">
            <span class="text-xs font-medium text-zinc-500">{t('mcp.field_command')}</span>
            <input
              bind:value={formCommand}
              placeholder="npx"
              class="w-full px-3 py-2 text-sm font-mono bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg focus:outline-hidden focus:border-indigo-400"
            />
          </label>
          <label class="space-y-1.5 block">
            <span class="text-xs font-medium text-zinc-500">{t('mcp.field_args')}</span>
            <input
              bind:value={formArgs}
              placeholder="-y @modelcontextprotocol/server-filesystem /tmp"
              class="w-full px-3 py-2 text-sm font-mono bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg focus:outline-hidden focus:border-indigo-400"
            />
            <span class="text-xs text-zinc-400">{t('mcp.args_hint')}</span>
          </label>
        {:else}
          <label class="space-y-1.5 block">
            <span class="text-xs font-medium text-zinc-500">{t('mcp.field_url')}</span>
            <input
              bind:value={formUrl}
              placeholder="https://example.com/mcp"
              class="w-full px-3 py-2 text-sm font-mono bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg focus:outline-hidden focus:border-indigo-400"
            />
          </label>
        {/if}

        <label class="space-y-1.5 block">
          <span class="text-xs font-medium text-zinc-500">
            {formKind === 'stdio' ? t('mcp.field_env') : t('mcp.field_headers')}
          </span>
          <textarea
            bind:value={formKeyValues}
            rows="3"
            placeholder="API_KEY=secret"
            class="w-full px-3 py-2 text-sm font-mono bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg focus:outline-hidden focus:border-indigo-400 resize-y"
          ></textarea>
          <span class="text-xs text-zinc-400">{t('mcp.keyvalues_hint')}</span>
        </label>
      </div>

      <div class="flex items-center justify-end gap-2 px-5 py-4 border-t border-zinc-200 dark:border-zinc-800">
        <button
          onclick={closeEditor}
          class="px-3.5 py-2 text-sm rounded-lg border border-zinc-200 dark:border-zinc-700 text-zinc-600 dark:text-zinc-300 transition cursor-pointer"
        >
          {t('common.cancel')}
        </button>
        <button
          onclick={save}
          disabled={saving}
          class="px-3.5 py-2 text-sm rounded-lg bg-indigo-600 hover:bg-indigo-700 text-white font-medium flex items-center gap-2 transition cursor-pointer disabled:opacity-50"
        >
          <Save class="w-4 h-4" />
          {saving ? t('instances.saving') : t('common.save')}
        </button>
      </div>
    </div>
  </div>
{/if}
