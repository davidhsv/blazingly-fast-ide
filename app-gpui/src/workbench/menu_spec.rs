//! The menu bar's table: which menus there are on each OS, what they hold and which action
//! each item runs, with the functions that route a chosen item and say whether it is enabled.
//!
//! Written by `docs/gpui-port/menuspec.mjs` from `src-tauri/src/menu.rs`. Do not edit: change the
//! table there and run the script.

use std::collections::HashSet;
use std::path::Path;

/// Prefix of Open Recent item ids; the rest of the id is the project path.
pub const RECENT_PREFIX: &str = "recent:";
/// Prefix of items the frontend lists (`MenuConfig::dynamic`): `dyn:<action>` or `dyn:<action>|<arg>`.
pub const DYNAMIC_PREFIX: &str = "dyn:";
/// Action run for an Open Recent entry (argument: the project path).
pub const OPEN_RECENT_ACTION: &str = "file.openRecentProject";
pub const MAX_RECENT: usize = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// Native (role) items, implemented by the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Native {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Minimize,
    Zoom,
    BringAllToFront,
}

impl Native {
    pub fn key(self) -> &'static str {
        match self {
            Native::Undo => "native.undo",
            Native::Redo => "native.redo",
            Native::Cut => "native.cut",
            Native::Copy => "native.copy",
            Native::Paste => "native.paste",
            Native::SelectAll => "native.selectAll",
            Native::Services => "native.services",
            Native::Hide => "native.hide",
            Native::HideOthers => "native.hideOthers",
            Native::ShowAll => "native.showAll",
            Native::Minimize => "native.minimize",
            Native::Zoom => "native.zoom",
            Native::BringAllToFront => "native.bringAllToFront",
        }
    }

    pub fn default_label(self) -> &'static str {
        match self {
            Native::Undo => "Undo",
            Native::Redo => "Redo",
            Native::Cut => "Cut",
            Native::Copy => "Copy",
            Native::Paste => "Paste",
            Native::SelectAll => "Select All",
            Native::Services => "Services",
            Native::Hide => "Hide OpenIDEA",
            Native::HideOthers => "Hide Others",
            Native::ShowAll => "Show All",
            Native::Minimize => "Minimize",
            Native::Zoom => "Zoom",
            Native::BringAllToFront => "Bring All to Front",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Submenu {
        id: &'static str,
        label: &'static str,
        children: Vec<Entry>,
    },
    /// An IDE action; `id` is the action id.
    Action {
        id: &'static str,
        label: &'static str,
        /// Native accelerator (Tauri syntax) that fires natively ([`fires_natively`]) and wins
        /// over the keymap's. Only for keys the webview never handles.
        accel: Option<&'static str>,
        /// Shortcut text after the label, for the Windows/Linux edit items without a keymap binding.
        hint: Option<&'static str>,
        /// A toggle (IDEA's ToggleAction) drawn with a check mark; on when its id is in
        /// `MenuConfig::checked`.
        check: bool,
    },
    Native(Native),
    Separator,
    /// Open Recent entries, filled from the recent-projects list.
    RecentProjects,
    /// Items the frontend lists under this slot name (registered tool windows,
    /// saved layouts), from `MenuConfig::dynamic`.
    Dynamic(&'static str),
}

pub fn sub(id: &'static str, label: &'static str, children: Vec<Entry>) -> Entry {
    Entry::Submenu { id, label, children }
}
pub fn act(id: &'static str, label: &'static str) -> Entry {
    Entry::Action { id, label, accel: None, hint: None, check: false }
}
pub fn accel(id: &'static str, label: &'static str, accel: &'static str) -> Entry {
    Entry::Action { id, label, accel: Some(accel), hint: None, check: false }
}
pub fn hinted(id: &'static str, label: &'static str, hint: &'static str) -> Entry {
    Entry::Action { id, label, accel: None, hint: Some(hint), check: false }
}
pub fn toggle(id: &'static str, label: &'static str) -> Entry {
    Entry::Action { id, label, accel: None, hint: None, check: true }
}
pub const SEP: Entry = Entry::Separator;

/// [`Entry::Dynamic`] slots the frontend fills.
pub const DYNAMIC_TOOL_WINDOWS: &str = "toolWindows";
pub const DYNAMIC_LAYOUTS: &str = "toolWindowLayouts";
/// View › Appearance › Status Bar Widgets: every titled widget with its check mark.
pub const DYNAMIC_STATUS_BAR_WIDGETS: &str = "statusBarWidgets";
/// Tools › entries of plugins (`[[contributes.menus]] menu = "tools"`).
pub const DYNAMIC_PLUGIN_TOOLS: &str = "pluginTools";

/// The menu bar for `os`. Labels are English defaults; the frontend replaces
/// them with `menu.<id>` strings from the active locale catalog.
pub fn spec(os: Os) -> Vec<Entry> {
    let mac = os == Os::Mac;
    let show_log = match os {
        Os::Mac => "Show Log in Finder",
        Os::Windows => "Show Log in Explorer",
        Os::Linux => "Show Log in Files",
    };
    let mut menus = Vec::new();

    if mac {
        menus.push(sub(
            "app",
            "OpenIDEA",
            vec![
                act("app.about", "About OpenIDEA"),
                act("app.checkForUpdates", "Check for Updates…"),
                SEP,
                act("app.installCli", "Install 'openidea' Command"),
                act("app.uninstallCli", "Uninstall 'openidea' Command"),
                SEP,
                act("file.settings", "Settings…"),
                SEP,
                Entry::Native(Native::Services),
                SEP,
                Entry::Native(Native::Hide),
                Entry::Native(Native::HideOthers),
                Entry::Native(Native::ShowAll),
                SEP,
                accel("app.quit", "Quit OpenIDEA", "CmdOrCtrl+Q"),
            ],
        ));
    }

    let mut file = vec![
        act("file.newProject", "New Project…"),
        act("workspace.create", "New Workspace…"),
        act("workspace.open", "Open Workspace…"),
        act("workspace.manage", "Manage Workspace…"),
        act("file.new", "New…"),
        act("file.newScratchFile", "New Scratch File…"),
        act("file.openProject", "Open…"),
        act("project.attach", "Attach Project…"),
        act("file.openFile", "Open File…"),
        act("file.openInNewWindow", "Open in New Window…"),
        sub(
            "openRecent",
            "Open Recent",
            vec![Entry::RecentProjects, SEP, act("welcome.recentProjects", "Manage Projects…")],
        ),
        act("file.closeProject", "Close Project"),
        act("file.renameProject", "Rename Project…"),
        SEP,
        act("remote.connect", "Remote Development…"),
        act("remote.reopenInContainer", "Reopen in Container"),
        act("remote.disconnect", "Disconnect from Remote Host"),
        act("remote.reloadEnvironment", "Reload Environment"),
        SEP,
        act("file.save", "Save All"),
        act("file.reloadAllFromDisk", "Reload All from Disk"),
        sub("fileProperties", "File Properties", vec![act("file.toggleReadOnly", "Make File Read-only / Writable")]),
        SEP,
        act("file.projectStructure", "Project Structure…"),
        SEP,
        act("file.invalidateCaches", "Invalidate Caches…"),
        SEP,
        sub(
            "manageIdeSettings",
            "Manage IDE Settings",
            vec![act("file.exportSettings", "Export Settings…"), act("file.importSettings", "Import Settings…")],
        ),
        sub(
            "newProjectsSetup",
            "New Projects Setup",
            vec![act("file.newProjectsSettings", "Settings for New Projects…")],
        ),
        SEP,
        act("file.togglePowerSave", "Power Save Mode"),
    ];
    if !mac {
        // macOS keeps Settings… and Quit in the application menu.
        file.extend([SEP, act("file.settings", "Settings…"), SEP, accel("app.quit", "Exit", "Ctrl+Q")]);
    }
    menus.push(sub("file", "File", file));

    let find = sub(
        "find",
        "Find",
        vec![
            act("find.inFile", "Find…"),
            act("find.replaceInFile", "Replace…"),
            SEP,
            act("find.inPath", "Find in Files…"),
            act("find.replaceInPath", "Replace in Files…"),
            act("find.structural", "Search Structurally…"),
            act("find.replaceStructural", "Replace Structurally…"),
            SEP,
            act("find.usages", "Find Usages"),
            act("find.showUsages", "Show Usages"),
        ],
    );
    let macros = || {
        sub(
            "macros",
            "Macros",
            vec![
                act("macro.startRecording", "Start Macro Recording"),
                act("macro.stopRecording", "Stop Macro Recording"),
                act("macro.playBack", "Play Back Last Macro"),
                SEP,
                act("macro.playSaved", "Play Saved Macros…"),
                act("macro.edit", "Edit Macros…"),
            ],
        )
    };
    let bookmarks = || {
        sub(
            "bookmarks",
            "Bookmarks",
            vec![
                act("navigate.toggleBookmark", "Toggle Bookmark"),
                act("navigate.toggleBookmarkWithMnemonic", "Toggle Bookmark with Mnemonic"),
                SEP,
                act("navigate.showBookmarks", "Show All Bookmarks…"),
                act("navigate.nextBookmark", "Next Bookmark"),
                act("navigate.prevBookmark", "Previous Bookmark"),
            ],
        )
    };
    let edit = if mac {
        vec![
            Entry::Native(Native::Undo),
            Entry::Native(Native::Redo),
            SEP,
            Entry::Native(Native::Cut),
            Entry::Native(Native::Copy),
            Entry::Native(Native::Paste),
            act("code.pasteFromHistory", "Paste from History…"),
            act("code.pasteAsPlainText", "Paste as Plain Text"),
            Entry::Native(Native::SelectAll),
            SEP,
            find,
            SEP,
            act("editor.copyPath", "Copy Path"),
            act("editor.copyReference", "Copy Reference"),
            SEP,
            act("code.sortLines", "Sort Lines"),
            act("code.reverseLines", "Reverse Lines"),
            SEP,
            act("editor.toggleColumnSelection", "Column Selection Mode"),
            SEP,
            macros(),
            bookmarks(),
        ]
    } else {
        // GTK/Win32 accelerators would swallow these keys before the webview, so
        // they are plain items routed to `document.execCommand` by the frontend.
        vec![
            hinted("edit.undo", "Undo", "Ctrl+Z"),
            hinted("edit.redo", "Redo", "Ctrl+Shift+Z"),
            SEP,
            hinted("edit.cut", "Cut", "Ctrl+X"),
            hinted("edit.copy", "Copy", "Ctrl+C"),
            hinted("edit.paste", "Paste", "Ctrl+V"),
            act("code.pasteFromHistory", "Paste from History…"),
            act("code.pasteAsPlainText", "Paste as Plain Text"),
            hinted("edit.selectAll", "Select All", "Ctrl+A"),
            SEP,
            find,
            SEP,
            act("editor.copyPath", "Copy Path"),
            act("editor.copyReference", "Copy Reference"),
            SEP,
            act("code.sortLines", "Sort Lines"),
            act("code.reverseLines", "Reverse Lines"),
            SEP,
            act("editor.toggleColumnSelection", "Column Selection Mode"),
            SEP,
            macros(),
            bookmarks(),
        ]
    };
    menus.push(sub("edit", "Edit", edit));

    menus.push(sub(
        "view",
        "View",
        vec![
            sub(
                "toolWindows",
                "Tool Windows",
                vec![Entry::Dynamic(DYNAMIC_TOOL_WINDOWS), SEP, act("toolwindow.hideAll", "Hide All Tool Windows")],
            ),
            sub(
                "appearance",
                "Appearance",
                vec![
                    act("editor.toggleZen", "Enter Distraction Free Mode"),
                    act("window.toggleFullScreen", "Toggle Full Screen"),
                    toggle("view.toggleCompactMode", "Compact Mode"),
                    SEP,
                    act("view.zoomIn", "Zoom In IDE"),
                    act("view.zoomOut", "Zoom Out IDE"),
                    act("view.resetZoom", "Reset IDE Zoom"),
                    SEP,
                    act("view.toggleTheme", "Toggle Light/Dark Theme"),
                    act("toolbar.customize", "Customize Main Toolbar…"),
                    SEP,
                    toggle("view.toggleToolbar", "Toolbar"),
                    toggle("view.toggleToolWindowBars", "Tool Window Bars"),
                    toggle("view.toggleStatusBar", "Status Bar"),
                    sub("statusBarWidgets", "Status Bar Widgets", vec![Entry::Dynamic(DYNAMIC_STATUS_BAR_WIDGETS)]),
                ],
            ),
            sub("activeEditor", "Active Editor", vec![act("editor.toggleSoftWrap", "Soft-Wrap")]),
            SEP,
            act("code.quickDefinition", "Quick Definition"),
            act("code.quickTypeDefinition", "Quick Type Definition"),
            SEP,
            act("navigate.recentLocations", "Recent Locations"),
            act("navigate.recentFiles", "Recent Files"),
            act("localHistory.recentChanges", "Recent Changes"),
            SEP,
            act("diff.compareWithClipboard", "Compare with Clipboard"),
            SEP,
            act("view.quickSwitchScheme", "Quick Switch Scheme…"),
            SEP,
            act("editor.resetFontSize", "Reset Font Size in All Editors"),
            SEP,
            act("view.notifications", "Notifications"),
        ],
    ));

    menus.push(sub(
        "navigate",
        "Navigate",
        vec![
            act("navigate.searchEverywhere", "Search Everywhere"),
            act("navigate.gotoClass", "Class…"),
            act("navigate.gotoFile", "File…"),
            act("navigate.gotoSymbol", "Symbol…"),
            act("navigate.gotoLine", "Line/Column…"),
            SEP,
            act("navigate.back", "Back"),
            act("navigate.forward", "Forward"),
            act("navigate.lastEditLocation", "Last Edit Location"),
            act("navigate.nextEditLocation", "Next Edit Location"),
            SEP,
            act("navigate.declaration", "Declaration or Usages"),
            act("navigate.implementation", "Implementation(s)"),
            act("navigate.typeDeclaration", "Type Declaration"),
            act("navigate.superMethod", "Super Method"),
            act("navigate.gotoTest", "Test"),
            SEP,
            act("navigate.fileStructure", "File Structure"),
            act("navigate.showFilePath", "File Path"),
            act("navigate.callHierarchy", "Call Hierarchy"),
            act("navigate.typeHierarchy", "Type Hierarchy"),
            act("navigate.customRegion", "Custom Folding…"),
            SEP,
            act("navigate.nextMethod", "Next Method"),
            act("navigate.previousMethod", "Previous Method"),
            SEP,
            act("navigate.nextError", "Next Highlighted Error"),
            act("navigate.prevError", "Previous Highlighted Error"),
            SEP,
            act("navigate.selectIn", "Select In…"),
        ],
    ));

    menus.push(sub(
        "code",
        "Code",
        vec![
            sub(
                "completion",
                "Code Completion",
                vec![
                    act("code.completion", "Basic"),
                    SEP,
                    act("code.cyclicExpandWord", "Cyclic Expand Word"),
                    act("code.cyclicExpandWordBackward", "Cyclic Expand Word (Backward)"),
                ],
            ),
            act("code.parameterInfo", "Parameter Info"),
            act("code.quickDocumentation", "Quick Documentation"),
            act("code.expressionTypeInfo", "Expression Type Info"),
            act("code.showErrorDescription", "Error Description"),
            act("editor.contextInfo", "Context Info"),
            act("code.showIntentions", "Show Context Actions"),
            act("code.generate", "Generate…"),
            act("code.overrideMethods", "Override Methods…"),
            act("code.implementMethods", "Implement Methods…"),
            SEP,
            act("code.insertLiveTemplate", "Insert Live Template…"),
            act("code.surroundWith", "Surround With…"),
            act("code.surroundWithLiveTemplate", "Surround with Live Template…"),
            act("code.unwrap", "Unwrap/Remove…"),
            SEP,
            act("code.reformat", "Reformat Code"),
            act("code.autoIndentLines", "Auto-Indent Lines"),
            act("code.optimizeImports", "Optimize Imports"),
            act("code.cleanup", "Code Cleanup…"),
            SEP,
            act("code.commentLine", "Comment with Line Comment"),
            act("code.commentBlock", "Comment with Block Comment"),
            SEP,
            act("code.extendSelection", "Extend Selection"),
            act("code.shrinkSelection", "Shrink Selection"),
            act("code.duplicateLine", "Duplicate Line or Selection"),
            act("code.deleteLine", "Delete Line"),
            act("code.moveStatementUp", "Move Statement Up"),
            act("code.moveStatementDown", "Move Statement Down"),
            act("code.moveElementLeft", "Move Element Left"),
            act("code.moveElementRight", "Move Element Right"),
            act("code.moveLineUp", "Move Line Up"),
            act("code.moveLineDown", "Move Line Down"),
            SEP,
            sub(
                "folding",
                "Folding",
                vec![
                    act("code.foldToggle", "Expand/Collapse"),
                    act("code.foldAll", "Collapse All"),
                    act("code.unfoldAll", "Expand All"),
                    sub(
                        "expandToLevel",
                        "Expand to Level",
                        vec![
                            act("code.expandToLevel1", "1"),
                            act("code.expandToLevel2", "2"),
                            act("code.expandToLevel3", "3"),
                            act("code.expandToLevel4", "4"),
                            act("code.expandToLevel5", "5"),
                        ],
                    ),
                    sub(
                        "expandAllToLevel",
                        "Expand All to Level",
                        vec![
                            act("code.expandAllToLevel1", "1"),
                            act("code.expandAllToLevel2", "2"),
                            act("code.expandAllToLevel3", "3"),
                            act("code.expandAllToLevel4", "4"),
                            act("code.expandAllToLevel5", "5"),
                        ],
                    ),
                    SEP,
                    act("code.collapseBlock", "Fold Code Block"),
                    act("code.foldSelection", "Fold Selection / Remove Region"),
                ],
            ),
            SEP,
            act("analyze.inspectCode", "Inspect Code…"),
            sub(
                "analyzeCode",
                "Analyze Code",
                vec![
                    act("analyze.inspectFile", "Inspect Current File"),
                    act("analyze.locateDuplicates", "Locate Duplicates…"),
                ],
            ),
            act("analyze.stackTrace", "Analyze Stack Trace or Thread Dump…"),
        ],
    ));

    menus.push(sub(
        "refactor",
        "Refactor",
        vec![
            act("refactor.thisMenu", "Refactor This…"),
            SEP,
            act("refactor.rename", "Rename…"),
            act("refactor.changeSignature", "Change Signature…"),
            SEP,
            act("refactor.extractVariable", "Introduce Variable…"),
            act("refactor.extractConstant", "Introduce Constant…"),
            act("refactor.extractField", "Introduce Field…"),
            act("refactor.extractParameter", "Introduce Parameter…"),
            act("refactor.extractMethod", "Extract Method…"),
            act("refactor.inline", "Inline…"),
            SEP,
            act("refactor.pullMembersUp", "Pull Members Up…"),
            act("refactor.pushMembersDown", "Push Members Down…"),
            SEP,
            act("refactor.move", "Move…"),
            act("project.copyElement", "Copy…"),
            act("refactor.safeDelete", "Safe Delete…"),
        ],
    ));

    menus.push(sub(
        "build",
        "Build",
        vec![
            act("build.buildProject", "Build Project"),
            act("build.buildModule", "Build Module"),
            act("build.rebuildProject", "Rebuild Project"),
            SEP,
            act("build.buildArtifacts", "Build Artifacts"),
        ],
    ));

    menus.push(sub(
        "run",
        "Run",
        vec![
            act("run.run", "Run"),
            act("run.debug", "Debug"),
            act("run.runContext", "Run Context Configuration"),
            act("run.debugContext", "Debug Context Configuration"),
            act("run.coverage", "Run with Coverage"),
            act("run.profile", "Profile"),
            act("run.stop", "Stop"),
            SEP,
            act("run.chooseConfig", "Run…"),
            act("run.chooseDebugConfig", "Debug…"),
            act("runAnything.show", "Run Anything…"),
            act("run.editConfigs", "Edit Configurations…"),
            act("run.updateApplication", "Update Application"),
            act("debug.attachToProcess", "Attach to Process…"),
            SEP,
            act("debug.resume", "Resume Program"),
            act("debug.pause", "Pause Program"),
            act("debug.stepOver", "Step Over"),
            act("debug.forceStepOver", "Force Step Over"),
            act("debug.stepInto", "Step Into"),
            act("debug.forceStepInto", "Force Step Into"),
            act("debug.smartStepInto", "Smart Step Into"),
            act("debug.stepOut", "Step Out"),
            act("debug.runToCursor", "Run to Cursor"),
            act("debug.forceRunToCursor", "Force Run to Cursor"),
            act("debug.showExecutionPoint", "Show Execution Point"),
            act("debug.resetFrame", "Reset Frame"),
            act("debug.evaluate", "Evaluate Expression…"),
            act("debug.quickEvaluate", "Quick Evaluate Expression"),
            sub(
                "debuggingActions",
                "Debugging Actions",
                vec![act("debug.reloadChangedClasses", "Reload Changed Classes")],
            ),
            SEP,
            act("coverage.hide", "Hide Coverage"),
            SEP,
            act("debug.toggleBreakpoint", "Toggle Line Breakpoint"),
            act("debug.toggleTemporaryBreakpoint", "Toggle Temporary Line Breakpoint"),
            act("debug.toggleMethodBreakpoint", "Toggle Method Breakpoint"),
            act("debug.toggleFieldWatchpoint", "Toggle Field Watchpoint"),
            act("debug.viewBreakpoints", "View Breakpoints…"),
        ],
    ));

    menus.push(sub(
        "git",
        "Git",
        vec![
            act("vcs.commit", "Commit…"),
            act("vcs.push", "Push…"),
            act("vcs.update", "Update Project…"),
            act("vcs.pull", "Pull…"),
            act("vcs.fetch", "Fetch"),
            SEP,
            // Operation items (Abort Merge, Continue Rebase…) are enabled only while it is in progress.
            act("vcs.merge", "Merge…"),
            act("vcs.merge.abort", "Abort Merge"),
            act("vcs.rebase", "Rebase…"),
            act("vcs.rebase.abort", "Abort Rebase"),
            act("vcs.rebase.continue", "Continue Rebase"),
            act("vcs.rebase.skip", "Skip Commit"),
            act("vcs.resolveConflicts", "Resolve Conflicts…"),
            SEP,
            act("vcs.branches", "Branches…"),
            act("vcs.newBranch", "New Branch…"),
            act("vcs.newTag", "New Tag…"),
            act("vcs.resetHead", "Reset HEAD…"),
            SEP,
            act("vcs.newWorktree", "New Worktree…"),
            act("vcs.worktrees", "Worktrees…"),
            SEP,
            act("vcs.showLog", "Show Git Log"),
            sub(
                "gitPatch",
                "Patch",
                vec![
                    act("vcs.createPatch", "Create Patch…"),
                    act("vcs.applyPatch", "Apply Patch…"),
                    act("vcs.applyPatchFromClipboard", "Apply Patch from Clipboard…"),
                ],
            ),
            sub(
                "gitUncommittedChanges",
                "Uncommitted Changes",
                vec![
                    act("vcs.shelve", "Shelve Changes…"),
                    act("vcs.showShelf", "Show Shelf"),
                    act("vcs.stash", "Stash Changes…"),
                    act("vcs.unstash", "Unstash Changes…"),
                    act("vcs.rollback", "Rollback…"),
                ],
            ),
            sub(
                "gitCurrentFile",
                "Current File",
                vec![
                    act("vcs.addToVcs", "Add to VCS"),
                    SEP,
                    act("vcs.annotate", "Annotate with Git Blame"),
                    act("vcs.showDiff", "Show Diff"),
                    act("vcs.compareWithBranch", "Compare with Branch or Revision…"),
                    act("vcs.showHistory", "Show History"),
                    act("vcs.showHistoryForSelection", "Show History for Selection"),
                    SEP,
                    act("vcs.localHistory", "Local History"),
                    act("localHistory.putLabel", "Put Label…"),
                ],
            ),
            SEP,
            sub(
                "gitHub",
                "GitHub",
                vec![
                    act("github.pullRequests", "View Pull Requests"),
                    act("github.createPullRequest", "Create Pull Request…"),
                    act("github.checkoutPullRequest", "Checkout Pull Request…"),
                    act("github.openInBrowser", "Open on GitHub"),
                ],
            ),
            act("vcs.manageRemotes", "Manage Remotes…"),
            act("vcs.clone", "Clone…"),
            SEP,
            act("vcs.operations", "VCS Operations Popup…"),
            SEP,
            act("vcs.revert.continue", "Continue Revert"),
            act("vcs.revert.abort", "Abort Revert"),
            act("vcs.cherryPick.continue", "Continue Cherry-Pick"),
            act("vcs.cherryPick.abort", "Abort Cherry-Pick"),
        ],
    ));

    let mut tools = vec![
        act("http.newRequest", "New HTTP Request"),
        act("http.showToolWindow", "HTTP Client"),
        SEP,
        act("db.showToolWindow", "Database"),
        act("db.newDataSource", "New Data Source…"),
        act("db.openConsole", "Open Query Console"),
        SEP,
        act("ssh.startSession", "Start SSH Session…"),
        act("ssh.configurations", "SSH Configurations…"),
        sub(
            "deployment",
            "Deployment",
            vec![
                act("deployment.uploadToDefault", "Upload to Default Server"),
                act("deployment.uploadTo", "Upload to…"),
                act("deployment.downloadFromDefault", "Download from Default Server"),
                act("deployment.downloadFrom", "Download from…"),
                act("deployment.syncWith", "Sync with Deployed…"),
                act("deployment.deleteFromServer", "Delete from Server"),
                SEP,
                act("deployment.toggleAutoUpload", "Automatic Upload"),
                act("deployment.browseRemoteHost", "Browse Remote Host"),
                act("deployment.openInBrowser", "Open in Browser"),
                act("deployment.configure", "Configuration…"),
            ],
        ),
        SEP,
        act("vim.toggle", "Vim"),
        Entry::Dynamic(DYNAMIC_PLUGIN_TOOLS),
    ];
    if !mac {
        tools.extend([
            SEP,
            act("app.installCli", "Install 'openidea' Command"),
            act("app.uninstallCli", "Uninstall 'openidea' Command"),
        ]);
    }
    menus.push(sub("tools", "Tools", tools));

    let mut window = Vec::new();
    if mac {
        window.extend([Entry::Native(Native::Minimize), Entry::Native(Native::Zoom), SEP]);
    }
    window.extend([
        sub(
            "toolWindowLayouts",
            "Tool Window Layouts",
            vec![
                act("layout.save", "Save Current Layout as…"),
                act("layout.restoreDefault", "Restore Default Layout"),
                SEP,
                Entry::Dynamic(DYNAMIC_LAYOUTS),
                SEP,
                act("layout.rename", "Rename Layout…"),
                act("layout.delete", "Delete Layout…"),
            ],
        ),
        SEP,
        sub(
            "activeToolWindow",
            "Active Tool Window",
            vec![
                act("toolwindow.hideActive", "Hide Active Tool Window"),
                act("toolwindow.maximize", "Maximize Tool Window"),
                act("toolwindow.closeContent", "Close Active Tab"),
                sub(
                    "resizeToolWindow",
                    "Resize",
                    vec![
                        act("toolwindow.resizeLeft", "Stretch to Left"),
                        act("toolwindow.resizeRight", "Stretch to Right"),
                        act("toolwindow.resizeUp", "Stretch to Top"),
                        act("toolwindow.resizeDown", "Stretch to Bottom"),
                    ],
                ),
                SEP,
                act("toolwindow.jumpToLast", "Jump to Last Tool Window"),
                act("toolwindow.focusEditor", "Focus Editor"),
            ],
        ),
        sub(
            "editorTabs",
            "Editor Tabs",
            vec![
                act("editor.nextTab", "Select Next Tab"),
                act("editor.prevTab", "Select Previous Tab"),
                SEP,
                act("editor.pinTab", "Pin Tab"),
                act("editor.closeTab", "Close Tab"),
                act("editor.closeOtherTabs", "Close Other Tabs"),
                act("editor.closeAllTabs", "Close All Tabs"),
                act("editor.reopenClosedTab", "Reopen Closed Tab"),
                SEP,
                act("editor.splitRight", "Split Right"),
                act("editor.splitDown", "Split Down"),
                act("editor.unsplit", "Unsplit"),
                act("editor.moveToOppositeGroup", "Move to Opposite Group"),
                act("editor.openInOppositeGroup", "Open in Opposite Group"),
                act("editor.changeSplitOrientation", "Change Splitter Orientation"),
                act("editor.unsplitAll", "Unsplit All"),
                act("editor.nextSplitter", "Go to Next Splitter"),
                act("editor.prevSplitter", "Go to Previous Splitter"),
            ],
        ),
        SEP,
        act("window.new", "New Window"),
        act("window.mergeAllWindows", "Merge All Project Windows"),
        act("window.detachTab", "Move Tab to New Window"),
        act("window.nextTab", "Next Project Tab"),
        act("window.previousTab", "Previous Project Tab"),
    ]);
    // ⌘` is macOS's window cycling; elsewhere Ctrl+` is Quick Switch Scheme and the keymap's
    // Ctrl+Alt+] / Ctrl+Alt+[ switch windows.
    if mac {
        window.extend([
            accel("window.next", "Next Project Window", "Cmd+`"),
            accel("window.previous", "Previous Project Window", "Cmd+Shift+`"),
        ]);
    } else {
        window.extend([act("window.next", "Next Project Window"), act("window.previous", "Previous Project Window")]);
    }
    if mac {
        window.extend([SEP, Entry::Native(Native::BringAllToFront)]);
    }
    menus.push(sub("window", "Window", window));

    let mut help = vec![
        act("navigate.findAction", "Find Action…"),
        act("help.keymapReference", "Keymap Reference"),
        SEP,
        act("help.showLog", show_log),
    ];
    if !mac {
        help.extend([act("app.checkForUpdates", "Check for Updates…"), SEP, act("app.about", "About OpenIDEA")]);
    }
    menus.push(sub("help", "Help", help));
    menus
}

/// What a clicked menu item asks the frontend to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuRoute {
    pub action: String,
    pub arg: Option<String>,
}

/// Maps a clicked item id to the action to run. `None` for ids that are not
/// IDE items (native roles, placeholders).
pub fn route(spec: &[Entry], id: &str) -> Option<MenuRoute> {
    if let Some(path) = id.strip_prefix(RECENT_PREFIX) {
        return Some(MenuRoute { action: OPEN_RECENT_ACTION.into(), arg: Some(path.into()) });
    }
    if let Some(rest) = id.strip_prefix(DYNAMIC_PREFIX) {
        let (action, arg) = match rest.split_once('|') {
            Some((action, arg)) => (action, Some(arg.to_owned())),
            None => (rest, None),
        };
        return (!action.is_empty()).then(|| MenuRoute { action: action.into(), arg });
    }
    action_ids(spec).into_iter().any(|a| a == id).then(|| MenuRoute { action: id.into(), arg: None })
}

/// `entries` without the action `id` (and a separator it leaves doubled or trailing).
pub fn without_action(entries: Vec<Entry>, id: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::with_capacity(entries.len());
    for e in entries {
        match e {
            Entry::Action { id: a, .. } if a == id => {}
            Entry::Submenu { id: sid, label, children } => {
                out.push(Entry::Submenu { id: sid, label, children: without_action(children, id) })
            }
            Entry::Separator if matches!(out.last(), None | Some(Entry::Separator)) => {}
            e => out.push(e),
        }
    }
    if matches!(out.last(), Some(Entry::Separator)) {
        out.pop();
    }
    out
}

/// Every action id reachable from the spec (with duplicates).
pub fn action_ids(spec: &[Entry]) -> Vec<&'static str> {
    fn walk(entries: &[Entry], out: &mut Vec<&'static str>) {
        for e in entries {
            match e {
                Entry::Submenu { children, .. } => walk(children, out),
                Entry::Action { id, .. } => out.push(id),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(spec, &mut out);
    out
}

/// An item of a frontend-listed menu slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicItem {
    pub action: String,
    pub arg: Option<String>,
    pub label: String,
    /// Drawn with a check mark in this state; `None` for a plain item.
    pub checked: Option<bool>,
}

impl DynamicItem {
    /// Native menu item id, decoded again by [`route`].
    pub fn item_id(&self) -> String {
        match &self.arg {
            Some(arg) => format!("{DYNAMIC_PREFIX}{}|{arg}", self.action),
            None => format!("{DYNAMIC_PREFIX}{}", self.action),
        }
    }
}

/// Whether the item `id` performs its own key equivalent: only items with a spec accelerator.
/// A keymap accelerator is drawn for display, and its key reached the menu because the
/// keymap declined it, so an event it fires is dropped.
pub fn fires_natively(spec: &[Entry], id: &str) -> bool {
    spec.iter().any(|entry| match entry {
        Entry::Submenu { children, .. } => fires_natively(children, id),
        Entry::Action { id: action, accel, .. } => *action == id && accel.is_some(),
        _ => false,
    })
}

/// The action an item id stands for, if it is an IDE action item.
pub fn item_action(id: &str) -> Option<&str> {
    if id.starts_with(RECENT_PREFIX) || id == "recent.none" || id.starts_with("submenu:") {
        return None;
    }
    Some(id.strip_prefix(DYNAMIC_PREFIX).map_or(id, |rest| rest.split_once('|').map_or(rest, |(a, _)| a)))
}

/// Whether the item `id` is enabled given the disabled action ids.
pub fn item_enabled(id: &str, disabled: &HashSet<String>) -> bool {
    id != "recent.none" && item_action(id).is_none_or(|a| !disabled.contains(a))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentItem {
    pub path: String,
    pub label: String,
}

pub fn recent_items(recent: &[ide_workspace::RecentProject], home: Option<&Path>) -> Vec<RecentItem> {
    recent
        .iter()
        .take(MAX_RECENT)
        .map(|p| {
            let shown = match home.and_then(|h| p.path.strip_prefix(h).ok()) {
                Some(rest) => format!("~/{}", rest.display()),
                None => p.path.display().to_string(),
            };
            RecentItem { path: p.path.to_string_lossy().into_owned(), label: format!("{}  —  {shown}", p.name) }
        })
        .collect()
}
