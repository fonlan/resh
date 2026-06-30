#!/usr/bin/env node
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

const workflowDir = '.github/workflows';
const pinnedRefPattern = /^[a-f0-9]{40}$/i;

function workflowFiles(directory) {
  return readdirSync(directory)
    .map((name) => join(directory, name))
    .filter((path) => statSync(path).isFile() && /\.(ya?ml)$/i.test(path));
}

function findUnpinnedUses(path) {
  const content = readFileSync(path, 'utf8');
  const lines = content.split(/\r?\n/);
  const findings = [];

  lines.forEach((line, index) => {
    const match = line.match(/^\s*uses:\s*([^@\s#]+)@([^\s#]+)/);
    if (!match) {
      return;
    }

    const [, action, ref] = match;
    if (action.startsWith('./') || pinnedRefPattern.test(ref)) {
      return;
    }

    findings.push(`${path}:${index + 1} uses ${action}@${ref}`);
  });

  return findings;
}

const findings = workflowFiles(workflowDir).flatMap(findUnpinnedUses);

if (findings.length > 0) {
  console.error('GitHub Actions must be pinned to full commit SHAs:');
  for (const finding of findings) {
    console.error(`- ${finding}`);
  }
  process.exit(1);
}

console.log('All GitHub Actions are pinned to full commit SHAs.');
