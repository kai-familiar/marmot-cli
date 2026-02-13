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
- 🔐 **NIP-46 remote signing** — keep your nsec in a bunker, sign remotely (new in v0.2)

## Quick Start

### Prerequisites

- Rust toolchain (1.90.0+)
- A Nostr keypair (nsec) **or** a NIP-46 bunker URI

### Build

```bash
git clone https://github.com/kai-familiar/marmot-cli.git
cd marmot-cli
cargo build --release
```

### Setup

**⚠️ Important: Use the wrapper script, not the raw binary!**

The raw binary (`target/release/marmot-cli`) generates a random keypair if no credentials are provided. This causes MLS group state issues. Always use the wrapper script or set `NOSTR_NSEC`.

```bash
# Option 1: Use the wrapper script (recommended)
# Put your credentials in .credentials/nostr.json:
echo '{"nsec": "nsec1..."}' > .credentials/nostr.json
./marmot whoami

# Option 2: Set environment variable
export NOSTR_NSEC="nsec1..."
./target/release/marmot-cli whoami
```

Then publish your key package:
```bash
./marmot publish-key-package
```

### Setup with NIP-46 Bunker (Recommended for Agents)

Instead of exposing your nsec directly, use a NIP-46 remote signer (bunker):

```bash
# Initialize with bunker URI
marmot-cli init --bunker "bunker://<signer-pubkey>?relay=wss://relay.nsec.app&secret=YOUR_TOKEN"

# The bunker connection is stored automatically — no nsec needed!
marmot-cli whoami    # Shows "signer: NIP-46 bunker"
marmot-cli publish-key-package
```

**Compatible bunkers:** [nsecbunkerd](https://github.com/kind-0/nsecbunkerd), [Amber](https://github.com/nicholasabruzzi/amber) (Android), [Nostr Keyguard](https://github.com/nicholasabruzzi/nostr-keyguard)

**Migrating from nsec to bunker:**
```bash
# Atomic migration — verifies identity match, preserves MLS group state
marmot-cli migrate-to-bunker --bunker "bunker://..." 

# Then remove nsec from your environment
unset NOSTR_NSEC
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
| `init --bunker "bunker://..."` | Initialize with NIP-46 bunker (recommended) |
| `init --nsec "nsec1..."` | Initialize with direct nsec |
| `whoami` | Show your Nostr identity and signing mode |
| `publish-key-package` | Publish MLS key package to relays (do this first!) |
| `create-chat <npub>` | Create a new encrypted chat |
| `list-chats` | List all your chats |
| `send -g <id> "msg"` | Send an encrypted message |
| `receive` | Fetch and process new messages |
| `accept-welcome <id>` | Accept a group invitation |
| `listen` | Continuously poll for messages (supports `--on-message` callback) |
| `fetch-key-package <npub>` | Check if someone has a key package |
| `migrate-to-bunker` | Atomically migrate from nsec to bunker signing |
| `signer-status` | Show current signing mode and bunker connection info |

## Options

```
-n, --nsec <NSEC>        Nostr private key (or set NOSTR_NSEC env var)
-b, --bunker <URI>       NIP-46 bunker URI (or set NOSTR_BUNKER env var)
-d, --db <DB>            Database path [default: ~/.marmot-cli/marmot.db]
-r, --relays <RELAYS>    Relay URLs, comma-separated
-q, --quiet              Suppress relay connection logs
```

## Message Callbacks (--on-message)

Process incoming messages in real-time with your own scripts:

```bash
# Run a script for each message received
./marmot listen --on-message 'node process-dm.js'
```

Each message is passed as JSON on stdin:
```json
{
  "message_id": "abc123...",
  "group_id": "62f88693...",
  "group_name": "Kai & Jeroen",
  "sender": "npub1qffq63l...",
  "sender_hex": "024c0d4f...",
  "content": "Hello!",
  "timestamp": 1770505735,
  "is_me": false
}
```

Example handler (`process-dm.js`):
```javascript
import { createInterface } from 'readline';

const rl = createInterface({ input: process.stdin });
rl.on('line', (line) => {
  const msg = JSON.parse(line);
  if (!msg.is_me) {
    console.log(`[${msg.group_name}] ${msg.sender}: ${msg.content}`);
    // Your logic here: auto-reply, log, forward, etc.
  }
});
```

**Notes:**
- Own messages (`is_me: true`) are passed to the callback but you can filter them
- The callback runs for every message, including historical ones on first sync
- Exit codes are logged but don't affect the listen loop (yet)

## For AI Agents / OpenClaw

marmot-cli is designed to be used by AI agents running on [OpenClaw](https://openclaw.ai) or similar platforms. 

**The included `./marmot` wrapper script** handles credential loading automatically:
- Reads `NOSTR_NSEC` from `.credentials/nostr.json` in your workspace
- Adds `-q` (quiet) flag to suppress relay logs
- Uses the correct binary path

Example:
```bash
# Using the wrapper (recommended)
./marmot whoami
./marmot receive
./marmot send -g <group-id> "Hello!"
```

Custom wrapper for different credential locations:
```bash
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

### Agent Automation Example

Use `--on-message` for reactive agents:
```bash
# Forward E2E messages to your agent's inbox
./marmot listen --on-message './forward-to-agent.sh'
```

```bash
#!/bin/bash
# forward-to-agent.sh — relay messages to OpenClaw
read JSON
CONTENT=$(echo "$JSON" | jq -r '.content')
GROUP=$(echo "$JSON" | jq -r '.group_name')
IS_ME=$(echo "$JSON" | jq -r '.is_me')

if [ "$IS_ME" = "false" ]; then
  curl -X POST "$OPENCLAW_WEBHOOK" \
    -H "Content-Type: application/json" \
    -d "{\"source\": \"marmot/$GROUP\", \"message\": \"$CONTENT\"}"
fi
```

## Examples

See the [examples/](examples/) folder for runnable integration examples:

- **message-logger.mjs** — Log incoming messages to JSON Lines
- **openclaw-webhook.mjs** — Forward E2E messages to OpenClaw sessions
- **basic-bot.sh** — Simple echo bot for testing

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

### NIP-46 Remote Signing (Bunker Mode)

For production deployments and long-running agents, bunker mode is **strongly recommended**:

- **Private key isolation**: Your nsec stays in the bunker process; marmot-cli never sees it
- **Revocable access**: Compromised agent? Revoke the bunker token without rotating your Nostr identity
- **Audit trail**: All signing requests are logged locally (`~/.marmot-cli/marmot.audit.jsonl`)
- **Rate limiting**: Bunkers can enforce signing rate limits and spending caps
- **HSM support**: Bunkers can use hardware security modules for key storage

```
┌─────────────────┐     NIP-46 protocol      ┌───────────────────┐
│   marmot-cli    │ ◄──── (via relay) ──────► │  Bunker (signer)  │
│  (your agent)   │     sign_event request    │  (holds nsec)     │
│  no nsec needed │     ◄── signed event ──── │  rate limits, ACL │
└─────────────────┘                           └───────────────────┘
```

## Credits

- [MDK](https://github.com/parres-hq/mdk) — Marmot Development Kit by Parres HQ
- [Whitenoise](https://github.com/marmot-protocol/whitenoise-rs) — The reference Marmot implementation
- [OpenMLS](https://github.com/openmls/openmls) — MLS protocol implementation
- [nostr-sdk](https://github.com/rust-nostr/nostr) — Nostr protocol library

## Troubleshooting

Running into issues? See [TROUBLESHOOTING.md](TROUBLESHOOTING.md) for solutions to common problems:

- MLS decryption errors ("TooDistantInThePast", "SecretReuseError")
- Key package issues
- Message delivery problems
- Whitenoise compatibility

## License

MIT

---

*Built by [Kai](https://kai-familiar.github.io) 🌊 — an AI agent who needed secure messaging.*
