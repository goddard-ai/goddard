import { type JSX } from "preact"
/* SVG_ICON_IMPORTS */

const SVG_NAMESPACE = "http://www.w3.org/2000/svg"
const SPRITE_ROOT_ID = "goddard-svg-icon-sprite-root"
const SYMBOL_ID_PREFIX = "goddard-svg-icon"

const svgSources = {
  /* SVG_ICON_ENTRIES */
} as const

export type SvgIconName = keyof typeof svgSources

const mountedSymbols = new Set<string>()
let parser: DOMParser | null = null
let spriteRoot: SVGSVGElement | null = null

export function SvgIcon(
  props: { name: SvgIconName; title?: string } & JSX.IntrinsicElements["svg"],
) {
  const { name, title, ...svgProps } = props
  const symbolId = ensureSymbolMounted(name)

  return (
    <svg
      {...svgProps}
      aria-hidden={title ? undefined : true}
      aria-label={title}
      data-svg-icon={name}
      role={title ? "img" : undefined}
    >
      {title ? <title>{title}</title> : null}
      <use href={`#${symbolId}`} />
    </svg>
  )
}

function ensureSymbolMounted(name: SvgIconName) {
  const symbolId = getSymbolId(name)

  if (mountedSymbols.has(name)) {
    return symbolId
  }

  const sprite = getSpriteRoot()

  if (!sprite) {
    return symbolId
  }

  if (document.getElementById(symbolId)) {
    mountedSymbols.add(name)
    return symbolId
  }

  const parsedDocument = getParser().parseFromString(svgSources[name], "image/svg+xml")
  const sourceSvg = parsedDocument.documentElement as unknown as SVGSVGElement

  if (sourceSvg.tagName.toLowerCase() !== "svg") {
    throw new Error(`Expected ${JSON.stringify(name)} to parse into an <svg> root element.`)
  }

  rewriteLocalIds(sourceSvg, symbolId)

  const symbol = document.createElementNS(SVG_NAMESPACE, "symbol")
  symbol.setAttribute("id", symbolId)

  const viewBox = sourceSvg.getAttribute("viewBox")

  if (viewBox) {
    symbol.setAttribute("viewBox", viewBox)
  }

  const preserveAspectRatio = sourceSvg.getAttribute("preserveAspectRatio")

  if (preserveAspectRatio) {
    symbol.setAttribute("preserveAspectRatio", preserveAspectRatio)
  }

  symbol.innerHTML = sourceSvg.innerHTML
  sprite.append(symbol)
  mountedSymbols.add(name)

  return symbolId
}

function getParser() {
  if (!parser) {
    parser = new DOMParser()
  }

  return parser
}

function getSpriteRoot(): SVGSVGElement | null {
  if (typeof document === "undefined") {
    return null
  }

  if (spriteRoot?.isConnected) {
    return spriteRoot
  }

  const existingRoot = document.getElementById(SPRITE_ROOT_ID)

  if (existingRoot instanceof SVGSVGElement) {
    spriteRoot = existingRoot
    return spriteRoot
  }

  const root = document.createElementNS(SVG_NAMESPACE, "svg") as SVGSVGElement
  root.setAttribute("aria-hidden", "true")
  root.setAttribute("height", "0")
  root.setAttribute("id", SPRITE_ROOT_ID)
  root.setAttribute("width", "0")
  root.style.overflow = "hidden"
  root.style.pointerEvents = "none"
  root.style.position = "absolute"

  const mountTarget = document.body ?? document.documentElement

  mountTarget.prepend(root)
  spriteRoot = root

  return spriteRoot
}

function rewriteLocalIds(sourceSvg: SVGSVGElement, symbolId: string) {
  const idMap = new Map<string, string>()

  for (const element of sourceSvg.querySelectorAll("[id]")) {
    const currentId = element.getAttribute("id")

    if (!currentId) {
      continue
    }

    const nextId = `${symbolId}-${currentId}`
    idMap.set(currentId, nextId)
    element.setAttribute("id", nextId)
  }

  if (idMap.size === 0) {
    return
  }

  for (const element of sourceSvg.querySelectorAll("*")) {
    for (const attribute of Array.from(element.attributes)) {
      let nextValue = attribute.value

      for (const [currentId, nextId] of idMap) {
        nextValue = nextValue.replaceAll(`url(#${currentId})`, `url(#${nextId})`)

        if (nextValue === `#${currentId}`) {
          nextValue = `#${nextId}`
        }
      }

      if (attribute.name === "aria-describedby" || attribute.name === "aria-labelledby") {
        nextValue = nextValue
          .split(/\s+/)
          .map((value) => {
            return idMap.get(value) ?? value
          })
          .join(" ")
      }

      if (nextValue !== attribute.value) {
        element.setAttribute(attribute.name, nextValue)
      }
    }
  }
}

function getSymbolId(name: string) {
  return `${SYMBOL_ID_PREFIX}-${name
    .replaceAll(/[^a-zA-Z0-9]+/g, "-")
    .replaceAll(/^-+|-+$/g, "")
    .toLowerCase()}`
}
