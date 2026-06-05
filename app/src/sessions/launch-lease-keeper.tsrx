import { signal } from "@preact/signals"
import { useEffect } from "preact/hooks"

import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { releaseSessionLaunchLease } from "./mutations.ts"

const keptLaunchLeaseId = signal<string | null>(null)
const lastUsedLaunchAgentId = signal<string | null>(null)

export function recordSessionLaunchAgentUse(agentId: string | null | undefined) {
  if (agentId) {
    lastUsedLaunchAgentId.value = agentId
  }
}

export function isKeptSessionLaunchLease(launchLeaseId: string | null | undefined) {
  return launchLeaseId !== null && launchLeaseId !== undefined &&
    keptLaunchLeaseId.value === launchLeaseId
}

function resolveKeptLaunchAgentId(adapterCatalog: Awaited<ReturnType<typeof goddardSdk.adapter.list>>) {
  const availableAdapterIds = new Set(adapterCatalog.adapters.map((adapter) => adapter.id))

  if (lastUsedLaunchAgentId.value && availableAdapterIds.has(lastUsedLaunchAgentId.value)) {
    return lastUsedLaunchAgentId.value
  }

  if (adapterCatalog.defaultAdapterId && availableAdapterIds.has(adapterCatalog.defaultAdapterId)) {
    return adapterCatalog.defaultAdapterId
  }

  return adapterCatalog.adapters[0]?.id ?? null
}

export component SessionLaunchLeaseKeeper(props: {
  currentProjectPath: string | null;
}) {
  const adapterCatalog = useQuery(props.currentProjectPath ? goddardSdk.adapter.list : null, {
    params: props.currentProjectPath ? [{ cwd: props.currentProjectPath }] : null,
  })
  const agentId =
    props.currentProjectPath ? resolveKeptLaunchAgentId(adapterCatalog) : null
  const launchPreview = useQuery(
    props.currentProjectPath && agentId ? goddardSdk.session.launchPreview : null,
    {
      params: props.currentProjectPath && agentId
        ? [
            {
              agent: agentId,
              cwd: props.currentProjectPath,
            },
          ]
        : null,
    },
  )

  useEffect(() => {
    const launchLeaseId = launchPreview?.launchLeaseId ?? null
    keptLaunchLeaseId.value = launchLeaseId

    return () => {
      if (keptLaunchLeaseId.value === launchLeaseId) {
        keptLaunchLeaseId.value = null
      }

      void releaseSessionLaunchLease(launchLeaseId).catch((error) => {
        console.error("Failed to release launch lease.", error)
      })
    }
  }, [launchPreview?.launchLeaseId])

  <></>
}
