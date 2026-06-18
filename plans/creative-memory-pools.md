# Creative Memory Pools

## Concept

Creative inputs should enter the system as human-authored content drops and prompts. Agents should help organize those inputs into manageable pools, but pool creation should remain human-authorized so the taxonomy reflects authorial intent instead of automatic folder sprawl.

The core division of labor:

- Humans set inspiration, intent, taste, and boundaries.
- Agents maintain the memory system: classify drops, suggest pool membership, populate requested pools, and keep context organized.
- Writer agents deliberately search for relevant pools when composing a scene instead of receiving every possible input as undifferentiated context.

## Content Drops

A content drop is any raw input a human wants the system to remember or use later:

- prose fragments
- research notes
- images or sensory references
- character ideas
- worldbuilding notes
- style samples
- constraints or taboos
- prompts for a specific scene or project

Drops should remain lightweight and easy to add. The human-facing input should be "drop content and prompt the system," not "maintain a database."

```ts
type ContentDrop = {
  id: string
  text: string
  source: "human" | "agent" | "import"
  tags: string[]
  pools: PoolId[]
  weight: number
  status: "raw" | "curated" | "rejected"
}
```

## Pools

A pool is an editorial memory surface. It is not just a storage bucket; it has a role, attachment policy, and lifecycle.

```ts
type Pool = {
  id: string
  name: string
  description: string
  role: "style" | "theme" | "character" | "world" | "research" | "constraint" | "sensory"
  attachmentPolicy: "manual" | "suggested" | "writer-searchable" | "always"
  status: "draft" | "active" | "frozen" | "archived"
  weight: number
}
```

Pool states:

- `draft`: being assembled; writer agents do not use it by default.
- `active`: writer agents may search and attach it.
- `frozen`: stable contents; no automatic population unless explicitly requested.
- `archived`: retained for history, not used by default.

## Pool Creation

Agents may create pools at the request of a human. They should not create persistent pools silently as a background habit.

Example human requests:

- "Create a pool for nautical decay."
- "Make a recurring lighthouse imagery pool from these notes."
- "Create a style pool from these three passages."
- "Build a taboo pool for things this story should avoid."

The agent should turn the request into pool metadata:

```ts
type PoolCreationRequest = {
  name: string
  description?: string
  role?: Pool["role"]
  populate?: {
    from: "all-drops" | "selected-drops" | "pool"
    query?: string
    threshold?: number
    maxDrops?: number
    requireApproval?: boolean
  }
}
```

If the human asks for automatic population, the agent can use vector search and classification to attach matching drops. The agent should report:

- drops attached
- drops skipped
- ambiguous candidates
- confidence or rationale for notable attachments

For high-impact pools such as continuity, character, style, or taboo pools, `requireApproval` should default to true unless the human explicitly asks for direct attachment.

## Automatic Pool Attachment

Vector search should suggest relevant pools and drops, but it should not blindly inject everything into generation context.

Recommended flow:

1. Human adds content drops or prompts.
2. Librarian agent classifies drops and suggests matching pools.
3. Human requests pool creation or approves suggested attachments.
4. Agent optionally backfills the pool from existing drops.
5. Architect agent creates a scene ledger.
6. Writer agent searches for pools relevant to the scene.
7. Chaos Weaver samples from selected pools with novelty and distance controls.
8. Writer agent produces prose.
9. Editor agent flags contradictions, overused pools, or stale motifs.

The important control point is that the writer agent chooses which pools to search or attach for the current scene. This keeps memory retrieval intentional and prevents context flooding.

## MVP

Start with a small fixed set of pool roles:

- `style`
- `theme`
- `character`
- `world`
- `sensory`
- `research`
- `constraint`

For each generation run, the writer agent should normally attach only two to five pools. If more are needed, the Architect should justify why the scene requires that much context.

The first implementation can use local embedded drops and deterministic scoring. A later implementation can replace the local search with a vector index while preserving the same pool and attachment contracts.

## Guardrails

- Human authorization is required for creating persistent pools.
- Agents may suggest pool creation, but should not silently add new active pools.
- Agents may automatically populate a requested pool when the human asks for it.
- Draft pools should not be writer-searchable by default.
- Frozen pools should not receive automatic attachments by default.
- Human-pinned pools override automatic retrieval.
- Pool merge and prune operations should be cheap, because pool sprawl is the main failure mode.

## Open Questions

- Should pool membership be many-to-many from the start, or should each drop have one primary pool plus secondary suggestions?
- Should automatic population attach drops immediately, or stage candidates by default?
- Should writer agents be allowed to create temporary scene-local pools that disappear after the run?
- How should negative pools work for material that should be avoided rather than used?
- What signals should decay pool weights over time: repeated use, human rejection, story phase, or scene distance?
