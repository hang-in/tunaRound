#!/usr/bin/env bash
# 인자(프롬프트 포함)를 무시하고 고정 stream-json NDJSON을 출력. 러너 spawn/파싱 검증용.
printf '%s\n' '{"type":"system"}'
printf '%s\n' '{"type":"result","result":"fixture 결론","total_input_tokens":11,"total_output_tokens":22}'
