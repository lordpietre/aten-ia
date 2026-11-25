#!/usr/bin/env python3
"""Static documentation checks for aten-ia.

- required files exist
- local Markdown links resolve
- config.example.json never contains a token
- no obvious secrets in tracked text files
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

REQUIRED = [
    "README.md",
    "LICENSE",
    "SECURITY.md",
    "PLAN.md",
    "SDD.md",
    "AGENTS.md",
    "man/aten-ia.1",
    "memvid-agent-core/config.example.json",
    "wiki/Home.md",
    "wiki/Installation.md",
    "wiki/Usage.md",
    "wiki/Architecture.md",
    "wiki/Development.md",
]

DOC_FILES = [
    "README.md",
    "SECURITY.md",
    "PLAN.md",
    "SDD.md",
    "wiki/Home.md",
    "wiki/Installation.md",
    "wiki/Usage.md",
    "wiki/Architecture.md",
    "wiki/Development.md",
]

LINK_RE = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
SECRET_RE = re.compile(
    r"(ghp_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|"
    r"-----BEGIN [A-Z ]*PRIVATE KEY-----|sk-[A-Za-z0-9]{20,})"
)


def fail(msg: str) -> None:
    print(f"  FAIL  {msg}")
    raise SystemExit(1)


def ok(msg: str) -> None:
    print(f"  ok    {msg}")


def check_required(root: Path) -> None:
    missing = [p for p in REQUIRED if not (root / p).is_file()]
    if missing:
        fail("missing required files: " + ", ".join(missing))
    ok("required files present")


def check_links(root: Path) -> None:
    broken: list[str] = []
    for rel in DOC_FILES:
        path = root / rel
        text = path.read_text(encoding="utf-8")
        for match in LINK_RE.finditer(text):
            target = match.group(1).strip()
            if target.startswith("<") and target.endswith(">"):
                target = target[1:-1]
            target = target.split()[0]
            if (
                not target
                or target.startswith(("http://", "https://", "mailto:", "#"))
                or "://" in target
            ):
                continue
            target = target.split("#", 1)[0]
            if not target:
                continue
            resolved = (path.parent / target).resolve()
            if resolved.exists():
                continue
            if resolved.suffix == "" and resolved.with_suffix(".md").exists():
                continue
            broken.append(f"{rel}: {target}")
    if broken:
        fail("broken local links:\n    " + "\n    ".join(broken))
    ok("local Markdown links resolve")


def check_example_config(root: Path) -> None:
    cfg = json.loads((root / "memvid-agent-core/config.example.json").read_text())
    if cfg.get("api", {}).get("token") is not None:
        fail("config.example.json must not contain an API token")
    ok("config.example.json has no API token")


def check_secrets(root: Path) -> None:
    skip_dirs = {".git", "target", "llama-cpp-turboquant", "models"}
    hits: list[str] = []
    for path in root.rglob("*"):
        if not path.is_file() or any(part in skip_dirs for part in path.parts):
            continue
        if path.suffix.lower() in {
            ".gguf", ".mv2", ".png", ".jpg", ".jpeg", ".gif", ".pdf", ".deb",
            ".gz", ".zip", ".bin",
        }:
            continue
        try:
            text = path.read_text(encoding="utf-8", errors="ignore")
        except OSError:
            continue
        for match in SECRET_RE.finditer(text):
            hits.append(f"{path.relative_to(root)}: {match.group(0)[:12]}…")
    if hits:
        fail("possible secrets in tracked files:\n    " + "\n    ".join(hits))
    ok("no obvious secrets in tracked files")


def main() -> None:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    print(f"checking docs in {root}")
    check_required(root)
    check_links(root)
    check_example_config(root)
    check_secrets(root)
    print("docs: OK")


if __name__ == "__main__":
    main()
