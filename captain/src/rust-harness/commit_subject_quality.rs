pub fn normalize_scope_token(input: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;

    for ch in input.trim().to_ascii_lowercase().chars() {
        let mapped = if ch.is_ascii_alphanumeric() { ch } else { '-' };
        if mapped == '-' {
            if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        } else {
            out.push(mapped);
            prev_dash = false;
        }
    }

    out.trim_matches('-').to_string()
}

pub fn normalize_subject_text(subject: &str) -> String {
    subject
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_file_stem_scope(path: &str) -> String {
    let stem = path
        .rsplit('/')
        .next()
        .and_then(|name| name.split('.').next())
        .unwrap_or("");
    normalize_scope_token(stem)
}

pub fn is_generic_subject(subject: &str) -> bool {
    let normalized = normalize_subject_text(subject);
    if normalized.is_empty() {
        return true;
    }

    let blocked_patterns = [
        "generalizable",
        "build a generalizable",
        "harness coding cycle",
        "coding cycle",
        "advance harness workflow",
        "update code",
        "update files",
        "misc updates",
        "minor fixes",
        "work in progress",
        "changes",
    ];

    normalized == "wip"
        || blocked_patterns
            .iter()
            .any(|pattern| normalized.contains(pattern))
}

fn path_has_src_component(path: &str) -> bool {
    path == "src" || path.starts_with("src/") || path.contains("/src/")
}

pub fn deterministic_subject_from_files(files: &[String]) -> String {
    let mut names = files.to_vec();
    names.sort_by(|a, b| {
        let a_src = path_has_src_component(a);
        let b_src = path_has_src_component(b);
        b_src.cmp(&a_src).then_with(|| a.cmp(b))
    });
    names.dedup();

    let top = names
        .iter()
        .take(2)
        .map(|f| f.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let intent = if names.iter().any(|f| path_has_src_component(f)) {
        "implement scoped code updates"
    } else if names
        .iter()
        .any(|f| f.ends_with("README.md") || f.ends_with("RUNBOOK.md"))
    {
        "document operator workflow changes"
    } else if names
        .iter()
        .any(|f| f.contains("test") || f.contains("fixtures/"))
    {
        "add regression coverage"
    } else {
        "update harness workflow"
    };

    if top.is_empty() {
        format!("{intent} in tracked files")
    } else {
        format!("{intent} in {top}")
    }
}

pub fn has_informative_subject_scope(subject: &str, changed_files: &[&str]) -> bool {
    if is_generic_subject(subject) {
        return false;
    }

    let normalized_subject = normalize_subject_text(subject);
    if normalized_subject.is_empty() || changed_files.is_empty() {
        return false;
    }

    if changed_files
        .iter()
        .flat_map(|file| scope_tokens_from_file(file))
        .any(|token| normalized_subject.contains(&token))
    {
        return true;
    }

    let Some(open_idx) = subject.find('(') else {
        return false;
    };
    let Some(close_idx) = subject[open_idx + 1..].find(')') else {
        return false;
    };
    let close_idx = open_idx + 1 + close_idx;

    let scope_raw = &subject[open_idx + 1..close_idx];
    let scope = normalize_scope_token(scope_raw);
    if scope.is_empty() {
        return false;
    }

    changed_files.iter().any(|path| {
        let p = path.to_ascii_lowercase();
        let stem_norm = normalize_file_stem_scope(&p);
        let dir_norm = normalize_scope_token(p.rsplit_once('/').map(|(dir, _)| dir).unwrap_or(""));

        scope == stem_norm
            || (!stem_norm.is_empty() && scope.contains(&stem_norm))
            || (!dir_norm.is_empty() && scope.contains(&dir_norm))
            || scope == "src"
            || scope == "code"
    })
}

fn scope_tokens_from_file(file: &str) -> Vec<String> {
    let normalized = file
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>();

    normalized
        .split_whitespace()
        .filter(|token| token.len() >= 3)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>()
}
