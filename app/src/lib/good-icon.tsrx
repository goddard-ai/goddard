import { svgIconSymbolIds } from "./svg-icon-data.ts"

/** Identifiers for SVG assets generated from SVG files under `src/icons`. */
export type SvgIconName = keyof typeof svgIconSymbolIds

/** SVG data type for inline SVG strings. */
export type SvgIconData = string & { _brand: "svg" }

type SvgIconSource = { name: SvgIconName; data?: never } | {
  data: SvgIconData;
  name?: never;
}

type ParsedInlineSvg = {
  innerMarkup: string;
  preserveAspectRatio: string | undefined;
  viewBox: string | undefined;
}

const parsedInlineSvgs = new Map<string, ParsedInlineSvg>()

/** Renders either one generated spritemap icon or one inline SVG string. */
export component GoodIcon(props: SvgIconSource & preact.JSX.IntrinsicElements["svg"]) {
  const svgProps = getSvgElementProps(props)

  if (props.data !== undefined) {
    const parsedSvg = getParsedInlineSvg(props.data)

    <svg
      preserveAspectRatio={parsedSvg.preserveAspectRatio}
      viewBox={parsedSvg.viewBox}
      {...svgProps}
      dangerouslySetInnerHTML={{
        __html: parsedSvg.innerMarkup,
      }}
    />
  } else {
    <svg {...svgProps}>
      <use href={`/__spritemap#${svgIconSymbolIds[props.name]}`} />
    </svg>
  }
}

/** Removes SvgIcon-only props before forwarding the rest to the `<svg>` element. */
function getSvgElementProps(props: SvgIconSource & preact.JSX.IntrinsicElements["svg"]) {
  const svgProps = { ...props }

  delete (svgProps as { data?: SvgIconData }).data
  delete (svgProps as { name?: SvgIconName }).name

  return svgProps
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

  const parsedSvg: ParsedInlineSvg = {
    innerMarkup: sourceSvg.innerHTML,
    preserveAspectRatio: sourceSvg.getAttribute("preserveAspectRatio") ?? undefined,
    viewBox: sourceSvg.getAttribute("viewBox") ?? undefined,
  }

  parsedInlineSvgs.set(data, parsedSvg)
  return parsedSvg
}
