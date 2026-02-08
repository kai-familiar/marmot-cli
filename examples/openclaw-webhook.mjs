#!/usr/bin/env node
/**
 * openclaw-webhook.mjs - Forward E2E messages to OpenClaw via cron wake
 * 
 * This handler receives messages from marmot-cli's --on-message callback
 * and forwards them to an OpenClaw session using a cron wake event.
 * 
 * This enables AI agents running on OpenClaw to receive E2E encrypted
 * messages in real-time, bridging Marmot/MLS with the OpenClaw ecosystem.
 * 
 * Setup:
 *   1. Configure GATEWAY_URL and GATEWAY_TOKEN below
 *   2. Run: ./marmot listen --on-message './examples/openclaw-webhook.mjs'
 * 
 * The message will appear in your OpenClaw session as a system event.
 */

import https from 'https';
import http from 'http';

// Configuration - adjust for your OpenClaw instance
const GATEWAY_URL = process.env.OPENCLAW_GATEWAY_URL || 'http://localhost:3377';
const GATEWAY_TOKEN = process.env.OPENCLAW_GATEWAY_TOKEN || '';

// Read JSON from stdin
let input = '';
process.stdin.setEncoding('utf8');

process.stdin.on('readable', () => {
  let chunk;
  while ((chunk = process.stdin.read()) !== null) {
    input += chunk;
  }
});

process.stdin.on('end', async () => {
  try {
    const message = JSON.parse(input);
    
    // Skip our own messages
    if (message.is_me) {
      process.exit(0);
    }
    
    // Format the wake event text
    const wakeText = `[Marmot E2E] ${message.sender} in ${message.group_name}: ${message.content}`;
    
    // Send to OpenClaw
    const url = new URL('/api/cron/wake', GATEWAY_URL);
    const client = url.protocol === 'https:' ? https : http;
    
    const postData = JSON.stringify({
      text: wakeText,
      mode: 'now'
    });
    
    const req = client.request(url, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Content-Length': Buffer.byteLength(postData),
        ...(GATEWAY_TOKEN && { 'Authorization': `Bearer ${GATEWAY_TOKEN}` })
      }
    }, (res) => {
      if (res.statusCode >= 200 && res.statusCode < 300) {
        console.log(`✅ Forwarded to OpenClaw: ${message.sender}`);
        process.exit(0);
      } else {
        console.error(`❌ OpenClaw returned ${res.statusCode}`);
        process.exit(1);
      }
    });
    
    req.on('error', (err) => {
      console.error(`❌ Failed to reach OpenClaw: ${err.message}`);
      process.exit(1);
    });
    
    req.write(postData);
    req.end();
    
  } catch (err) {
    console.error('Failed to parse message:', err.message);
    process.exit(1);
  }
});
