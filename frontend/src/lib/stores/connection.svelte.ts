// ConnectionStore — WS 상태 + Reconnect state machine (D21 c2/c3, R8 §F6).

export type WsState = 'connecting' | 'open' | 'closed' | 'reconnecting';

class ConnectionStore {
  state = $state<WsState>('connecting');
  attempt = $state<number>(0);
}

export const connectionStore = new ConnectionStore();
