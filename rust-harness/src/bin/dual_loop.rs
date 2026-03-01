use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::process::Command;

#[derive(Debug, Clone)]
struct Config {
    repo: PathBuf,
    minutes: f64,
    prompt: String,
    max_cycles: u64,
    session_prefix: String,
    timeout_sec: u64,
}

fn parse_args() -> Result<Config> {
    let mut repo = PathBuf::from(".");
    let mut minutes = 5.0;
    let mut prompt: Option<String> = None;
    let mut max_cycles = 8u64;
    let mut session_prefix = "dual-loop".to_string();
    let mut timeout_sec = 180u64;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repo" => repo = PathBuf::from(args.next().ok_or_else(|| anyhow!("--repo needs value"))?),
            "--minutes" => minutes = args.next().ok_or_else(|| anyhow!("--minutes needs value"))?.parse()?,
            "--prompt" => prompt = Some(args.next().ok_or_else(|| anyhow!("--prompt needs value"))?),
            "--max-cycles" => max_cycles = args.next().ok_or_else(|| anyhow!("--max-cycles needs value"))?.parse()?,
            "--session-prefix" => session_prefix = args.next().ok_or_else(|| anyhow!("--session-prefix needs value"))?,
            "--timeout" => timeout_sec = args.next().ok_or_else(|| anyhow!("--timeout needs value"))?.parse()?,
            other => return Err(anyhow!("unknown arg: {other}")),
        }
    }

    Ok(Config {
        repo,
        minutes,
        prompt: prompt.ok_or_else(|| anyhow!("--prompt is required"))?,
        max_cycles,
        session_prefix,
        timeout_sec,
    })
}

async fn run_openclaw(session_id: &str, prompt: &str, timeout_sec: u64) -> Result<String> {
    let output = Command::new("openclaw")
        .arg("agent")
        .arg("--local")
        .arg("--agent")
        .arg("main")
        .arg("--session-id")
        .arg(session_id)
        .arg("--timeout")
        .arg(timeout_sec.to_string())
        .arg("--thinking")
        .arg("low")
        .arg("--json")
        .arg("--message")
        .arg(prompt)
        .output()
        .await
        .context("failed to execute openclaw")?;

    if !output.status.success() {
        return Err(anyhow!(
            "openclaw failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if let Ok(v) = serde_json::from_str::<Value>(&stdout) {
        if let Some(text) = v
            .get("payloads")
            .and_then(Value::as_array)
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("text"))
            .and_then(Value::as_str)
        {
            return Ok(text.trim().to_string());
        }
    }

    Ok(stdout.trim().to_string())
}

async fn run_cmd(repo: &Path, argv: &[&str]) -> Result<(bool, String)> {
    let mut cmd = Command::new(argv[0]);
    cmd.args(&argv[1..]).current_dir(repo);
    let out = cmd.output().await?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok((out.status.success(), text))
}

async fn verify(repo: &Path) -> Result<(bool, String)> {
    for argv in [
        ["cargo", "fmt", "--all"].as_slice(),
        ["cargo", "check", "--all-targets"].as_slice(),
        ["cargo", "test", "--all-targets"].as_slice(),
    ] {
        let (ok, out) = run_cmd(repo, argv).await?;
        if !ok {
            return Ok((false, out));
        }
    }
    Ok((true, "verify ok".to_string()))
}

async fn commit_push(repo: &Path, msg: &str) -> Result<(bool, String)> {
    let (ok, out) = run_cmd(repo, &["git", "add", "-A", "."]).await?;
    if !ok {
        return Ok((false, out));
    }

    let (ok, staged) = run_cmd(repo, &["git", "diff", "--cached", "--name-only"]).await?;
    if !ok || staged.trim().is_empty() {
        return Ok((false, "no staged changes".to_string()));
    }

    let (ok, out) = run_cmd(repo, &["git", "commit", "-m", msg]).await?;
    if !ok {
        return Ok((false, out));
    }

    let (ok, out) = run_cmd(repo, &["git", "push", "origin", "HEAD"]).await?;
    Ok((ok, out))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = parse_args()?;
    let repo = cfg.repo.canonicalize().unwrap_or(cfg.repo);

    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let gen_session = format!("{}-gen-{}", cfg.session_prefix, now);
    let disc_session = format!("{}-disc-{}", cfg.session_prefix, now);
    let deadline = SystemTime::now() + Duration::from_secs((cfg.minutes * 60.0) as u64);

    println!("start repo={}", repo.display());
    println!("generator={gen_session} discriminator={disc_session}");

    let mut user_prompt = cfg.prompt.clone();
    let mut cycle = 0u64;

    while SystemTime::now() < deadline && cycle < cfg.max_cycles {
        cycle += 1;
        println!("\n=== cycle {} ===", cycle);

        let gen_prompt = format!(
            "You are the coding generator. Return STRICT JSON with keys: rationale, acceptance_checks, edits. Keep changes scoped and high quality. Objective: {}",
            user_prompt
        );

        let gen_out = match run_openclaw(&gen_session, &gen_prompt, cfg.timeout_sec).await {
            Ok(v) => v,
            Err(err) => {
                println!("generator_error: {err}");
                continue;
            }
        };
        println!("generator_out_preview: {}", gen_out.replace('\n', " ").chars().take(280).collect::<String>());

        let disc_prompt = format!(
            "You are a strict discriminator acting as the user. Evaluate this generator output and return JSON with quality_score (0-100), issues (array), next_prompt (string).\n\nGenerator output:\n{}",
            gen_out
        );

        let mut quality = 0i64;
        let mut next_prompt = user_prompt.clone();
        if let Ok(disc_out) = run_openclaw(&disc_session, &disc_prompt, cfg.timeout_sec).await {
            println!("discriminator_out_preview: {}", disc_out.replace('\n', " ").chars().take(280).collect::<String>());
            if let Ok(v) = serde_json::from_str::<Value>(&disc_out) {
                quality = v.get("quality_score").and_then(Value::as_i64).unwrap_or(0);
                if let Some(np) = v.get("next_prompt").and_then(Value::as_str) {
                    if !np.trim().is_empty() {
                        next_prompt = np.to_string();
                    }
                }
            }
        }

        let (ok, verify_msg) = verify(&repo).await?;
        println!("verify: {}", ok);
        if !ok {
            println!("verify_error: {}", verify_msg.chars().take(300).collect::<String>());
        }

        if ok && quality >= 60 {
            let msg = format!("feat(loop): dual-loop quality cycle {}", cycle);
            let (pushed, push_msg) = commit_push(&repo, &msg).await?;
            println!("commit_push: {}", pushed);
            if !pushed {
                println!("commit_push_msg: {}", push_msg.chars().take(300).collect::<String>());
            }
            if pushed {
                user_prompt = format!(
                    "Continue from latest successful commit. Improve robustness/tests. Prev objective: {}",
                    next_prompt
                );
                continue;
            }
        }

        user_prompt = next_prompt;
    }

    println!("done");
    Ok(())
}
