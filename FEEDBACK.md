# Open implementation questions

Track unresolved client and protocol gaps here. Each entry identifies the owning
follow-up, evidence, and the behavior that must remain intact.

## U8 voice playback: client surface missing

**Status:** Open. **Owner:** U0 daemon-side protocol follow-up. **Decision:** Q7
(voice/speak in v1) remains unspecified.

**Expected:** Implement the desktop speak contract: persisted clip library,
reuse evaluation, TTS synthesis, and briefing-queue playback, with consent outside
the Boss chat.

**Observed:** `waku-client` exposes
[`subscribeBossSpeech`](packages/waku-client/src/client.ts), delivering a live
`BossSpeechRequest` with `requestId` and `parts`. It exposes no clip-library,
reuse-evaluation, TTS, or audio-playback pipeline. Desktop owns those facilities
locally in the Goddard repository's `src/app/speech.rs` and
`src/app/voice_briefing.rs`; a broadcast alone cannot reproduce their behavior.

**Current outcome:** The approved U8 subset landed as
`0b6898b396275b81f8bddee4fafa371d204ea0de`: durable “Spoken aloud” transcript
receipts, browse, and pinned-terminal handling. Voice playback and consent are
not implemented, and no replacement or stub pipeline was added.

**Open question:** Should v1 include playback (Q7)? If so, which typed daemon
operations and capability signals should U0 expose for resolving fragments and
providing playable clips, and which parts should remain client-owned?

Any follow-up must preserve request deduplication, consent before playback
outside the Boss chat, and the durable transcript receipt regardless of playback.
Speech missed while disconnected remains lost; reconnect must not replay it.
