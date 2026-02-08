# marmot-cli Examples

Real, runnable examples for integrating marmot-cli into your workflows.

## Message Handlers (for `--on-message`)

These scripts work with the `--on-message` callback feature of the `listen` command:

### message-logger.mjs

Logs all incoming E2E encrypted messages to a JSON Lines file.

```bash
./marmot listen --on-message './examples/message-logger.mjs'
```

Creates `messages.jsonl` with entries like:
```json
{"message_id":"abc123","group_id":"def456","group_name":"My Chat","sender":"npub1...","content":"Hello!","timestamp":"2026-02-08T05:00:00Z","is_me":false,"logged_at":"2026-02-08T05:00:01Z"}
```

### openclaw-webhook.mjs

Forwards E2E messages to an HTTP webhook endpoint. Works with any HTTP receiver (n8n, Zapier, custom services).

```bash
export OPENCLAW_GATEWAY_URL="https://your-webhook.example.com/receive"
./marmot listen --on-message './examples/openclaw-webhook.mjs'
```

**Note:** OpenClaw's cron wake API isn't directly exposed via HTTP by default. For direct OpenClaw integration, agents receive messages through the built-in marmot-cli channels. This webhook example is useful for bridging to other services.

## Simple Bots

### basic-bot.sh

A shell-based echo bot that replies to incoming messages.

```bash
chmod +x examples/basic-bot.sh
./examples/basic-bot.sh
```

**Note:** This is a simple example. For production use, prefer the Node.js handlers with `--on-message` for better reliability.

## Handler JSON Schema

When using `--on-message`, your handler receives JSON on stdin:

```json
{
  "message_id": "unique-id",
  "group_id": "mls-group-id",
  "group_name": "Chat Name",
  "sender": "npub1...",
  "sender_hex": "hex-pubkey",
  "content": "The message text",
  "timestamp": "2026-02-08T05:00:00Z",
  "is_me": false
}
```

Your handler should:
1. Parse JSON from stdin
2. Process the message
3. Exit with code 0 on success, non-zero on failure

## Contributing

Built something useful? Open a PR at [github.com/kai-familiar/marmot-cli](https://github.com/kai-familiar/marmot-cli)!
