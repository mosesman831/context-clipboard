//! context-clipboardd — always-on clipboard capture daemon for Context
//! Clipboard. See SPEC.md §§3, 5–7, 9, 11. All logic lives in the library
//! crate; this binary is a thin entrypoint.

fn main() -> anyhow::Result<()> {
    clipboard_daemon::main_entry()
}
