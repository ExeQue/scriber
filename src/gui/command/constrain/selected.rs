//! A constraint command started with something selected applies to the selection at once
//! (a dimension still asks where it goes and its value).

use super::{App, Cmd, Step};
use crate::model::constraints::selection::selection_refs;

impl App {
    pub(super) fn constrain_selection(&mut self, cmd: Cmd) {
        let found = {
            let d = self.doc.lock().unwrap();
            if d.selection.is_empty() {
                return;
            }
            selection_refs(&d.s, cmd.kind(), &d.selection)
        };
        let sets = match found {
            Ok(sets) => sets,
            Err(what) => return self.cmdline.log(format!("{} needs {what} selected; pick the objects instead.", cmd.title())),
        };
        if cmd == Cmd::DcLinear || cmd.kind().is_dimension() {
            self.cmdline.constrain.refs = sets.into_iter().next().unwrap_or_default();
            return self.set_step(Step::Location);
        }
        let n = sets.len();
        let result = self.change(|d| {
            for refs in sets {
                d.add_constraint(cmd.kind(), refs)?;
            }
            Ok(String::new())
        });
        self.end();
        let msg = match result {
            Ok(_) => format!("{}: {n} constraint(s) added to the selection.", cmd.title()),
            Err(e) => format!("{}: {e}", cmd.title()),
        };
        self.cmdline.log(msg.clone());
        self.message = msg;
    }
}
