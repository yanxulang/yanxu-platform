//! ABI v2 值与内部 `Data` 的有界转换和所有权回收。

use crate::abi::*;
use crate::data::Data;
use std::collections::BTreeMap;
use std::ptr;

const MAX_ARGUMENTS: usize = 65_536;
const MAX_DEPTH: usize = 64;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_BINARY_BYTES: usize = 16 * 1024 * 1024;

/// 深拷贝一次 ABI v2 调用的借用参数。
///
/// # Safety
///
/// `arguments` 在 `count` 非零时必须指向至少 `count` 个调用期有效的 ABI v2 值；
/// 递归指针也必须满足 ABI v2 的借用约定。
pub unsafe fn decode_arguments(
    arguments: *const Value,
    count: usize,
) -> Result<Vec<Data>, &'static str> {
    if count > MAX_ARGUMENTS || (count > 0 && arguments.is_null()) {
        return Err("PLATFORM_VALUE_LIMIT");
    }
    let arguments = if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(arguments, count) }
    };
    arguments
        .iter()
        .map(|value| unsafe { decode_value(value, 0) })
        .collect()
}

unsafe fn decode_value(value: &Value, depth: usize) -> Result<Data, &'static str> {
    if depth > MAX_DEPTH {
        return Err("PLATFORM_VALUE_LIMIT");
    }
    Ok(match value.kind {
        NULL => Data::Nil,
        BOOL => Data::Bool(value.flags & FLAG_TRUE != 0),
        INTEGER => Data::Integer(unsafe { value.data.integer }),
        NUMBER => {
            let number = unsafe { value.data.number };
            if !number.is_finite() {
                return Err("PLATFORM_VALUE_TYPE");
            }
            Data::Number(number)
        }
        STRING => Data::String(
            String::from_utf8(unsafe { copy_bytes(value, MAX_TEXT_BYTES) }?)
                .map_err(|_| "PLATFORM_VALUE_UTF8")?,
        ),
        BYTES => Data::Bytes(unsafe { copy_bytes(value, MAX_BINARY_BYTES) }?),
        ARRAY => {
            let count = usize::try_from(value.length).map_err(|_| "PLATFORM_VALUE_LIMIT")?;
            let values = unsafe { value_slice(value.data.items, count) }?;
            Data::Array(
                values
                    .iter()
                    .map(|value| unsafe { decode_value(value, depth + 1) })
                    .collect::<Result<Vec<_>, _>>()?,
            )
        }
        MAP => {
            let count = usize::try_from(value.length).map_err(|_| "PLATFORM_VALUE_LIMIT")?;
            let item_count = count.checked_mul(2).ok_or("PLATFORM_VALUE_LIMIT")?;
            let values = unsafe { value_slice(value.data.items, item_count) }?;
            let mut map = BTreeMap::new();
            for pair in values.chunks_exact(2) {
                let Data::String(key) = (unsafe { decode_value(&pair[0], depth + 1) })? else {
                    return Err("PLATFORM_VALUE_TYPE");
                };
                let value = unsafe { decode_value(&pair[1], depth + 1) }?;
                if map.insert(key, value).is_some() {
                    return Err("PLATFORM_VALUE_TYPE");
                }
            }
            Data::Map(map)
        }
        RESOURCE if value.flags & FLAG_RESOURCE_HANDLE != 0 => {
            Data::Resource(unsafe { value.data.handle })
        }
        CALLBACK => Data::Callback(unsafe { value.data.handle }),
        _ => return Err("PLATFORM_VALUE_TYPE"),
    })
}

unsafe fn copy_bytes(value: &Value, limit: usize) -> Result<Vec<u8>, &'static str> {
    let length = usize::try_from(value.length).map_err(|_| "PLATFORM_VALUE_LIMIT")?;
    if length > limit {
        return Err("PLATFORM_VALUE_LIMIT");
    }
    if length == 0 {
        return Ok(Vec::new());
    }
    let pointer = unsafe { value.data.bytes };
    if pointer.is_null() {
        return Err("PLATFORM_VALUE_TYPE");
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec())
}

unsafe fn value_slice<'a>(
    pointer: *const Value,
    length: usize,
) -> Result<&'a [Value], &'static str> {
    if length > MAX_ARGUMENTS {
        return Err("PLATFORM_VALUE_LIMIT");
    }
    if length == 0 {
        return Ok(&[]);
    }
    if pointer.is_null() {
        return Err("PLATFORM_VALUE_TYPE");
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer, length) })
}

#[must_use]
pub fn encode_data(data: Data) -> Value {
    match data {
        Data::Nil => Value::default(),
        Data::Bool(value) => Value {
            kind: BOOL,
            flags: if value { FLAG_TRUE } else { 0 },
            ..Value::default()
        },
        Data::Integer(value) => Value {
            kind: INTEGER,
            data: ValueData { integer: value },
            ..Value::default()
        },
        Data::Number(value) => Value {
            kind: NUMBER,
            data: ValueData { number: value },
            ..Value::default()
        },
        Data::String(value) => encode_bytes(STRING, value.into_bytes()),
        Data::Bytes(value) => encode_bytes(BYTES, value),
        Data::Array(values) => {
            encode_children(ARRAY, values.into_iter().map(encode_data).collect(), false)
        }
        Data::Map(values) => {
            let length = values.len();
            let mut children = Vec::with_capacity(length.saturating_mul(2));
            for (key, value) in values {
                children.push(encode_data(Data::String(key)));
                children.push(encode_data(value));
            }
            encode_children(MAP, children, true)
        }
        Data::Resource(handle) => Value {
            kind: RESOURCE,
            flags: FLAG_RESOURCE_HANDLE,
            data: ValueData { handle },
            ..Value::default()
        },
        Data::Callback(handle) => Value {
            kind: CALLBACK,
            data: ValueData { handle },
            ..Value::default()
        },
    }
}

fn encode_bytes(kind: u32, bytes: Vec<u8>) -> Value {
    if bytes.is_empty() {
        return Value {
            kind,
            ..Value::default()
        };
    }
    let bytes = bytes.into_boxed_slice();
    let length = bytes.len() as u64;
    let pointer = Box::into_raw(bytes) as *mut u8;
    Value {
        kind,
        length,
        data: ValueData { bytes: pointer },
        ..Value::default()
    }
}

fn encode_children(kind: u32, children: Vec<Value>, map: bool) -> Value {
    let logical_length = if map {
        children.len() / 2
    } else {
        children.len()
    };
    if children.is_empty() {
        return Value {
            kind,
            ..Value::default()
        };
    }
    let children = children.into_boxed_slice();
    let pointer = Box::into_raw(children) as *mut Value;
    Value {
        kind,
        length: logical_length as u64,
        data: ValueData { items: pointer },
        ..Value::default()
    }
}

/// 释放由本模块 `encode_data` 或资源输出创建的完整 ABI v2 值树。
///
/// # Safety
///
/// `value` 必须为空，或指向由本模块初始化且尚未由其他所有者释放的 `Value`。
pub unsafe extern "C" fn free_value(value: *mut Value) {
    let Some(value) = (unsafe { value.as_mut() }) else {
        return;
    };
    unsafe { free_value_inner(value) };
    *value = Value::default();
}

unsafe fn free_value_inner(value: &mut Value) {
    match value.kind {
        STRING | BYTES => {
            let length = usize::try_from(value.length).unwrap_or(0);
            let pointer = unsafe { value.data.bytes as *mut u8 };
            if length > 0 && !pointer.is_null() {
                drop(unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(pointer, length)) });
            }
        }
        ARRAY | MAP | ERROR_VALUE => {
            let logical = usize::try_from(value.length).unwrap_or(0);
            let length = if value.kind == MAP {
                logical.saturating_mul(2)
            } else {
                logical
            };
            let pointer = unsafe { value.data.items as *mut Value };
            if length > 0 && !pointer.is_null() {
                let mut values =
                    unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(pointer, length)) };
                for value in &mut values {
                    unsafe { free_value_inner(value) };
                }
            }
        }
        RESOURCE if value.flags & FLAG_RESOURCE_HANDLE == 0 => {
            let pointer = unsafe { value.data.resource };
            if !pointer.is_null() {
                let descriptor = unsafe { Box::from_raw(pointer) };
                if !descriptor.resource.is_null()
                    && let Some(drop_resource) = descriptor.drop_resource
                {
                    unsafe { drop_resource(descriptor.resource) };
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursive_value_round_trips_and_frees_once() {
        let expected = Data::map([
            ("空", Data::Nil),
            ("逻辑", Data::Bool(true)),
            ("整数", Data::Integer(42)),
            ("小数", Data::Number(3.5)),
            ("文字", Data::String("中文".to_owned())),
            ("字节", Data::Bytes(vec![0, 255, 7])),
            (
                "列",
                Data::Array(vec![Data::Resource(8), Data::Callback(9)]),
            ),
        ]);
        let mut encoded = encode_data(expected.clone());
        let decoded = unsafe { decode_arguments(&encoded, 1) }.unwrap();
        assert_eq!(decoded, vec![expected]);
        unsafe { free_value(&mut encoded) };
        assert_eq!(encoded.kind, NULL);
        unsafe { free_value(&mut encoded) };
        assert_eq!(encoded.kind, NULL);
    }

    #[test]
    fn rejects_non_finite_number() {
        let value = Value {
            kind: NUMBER,
            data: ValueData { number: f64::NAN },
            ..Value::default()
        };
        assert_eq!(
            unsafe { decode_arguments(&value, 1) },
            Err("PLATFORM_VALUE_TYPE")
        );
    }

    #[test]
    fn rejects_null_non_empty_bytes() {
        let value = Value {
            kind: BYTES,
            length: 1,
            data: ValueData { bytes: ptr::null() },
            ..Value::default()
        };
        assert_eq!(
            unsafe { decode_arguments(&value, 1) },
            Err("PLATFORM_VALUE_TYPE")
        );
    }
}
