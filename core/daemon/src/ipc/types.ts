export type DaemonServer = {
  daemonUrl: string
  port: number
  close: () => Promise<void>
}
