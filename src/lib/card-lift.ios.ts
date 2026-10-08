/// <reference types="@nativescript/types" />
import { Color } from '@nativescript/core';

/** The card-lift edge shadow under the content card — a static CALayer
 * shadow paints offscreen until the drawer slides the card, so no
 * per-progress animation is needed. */
export function liftCard(card: { ios?: UIView } | null): void {
	const layer = card?.ios?.layer;
	if (!layer) {return;}
	layer.shadowColor = new Color('#000000').ios.CGColor;
	layer.shadowOpacity = 0.12;
	layer.shadowRadius = 12;
	layer.shadowOffset = CGSizeMake(0, 0);
}
