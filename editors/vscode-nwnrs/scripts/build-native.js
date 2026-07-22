'use strict';

const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');

const extensionRoot = path.resolve(__dirname, '..');
const repositoryRoot = path.resolve(extensionRoot, '..', '..');

if (process.platform !== 'darwin' || process.arch !== 'arm64') {
  throw new Error(
    `native compiler packaging is not implemented for ${process.platform}-${process.arch}; `
    + 'see VSCODE_TODO.md',
  );
}

const build = spawnSync(
  'cargo',
  ['build', '--locked', '--release', '-p', 'nwnrs-vscode-native'],
  { cwd: repositoryRoot, stdio: 'inherit' },
);
if (build.error) {
  throw build.error;
}
if (build.status !== 0) {
  process.exitCode = build.status ?? 1;
} else {
  const source = path.join(
    repositoryRoot,
    'target',
    'release',
    'libnwnrs_vscode_native.dylib',
  );
  const nativeDirectory = path.join(extensionRoot, 'native');
  const destination = path.join(nativeDirectory, 'nwnrs-vscode.darwin-arm64.node');
  fs.mkdirSync(nativeDirectory, { recursive: true });
  fs.copyFileSync(source, destination);
  fs.copyFileSync(
    path.join(repositoryRoot, 'LICENSE'),
    path.join(extensionRoot, 'LICENSE'),
  );
  const imageDirectory = path.join(extensionRoot, 'images');
  const iconDestination = path.join(imageDirectory, 'icon.png');
  fs.mkdirSync(imageDirectory, { recursive: true });
  fs.copyFileSync(
    path.join(repositoryRoot, 'assets', 'logo', 'icon.png'),
    iconDestination,
  );
  process.stdout.write(`Bundled ${destination}\n`);
  process.stdout.write(`Bundled ${iconDestination}\n`);
}
