from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def test_personal_files_are_gitignored():
    gitignore = (ROOT / ".gitignore").read_text(encoding="utf-8")
    for name in ["AGENTS.md", "HEARTBEAT.md", "IDENTITY.md", "SOUL.md", "TOOLS.md", "USER.md", "MEMORY.md"]:
        assert name in gitignore


def test_private_zone_exists():
    assert (ROOT / "captain" / "private" / "README.md").exists()
    assert (ROOT / "captain" / "private" / ".gitkeep").exists()
