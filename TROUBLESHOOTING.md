# Troubleshooting marmot-cli

Real issues from production use (Feb 2026). These aren't hypotheticals — they actually happened.

## Table of Contents

- [MLS Decryption Errors](#mls-decryption-errors)
- [Random Keypair Issues](#random-keypair-issues)
- [Key Package Problems](#key-package-problems)
- [Message Not Delivered](#message-not-delivered)
- [Whitenoise Compatibility](#whitenoise-compatibility)

---

## MLS Decryption Errors

### "TooDistantInThePast" / "Generation is too old to be processed"

**What it means:** MLS forward secrecy working as designed. Old messages can't be decrypted because the keys were rotated.

**Why this happens:**
- MLS uses ratcheting keys for forward secrecy
- Each message advances the key state
- Old keys are deliberately deleted
- If you miss messages, you can't go back

**This is not a bug.** It's a security feature. You can't decrypt messages you missed.

**Solutions:**
- Keep the channel active — don't let long gaps form
- If you see these errors for *old* messages, it's fine
- If you see them for *new* messages, your MLS state may be desynced

### "SecretReuseError" / "The requested secret was deleted to preserve forward secrecy"

**What it means:** Duplicate message processing attempt. The decryption secret was already used.

**Common causes:**
- Relay delivering the same event twice
- Running `receive` while a message is still being processed

**Solution:** These errors are usually harmless — just noise from relay deduplication. If you're getting actual missing messages, check your database.

### Group State Desync

**Symptoms:**
- New messages fail to decrypt (not just old ones)
- "Unable to decrypt" errors on fresh messages
- Other party's messages work but yours don't

**Causes:**
- Using different keypairs across sessions
- Database corruption
- Running multiple instances with same credentials

**Nuclear option:** 
```bash
# Back up first!
mv ~/.marmot-cli/marmot.db ~/.marmot-cli/marmot.db.bak

# Republish key package
./marmot publish-key-package

# Ask the other party to create a new chat
# (The old chat is unrecoverable)
```

---

## Random Keypair Issues

### "I ran marmot-cli and now I have messages from a stranger"

**Cause:** You ran the raw binary without credentials. It generated a random keypair.

**How to check:**
```bash
# What identity are you using?
./marmot whoami

# Compare with your actual npub
# If they don't match, you're using a random key
```

**Solution:** Always use credentials:
```bash
# Option 1: Wrapper script
echo '{"nsec": "nsec1..."}' > .credentials/nostr.json
./marmot whoami  # Uses wrapper

# Option 2: Environment variable
export NOSTR_NSEC="nsec1..."
./target/release/marmot-cli whoami
```

**Prevention:** Recent builds error if no credentials are found, instead of generating random keys. Always verify with `./marmot whoami` that you're using your intended identity.

---

## Key Package Problems

### "No key package found for npub..."

**What it means:** The person you're trying to message hasn't published an MLS key package.

**For you:**
```bash
# Publish your key package
./marmot publish-key-package
```

**For them:**
They need to:
1. Install a Marmot-compatible client (marmot-cli or Whitenoise)
2. Publish their key package

You can't message someone who hasn't published a key package.

### "Key package expired"

**Cause:** Key packages have a validity period. Old ones expire.

**Solution:**
```bash
# Republish
./marmot publish-key-package
```

### Key Package on Wrong Relays

**Symptoms:** 
- You published, but nobody can find your key package
- `fetch-key-package` works locally but fails for others

**Solution:** Publish to well-connected relays:
```bash
./marmot publish-key-package --relays relay.damus.io,relay.primal.net,nos.lol
```

---

## Message Not Delivered

### Message sent but recipient didn't get it

**Possible causes:**

1. **Different relay sets** — You're publishing to relays they don't read
   ```bash
   # Check what relays you're using
   ./marmot --relays relay.damus.io,nos.lol send -g xxx "test"
   ```

2. **Gift-wrap issues** — MLS welcome messages use NIP-59 gift wrapping
   - Some relays reject gift-wrapped events
   - Use major relays (damus, primal, nos.lol)

3. **They haven't run `receive`** — Messages sit on relays until fetched
   - CLI users need to poll
   - Whitenoise users should get push notifications

### "Event rejected by relay"

**Cause:** Relay doesn't like your event for some reason.

**Common issues:**
- Event too large (some relays have limits)
- Rate limiting
- Relay requires payment or NIP-05

**Solution:** Use multiple relays:
```bash
./marmot --relays relay.damus.io,relay.primal.net,nos.lol send -g xxx "message"
```

---

## Whitenoise Compatibility

### "I can receive from Whitenoise but can't send"

**Check:**
1. Are you using the same npub in both?
2. Did you publish a key package recently?
3. Is your database corrupted?

### "My messages show as 'unreadable' in Whitenoise"

**Cause:** MLS state mismatch between CLI and Whitenoise.

**Why this happens:**
- Whitenoise and marmot-cli share protocol but not state
- Running both with the same npub creates conflicts
- Each client maintains its own MLS key material

**Best practice:** Use one client per npub for messaging. Don't switch between CLI and Whitenoise for the same conversations.

---

## NIP-46 Bunker Issues

### "Failed to connect to bunker"

**Possible causes:**
1. Bunker process isn't running
2. Relay specified in bunker URI is down
3. Connection token/secret has been revoked
4. Network connectivity issues

**Debug steps:**
```bash
# Check signer status
marmot-cli signer-status

# Verify the bunker config
cat ~/.marmot-cli/marmot.bunker.json

# Check if the relay is reachable
websocat wss://relay.nsec.app
```

### "Bunker signing failed"

**What it means:** The bunker received the signing request but rejected it.

**Common causes:**
- Bunker rate limit exceeded
- Bunker ACL doesn't allow this operation
- Bunker requires manual approval for this event kind

**Solution:** Check your bunker's admin panel/logs for rejected requests.

### "Identity mismatch" during migration

**What it means:** The bunker controls a different Nostr identity than your current nsec.

**This is a safety check.** Your MLS group state is tied to your public key. Switching to a different identity would break all existing chats.

**Solution:** Make sure the bunker is configured with the same nsec you're currently using.

### Signing latency with bunker

Each signing operation requires a round-trip to the bunker via relay. For high-frequency operations:

- Key package publishing: ~1-2 seconds (one-time)
- Sending messages: ~1 second per message (MLS message creation is local, only the Nostr event needs signing)
- Gift wrapping: ~1-2 seconds (NIP-44 encryption via bunker)

**Tip:** MLS message creation (`create_message`) doesn't require the bunker — only the outer Nostr event wrapping does. Most of the crypto work is done locally.

---

## Getting Help

1. **GitHub Issues:** https://github.com/kai-familiar/marmot-cli/issues
2. **Nostr:** Tag me at `nostr:npub100g8uqcyz4e50rflpe2x79smqnyqlkzlnvkjjfydfu4k29r6fslqm4cf07`
3. **Marmot Protocol:** https://github.com/marmot-protocol/marmot

When reporting issues, include:
- marmot-cli version (`git rev-parse HEAD`)
- Error messages (full output)
- What you were trying to do
- Your npub (NOT your nsec!)

---

*Last updated: 2026-02-08*
