# Creative Weaver POC

This playground proposes the smallest useful system for layered creative generation with intentional entropy.

It supports two workflows:

- A model-free payload mode that produces the intermediate artifacts a host would send to models.
- A daemon-backed pipeline definition under `.goddard/pipelines/creative-weaver/` that runs Architect -> Chaos Weaver -> Artisan -> Editor as inspectable pipeline steps.

The payload proof of concept does not call an LLM. It produces:

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

To create an inspectable pipeline run through the Goddard daemon, run it from the playground root or pass `--cwd playground/creative-weaver`:

```sh
./playground/creative-weaver/bin/creative-weaver-poc \
  --mode spawn \
  --cwd playground/creative-weaver \
  --premise "A lighthouse keeper ends a long friendship before dawn" \
  --emotion grief \
  --seed 144
```

The spawn command returns a pipeline run id. Use that id to inspect step state and outputs:

```sh
./playground/creative-weaver/bin/creative-weaver-poc \
  --mode inspect \
  --run-id plr_...
```

Use `--advance` with `--mode spawn` when a daemon agent service is available and the run should immediately execute until it waits, fails, or completes. Runs are also visible in the app Pipelines view when the app is pointed at `playground/creative-weaver`.

Useful options:

- `--mode`: `payload`, `spawn`, or `inspect`. Defaults to `payload`.
- `--premise`: one sentence describing the next scene.
- `--emotion`: scene emotion. Supported values are `grief`, `dread`, `awe`, `tension`, `calm`, and `obsession`.
- `--seed`: deterministic seed for repeatable chaos sampling.
- `--words`: target word count for the Artisan model. Defaults to `500`.
- `--cwd`: project root containing `.goddard/pipelines`. Defaults to the current working directory.
- `--run-id`: pipeline run id for `inspect` mode.
- `--advance`: advance a spawned pipeline run immediately.
- `--daemon-url`: optional daemon URL override for SDK-backed modes.
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

The checked-in pipeline keeps that same deterministic Layer 2 as the `creative-weaver.build-payload` script transformer. The Architect, Artisan, and Editor are hidden pipeline agent steps, so their sessions stay out of the normal app session list while the pipeline run tab shows progress, inputs, outputs, and linked session ids.

Later extensions can replace the local snippet pool with vector search and replace the deterministic ledger fallback with richer Architect output parsing.
