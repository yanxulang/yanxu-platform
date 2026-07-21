//! 言台原生后端。
//!
//! 公开边界是言序 ABI v2；Rust 模块同时作为 `rlib` 构建，以便协议、资源与无显示
//! 模型能够由普通单元测试覆盖。

pub mod abi;
pub mod accessibility;
pub mod backend;
pub mod bridge;
pub mod data;
pub mod draw;
pub mod event;
pub mod model;
mod native_accessibility;
pub mod protocol;
pub mod render;
pub mod sync;
pub mod text;
pub mod windowing;

use abi::{NativeError, NativeFunction, NativeModule, NativeResource, Value, ValueData};
use backend::{HostApi, Operation, Output};
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

static FUNCTIONS: &[(&[u8], Operation)] = &[
    ("协议查询".as_bytes(), Operation::ProtocolInfo),
    ("能力查询".as_bytes(), Operation::Capabilities),
    ("应用创建".as_bytes(), Operation::ApplicationCreate),
    ("窗口创建".as_bytes(), Operation::WindowCreate),
    ("窗口命令".as_bytes(), Operation::WindowCommand),
    ("窗口查询".as_bytes(), Operation::WindowQuery),
    ("事件冲刷".as_bytes(), Operation::FlushEvents),
    ("帧提交".as_bytes(), Operation::SubmitFrame),
    ("绘制解码".as_bytes(), Operation::InspectDraw),
    ("单调时间".as_bytes(), Operation::MonotonicTime),
    ("关闭".as_bytes(), Operation::Close),
    ("调试快照".as_bytes(), Operation::DebugSnapshot),
    ("字体枚举".as_bytes(), Operation::FontFamilies),
    ("字体匹配".as_bytes(), Operation::FontMatch),
    ("字体加载".as_bytes(), Operation::FontLoad),
    ("文字整形".as_bytes(), Operation::TextShape),
    ("文字测量".as_bytes(), Operation::TextMeasure),
    ("文字命中".as_bytes(), Operation::TextHitTest),
    ("计时器创建".as_bytes(), Operation::TimerCreate),
    ("计时器取消".as_bytes(), Operation::TimerCancel),
    ("剪贴板读取".as_bytes(), Operation::ClipboardRead),
    ("剪贴板写入".as_bytes(), Operation::ClipboardWrite),
    ("文件对话框".as_bytes(), Operation::FileDialog),
    ("图片加载".as_bytes(), Operation::ImageLoad),
    ("图片查询".as_bytes(), Operation::ImageInfo),
    ("输入法配置".as_bytes(), Operation::ImeConfigure),
    ("光标设置".as_bytes(), Operation::CursorSet),
    ("显示器查询".as_bytes(), Operation::Displays),
    ("主题查询".as_bytes(), Operation::Theme),
    ("应用运行".as_bytes(), Operation::ApplicationRun),
    ("应用退出".as_bytes(), Operation::ApplicationExit),
    ("事件唤醒".as_bytes(), Operation::Wake),
    ("计时器查询".as_bytes(), Operation::TimerQuery),
    ("绘制编码".as_bytes(), Operation::DrawEncode),
    ("剪贴板读取图片".as_bytes(), Operation::ClipboardReadImage),
    ("剪贴板写入图片".as_bytes(), Operation::ClipboardWriteImage),
    ("帧提交反馈".as_bytes(), Operation::SubmitFrameFeedback),
    ("无障碍更新".as_bytes(), Operation::AccessibilityUpdate),
    ("无障碍查询".as_bytes(), Operation::AccessibilityQuery),
    ("配额查询".as_bytes(), Operation::ResourceLimitsQuery),
    ("配额配置".as_bytes(), Operation::ResourceLimitsConfigure),
];

#[unsafe(no_mangle)]
pub extern "C" fn yanxu_native_module_v2() -> *const NativeModule {
    *MODULE.get_or_init(|| {
        let functions = Box::leak(
            FUNCTIONS
                .iter()
                .map(|(name, operation)| NativeFunction {
                    name: name.as_ptr(),
                    name_length: name.len(),
                    context: (*operation as usize) as *mut c_void,
                    call: Some(dispatch),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
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

unsafe extern "C" fn dispatch(
    context: *mut c_void,
    arguments: *const Value,
    count: usize,
    host: *const abi::NativeHost,
    output: *mut Value,
    error: *mut NativeError,
) -> i32 {
    if output.is_null() || host.is_null() {
        return fail(error, "PLATFORM_HOST_ABI", "输出指针为空");
    }
    unsafe { *output = Value::default() };
    if let Some(error) = unsafe { error.as_mut() } {
        *error = NativeError::default();
    }
    let Some(operation) = Operation::from_context(context) else {
        return fail(error, "PLATFORM_FUNCTION", "未知平台函数");
    };
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        let arguments = bridge::decode_arguments(arguments, count)?;
        backend::call(operation, &arguments, HostApi(*host))
    }));
    match result {
        Ok(Ok(Output::Value(value))) => {
            unsafe { *output = bridge::encode_data(value) };
            abi::OK
        }
        Ok(Ok(Output::Resource(resource))) => {
            let raw = Box::into_raw(resource.resource).cast::<c_void>();
            let descriptor = Box::new(NativeResource {
                struct_size: std::mem::size_of::<NativeResource>(),
                resource: raw,
                type_name: resource.type_name.as_ptr(),
                type_name_length: resource.type_name.len(),
                parent: resource.parent,
                drop_resource: Some(backend::drop_platform_resource),
            });
            unsafe {
                *output = Value {
                    kind: abi::RESOURCE,
                    data: ValueData {
                        resource: Box::into_raw(descriptor),
                    },
                    ..Value::default()
                };
            }
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
        assert_eq!(descriptor.function_count, FUNCTIONS.len());
        assert_eq!(descriptor.function_count, 41);
        assert_eq!(FUNCTIONS[39].0, "配额查询".as_bytes());
        assert_eq!(FUNCTIONS[40].0, "配额配置".as_bytes());
        assert_eq!(descriptor.resource_type_count, RESOURCE_TYPES.len());
        assert!(descriptor.free_value.is_some());
    }

    #[test]
    fn failed_dispatch_clears_stale_output_and_returns_stable_error() {
        let host = abi::NativeHost {
            abi_version: abi::ABI,
            struct_size: std::mem::size_of::<abi::NativeHost>(),
            context: ptr::null_mut(),
            callback_retain: None,
            callback_release: None,
            callback_post: None,
            wake: None,
            pump: None,
            has_permission: None,
            resource_get: None,
            event_loop_id: 0,
            owner_thread_token: 0,
        };
        let mut output = Value {
            kind: abi::STRING,
            length: 99,
            ..Value::default()
        };
        let mut error = NativeError::default();

        let status = unsafe {
            dispatch(
                usize::MAX as *mut c_void,
                ptr::null(),
                0,
                &host,
                &mut output,
                &mut error,
            )
        };

        assert_eq!(status, abi::ERROR);
        assert_eq!(output.kind, abi::NULL);
        assert_eq!(output.length, 0);
        assert_eq!(
            unsafe { std::slice::from_raw_parts(error.code, error.code_length) },
            b"PLATFORM_FUNCTION"
        );
    }
}
