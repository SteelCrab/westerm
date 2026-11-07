#!/usr/bin/env python3
"""Writes a dashboard layout with one browser pane, for the end-to-end bench.

Usage: e2e-layout.py <state dir> <url> <session id>
The dashboard must use that state dir (WESTERM_STATE_DIR) and be reloaded.
"""

import json
import sys
from pathlib import Path

state, url, session = sys.argv[1:4]
layout = {
    "version": 1,
    "activeTabId": "tbench1",
    "tabs": [
        {
            "id": "tbench1",
            "workspaceId": "wsbench1",
            "name": None,
            "layout": {"type": "pane", "id": session},
            "paneIds": [session],
            "docIds": [],
            "focusedId": session,
            "maximizedId": None,
        }
    ],
    "sessions": [{"id": session, "kind": "browser", "title": "browser", "name": None, "url": url, "target": None}],
    "docs": [],
    "workspaces": [{"id": "wsbench1", "name": None, "activeTabId": "tbench1", "icon": "layers", "color": "blue"}],
    "activeWorkspaceId": "wsbench1",
}
Path(state).mkdir(parents=True, exist_ok=True)
Path(state, "workspace.json").write_text(json.dumps(layout))
print(f"layout: {url} as {session}")
