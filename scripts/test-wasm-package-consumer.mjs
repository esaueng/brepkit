#!/usr/bin/env node
/**
 * Verify the packed WASM package exactly as an application consumes it.
 *
 * Usage: node scripts/test-wasm-package-consumer.mjs
 */

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const packageDirectory = resolve(projectRoot, 'crates/wasm/pkg');
const temporaryRoot = mkdtempSync(resolve(tmpdir(), 'brepkit-wasm-consumer-'));
const npmCache = resolve(temporaryRoot, 'npm-cache');
const commandEnvironment = { ...process.env, npm_config_cache: npmCache };

const run = (command, arguments_, options = {}) =>
  execFileSync(command, arguments_, {
    encoding: 'utf8',
    env: commandEnvironment,
    stdio: ['ignore', 'pipe', 'pipe'],
    ...options,
  });

try {
  const packed = JSON.parse(
    run('npm', ['pack', '--json', '--pack-destination', temporaryRoot], {
      cwd: packageDirectory,
    }),
  );
  assert.equal(packed.length, 1, 'npm pack should produce exactly one archive');

  const consumerDirectory = resolve(temporaryRoot, 'consumer');
  mkdirSync(consumerDirectory);
  writeFileSync(
    resolve(consumerDirectory, 'package.json'),
    JSON.stringify({ name: 'brepkit-wasm-consumer-smoke', private: true, type: 'module' }),
  );
  run('npm', ['install', '--ignore-scripts', resolve(temporaryRoot, packed[0].filename)], {
    cwd: consumerDirectory,
  });
  run(
    'node',
    [
      '--input-type=module',
      '--eval',
      "import { BrepKernel } from 'brepkit-wasm'; const kernel = new BrepKernel(); const solid = kernel.makeBox(2, 3, 4); if (kernel.volume(solid, 0.1) !== 24) process.exit(1);",
    ],
    { cwd: consumerDirectory },
  );
  console.log('WASM package consumer smoke test passed.');
} finally {
  rmSync(temporaryRoot, { recursive: true, force: true });
}
