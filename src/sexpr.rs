use std::fmt::Write as _;
use ustr::Ustr;

/// Canonical S-expression writer for AST nodes and primitive values.
///
/// The output format is deterministic:
///
/// - Sum nodes write `(Constructor (field child) ...)` with fields in ASDL declaration order.
/// - Fieldless constructors write bare atoms (for example, `Add` or `Null`).
/// - Product nodes write `(TypeName (field child) ...)`.
/// - Optional values write `None` or the inner value; slices write `[a b c]`.
/// - Strings use double quotes with C-style escapes; integers use decimal digits;
///   booleans write `true` or `false`.
///
/// # Examples
///
/// ```
/// use dbt_asdl::Sexpr;
///
/// assert_eq!(42_i32.to_sexpr(), "42");
/// assert_eq!(String::from("hello\nworld").to_sexpr(), "\"hello\\nworld\"");
/// assert_eq!(Some(true).to_sexpr(), "true");
/// assert_eq!(None::<i32>.to_sexpr(), "None");
/// ```
pub trait Sexpr {
    /// Appends the S-expression representation of `self` to `out`.
    fn write_sexpr(&self, out: &mut String);

    /// Returns the S-expression representation of `self` as a new [`String`].
    fn to_sexpr(&self) -> String
    where
        Self: Sized,
    {
        let mut out = String::new();
        self.write_sexpr(&mut out);
        out
    }
}

impl Sexpr for String {
    fn write_sexpr(&self, out: &mut String) {
        write_escaped(self, out);
    }
}

impl Sexpr for str {
    fn write_sexpr(&self, out: &mut String) {
        write_escaped(self, out);
    }
}

impl Sexpr for Ustr {
    fn write_sexpr(&self, out: &mut String) {
        write_escaped(self, out);
    }
}

macro_rules! impl_sexpr_int {
    ($($t:ty),*) => {
        $(
            impl Sexpr for $t {
                fn write_sexpr(&self, out: &mut String) {
                    write!(out, "{self}").expect("writing to a String cannot fail");
                }
            }
        )*
    };
}

impl_sexpr_int!(u8, i32, i64);

impl Sexpr for bool {
    fn write_sexpr(&self, out: &mut String) {
        out.push_str(if *self { "true" } else { "false" });
    }
}

impl Sexpr for &str {
    fn write_sexpr(&self, out: &mut String) {
        write_escaped(self, out);
    }
}

impl<T: Sexpr> Sexpr for Option<T> {
    fn write_sexpr(&self, out: &mut String) {
        match self {
            None => out.push_str("None"),
            Some(v) => v.write_sexpr(out),
        }
    }
}

impl<T: Sexpr> Sexpr for [T] {
    fn write_sexpr(&self, out: &mut String) {
        write_slice_sexpr(self, out);
    }
}

fn write_slice_sexpr<T: Sexpr>(slice: &[T], out: &mut String) {
    out.push('[');
    for (i, v) in slice.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        v.write_sexpr(out);
    }
    out.push(']');
}

pub(crate) fn write_escaped(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                write!(out, "\\u{{{:x}}}", c as u32).expect("writing to a String cannot fail");
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A borrowed slice of interned identifiers, identical in both arena and owned trees.
pub type IdentList<'a> = &'a [Ustr];

/// A borrowed list of strings in either tree representation.
///
/// The arena tree stores bump-allocated `&'a str` slices, and the owned tree
/// stores `Box<str>` slices. Both representations yield `&'a str` items.
///
/// # Examples
///
/// ```
/// use dbt_asdl::{Sexpr, StringList};
///
/// let slice = ["select", "from"];
/// let list = StringList::Arena(&slice);
/// assert_eq!(list.len(), 2);
/// assert_eq!(list.get(0), Some("select"));
/// assert_eq!(list.to_sexpr(), "[\"select\" \"from\"]");
/// ```
#[derive(Clone, Copy, Debug)]
pub enum StringList<'a> {
    /// Borrowed slice of owned `Box<str>` elements from an owned tree.
    Owned(&'a [Box<str>]),
    /// Borrowed slice of arena-allocated `&'a str` elements from an arena tree.
    Arena(&'a [&'a str]),
}

impl<'a> StringList<'a> {
    /// Returns the number of strings in the list.
    pub fn len(self) -> usize {
        match self {
            StringList::Owned(s) => s.len(),
            StringList::Arena(s) => s.len(),
        }
    }

    /// Returns `true` if the list contains no strings.
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// Returns the string at `i`, or `None` if `i` is out of bounds.
    pub fn get(self, i: usize) -> Option<&'a str> {
        match self {
            StringList::Owned(s) => s.get(i).map(|b| &**b),
            StringList::Arena(s) => s.get(i).copied(),
        }
    }

    /// Returns an iterator over the strings in the list.
    pub fn iter(self) -> StringListIter<'a> {
        StringListIter {
            list: self,
            next: 0,
        }
    }
}

/// An iterator over the strings in a [`StringList`].
pub struct StringListIter<'a> {
    list: StringList<'a>,
    next: usize,
}

impl<'a> Iterator for StringListIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.list.get(self.next)?;
        self.next += 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.list.len() - self.next;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for StringListIter<'_> {}

impl<'a> IntoIterator for StringList<'a> {
    type Item = &'a str;
    type IntoIter = StringListIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl Sexpr for StringList<'_> {
    fn write_sexpr(&self, out: &mut String) {
        out.push('[');
        for (i, item) in self.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            item.write_sexpr(out);
        }
        out.push(']');
    }
}
