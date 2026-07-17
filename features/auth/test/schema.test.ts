import { expect, test } from "bun:test"

import { AuthSession, DeviceFlowComplete, DeviceFlowStart } from "../src/schema.ts"

test("auth session contract is provider-neutral", () => {
  expect(
    AuthSession.safeParse({
      token: "tok_1",
      principal: {
        id: "principal_1",
        providerIdentities: [
          {
            provider: "github",
            subject: "42",
            displayName: "alec",
          },
        ],
      },
    }).success,
  ).toBe(true)

  expect(
    AuthSession.safeParse({
      token: "tok_1",
      githubUsername: "alec",
      githubUserId: 42,
    }).success,
  ).toBe(false)
})

test("device flow contract uses provider-neutral identity fields", () => {
  expect(
    DeviceFlowStart.safeParse({
      provider: "github",
      loginHint: "alec",
    }).success,
  ).toBe(true)
  expect(
    DeviceFlowComplete.safeParse({
      deviceCode: "dev_1",
      providerIdentity: {
        provider: "github",
        subject: "42",
        displayName: "alec",
      },
    }).success,
  ).toBe(true)
})
