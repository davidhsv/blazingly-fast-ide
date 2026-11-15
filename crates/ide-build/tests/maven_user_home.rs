//! The Maven user folder (`~/.m2`) a caller names in place of the user's. One test in its own
//! binary: it sets the process's Maven user folder.

use ide_build::maven::{default_local_repo, maven_user_home, set_maven_user_home, user_settings_file};
use ide_core::settings::MavenSettings;
use std::path::Path;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn the_named_maven_user_folder_stands_for_the_users_settings_and_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let m2 = tmp.path().join("m2");
    let root = tmp.path().join("proj");
    set_maven_user_home(Some(m2.clone()));
    assert_eq!(maven_user_home(), Some(m2.clone()));
    assert_eq!(default_local_repo(), Some(m2.join("repository")), "its own repository");
    assert_eq!(user_settings_file(&root, &MavenSettings::default()), Some(m2.join("settings.xml")));

    // Its settings name the repository, and a parent POM outside the project is found there.
    let repo = tmp.path().join("elsewhere");
    let settings = format!("<settings><localRepository>{}</localRepository></settings>", repo.display());
    write(&m2.join("settings.xml"), &settings);
    write(
        &repo.join("org/demo/base/3.0/base-3.0.pom"),
        concat!(
            "<project><groupId>org.demo</groupId><artifactId>base</artifactId><version>3.0</version>",
            "<properties><java.version>17</java.version></properties></project>"
        ),
    );
    write(
        &root.join("pom.xml"),
        concat!(
            "<project><parent><groupId>org.demo</groupId><artifactId>base</artifactId><version>3.0</version>",
            "<relativePath/></parent><artifactId>app</artifactId></project>"
        ),
    );
    assert_eq!(default_local_repo(), Some(repo));
    let model = ide_build::load(&root, &tmp.path().join("cache"), None);
    let maven = model.maven.expect("the project is a Maven project");
    assert_eq!(maven.projects[0].java_version, Some(17), "read from the parent in the named repository");

    set_maven_user_home(None);
    assert_eq!(maven_user_home(), dirs::home_dir().map(|home| home.join(".m2")), "the user's again");
}
