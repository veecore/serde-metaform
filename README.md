# serde_metaform

[![Crates.io](https://img.shields.io/crates/v/serde-metaform.svg)](https://crates.io/crates/serde-metaform)
[![Docs.rs](https://docs.rs/serde-metaform/badge.svg)](https://docs.rs/serde-metaform)
[![CI](https://github.com/veecore/serde-metaform/actions/workflows/ci.yaml/badge.svg)](https://github.com/veecore/serde-metaform/actions/workflows/ci.yaml)

A high-performance `serde` serializer for the hybrid **"Form + JSON"** encoding format used by APIs like Meta’s (WhatsApp Business, Instagram Messaging, etc.).

> ⚠️ **Warning:** This is *not* a standard `application/x-www-form-urlencoded` serializer.
> It produces a specialized encoding where **values are JSON-encoded before being percent-encoded.**
> Do **not** use this for ordinary HTML form submissions.

---

## 🧩 What Is “Form + JSON” Encoding?

This format looks like a typical form payload (`key=value&key2=value2`), but each value is actually JSON.

1. Top-level data is represented as key–value pairs.
2. Keys are percent-encoded strings.
3. Values are serialized as JSON strings, then **the entire JSON string** is percent-encoded.

This hybrid structure allows complex nested JSON to be transmitted in APIs that only accept form-like bodies.

### Example

Standard form encoding:

```
user_id=123&tags=rust&tags=serde
```

**Form + JSON** encoding:

```
user_id=123&
profile=%7B%22username%22%3A%22jdoe%22%2C%22tags%22%3A%5B%22rust%22%2C%22serde%22%5D%7D
```

The `profile` value above is the percent-encoded JSON string:

```json
{"username":"jdoe","tags":["rust","serde"]}
```

---

## 🚀 Features

* **Single-Pass Serialization:** No intermediate `serde_json::Value`, no double parsing.
* **Zero-Copy:** Streams directly into the output writer with minimal allocation.
* **Full `serde` Integration:** Works out of the box with `#[derive(Serialize)]`.
* **Nested Data Support:** Handles structs, enums, sequences, and maps cleanly.
* **Battle-Tested:** Used internally in [`whatsapp-business-rs`](https://github.com/veecore/whatsapp-business-rs).

---

## 🧠 Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_metaform = "1"
```

Then simply serialize your data:

```rust
use serde::Serialize;
use serde_metaform::to_string;

#[derive(Serialize)]
struct Message<'a> {
    recipient: &'a str,
    content: Content<'a>,
}

#[derive(Serialize)]
struct Content<'a> {
    #[serde(rename = "type")]
    message_type: &'a str,
    text: &'a str,
    buttons: Vec<Button<'a>>,
}

#[derive(Serialize)]
struct Button<'a> {
    id: &'a str,
    title: &'a str,
}

fn main() -> Result<(), serde_metaform::error::Error> {
    let msg = Message {
        recipient: "1234567890",
        content: Content {
            message_type: "interactive",
            text: "Choose an option:",
            buttons: vec![
                Button { id: "opt1", title: "Option 1" },
                Button { id: "opt2", title: "Option 2" },
            ],
        },
    };

    let encoded = to_string(&msg)?;
    println!("{encoded}");

    // Output (simplified, actual output is percent-encoded):
    // recipient=1234567890&
    // content={"type":"interactive","text":"Choose an option:","buttons":[{"id":"opt1","title":"Option 1"},{"id":"opt2","title":"Option 2"}]}

    Ok(())
}
```

---

## ⚡ Performance

`serde_metaform` was originally extracted from a production WhatsApp integration layer,
where JSON bodies were pre-built before conversion to the hybrid format.
This crate eliminates that two-step process.

| Benchmark       | Description                         | Mean Time   | Relative         |
| --------------- | ----------------------------------- | ----------- | ---------------- |
| `json_pipeline` | struct → JSON → form (old pipeline) | **25.0 µs** | 1.00×            |
| `from_bytes`    | JSON → form (transcoder only)       | **18.0 µs** | 1.39× faster     |
| `from_struct`   | struct → form (`serde_metaform`)    | **15.1 µs** | **1.65× faster** |

> Real-world WhatsApp payloads typically gain **35–45 %** throughput improvement.

Even under realistic, non-pretty-printed conditions,
**serde_metaform reduces total serialization time by ~40%**
and cuts memory allocations roughly in half.
Performance gains scale further with larger or deeply nested payloads.
---

## 📜 License

Licensed under either of:

* [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
* [MIT License](http://opensource.org/licenses/MIT)

at your option.