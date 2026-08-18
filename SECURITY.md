# Security Policy

Splitwave is a native, offline audio router: your audio and pipeline data stay
on your machine, and the app makes no background network calls beyond checking
for updates when you ask it to.

## Reporting a vulnerability

Do **not** open a public issue for a security vulnerability. Instead, report it
privately by emailing `support@splitwave.app`.

Please include:

- The affected version(s)
- A description of the vulnerability and its impact
- Steps to reproduce, if possible
- Any proof-of-concept

You will receive an acknowledgment within a few days and a timeline for the
fix. Please give us reasonable time to address the issue before disclosing it
publicly.

## What is in scope

- The Rust engine (`src-tauri/`), especially the audio pipeline and plugin
  hosting (CLAP, VST3, AU)
- The Tauri IPC surface between the frontend and the Rust backend
- Bundling, update, and installation paths
- The macOS virtual device driver (`src-tauri/native/virtual_driver/`)

## What is out of scope

- Third-party plugins you install and run yourself. Like every DAW, we load
  plugins into the app process without sandboxing — a plugin runs with the
  app's privileges, so install only plugins you trust.
- Known limitations of the upstream crates we depend on, unless we failed to
  apply a published fix

## Supported versions

Security fixes are applied to the latest release. We do not maintain separate
long-term-support branches.
