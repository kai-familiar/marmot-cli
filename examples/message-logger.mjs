#!/usr/bin/env node
/**
 * message-logger.mjs - Log incoming E2E encrypted messages
 * 
 * This is a handler script for marmot-cli's --on-message callback.
 * It logs all incoming messages to a JSON Lines file for later analysis.
 * 
 * Usage:
 *   ./marmot listen --on-message './examples/message-logger.mjs'
 * 
 * Output: Creates messages.jsonl in the current directory
 */

import { appendFileSync, existsSync, mkdirSync } from 'fs';
import { dirname } from 'path';

const LOG_FILE = './messages.jsonl';

// Ensure log file directory exists
const dir = dirname(LOG_FILE);
if (dir !== '.' && !existsSync(dir)) {
  mkdirSync(dir, { recursive: true });
}

// Read JSON from stdin
let input = '';
process.stdin.setEncoding('utf8');

process.stdin.on('readable', () => {
  let chunk;
  while ((chunk = process.stdin.read()) !== null) {
    input += chunk;
  }
});

process.stdin.on('end', () => {
  try {
    const message = JSON.parse(input);
    
    // Add timestamp
    message.logged_at = new Date().toISOString();
    
    // Log to file
    appendFileSync(LOG_FILE, JSON.stringify(message) + '\n');
    
    // Console output for visibility
    console.log(`[${message.timestamp}] ${message.sender} in ${message.group_name}: ${message.content}`);
    
    process.exit(0);
  } catch (err) {
    console.error('Failed to parse message:', err.message);
    process.exit(1);
  }
});
