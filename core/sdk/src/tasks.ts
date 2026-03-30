// Supported normalized priority levels for tasks managed through the SDK.
export const TASK_PRIORITIES = ["urgent", "high", "medium", "low"] as const

// Supported normalized priority levels for tasks managed through the SDK.
export type TaskPriority = (typeof TASK_PRIORITIES)[number]

// Generic task record returned by task plugins after normalization.
export interface Task<TMetadata = Record<string, unknown>> {
  id: string
  title: string
  description?: string
  owner?: string
  repo?: string
  priority?: TaskPriority
  labels?: string[]
  metadata?: TMetadata
  closed: boolean
}

// Input for creating a normalized task through a provider-specific plugin.
export interface CreateTaskInput<
  TMetadata = Record<string, unknown>,
  TExtra = Record<string, unknown>,
> {
  title: string
  description?: string
  owner?: string
  repo?: string
  priority?: TaskPriority
  labels?: string[]
  metadata?: TMetadata
  extra?: TExtra
}

// Input for partially updating a normalized task through a provider-specific plugin.
export interface UpdateTaskInput<
  TMetadata = Record<string, unknown>,
  TExtra = Record<string, unknown>,
> {
  id: string
  title?: string
  description?: string
  owner?: string
  repo?: string
  priority?: TaskPriority
  labels?: string[]
  metadata?: TMetadata
  closed?: boolean
  extra?: TExtra
}

// Input for closing an existing normalized task through a provider-specific plugin.
export interface CloseTaskInput<TExtra = Record<string, unknown>> {
  id: string
  extra?: TExtra
}

// Contract for integrating an external task system with Goddard's normalized task model.
//
// Raw provider payloads such as webhook events should be translated into the
// normalized inputs above before invoking these methods.
export interface TaskPlugin<
  TMetadata = Record<string, unknown>,
  TCreateExtra = Record<string, unknown>,
  TUpdateExtra = Record<string, unknown>,
  TCloseExtra = Record<string, unknown>,
> {
  readonly name: string
  addTask(input: CreateTaskInput<TMetadata, TCreateExtra>): Promise<Task<TMetadata>>
  updateTask(input: UpdateTaskInput<TMetadata, TUpdateExtra>): Promise<Task<TMetadata>>
  closeTask(input: CloseTaskInput<TCloseExtra>): Promise<Task<TMetadata>>
}
