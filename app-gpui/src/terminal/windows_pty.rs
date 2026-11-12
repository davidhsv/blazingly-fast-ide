//! What a terminal emulator has to know about the Windows pseudo console behind a local shell:
//! ConPTY rewraps lines itself on resize, and how it does depends on the OS build.

use crate::platform::misc::platform_info::{Os, PlatformInfo};

/// The pseudo console of local Windows shells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConPty {
    /// The Windows build; `None` while the backend has not reported it. The backend is ConPTY
    /// either way.
    pub build_number: Option<u32>,
}

/// The pseudo console behind a terminal of this window, `None` when it is not ConPTY. `remote`:
/// the window's shells run on a remote host, which must not inherit the desktop's Windows build
/// or backend.
pub fn windows_pty(platform: &PlatformInfo, remote: bool) -> Option<ConPty> {
    (!remote && platform.os == Os::Windows).then_some(ConPty { build_number: platform.windows_build_number })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platform(os: Os, windows_build_number: Option<u32>) -> PlatformInfo {
        PlatformInfo { os, windows_build_number, ..PlatformInfo::guessed(None, true) }
    }

    // src/features/terminal/windowsPty.test.ts › does not apply the desktop build to a remote %s PTY
    #[test]
    fn does_not_apply_the_desktop_build_to_a_remote_pty() {
        assert_eq!(windows_pty(&platform(Os::Windows, Some(22631)), true), None);
    }

    // src/features/terminal/embedded.test.ts › configures embedded terminals for %s build %s
    #[test]
    fn configures_local_terminals_for_the_os_and_build() {
        let conpty = |build_number| Some(ConPty { build_number });
        assert_eq!(windows_pty(&platform(Os::Windows, Some(19045)), false), conpty(Some(19045)));
        assert_eq!(windows_pty(&platform(Os::Windows, Some(22631)), false), conpty(Some(22631)));
        assert_eq!(windows_pty(&platform(Os::Windows, None), false), conpty(None));
        assert_eq!(windows_pty(&platform(Os::Macos, None), false), None);
        assert_eq!(windows_pty(&platform(Os::Linux, None), false), None);
    }
}
