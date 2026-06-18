/*
 * Validates public docs folders for required page shape, README coverage,
 * resolvable local links, and banned implementation-focused phrasing.
 */
import fs from "node:fs"
import path from "node:path"

type CheckError = string
type FileCheck = (file: string, text: string, errors: CheckError[]) => void

const root = process.cwd()
const ignoredDirs = new Set([".git", ".turbo", "dist", "node_modules", "styled-system"])
const legacyTechnicalDocsDirs = new Set([
  "core/schema/docs",
  "core/ui-primitives/docs",
  "workforce/docs",
])

const bannedPatterns = [
  { pattern: /\bTODO\b/i, label: "TODO marker" },
  { pattern: /\bFIXME\b/i, label: "FIXME marker" },
  { pattern: /\bimplementation details?\b/i, label: "implementation detail phrasing" },
  { pattern: /\bsource tours?\b/i, label: "source tour phrasing" },
  { pattern: /\bchangelog\b/i, label: "changelog phrasing" },
  { pattern: /\bexact diagnostic\b/i, label: "exact diagnostic phrasing" },
  { pattern: /\bprivate schemas?\b/i, label: "private schema phrasing" },
  { pattern: /\bhelper functions?\b/i, label: "helper function phrasing" },
  {
    pattern: /(?:^|[\s(])(?:\.{0,2}\/)?(?:[^\s`)]*\/)?src\/[^\s`)]*\.(?:js|mjs|ts|tsx|tsrx)\b/i,
    label: "private source path",
  },
]

function relative(filePath: string) {
  return path.relative(root, filePath)
}

function walkDirs(dir: string, visit: (dir: string) => void) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isDirectory() || ignoredDirs.has(entry.name)) {
      continue
    }

    const entryPath = path.join(dir, entry.name)
    visit(entryPath)
    walkDirs(entryPath, visit)
  }
}

function findDocsDirs() {
  const docsDirs: string[] = []
  walkDirs(root, (dir) => {
    if (path.basename(dir) === "docs" && !legacyTechnicalDocsDirs.has(relative(dir))) {
      docsDirs.push(dir)
    }
  })
  return docsDirs
}

function collectMarkdownFiles(dir: string) {
  const files: string[] = []
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const entryPath = path.join(dir, entry.name)
    if (entry.isDirectory()) {
      files.push(...collectMarkdownFiles(entryPath))
    } else if (entry.isFile() && entry.name.endsWith(".md")) {
      files.push(entryPath)
    }
  }
  return files
}

function collectDocsSubdirs(dir: string) {
  const dirs = [dir]
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      dirs.push(...collectDocsSubdirs(path.join(dir, entry.name)))
    }
  }
  return dirs
}

function containsMarkdown(dir: string): boolean {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const entryPath = path.join(dir, entry.name)
    if (entry.isDirectory() && containsMarkdown(entryPath)) {
      return true
    }
    if (entry.isFile() && entry.name.endsWith(".md")) {
      return true
    }
  }
  return false
}

function lineNumberForIndex(text: string, index: number) {
  return text.slice(0, index).split("\n").length
}

const checkPageShape: FileCheck = (file, text, errors) => {
  const lines = text.split("\n")
  if (!lines[0]?.startsWith("# ")) {
    errors.push(`${relative(file)}:1 must start with one H1 heading`)
  }
  if (lines[1] !== "") {
    errors.push(`${relative(file)}:2 must be blank after the H1`)
  }
  if (!lines[2]?.startsWith("> ")) {
    errors.push(`${relative(file)}:3 must be a Markdown blockquote opener`)
  } else if (/^>\s*["“]/.test(lines[2])) {
    errors.push(`${relative(file)}:3 blockquote opener must not wrap the sentence in quotes`)
  }
  if (lines[3] !== "") {
    errors.push(`${relative(file)}:4 must be blank after the opening blockquote`)
  }
}

const checkLinks: FileCheck = (file, text, errors) => {
  for (const match of text.matchAll(/\[[^\]]+\]\(([^)#][^)]*)\)/g)) {
    const target = match[1]
    if (!target) {
      continue
    }

    if (/^[a-z][a-z0-9+.-]*:/i.test(target)) {
      continue
    }

    const fullPath = path.resolve(path.dirname(file), target)
    if (!fs.existsSync(fullPath)) {
      errors.push(
        `${relative(file)}:${lineNumberForIndex(text, match.index)} links to missing ${target}`,
      )
    }
  }
}

const checkBannedMarkers: FileCheck = (file, text, errors) => {
  for (const { pattern, label } of bannedPatterns) {
    const match = pattern.exec(text)
    if (match) {
      errors.push(`${relative(file)}:${lineNumberForIndex(text, match.index)} contains ${label}`)
    }
  }
}

const errors: CheckError[] = []
const docsDirs = findDocsDirs()

for (const docsDir of docsDirs) {
  for (const dir of collectDocsSubdirs(docsDir)) {
    if (containsMarkdown(dir) && !fs.existsSync(path.join(dir, "README.md"))) {
      errors.push(`${relative(dir)} must include README.md`)
    }
  }

  for (const file of collectMarkdownFiles(docsDir)) {
    const text = fs.readFileSync(file, "utf8")
    checkPageShape(file, text, errors)
    checkLinks(file, text, errors)
    checkBannedMarkers(file, text, errors)
  }
}

if (errors.length > 0) {
  console.error(errors.join("\n"))
  process.exit(1)
}

console.log(`Checked ${docsDirs.length} public docs folder${docsDirs.length === 1 ? "" : "s"}.`)
