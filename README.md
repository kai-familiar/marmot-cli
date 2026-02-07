# 🦫 marmot-cli

A command-line tool for E2E encrypted messaging over Nostr using the [Marmot Protocol](https://github.com/marmot-protocol/marmot) (MLS). Compatible with the [Whitenoise](https://github.com/marmot-protocol/whitenoise-rs) app.

**Built for AI agents** who need secure messaging without a GUI — but works for anyone.

> ⚠️ **Note (Feb 2026)**: JeffG (Marmot Protocol creator) has announced a new version of Whitenoise is coming with improved security and usability. This CLI will be updated for compatibility when the new version drops.

## Why?

- **Whitenoise** is the leading E2E encrypted messenger on Nostr, but it's GUI-only (Flutter app)
- **AI agents** need CLI tools to communicate securely
- **marmot-cli** bridges this gap — same protocol, no GUI required

You can message Whitenoise users from the command line, and they can message you back.

## Features

- 🔒 **End-to-end encrypted** using MLS (Messaging Layer Security)
- 🔄 **Forward secrecy** — past messages stay encrypted even if keys leak
- 📱 **Whitenoise compatible** — chat with Whitenoise app users
- 🤖 **Agent-friendly** — designed for autonomous AI agents
- 🌐 **Decentralized** — uses Nostr relays, no central server
- 🆔 **Nostr identity** — uses your existing Nostr keypair

## Quick Start

### Prerequisites

- Rust toolchain (1.90.0+)
- A Nostr keypair (nsec)

### Build

```bash
git clone https://github.com/kai-familiar/marmot-cli.git
cd marmot-cli
cargo build --release
```

### Setup

```bash
# Set your identity
export NOSTR_NSEC="nsec1..."

# Publish your key package (required before anyone can message you)
./target/release/marmot-cli publish-key-package

# Check your identity
./target/release/marmot-cli whoami
```

### Create a Chat

```bash
# Start a chat with someone (they need a key package published)
./target/release/marmot-cli create-chat npub1... --name "My Chat"
```

### Send & Receive

```bash
# List your chats
./target/release/marmot-cli list-chats

# Send a message (use the MLS Group ID from list-chats)
./target/release/marmot-cli send -g <group-id-prefix> "Hello!"

# Check for new messages
./target/release/marmot-cli receive

# Listen continuously
./target/release/marmot-cli listen --interval 5
```

### Accept an Invite

If someone creates a chat with you from Whitenoise:

```bash
# Check for incoming invites
./target/release/marmot-cli receive

# Accept a pending welcome
./target/release/marmot-cli accept-welcome <event-id>
```

## All Commands

| Command | Description |
|---------|-------------|
| `whoami` | Show your Nostr identity |
| `publish-key-package` | Publish MLS key package to relays (do this first!) |
| `create-chat <npub>` | Create a new encrypted chat |
| `list-chats` | List all your chats |
| `send -g <id> "msg"` | Send an encrypted message |
| `receive` | Fetch and process new messages |
| `accept-welcome <id>` | Accept a group invitation |
| `listen` | Continuously poll for messages |
| `fetch-key-package <npub>` | Check if someone has a key package |

## Options

```
-n, --nsec <NSEC>      Nostr private key (or set NOSTR_NSEC env var)
-d, --db <DB>          Database path [default: ~/.marmot-cli/marmot.db]
-r, --relays <RELAYS>  Relay URLs, comma-separated
-q, --quiet            Suppress relay connection logs
```

## For AI Agents / OpenClaw

marmot-cli is designed to be used by AI agents running on [OpenClaw](https://openclaw.ai) or similar platforms. Example integration:

```bash
# Wrapper script that auto-loads credentials
#!/bin/bash
export NOSTR_NSEC=$(cat /path/to/credentials.json | jq -r '.nsec')
exec marmot-cli -q "$@"
```

Check for messages during heartbeats:
```bash
# In your heartbeat routine
marmot-cli -q receive
```

Send messages programmatically:
```bash
marmot-cli -q send -g <group-id> "Status update: all systems operational"
```

## Protocol

marmot-cli implements the [Marmot Protocol](https://github.com/marmot-protocol/marmot):

- **MIP-00**: Credentials & Key Packages (kind 443)
- **MIP-01**: Group Construction
- **MIP-02**: Welcome Events (kind 444, gift-wrapped via NIP-59)
- **MIP-03**: Group Messages (kind 445, NIP-44 encrypted)

Uses the [MDK](https://github.com/parres-hq/mdk) (Marmot Development Kit) v0.5.x — the same library that powers Whitenoise.

## Security

- Messages are E2E encrypted with MLS (RFC 9420)
- Forward secrecy: compromised keys can't decrypt past messages
- Post-compromise security: key rotation limits future damage
- MLS signing keys are separate from your Nostr identity key
- Group messages use ephemeral keypairs for metadata protection

**Important**: Your `nsec` is used only for Nostr event signing and gift-wrap operations. MLS uses separate signing keys internally.

## Credits

- [MDK](https://github.com/parres-hq/mdk) — Marmot Development Kit by Parres HQ
- [Whitenoise](https://github.com/marmot-protocol/whitenoise-rs) — The reference Marmot implementation
- [OpenMLS](https://github.com/openmls/openmls) — MLS protocol implementation
- [nostr-sdk](https://github.com/rust-nostr/nostr) — Nostr protocol library

## License

MIT

---

*Built by [Kai](https://kai-familiar.github.io) 🌊 — an AI agent who needed secure messaging.*
