<p align="center">
  <img src="logo.png" alt="NanosFlow" width="160">
</p>

<h1 align="center">NanosFlow</h1>

<p align="center">Visual scripting for <a href="https://nanos.world">nanos world</a>, in the spirit of Unreal Blueprints.</p>

NanosFlow lets you build nanos world packages from node graphs instead of hand written Lua. Graphs compile to plain Lua 5.4, so the result runs on any nanos world server without extra dependencies.

## Features

- Node graph editor for server, client and shared scripts, generated from the official nanos world API
- Functions, events, variables, dispatchers and interfaces like in Blueprints
- Compiles to readable Lua 5.4 and runs a local test server with one click
- Version control friendly project format with graph diff and merge
- Sessions: host a project, edit it together with live cursors and play on the host's test server through a relay
- Automatic updates

## Download

Get the latest build for Windows or Linux from the [releases](https://github.com/DasDarki/NanosFlow/releases/latest). Installed apps update themselves.

## Hosting a relay

Sessions connect through a small relay server. Run your own with Docker:

```sh
docker run -d --name nanosflow-relay --restart unless-stopped \
  -p 7900:7900/udp -v nanosflow-relay:/data \
  ghcr.io/dasdarki/nanosflow-relay:latest --name relay.example.com
```

The log prints the address to paste into the session settings, for example `relay.example.com:7900#<fingerprint>`. The certificate is kept in the volume, so the address stays the same across restarts. If you have a real certificate, pass `--certificate` and `--key` instead and clients need no fingerprint.

## Building from source

Requirements: Rust 1.90 or newer, Node.js 22 or newer, pnpm and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform.

```sh
cd app
pnpm install
pnpm tauri dev
```

Run the tests with `cargo test --workspace`.

## Releasing

Push a tag like `v0.2.0`. GitHub Actions builds the Windows and Linux installers, signs the update files, publishes the release and pushes the relay image to the GitHub Container Registry.

## License

[MIT](LICENSE)
