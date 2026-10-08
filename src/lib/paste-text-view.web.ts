/**
 * Typecheck stand-in for the web config — `composer-text-input.tsx` (the
 * unsuffixed native-default leaf) imports this module and the web tsconfig
 * still visits that file, though the web runtime resolves
 * `composer-text-input.web.tsx` and never loads it. The index signature
 * keeps `Attributes<typeof PasteAwareTextView>` permissive so the
 * native-only element props typecheck here.
 */
export class PasteAwareTextView {
	[key: string]: any;
}
