import type { DaemonConfigWriter, DaemonLogger } from "@goddard-ai/daemon-plugin"
import { getErrorMessage, omit } from "radashi"

import {
  SessionErrorCodes,
  SessionProfilesConfig,
  type RemoveSessionProfileRequest,
  type SetSessionProfileRequest,
} from "../schema.ts"
import { createSessionIpcError } from "./ipc-error.ts"

/** Owns global session-profile reads and serialized configuration mutations. */
export function createSessionProfilesService(input: {
  configWriter: DaemonConfigWriter
  logger: DaemonLogger
}) {
  async function runOperation<TResult>(
    operation: "list" | "remove" | "set",
    callback: () => Promise<TResult>,
  ) {
    try {
      return await callback()
    } catch (error) {
      input.logger.log("session.profile_configuration_failed", {
        operation,
        errorMessage: getErrorMessage(error),
      })
      throw createSessionIpcError(SessionErrorCodes.ProfileConfigurationFailed)
    }
  }

  function readProfiles(config: Readonly<Record<string, unknown>>) {
    return SessionProfilesConfig.parse(config.sessionProfiles ?? {})
  }

  return {
    list() {
      return runOperation("list", async () => {
        const config = await input.configWriter.getGlobalConfig()
        return {
          profiles: SessionProfilesConfig.parse(config.sessionProfiles ?? {}),
        }
      })
    },

    set(request: SetSessionProfileRequest) {
      return runOperation("set", async () => {
        const config = await input.configWriter.updateGlobalConfig((currentConfig) => {
          const profiles = readProfiles(currentConfig)
          return {
            ...currentConfig,
            sessionProfiles: {
              ...profiles,
              [request.agentId]: {
                ...profiles[request.agentId],
                [request.profileId]: request.profile,
              },
            },
          }
        })
        return { profiles: readProfiles(config) }
      })
    },

    remove(request: RemoveSessionProfileRequest) {
      return runOperation("remove", async () => {
        const config = await input.configWriter.updateGlobalConfig((currentConfig) => {
          const profiles = readProfiles(currentConfig)
          const currentAgentProfiles = profiles[request.agentId]
          if (!currentAgentProfiles?.[request.profileId]) {
            return { ...currentConfig }
          }

          const nextAgentProfiles = omit(currentAgentProfiles, [request.profileId])
          const nextProfiles = { ...profiles }
          if (Object.keys(nextAgentProfiles).length === 0) {
            delete nextProfiles[request.agentId]
          } else {
            nextProfiles[request.agentId] = nextAgentProfiles
          }

          return Object.keys(nextProfiles).length === 0
            ? omit(currentConfig, ["sessionProfiles"])
            : { ...currentConfig, sessionProfiles: nextProfiles }
        })
        return { profiles: readProfiles(config) }
      })
    },
  }
}
