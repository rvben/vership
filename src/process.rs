use std::io;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;

/// Run a child while keeping the caller's stdout machine-readable.
///
/// Child stdout is streamed to the caller's stderr, and child stderr is
/// inherited directly. This preserves live diagnostics without buffering
/// unbounded logs or contaminating structured stdout.
pub(crate) fn status_with_stdout_to_stderr(command: &mut Command) -> io::Result<ExitStatus> {
    command.stdout(Stdio::piped()).stderr(Stdio::inherit());
    let mut child = command.spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .expect("stdout is piped immediately before spawning");
    let relay = thread::spawn(move || io::copy(&mut stdout, &mut io::stderr()));

    let status = child.wait();
    let relay_result = relay
        .join()
        .map_err(|_| io::Error::other("child stdout relay thread panicked"))?;
    relay_result?;
    status
}
