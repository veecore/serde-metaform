//! A Composable, Streaming JSON `serde::Serializer`.
//!
//! ## A Necessary Invention
//!
//! At first glance, this module looks like a re-implementation of `serde_json`.
//! So, why does it exist? The answer lies in one word: **composition**.
//!
//! The main serializer in this crate needs to build a hybrid format: `key=json_value&...`.
//! To do this, it needs to delegate the `json_value` part to a JSON serializer. The natural
//! choice, `serde_json`, is unfortunately not designed to be used as a sub-component
//! in this way.
//!
//! When you ask `serde_json` to serialize a sequence (like a `Vec`), its `serialize_seq`
//! method returns a helper struct (`Compound<'a, ...>`) which holds a mutable borrow
//! with a lifetime (`'a`) back to the original `serde_json::Serializer`. This makes it
//! impossible for our main serializer to create a temporary `serde_json::Serializer`,
//! start a sequence, and then return the sequence helper. The borrow checker would
//! correctly stop us from returning a value that borrows a temporary that's about to be
//! destroyed.
//!
//! This module was written to overcome that specific architectural challenge. It provides
//! a `serde::Serializer` for JSON that **is** designed to be composable, allowing it to
//! act as a "sub-serializer" that can be freely passed around and used to render JSON
//! into the writers managed by our main form-encoder.
//!
//! So, this isn't a replacement for `serde_json`—it's a specialized tool that makes
//! the parent module's hybrid format possible. If you know a way to solve this
//! composition problem without this module, we welcome a PR! 🙏

use std::fmt::Write;

use serde::{Serialize, ser};

use crate::{
    error::{Error, float_key_must_be_finite, key_must_be_string},
    error_unsupported,
    write::WWrite,
};

pub struct SeqSerializer<W> {
    output: W,
    is_first: bool,
}

impl<W: WWrite> SeqSerializer<W> {
    #[inline]
    pub(crate) fn new(mut output: W, _len: Option<usize>) -> Result<Self, Error> {
        output.write_left_sq_bracket()?;
        Ok(SeqSerializer {
            output,
            is_first: true,
        })
    }
}

impl<W: WWrite> ser::SerializeSeq for SeqSerializer<W> {
    type Ok = ();

    type Error = Error;

    #[inline]
    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        if !self.is_first {
            self.output.write_comma()?;
        }
        value.serialize(JsonSerializer {
            output: self.output.as_mut(),
            is_top_level_value: false,
        })?;
        self.is_first = false;
        Ok(())
    }

    #[inline]
    fn end(mut self) -> Result<Self::Ok, Self::Error> {
        Ok(self.output.write_right_sq_bracket()?)
    }
}

pub type TupleSerializer<W> = SeqSerializer<W>;

impl<W: WWrite> ser::SerializeTuple for TupleSerializer<W> {
    type Ok = ();

    type Error = Error;

    #[inline]
    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeSeq::serialize_element(self, value)
    }

    #[inline]
    fn end(self) -> Result<Self::Ok, Self::Error> {
        ser::SerializeSeq::end(self)
    }
}

pub type TupleStructSerializer<W> = SeqSerializer<W>;

impl<W: WWrite> ser::SerializeTupleStruct for TupleStructSerializer<W> {
    type Ok = ();

    type Error = Error;

    #[inline]
    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeSeq::serialize_element(self, value)
    }

    #[inline]
    fn end(self) -> Result<Self::Ok, Self::Error> {
        ser::SerializeSeq::end(self)
    }
}

/// Serializer for enum tuple variants, e.g., `Enum::Variant(a, b)`.
/// Serializes to `{"Variant":[a,b]}`.
pub struct TupleVariantSerializer<W> {
    inner: SeqSerializer<W>,
}

impl<W: WWrite> TupleVariantSerializer<W> {
    #[inline]
    pub fn new(mut output: W, variant: &'static str, len: usize) -> Result<Self, Error> {
        // Write the outer map structure `{"variant":`
        {
            use ser::SerializeMap as _;

            let mut map = MapSerializer::new(output.as_mut(), Some(1))?;
            map.serialize_key(variant)?;
        }
        // Now, start the inner sequence.
        let seq = SeqSerializer::new(output, Some(len))?;
        Ok(Self { inner: seq })
    }
}

impl<W: WWrite> ser::SerializeTupleVariant for TupleVariantSerializer<W> {
    type Ok = ();

    type Error = Error;

    #[inline]
    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        // Serialize each element into the sequence: `[elem1, elem2, ...`
        use ser::SerializeSeq as _;
        self.inner.serialize_element(value)
    }

    #[inline]
    fn end(mut self) -> Result<Self::Ok, Self::Error> {
        // Close the sequence `]` and the outer map `}`
        self.inner.output.write_right_sq_bracket()?;
        self.inner.output.write_right_bracket()?;
        Ok(())
    }
}

/// Serializer for enum struct variants, e.g., `Enum::Variant { a, b }`.
/// Serializes to `{"Variant":{"a":a,"b":b}}`.
pub struct StructVariantSerializer<W: WWrite> {
    inner: StructSerializer<W>,
}

impl<W: WWrite> StructVariantSerializer<W> {
    #[inline]
    pub fn new(mut output: W, variant: &'static str, len: usize) -> Result<Self, Error> {
        // Write the outer map structure `{"variant":`
        {
            use ser::SerializeMap as _;

            let mut map = MapSerializer::new(output.as_mut(), Some(1))?;
            map.serialize_key(variant)?;
        }
        // Now, start the inner struct map.
        let map = StructSerializer::new(output, Some(len))?;
        Ok(Self { inner: map })
    }
}

impl<W: WWrite> ser::SerializeStructVariant for StructVariantSerializer<W> {
    type Ok = ();

    type Error = Error;

    #[inline]
    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        use ser::SerializeStruct as _;
        // Serialize each field into the inner struct map.
        self.inner.serialize_field(key, value)
    }

    #[inline]
    fn end(mut self) -> Result<Self::Ok, Self::Error> {
        // Close the inner struct map `}` and the outer map `}`.
        self.inner.output.write_right_bracket()?;
        self.inner.output.write_right_bracket()?;
        Ok(())
    }
}

pub struct MapSerializer<W: WWrite> {
    output: W,
    is_first: bool,
}

impl<W: WWrite> MapSerializer<W> {
    #[inline]
    pub fn new(mut output: W, _len: Option<usize>) -> Result<Self, Error> {
        output.write_left_bracket()?;
        Ok(Self {
            output,
            is_first: true,
        })
    }
}

impl<W: WWrite> ser::SerializeMap for MapSerializer<W> {
    type Ok = ();

    type Error = Error;

    #[inline]
    fn serialize_key<T>(&mut self, key: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        if !self.is_first {
            self.output.write_comma()?;
        }
        // Write the key, quoted and escaped.
        self.output.write_quote()?;
        key.serialize(KeySerializerNoQuotes {
            output: self.output.escape(),
        })?;
        self.output.write_quote()?;
        self.output.write_colon()?;
        self.is_first = false;
        Ok::<(), Error>(())
    }

    #[inline]
    fn serialize_value<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(JsonSerializer {
            output: self.output.as_mut(),
            is_top_level_value: false,
        })
    }

    #[inline]
    fn end(mut self) -> Result<Self::Ok, Self::Error> {
        self.output.write_right_bracket()?;
        Ok(())
    }
}

pub type StructSerializer<W> = MapSerializer<W>;

impl<W: WWrite> ser::SerializeStruct for StructSerializer<W> {
    type Ok = ();

    type Error = Error;

    #[inline]
    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        use ser::SerializeMap as _;
        // A struct field is just a map entry.
        self.serialize_entry(key, value)
    }

    #[inline]
    fn end(self) -> Result<Self::Ok, Self::Error> {
        ser::SerializeMap::end(self)
    }
}

pub struct JsonSerializer<W: WWrite> {
    pub(crate) output: W,
    /// If true, strings are not quoted or escaped.
    pub(crate) is_top_level_value: bool,
}

macro_rules! inner_integer {
    ($($ty:ident)*) => {
        paste::paste! {
            $(
                #[inline]
                fn [<serialize_ $ty>](mut self, v: $ty) -> Result<Self::Ok, Self::Error> {
                    Ok(self.output.write_integer(v)?)
                }
            )*
        }
    }
}

macro_rules! defer_integer_n_bool_to_write {
    () => {
        inner_integer! {
            i8 i16 i32 i64 i128 u8 u16 u32 u64 u128
        }

        #[inline]
        fn serialize_bool(mut self, v: bool) -> Result<Self::Ok, Self::Error> {
            self.output.write_bool(v)?;
            Ok(())
        }
    };
}

macro_rules! inner_float {
    ($($ty:ident)*) => {
        paste::paste! {
            $(
                #[inline]
                fn [<serialize_ $ty>](mut self, v: $ty) -> Result<Self::Ok, Self::Error> {
                    // Non-finite floats are serialized as `null`.
                    if !v.is_finite() {
                        Ok(self.output.write_null()?)
                    } else {
                        Ok(self.output.write_float(v)?)
                    }
                }
            )*
        }
    }
}
macro_rules! defer_float_to_write_or_not_finite {
    () => {
        inner_float! {
            f32 f64
        }
    };
}

impl<W: WWrite> serde::Serializer for JsonSerializer<W> {
    type Ok = ();

    type Error = Error;

    type SerializeSeq = SeqSerializer<W>;
    type SerializeTuple = TupleSerializer<W>;
    type SerializeTupleStruct = TupleStructSerializer<W>;
    type SerializeTupleVariant = TupleVariantSerializer<W>;
    type SerializeMap = MapSerializer<W>;
    type SerializeStruct = StructSerializer<W>;
    type SerializeStructVariant = StructVariantSerializer<W>;

    defer_integer_n_bool_to_write! {}
    defer_float_to_write_or_not_finite! {}

    #[inline]
    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        // A char encoded as UTF-8 takes 4 bytes at most.
        let mut buf = [0; 4];
        self.serialize_str(v.encode_utf8(&mut buf))
    }

    #[inline]
    fn serialize_str(mut self, v: &str) -> Result<Self::Ok, Self::Error> {
        if self.is_top_level_value {
            // Do not quote or escape top-level strings from the form
            // serializer.
            Ok(self.output.write_str(v)?)
        } else {
            self.output.write_quote()?;
            self.output.escape().write_str(v)?;
            Ok(self.output.write_quote()?)
        }
    }

    #[inline]
    fn serialize_bytes(mut self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Ok(self.output.write_byte_array(v)?)
    }

    #[inline]
    fn collect_str<T>(mut self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + std::fmt::Display,
    {
        if self.is_top_level_value {
            // Do not quote or escape top-level strings from the form
            // serializer.
            Ok(write!(self.output, "{value}")?)
        } else {
            self.output.write_quote()?;
            write!(self.output.escape(), "{value}")?;
            Ok(self.output.write_quote()?)
        }
    }

    #[inline]
    fn serialize_unit(mut self) -> Result<Self::Ok, Self::Error> {
        Ok(self.output.write_null()?)
    }

    #[inline]
    fn serialize_unit_struct(mut self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(self.output.write_null()?)
    }

    #[inline]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.serialize_str(variant)
    }

    #[inline]
    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    #[inline]
    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        use ser::SerializeMap as _;

        let mut map = self.serialize_map(Some(1))?;
        map.serialize_entry(variant, value)?;
        map.end()
    }

    #[inline]
    fn serialize_none(mut self) -> Result<Self::Ok, Self::Error> {
        Ok(self.output.write_null()?)
    }

    #[inline]
    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    #[inline]
    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        SeqSerializer::new(self.output, len)
    }

    #[inline]
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        self.serialize_seq(Some(len))
    }

    #[inline]
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        self.serialize_seq(Some(len))
    }

    #[inline]
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        TupleVariantSerializer::new(self.output, variant, len)
    }

    #[inline]
    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        MapSerializer::new(self.output, len)
    }

    #[inline]
    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        StructSerializer::new(self.output, Some(len))
    }

    #[inline]
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        StructVariantSerializer::new(self.output, variant, len)
    }
}

pub struct KeySerializerNoQuotes<W: WWrite> {
    pub(crate) output: W,
}

macro_rules! inner_float {
    ($($ty:ident)*) => {
        paste::paste! {
            $(
                #[inline]
                fn [<serialize_ $ty>](mut self, v: $ty) -> Result<Self::Ok, Self::Error> {
                    // JSON/Form keys cannot be non-finite.
                    if !v.is_finite() {
                        Err(float_key_must_be_finite())
                    } else {
                        Ok(self.output.write_float(v)?)
                    }
                }
            )*
        }
    }
}

macro_rules! defer_float_to_write_or_err_not_finite {
    () => {
        inner_float! {
            f32 f64
        }
    };
}

impl<W: WWrite> ser::Serializer for KeySerializerNoQuotes<W> {
    type Ok = ();
    type Error = crate::error::Error;

    #[inline]
    fn serialize_str(mut self, v: &str) -> Result<Self::Ok, Self::Error> {
        Ok(self.output.write_str(v)?)
    }

    #[inline]
    fn serialize_char(mut self, v: char) -> Result<Self::Ok, Self::Error> {
        Ok(self.output.write_char(v)?)
    }

    #[inline]
    fn collect_str<T>(mut self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + std::fmt::Display,
    {
        Ok(write!(self.output, "{value}")?)
    }

    #[inline]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.serialize_str(variant)
    }

    #[inline]
    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    #[inline]
    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    defer_integer_n_bool_to_write! {}
    defer_float_to_write_or_err_not_finite! {}
    // Complex types are not allowed for keys.
    error_unsupported! {
        key_must_be_string, [empty bytes array object]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write::PercentEncoding;
    use serde::Serialize;
    use std::collections::{BTreeMap, HashMap};

    fn to_string<T: Serialize>(value: &T) -> Result<String, Error> {
        let mut buf = String::with_capacity(128);
        let writer = PercentEncoding::new(&mut buf);
        let serializer = JsonSerializer {
            output: writer,
            is_top_level_value: false,
        };
        value.serialize(serializer)?;
        Ok(buf)
    }

    fn to_string_top_level<T: Serialize>(value: &T) -> Result<String, Error> {
        let mut buf = String::new();
        let writer = PercentEncoding::new(&mut buf);
        let serializer = JsonSerializer {
            output: writer,
            is_top_level_value: true,
        };
        value.serialize(serializer)?;
        Ok(buf)
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_primitives() {
        assert_eq!(to_string(&true).unwrap(), "true");
        assert_eq!(to_string(&-42).unwrap(), "-42");
        assert_eq!(to_string(&3.14).unwrap(), "3.14");
        assert_eq!(to_string(&()).unwrap(), "null");
        assert_eq!(to_string(&Option::<i32>::None).unwrap(), "null");
        assert_eq!(to_string(&Some(100)).unwrap(), "100");
    }

    #[test]
    fn test_floats_non_finite() {
        assert_eq!(to_string(&f64::NAN).unwrap(), "null");
        assert_eq!(to_string(&f64::INFINITY).unwrap(), "null");
        assert_eq!(to_string(&f64::NEG_INFINITY).unwrap(), "null");
    }

    #[test]
    fn test_string() {
        assert_eq!(to_string(&"hello").unwrap(), "%22hello%22");
        // Test escaping and encoding
        // "a \"b\"" -> json escaped -> "a \\\"b\\\"" -> percent encoded
        assert_eq!(to_string(&"a \"b\"").unwrap(), "%22a%20%5C%22b%5C%22%22");
    }

    #[test]
    fn test_top_level_string() {
        // As a top-level value, it should not be quoted or json-escaped, only percent-encoded.
        assert_eq!(
            to_string_top_level(&"a \"key\"=value").unwrap(),
            "a%20%22key%22%3Dvalue"
        );
    }

    #[test]
    fn test_seq() {
        let vec = vec![1, 2, 3];
        assert_eq!(to_string(&vec).unwrap(), "%5B1%2C2%2C3%5D");

        let tuple = (true, "hello", 42);
        assert_eq!(to_string(&tuple).unwrap(), "%5Btrue%2C%22hello%22%2C42%5D");

        let empty: Vec<i32> = vec![];
        assert_eq!(to_string(&empty).unwrap(), "%5B%5D");
    }

    #[test]
    fn test_map() {
        let mut map = BTreeMap::new();
        map.insert("a key", 1);
        map.insert("b key", 2);
        assert_eq!(
            to_string(&map).unwrap(),
            "%7B%22a%20key%22%3A1%2C%22b%20key%22%3A2%7D"
        );

        let empty: BTreeMap<String, i32> = BTreeMap::new();
        assert_eq!(to_string(&empty).unwrap(), "%7B%7D");
    }

    #[derive(Serialize)]
    struct MyStruct {
        x: i32,
        y: String,
        z: bool,
    }

    #[test]
    fn test_struct() {
        let s = MyStruct {
            x: 1,
            y: "hi".to_string(),
            z: false,
        };
        assert_eq!(
            to_string(&s).unwrap(),
            "%7B%22x%22%3A1%2C%22y%22%3A%22hi%22%2C%22z%22%3Afalse%7D"
        );
    }

    #[derive(Serialize)]
    enum MyEnum {
        Unit,
        Newtype(u32),
        Tuple(u32, u32),
        Struct { a: u32, b: u32 },
    }

    #[test]
    fn test_enum() {
        let e1 = MyEnum::Unit;
        assert_eq!(to_string(&e1).unwrap(), "%22Unit%22");

        let e2 = MyEnum::Newtype(123);
        assert_eq!(to_string(&e2).unwrap(), "%7B%22Newtype%22%3A123%7D");

        let e3 = MyEnum::Tuple(1, 2);
        assert_eq!(to_string(&e3).unwrap(), "%7B%22Tuple%22%3A%5B1%2C2%5D%7D");

        let e4 = MyEnum::Struct { a: 1, b: 2 };
        assert_eq!(
            to_string(&e4).unwrap(),
            "%7B%22Struct%22%3A%7B%22a%22%3A1%2C%22b%22%3A2%7D%7D"
        );
    }

    #[test]
    fn test_invalid_key() {
        let map = HashMap::from([([1], 3)]);
        to_string(&map).unwrap_err();
    }
}
