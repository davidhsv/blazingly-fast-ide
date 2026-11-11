//! Command-line helpers shared by the terminal and External Tools.

use std::borrow::Cow;

/// White space in the ECMAScript sense (`\s`), the set stored command lines are split on. Unlike
/// `char::is_whitespace` it has U+FEFF and lacks U+0085.
fn is_space(c: char) -> bool {
    matches!(c, '\t'..='\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}')
        || matches!(c, '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

/// Splits `input` into words on white space. Quotes group words, and a backslash makes the next
/// character literal where `escapes_quoted` (inside double quotes) or `escapes_bare` (outside
/// quotes) accepts that character; anywhere else the backslash stays as typed.
fn split_words(input: &str, escapes_quoted: impl Fn(char) -> bool, escapes_bare: impl Fn(char) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    // Quotes or an escape were seen: the word counts even when it is empty (`''`).
    let mut has = false;
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(open) = quote {
            if c == open {
                quote = None;
            } else if c == '\\'
                && open == '"'
                && let Some(escaped) = chars.next_if(|&next| escapes_quoted(next))
            {
                cur.push(escaped);
            } else {
                cur.push(c);
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
            has = true;
        } else if c == '\\'
            && let Some(escaped) = chars.next_if(|&next| escapes_bare(next))
        {
            cur.push(escaped);
            has = true;
        } else if is_space(c) {
            if has || !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            has = false;
        } else {
            cur.push(c);
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Splits a command line (`/bin/zsh --login`, `-c "a b"`) into words, honouring quotes.
pub fn split_command_line(line: &str) -> Vec<String> {
    split_words(line, |_| true, |_| true)
}

/// Splits a command line into arguments: whitespace, '…' and "…" quoting, backslash escapes.
/// `windows_paths`: a backslash only escapes a quote, so `C:\dir\app.jar` stays as typed.
pub fn parse_args(input: &str, windows_paths: bool) -> Vec<String> {
    split_words(
        input,
        |next| next == '"' || (!windows_paths && matches!(next, '\\' | '$' | '`')),
        |next| !windows_paths || matches!(next, '"' | '\''),
    )
}

/// Where the program of an unquoted Windows command line ends: right after the first `.exe`,
/// `.cmd`, `.bat` or `.com` (in any letter case) that is followed by white space or nothing.
fn executable_end(text: &str) -> Option<usize> {
    const EXTENSIONS: [&[u8]; 4] = [b"exe", b"cmd", b"bat", b"com"];
    text.match_indices('.').map(|(dot, _)| dot + 4).find(|&end| {
        // Compared as bytes: `end` is a character boundary only once the extension matched.
        text.as_bytes().get(end - 3..end).is_some_and(|ext| EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
            && text[end..].chars().next().is_none_or(is_space)
    })
}

/// The program and arguments of a shell command line (Settings › Terminal › Shell path). On
/// Windows a backslash is a path separator, and an unquoted path up to its `.exe` / `.cmd` /
/// `.bat` / `.com` is the program even with spaces in it (`C:\Program Files\…\pwsh.exe -NoLogo`).
pub fn parse_shell_command(line: &str, windows: bool) -> Vec<String> {
    let text = line.trim_matches(is_space);
    if windows
        && !text.contains(['"', '\''])
        && let Some(end) = executable_end(text)
    {
        let mut words = vec![text[..end].to_owned()];
        words.extend(parse_args(&text[end..], true));
        return words;
    }
    parse_args(text, windows)
}

/// `path` as one word of a command line: quoted when it has whitespace.
pub fn quote_path(path: &str) -> String {
    if path.contains(is_space) { format!("\"{path}\"") } else { path.to_owned() }
}

/// `arg` as [`parse_args`] reads it back: single-quoted when it is empty or has a character the
/// parser treats specially.
fn quote_arg(arg: &str) -> Cow<'_, str> {
    if arg.is_empty() {
        Cow::Borrowed("''")
    } else if arg.contains(|c: char| is_space(c) || matches!(c, '\'' | '"' | '\\' | '$' | '`')) {
        Cow::Owned(format!("'{}'", arg.replace('\'', r"'\''")))
    } else {
        Cow::Borrowed(arg)
    }
}

/// Inverse of [`parse_args`] (quotes only when needed).
pub fn format_args<S: AsRef<str>>(args: &[S]) -> String {
    args.iter().map(|arg| quote_arg(arg.as_ref())).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // src/platform/commandLine.test.ts › keeps Windows paths as typed
    #[test]
    fn keeps_windows_paths_as_typed() {
        assert_eq!(parse_shell_command(r"C:\Windows\system32\cmd.exe", true), [r"C:\Windows\system32\cmd.exe"]);
        assert_eq!(
            parse_shell_command(r"C:\Windows\system32\cmd.exe /k C:\tools\init.bat", true),
            [r"C:\Windows\system32\cmd.exe", "/k", r"C:\tools\init.bat"]
        );
    }

    // src/platform/commandLine.test.ts › takes an unquoted Windows path with spaces up to its executable
    #[test]
    fn takes_an_unquoted_windows_path_with_spaces_up_to_its_executable() {
        assert_eq!(
            parse_shell_command(r"C:\Program Files\PowerShell\7\pwsh.exe", true),
            [r"C:\Program Files\PowerShell\7\pwsh.exe"]
        );
        assert_eq!(
            parse_shell_command(r"C:\Program Files\Git\bin\bash.EXE --login -i", true),
            [r"C:\Program Files\Git\bin\bash.EXE", "--login", "-i"]
        );
    }

    // src/platform/commandLine.test.ts › honours quotes
    #[test]
    fn honours_quotes() {
        assert_eq!(
            parse_shell_command(r#""C:\Program Files\Git\bin\bash.exe" --login"#, true),
            [r"C:\Program Files\Git\bin\bash.exe", "--login"]
        );
        assert_eq!(parse_shell_command("/bin/zsh --login", false), ["/bin/zsh", "--login"]);
        assert_eq!(
            parse_shell_command(r#""/Applications/My Shell/bin/fish" -l"#, false),
            ["/Applications/My Shell/bin/fish", "-l"]
        );
        assert_eq!(parse_shell_command(r"/bin/sh -c a\ b", false), ["/bin/sh", "-c", "a b"]);
    }

    // src/platform/commandLine.test.ts › is empty for an empty setting
    #[test]
    fn is_empty_for_an_empty_setting() {
        assert!(parse_shell_command("  ", true).is_empty());
        assert!(parse_shell_command("", false).is_empty());
    }

    // src/platform/commandLine.test.ts › quotes only paths with whitespace
    #[test]
    fn quotes_only_paths_with_whitespace() {
        assert_eq!(quote_path(r"C:\Windows\system32\cmd.exe"), r"C:\Windows\system32\cmd.exe");
        assert_eq!(
            quote_path(r"C:\Program Files\PowerShell\7\pwsh.exe"),
            r#""C:\Program Files\PowerShell\7\pwsh.exe""#
        );
        assert_eq!(
            parse_shell_command(&quote_path("/Applications/My Shell/fish"), false),
            ["/Applications/My Shell/fish"]
        );
    }
}
