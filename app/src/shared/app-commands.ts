import type { AppCommand, AppCommandFunction } from "~/commands/app-command.ts"

export type AppCommandId = AppCommand extends AppCommandFunction<infer Id> ? Id : never
