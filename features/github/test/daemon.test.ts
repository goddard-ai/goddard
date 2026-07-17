import { expect, test } from "bun:test"

import { parseGitHubRepositoryUrl } from "../src/daemon.ts"

test("parses GitHub repository remotes", () => {
  expect(parseGitHubRepositoryUrl("https://github.com/acme/widgets.git")).toEqual({
    provider: "github",
    owner: "acme",
    repo: "widgets",
  })
  expect(parseGitHubRepositoryUrl("git@github.com:acme/widgets.git")).toEqual({
    provider: "github",
    owner: "acme",
    repo: "widgets",
  })
})

test("ignores non-GitHub repository remotes", () => {
  expect(parseGitHubRepositoryUrl("https://gitlab.com/acme/widgets.git")).toBeUndefined()
})
