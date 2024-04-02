use std::fmt::Debug;

/// Writes the contained `&str` directly (without `"`s) to the Formatter.
pub struct DebugInlineStr<'a>(pub &'a str);

impl<'a> Debug for DebugInlineStr<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// Wrapper that formats its inner value with `Debug` but with alternate overriden to `false`.
///
/// That is, with `format!` string `{:?}`, not `{:#?}`, even if this struct is formatted
/// with `{:#?}`.
pub struct DebugNoAlternate<'a, T: Debug>(pub &'a T);

impl<'a, T: Debug> Debug for DebugNoAlternate<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let d = format!("{:?}", self.0);
        f.write_str(&d)
    }
}

pub fn opt_field<'a, 'b: 'a>(debug_struct: &mut std::fmt::DebugStruct<'a, 'b>,
                             name: &str, value: &Option<impl Debug>)
{
    let Some(ref value) = value else {
        return;
    };

    debug_struct.field(name, value);
}
