use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::process::Command;

#[derive(Debug, Clone)]
struct Config {
    repo: PathBuf,
    minutes: f64,
    goal: String,
    session_prefix: String,
    generator_timeout_sec: u64,
    discriminator_timeout_sec: u64,
    commit_every: u64,
}

fn parse_args() -> Result<Config> {
    let mut repo = PathBuf::from(".");
    let mut minutes = 5.0;
    let mut goal: Option<String> = None;
    let mut session_prefix = "harness".to_string();
    let mut generator_timeout_sec = 600u64;
    let mut discriminator_timeout_sec = 600u64;
    let mut commit_every = 1u64;

    let mut args = std::env::args().skip(1);
    let mut saw_start = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "start" => saw_start = true,
            "--repo" => {
                repo = PathBuf::from(args.next().ok_or_else(|| anyhow!("--repo needs value"))?)
            }
            "--time" | "--minutes" => {
                let raw = args.next().ok_or_else(|| anyhow!("--time needs value"))?;
                minutes = parse_minutes(&raw)?;
            }
            "--goal" | "--prompt" => {
                goal = Some(args.next().ok_or_else(|| anyhow!("--goal needs value"))?)
            }
            "--session-prefix" => {
                session_prefix = args
                    .next()
                    .ok_or_else(|| anyhow!("--session-prefix needs value"))?
            }
            "--generator-timeout" => {
                generator_timeout_sec = args
                    .next()
                    .ok_or_else(|| anyhow!("--generator-timeout needs value"))?
                    .parse()?;
            }
            "--discriminator-timeout" => {
                discriminator_timeout_sec = args
                    .next()
                    .ok_or_else(|| anyhow!("--discriminator-timeout needs value"))?
                    .parse()?;
            }
            "--commit-every" => {
                commit_every = args
                    .next()
                    .ok_or_else(|| anyhow!("--commit-every needs value"))?
                    .parse()?
            }
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(anyhow!("unknown arg: {other}")),
        }
    }

    if !saw_start {
        return Err(anyhow!(
            "usage: cargo run -- start --goal \"...\" [--time 5m]"
        ));
    }

    Ok(Config {
        repo,
        minutes,
        goal: goal.ok_or_else(|| anyhow!("--goal is required"))?,
        session_prefix,
        generator_timeout_sec,
        discriminator_timeout_sec,
        commit_every,
    })
}

fn print_help() {
    println!(
        "rust-harness\n\nUSAGE:\n  cargo run -- start --goal \"...\" [--time 5m]\n\nFLAGS:\n  --repo <path>\n  --time <5m|300s|minutes>\n  --goal <text>\n  --session-prefix <id>\n  --generator-timeout <sec>\n  --discriminator-timeout <sec>\n  --commit-every <cycles>\n"
    );
}

fn parse_minutes(raw: &str) -> Result<f64> {
    if let Some(v) = raw.strip_suffix('m') {
        return Ok(v.parse()?);
    }
    if let Some(v) = raw.strip_suffix('s') {
        let sec: f64 = v.parse()?;
        return Ok(sec / 60.0);
    }
    Ok(raw.parse()?)
}

async fn run_openclaw(session_id: &str, prompt: &str, timeout_sec: u64) -> Result<String> {
    let out = Command::new("openclaw")
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
        .context("failed to run openclaw")?;

    if !out.status.success() {
        return Err(anyhow!(
            "openclaw returned {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if let Ok(v) = serde_json::from_str::<Value>(&stdout) {
        if let Some(text) = v
            .get("payloads")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|x| x.get("text"))
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
    let prompt_log_path = repo.join(format!("runs/discriminator-prompts-{}.md", now));

    println!("start repo={} goal={}", repo.display(), cfg.goal);
    println!("generator={} discriminator={}", gen_session, disc_session);

    let mut cycle = 0u64;
    let mut user_prompt = cfg.goal.clone();

    while SystemTime::now() < deadline {
        cycle += 1;
        println!("\n=== cycle {} ===", cycle);

        let generator_prompt = format!(
            "You are the coding generator. Goal: {}\n\nCurrent instruction: {}\n\nIMPORTANT: actually edit files in this repository now. Do not just describe changes. Use your available tools to write real code to disk, then briefly summarize what you changed.",
            cfg.goal, user_prompt
        );

        let gen_out =
            match run_openclaw(&gen_session, &generator_prompt, cfg.generator_timeout_sec).await {
                Ok(v) => v,
                Err(err) => {
                    println!("generator_error: {err}");
                    continue;
                }
            };
        println!(
            "generator_out: {}",
            gen_out
                .replace('\n', " ")
                .chars()
                .take(220)
                .collect::<String>()
        );

        let discriminator_prompt = format!(
            "You are the discriminator. Original human goal: {}\n\nGenerator response:\n{}\n\nReturn strict JSON: {{\"score\":0-100,\"next_prompt\":\"...\",\"commit_now\":true/false}}",
            cfg.goal, gen_out
        );

        if let Some(parent) = prompt_log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&prompt_log_path) {
            let _ = writeln!(f, "\n## cycle {}\n\n{}\n", cycle, discriminator_prompt);
        }

        let mut score = 0i64;
        let mut next_prompt = user_prompt.clone();
        let mut commit_now = cfg.commit_every > 0 && cycle % cfg.commit_every == 0;

        if let Ok(disc_out) = run_openclaw(
            &disc_session,
            &discriminator_prompt,
            cfg.discriminator_timeout_sec,
        )
        .await
        {
            println!(
                "discriminator_out: {}",
                disc_out
                    .replace('\n', " ")
                    .chars()
                    .take(220)
                    .collect::<String>()
            );
            if let Ok(v) = serde_json::from_str::<Value>(&disc_out) {
                score = v.get("score").and_then(Value::as_i64).unwrap_or(0);
                if let Some(np) = v.get("next_prompt").and_then(Value::as_str) {
                    if !np.trim().is_empty() {
                        next_prompt = np.to_string();
                    }
                }
                if let Some(cn) = v.get("commit_now").and_then(Value::as_bool) {
                    commit_now = cn;
                }
            }
        }

        if commit_now || score >= 70 {
            let msg = format!("feat(loop): cycle {} generator/discriminator", cycle);
            let (ok, out) = commit_push(&repo, &msg).await?;
            println!(
                "commit_push={} msg={}",
                ok,
                out.replace('\n', " ").chars().take(180).collect::<String>()
            );
        }

        user_prompt = next_prompt;
    }

    println!("done");
    Ok(())
}
