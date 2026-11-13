//! The process-wide switch that keeps saved passwords in files. It runs in its own test binary
//! because the switch cannot be turned off again.

use ide_ssh::secrets::{SecretFile, default_store_installed, keep_secrets_in_files, secrets_in_files};

#[test]
fn once_secrets_are_kept_in_files_saved_passwords_go_to_a_file_and_the_os_store_is_never_set_up() {
    assert!(!secrets_in_files());
    keep_secrets_in_files();
    assert!(secrets_in_files());

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("git-credentials.json");
    SecretFile::new(path.clone()).set("https://example.com", "me\nfile-secret").unwrap();
    let vault = tmp.path().join("git-credentials.json.vault");
    assert!(std::fs::read_to_string(&vault).unwrap().contains("file-secret"));
    assert!(!std::fs::read_to_string(&path).unwrap().contains("file-secret"), "the index holds no secret");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&vault).unwrap().permissions().mode() & 0o777, 0o600);
    }

    let reopened = SecretFile::new(path);
    assert_eq!(reopened.get("https://example.com").as_deref(), Some("me\nfile-secret"));
    reopened.delete("https://example.com");
    assert_eq!(reopened.get("https://example.com"), None);
    assert!(!std::fs::read_to_string(&vault).unwrap().contains("file-secret"));

    assert!(!default_store_installed(), "the OS credential store was set up");
}
