import { CString, dlopen, FFIType, ptr, read, suffix, type Pointer } from "bun:ffi"

import { GitHostError, GitNotRepositoryError } from "../errors.ts"
import { nativeLibgit2PathCandidates } from "./native-artifact.ts"

export type Libgit2Symbols = ReturnType<typeof loadLibgit2>["symbols"]
export type FfiPointer = Pointer

let initializedLibgit2: Libgit2Symbols | undefined

export function resetLibgit2ForTests() {
  initializedLibgit2 = undefined
}

export function ensureLibgit2(candidates?: string[]) {
  if (!candidates && initializedLibgit2) {
    return initializedLibgit2
  }

  const libgit2 = loadLibgit2(candidates)
  const initStatus = libgit2.symbols.git_libgit2_init()
  if (initStatus < 0) {
    throw new GitHostError(`git_libgit2_init failed with status ${initStatus}`)
  }

  if (candidates) {
    return libgit2.symbols
  }

  initializedLibgit2 = libgit2.symbols
  return initializedLibgit2
}

function loadLibgit2(candidates = libgit2PathCandidates()) {
  const errors: string[] = []
  for (const candidate of candidates) {
    try {
      return dlopen(candidate, {
        git_libgit2_init: {
          args: [],
          returns: FFIType.i32,
        },
        git_repository_open_ext: {
          args: [FFIType.ptr, FFIType.cstring, FFIType.u32, FFIType.ptr],
          returns: FFIType.i32,
        },
        git_repository_free: {
          args: [FFIType.ptr],
          returns: FFIType.void,
        },
        git_repository_path: {
          args: [FFIType.ptr],
          returns: FFIType.cstring,
        },
        git_repository_workdir: {
          args: [FFIType.ptr],
          returns: FFIType.cstring,
        },
        git_repository_commondir: {
          args: [FFIType.ptr],
          returns: FFIType.cstring,
        },
        git_repository_is_bare: {
          args: [FFIType.ptr],
          returns: FFIType.i32,
        },
        git_repository_head: {
          args: [FFIType.ptr, FFIType.ptr],
          returns: FFIType.i32,
        },
        git_reference_name: {
          args: [FFIType.ptr],
          returns: FFIType.cstring,
        },
        git_reference_name_to_id: {
          args: [FFIType.ptr, FFIType.ptr, FFIType.cstring],
          returns: FFIType.i32,
        },
        git_reference_create: {
          args: [FFIType.ptr, FFIType.ptr, FFIType.cstring, FFIType.ptr, FFIType.i32, FFIType.ptr],
          returns: FFIType.i32,
        },
        git_reference_lookup: {
          args: [FFIType.ptr, FFIType.ptr, FFIType.cstring],
          returns: FFIType.i32,
        },
        git_reference_delete: {
          args: [FFIType.ptr],
          returns: FFIType.i32,
        },
        git_reference_free: {
          args: [FFIType.ptr],
          returns: FFIType.void,
        },
        git_revparse_single: {
          args: [FFIType.ptr, FFIType.ptr, FFIType.cstring],
          returns: FFIType.i32,
        },
        git_object_id: {
          args: [FFIType.ptr],
          returns: FFIType.ptr,
        },
        git_object_free: {
          args: [FFIType.ptr],
          returns: FFIType.void,
        },
        git_oid_tostr_s: {
          args: [FFIType.ptr],
          returns: FFIType.cstring,
        },
        git_oid_fromstr: {
          args: [FFIType.ptr, FFIType.cstring],
          returns: FFIType.i32,
        },
        git_graph_descendant_of: {
          args: [FFIType.ptr, FFIType.ptr, FFIType.ptr],
          returns: FFIType.i32,
        },
        git_merge_base: {
          args: [FFIType.ptr, FFIType.ptr, FFIType.ptr, FFIType.ptr],
          returns: FFIType.i32,
        },
        git_status_list_new: {
          args: [FFIType.ptr, FFIType.ptr, FFIType.ptr],
          returns: FFIType.i32,
        },
        git_status_options_init: {
          args: [FFIType.ptr, FFIType.u32],
          returns: FFIType.i32,
        },
        git_status_list_entrycount: {
          args: [FFIType.ptr],
          returns: FFIType.u64,
        },
        git_status_byindex: {
          args: [FFIType.ptr, FFIType.u64],
          returns: FFIType.ptr,
        },
        git_status_list_free: {
          args: [FFIType.ptr],
          returns: FFIType.void,
        },
        git_worktree_list: {
          args: [FFIType.ptr, FFIType.ptr],
          returns: FFIType.i32,
        },
        git_worktree_lookup: {
          args: [FFIType.ptr, FFIType.ptr, FFIType.cstring],
          returns: FFIType.i32,
        },
        git_worktree_path: {
          args: [FFIType.ptr],
          returns: FFIType.cstring,
        },
        git_worktree_free: {
          args: [FFIType.ptr],
          returns: FFIType.void,
        },
        git_strarray_dispose: {
          args: [FFIType.ptr],
          returns: FFIType.void,
        },
        git_reflog_read: {
          args: [FFIType.ptr, FFIType.ptr, FFIType.cstring],
          returns: FFIType.i32,
        },
        git_reflog_entrycount: {
          args: [FFIType.ptr],
          returns: FFIType.u64,
        },
        git_reflog_entry_byindex: {
          args: [FFIType.ptr, FFIType.u64],
          returns: FFIType.ptr,
        },
        git_reflog_entry_message: {
          args: [FFIType.ptr],
          returns: FFIType.cstring,
        },
        git_reflog_free: {
          args: [FFIType.ptr],
          returns: FFIType.void,
        },
      })
    } catch (error) {
      errors.push(`${candidate}: ${error instanceof Error ? error.message : String(error)}`)
    }
  }

  throw new GitHostError(`Unable to load libgit2. Tried: ${errors.join("; ")}`)
}

function libgit2PathCandidates() {
  return [
    process.env.GODDARD_GIT_LIBGIT2_PATH,
    process.env.LIBGIT2_PATH,
    ...nativeLibgit2PathCandidates(),
    `libgit2.${suffix}`,
    `/opt/homebrew/lib/libgit2.${suffix}`,
    `/usr/local/lib/libgit2.${suffix}`,
  ].filter((path) => typeof path === "string")
}

export async function createLibgit2RefResolver(
  libgit2: Libgit2Symbols,
  cwd: string,
  refName: string,
) {
  return await withLibgit2Repository(libgit2, cwd, (repo) => {
    const object = resolveLibgit2Object(libgit2, repo, refName)
    if (!object) {
      return null
    }

    libgit2.git_object_free(object)
    return refName
  })
}

export function resolveLibgit2Object(libgit2: Libgit2Symbols, repo: FfiPointer, refName: string) {
  const out = pointerStorage()
  const status = libgit2.git_revparse_single(ptr(out), repo, ptr(cString(refName)))
  return status === 0 ? readPointer(out) : null
}

export async function withLibgit2Repository<T>(
  libgit2: Libgit2Symbols,
  cwd: string,
  operation: (repo: FfiPointer) => T | Promise<T>,
) {
  const out = pointerStorage()
  const status = libgit2.git_repository_open_ext(ptr(out), ptr(cString(cwd)), 0, 0)
  if (status !== 0) {
    throw new GitNotRepositoryError(cwd)
  }

  const repo = readPointer(out)
  try {
    return await operation(repo)
  } finally {
    libgit2.git_repository_free(repo)
  }
}

export function pointerStorage() {
  return new BigUint64Array(1)
}

export function readPointer(storage: BigUint64Array) {
  return Number(storage[0]) as FfiPointer
}

export function cString(value: string) {
  return Buffer.from(`${value}\0`)
}

export function toFfiPointer(value: Uint8Array | BigUint64Array | Buffer) {
  return ptr(value)
}

export function readUint32(pointer: FfiPointer, byteOffset = 0) {
  return read.u32(pointer, byteOffset)
}

export function readUint64(pointer: FfiPointer, byteOffset = 0) {
  return read.u64(pointer, byteOffset)
}

export function readNestedPointer(pointer: FfiPointer, byteOffset = 0) {
  return read.ptr(pointer, byteOffset) as FfiPointer
}

export function readCString(pointer: FfiPointer) {
  return String(new CString(pointer))
}
