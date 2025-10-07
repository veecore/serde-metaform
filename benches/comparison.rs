use criterion::{Criterion, black_box, criterion_group, criterion_main};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Serialize;
use serde::de::{Deserializer, Error as DeError, MapAccess, Visitor};
use serde_json::value::RawValue;
use std::borrow::Cow;

// --- Alternative implementation for comparison ---

/// A custom `AsciiSet` for form-urlencoded values.
const FORM_URLENCODING_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Converts a top-level JSON object from a byte slice into a form-encoded string.
/// This implementation uses a streaming deserializer to avoid allocating an intermediate HashMap,
/// resulting in excellent performance and low memory usage.
pub fn encode_request_body_from_bytes(body: &[u8]) -> Result<String, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    deserializer.deserialize_map(FormVisitor)
}

// A stateless visitor for building the form-encoded string from a JSON stream.
struct FormVisitor;

impl<'de> Visitor<'de> for FormVisitor {
    type Value = String;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut result = String::with_capacity(map.size_hint().unwrap_or(0) * 20);
        while let Some((key, value)) = map.next_entry::<Cow<str>, &RawValue>()? {
            if !result.is_empty() {
                result.push('&');
            }
            let s = value.get();
            let val: Cow<'_, str> = if s.starts_with('"') {
                // unescape anything escaped
                serde_json::from_str(s).map_err(DeError::custom)?
            } else {
                Cow::Borrowed(s)
            };
            result.push_str(&key);
            result.push('=');
            result.extend(utf8_percent_encode(&val, FORM_URLENCODING_ENCODE_SET));
        }
        Ok(result)
    }
}

pub fn encode_request_body_from_value(
    value: &serde_json::Value,
) -> Result<String, serde_json::Error> {
    let map = if let Some(map) = value.as_object() {
        map
    } else {
        return Err(serde_json::Error::custom("value must be an object"));
    };

    let mut result = String::with_capacity(map.len() * 20);
    for (key, value) in map {
        if !result.is_empty() {
            result.push('&');
        }

        let val = if let Some(s) = value.as_str() {
            // no unescaping needed
            s
        } else {
            // Now we need to json-fy whatever we have here
            &serde_json::to_string(value)?
        };

        result.push_str(key);
        result.push('=');
        result.extend(utf8_percent_encode(val, FORM_URLENCODING_ENCODE_SET));
    }

    Ok(result)
}

// --- Struct definitions for the typed benchmark ---

#[derive(Serialize)]
struct Message<'a> {
    messaging_product: &'a str,
    recipient_type: &'a str,
    to: &'a str,
    #[serde(rename = "type")]
    message_type: &'a str,
    interactive: Interactive<'a>,
}

#[derive(Serialize)]
struct Interactive<'a> {
    #[serde(rename = "type")]
    interactive_type: &'a str,
    header: Header<'a>,
    body: Body<'a>,
    footer: Footer<'a>,
    action: Action<'a>,
}

#[derive(Serialize)]
struct Header<'a> {
    #[serde(rename = "type")]
    header_type: &'a str,
    text: &'a str,
}

#[derive(Serialize)]
struct Body<'a> {
    text: &'a str,
}

#[derive(Serialize)]
struct Footer<'a> {
    text: &'a str,
}

#[derive(Serialize)]
struct Action<'a> {
    button: &'a str,
    sections: Vec<Section<'a>>,
}

#[derive(Serialize)]
struct Section<'a> {
    title: &'a str,
    rows: Vec<Row<'a>>,
}

#[derive(Serialize)]
struct Row<'a> {
    id: &'a str,
    title: &'a str,
    description: &'a str,
}

// --- Benchmark function ---

pub fn bench_encoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("Encoding Comparison");

    // --- Benchmark Data ---

    // Typed struct, for the direct serialization benchmark
    let message = Message {
        messaging_product: "whatsapp",
        recipient_type: "individual",
        to: "phone_number",
        message_type: "interactive",
        interactive: Interactive {
            interactive_type: "list",
            header: Header {
                header_type: "text",
                text: "<HEADER_TEXT>",
            },
            body: Body {
                text: "<BODY_TEXT>",
            },
            footer: Footer {
                text: "<FOOTER_TEXT>",
            },
            action: Action {
                button: "<BUTTON_TEXT>",
                sections: vec![
                    Section {
                        title: "<LIST_SECTION_1_TITLE>",
                        rows: vec![
                            Row {
                                id: "<LIST_SECTION_1_ROW_1_ID>",
                                title: "<SECTION_1_ROW_1_TITLE>",
                                description: "<SECTION_1_ROW_1_DESC>",
                            },
                            Row {
                                id: "<LIST_SECTION_1_ROW_2_ID>",
                                title: "<SECTION_1_ROW_2_TITLE>",
                                description: "<SECTION_1_ROW_2_DESC>",
                            },
                        ],
                    },
                    Section {
                        title: "<LIST_SECTION_2_TITLE>",
                        rows: vec![
                            Row {
                                id: "<LIST_SECTION_2_ROW_1_ID>",
                                title: "<SECTION_2_ROW_1_TITLE>",
                                description: "<SECTION_2_ROW_1_DESC>",
                            },
                            Row {
                                id: "<LIST_SECTION_2_ROW_2_ID>",
                                title: "<SECTION_2_ROW_2_TITLE>",
                                description: "<SECTION_2_ROW_2_DESC>",
                            },
                        ],
                    },
                ],
            },
        },
    };

    // --- Benchmarks ---

    // Benchmark 1: From struct to transcoding from a raw JSON byte slice.
    // This simulates having JSON data that needs to be converted.
    // It's efficient for what it does but includes JSON serialization &
    // parsing overhead.
    group.bench_function("from_bytes (struct -> json_bytes -> form)", |b| {
        b.iter(|| {
            let json_bytes = serde_json::to_vec(black_box(&message)).unwrap();
            encode_request_body_from_bytes(black_box(&json_bytes)).unwrap();
        });
    });

    // Benchmark 2: Like 1 but from struct to serde_json::Value instead of
    // raw JSON byte slice.
    group.bench_function("from_value (struct -> json_value -> form)", |b| {
        b.iter(|| {
            let json_value = serde_json::to_value(&message).unwrap();
            encode_request_body_from_value(black_box(&json_value)).unwrap();
        });
    });

    // Benchmark 3: Direct serialization from a typed struct.
    // This is the primary, intended use case for serde_metaform.
    // It should be much faster as it avoids any form of parsing.
    group.bench_function("from_struct (serde_metaform)", |b| {
        b.iter(|| {
            serde_metaform::to_string(black_box(&message)).unwrap();
        });
    });

    group.finish();
}

criterion_group!(benches, bench_encoding);
criterion_main!(benches);
