'use strict';

// Which package holds the program for this machine, and where the program is inside it.

const PACKAGES = {
  'linux x64': '@miyu-agent/litemiyu-linux-x64',
  'linux arm64': '@miyu-agent/litemiyu-linux-arm64',
  'darwin arm64': '@miyu-agent/litemiyu-darwin-arm64',
};

function program() {
  const name = PACKAGES[`${process.platform} ${process.arch}`];
  if (!name) {
    throw new Error(
      `there is no prebuilt program for ${process.platform} ${process.arch}. ` +
        'Supported: Linux x64, Linux arm64, macOS arm64.'
    );
  }
  try {
    return require.resolve(`${name}/bin/litemiyu`);
  } catch {
    throw new Error(
      `the package ${name} is not installed. It is an optional dependency of litemiyu: ` +
        'install again without --omit=optional (or --no-optional).'
    );
  }
}

module.exports = { program };
