#!/bin/sh
# Records every request into requests.jsonl next to the binary so tests can
# inspect the wire params, then drives one tiny turn per turn/start.
fixture_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
printf '%s\n' "$@" > "$fixture_dir/arguments.txt"
thread_id=thread-ephemeral

while IFS= read -r request; do
    printf '%s\n' "$request" >> "$fixture_dir/requests.jsonl"
    id=$(printf '%s' "$request" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
    case "$request" in
        *'"method":"initialize"'*)
            printf '{"id":%s,"result":{}}\n' "$id"
            ;;
        *'"method":"initialized"'*) ;;
        *'"method":"thread/start"'*)
            printf '{"id":%s,"result":{"thread":{"id":"%s","turns":[]}}}\n' "$id" "$thread_id"
            ;;
        *'"method":"turn/start"'*)
            printf '{"id":%s,"result":{"turn":{"id":"turn-1"}}}\n' "$id"
            printf '{"method":"turn/started","params":{"threadId":"%s","turn":{"id":"turn-1"}}}\n' "$thread_id"
            printf '{"method":"item/agentMessage/delta","params":{"threadId":"%s","delta":"OK"}}\n' "$thread_id"
            printf '{"method":"turn/completed","params":{"threadId":"%s","turn":{"id":"turn-1","status":"completed"}}}\n' "$thread_id"
            ;;
        *) printf '{"id":%s,"error":{"code":-32601,"message":"method not found"}}\n' "$id" ;;
    esac
done
