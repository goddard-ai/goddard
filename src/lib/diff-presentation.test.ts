import { expect, test } from 'bun:test';
import { diffStats, parseUnifiedDiff } from './diff-presentation';

test('review rows retain desktop old/new numbering and three lines of context', () => {
  const diff = 'diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -10,8 +10,8 @@\n a\n b\n c\n d\n-before\n+after\n e\n f\n g';
  const result = parseUnifiedDiff(diff, { contextLines: 3, hidePositionedHunks: true });
  expect(result.lines.find((line) => line.kind === 'remove')).toMatchObject({ text: 'before', oldLine: 14, newLine: null });
  expect(result.lines.find((line) => line.kind === 'add')).toMatchObject({ text: 'after', oldLine: null, newLine: 14 });
  expect(diffStats(result.lines)).toEqual({ additions: 1, deletions: 1 });
  expect(result.lines.some((line) => line.kind === 'hunk')).toBe(false);
  expect(result.lines[0]?.kind).toBe('gap');
});

test('large diff payloads render a bounded row set with an honest truncation flag', () => {
  const result = parseUnifiedDiff('@@ -1 +1,1000 @@\n' + Array.from({ length: 1000 }, (_, i) => `+line ${i}`).join('\n'));
  expect(result.lines.length).toBe(400);
  expect(result.truncated).toBe(true);
});
