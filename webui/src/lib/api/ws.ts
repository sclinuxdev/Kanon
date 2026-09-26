export type WsStatus =
  | 'connecting'
  | 'connected'
  | 'disconnected'
  | 'reconnecting';

export interface RingBufferOptions<T> {
  url: string;
  capacity?: number;
  onMessage?: (item: T) => void;
  onStatusChange?: (status: WsStatus) => void;
}

export class WsRingBuffer<T> {
  private ws: WebSocket | null = null;
  private buffer: T[] = [];
  private capacity: number;
  private url: string;
  private status: WsStatus = 'disconnected';
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 20;
  private reconnectTimer: number | null = null;
  private isDestroyed = false;

  private onMessageCallback?: (item: T) => void;
  private onStatusChangeCallback?: (status: WsStatus) => void;

  constructor(options: RingBufferOptions<T>) {
    this.url = options.url;
    this.capacity = options.capacity ?? 1000;
    this.onMessageCallback = options.onMessage;
    this.onStatusChangeCallback = options.onStatusChange;
  }

  public connect() {
    if (this.isDestroyed) return;
    this.setStatus(this.reconnectAttempts > 0 ? 'reconnecting' : 'connecting');

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const fullUrl = this.url.startsWith('ws')
      ? this.url
      : `${protocol}//${window.location.host}${this.url.startsWith('/') ? '' : '/'}${this.url}`;

    try {
      this.ws = new WebSocket(fullUrl);

      this.ws.onopen = () => {
        this.reconnectAttempts = 0;
        this.setStatus('connected');
      };

      this.ws.onmessage = (event) => {
        try {
          const data: T = JSON.parse(event.data);
          this.push(data);
          this.onMessageCallback?.(data);
        } catch (e) {
          console.warn('Failed to parse WebSocket message', e);
        }
      };

      this.ws.onclose = () => {
        if (!this.isDestroyed) {
          this.setStatus('disconnected');
          this.scheduleReconnect();
        }
      };

      this.ws.onerror = () => {
        if (this.ws) {
          this.ws.close();
        }
      };
    } catch {
      this.scheduleReconnect();
    }
  }

  private setStatus(status: WsStatus) {
    this.status = status;
    this.onStatusChangeCallback?.(status);
  }

  public push(item: T) {
    if (this.buffer.length >= this.capacity) {
      this.buffer.shift();
    }
    this.buffer.push(item);
  }

  public getItems(): T[] {
    return [...this.buffer];
  }

  public clear() {
    this.buffer = [];
  }

  public send(data: unknown) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(typeof data === 'string' ? data : JSON.stringify(data));
    }
  }

  private scheduleReconnect() {
    if (this.isDestroyed || this.reconnectTimer !== null) return;

    // Use exponential backoff capped at 5 seconds; never permanently abandon
    const delay = Math.min(1000 * 1.5 ** Math.min(this.reconnectAttempts, 8), 5000);
    this.reconnectAttempts++;

    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, delay);
  }

  public reconnect() {
    if (this.isDestroyed) return;
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.ws) {
      try {
        this.ws.close();
      } catch {
        // ignore close errors
      }
      this.ws = null;
    }
    this.reconnectAttempts = 0;
    this.connect();
  }

  public destroy() {
    this.isDestroyed = true;
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
    this.setStatus('disconnected');
  }

  public getStatus(): WsStatus {
    return this.status;
  }
}
