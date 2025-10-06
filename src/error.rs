use std::fmt;

#[derive(Debug)]
pub struct Error {
    pub(crate) inner: ErrorInner,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner {
            ErrorInner::Message(msg) => write!(f, "{}", msg),
            ErrorInner::NotAnObject(t) => {
                write!(f, "Top-level value must be a struct or map, but got {t}")
            }
            ErrorInner::KeyMustBeAString(t) => write!(f, "Map key must be a string, but got {t}"),
            ErrorInner::FloatKeyMustBeFinite => write!(f, "Map key must be finite"),
            ErrorInner::Fmt => write!(f, "Error writing to the underlying write"),
        }
    }
}

impl std::error::Error for Error {}

impl serde::ser::Error for Error {
    fn custom<T>(msg: T) -> Self
    where
        T: fmt::Display,
    {
        Error {
            inner: ErrorInner::Message(msg.to_string().into()),
        }
    }
}

impl From<std::fmt::Error> for Error {
    fn from(_: std::fmt::Error) -> Self {
        Self {
            inner: ErrorInner::Fmt,
        }
    }
}

pub(crate) const fn top_level_must_be_object(got: &'static str) -> Error {
    Error {
        inner: ErrorInner::NotAnObject(got),
    }
}

pub(crate) const fn key_must_be_string(got: &'static str) -> Error {
    Error {
        inner: ErrorInner::KeyMustBeAString(got),
    }
}

pub(crate) const fn float_key_must_be_finite() -> Error {
    Error {
        inner: ErrorInner::FloatKeyMustBeFinite,
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum ErrorInner {
    /// A custom error message.
    Message(Box<str>),
    /// The top-level value was not a type that'll end-up being an object
    /// if serialized by serde_json.
    NotAnObject(&'static str),
    /// A map key was not a string.
    KeyMustBeAString(&'static str),
    /// Object key is a non-finite float value.
    FloatKeyMustBeFinite,
    /// An I/O error occurred in the writer.
    Fmt,
}

#[macro_export]
#[doc(hidden)]
macro_rules! cfg_error {
    (error {$($if_err:tt)*} {$($if_not:tt)*}) => {
        $($if_err)*
    };
    (ok {$($if_err:tt)*} {$($if_not:tt)*}) => {
        $($if_not)*
    }
}

#[macro_export]
#[doc(hidden)]
macro_rules! serialize_normal {
    (@$is_err:ident $f:expr, [$($base_name:ident)*]) => {
        paste::paste! {
            $(
                #[inline]
                #[allow(unused_variables)]
                fn [<serialize_ $base_name>](self, v: $base_name) -> Result<Self::Ok, Self::Error> {
                    $crate::cfg_error! {
                        $is_err

                        {Err($f(stringify!($base_name)))}

                        {$f}
                    }
                }
            )*
        }
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! forward_unit {
    (@$is_err:ident $f:expr, bool) => {
        $crate::serialize_normal!(@$is_err $f, [bool]);
    };
    (@$is_err:ident $f:expr, integers) => {
        $crate::serialize_normal!(@$is_err $f, [i8 i16 i32 i64 u8 u16 u32 u64 f32 f64]);
    };
    (@$is_err:ident $f:expr, char) => {
        $crate::serialize_normal!(@$is_err $f, [char]);
    };
    (@$is_err:ident $f:expr, str) => {
        #[inline]
        #[allow(unused_variables)]
        fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
            $crate::cfg_error! {
                $is_err

                {Err($f("str"))}

                {$f}
            }
        }

        #[inline]
        #[allow(unused_variables)]
        fn serialize_unit_variant(
            self,
            name: &'static str,
            variant_index: u32,
            variant: &'static str,
        ) -> Result<Self::Ok, Self::Error> {
            $crate::cfg_error! {
                $is_err

                {Err($f("UnitVariant"))}

                {$f}
            }
       }
    };
    (@$is_err:ident $f:expr, bytes) => {
        #[inline]
        #[allow(unused_variables)]
        fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
            $crate::cfg_error! {
                $is_err

                {Err($f("bytes"))}

                {$f}
            }
        }
    };
    (@$is_err:ident $f:expr, empty) => {
        #[inline]
        fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
            $crate::cfg_error! {
                $is_err

                {Err($f("Option::<T>::None"))}

                {$f}
            }
        }

        #[inline]
        fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
            $crate::cfg_error! {
                $is_err

                {Err($f("()"))}

                {$f}
            }
        }

        #[inline]
        #[allow(unused_variables)]
        fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok, Self::Error> {
            $crate::cfg_error! {
                $is_err

                {Err($f("UnitStruct"))}

                {$f}
            }
        }
    };
    (@$is_err:ident $f:expr, array) => {
        type SerializeSeq = serde::ser::Impossible<(), Self::Error>;
        #[inline]
        fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
            Err($f("Seq"))
        }

        type SerializeTuple = serde::ser::Impossible<(), Self::Error>;
        #[inline]
        fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
            Err($f("Tuple"))
        }

        type SerializeTupleStruct = serde::ser::Impossible<(), Self::Error>;
        #[inline]
        fn serialize_tuple_struct(
                self,
                _name: &'static str,
                _len: usize,
            ) -> Result<Self::SerializeTupleStruct, Self::Error> {
            Err($f("TupleStruct"))
        }
    };
    (@error $f:expr, object) => {
        #[inline]
        fn serialize_newtype_variant<T>(
            self,
            _name: &'static str,
            _variant_index: u32,
            _variant: &'static str,
            _value: &T,
        ) -> Result<Self::Ok, Self::Error>
        where
            T: ?Sized + Serialize
        {
            Err($f("NewtypeVariant"))
        }

        type SerializeTupleVariant = serde::ser::Impossible<(), Self::Error>;
        #[inline]
        fn serialize_tuple_variant(
            self,
            _name: &'static str,
            _variant_index: u32,
            _variant: &'static str,
            _len: usize,
        ) -> Result<Self::SerializeTupleVariant, Self::Error> {
            Err($f("TupleVariant"))
        }

        type SerializeMap = serde::ser::Impossible<(), Self::Error>;
        #[inline]
        fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
            Err($f("Map"))
        }

        type SerializeStruct = serde::ser::Impossible<(), Self::Error>;
        #[inline]
        fn serialize_struct(
            self,
            _name: &'static str,
            _len: usize,
        ) -> Result<Self::SerializeStruct, Self::Error> {
            Err($f("struct"))
        }

        type SerializeStructVariant = serde::ser::Impossible<(), Self::Error>;
        #[inline]
        fn serialize_struct_variant(
            self,
            _name: &'static str,
            _variant_index: u32,
            _variant: &'static str,
            _len: usize,
        ) -> Result<Self::SerializeStructVariant, Self::Error> {
            Err($f("StructVariant"))
        }

    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! error_unsupported {
    ($f:expr, [$($base_name:ident)*]) => {
        $(
            $crate::forward_unit!(@error $f, $base_name);
        )*
    }
}

#[macro_export]
#[doc(hidden)]
macro_rules! defer_integer_n_bool_to_write {
    () => {
        macro_rules! inner {
            ($($ty:ident)*) => {
                paste::paste! {
                    $(
                        #[inline]
                        fn [<serialize_ $ty>](mut self, v: $ty) -> Result<Self::Ok, Self::Error> {
                            Ok(self.output.[<write_ $ty>](v)?)
                        }
                    )*
                }
            }
        }

        inner! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128
        }
    }
}

#[macro_export]
#[doc(hidden)]
macro_rules! defer_float_to_write_or_not_finite {
    ($or:expr) => {
        paste::paste! {
            $(
                #[inline]
                fn [<serialize_ $ty>](mut self, v: $ty) -> Result<Self::Ok, Self::Error> {
                    if !v.is_finite() {
                        return $or
                    }
                    Ok(self.output.[<write_ $ty>](v)?)
                }
            )*
        }
    }
}
