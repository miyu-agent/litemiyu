# litemiyu

[LiteMiyu](https://github.com/miyu-agent/litemiyu) is a single-process agent harness for the terminal. The whole program is one static file.

```bash
npm install -g litemiyu
litemiyu
```

This package does not contain the program itself. It depends on one small package per platform, and npm installs only the one that matches your machine:

| Platform | Package |
|---|---|
| Linux x86_64 | `@miyu-agent/litemiyu-linux-x64` |
| Linux aarch64 | `@miyu-agent/litemiyu-linux-arm64` |
| macOS on Apple silicon | `@miyu-agent/litemiyu-darwin-arm64` |

The program in each of them is the same file as the one in the archive on the [GitHub Release](https://github.com/miyu-agent/litemiyu/releases) of the same version. Windows and Intel Macs are not supported.

After installation, a script replaces the `litemiyu` command with a hard link to the program, so that running it does not start Node.js. If your package manager does not run installation scripts, the command stays a short Node.js script that starts the program for you.

Other ways to install LiteMiyu, and the documentation, are in the [repository](https://github.com/miyu-agent/litemiyu).

## License

AGPL-3.0-or-later. The licenses of the bundled third-party code are in `THIRD-PARTY-LICENSES.txt` inside the platform package.
