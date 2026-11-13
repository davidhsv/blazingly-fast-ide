use super::*;

/// Waits until the backend's index of the project open as `w1` counts each of `expected` (extension, files).
fn wait_for_index(cx: &mut TestAppContext, expected: &[(&str, u32)]) {
    let host = cx.update(|cx| cx.global::<Backend>().host.clone());
    for _ in 0..300 {
        let counts = ide_host::ops::nav::index_file_extensions(&host, "w1").unwrap_or_default();
        let counted = |(extension, files): &(&str, u32)| {
            counts.iter().any(|count| count.extension == *extension && count.files == *files)
        };
        if expected.iter().all(counted) {
            return;
        }
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the project was not indexed within three seconds");
}

/// The masks list offers the recent masks first, then `*.ext` masks of the project's most common file
/// types, most files first: only types some language claims (the four `.zzq` files are left out), and a
/// type that is already a recent mask is not offered twice. Choosing one puts it in the mask.
#[gpui_kit::test]
async fn the_masks_list_offers_the_projects_most_common_language_file_types_after_the_recent_ones(
    cx: &mut TestAppContext,
) {
    let files = [
        ("src/a.rs", "fn a() {}\n"),
        ("src/b.rs", "fn b() {}\n"),
        ("src/c.rs", "fn c() {}\n"),
        ("d.py", "d = 1\n"),
        ("e.py", "e = 1\n"),
        ("f.go", "package f\n"),
        ("data/1.zzq", "x\n"),
        ("data/2.zzq", "x\n"),
        ("data/3.zzq", "x\n"),
        ("data/4.zzq", "x\n"),
    ];
    let project = project(cx, &files).await;
    wait_for_index(cx, &[("rs", 3), ("py", 2), ("go", 1)]);
    cx.update(|cx| cx.global_mut::<UiStorage>().set(MASKS_KEY, r#"["*.py","*.md"]"#.to_owned()));
    let (popup, _, cx) = open(cx, project.input());
    settle(cx, |cx| popup.read_with(cx, |popup, _| !popup.project_masks.is_empty()));

    click(cx, "find-popup-masks", Modifiers::default());
    assert_eq!(
        popup.read_with(cx, |popup, _| (popup.menu(), popup.menu_lines())),
        (Some(FindMenu::Masks), ["*.py", "*.md", "*.rs", "*.go"].map(str::to_owned).to_vec())
    );
    click(cx, "find-popup-menu-2", Modifiers::default());
    popup.read_with(cx, |popup, cx| {
        assert_eq!(popup.menu(), None);
        assert_eq!(popup.mask_field().read(cx).text(), "*.rs");
        assert_eq!(popup.query().include, ["*.rs"]);
    });
    close_project(cx);
}
