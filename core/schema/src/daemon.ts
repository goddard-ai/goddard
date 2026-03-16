export type DaemonHealth = {
  ok: boolean
}

export type SubmitPrDaemonRequest = {
  cwd: string
  title: string
  body: string
  head?: string
  base?: string
}

export type SubmitPrDaemonResponse = {
  number: number
  url: string
}

export type ReplyPrDaemonRequest = {
  cwd: string
  message: string
  prNumber?: number
}

export type ReplyPrDaemonResponse = {
  success: boolean
}
