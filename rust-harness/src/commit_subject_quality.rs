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

pub fn is_generic_subject(subject: &str) -> bool {
    let s = subject.trim().to_ascii_lowercase().replace('—', "-");
    s.contains("build a generalizable") && s.contains("harness: coding cycle")
}

pub fn has_informative_subject_scope(subject: &str, changed_files: &[&str]) -> bool {
    if is_generic_subject(subject) {
        return false;
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
        let stem = p
            .rsplit('/')
            .next()
            .and_then(|name| name.split('.').next())
            .unwrap_or("");
        let stem_norm = normalize_scope_token(stem);
        let dir_norm = normalize_scope_token(p.rsplit_once('/').map(|(dir, _)| dir).unwrap_or(""));

        scope == stem_norm
            || (!stem_norm.is_empty() && scope.contains(&stem_norm))
            || (!dir_norm.is_empty() && scope.contains(&dir_norm))
            || scope == "src"
            || scope == "code"
    })
}
