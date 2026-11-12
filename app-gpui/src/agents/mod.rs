//! Coding agents (Claude Code, Codex, Gemini CLI over ACP, run by `ide-agent`): the Agents tool window
//! lists tasks by what they need from the user, a task opens as a conversation (an editor tab, or docked
//! in the tool window), and New Task is a page of its own. These are the feature's models; views
//! render them and carry out the effects they return.

pub mod builtin_agents;
pub mod cards;
pub mod composer;
pub mod config_bar;
pub mod config_pickers;
pub mod conversation;
pub mod follow;
pub mod login_hint;
pub mod markdown;
pub mod markdown_images;
pub mod message_editor;
pub mod model;
pub mod permission;
pub mod registry;
pub mod reply_actions;
pub mod session;
pub mod store;
pub mod task_actions;
pub mod task_list;

#[cfg(test)]
pub(crate) mod fixtures;
