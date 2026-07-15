//! ABI 操作到无句柄平台模型的映射。

use crate::abi::{self, NativeError, NativeHost};
use crate::bridge::{encode_data, free_value};
use crate::data::Data;
use crate::event::{EventKind, PlatformEvent};
use crate::model::{Model, ResourceKind, ResourceState, WindowState};
use crate::protocol;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

const TYPE_APPLICATION: &[u8] = b"yanxu.platform.application";
const TYPE_WINDOW: &[u8] = b"yanxu.platform.window";

#[derive(Clone, Copy)]
pub struct HostApi(pub NativeHost);

impl HostApi {
    fn validate(self) -> Result<(), &'static str> {
        if self.0.abi_version != abi::ABI || self.0.struct_size < std::mem::size_of::<NativeHost>()
        {
            Err("PLATFORM_HOST_ABI")
        } else {
            Ok(())
        }
    }

    fn retain(self, callback: u64) -> Result<(), &'static str> {
        let function = self.0.callback_retain.ok_or("PLATFORM_HOST_MISSING")?;
        (unsafe { function(self.0.context, callback) } == abi::OK)
            .then_some(())
            .ok_or("PLATFORM_CALLBACK_RELEASED")
    }

    fn release(self, callback: u64) {
        if let Some(function) = self.0.callback_release {
            let _ = unsafe { function(self.0.context, callback) };
        }
    }

    fn permission(self, name: &str) -> bool {
        self.0.has_permission.is_some_and(|function| unsafe {
            function(self.0.context, name.as_ptr(), name.len()) != 0
        })
    }

    fn post(self, callback: u64, event: Data) -> Result<(), &'static str> {
        let function = self.0.callback_post.ok_or("PLATFORM_HOST_MISSING")?;
        let mut value = encode_data(event);
        let mut error = NativeError::default();
        let result = unsafe { function(self.0.context, callback, &value, 1, &mut error) };
        unsafe { free_value(&mut value) };
        (result == abi::OK)
            .then_some(())
            .ok_or("PLATFORM_CALLBACK_POST")
    }

    fn pump(self) -> Result<(), &'static str> {
        let Some(function) = self.0.pump else {
            return Ok(());
        };
        let mut error = NativeError::default();
        (unsafe { function(self.0.context, 4_096, &mut error) } == abi::OK)
            .then_some(())
            .ok_or("PLATFORM_CALLBACK_PUMP")
    }

    fn raw_resource(self, handle: u64) -> Result<*mut c_void, &'static str> {
        let function = self.0.resource_get.ok_or("PLATFORM_HOST_MISSING")?;
        let mut raw = std::ptr::null_mut();
        if unsafe { function(self.0.context, handle, &mut raw) } != abi::OK || raw.is_null() {
            return Err("PLATFORM_RESOURCE_CLOSED");
        }
        Ok(raw)
    }
}

pub struct PlatformResource {
    pub model: Arc<Mutex<Model>>,
    pub kind: ResourceKind,
    pub id: u64,
    pub host: HostApi,
    callbacks: Vec<u64>,
    cleaned: AtomicBool,
}

impl PlatformResource {
    fn cleanup(&self) {
        if self.cleaned.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = self
            .model
            .lock()
            .expect("platform model poisoned")
            .close(self.id);
        for callback in &self.callbacks {
            self.host.release(*callback);
        }
    }

    fn callback(&self) -> Result<u64, &'static str> {
        self.callbacks
            .first()
            .copied()
            .ok_or("PLATFORM_CALLBACK_RELEASED")
    }
}

impl Drop for PlatformResource {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// 释放由言台返回的原生资源包装。
///
/// # Safety
///
/// `resource` 必须为空，或来自 `Box::into_raw(Box<PlatformResource>)` 且尚未释放。
pub unsafe extern "C" fn drop_platform_resource(resource: *mut c_void) {
    if !resource.is_null() {
        drop(unsafe { Box::from_raw(resource.cast::<PlatformResource>()) });
    }
}

pub struct ResourceOutput {
    pub resource: Box<PlatformResource>,
    pub type_name: &'static [u8],
    pub parent: u64,
}

pub enum Output {
    Value(Data),
    Resource(ResourceOutput),
}

#[derive(Debug, Clone, Copy)]
#[repr(usize)]
pub enum Operation {
    ProtocolInfo = 1,
    Capabilities,
    ApplicationCreate,
    WindowCreate,
    WindowCommand,
    WindowQuery,
    FlushEvents,
    SubmitFrame,
    InspectDraw,
    MonotonicTime,
    Close,
    DebugSnapshot,
}

impl Operation {
    #[must_use]
    pub fn from_context(context: *mut c_void) -> Option<Self> {
        Some(match context as usize {
            1 => Self::ProtocolInfo,
            2 => Self::Capabilities,
            3 => Self::ApplicationCreate,
            4 => Self::WindowCreate,
            5 => Self::WindowCommand,
            6 => Self::WindowQuery,
            7 => Self::FlushEvents,
            8 => Self::SubmitFrame,
            9 => Self::InspectDraw,
            10 => Self::MonotonicTime,
            11 => Self::Close,
            12 => Self::DebugSnapshot,
            _ => return None,
        })
    }
}

#[allow(clippy::too_many_lines)]
/// 执行一个已经由 ABI 描述符选择的平台操作。
///
/// # Safety
///
/// `arguments` 中的资源句柄必须属于 `host`；`host.resource_get` 返回的原始指针必须
/// 来自本模块的 `PlatformResource`，且在本次调用结束前保持有效。
pub unsafe fn call(
    operation: Operation,
    arguments: &[Data],
    host: HostApi,
) -> Result<Output, &'static str> {
    host.validate()?;
    match operation {
        Operation::ProtocolInfo => {
            require_count(arguments, 0)?;
            Ok(Output::Value(protocol_info()))
        }
        Operation::Capabilities => {
            require_count(arguments, 0)?;
            Ok(Output::Value(capabilities()))
        }
        Operation::ApplicationCreate => {
            require_count(arguments, 2)?;
            if !host.permission("图形界面") {
                return Err("PLATFORM_PERMISSION_GUI");
            }
            let name = text(&arguments[0])?.to_owned();
            let callback = callback(&arguments[1])?;
            host.retain(callback)?;
            let model = Arc::new(Mutex::new(Model::default()));
            let id = match model.lock().expect("platform model poisoned").create(
                None,
                ResourceState::Application {
                    name,
                    exit_requested: false,
                },
            ) {
                Ok(id) => id,
                Err(_) => {
                    host.release(callback);
                    return Err("PLATFORM_RESOURCE_LIMIT");
                }
            };
            model
                .lock()
                .expect("platform model poisoned")
                .events
                .push(PlatformEvent::new(
                    EventKind::ApplicationStarted,
                    None,
                    monotonic_seconds(),
                ))
                .map_err(|_| "PLATFORM_QUEUE_FULL")?;
            Ok(resource_output(
                model,
                ResourceKind::Application,
                id,
                host,
                vec![callback],
                TYPE_APPLICATION,
                0,
            ))
        }
        Operation::WindowCreate => {
            require_count(arguments, 2)?;
            let (parent_handle, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let config = map(&arguments[1])?;
            let mut window = WindowState::default();
            apply_window_config(&mut window, config)?;
            let mut model = application.model.lock().expect("platform model poisoned");
            let id = model
                .create(Some(application.id), ResourceState::Window(window.clone()))
                .map_err(|_| "PLATFORM_RESOURCE_LIMIT")?;
            if window.visible {
                model
                    .events
                    .push(PlatformEvent::new(
                        EventKind::WindowShown,
                        Some(id),
                        monotonic_seconds(),
                    ))
                    .map_err(|_| "PLATFORM_QUEUE_FULL")?;
            }
            model
                .events
                .push(PlatformEvent::new(
                    EventKind::RedrawRequested,
                    Some(id),
                    monotonic_seconds(),
                ))
                .map_err(|_| "PLATFORM_QUEUE_FULL")?;
            drop(model);
            Ok(resource_output(
                application.model.clone(),
                ResourceKind::Window,
                id,
                host,
                Vec::new(),
                TYPE_WINDOW,
                parent_handle,
            ))
        }
        Operation::WindowCommand => {
            require_count(arguments, 3)?;
            let (_, resource) = unsafe { resource(arguments, 0, host, ResourceKind::Window) }?;
            let name = text(&arguments[1])?;
            window_command(resource, name, &arguments[2])?;
            Ok(Output::Value(Data::Nil))
        }
        Operation::WindowQuery => {
            require_count(arguments, 1)?;
            let (_, resource) = unsafe { resource(arguments, 0, host, ResourceKind::Window) }?;
            Ok(Output::Value(window_snapshot(resource)?))
        }
        Operation::FlushEvents => {
            require_count(arguments, 1)?;
            let (_, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let batch = application
                .model
                .lock()
                .expect("platform model poisoned")
                .events
                .take_data();
            if let Some(batch) = batch {
                host.post(application.callback()?, batch)?;
                host.pump()?;
                Ok(Output::Value(Data::Bool(true)))
            } else {
                Ok(Output::Value(Data::Bool(false)))
            }
        }
        Operation::SubmitFrame => {
            require_count(arguments, 2)?;
            let (_, resource) = unsafe { resource(arguments, 0, host, ResourceKind::Window) }?;
            let bytes = bytes(&arguments[1])?;
            let frame = protocol::decode(bytes).map_err(draw_error_code)?;
            let count = i64::try_from(frame.commands.len()).map_err(|_| "PLATFORM_DRAW_LIMIT")?;
            let mut model = resource.model.lock().expect("platform model poisoned");
            let node = model
                .get_mut(resource.id)
                .map_err(|_| "PLATFORM_RESOURCE_CLOSED")?;
            let ResourceState::Window(window) = &mut node.state else {
                return Err("PLATFORM_RESOURCE_TYPE");
            };
            window.frame = bytes.to_vec();
            window.redraw_requested = true;
            model
                .events
                .push(PlatformEvent::new(
                    EventKind::RedrawRequested,
                    Some(resource.id),
                    monotonic_seconds(),
                ))
                .map_err(|_| "PLATFORM_QUEUE_FULL")?;
            Ok(Output::Value(Data::Integer(count)))
        }
        Operation::InspectDraw => {
            require_count(arguments, 1)?;
            let frame = protocol::decode(bytes(&arguments[0])?).map_err(draw_error_code)?;
            Ok(Output::Value(Data::map([
                ("协议主", Data::Integer(i64::from(protocol::DRAW_MAJOR))),
                ("协议次", Data::Integer(i64::from(frame.minor))),
                ("标志", Data::Integer(i64::from(frame.flags))),
                (
                    "操作码",
                    Data::Array(
                        frame
                            .commands
                            .into_iter()
                            .map(|command| Data::Integer(i64::from(command.opcode)))
                            .collect(),
                    ),
                ),
            ])))
        }
        Operation::MonotonicTime => {
            require_count(arguments, 0)?;
            Ok(Output::Value(Data::Number(monotonic_seconds())))
        }
        Operation::Close => {
            require_count(arguments, 1)?;
            let (_, resource) = unsafe { resource_any(arguments, 0, host) }?;
            resource.cleanup();
            Ok(Output::Value(Data::Nil))
        }
        Operation::DebugSnapshot => {
            require_count(arguments, 1)?;
            let (_, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let model = application.model.lock().expect("platform model poisoned");
            Ok(Output::Value(Data::map([
                (
                    "应用",
                    Data::Integer(model.count(ResourceKind::Application) as i64),
                ),
                (
                    "窗口",
                    Data::Integer(model.count(ResourceKind::Window) as i64),
                ),
                (
                    "计时器",
                    Data::Integer(model.count(ResourceKind::Timer) as i64),
                ),
                (
                    "图片",
                    Data::Integer(model.count(ResourceKind::Image) as i64),
                ),
                (
                    "字体",
                    Data::Integer(model.count(ResourceKind::Font) as i64),
                ),
                ("待处理事件", Data::Integer(model.events.len() as i64)),
                ("运行中", Data::Bool(model.running)),
            ])))
        }
    }
}

fn protocol_info() -> Data {
    Data::map([
        ("平台主", Data::Integer(1)),
        ("平台次", Data::Integer(0)),
        ("事件主", Data::Integer(1)),
        ("事件次", Data::Integer(0)),
        ("绘制主", Data::Integer(i64::from(protocol::DRAW_MAJOR))),
        ("绘制次", Data::Integer(i64::from(protocol::DRAW_MINOR))),
        ("ABI", Data::Integer(2)),
    ])
}

fn capabilities() -> Data {
    Data::map([
        ("系统", Data::String(std::env::consts::OS.to_owned())),
        ("架构", Data::String(std::env::consts::ARCH.to_owned())),
        ("原生窗口", Data::Bool(true)),
        ("多窗口", Data::Bool(true)),
        ("高DPI", Data::Bool(true)),
        ("触摸", Data::Bool(true)),
        ("触控笔", Data::Bool(true)),
        ("IME", Data::Bool(true)),
        ("剪贴板", Data::Bool(true)),
        ("文件对话框", Data::Bool(true)),
        ("文件拖放", Data::Bool(true)),
        ("CPU二维绘制", Data::Bool(true)),
        ("复杂文字", Data::Bool(true)),
        ("Wayland", Data::Bool(cfg!(target_os = "linux"))),
        ("X11", Data::Bool(cfg!(target_os = "linux"))),
    ])
}

fn apply_window_config(
    window: &mut WindowState,
    config: &BTreeMap<String, Data>,
) -> Result<(), &'static str> {
    if let Some(value) = config.get("标题") {
        window.title = text(value)?.to_owned();
    }
    if let Some(value) = config.get("宽") {
        window.width = positive_number(value)?;
    }
    if let Some(value) = config.get("高") {
        window.height = positive_number(value)?;
    }
    if let Some(value) = config.get("可见") {
        window.visible = boolean(value)?;
    }
    if let Some(value) = config.get("透明") {
        window.transparent = boolean(value)?;
    }
    if let Some(value) = config.get("无边框") {
        window.borderless = boolean(value)?;
    }
    if let Some(value) = config.get("置顶") {
        window.always_on_top = boolean(value)?;
    }
    if let Some(value) = config.get("最小尺寸") {
        window.minimum = Some(size(value)?);
    }
    if let Some(value) = config.get("最大尺寸") {
        window.maximum = Some(size(value)?);
    }
    if let Some(value) = config.get("位置") {
        window.position = Some(pair(value)?);
    }
    Ok(())
}

fn window_command(
    resource: &PlatformResource,
    name: &str,
    value: &Data,
) -> Result<(), &'static str> {
    let mut model = resource.model.lock().expect("platform model poisoned");
    let node = model
        .get_mut(resource.id)
        .map_err(|_| "PLATFORM_RESOURCE_CLOSED")?;
    let ResourceState::Window(window) = &mut node.state else {
        return Err("PLATFORM_RESOURCE_TYPE");
    };
    let mut event = None;
    match name {
        "标题" => window.title = text(value)?.to_owned(),
        "大小" => {
            let [width, height] = size(value)?;
            window.width = width;
            window.height = height;
            event = Some(
                PlatformEvent::new(
                    EventKind::WindowResized,
                    Some(resource.id),
                    monotonic_seconds(),
                )
                .with("宽", width)
                .with("高", height),
            );
        }
        "最小尺寸" => window.minimum = Some(size(value)?),
        "最大尺寸" => window.maximum = Some(size(value)?),
        "位置" => {
            let [x, y] = pair(value)?;
            window.position = Some([x, y]);
            event = Some(
                PlatformEvent::new(
                    EventKind::WindowMoved,
                    Some(resource.id),
                    monotonic_seconds(),
                )
                .with("横坐标", x)
                .with("纵坐标", y),
            );
        }
        "显示" => {
            window.visible = boolean(value)?;
            event = Some(PlatformEvent::new(
                if window.visible {
                    EventKind::WindowShown
                } else {
                    EventKind::WindowHidden
                },
                Some(resource.id),
                monotonic_seconds(),
            ));
        }
        "最大化" => window.maximized = boolean(value)?,
        "最小化" => window.minimized = boolean(value)?,
        "全屏" => window.fullscreen = boolean(value)?,
        "无边框" => window.borderless = boolean(value)?,
        "透明" => window.transparent = boolean(value)?,
        "置顶" => window.always_on_top = boolean(value)?,
        "请求重绘" => {
            window.redraw_requested = true;
            event = Some(PlatformEvent::new(
                EventKind::RedrawRequested,
                Some(resource.id),
                monotonic_seconds(),
            ));
        }
        _ => return Err("PLATFORM_WINDOW_COMMAND"),
    }
    if let Some(event) = event {
        model
            .events
            .push(event)
            .map_err(|_| "PLATFORM_QUEUE_FULL")?;
    }
    Ok(())
}

fn window_snapshot(resource: &PlatformResource) -> Result<Data, &'static str> {
    let model = resource.model.lock().expect("platform model poisoned");
    let node = model
        .get(resource.id)
        .map_err(|_| "PLATFORM_RESOURCE_CLOSED")?;
    let ResourceState::Window(window) = &node.state else {
        return Err("PLATFORM_RESOURCE_TYPE");
    };
    Ok(Data::map([
        ("编号", Data::Integer(resource.id as i64)),
        ("标题", Data::String(window.title.clone())),
        ("宽", Data::Number(window.width)),
        ("高", Data::Number(window.height)),
        ("可见", Data::Bool(window.visible)),
        ("最大化", Data::Bool(window.maximized)),
        ("最小化", Data::Bool(window.minimized)),
        ("全屏", Data::Bool(window.fullscreen)),
        ("无边框", Data::Bool(window.borderless)),
        ("透明", Data::Bool(window.transparent)),
        ("置顶", Data::Bool(window.always_on_top)),
        ("比例因子", Data::Number(window.scale_factor)),
        ("已有帧", Data::Bool(!window.frame.is_empty())),
    ]))
}

fn resource_output(
    model: Arc<Mutex<Model>>,
    kind: ResourceKind,
    id: u64,
    host: HostApi,
    callbacks: Vec<u64>,
    type_name: &'static [u8],
    parent: u64,
) -> Output {
    Output::Resource(ResourceOutput {
        resource: Box::new(PlatformResource {
            model,
            kind,
            id,
            host,
            callbacks,
            cleaned: AtomicBool::new(false),
        }),
        type_name,
        parent,
    })
}

unsafe fn resource<'a>(
    arguments: &[Data],
    index: usize,
    host: HostApi,
    kind: ResourceKind,
) -> Result<(u64, &'a PlatformResource), &'static str> {
    let (handle, resource) = unsafe { resource_any(arguments, index, host) }?;
    if resource.kind != kind {
        return Err("PLATFORM_RESOURCE_TYPE");
    }
    Ok((handle, resource))
}

unsafe fn resource_any<'a>(
    arguments: &[Data],
    index: usize,
    host: HostApi,
) -> Result<(u64, &'a PlatformResource), &'static str> {
    let Data::Resource(handle) = arguments.get(index).ok_or("PLATFORM_ARGUMENT_COUNT")? else {
        return Err("PLATFORM_VALUE_TYPE");
    };
    let resource = unsafe { &*host.raw_resource(*handle)?.cast::<PlatformResource>() };
    if resource.host.0.event_loop_id != host.0.event_loop_id
        || resource.host.0.owner_thread_token != host.0.owner_thread_token
    {
        return Err("PLATFORM_WRONG_THREAD");
    }
    if resource.cleaned.load(Ordering::Acquire) {
        return Err("PLATFORM_RESOURCE_CLOSED");
    }
    Ok((*handle, resource))
}

fn require_count(arguments: &[Data], expected: usize) -> Result<(), &'static str> {
    if arguments.len() == expected {
        Ok(())
    } else {
        Err("PLATFORM_ARGUMENT_COUNT")
    }
}

fn text(value: &Data) -> Result<&str, &'static str> {
    if let Data::String(value) = value {
        Ok(value)
    } else {
        Err("PLATFORM_VALUE_TYPE")
    }
}

fn callback(value: &Data) -> Result<u64, &'static str> {
    if let Data::Callback(value) = value {
        Ok(*value)
    } else {
        Err("PLATFORM_VALUE_TYPE")
    }
}

fn bytes(value: &Data) -> Result<&[u8], &'static str> {
    if let Data::Bytes(value) = value {
        Ok(value)
    } else {
        Err("PLATFORM_VALUE_TYPE")
    }
}

fn map(value: &Data) -> Result<&BTreeMap<String, Data>, &'static str> {
    if let Data::Map(value) = value {
        Ok(value)
    } else {
        Err("PLATFORM_VALUE_TYPE")
    }
}

fn boolean(value: &Data) -> Result<bool, &'static str> {
    if let Data::Bool(value) = value {
        Ok(*value)
    } else {
        Err("PLATFORM_VALUE_TYPE")
    }
}

fn number(value: &Data) -> Result<f64, &'static str> {
    value
        .as_number()
        .filter(|number| number.is_finite())
        .ok_or("PLATFORM_VALUE_TYPE")
}

fn positive_number(value: &Data) -> Result<f64, &'static str> {
    let number = number(value)?;
    if number > 0.0 && number <= 1_000_000.0 {
        Ok(number)
    } else {
        Err("PLATFORM_VALUE_RANGE")
    }
}

fn pair(value: &Data) -> Result<[f64; 2], &'static str> {
    let Data::Array(values) = value else {
        return Err("PLATFORM_VALUE_TYPE");
    };
    if values.len() != 2 {
        return Err("PLATFORM_VALUE_TYPE");
    }
    Ok([number(&values[0])?, number(&values[1])?])
}

fn size(value: &Data) -> Result<[f64; 2], &'static str> {
    let [width, height] = pair(value)?;
    if width > 0.0 && height > 0.0 && width <= 1_000_000.0 && height <= 1_000_000.0 {
        Ok([width, height])
    } else {
        Err("PLATFORM_VALUE_RANGE")
    }
}

fn draw_error_code(error: protocol::ProtocolError) -> &'static str {
    match error {
        protocol::ProtocolError::Major { .. } => "PLATFORM_DRAW_MAJOR",
        protocol::ProtocolError::Limit(_) => "PLATFORM_DRAW_LIMIT",
        protocol::ProtocolError::Utf8 => "PLATFORM_DRAW_UTF8",
        protocol::ProtocolError::NonFinite => "PLATFORM_DRAW_NUMBER",
        _ => "PLATFORM_DRAW_CORRUPT",
    }
}

fn monotonic_seconds() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    static RETAINS: AtomicUsize = AtomicUsize::new(0);
    static RELEASES: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn retain(_: *mut c_void, _: u64) -> i32 {
        RETAINS.fetch_add(1, AtomicOrdering::SeqCst);
        abi::OK
    }

    unsafe extern "C" fn release(_: *mut c_void, _: u64) -> i32 {
        RELEASES.fetch_add(1, AtomicOrdering::SeqCst);
        abi::OK
    }

    unsafe extern "C" fn permission(_: *mut c_void, _: *const u8, _: usize) -> i32 {
        1
    }

    fn host() -> HostApi {
        HostApi(NativeHost {
            abi_version: 2,
            struct_size: std::mem::size_of::<NativeHost>(),
            context: std::ptr::null_mut(),
            callback_retain: Some(retain),
            callback_release: Some(release),
            callback_post: None,
            wake: None,
            pump: None,
            has_permission: Some(permission),
            resource_get: None,
            event_loop_id: 1,
            owner_thread_token: 2,
        })
    }

    #[test]
    fn creates_application_and_balances_callback_lifetime() {
        RETAINS.store(0, AtomicOrdering::SeqCst);
        RELEASES.store(0, AtomicOrdering::SeqCst);
        let output = unsafe {
            call(
                Operation::ApplicationCreate,
                &[Data::String("测试".to_owned()), Data::Callback(9)],
                host(),
            )
        }
        .unwrap();
        let Output::Resource(resource) = output else {
            panic!("application must return a resource")
        };
        assert_eq!(resource.resource.kind, ResourceKind::Application);
        assert_eq!(resource.resource.model.lock().unwrap().events.len(), 1);
        assert_eq!(RETAINS.load(AtomicOrdering::SeqCst), 1);
        drop(resource);
        assert_eq!(RELEASES.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn validates_window_configuration_ranges() {
        let mut window = WindowState::default();
        let config = BTreeMap::from([
            ("标题".to_owned(), Data::String("主窗口".to_owned())),
            ("宽".to_owned(), Data::Integer(1024)),
            ("高".to_owned(), Data::Number(768.0)),
            (
                "最小尺寸".to_owned(),
                Data::Array(vec![Data::Integer(320), Data::Integer(240)]),
            ),
        ]);
        apply_window_config(&mut window, &config).unwrap();
        assert_eq!(window.title, "主窗口");
        assert_eq!([window.width, window.height], [1024.0, 768.0]);
        assert_eq!(window.minimum, Some([320.0, 240.0]));

        let invalid = BTreeMap::from([("宽".to_owned(), Data::Integer(0))]);
        assert_eq!(
            apply_window_config(&mut window, &invalid),
            Err("PLATFORM_VALUE_RANGE")
        );
    }

    #[test]
    fn protocol_and_capability_maps_are_versioned() {
        assert_eq!(protocol_info().as_map().unwrap()["ABI"], Data::Integer(2));
        assert_eq!(
            capabilities().as_map().unwrap()["原生窗口"],
            Data::Bool(true)
        );
    }
}
