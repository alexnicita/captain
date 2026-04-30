from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def test_personal_files_are_gitignored():
    gitignore = (ROOT / ".gitignore").read_text(encoding="utf-8")
    for name in ["AGENTS.md", "HEARTBEAT.md", "IDENTITY.md", "SOUL.md", "TOOLS.md", "USER.md", "MEMORY.md"]:
        assert name in gitignore


def test_private_zone_exists():
    assert (ROOT / "captain" / "private" / "README.md").exists()
    assert (ROOT / "captain" / "private" / ".gitkeep").exists()


def test_security_docs_define_isolation_boundary():
    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    security = (ROOT / "SECURITY.md").read_text(encoding="utf-8")
    threat_model = (ROOT / "docs" / "captain" / "security-threat-model.md").read_text(
        encoding="utf-8"
    )

    assert "governance, not a VM/container sandbox" in readme
    assert "not a container or VM sandbox by default" in security
    assert "What Captain Does Not Control By Default" in threat_model
    assert "Event Log Sharing Checklist" in threat_model


def test_product_naming_explains_agent_harness_compatibility():
    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    assert "Captain** is the product and repository" in readme
    assert "`agent-harness`** is the current Rust package/binary" in readme
