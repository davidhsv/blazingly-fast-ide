//! Code analysis: Inspect Code (batch language-server diagnostics over a scope, grouped like IDEA's
//! Inspection Results), Locate Duplicates and Search / Replace Structurally, over the scopes of IDEA's
//! "Specify Scope" dialog.

pub mod duplicates;
pub mod inspect;
pub mod scope;
pub mod structural;
pub mod tree;
