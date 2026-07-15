//! 言台原生后端。
//!
//! 公开边界是言序 ABI v2；Rust 模块同时作为 `rlib` 构建，以便协议、资源与无显示
//! 模型能够由普通单元测试覆盖。

pub mod abi;
pub mod bridge;
pub mod data;
pub mod event;
pub mod model;
pub mod protocol;

use abi::{NativeError, NativeFunction, NativeModule, Value};
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::OnceLock;

static MODULE: OnceLock<usize> = OnceLock::new();
static MODULE_NAME: &[u8] = b"yanxu-platform";
static PANIC_CODE: &[u8] = b"PLATFORM_BACKEND_PANIC";
static PANIC_MESSAGE: &[u8] = b"panic isolated inside yanxu-platform backend";

static RESOURCE_TYPES: &[&[u8]] = &[
    b"yanxu.platform.application",
    b"yanxu.platform.window",
    b"yanxu.platform.timer",
    b"yanxu.platform.image",
    b"yanxu.platform.font",
];

#[unsafe(no_mangle)]
pub extern "C" fn yanxu_native_module_v2() -> *const NativeModule {
    *MODULE.get_or_init(|| {
        let functions: &'static mut [NativeFunction] = Box::leak(Vec::new().into_boxed_slice());
        let resource_types = Box::leak(
            RESOURCE_TYPES
                .iter()
                .map(|name| name.as_ptr())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let resource_lengths = Box::leak(
            RESOURCE_TYPES
                .iter()
                .map(|name| name.len())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        Box::into_raw(Box::new(NativeModule {
            abi_version: abi::ABI,
            struct_size: std::mem::size_of::<NativeModule>(),
            name: MODULE_NAME.as_ptr(),
            name_length: MODULE_NAME.len(),
            functions: functions.as_ptr(),
            function_count: functions.len(),
            constants: ptr::null(),
            constant_count: 0,
            resource_types: resource_types.as_ptr(),
            resource_type_lengths: resource_lengths.as_ptr(),
            resource_type_count: resource_types.len(),
            free_value: Some(bridge::free_value),
            capabilities: 0b1111_1111,
        })) as usize
    }) as *const NativeModule
}

#[allow(dead_code)]
unsafe extern "C" fn guarded_dispatch(
    _context: *mut c_void,
    arguments: *const Value,
    count: usize,
    _host: *const abi::NativeHost,
    output: *mut Value,
    error: *mut NativeError,
) -> i32 {
    if output.is_null() {
        return fail(error, "PLATFORM_HOST_ABI", "输出指针为空");
    }
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        bridge::decode_arguments(arguments, count)
    }));
    match result {
        Ok(Ok(_)) => {
            unsafe { *output = Value::default() };
            abi::OK
        }
        Ok(Err(code)) => fail(error, code, "ABI v2 参数无效"),
        Err(_) => {
            if let Some(error) = unsafe { error.as_mut() } {
                *error = NativeError {
                    code: PANIC_CODE.as_ptr(),
                    code_length: PANIC_CODE.len(),
                    message: PANIC_MESSAGE.as_ptr(),
                    message_length: PANIC_MESSAGE.len(),
                };
            }
            abi::ERROR
        }
    }
}

fn fail(error: *mut NativeError, code: &'static str, message: &'static str) -> i32 {
    if let Some(error) = unsafe { error.as_mut() } {
        *error = NativeError {
            code: code.as_ptr(),
            code_length: code.len(),
            message: message.as_ptr(),
            message_length: message.len(),
        };
    }
    abi::ERROR
}

#[cfg(test)]
mod ffi_tests {
    use super::*;

    #[test]
    fn exports_stable_abi_v2_descriptor() {
        let first = yanxu_native_module_v2();
        let second = yanxu_native_module_v2();
        assert_eq!(first, second);
        let descriptor = unsafe { &*first };
        assert_eq!(descriptor.abi_version, 2);
        assert_eq!(descriptor.struct_size, std::mem::size_of::<NativeModule>());
        assert_eq!(descriptor.name_length, MODULE_NAME.len());
        assert_eq!(descriptor.function_count, 0);
        assert_eq!(descriptor.resource_type_count, RESOURCE_TYPES.len());
        assert!(descriptor.free_value.is_some());
    }
}
