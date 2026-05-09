#!/usr/bin/env node

console.log(JSON.stringify({
  productVersion: '0.1.0',
  buildVersion: '0.1.0',
  buildTime: new Date().toISOString(),
  gitCommit: 'template',
}, null, 2))
