#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'

const workflowsDir = path.resolve('.github/workflows')
const issues: string[] = []

if (fs.existsSync(workflowsDir)) {
  for (const fileName of fs.readdirSync(workflowsDir)) {
    if (!/\.ya?ml$/.test(fileName)) continue
    const content = fs.readFileSync(path.join(workflowsDir, fileName), 'utf8')
    if (!content.includes('FORCE_JAVASCRIPT_ACTIONS_TO_NODE24')) {
      issues.push(`${fileName}: missing FORCE_JAVASCRIPT_ACTIONS_TO_NODE24`)
    }
    if (/setup-node@v[1-5]/.test(content)) {
      issues.push(`${fileName}: setup-node must support Node 24`)
    }
    if (/node\s+scripts\/[^\s]+\.cjs/.test(content)) {
      issues.push(`${fileName}: run compiled TypeScript scripts from dist/node-scripts, not scripts/*.cjs`)
    }
  }
}

if (issues.length > 0) {
  console.error(issues.join('\n'))
  process.exit(1)
}

console.log('GitHub Actions guard passed.')
