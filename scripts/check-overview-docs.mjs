import fs from "node:fs"
import path from "node:path"

const root = process.cwd()
const ignoredDirs = new Set([".git", ".turbo", "dist", "node_modules", "styled-system"])

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

function relative(filePath) {
  return path.relative(root, filePath)
}

function walkDirs(dir, visit) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isDirectory() || ignoredDirs.has(entry.name)) {
      continue
    }

    const entryPath = path.join(dir, entry.name)
    visit(entryPath)
    walkDirs(entryPath, visit)
  }
}

function findOverviewDirs() {
  const overviewDirs = []
  walkDirs(root, (dir) => {
    if (path.basename(dir) === "overview") {
      overviewDirs.push(dir)
    }
  })
  return overviewDirs
}

function collectMarkdownFiles(dir) {
  const files = []
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

function collectOverviewSubdirs(dir) {
  const dirs = [dir]
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      dirs.push(...collectOverviewSubdirs(path.join(dir, entry.name)))
    }
  }
  return dirs
}

function containsMarkdown(dir) {
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

function lineNumberForIndex(text, index) {
  return text.slice(0, index).split("\n").length
}

function checkPageShape(file, text, errors) {
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

function checkLinks(file, text, errors) {
  for (const match of text.matchAll(/\[[^\]]+\]\(([^)#][^)]*)\)/g)) {
    const target = match[1]
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

function checkBannedMarkers(file, text, errors) {
  for (const { pattern, label } of bannedPatterns) {
    const match = pattern.exec(text)
    if (match) {
      errors.push(`${relative(file)}:${lineNumberForIndex(text, match.index)} contains ${label}`)
    }
  }
}

const errors = []
const overviewDirs = findOverviewDirs()

for (const overviewDir of overviewDirs) {
  for (const dir of collectOverviewSubdirs(overviewDir)) {
    if (containsMarkdown(dir) && !fs.existsSync(path.join(dir, "README.md"))) {
      errors.push(`${relative(dir)} must include README.md`)
    }
  }

  for (const file of collectMarkdownFiles(overviewDir)) {
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

console.log(
  `Checked ${overviewDirs.length} overview folder${overviewDirs.length === 1 ? "" : "s"}.`,
)
