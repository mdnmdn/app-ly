#!/usr/bin/env node
// _tools/check-page.mjs — reusable Playwright page-load checker for this
// no-build prototype. There's no compiler to catch a bad Vue template or a
// typo'd method name, so this is the fastest way to find out before opening
// a browser by hand: boot a static server over design/, load a page,
// report console/page errors, optionally run interaction checks and/or
// take a screenshot.
//
// Usage:
//   node _tools/check-page.mjs <path> [--screenshot out.png] [--actions file.mjs] [--port 8934]
//
// <path> is relative to design/, e.g. views/customers.html or storybook.html
//
// --actions file.mjs   a module exporting `export default async (page) => {...}`,
//                       run after the page loads. Whatever it returns is
//                       printed as part of the JSON result — put Playwright
//                       interaction assertions here instead of one-off
//                       scratch scripts.
//
// Reuses an already-running server on --port if one answers; only a server
// this script started itself gets killed on exit, so it's safe to leave one
// running across repeated checks.
//
// Exit code is non-zero if any page/console error or thrown assertion
// occurred, so this is CI/hook-friendly, not just interactive.

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';
import path from 'node:path';

const DESIGN_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function parseArgs(argv) {
  const args = { screenshot: null, actions: null, port: 8934, target: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--screenshot') args.screenshot = argv[++i];
    else if (a === '--actions') args.actions = argv[++i];
    else if (a === '--port') args.port = Number(argv[++i]);
    else if (!args.target) args.target = a;
  }
  return args;
}

async function isUp(port) {
  try {
    await fetch(`http://localhost:${port}/`, { signal: AbortSignal.timeout(400) });
    return true;
  } catch {
    return false;
  }
}

function spawnServer(port) {
  const proc = spawn('python3', ['-m', 'http.server', String(port)], {
    cwd: DESIGN_ROOT,
    stdio: 'ignore',
  });
  return new Promise((resolve, reject) => {
    proc.on('error', reject);
    const start = Date.now();
    (async function poll() {
      if (await isUp(port)) return resolve(proc);
      if (Date.now() - start > 3000) return reject(new Error(`server did not start on :${port}`));
      setTimeout(poll, 100);
    })();
  });
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!args.target) {
    console.error('Usage: node _tools/check-page.mjs <path> [--screenshot out.png] [--actions file.mjs] [--port 8934]');
    process.exit(1);
  }

  let server = null;
  if (!(await isUp(args.port))) {
    server = await spawnServer(args.port);
  }

  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  const errors = [];
  page.on('pageerror', (e) => errors.push('pageerror: ' + e.message));
  page.on('console', (m) => { if (m.type() === 'error') errors.push('console: ' + m.text()); });

  const url = `http://localhost:${args.port}/${args.target}`;
  let result = null;
  try {
    await page.goto(url, { waitUntil: 'networkidle' });

    if (args.actions) {
      const mod = await import(pathToFileURL(path.resolve(args.actions)).href);
      result = await mod.default(page);
    }

    if (args.screenshot) {
      await page.screenshot({ path: args.screenshot, fullPage: true });
    }
  } catch (e) {
    errors.push('script: ' + e.message);
  } finally {
    await browser.close();
    if (server) server.kill();
  }

  console.log(JSON.stringify({ url, errors, result, screenshot: args.screenshot || null }, null, 2));
  process.exit(errors.length ? 1 : 0);
}

main();
