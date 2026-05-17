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
    assert "**`agent-harness`** is the current Rust package/binary" in readme


def test_readme_frontloads_captain_cli_for_agent_machines():
    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    front_matter = readme[:3500]

    required_phrases = [
        "Use Captain on an Agent Machine",
        "EC2 instance or MacBook",
        "curl -fsSL https://raw.githubusercontent.com/alexnicita/captain/main/install.sh | bash",
        "captain hermes \"fix the failing tests\"",
        "captain openclaw \"implement the next scoped improvement\"",
        "captain claude \"tighten the parser tests\"",
        "captain codex \"refactor the flaky fixture loader\"",
        "--dry-run",
        "--push-each-cycle",
        "Pushes are opt-in",
    ]
    forbidden_phrases = [
        "git clone https://github.com/alexnicita/captain.git",
        "bash captain/scripts/setup-openclaw-captain.sh",
        "export PATH=\"$PWD/captain/bin:$PATH\"",
    ]
    for phrase in required_phrases:
        assert phrase in front_matter
    for phrase in forbidden_phrases:
        assert phrase not in front_matter


def test_work_toolset_defines_first_cycle_spec():
    work_dir = ROOT / "captain" / "harnesses" / "rust-harness" / "toolsets" / "work"
    spec = (work_dir / "cycle-spec.md").read_text(encoding="utf-8")
    readme = (work_dir / "README.md").read_text(encoding="utf-8")

    required_sections = [
        "## Objective",
        "## Inputs",
        "## Constraints",
        "## Cycle Steps",
        "## Outputs",
        "## Commit and Log Policy",
        "## Acceptance Criteria",
    ]
    for section in required_sections:
        assert section in spec

    assert "customer-or-market-research" in spec
    assert "No secrets or credentials" in spec
    assert "cycle-spec.md" in readme
