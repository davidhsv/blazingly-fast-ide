//! The threads a project starts in the backend (its file watcher, its indexer, its query history
//! recorder) end when the project leaves its window: Close Project, another project opening in the
//! window, the window closing. Counted in a process of its own (the test binary run again for this
//! one test), so that no other test's backend is counted.

use super::*;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Weak};

/// Set in the process the test starts for itself, which then counts.
const COUNTING: &str = "OPENIDEA_TEST_COUNT_PROJECT_THREADS";

/// The threads each project starts, by name.
const PROJECT_THREADS: [&str; 3] = ["openidea-fs-watch", "openidea-index", "openidea-db-history"];

/// The process's threads by name, through the Mach and pthread calls of the C library.
#[cfg(target_os = "macos")]
fn named_threads() -> BTreeMap<String, usize> {
    use std::ffi::{CStr, c_char, c_int, c_void};
    type MachPort = u32;
    unsafe extern "C" {
        static mach_task_self_: MachPort;
        fn task_threads(task: MachPort, list: *mut *mut MachPort, count: *mut u32) -> c_int;
        fn pthread_from_mach_thread_np(port: MachPort) -> *mut c_void;
        fn pthread_getname_np(thread: *mut c_void, name: *mut c_char, len: usize) -> c_int;
        fn mach_port_deallocate(task: MachPort, name: MachPort) -> c_int;
        fn vm_deallocate(task: MachPort, address: usize, size: usize) -> c_int;
    }
    let mut counts = BTreeMap::new();
    // SAFETY: the list and every port in it are the kernel's answer for this task, read within
    // their count and handed back once; a name is read into a buffer of the length given.
    unsafe {
        let task = mach_task_self_;
        let (mut list, mut count) = (std::ptr::null_mut::<MachPort>(), 0u32);
        assert_eq!(task_threads(task, &mut list, &mut count), 0, "the task lists its threads");
        for index in 0..count as usize {
            let port = *list.add(index);
            let thread = pthread_from_mach_thread_np(port);
            let mut name = [0 as c_char; 64];
            let named = !thread.is_null() && pthread_getname_np(thread, name.as_mut_ptr(), name.len()) == 0;
            let name = if named { CStr::from_ptr(name.as_ptr()).to_string_lossy().into_owned() } else { String::new() };
            *counts.entry(name).or_default() += 1;
            mach_port_deallocate(task, port);
        }
        vm_deallocate(task, list as usize, count as usize * std::mem::size_of::<MachPort>());
    }
    counts
}

/// The number of threads of each of [`PROJECT_THREADS`] and of all threads.
#[cfg(target_os = "macos")]
fn project_threads() -> (Vec<(&'static str, usize)>, usize) {
    let named = named_threads();
    let each = PROJECT_THREADS.iter().map(|name| (*name, named.get(*name).copied().unwrap_or(0))).collect();
    (each, named.values().sum())
}

/// Waits for the threads of the projects to be back at `baseline`.
#[cfg(target_os = "macos")]
fn wait_for_threads(cx: &mut VisualTestContext, what: &str, baseline: &[(&'static str, usize)]) {
    wait_long(cx, what, |_| project_threads().0 == baseline);
}

/// A trusted project folder holding one file.
fn folder() -> (tempfile::TempDir, PathBuf) {
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::write(root.join("a.txt"), "a\n").expect("the file is written");
    (folder, root)
}

/// Opens the project at `root` (trusted first) in the window of `workbench`; the backend's project of
/// the window, held weakly.
fn open(workbench: &Entity<Workbench>, root: &Path, cx: &mut VisualTestContext) -> Weak<ide_host::Project> {
    cx.update(|_, cx| cx.global::<Backend>().host.trust.decide(root, true, false)).expect("the project is trusted");
    let path = root.to_path_buf();
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(path, window, cx));
    wait_long(cx, "the project opens", |cx| {
        workbench.read_with(cx, |w, cx| w.project_root(cx)).is_some_and(|shown| shown == root)
    });
    let label = workbench.read_with(cx, |w, _| w.label().to_owned());
    let project = cx.update(|_, cx| cx.global::<Backend>().host.project(&label)).expect("the backend has it");
    Arc::downgrade(&project)
}

/// Waits until nothing holds the project any more.
fn wait_released(cx: &mut VisualTestContext, project: &Weak<ide_host::Project>) {
    wait_long(cx, "the project is released", |_| project.upgrade().is_none());
}

/// Close Project, Open Project in the same window and closing the window each release the project the
/// window had, and with it the threads it started: after ten rounds of each the threads are those of
/// the backend alone.
#[gpui_kit::test]
async fn a_projects_threads_end_when_it_leaves_its_window(cx: &mut TestAppContext) {
    if std::env::var_os(COUNTING).is_none() {
        let name = "workbench::tests::project_threads::a_projects_threads_end_when_it_leaves_its_window";
        let run = std::process::Command::new(std::env::current_exe().expect("the test binary"))
            .args(["--exact", name, "--nocapture", "--test-threads=1"])
            .env(COUNTING, "1")
            .output()
            .expect("the test binary runs");
        let printed = String::from_utf8_lossy(&run.stdout);
        let failed = String::from_utf8_lossy(&run.stderr);
        assert!(run.status.success(), "the counting run failed:\n{printed}\n{failed}");
        assert!(printed.contains("1 passed"), "the counting run ran the test:\n{printed}");
        return;
    }
    let _home = start(cx);
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    #[cfg(target_os = "macos")]
    let (baseline, total) = project_threads();
    #[cfg(target_os = "macos")]
    println!("threads: baseline {baseline:?}, {total} in all");

    let (_first, first) = folder();
    let project = open(&workbench, &first, cx);
    #[cfg(target_os = "macos")]
    {
        let (open, total) = project_threads();
        println!("threads: one project open {open:?}, {total} in all");
    }
    run_action(cx, "file.closeProject", None);
    wait_released(cx, &project);
    #[cfg(target_os = "macos")]
    {
        wait_for_threads(cx, "the project's threads end after Close Project", &baseline);
        println!("threads: after Close Project {:?}, {} in all", project_threads().0, project_threads().1);
    }

    let folders: Vec<_> = (0..11).map(|_| folder()).collect();
    for (_folder, root) in folders.iter().take(10) {
        let project = open(&workbench, root, cx);
        run_action(cx, "file.closeProject", None);
        wait_released(cx, &project);
    }
    #[cfg(target_os = "macos")]
    {
        wait_for_threads(cx, "the threads end after ten Close Project rounds", &baseline);
        println!("threads: after ten open and close rounds {:?}, {} in all", project_threads().0, project_threads().1);
    }

    // Another project opening in the window takes the place of the one it had.
    let mut shown = open(&workbench, &folders[0].1, cx);
    for (_folder, root) in &folders[1..] {
        let next = open(&workbench, root, cx);
        wait_released(cx, &shown);
        shown = next;
    }
    #[cfg(target_os = "macos")]
    {
        let (each, total) = project_threads();
        println!("threads: after ten projects opened in turn, the last one open {each:?}, {total} in all");
    }
    run_action(cx, "file.closeProject", None);
    wait_released(cx, &shown);
    #[cfg(target_os = "macos")]
    {
        wait_for_threads(cx, "the threads end once the last project closes", &baseline);
        println!("threads: after the last Close Project {:?}, {} in all", project_threads().0, project_threads().1);
    }

    // Closing a second window releases its project too.
    for (_folder, root) in folders.iter().take(10) {
        let (other, handle) = {
            let (other, other_cx) = cx.add_window_view(|window, cx| Workbench::new("w2", window, cx));
            (other, other_cx.update(|window, _| window.window_handle()))
        };
        let mut other_cx = VisualTestContext::from_window(handle, cx);
        other_cx.update(|window, _| window.activate_window());
        let project = open(&other, root, &mut other_cx);
        drop(other);
        assert!(other_cx.simulate_close(), "one of two windows closes at once");
        other_cx.update(|window, _| window.remove_window());
        other_cx.run_until_parked();
        wait_released(cx, &project);
    }
    #[cfg(target_os = "macos")]
    {
        wait_for_threads(cx, "the threads end after ten windows closed", &baseline);
        println!("threads: after ten windows closed {:?}, {} in all", project_threads().0, project_threads().1);
    }
}
