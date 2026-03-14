
export interface LoopStatus {
  cycle: number;
  tokensUsed: number;
  uptime: number;
}

export interface GoddardLoop {
  start: () => Promise<void>;
  status: LoopStatus;
}

