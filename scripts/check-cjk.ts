#!/usr/bin/env node
// 防回归:i18n 迁移完成后,新代码不应该再往前端页面 / 组件里加硬编码中文——
// 应该走 lib/i18n/locales/** 的字典 + t()。这个脚本扫描 App.tsx / pages/**/*.tsx /
// components/**/*.tsx / lib/*.ts(顶层,不含 lib/i18n/** 和测试文件),剔除注释后
// 匹配 CJK 统一表意文字区间,命中就报 file:line 并非零退出。
//
// 逃生舱:行尾带 `i18n-ignore` 的行会被跳过(例如引用第三方 API 报文里天然的中文,
// 或者故意保留的中文字面量)。用之前想一下能不能改成走字典——这个开关是给真正
// 例外用的,不是给"懒得抽 key"用的。

import fs from 'node:fs'
import path from 'node:path'

// 和其它 scripts/*.ts 一样,假定从仓库根目录运行(verify-all.sh 已经 cd 过去了)。
const repoRoot = process.cwd()
const webSrc = path.join(repoRoot, 'src/web/src')

const CJK_RANGE = /[一-鿿]/
const IGNORE_MARKER = 'i18n-ignore'
const BLOCK_COMMENT_RE = /\/\*[\s\S]*?\*\//g

function isTestFile(filePath: string): boolean {
  return /\.test\.[jt]sx?$/.test(filePath)
}

/** 递归收集目录下所有匹配扩展名的文件;跳过 exclude 命中的子目录/文件。 */
function collectFiles(dir: string, extensions: string[], exclude: (p: string) => boolean, out: string[] = []): string[] {
  if (!fs.existsSync(dir)) return out
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name)
    if (exclude(full)) continue
    if (entry.isDirectory()) {
      collectFiles(full, extensions, exclude, out)
    } else if (extensions.some((ext) => entry.name.endsWith(ext)) && !isTestFile(full)) {
      out.push(full)
    }
  }
  return out
}

function targetFiles(): string[] {
  const files: string[] = []
  const appTsx = path.join(webSrc, 'App.tsx')
  if (fs.existsSync(appTsx)) files.push(appTsx)

  files.push(...collectFiles(path.join(webSrc, 'pages'), ['.tsx', '.ts'], () => false))
  files.push(...collectFiles(path.join(webSrc, 'components'), ['.tsx', '.ts'], () => false))

  // lib/*.ts 只扫顶层(排除 lib/i18n/**——那里就是字典本身,天然全是中文)。
  const libDir = path.join(webSrc, 'lib')
  if (fs.existsSync(libDir)) {
    for (const entry of fs.readdirSync(libDir, { withFileTypes: true })) {
      if (entry.isDirectory()) continue
      const full = path.join(libDir, entry.name)
      if ((entry.name.endsWith('.ts') || entry.name.endsWith('.tsx')) && !isTestFile(full)) {
        files.push(full)
      }
    }
  }
  return files
}

/** 去掉 /* ... *\/ 块注释,但保留换行,行号不受影响(列位置不重要,只报 file:line)。 */
function stripBlockComments(content: string): string {
  return content.replace(BLOCK_COMMENT_RE, (match) => match.replace(/[^\n]/g, ' '))
}

/** 找一行里 `//` 注释的起点;`://`(比如 http://)不算注释起点。 找不到返回 -1。 */
function lineCommentStart(line: string): number {
  for (let i = 0; i < line.length - 1; i++) {
    if (line[i] === '/' && line[i + 1] === '/' && line[i - 1] !== ':') {
      return i
    }
  }
  return -1
}

type Violation = { file: string; line: number; text: string }

function scanFile(filePath: string): Violation[] {
  const raw = fs.readFileSync(filePath, 'utf8')
  // 逃生舱标记可能就写在 JSX 块注释里(`{/* i18n-ignore: ... */}`),必须在
  // 剔除块注释**之前**、按原始行检查,否则标记会跟着注释一起被削掉。
  const rawLines = raw.split('\n')
  const strippedLines = stripBlockComments(raw).split('\n')
  const violations: Violation[] = []

  strippedLines.forEach((line, index) => {
    if (rawLines[index]?.includes(IGNORE_MARKER)) return
    const commentAt = lineCommentStart(line)
    const code = commentAt >= 0 ? line.slice(0, commentAt) : line
    if (CJK_RANGE.test(code)) {
      violations.push({ file: filePath, line: index + 1, text: code.trim() })
    }
  })
  return violations
}

function main(): void {
  const files = targetFiles()
  const violations = files.flatMap(scanFile)

  if (violations.length > 0) {
    for (const v of violations) {
      const relative = path.relative(repoRoot, v.file)
      console.error(`${relative}:${v.line}: ${v.text}`)
    }
    console.error(
      `\ncheck-cjk: found ${violations.length} hardcoded CJK line(s) outside lib/i18n/**. ` +
        'Move the text into a locales dictionary and use t(), or add a trailing `// i18n-ignore` if this is a deliberate exception.'
    )
    process.exit(1)
  }

  console.log(`check-cjk: no hardcoded CJK found across ${files.length} files.`)
}

main()
