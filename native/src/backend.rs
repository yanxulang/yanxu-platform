//! ABI 操作到无句柄平台模型的映射。

use crate::abi::{self, NativeError, NativeHost};
use crate::accessibility::{
    ACCESSIBILITY_MAJOR, ACCESSIBILITY_MINOR, AccessibilityState, MAX_SEMANTIC_ACTIONS,
    MAX_SEMANTIC_CHILDREN, MAX_SEMANTIC_DEPTH, MAX_SEMANTIC_NODE_TEXT_BYTES, MAX_SEMANTIC_NODES,
    MAX_SEMANTIC_TEXT_BYTES, SEMANTIC_ACTIONS, SEMANTIC_ROLES, SEMANTIC_STATES, SemanticTree,
};
use crate::bridge::{encode_data, free_value};
use crate::data::Data;
use crate::event::{EVENT_MAJOR, EVENT_MINOR, EventKind, PlatformEvent};
use crate::model::{
    FrameSubmission, Model, ModelError, QuotaMetrics, ResourceKind, ResourceLimits, ResourceState,
    ResourceUsage, TimerState, WindowState,
};
use crate::protocol;
use crate::sync::{RecoverMutex, recovered_lock_count};
use crate::text::{TextError, TextLayout, TextOptions, TextService};
use std::borrow::Cow;
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
const PLATFORM_MAJOR: i64 = 1;
const PLATFORM_MINOR: i64 = 7;
const MAX_CLIPBOARD_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_CLIPBOARD_IMAGE_DIMENSION: usize = 16_384;
const MAX_CLIPBOARD_IMAGE_BYTES: usize = 256 * 1024 * 1024;

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
        let _ = self.model.lock_recover().close(self.id);
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
    DrawEncode,
    ClipboardReadImage,
    ClipboardWriteImage,
    SubmitFrameFeedback,
    AccessibilityUpdate,
    AccessibilityQuery,
    ResourceLimitsQuery,
    ResourceLimitsConfigure,
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
            34 => Self::DrawEncode,
            35 => Self::ClipboardReadImage,
            36 => Self::ClipboardWriteImage,
            37 => Self::SubmitFrameFeedback,
            38 => Self::AccessibilityUpdate,
            39 => Self::AccessibilityQuery,
            40 => Self::ResourceLimitsQuery,
            41 => Self::ResourceLimitsConfigure,
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
            let id = match model.lock_recover().create(
                None,
                ResourceState::Application {
                    name,
                    exit_requested: false,
                },
            ) {
                Ok(id) => id,
                Err(error) => {
                    host.release(callback);
                    return Err(model_error_code(error));
                }
            };
            model
                .lock_recover()
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
            let mut model = application.model.lock_recover();
            let id = model
                .create(
                    Some(application.id),
                    ResourceState::Window(Box::new(window.clone())),
                )
                .map_err(model_error_code)?;
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
            let batch = application.model.lock_recover().events.take_data();
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
            let (count, _) = submit_frame(resource, bytes)?;
            Ok(Output::Value(Data::Integer(count)))
        }
        Operation::SubmitFrameFeedback => {
            require_count(arguments, 2)?;
            let (_, resource) = unsafe { resource(arguments, 0, host, ResourceKind::Window) }?;
            let bytes = bytes(&arguments[1])?;
            let (count, submission) = submit_frame(resource, bytes)?;
            Ok(Output::Value(frame_submission_data(count, submission)))
        }
        Operation::AccessibilityUpdate => {
            require_count(arguments, 2)?;
            let (_, resource) = unsafe { resource(arguments, 0, host, ResourceKind::Window) }?;
            let tree = match &arguments[1] {
                Data::Nil => None,
                value => match SemanticTree::validate(value) {
                    Ok(tree) => Some(tree),
                    Err(error) => {
                        resource
                            .model
                            .lock_recover()
                            .record_accessibility_rejection();
                        return Err(error.code());
                    }
                },
            };
            let mut model = resource.model.lock_recover();
            let changed = model
                .replace_accessibility(resource.id, tree)
                .map_err(|error| error.code())?;
            let node = model
                .get(resource.id)
                .map_err(|_| "PLATFORM_RESOURCE_CLOSED")?;
            let ResourceState::Window(window) = &node.state else {
                return Err("PLATFORM_RESOURCE_TYPE");
            };
            let result = accessibility_state_data(&window.accessibility, Some(changed));
            drop(model);
            if changed {
                let _ = crate::windowing::wake(resource.host.0.event_loop_id);
                resource.host.wake();
            }
            Ok(Output::Value(result))
        }
        Operation::AccessibilityQuery => {
            require_count(arguments, 1)?;
            let (_, resource) = unsafe { resource(arguments, 0, host, ResourceKind::Window) }?;
            let model = resource.model.lock_recover();
            let node = model
                .get(resource.id)
                .map_err(|_| "PLATFORM_RESOURCE_CLOSED")?;
            let ResourceState::Window(window) = &node.state else {
                return Err("PLATFORM_RESOURCE_TYPE");
            };
            Ok(Output::Value(accessibility_state_data(
                &window.accessibility,
                None,
            )))
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
            let model = application.model.lock_recover();
            Ok(Output::Value(debug_snapshot(&model)))
        }
        Operation::ResourceLimitsQuery => {
            require_count(arguments, 1)?;
            let (_, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let model = application.model.lock_recover();
            Ok(Output::Value(resource_limits_data(model.resource_limits())))
        }
        Operation::ResourceLimitsConfigure => {
            require_count(arguments, 2)?;
            let (_, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let config = map(&arguments[1])?;
            let mut model = application.model.lock_recover();
            let limits = configure_resource_limits_from_data(&mut model, config)?;
            Ok(Output::Value(resource_limits_data(limits)))
        }
        Operation::FontFamilies => {
            require_count(arguments, 1)?;
            let (_, application) =
                unsafe { resource(arguments, 0, host, ResourceKind::Application) }?;
            let families = application
                .text
                .lock_recover()
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
                .lock_recover()
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
                .lock_recover()
                .load_font(font_bytes.clone())
                .map_err(text_error_code)?;
            let family = families.first().cloned().ok_or("PLATFORM_FONT_INVALID")?;
            let id = application
                .model
                .lock_recover()
                .create(
                    Some(application.id),
                    ResourceState::Font {
                        family,
                        bytes: Some(font_bytes),
                    },
                )
                .map_err(model_error_code)?;
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
                .lock_recover()
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
                .lock_recover()
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
                .lock_recover()
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
                .lock_recover()
                .create(
                    Some(application.id),
                    ResourceState::Timer(TimerState {
                        interval,
                        repeating,
                        next_deadline: Instant::now() + interval,
                        cancelled: false,
                    }),
                )
                .map_err(model_error_code)?;
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
            let mut model = timer.model.lock_recover();
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
                Ok(value) => {
                    validate_clipboard_text_length(value.len())?;
                    Ok(Output::Value(Data::String(value)))
                }
                Err(arboard::Error::ContentNotAvailable) => Ok(Output::Value(Data::Nil)),
                Err(_) => Err("PLATFORM_CLIPBOARD"),
            }
        }
        Operation::ClipboardWrite => {
            require_count(arguments, 1)?;
            if !host.permission("剪贴板") {
                return Err("PLATFORM_PERMISSION_CLIPBOARD");
            }
            let value = text(&arguments[0])?;
            validate_clipboard_text_length(value.len())?;
            arboard::Clipboard::new()
                .and_then(|mut clipboard| clipboard.set_text(value.to_owned()))
                .map_err(|_| "PLATFORM_CLIPBOARD")?;
            Ok(Output::Value(Data::Nil))
        }
        Operation::ClipboardReadImage => {
            require_count(arguments, 0)?;
            if !host.permission("剪贴板") {
                return Err("PLATFORM_PERMISSION_CLIPBOARD");
            }
            let mut clipboard = arboard::Clipboard::new().map_err(|_| "PLATFORM_CLIPBOARD")?;
            match clipboard.get_image() {
                Ok(image) => {
                    validate_clipboard_image(image.width, image.height, image.bytes.len())?;
                    Ok(Output::Value(Data::map([
                        ("格式", Data::String("RGBA8".to_owned())),
                        ("宽", usize_data(image.width)),
                        ("高", usize_data(image.height)),
                        ("内容", Data::Bytes(image.bytes.into_owned())),
                    ])))
                }
                Err(arboard::Error::ContentNotAvailable) => Ok(Output::Value(Data::Nil)),
                Err(_) => Err("PLATFORM_CLIPBOARD"),
            }
        }
        Operation::ClipboardWriteImage => {
            require_count(arguments, 1)?;
            if !host.permission("剪贴板") {
                return Err("PLATFORM_PERMISSION_CLIPBOARD");
            }
            let image = clipboard_image(map(&arguments[0])?)?;
            arboard::Clipboard::new()
                .and_then(|mut clipboard| clipboard.set_image(image))
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
                .lock_recover()
                .create(
                    Some(application.id),
                    ResourceState::Image {
                        width,
                        height,
                        rgba,
                    },
                )
                .map_err(model_error_code)?;
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
            let model = image.model.lock_recover();
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
            let mut model = window_resource.model.lock_recover();
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
            let mut model = window_resource.model.lock_recover();
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
            let model = application.model.lock_recover();
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
            let theme = application.model.lock_recover().system_theme.clone();
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
            let mut model = application.model.lock_recover();
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
            let model = timer_resource.model.lock_recover();
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
        Operation::DrawEncode => {
            require_count(arguments, 1)?;
            Ok(Output::Value(Data::Bytes(crate::draw::encode_commands(
                &arguments[0],
            )?)))
        }
    }
}

fn protocol_info() -> Data {
    Data::map([
        ("平台主", Data::Integer(PLATFORM_MAJOR)),
        ("平台次", Data::Integer(PLATFORM_MINOR)),
        ("事件主", Data::Integer(EVENT_MAJOR)),
        ("事件次", Data::Integer(EVENT_MINOR)),
        ("无障碍主", Data::Integer(ACCESSIBILITY_MAJOR)),
        ("无障碍次", Data::Integer(ACCESSIBILITY_MINOR)),
        ("绘制主", Data::Integer(i64::from(protocol::DRAW_MAJOR))),
        ("绘制次", Data::Integer(i64::from(protocol::DRAW_MINOR))),
        ("ABI", Data::Integer(2)),
    ])
}

fn submit_frame(
    resource: &PlatformResource,
    bytes: &[u8],
) -> Result<(i64, FrameSubmission), &'static str> {
    let frame = protocol::decode(bytes).map_err(draw_error_code)?;
    let count = i64::try_from(frame.commands.len()).map_err(|_| "PLATFORM_DRAW_LIMIT")?;
    let submitted_at_seconds = monotonic_seconds();
    let submission = resource
        .model
        .lock_recover()
        .submit_frame(resource.id, bytes.to_vec(), submitted_at_seconds)
        .map_err(model_error_code)?;
    let _ = crate::windowing::wake(resource.host.0.event_loop_id);
    resource.host.wake();
    Ok((count, submission))
}

fn frame_submission_data(count: i64, submission: FrameSubmission) -> Data {
    Data::map([
        ("帧序号", u64_data(submission.sequence)),
        ("命令数", Data::Integer(count)),
        ("替换", Data::Bool(submission.replaced_sequence.is_some())),
        (
            "被替换帧",
            submission.replaced_sequence.map_or(Data::Nil, u64_data),
        ),
        ("提交时间", Data::Number(submission.submitted_at_seconds)),
    ])
}

fn accessibility_state_data(state: &AccessibilityState, changed: Option<bool>) -> Data {
    let mut result = BTreeMap::from([
        ("修订".to_owned(), Data::Integer(state.revision())),
        ("节点数".to_owned(), usize_data(state.node_count())),
        ("文字字节".to_owned(), usize_data(state.text_bytes())),
        (
            "焦点".to_owned(),
            state.focused().map_or(Data::Nil, Data::Integer),
        ),
    ]);
    if let Some(changed) = changed {
        result.insert("变化".to_owned(), Data::Bool(changed));
    } else {
        result.insert(
            "树".to_owned(),
            state.tree().map_or(Data::Nil, SemanticTree::to_data),
        );
    }
    Data::Map(result)
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
        ("剪贴板文字", Data::Bool(true)),
        ("剪贴板图片", Data::Bool(true)),
        ("剪贴板文字字节上限", usize_data(MAX_CLIPBOARD_TEXT_BYTES)),
        (
            "剪贴板图片边长上限",
            usize_data(MAX_CLIPBOARD_IMAGE_DIMENSION),
        ),
        ("剪贴板图片字节上限", usize_data(MAX_CLIPBOARD_IMAGE_BYTES)),
        (
            "剪贴板图片格式",
            Data::Array(vec![Data::String("RGBA8".to_owned())]),
        ),
        ("文件对话框", Data::Bool(true)),
        ("文件拖放", Data::Bool(true)),
        ("CPU二维绘制", Data::Bool(true)),
        ("复杂文字", Data::Bool(true)),
        ("状态故障恢复", Data::Bool(true)),
        ("运行可观测性", Data::Bool(true)),
        ("应用资源配额", Data::Bool(true)),
        ("应用资源配额可下调", Data::Bool(true)),
        ("应用资源配额拒绝统计", Data::Bool(true)),
        (
            "应用资源硬上限",
            resource_limits_data(ResourceLimits::default()),
        ),
        ("待呈现帧上限", Data::Integer(1)),
        ("帧提交反馈", Data::Bool(true)),
        ("帧呈现反馈", Data::Bool(true)),
        ("帧时间基准", Data::String("进程内单调秒".to_owned())),
        ("动画驱动事件", Data::String("帧呈现".to_owned())),
        ("无障碍语义树", Data::Bool(true)),
        ("无障碍焦点请求", Data::Bool(true)),
        ("无障碍动作请求", Data::Bool(true)),
        (
            "原生无障碍桥",
            Data::Bool(native_accessibility_backend().is_some()),
        ),
        (
            "原生无障碍后端",
            native_accessibility_backend()
                .map_or(Data::Nil, |backend| Data::String(backend.to_owned())),
        ),
        ("无障碍节点上限", usize_data(MAX_SEMANTIC_NODES)),
        ("无障碍深度上限", usize_data(MAX_SEMANTIC_DEPTH)),
        ("无障碍单节点子上限", usize_data(MAX_SEMANTIC_CHILDREN)),
        ("无障碍单节点操作上限", usize_data(MAX_SEMANTIC_ACTIONS)),
        (
            "无障碍单字段文字字节上限",
            usize_data(MAX_SEMANTIC_NODE_TEXT_BYTES),
        ),
        ("无障碍文字字节上限", usize_data(MAX_SEMANTIC_TEXT_BYTES)),
        ("无障碍角色", string_list(SEMANTIC_ROLES)),
        ("无障碍状态", string_list(SEMANTIC_STATES)),
        ("无障碍动作", string_list(SEMANTIC_ACTIONS)),
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
    let mut model = resource.model.lock_recover();
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
    let model = resource.model.lock_recover();
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
        ("待呈现帧", Data::Bool(window.frame_pending)),
        (
            "帧序号",
            if window.frame_generation != 0 {
                u64_data(window.frame_generation)
            } else {
                Data::Nil
            },
        ),
        (
            "待呈现帧序号",
            if window.frame_pending {
                u64_data(window.frame_generation)
            } else {
                Data::Nil
            },
        ),
        (
            "帧提交时间",
            if window.frame_generation != 0 {
                Data::Number(window.frame_submitted_at_seconds)
            } else {
                Data::Nil
            },
        ),
        ("帧字节", usize_data(window.frame.len())),
    ]))
}

fn debug_snapshot(model: &Model) -> Data {
    let events = model.events.metrics();
    let resources = model.resource_metrics();
    let resource_limits = model.resource_limits();
    let resource_usage = model.resource_usage();
    let quota_metrics = model.quota_metrics();
    let frames = model.frame_metrics();
    let accessibility = model.accessibility_metrics();
    Data::map([
        ("应用", usize_data(model.count(ResourceKind::Application))),
        ("窗口", usize_data(model.count(ResourceKind::Window))),
        ("计时器", usize_data(model.count(ResourceKind::Timer))),
        ("图片", usize_data(model.count(ResourceKind::Image))),
        ("字体", usize_data(model.count(ResourceKind::Font))),
        ("待处理事件", usize_data(events.queued)),
        ("运行中", Data::Bool(model.running)),
        ("状态恢复次数", u64_data(recovered_lock_count())),
        (
            "事件队列",
            Data::map([
                ("容量", usize_data(events.capacity)),
                ("当前", usize_data(events.queued)),
                ("高水位", usize_data(events.high_watermark)),
                ("接收总数", u64_data(events.accepted)),
                ("合并总数", u64_data(events.coalesced)),
                ("拒绝总数", u64_data(events.rejected)),
                ("批次总数", u64_data(events.batches)),
                ("排出总数", u64_data(events.drained)),
            ]),
        ),
        (
            "资源统计",
            Data::map([
                ("当前", usize_data(resources.live)),
                ("高水位", usize_data(resources.high_watermark)),
                ("创建总数", u64_data(resources.created)),
                ("关闭总数", u64_data(resources.closed)),
            ]),
        ),
        (
            "资源配额",
            Data::map([
                ("已冻结", Data::Bool(model.resource_limits_locked())),
                ("上限", resource_limits_data(resource_limits)),
                ("使用", resource_usage_data(resource_usage)),
                ("拒绝统计", quota_metrics_data(quota_metrics)),
            ]),
        ),
        (
            "帧统计",
            Data::map([
                ("待呈现", usize_data(frames.pending)),
                ("待呈现高水位", usize_data(frames.pending_high_watermark)),
                ("字节高水位", usize_data(frames.bytes_high_watermark)),
                ("提交总数", u64_data(frames.submitted)),
                ("替换总数", u64_data(frames.replaced)),
                ("渲染总数", u64_data(frames.rendered)),
                ("呈现总数", u64_data(frames.presented)),
                ("失败总数", u64_data(frames.failed)),
            ]),
        ),
        (
            "无障碍统计",
            Data::map([
                ("当前树", usize_data(accessibility.current_trees)),
                ("当前节点", usize_data(accessibility.current_nodes)),
                ("节点高水位", usize_data(accessibility.nodes_high_watermark)),
                ("当前文字字节", usize_data(accessibility.current_text_bytes)),
                (
                    "文字字节高水位",
                    usize_data(accessibility.text_bytes_high_watermark),
                ),
                ("更新总数", u64_data(accessibility.updates)),
                ("去重总数", u64_data(accessibility.unchanged)),
                ("清除总数", u64_data(accessibility.cleared)),
                ("焦点请求总数", u64_data(accessibility.focus_requests)),
                ("动作请求总数", u64_data(accessibility.action_requests)),
                ("拒绝总数", u64_data(accessibility.rejected)),
                (
                    "原生桥当前激活",
                    usize_data(accessibility.native_bridges_active),
                ),
                (
                    "原生桥激活高水位",
                    usize_data(accessibility.native_bridges_high_watermark),
                ),
                (
                    "原生桥激活总数",
                    u64_data(accessibility.native_bridge_activations),
                ),
                (
                    "原生桥停用总数",
                    u64_data(accessibility.native_bridge_deactivations),
                ),
                ("原生树同步总数", u64_data(accessibility.native_tree_syncs)),
                ("原生请求总数", u64_data(accessibility.native_requests)),
                ("原生拒绝总数", u64_data(accessibility.native_rejected)),
            ]),
        ),
    ])
}

const RESOURCE_LIMIT_FIELDS: [&str; 10] = [
    "资源总数",
    "窗口数",
    "计时器数",
    "图片数",
    "字体数",
    "图片字节",
    "字体字节",
    "帧字节",
    "无障碍节点",
    "无障碍文字字节",
];

fn parse_resource_limits(
    config: &BTreeMap<String, Data>,
    current: ResourceLimits,
) -> Result<ResourceLimits, &'static str> {
    if config
        .keys()
        .any(|name| !RESOURCE_LIMIT_FIELDS.contains(&name.as_str()))
    {
        return Err("PLATFORM_QUOTA_CONFIG");
    }
    ResourceLimits {
        resources: configured_limit(config, "资源总数", current.resources)?,
        windows: configured_limit(config, "窗口数", current.windows)?,
        timers: configured_limit(config, "计时器数", current.timers)?,
        images: configured_limit(config, "图片数", current.images)?,
        fonts: configured_limit(config, "字体数", current.fonts)?,
        image_bytes: configured_limit(config, "图片字节", current.image_bytes)?,
        font_bytes: configured_limit(config, "字体字节", current.font_bytes)?,
        frame_bytes: configured_limit(config, "帧字节", current.frame_bytes)?,
        accessibility_nodes: configured_limit(config, "无障碍节点", current.accessibility_nodes)?,
        accessibility_text_bytes: configured_limit(
            config,
            "无障碍文字字节",
            current.accessibility_text_bytes,
        )?,
    }
    .validate()
    .map_err(|_| "PLATFORM_QUOTA_CONFIG")
}

fn configure_resource_limits_from_data(
    model: &mut Model,
    config: &BTreeMap<String, Data>,
) -> Result<ResourceLimits, &'static str> {
    let limits = match parse_resource_limits(config, model.resource_limits()) {
        Ok(limits) => limits,
        Err(error) => {
            model.record_quota_configuration_rejection();
            return Err(error);
        }
    };
    model
        .configure_resource_limits(limits)
        .map_err(model_error_code)
}

fn configured_limit(
    config: &BTreeMap<String, Data>,
    name: &str,
    current: usize,
) -> Result<usize, &'static str> {
    let Some(value) = config.get(name) else {
        return Ok(current);
    };
    let Data::Integer(value) = value else {
        return Err("PLATFORM_QUOTA_CONFIG");
    };
    usize::try_from(*value).map_err(|_| "PLATFORM_QUOTA_CONFIG")
}

fn resource_limits_data(limits: ResourceLimits) -> Data {
    Data::map([
        ("资源总数", usize_data(limits.resources)),
        ("窗口数", usize_data(limits.windows)),
        ("计时器数", usize_data(limits.timers)),
        ("图片数", usize_data(limits.images)),
        ("字体数", usize_data(limits.fonts)),
        ("图片字节", usize_data(limits.image_bytes)),
        ("字体字节", usize_data(limits.font_bytes)),
        ("帧字节", usize_data(limits.frame_bytes)),
        ("无障碍节点", usize_data(limits.accessibility_nodes)),
        (
            "无障碍文字字节",
            usize_data(limits.accessibility_text_bytes),
        ),
    ])
}

fn resource_usage_data(usage: ResourceUsage) -> Data {
    Data::map([
        ("资源总数", usize_data(usage.resources)),
        ("窗口数", usize_data(usage.windows)),
        ("计时器数", usize_data(usage.timers)),
        ("图片数", usize_data(usage.images)),
        ("字体数", usize_data(usage.fonts)),
        ("图片字节", usize_data(usage.image_bytes)),
        ("字体字节", usize_data(usage.font_bytes)),
        ("帧字节", usize_data(usage.frame_bytes)),
        ("无障碍节点", usize_data(usage.accessibility_nodes)),
        ("无障碍文字字节", usize_data(usage.accessibility_text_bytes)),
    ])
}

fn quota_metrics_data(metrics: QuotaMetrics) -> Data {
    Data::map([
        ("总数", u64_data(metrics.rejected)),
        ("上限", u64_data(metrics.limit_rejected)),
        ("配置", u64_data(metrics.configuration_rejected)),
        ("冻结", u64_data(metrics.locked_rejected)),
        (
            "按配额",
            Data::map([
                ("资源总数", u64_data(metrics.resources)),
                ("窗口数", u64_data(metrics.windows)),
                ("计时器数", u64_data(metrics.timers)),
                ("图片数", u64_data(metrics.images)),
                ("字体数", u64_data(metrics.fonts)),
                ("图片字节", u64_data(metrics.image_bytes)),
                ("字体字节", u64_data(metrics.font_bytes)),
                ("帧字节", u64_data(metrics.frame_bytes)),
                ("无障碍节点", u64_data(metrics.accessibility_nodes)),
                ("无障碍文字字节", u64_data(metrics.accessibility_text_bytes)),
            ]),
        ),
    ])
}

const fn native_accessibility_backend() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        Some("UIA")
    } else if cfg!(target_os = "macos") {
        Some("NSAccessibility")
    } else if cfg!(target_os = "linux") {
        Some("AT-SPI")
    } else {
        None
    }
}

fn usize_data(value: usize) -> Data {
    Data::Integer(i64::try_from(value).unwrap_or(i64::MAX))
}

fn u64_data(value: u64) -> Data {
    Data::Integer(i64::try_from(value).unwrap_or(i64::MAX))
}

fn string_list(values: &[&str]) -> Data {
    Data::Array(
        values
            .iter()
            .map(|value| Data::String((*value).to_owned()))
            .collect(),
    )
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

const fn validate_clipboard_text_length(length: usize) -> Result<(), &'static str> {
    if length > MAX_CLIPBOARD_TEXT_BYTES {
        Err("PLATFORM_CLIPBOARD_LIMIT")
    } else {
        Ok(())
    }
}

fn clipboard_image<'a>(
    value: &'a BTreeMap<String, Data>,
) -> Result<arboard::ImageData<'a>, &'static str> {
    if value.get("格式").map(text).transpose()?.unwrap_or("RGBA8") != "RGBA8" {
        return Err("PLATFORM_CLIPBOARD_IMAGE");
    }
    let width = clipboard_dimension(value.get("宽").ok_or("PLATFORM_CLIPBOARD_IMAGE")?)?;
    let height = clipboard_dimension(value.get("高").ok_or("PLATFORM_CLIPBOARD_IMAGE")?)?;
    let content = bytes(value.get("内容").ok_or("PLATFORM_CLIPBOARD_IMAGE")?)?;
    validate_clipboard_image(width, height, content.len())?;
    Ok(arboard::ImageData {
        width,
        height,
        bytes: Cow::Borrowed(content),
    })
}

fn clipboard_dimension(value: &Data) -> Result<usize, &'static str> {
    let value = usize::try_from(integer(value)?).map_err(|_| "PLATFORM_CLIPBOARD_IMAGE")?;
    if value == 0 {
        return Err("PLATFORM_CLIPBOARD_IMAGE");
    }
    if value > MAX_CLIPBOARD_IMAGE_DIMENSION {
        return Err("PLATFORM_CLIPBOARD_LIMIT");
    }
    Ok(value)
}

fn validate_clipboard_image(
    width: usize,
    height: usize,
    length: usize,
) -> Result<(), &'static str> {
    if width == 0 || height == 0 {
        return Err("PLATFORM_CLIPBOARD_IMAGE");
    }
    if width > MAX_CLIPBOARD_IMAGE_DIMENSION || height > MAX_CLIPBOARD_IMAGE_DIMENSION {
        return Err("PLATFORM_CLIPBOARD_LIMIT");
    }
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or("PLATFORM_CLIPBOARD_LIMIT")?;
    if expected > MAX_CLIPBOARD_IMAGE_BYTES {
        return Err("PLATFORM_CLIPBOARD_LIMIT");
    }
    if length != expected {
        return Err("PLATFORM_CLIPBOARD_IMAGE");
    }
    Ok(())
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

const fn model_error_code(error: ModelError) -> &'static str {
    match error {
        ModelError::Missing(_) => "PLATFORM_RESOURCE_CLOSED",
        ModelError::Kind(_) => "PLATFORM_RESOURCE_TYPE",
        ModelError::FrameSequence => "PLATFORM_FRAME_SEQUENCE",
        ModelError::Quota(kind) => kind.code(),
        ModelError::QuotaConfiguration(_) => "PLATFORM_QUOTA_CONFIG",
        ModelError::QuotaLocked => "PLATFORM_QUOTA_LOCKED",
        ModelError::Parent(_) | ModelError::Overflow => "PLATFORM_RESOURCE_LIMIT",
    }
}

fn text_options(config: &BTreeMap<String, Data>) -> Result<TextOptions, &'static str> {
    let mut options = TextOptions::default();
    if let Some(value) = config.get("字族") {
        options.family = Some(text(value)?.to_owned());
    }
    if let Some(value) = config.get("字重") {
        options.weight = u16::try_from(integer(value)?).map_err(|_| "PLATFORM_TEXT_OPTIONS")?;
    }
    if let Some(value) = config.get("斜体") {
        options.italic = boolean(value)?;
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
                ("字族", Data::String(glyph.font.clone())),
                ("字形", Data::Integer(i64::from(glyph.glyph_id))),
                ("字重", Data::Integer(i64::from(glyph.weight))),
                ("斜体", Data::Bool(glyph.italic)),
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
        assert_eq!(
            protocol_info().as_map().unwrap()["平台主"],
            Data::Integer(PLATFORM_MAJOR)
        );
        assert_eq!(
            protocol_info().as_map().unwrap()["平台次"],
            Data::Integer(PLATFORM_MINOR)
        );
        assert_eq!(PLATFORM_MINOR, 7);
        assert_eq!(protocol_info().as_map().unwrap()["ABI"], Data::Integer(2));
        assert_eq!(
            protocol_info().as_map().unwrap()["事件次"],
            Data::Integer(EVENT_MINOR)
        );
        assert_eq!(
            protocol_info().as_map().unwrap()["无障碍主"],
            Data::Integer(ACCESSIBILITY_MAJOR)
        );
        assert_eq!(
            protocol_info().as_map().unwrap()["无障碍次"],
            Data::Integer(ACCESSIBILITY_MINOR)
        );
        assert_eq!(
            protocol_info().as_map().unwrap()["绘制次"],
            Data::Integer(i64::from(protocol::DRAW_MINOR))
        );
        assert_eq!(
            capabilities().as_map().unwrap()["原生窗口"],
            Data::Bool(true)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["状态故障恢复"],
            Data::Bool(true)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["运行可观测性"],
            Data::Bool(true)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["应用资源配额"],
            Data::Bool(true)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["应用资源配额可下调"],
            Data::Bool(true)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["应用资源配额拒绝统计"],
            Data::Bool(true)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["应用资源硬上限"],
            resource_limits_data(ResourceLimits::default())
        );
        assert_eq!(
            capabilities().as_map().unwrap()["待呈现帧上限"],
            Data::Integer(1)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["帧提交反馈"],
            Data::Bool(true)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["帧呈现反馈"],
            Data::Bool(true)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["帧时间基准"],
            Data::String("进程内单调秒".to_owned())
        );
        assert_eq!(
            capabilities().as_map().unwrap()["动画驱动事件"],
            Data::String("帧呈现".to_owned())
        );
        assert_eq!(
            capabilities().as_map().unwrap()["无障碍语义树"],
            Data::Bool(true)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["无障碍焦点请求"],
            Data::Bool(true)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["无障碍动作请求"],
            Data::Bool(true)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["原生无障碍桥"],
            Data::Bool(native_accessibility_backend().is_some())
        );
        assert_eq!(
            capabilities().as_map().unwrap()["原生无障碍后端"],
            native_accessibility_backend()
                .map_or(Data::Nil, |backend| Data::String(backend.to_owned()))
        );
        assert_eq!(
            capabilities().as_map().unwrap()["无障碍节点上限"],
            Data::Integer(MAX_SEMANTIC_NODES as i64)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["无障碍文字字节上限"],
            Data::Integer(MAX_SEMANTIC_TEXT_BYTES as i64)
        );
        let capability_data = capabilities();
        let Data::Array(roles) = &capability_data.as_map().unwrap()["无障碍角色"] else {
            panic!("accessibility roles expected")
        };
        assert!(roles.contains(&Data::String("按钮".to_owned())));
        assert_eq!(
            capabilities().as_map().unwrap()["剪贴板文字"],
            Data::Bool(true)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["剪贴板图片"],
            Data::Bool(true)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["剪贴板文字字节上限"],
            Data::Integer(MAX_CLIPBOARD_TEXT_BYTES as i64)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["剪贴板图片边长上限"],
            Data::Integer(MAX_CLIPBOARD_IMAGE_DIMENSION as i64)
        );
        assert_eq!(
            capabilities().as_map().unwrap()["剪贴板图片格式"],
            Data::Array(vec![Data::String("RGBA8".to_owned())])
        );
    }

    #[test]
    fn accessibility_updates_return_metadata_and_queries_return_the_tree() {
        let tree = Data::map([
            ("编号", Data::Integer(1)),
            ("角色", Data::String("面板".to_owned())),
            ("名称", Data::String("根".to_owned())),
            (
                "边界",
                Data::Array(vec![0.into(), 0.into(), 320.into(), 200.into()]),
            ),
        ]);
        let mut state = AccessibilityState::default();
        assert!(
            state
                .replace(Some(SemanticTree::validate(&tree).unwrap()))
                .unwrap()
        );
        let update = accessibility_state_data(&state, Some(true));
        let update = update.as_map().unwrap();
        assert_eq!(update["修订"], Data::Integer(1));
        assert_eq!(update["节点数"], Data::Integer(1));
        assert_eq!(update["变化"], Data::Bool(true));
        assert!(!update.contains_key("树"));

        let query = accessibility_state_data(&state, None);
        let query = query.as_map().unwrap();
        assert_eq!(query["树"], state.tree().unwrap().to_data());
        assert!(!query.contains_key("变化"));
    }

    #[test]
    fn debug_snapshot_reports_queue_resource_and_frame_metrics() {
        let mut model = Model::default();
        let application = model
            .create(
                None,
                ResourceState::Application {
                    name: "测试".to_owned(),
                    exit_requested: false,
                },
            )
            .unwrap();
        let window = model
            .create(Some(application), ResourceState::Window(Box::default()))
            .unwrap();
        for time in [1.0, 2.0] {
            model
                .events
                .push(PlatformEvent::new(
                    EventKind::PointerMoved,
                    Some(window),
                    time,
                ))
                .unwrap();
        }
        model.submit_frame(window, vec![1, 2], 1.0).unwrap();
        model.submit_frame(window, vec![3, 4, 5], 2.0).unwrap();
        model.record_quota_configuration_rejection();
        assert_eq!(
            model.configure_resource_limits(ResourceLimits::default()),
            Err(ModelError::QuotaLocked)
        );

        let snapshot = debug_snapshot(&model);
        let snapshot = snapshot.as_map().unwrap();
        assert_eq!(snapshot["待处理事件"], Data::Integer(1));
        let events = snapshot["事件队列"].as_map().unwrap();
        assert_eq!(events["接收总数"], Data::Integer(2));
        assert_eq!(events["合并总数"], Data::Integer(1));
        assert_eq!(events["高水位"], Data::Integer(1));
        let resources = snapshot["资源统计"].as_map().unwrap();
        assert_eq!(resources["当前"], Data::Integer(2));
        assert_eq!(resources["创建总数"], Data::Integer(2));
        let quota = snapshot["资源配额"].as_map().unwrap();
        assert_eq!(quota["已冻结"], Data::Bool(true));
        assert_eq!(
            quota["上限"],
            resource_limits_data(ResourceLimits::default())
        );
        let quota_usage = quota["使用"].as_map().unwrap();
        assert_eq!(quota_usage["资源总数"], Data::Integer(2));
        assert_eq!(quota_usage["窗口数"], Data::Integer(1));
        assert_eq!(quota_usage["帧字节"], Data::Integer(3));
        let quota_rejections = quota["拒绝统计"].as_map().unwrap();
        assert_eq!(quota_rejections["总数"], Data::Integer(2));
        assert_eq!(quota_rejections["上限"], Data::Integer(0));
        assert_eq!(quota_rejections["配置"], Data::Integer(1));
        assert_eq!(quota_rejections["冻结"], Data::Integer(1));
        assert_eq!(
            quota_rejections["按配额"].as_map().unwrap()["帧字节"],
            Data::Integer(0)
        );
        let frames = snapshot["帧统计"].as_map().unwrap();
        assert_eq!(frames["待呈现"], Data::Integer(1));
        assert_eq!(frames["提交总数"], Data::Integer(2));
        assert_eq!(frames["替换总数"], Data::Integer(1));
        assert_eq!(frames["字节高水位"], Data::Integer(3));
        let accessibility = snapshot["无障碍统计"].as_map().unwrap();
        assert_eq!(accessibility["原生桥当前激活"], Data::Integer(0));
        assert_eq!(accessibility["原生树同步总数"], Data::Integer(0));
        assert_eq!(accessibility["原生请求总数"], Data::Integer(0));
        assert_eq!(accessibility["原生拒绝总数"], Data::Integer(0));
    }

    #[test]
    fn parses_strict_resource_quota_overrides_and_stable_errors() {
        let current = ResourceLimits::default();
        let limits = parse_resource_limits(
            &BTreeMap::from([
                ("窗口数".to_owned(), Data::Integer(4)),
                ("图片字节".to_owned(), Data::Integer(1_024)),
                ("无障碍节点".to_owned(), Data::Integer(2_048)),
            ]),
            current,
        )
        .unwrap();
        assert_eq!(limits.windows, 4);
        assert_eq!(limits.image_bytes, 1_024);
        assert_eq!(limits.accessibility_nodes, 2_048);
        assert_eq!(limits.timers, current.timers);
        assert_eq!(
            parse_resource_limits(
                &BTreeMap::from([("窗口".to_owned(), Data::Integer(1))]),
                current,
            ),
            Err("PLATFORM_QUOTA_CONFIG")
        );
        assert_eq!(
            parse_resource_limits(
                &BTreeMap::from([("窗口数".to_owned(), Data::Integer(-1))]),
                current,
            ),
            Err("PLATFORM_QUOTA_CONFIG")
        );
        assert_eq!(
            parse_resource_limits(
                &BTreeMap::from([(
                    "窗口数".to_owned(),
                    Data::Integer(i64::try_from(current.windows + 1).unwrap()),
                )]),
                current,
            ),
            Err("PLATFORM_QUOTA_CONFIG")
        );
        assert_eq!(
            model_error_code(ModelError::Quota(crate::model::QuotaKind::Windows)),
            "PLATFORM_QUOTA_WINDOWS"
        );
        assert_eq!(
            model_error_code(ModelError::QuotaLocked),
            "PLATFORM_QUOTA_LOCKED"
        );
    }

    #[test]
    fn counts_each_quota_configuration_rejection_once() {
        let mut model = Model::default();
        let invalid = BTreeMap::from([("窗口".to_owned(), Data::Integer(1))]);
        assert_eq!(
            configure_resource_limits_from_data(&mut model, &invalid),
            Err("PLATFORM_QUOTA_CONFIG")
        );
        assert_eq!(model.quota_metrics().rejected, 1);
        assert_eq!(model.quota_metrics().configuration_rejected, 1);

        let excessive = BTreeMap::from([(
            "窗口数".to_owned(),
            Data::Integer(i64::try_from(ResourceLimits::default().windows + 1).unwrap()),
        )]);
        assert_eq!(
            configure_resource_limits_from_data(&mut model, &excessive),
            Err("PLATFORM_QUOTA_CONFIG")
        );
        assert_eq!(model.quota_metrics().rejected, 2);
        assert_eq!(model.quota_metrics().configuration_rejected, 2);

        model.lock_resource_limits();
        assert_eq!(
            configure_resource_limits_from_data(&mut model, &BTreeMap::new()),
            Err("PLATFORM_QUOTA_LOCKED")
        );
        assert_eq!(model.quota_metrics().rejected, 3);
        assert_eq!(model.quota_metrics().configuration_rejected, 2);
        assert_eq!(model.quota_metrics().locked_rejected, 1);
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
    fn bounds_clipboard_text_by_utf8_bytes() {
        assert_eq!(validate_clipboard_text_length(0), Ok(()));
        assert_eq!(
            validate_clipboard_text_length(MAX_CLIPBOARD_TEXT_BYTES),
            Ok(())
        );
        assert_eq!(
            validate_clipboard_text_length(MAX_CLIPBOARD_TEXT_BYTES + 1),
            Err("PLATFORM_CLIPBOARD_LIMIT")
        );
        assert_eq!("言".len(), 3);
    }

    #[test]
    fn validates_bounded_rgba_clipboard_images() {
        assert_eq!(validate_clipboard_image(2, 1, 8), Ok(()));
        assert_eq!(
            validate_clipboard_image(2, 1, 7),
            Err("PLATFORM_CLIPBOARD_IMAGE")
        );
        assert_eq!(
            validate_clipboard_image(0, 1, 0),
            Err("PLATFORM_CLIPBOARD_IMAGE")
        );
        assert_eq!(
            validate_clipboard_image(MAX_CLIPBOARD_IMAGE_DIMENSION + 1, 1, 4),
            Err("PLATFORM_CLIPBOARD_LIMIT")
        );
        assert_eq!(
            validate_clipboard_image(16_384, 16_384, 1_073_741_824),
            Err("PLATFORM_CLIPBOARD_LIMIT")
        );

        let value = BTreeMap::from([
            ("格式".to_owned(), Data::String("RGBA8".to_owned())),
            ("宽".to_owned(), Data::Integer(2)),
            ("高".to_owned(), Data::Integer(1)),
            ("内容".to_owned(), Data::Bytes(vec![0; 8])),
        ]);
        let image = clipboard_image(&value).unwrap();
        assert_eq!([image.width, image.height, image.bytes.len()], [2, 1, 8]);
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
