use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

type ErrorContract = (&'static str, &'static str, &'static str, &'static str);

const ERROR_CODES: &[ErrorContract] = &[
    (
        "PLATFORM_ACCESSIBILITY_ACTION",
        "accessibility",
        "0.6.0",
        "无障碍动作、目标状态或参数无效",
    ),
    (
        "PLATFORM_ACCESSIBILITY_DUPLICATE",
        "accessibility",
        "0.6.0",
        "语义树含重复节点编号",
    ),
    (
        "PLATFORM_ACCESSIBILITY_FOCUS",
        "accessibility",
        "0.6.0",
        "语义树焦点不唯一或目标不可聚焦",
    ),
    (
        "PLATFORM_ACCESSIBILITY_LIMIT",
        "accessibility",
        "0.6.0",
        "语义树超过节点、深度、文字或操作上限",
    ),
    (
        "PLATFORM_ACCESSIBILITY_NODE",
        "accessibility",
        "0.6.0",
        "当前树修订中不存在目标节点",
    ),
    (
        "PLATFORM_ACCESSIBILITY_REVISION",
        "accessibility",
        "0.6.0",
        "语义树修订计数已耗尽",
    ),
    (
        "PLATFORM_ACCESSIBILITY_ROLE",
        "accessibility",
        "0.6.0",
        "无障碍角色未知",
    ),
    (
        "PLATFORM_ACCESSIBILITY_STATE",
        "accessibility",
        "0.6.0",
        "无障碍状态名称、类型或角色组合无效",
    ),
    (
        "PLATFORM_ACCESSIBILITY_TREE",
        "accessibility",
        "0.6.0",
        "语义树结构或必需字段无效",
    ),
    (
        "PLATFORM_APPLICATION_EXITED",
        "lifecycle",
        "0.8.0",
        "已退出应用不能再次运行",
    ),
    (
        "PLATFORM_ARGUMENT_COUNT",
        "input",
        "0.1.0",
        "原生操作参数数量不匹配",
    ),
    (
        "PLATFORM_BACKEND_PANIC",
        "runtime",
        "0.1.0",
        "后端恐慌已在 FFI 边界隔离",
    ),
    (
        "PLATFORM_CALLBACK_POST",
        "callback",
        "0.1.0",
        "事件回调投递失败",
    ),
    (
        "PLATFORM_CALLBACK_PUMP",
        "callback",
        "0.1.0",
        "宿主回调泵执行失败",
    ),
    (
        "PLATFORM_CALLBACK_RELEASED",
        "callback",
        "0.1.0",
        "回调句柄已释放或无法保留",
    ),
    (
        "PLATFORM_CLIPBOARD",
        "clipboard",
        "0.1.0",
        "系统剪贴板访问失败",
    ),
    (
        "PLATFORM_CLIPBOARD_IMAGE",
        "clipboard",
        "0.4.0",
        "剪贴板图片格式、尺寸或字节数无效",
    ),
    (
        "PLATFORM_CLIPBOARD_LIMIT",
        "clipboard",
        "0.4.0",
        "剪贴板文字或图片超过容量上限",
    ),
    (
        "PLATFORM_DIALOG_KIND",
        "dialog",
        "0.1.0",
        "文件对话框种类未知",
    ),
    (
        "PLATFORM_DRAW_COMMAND",
        "draw",
        "0.1.0",
        "结构化绘制命令未知",
    ),
    ("PLATFORM_DRAW_CORRUPT", "draw", "0.1.0", "二进制绘制帧损坏"),
    (
        "PLATFORM_DRAW_FIELD",
        "draw",
        "0.1.0",
        "结构化绘制命令缺少必需字段",
    ),
    (
        "PLATFORM_DRAW_LIMIT",
        "draw",
        "0.1.0",
        "绘制帧、命令、文字或路径超过上限",
    ),
    (
        "PLATFORM_DRAW_MAJOR",
        "draw",
        "0.1.0",
        "绘制协议主版本不兼容",
    ),
    (
        "PLATFORM_DRAW_NUMBER",
        "draw",
        "0.1.0",
        "绘制数据含非有限数或无法表达的浮点数",
    ),
    (
        "PLATFORM_DRAW_RANGE",
        "draw",
        "0.1.0",
        "绘制字段数值超出允许范围",
    ),
    (
        "PLATFORM_DRAW_TYPE",
        "draw",
        "0.1.0",
        "结构化绘制字段类型无效",
    ),
    (
        "PLATFORM_DRAW_UTF8",
        "draw",
        "0.1.0",
        "绘制协议文字不是有效 UTF-8",
    ),
    ("PLATFORM_DRAW_VALUE", "draw", "0.1.0", "绘制字段枚举值无效"),
    (
        "PLATFORM_EVENT_LOOP",
        "window",
        "0.1.0",
        "系统事件循环创建或运行失败",
    ),
    (
        "PLATFORM_EVENT_LOOP_RUNNING",
        "lifecycle",
        "0.1.0",
        "应用事件循环已经运行",
    ),
    (
        "PLATFORM_FONT_INVALID",
        "text",
        "0.1.0",
        "字体数据无效或没有可用字体",
    ),
    (
        "PLATFORM_FRAME_SEQUENCE",
        "frame",
        "0.3.0",
        "窗口帧序号已耗尽",
    ),
    ("PLATFORM_FUNCTION", "abi", "0.1.0", "原生操作编号未知"),
    (
        "PLATFORM_HOST_ABI",
        "abi",
        "0.1.0",
        "宿主 ABI 版本、结构或必需指针无效",
    ),
    (
        "PLATFORM_HOST_MISSING",
        "abi",
        "0.1.0",
        "宿主缺少当前操作必需的函数",
    ),
    (
        "PLATFORM_IMAGE_INVALID",
        "image",
        "0.1.0",
        "图片格式、尺寸或解码结果无效",
    ),
    (
        "PLATFORM_PERMISSION_CLIPBOARD",
        "permission",
        "0.1.0",
        "缺少剪贴板权限",
    ),
    (
        "PLATFORM_PERMISSION_DIALOG",
        "permission",
        "0.1.0",
        "缺少文件对话框权限",
    ),
    (
        "PLATFORM_PERMISSION_GUI",
        "permission",
        "0.1.0",
        "缺少图形界面权限",
    ),
    ("PLATFORM_PRESENT", "render", "0.1.0", "原生表面呈现失败"),
    (
        "PLATFORM_QUEUE_FULL",
        "event",
        "0.1.0",
        "离散事件无法进入有界队列",
    ),
    (
        "PLATFORM_QUOTA_ACCESSIBILITY_NODES",
        "quota",
        "0.8.0",
        "应用无障碍节点配额耗尽",
    ),
    (
        "PLATFORM_QUOTA_ACCESSIBILITY_TEXT_BYTES",
        "quota",
        "0.8.0",
        "应用无障碍文字字节配额耗尽",
    ),
    (
        "PLATFORM_QUOTA_CONFIG",
        "quota",
        "0.8.0",
        "资源配额配置字段或数值无效",
    ),
    (
        "PLATFORM_QUOTA_FONTS",
        "quota",
        "0.8.0",
        "应用字体数量配额耗尽",
    ),
    (
        "PLATFORM_QUOTA_FONT_BYTES",
        "quota",
        "0.8.0",
        "应用字体字节配额耗尽",
    ),
    (
        "PLATFORM_QUOTA_FRAME_BYTES",
        "quota",
        "0.8.0",
        "应用保留帧字节配额耗尽",
    ),
    (
        "PLATFORM_QUOTA_IMAGES",
        "quota",
        "0.8.0",
        "应用图片数量配额耗尽",
    ),
    (
        "PLATFORM_QUOTA_IMAGE_BYTES",
        "quota",
        "0.8.0",
        "应用图片字节配额耗尽",
    ),
    ("PLATFORM_QUOTA_LOCKED", "quota", "0.8.0", "资源配额已冻结"),
    (
        "PLATFORM_QUOTA_RESOURCES",
        "quota",
        "0.8.0",
        "应用资源总数配额耗尽",
    ),
    (
        "PLATFORM_QUOTA_TIMERS",
        "quota",
        "0.8.0",
        "应用计时器数量配额耗尽",
    ),
    (
        "PLATFORM_QUOTA_WINDOWS",
        "quota",
        "0.8.0",
        "应用窗口数量配额耗尽",
    ),
    (
        "PLATFORM_RENDER",
        "render",
        "0.1.0",
        "CPU 绘制解码或栅格化失败",
    ),
    ("PLATFORM_RESOURCE", "resource", "0.1.0", "资源模型操作失败"),
    (
        "PLATFORM_RESOURCE_CLOSED",
        "resource",
        "0.1.0",
        "资源已经关闭或不存在",
    ),
    (
        "PLATFORM_RESOURCE_LIMIT",
        "resource",
        "0.1.0",
        "资源父子关系、编号或计数达到安全边界",
    ),
    (
        "PLATFORM_RESOURCE_TYPE",
        "resource",
        "0.1.0",
        "资源类型与操作要求不匹配",
    ),
    (
        "PLATFORM_SURFACE_CREATE",
        "render",
        "0.1.0",
        "原生窗口表面创建失败",
    ),
    (
        "PLATFORM_TEXT_LIMIT",
        "text",
        "0.1.0",
        "文字、字体或布局结果超过容量上限",
    ),
    ("PLATFORM_TEXT_OPTIONS", "text", "0.1.0", "文字布局选项无效"),
    (
        "PLATFORM_TIMER_RANGE",
        "timer",
        "0.1.0",
        "计时器间隔不在有效范围",
    ),
    (
        "PLATFORM_VALUE_LIMIT",
        "value",
        "0.1.0",
        "ABI 值深度、元素或字节超过上限",
    ),
    (
        "PLATFORM_VALUE_RANGE",
        "value",
        "0.1.0",
        "平台参数数值超出允许范围",
    ),
    (
        "PLATFORM_VALUE_TYPE",
        "value",
        "0.1.0",
        "ABI 值或平台参数类型无效",
    ),
    (
        "PLATFORM_VALUE_UTF8",
        "value",
        "0.1.0",
        "ABI 文字不是有效 UTF-8",
    ),
    ("PLATFORM_WINDOW_COMMAND", "window", "0.1.0", "窗口命令未知"),
    (
        "PLATFORM_WINDOW_CREATE",
        "window",
        "0.1.0",
        "系统窗口创建失败",
    ),
    (
        "PLATFORM_WRONG_THREAD",
        "threading",
        "0.1.0",
        "资源不属于当前事件循环或所有者线程",
    ),
];

fn extract_codes(source: &str, output: &mut BTreeSet<String>) {
    for (offset, _) in source.match_indices("\"PLATFORM_") {
        let start = offset + 1;
        let bytes = source.as_bytes();
        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_uppercase()
                || bytes[end].is_ascii_digit()
                || bytes[end] == b'_')
        {
            end += 1;
        }
        let code = &source[start..end];
        if !code.ends_with('_') {
            output.insert(code.to_owned());
        }
    }
}

fn runtime_error_codes() -> BTreeSet<String> {
    let source_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut paths = std::fs::read_dir(source_directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .filter(|path| {
            !matches!(
                path.file_name().unwrap().to_str().unwrap(),
                "contract_snapshot.rs" | "error_snapshot.rs" | "protocol_snapshot.rs"
            )
        })
        .collect::<Vec<_>>();
    paths.sort();

    let mut codes = BTreeSet::new();
    for path in paths {
        let source = std::fs::read_to_string(path).unwrap();
        let runtime = source
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .unwrap()
            .split("\n#[cfg(test)]\nmod ffi_tests")
            .next()
            .unwrap();
        extract_codes(runtime, &mut codes);
    }
    codes
}

fn error_snapshot() -> String {
    let catalog = ERROR_CODES
        .iter()
        .map(|(code, _, _, _)| *code)
        .collect::<BTreeSet<_>>();
    assert_eq!(catalog.len(), ERROR_CODES.len(), "错误码目录不能重复");
    assert_eq!(
        runtime_error_codes(),
        catalog.iter().map(|code| (*code).to_owned()).collect(),
        "运行时错误码必须与 1.0 目录完全一致"
    );
    assert!(ERROR_CODES.windows(2).all(|pair| pair[0].0 < pair[1].0));

    let mut output = String::new();
    output.push_str("{\n");
    output.push_str("  \"schema\": 1,\n");
    output.push_str("  \"contract\": \"yanxu-platform-error-codes-1.0\",\n");
    output.push_str("  \"compatibility\": {\n");
    output.push_str("    \"codes\": \"stable-in-1.x\",\n");
    output.push_str("    \"messages\": \"diagnostic-only\",\n");
    output.push_str("    \"additions\": \"allowed-in-minor\",\n");
    output.push_str("    \"removal_or_repurpose\": \"major-only\"\n");
    output.push_str("  },\n");
    output.push_str("  \"codes\": [\n");
    for (index, (code, category, since, summary)) in ERROR_CODES.iter().enumerate() {
        writeln!(
            output,
            "    {{ \"code\": \"{code}\", \"category\": \"{category}\", \"since\": \"{since}\", \"summary\": \"{summary}\" }}{}",
            if index + 1 == ERROR_CODES.len() { "" } else { "," }
        )
        .unwrap();
    }
    output.push_str("  ]\n");
    output.push_str("}\n");
    output
}

#[test]
fn error_codes_match_the_1_0_frozen_snapshot() {
    assert_eq!(
        error_snapshot(),
        include_str!("../../api/error-codes-v1.json")
    );
}
