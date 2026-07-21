use std::ptr;

use yanxu_platform_native::abi::{
    ARRAY, BYTES, ERROR_VALUE, INTEGER, MAP, NUMBER, RESOURCE, STRING, Value, ValueData,
};
use yanxu_platform_native::bridge::decode_arguments;

const MAX_ARGUMENTS: usize = 65_536;
const MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_BINARY_BYTES: u64 = 16 * 1024 * 1024;

struct ValueCase {
    name: &'static str,
    value: Value,
    expected: &'static str,
}

#[test]
fn rejects_malformed_scalar_and_container_headers_with_stable_errors() {
    static INVALID_UTF8: [u8; 1] = [0xff];
    let cases = [
        ValueCase {
            name: "未知类型",
            value: Value {
                kind: u32::MAX,
                ..Value::default()
            },
            expected: "PLATFORM_VALUE_TYPE",
        },
        ValueCase {
            name: "错误值不能作为输入",
            value: Value {
                kind: ERROR_VALUE,
                ..Value::default()
            },
            expected: "PLATFORM_VALUE_TYPE",
        },
        ValueCase {
            name: "资源缺少句柄标志",
            value: Value {
                kind: RESOURCE,
                data: ValueData { handle: 1 },
                ..Value::default()
            },
            expected: "PLATFORM_VALUE_TYPE",
        },
        ValueCase {
            name: "非有限小数",
            value: Value {
                kind: NUMBER,
                data: ValueData {
                    number: f64::INFINITY,
                },
                ..Value::default()
            },
            expected: "PLATFORM_VALUE_TYPE",
        },
        ValueCase {
            name: "无效 UTF-8",
            value: Value {
                kind: STRING,
                length: 1,
                data: ValueData {
                    bytes: INVALID_UTF8.as_ptr(),
                },
                ..Value::default()
            },
            expected: "PLATFORM_VALUE_UTF8",
        },
        ValueCase {
            name: "非空文字空指针",
            value: Value {
                kind: STRING,
                length: 1,
                data: ValueData { bytes: ptr::null() },
                ..Value::default()
            },
            expected: "PLATFORM_VALUE_TYPE",
        },
        ValueCase {
            name: "文字超过上限",
            value: Value {
                kind: STRING,
                length: MAX_TEXT_BYTES + 1,
                data: ValueData { bytes: ptr::null() },
                ..Value::default()
            },
            expected: "PLATFORM_VALUE_LIMIT",
        },
        ValueCase {
            name: "非空字节空指针",
            value: Value {
                kind: BYTES,
                length: 1,
                data: ValueData { bytes: ptr::null() },
                ..Value::default()
            },
            expected: "PLATFORM_VALUE_TYPE",
        },
        ValueCase {
            name: "字节超过上限",
            value: Value {
                kind: BYTES,
                length: MAX_BINARY_BYTES + 1,
                data: ValueData { bytes: ptr::null() },
                ..Value::default()
            },
            expected: "PLATFORM_VALUE_LIMIT",
        },
        ValueCase {
            name: "非空数组空指针",
            value: Value {
                kind: ARRAY,
                length: 1,
                data: ValueData { items: ptr::null() },
                ..Value::default()
            },
            expected: "PLATFORM_VALUE_TYPE",
        },
        ValueCase {
            name: "数组计数超过上限",
            value: Value {
                kind: ARRAY,
                length: MAX_ARGUMENTS as u64 + 1,
                data: ValueData { items: ptr::null() },
                ..Value::default()
            },
            expected: "PLATFORM_VALUE_LIMIT",
        },
        ValueCase {
            name: "非空映射空指针",
            value: Value {
                kind: MAP,
                length: 1,
                data: ValueData { items: ptr::null() },
                ..Value::default()
            },
            expected: "PLATFORM_VALUE_TYPE",
        },
        ValueCase {
            name: "映射物理计数溢出",
            value: Value {
                kind: MAP,
                length: u64::MAX,
                data: ValueData { items: ptr::null() },
                ..Value::default()
            },
            expected: "PLATFORM_VALUE_LIMIT",
        },
    ];

    for case in cases {
        assert_eq!(
            unsafe { decode_arguments(&case.value, 1) },
            Err(case.expected),
            "ABI 值语料用例 {}",
            case.name
        );
    }
}

#[test]
fn rejects_invalid_and_duplicate_map_keys() {
    let invalid_key = [
        Value {
            kind: INTEGER,
            data: ValueData { integer: 7 },
            ..Value::default()
        },
        Value::default(),
    ];
    let invalid_map = Value {
        kind: MAP,
        length: 1,
        data: ValueData {
            items: invalid_key.as_ptr(),
        },
        ..Value::default()
    };
    assert_eq!(
        unsafe { decode_arguments(&invalid_map, 1) },
        Err("PLATFORM_VALUE_TYPE")
    );

    static KEY: &str = "重复";
    let duplicate_keys = [
        Value {
            kind: STRING,
            length: KEY.len() as u64,
            data: ValueData {
                bytes: KEY.as_ptr(),
            },
            ..Value::default()
        },
        Value::default(),
        Value {
            kind: STRING,
            length: KEY.len() as u64,
            data: ValueData {
                bytes: KEY.as_ptr(),
            },
            ..Value::default()
        },
        Value::default(),
    ];
    let duplicate_map = Value {
        kind: MAP,
        length: 2,
        data: ValueData {
            items: duplicate_keys.as_ptr(),
        },
        ..Value::default()
    };
    assert_eq!(
        unsafe { decode_arguments(&duplicate_map, 1) },
        Err("PLATFORM_VALUE_TYPE")
    );
}

#[test]
fn bounds_recursive_and_self_referential_containers() {
    let mut cycle = Box::new(Value {
        kind: ARRAY,
        length: 1,
        ..Value::default()
    });
    cycle.data = ValueData {
        items: ptr::from_ref(cycle.as_ref()),
    };
    assert_eq!(
        unsafe { decode_arguments(cycle.as_ref(), 1) },
        Err("PLATFORM_VALUE_LIMIT")
    );

    let mut nodes = vec![Box::new(Value::default())];
    for _ in 0..=65 {
        let child = ptr::from_ref(nodes.last().unwrap().as_ref());
        nodes.push(Box::new(Value {
            kind: ARRAY,
            length: 1,
            data: ValueData { items: child },
            ..Value::default()
        }));
    }
    assert_eq!(
        unsafe { decode_arguments(nodes.last().unwrap().as_ref(), 1) },
        Err("PLATFORM_VALUE_LIMIT")
    );
}

#[test]
fn bounds_top_level_argument_vectors_before_pointer_access() {
    assert_eq!(unsafe { decode_arguments(ptr::null(), 0) }, Ok(Vec::new()));
    assert_eq!(
        unsafe { decode_arguments(ptr::null(), 1) },
        Err("PLATFORM_VALUE_LIMIT")
    );
    assert_eq!(
        unsafe { decode_arguments(ptr::null(), MAX_ARGUMENTS + 1) },
        Err("PLATFORM_VALUE_LIMIT")
    );
}
