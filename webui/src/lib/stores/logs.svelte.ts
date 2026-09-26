import { WsRingBuffer, type WsStatus } from '../api/ws';
import type { LogLevel, LogRecord } from '../types';

class LogStore {
  records = $state<LogRecord[]>([]);
  status = $state<WsStatus>('disconnected');
  filterLevel = $state<LogLevel | 'ALL'>('ALL');
  searchQuery = $state<string>('');
  autoScroll = $state<boolean>(true);

  private wsBuffer: WsRingBuffer<unknown>;

  constructor() {
    this.wsBuffer = new WsRingBuffer<unknown>({
      url: '/ws/v1/logs',
      capacity: 1000,
      onMessage: (raw: unknown) => {
        let rec: LogRecord | null = null;
        if (raw && typeof raw === 'object') {
          const frame = raw as Record<string, unknown>;
          if (frame.type === 'log' && frame.record) {
            rec = frame.record as LogRecord;
          } else if (frame.level && frame.message) {
            rec = frame as unknown as LogRecord;
          }
        }
        if (rec) {
          const upperLevel = String(rec.level || 'INFO').toUpperCase();
          const normalized: LogRecord = {
            ...rec,
            level: (['INFO', 'WARN', 'ERROR', 'DEBUG'].includes(upperLevel)
              ? upperLevel
              : 'INFO') as LogLevel,
            target: rec.target || 'kanon_core',
            message: rec.message || '',
            timestamp_ms: rec.timestamp_ms || Date.now(),
          };
          this.records = [...this.records.slice(-999), normalized];
        }
      },
      onStatusChange: (status) => {
        this.status = status;
      },
    });

    if (typeof window !== 'undefined') {
      this.wsBuffer.connect();
    }
  }

  reconnect() {
    this.wsBuffer.reconnect();
  }

  get filteredRecords(): LogRecord[] {
    return this.records.filter((rec) => {
      if (!rec?.message) return false;
      const lvl = rec.level ? String(rec.level).toUpperCase() : 'INFO';
      if (this.filterLevel !== 'ALL' && lvl !== this.filterLevel) {
        return false;
      }
      if (this.searchQuery.trim()) {
        const q = this.searchQuery.toLowerCase();
        return (
          rec.message.toLowerCase().includes(q) ||
          rec.target?.toLowerCase().includes(q) ||
          lvl.toLowerCase().includes(q)
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
