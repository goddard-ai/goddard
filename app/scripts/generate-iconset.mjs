#!/usr/bin/env bun

import { access, mkdir, rm, writeFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"
import pngToIco from "png-to-ico"
import sharp from "sharp"

const assetDirUrl = new URL("../assets/", import.meta.url)
const inputPath = new URL("./icon.png", assetDirUrl)
const outputDirUrl = new URL("./icon.iconset/", assetDirUrl)
const windowsIcoPath = new URL("./icon.ico", assetDirUrl)
const inputFilePath = fileURLToPath(inputPath)
const outputDirPath = path.resolve(fileURLToPath(outputDirUrl))
const assetDirPath = path.resolve(fileURLToPath(assetDirUrl))

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
        "Add icon.png in app/assets and rerun the generator.",
    )
  }
}

/** Resize the source icon into the full macOS iconset expected by Electrobun. */
async function main() {
  await assertSourceIconExists()

  const sourceImage = sharp(inputFilePath)
  const metadata = await sourceImage.metadata()

  warnIfSourceIsSmall(metadata)

  await mkdir(assetDirPath, { recursive: true })
  await rm(outputDirPath, { force: true, recursive: true })
  await mkdir(outputDirPath, { recursive: true })
  await rm(fileURLToPath(windowsIcoPath), { force: true })

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

  await writeFile(
    fileURLToPath(windowsIcoPath),
    await pngToIco(path.join(outputDirPath, "icon_256x256.png")),
  )

  console.log(`Generated ${iconFiles.length} icons in ${outputDirPath}`)
  console.log(`Generated Windows ICO at ${fileURLToPath(windowsIcoPath)}`)
}

await main()
