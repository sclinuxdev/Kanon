import { api } from '../api/client';
import type {
  ProvidersCatalog,
  SystemConfig,
  TestProviderRequest,
  TestProviderResponse,
} from '../types';

class ProvidersStore {
  catalog = $state<ProvidersCatalog | null>(null);
  systemConfig = $state<SystemConfig | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);

  isTesting = $state(false);
  testResult = $state<TestProviderResponse | null>(null);
  testError = $state<string | null>(null);

  constructor() {
    this.load();
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

  async runTest(req?: TestProviderRequest) {
    this.isTesting = true;
    this.testResult = null;
    this.testError = null;
    try {
      const res = await api.testProvider(req);
      this.testResult = res;
      if (res.status === 'error') {
        this.testError = res.error ?? 'Unknown error';
      }
      return res;
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      this.testError = msg;
      this.testResult = {
        status: 'error',
        latency_ms: 0,
        model: req?.model ?? 'unknown',
        reply: null,
        error: msg,
      };
      return this.testResult;
    } finally {
      this.isTesting = false;
    }
  }
}

export const providersStore = new ProvidersStore();
