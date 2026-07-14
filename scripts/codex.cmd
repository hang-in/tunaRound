@echo off
rem codex 자동무장 마커 래퍼 진입점(Windows) - PATH 앞에 두면 codex 호출을 가로챈다.
python "%~dp0codex_wrapper.py" %*
