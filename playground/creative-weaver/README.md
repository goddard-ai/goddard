# Creative Weaver POC

This playground proposes the smallest useful system for layered creative generation with intentional entropy.

The proof of concept does not call an LLM. It produces the intermediate artifacts a host would send to models:

- **Architect ledger:** a compact scene brief with narrative goal, emotional state, sensory focus, and continuity constraints.
- **Chaos seeds:** one oblique strategy plus sensory anchors sampled from a curated pool with deterministic pseudo-randomness.
- **Artisan payload:** a final prose prompt that combines structure, constraint, and entropy.

Run it from the repository root:

```sh
./playground/creative-weaver/bin/creative-weaver-poc \
  --premise "A lighthouse keeper ends a long friendship before dawn" \
  --emotion grief \
  --seed 144
```

Useful options:

- `--premise`: one sentence describing the next scene.
- `--emotion`: scene emotion. Supported values are `grief`, `dread`, `awe`, `tension`, `calm`, and `obsession`.
- `--seed`: deterministic seed for repeatable chaos sampling.
- `--words`: target word count for the Artisan model. Defaults to `500`.
- `--help`: show CLI usage.

## Minimal System

The system has three replaceable stages:

1. **Architect:** turn project state into a scene ledger, not prose. Keep it low-temperature and structural.
2. **Chaos Weaver:** use deterministic entropy to pick related-but-offset context from curated local pools.
3. **Artisan:** ask a prose model to obey the ledger while metabolizing the injected anchors.

The minimal implementation keeps Layer 2 local and inspectable:

- A seedable random generator makes each run reproducible.
- Emotion chooses the neighborhood of acceptable snippets.
- Candidate anchors are scored by distance from the current scene tone, then sampled from the middle distance rather than the nearest match.
- The entropy dial maps emotion to temperature and top-p recommendations for the downstream Artisan call.

Later extensions can replace the local snippet pool with vector search, replace the heuristic ledger with a live Architect model, and add an Editor pass that checks continuity without flattening the stranger imagery.
