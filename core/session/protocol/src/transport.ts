export type SessionEndpoint =
  | { kind: "tcp"; port: number; url: string }
  | { kind: "ipc"; socketPath: string; url: string }
