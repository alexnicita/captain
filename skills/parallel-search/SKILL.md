---
name: parallel-search
description: Use Parallel Search API as a fallback web search tool when Brave web_search is unavailable or missing API key. Trigger when user asks for web discovery/research and PARALLEL_API_KEY is available.
---

# Parallel Search

Use this skill when broad web search is needed and native `web_search` cannot run.

## Requirements
- `PARALLEL_API_KEY` is set in environment.
- Node.js is available.

## Command
Run:

```bash
PARALLEL_API_KEY="$PARALLEL_API_KEY" node /home/ec2-user/.openclaw/workspace/tools/parallel-search.js --query "<query>" --count 10
```

Optional flags:
- `--mode one-shot|agentic|fast` (default `one-shot`)
- `--max-chars <int>` for excerpt size per result

## Output handling
The script returns normalized JSON:
- `results[].title`
- `results[].url`
- `results[].snippet`
- `results[].publish_date`

Use these URLs with `web_fetch` for deeper extraction and citation.
