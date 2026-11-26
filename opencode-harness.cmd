@echo off
rem Start OpenCode with the Agent Browser config and your target project.
rem Usage: opencode-harness [C:\path\to\project]
cd /d "%~dp0"
call opencode %*
