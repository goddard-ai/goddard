#!/usr/bin/env bun

import { access, mkdir, rm } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"
import sharp from "sharp"

const inputPath = new URL("../icon.png", import.meta.url)
const outputDirUrl = new URL("../icon.iconset/", import.meta.url)
const windowsIconPath = new URL("../icon.windows.png", import.meta.url)
const linuxIconPath = new URL("../icon.linux.png", import.meta.url)
const inputFilePath = fileURLToPath(inputPath)
const outputDirPath = path.resolve(fileURLToPath(outputDirUrl))

const iconFiles = [
  { fileName: "icon_16x16.png", size: 16 },
  { fileName: "icon_16x16@2x.png", size: 32 },
  { fileName: "icon_32x32.png", size: 32 },
  { fileName: "icon_32x32@2x.png", size: 64 },
  { fileName: "icon_128x128.png", size: 128 },
  { fileName: "icon_128x128@2x.png", size: 256 },
  { fileName: "icon_256x256.png", size: 256 },
  { fileName: "icon_256x256@2x.png", size: 512 },
  { fileName: "icon_512x512.png", size: 512 },
  { fileName: "icon_512x512@2x.png", size: 1024 },
]

const platformIcons = [
  { label: "Windows", outputPath: windowsIconPath, size: 256 },
  { label: "Linux", outputPath: linuxIconPath, size: 512 },
]

/** Warn when the source icon is undersized for the largest output. */
function warnIfSourceIsSmall(metadata) {
  if (!metadata.width || !metadata.height) {
    return
  }

  if (metadata.width < 1024 || metadata.height < 1024) {
    console.warn(
      `Warning: icon.png is ${metadata.width}x${metadata.height}. ` +
        "The 1024x1024 output will be upscaled.",
    )
  }
}

/** Fail early with a clear error when the source icon is missing. */
async function assertSourceIconExists() {
  try {
    await access(inputPath)
  } catch {
    throw new Error(
      `Missing source icon: ${fileURLToPath(inputPath)}. ` +
        "Add icon.png in the app root and rerun the generator.",
    )
  }
}

/** Resize the source icon into the full macOS iconset expected by Electrobun. */
async function main() {
  await assertSourceIconExists()

  const sourceImage = sharp(inputFilePath)
  const metadata = await sourceImage.metadata()

  warnIfSourceIsSmall(metadata)

  await rm(outputDirPath, { force: true, recursive: true })
  await mkdir(outputDirPath, { recursive: true })

  await Promise.all(
    iconFiles.map(({ fileName, size }) =>
      sharp(inputFilePath)
        .resize(size, size, {
          fit: "contain",
          background: { r: 0, g: 0, b: 0, alpha: 0 },
        })
        .png()
        .toFile(path.join(outputDirPath, fileName)),
    ),
  )

  await Promise.all(
    platformIcons.map(({ outputPath, size }) =>
      sharp(inputFilePath)
        .resize(size, size, {
          fit: "contain",
          background: { r: 0, g: 0, b: 0, alpha: 0 },
        })
        .png()
        .toFile(fileURLToPath(outputPath)),
    ),
  )

  console.log(`Generated ${iconFiles.length} icons in ${outputDirPath}`)

  for (const { label, outputPath, size } of platformIcons) {
    console.log(`Generated ${label} icon ${size}x${size} at ${fileURLToPath(outputPath)}`)
  }
}

await main()
