#!/usr/bin/env node

import { existsSync, mkdtempSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { spawnSync } from "node:child_process"
import process from "node:process"

const args = new Set(process.argv.slice(2))
const skipBuild = args.has("--skip-build")
const appPath = "src-tauri/target/release/bundle/macos/Resh.app"
const bundleId = "com.fonlan.resh"
const processName = "resh"
const windowOwnerNames = ["Resh", "resh"]

const run = (command, commandArgs, options = {}) => {
  const result = spawnSync(command, commandArgs, {
    encoding: "utf8",
    stdio: options.capture ? ["ignore", "pipe", "pipe"] : "inherit",
    ...options,
  })

  if (result.error) {
    throw result.error
  }
  return result
}

if (process.platform !== "darwin") {
  console.log("macOS Tauri smoke test skipped outside macOS.")
  process.exit(0)
}

if (!skipBuild) {
  const build = run("npm", [
    "run",
    "tauri-build",
    "--",
    "--config",
    "src-tauri/tauri.macos.conf.json",
    "--bundles",
    "app",
  ])
  if (build.status !== 0) {
    process.exit(build.status ?? 1)
  }
}

if (!existsSync(appPath)) {
  console.error(`Built app was not found: ${appPath}`)
  process.exit(1)
}

const swiftDir = mkdtempSync(join(tmpdir(), "resh-smoke-"))
const swiftPath = join(swiftDir, "visible-window.swift")
writeFileSync(
  swiftPath,
  `
import CoreGraphics
import Foundation

var owners = Set(CommandLine.arguments.dropFirst())
if owners.isEmpty {
  owners.insert("Resh")
}
let options = CGWindowListOption(arrayLiteral: .optionOnScreenOnly, .excludeDesktopElements)
guard let windows = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else {
  exit(2)
}

for window in windows {
  guard let owner = window[kCGWindowOwnerName as String] as? String, owners.contains(owner) else { continue }
  guard let bounds = window[kCGWindowBounds as String] as? [String: Any] else { continue }
  let width = bounds["Width"] as? Double ?? 0
  let height = bounds["Height"] as? Double ?? 0
  let alpha = window[kCGWindowAlpha as String] as? Double ?? 0
  if width >= 320 && height >= 240 && alpha > 0 {
    print("visible")
    exit(0)
  }
}

exit(1)
`,
)

let pid = null
let exitCode = 1
try {
  const open = run("open", ["-n", appPath])
  if (open.status !== 0) {
    exitCode = open.status ?? 1
    console.error("Failed to open app bundle.")
  } else {
    const deadline = Date.now() + 30_000
    while (Date.now() < deadline) {
      const pgrep = run("pgrep", ["-x", processName], { capture: true })
      if (pgrep.status === 0) {
        pid = pgrep.stdout.trim().split(/\s+/)[0]
        const visible = run("swift", [swiftPath, ...windowOwnerNames], { capture: true })
        if (visible.status === 0) {
          console.log(`macOS smoke passed: ${processName} launched with a visible main window.`)
          exitCode = 0
          break
        }
      }
      if (exitCode === 0) {
        break
      }
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 1000)
    }

    if (exitCode !== 0) {
      console.error(`Timed out waiting for ${processName} to expose a visible main window.`)
    }
  }
} finally {
  const quit = run("osascript", ["-e", `tell application id "${bundleId}" to quit`], {
    capture: true,
  })
  if (quit.status !== 0 && pid) {
    run("kill", [pid], { capture: true })
  }
}

process.exit(exitCode)
