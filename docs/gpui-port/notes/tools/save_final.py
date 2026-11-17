"""Writes an agent's final report from its task transcript to a file: the last text it said, or, when
that is a short closing line after the report (an agent that waited on its own background work
says one), the longest of its last five texts.

usage: save_final.py <task .output (JSONL transcript)> <destination .md>
"""
import json, os, sys

src, dst = sys.argv[1], sys.argv[2]


def texts(node):
    """The text blocks of one transcript line whose author is the assistant."""
    message = node.get("message") if isinstance(node, dict) else None
    if not isinstance(message, dict) or message.get("role") != "assistant":
        return []
    content = message.get("content")
    if isinstance(content, str):
        return [content]
    out = []
    for block in content or []:
        if isinstance(block, dict) and block.get("type") == "text" and block.get("text", "").strip():
            out.append(block["text"])
    return out


said = []
with open(src, encoding="utf-8", errors="replace") as f:
    for line in f:
        line = line.strip()
        if not line:
            continue
        try:
            node = json.loads(line)
        except ValueError:
            continue
        found = texts(node)
        if found:
            said.append("\n\n".join(found))
if not said:
    sys.exit(f"no assistant text found in {src}")
last = said[-1] if len(said[-1]) >= 2000 else max(said[-5:], key=len)
with open(dst, "w", encoding="utf-8") as f:
    f.write(last.rstrip() + "\n")
print(os.path.basename(dst), len(last.encode("utf-8")))
