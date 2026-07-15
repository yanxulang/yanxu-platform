//! ABI 操作到无句柄平台模型的映射。

use crate::abi::{self, NativeError, NativeHost};
use crate::bridge::{encode_data, free_value};
use crate::data::Data;
use crate::event::{EVENT_MAJOR, EVENT_MINOR, EventKind, PlatformEvent};
use crate::model::{Model, ResourceKind, ResourceState, TimerState, WindowState};
use crate::protocol;
use crate::text::{TextError, TextLayout, TextOptions, TextService};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const TYPE_APPLICATION: &[u8] = b"yanxu.platform.application";
const TYPE_WINDOW: &[u8] = b"yanxu.platform.window";
const TYPE_FONT: &[u8] = b"yanxu.platform.font";
const TYPE_TIMER: &[u8] = b"yanxu.platform.timer";
const TYPE_IMAGE: &[u8] = b"yanxu.platform.image";

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

    pub(crate) fn post(self, callback: u64, event: Data) -> Result<(), &'static str> {
        let function = self.0.callback_post.ok_or("PLATFORM_HOST_MISSING")?;
        let mut value = encode_data(event);
        let mut error = NativeError::default();
        let result = unsafe { function(self.0.context, callback, &value, 1, &mut error) };
        unsafe { free_value(&mut value) };
        (result == abi::OK)
            .then_some(())
            .ok_or("PLATFORM_CALLBACK_POST")
    }

    pub(crate) fn pump(self) -> Result<(), &'static str> {
        let Some(function) = self.0.pump else {
            return Ok(());
        };
        let mut error = NativeError::default();
        (unsafe { function(self.0.context, 4_096, &mut error) } == abi::OK)
            .then_some(())
            .ok_or("PLATFORM_CALLBACK_PUMP")
    }

    pub(crate) fn wake(self) {
        if let Some(function) = self.0.wake {
            unsafe { function(self.0.context) };
        }
    }

    pub(crate) fn raw_resource(self, handle: u64) -> Result<*mut c_void, &'static str> {
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
    pub text: Arc<Mutex<TextService>>,
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

    pub(crate) fn callback(&self) -> Result<u64, &'static str> {
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
    FontFamilies,
    FontMatch,
    FontLoad,
    TextShape,
    TextMeasure,
    TextHitTest,
    TimerCreate,
    TimerCancel,
    ClipboardRead,
    ClipboardWrite,
    FileDialog,
    ImageLoad,
    ImageInfo,
    ImeConfigure,
    CursorSet,
    Displays,
    Theme,
    ApplicationRun,
    ApplicationExit,
    Wake,
    TimerQuery,
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
            13 => Self::FontFamilies,
            14 => Self::FontMatch,
            15 => Self::FontLoad,
            16 => Self::TextShape,
            17 => Self::TextMeasure,
            18 => Self::TextHitTest,
            19 => Self::TimerCreate,
            20 => Self::TimerCancel,
            21 => Self::ClipboardRead,
            22 => Self::ClipboardWrite,
            23 => Self::FileDialog,
            24 => Self::ImageLoad,
            25 => Self::ImageInfo,
            26 => Self::ImeConfigure,
            27 => Self::CursorSet,
            28 => Self::Displays,
            29 => Self::Theme,
            30 => Self::ApplicationRun,
            31 => Self::ApplicationExit,
            32 => Self::Wake,
            33 => Self::TimerQuery,
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
            let text_service = Arc::new(Mutex::new(TextService::new()));
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
                text_service,
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
                .create(
                    Some(application.id),
                    ResourceState::Window(Box::new(window.clone())),
                )
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
                application.text.clone(),
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
        Operation::FontFamilies => {
            require_count(arguments, 1)?;
            let (_, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let families = application
                .text
                .lock()
                .expect("platform text service poisoned")
                .families()
                .into_iter()
                .map(Data::String)
                .collect();
            Ok(Output::Value(Data::Array(families)))
        }
        Operation::FontMatch => {
            require_count(arguments, 2)?;
            let (_, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let config = map(&arguments[1])?;
            let family = config.get("字族").map(text).transpose()?.unwrap_or("");
            let weight = config
                .get("字重")
                .map(integer)
                .transpose()?
                .unwrap_or(400)
                .clamp(1, 1_000) as u16;
            let italic = config
                .get("斜体")
                .map(boolean)
                .transpose()?
                .unwrap_or(false);
            let matched = application
                .text
                .lock()
                .expect("platform text service poisoned")
                .match_font(family, weight, italic);
            Ok(Output::Value(matched.map_or(Data::Nil, |font| {
                Data::map([
                    ("字族", Data::String(font.family)),
                    ("PostScript名称", Data::String(font.postscript_name)),
                    ("字重", Data::Integer(i64::from(font.weight))),
                    ("斜体", Data::Bool(font.italic)),
                    ("等宽", Data::Bool(font.monospaced)),
                ])
            })))
        }
        Operation::FontLoad => {
            require_count(arguments, 2)?;
            let (parent_handle, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let font_bytes = bytes(&arguments[1])?.to_vec();
            let families = application
                .text
                .lock()
                .expect("platform text service poisoned")
                .load_font(font_bytes.clone())
                .map_err(text_error_code)?;
            let family = families.first().cloned().ok_or("PLATFORM_FONT_INVALID")?;
            let id = application
                .model
                .lock()
                .expect("platform model poisoned")
                .create(
                    Some(application.id),
                    ResourceState::Font {
                        family,
                        bytes: Some(font_bytes),
                    },
                )
                .map_err(|_| "PLATFORM_RESOURCE_LIMIT")?;
            Ok(resource_output(
                application.model.clone(),
                application.text.clone(),
                ResourceKind::Font,
                id,
                host,
                Vec::new(),
                TYPE_FONT,
                parent_handle,
            ))
        }
        Operation::TextShape => {
            require_count(arguments, 3)?;
            let (_, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let content = text(&arguments[1])?;
            let options = text_options(map(&arguments[2])?)?;
            let layout = application
                .text
                .lock()
                .expect("platform text service poisoned")
                .shape(content, &options)
                .map_err(text_error_code)?;
            Ok(Output::Value(layout_data(&layout)))
        }
        Operation::TextMeasure => {
            require_count(arguments, 3)?;
            let (_, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let content = text(&arguments[1])?;
            let options = text_options(map(&arguments[2])?)?;
            let layout = application
                .text
                .lock()
                .expect("platform text service poisoned")
                .shape(content, &options)
                .map_err(text_error_code)?;
            Ok(Output::Value(Data::map([
                ("宽", Data::Number(f64::from(layout.width))),
                ("高", Data::Number(f64::from(layout.height))),
                ("基线", Data::Number(f64::from(layout.baseline))),
                ("行高", Data::Number(f64::from(options.line_height))),
                ("行数", Data::Integer(layout.lines.len() as i64)),
            ])))
        }
        Operation::TextHitTest => {
            require_count(arguments, 5)?;
            let (_, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let content = text(&arguments[1])?;
            let options = text_options(map(&arguments[2])?)?;
            let x = number(&arguments[3])? as f32;
            let y = number(&arguments[4])? as f32;
            let layout = application
                .text
                .lock()
                .expect("platform text service poisoned")
                .shape(content, &options)
                .map_err(text_error_code)?;
            Ok(Output::Value(Data::Integer(
                layout.hit_test(x, y, content.len()) as i64,
            )))
        }
        Operation::TimerCreate => {
            require_count(arguments, 3)?;
            let (parent_handle, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let millis = number(&arguments[1])?;
            if !(10.0..=86_400_000.0).contains(&millis) {
                return Err("PLATFORM_TIMER_RANGE");
            }
            let repeating = boolean(&arguments[2])?;
            let interval = Duration::from_secs_f64(millis / 1_000.0);
            let id = application
                .model
                .lock()
                .expect("platform model poisoned")
                .create(
                    Some(application.id),
                    ResourceState::Timer(TimerState {
                        interval,
                        repeating,
                        next_deadline: Instant::now() + interval,
                        cancelled: false,
                    }),
                )
                .map_err(|_| "PLATFORM_RESOURCE_LIMIT")?;
            Ok(resource_output(
                application.model.clone(),
                application.text.clone(),
                ResourceKind::Timer,
                id,
                host,
                Vec::new(),
                TYPE_TIMER,
                parent_handle,
            ))
        }
        Operation::TimerCancel => {
            require_count(arguments, 1)?;
            let (_, timer) = unsafe { resource(arguments, 0, host, ResourceKind::Timer) }?;
            let mut model = timer.model.lock().expect("platform model poisoned");
            let node = model
                .get_mut(timer.id)
                .map_err(|_| "PLATFORM_RESOURCE_CLOSED")?;
            let ResourceState::Timer(timer) = &mut node.state else {
                return Err("PLATFORM_RESOURCE_TYPE");
            };
            timer.cancelled = true;
            Ok(Output::Value(Data::Nil))
        }
        Operation::ClipboardRead => {
            require_count(arguments, 0)?;
            if !host.permission("剪贴板") {
                return Err("PLATFORM_PERMISSION_CLIPBOARD");
            }
            let mut clipboard = arboard::Clipboard::new().map_err(|_| "PLATFORM_CLIPBOARD")?;
            match clipboard.get_text() {
                Ok(value) => Ok(Output::Value(Data::String(value))),
                Err(arboard::Error::ContentNotAvailable) => Ok(Output::Value(Data::Nil)),
                Err(_) => Err("PLATFORM_CLIPBOARD"),
            }
        }
        Operation::ClipboardWrite => {
            require_count(arguments, 1)?;
            if !host.permission("剪贴板") {
                return Err("PLATFORM_PERMISSION_CLIPBOARD");
            }
            let value = text(&arguments[0])?.to_owned();
            arboard::Clipboard::new()
                .and_then(|mut clipboard| clipboard.set_text(value))
                .map_err(|_| "PLATFORM_CLIPBOARD")?;
            Ok(Output::Value(Data::Nil))
        }
        Operation::FileDialog => {
            require_count(arguments, 2)?;
            if !host.permission("文件对话框") {
                return Err("PLATFORM_PERMISSION_DIALOG");
            }
            let kind = text(&arguments[0])?;
            let config = map(&arguments[1])?;
            Ok(Output::Value(file_dialog(kind, config)?))
        }
        Operation::ImageLoad => {
            require_count(arguments, 2)?;
            let (parent_handle, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let (width, height, rgba) = decode_image(bytes(&arguments[1])?)?;
            let id = application
                .model
                .lock()
                .expect("platform model poisoned")
                .create(
                    Some(application.id),
                    ResourceState::Image {
                        width,
                        height,
                        rgba,
                    },
                )
                .map_err(|_| "PLATFORM_RESOURCE_LIMIT")?;
            Ok(resource_output(
                application.model.clone(),
                application.text.clone(),
                ResourceKind::Image,
                id,
                host,
                Vec::new(),
                TYPE_IMAGE,
                parent_handle,
            ))
        }
        Operation::ImageInfo => {
            require_count(arguments, 1)?;
            let (_, image) = unsafe { resource(arguments, 0, host, ResourceKind::Image) }?;
            let model = image.model.lock().expect("platform model poisoned");
            let node = model
                .get(image.id)
                .map_err(|_| "PLATFORM_RESOURCE_CLOSED")?;
            let ResourceState::Image {
                width,
                height,
                rgba,
            } = &node.state
            else {
                return Err("PLATFORM_RESOURCE_TYPE");
            };
            Ok(Output::Value(Data::map([
                ("宽", Data::Integer(i64::from(*width))),
                ("高", Data::Integer(i64::from(*height))),
                ("字节数", Data::Integer(rgba.len() as i64)),
            ])))
        }
        Operation::ImeConfigure => {
            require_count(arguments, 2)?;
            let (_, window_resource) =
                unsafe { resource(arguments, 0, host, ResourceKind::Window) }?;
            let config = map(&arguments[1])?;
            let mut model = window_resource
                .model
                .lock()
                .expect("platform model poisoned");
            let node = model
                .get_mut(window_resource.id)
                .map_err(|_| "PLATFORM_RESOURCE_CLOSED")?;
            let ResourceState::Window(window) = &mut node.state else {
                return Err("PLATFORM_RESOURCE_TYPE");
            };
            if let Some(value) = config.get("启用") {
                window.ime_allowed = boolean(value)?;
            }
            if let Some(value) = config.get("光标区域") {
                window.ime_cursor_area = Some(rectangle(value)?);
            }
            if let Some(value) = config.get("用途") {
                window.ime_purpose = text(value)?.to_owned();
            }
            Ok(Output::Value(Data::Nil))
        }
        Operation::CursorSet => {
            require_count(arguments, 3)?;
            let (_, window_resource) =
                unsafe { resource(arguments, 0, host, ResourceKind::Window) }?;
            let name = text(&arguments[1])?.to_owned();
            let visible = boolean(&arguments[2])?;
            let mut model = window_resource
                .model
                .lock()
                .expect("platform model poisoned");
            let node = model
                .get_mut(window_resource.id)
                .map_err(|_| "PLATFORM_RESOURCE_CLOSED")?;
            let ResourceState::Window(window) = &mut node.state else {
                return Err("PLATFORM_RESOURCE_TYPE");
            };
            window.cursor = name;
            window.cursor_visible = visible;
            Ok(Output::Value(Data::Nil))
        }
        Operation::Displays => {
            require_count(arguments, 1)?;
            let (_, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let model = application.model.lock().expect("platform model poisoned");
            Ok(Output::Value(Data::Array(
                model
                    .displays
                    .iter()
                    .map(|display| {
                        Data::map([
                            ("名称", display.name.clone().map_or(Data::Nil, Data::String)),
                            (
                                "位置",
                                Data::Array(
                                    display
                                        .position
                                        .into_iter()
                                        .map(|value| Data::Integer(i64::from(value)))
                                        .collect(),
                                ),
                            ),
                            (
                                "尺寸",
                                Data::Array(
                                    display
                                        .size
                                        .into_iter()
                                        .map(|value| Data::Integer(i64::from(value)))
                                        .collect(),
                                ),
                            ),
                            ("比例因子", Data::Number(display.scale_factor)),
                            ("主显示器", Data::Bool(display.primary)),
                        ])
                    })
                    .collect(),
            )))
        }
        Operation::Theme => {
            require_count(arguments, 1)?;
            let (_, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let theme = application
                .model
                .lock()
                .expect("platform model poisoned")
                .system_theme
                .clone();
            Ok(Output::Value(Data::String(theme)))
        }
        Operation::ApplicationRun => {
            require_count(arguments, 1)?;
            let (_, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let model = application.model.clone();
            let callback = application.callback()?;
            let application_id = application.id;
            host.retain(callback)?;
            let result = crate::windowing::run(model, host, callback, application_id);
            host.release(callback);
            result?;
            Ok(Output::Value(Data::Nil))
        }
        Operation::ApplicationExit => {
            require_count(arguments, 1)?;
            let (_, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let mut model = application.model.lock().expect("platform model poisoned");
            let node = model
                .get_mut(application.id)
                .map_err(|_| "PLATFORM_RESOURCE_CLOSED")?;
            let ResourceState::Application { exit_requested, .. } = &mut node.state else {
                return Err("PLATFORM_RESOURCE_TYPE");
            };
            *exit_requested = true;
            drop(model);
            let _ = crate::windowing::wake(host.0.event_loop_id);
            host.wake();
            Ok(Output::Value(Data::Nil))
        }
        Operation::Wake => {
            require_count(arguments, 1)?;
            let _ = unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let _ = crate::windowing::wake(host.0.event_loop_id);
            host.wake();
            Ok(Output::Value(Data::Nil))
        }
        Operation::TimerQuery => {
            require_count(arguments, 1)?;
            let (_, timer_resource) = unsafe { resource(arguments, 0, host, ResourceKind::Timer) }?;
            let model = timer_resource
                .model
                .lock()
                .expect("platform model poisoned");
            let node = model
                .get(timer_resource.id)
                .map_err(|_| "PLATFORM_RESOURCE_CLOSED")?;
            let ResourceState::Timer(timer) = &node.state else {
                return Err("PLATFORM_RESOURCE_TYPE");
            };
            Ok(Output::Value(Data::map([
                ("编号", Data::Integer(timer_resource.id as i64)),
                (
                    "间隔毫秒",
                    Data::Number(timer.interval.as_secs_f64() * 1_000.0),
                ),
                ("重复", Data::Bool(timer.repeating)),
                ("已取消", Data::Bool(timer.cancelled)),
            ])))
        }
    }
}

fn protocol_info() -> Data {
    Data::map([
        ("平台主", Data::Integer(1)),
        ("平台次", Data::Integer(0)),
        ("事件主", Data::Integer(EVENT_MAJOR)),
        ("事件次", Data::Integer(EVENT_MINOR)),
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
    let running = model.running;
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
    let event_without_native_equivalent = event.as_ref().is_some_and(|event| {
        matches!(event.kind, EventKind::WindowShown | EventKind::WindowHidden)
    });
    if (!running || event_without_native_equivalent)
        && let Some(event) = event
    {
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
        (
            "显示器",
            window.display.as_ref().map_or(Data::Nil, display_data),
        ),
        ("已有帧", Data::Bool(!window.frame.is_empty())),
    ]))
}

fn display_data(display: &crate::model::DisplayState) -> Data {
    Data::map([
        ("名称", display.name.clone().map_or(Data::Nil, Data::String)),
        (
            "位置",
            Data::Array(
                display
                    .position
                    .into_iter()
                    .map(|value| Data::Integer(i64::from(value)))
                    .collect(),
            ),
        ),
        (
            "尺寸",
            Data::Array(
                display
                    .size
                    .into_iter()
                    .map(|value| Data::Integer(i64::from(value)))
                    .collect(),
            ),
        ),
        ("比例因子", Data::Number(display.scale_factor)),
        ("主显示器", Data::Bool(display.primary)),
    ])
}

#[allow(clippy::too_many_arguments)]
fn resource_output(
    model: Arc<Mutex<Model>>,
    text: Arc<Mutex<TextService>>,
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
            text,
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

fn integer(value: &Data) -> Result<i64, &'static str> {
    if let Data::Integer(value) = value {
        Ok(*value)
    } else {
        Err("PLATFORM_VALUE_TYPE")
    }
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

fn rectangle(value: &Data) -> Result<[f64; 4], &'static str> {
    let Data::Array(values) = value else {
        return Err("PLATFORM_VALUE_TYPE");
    };
    if values.len() != 4 {
        return Err("PLATFORM_VALUE_TYPE");
    }
    let rectangle = [
        number(&values[0])?,
        number(&values[1])?,
        number(&values[2])?,
        number(&values[3])?,
    ];
    if rectangle[2] < 0.0 || rectangle[3] < 0.0 {
        return Err("PLATFORM_VALUE_RANGE");
    }
    Ok(rectangle)
}

fn file_dialog(kind: &str, config: &BTreeMap<String, Data>) -> Result<Data, &'static str> {
    let mut dialog = rfd::FileDialog::new();
    if let Some(value) = config.get("标题") {
        dialog = dialog.set_title(text(value)?);
    }
    if let Some(value) = config.get("目录") {
        dialog = dialog.set_directory(PathBuf::from(text(value)?));
    }
    if let Some(value) = config.get("文件名") {
        dialog = dialog.set_file_name(text(value)?);
    }
    if let Some(Data::Array(filters)) = config.get("过滤器") {
        for filter in filters {
            let filter = map(filter)?;
            let name = filter.get("名称").map(text).transpose()?.unwrap_or("文件");
            let extensions = filter
                .get("扩展")
                .map(string_array)
                .transpose()?
                .unwrap_or_default();
            let extension_refs: Vec<_> = extensions.iter().map(String::as_str).collect();
            dialog = dialog.add_filter(name, &extension_refs);
        }
    }
    let path = |value: PathBuf| Data::String(value.to_string_lossy().into_owned());
    match kind {
        "打开文件" => Ok(dialog.pick_file().map_or(Data::Nil, path)),
        "打开多个文件" => Ok(dialog.pick_files().map_or(Data::Nil, |paths| {
            Data::Array(paths.into_iter().map(path).collect())
        })),
        "保存文件" => Ok(dialog.save_file().map_or(Data::Nil, path)),
        "选择目录" => Ok(dialog.pick_folder().map_or(Data::Nil, path)),
        "选择多个目录" => Ok(dialog.pick_folders().map_or(Data::Nil, |paths| {
            Data::Array(paths.into_iter().map(path).collect())
        })),
        _ => Err("PLATFORM_DIALOG_KIND"),
    }
}

fn string_array(value: &Data) -> Result<Vec<String>, &'static str> {
    let Data::Array(values) = value else {
        return Err("PLATFORM_VALUE_TYPE");
    };
    values
        .iter()
        .map(|value| text(value).map(str::to_owned))
        .collect()
}

fn decode_image(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), &'static str> {
    if bytes.is_empty() {
        return Err("PLATFORM_IMAGE_INVALID");
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    let mut reader = image::ImageReader::new(Cursor::new(bytes));
    reader = reader
        .with_guessed_format()
        .map_err(|_| "PLATFORM_IMAGE_INVALID")?;
    reader.limits(limits);
    let decoded = reader.decode().map_err(|_| "PLATFORM_IMAGE_INVALID")?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok((width, height, rgba.into_raw()))
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

fn text_options(config: &BTreeMap<String, Data>) -> Result<TextOptions, &'static str> {
    let mut options = TextOptions::default();
    if let Some(value) = config.get("字族") {
        options.family = Some(text(value)?.to_owned());
    }
    if let Some(value) = config.get("字号") {
        options.font_size = number(value)? as f32;
    }
    if let Some(value) = config.get("行高") {
        options.line_height = number(value)? as f32;
    }
    if let Some(value) = config.get("最大宽") {
        options.max_width = match value {
            Data::Nil => None,
            _ => Some(number(value)? as f32),
        };
    }
    if let Some(value) = config.get("换行") {
        options.wrap = boolean(value)?;
    }
    Ok(options)
}

fn layout_data(layout: &TextLayout) -> Data {
    let glyphs = layout
        .glyphs
        .iter()
        .map(|glyph| {
            Data::map([
                ("字体", Data::String(glyph.font.clone())),
                ("字形", Data::Integer(i64::from(glyph.glyph_id))),
                ("原文起", Data::Integer(glyph.source_start as i64)),
                ("原文终", Data::Integer(glyph.source_end as i64)),
                ("横坐标", Data::Number(f64::from(glyph.x))),
                ("基线", Data::Number(f64::from(glyph.baseline))),
                ("宽", Data::Number(f64::from(glyph.width))),
                ("从右至左", Data::Bool(glyph.rtl)),
            ])
        })
        .collect();
    let lines = layout
        .lines
        .iter()
        .map(|line| {
            Data::map([
                ("原文行", Data::Integer(line.source_line as i64)),
                ("顶部", Data::Number(f64::from(line.top))),
                ("基线", Data::Number(f64::from(line.baseline))),
                ("行高", Data::Number(f64::from(line.height))),
                ("宽", Data::Number(f64::from(line.width))),
                ("从右至左", Data::Bool(line.rtl)),
                ("字形起", Data::Integer(line.glyph_start as i64)),
                ("字形终", Data::Integer(line.glyph_end as i64)),
            ])
        })
        .collect();
    Data::map([
        ("宽", Data::Number(f64::from(layout.width))),
        ("高", Data::Number(f64::from(layout.height))),
        ("基线", Data::Number(f64::from(layout.baseline))),
        ("字形", Data::Array(glyphs)),
        ("行", Data::Array(lines)),
    ])
}

const fn text_error_code(error: TextError) -> &'static str {
    match error {
        TextError::Limit(_) => "PLATFORM_TEXT_LIMIT",
        TextError::Options => "PLATFORM_TEXT_OPTIONS",
        TextError::Font => "PLATFORM_FONT_INVALID",
    }
}

pub(crate) fn monotonic_seconds() -> f64 {
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
            protocol_info().as_map().unwrap()["事件次"],
            Data::Integer(EVENT_MINOR)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["原生窗口"],
            Data::Bool(true)
        );
    }

    #[test]
    fn decodes_bounded_png_resources() {
        let source = image::RgbaImage::from_pixel(2, 1, image::Rgba([10, 20, 30, 255]));
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let (width, height, rgba) = decode_image(encoded.get_ref()).unwrap();
        assert_eq!([width, height], [2, 1]);
        assert_eq!(rgba, vec![10, 20, 30, 255, 10, 20, 30, 255]);
        assert_eq!(decode_image(&[1, 2, 3]), Err("PLATFORM_IMAGE_INVALID"));
    }

    #[test]
    fn validates_ime_cursor_rectangles() {
        assert_eq!(
            rectangle(&Data::Array(vec![
                Data::Integer(1),
                Data::Integer(2),
                Data::Integer(30),
                Data::Integer(20),
            ]))
            .unwrap(),
            [1.0, 2.0, 30.0, 20.0]
        );
        assert_eq!(
            rectangle(&Data::Array(vec![
                Data::Integer(1),
                Data::Integer(2),
                Data::Integer(-1),
                Data::Integer(20),
            ])),
            Err("PLATFORM_VALUE_RANGE")
        );
    }
}
