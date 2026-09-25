import { WsRingBuffer, type WsStatus } from '../api/ws';
import type { LogLevel, LogRecord } from '../types';

class LogStore {
  records = $state<LogRecord[]>([]);
  status = $state<WsStatus>('disconnected');
  filterLevel = $state<LogLevel | 'ALL'>('ALL');
  searchQuery = $state<string>('');
  autoScroll = $state<boolean>(true);

  private wsBuffer: WsRingBuffer<LogRecord>;

  constructor() {
    this.wsBuffer = new WsRingBuffer<LogRecord>({
      url: '/ws/v1/logs',
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

  get filteredRecords(): LogRecord[] {
    return this.records.filter((rec) => {
      if (this.filterLevel !== 'ALL' && rec.level !== this.filterLevel) {
        return false;
      }
      if (this.searchQuery.trim()) {
        const q = this.searchQuery.toLowerCase();
        return (
          rec.message.toLowerCase().includes(q) ||
          rec.target.toLowerCase().includes(q) ||
          rec.level.toLowerCase().includes(q)
        );
      }
      return true;
    });
  }

  clear() {
    this.wsBuffer.clear();
    this.records = [];
  }

  setFilter(level: LogLevel | 'ALL') {
    this.filterLevel = level;
    if (this.status === 'connected') {
      this.wsBuffer.send({
        action: 'set_filter',
        level: level === 'ALL' ? 'debug' : level.toLowerCase(),
      });
    }
  }

  destroy() {
    this.wsBuffer.destroy();
  }
}

export const logStore = new LogStore();
