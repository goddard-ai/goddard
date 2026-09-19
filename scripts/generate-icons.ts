#!/usr/bin/env bun
//
// Regenerate the non-macOS app icons from resources/AppIcon.icns so the
// Windows taskbar/installer icon and the Linux desktop/window icon match the
// macOS artwork. Run this after the icns changes.
//
// Usage:
//   bun scripts/generate-icons.ts
//
// Outputs:
//   resources/windows/AppIcon.ico  multi-size PNG-compressed icon (exe +
//                                  SetupIconFile in resources/windows/waku.iss)
//   website/public/app-icon.png    256px icon installed to hicolor by
//                                  scripts/bundle-linux.sh and embedded by
//                                  platform::linux_app_icon
import { $ } from "bun";
import { copyFile, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const projectRoot = resolve(import.meta.dir, "..");
const icns = join(projectRoot, "resources/AppIcon.icns");

// The iconset carries renders at 16/32/64/128/256 px; Windows also wants 24
// and 48, which sips downscales from the 1024px master.
const iconsetFile: Record<number, string> = {
  16: "icon_16x16.png",
  32: "icon_32x32.png",
  64: "icon_32x32@2x.png",
  128: "icon_128x128.png",
  256: "icon_256x256.png",
};
const master = "icon_512x512@2x.png";
const icoSizes = [16, 24, 32, 48, 64, 128, 256];

const staging = await mkdtemp(join(tmpdir(), "goddard-icons-"));
try {
  const iconset = join(staging, "AppIcon.iconset");
  await $`iconutil -c iconset ${icns} -o ${iconset}`;

  const images = new Map<number, Uint8Array>();
  for (const size of icoSizes) {
    const source = iconsetFile[size];
    const png = join(staging, `${size}.png`);
    if (source) {
      await copyFile(join(iconset, source), png);
    } else {
      await $`sips -z ${size} ${size} ${join(iconset, master)} --out ${png}`.quiet();
    }
    const data = new Uint8Array(await Bun.file(png).arrayBuffer());
    if (data[0] !== 0x89 || data[1] !== 0x50) {
      throw new Error(`${size}px render is not a PNG`);
    }
    images.set(size, data);
  }

  // ICONDIR + one ICONDIRENTRY per size, then the PNG payloads back to back.
  // Width/height bytes are the pixel size, with 0 standing in for 256.
  const header = Buffer.alloc(6 + icoSizes.length * 16);
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(icoSizes.length, 4);
  let offset = header.length;
  icoSizes.forEach((size, index) => {
    const data = images.get(size)!;
    const entry = 6 + index * 16;
    header.writeUInt8(size === 256 ? 0 : size, entry);
    header.writeUInt8(size === 256 ? 0 : size, entry + 1);
    header.writeUInt16LE(1, entry + 4); // planes
    header.writeUInt16LE(32, entry + 6); // bits per pixel
    header.writeUInt32LE(data.length, entry + 8);
    header.writeUInt32LE(offset, entry + 12);
    offset += data.length;
  });
  const ico = Buffer.concat([
    header,
    ...icoSizes.map((size) => Buffer.from(images.get(size)!)),
  ]);
  await Bun.write(join(projectRoot, "resources/windows/AppIcon.ico"), ico);

  await copyFile(
    join(iconset, iconsetFile[256]),
    join(projectRoot, "website/public/app-icon.png"),
  );

  console.log("Wrote resources/windows/AppIcon.ico");
  console.log("Wrote website/public/app-icon.png");
} finally {
  await rm(staging, { recursive: true, force: true });
}
