export interface WorktreeSetupOptions {
  cwd: string
  branchName: string
  defaultDirName?: string
}

export interface WorktreePlugin {
  name: string
  isApplicable(cwd: string): boolean
  setup(options: WorktreeSetupOptions): string | null
  cleanup(worktreeDir: string, branchName: string): boolean
}
