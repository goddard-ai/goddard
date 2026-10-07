//! The in-app wireframe renderer.
//!
//! A `.wireframe.json` screen's `frame`/`rect`/`text`/`divider` tree maps
//! onto GPUI flex elements — HTML/CSS semantics carried by the element
//! tree, so `gap`, `padding`, `align`, `justify`, and the
//! hug/fill/fixed sizes land on the same layout engine real UI uses.
//! Every painted value resolves through the active [`Theme`]: a preview
//! follows the selected scheme, light or dark, like real UI would.
//! Node `annotations` become tooltips, matching the "wireframes carry
//! intent" contract.

use gpui::TextAlign;
use waku_protocol::wireframe::{
    WireAlign, WireDirection, WireFrame, WireJustify, WireNode, WirePadding, WireRect, WireScreen,
    WireSize, WireText, WireTextAlign, WireTextRole, WireTextTone, WireTone,
};

use super::*;

/// One screen at its declared size on the theme's canvas — a fixed-size
/// proposal, not a responsive layout. The root node always spans the
/// whole screen; its own sizing is ignored per the schema.
#[track_caller]
pub(super) fn wireframe_screen_element(screen: &WireScreen, theme: &Theme) -> Div {
    let mut ids = 0usize;
    div()
        .flex_none()
        .w(px(screen.width))
        .h(px(screen.height))
        .overflow_hidden()
        .rounded(px(10.0))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.canvas)
        .child(node_element(
            &screen.root,
            WireDirection::Column,
            true,
            &mut ids,
            theme,
        ))
}

fn node_element(
    node: &WireNode,
    parent_direction: WireDirection,
    force_full: bool,
    ids: &mut usize,
    theme: &Theme,
) -> AnyElement {
    match node {
        WireNode::Frame(frame) => frame_element(frame, parent_direction, force_full, ids, theme),
        WireNode::Rect(rect) => rect_element(rect, parent_direction, force_full, ids, theme),
        WireNode::Text(text) => text_element(text, parent_direction, force_full, ids, theme),
        WireNode::Divider(divider) => {
            divider_element(parent_direction, divider.annotations.as_deref(), ids, theme)
        }
    }
}

/// `width`/`height` are the node's own axes; the parent's direction says
/// which is the main axis. On the main axis `fill` claims an equal share
/// of the space left after hug and fixed siblings (`flex_1`); on the
/// cross axis it takes the parent's full inner span (`w_full`/`h_full`).
/// `force_full` is the root node's contract: it spans its whole screen.
fn apply_sizing<T: Styled>(
    element: T,
    width: WireSize,
    height: WireSize,
    parent_direction: WireDirection,
    force_full: bool,
) -> T {
    if force_full {
        return element.size_full();
    }
    let element = match (width, parent_direction) {
        (WireSize::Fixed(v), _) => element.w(px(v)),
        (WireSize::Fill, WireDirection::Row) => element.flex_1().min_w_0(),
        (WireSize::Fill, WireDirection::Column) => element.w_full(),
        (WireSize::Hug, _) => element,
    };
    match (height, parent_direction) {
        (WireSize::Fixed(v), _) => element.h(px(v)),
        (WireSize::Fill, WireDirection::Column) => element.flex_1().min_h_0(),
        (WireSize::Fill, WireDirection::Row) => element.h_full(),
        (WireSize::Hug, _) => element,
    }
}

/// Surface styling shared by `frame` and `rect`: a `WireTone` fill, a
/// tonal hairline stroke one step stronger than the fill, and a corner
/// radius. The paint only exists when the document asks for it.
fn apply_surface<T: Styled>(
    element: T,
    fill: Option<WireTone>,
    stroke: Option<WireTone>,
    corner_radius: f32,
    theme: &Theme,
) -> T {
    let element = match fill {
        Some(tone) => element.bg(fill_color(tone, theme)),
        None => element,
    };
    let element = match stroke {
        Some(tone) => element.border_1().border_color(line_color(tone, theme)),
        None => element,
    };
    if corner_radius > 0.0 {
        element.rounded(px(corner_radius))
    } else {
        element
    }
}

/// Annotation tooltips need a stateful element — only annotated nodes
/// pay for one. The counter keeps ids unique within a screen render.
fn annotated(note: Option<&str>, ids: &mut usize) -> Option<Stateful<Div>> {
    note.map(|note| {
        let id = *ids;
        *ids += 1;
        div()
            .id(gpui::ElementId::named_usize("wireframe-annotation", id))
            .tooltip(Tooltip::text_wrapped(note.to_owned()))
    })
}

fn frame_element(
    frame: &WireFrame,
    parent_direction: WireDirection,
    force_full: bool,
    ids: &mut usize,
    theme: &Theme,
) -> AnyElement {
    match annotated(frame.annotations.as_deref(), ids) {
        Some(element) => finish_frame(element, frame, parent_direction, force_full, ids, theme),
        None => finish_frame(div(), frame, parent_direction, force_full, ids, theme),
    }
}

fn finish_frame<T: Styled + ParentElement + IntoElement>(
    element: T,
    frame: &WireFrame,
    parent_direction: WireDirection,
    force_full: bool,
    ids: &mut usize,
    theme: &Theme,
) -> AnyElement {
    let element = element.flex();
    let element = match frame.direction {
        WireDirection::Row => element.flex_row(),
        WireDirection::Column => element.flex_col(),
    };
    let element = element.gap(px(frame.gap));
    let element = match frame.padding {
        WirePadding::Uniform(v) => element.p(px(v)),
        WirePadding::Axes { x, y } => element.px(px(x)).py(px(y)),
    };
    let element = match frame.align {
        WireAlign::Start => element.items_start(),
        WireAlign::Center => element.items_center(),
        WireAlign::End => element.items_end(),
        WireAlign::Stretch => element.items_stretch(),
    };
    let element = match frame.justify {
        WireJustify::Start => element,
        WireJustify::Center => element.justify_center(),
        WireJustify::End => element.justify_end(),
        WireJustify::SpaceBetween => element.justify_between(),
    };
    let element = apply_surface(
        element,
        frame.fill,
        frame.stroke,
        frame.corner_radius,
        theme,
    );
    let element = apply_sizing(
        element,
        frame.width,
        frame.height,
        parent_direction,
        force_full,
    );
    element
        .children(
            frame
                .children
                .iter()
                .map(|child| node_element(child, frame.direction, false, ids, theme)),
        )
        .into_any_element()
}

fn rect_element(
    rect: &WireRect,
    parent_direction: WireDirection,
    force_full: bool,
    ids: &mut usize,
    theme: &Theme,
) -> AnyElement {
    match annotated(rect.annotations.as_deref(), ids) {
        Some(element) => finish_rect(element, rect, parent_direction, force_full, theme),
        None => finish_rect(div(), rect, parent_direction, force_full, theme),
    }
}

fn finish_rect<T: Styled + ParentElement + IntoElement>(
    element: T,
    rect: &WireRect,
    parent_direction: WireDirection,
    force_full: bool,
    theme: &Theme,
) -> AnyElement {
    let element = apply_surface(element, rect.fill, rect.stroke, rect.corner_radius, theme);
    let element = apply_sizing(
        element,
        rect.width,
        rect.height,
        parent_direction,
        force_full,
    );
    let Some(label) = &rect.label else {
        return element.into_any_element();
    };
    // A labeled rect hugs its label like a chip: the text centers, and
    // dark fills get the on-inverse glyph where anything quieter reads
    // in the tertiary tone.
    let label_color = match rect.fill {
        Some(WireTone::Ink) | Some(WireTone::Accent) => theme.on_inverse,
        _ => theme.text_tertiary,
    };
    element
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .child(
            div()
                .text_size(sp(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(label_color)
                .truncate()
                .child(label.clone()),
        )
        .into_any_element()
}

fn text_element(
    text: &WireText,
    parent_direction: WireDirection,
    force_full: bool,
    ids: &mut usize,
    theme: &Theme,
) -> AnyElement {
    match annotated(text.annotations.as_deref(), ids) {
        Some(element) => finish_text(element, text, parent_direction, force_full, theme),
        None => finish_text(div(), text, parent_direction, force_full, theme),
    }
}

fn finish_text<T: Styled + ParentElement + IntoElement>(
    element: T,
    text: &WireText,
    parent_direction: WireDirection,
    force_full: bool,
    theme: &Theme,
) -> AnyElement {
    // The fixed type scale: wireframes use type roles, not raw font
    // specs. Sizes ride `sp` so they follow the user's UI font scale.
    let (size, weight) = match text.role {
        WireTextRole::Title => (16.0, FontWeight::SEMIBOLD),
        WireTextRole::Body => (13.0, FontWeight::NORMAL),
        WireTextRole::Caption => (11.0, FontWeight::NORMAL),
        WireTextRole::Label => (12.0, FontWeight::MEDIUM),
    };
    let align = match text.align {
        WireTextAlign::Start => TextAlign::Left,
        WireTextAlign::Center => TextAlign::Center,
        WireTextAlign::End => TextAlign::Right,
    };
    let element = element
        .text_size(sp(size))
        .font_weight(weight)
        .text_color(text_color(text.tone, theme))
        .text_align(align)
        // Wireframe text ellipsizes like real UI: one line, clipped at
        // the box edge.
        .truncate();
    let element = apply_sizing(
        element,
        text.width,
        text.height,
        parent_direction,
        force_full,
    );
    element.child(text.text.clone()).into_any_element()
}

/// A hairline separator: 1px on the parent's main axis, full span on the
/// cross axis — a horizontal rule in a column, a vertical one in a row.
fn divider_element(
    parent_direction: WireDirection,
    note: Option<&str>,
    ids: &mut usize,
    theme: &Theme,
) -> AnyElement {
    match annotated(note, ids) {
        Some(element) => finish_divider(element, parent_direction, theme),
        None => finish_divider(div(), parent_direction, theme),
    }
}

fn finish_divider<T: Styled + IntoElement>(
    element: T,
    parent_direction: WireDirection,
    theme: &Theme,
) -> AnyElement {
    let element = element.flex_none().bg(theme.separator);
    match parent_direction {
        WireDirection::Column => element.w_full().h(hairline()).into_any_element(),
        WireDirection::Row => element.h_full().w(hairline()).into_any_element(),
    }
}

/// `WireTone` as a shape fill — the palette mapped onto surface tokens.
fn fill_color(tone: WireTone, theme: &Theme) -> Hsla {
    match tone {
        // Raised card or selected-segment surface.
        WireTone::Surface => theme.raised,
        // Recessed well — input fields, segmented-control tracks.
        WireTone::Inset => theme.inset,
        // Mid emphasis — selected rows, icon and avatar placeholders.
        WireTone::Overlay => theme.overlay,
        // Dark emphasis — primary buttons and active segments.
        WireTone::Ink => theme.inverse,
        // The single accent, reserved for interactive and unread cues.
        WireTone::Accent => theme.accent,
    }
}

/// `WireTone` as a hairline stroke — a step stronger than its fill so a
/// 1px edge reads.
fn line_color(tone: WireTone, theme: &Theme) -> Hsla {
    match tone {
        WireTone::Surface | WireTone::Inset => theme.border,
        WireTone::Overlay | WireTone::Ink => theme.border_strong,
        WireTone::Accent => theme.accent,
    }
}

/// `WireTextTone` — the text palette, quieter than the fill palette.
fn text_color(tone: WireTextTone, theme: &Theme) -> Hsla {
    match tone {
        WireTextTone::Primary => theme.text,
        WireTextTone::Secondary => theme.text_secondary,
        WireTextTone::Tertiary => theme.text_tertiary,
        WireTextTone::Accent => theme.accent,
    }
}
