//! RA Proc Macro Server
//!
//! This library is able to call compiled Rust custom derive dynamic libraries on arbitrary code.
//! The general idea here is based on <https://github.com/fedochet/rust-proc-macro-expander>.
//!
//! But we adapt it to better fit RA needs:
//!
//! * We use `tt` for proc-macro `TokenStream` server, it is easier to manipulate and interact with
//!   RA than `proc-macro2` token stream.
//! * By **copying** the whole rustc `lib_proc_macro` code, we are able to build this with `stable`
//!   rustc rather than `unstable`. (Although in general ABI compatibility is still an issue)…
#![allow(unreachable_pub)]

mod dylib;
mod abis;

use std::{
    collections::{hash_map::Entry, HashMap},
    env, fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use proc_macro_api::{
    msg::{ExpandMacro, FlatTree, PanicMessage},
    ProcMacroKind,
};

#[derive(Default)]
pub(crate) struct ProcMacroSrv {
    expanders: HashMap<(PathBuf, SystemTime), dylib::Expander>,
}

impl ProcMacroSrv {
    pub fn expand(&mut self, task: ExpandMacro) -> Result<FlatTree, PanicMessage> {
        let expander = self.expander(task.lib.as_ref()).map_err(|err| {
            debug_assert!(false, "should list macros before asking to expand");
            PanicMessage(format!("failed to load macro: {}", err))
        })?;

        let mut prev_env = HashMap::new();
        for (k, v) in &task.env {
            prev_env.insert(k.as_str(), env::var_os(k));
            env::set_var(k, v);
        }
        let prev_working_dir = match task.current_dir {
            Some(dir) => {
                let prev_working_dir = std::env::current_dir().ok();
                if let Err(err) = std::env::set_current_dir(&dir) {
                    eprintln!("Failed to set the current working dir to {}. Error: {:?}", dir, err)
                }
                prev_working_dir
            }
            None => None,
        };

        let macro_body = task.macro_body.to_subtree();
        let attributes = task.attributes.map(|it| it.to_subtree());
        let result = expander
            .expand(&task.macro_name, &macro_body, attributes.as_ref())
            .map(|it| FlatTree::new(&it));

        for (k, _) in &task.env {
            match &prev_env[k.as_str()] {
                Some(v) => env::set_var(k, v),
                None => env::remove_var(k),
            }
        }
        if let Some(dir) = prev_working_dir {
            if let Err(err) = std::env::set_current_dir(&dir) {
                eprintln!(
                    "Failed to set the current working dir to {}. Error: {:?}",
                    dir.display(),
                    err
                )
            }
        }

        result.map_err(PanicMessage)
    }

    pub(crate) fn list_macros(
        &mut self,
        dylib_path: &Path,
    ) -> Result<Vec<(String, ProcMacroKind)>, String> {
        let expander = self.expander(dylib_path)?;
        Ok(expander.list_macros())
    }

    fn expander(&mut self, path: &Path) -> Result<&dylib::Expander, String> {
        let time = fs::metadata(path).and_then(|it| it.modified()).map_err(|err| {
            format!("Failed to get file metadata for {}: {:?}", path.display(), err)
        })?;

        Ok(match self.expanders.entry((path.to_path_buf(), time)) {
            Entry::Vacant(v) => v.insert(dylib::Expander::new(path).map_err(|err| {
                format!("Cannot create expander for {}: {:?}", path.display(), err)
            })?),
            Entry::Occupied(e) => e.into_mut(),
        })
    }
}

pub mod cli;

#[cfg(test)]
mod tests;
