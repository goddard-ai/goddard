import { signal } from "@preact/signals"
import { useEffect } from "preact/hooks"

import { useSelectorUsage } from "~/app-state-context.tsrx"
import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { resolvePreferredLaunchAgentId, resolvePreferredLaunchCwd } from "./launch-preferences.ts"
import { releaseSessionLaunchLease } from "./mutations.ts"

const keptLaunchLeaseId = signal<string | null>(null)

export function isKeptSessionLaunchLease(launchLeaseId: string | null | undefined) {
  return launchLeaseId !== null && launchLeaseId !== undefined &&
    keptLaunchLeaseId.value === launchLeaseId
}

export function SessionLaunchLeaseKeeper(props: {
  currentProjectPath: string | null;
}) @{
  const selectorUsage = useSelectorUsage()
  const subpackages = useQuery(props.currentProjectPath ? goddardSdk.session.subpackages : null, {
    params: props.currentProjectPath ? [{ cwd: props.currentProjectPath }] : null,
  })
  const cwd =
    props.currentProjectPath
      ? resolvePreferredLaunchCwd(selectorUsage, props.currentProjectPath, subpackages.subpackages)
      : null
  const adapterCatalog = useQuery(cwd ? goddardSdk.adapter.list : null, {
    params: cwd ? [{ cwd }] : null,
  })
  const agentId =
    props.currentProjectPath ? resolvePreferredLaunchAgentId(selectorUsage, adapterCatalog) : null
  const launchPreview = useQuery(cwd && agentId ? goddardSdk.session.launchPreview : null, {
    params: cwd && agentId
      ? [
          {
            agent: agentId,
            cwd,
          },
        ]
      : null,
  })

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
