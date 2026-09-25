import { api } from '../api/client';
import type { NodeHealth } from '../types';

class NodeStore {
  health = $state<NodeHealth | null>(null);
  loading = $state(true);
  error = $state<string | null>(null);
  lastUpdated = $state<Date | null>(null);
  private timer: number | null = null;

  constructor() {
    this.refresh();
    if (typeof window !== 'undefined') {
      this.timer = window.setInterval(() => this.refresh(), 5000);
    }
  }

  async refresh() {
    try {
      this.health = await api.getHealth();
      this.error = null;
      this.lastUpdated = new Date();
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    } finally {
      this.loading = false;
    }
  }

  destroy() {
    if (this.timer !== null) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }
}

export const nodeStore = new NodeStore();
