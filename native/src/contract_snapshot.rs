use super::*;
use crate::abi::{
    BOOL, BYTES, CALLBACK, ERROR_VALUE, FLAG_RESOURCE_HANDLE, FLAG_TRUE, INTEGER, MAP, NULL,
    NUMBER, NativeConstant, NativeFunction, NativeHost, NativeModule, NativeResource, RESOURCE,
    STRING, Value, ValueData,
};
use std::ffi::c_void;
use std::fmt::Write as _;
use std::ptr;

type Field<'a> = (&'a str, &'a str, usize);

const _: () = {
    assert!(usize::BITS == 64, "言台正式 ABI 只支持 64 位目标");
    assert!(
        cfg!(target_endian = "little"),
        "言台正式 ABI 只支持小端目标"
    );
};

fn write_layout(
    output: &mut String,
    name: &str,
    size: usize,
    align: usize,
    fields: &[Field<'_>],
    last: bool,
) {
    writeln!(output, "      {{").unwrap();
    writeln!(output, "        \"name\": \"{name}\",").unwrap();
    writeln!(output, "        \"size\": {size},").unwrap();
    writeln!(output, "        \"align\": {align},").unwrap();
    writeln!(output, "        \"fields\": [").unwrap();
    for (index, (field, kind, offset)) in fields.iter().enumerate() {
        let comma = if index + 1 == fields.len() { "" } else { "," };
        writeln!(
            output,
            "          {{ \"name\": \"{field}\", \"type\": \"{kind}\", \"offset\": {offset} }}{comma}"
        )
        .unwrap();
    }
    writeln!(output, "        ]").unwrap();
    writeln!(output, "      }}{}", if last { "" } else { "," }).unwrap();
}

fn assert_c_signatures(descriptor: &NativeModule, functions: &[NativeFunction]) {
    let _: extern "C" fn() -> *const NativeModule = yanxu_native_module_v2;
    let _: unsafe extern "C" fn(*mut Value) =
        descriptor.free_value.expect("ABI v2 必须提供值释放函数");

    for function in functions {
        let _: unsafe extern "C" fn(
            *mut c_void,
            *const Value,
            usize,
            *const NativeHost,
            *mut Value,
            *mut NativeError,
        ) -> i32 = function.call.expect("每个 ABI v2 操作都必须可调用");
    }

    let host = NativeHost {
        abi_version: crate::abi::ABI,
        struct_size: std::mem::size_of::<NativeHost>(),
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
    let _: Option<unsafe extern "C" fn(*mut c_void, u64) -> i32> = host.callback_retain;
    let _: Option<unsafe extern "C" fn(*mut c_void, u64) -> i32> = host.callback_release;
    let _: Option<
        unsafe extern "C" fn(*mut c_void, u64, *const Value, usize, *mut NativeError) -> i32,
    > = host.callback_post;
    let _: Option<unsafe extern "C" fn(*mut c_void)> = host.wake;
    let _: Option<unsafe extern "C" fn(*mut c_void, usize, *mut NativeError) -> i32> = host.pump;
    let _: Option<unsafe extern "C" fn(*mut c_void, *const u8, usize) -> i32> = host.has_permission;
    let _: Option<unsafe extern "C" fn(*mut c_void, u64, *mut *mut c_void) -> i32> =
        host.resource_get;

    let resource = NativeResource {
        struct_size: std::mem::size_of::<NativeResource>(),
        resource: ptr::null_mut(),
        type_name: ptr::null(),
        type_name_length: 0,
        parent: 0,
        drop_resource: None,
    };
    let _: Option<unsafe extern "C" fn(*mut c_void)> = resource.drop_resource;
}

fn native_abi_snapshot() -> String {
    let descriptor_pointer = yanxu_native_module_v2();
    assert!(!descriptor_pointer.is_null(), "ABI v2 模块描述符不能为空");
    let descriptor = unsafe { &*descriptor_pointer };
    assert_eq!(
        descriptor.struct_size,
        std::mem::size_of::<NativeModule>(),
        "ABI v2 模块必须声明完整结构大小"
    );
    assert_eq!(
        descriptor.name_length,
        MODULE_NAME.len(),
        "ABI v2 模块名称长度必须稳定"
    );
    assert!(!descriptor.name.is_null(), "ABI v2 模块名称不能为空");
    let module_name = std::str::from_utf8(unsafe {
        std::slice::from_raw_parts(descriptor.name, descriptor.name_length)
    })
    .unwrap();
    assert_eq!(
        descriptor.function_count,
        FUNCTIONS.len(),
        "ABI v2 操作数量必须与导出表一致"
    );
    assert!(!descriptor.functions.is_null(), "ABI v2 操作表不能为空");
    let functions =
        unsafe { std::slice::from_raw_parts(descriptor.functions, descriptor.function_count) };
    assert_eq!(descriptor.constant_count, 0, "1.0 ABI 不导出常量");
    assert!(
        descriptor.constants.is_null(),
        "零长度 ABI 常量表必须使用空指针"
    );
    assert_eq!(
        descriptor.resource_type_count,
        RESOURCE_TYPES.len(),
        "ABI v2 资源类型数量必须与导出表一致"
    );
    assert!(
        !descriptor.resource_types.is_null() && !descriptor.resource_type_lengths.is_null(),
        "ABI v2 资源类型表不能为空"
    );
    let resource_types = unsafe {
        std::slice::from_raw_parts(descriptor.resource_types, descriptor.resource_type_count)
    };
    let resource_type_lengths = unsafe {
        std::slice::from_raw_parts(
            descriptor.resource_type_lengths,
            descriptor.resource_type_count,
        )
    };
    assert_c_signatures(descriptor, functions);
    let value_kinds = [
        (NULL, "null"),
        (BOOL, "bool"),
        (INTEGER, "integer"),
        (NUMBER, "number"),
        (STRING, "string"),
        (BYTES, "bytes"),
        (crate::abi::ARRAY, "array"),
        (MAP, "map"),
        (RESOURCE, "resource"),
        (CALLBACK, "callback"),
        (ERROR_VALUE, "error"),
    ];

    let mut output = String::new();
    output.push_str("{\n");
    output.push_str("  \"schema\": 1,\n");
    output.push_str("  \"contract\": \"yanxu-platform-native-abi-v2\",\n");
    output.push_str("  \"host\": {\n");
    output.push_str("    \"minimum_yanxu\": \"1.1.7\",\n");
    output.push_str("    \"reference_tag\": \"v1.1.7\",\n");
    output
        .push_str("    \"reference_tag_object\": \"7bfac5b88cb6c1e99502f9b0a1af2ebf44099398\",\n");
    output.push_str("    \"reference_commit\": \"7d0e8b2b7bdd1125a7d271bce690ab854c79c31c\"\n");
    output.push_str("  },\n");
    output.push_str("  \"data_model\": {\n");
    writeln!(output, "    \"pointer_width\": {},", usize::BITS).unwrap();
    writeln!(output, "    \"size_t_width\": {},", usize::BITS).unwrap();
    writeln!(
        output,
        "    \"endianness\": \"{}\"",
        if cfg!(target_endian = "little") {
            "little"
        } else {
            "big"
        }
    )
    .unwrap();
    output.push_str("  },\n");
    output.push_str("  \"abi\": {\n");
    writeln!(output, "    \"version\": {},", descriptor.abi_version).unwrap();
    output.push_str("    \"calling_convention\": \"C\",\n");
    output.push_str("    \"entry_symbol\": \"yanxu_native_module_v2\",\n");
    writeln!(
        output,
        "    \"status\": {{ \"ok\": {}, \"error\": {} }},",
        crate::abi::OK,
        crate::abi::ERROR
    )
    .unwrap();
    output.push_str("    \"aliases\": {\n");
    output.push_str("      \"YanxuCallbackHandleV2\": \"uint64_t\",\n");
    output.push_str("      \"YanxuResourceHandleV2\": \"uint64_t\"\n");
    output.push_str("    },\n");
    output.push_str("    \"value_kinds\": [\n");
    for (index, (id, name)) in value_kinds.iter().enumerate() {
        writeln!(
            output,
            "      {{ \"id\": {id}, \"name\": \"{name}\" }}{}",
            if index + 1 == value_kinds.len() {
                ""
            } else {
                ","
            }
        )
        .unwrap();
    }
    output.push_str("    ],\n");
    writeln!(
        output,
        "    \"value_flags\": {{ \"true\": {FLAG_TRUE}, \"resource_handle\": {FLAG_RESOURCE_HANDLE} }},"
    )
    .unwrap();
    output.push_str("    \"signatures\": [\n");
    output.push_str("      { \"name\": \"YanxuNativeDropResourceV2\", \"declaration\": \"void (*)(void *)\" },\n");
    output.push_str("      { \"name\": \"YanxuCallbackRetainV2\", \"declaration\": \"int32_t (*)(void *, uint64_t)\" },\n");
    output.push_str("      { \"name\": \"YanxuCallbackReleaseV2\", \"declaration\": \"int32_t (*)(void *, uint64_t)\" },\n");
    output.push_str("      { \"name\": \"YanxuCallbackPostV2\", \"declaration\": \"int32_t (*)(void *, uint64_t, const YanxuValueV2 *, size_t, YanxuNativeErrorV2 *)\" },\n");
    output.push_str(
        "      { \"name\": \"YanxuHostWakeV2\", \"declaration\": \"void (*)(void *)\" },\n",
    );
    output.push_str("      { \"name\": \"YanxuHostPumpV2\", \"declaration\": \"int32_t (*)(void *, size_t, YanxuNativeErrorV2 *)\" },\n");
    output.push_str("      { \"name\": \"YanxuHostPermissionV2\", \"declaration\": \"int32_t (*)(void *, const uint8_t *, size_t)\" },\n");
    output.push_str("      { \"name\": \"YanxuHostResourceGetV2\", \"declaration\": \"int32_t (*)(void *, uint64_t, void **)\" },\n");
    output.push_str("      { \"name\": \"YanxuNativeFunctionPointerV2\", \"declaration\": \"int32_t (*)(void *, const YanxuValueV2 *, size_t, const YanxuNativeHostV2 *, YanxuValueV2 *, YanxuNativeErrorV2 *)\" },\n");
    output.push_str("      { \"name\": \"YanxuNativeFreeValueV2\", \"declaration\": \"void (*)(YanxuValueV2 *)\" },\n");
    output.push_str("      { \"name\": \"YanxuNativeModuleEntryV2\", \"declaration\": \"const YanxuNativeModuleV2 *(*)(void)\" }\n");
    output.push_str("    ],\n");
    output.push_str("    \"layouts\": [\n");
    write_layout(
        &mut output,
        "YanxuValueDataV2",
        std::mem::size_of::<ValueData>(),
        std::mem::align_of::<ValueData>(),
        &[
            ("integer", "int64_t", 0),
            ("number", "double", 0),
            ("bytes", "const uint8_t *", 0),
            ("items", "const YanxuValueV2 *", 0),
            ("resource", "YanxuNativeResourceV2 *", 0),
            ("handle", "uint64_t", 0),
        ],
        false,
    );
    write_layout(
        &mut output,
        "YanxuValueV2",
        std::mem::size_of::<Value>(),
        std::mem::align_of::<Value>(),
        &[
            ("kind", "uint32_t", std::mem::offset_of!(Value, kind)),
            ("flags", "uint32_t", std::mem::offset_of!(Value, flags)),
            ("length", "uint64_t", std::mem::offset_of!(Value, length)),
            (
                "value",
                "YanxuValueDataV2",
                std::mem::offset_of!(Value, data),
            ),
        ],
        false,
    );
    write_layout(
        &mut output,
        "YanxuNativeErrorV2",
        std::mem::size_of::<NativeError>(),
        std::mem::align_of::<NativeError>(),
        &[
            (
                "code",
                "const uint8_t *",
                std::mem::offset_of!(NativeError, code),
            ),
            (
                "code_length",
                "size_t",
                std::mem::offset_of!(NativeError, code_length),
            ),
            (
                "message",
                "const uint8_t *",
                std::mem::offset_of!(NativeError, message),
            ),
            (
                "message_length",
                "size_t",
                std::mem::offset_of!(NativeError, message_length),
            ),
        ],
        false,
    );
    write_layout(
        &mut output,
        "YanxuNativeResourceV2",
        std::mem::size_of::<NativeResource>(),
        std::mem::align_of::<NativeResource>(),
        &[
            (
                "struct_size",
                "size_t",
                std::mem::offset_of!(NativeResource, struct_size),
            ),
            (
                "resource",
                "void *",
                std::mem::offset_of!(NativeResource, resource),
            ),
            (
                "type_name",
                "const uint8_t *",
                std::mem::offset_of!(NativeResource, type_name),
            ),
            (
                "type_name_length",
                "size_t",
                std::mem::offset_of!(NativeResource, type_name_length),
            ),
            (
                "parent",
                "YanxuResourceHandleV2",
                std::mem::offset_of!(NativeResource, parent),
            ),
            (
                "drop_resource",
                "YanxuNativeDropResourceV2",
                std::mem::offset_of!(NativeResource, drop_resource),
            ),
        ],
        false,
    );
    write_layout(
        &mut output,
        "YanxuNativeHostV2",
        std::mem::size_of::<NativeHost>(),
        std::mem::align_of::<NativeHost>(),
        &[
            (
                "abi_version",
                "uint32_t",
                std::mem::offset_of!(NativeHost, abi_version),
            ),
            (
                "struct_size",
                "size_t",
                std::mem::offset_of!(NativeHost, struct_size),
            ),
            (
                "context",
                "void *",
                std::mem::offset_of!(NativeHost, context),
            ),
            (
                "callback_retain",
                "YanxuCallbackRetainV2",
                std::mem::offset_of!(NativeHost, callback_retain),
            ),
            (
                "callback_release",
                "YanxuCallbackReleaseV2",
                std::mem::offset_of!(NativeHost, callback_release),
            ),
            (
                "callback_post",
                "YanxuCallbackPostV2",
                std::mem::offset_of!(NativeHost, callback_post),
            ),
            (
                "wake",
                "YanxuHostWakeV2",
                std::mem::offset_of!(NativeHost, wake),
            ),
            (
                "pump",
                "YanxuHostPumpV2",
                std::mem::offset_of!(NativeHost, pump),
            ),
            (
                "has_permission",
                "YanxuHostPermissionV2",
                std::mem::offset_of!(NativeHost, has_permission),
            ),
            (
                "resource_get",
                "YanxuHostResourceGetV2",
                std::mem::offset_of!(NativeHost, resource_get),
            ),
            (
                "event_loop_id",
                "uint64_t",
                std::mem::offset_of!(NativeHost, event_loop_id),
            ),
            (
                "owner_thread_token",
                "uint64_t",
                std::mem::offset_of!(NativeHost, owner_thread_token),
            ),
        ],
        false,
    );
    write_layout(
        &mut output,
        "YanxuNativeFunctionV2",
        std::mem::size_of::<NativeFunction>(),
        std::mem::align_of::<NativeFunction>(),
        &[
            (
                "name",
                "const uint8_t *",
                std::mem::offset_of!(NativeFunction, name),
            ),
            (
                "name_length",
                "size_t",
                std::mem::offset_of!(NativeFunction, name_length),
            ),
            (
                "context",
                "void *",
                std::mem::offset_of!(NativeFunction, context),
            ),
            (
                "call",
                "YanxuNativeFunctionPointerV2",
                std::mem::offset_of!(NativeFunction, call),
            ),
        ],
        false,
    );
    write_layout(
        &mut output,
        "YanxuNativeConstantV2",
        std::mem::size_of::<NativeConstant>(),
        std::mem::align_of::<NativeConstant>(),
        &[
            (
                "name",
                "const uint8_t *",
                std::mem::offset_of!(NativeConstant, name),
            ),
            (
                "name_length",
                "size_t",
                std::mem::offset_of!(NativeConstant, name_length),
            ),
            (
                "value",
                "const YanxuValueV2 *",
                std::mem::offset_of!(NativeConstant, value),
            ),
        ],
        false,
    );
    write_layout(
        &mut output,
        "YanxuNativeModuleV2",
        std::mem::size_of::<NativeModule>(),
        std::mem::align_of::<NativeModule>(),
        &[
            (
                "abi_version",
                "uint32_t",
                std::mem::offset_of!(NativeModule, abi_version),
            ),
            (
                "struct_size",
                "size_t",
                std::mem::offset_of!(NativeModule, struct_size),
            ),
            (
                "name",
                "const uint8_t *",
                std::mem::offset_of!(NativeModule, name),
            ),
            (
                "name_length",
                "size_t",
                std::mem::offset_of!(NativeModule, name_length),
            ),
            (
                "functions",
                "const YanxuNativeFunctionV2 *",
                std::mem::offset_of!(NativeModule, functions),
            ),
            (
                "function_count",
                "size_t",
                std::mem::offset_of!(NativeModule, function_count),
            ),
            (
                "constants",
                "const YanxuNativeConstantV2 *",
                std::mem::offset_of!(NativeModule, constants),
            ),
            (
                "constant_count",
                "size_t",
                std::mem::offset_of!(NativeModule, constant_count),
            ),
            (
                "resource_types",
                "const uint8_t *const *",
                std::mem::offset_of!(NativeModule, resource_types),
            ),
            (
                "resource_type_lengths",
                "const size_t *",
                std::mem::offset_of!(NativeModule, resource_type_lengths),
            ),
            (
                "resource_type_count",
                "size_t",
                std::mem::offset_of!(NativeModule, resource_type_count),
            ),
            (
                "free_value",
                "YanxuNativeFreeValueV2",
                std::mem::offset_of!(NativeModule, free_value),
            ),
            (
                "capabilities",
                "uint64_t",
                std::mem::offset_of!(NativeModule, capabilities),
            ),
        ],
        true,
    );
    output.push_str("    ]\n");
    output.push_str("  },\n");
    output.push_str("  \"module\": {\n");
    writeln!(output, "    \"name\": \"{module_name}\",").unwrap();
    writeln!(output, "    \"struct_size\": {},", descriptor.struct_size).unwrap();
    writeln!(output, "    \"capabilities\": {},", descriptor.capabilities).unwrap();
    writeln!(
        output,
        "    \"constant_count\": {},",
        descriptor.constant_count
    )
    .unwrap();
    writeln!(
        output,
        "    \"free_value\": {},",
        descriptor.free_value.is_some()
    )
    .unwrap();
    output.push_str("    \"functions\": [\n");
    for (index, function) in functions.iter().enumerate() {
        let id = function.context as usize;
        assert_eq!(id, index + 1, "ABI 操作编号必须连续且只能在尾部追加");
        assert!(
            !function.name.is_null() && (1..=1_024).contains(&function.name_length),
            "ABI 操作名称必须是有界的非空字节串"
        );
        let name = std::str::from_utf8(unsafe {
            std::slice::from_raw_parts(function.name, function.name_length)
        })
        .unwrap();
        writeln!(
            output,
            "      {{ \"id\": {id}, \"name\": \"{name}\", \"call\": {} }}{}",
            function.call.is_some(),
            if index + 1 == functions.len() {
                ""
            } else {
                ","
            }
        )
        .unwrap();
    }
    output.push_str("    ],\n");
    output.push_str("    \"resource_types\": [\n");
    for (index, (&resource_type, &length)) in
        resource_types.iter().zip(resource_type_lengths).enumerate()
    {
        assert!(
            !resource_type.is_null() && (1..=1_024).contains(&length),
            "ABI 资源类型必须是有界的非空字节串"
        );
        let resource_type =
            std::str::from_utf8(unsafe { std::slice::from_raw_parts(resource_type, length) })
                .unwrap();
        writeln!(
            output,
            "      \"{resource_type}\"{}",
            if index + 1 == resource_types.len() {
                ""
            } else {
                ","
            }
        )
        .unwrap();
    }
    output.push_str("    ]\n");
    output.push_str("  }\n");
    output.push_str("}\n");
    output
}

#[test]
fn native_abi_v2_matches_the_1_0_frozen_snapshot() {
    assert_eq!(
        native_abi_snapshot(),
        include_str!("../../api/native-abi-v2.json")
    );
}
