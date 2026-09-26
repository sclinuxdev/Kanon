<script lang="ts">
import {
  AlertTriangle,
  BookOpen,
  Check,
  FolderOpen,
  Power,
  RefreshCw,
  Trash2,
  Upload,
  X,
} from 'lucide-svelte';
import { api } from '../../api/client';
import { t } from '../../stores/i18n.svelte';
import type { SkillItem } from '../../types';

/**
 * Skill console.
 *
 * Only a skill's name and description reach the model's system prompt; the body is fetched on
 * demand through the `read_skill` tool. Installing therefore costs context only when the model
 * actually decides a skill is relevant.
 */
let skills = $state<SkillItem[]>([]);
let loading = $state(true);
let error = $state<string | null>(null);
let notice = $state<string | null>(null);

let installOpen = $state(false);
let installKind = $state<'archive' | 'path'>('archive');
let archiveFile = $state<File | null>(null);
let pathInput = $state('');
let idInput = $state('');
let installing = $state(false);
let installError = $state<string | null>(null);

async function loadData() {
  loading = true;
  error = null;
  try {
    const res = await api.getSkills();
    skills = res.skills;
  } catch (e) {
    error = e instanceof Error ? e.message : String(e);
  } finally {
    loading = false;
  }
}

function openInstall() {
  installKind = 'archive';
  archiveFile = null;
  pathInput = '';
  idInput = '';
  installError = null;
  installOpen = true;
}

function handleArchiveChange(event: Event) {
  const target = event.target as HTMLInputElement;
  archiveFile =
    target.files && target.files.length > 0 ? target.files[0] : null;
}

async function install() {
  installing = true;
  installError = null;
  try {
    const id = idInput.trim() || undefined;
    const installed =
      installKind === 'archive'
        ? await (() => {
            if (!archiveFile) throw new Error('Select a .zip archive first');
            return api.installSkillArchive(archiveFile, id);
          })()
        : await (() => {
            if (!pathInput.trim())
              throw new Error('Enter a directory containing SKILL.md');
            return api.installSkillPath(pathInput.trim(), id);
          })();

    notice = `Skill '${installed.id}' installed`;
    installOpen = false;
    await loadData();
  } catch (e) {
    installError = e instanceof Error ? e.message : String(e);
  } finally {
    installing = false;
  }
}

async function toggle(skill: SkillItem) {
  try {
    const res = await api.setSkillEnabled(skill.id, !skill.enabled);
    notice = res.message;
    await loadData();
  } catch (e) {
    error = e instanceof Error ? e.message : String(e);
  }
}

async function remove(skill: SkillItem) {
  if (
    !confirm(
      `Remove skill '${skill.id}'? It disappears from every instance's catalog.`,
    )
  )
    return;
  try {
    const res = await api.removeSkill(skill.id);
    notice = res.message;
    await loadData();
  } catch (e) {
    error = e instanceof Error ? e.message : String(e);
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
        {t('skills.title')}
      </h3>
      <p class="text-xs sm:text-sm text-zinc-500">{t('skills.subtitle')}</p>
    </div>
    <div class="flex items-center gap-2">
      <button
        onclick={openInstall}
        class="px-3 py-1.5 text-xs sm:text-sm font-medium rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white transition cursor-pointer flex items-center gap-1.5 shadow-2xs"
      >
        <Upload class="w-4 h-4" />
        <span>{t('skills.install')}</span>
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
  {:else if skills.length === 0}
    <div
      class="p-8 rounded-xl border border-dashed border-zinc-300 dark:border-zinc-800 text-center text-zinc-400 text-sm space-y-2"
    >
      <BookOpen class="w-6 h-6 mx-auto stroke-[1.5]" />
      <p>{t('skills.empty')}</p>
      <p class="text-xs">{t('skills.empty_hint')}</p>
    </div>
  {:else}
    <div class="grid grid-cols-1 gap-4">
      {#each skills as skill (skill.id)}
        <div
          class="p-5 rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-2xs space-y-3"
        >
          <div class="flex flex-wrap items-start justify-between gap-3">
            <div class="flex items-start gap-3">
              <div class="p-2.5 rounded-xl bg-violet-500/10 text-violet-600 dark:text-violet-400">
                <BookOpen class="w-5 h-5" />
              </div>
              <div>
                <div class="flex items-center gap-2 flex-wrap">
                  <span class="font-semibold text-zinc-900 dark:text-zinc-100">{skill.name}</span>
                  <code class="text-xs font-mono text-zinc-400">{skill.id}</code>
                  <span
                    class="px-2 py-0.5 rounded-md text-xs font-medium border
                      {skill.enabled
                      ? 'border-emerald-200 dark:border-emerald-800/60 text-emerald-600 dark:text-emerald-400 bg-emerald-50 dark:bg-emerald-950/60'
                      : 'border-zinc-200 dark:border-zinc-700 text-zinc-500'}"
                  >
                    {skill.enabled ? t('plugins.enable') : t('plugins.disabled')}
                  </span>
                </div>
                <p class="text-xs text-zinc-500 mt-1.5 max-w-3xl">{skill.description}</p>
              </div>
            </div>

            <div class="flex items-center gap-2">
              <button
                onclick={() => toggle(skill)}
                class="px-3 py-1.5 rounded-lg text-xs font-medium border transition cursor-pointer
                  {skill.enabled
                  ? 'border-zinc-200 dark:border-zinc-700 text-zinc-600 dark:text-zinc-300 hover:border-zinc-400'
                  : 'border-emerald-300 dark:border-emerald-800 text-emerald-600 dark:text-emerald-400 hover:bg-emerald-50 dark:hover:bg-emerald-950/40'}"
              >
                <span class="flex items-center gap-1.5">
                  <Power class="w-3.5 h-3.5" />
                  {skill.enabled ? t('plugins.disable') : t('plugins.enable')}
                </span>
              </button>
              <button
                onclick={() => remove(skill)}
                class="p-1.5 rounded-lg text-zinc-500 hover:text-rose-600 hover:bg-rose-50 dark:hover:bg-rose-950/30 transition cursor-pointer"
                title={t('skills.remove')}
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

{#if installOpen}
  <div class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/40 backdrop-blur-sm">
    <div
      class="bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-2xl shadow-xl w-full max-w-xl max-h-[90vh] overflow-y-auto"
    >
      <div class="flex items-center justify-between px-5 py-4 border-b border-zinc-200 dark:border-zinc-800">
        <h3 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">
          {t('skills.install_title')}
        </h3>
        <button
          onclick={() => (installOpen = false)}
          class="p-1.5 rounded-lg text-zinc-400 hover:text-zinc-700 dark:hover:text-zinc-200 transition cursor-pointer"
        >
          <X class="w-4.5 h-4.5" />
        </button>
      </div>

      <div class="p-5 space-y-4">
        {#if installError}
          <p class="text-sm text-rose-600 dark:text-rose-400 flex items-center gap-2">
            <AlertTriangle class="w-4 h-4" /> {installError}
          </p>
        {/if}

        <div class="flex items-center gap-2">
          <button
            onclick={() => (installKind = 'archive')}
            class="px-3 py-1.5 text-xs font-medium rounded-lg border transition cursor-pointer
              {installKind === 'archive'
              ? 'border-indigo-400 text-indigo-600 dark:text-indigo-400 bg-indigo-50 dark:bg-indigo-950/40'
              : 'border-zinc-200 dark:border-zinc-700 text-zinc-600 dark:text-zinc-300'}"
          >
            <span class="flex items-center gap-1.5"><Upload class="w-3.5 h-3.5" /> {t('plugins.tab_upload_archive')}</span>
          </button>
          <button
            onclick={() => (installKind = 'path')}
            class="px-3 py-1.5 text-xs font-medium rounded-lg border transition cursor-pointer
              {installKind === 'path'
              ? 'border-indigo-400 text-indigo-600 dark:text-indigo-400 bg-indigo-50 dark:bg-indigo-950/40'
              : 'border-zinc-200 dark:border-zinc-700 text-zinc-600 dark:text-zinc-300'}"
          >
            <span class="flex items-center gap-1.5"><FolderOpen class="w-3.5 h-3.5" /> {t('plugins.tab_local_path')}</span>
          </button>
        </div>

        {#if installKind === 'archive'}
          <label class="space-y-1.5 block">
            <span class="text-xs font-medium text-zinc-500">{t('skills.archive')}</span>
            <input
              type="file"
              accept=".zip,application/zip"
              onchange={handleArchiveChange}
              class="w-full text-xs text-zinc-600 dark:text-zinc-300 file:mr-3 file:px-3 file:py-1.5 file:rounded-lg file:border-0 file:text-xs file:font-medium file:bg-indigo-600 file:text-white hover:file:bg-indigo-500 file:cursor-pointer"
            />
            <span class="text-xs text-zinc-400">{t('skills.archive_hint')}</span>
          </label>
        {:else}
          <label class="space-y-1.5 block">
            <span class="text-xs font-medium text-zinc-500">{t('skills.path')}</span>
            <input
              bind:value={pathInput}
              placeholder="./my-skill"
              class="w-full px-3 py-2 text-sm font-mono bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg focus:outline-hidden focus:border-indigo-400"
            />
            <span class="text-xs text-zinc-400">{t('skills.path_hint')}</span>
          </label>
        {/if}

        <label class="space-y-1.5 block">
          <span class="text-xs font-medium text-zinc-500">{t('skills.field_id')}</span>
          <input
            bind:value={idInput}
            placeholder="my-skill"
            class="w-full px-3 py-2 text-sm font-mono bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg focus:outline-hidden focus:border-indigo-400"
          />
          <span class="text-xs text-zinc-400">{t('skills.id_hint')}</span>
        </label>
      </div>

      <div class="flex items-center justify-end gap-2 px-5 py-4 border-t border-zinc-200 dark:border-zinc-800">
        <button
          onclick={() => (installOpen = false)}
          class="px-3.5 py-2 text-sm rounded-lg border border-zinc-200 dark:border-zinc-700 text-zinc-600 dark:text-zinc-300 transition cursor-pointer"
        >
          {t('common.cancel')}
        </button>
        <button
          onclick={install}
          disabled={installing}
          class="px-3.5 py-2 text-sm rounded-lg bg-indigo-600 hover:bg-indigo-700 text-white font-medium flex items-center gap-2 transition cursor-pointer disabled:opacity-50"
        >
          <Upload class="w-4 h-4" />
          {installing ? t('common.loading') : t('skills.install')}
        </button>
      </div>
    </div>
  </div>
{/if}
