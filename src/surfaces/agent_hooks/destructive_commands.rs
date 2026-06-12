//! Built-in destructive-command guard: pure shell-command analysis that
//! blocks history-destroying Git invocations and forceful recursive deletes,
//! exposed both as plain checks and as the `guardDestructive` hook command.

use std::sync::LazyLock;

use regex::Regex;

use super::contract::{HookEvent, HookOutcome};
use super::hook_command::block_reason_outcome;

pub const DESTRUCTIVE_COMMAND_MERGE_HINT: &str =
    "git merge without --ff-only (use `git rebase` then `git merge --ff-only` for linear history)";

static GIT_FORCE_WITH_LEASE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git\s+push\s+--force-with-lease\b").unwrap());
static GIT_FORCE_LONG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git\s+push\s+--force").unwrap());
static GIT_FORCE_SHORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git\s+push\s+-f\b").unwrap());
static GIT_RESET_HARD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git\s+reset\s+--hard\b").unwrap());
static GIT_CLEAN_FORCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git\s+clean\s+-f").unwrap());
static GIT_CHECKOUT_DOT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git\s+checkout\s+\.$").unwrap());
static GIT_CHECKOUT_DASH_DOT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git\s+checkout\s+--\s+\.$").unwrap());
static GIT_RESTORE_DOT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git\s+restore\s+\.$").unwrap());
static GIT_BRANCH_DELETE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git\s+branch\s+-D\b").unwrap());
static GIT_STASH_DROP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git\s+stash\s+drop\b").unwrap());
static GIT_STASH_CLEAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git\s+stash\s+clear\b").unwrap());
static GIT_MERGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"git\s+merge\b").unwrap());
static FF_ONLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"--ff-only").unwrap());

const SHELL_EXECUTORS: &[&str] = &["bash", "dash", "fish", "ksh", "sh", "zsh"];

/// Returns the matched destructive-command label when `cmd` should be
/// blocked, scanning nested shell payloads and ignoring string literals.
pub fn check_destructive_command(cmd: &str) -> Option<String> {
    for payload in shell_payloads(cmd) {
        if let Some(payload_match) =
            check_destructive_command(&payload).or_else(|| check_destructive_merge(&payload))
        {
            return Some(payload_match);
        }
    }

    let sanitized = strip_string_literals(cmd);
    if let Some(rm_match) = check_rm(&tokenise(&sanitized)) {
        return Some(rm_match.to_owned());
    }

    if matches_git_push_force(&sanitized) {
        return Some("git push --force".to_owned());
    }

    for (pattern, label) in blocked_patterns() {
        if pattern.is_match(&sanitized) {
            return Some((*label).to_owned());
        }
    }
    None
}

/// Returns the merge hint when `cmd` runs `git merge` without `--ff-only`.
pub fn check_destructive_merge(cmd: &str) -> Option<String> {
    for payload in shell_payloads(cmd) {
        if let Some(payload_match) = check_destructive_merge(&payload) {
            return Some(payload_match);
        }
    }

    let sanitized = strip_string_literals(cmd);
    if !GIT_MERGE.is_match(&sanitized) {
        return None;
    }
    if FF_ONLY.is_match(&sanitized) {
        return None;
    }
    Some(DESTRUCTIVE_COMMAND_MERGE_HINT.to_owned())
}

/// Formats the agent-facing block reason for a destructive `tool_command`,
/// or `None` when the command is safe.
pub fn guard_destructive_command(tool_command: &str) -> Option<String> {
    check_destructive_command(tool_command)
        .or_else(|| check_destructive_merge(tool_command))
        .map(|matched| format!("Destructive command blocked: {matched}\nCommand: {tool_command}"))
}

/// The `guardDestructive` hook command: blocks destructive Bash commands and
/// rejects Bash events that carry no command string.
pub fn guard_destructive(event: &HookEvent) -> anyhow::Result<HookOutcome> {
    Ok(match event.tool_command.as_deref() {
        Some(command) => block_reason_outcome(guard_destructive_command(command)),
        None => HookOutcome::block(
            "Hook payload field tool_input.command must be a non-empty string for Bash.",
        ),
    })
}

fn strip_string_literals(cmd: &str) -> String {
    let without_heredocs = strip_heredocs(cmd);
    let mut stripped = String::with_capacity(without_heredocs.len());
    let mut chars = without_heredocs.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                stripped.push_str("\"\"");
                let mut escaping = false;
                for next in chars.by_ref() {
                    if escaping {
                        escaping = false;
                    } else if next == '\\' {
                        escaping = true;
                    } else if next == '"' {
                        break;
                    }
                }
            }
            '\'' => {
                stripped.push_str("''");
                for next in chars.by_ref() {
                    if next == '\'' {
                        break;
                    }
                }
            }
            _ => stripped.push(ch),
        }
    }

    stripped
}

fn strip_heredocs(cmd: &str) -> String {
    let Some(marker_start) = cmd.find("<<") else {
        return cmd.to_owned();
    };

    let bytes = cmd.as_bytes();
    let mut cursor = marker_start + 2;
    if bytes.get(cursor) == Some(&b'-') {
        cursor += 1;
    }
    while bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_whitespace() && *byte != b'\n')
    {
        cursor += 1;
    }
    if bytes.get(cursor) == Some(&b'\'') {
        cursor += 1;
    }
    let name_start = cursor;
    while bytes.get(cursor).is_some_and(u8::is_ascii_alphanumeric)
        || bytes.get(cursor) == Some(&b'_')
    {
        cursor += 1;
    }
    if name_start == cursor {
        return cmd.to_owned();
    }
    let marker = &cmd[name_start..cursor];
    let Some(body_start) = cmd[cursor..].find('\n').map(|offset| cursor + offset + 1) else {
        return cmd[..marker_start].to_owned();
    };

    let mut line_start = body_start;
    for line in cmd[body_start..].split_inclusive('\n') {
        let trimmed = line.trim();
        let line_end = line_start + line.len();
        if trimmed == marker {
            let mut output = String::with_capacity(cmd.len());
            output.push_str(&cmd[..marker_start]);
            output.push_str(&cmd[line_end..]);
            return strip_heredocs(&output);
        }
        line_start = line_end;
    }

    cmd[..marker_start].to_owned()
}

fn shell_words(cmd: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaping = false;

    for ch in cmd.chars() {
        if escaping {
            current.push(ch);
            escaping = false;
            continue;
        }
        if ch == '\\' {
            escaping = true;
            continue;
        }
        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            continue;
        }
        if ch.is_whitespace() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(ch);
    }

    if escaping {
        current.push('\\');
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn tokenise(cmd: &str) -> Vec<&str> {
    cmd.split_whitespace().collect()
}

fn executable_name(token: &str) -> &str {
    let normalized_split = token.rsplit(['/', '\\']).next();
    normalized_split.unwrap_or(token)
}

fn shell_payloads(cmd: &str) -> Vec<String> {
    let words = shell_words(cmd);
    let mut payloads = Vec::new();

    for index in 0..words.len().saturating_sub(1) {
        let executable = executable_name(&words[index]);
        if !SHELL_EXECUTORS.contains(&executable) {
            continue;
        }
        let flags = &words[index + 1];
        if (flags == "-c"
            || (flags.starts_with('-') && !flags.starts_with("--") && flags.contains('c')))
            && let Some(payload) = words.get(index + 2)
            && !payload.trim().is_empty()
        {
            payloads.push(payload.clone());
        }
    }

    payloads
}

fn check_rm(tokens: &[&str]) -> Option<&'static str> {
    if tokens.first().copied() != Some("rm") {
        return None;
    }

    let mut short_letters = String::new();
    let mut long_recursive = false;
    let mut long_force = false;
    let mut absolute_target = false;

    for token in &tokens[1..] {
        if token.starts_with("--") {
            if *token == "--recursive" {
                long_recursive = true;
            } else if *token == "--force" {
                long_force = true;
            }
        } else if is_short_flag_group(token) {
            short_letters.push_str(&token[1..]);
        } else if token.starts_with('/') {
            absolute_target = true;
        }
    }

    let recursive = short_letters.contains('r') || short_letters.contains('R') || long_recursive;
    let force = short_letters.contains('f') || long_force;

    if recursive && force {
        Some("rm recursive + force")
    } else if recursive && absolute_target {
        Some("rm recursive on absolute path")
    } else {
        None
    }
}

fn is_short_flag_group(token: &str) -> bool {
    let Some(rest) = token.strip_prefix('-') else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_alphabetic())
}

fn matches_git_push_force(cmd: &str) -> bool {
    GIT_FORCE_LONG.find_iter(cmd).any(|matched| {
        cmd[matched.end()..]
            .chars()
            .next()
            .is_none_or(|next| next != '-')
    })
}

fn blocked_patterns() -> [(&'static Regex, &'static str); 10] {
    [
        (&GIT_FORCE_WITH_LEASE, "git push --force-with-lease"),
        (&GIT_FORCE_SHORT, "git push -f"),
        (&GIT_RESET_HARD, "git reset --hard"),
        (&GIT_CLEAN_FORCE, "git clean -f"),
        (&GIT_CHECKOUT_DOT, "git checkout ."),
        (&GIT_CHECKOUT_DASH_DOT, "git checkout -- ."),
        (&GIT_RESTORE_DOT, "git restore ."),
        (&GIT_BRANCH_DELETE, "git branch -D"),
        (&GIT_STASH_DROP, "git stash drop"),
        (&GIT_STASH_CLEAR, "git stash clear"),
    ]
}

#[cfg(test)]
mod tests {
    use super::super::contract::HookStage;
    use super::*;

    fn bash_event(tool_command: Option<&str>) -> HookEvent {
        HookEvent {
            harness: "codex".to_owned(),
            stage: HookStage::PreTool,
            session_id: "session".to_owned(),
            tool_call_id: Some("tool".to_owned()),
            transcript_path: None,
            cwd: "/repo".to_owned(),
            touched_path_candidates: Vec::new(),
            patch_text: None,
            tool_command: tool_command.map(str::to_owned),
            tool_name: Some("Bash".to_owned()),
            tool_response: None,
            stop_hook_active: false,
        }
    }

    #[test]
    fn blocks_rm_recursive_force_regardless_of_flag_order() {
        assert_eq!(
            check_destructive_command("rm -rf tmp").as_deref(),
            Some("rm recursive + force")
        );
        assert_eq!(
            check_destructive_command("rm -fr tmp").as_deref(),
            Some("rm recursive + force")
        );
        assert_eq!(
            check_destructive_command("rm --recursive --force tmp").as_deref(),
            Some("rm recursive + force")
        );
    }

    #[test]
    fn blocks_rm_recursive_on_absolute_paths_only() {
        assert_eq!(
            check_destructive_command("rm -r /tmp/scratch").as_deref(),
            Some("rm recursive on absolute path")
        );
        assert_eq!(check_destructive_command("rm -r tmp"), None);
        assert_eq!(check_destructive_command("rm file.txt"), None);
    }

    #[test]
    fn blocks_nested_shell_payloads() {
        assert_eq!(
            check_destructive_command("bash -lc 'git reset --hard'").as_deref(),
            Some("git reset --hard")
        );
        assert_eq!(
            check_destructive_command("sh -c \"git clean -fd\"").as_deref(),
            Some("git clean -f")
        );
    }

    #[test]
    fn ignores_commands_inside_string_literals_and_heredocs() {
        assert_eq!(check_destructive_command("printf 'git reset --hard'"), None);
        assert_eq!(
            check_destructive_command("cat <<EOF\ngit push --force\nEOF\necho done"),
            None
        );
    }

    #[test]
    fn preserves_force_negative_lookahead_semantics() {
        assert_eq!(
            check_destructive_command("git push --force; echo done").as_deref(),
            Some("git push --force")
        );
        assert_eq!(
            check_destructive_command("git push --force-with-lease").as_deref(),
            Some("git push --force-with-lease")
        );
        assert_eq!(
            check_destructive_command("git push --force-if-includes"),
            None
        );
    }

    #[test]
    fn blocks_merge_without_ff_only() {
        assert_eq!(
            check_destructive_merge("git merge main").as_deref(),
            Some(DESTRUCTIVE_COMMAND_MERGE_HINT)
        );
        assert_eq!(check_destructive_merge("git merge --ff-only main"), None);
        assert_eq!(check_destructive_merge("cargo build"), None);
    }

    #[test]
    fn guard_formats_block_reason_with_command_context() {
        let reason = guard_destructive_command("git reset --hard HEAD")
            .expect("git reset --hard must be blocked");

        assert_eq!(
            reason,
            "Destructive command blocked: git reset --hard\nCommand: git reset --hard HEAD"
        );
        assert_eq!(guard_destructive_command("pwd"), None);
    }

    #[test]
    fn guard_includes_merge_hint_for_non_ff_merges() {
        let reason =
            guard_destructive_command("git merge main").expect("git merge must be blocked");

        assert!(reason.contains(DESTRUCTIVE_COMMAND_MERGE_HINT));
    }

    #[test]
    fn hook_command_blocks_destructive_and_passes_safe_events() {
        assert!(matches!(
            guard_destructive(&bash_event(Some("git reset --hard"))).unwrap(),
            HookOutcome::Block { ref reason, .. } if reason.contains("git reset --hard")
        ));
        assert!(matches!(
            guard_destructive(&bash_event(Some("pwd"))).unwrap(),
            HookOutcome::Pass { .. }
        ));
    }

    #[test]
    fn hook_command_blocks_bash_events_without_command_string() {
        assert!(matches!(
            guard_destructive(&bash_event(None)).unwrap(),
            HookOutcome::Block { ref reason, .. }
                if reason == "Hook payload field tool_input.command must be a non-empty string for Bash."
        ));
    }
}
