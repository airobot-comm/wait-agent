//! Cross-platform detached child process creation.

use std::io;
use std::process::{Child, Command};

/// One-shot marker environment variable set on the daemonized copy of a
/// Windows node server so the copy runs the normal server logic instead of
/// forking again.
#[cfg(windows)]
pub const DAEMONIZED_ENV: &str = "WAITAGENT_DAEMONIZED";

/// Decides whether the current process should self-daemonize.
///
/// A Windows `__ratatui-node-server` launched through an SSH exec session has
/// redirected stdio (and often no console at all, which also reports as "not
/// a terminal"), so the tty checks cover both cases from the design. The
/// marker check prevents the daemonized copy from forking a second time.
#[cfg(any(windows, test))]
pub fn needs_daemonize(stdin_is_tty: bool, stdout_is_tty: bool, marker_present: bool) -> bool {
    !marker_present && (!stdin_is_tty || !stdout_is_tty)
}

/// Poll `127.0.0.1:port` until a TCP connect succeeds or `timeout` elapses,
/// sleeping `interval` between attempts. Returns true once the port accepts
/// connections.
#[cfg(any(windows, test))]
pub fn wait_for_local_port_ready(
    port: u16,
    timeout: std::time::Duration,
    interval: std::time::Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(interval);
    }
}

/// Spawn `command` as a detached child process so it survives the parent's
/// terminal/session exit.
#[cfg(unix)]
pub fn spawn_detached(command: &mut Command) -> io::Result<Child> {
    use std::os::unix::process::CommandExt;

    // SAFETY: `pre_exec` runs in the child between fork and exec. We only call
    // the async-signal-safe `libc::setsid` and propagate the error as the spawn
    // error if it fails.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn()
}

/// Windows: spawn the command in its own process group without a console so
/// it survives the parent's terminal exit.
#[cfg(windows)]
pub fn spawn_detached(command: &mut Command) -> io::Result<Child> {
    use std::process::Stdio;

    // A detached process cannot inherit any console handles. Force all stdio
    // to null so the spawn succeeds even if the caller left one as `inherit`.
    spawn_detached_with_stdio(command, Stdio::null(), Stdio::null(), Stdio::null())
}

/// Windows: like [`spawn_detached`], but with caller-provided stdio so the
/// detached child can log to a file instead of the null device.
///
/// `CREATE_BREAKAWAY_FROM_JOB` is required for the OpenSSH-for-Windows
/// scenario: sshd places each exec session in a kill-on-close Job Object, and
/// only a child created with this flag escapes that job (provided the job
/// permits breakaway, as the Win32-OpenSSH session job does).
#[cfg(windows)]
pub fn spawn_detached_with_stdio(
    command: &mut Command,
    stdin: std::process::Stdio,
    stdout: std::process::Stdio,
    stderr: std::process::Stdio,
) -> io::Result<Child> {
    use std::os::windows::process::CommandExt;

    command
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr)
        .creation_flags(
            windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP
                | windows_sys::Win32::System::Threading::DETACHED_PROCESS
                | windows_sys::Win32::System::Threading::CREATE_BREAKAWAY_FROM_JOB,
        )
        .spawn()
}

/// Windows self-daemonization for `__ratatui-node-server`.
///
/// When the process was launched from an SSH exec session (stdio redirected),
/// re-launch the current executable as a detached copy with
/// [`DAEMONIZED_ENV`] set and stdio redirected to `%TEMP%\waitagent-<port>.log`,
/// then poll the server port. The parent process exits here: status 0 once
/// the port accepts connections, 1 on timeout, so the SSH exec channel
/// reports bootstrap success or failure. When no fork is needed (interactive
/// launch, or the daemonized copy itself), this returns and the caller runs
/// the normal server logic.
///
/// This function never returns in the forking parent.
#[cfg(windows)]
pub fn daemonize_self_if_needed(port: u16) -> io::Result<()> {
    use std::io::IsTerminal;
    use std::process::Stdio;

    let marker_present = std::env::var_os(DAEMONIZED_ENV).is_some();
    if !needs_daemonize(
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
        marker_present,
    ) {
        return Ok(());
    }

    let temp = std::env::var_os("TEMP").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "TEMP environment variable is not set; cannot place the daemon log",
        )
    })?;
    let log_path = std::path::Path::new(&temp).join(format!("waitagent-{port}.log"));
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let log_stderr = log.try_clone()?;

    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(std::env::args_os().skip(1))
        .env(DAEMONIZED_ENV, "1");
    // The `Child` handle is dropped without waiting: the detached copy is
    // re-parented and outlives this process by design.
    spawn_detached_with_stdio(
        &mut command,
        Stdio::null(),
        Stdio::from(log),
        Stdio::from(log_stderr),
    )?;

    let ready = wait_for_local_port_ready(
        port,
        std::time::Duration::from_secs(10),
        std::time::Duration::from_millis(200),
    );
    // The stdio of the forking parent belongs to the SSH session, which
    // sshd tears down when this process exits; nothing buffered needs a flush.
    std::process::exit(if ready { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn daemonizes_when_stdin_redirected() {
        assert!(needs_daemonize(false, true, false));
    }

    #[test]
    fn daemonizes_when_stdout_redirected() {
        assert!(needs_daemonize(true, false, false));
    }

    #[test]
    fn daemonizes_when_both_redirected() {
        assert!(needs_daemonize(false, false, false));
    }

    #[test]
    fn runs_normally_when_interactive() {
        assert!(!needs_daemonize(true, true, false));
    }

    #[test]
    fn daemonized_copy_does_not_fork_again() {
        assert!(!needs_daemonize(false, false, true));
    }

    #[test]
    fn port_ready_succeeds_once_listener_accepts() {
        let listener =
            std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback test listener");
        let port = listener.local_addr().expect("listener local addr").port();
        assert!(wait_for_local_port_ready(
            port,
            Duration::from_secs(5),
            Duration::from_millis(20)
        ));
    }

    #[test]
    fn port_ready_times_out_when_nothing_listens() {
        // Port 1 is reserved and nothing on loopback listens there, so every
        // connect is refused and the full timeout elapses.
        assert!(!wait_for_local_port_ready(
            1,
            Duration::from_millis(150),
            Duration::from_millis(25)
        ));
    }
}
