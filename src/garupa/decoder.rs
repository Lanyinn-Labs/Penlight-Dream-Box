//! Small lenient protobuf wire decoder used for the profile response subset.

use std::collections::HashMap;

use serde_json::{Map, Value};

use super::schema::{ProtoType, Schema};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WireType {
    Varint,
    Fixed32,
    Fixed64,
    LengthDelimited,
}

#[derive(Debug, Clone)]
enum RawData {
    Varint(u64),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone)]
struct RawField {
    field: u32,
    wire_type: WireType,
    data: RawData,
}

#[derive(Debug)]
pub struct ProtoError(pub String);

impl std::fmt::Display for ProtoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn read_varint(buf: &[u8], offset: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    let mut cursor = offset;

    while cursor < buf.len() {
        let byte = buf[cursor];
        cursor += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some((value, cursor));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }

    None
}

fn parse_raw_fields(buf: &[u8]) -> Vec<RawField> {
    let mut fields = Vec::new();
    let mut offset = 0;

    while offset < buf.len() {
        let (key, next) = match read_varint(buf, offset) {
            Some(value) => value,
            None => break,
        };
        offset = next;
        let Ok(field) = u32::try_from(key >> 3) else {
            break;
        };
        if field == 0 {
            break;
        }

        match key & 0x07 {
            0 => {
                let (value, next) = match read_varint(buf, offset) {
                    Some(value) => value,
                    None => break,
                };
                offset = next;
                fields.push(RawField {
                    field,
                    wire_type: WireType::Varint,
                    data: RawData::Varint(value),
                });
            }
            1 => {
                let Some(end) = offset.checked_add(8) else {
                    break;
                };
                if end > buf.len() {
                    break;
                }
                fields.push(RawField {
                    field,
                    wire_type: WireType::Fixed64,
                    data: RawData::Bytes(buf[offset..end].to_vec()),
                });
                offset = end;
            }
            2 => {
                let (length, next) = match read_varint(buf, offset) {
                    Some(value) => value,
                    None => break,
                };
                offset = next;
                let Ok(length) = usize::try_from(length) else {
                    break;
                };
                let Some(end) = offset.checked_add(length) else {
                    break;
                };
                if end > buf.len() {
                    break;
                }
                fields.push(RawField {
                    field,
                    wire_type: WireType::LengthDelimited,
                    data: RawData::Bytes(buf[offset..end].to_vec()),
                });
                offset = end;
            }
            5 => {
                let Some(end) = offset.checked_add(4) else {
                    break;
                };
                if end > buf.len() {
                    break;
                }
                fields.push(RawField {
                    field,
                    wire_type: WireType::Fixed32,
                    data: RawData::Bytes(buf[offset..end].to_vec()),
                });
                offset = end;
            }
            _ => break,
        }
    }

    fields
}

pub fn decode(buf: &[u8], schema: &Schema) -> Result<Value, ProtoError> {
    let mut groups: HashMap<u32, Vec<RawField>> = HashMap::new();
    for field in parse_raw_fields(buf) {
        groups.entry(field.field).or_default().push(field);
    }

    let mut result = Map::new();
    for (tag, definition) in schema.fields {
        let Some(items) = groups.get(tag) else {
            continue;
        };

        let parse = |item: &RawField| -> Option<Value> {
            match (definition.ty, &item.wire_type, &item.data) {
                (ProtoType::Int | ProtoType::Long, WireType::Varint, RawData::Varint(value)) => {
                    Some(Value::from(*value as i64))
                }
                (ProtoType::String, WireType::LengthDelimited, RawData::Bytes(value)) => {
                    Some(Value::String(String::from_utf8_lossy(value).into_owned()))
                }
                (ProtoType::Message(sub), WireType::LengthDelimited, RawData::Bytes(value)) => {
                    decode(value, sub).ok()
                }
                _ => None,
            }
        };

        if definition.repeated {
            result.insert(
                definition.name.to_string(),
                Value::Array(items.iter().filter_map(parse).collect()),
            );
        } else if let Some(value) = items.iter().rev().find_map(parse) {
            result.insert(definition.name.to_string(), value);
        }
    }

    Ok(Value::Object(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::garupa::schema::{field, ProtoType, Schema};

    fn encode_varint(mut value: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            bytes.push(byte);
            if value == 0 {
                return bytes;
            }
        }
    }

    fn encode_varint_field(field_num: u32, value: u64) -> Vec<u8> {
        let mut out = encode_varint(u64::from(field_num) << 3);
        out.extend(encode_varint(value));
        out
    }

    fn encode_bytes_field(field_num: u32, data: &[u8]) -> Vec<u8> {
        let mut out = encode_varint((u64::from(field_num) << 3) | 2);
        out.extend(encode_varint(data.len() as u64));
        out.extend(data);
        out
    }

    #[test]
    fn decodes_10_byte_negative_and_bit63_varints_without_dropping_subsequent_fields() {
        static TEST_SCHEMA: Schema = Schema {
            fields: &[
                (1, field("negOne", ProtoType::Int, false)),
                (2, field("minLong", ProtoType::Long, false)),
                (3, field("after", ProtoType::String, false)),
            ],
        };

        let mut payload = encode_varint_field(1, (-1i64) as u64);
        payload.extend(encode_varint_field(2, i64::MIN as u64));
        payload.extend(encode_bytes_field(3, b"kept"));

        let decoded = decode(&payload, &TEST_SCHEMA).unwrap();
        assert_eq!(decoded["negOne"], -1);
        assert_eq!(decoded["minLong"], i64::MIN);
        assert_eq!(decoded["after"], "kept");
    }

    #[test]
    fn rejects_overlong_varint_more_than_10_bytes() {
        let overlong = [0x80u8; 11];
        assert!(read_varint(&overlong, 0).is_none());
    }

    #[test]
    fn corrupt_huge_length_delimited_field_does_not_overflow_or_panic() {
        static TEST_SCHEMA: Schema = Schema {
            fields: &[
                (1, field("ok", ProtoType::Int, false)),
                (2, field("corrupt", ProtoType::String, false)),
            ],
        };

        let mut payload = encode_varint_field(1, 42);
        // Field 2 wire type 2 (length-delimited) with length = u64::MAX
        payload.extend(encode_varint((2u64 << 3) | 2));
        payload.extend(encode_varint(u64::MAX));
        payload.extend(b"trailing");

        let decoded = decode(&payload, &TEST_SCHEMA).unwrap();
        assert_eq!(decoded["ok"], 42);
        assert!(decoded.get("corrupt").is_none());
    }
}
