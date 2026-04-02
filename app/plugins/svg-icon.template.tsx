import { type JSX } from "preact"

const svgIconSymbolIds = {
  /* SVG_ICON_ENTRIES */
} as const

/** Identifiers for SVG assets generated from SVG files under the public directory. */
export type SvgIconName = keyof typeof svgIconSymbolIds

/** SVG data type for inline SVG strings. */
export type SvgIconData = string & { _brand: "svg" }

/** A SvgIcon source can be a generated icon name or an inline SVG string. */
type SvgIconSource = { name: SvgIconName; data?: never } | { data: SvgIconData; name?: never }

type ParsedInlineSvg = {
  innerMarkup: string
  preserveAspectRatio: string | null
  viewBox: string | null
}

const parsedInlineSvgs = new Map<string, ParsedInlineSvg>()

export type SvgIconProps = SvgIconSource & JSX.IntrinsicElements["svg"]

/** Renders either one generated spritemap icon or one inline SVG string. */
export function SvgIcon({ name, data, ...props }: SvgIconProps) {
  if (data !== undefined) {
    const parsedSvg = getParsedInlineSvg(data)
    return (
      <svg
        {...props}
        dangerouslySetInnerHTML={{ __html: parsedSvg.innerMarkup }}
        preserveAspectRatio={
          props.preserveAspectRatio ?? parsedSvg.preserveAspectRatio ?? undefined
        }
        viewBox={props.viewBox ?? parsedSvg.viewBox ?? undefined}
      />
    )
  }
  return (
    <svg {...props}>
      <use href={`/__spritemap#${svgIconSymbolIds[name]}`} />
    </svg>
  )
}

/** Returns a cached parsed shape for one inline SVG string. */
function getParsedInlineSvg(data: SvgIconData) {
  const cachedSvg = parsedInlineSvgs.get(data)

  if (cachedSvg) {
    return cachedSvg
  }

  const document = new DOMParser().parseFromString(data, "image/svg+xml")
  const sourceSvg = document.documentElement as unknown as SVGSVGElement

  if (sourceSvg.tagName.toLowerCase() !== "svg") {
    throw new Error("Expected inline SvgIcon data to parse into an <svg> root element.")
  }

  const parsedSvg = {
    innerMarkup: sourceSvg.innerHTML,
    preserveAspectRatio: sourceSvg.getAttribute("preserveAspectRatio"),
    viewBox: sourceSvg.getAttribute("viewBox"),
  }

  parsedInlineSvgs.set(data, parsedSvg)
  return parsedSvg
}
