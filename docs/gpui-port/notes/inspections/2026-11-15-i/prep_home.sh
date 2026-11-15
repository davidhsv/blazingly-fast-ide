#!/bin/sh
# Fills a fresh app home: the agent "fake" (a command that is never run here) and the settings it reported before.
home=$1
mkdir -p "$home/config/agents" "$home/data/agents"
printf 'name = "Fake Agent"\ncommand = "/usr/bin/true"\nargs = []\n' > "$home/config/agents/fake.toml"
cat > "$home/data/agents/models.json" <<'JSON'
{"fake": {"config": [
  {"id": "model", "name": "Model", "description": null, "category": "model", "current": "sonnet",
   "values": [{"value": "sonnet", "name": "Claude Sonnet 5.5", "description": "Fast and capable"},
              {"value": "opus", "name": "Claude Opus 5.5", "description": "The most capable"},
              {"value": "haiku", "name": "Claude Haiku 5", "description": "The fastest"}], "via": "option"},
  {"id": "effort", "name": "Effort", "description": null, "category": "thought_level", "current": "medium",
   "values": [{"value": "low", "name": "low", "description": null}, {"value": "medium", "name": "medium", "description": null},
              {"value": "high", "name": "high", "description": null}], "via": "option"},
  {"id": "mode", "name": "Mode", "description": null, "category": "mode", "current": "default",
   "values": [{"value": "default", "name": "Default", "description": null}, {"value": "plan", "name": "Plan", "description": null}], "via": "option"}
 ],
 "thoughtLevels": {"sonnet": [
  {"id": "effort", "name": "Effort", "description": null, "category": "thought_level", "current": "medium",
   "values": [{"value": "low", "name": "low", "description": null}, {"value": "medium", "name": "medium", "description": null},
              {"value": "high", "name": "high", "description": null}], "via": "option"}]},
 "projectCommands": {}}}
JSON
