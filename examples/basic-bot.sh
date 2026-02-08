#!/bin/bash
# basic-bot.sh - Simple Marmot echo bot example
#
# This bot listens for incoming messages and echoes them back.
# Use it as a starting point for your own agent automation.
#
# Usage: ./basic-bot.sh

set -e

# Configuration
MARMOT="./marmot"
INTERVAL=10

echo "🦫 Starting echo bot..."
echo "Listening for messages every ${INTERVAL}s..."
echo "Press Ctrl+C to stop."

# Main loop
while true; do
    # Check for new messages
    output=$($MARMOT receive 2>&1)
    
    # Parse and echo back any received messages
    echo "$output" | grep -E "^\[.*\] →" | while read -r line; do
        # Extract chat name and message
        chat=$(echo "$line" | sed -E 's/\[(.*)\].*/\1/')
        sender=$(echo "$line" | sed -E 's/.*→ ([^:]+):.*/\1/')
        message=$(echo "$line" | sed -E 's/.*→ [^:]+: (.*)/\1/')
        
        # Don't echo our own messages
        if [[ "$sender" != "You" && -n "$message" ]]; then
            echo "📨 Received from $sender in [$chat]: $message"
            
            # Get group ID for this chat (simplified - uses first 8 chars)
            group_id=$($MARMOT list-chats | grep -A1 "^$chat" | grep "MLS Group ID" | awk '{print $4}' | cut -c1-8)
            
            if [[ -n "$group_id" ]]; then
                echo "📤 Echoing back to $group_id..."
                $MARMOT send -g "$group_id" "Echo: $message"
            fi
        fi
    done
    
    sleep $INTERVAL
done
