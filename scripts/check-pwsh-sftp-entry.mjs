#!/usr/bin/env node

import { spawnSync } from "node:child_process"
import process from "node:process"

const scriptPath = "scripts/sftp_perf_suite.ps1"
const result = spawnSync(process.execPath, ["scripts/run-pwsh.mjs", scriptPath, "-?"], {
  encoding: "utf8",
  stdio: ["ignore", "pipe", "pipe"],
})

const output = `${result.stdout || ""}${result.stderr || ""}`
if (result.status !== 0 || !output.includes("ServerHost") || !output.includes("SkipFairness")) {
  process.stdout.write(output)
  console.error("SFTP PowerShell entry check failed.")
  process.exit(result.status || 1)
}

console.log("SFTP PowerShell entry is available and exposes expected parameters.")
