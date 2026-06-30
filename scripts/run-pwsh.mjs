#!/usr/bin/env node

import { spawnSync } from "node:child_process"
import process from "node:process"

const [, , scriptPath, ...scriptArgs] = process.argv

if (!scriptPath) {
  console.error("Usage: node scripts/run-pwsh.mjs <script.ps1> [args...]")
  process.exit(2)
}

const candidates =
  process.platform === "win32"
    ? ["pwsh.exe", "powershell.exe", "pwsh", "powershell"]
    : ["pwsh", "powershell"]

const findPowerShell = () => {
  for (const candidate of candidates) {
    const result = spawnSync(candidate, ["-NoLogo", "-NoProfile", "-Command", "$PSVersionTable.PSVersion.ToString()"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    })

    if (result.status === 0) {
      return candidate
    }
  }
  return null
}

const shell = findPowerShell()
if (!shell) {
  console.error(
    "PowerShell was not found. Install PowerShell 7+ (`pwsh`) or Windows PowerShell (`powershell`) to run SFTP performance scripts.",
  )
  process.exit(127)
}

const result = spawnSync(
  shell,
  [
    "-NoLogo",
    "-NoProfile",
    "-NonInteractive",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    scriptPath,
    ...scriptArgs,
  ],
  { stdio: "inherit" },
)

if (result.error) {
  console.error(result.error.message)
  process.exit(1)
}

process.exit(result.status ?? 1)
