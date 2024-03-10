use std::fmt::Debug;

pub struct DebugInline<'a>(pub &'a str);

impl<'a> Debug for DebugInline<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
