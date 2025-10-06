//! A `serde` serializer for the "Form + JSON" encoding format.
//!
//! This crate provides a robust `serde` serializer for a specific hybrid data format
//! used by some web APIs, most notably Meta's. This is **not** a standard
//! `application/x-www-form-urlencoded` implementation; it follows a unique set of rules
//! where values are encoded as JSON.
//!
//! The serializer is designed to be efficient and stream-based, performing the JSON
//! serialization and percent-encoding in a single pass without creating intermediate
//! string representations of the entire data structure.
//!
//! ## The "Form + JSON" Format
//!
//! The format can be conceptually understood as a standard form-encoded payload
//! where each value has been replaced by its JSON representation.
//!
//! ### Format Rules
//!
//! 1.  **Top-Level Structure**: The root data structure must be a key-value pair collection,
//!     such as a Rust `struct`, a `map`, or an `enum` variant that holds data (newtype,
//!     struct, or tuple variants). Primitives (like `u32`) or simple sequences (like `Vec<u32>`)
//!     are not valid at the top level.
//!
//! 2.  **Key Serialization**: Each key is converted to a string and then **percent-encoded**.
//!     The keys are never quoted.
//!
//! 3.  **Value Serialization**: Each value is **serialized to a JSON string**, and that
//!     resulting JSON string is then **percent-encoded**.
//!     -   Simple types like numbers, booleans, and strings become their direct JSON
//!         counterparts (e.g., `123`, `true`, `"hello world"`).
//!     -   Complex types like structs, maps, and vectors become JSON objects (`{...}`)
//!         or arrays (`[...]`).
//!
//! 4.  **Separators**: Key-value pairs are joined by `&`, and each key is separated
//!     from its value by `=`.
//!
//! ---
//!
//! ### A Note on Compatibility
//!
//! ⚠️ **Warning:** This format is highly specific. Do not use this serializer to send data
//! to a standard web server expecting `application/x-www-form-urlencoded`. A server
//! that isn't explicitly designed to parse JSON-encoded values from a form payload
//! will not be able to understand the output of this crate.
//!
//! ## Examples
//!
//! ### Simple Struct
//!
//! For simple values, the output looks very similar to standard form encoding.
//!
//! ```rust
//! use serde::Serialize;
//! use serde_metaform::to_string;
//!
//! #[derive(Serialize)]
//! struct User {
//!     id: u64,
//!     username: String,
//!     is_verified: bool,
//! }
//!
//! let user = User {
//!     id: 9001,
//!     username: "gordon_freeman".to_string(),
//!     is_verified: false,
//! };
//!
//! let encoded = to_string(&user).unwrap();
//!
//! // The values 9001, "gordon_freeman", and false are serialized as JSON primitives.
//! assert_eq!(encoded, "id=9001&username=gordon_freeman&is_verified=false");
//! ```
//!
//! ### Complex Nested Data and Enums
//!
//! The power of this format becomes clear with complex, nested data.
//!
//! ```rust
//! use serde::Serialize;
//! use serde_metaform::to_string;
//!
//! #[derive(Serialize)]
//! struct Attachment {
//!     type_: String,
//!     url: String,
//! }
//!
//! #[derive(Serialize)]
//! enum Message {
//!     Text(String),
//!     WithAttachment {
//!         text: String,
//!         attachment: Attachment,
//!     },
//! }
//!
//! let message = Message::WithAttachment {
//!     text: "Check this out!".to_string(),
//!     attachment: Attachment {
//!         type_: "image".to_string(),
//!         url: "https://example.com/img.png".to_string(),
//!     },
//! };
//!
//! let encoded = to_string(&message).unwrap();
//!
//! // The key is the enum variant name: "WithAttachment".
//! // The value is the entire associated struct, serialized to JSON:
//! // {"text":"Check this out!","attachment":{"type_":"image","url":"https://example.com/img.png"}}
//! // ...and then that JSON string is percent-encoded.
//!
//! let expected_value = "%7B%22text%22%3A%22Check%20this%20out%21%22%2C%22attachment%22%3A%7B%22type_%22%3A%22image%22%2C%22url%22%3A%22https%3A%2F%2Fexample.com%2Fimg.png%22%7D%7D";
//! let expected_string = format!("WithAttachment={}", expected_value);
//!
//! assert_eq!(encoded, expected_string);
//! ```

use std::fmt::{Display, Write};

use error::{Error, top_level_must_be_object};
use json::{JsonSerializer, KeySerializerNoQuotes};
use serde::Serialize;
use write::PercentEncoding;

pub mod error;
mod json;
mod write;

/// Serializes the given data structure into the provided writer.
///
/// This is the most flexible serialization function, allowing for direct streaming
/// to any type that implements `std::fmt::Write`, such as a `String`, a file,
/// or a network socket buffer.
///
/// For a detailed explanation of the format, see the [module-level documentation](self).
///
/// # Errors
///
/// This function will return an error if the serialization fails, which can happen
/// if the data structure is not a valid top-level type (e.g., a primitive), if a map
/// key is not a string, or if the writer returns an error.
#[inline]
pub fn to_writer<W, T>(writer: W, value: &T) -> Result<(), Error>
where
    W: Write,
    T: ?Sized + Serialize,
{
    let ser = Serializer::new(writer);
    value.serialize(ser)
}

/// Serializes the given data structure as a `String`.
///
/// This is a convenience function that wraps [`to_writer`] and allocates a new
/// `String` to hold the serialized output.
///
/// # Errors
///
/// Returns an error if serialization fails. See [`to_writer`] for details.
#[inline]
pub fn to_string<T>(value: &T) -> Result<String, Error>
where
    T: ?Sized + Serialize,
{
    let mut writer = String::with_capacity(128);
    to_writer(&mut writer, value)?;
    Ok(writer)
}

/// Serializes the given data structure as a vector of bytes (`Vec<u8>`).
///
/// This is a convenience function that serializes to a `String` via [`to_string`]
/// and then converts it to a byte vector.
///
/// # Errors
///
/// Returns an error if serialization fails. See [`to_writer`] for details.
#[inline]
pub fn to_vec<T>(value: &T) -> Result<Vec<u8>, Error>
where
    T: ?Sized + Serialize,
{
    let string = to_string(value)?;
    Ok(string.into())
}

/// Creates a displayable wrapper for a serializable type.
///
/// This helper is useful for integrating serialization directly with formatting
/// macros like `println!` or `format!`.
///
/// # Example
///
/// ```rust
/// # use serde::Serialize;
/// #[derive(Serialize)]
/// struct Point { x: i32, y: i32 }
///
/// let p = Point { x: 1, y: 2 };
/// println!("{}", serde_metaform::display(&p));
/// // Output: x=1&y=2
/// ```
#[inline]
pub fn display<T>(value: &T) -> DisplaySerializer<'_, T>
where
    T: ?Sized + Serialize,
{
    DisplaySerializer { value }
}

/// A wrapper struct that implements `std::fmt::Display` for any `serde::Serialize` type.
///
/// This struct is created by the [`display`] function.
#[derive(Debug)]
pub struct DisplaySerializer<'a, V: ?Sized> {
    value: &'a V,
}

impl<V> Display for DisplaySerializer<'_, V>
where
    V: ?Sized + Serialize,
{
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ser = Serializer::new(f);
        self.value.serialize(ser).map_err(|_| std::fmt::Error)
    }
}

/// The main serializer for the "Form + JSON" format.
///
/// This serializer manages the top-level state, ensuring that the output
/// is a series of `key=value` pairs separated by `&`. It is the entry point
/// for serializing structs, maps, and enum variants.
pub struct Serializer<W> {
    output: W,
    is_first: bool,
}

impl<W: Write> Serializer<W> {
    /// Creates a new serializer that writes to the given `writer`.
    #[inline]
    pub fn new(writer: W) -> Self {
        Self {
            output: writer,
            is_first: true,
        }
    }

    /// Unwraps the serializer, returning the underlying writer.
    pub fn into_inner(self) -> W {
        self.output
    }
}

impl<W: Write> serde::Serializer for Serializer<W> {
    type Ok = ();
    type Error = Error;

    type SerializeMap = MapSerializer<W>;
    type SerializeStruct = StructSerializer<W>;
    type SerializeTupleVariant = TupleVariantSerializer<W>;
    type SerializeStructVariant = StructVariantSerializer<W>;

    // ---- T ----

    #[inline]
    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
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

    // ---- EMPTY ----

    /// Serializes a unit `()` or `Option::None` as an empty string.
    #[inline]
    fn serialize_unit(mut self) -> Result<Self::Ok, Self::Error> {
        // Treat unit/None as empty string value
        Ok(self.output.write_str("")?)
    }

    /// Serializes a unit struct as an empty string.
    #[inline]
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        self.serialize_unit()
    }

    /// Serializes `Option::None` as an empty string.
    #[inline]
    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.serialize_unit()
    }

    // ---- OBJECT ----

    /// Serializes a newtype enum variant as `variant=value`.
    #[inline]
    fn serialize_newtype_variant<T>(
        mut self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        use serde::ser::SerializeMap as _;

        self.serialize_entry(variant, value)
    }

    /// Prepares to serialize a tuple enum variant as `variant=[...]`.
    #[inline]
    fn serialize_tuple_variant(
        mut self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        use serde::ser::SerializeMap as _;
        // Write the key: `variant=`
        self.serialize_key(variant)?;
        // Prepare to write the value as a JSON array: `[...]`
        let seq = json::SeqSerializer::new(PercentEncoding::new(self.output), Some(len))?;
        Ok(TupleVariantSerializer { inner: seq })
    }

    /// Prepares to serialize a struct enum variant as `variant={...}`.
    #[inline]
    fn serialize_struct_variant(
        mut self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        use serde::ser::SerializeMap as _;
        // Write the key: `variant=`
        self.serialize_key(variant)?;
        // Prepare to write the value as a JSON object: `{...}`
        let object = json::StructSerializer::new(PercentEncoding::new(self.output), Some(len))?;
        Ok(StructVariantSerializer { inner: object })
    }

    #[inline]
    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(self)
    }

    #[inline]
    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(self)
    }

    error_unsupported! {
        top_level_must_be_object, [bool integers char str bytes array]
    }
}

pub type MapSerializer<W> = Serializer<W>;

impl<W: Write> serde::ser::SerializeMap for MapSerializer<W> {
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_key<T>(&mut self, key: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        if !self.is_first {
            self.output.write_str("&")?;
        }

        key.serialize(KeySerializerNoQuotes {
            output: PercentEncoding::new(&mut self.output),
        })?;

        self.output.write_str("=")?;
        Ok(())
    }

    #[inline]
    fn serialize_value<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(JsonSerializer {
            output: PercentEncoding::new(&mut self.output),
            is_top_level_value: true,
        })?;
        self.is_first = false;
        Ok(())
    }

    #[inline]
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

pub type StructSerializer<W> = Serializer<W>;

impl<W: Write> serde::ser::SerializeStruct for StructSerializer<W> {
    type Ok = ();

    type Error = Error;

    #[inline]
    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        use serde::ser::SerializeMap as _;
        self.serialize_entry(key, value)
    }

    #[inline]
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

pub struct TupleVariantSerializer<W: Write> {
    inner: json::SeqSerializer<PercentEncoding<W>>,
}

impl<W: Write> serde::ser::SerializeTupleVariant for TupleVariantSerializer<W> {
    type Ok = ();

    type Error = Error;

    #[inline]
    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        use serde::ser::SerializeSeq as _;
        // variant=[
        // must've been written before now.
        self.inner.serialize_element(value)
    }

    #[inline]
    fn end(self) -> Result<Self::Ok, Self::Error> {
        use serde::ser::SerializeSeq as _;
        // ]
        self.inner.end()
    }
}

pub struct StructVariantSerializer<W: Write> {
    inner: json::StructSerializer<PercentEncoding<W>>,
}

impl<W: Write> serde::ser::SerializeStructVariant for StructVariantSerializer<W> {
    type Ok = ();

    type Error = Error;

    #[inline]
    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        use serde::ser::SerializeStruct as _;
        // variant={
        // must've been written before now.
        self.inner.serialize_field(key, value)
    }

    #[inline]
    fn end(self) -> Result<Self::Ok, Self::Error> {
        // }
        use serde::ser::SerializeStruct as _;

        self.inner.end()
    }
}

//==============================================================================
// Tests
//==============================================================================

#[cfg(test)]
mod tests {
    use crate::error::ErrorInner;

    use super::*;
    use serde::Serialize;
    use std::collections::BTreeMap;

    #[derive(Debug, Serialize)]
    struct SimplePayload {
        id: u64,
        name: String,
    }

    #[derive(Debug, Serialize)]
    struct ComplexPayloadFieldValue {
        recipient: String,
        amount: u32,
    }

    #[derive(Debug, Serialize)]
    struct ComplexPayload {
        field: ComplexPayloadFieldValue,
        id: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        key: Option<String>,
        is_active: bool,
    }

    #[test]
    fn test_simple_struct() {
        let payload = SimplePayload {
            id: 123,
            name: "John Doe".to_string(),
        };
        let result = to_string(&payload).unwrap();
        assert_eq!(result, "id=123&name=John%20Doe");
    }

    #[test]
    fn test_special_chars() {
        let mut map = BTreeMap::new();
        map.insert("key w/ spaces & symbols", "value w/ spaces & symbols");
        map.insert("another key", "a=b&c=d");
        let result = to_string(&map).unwrap();
        assert_eq!(
            result,
            "another%20key=a%3Db%26c%3Dd&key%20w%2F%20spaces%20%26%20symbols=value%20w%2F%20spaces%20%26%20symbols"
        );
    }

    #[test]
    fn test_empty_struct() {
        #[derive(Serialize)]
        struct Empty;
        let result = to_string(&Empty).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_complex_field_json_encoding() {
        let payload = ComplexPayload {
            field: ComplexPayloadFieldValue {
                recipient: "Victor + Sons".to_string(),
                amount: 100,
            },
            id: 12345,
            key: Some("api_key_123".to_string()),
            is_active: true,
        };
        let result = to_string(&payload).unwrap();

        // Expected JSON for 'field': {"recipient":"Victor + Sons","amount":100}
        let expected_field_value =
            "%7B%22recipient%22%3A%22Victor%20%2B%20Sons%22%2C%22amount%22%3A100%7D";
        let expected = format!(
            "field={}&id=12345&key=api_key_123&is_active=true",
            expected_field_value
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn test_top_level_enum_variants() {
        #[derive(Serialize)]
        enum Status {
            Pending,
            Complete(u32),
            Error { code: u32, message: String },
            List(Vec<String>),
        }

        // Unit variant: ``
        let pending = Status::Pending;
        assert_eq!(
            to_string(&pending).unwrap_err().inner,
            ErrorInner::NotAnObject("UnitVariant")
        );

        // Newtype variant: `Variant=<json_value>`
        // Expected JSON: {"Complete":404}
        let complete = Status::Complete(404);
        assert_eq!(to_string(&complete).unwrap(), "Complete=404");

        // Struct variant: `Variant=<json_object>`
        // Expected JSON: {"Error":{"code":500,"message":"Server Issue"}}
        let error = Status::Error {
            code: 500,
            message: "Server Issue".to_string(),
        };
        let expected_error_val = "%7B%22code%22%3A500%2C%22message%22%3A%22Server%20Issue%22%7D";
        assert_eq!(
            to_string(&error).unwrap(),
            format!("Error={}", expected_error_val)
        );

        // Tuple variant: `Variant=<json_array>`
        // Expected JSON: {"List":["item 1","item/2"]}
        let list = Status::List(vec!["item 1".to_string(), "item/2".to_string()]);
        let expected_list_val = "%5B%22item%201%22%2C%22item%2F2%22%5D";
        assert_eq!(
            to_string(&list).unwrap(),
            format!("List={}", expected_list_val)
        );
    }

    #[test]
    fn test_skip_none_field() {
        let payload = ComplexPayload {
            field: ComplexPayloadFieldValue {
                recipient: "test".to_string(),
                amount: 50,
            },
            id: 99,
            key: None,
            is_active: false,
        };
        let result = to_string(&payload).unwrap();
        let expected_field_value = "%7B%22recipient%22%3A%22test%22%2C%22amount%22%3A50%7D";
        let expected = format!("field={}&id=99&is_active=false", expected_field_value);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_top_level_errors() {
        // Top-level primitives must fail
        let err_int = to_string(&123).unwrap_err();
        assert_eq!(err_int.inner, ErrorInner::NotAnObject("i32"));

        // Top-level sequences must fail
        let err_seq = to_string(&vec![1, 2, 3]).unwrap_err();
        assert_eq!(err_seq.inner, ErrorInner::NotAnObject("Seq"));

        // Top-level enums (that aren't structs/maps) must fail
        #[derive(Serialize)]
        enum MyEnum {
            Unit,
        }
        let err_enum = to_string(&MyEnum::Unit).unwrap_err();
        assert_eq!(err_enum.inner, ErrorInner::NotAnObject("UnitVariant"));

        // Primitives must fail
        assert_eq!(
            to_string(&123).unwrap_err().inner,
            ErrorInner::NotAnObject("i32")
        );
        assert_eq!(
            to_string("a str").unwrap_err().inner,
            ErrorInner::NotAnObject("str")
        );

        // Sequences must fail
        assert_eq!(
            to_string(&vec![1, 2]).unwrap_err().inner,
            ErrorInner::NotAnObject("Seq")
        );
        assert_eq!(
            to_string(&(1, 4)).unwrap_err().inner,
            ErrorInner::NotAnObject("Tuple")
        );
    }
}
