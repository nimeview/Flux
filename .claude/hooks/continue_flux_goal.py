#!/usr/bin/env python3
"""Bounded Claude Code Stop hook for the persistent Flux Core goal."""

import json
import os
import re
import sys
from pathlib import Path


def main() -> None:
    try:
        hook_input = json.load(sys.stdin)
    except (json.JSONDecodeError, OSError):
        return

    if hook_input.get("stop_hook_active"):
        return

    project = Path(
        os.environ.get("CLAUDE_PROJECT_DIR") or hook_input.get("cwd") or Path.cwd()
    ).resolve()
    config_path = project / ".claude" / "flux_goal.json"
    try:
        config = json.loads(config_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return

    if config.get("status") != "active" or not config.get("continue_on_stop", False):
        return

    session_id = re.sub(r"[^A-Za-z0-9_-]", "_", str(hook_input.get("session_id", "session")))
    state_dir = project / ".claude" / ".continuation_state"
    state_file = state_dir / f"{session_id}.json"
    try:
        state_dir.mkdir(parents=True, exist_ok=True)
        state = json.loads(state_file.read_text(encoding="utf-8"))
    except FileNotFoundError:
        state = {"count": 0}
    except (OSError, json.JSONDecodeError):
        state = {"count": 0}

    try:
        limit = max(0, int(config.get("max_auto_continuations_per_session", 3)))
    except (TypeError, ValueError):
        limit = 3
    count = int(state.get("count", 0))
    if count >= limit:
        return

    try:
        state_file.write_text(json.dumps({"count": count + 1}), encoding="utf-8")
    except OSError:
        return

    context = (
        "Continue the active Flux Core objective. Read CLAUDE.md and "
        ".claude/FLUX_CORE_PROGRESS.md, inspect the current git status/diff, and "
        "continue with the earliest incomplete item. Preserve existing user changes. "
        "Update the progress checkpoint before stopping. The automatic continuation "
        "budget for this session is bounded; if work remains, leave a clear next action."
    )
    print(
        json.dumps(
            {
                "hookSpecificOutput": {
                    "hookEventName": "Stop",
                    "additionalContext": context,
                }
            },
            ensure_ascii=False,
        )
    )


if __name__ == "__main__":
    main()
