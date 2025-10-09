//! This module provides a flexible writer trait, `WWrite`, for serializing
//! data types into different string-based formats, with a primary focus on
//! percent-encoding for URL query strings.
//!
//! It features:
//! - A core `WWrite` trait for writing primitives like booleans, integers, floats, etc.
//! - `PercentEncoding`, a writer that percent-encodes all string data written to it.
//! - `EscapingPercentEncodingWrite`, a writer that first applies JSON-style string
//!   escaping and then percent-encodes the result.

use std::fmt::Write;

use itoa::Integer;
use json_escape::token::{EscapedToken, escape_str};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC};
use ryu::Float;

macro_rules! w_const_chars {
    ($($name:ident $literal:literal;)*) => {
        paste::paste! {
            $(
                #[inline]
                fn [<write_ $name:lower>](&mut self) -> std::fmt::Result {
                    self.write_str($literal)
                }
            )*
        }
    }
}
/// A specialized `Write` trait for serializing common data types.
///
/// This trait extends `std::fmt::Write` with methods for writing primitives
/// like booleans, numbers, and byte arrays, along with common structural
/// characters used in formats like JSON or query strings (e.g., `:`, `,`, `[`).
pub(crate) trait WWrite: Write {
    /// Writes the string "null" to the underlying writer.
    #[inline]
    fn write_null(&mut self) -> std::fmt::Result {
        self.write_str("null")
    }

    /// Writes a boolean value as "true" or "false".
    #[inline]
    fn write_bool(&mut self, value: bool) -> std::fmt::Result {
        if value {
            self.write_str("true")
        } else {
            self.write_str("false")
        }
    }

    /// Writes any integer type that implements `itoa::Integer`.
    #[inline]
    fn write_integer<I: Integer>(&mut self, value: I) -> std::fmt::Result {
        let mut buffer = itoa::Buffer::new();
        let s = buffer.format(value);
        self.write_str(s)
    }

    /// Writes any float type that implements `ryu::Float`.
    ///
    /// # Undefined Behavior
    ///
    /// Calling this with a non-finite (NaN or infinity) float value is
    /// undefined behavior. The caller **must** ensure the value is finite
    /// before calling this method (e.g., by checking `value.is_finite()`).
    #[inline]
    fn write_float<F: Float>(&mut self, value: F) -> std::fmt::Result {
        let mut buffer = ryu::Buffer::new();
        let s = buffer.format_finite(value);
        self.write_str(s)
    }

    /// Writes a slice of bytes as a comma-separated list of numbers enclosed
    /// in square brackets (e.g., `[1,2,3]`).
    #[inline]
    fn write_byte_array(&mut self, value: &[u8]) -> std::fmt::Result {
        self.write_left_sq_bracket()?;
        let mut is_first = true;
        value.iter().try_for_each(|b| {
            if !is_first {
                self.write_comma()?
            }
            self.write_integer(*b)?;
            is_first = false;
            Ok(())
        })?;
        self.write_right_sq_bracket()
    }

    /// Returns a new writer that applies format-specific string escaping.
    ///
    /// This method wraps the current writer in a new writer that performs
    /// JSON-style escaping on any string data written to it.
    ///
    /// **Note**: This operation is not idempotent. Calling `escape()`
    /// multiple times will result in multiple layers of escaping wrappers,
    /// which can lead to unexpected behavior or, in extreme cases of recursive
    /// type definitions, a compiler stack overflow.
    #[inline]
    fn escape(&mut self) -> impl WWrite
    where
        Self: Sized,
    {
        struct Escape<'a, W>(&'a mut W);

        impl<W: WWrite> Write for Escape<'_, W> {
            #[inline]
            fn write_str(&mut self, s: &str) -> std::fmt::Result {
                write!(self.0, "{}", escape_str(s))
            }
        }

        impl<W: WWrite> WWrite for Escape<'_, W> {}

        Escape(self)
    }

    /// Returns a wrapped writer to prevent compiler recursion overflows.
    ///
    /// ### The Problem
    /// When serializing deeply nested or recursive data structures, the Rust
    /// compiler can hit its recursion limit during trait evaluation. This
    /// happens because each level of recursion can add another mutable
    /// reference to the writer's type (e.g., `W`, `&mut W`, `&mut &mut W`, ...),
    /// and the compiler must prove that `WWrite` is implemented for each new type
    /// in the chain. For a truly recursive type, this chain is infinite,
    /// leading to a compiler error.
    ///
    /// ### The Solution
    /// This method acts as a "stop-gap" by wrapping the writer in a newtype,
    /// `AsMut`. This wrapper implements `WWrite` by forwarding all calls to
    /// the inner writer. Crucially, when `as_mut()` is called on the wrapper
    /// itself, it returns itself instead of creating a new layer. This breaks
    /// the infinite chain of nested types and allows the compiler to terminate
    /// its trait resolution analysis.
    ///
    /// See the `trait_evaluation_overflow` test for a concrete example of the
    /// issue this method solves.
    ///
    /// # Safety
    /// This function uses `std::mem::transmute` to cast `&mut Self` to `&mut AsMut<Self>`.
    /// This is safe because the `AsMut` struct is a `#[repr(transparent)]`
    /// newtype wrapper around `W`, guaranteeing that a pointer to `AsMut<W>`
    /// has the same layout and ABI as a pointer to `W`.
    #[inline]
    fn as_mut(&mut self) -> impl WWrite
    where
        Self: Sized,
    {
        #[repr(transparent)]
        struct AsMut<W>(W);

        impl<W: WWrite> Write for &mut AsMut<W> {
            #[inline]
            fn write_str(&mut self, s: &str) -> std::fmt::Result {
                self.0.write_str(s)
            }
        }

        macro_rules! w_mut_const_chars {
            ($($name:ident)*) => {
                paste::paste! {
                    $(
                        #[inline]
                        fn [<write_ $name:lower>](&mut self) -> std::fmt::Result {
                            self.0.[<write_ $name:lower>]()
                        }
                    )*
                }
            }
        }

        impl<W: WWrite> WWrite for &mut AsMut<W> {
            #[inline]
            fn write_null(&mut self) -> std::fmt::Result {
                self.0.write_null()
            }

            #[inline]
            fn write_bool(&mut self, value: bool) -> std::fmt::Result {
                self.0.write_bool(value)
            }

            #[inline]
            fn write_integer<I: Integer>(&mut self, value: I) -> std::fmt::Result {
                self.0.write_integer(value)
            }

            #[inline]
            fn write_float<F: Float>(&mut self, value: F) -> std::fmt::Result {
                self.0.write_float(value)
            }

            #[inline]
            fn write_byte_array(&mut self, value: &[u8]) -> std::fmt::Result {
                self.0.write_byte_array(value)
            }

            #[inline]
            fn as_mut(&mut self) -> impl WWrite {
                // Stop the recursive type nesting by returning the current wrapper.
                let me: &mut AsMut<W> = self;
                me
            }

            #[inline]
            fn escape(&mut self) -> impl WWrite {
                self.0.escape()
            }

            w_mut_const_chars! {
                colon quote comma
                left_bracket right_bracket
                left_sq_bracket right_sq_bracket
            }
        }

        unsafe { std::mem::transmute::<&mut Self, &mut AsMut<Self>>(self) }
    }

    w_const_chars! {
        colon ":";
        quote "\"";
        comma ",";
        left_bracket "{";
        right_bracket "}";
        left_sq_bracket "[";
        right_sq_bracket "]";
    }
}

macro_rules! const_chars {
    ($($name:ident $encoding:literal $literal:literal;)*) => {
        paste::paste! {
            $(
                #[inline]
                fn [<write_ $name:lower>](&mut self) -> std::fmt::Result {
                    const [<$name:upper>]: &str = $encoding;

                    self.w.write_str([<$name:upper>])
                }
            )*
        }
    }
}

/// A custom `AsciiSet` for form-urlencoded values.
///
/// This set defines which characters should be percent-encoded. According to
/// RFC 3986, alphanumeric characters and `*-._~` are considered "unreserved"
/// and do not require encoding. This set includes all other characters.
const FORM_URLENCODING_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// A writer that percent-encodes string data.
///
/// This struct wraps another writer (`W`) and encodes any string written to it
/// using the `FORM_URLENCODING_ENCODE_SET`. Primitives like numbers and booleans
/// are written directly without encoding, as they are already URL-safe.
#[derive(Debug)]
pub(crate) struct PercentEncoding<W> {
    w: W,
}

impl<W> PercentEncoding<W> {
    /// Creates a new `PercentEncoding` writer wrapping `w`.
    #[inline(always)]
    pub fn new(w: W) -> Self {
        Self { w }
    }

    /// The percent-encoded representation of a double quote (`"`).
    const QUOTE: &'static str = "%22";
}

impl<W> Write for PercentEncoding<W>
where
    W: Write,
{
    #[inline(always)]
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let mut encoded = percent_encoding::utf8_percent_encode(s, FORM_URLENCODING_ENCODE_SET);
        encoded.try_for_each(|s| self.w.write_str(s))?;
        Ok(())
    }
}

impl<W: Write> WWrite for PercentEncoding<W> {
    #[inline]
    fn write_null(&mut self) -> std::fmt::Result {
        // ENCODING: Needs no encoding.
        self.w.write_str("null")
    }

    #[inline]
    fn write_bool(&mut self, value: bool) -> std::fmt::Result {
        // ENCODING: Needs no encoding.
        if value {
            self.w.write_str("true")
        } else {
            self.w.write_str("false")
        }
    }

    #[inline]
    fn write_integer<I: Integer>(&mut self, value: I) -> std::fmt::Result {
        let mut buffer = itoa::Buffer::new();
        let s = buffer.format(value);
        // ENCODING: Needs no encoding. It only produces 0-9 and -
        self.w.write_str(s)
    }

    const_chars! {
        colon "%3A" ":";
        quote "%22" "\"";
        comma "%2C" ",";
        left_bracket "%7B" "{";
        right_bracket "%7D" "}";
        left_sq_bracket "%5B" "[";
        right_sq_bracket "%5D" "]";
    }

    #[inline]
    fn escape(&mut self) -> impl WWrite {
        EscapingPercentEncodingWrite { inner: self }
    }
}

impl<W> std::io::Write for PercentEncoding<W>
where
    W: Write,
{
    #[inline(always)]
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        let mut encoded = percent_encoding::percent_encode(b, FORM_URLENCODING_ENCODE_SET);
        encoded
            .try_for_each(|s| self.w.write_str(s))
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
        Ok(b.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A writer that first applies JSON-style string escaping and then percent-encodes the result.
///
/// This is useful for serializing string values that are themselves expected
/// to be valid JSON strings, but embedded within a URL. For example, writing
/// the string `a"b\c` would result in `a%5C%22b%5C%5Cc`.
#[derive(Debug)]
pub(crate) struct EscapingPercentEncodingWrite<'a, W> {
    inner: &'a mut PercentEncoding<W>,
}

impl<W> Write for EscapingPercentEncodingWrite<'_, W>
where
    W: Write,
{
    #[inline]
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let mut escaped = escape_str(s);
        escaped.try_for_each(|escaped_token| {
            match escaped_token {
                EscapedToken::Literal(literal) => {
                    // This literal part is already safe from JSON's perspective,
                    // but still needs percent-encoding.
                    self.inner.write_str(literal)
                }
                EscapedToken::Escaped(escaped) => {
                    const REVERSE_SOLIDUS: &str = "%5C";

                    self.inner.w.write_str(REVERSE_SOLIDUS)?;

                    match &escaped[1..] {
                        "\"" => self.inner.w.write_str(PercentEncoding::<W>::QUOTE),
                        "\\" => self.inner.w.write_str(REVERSE_SOLIDUS),
                        // `json_escape` doesn't escape '/', but we handle it defensively.
                        "/" => {
                            const SOLIDUS: &str = "%2F";

                            self.inner.w.write_str(SOLIDUS)
                        }
                        other =>
                        // ENCODING: Other JSON escapes like '\b', '\f', '\n', '\r', '\t'
                        // are valid in the URL set and do not need further encoding.
                        {
                            self.inner.w.write_str(other)
                        }
                    }
                }
            }
        })
    }
}

macro_rules! w_ep_const_chars {
    ($($name:ident)*) => {
        paste::paste! {
            $(
                #[inline]
                fn [<write_ $name:lower>](&mut self) -> std::fmt::Result {
                    self.inner.[<write_ $name:lower>]()
                }
            )*
        }
    }
}

impl<W: Write> WWrite for EscapingPercentEncodingWrite<'_, W> {
    #[inline]
    fn write_null(&mut self) -> std::fmt::Result {
        // Primitives are not JSON-escaped.
        self.inner.write_null()
    }

    #[inline]
    fn write_bool(&mut self, value: bool) -> std::fmt::Result {
        self.inner.write_bool(value)
    }

    #[inline]
    fn write_integer<I: Integer>(&mut self, value: I) -> std::fmt::Result {
        self.inner.write_integer(value)
    }

    #[inline]
    fn write_float<F: Float>(&mut self, value: F) -> std::fmt::Result {
        self.inner.write_float(value)
    }

    #[inline]
    fn write_byte_array(&mut self, value: &[u8]) -> std::fmt::Result {
        self.inner.write_byte_array(value)
    }

    w_ep_const_chars! {
        colon quote comma
        left_bracket right_bracket
        left_sq_bracket right_sq_bracket
    }
    // We shouldn't prevent escape on escape like we did before. If it's a bug,
    // let's catch it.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trait_evaluation_overflow() {
        use std::collections::HashMap;
        // ISSUE:
        //     error[E0275]: overflow evaluating the requirement `&mut std::string::String: std::fmt::Write`
        //     |
        //     = help: consider increasing the recursion limit by adding a `#![recursion_limit = "256"]` attribute to your crate (`serde_metaform`)
        //     = note: required for `&mut &mut std::string::String` to implement `std::fmt::Write`
        //     = note: 126 redundant requirements hidden
        //     = note: required for `&mut &mut &mut &mut &mut &mut &mut &mut &mut &mut &mut &mut &mut &mut &mut ...` to implement `std::fmt::Write`
        // note: required for `EscapingPercentEncodingWrite<&mut &mut &mut &mut &mut &mut &mut &mut &mut ...>` to implement `write::WWrite`
        //    --> src/write.rs:278:16
        //     |
        // 278 | impl<W: Write> WWrite for EscapingPercentEncodingWrite<W> {
        //     |         -----  ^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        //     |         |
        //     |         unsatisfied trait bound introduced here
        // note: required for `KeySerializerNoQuotes<EscapingPercentEncodingWrite<&mut &mut &mut &mut ...>>` to implement `tests::_::_serde::Serializer`
        //    --> src/json.rs:562:17
        //     |
        // 562 | impl<W: WWrite> ser::Serializer for KeySerializerNoQuotes<W> {
        //     |         ------  ^^^^^^^^^^^^^^^     ^^^^^^^^^^^^^^^^^^^^^^^^
        //     |         |
        //     |         unsatisfied trait bound introduced here
        //
        // In the former version, I pulled a trick to avoid work and it resulted in this (even without the trick, we'd get this)
        // The original issue was discovered with serde_json::Value whose sheer existence is statically recursive.
        let _ = crate::to_string(&serde_json::json!(()));

        // Adding this incase to make it clearer what this is.
        #[derive(serde::Serialize)]
        struct NestedSerialize {
            value: HashMap<String, NestedSerialize>,
        }

        let _ = crate::to_string(&NestedSerialize {
            value: HashMap::new(),
        });
    }

    /// Tests the `PercentEncoding` writer.
    #[test]
    fn test_percent_encoding_writer() {
        let mut buf = String::new();
        let mut writer = PercentEncoding::new(&mut buf);

        writer.write_str("hello world").unwrap();
        assert_eq!(buf, "hello%20world");
        buf.clear();

        let mut writer = PercentEncoding::new(&mut buf);
        writer.write_null().unwrap();
        assert_eq!(buf, "null");
        buf.clear();

        let mut writer = PercentEncoding::new(&mut buf);
        writer.write_bool(true).unwrap();
        assert_eq!(buf, "true");
        buf.clear();

        let mut writer = PercentEncoding::new(&mut buf);
        writer.write_integer(-123).unwrap();
        assert_eq!(buf, "-123");
        buf.clear();

        let mut writer = PercentEncoding::new(&mut buf);
        writer.write_float(45.67).unwrap();
        assert_eq!(writer.w, "45.67");
        buf.clear();

        let mut writer = PercentEncoding::new(&mut buf);
        writer.write_colon().unwrap();
        writer.write_comma().unwrap();
        writer.write_left_bracket().unwrap();
        writer.write_quote().unwrap();
        assert_eq!(writer.w, "%3A%2C%7B%22");
        writer.w.clear();

        writer.write_byte_array(&[10, 20, 30]).unwrap();
        assert_eq!(buf, "%5B10%2C20%2C30%5D");
    }

    /// Tests the `EscapingPercentEncodingWrite` writer.
    #[test]
    fn test_escaping_percent_encoding_writer() {
        let buf = String::new();
        let mut writer = EscapingPercentEncodingWrite {
            inner: &mut PercentEncoding { w: buf },
        };

        // Simple string needs percent encoding but no JSON escaping.
        writer.write_str("key=value").unwrap();
        assert_eq!(writer.inner.w, "key%3Dvalue");
        writer.inner.w.clear();

        // String with characters that need JSON escaping (`"` and `\`).
        // " -> \" -> %5C%22
        // \ -> \\ -> %5C%5C
        writer.write_str("\"hello\\world\"").unwrap();
        assert_eq!(writer.inner.w, "%5C%22hello%5C%5Cworld%5C%22");
        writer.inner.w.clear();

        // String with both types of characters.
        // ` ` needs percent encoding.
        // `"` needs JSON and then percent encoding.
        writer.write_str("a \"quoted\" string").unwrap();
        assert_eq!(writer.inner.w, "a%20%5C%22quoted%5C%22%20string");
        writer.inner.w.clear();

        // Primitives should not be escaped or encoded.
        writer.write_null().unwrap();
        assert_eq!(writer.inner.w, "null");
        writer.inner.w.clear();

        writer.write_integer(999).unwrap();
        assert_eq!(writer.inner.w, "999");
        writer.inner.w.clear();
    }

    /// Tests the interaction of `as_mut` with the writers.
    #[test]
    fn test_as_mut_functionality() {
        let mut buf = String::new();
        let mut writer = PercentEncoding::new(&mut buf);

        // Use as_mut to pass a writer to a helper function.
        fn write_some_data(w: &mut impl WWrite) {
            w.write_left_bracket().unwrap();
            w.write_integer(1).unwrap();
            w.write_comma().unwrap();
            w.write_integer(2).unwrap();
            w.write_right_bracket().unwrap();
        }

        write_some_data(&mut writer);
        assert_eq!(buf, "%7B1%2C2%7D");
    }

    /// Tests `write_byte_array` specifically.
    #[test]
    fn test_write_byte_array() {
        let buf = String::new();
        let mut writer = PercentEncoding::new(buf);

        writer.write_byte_array(&[]).unwrap();
        assert_eq!(writer.w, "%5B%5D");
        writer.w.clear();

        writer.write_byte_array(&[255]).unwrap();
        assert_eq!(writer.w, "%5B255%5D");
        writer.w.clear();

        writer.write_byte_array(&[1, 2, 128]).unwrap();
        assert_eq!(writer.w, "%5B1%2C2%2C128%5D");
    }
}
