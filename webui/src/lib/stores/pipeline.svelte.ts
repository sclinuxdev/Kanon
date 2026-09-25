import { WsRingBuffer, type WsStatus } from '../api/ws';
import type { TraceRecord } from '../types';

class PipelineStore {
  records = $state<TraceRecord[]>([]);
  status = $state<WsStatus>('disconnected');
  selectedStage = $state<string>('ALL');
  searchQuery = $state<string>('');

  private wsBuffer: WsRingBuffer<TraceRecord>;

  constructor() {
    this.wsBuffer = new WsRingBuffer<TraceRecord>({
      url: '/ws/v1/events',
      capacity: 1000,
      onMessage: () => {
        this.records = this.wsBuffer.getItems();
      },
      onStatusChange: (status) => {
        this.status = status;
      },
    });

    if (typeof window !== 'undefined') {
      this.wsBuffer.connect();
    }
  }

  get stats() {
    const counts: Record<string, number> = {
      ingested: 0,
      pre_filter: 0,
      command: 0,
      llm: 0,
      tool: 0,
      outbound: 0,
      breaker: 0,
    };

    for (const rec of this.records) {
      const stage = rec.event.stage;
      if (stage === 'ingested') counts.ingested++;
      else if (stage.startsWith('pre_filter')) counts.pre_filter++;
      else if (stage.startsWith('command')) counts.command++;
      else if (stage.startsWith('llm')) counts.llm++;
      else if (stage.startsWith('tool')) counts.tool++;
      else if (stage.startsWith('outbound')) counts.outbound++;
      else if (stage === 'circuit_breaker_tripped') counts.breaker++;
    }

    return counts;
  }

  get filteredRecords(): TraceRecord[] {
    return this.records.filter((rec) => {
      const stage = rec.event.stage;
      if (this.selectedStage !== 'ALL') {
        if (
          this.selectedStage === 'pre_filter' &&
          !stage.startsWith('pre_filter')
        )
          return false;
        if (this.selectedStage === 'command' && !stage.startsWith('command'))
          return false;
        if (this.selectedStage === 'llm' && !stage.startsWith('llm'))
          return false;
        if (this.selectedStage === 'tool' && !stage.startsWith('tool'))
          return false;
        if (this.selectedStage === 'outbound' && !stage.startsWith('outbound'))
          return false;
        if (
          this.selectedStage === 'breaker' &&
          stage !== 'circuit_breaker_tripped'
        )
          return false;
        if (this.selectedStage === 'ingested' && stage !== 'ingested')
          return false;
      }

      if (this.searchQuery.trim()) {
        const q = this.searchQuery.toLowerCase();
        const json = JSON.stringify(rec.event).toLowerCase();
        return json.includes(q);
      }

      return true;
    });
  }

  clear() {
    this.wsBuffer.clear();
    this.records = [];
  }

  destroy() {
    this.wsBuffer.destroy();
  }
}

export const pipelineStore = new PipelineStore();
