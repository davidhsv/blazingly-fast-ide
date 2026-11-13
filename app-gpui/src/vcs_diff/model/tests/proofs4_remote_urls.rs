use super::*;

/// ssh (with a port and a nested group), scp-like and https (with a user and password) remotes map
/// to their web page; a local path has none. The commit page is GitHub's `commit/` and Bitbucket's
/// `commits/`.
#[test]
fn maps_ssh_scp_like_and_https_remotes_to_their_web_page() {
    assert_eq!(remote_web_url("git@github.com:acme/app.git").as_deref(), Some("https://github.com/acme/app"));
    assert_eq!(
        remote_web_url("ssh://git@gitlab.example.com:2222/group/sub/app.git").as_deref(),
        Some("https://gitlab.example.com/group/sub/app")
    );
    assert_eq!(remote_web_url("https://user:token@github.com/o/r.git/").as_deref(), Some("https://github.com/o/r"));
    assert_eq!(remote_web_url("/srv/git/app.git"), None);
    assert_eq!(commit_web_url("git@github.com:o/r.git", "abc").as_deref(), Some("https://github.com/o/r/commit/abc"));
    assert_eq!(
        commit_web_url("git@bitbucket.org:o/r.git", "abc").as_deref(),
        Some("https://bitbucket.org/o/r/commits/abc")
    );
}
