//! Database tools: data sources for SQLite / PostgreSQL / MySQL-MariaDB / SQL Server / ClickHouse /
//! Oracle, a schema tree, query consoles with SQL support and result grids, and a table data
//! editor. Connections, introspection and statements run in the backend (`ide-db`); these are the
//! models the views drive, with no GPUI types in them.

pub mod console_view;
pub mod dialog_parts;
pub mod dialogs;
pub mod export;
pub mod grid;
pub mod grid_view;
pub mod history_view;
pub mod import_dialog;
pub mod model;
pub mod modify_table_dialog;
pub mod nodes;
pub mod objects;
pub mod plan_view;
pub mod result_editing;
pub mod result_session;
pub mod result_view;
pub mod results_view;
pub mod source_dialog;
pub mod sql;
pub mod sql_filter;
pub mod store;
pub mod table_editor;
pub mod table_editor_view;
pub mod tree;
pub mod tree_view;
pub mod truncate_dialog;
pub mod vendors;
