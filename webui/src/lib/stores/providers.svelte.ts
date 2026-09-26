import { api } from '../api/client';
import type {
  CustomProvider,
  ProviderPreset,
  ProvidersCatalog,
  SystemConfig,
  TestProviderRequest,
  TestProviderResponse,
} from '../types';

const STORAGE_KEY_PROVIDERS = 'kanon_custom_providers_v3';
const STORAGE_KEY_ACTIVE_MODEL = 'kanon_active_model_v3';

class ProvidersStore {
  catalog = $state<ProvidersCatalog | null>(null);
  systemConfig = $state<SystemConfig | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);

  // Two-tier providers & models: Empty by default per user rule
  providers = $state<CustomProvider[]>([]);
  selectedProviderId = $state<string>('');
  activeModel = $state<string>('');

  // Per-model test results and state
  testingModelKey = $state<string | null>(null);
  modelTestResults = $state<Record<string, TestProviderResponse>>({});

  // Remote models fetching state
  isFetchingModels = $state(false);
  fetchModelsError = $state<string | null>(null);
  fetchedModelCandidates = $state<string[]>([]);

  constructor() {
    this.loadStorage();
    this.load();
  }

  private loadStorage() {
    if (typeof window === 'undefined') return;
    try {
      const savedProviders = localStorage.getItem(STORAGE_KEY_PROVIDERS);
      if (savedProviders) {
        const parsed = JSON.parse(savedProviders);
        if (Array.isArray(parsed)) {
          this.providers = parsed;
        }
      }

      if (this.providers.length > 0) {
        this.selectedProviderId = this.providers[0].id;
      }

      const savedActive = localStorage.getItem(STORAGE_KEY_ACTIVE_MODEL);
      if (savedActive) {
        this.activeModel = savedActive;
      } else if (this.providers[0]?.name && this.providers[0]?.models[0]?.id) {
        this.activeModel = `${this.providers[0].name}/${this.providers[0].models[0].id}`;
      }
    } catch (e) {
      console.warn('Failed to load saved providers from localStorage', e);
      this.providers = [];
      this.selectedProviderId = '';
      this.activeModel = '';
    }
  }

  private persistStorage() {
    if (typeof window === 'undefined') return;
    try {
      localStorage.setItem(
        STORAGE_KEY_PROVIDERS,
        JSON.stringify(this.providers),
      );
      localStorage.setItem(STORAGE_KEY_ACTIVE_MODEL, this.activeModel);
    } catch (e) {
      console.warn('Failed to save providers to localStorage', e);
    }
  }

  get selectedProvider(): CustomProvider | undefined {
    return (
      this.providers.find((p) => p.id === this.selectedProviderId) ??
      this.providers[0]
    );
  }

  get allModelKeys(): string[] {
    const keys: string[] = [];
    for (const p of this.providers) {
      for (const m of p.models) {
        keys.push(`${p.name}/${m.id}`);
      }
    }
    return keys;
  }

  selectProvider(id: string) {
    this.selectedProviderId = id;
    this.fetchedModelCandidates = [];
    this.fetchModelsError = null;
  }

  addProvider(data: {
    name: string;
    protocol: 'openai' | 'openai_responses' | 'anthropic';
    base_url: string;
    api_key: string;
  }) {
    const cleanName = data.name
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9_-]/g, '_');
    const newProv: CustomProvider = {
      id: `prov_${Date.now()}`,
      name: cleanName || `provider_${Date.now().toString().slice(-4)}`,
      protocol: data.protocol,
      base_url: data.base_url.trim(),
      api_key: data.api_key.trim(),
      models: [],
    };
    this.providers = [...this.providers, newProv];
    this.selectedProviderId = newProv.id;

    this.persistStorage();
    return newProv;
  }

  // One-click quick configuration from backend presets
  applyPreset(preset: ProviderPreset) {
    // Generate a unique name if already exists
    let candidateName = preset.id;
    let counter = 1;
    while (this.providers.some((p) => p.name === candidateName)) {
      counter++;
      candidateName = `${preset.id}_${counter}`;
    }

    return this.addProvider({
      name: candidateName,
      protocol: (preset.protocol === 'anthropic' ? 'anthropic' : 'openai') as
        | 'openai'
        | 'anthropic',
      base_url: preset.base_url,
      api_key: '',
    });
  }

  updateProvider(
    id: string,
    updates: Partial<Omit<CustomProvider, 'id' | 'models'>>,
  ) {
    const oldProv = this.providers.find((p) => p.id === id);
    const oldName = oldProv?.name;

    this.providers = this.providers.map((p) => {
      if (p.id !== id) return p;
      const updatedName = updates.name
        ? updates.name
            .trim()
            .toLowerCase()
            .replace(/[^a-z0-9_-]/g, '_')
        : p.name;
      return {
        ...p,
        ...updates,
        name: updatedName || p.name,
      };
    });

    if (oldName && updates.name && oldName !== updates.name) {
      if (this.activeModel.startsWith(`${oldName}/`)) {
        this.activeModel = this.activeModel.replace(
          `${oldName}/`,
          `${updates.name}/`,
        );
      }
    }

    this.persistStorage();
  }

  deleteProvider(id: string) {
    const prov = this.providers.find((p) => p.id === id);
    if (!prov) return;

    this.providers = this.providers.filter((p) => p.id !== id);
    if (this.selectedProviderId === id) {
      this.selectedProviderId = this.providers[0]?.id ?? '';
    }

    if (this.activeModel.startsWith(`${prov.name}/`)) {
      if (this.providers[0]?.models[0]) {
        this.activeModel = `${this.providers[0].name}/${this.providers[0].models[0].id}`;
      } else {
        this.activeModel = '';
      }
    }

    this.persistStorage();
  }

  addModel(providerId: string, modelId: string, name?: string) {
    const cleanId = modelId.trim();
    if (!cleanId) return;

    this.providers = this.providers.map((p) => {
      if (p.id !== providerId) return p;
      if (p.models.some((m) => m.id === cleanId)) return p;
      return {
        ...p,
        models: [...p.models, { id: cleanId, name: name?.trim() || cleanId }],
      };
    });

    const prov = this.providers.find((p) => p.id === providerId);
    if (prov && !this.activeModel) {
      this.activeModel = `${prov.name}/${cleanId}`;
    }

    this.persistStorage();
  }

  addMultipleModels(providerId: string, modelIds: string[]) {
    this.providers = this.providers.map((p) => {
      if (p.id !== providerId) return p;
      const existing = new Set(p.models.map((m) => m.id));
      const additions = modelIds
        .map((m) => m.trim())
        .filter((m) => m && !existing.has(m))
        .map((m) => ({ id: m, name: m }));
      return {
        ...p,
        models: [...p.models, ...additions],
      };
    });

    const prov = this.providers.find((p) => p.id === providerId);
    if (prov && !this.activeModel && prov.models[0]) {
      this.activeModel = `${prov.name}/${prov.models[0].id}`;
    }

    this.persistStorage();
  }

  deleteModel(providerId: string, modelId: string) {
    const prov = this.providers.find((p) => p.id === providerId);
    this.providers = this.providers.map((p) => {
      if (p.id !== providerId) return p;
      return {
        ...p,
        models: p.models.filter((m) => m.id !== modelId),
      };
    });

    if (prov && this.activeModel === `${prov.name}/${modelId}`) {
      const remaining = prov.models.filter((m) => m.id !== modelId);
      if (remaining[0]) {
        this.activeModel = `${prov.name}/${remaining[0].id}`;
      } else {
        const nextProv = this.providers.find((p) => p.models.length > 0);
        this.activeModel = nextProv
          ? `${nextProv.name}/${nextProv.models[0].id}`
          : '';
      }
    }

    this.persistStorage();
  }

  setActiveModel(fullModelKey: string) {
    this.activeModel = fullModelKey;
    this.persistStorage();
  }

  // Fetch remote models list using backend endpoint (Zero browser CORS issues)
  async fetchRemoteModels(
    providerId: string,
    overrideApiKey?: string,
    overrideBaseUrl?: string,
    overrideProtocol?: 'openai' | 'openai_responses' | 'anthropic',
  ) {
    const prov = this.providers.find((p) => p.id === providerId);
    if (!prov) return [];

    this.isFetchingModels = true;
    this.fetchModelsError = null;
    this.fetchedModelCandidates = [];

    const apiKey = (
      overrideApiKey !== undefined ? overrideApiKey : prov.api_key
    ).trim();
    const baseUrl = (
      overrideBaseUrl !== undefined ? overrideBaseUrl : prov.base_url
    ).trim();
    const protocol =
      overrideProtocol !== undefined ? overrideProtocol : prov.protocol;

    const isLocal =
      baseUrl.includes('127.0.0.1') || baseUrl.includes('localhost');
    if (!apiKey && !isLocal) {
      this.isFetchingModels = false;
      this.fetchModelsError =
        '该在线服务商需要 API Key 才能获取模型列表，请先在上方输入 API 密钥并保存修改。';
      return [];
    }

    try {
      const res = await api.fetchModels({
        protocol,
        base_url: baseUrl,
        api_key: apiKey || undefined,
      });
      this.fetchedModelCandidates = res.models;
      return res.models;
    } catch (e) {
      let msg = e instanceof Error ? e.message : String(e);
      if (
        msg.includes('401') ||
        msg.includes('Unauthorized') ||
        msg.includes('Authentication Fails') ||
        msg.includes('governor')
      ) {
        msg = `鉴权失败 (HTTP 401)：API 密钥无效、未配置或配额不足。请确认 API Key 并重新保存。详细信息: ${msg}`;
      }
      this.fetchModelsError = msg;
      return [];
    } finally {
      this.isFetchingModels = false;
    }
  }

  async testModel(
    providerId: string,
    modelId: string,
    prompt: string = 'ping',
    overrideApiKey?: string,
  ) {
    const prov = this.providers.find((p) => p.id === providerId);
    if (!prov) return;

    const fullKey = `${prov.name}/${modelId}`;
    this.testingModelKey = fullKey;
    const apiKey = (
      overrideApiKey !== undefined ? overrideApiKey : prov.api_key
    ).trim();

    try {
      const res = await api.testProvider({
        protocol: prov.protocol,
        base_url: prov.base_url || undefined,
        api_key: apiKey || undefined,
        model: modelId,
        prompt,
      });
      this.modelTestResults = {
        ...this.modelTestResults,
        [fullKey]: res,
      };
      return res;
    } catch (e) {
      const errRes: TestProviderResponse = {
        status: 'error',
        latency_ms: 0,
        model: modelId,
        reply: null,
        error: e instanceof Error ? e.message : String(e),
      };
      this.modelTestResults = {
        ...this.modelTestResults,
        [fullKey]: errRes,
      };
      return errRes;
    } finally {
      this.testingModelKey = null;
    }
  }

  async load() {
    this.loading = true;
    this.error = null;
    try {
      const [cat, sys] = await Promise.all([
        api.getProviders(),
        api.getSystemConfig(),
      ]);
      this.catalog = cat;
      this.systemConfig = sys;
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    } finally {
      this.loading = false;
    }
  }

  async refresh() {
    return this.load();
  }

  async runTest(req?: TestProviderRequest) {
    return api.testProvider(req);
  }
}

export const providersStore = new ProvidersStore();
