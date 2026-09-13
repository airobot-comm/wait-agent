//! Best-effort provisioning of a compact one-line prompt into the operator's
//! `~/.bashrc` so shells spawned by waitagent (local panes and sessions
//! hosted for remote viewers alike) render `(.venv) <path> (branch)$` on a
//! single line instead of Git Bash's two-line `user@host MSYSTEM <path>`
//! default, which wraps as soon as a venv prefix is prepended.
//!
//! The provisioning is idempotent (marker-guarded) and respectful: it never
//! touches a dotfile that already carries the managed block or the operator's
//! own `PS1` assignment. All failures are the caller's choice; the provided
//! entry point is best-effort and never fatal.

#[cfg(windows)]
use std::fs::OpenOptions;
#[cfg(windows)]
use std::io::Write;
#[cfg(windows)]
use std::path::PathBuf;

/// Marks the start of the waitagent-managed block inside `.bashrc`.
/// Its presence is the idempotency guard for provisioning.
#[cfg(windows)]
const BLOCK_BEGIN: &str = "# >>> waitagent compact bash prompt >>>";

/// The managed block appended to `.bashrc`. Kept self-contained: the git
/// branch helper is defined right here so the prompt works in non-login
/// interactive shells too, where Git for Windows' `__git_ps1` is not loaded.
#[cfg(windows)]
const MANAGED_BLOCK: &str = r#"
# >>> waitagent compact bash prompt >>>
# Managed by waitagent. A compact one-line prompt so venv prefixes like
# "(.venv) " fit without wrapping. Remove this block to revert.
waitagent_git_ps1_branch() {
    command -v __git_ps1 >/dev/null 2>&1 && __git_ps1 " (%s)"
}
PS1='\[\033[32m\]\w\[\033[0m\]\[\033[35m\]$(waitagent_git_ps1_branch)\[\033[0m\]\$ '
# <<< waitagent compact bash prompt <<<
"#;

/// Decide whether `bashrc_content` should receive the managed block.
///
/// Returns `false` when the block is already present (idempotency) or when
/// the operator assigned their own `PS1` anywhere in the file (respecting
/// user configuration wins over provisioning). Commented-out assignments do
/// not count.
#[cfg(windows)]
fn needs_managed_block(bashrc_content: &str) -> bool {
    if bashrc_content.contains(BLOCK_BEGIN) {
        return false;
    }
    !bashrc_content.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("PS1=")
    })
}

/// Resolve the dotfile to provision: `$HOME/.bashrc`, falling back to
/// `%USERPROFILE%\.bashrc` (Git Bash exports `HOME`, the native node server
/// process on Windows may only have `USERPROFILE`).
#[cfg(windows)]
fn bashrc_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .map(|home| home.join(".bashrc"))
}

/// Ensure the operator's `~/.bashrc` contains the compact prompt block.
///
/// Idempotent and side-effect-free when the file already carries the block or
/// an operator-defined `PS1`. No-op on non-Windows platforms, where
/// distribution prompt conventions do not need fixing.
pub fn ensure_bashrc_compact_prompt() -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let Some(path) = bashrc_path() else {
            return Ok(());
        };
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        if !needs_managed_block(&content) {
            return Ok(());
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        file.write_all(MANAGED_BLOCK.as_bytes())?;
    }
    #[cfg(not(windows))]
    let _ = ();
    Ok(())
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    #[test]
    fn needs_block_for_plain_bashrc() {
        assert!(needs_managed_block("export PATH=$PATH:/somewhere\n"));
    }

    #[test]
    fn skips_when_block_present() {
        let content = format!("{MANAGED_BLOCK}\nPS1='custom'\n");
        assert!(!needs_managed_block(&content));
    }

    #[test]
    fn skips_when_operator_assigned_ps1() {
        assert!(!needs_managed_block("PS1='custom$ '\n"));
        assert!(!needs_managed_block("  export FOO=1\n\tPS1=\"x\"\n"));
    }

    #[test]
    fn commented_out_ps1_still_gets_block() {
        assert!(needs_managed_block("# PS1='ignored'\n"));
    }
}
