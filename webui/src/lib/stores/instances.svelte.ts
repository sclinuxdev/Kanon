import { api } from '../api/client';
import type {
  AdapterItem,
  BotInstanceView,
  InstanceRequest,
  InstancesResponse,
  PersonaItem,
} from '../types';

/**
 * Console state for bot instances.
 *
 * Instances are a node-level catalog: adapters only declare where messages come from, while an
 * instance decides whether a bot answers them at all. This store keeps that catalog, the adapter
 * and persona choices it can be built from, and the form state shared by the create/edit modal.
 */
class InstancesStore {
  catalog = $state<InstancesResponse | null>(null);
  adapters = $state<AdapterItem[]>([]);
  personas = $state<PersonaItem[]>([]);

  loading = $state(false);
  saving = $state(false);
  error = $state<string | null>(null);
  notice = $state<string | null>(null);

  /** Instance currently being edited, or `null` when the form creates a new one. */
  editingId = $state<string | null>(null);
  isFormOpen = $state(false);

  // Form fields
  formName = $state('');
  formEnabled = $state(true);
  formAdapters = $state<string[]>([]);
  formPersonaId = $state('');
  formSystemPrompt = $state('');
  formUseCustomModel = $state(false);
  formModel = $state('');

  get instances(): BotInstanceView[] {
    return this.catalog?.instances ?? [];
  }

  get enabledCount(): number {
    return this.catalog?.enabled ?? 0;
  }

  /** Name of the instance that currently owns an adapter, if any. */
  ownerOf(platform: string): string | null {
    for (const instance of this.instances) {
      if (!instance.enabled || instance.id === this.editingId) continue;
      if (instance.adapters.includes(platform)) return instance.name;
    }
    return null;
  }

  async load() {
    this.loading = true;
    this.error = null;
    try {
      const [catalog, adapters, personas] = await Promise.all([
        api.getInstances(),
        api.getAdapters(),
        api.getPersonas(),
      ]);
      this.catalog = catalog;
      this.adapters = adapters.adapters;
      this.personas = personas.personas;
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    } finally {
      this.loading = false;
    }
  }

  openCreate() {
    this.editingId = null;
    this.formName = '';
    this.formEnabled = true;
    this.formAdapters = [];
    this.formPersonaId = '';
    this.formSystemPrompt = '';
    this.formUseCustomModel = false;
    this.formModel = '';
    this.notice = null;
    this.error = null;
    this.isFormOpen = true;
  }

  openEdit(instance: BotInstanceView) {
    this.editingId = instance.id;
    this.formName = instance.name;
    this.formEnabled = instance.enabled;
    this.formAdapters = [...instance.adapters];
    this.formPersonaId = instance.persona_id ?? '';
    this.formSystemPrompt = instance.system_prompt ?? '';
    this.formUseCustomModel = Boolean(instance.model);
    this.formModel = instance.model ?? '';
    this.notice = null;
    this.error = null;
    this.isFormOpen = true;
  }

  closeForm() {
    this.isFormOpen = false;
  }

  toggleAdapter(platform: string) {
    this.formAdapters = this.formAdapters.includes(platform)
      ? this.formAdapters.filter((p) => p !== platform)
      : [...this.formAdapters, platform];
  }

  private payload(): InstanceRequest {
    return {
      name: this.formName.trim(),
      enabled: this.formEnabled,
      adapters: this.formAdapters,
      persona_id: this.formPersonaId || null,
      system_prompt: this.formSystemPrompt.trim() || null,
      model: this.formUseCustomModel && this.formModel.trim() ? this.formModel.trim() : null,
    };
  }

  async save() {
    this.saving = true;
    this.error = null;
    this.notice = null;
    try {
      const body = this.payload();
      const res = this.editingId
        ? await api.updateInstance(this.editingId, body)
        : await api.createInstance(body);
      this.notice = res.message;
      this.isFormOpen = false;
      await this.load();
      return true;
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
      return false;
    } finally {
      this.saving = false;
    }
  }

  async toggleEnabled(instance: BotInstanceView) {
    this.saving = true;
    this.error = null;
    try {
      const res = await api.updateInstance(instance.id, {
        name: instance.name,
        enabled: !instance.enabled,
        adapters: instance.adapters,
        persona_id: instance.persona_id,
        system_prompt: instance.system_prompt,
        model: instance.model,
      });
      this.notice = res.message;
      await this.load();
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    } finally {
      this.saving = false;
    }
  }

  async remove(id: string) {
    this.saving = true;
    this.error = null;
    try {
      const res = await api.deleteInstance(id);
      this.notice = res.message;
      await this.load();
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    } finally {
      this.saving = false;
    }
  }
}

export const instancesStore = new InstancesStore();
