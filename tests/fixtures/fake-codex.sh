#!/usr/bin/env bash
# stdin(프롬프트)을 버리고 고정 JSONL을 stdout으로 낸다. 러너 spawn/파싱 검증용.
cat > /dev/null
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"fixture 응답"}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":5,"output_tokens":7}}'
