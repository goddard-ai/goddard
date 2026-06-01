const defaultFeatureNames = [
  "action",
  "adapter",
  "auth",
  "session",
  "inbox",
  "pullRequest",
  "reviewSession",
  "loop",
  "workforce",
] as const

type DefaultFeatureName = (typeof defaultFeatureNames)[number]

type DefaultFeatureContributions<TContributions extends Record<DefaultFeatureName, unknown>> =
  readonly [
    TContributions["action"],
    TContributions["adapter"],
    TContributions["auth"],
    TContributions["session"],
    TContributions["inbox"],
    TContributions["pullRequest"],
    TContributions["reviewSession"],
    TContributions["loop"],
    TContributions["workforce"],
  ]

/** Selects default feature contributions using the shared product feature order. */
export function selectDefaultFeatureContributions<
  const TContributions extends Record<DefaultFeatureName, unknown>,
>(contributions: TContributions): DefaultFeatureContributions<TContributions> {
  return defaultFeatureNames.map(
    (name) => contributions[name],
  ) as unknown as DefaultFeatureContributions<TContributions>
}
