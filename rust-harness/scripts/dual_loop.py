#!/usr/bin/env python3
import argparse
import json
import subprocess
import time
from pathlib import Path


def run_openclaw(session_id: str, prompt: str, timeout_sec: int) -> str:
    cmd = [
        "openclaw",
        "agent",
        "--local",
        "--agent",
        "main",
        "--session-id",
        session_id,
        "--timeout",
        str(timeout_sec),
        "--thinking",
        "low",
        "--json",
        "--message",
        prompt,
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(proc.stderr.strip() or f"openclaw failed: {proc.returncode}")

    text = proc.stdout.strip()
    try:
        payload = json.loads(text)
        payloads = payload.get("payloads") or []
        if payloads and isinstance(payloads[0], dict):
            return (payloads[0].get("text") or "").strip()
    except Exception:
        pass
    return text


def run_verify(repo: Path) -> tuple[bool, str]:
    checks = [
        ["cargo", "fmt", "--all"],
        ["cargo", "check", "--all-targets"],
        ["cargo", "test", "--all-targets"],
    ]
    for cmd in checks:
        proc = subprocess.run(cmd, cwd=repo, capture_output=True, text=True)
        if proc.returncode != 0:
            return False, (proc.stderr or proc.stdout)[-1500:]
    return True, "verify ok"


def commit_push(repo: Path, msg: str) -> tuple[bool, str]:
    add = subprocess.run(["git", "add", "-A", "."], cwd=repo, capture_output=True, text=True)
    if add.returncode != 0:
        return False, add.stderr.strip()

    diff = subprocess.run(["git", "diff", "--cached", "--name-only"], cwd=repo, capture_output=True, text=True)
    if not diff.stdout.strip():
        return False, "no staged changes"

    commit = subprocess.run(["git", "commit", "-m", msg], cwd=repo, capture_output=True, text=True)
    if commit.returncode != 0:
        return False, commit.stderr.strip() or commit.stdout.strip()

    push = subprocess.run(["git", "push", "origin", "HEAD"], cwd=repo, capture_output=True, text=True)
    if push.returncode != 0:
        return False, push.stderr.strip() or push.stdout.strip()

    return True, "commit+push ok"


def main():
    ap = argparse.ArgumentParser(description="Simple generator/discriminator loop using two OpenClaw sessions")
    ap.add_argument("--repo", default=".")
    ap.add_argument("--minutes", type=float, default=5)
    ap.add_argument("--prompt", required=True)
    ap.add_argument("--max-cycles", type=int, default=8)
    ap.add_argument("--session-prefix", default="dual-loop")
    ap.add_argument("--timeout", type=int, default=180)
    args = ap.parse_args()

    repo = Path(args.repo).resolve()
    deadline = time.time() + args.minutes * 60

    gen_session = f"{args.session_prefix}-gen-{int(time.time())}"
    disc_session = f"{args.session_prefix}-disc-{int(time.time())}"

    user_prompt = args.prompt
    cycle = 0

    print(f"start repo={repo} deadline={int(deadline)}")
    print(f"generator={gen_session} discriminator={disc_session}")

    while time.time() < deadline and cycle < args.max_cycles:
        cycle += 1
        print(f"\n=== cycle {cycle} ===")

        gen_prompt = (
            "You are the coding generator. Return STRICT JSON with keys: rationale, acceptance_checks, edits. "
            "Edits should be high-quality Rust repo changes scoped to existing files when possible. "
            f"Current objective: {user_prompt}"
        )

        try:
            gen_out = run_openclaw(gen_session, gen_prompt, args.timeout)
            print("generator_out_preview:", gen_out[:280].replace("\n", " "))
        except Exception as e:
            print("generator_error:", e)
            continue

        # Ask discriminator to critique and craft the next user prompt.
        disc_prompt = (
            "You are a strict discriminator acting as the user. Evaluate this generator output for code quality, "
            "specificity, and likely verifiability. Return JSON with: quality_score (0-100), issues (array), next_prompt (string).\n\n"
            f"Generator output:\n{gen_out}"
        )

        quality = 0
        next_prompt = user_prompt
        try:
            disc_out = run_openclaw(disc_session, disc_prompt, args.timeout)
            print("discriminator_out_preview:", disc_out[:280].replace("\n", " "))
            maybe = json.loads(disc_out)
            quality = int(maybe.get("quality_score", 0))
            next_prompt = maybe.get("next_prompt") or user_prompt
        except Exception:
            # Keep going even if discriminator format is imperfect.
            quality = 0

        ok, verify_msg = run_verify(repo)
        print("verify:", ok, verify_msg[:240].replace("\n", " "))

        if ok and quality >= 60:
            commit_msg = f"feat(loop): dual-loop quality cycle {cycle}"
            pushed, push_msg = commit_push(repo, commit_msg)
            print("commit_push:", pushed, push_msg[:240])
            if pushed:
                # tighten objective after successful ship
                user_prompt = f"Continue from last successful commit; improve tests and robustness. Previous objective: {next_prompt}"
                continue

        user_prompt = next_prompt

    print("done")


if __name__ == "__main__":
    main()
