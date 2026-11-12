//! Create Patch: what is asked once the patch file is saved.

use crate::platform::misc::file_manager::file_manager_name;
use crate::platform::misc::platform_info::Os;

/// The question whether to show the saved patch in the file manager. English source text: each
/// file manager has a sentence of its own, so that every locale can translate it as a whole.
pub fn patch_saved_body(os: Os) -> &'static str {
    match file_manager_name(os) {
        "Finder" => "Patch file created successfully. Show it in Finder?",
        "Explorer" => "Patch file created successfully. Show it in Explorer?",
        _ => "Patch file created successfully. Show it in Files?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::I18n;

    // src/features/vcs/__tests__/patchFileManager.test.ts › asks to show the saved patch in the %s file manager
    #[test]
    fn asks_to_show_the_saved_patch_in_the_file_manager() {
        const FINDER: &str = "Patch file created successfully. Show it in Finder?";
        const EXPLORER: &str = "Patch file created successfully. Show it in Explorer?";
        const FILES: &str = "Patch file created successfully. Show it in Files?";
        let cases = [
            (Os::Macos, FINDER, "补丁文件已创建。是否在访达中显示？"),
            (Os::Windows, EXPLORER, "补丁文件已创建。是否在资源管理器中显示？"),
            (Os::Linux, FILES, "补丁文件已创建。是否在文件管理器中显示？"),
        ];
        let mut chinese = I18n::default();
        chinese.set_locale(Some("zh"), "en");
        for (os, english, translated) in cases {
            assert_eq!(patch_saved_body(os), english);
            assert_eq!(chinese.tx(patch_saved_body(os)), translated);
        }
    }
}
